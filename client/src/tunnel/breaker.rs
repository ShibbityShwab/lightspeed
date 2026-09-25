//! # Relay circuit breaker
//!
//! The pacer ([`crate::tunnel::pacer`]) reacts to a **single** loss sample by
//! decreasing the send rate. That is correct for a burst, but it cannot tell a
//! one-off burst from a path that has genuinely gone bad, and it has no way to
//! stop using a relay that stays bad. This module is the state machine that
//! sits above the pacer: RFC 8085 guidance to back off and fail over rather
//! than keep hammering a broken path.
//!
//! ## States
//!
//! ```text
//!              sustained bad                    sustained bad
//!   Closed ────────────────────► BackingOff ──────────────────► Open
//!     ▲                              │                            │
//!     │ sustained clean              │ sustained clean            │ cooldown
//!     └──────────────────────────────┘                            ▼
//!     ▲                                                       HalfOpen
//!     │                          sustained clean                  │
//!     └───────────────────────────────────────────────────────────┘
//! ```
//!
//! * [`BreakerState::Closed`] is healthy; the breaker has no opinion and the
//!   relay behaves exactly as it did before this module existed.
//! * [`BreakerState::BackingOff`] means the loss estimate has stayed high for a
//!   sustained window. The relay keeps using the path but gives the pacer an
//!   extra multiplicative decrease.
//! * [`BreakerState::Open`] means the path is still bad after a longer window.
//!   The caller must stop using this relay for new sessions and fall back to
//!   the direct path or another relay ([`Breaker::should_fall_back`]).
//! * [`BreakerState::HalfOpen`] is the recovery trial after
//!   [`BreakerConfig::open_cooldown`]. A sustained clean window closes the
//!   breaker; a sustained bad window opens it again.
//!
//! ## Anti-oscillation
//!
//! Every transition needs both a sustained **duration** and a minimum number of
//! samples, so a single burst cannot trip the breaker and a single clean packet
//! cannot close it. The bad and clean thresholds are separated
//! (`bad_loss_pct` > `clean_loss_pct`), so a path hovering at the edge sits in
//! the neutral band rather than flapping. The bad window must also outlast the
//! EWMA decay of a burst: the window is reset the moment the estimate leaves
//! the bad band.
//!
//! ## Observability
//!
//! Every transition increments [`Breaker::transitions`] and emits a `tracing`
//! event: `warn` when degrading, `info` when recovering. [`Breaker::stats`]
//! returns a snapshot for metrics.

use std::time::Duration;

use tokio::time::Instant;

/// EWMA weight for a new loss sample (0.0..=1.0). Matches the adaptive FEC
/// controller so the two agree on what "current loss" means.
pub const DEFAULT_EWMA_ALPHA: f64 = 0.3;
/// Loss percentage at or above which the estimate is unhealthy.
pub const DEFAULT_BAD_LOSS_PCT: f64 = 2.0;
/// Loss percentage at or below which the estimate is healthy. Must be below
/// [`DEFAULT_BAD_LOSS_PCT`]; the gap is the anti-flap hysteresis band.
pub const DEFAULT_CLEAN_LOSS_PCT: f64 = 0.5;
/// How long the estimate must stay unhealthy before backing off.
pub const DEFAULT_BACKOFF_AFTER: Duration = Duration::from_secs(3);
/// How long the estimate must stay unhealthy (from the start of the bad run)
/// before the breaker opens.
pub const DEFAULT_OPEN_AFTER: Duration = Duration::from_secs(10);
/// Minimum samples in the bad window before any escalation.
pub const DEFAULT_MIN_BAD_SAMPLES: u32 = 8;
/// How long the breaker stays open before it trials recovery.
pub const DEFAULT_OPEN_COOLDOWN: Duration = Duration::from_secs(5);
/// How long the estimate must stay healthy before the breaker closes.
pub const DEFAULT_CLOSE_AFTER: Duration = Duration::from_secs(5);
/// Minimum samples in the clean window before closing.
pub const DEFAULT_MIN_CLEAN_SAMPLES: u32 = 8;

/// Breaker state. The numeric ordering (`metric_code`) rises with severity.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BreakerState {
    /// Healthy; no opinion.
    Closed,
    /// Sustained loss; back off the send rate but keep using the relay.
    BackingOff,
    /// Recovery trial after the open cooldown.
    HalfOpen,
    /// Path stayed bad; the caller must fall back.
    Open,
}

