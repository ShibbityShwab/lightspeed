//! Pure, I/O-free server-rotation state machine for the Linux interceptor.
//!
//! The Linux nftables REDIRECT rule matches exactly one game-server address at
//! a time, so the interceptor must decide when to install, keep, swap, or tear
//! down that rule as the [`ProcessScanner`] observes the game's connected UDP
//! routes change. This module is the single source of truth for that decision.
//!
//! It performs no I/O and takes `now` as an explicit parameter, so the full
//! state machine is deterministically unit-testable without root, without
//! nftables, and without a game.
//!
//! ## Why the swap gate is conservative
//!
//! Deleting a REDIRECT rule does **not** unhook an established flow: conntrack
//! keeps redirecting that flow's packets to our listener for up to 30 s
//! (unreplied) / 120 s (streamed). A naive "swap the rule, then tunnel
//! everything to the new server" would therefore misroute the old flow's
//! packets to the new server. The gate below only swaps when the scanner has
//! stopped reporting the old server, the old server has been silent for
//! [`ROTATE_SILENCE`], and the *same* candidate has been reported on
//! [`SWAP_CONFIRMATIONS`] consecutive scans.
//!
//! [`ProcessScanner`]: super::process_scanner

use std::net::SocketAddrV4;
use std::time::{Duration, Instant};

/// How often the interceptor polls the [`ProcessScanner`] for routes.
///
/// [`ProcessScanner`]: super::process_scanner
pub const ROTATE_SCAN_INTERVAL: Duration = Duration::from_secs(5);

/// How long the current server must have been silent (no intercepted packets)
/// before a swap away from it is even considered.
pub const ROTATE_SILENCE: Duration = Duration::from_secs(5);

/// Consecutive scans that must report the same replacement candidate.
pub const SWAP_CONFIRMATIONS: u32 = 2;

/// Consecutive scans with no candidate at all before the rule is torn down.
pub const TEARDOWN_SCANS: u32 = 3;

/// The action the interceptor should take after a scan.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// No server known and no candidate: install nothing, leave traffic direct.
    Wait,
    /// Install a REDIRECT rule for the discovered server.
    Install(SocketAddrV4),
    /// Keep the currently installed rule unchanged.
    Keep,
    /// Atomically replace the rule with a REDIRECT for the new server.
    Swap(SocketAddrV4),
    /// Remove the rule entirely (game has no matching public route).
    Teardown,
}

/// Server-rotation state machine.
///
/// Feed every [`ProcessScanner`] scan to [`on_scan`] and every intercepted
/// packet to [`note_intercepted_packet`].
///
/// [`ProcessScanner`]: super::process_scanner
/// [`on_scan`]: RotationTracker::on_scan
#[derive(Debug, Default)]
pub struct RotationTracker {
    current: Option<SocketAddrV4>,
    last_packet_at: Option<Instant>,
    /// Consecutive scans that reported no candidate at all.
    empty_scans: u32,
    /// Candidate observed on the previous scan while `current` was absent.
    pending: Option<SocketAddrV4>,
}

impl RotationTracker {
    /// Create a tracker with no server installed.
    pub fn new() -> Self {
        Self::default()
    }

    /// The server the rule currently matches, if any.
    pub fn current(&self) -> Option<SocketAddrV4> {
        self.current
    }

    /// Record that an intercepted packet arrived for the current server.
    ///
    /// Refreshes the silence anchor that the swap gate checks.
    pub fn note_intercepted_packet(&mut self, now: Instant) {
        self.last_packet_at = Some(now);
    }

    /// Forget all state so the next scan starts from a clean `Wait`/`Install`.
    ///
    /// Used by the caller to re-synchronise after a failed rule transaction:
    /// the tracker's notion of `current` must never diverge from the rule that
    /// is actually installed.
    pub fn reset(&mut self) {
        self.current = None;
        self.empty_scans = 0;
        self.pending = None;
    }

