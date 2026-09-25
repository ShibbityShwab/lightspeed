//! Conservative client-to-relay payload budget.
//!
//! Every game datagram the client tunnels gains a LightSpeed header, so a
//! datagram sized to the game's own path MTU no longer fits once wrapped.
//! [`classify`] compares a payload against [`max_game_payload`]. A payload that
//! fits is sent under the socket's don't-fragment guarantee. A payload over
//! budget is never dropped: dropping real game state stutters the game. It is
//! forwarded with don't-fragment temporarily cleared so the local kernel may
//! fragment it, and it is counted. This is a fixed clamp over the
//! client-to-relay hop, not path-MTU discovery.

pub use lightspeed_protocol::{max_game_payload, max_tunnel_datagram};

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
pub fn classify(payload_len: usize, fec: bool) -> PayloadFit {
    let budget = max_game_payload(fec);
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
    datagram_len <= max_tunnel_datagram()
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
