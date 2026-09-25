//! # Destination-aware path estimation
//!
//! The nearest-proxy selector ranks relays by the client-to-relay round trip
//! alone. That leg is cheap to probe for every candidate, but it is only half
//! the path: the relay-to-game-server leg decides whether tunnelling actually
//! beats the direct route. This module estimates that second leg from
//! measurements the client already takes:
//!
//! * the client-to-relay round trip (the keepalive probe, stored per relay in
//!   [`crate::route::ProxyNode::latency_us`]), and
//! * the end-to-end tunnelled round trip (the game response arriving back
//!   through the relay).
//!
//! Their difference is an estimate of the relay-to-destination round trip:
//!
//! ```text
//! dest_leg ~= p50(tunnelled) - p50(client_leg)
//! ```
//!
//! The two legs are reduced independently with medians because the client
//! cannot pair every tunnelled sample with the exact probe that preceded it.
//! Medians are robust to the long tail a busy relay shows, and the estimator
//! refuses to publish anything until both legs have [`MIN_DEST_SAMPLES`]
//! samples. A non-positive difference is discarded rather than clamped: it
//! means the tunnelled round trip did not exceed the client-to-relay round
//! trip, which is a clock or rate-limit artefact, not a destination leg.
//!
//! The estimate is deliberately coarse and carries its own uncertainty
//! ([`DestinationEstimate::uncertainty_us`] is half the tunnelled
//! interquartile range). Consumers must treat it as a prior, not a fact.

use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

/// Minimum samples required on *each* leg before an estimate is published.
pub const MIN_DEST_SAMPLES: usize = 8;
/// Rolling window capacity per leg.
pub const DEST_RING_CAPACITY: usize = 64;
/// Smallest relay-to-destination round trip in microseconds treated as real.
/// Anything below this is an inverted or clamped measurement.
pub const MIN_PLAUSIBLE_DEST_US: u64 = 1;
/// Largest round trip in microseconds treated as real. A sample above this is
/// almost certainly a timeout or a clock artefact, not a WAN round trip.
pub const MAX_PLAUSIBLE_RTT_US: u64 = 5_000_000;
/// Minimum interval between always-on destination samples for one server, so
/// the routing estimator does not mirror every game packet.
pub const DEST_SAMPLE_INTERVAL: Duration = Duration::from_millis(250);

/// A destination-leg estimate together with its uncertainty.
///
/// `confidence` is a heuristic in `0.0..=1.0` derived from the sample count
/// and the tunnelled path's relative spread; it is advisory only.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DestinationEstimate {
    /// Estimated relay-to-destination round trip in microseconds.
    pub dest_leg_us: u64,
    /// Samples contributing to the estimate (the smaller of the two legs).
    pub samples: usize,
    /// Half the tunnelled interquartile range, in microseconds. The estimate
    /// inherits the tunnelled path's dispersion.
    pub uncertainty_us: u64,
    /// Advisory confidence in `0.0..=1.0`.
    pub confidence: f64,
}

#[derive(Default)]
struct LegSamples {
    client_leg_us: VecDeque<u64>,
    tunnelled_us: VecDeque<u64>,
}

impl LegSamples {
    fn is_ready(&self) -> bool {
        self.client_leg_us.len() >= MIN_DEST_SAMPLES && self.tunnelled_us.len() >= MIN_DEST_SAMPLES
    }

    fn sample_count(&self) -> usize {
        self.client_leg_us.len().min(self.tunnelled_us.len())
    }
}

/// Rolling per-relay estimator of the relay-to-destination leg.
#[derive(Default)]
pub struct RelayDestinationEstimator {
    relays: StdMutex<HashMap<SocketAddrV4, LegSamples>>,
}

impl RelayDestinationEstimator {
    /// Create an empty estimator.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SocketAddrV4, LegSamples>> {
        self.relays.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Record one observed client-to-relay round trip for `relay`.
    ///
    /// Zero, over-long, and clamped samples are dropped.
    pub fn observe_client_leg(&self, relay: SocketAddrV4, rtt_us: u64) {
        if !is_plausible(rtt_us) {
            return;
        }
        let mut relays = self.lock();
        let leg = relays.entry(relay).or_default();
        push_bounded(&mut leg.client_leg_us, rtt_us);
    }