impl BreakerState {
    /// Stable lowercase name for logs and metrics.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Closed => "closed",
            Self::BackingOff => "backing_off",
            Self::HalfOpen => "half_open",
            Self::Open => "open",
        }
    }

    /// Numeric code for a metric gauge, rising with severity.
    pub fn metric_code(self) -> u64 {
        match self {
            Self::Closed => 0,
            Self::BackingOff => 1,
            Self::HalfOpen => 2,
            Self::Open => 3,
        }
    }
}

/// Tunables for [`Breaker`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreakerConfig {
    /// Master switch. When `false`, [`Breaker::on_sample`] is a no-op and the
    /// state is always [`BreakerState::Closed`].
    pub enabled: bool,
    /// EWMA weight for a new loss sample (clamped into `0.0..=1.0`).
    pub ewma_alpha: f64,
    /// Loss percent at or above which the estimate is unhealthy.
    pub bad_loss_pct: f64,
    /// Loss percent at or below which the estimate is healthy.
    pub clean_loss_pct: f64,
    /// Sustained unhealthy duration before [`BreakerState::BackingOff`].
    pub backoff_after: Duration,
    /// Sustained unhealthy duration (from the start of the bad run) before
    /// [`BreakerState::Open`]. Clamped up to at least `backoff_after`.
    pub open_after: Duration,
    /// Minimum raw bad samples in the bad window before escalating.
    pub min_bad_samples: u32,
    /// How long [`BreakerState::Open`] lasts before a recovery trial.
    pub open_cooldown: Duration,
    /// Sustained healthy duration before closing.
    pub close_after: Duration,
    /// Minimum clean samples in the healthy window before closing.
    pub min_clean_samples: u32,
}

impl Default for BreakerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            ewma_alpha: DEFAULT_EWMA_ALPHA,
            bad_loss_pct: DEFAULT_BAD_LOSS_PCT,
            clean_loss_pct: DEFAULT_CLEAN_LOSS_PCT,
            backoff_after: DEFAULT_BACKOFF_AFTER,
            open_after: DEFAULT_OPEN_AFTER,
            min_bad_samples: DEFAULT_MIN_BAD_SAMPLES,
            open_cooldown: DEFAULT_OPEN_COOLDOWN,
            close_after: DEFAULT_CLOSE_AFTER,
            min_clean_samples: DEFAULT_MIN_CLEAN_SAMPLES,
        }
    }
}

impl BreakerConfig {
    /// The effective open window, never shorter than the backoff window.
    fn open_window(&self) -> Duration {
        self.open_after.max(self.backoff_after)
    }

    /// The EWMA weight actually used, clamped to a sane range.
    fn alpha(&self) -> f64 {
        if self.ewma_alpha.is_finite() {
            self.ewma_alpha.clamp(0.0, 1.0)
        } else {
            DEFAULT_EWMA_ALPHA
        }
    }
}

/// A point-in-time snapshot of the breaker, for observability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BreakerStats {
    /// Current state.
    pub state: BreakerState,
    /// Number of state transitions since creation.
    pub transitions: u64,
    /// Current loss estimate in percent.
    pub loss_ema_pct: f64,
    /// Raw bad samples counted in the current bad window.
    pub bad_samples: u32,
    /// Clean samples counted in the current healthy window.
    pub good_samples: u32,
}

/// Sustained-loss circuit breaker, one per relay path.
#[derive(Debug, Clone)]
pub struct Breaker {
    cfg: BreakerConfig,
    state: BreakerState,
    loss_ema_pct: f64,
    seeded: bool,
    bad_since: Option<Instant>,
    bad_samples: u32,
    good_since: Option<Instant>,
    good_samples: u32,
    opened_at: Option<Instant>,
    transitions: u64,
}

impl Breaker {
    /// Create a breaker from `cfg`, starting [`BreakerState::Closed`].
    pub fn new(cfg: BreakerConfig) -> Self {
        Self {
            cfg,
            state: BreakerState::Closed,
            loss_ema_pct: 0.0,
            seeded: false,
            bad_since: None,
            bad_samples: 0,
            good_since: None,
            good_samples: 0,
            opened_at: None,
            transitions: 0,
        }
    }

    /// The configuration in force.
    pub fn config(&self) -> BreakerConfig {
        self.cfg
    }

    /// The current state.
    pub fn state(&self) -> BreakerState {
        self.state
    }

