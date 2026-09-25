//! # Conservative outbound pacing
//!
//! The tunnel sends raw UDP with no congestion control, so an unpaced client
//! under load can cause the very loss it exists to fix. This is a deliberately
//! minimal token-bucket pacer, **not** a congestion-control algorithm:
//!
//! * the bucket bounds the burst a client may emit at once,
//! * sustained rate is capped by a conservative ceiling, and
//! * the rate decreases multiplicatively on measured loss and recovers
//!   additively, slowly, back toward the ceiling.
//!
//! There is no RTT estimation, no slow start, and no fairness signalling. The
//! floor ([`MIN_RATE_BPS`]) exists so pacing can never starve gameplay, and the
//! ceiling exists so pacing never becomes the bottleneck on a healthy path.

use std::time::Duration;

use tokio::time::Instant;

/// Rate ceiling for one client, in bytes per second. Conservative: real
/// gameplay is a few tens of KB/s, and this leaves headroom for FEC parity.
pub const DEFAULT_CEILING_BPS: u64 = 2 * 1024 * 1024;
/// Hard floor. Pacing never drops below this, to avoid starving gameplay.
pub const MIN_RATE_BPS: u64 = 32 * 1024;
/// Loss above this percentage (0.0..=100.0) triggers a rate decrease.
pub const LOSS_THRESHOLD_PCT: f64 = 2.0;
/// Smallest burst allowance, so a single MTU-sized datagram still fits.
const MIN_BURST_BYTES: u64 = 1400;
/// Multiplicative decrease on loss: rate * 4/5.
const DECREASE_NUM: u64 = 4;
const DECREASE_DEN: u64 = 5;
/// Additive increase per recovery step, in bytes per second.
const RECOVERY_STEP_BPS: u64 = 16 * 1024;
/// Minimum spacing between recovery steps.
const RECOVERY_INTERVAL: Duration = Duration::from_secs(1);

/// Returns the burst allowance for `rate_bps`: one fifth of a second of
/// traffic, never below [`MIN_BURST_BYTES`].
fn burst_for(rate_bps: u64) -> u64 {
    (rate_bps / 5).max(MIN_BURST_BYTES)
}

/// A token-bucket pacer with a loss-responsive rate.
pub struct Pacer {
    rate_bps: u64,
    ceiling_bps: u64,
    burst_bytes: u64,
    tokens: f64,
    last_refill: Instant,
    last_recovery: Instant,
}

impl Pacer {
    /// A pacer whose ceiling is `rate_bps`, clamped into
    /// [`MIN_RATE_BPS`]..=`rate_bps`.
    pub fn new(rate_bps: u64) -> Self {
        Self::with_ceiling(rate_bps, rate_bps)
    }

    /// A pacer that starts at `initial_bps` and may recover up to
    /// `ceiling_bps`.
    pub fn with_ceiling(initial_bps: u64, ceiling_bps: u64) -> Self {
        let ceiling = ceiling_bps.max(MIN_RATE_BPS);
        let rate = initial_bps.clamp(MIN_RATE_BPS, ceiling);
        let burst = burst_for(rate);
        Self {
            rate_bps: rate,
            ceiling_bps: ceiling,
            burst_bytes: burst,
            tokens: burst as f64,
            last_refill: Instant::now(),
            last_recovery: Instant::now(),
        }
    }

    /// Current rate in bytes per second.
    pub fn rate_bps(&self) -> u64 {
        self.rate_bps
    }

    /// Ceiling the rate may recover to.
    pub fn ceiling_bps(&self) -> u64 {
        self.ceiling_bps
    }

    /// Floor the rate may fall to.
    pub fn floor_bps(&self) -> u64 {
        MIN_RATE_BPS
    }

    /// Burst allowance in bytes.
    pub fn burst_bytes(&self) -> u64 {
        self.burst_bytes
    }

    fn refill_to(&mut self, now: Instant, capacity: f64) {
        let elapsed = now.saturating_duration_since(self.last_refill);
        self.last_refill = now;
        let added = elapsed.as_secs_f64() * self.rate_bps as f64;
        self.tokens = (self.tokens + added).min(capacity);
    }

    /// Wait until `bytes` may be sent under the current rate, then consume
    /// them. The wait is skipped entirely while the bucket has tokens.
    pub async fn acquire(&mut self, bytes: usize) {
        if bytes == 0 {
            return;
        }
        let need = bytes as f64;
        // A single datagram larger than the bucket is allowed to borrow a
        // proportional bucket, otherwise it could never be sent.
        let capacity = (self.burst_bytes as f64).max(need);
        loop {
            self.refill_to(Instant::now(), capacity);
            if self.tokens >= need {
                self.tokens -= need;
                return;
            }
            let rate = self.rate_bps.max(1) as f64;
            let wait_secs = ((need - self.tokens) / rate).max(0.0005);
            tokio::time::sleep(Duration::from_secs_f64(wait_secs)).await;
        }
    }

    /// Record a loss event: decrease the rate multiplicatively, bounded below.
    pub fn on_loss(&mut self) {
        let decreased = self.rate_bps.saturating_mul(DECREASE_NUM) / DECREASE_DEN;
        self.rate_bps = decreased.max(MIN_RATE_BPS).min(self.ceiling_bps);
    }

