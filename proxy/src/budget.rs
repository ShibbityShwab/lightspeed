//! # Per-Relay Egress Budget Guard
//!
//! Protects the zero-cost operating constraint: a relay runs on a metered plan,
//! so an unusual burst of growth or abuse could push egress past the allowance
//! and start costing money. The guard watches the relay's cumulative egress
//! (bytes forwarded to game servers plus bytes sent back to clients, the two
//! data-plane directions) and applies two optional controls:
//!
//! * a **soft threshold** that logs a warning and flips an observable metric
//!   once a chosen percentage of the budget is consumed, and
//! * a **hard threshold** that refuses NEW sessions once a chosen percentage is
//!   consumed.
//!
//! Live traffic is never dropped: the hard stop only rejects the creation of a
//! new session, so existing sessions keep relaying until they end naturally.
//! The whole guard is disabled by default, so an unconfigured relay behaves
//! exactly as it did before the feature existed.
//!
//! ## Accounting scope
//!
//! The data-plane counter is process-lifetime and resets on restart. The relay's
//! own measurement traffic (the server-leg ICMP prober) is charged through the
//! same guard with [`BudgetGuard::try_charge_probe`], so a probe can never push
//! egress past the hard threshold it is meant to protect; the charged probe
//! bytes are added to the data-plane total wherever the guard is evaluated
//! ([`BudgetGuard::metered_total`]). For month-accurate enforcement the operator
//! script `infra/scripts/egress-budget.sh` combines this process counter with the
//! reset-safe history collected by `collect-metrics.sh`.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};

use tracing::warn;

use crate::config::BudgetConfig;
use crate::metrics::ProxyMetrics;

/// The egress budget resolved for one evaluation.
///
/// `limit_bytes` and `used_bytes` describe whichever configured limit is
/// currently binding (the flat ceiling or the monthly allowance); a disabled
/// guard reports all fields zero and `enabled = false`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BudgetUsage {
    /// Whether a limit is configured and enforced.
    pub enabled: bool,
    /// The binding limit in bytes.
    pub limit_bytes: u64,
    /// Usage against the binding limit, in bytes.
    pub used_bytes: u64,
    /// Usage as a whole percentage of the binding limit.
    pub used_pct: u64,
    /// Whether the soft threshold has been reached.
    pub soft_exceeded: bool,
    /// Whether the hard threshold has been reached.
    pub hard_exceeded: bool,
}

/// Returned by [`BudgetGuard::check_new_session`] when the hard threshold blocks
/// session creation. Carries the numbers so the caller can record why.
#[derive(Debug, thiserror::Error)]
#[error("egress budget hard threshold reached: {used} of {limit} bytes ({pct}% of limit)")]
pub struct BudgetExceeded {
    /// Usage against the binding limit, in bytes.
    pub used: u64,
    /// The binding limit in bytes.
    pub limit: u64,
    /// Usage as a whole percentage of the binding limit.
    pub pct: u64,
}

/// Enforces the configured egress budget against a cumulative byte counter.
///
/// Holds only the one-shot transition latches and the monthly rebase anchor,
/// so it is cheap to check on the session-creation path and safe to share
/// behind an `Arc` across tasks.
pub struct BudgetGuard {
    config: BudgetConfig,
    soft_latched: AtomicBool,
    hard_latched: AtomicBool,
    month_key: AtomicU64,
    month_baseline: AtomicU64,
    /// Egress charged on behalf of the relay's own measurement traffic (the
    /// server-leg ICMP prober), counted against the same ceiling as relayed
    /// traffic. Kept separate from the data-plane counter so the relayed-bytes
    /// series keeps its meaning; [`Self::metered_total`] is the sum the guard
    /// evaluates.
    probe_egress_bytes: AtomicU64,
}

impl BudgetGuard {
    /// Build a guard from configuration. A disabled or limit-less configuration
    /// produces an inert guard.
    pub fn new(config: BudgetConfig) -> Self {
        Self {
            config,
            soft_latched: AtomicBool::new(false),
            hard_latched: AtomicBool::new(false),
            month_key: AtomicU64::new(0),
            month_baseline: AtomicU64::new(0),
            probe_egress_bytes: AtomicU64::new(0),
        }
    }

    /// The configuration this guard was built from.
    pub fn config(&self) -> &BudgetConfig {
        &self.config
    }