    /// Whether the breaker has no opinion (healthy).
    pub fn is_healthy(&self) -> bool {
        self.state == BreakerState::Closed
    }

    /// Whether the breaker is asking for a reduced send rate while it keeps
    /// using the relay.
    pub fn backs_off(&self) -> bool {
        self.state == BreakerState::BackingOff
    }

    /// Whether the caller must stop using this relay for new sessions and fall
    /// back to the direct path or another relay.
    pub fn should_fall_back(&self) -> bool {
        matches!(self.state, BreakerState::Open | BreakerState::HalfOpen)
    }

    /// Whether the relay is usable for new sessions. False from the moment the
    /// breaker opens, and only restored by a full close.
    pub fn accepts_new_sessions(&self) -> bool {
        matches!(self.state, BreakerState::Closed | BreakerState::BackingOff)
    }

    /// Number of state transitions since creation.
    pub fn transitions(&self) -> u64 {
        self.transitions
    }

    /// Current loss estimate in percent.
    pub fn loss_estimate_pct(&self) -> f64 {
        self.loss_ema_pct
    }

    /// Snapshot the breaker for metrics.
    pub fn stats(&self) -> BreakerStats {
        BreakerStats {
            state: self.state,
            transitions: self.transitions,
            loss_ema_pct: self.loss_ema_pct,
            bad_samples: self.bad_samples,
            good_samples: self.good_samples,
        }
    }

    /// Feed one loss observation using the current instant.
    pub fn on_sample(&mut self, loss_pct: f64) {
        self.on_sample_at(Instant::now(), loss_pct);
    }

    /// Feed one loss observation at `now`, advancing the state machine.
    pub fn on_sample_at(&mut self, now: Instant, loss_pct: f64) {
        if !self.cfg.enabled {
            return;
        }
        let Some(loss) = sanitize_loss(loss_pct) else {
            return;
        };
        self.update_ema(loss);

        if self.state == BreakerState::Open {
            if !self.cooldown_elapsed(now) {
                return;
            }
            self.set_state(now, BreakerState::HalfOpen, "open cooldown elapsed");
            self.reset_runs();
        }

        if self.loss_ema_pct >= self.cfg.bad_loss_pct {
            self.step_bad(now, loss);
        } else if self.loss_ema_pct <= self.cfg.clean_loss_pct {
            self.step_clean(now);
        } else {
            // Neutral band: hysteresis. End both windows so only a continuous
            // bad or clean run can advance the machine.
            self.reset_runs();
        }
    }

    /// Advance the open cooldown without a sample, so [`BreakerState::Open`]
    /// can still reach the recovery trial on a quiet path.
    pub fn tick(&mut self, now: Instant) {
        if !self.cfg.enabled || self.state != BreakerState::Open {
            return;
        }
        if self.cooldown_elapsed(now) {
            self.set_state(now, BreakerState::HalfOpen, "open cooldown elapsed");
            self.reset_runs();
        }
    }

    /// Reset to [`BreakerState::Closed`] and forget all windows and history.
    ///
    /// Call this when the underlying path changes, so a new relay is never
    /// judged on the previous one's loss.
    pub fn reset(&mut self) {
        self.state = BreakerState::Closed;
        self.loss_ema_pct = 0.0;
        self.seeded = false;
        self.reset_runs();
        self.opened_at = None;
    }

    fn update_ema(&mut self, loss: f64) {
        let alpha = self.cfg.alpha();
        if self.seeded {
            self.loss_ema_pct = alpha.mul_add(loss, (1.0 - alpha) * self.loss_ema_pct);
        } else {
            self.loss_ema_pct = loss;
            self.seeded = true;
        }
    }

    fn step_bad(&mut self, now: Instant, raw_loss: f64) {
        if self.bad_since.is_none() {
            self.bad_since = Some(now);
            self.bad_samples = 0;
        }
        if raw_loss >= self.cfg.bad_loss_pct {
            self.bad_samples = self.bad_samples.saturating_add(1);
        }
        // A bad estimate ends any healthy window.
        self.good_since = None;
        self.good_samples = 0;

        let since = self.bad_since.unwrap_or(now);
        let elapsed = now.saturating_duration_since(since);
        let enough = self.bad_samples >= self.cfg.min_bad_samples;
        match self.state {
            BreakerState::Closed if enough && elapsed >= self.cfg.backoff_after => {
                self.set_state(now, BreakerState::BackingOff, "sustained loss");
            }
            BreakerState::BackingOff if enough && elapsed >= self.cfg.open_window() => {
                self.set_state(now, BreakerState::Open, "path stayed bad");
            }
            BreakerState::HalfOpen if enough && elapsed >= self.cfg.backoff_after => {
                self.set_state(now, BreakerState::Open, "recovery trial failed");
            }
            _ => {}
        }
    }

