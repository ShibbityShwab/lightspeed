//! Conservative client-to-relay payload budget.
//!
//! Every game datagram the client tunnels gains a LightSpeed header, so a
//! datagram sized to the game's own path MTU no longer fits once wrapped.
//! [`classify`] compares a payload against [`max_game_payload`] and the send
//! sites drop and count anything over budget instead of emitting a datagram
//! the kernel would fragment. This is a fixed clamp, not path-MTU discovery.

pub use lightspeed_protocol::max_game_payload;

/// Whether a game payload fits the conservative tunnel budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PayloadFit {
    /// The wrapped datagram fits the clamped path MTU.
    Fits,
    /// The wrapped datagram would exceed the clamped path MTU.
    OverBudget { payload_len: usize, budget: usize },
}

impl PayloadFit {
    /// True when the payload fits and may be tunnelled.
    pub fn fits(self) -> bool {
        matches!(self, Self::Fits)
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
    }
}