    /// Record one observed end-to-end tunnelled round trip for `relay`.
    ///
    /// Zero, over-long, and clamped samples are dropped.
    pub fn observe_tunnelled(&self, relay: SocketAddrV4, rtt_us: u64) {
        if !is_plausible(rtt_us) {
            return;
        }
        let mut relays = self.lock();
        let leg = relays.entry(relay).or_default();
        push_bounded(&mut leg.tunnelled_us, rtt_us);
    }

    /// The current estimate for `relay`, or `None` while either leg is short
    /// of [`MIN_DEST_SAMPLES`] or the implied destination leg is implausible.
    pub fn estimate(&self, relay: SocketAddrV4) -> Option<DestinationEstimate> {
        let relays = self.lock();
        let leg = relays.get(&relay)?;
        estimate_from(leg)
    }

    /// A pooled destination-leg estimate across every relay that has one.
    ///
    /// Used as a prior for a candidate whose own destination leg is not yet
    /// measured; `None` until at least one relay has an estimate.
    pub fn pooled_estimate(&self) -> Option<DestinationEstimate> {
        let relays = self.lock();
        let mut dests: Vec<u64> = Vec::new();
        let mut samples = 0usize;
        let mut uncertainty_sum = 0u64;
        let mut confidence_sum = 0.0f64;
        for leg in relays.values() {
            if let Some(est) = estimate_from(leg) {
                dests.push(est.dest_leg_us);
                samples += est.samples;
                uncertainty_sum = uncertainty_sum.saturating_add(est.uncertainty_us);
                confidence_sum += est.confidence;
            }
        }
        if dests.is_empty() {
            return None;
        }
        let n = dests.len() as f64;
        dests.sort_unstable();
        Some(DestinationEstimate {
            dest_leg_us: median_sorted(&dests),
            samples,
            uncertainty_us: (uncertainty_sum as f64 / n).round() as u64,
            confidence: confidence_sum / n,
        })
    }

    /// Drop every sample. Test-only convenience, but harmless in production.
    pub fn clear(&self) {
        self.lock().clear();
    }
}

fn estimate_from(leg: &LegSamples) -> Option<DestinationEstimate> {
    if !leg.is_ready() {
        return None;
    }
    let mut client = leg.client_leg_us.iter().copied().collect::<Vec<_>>();
    let mut tunnelled = leg.tunnelled_us.iter().copied().collect::<Vec<_>>();
    client.sort_unstable();
    tunnelled.sort_unstable();

    let client_median = median_sorted(&client);
    let tunnelled_median = median_sorted(&tunnelled);
    let dest_leg = tunnelled_median.checked_sub(client_median)?;
    if dest_leg < MIN_PLAUSIBLE_DEST_US {
        return None;
    }

    let q1 = percentile_sorted(&tunnelled, 25.0);
    let q3 = percentile_sorted(&tunnelled, 75.0);
    let uncertainty_us = (q3 - q1) / 2;
    let relative_spread = if tunnelled_median == 0 {
        1.0
    } else {
        (uncertainty_us as f64 / tunnelled_median as f64).clamp(0.0, 1.0)
    };
    let count_factor = (leg.sample_count() as f64 / (MIN_DEST_SAMPLES as f64 * 4.0)).min(1.0);
    let confidence = (count_factor * (1.0 - relative_spread)).clamp(0.0, 1.0);

    Some(DestinationEstimate {
        dest_leg_us: dest_leg,
        samples: leg.sample_count(),
        uncertainty_us,
        confidence,
    })
}

fn is_plausible(rtt_us: u64) -> bool {
    rtt_us > 0 && rtt_us <= MAX_PLAUSIBLE_RTT_US
}

fn push_bounded(ring: &mut VecDeque<u64>, value: u64) {
    if ring.len() >= DEST_RING_CAPACITY {
        ring.pop_front();
    }
    ring.push_back(value);
}

fn median_sorted(sorted: &[u64]) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    sorted[sorted.len() / 2]
}

/// Nearest-rank percentile over an ascending slice.
fn percentile_sorted(sorted: &[u64], pct: f64) -> u64 {
    if sorted.is_empty() {
        return 0;
    }
    let rank = ((pct / 100.0) * sorted.len() as f64).ceil() as usize;
    let idx = rank.saturating_sub(1).min(sorted.len() - 1);
    sorted[idx]
}