    /// Decide what to do given the scanner's current public-route candidates.
    pub fn on_scan(&mut self, candidates: &[SocketAddrV4], now: Instant) -> Action {
        let Some(current) = self.current else {
            return match candidates.first().copied() {
                Some(server) => {
                    self.current = Some(server);
                    self.empty_scans = 0;
                    self.pending = None;
                    Action::Install(server)
                }
                None => Action::Wait,
            };
        };

        if candidates.contains(&current) {
            self.empty_scans = 0;
            self.pending = None;
            return Action::Keep;
        }

        // The current server is gone. With no candidate at all the game has no
        // matching public route; after `TEARDOWN_SCANS` in a row, remove the
        // rule so it cannot hijack unrelated traffic.
        if candidates.is_empty() {
            self.pending = None;
            self.empty_scans = self.empty_scans.saturating_add(1);
            if self.empty_scans >= TEARDOWN_SCANS {
                self.current = None;
                self.empty_scans = 0;
                return Action::Teardown;
            }
            return Action::Keep;
        }
        self.empty_scans = 0;

        // A replacement candidate exists. The conservative gate requires the
        // old server to have been silent, and the *same* candidate on two
        // consecutive silent scans.
        let Some(replacement) = candidates.iter().copied().find(|c| *c != current) else {
            self.pending = None;
            return Action::Keep;
        };

        let silent = self
            .last_packet_at
            .is_none_or(|t| now.saturating_duration_since(t) >= ROTATE_SILENCE);
        if !silent {
            // Silence not met: this scan provides no evidence toward a swap.
            self.pending = None;
            return Action::Keep;
        }
        if self.pending == Some(replacement) {
            self.current = Some(replacement);
            self.pending = None;
            Action::Swap(replacement)
        } else {
            self.pending = Some(replacement);
            Action::Keep
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn addr(last: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, last), 34568)
    }

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn no_current_no_candidate_waits() {
        let mut t = RotationTracker::new();
        assert_eq!(t.on_scan(&[], Instant::now()), Action::Wait);
        assert_eq!(t.current(), None);
    }

    #[test]
    fn no_current_with_candidate_installs() {
        let b = addr(9);
        let mut t = RotationTracker::new();
        assert_eq!(t.on_scan(&[b], Instant::now()), Action::Install(b));
        assert_eq!(t.current(), Some(b));
    }

    #[test]
    fn current_still_reported_keeps() {
        let b = addr(9);
        let mut t = RotationTracker::new();
        let t0 = Instant::now();
        assert_eq!(t.on_scan(&[b], t0), Action::Install(b));
        assert_eq!(t.on_scan(&[b], at(t0, 5)), Action::Keep);
        assert_eq!(t.current(), Some(b));
    }

    #[test]
    fn current_absent_silent_two_scans_swaps() {
        let a = addr(9);
        let b = addr(10);
        let t0 = Instant::now();
        let mut t = RotationTracker::new();
        assert_eq!(t.on_scan(&[a], t0), Action::Install(a));

        // Last packet to A at t0+1s: the 5s silence gate is not met yet.
        t.note_intercepted_packet(at(t0, 1));
        assert_eq!(t.on_scan(&[b], at(t0, 2)), Action::Keep);
        // First *silent* scan (t0+6 > t0+1+5): records B.
        assert_eq!(t.on_scan(&[b], at(t0, 6)), Action::Keep);
        // Second consecutive silent scan of the same B: swap.
        assert_eq!(t.on_scan(&[b], at(t0, 8)), Action::Swap(b));
        assert_eq!(t.current(), Some(b));
    }

    #[test]
    fn current_absent_silence_not_satisfied_keeps() {
        let a = addr(9);
        let b = addr(10);
        let t0 = Instant::now();
        let mut t = RotationTracker::new();
        assert_eq!(t.on_scan(&[a], t0), Action::Install(a));
        t.note_intercepted_packet(t0);

        // B appears immediately, but A was talking < 5s ago.
        assert_eq!(t.on_scan(&[b], at(t0, 1)), Action::Keep);
        assert_eq!(t.on_scan(&[b], at(t0, 2)), Action::Keep);
        assert_eq!(
            t.current(),
            Some(a),
            "must stay on A until A is silent for 5s"
        );
    }

    #[test]
    fn no_candidates_three_scans_teardowns() {
        let a = addr(9);
        let t0 = Instant::now();
        let mut t = RotationTracker::new();
        assert_eq!(t.on_scan(&[a], t0), Action::Install(a));
        assert_eq!(t.on_scan(&[], at(t0, 5)), Action::Keep);
        assert_eq!(t.on_scan(&[], at(t0, 10)), Action::Keep);
        assert_eq!(t.on_scan(&[], at(t0, 15)), Action::Teardown);
        assert_eq!(t.current(), None);
    }

    #[test]
    fn flapping_does_not_swap_until_two_consecutive() {
        let a = addr(9);
        let b = addr(10);
        let t0 = Instant::now();
        let mut t = RotationTracker::new();
        assert_eq!(t.on_scan(&[a], t0), Action::Install(a));
        t.note_intercepted_packet(t0);

        // B (silent), then A reappears (resets the candidate), then B (silent
        // once more): the gate must not be met because B was never reported on
        // two *consecutive* scans.
        assert_eq!(t.on_scan(&[b], at(t0, 6)), Action::Keep);
        assert_eq!(t.on_scan(&[a], at(t0, 7)), Action::Keep);
        assert_eq!(t.on_scan(&[b], at(t0, 8)), Action::Keep);
        assert_eq!(t.current(), Some(a), "flapping B/A/B must not swap");

        // Now B is reported twice in a row while A stays silent.
        assert_eq!(t.on_scan(&[b], at(t0, 9)), Action::Swap(b));
        assert_eq!(t.current(), Some(b));
    }
}