    /// Whether the guard enforces anything: enabled and at least one limit set.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && (self.config.max_bytes > 0 || self.config.monthly_bytes > 0)
    }

    /// Resolve the binding limit and current usage.
    ///
    /// `used_total` is the process-lifetime cumulative egress. `month_key` is a
    /// monotonically increasing calendar-month identifier (see
    /// [`current_month_key`]); pass `0` to skip monthly accounting. A change of
    /// `month_key` rebases the monthly allowance and re-arms both one-shot
    /// latches, so a new month can warn again.
    pub fn evaluate(&self, used_total: u64, month_key: u64) -> BudgetUsage {
        if !self.is_enabled() {
            return BudgetUsage::default();
        }
        let flat_limit = self.config.max_bytes;
        let monthly_limit = self.config.monthly_bytes;

        let mut monthly_used = 0u64;
        if monthly_limit > 0 && month_key != 0 {
            let previous = self.month_key.swap(month_key, Ordering::AcqRel);
            if previous != month_key {
                self.month_baseline.store(used_total, Ordering::Release);
                self.soft_latched.store(false, Ordering::Release);
                self.hard_latched.store(false, Ordering::Release);
            }
            monthly_used = used_total.saturating_sub(self.month_baseline.load(Ordering::Acquire));
        }

        let use_monthly = monthly_limit > 0
            && (flat_limit == 0
                || u128::from(monthly_used) * u128::from(flat_limit)
                    > u128::from(used_total) * u128::from(monthly_limit));
        let (limit_bytes, used_bytes) = if use_monthly {
            (monthly_limit, monthly_used)
        } else {
            (flat_limit, used_total)
        };
        let used_pct = pct_of(used_bytes, limit_bytes);

        let soft_exceeded = self.config.soft_threshold_pct > 0
            && used_pct >= u64::from(self.config.soft_threshold_pct);
        let hard_exceeded = self.config.hard_threshold_pct > 0
            && used_pct >= u64::from(self.config.hard_threshold_pct);

        BudgetUsage {
            enabled: true,
            limit_bytes,
            used_bytes,
            used_pct,
            soft_exceeded,
            hard_exceeded,
        }
    }

    /// Reject session creation when the hard threshold is reached.
    ///
    /// Existing sessions are unaffected: callers only invoke this on the
    /// new-session path.
    pub fn check_new_session(&self, used_total: u64, month_key: u64) -> Result<(), BudgetExceeded> {
        let usage = self.evaluate(used_total, month_key);
        if usage.hard_exceeded {
            Err(BudgetExceeded {
                used: usage.used_bytes,
                limit: usage.limit_bytes,
                pct: usage.used_pct,
            })
        } else {
            Ok(())
        }
    }

    /// Egress charged through this guard on behalf of the relay's own
    /// measurement traffic (the server-leg prober).
    pub fn probe_egress_bytes(&self) -> u64 {
        self.probe_egress_bytes.load(Ordering::Relaxed)
    }

    /// The cumulative egress the guard accounts against: the caller's
    /// data-plane total plus the probe egress this guard has charged.
    ///
    /// Every budget evaluation must pass this sum rather than the data-plane
    /// counter alone, or probe traffic would be invisible to the ceiling.
    pub fn metered_total(&self, data_used: u64) -> u64 {
        data_used.saturating_add(self.probe_egress_bytes())
    }

    /// Charge `bytes` of prober egress before the probe is sent.
    ///
    /// `data_used` is the process-lifetime data-plane egress the guard accounts
    /// against; the probe egress already charged through this guard is added to
    /// it. Returns `false` when the charge would reach the hard threshold, in
    /// which case nothing is charged and the caller must not send. On success
    /// the bytes are recorded in [`Self::probe_egress_bytes`].
    ///
    /// The compare-exchange loop is what makes this safe to call from more than
    /// one prober: two chargers cannot both spend the same headroom, so a tight
    /// budget saturates just below the ceiling instead of over-sending. An
    /// inert guard charges unconditionally (metering still happens, nothing is
    /// refused), exactly like [`Self::check_new_session`].
    pub fn try_charge_probe(&self, data_used: u64, bytes: u64, month_key: u64) -> bool {
        if !self.is_enabled() {
            self.probe_egress_bytes.fetch_add(bytes, Ordering::Relaxed);
            return true;
        }
        loop {
            let charged = self.probe_egress_bytes.load(Ordering::Acquire);
            let prospective = data_used.saturating_add(charged).saturating_add(bytes);
            if self.evaluate(prospective, month_key).hard_exceeded {
                return false;
            }
            if self
                .probe_egress_bytes
                .compare_exchange_weak(
                    charged,
                    charged.saturating_add(bytes),
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                return true;
            }
        }
    }

    /// Evaluate the budget, publish it to `metrics`, and log each threshold
    /// crossing exactly once.
    ///
    /// Safe to call from a periodic task: the soft and hard logs fire only on
    /// the transition, while the gauges track the current usage every call.
    pub fn observe(&self, used_total: u64, month_key: u64, metrics: &ProxyMetrics) -> BudgetUsage {
        let usage = self.evaluate(used_total, month_key);
        if !usage.enabled {
            metrics.record_egress_budget(0, 0, false, false, false);
            return usage;
        }

        let soft_first = usage.soft_exceeded && !self.soft_latched.swap(true, Ordering::AcqRel);
        let hard_first = usage.hard_exceeded && !self.hard_latched.swap(true, Ordering::AcqRel);
        metrics.record_egress_budget(
            usage.limit_bytes,
            usage.used_bytes,
            usage.soft_exceeded,
            usage.hard_exceeded,
            soft_first,
        );

        if soft_first {
            warn!(
                limit_bytes = usage.limit_bytes,
                used_bytes = usage.used_bytes,
                used_pct = usage.used_pct,
                "Egress budget soft threshold reached"
            );
        }
        if hard_first {
            warn!(
                limit_bytes = usage.limit_bytes,
                used_bytes = usage.used_bytes,
                used_pct = usage.used_pct,
                "Egress budget hard threshold reached; refusing new sessions, existing sessions continue"
            );
        }
        usage
    }
}

