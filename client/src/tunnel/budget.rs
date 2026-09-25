//! Conservative client-to-relay payload budget.
//!
//! Every game datagram the client tunnels gains a LightSpeed header, so a
//! datagram sized to the game's own path MTU no longer fits once wrapped.
//! [`classify`] compares a payload against
//! [`lightspeed_protocol::max_game_payload`]. A payload that
//! fits is sent under the socket's don't-fragment guarantee. A payload over
//! budget is never dropped: dropping real game state stutters the game. It is
//! forwarded with don't-fragment temporarily cleared so the local kernel may
//! fragment it, and it is counted. This is a fixed clamp over the
//! client-to-relay hop, not path-MTU discovery.

use crate::tunnel::pmtud::{max_game_payload_at, max_tunnel_datagram_at};

/// Whether a game payload fits the conservative tunnel budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadFit {
    /// The wrapped datagram fits the clamped path MTU without fragmenting.
    Fits,
    /// The wrapped datagram exceeds the clamped path MTU. It is forwarded with
    /// kernel fragmentation allowed rather than dropped.
    OverBudget { payload_len: usize, budget: usize },
}

impl PayloadFit {
    /// True when the payload fits and needs no fragmentation.
    pub fn fits(self) -> bool {
        matches!(self, Self::Fits)
    }

    /// True when the payload exceeds the conservative budget.
    pub fn is_oversized(self) -> bool {
        !self.fits()
    }
}

/// Classify `payload_len` against the budget for the current FEC mode.
///
/// Uses the fixed conservative clamp. Callers that have a discovered path MTU
/// should use [`classify_at`].
pub fn classify(payload_len: usize, fec: bool) -> PayloadFit {
    classify_at(payload_len, fec, lightspeed_protocol::CONSERVATIVE_PATH_MTU)
}

/// Classify `payload_len` against the budget for an effective `path_mtu`.
///
/// This is the discovery-aware form: the caller passes the path MTU in force
/// (the conservative clamp by default, or a discovered-and-capped value).
pub fn classify_at(payload_len: usize, fec: bool, path_mtu: usize) -> PayloadFit {
    let budget = max_game_payload_at(path_mtu, fec);
    if payload_len > budget {
        PayloadFit::OverBudget {
            payload_len,
            budget,
        }
    } else {
        PayloadFit::Fits
    }
}

/// Whether an already-encoded tunnel datagram fits the clamp.
///
/// Used to decide, per datagram (including an FEC parity packet, whose size
/// differs from the data packet), whether the socket's don't-fragment bit must
/// be cleared before sending.
pub fn datagram_fits(datagram_len: usize) -> bool {
    datagram_fits_at(datagram_len, lightspeed_protocol::CONSERVATIVE_PATH_MTU)
}

/// Whether an already-encoded tunnel datagram fits an effective `path_mtu`.
///
/// This is the discovery-aware form of [`datagram_fits`]: once discovery has
/// raised the path MTU, a larger datagram is sent with don't-fragment still set
/// instead of being fragmented.
pub fn datagram_fits_at(datagram_len: usize, path_mtu: usize) -> bool {
    datagram_len <= max_tunnel_datagram_at(path_mtu)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lightspeed_protocol::{max_game_payload, max_tunnel_datagram};

    #[test]
    fn payload_exactly_at_the_plain_budget_fits() {
        let budget = max_game_payload(false);
        assert_eq!(classify(budget, false), PayloadFit::Fits);
    }

    #[test]
    fn payload_one_byte_over_the_plain_budget_is_reported() {
        let budget = max_game_payload(false);
        assert_eq!(
            classify(budget + 1, false),
            PayloadFit::OverBudget {
                payload_len: budget + 1,
                budget,
            }
        );
        assert!(!classify(budget + 1, false).fits());
    }

    #[test]
    fn fec_budget_is_smaller_than_the_plain_budget() {
        assert!(max_game_payload(true) < max_game_payload(false));
        assert_eq!(
            max_game_payload(false) - max_game_payload(true),
            4 + lightspeed_protocol::FEC_PARITY_TRAILER_SIZE
        );
    }

    #[test]
    fn over_budget_payload_is_flagged_in_fec_mode() {
        let budget = max_game_payload(true);
        assert_eq!(
            classify(budget + 1, true),
            PayloadFit::OverBudget {
                payload_len: budget + 1,
                budget,
            }
        );
        assert!(classify(budget + 1, true).is_oversized());
    }

    #[test]
    fn datagram_fits_matches_the_clamp() {
        assert!(datagram_fits(max_tunnel_datagram()));
        assert!(!datagram_fits(max_tunnel_datagram() + 1));
    }

    #[test]
    fn classify_at_widens_the_budget_to_the_discovered_path() {
        use crate::tunnel::pmtud::HARD_CAP_PATH_MTU;

        let conservative = max_game_payload(false);
        let raised = crate::tunnel::pmtud::max_game_payload_at(HARD_CAP_PATH_MTU, false);
        assert!(raised > conservative);
        assert_eq!(
            classify(conservative + 1, false),
            PayloadFit::OverBudget {
                payload_len: conservative + 1,
                budget: conservative,
            }
        );
        assert_eq!(
            classify_at(conservative + 1, false, HARD_CAP_PATH_MTU),
            PayloadFit::Fits,
            "a payload between the clamp and the discovered budget must fit"
        );
    }

    #[test]
    fn datagram_fits_at_uses_the_discovered_path() {
        use crate::tunnel::pmtud::HARD_CAP_PATH_MTU;

        let raised = crate::tunnel::pmtud::max_tunnel_datagram_at(HARD_CAP_PATH_MTU);
        assert!(raised > max_tunnel_datagram());
        assert!(!datagram_fits(max_tunnel_datagram() + 1));
        assert!(datagram_fits_at(
            max_tunnel_datagram() + 1,
            HARD_CAP_PATH_MTU
        ));
        assert!(datagram_fits_at(raised, HARD_CAP_PATH_MTU));
        assert!(!datagram_fits_at(raised + 1, HARD_CAP_PATH_MTU));
    }
}