// ── Process-wide estimator + always-on sampler ──────────────────────────────

static GLOBAL: OnceLock<Arc<RelayDestinationEstimator>> = OnceLock::new();

/// Install the process-wide estimator. Idempotent: the first call wins.
pub fn install_global(estimator: Arc<RelayDestinationEstimator>) {
    let _ = GLOBAL.set(estimator);
}

/// The installed estimator, or `None` when routing has not installed one.
pub fn global() -> Option<&'static Arc<RelayDestinationEstimator>> {
    GLOBAL.get()
}

/// Install the process-wide estimator on first use and return it.
pub fn ensure_global() -> Arc<RelayDestinationEstimator> {
    GLOBAL
        .get_or_init(|| Arc::new(RelayDestinationEstimator::new()))
        .clone()
}

/// Record a client-to-relay sample into the global estimator, if installed.
pub fn observe_client_leg(relay: SocketAddrV4, rtt_us: u64) {
    if let Some(estimator) = global() {
        estimator.observe_client_leg(relay, rtt_us);
    }
}

/// Record an end-to-end tunnelled sample into the global estimator, if
/// installed.
pub fn observe_tunnelled(relay: SocketAddrV4, rtt_us: u64) {
    if let Some(estimator) = global() {
        estimator.observe_tunnelled(relay, rtt_us);
    }
}

#[derive(Clone, Copy)]
struct PendingSample {
    sent_at: Instant,
    relay: Option<SocketAddrV4>,
}

#[derive(Default)]
struct Sampler {
    pending: HashMap<Ipv4Addr, PendingSample>,
    last_sample: HashMap<Ipv4Addr, Instant>,
}

static SAMPLER: OnceLock<StdMutex<Sampler>> = OnceLock::new();

fn sampler() -> &'static StdMutex<Sampler> {
    SAMPLER.get_or_init(|| StdMutex::new(Sampler::default()))
}

/// Always-on helper: note that a game packet for `server` was tunnelled.
///
/// Records the relay in use at send time. Rate-limited per server so the
/// estimator samples a small fraction of gameplay rather than every packet.
pub fn note_outbound(server: Ipv4Addr) {
    let now = Instant::now();
    let relay = crate::session::current_proxy();
    let mut sampler = sampler().lock().unwrap_or_else(PoisonError::into_inner);
    match sampler.last_sample.get(&server) {
        Some(last) if now.saturating_duration_since(*last) < DEST_SAMPLE_INTERVAL => return,
        _ => sampler.last_sample.insert(server, now),
    };
    sampler.pending.insert(
        server,
        PendingSample {
            sent_at: now,
            relay,
        },
    );
}