    fn step_clean(&mut self, now: Instant) {
        self.bad_since = None;
        self.bad_samples = 0;
        if self.good_since.is_none() {
            self.good_since = Some(now);
            self.good_samples = 0;
        }
        self.good_samples = self.good_samples.saturating_add(1);

        if !matches!(
            self.state,
            BreakerState::BackingOff | BreakerState::HalfOpen
        ) {
            return;
        }
        let since = self.good_since.unwrap_or(now);
        let elapsed = now.saturating_duration_since(since);
        if elapsed >= self.cfg.close_after && self.good_samples >= self.cfg.min_clean_samples {
            self.set_state(now, BreakerState::Closed, "sustained clean");
        }
    }

    fn reset_runs(&mut self) {
        self.bad_since = None;
        self.bad_samples = 0;
        self.good_since = None;
        self.good_samples = 0;
    }

    fn cooldown_elapsed(&self, now: Instant) -> bool {
        match self.opened_at {
            Some(opened) => now.saturating_duration_since(opened) >= self.cfg.open_cooldown,
            None => true,
        }
    }

    fn set_state(&mut self, now: Instant, to: BreakerState, reason: &'static str) {
        let from = self.state;
        if from == to {
            return;
        }
        self.state = to;
        self.transitions = self.transitions.saturating_add(1);
        self.opened_at = (to == BreakerState::Open).then_some(now);
        self.log_transition(from, to, reason);
    }

    fn log_transition(&self, from: BreakerState, to: BreakerState, reason: &'static str) {
        match to {
            BreakerState::Open => tracing::warn!(
                from = from.as_str(),
                to = to.as_str(),
                loss_pct = self.loss_ema_pct,
                reason = reason,
                "relay circuit breaker opened; the caller should fall back"
            ),
            BreakerState::BackingOff => tracing::warn!(
                from = from.as_str(),
                to = to.as_str(),
                loss_pct = self.loss_ema_pct,
                reason = reason,
                "relay circuit breaker backing off sending"
            ),
            BreakerState::HalfOpen => tracing::info!(
                from = from.as_str(),
                to = to.as_str(),
                loss_pct = self.loss_ema_pct,
                reason = reason,
                "relay circuit breaker half-open; recovery trial"
            ),
            BreakerState::Closed => tracing::info!(
                from = from.as_str(),
                to = to.as_str(),
                loss_pct = self.loss_ema_pct,
                reason = reason,
                "relay circuit breaker closed; normal sending restored"
            ),
        }
    }
}

fn sanitize_loss(loss_pct: f64) -> Option<f64> {
    loss_pct.is_finite().then(|| loss_pct.clamp(0.0, 100.0))
}

