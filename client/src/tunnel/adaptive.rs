//! # Adaptive FEC and loss-gated duplication
//!
//! The tunnel's XOR FEC is useful only when the client-to-relay leg actually
//! loses packets. Sending parity on a clean link is pure overhead: the default
//! block size of 4 costs a permanent 25 percent. This controller measures loss
//! (and jitter) and decides two things:
//!
//! * whether to emit the block's parity packet at all, and
//! * whether packet duplication (the multipath spread) is currently earned.
//!
//! Both decisions are hysteretic and conservative:
//!
//! * a clean link emits **no parity** and **no duplication**,
//! * parity turns on only when the measured loss estimate reaches
//!   [`AdaptiveConfig::loss_on_pct`],
//! * duplication turns on only when loss reaches
//!   [`AdaptiveConfig::dup_loss_on_pct`] or jitter reaches
//!   [`AdaptiveConfig::dup_jitter_on_ms`],
//! * and each returns to the baseline after
//!   [`AdaptiveConfig::recover_samples`] / [`AdaptiveConfig::dup_recover_samples`]
//!   consecutive clean observations, so recovery is quick.
//!
//! The parity overhead is hard-bounded by [`AdaptiveConfig::max_overhead_pct`]:
//! the effective block size is never smaller than `ceil(100 / max_overhead_pct)`,
//! so the controller can never add more parity than the configured ceiling.
//!
//! The whole feature is opt-in. [`AdaptiveConfig::default`] has `enabled: false`
//! and callers that never enable it keep the previous fixed behaviour.
//!
//! ## Counter observability
//!
//! [`AdaptiveFec::parity_ratio`] reports the current parity-to-data ratio
//! (0.0 when suppressed, `1 / effective_k` when active) and
//! [`AdaptiveFec::is_duplicating`] reports the duplication state, alongside the
//! block counters [`AdaptiveFec::blocks_data`] / [`AdaptiveFec::blocks_parity`].

/// Smallest block size the codec supports (also the largest overhead, 50%).
const MIN_BLOCK_SIZE: u8 = 2;
/// Largest block size the codec supports (smallest overhead, 6.25%).
const MAX_BLOCK_SIZE: u8 = 16;
/// Default block size, matching the historical fixed FEC parameters.
pub const DEFAULT_K: u8 = 4;
/// Default overhead ceiling, matching the historical 1/4 parity ratio.
pub const DEFAULT_MAX_OVERHEAD_PCT: u32 = 25;
/// EWMA weight for a new loss/jitter sample (0.0..=1.0).
const EWMA_ALPHA: f64 = 0.3;

/// Tunables for [`AdaptiveFec`].
///
/// The defaults are inert: `enabled` is `false`, so a caller that does not opt
/// in sees the previous fixed `K=4` behaviour and no duplication gating.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveConfig {
    /// Master switch. When `false` the controller reports no opinion and the
    /// relay must keep its fixed behaviour.
    pub enabled: bool,
    /// Preferred block size K when parity is active. Overhead is `1/K`.
    pub k_size: u8,
    /// Loss percent at or above which parity is enabled.
    pub loss_on_pct: f64,
    /// Loss percent at or below which the controller may return to baseline.
    pub loss_off_pct: f64,
    /// Consecutive clean observations needed to suppress parity again.
    pub recover_samples: u32,
    /// Loss percent at or above which duplication is enabled.
    pub dup_loss_on_pct: f64,
    /// Jitter in milliseconds at or above which duplication is enabled.
    pub dup_jitter_on_ms: f32,
    /// Consecutive clean observations needed to stop duplicating.
    pub dup_recover_samples: u32,
    /// Hard ceiling on parity overhead, in percent. Bounds the effective K.
    pub max_overhead_pct: u32,
}

impl Default for AdaptiveConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            k_size: DEFAULT_K,
            loss_on_pct: 1.0,
            loss_off_pct: 0.25,
            recover_samples: 8,
            dup_loss_on_pct: 2.0,
            dup_jitter_on_ms: 10.0,
            dup_recover_samples: 8,
            max_overhead_pct: DEFAULT_MAX_OVERHEAD_PCT,
        }
    }
}

impl AdaptiveConfig {
    /// The smallest block size that keeps overhead at or below the ceiling.
    fn k_floor_for_ceiling(&self) -> u8 {
        if self.max_overhead_pct == 0 {
            return MAX_BLOCK_SIZE;
        }
        // overhead = 1/K <= pct/100  =>  K >= 100/pct.
        let needed = 100.0 / f64::from(self.max_overhead_pct);
        let needed = needed.ceil().max(1.0);
        let bounded = needed.min(f64::from(MAX_BLOCK_SIZE));
        // `bounded` is finite and in 1..=16, so the cast is exact.
        bounded as u8
    }