/// Always-on helper: note that a response for `server` arrived through the
/// tunnel. Pairs with the most recent [`note_outbound`] and feeds the global
/// estimator. A response with no pending outbound, or one from a relay we no
/// longer know, is discarded.
pub fn note_inbound(server: Ipv4Addr) {
    let now = Instant::now();
    let Some(sample) = sampler()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .pending
        .remove(&server)
    else {
        return;
    };
    let Some(relay) = sample.relay else {
        return;
    };
    let rtt_us = now.saturating_duration_since(sample.sent_at).as_micros() as u64;
    observe_tunnelled(relay, rtt_us);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn relay(a: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, a), 4434)
    }

    fn feed(
        estimator: &RelayDestinationEstimator,
        r: SocketAddrV4,
        client: u64,
        tunnelled: u64,
        n: usize,
    ) {
        for _ in 0..n {
            estimator.observe_client_leg(r, client);
            estimator.observe_tunnelled(r, tunnelled);
        }
    }

    #[test]
    fn estimate_requires_minimum_samples_on_both_legs() {
        let estimator = RelayDestinationEstimator::new();
        // One sample short of the threshold on both legs.
        feed(&estimator, relay(1), 10_000, 50_000, MIN_DEST_SAMPLES - 1);
        assert_eq!(estimator.estimate(relay(1)), None);

        feed(&estimator, relay(1), 10_000, 50_000, 1);
        assert!(estimator.estimate(relay(1)).is_some());
    }

    #[test]
    fn dest_leg_is_tunnelled_median_minus_client_median() {
        let estimator = RelayDestinationEstimator::new();
        feed(&estimator, relay(1), 10_000, 60_000, MIN_DEST_SAMPLES);
        let est = estimator.estimate(relay(1)).expect("estimate");
        assert_eq!(est.dest_leg_us, 50_000);
        assert_eq!(est.samples, MIN_DEST_SAMPLES);
    }

    #[test]
    fn zero_and_clamped_samples_are_ignored() {
        let estimator = RelayDestinationEstimator::new();
        for _ in 0..MIN_DEST_SAMPLES {
            estimator.observe_client_leg(relay(1), 0);
            estimator.observe_tunnelled(relay(1), u64::MAX);
        }
        assert_eq!(estimator.estimate(relay(1)), None);
    }

    #[test]
    fn inverted_measurement_is_discarded_not_clamped() {
        let estimator = RelayDestinationEstimator::new();
        // Tunnelled round trip below the client leg is impossible; the
        // estimator must publish nothing rather than a zero destination leg.
        feed(&estimator, relay(1), 50_000, 40_000, MIN_DEST_SAMPLES);
        assert_eq!(estimator.estimate(relay(1)), None);
    }

    #[test]
    fn median_resists_a_single_outlier() {
        let estimator = RelayDestinationEstimator::new();
        for _ in 0..MIN_DEST_SAMPLES {
            estimator.observe_client_leg(relay(1), 10_000);
            estimator.observe_tunnelled(relay(1), 60_000);
        }
        // One late spike must not move the median.
        estimator.observe_tunnelled(relay(1), 5_000_000);
        let est = estimator.estimate(relay(1)).expect("estimate");
        assert_eq!(est.dest_leg_us, 50_000);
    }

    #[test]
    fn uncertainty_tracks_tunnelled_spread() {
        let estimator = RelayDestinationEstimator::new();
        for _ in 0..MIN_DEST_SAMPLES {
            estimator.observe_client_leg(relay(1), 10_000);
        }
        // Half the samples at 50ms, half at 70ms: IQR is 20ms, half is 10ms.
        for _ in 0..MIN_DEST_SAMPLES / 2 {
            estimator.observe_tunnelled(relay(1), 60_000);
        }
        for _ in 0..MIN_DEST_SAMPLES / 2 {
            estimator.observe_tunnelled(relay(1), 80_000);
        }
        let est = estimator.estimate(relay(1)).expect("estimate");
        assert!(est.uncertainty_us > 0, "spread must be reported");
        assert!(est.confidence < 1.0);
    }

    #[test]
    fn tighter_spread_has_higher_confidence() {
        let steady = RelayDestinationEstimator::new();
        feed(&steady, relay(1), 10_000, 60_000, MIN_DEST_SAMPLES * 4);
        let noisy = RelayDestinationEstimator::new();
        for _ in 0..MIN_DEST_SAMPLES * 4 {
            noisy.observe_client_leg(relay(1), 10_000);
        }
        for i in 0..MIN_DEST_SAMPLES * 4 {
            noisy.observe_tunnelled(relay(1), 40_000 + (i as u64 % 2) * 40_000);
        }
        let s = steady.estimate(relay(1)).unwrap();
        let n = noisy.estimate(relay(1)).unwrap();
        assert!(s.confidence > n.confidence);
    }

    #[test]
    fn pooled_estimate_is_defined_once_any_relay_is_ready() {
        let estimator = RelayDestinationEstimator::new();
        assert_eq!(estimator.pooled_estimate(), None);
        feed(&estimator, relay(1), 10_000, 60_000, MIN_DEST_SAMPLES);
        let pooled = estimator.pooled_estimate().expect("pooled");
        assert_eq!(pooled.dest_leg_us, 50_000);
    }

    #[test]
    fn clear_drops_all_samples() {
        let estimator = RelayDestinationEstimator::new();
        feed(&estimator, relay(1), 10_000, 60_000, MIN_DEST_SAMPLES);
        assert!(estimator.estimate(relay(1)).is_some());
        estimator.clear();
        assert_eq!(estimator.estimate(relay(1)), None);
    }
}