impl Default for Breaker {
    fn default() -> Self {
        Self::new(BreakerConfig::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Small windows and fast decay so the tests are deterministic and quick.
    fn fast_config() -> BreakerConfig {
        BreakerConfig {
            enabled: true,
            ewma_alpha: 0.5,
            bad_loss_pct: 2.0,
            clean_loss_pct: 0.5,
            backoff_after: Duration::from_secs(2),
            open_after: Duration::from_secs(6),
            min_bad_samples: 4,
            open_cooldown: Duration::from_secs(2),
            close_after: Duration::from_secs(3),
            min_clean_samples: 4,
        }
    }

    const STEP: Duration = Duration::from_millis(500);

    fn drive_bad_until(
        b: &mut Breaker,
        start: Instant,
        mut stop: impl FnMut(BreakerState) -> bool,
    ) -> Instant {
        let mut t = start;
        loop {
            b.on_sample_at(t, 25.0);
            if stop(b.state()) {
                return t;
            }
            t += STEP;
            assert!(
                t.saturating_duration_since(start) < Duration::from_secs(120),
                "bad driver never reached the target state"
            );
        }
    }

    fn drive_clean_until(
        b: &mut Breaker,
        start: Instant,
        mut stop: impl FnMut(BreakerState) -> bool,
    ) -> Instant {
        let mut t = start;
        loop {
            b.on_sample_at(t, 0.0);
            if stop(b.state()) {
                return t;
            }
            t += STEP;
            assert!(
                t.saturating_duration_since(start) < Duration::from_secs(120),
                "clean driver never reached the target state"
            );
        }
    }

    fn back_off(b: &mut Breaker, start: Instant) -> Instant {
        drive_bad_until(b, start, |s| s == BreakerState::BackingOff)
    }

    fn open_breaker(b: &mut Breaker, start: Instant) -> Instant {
        let t = back_off(b, start);
        drive_bad_until(b, t, |s| s == BreakerState::Open)
    }

    #[test]
    fn engage_on_sustained_loss() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        assert_eq!(b.state(), BreakerState::Closed, "starts healthy");

        let t = back_off(&mut b, t0);
        assert_eq!(
            b.state(),
            BreakerState::BackingOff,
            "sustained loss must back off, went to {}",
            b.state().as_str()
        );

        let t_open = drive_bad_until(&mut b, t, |s| s == BreakerState::Open);
        assert_eq!(
            b.state(),
            BreakerState::Open,
            "loss that stays bad must open the breaker, went to {}",
            b.state().as_str()
        );
        assert!(
            t_open.saturating_duration_since(t0) >= Duration::from_secs(6),
            "opening must wait for the open window"
        );
    }

    #[test]
    fn stay_closed_on_a_single_burst() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();

        for i in 0..6 {
            b.on_sample_at(t0 + Duration::from_millis(20 * i), 25.0);
        }
        let mut t = t0 + Duration::from_millis(120);
        while t.saturating_duration_since(t0) < Duration::from_secs(3) {
            b.on_sample_at(t, 0.0);
            t += Duration::from_millis(20);
        }

        assert_eq!(
            b.state(),
            BreakerState::Closed,
            "a single burst must not trip the breaker"
        );
        assert_eq!(b.transitions(), 0, "a single burst must not transition");
    }

    #[test]
    fn recover_after_clean_window() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        let t = back_off(&mut b, t0);
        assert_eq!(b.state(), BreakerState::BackingOff);