/// A calendar-month identifier: `year * 12 + month` (month 1..=12).
///
/// `0` is reserved for "no monthly accounting" and never returned.
pub fn current_month_key() -> u64 {
    month_key(time::OffsetDateTime::now_utc())
}

/// The month key of `at` in UTC.
///
/// Split out from [`current_month_key`] so the year/month arithmetic is
/// unit-tested at fixed instants. UTC is deliberate: the egress budget rolls
/// over at the UTC month boundary (what the previous UTC clock did), so neither
/// the local zone nor a DST transition may move the reset.
fn month_key(at: time::OffsetDateTime) -> u64 {
    let month = u8::from(at.month());
    (at.year() as u64) * 12 + u64::from(month)
}

fn pct_of(used: u64, limit: u64) -> u64 {
    if limit == 0 {
        0
    } else {
        (u128::from(used) * 100 / u128::from(limit)) as u64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn enabled(max_bytes: u64, soft: u8, hard: u8) -> BudgetGuard {
        BudgetGuard::new(BudgetConfig {
            enabled: true,
            max_bytes,
            monthly_bytes: 0,
            soft_threshold_pct: soft,
            hard_threshold_pct: hard,
        })
    }

    #[test]
    fn soft_threshold_emits_metric_once() {
        let guard = enabled(1000, 80, 100);
        let metrics = ProxyMetrics::new();

        let below = guard.observe(799, 0, &metrics);
        assert!(below.enabled);
        assert!(!below.soft_exceeded);
        assert_eq!(below.used_pct, 79);
        assert_eq!(metrics.egress_budget_soft_events.load(Ordering::Relaxed), 0);

        let at = guard.observe(800, 0, &metrics);
        assert!(at.soft_exceeded, "80% of 1000 must trip the soft threshold");
        assert_eq!(at.used_pct, 80);
        assert_eq!(metrics.egress_budget_soft_events.load(Ordering::Relaxed), 1);
        assert_eq!(
            metrics.egress_budget_soft_exceeded.load(Ordering::Relaxed),
            1
        );

        let again = guard.observe(950, 0, &metrics);
        assert!(again.soft_exceeded);
        assert_eq!(
            metrics.egress_budget_soft_events.load(Ordering::Relaxed),
            1,
            "the soft transition is one-shot"
        );
    }

    #[test]
    fn hard_threshold_refuses_at_limit() {
        let guard = enabled(1000, 80, 100);
        assert!(guard.check_new_session(999, 0).is_ok());
        let err = guard.check_new_session(1000, 0).unwrap_err();
        assert_eq!(err.limit, 1000);
        assert_eq!(err.used, 1000);
        assert_eq!(err.pct, 100);
    }

    #[test]
    fn probe_charge_is_allowed_and_recorded_while_under_the_limit() {
        let guard = enabled(10_000, 80, 100);

        let first = guard.try_charge_probe(0, 64, 0);
        let second = guard.try_charge_probe(0, 64, 0);
        assert!(first && second, "64-byte probes are far under 10 kB");
        assert_eq!(guard.probe_egress_bytes(), 128);
        // The data-plane total the guard accounts against includes the charges.
        assert_eq!(guard.metered_total(1_000), 1_128);
    }

    #[test]
    fn probe_charge_is_refused_at_the_hard_threshold_and_stays_put() {
        let guard = enabled(1_000, 80, 100);

        // 960 used + 64 would be 102 %: refused, nothing charged.
        assert!(!guard.try_charge_probe(960, 64, 0));
        assert_eq!(guard.probe_egress_bytes(), 0);
        // 896 + 64 is 96 %: allowed.
        assert!(guard.try_charge_probe(896, 64, 0));
        assert_eq!(guard.probe_egress_bytes(), 64);
        // The charged bytes are part of the next evaluation: 896 + 64 + 64.
        assert!(!guard.try_charge_probe(896, 64, 0));
        assert_eq!(guard.probe_egress_bytes(), 64);
    }

    #[test]
    fn a_tight_probe_budget_saturates_instead_of_oversending() {
        let guard = enabled(256, 0, 100);
        // 64-byte charges against 256 bytes: 64/128/192 fit (75 %), the fourth
        // would be exactly 100 % and is refused.
        for expected in 1..=3u64 {
            assert!(guard.try_charge_probe(0, 64, 0));
            assert_eq!(guard.probe_egress_bytes(), expected * 64);
        }
        for _ in 0..10 {
            assert!(
                !guard.try_charge_probe(0, 64, 0),
                "a saturated budget must keep refusing, never over-send"
            );
        }
        assert_eq!(guard.probe_egress_bytes(), 192);
        assert!(
            guard.probe_egress_bytes() < 256,
            "saturates below the ceiling"
        );
    }

    #[test]
    fn an_inert_guard_still_counts_probe_egress_without_refusing() {
        let guard = BudgetGuard::new(BudgetConfig::default());
        assert!(guard.try_charge_probe(u64::MAX, 64, 0));
        assert_eq!(guard.probe_egress_bytes(), 64);
    }

    #[test]
    fn disabled_default_is_inert() {
        let guard = BudgetGuard::new(BudgetConfig::default());
        assert!(!guard.is_enabled());

        let usage = guard.evaluate(u64::MAX, 0);
        assert!(!usage.enabled);
        assert!(!usage.soft_exceeded);
        assert!(!usage.hard_exceeded);
        assert!(guard.check_new_session(u64::MAX, 0).is_ok());

        let metrics = ProxyMetrics::new();
        guard.observe(u64::MAX, 0, &metrics);
        assert_eq!(metrics.egress_budget_soft_events.load(Ordering::Relaxed), 0);
        assert_eq!(
            metrics.egress_budget_hard_exceeded.load(Ordering::Relaxed),
            0
        );
    }

    #[test]
    fn enabled_without_a_limit_is_inert() {
        let guard = BudgetGuard::new(BudgetConfig {
            enabled: true,
            ..BudgetConfig::default()
        });
        assert!(!guard.is_enabled());
        assert!(guard.check_new_session(u64::MAX, 0).is_ok());
    }

    #[test]
    fn monthly_allowance_rebases_on_month_change() {
        let guard = BudgetGuard::new(BudgetConfig {
            enabled: true,
            max_bytes: 0,
            monthly_bytes: 1000,
            soft_threshold_pct: 80,
            hard_threshold_pct: 100,
        });

        // Anchor the current month at zero, then consume 900 of the allowance.
        guard.evaluate(0, 100);
        let first = guard.evaluate(900, 100);
        assert!(first.soft_exceeded);
        assert_eq!(first.limit_bytes, 1000);
        assert_eq!(first.used_bytes, 900);

        let rolled = guard.evaluate(1500, 101);
        assert!(!rolled.soft_exceeded, "a new month resets the allowance");
        assert_eq!(rolled.used_bytes, 0);

        let used = guard.evaluate(2300, 101);
        assert_eq!(used.used_bytes, 800);
        assert!(used.soft_exceeded);
    }

    #[test]
    fn month_key_is_year_times_twelve_plus_utc_month() {
        use time::macros::datetime;

        // The key is `year * 12 + month`; 1-based, so 1970-01 maps to 1970*12+1
        // (0 stays reserved for "no monthly accounting").
        assert_eq!(month_key(datetime!(1970-01-01 0:00 UTC)), 1970 * 12 + 1);
        assert_eq!(
            month_key(datetime!(2026-11-18 14:39:00 UTC)),
            2026 * 12 + 11
        );

        // UTC decides the month: 2026-12-01 00:00 UTC is December even in a
        // negative-offset local zone, and the last second of a month keeps its
        // key right up to the boundary.
        assert_eq!(month_key(datetime!(2026-12-01 0:00 UTC)), 2026 * 12 + 12);
        assert_eq!(
            month_key(datetime!(2026-11-30 23:59:59 UTC)),
            2026 * 12 + 11
        );

        // Consecutive months are consecutive keys across the year boundary, so
        // a rollover compares unequal and the allowance rebases exactly once.
        let november = month_key(datetime!(2026-11-01 0:00 UTC));
        let december = month_key(datetime!(2026-12-01 0:00 UTC));
        let january = month_key(datetime!(2027-01-01 0:00 UTC));
        assert_eq!(december - november, 1);
        assert_eq!(january - december, 1);

        // The live clock stays in the same key space (never the reserved 0).
        assert_ne!(current_month_key(), 0);
    }
}
