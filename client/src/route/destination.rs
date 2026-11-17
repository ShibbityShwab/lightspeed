//! # Destination-aware path estimation
//!
//! The nearest-proxy selector ranks relays by the client-to-relay round trip
//! alone. That leg is cheap to probe for every candidate, but it is only half
//! the path: the relay-to-game-server leg decides whether tunnelling actually
//! beats the direct route. This module estimates that second leg from
//! measurements the client already takes:
//!
//! * the client-to-relay round trip (the keepalive probe, fed through
//!   [`crate::latency::record_client_relay_rtt`]), and
//! * the end-to-end tunnelled round trip, fed only by the physically validated
//!   quiet-flow probe in [`crate::latency`] (`client/src/latency.rs`). The
//!   earlier busy-flow pairing was disabled in `7f34412`: it measured one
//!   inter-send gap, not a round trip.
//!
//! Their difference is an estimate of the relay-to-destination round trip:
//!
//! ```text
//! dest_leg ~= p50(tunnelled) - p50(client_leg)
//! ```
//!
//! The two legs are reduced independently with medians because one median per
//! leg is what the pair can support; medians are robust to the long tail a busy
//! relay shows. The estimator refuses to publish anything until both legs have
//! [`MIN_DEST_SAMPLES`] samples **and** the newest tunnelled sample is within
//! [`DEST_FRESHNESS_TTL`], so a stale path is never presented as current. A
//! non-positive difference is discarded rather than clamped: it means the
//! tunnelled round trip did not exceed the client-to-relay round trip, which
//! cannot describe a real path.
//!
//! The estimate is deliberately coarse and carries its own uncertainty
//! ([`DestinationEstimate::uncertainty_us`] is half the tunnelled
//! interquartile range). Consumers must treat it as a prior, not a fact, and
//! [`RelayDestinationEstimator::measurement_age`] is how far from current the
//! samples behind it are.

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
/// How long the newest tunnelled sample stays usable for an estimate.
///
/// A path whose last validated sample is older than this is not presented as
/// current: TTLs in this crate are the rule for every published window (the
/// direct median, the shadow window and the client-relay keepalive already
/// carry one), and a relay-to-server path can change under the client when the
/// route or the server does. Two minutes spans several keepalive intervals and
/// a typical map change, and it deliberately matches the "fresh enough to show
/// next to an n" bar the GUI applies to the other figures.
pub const DEST_FRESHNESS_TTL: Duration = Duration::from_secs(120);

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

/// Per-relay samples: the client-to-relay leg is shared by every destination,
/// while the end-to-end tunnelled leg is kept per destination so a sample for
/// one game server can never be misread as an estimate for another.
#[derive(Default)]
struct RelaySamples {
    client_leg_us: VecDeque<u64>,
    tunnelled: HashMap<Ipv4Addr, VecDeque<u64>>,
    /// When the newest validated tunnelled sample for each destination arrived,
    /// so a stale path is never published as a current measurement.
    tunnelled_newest: HashMap<Ipv4Addr, Instant>,
}

/// Rolling estimator of the relay-to-destination leg.
///
/// The client-to-relay leg is keyed by relay alone. The tunnelled leg is keyed
/// by `(relay, destination)`, so an estimate describes one relay's path to one
/// game server. A relay that has never carried traffic to a destination has no
/// measured estimate for it and must rely on the region prior.
#[derive(Default)]
pub struct RelayDestinationEstimator {
    relays: StdMutex<HashMap<SocketAddrV4, RelaySamples>>,
    /// Destination IPv4 to canonical region key, populated from the relay's
    /// registration ack when the relay can resolve the destination country.
    dest_regions: StdMutex<HashMap<Ipv4Addr, String>>,
    /// Recent quiet-flow probes per (relay, destination): whole-path round trips
    /// the window shows, kept away from the ranking samples.
    path_probes: StdMutex<HashMap<(SocketAddrV4, Ipv4Addr), VecDeque<PathProbe>>>,
}

/// One attributed whole-path round trip and when it was taken.
#[derive(Clone, Copy, Debug)]
pub struct PathProbe {
    /// Round trip in microseconds.
    pub us: u64,
    /// When the probe was recorded.
    pub at: Instant,
}

