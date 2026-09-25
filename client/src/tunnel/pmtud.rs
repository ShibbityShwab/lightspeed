//! Bounded client-to-relay path-MTU discovery.
//!
//! The tunnel payload budget used to be a fixed clamp
//! ([`CONSERVATIVE_PATH_MTU`], 1492 bytes, sized for PPPoE). This module adds
//! real (DPLPMTUD, RFC 8899 style) discovery on top of that clamp without ever
//! removing it:
//!
//! * The clamp is the **floor**. Discovery may only move the effective path MTU
//!   between the clamp and [`HARD_CAP_PATH_MTU`].
//! * Discovery is **off by default**. With it disabled the effective MTU is the
//!   clamp and behaviour is byte-for-byte today's.
//! * Any failure, timeout, or ambiguous observation falls back to the clamp.
//!   A path that never answers can never stall or break a session.
//!
//! The actual probing is delegated to the already-encrypted QUIC control plane
//! (`quinn` implements DPLPMTUD internally), so this module is pure policy: it
//! consumes an observed path MTU and decides the effective budget. Keeping the
//! decision pure is what makes the raise, fallback, cap, and disabled cases
//! deterministically testable.

use lightspeed_protocol::{
    CONSERVATIVE_PATH_MTU, FEC_HEADER_SIZE, FEC_PARITY_TRAILER_SIZE, HEADER_SIZE,
    OUTER_IPV4_HEADER_SIZE, OUTER_UDP_HEADER_SIZE,
};

/// Hard ceiling on the discovered client-to-relay path MTU.
///
/// Standard (non-jumbo) Ethernet, the universal maximum on the public internet.
/// The client never trusts a discovered value above this even if the control
/// plane reports one, so an oversized datagram can never be emitted toward the
/// relay. It also stays well under the relay's 2048-byte data-plane receive
/// buffer, so a datagram within this budget is delivered whole rather than
/// silently truncated.
pub const HARD_CAP_PATH_MTU: usize = 1500;

/// Largest UDP payload allowed by [`HARD_CAP_PATH_MTU`] (1472 bytes).
pub const HARD_CAP_UDP_PAYLOAD: usize =
    HARD_CAP_PATH_MTU - OUTER_IPV4_HEADER_SIZE - OUTER_UDP_HEADER_SIZE;

const _: () = assert!(CONSERVATIVE_PATH_MTU <= HARD_CAP_PATH_MTU);
// The relay receives data-plane datagrams into a 2048-byte buffer and silently
// truncates anything larger, so the capped datagram must fit.
const _: () = assert!(HARD_CAP_UDP_PAYLOAD < 2048);

/// Whether path-MTU discovery is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PmtudConfig {
    /// **Off by default.** When false the effective path MTU is always the
    /// conservative clamp and [`PathMtuDiscovery`] ignores every observation.
    pub enabled: bool,
}

/// Largest tunnel datagram (LightSpeed header(s) plus game payload) for an
/// arbitrary path MTU.
///
/// Mirrors [`lightspeed_protocol::max_tunnel_datagram`] but for a discovered
/// path MTU rather than the fixed clamp.
pub fn max_tunnel_datagram_at(path_mtu: usize) -> usize {
    path_mtu - OUTER_IPV4_HEADER_SIZE - OUTER_UDP_HEADER_SIZE
}

/// Latest game payload budget derived from a path MTU (bytes).
///
/// Mirrors the arithmetic in [`lightspeed_protocol::max_game_payload`] but for
/// an arbitrary discovered path MTU rather than the fixed clamp.
pub fn max_game_payload_at(path_mtu: usize, fec: bool) -> usize {
    let mut budget = path_mtu - OUTER_IPV4_HEADER_SIZE - OUTER_UDP_HEADER_SIZE - HEADER_SIZE;
    if fec {
        budget -= FEC_HEADER_SIZE + FEC_PARITY_TRAILER_SIZE;
    }
    budget
}

/// The pure discovery policy: a path MTU clamped to the safe range.
///
/// It starts at the conservative clamp and is only ever moved by observations
/// and failures fed to it. It never holds a value outside
/// `[CONSERVATIVE_PATH_MTU, HARD_CAP_PATH_MTU]`, so the effective budget is
/// always safe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PathMtuDiscovery {
    config: PmtudConfig,
    trusted_path_mtu: usize,
}

impl PathMtuDiscovery {
    /// Create a policy that starts at the conservative clamp.
    pub fn new(config: PmtudConfig) -> Self {
        Self {
            config,
            trusted_path_mtu: CONSERVATIVE_PATH_MTU,
        }
    }

    /// Whether discovery is enabled.
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    /// The path MTU in force, in bytes.
    ///
    /// Always clamped to `[CONSERVATIVE_PATH_MTU, HARD_CAP_PATH_MTU]`, so this
    /// is safe to consume directly.
    pub fn effective_path_mtu(&self) -> usize {
        self.trusted_path_mtu
            .clamp(CONSERVATIVE_PATH_MTU, HARD_CAP_PATH_MTU)
    }

    /// The largest tunnel datagram (LightSpeed header(s) plus game payload)
    /// that fits the effective path MTU without fragmenting.
    pub fn effective_tunnel_datagram(&self) -> usize {
        self.effective_path_mtu() - OUTER_IPV4_HEADER_SIZE - OUTER_UDP_HEADER_SIZE
    }