    /// The effective block size after clamping to the codec range and the
    /// overhead ceiling.
    pub fn effective_k(&self) -> u8 {
        self.k_size
            .clamp(MIN_BLOCK_SIZE, MAX_BLOCK_SIZE)
            .max(self.k_floor_for_ceiling())
            .min(MAX_BLOCK_SIZE)
    }

    /// The parity overhead, in percent, that the effective block size costs
    /// while parity is active.
    pub fn overhead_pct(&self) -> u32 {
        100 / u32::from(self.effective_k())
    }
}

/// Loss/jitter-driven FEC and duplication controller.
#[derive(Debug, Clone)]
pub struct AdaptiveFec {
    cfg: AdaptiveConfig,
    /// EWMA of the measured loss percentage.
    loss_ema_pct: f64,
    /// EWMA of the measured RTT jitter in milliseconds.
    jitter_ema_ms: f64,
    /// Whether a first sample has seeded the EWMA.
    seeded: bool,
    /// Consecutive clean observations while parity is active.
    clean_streak: u32,
    /// Consecutive clean observations while duplicating.
    dup_clean_streak: u32,
    /// Whether parity is currently emitted.
    parity_active: bool,
    /// Whether duplication is currently allowed.
    dup_active: bool,
    /// Blocks for which a data packet was encoded.
    blocks_data: u64,
    /// Blocks for which a parity packet was emitted.
    blocks_parity: u64,
}

impl AdaptiveFec {
    /// Create a controller from `cfg`.
    pub fn new(cfg: AdaptiveConfig) -> Self {
        Self {
            cfg,
            loss_ema_pct: 0.0,
            jitter_ema_ms: 0.0,
            seeded: false,
            clean_streak: 0,
            dup_clean_streak: 0,
            parity_active: false,
            dup_active: false,
            blocks_data: 0,
            blocks_parity: 0,
        }
    }

    /// The configuration in force.
    pub fn config(&self) -> AdaptiveConfig {
        self.cfg
    }

    /// Whether the adaptive mode is on.
    pub fn enabled(&self) -> bool {
        self.cfg.enabled
    }

    /// The effective block size after the overhead clamp.
    pub fn effective_k(&self) -> u8 {
        self.cfg.effective_k()
    }

    /// Whether parity is currently emitted.
    pub fn is_parity_active(&self) -> bool {
        self.parity_active
    }

    /// Whether duplication is currently allowed.
    pub fn is_duplicating(&self) -> bool {
        self.dup_active
    }

    /// The block size to use for parity, or `None` when parity is suppressed.
    ///
    /// Always `None` while the mode is disabled, so a caller that consults it
    /// unconditionally never adds overhead the user did not opt into.
    pub fn parity_k(&self) -> Option<u8> {
        (self.cfg.enabled && self.parity_active).then(|| self.cfg.effective_k())
    }

    /// Whether packet duplication is allowed right now.
    pub fn should_duplicate(&self) -> bool {
        self.cfg.enabled && self.dup_active
    }

    /// Current parity-to-data ratio: `0.0` when suppressed, `1/K` when active.
    pub fn parity_ratio(&self) -> f64 {
        if self.cfg.enabled && self.parity_active {
            1.0 / f64::from(self.cfg.effective_k())
        } else {
            0.0
        }
    }

    /// The maximum parity overhead this controller may ever add, in percent.
    pub fn overhead_bound_pct(&self) -> u32 {
        self.cfg.overhead_pct()
    }

    /// The current loss estimate, in percent.
    pub fn loss_estimate_pct(&self) -> f64 {
        self.loss_ema_pct
    }

    /// The current jitter estimate, in milliseconds.
    pub fn jitter_estimate_ms(&self) -> f32 {
        self.jitter_ema_ms as f32
    }

    /// Data blocks encoded.
    pub fn blocks_data(&self) -> u64 {
        self.blocks_data
    }

    /// Blocks whose parity was emitted.
    pub fn blocks_parity(&self) -> u64 {
        self.blocks_parity
    }

    /// Record a completed block and whether its parity was emitted.
    pub fn record_block(&mut self, parity_emitted: bool) {
        self.blocks_data = self.blocks_data.saturating_add(1);
        if parity_emitted {
            self.blocks_parity = self.blocks_parity.saturating_add(1);
        }
    }