    /// Record a measured loss percentage: decrease only when it exceeds
    /// [`LOSS_THRESHOLD_PCT`].
    pub fn on_measured_loss(&mut self, loss_pct: f64) {
        if loss_pct.is_finite() && loss_pct > LOSS_THRESHOLD_PCT {
            self.on_loss();
        }
    }

    /// One recovery step: increase the rate additively, bounded above. Call
    /// this on a slow timer (about once per second), never per packet.
    pub fn on_recovery(&mut self) {
        self.rate_bps = self
            .rate_bps
            .saturating_add(RECOVERY_STEP_BPS)
            .min(self.ceiling_bps);
    }

    /// One recovery step, but rate-limited to at most one per
    /// [`RECOVERY_INTERVAL`]. Safe to call on every received packet.
    pub fn maybe_recover(&mut self) {
        let now = Instant::now();
        if now.saturating_duration_since(self.last_recovery) >= RECOVERY_INTERVAL {
            self.last_recovery = now;
            self.on_recovery();
        }
    }
}

impl Default for Pacer {
    fn default() -> Self {
        Self::with_ceiling(DEFAULT_CEILING_BPS, DEFAULT_CEILING_BPS)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn burst_is_bounded_by_the_token_bucket() {
        let rate: u64 = 1_000_000;
        let mut pacer = Pacer::new(rate);
        let burst = pacer.burst_bytes() as usize;

        let start = Instant::now();
        pacer.acquire(burst).await;
        assert!(
            start.elapsed() < Duration::from_millis(1),
            "a full bucket must send immediately"
        );

        pacer.acquire(burst).await;
        let expected = Duration::from_secs_f64(burst as f64 / rate as f64);
        assert!(
            start.elapsed() >= expected.mul_f64(0.9),
            "the next burst must wait for the bucket to refill, took {:?} (expected ~{expected:?})",
            start.elapsed()
        );
    }

    #[tokio::test(start_paused = true)]
    async fn sustained_rate_matches_the_token_bucket() {
        let rate: u64 = 1_000_000;
        let mut pacer = Pacer::new(rate);
        let burst = pacer.burst_bytes() as usize;
        pacer.acquire(burst).await;

        let start = Instant::now();
        let chunk = burst / 4;
        for _ in 0..4 {
            pacer.acquire(chunk).await;
        }
        let expected = Duration::from_secs_f64((chunk * 4) as f64 / rate as f64);
        let elapsed = start.elapsed();
        assert!(
            elapsed >= expected.mul_f64(0.9),
            "a bucket of traffic must not finish in {elapsed:?} (expected ~{expected:?})"
        );
        assert!(
            elapsed <= expected.mul_f64(1.5),
            "pacing should not overshoot, took {elapsed:?}"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn first_burst_is_immediate() {
        let mut pacer = Pacer::new(1_000_000);
        let chunk = pacer.burst_bytes() as usize / 10;
        let start = Instant::now();
        for _ in 0..10 {
            pacer.acquire(chunk).await;
        }
        assert!(start.elapsed() < Duration::from_millis(1));
    }

    #[test]
    fn loss_decreases_the_rate_multiplicatively() {
        let mut pacer = Pacer::with_ceiling(1_000_000, 1_000_000);
        assert_eq!(pacer.rate_bps(), 1_000_000);
        pacer.on_loss();
        assert_eq!(pacer.rate_bps(), 800_000);
    }

    #[test]
    fn recovery_is_slow_and_capped() {
        let mut pacer = Pacer::with_ceiling(1_000_000, 1_000_000);
        pacer.on_loss();
        let after_loss = pacer.rate_bps();
        pacer.on_recovery();
        assert!(pacer.rate_bps() > after_loss);
        assert!(
            pacer.rate_bps() <= after_loss + RECOVERY_STEP_BPS,
            "one recovery step must be additive and small"
        );
        for _ in 0..1_000 {
            pacer.on_recovery();
        }
        assert_eq!(pacer.rate_bps(), 1_000_000);
    }

    #[test]
    fn repeated_loss_never_falls_below_floor() {
        let mut pacer = Pacer::new(MIN_RATE_BPS * 10);
        for _ in 0..1_000 {
            pacer.on_loss();
        }
        assert_eq!(pacer.rate_bps(), MIN_RATE_BPS);
    }

    #[test]
    fn measured_loss_below_threshold_is_ignored() {
        let mut pacer = Pacer::new(1_000_000);
        let before = pacer.rate_bps();
        pacer.on_measured_loss(LOSS_THRESHOLD_PCT - 0.5);
        assert_eq!(pacer.rate_bps(), before);
        pacer.on_measured_loss(LOSS_THRESHOLD_PCT + 5.0);
        assert!(pacer.rate_bps() < before);
    }

    #[test]
    fn initial_rate_is_clamped_to_the_floor() {
        let pacer = Pacer::new(1);
        assert_eq!(pacer.rate_bps(), MIN_RATE_BPS);
    }
}