/// How many quiet-flow probes the whole-loop figure remembers per pair.
const PATH_PROBE_WINDOW: usize = 16;

impl RelayDestinationEstimator {
    /// Create an empty estimator.
    pub fn new() -> Self {
        Self::default()
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, HashMap<SocketAddrV4, RelaySamples>> {
        self.relays.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn lock_regions(&self) -> std::sync::MutexGuard<'_, HashMap<Ipv4Addr, String>> {
        self.dest_regions
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Record one observed client-to-relay round trip for `relay`.
    ///
    /// Zero, over-long, and clamped samples are dropped.
    pub fn observe_client_leg(&self, relay: SocketAddrV4, rtt_us: u64) {
        if !is_plausible(rtt_us) {
            return;
        }
        let mut relays = self.lock();
        let samples = relays.entry(relay).or_default();
        push_bounded(&mut samples.client_leg_us, rtt_us);
    }

    /// Record one observed end-to-end tunnelled round trip from `relay` to
    /// `destination`.
    ///
    /// Zero, over-long, and clamped samples are dropped. Production feeds this
    /// only from the physically validated quiet-flow probe in [`crate::latency`],
    /// so an entry here is a real whole-path round trip, and its arrival time
    /// starts (or refreshes) the [`DEST_FRESHNESS_TTL`] clock.
    pub fn observe_tunnelled(&self, relay: SocketAddrV4, destination: Ipv4Addr, rtt_us: u64) {
        self.observe_tunnelled_at(relay, destination, rtt_us, Instant::now());
    }

    /// The shared body of [`RelayDestinationEstimator::observe_tunnelled`],
    /// with the arrival instant supplied so the freshness rule is testable
    /// without waiting two minutes.
    fn observe_tunnelled_at(
        &self,
        relay: SocketAddrV4,
        destination: Ipv4Addr,
        rtt_us: u64,
        at: Instant,
    ) {
        if !is_plausible(rtt_us) {
            return;
        }
        let mut relays = self.lock();
        let samples = relays.entry(relay).or_default();
        push_bounded(samples.tunnelled.entry(destination).or_default(), rtt_us);
        samples.tunnelled_newest.insert(destination, at);
    }

    /// The current estimate for the `(relay, destination)` pair, or `None`
    /// while either leg is short of [`MIN_DEST_SAMPLES`], the newest tunnelled
    /// sample is older than [`DEST_FRESHNESS_TTL`], or the implied destination
    /// leg is implausible.
    ///
    /// This is the single publishability gate callers use: when it returns
    /// `Some`, the figure describes a path measured recently enough to present
    /// as current.
    pub fn estimate(
        &self,
        relay: SocketAddrV4,
        destination: Ipv4Addr,
    ) -> Option<DestinationEstimate> {
        let now = Instant::now();
        let relays = self.lock();
        let samples = relays.get(&relay)?;
        let tunnelled = samples.tunnelled.get(&destination)?;
        if !is_fresh(samples.tunnelled_newest.get(&destination), now) {
            return None;
        }
        estimate_from(&samples.client_leg_us, tunnelled)
    }

    /// How long the newest validated tunnelled sample for the pair has been
    /// held, or `None` when the pair has no sample at all.
    ///
    /// The age is what turns a bare number into a statement about the current
    /// path: it is reported next to `n` so a stale path cannot be read as a
    /// live measurement. It returns `Some` whenever a sample exists (even one
    /// too stale or too few to publish through [`Self::estimate`]).
    pub fn measurement_age(&self, relay: SocketAddrV4, destination: Ipv4Addr) -> Option<Duration> {
        let now = Instant::now();
        let relays = self.lock();
        let newest = relays.get(&relay)?.tunnelled_newest.get(&destination)?;
        Some(now.saturating_duration_since(*newest))
    }

    /// Median end-to-end tunnelled round trip for the `(relay, destination)`
    /// pair: client to relay to the game server and back. This is the figure a
    /// player reads as their in-game latency, as opposed to the client-leg RTT
    /// the window shows from the keepalive echo.
    ///
    /// `None` until the pair has [`MIN_DEST_SAMPLES`] samples and the newest is
    /// within [`DEST_FRESHNESS_TTL`], so neither a handful of packets nor a
    /// stale window can present a number as if it were a current measurement.
    pub fn path_rtt_us(&self, relay: SocketAddrV4, destination: Ipv4Addr) -> Option<(u64, usize)> {
        let now = Instant::now();
        let relays = self.lock();
        let samples = relays.get(&relay)?;
        let tunnelled = samples.tunnelled.get(&destination)?;
        if tunnelled.len() < MIN_DEST_SAMPLES {
            return None;
        }
        if !is_fresh(samples.tunnelled_newest.get(&destination), now) {
            return None;
        }
        let mut sorted = tunnelled.iter().copied().collect::<Vec<_>>();
        sorted.sort_unstable();
        Some((median_sorted(&sorted), tunnelled.len()))
    }

    /// Record one quiet-flow probe: a round trip whose reply could be
    /// attributed to a single send because the flow had gone silent first.
    ///
    /// Kept apart from the ranking samples: a probe measures the whole path and
    /// says nothing about the destination leg on its own.
    pub fn observe_path_probe(&self, relay: SocketAddrV4, destination: Ipv4Addr, us: u64) {
        if !is_plausible(us) {
            return;
        }
        let mut probes = self
            .path_probes
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        let entry = probes.entry((relay, destination)).or_default();
        if entry.len() >= PATH_PROBE_WINDOW {
            entry.pop_front();
        }
        entry.push_back(PathProbe {
            us,
            at: Instant::now(),
        });
    }

    /// The newest quiet-flow probe for the pair, with the instant it was taken.
    ///
    /// The age matters: the client-to-relay leg beside it is instantaneous and
    /// swings by tens of milliseconds, so a probe from half a minute ago can
    /// legitimately sit below the current leg. Showing one without the other's
    /// vintage reads as a contradiction.
    pub fn path_probe(&self, relay: SocketAddrV4, destination: Ipv4Addr) -> Option<PathProbe> {
        self.path_probes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(&(relay, destination))
            .and_then(|samples| samples.back().copied())
    }

    /// The newest quiet-flow probe's value, without its age.
    pub fn path_probe_us(&self, relay: SocketAddrV4, destination: Ipv4Addr) -> Option<u64> {
        self.path_probe(relay, destination).map(|probe| probe.us)
    }

    /// A pooled destination-leg estimate across every measured relay pair.
    ///
    /// Retired from production use when the CLI's second ranking was removed:
    /// the canonical whole-path ranking reads per-pair measurements directly and
    /// never borrows a pooled prior. Kept test-only so the aggregation maths it
    /// covered stays asserted; `None` until at least one pair has an estimate,
    /// and pairs whose newest sample is stale are skipped.
    #[cfg(test)]
    pub fn pooled_estimate(&self) -> Option<DestinationEstimate> {
        let now = Instant::now();
        let relays = self.lock();
        let mut dests: Vec<u64> = Vec::new();
        let mut samples = 0usize;
        let mut uncertainty_sum = 0u64;
        let mut confidence_sum = 0.0f64;
        for relay in relays.values() {
            for (destination, tunnelled) in &relay.tunnelled {
                if !is_fresh(relay.tunnelled_newest.get(destination), now) {
                    continue;
                }
                if let Some(est) = estimate_from(&relay.client_leg_us, tunnelled) {
                    dests.push(est.dest_leg_us);
                    samples += est.samples;
                    uncertainty_sum = uncertainty_sum.saturating_add(est.uncertainty_us);
                    confidence_sum += est.confidence;
                }
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

    /// Record the region of a destination, resolved from the relay's
    /// registration ack. Unknown or unparseable regions are ignored, so a
    /// missing value simply leaves the destination without a prior.
    pub fn set_destination_region(&self, destination: Ipv4Addr, region: &str) {
        let Some(key) = super::regions::resolve_region(region) else {
            return;
        };
        self.lock_regions().insert(destination, key.to_string());
    }

    /// The canonical region of a destination, if one was recorded.
    pub fn destination_region(&self, destination: Ipv4Addr) -> Option<String> {
        self.lock_regions().get(&destination).cloned()
    }

    /// Drop every sample. Test-only convenience, but harmless in production.
    pub fn clear(&self) {
        self.lock().clear();
        self.lock_regions().clear();
    }
}

fn estimate_from(
    client_leg_us: &VecDeque<u64>,
    tunnelled_us: &VecDeque<u64>,
) -> Option<DestinationEstimate> {
    if client_leg_us.len() < MIN_DEST_SAMPLES || tunnelled_us.len() < MIN_DEST_SAMPLES {
        return None;
    }
    let sample_count = client_leg_us.len().min(tunnelled_us.len());
    let mut client = client_leg_us.iter().copied().collect::<Vec<_>>();
    let mut tunnelled = tunnelled_us.iter().copied().collect::<Vec<_>>();
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
    let count_factor = (sample_count as f64 / (MIN_DEST_SAMPLES as f64 * 4.0)).min(1.0);
    let confidence = (count_factor * (1.0 - relative_spread)).clamp(0.0, 1.0);

    Some(DestinationEstimate {
        dest_leg_us: dest_leg,
        samples: sample_count,
        uncertainty_us,
        confidence,
    })
}

fn is_plausible(rtt_us: u64) -> bool {
    rtt_us > 0 && rtt_us <= MAX_PLAUSIBLE_RTT_US
}

/// Whether a pair's newest validated sample is recent enough to publish.
fn is_fresh(newest: Option<&Instant>, now: Instant) -> bool {
    newest.is_some_and(|at| now.saturating_duration_since(*at) < DEST_FRESHNESS_TTL)
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

/// Record an end-to-end tunnelled sample for `(relay, destination)` into the
/// global estimator, if installed.
pub fn observe_tunnelled(relay: SocketAddrV4, destination: Ipv4Addr, rtt_us: u64) {
    if let Some(estimator) = global() {
        estimator.observe_tunnelled(relay, destination, rtt_us);
    }
}

/// Record a destination's region into the global estimator, installing it on
/// first use. A registration ack can arrive before routing has set the
/// estimator up, and the region must still be cached when selection runs.
pub fn set_destination_region(destination: Ipv4Addr, region: &str) {
    ensure_global().set_destination_region(destination, region);
}

#[derive(Default)]
struct Sampler {
    last_report: HashMap<Ipv4Addr, Instant>,
}

static SAMPLER: OnceLock<StdMutex<Sampler>> = OnceLock::new();

fn sampler() -> &'static StdMutex<Sampler> {
    SAMPLER.get_or_init(|| StdMutex::new(Sampler::default()))
}

/// Rate limit for reporting the destination to the control plane.
///
/// Returns `true` only when this call claimed the slot for `server`, so the
/// caller can hang the one-shot `report_destination` side effect off it.
///
/// This sampler no longer pairs replies or feeds the estimator: the whole-path
/// feed is the validated quiet-flow probe in [`crate::latency`], which calls
/// [`observe_tunnelled`] only for samples that passed its physical checks. The
/// pairing this used to do (reply minus the most recent outbound) read one
/// inter-send gap, not a round trip, and was disabled in `7f34412`.
pub fn note_outbound(server: Ipv4Addr) -> bool {
    let now = Instant::now();
    let mut sampler = sampler().lock().unwrap_or_else(PoisonError::into_inner);
    match sampler.last_report.get(&server) {
        Some(last) if now.saturating_duration_since(*last) < DEST_SAMPLE_INTERVAL => false,
        _ => {
            sampler.last_report.insert(server, now);
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn relay(a: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, a), 4434)
    }

    fn server(a: u8) -> Ipv4Addr {
        Ipv4Addr::new(8, 8, 8, a)
    }

    fn feed(
        estimator: &RelayDestinationEstimator,
        r: SocketAddrV4,
        destination: Ipv4Addr,
        client: u64,
        tunnelled: u64,
        n: usize,
    ) {
        for _ in 0..n {
            estimator.observe_client_leg(r, client);
            estimator.observe_tunnelled(r, destination, tunnelled);
        }
    }

    #[test]
    fn estimate_requires_minimum_samples_on_both_legs() {
        let estimator = RelayDestinationEstimator::new();
        // One sample short of the threshold on both legs.
        feed(
            &estimator,
            relay(1),
            server(1),
            10_000,
            50_000,
            MIN_DEST_SAMPLES - 1,
        );
        assert_eq!(estimator.estimate(relay(1), server(1)), None);

        feed(&estimator, relay(1), server(1), 10_000, 50_000, 1);
        assert!(estimator.estimate(relay(1), server(1)).is_some());
    }

    #[test]
    fn dest_leg_is_tunnelled_median_minus_client_median() {
        let estimator = RelayDestinationEstimator::new();
        feed(
            &estimator,
            relay(1),
            server(1),
            10_000,
            60_000,
            MIN_DEST_SAMPLES,
        );
        let est = estimator.estimate(relay(1), server(1)).expect("estimate");
        assert_eq!(est.dest_leg_us, 50_000);
        assert_eq!(est.samples, MIN_DEST_SAMPLES);
    }

    #[test]
    fn samples_are_keyed_per_destination_and_do_not_contaminate() {
        let estimator = RelayDestinationEstimator::new();
        feed(
            &estimator,
            relay(1),
            server(1),
            10_000,
            60_000,
            MIN_DEST_SAMPLES,
        );
        // The same relay has served one destination, not the other.
        assert!(
            estimator.estimate(relay(1), server(1)).is_some(),
            "the measured destination must have an estimate"
        );
        assert_eq!(
            estimator.estimate(relay(1), server(2)),
            None,
            "a sample for one game server must never describe another"
        );

        // The other destination's own samples are independent.
        feed(
            &estimator,
            relay(1),
            server(2),
            10_000,
            30_000,
            MIN_DEST_SAMPLES,
        );
        assert_eq!(
            estimator.estimate(relay(1), server(2)).unwrap().dest_leg_us,
            20_000
        );
        assert_eq!(
            estimator.estimate(relay(1), server(1)).unwrap().dest_leg_us,
            50_000
        );
    }

    #[test]
    fn zero_and_clamped_samples_are_ignored() {
        let estimator = RelayDestinationEstimator::new();
        for _ in 0..MIN_DEST_SAMPLES {
            estimator.observe_client_leg(relay(1), 0);
            estimator.observe_tunnelled(relay(1), server(1), u64::MAX);
        }
        assert_eq!(estimator.estimate(relay(1), server(1)), None);
    }

    #[test]
    fn inverted_measurement_is_discarded_not_clamped() {
        let estimator = RelayDestinationEstimator::new();
        // Tunnelled round trip below the client leg is impossible; the
        // estimator must publish nothing rather than a zero destination leg.
        feed(
            &estimator,
            relay(1),
            server(1),
            50_000,
            40_000,
            MIN_DEST_SAMPLES,
        );
        assert_eq!(estimator.estimate(relay(1), server(1)), None);
    }

    #[test]
    fn median_resists_a_single_outlier() {
        let estimator = RelayDestinationEstimator::new();
        for _ in 0..MIN_DEST_SAMPLES {
            estimator.observe_client_leg(relay(1), 10_000);
            estimator.observe_tunnelled(relay(1), server(1), 60_000);
        }
        // One late spike must not move the median.
        estimator.observe_tunnelled(relay(1), server(1), 5_000_000);
        let est = estimator.estimate(relay(1), server(1)).expect("estimate");
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
            estimator.observe_tunnelled(relay(1), server(1), 60_000);
        }
        for _ in 0..MIN_DEST_SAMPLES / 2 {
            estimator.observe_tunnelled(relay(1), server(1), 80_000);
        }
        let est = estimator.estimate(relay(1), server(1)).expect("estimate");
        assert!(est.uncertainty_us > 0, "spread must be reported");
        assert!(est.confidence < 1.0);
    }

    #[test]
    fn tighter_spread_has_higher_confidence() {
        let steady = RelayDestinationEstimator::new();
        feed(
            &steady,
            relay(1),
            server(1),
            10_000,
            60_000,
            MIN_DEST_SAMPLES * 4,
        );
        let noisy = RelayDestinationEstimator::new();
        for _ in 0..MIN_DEST_SAMPLES * 4 {
            noisy.observe_client_leg(relay(1), 10_000);
        }
        for i in 0..MIN_DEST_SAMPLES * 4 {
            noisy.observe_tunnelled(relay(1), server(1), 40_000 + (i as u64 % 2) * 40_000);
        }
        let s = steady.estimate(relay(1), server(1)).unwrap();
        let n = noisy.estimate(relay(1), server(1)).unwrap();
        assert!(s.confidence > n.confidence);
    }

    #[test]
    fn the_path_probe_reports_the_newest_sample_and_forgets_old_ones() {
        let est = RelayDestinationEstimator::new();
        assert_eq!(est.path_probe_us(relay(1), server(1)), None);
        est.observe_path_probe(relay(1), server(1), 260_000);
        est.observe_path_probe(relay(1), server(1), 244_000);
        assert_eq!(
            est.path_probe_us(relay(1), server(1)),
            Some(244_000),
            "the newest probe wins, not the fastest"
        );
        for _ in 0..PATH_PROBE_WINDOW {
            est.observe_path_probe(relay(1), server(1), 300_000);
        }
        assert_eq!(est.path_probe_us(relay(1), server(1)), Some(300_000));
        assert_eq!(
            est.path_probe_us(relay(2), server(1)),
            None,
            "a different relay has no probe"
        );
    }

    #[test]
    fn a_probe_never_feeds_the_relay_ranking() {
        let est = RelayDestinationEstimator::new();
        est.observe_path_probe(relay(1), server(1), 250_000);
        assert_eq!(
            est.estimate(relay(1), server(1)),
            None,
            "a whole-path probe is not a destination-leg measurement"
        );
    }

    #[test]
    fn the_path_rtt_is_the_end_to_end_median_not_the_client_leg() {
        let est = RelayDestinationEstimator::new();
        feed(
            &est,
            relay(1),
            server(1),
            20_000,
            60_000,
            MIN_DEST_SAMPLES + 2,
        );
        let (path, samples) = est.path_rtt_us(relay(1), server(1)).expect("measured");
        assert_eq!(path, 60_000, "the whole client-relay-server round trip");
        assert!(samples >= MIN_DEST_SAMPLES);
    }

    #[test]
    fn the_path_rtt_waits_for_enough_samples() {
        let est = RelayDestinationEstimator::new();
        feed(
            &est,
            relay(1),
            server(1),
            20_000,
            60_000,
            MIN_DEST_SAMPLES - 1,
        );
        assert_eq!(est.path_rtt_us(relay(1), server(1)), None);
        assert_eq!(
            est.path_rtt_us(relay(2), server(1)),
            None,
            "unmeasured relay"
        );
        assert_eq!(
            est.path_rtt_us(relay(1), server(9)),
            None,
            "unmeasured server"
        );
    }

    #[test]
    fn pooled_estimate_is_defined_once_any_relay_is_ready() {
        let estimator = RelayDestinationEstimator::new();
        assert_eq!(estimator.pooled_estimate(), None);
        feed(
            &estimator,
            relay(1),
            server(1),
            10_000,
            60_000,
            MIN_DEST_SAMPLES,
        );
        let pooled = estimator.pooled_estimate().expect("pooled");
        assert_eq!(pooled.dest_leg_us, 50_000);
    }

    #[test]
    fn a_stale_tunnelled_window_is_not_published_as_current() {
        // Feed a full window, then age only the newest sample past the TTL.
        // Every reader that presents the path as current must refuse it.
        let est = RelayDestinationEstimator::new();
        feed(&est, relay(1), server(1), 20_000, 260_000, MIN_DEST_SAMPLES);
        assert!(
            est.estimate(relay(1), server(1)).is_some(),
            "fresh is usable"
        );
        let stale_at = Instant::now() - DEST_FRESHNESS_TTL - Duration::from_secs(1);
        est.observe_tunnelled_at(relay(1), server(1), 260_000, stale_at);
        assert_eq!(
            est.estimate(relay(1), server(1)),
            None,
            "a path last validated more than the TTL ago must not be estimated"
        );
        assert_eq!(
            est.path_rtt_us(relay(1), server(1)),
            None,
            "nor presented as a current whole-path RTT"
        );
    }

    #[test]
    fn the_freshness_boundary_is_the_ttl() {
        let est = RelayDestinationEstimator::new();
        feed(&est, relay(1), server(1), 20_000, 260_000, MIN_DEST_SAMPLES);
        let inside = Instant::now() - DEST_FRESHNESS_TTL + Duration::from_secs(1);
        est.observe_tunnelled_at(relay(1), server(1), 300_000, inside);
        assert!(
            est.estimate(relay(1), server(1)).is_some(),
            "one second inside the TTL is still current"
        );

        let exactly = Instant::now() - DEST_FRESHNESS_TTL;
        est.observe_tunnelled_at(relay(1), server(1), 300_000, exactly);
        assert_eq!(
            est.estimate(relay(1), server(1)),
            None,
            "the TTL is exclusive, matching the other windows in this crate"
        );
    }

    #[test]
    fn measurement_age_reports_the_newest_sample_without_gating_on_n() {
        let est = RelayDestinationEstimator::new();
        assert_eq!(
            est.measurement_age(relay(1), server(1)),
            None,
            "no sample means no age"
        );
        let five_seconds_ago = Instant::now() - Duration::from_secs(5);
        est.observe_tunnelled_at(relay(1), server(1), 260_000, five_seconds_ago);
        let age = est
            .measurement_age(relay(1), server(1))
            .expect("one sample has an age");
        assert!(
            age >= Duration::from_secs(5),
            "the age is measured from the newest sample: {age:?}"
        );
        // Being under MIN_DEST_SAMPLES does not hide the age: the GUI shows it
        // beside the n that explains why no estimate is published.
        assert_eq!(est.estimate(relay(1), server(1)), None);
        assert!(est.measurement_age(relay(1), server(1)).is_some());
        assert_eq!(
            est.measurement_age(relay(2), server(1)),
            None,
            "a different relay has no sample"
        );
    }

    #[test]
    fn a_pooled_prior_never_borrows_from_a_stale_path() {
        let estimator = RelayDestinationEstimator::new();
        assert_eq!(estimator.pooled_estimate(), None);
        let stale_at = Instant::now() - DEST_FRESHNESS_TTL - Duration::from_secs(1);
        for _ in 0..MIN_DEST_SAMPLES {
            estimator.observe_client_leg(relay(1), 10_000);
            estimator.observe_tunnelled_at(relay(1), server(1), 60_000, stale_at);
        }
        assert_eq!(
            estimator.pooled_estimate(),
            None,
            "a stale pair must not seed another candidate's prior"
        );
    }

    #[test]
    fn a_new_sample_refreshes_the_freshness_clock() {
        let est = RelayDestinationEstimator::new();
        feed(&est, relay(1), server(1), 20_000, 260_000, MIN_DEST_SAMPLES);
        let stale_at = Instant::now() - DEST_FRESHNESS_TTL - Duration::from_secs(1);
        est.observe_tunnelled_at(relay(1), server(1), 260_000, stale_at);
        assert_eq!(est.estimate(relay(1), server(1)), None);
        // A new validated sample is a new measurement: the path is current again.
        est.observe_tunnelled(relay(1), server(1), 260_000);
        assert!(
            est.estimate(relay(1), server(1)).is_some(),
            "a new sample restarts the TTL"
        );
    }

    #[test]
    fn destination_region_cache_roundtrips_and_rejects_unknown() {
        let estimator = RelayDestinationEstimator::new();
        assert_eq!(estimator.destination_region(server(1)), None);
        estimator.set_destination_region(server(1), "AP-SOUTHEAST-2");
        assert_eq!(
            estimator.destination_region(server(1)).as_deref(),
            Some("oceania")
        );
        estimator.set_destination_region(server(2), "atlantis");
        assert_eq!(estimator.destination_region(server(2)), None);
    }

    #[test]
    fn clear_drops_all_samples() {
        let estimator = RelayDestinationEstimator::new();
        feed(
            &estimator,
            relay(1),
            server(1),
            10_000,
            60_000,
            MIN_DEST_SAMPLES,
        );
        estimator.set_destination_region(server(1), "oceania");
        assert!(estimator.estimate(relay(1), server(1)).is_some());
        estimator.clear();
        assert_eq!(estimator.estimate(relay(1), server(1)), None);
        assert_eq!(estimator.destination_region(server(1)), None);
    }
}