    /// Feed one loss/jitter observation (percent loss, milliseconds jitter).
    pub fn observe_sample(&mut self, loss_pct: f64, jitter_ms: f32) {
        let loss = if loss_pct.is_finite() {
            loss_pct.clamp(0.0, 100.0)
        } else {
            0.0
        };
        let jitter = if jitter_ms.is_finite() {
            f64::from(jitter_ms.max(0.0))
        } else {
            0.0
        };
        if !self.seeded {
            self.loss_ema_pct = loss;
            self.jitter_ema_ms = jitter;
            self.seeded = true;
        } else {
            self.loss_ema_pct = EWMA_ALPHA * loss + (1.0 - EWMA_ALPHA) * self.loss_ema_pct;
            self.jitter_ema_ms = EWMA_ALPHA * jitter + (1.0 - EWMA_ALPHA) * self.jitter_ema_ms;
        }

        if !self.cfg.enabled {
            // Disabled is a hard reset, never a delayed transition.
            self.parity_active = false;
            self.dup_active = false;
            self.clean_streak = 0;
            self.dup_clean_streak = 0;
            return;
        }

        self.step_parity();
        self.step_duplication();
    }

    /// Parity enters on loss and leaves after a clean streak.
    fn step_parity(&mut self) {
        if self.loss_ema_pct >= self.cfg.loss_on_pct {
            self.parity_active = true;
            self.clean_streak = 0;
            return;
        }
        if !self.parity_active {
            return;
        }
        if self.loss_ema_pct <= self.cfg.loss_off_pct {
            self.clean_streak = self.clean_streak.saturating_add(1);
        } else {
            self.clean_streak = 0;
        }
        if self.clean_streak >= self.cfg.recover_samples {
            self.parity_active = false;
            self.clean_streak = 0;
        }
    }

    /// Duplication enters on loss or jitter and leaves after a clean streak.
    fn step_duplication(&mut self) {
        let loss_earned = self.loss_ema_pct >= self.cfg.dup_loss_on_pct;
        let jitter_earned = self.jitter_ema_ms >= f64::from(self.cfg.dup_jitter_on_ms);
        if loss_earned || jitter_earned {
            self.dup_active = true;
            self.dup_clean_streak = 0;
            return;
        }
        if !self.dup_active {
            return;
        }
        self.dup_clean_streak = self.dup_clean_streak.saturating_add(1);
        if self.dup_clean_streak >= self.cfg.dup_recover_samples {
            self.dup_active = false;
            self.dup_clean_streak = 0;
        }
    }
}

/// A point-in-time snapshot of the adaptive controller, for observability.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AdaptiveStats {
    /// Whether the adaptive mode is on.
    pub enabled: bool,
    /// Current parity-to-data ratio (`0.0` when suppressed).
    pub parity_ratio: f64,
    /// Whether duplication is currently allowed.
    pub duplicating: bool,
    /// Effective block size K.
    pub effective_k: u8,
    /// Current loss estimate in percent.
    pub loss_pct: f64,
    /// Current jitter estimate in milliseconds.
    pub jitter_ms: f32,
    /// Blocks encoded.
    pub blocks_data: u64,
    /// Blocks whose parity was emitted.
    pub blocks_parity: u64,
    /// Hard ceiling on parity overhead, in percent.
    pub overhead_bound_pct: u32,
}