        drive_clean_until(&mut b, t, |s| s == BreakerState::Closed);
        assert_eq!(
            b.state(),
            BreakerState::Closed,
            "a sustained clean window must restore normal sending, went to {}",
            b.state().as_str()
        );
        assert!(b.is_healthy());
    }

    #[test]
    fn fall_back_when_open() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        open_breaker(&mut b, t0);
        assert_eq!(b.state(), BreakerState::Open);

        assert!(b.should_fall_back(), "an open breaker must signal fallback");
        assert!(
            !b.accepts_new_sessions(),
            "an open breaker must refuse new sessions"
        );
    }

    #[test]
    fn healthy_path_never_trips() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        let mut t = t0;
        while t.saturating_duration_since(t0) < Duration::from_secs(30) {
            b.on_sample_at(t, 0.0);
            t += Duration::from_millis(50);
        }
        assert_eq!(
            b.state(),
            BreakerState::Closed,
            "a healthy path must stay healthy"
        );
        assert_eq!(b.transitions(), 0, "a healthy path must never transition");
        assert!(b.accepts_new_sessions());
        assert!(!b.should_fall_back());
        assert!(!b.backs_off());
    }

    #[test]
    fn neutral_loss_band_does_not_flap() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        let mut t = t0;
        while t.saturating_duration_since(t0) <= Duration::from_secs(30) {
            b.on_sample_at(t, 1.0);
            t += Duration::from_millis(100);
        }
        assert_eq!(b.state(), BreakerState::Closed);
        assert_eq!(b.transitions(), 0);
    }

    #[test]
    fn cooldown_opens_a_recovery_trial_then_closes() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        let t_open = open_breaker(&mut b, t0);
        assert_eq!(b.state(), BreakerState::Open);

        b.tick(t_open + Duration::from_secs(1));
        assert_eq!(
            b.state(),
            BreakerState::Open,
            "still open inside the cooldown"
        );

        b.tick(t_open + Duration::from_secs(2));
        assert_eq!(b.state(), BreakerState::HalfOpen);
        assert!(b.should_fall_back(), "the trial still refuses new sessions");

        drive_clean_until(&mut b, t_open + Duration::from_secs(3), |s| {
            s == BreakerState::Closed
        });
        assert_eq!(b.state(), BreakerState::Closed);
        assert!(
            b.accepts_new_sessions(),
            "a closed breaker accepts sessions again"
        );
    }

    #[test]
    fn half_open_retrips_on_sustained_loss() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        let t_open = open_breaker(&mut b, t0);
        assert_eq!(b.state(), BreakerState::Open);

        b.tick(t_open + Duration::from_secs(2));
        assert_eq!(b.state(), BreakerState::HalfOpen);

        b.on_sample_at(t_open + Duration::from_secs(3), 25.0);
        assert_eq!(
            b.state(),
            BreakerState::HalfOpen,
            "one bad sample is not enough"
        );
        drive_bad_until(&mut b, t_open + Duration::from_secs(4), |s| {
            s == BreakerState::Open
        });
        assert_eq!(b.state(), BreakerState::Open);
    }

    #[test]
    fn closed_never_transitions_on_a_single_bad_sample() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        b.on_sample_at(t0, 100.0);
        assert_eq!(b.state(), BreakerState::Closed);
        assert_eq!(b.transitions(), 0);
    }

    #[test]
    fn disabled_breaker_is_inert() {
        let cfg = BreakerConfig {
            enabled: false,
            ..fast_config()
        };
        let mut b = Breaker::new(cfg);
        let t0 = Instant::now();
        let mut t = t0;
        while t.saturating_duration_since(t0) < Duration::from_secs(60) {
            b.on_sample_at(t, 25.0);
            t += Duration::from_millis(100);
        }
        assert_eq!(b.state(), BreakerState::Closed);
        assert_eq!(b.transitions(), 0);
        assert!(b.accepts_new_sessions());
    }

    #[test]
    fn non_finite_samples_are_ignored() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        for i in 0..100 {
            b.on_sample_at(t0 + Duration::from_millis(i), f64::NAN);
            b.on_sample_at(t0 + Duration::from_millis(i), f64::INFINITY);
        }
        assert_eq!(b.state(), BreakerState::Closed);
        assert_eq!(
            b.loss_estimate_pct(),
            0.0,
            "no estimate from non-finite input"
        );
    }

    #[test]
    fn state_codes_rise_with_severity_and_names_are_stable() {
        assert_eq!(BreakerState::Closed.metric_code(), 0);
        assert_eq!(BreakerState::BackingOff.metric_code(), 1);
        assert_eq!(BreakerState::HalfOpen.metric_code(), 2);
        assert_eq!(BreakerState::Open.metric_code(), 3);
        assert_eq!(BreakerState::Closed.as_str(), "closed");
        assert_eq!(BreakerState::BackingOff.as_str(), "backing_off");
        assert_eq!(BreakerState::HalfOpen.as_str(), "half_open");
        assert_eq!(BreakerState::Open.as_str(), "open");
    }

    #[test]
    fn stats_track_transitions_and_state() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        back_off(&mut b, t0);
        let s = b.stats();
        assert_eq!(s.state, BreakerState::BackingOff);
        assert_eq!(s.transitions, 1);
        assert!(s.loss_ema_pct >= 2.0);

        let t_open = open_breaker(&mut b, t0);
        assert_eq!(b.stats().state, BreakerState::Open);
        assert_eq!(b.transitions(), 2);

        b.tick(t_open + Duration::from_secs(2));
        assert_eq!(b.transitions(), 3, "half-open is a transition");

        drive_clean_until(&mut b, t_open + Duration::from_secs(3), |s| {
            s == BreakerState::Closed
        });
        assert_eq!(b.stats().state, BreakerState::Closed);
        assert_eq!(b.transitions(), 4);
    }

    #[test]
    fn open_window_is_never_shorter_than_backoff() {
        let cfg = BreakerConfig {
            backoff_after: Duration::from_secs(10),
            open_after: Duration::from_secs(1),
            ..fast_config()
        };
        assert_eq!(cfg.open_window(), Duration::from_secs(10));
    }

    #[test]
    fn reset_returns_to_closed_and_forgets_history() {
        let mut b = Breaker::new(fast_config());
        let t0 = Instant::now();
        open_breaker(&mut b, t0);
        assert_eq!(b.state(), BreakerState::Open);

        b.reset();
        assert_eq!(b.state(), BreakerState::Closed);
        assert_eq!(b.loss_estimate_pct(), 0.0);
        assert!(b.accepts_new_sessions());
        assert!(!b.should_fall_back());
    }
}
