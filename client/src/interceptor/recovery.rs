//! Pure retry policy for transient WinDivert receive failures.
//!
//! A single failed `WinDivert::recv` (a transient `WSAETIMEDOUT`, an aborted
//! packet, a momentarily invalidated filter) used to tear the whole intercept
//! thread down permanently. This policy decides how long to wait before
//! retrying and when a run of failures is genuinely fatal, so the retry logic
//! is identical in both WinDivert backends and testable without the driver.

use std::time::Duration;

/// Bounded exponential-backoff counter for a blocking receive loop.
#[derive(Debug, Clone)]
pub struct RecvBackoff {
    max_retries: u32,
    base: Duration,
    max_delay: Duration,
    consecutive: u32,
}

impl RecvBackoff {
    /// `max_retries` failures are retried before the session is torn down;
    /// delays grow from `base` up to `max_delay`.
    pub fn new(max_retries: u32, base: Duration, max_delay: Duration) -> Self {
        Self {
            max_retries,
            base,
            max_delay,
            consecutive: 0,
        }
    }

    /// Record a failed receive.
    ///
    /// Returns the delay to wait before retrying, or `None` once the retry
    /// budget is exhausted and the caller must tear the session down.
    pub fn on_failure(&mut self) -> Option<Duration> {
        self.consecutive = self.consecutive.saturating_add(1);
        if self.consecutive > self.max_retries {
            return None;
        }
        let shift = (self.consecutive - 1).min(16);
        Some((self.base * (1u32 << shift)).min(self.max_delay))
    }

    /// Record a successful receive, clearing the failure streak.
    pub fn on_success(&mut self) {
        self.consecutive = 0;
    }

    /// Number of consecutive failures recorded so far.
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn policy(max_retries: u32) -> RecvBackoff {
        RecvBackoff::new(
            max_retries,
            Duration::from_millis(10),
            Duration::from_millis(40),
        )
    }

    #[test]
    fn retries_with_exponential_backoff_up_to_the_cap() {
        let mut b = policy(4);
        assert_eq!(b.on_failure(), Some(Duration::from_millis(10)));
        assert_eq!(b.on_failure(), Some(Duration::from_millis(20)));
        assert_eq!(b.on_failure(), Some(Duration::from_millis(40)));
        // Capped, never above max_delay.
        assert_eq!(b.on_failure(), Some(Duration::from_millis(40)));
        assert_eq!(b.consecutive_failures(), 4);
    }

    #[test]
    fn tears_down_once_the_budget_is_exhausted() {
        let mut b = policy(2);
        assert!(b.on_failure().is_some());
        assert!(b.on_failure().is_some());
        assert_eq!(b.on_failure(), None);
        assert_eq!(b.on_failure(), None);
    }

    #[test]
    fn success_resets_the_streak() {
        let mut b = policy(2);
        assert!(b.on_failure().is_some());
        b.on_success();
        assert_eq!(b.consecutive_failures(), 0);
        // Budget is whole again after a good receive.
        assert_eq!(b.on_failure(), Some(Duration::from_millis(10)));
        assert!(b.on_failure().is_some());
        assert_eq!(b.on_failure(), None);
    }

    #[test]
    fn zero_budget_fails_immediately() {
        let mut b = policy(0);
        assert_eq!(b.on_failure(), None);
    }
}