impl AdaptiveFec {
    /// Snapshot the controller's current decisions and counters.
    pub fn stats(&self) -> AdaptiveStats {
        AdaptiveStats {
            enabled: self.cfg.enabled,
            parity_ratio: self.parity_ratio(),
            duplicating: self.should_duplicate(),
            effective_k: self.cfg.effective_k(),
            loss_pct: self.loss_ema_pct,
            jitter_ms: self.jitter_ema_ms as f32,
            blocks_data: self.blocks_data,
            blocks_parity: self.blocks_parity,
            overhead_bound_pct: self.cfg.overhead_pct(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled_config() -> AdaptiveConfig {
        AdaptiveConfig {
            enabled: true,
            ..AdaptiveConfig::default()
        }
    }

    #[test]
    fn default_is_disabled_and_reports_no_overhead() {
        let cfg = AdaptiveConfig::default();
        assert!(!cfg.enabled, "the adaptive mode must be opt-in");
        assert_eq!(cfg.k_size, DEFAULT_K);
        assert_eq!(cfg.max_overhead_pct, DEFAULT_MAX_OVERHEAD_PCT);
        assert_eq!(cfg.effective_k(), DEFAULT_K);
        assert_eq!(cfg.overhead_pct(), 25);

        let mut ctrl = AdaptiveFec::new(cfg);
        ctrl.observe_sample(50.0, 50.0);
        assert_eq!(
            ctrl.parity_k(),
            None,
            "a disabled controller must never add parity"
        );
        assert!(!ctrl.should_duplicate());
    }

    #[test]
    fn clean_link_emits_no_parity() {
        let mut ctrl = AdaptiveFec::new(enabled_config());
        for _ in 0..32 {
            ctrl.observe_sample(0.0, 0.0);
        }
        assert_eq!(ctrl.parity_k(), None, "a clean link must not send parity");
        assert_eq!(ctrl.parity_ratio(), 0.0);
        assert!(
            !ctrl.should_duplicate(),
            "a clean link must not duplicate packets"
        );
    }

    #[test]
    fn lossy_link_enables_parity() {
        let mut ctrl = AdaptiveFec::new(enabled_config());
        for _ in 0..5 {
            ctrl.observe_sample(5.0, 0.0);
        }
        assert_eq!(
            ctrl.parity_k(),
            Some(DEFAULT_K),
            "observed loss must enable parity"
        );
        assert!((ctrl.parity_ratio() - 0.25).abs() < 1e-9);
        assert!(ctrl.loss_estimate_pct() > 1.0);
    }

    #[test]
    fn duplication_needs_loss_or_jitter() {
        let mut ctrl = AdaptiveFec::new(enabled_config());
        for _ in 0..16 {
            ctrl.observe_sample(0.0, 0.0);
        }
        assert!(!ctrl.should_duplicate(), "clean link: no duplication");

        // Loss at the duplication threshold earns duplication...
        let mut lost = AdaptiveFec::new(enabled_config());
        for _ in 0..5 {
            lost.observe_sample(5.0, 0.0);
        }
        assert!(lost.should_duplicate(), "loss must enable duplication");

        // ...and so does jitter alone, with zero loss.
        let mut jittery = AdaptiveFec::new(enabled_config());
        for _ in 0..5 {
            jittery.observe_sample(0.0, 40.0);
        }
        assert!(
            jittery.should_duplicate(),
            "jitter must enable duplication even without loss"
        );
    }

    #[test]
    fn returns_to_baseline_after_loss_stops() {
        let mut ctrl = AdaptiveFec::new(enabled_config());
        for _ in 0..5 {
            ctrl.observe_sample(20.0, 0.0);
        }
        assert!(ctrl.parity_k().is_some());
        assert!(ctrl.should_duplicate());

        for _ in 0..64 {
            ctrl.observe_sample(0.0, 0.0);
        }
        assert_eq!(
            ctrl.parity_k(),
            None,
            "parity must be suppressed again once loss stops"
        );
        assert!(
            !ctrl.should_duplicate(),
            "duplication must stop again once loss stops"
        );
        assert_eq!(ctrl.parity_ratio(), 0.0);
    }

    #[test]
    fn below_threshold_loss_does_not_enable_parity() {
        let mut ctrl = AdaptiveFec::new(enabled_config());
        for _ in 0..16 {
            ctrl.observe_sample(0.2, 0.0);
        }
        assert_eq!(ctrl.parity_k(), None, "sub-threshold loss is not enough");
    }

    #[test]
    fn overhead_is_bounded_by_the_ceiling() {
        let cfg = AdaptiveConfig {
            enabled: true,
            k_size: 2,
            max_overhead_pct: 25,
            ..AdaptiveConfig::default()
        };
        assert_eq!(cfg.effective_k(), 4, "1/2 would breach a 25% ceiling");
        assert_eq!(cfg.overhead_pct(), 25);

        let loose = AdaptiveConfig {
            enabled: true,
            k_size: 2,
            max_overhead_pct: 50,
            ..AdaptiveConfig::default()
        };
        assert_eq!(loose.effective_k(), 2);
        assert_eq!(loose.overhead_pct(), 50);

        let mut ctrl = AdaptiveFec::new(cfg);
        ctrl.observe_sample(20.0, 0.0);
        assert_eq!(ctrl.parity_k(), Some(4));
        assert!(ctrl.parity_ratio() <= 0.25 + 1e-9);
    }

    #[test]
    fn counters_track_emitted_blocks() {
        let mut ctrl = AdaptiveFec::new(enabled_config());
        ctrl.record_block(false);
        ctrl.record_block(false);
        ctrl.record_block(true);
        assert_eq!(ctrl.blocks_data(), 3);
        assert_eq!(ctrl.blocks_parity(), 1);
    }
}
