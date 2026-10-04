//! Relay-switch hysteresis for the GUI's continuous auto-select re-check.
//!
//! While auto-select is on and the tunnel is up, the GUI periodically re-runs
//! the relay race. A re-race winner is often only a hair faster than the relay
//! already in use, and chasing that hair every re-check would flap the tunnel
//! between near-equal relays. [`decide_switch`] keeps the current relay unless
//! the winner beats it by a clear margin.

/// Whether a re-race winner is enough faster to justify switching relays.
///
/// Returns `true` only when `winner_rtt_ms + margin_ms` is strictly less than
/// `current_rtt_ms`; a tie or a worse winner keeps the current relay. The
/// addition saturates, so extreme inputs cannot wrap around and produce a
/// spurious switch.
pub fn decide_switch(current_rtt_ms: u64, winner_rtt_ms: u64, margin_ms: u64) -> bool {
    winner_rtt_ms.saturating_add(margin_ms) < current_rtt_ms
}

#[cfg(test)]
mod tests {
    use super::decide_switch;

    #[test]
    fn switches_when_the_winner_is_clearly_better() {
        // 60 + 20 = 80 < 100
        assert!(decide_switch(100, 60, 20));
        assert!(decide_switch(100, 79, 20));
    }

    #[test]
    fn keeps_the_current_relay_within_the_margin() {
        // Exactly at the margin is not "clearly better", so no switch.
        assert!(!decide_switch(100, 80, 20));
        // 85 + 20 = 105, not < 100.
        assert!(!decide_switch(100, 85, 20));
        // Zero margin still requires the winner to be strictly faster.
        assert!(!decide_switch(100, 100, 0));
    }

    #[test]
    fn keeps_the_current_relay_when_the_winner_is_worse() {
        assert!(!decide_switch(50, 90, 5));
        assert!(!decide_switch(50, 51, 0));
    }

    #[test]
    fn extreme_values_do_not_wrap_into_a_switch() {
        assert!(!decide_switch(u64::MAX, u64::MAX, 1));
        assert!(!decide_switch(u64::MAX, u64::MAX - 1, 10));
    }
}