    /// The largest game payload that fits the effective path MTU for `fec`.
    pub fn effective_game_payload(&self, fec: bool) -> usize {
        max_game_payload_at(self.effective_path_mtu(), fec)
    }

    /// Record an observed path MTU (a successful probe, or the control plane's
    /// current estimate).
    ///
    /// The observation is clamped into `[CONSERVATIVE_PATH_MTU,
    /// HARD_CAP_PATH_MTU]`; a value below the clamp therefore falls back to the
    /// clamp, and a value above the cap never exceeds it. Returns `true` when
    /// the effective MTU changed. A disabled policy ignores the observation.
    pub fn record_discovered(&mut self, path_mtu: usize) -> bool {
        if !self.config.enabled {
            return false;
        }
        let observed = path_mtu.clamp(CONSERVATIVE_PATH_MTU, HARD_CAP_PATH_MTU);
        if observed == self.trusted_path_mtu {
            return false;
        }
        self.trusted_path_mtu = observed;
        true
    }

    /// Record a failed or ambiguous probe: fall back to the conservative clamp.
    /// Returns `true` when the effective MTU changed. A disabled policy is a
    /// no-op so disabling discovery reproduces today's behaviour exactly.
    pub fn record_failure(&mut self) -> bool {
        if !self.config.enabled {
            return false;
        }
        if self.trusted_path_mtu == CONSERVATIVE_PATH_MTU {
            return false;
        }
        self.trusted_path_mtu = CONSERVATIVE_PATH_MTU;
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightspeed_protocol::{max_game_payload, max_tunnel_datagram};

    fn enabled() -> PathMtuDiscovery {
        PathMtuDiscovery::new(PmtudConfig { enabled: true })
    }

    // ── RED/GREEN: raise on success ─────────────────────────────────────

    #[test]
    fn discovery_raises_when_a_larger_path_is_confirmed() {
        let mut pmtud = enabled();
        assert_eq!(
            pmtud.effective_path_mtu(),
            CONSERVATIVE_PATH_MTU,
            "must start at the conservative clamp and never below it"
        );

        assert!(pmtud.record_discovered(HARD_CAP_PATH_MTU));
        assert_eq!(
            pmtud.effective_path_mtu(),
            HARD_CAP_PATH_MTU,
            "a confirmed larger path raises the effective MTU"
        );
        assert_eq!(
            pmtud.effective_tunnel_datagram(),
            HARD_CAP_PATH_MTU - OUTER_IPV4_HEADER_SIZE - OUTER_UDP_HEADER_SIZE
        );
        assert_eq!(
            pmtud.effective_game_payload(false),
            max_game_payload_at(HARD_CAP_PATH_MTU, false)
        );
    }

    // ── RED/GREEN: fall back on failure ─────────────────────────────────

    #[test]
    fn any_failure_falls_back_to_the_conservative_clamp() {
        let mut pmtud = enabled();
        assert!(pmtud.record_discovered(HARD_CAP_PATH_MTU));

        assert!(pmtud.record_failure());
        assert_eq!(
            pmtud.effective_path_mtu(),
            CONSERVATIVE_PATH_MTU,
            "a failed or ambiguous probe must fall back to the safe clamp"
        );
        assert_eq!(
            pmtud.effective_tunnel_datagram(),
            max_tunnel_datagram(),
            "the fallback budget must equal today's clamp exactly"
        );
    }

    #[test]
    fn an_observation_below_the_clamp_falls_back_to_the_clamp() {
        let mut pmtud = enabled();
        assert!(pmtud.record_discovered(HARD_CAP_PATH_MTU));

        assert!(pmtud.record_discovered(1200));
        assert_eq!(
            pmtud.effective_path_mtu(),
            CONSERVATIVE_PATH_MTU,
            "a shrunken path never takes the budget below the clamp"
        );
    }

    // ── RED/GREEN: never exceed the hard cap ────────────────────────────

    #[test]
    fn discovered_mtu_never_exceeds_the_hard_cap() {
        let mut pmtud = enabled();
        pmtud.record_discovered(9000);
        assert_eq!(pmtud.effective_path_mtu(), HARD_CAP_PATH_MTU);
        assert!(pmtud.effective_path_mtu() <= HARD_CAP_PATH_MTU);

        pmtud.record_discovered(usize::MAX);
        assert_eq!(pmtud.effective_path_mtu(), HARD_CAP_PATH_MTU);
        assert_eq!(
            pmtud.effective_tunnel_datagram(),
            HARD_CAP_UDP_PAYLOAD,
            "the datagram budget must never exceed the capped UDP payload"
        );
    }

    // ── RED/GREEN: disabled is the default and changes nothing ──────────

    #[test]
    fn discovery_is_disabled_by_default_and_changes_nothing() {
        let mut pmtud = PathMtuDiscovery::new(PmtudConfig::default());
        assert!(!pmtud.enabled(), "discovery must be opt-in");

        assert!(
            !pmtud.record_discovered(HARD_CAP_PATH_MTU),
            "a disabled policy must ignore observations"
        );
        assert!(!pmtud.record_failure());
        assert_eq!(pmtud.effective_path_mtu(), CONSERVATIVE_PATH_MTU);
        assert_eq!(pmtud.effective_tunnel_datagram(), max_tunnel_datagram());
        assert_eq!(pmtud.effective_game_payload(false), max_game_payload(false));
        assert_eq!(pmtud.effective_game_payload(true), max_game_payload(true));
    }
}
