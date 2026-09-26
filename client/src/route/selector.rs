//! # Route Selector Implementations
//!
//! Concrete route selection strategies:
//! - **NearestSelector** - picks proxy with lowest measured latency (MVP default)
//! - **MlSelector** - AI-powered route prediction using linfa model

use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::sync::Arc;

use super::destination::{self, DestinationEstimate, RelayDestinationEstimator};
use super::regions;
use super::{ProxyHealth, ProxyNode, RouteSelector, RouteStrategy, SelectedRoute};
use crate::error::RouteError;
use crate::ml::features::{extract_features, LatencyTracker, NetworkFeatures};
use crate::ml::RouteModel;

/// Advisory confidence carried by a region-prior destination leg. The prior is
/// a distance heuristic, not a measurement, so it must never look as certain as
/// real samples.
const PRIOR_CONFIDENCE: f64 = 0.25;

/// Largest measured uncertainty priced into a candidate's score. A very noisy
/// sample is capped so a single bad window cannot dominate the ranking.
const MEASURED_UNCERTAINTY_CAP_US: u64 = 25_000;

/// Flat uncertainty charged to a borrowed destination leg, which describes a
/// different relay's path and is therefore always less trustworthy than a
/// direct measurement.
const BORROWED_UNCERTAINTY_US: u64 = 20_000;

/// Flat uncertainty charged to a region-prior destination leg, which is a
/// great-circle heuristic rather than a measurement.
const PRIOR_UNCERTAINTY_US: u64 = 15_000;

/// Scores within this window are treated as tied and resolved by provenance,
/// so a difference smaller than the measurement floor cannot flip the choice.
const PROVENANCE_TIE_BAND_US: u64 = 1_000;

/// Where a destination leg came from, ordered from most to least preferred
/// within the tie-band: real samples beat another relay's pooled samples, which
/// beat a region-level distance heuristic.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Provenance {
    /// Direct client-to-relay samples for this destination.
    Measured,
    /// Another relay's pooled estimate, borrowed for an unmeasured candidate.
    Borrowed,
    /// A coarse region-level distance heuristic.
    Prior,
}

/// The estimated relay-to-destination leg for one candidate: the point
/// estimate, advisory confidence, provenance, and the uncertainty charged when
/// ranking.
#[derive(Clone, Copy)]
struct DestLeg {
    /// Estimated relay-to-destination round trip in microseconds.
    dest_leg_us: u64,
    /// Advisory confidence in `0.0..=1.0` (see [`DestinationEstimate`]).
    confidence: f64,
    /// Uncertainty charged to the risk-adjusted score, in microseconds.
    penalty_us: u64,
    /// Where this leg's estimate came from.
    provenance: Provenance,
}

/// A scored candidate: the proxy plus every field the comparator orders on.
struct Candidate<'a> {
    proxy: &'a ProxyNode,
    score_us: u64,
    provenance: Provenance,
    confidence: f64,
    client_leg_us: u64,
}

/// Destination-aware selector: ranks relays by the whole path, not just the
/// client-to-relay leg.
///
/// For each healthy candidate it adds the locally measured client leg
/// ([`ProxyNode::latency_us`]) to the estimated relay-to-destination leg from
/// [`RelayDestinationEstimator`]. The ranking is risk-adjusted: each leg's
/// uncertainty is priced in with a capped penalty (measured uncertainty up to
/// [`MEASURED_UNCERTAINTY_CAP_US`], [`PRIOR_UNCERTAINTY_US`] for a region
/// prior, [`BORROWED_UNCERTAINTY_US`] for a pooled borrow), so a stable path
/// beats a noisy one of the same point estimate and a thin measurement cannot
/// outrank solid evidence or freeze out an unmeasured relay whose prior is
/// genuinely better. Candidates within [`PROVENANCE_TIE_BAND_US`] are separated
/// by provenance rather than by noise. A candidate with no destination signal
/// of its own borrows the pooled estimate across relays that do have one. When
/// no candidate has any destination signal, this delegates to
/// [`NearestSelector`], so behaviour is never worse than the MVP selector.
pub struct DestinationAwareSelector {
    estimator: Option<Arc<RelayDestinationEstimator>>,
}

impl DestinationAwareSelector {
    /// A selector reading the process-wide estimator, if one was installed.
    pub fn new() -> Self {
        Self {
            estimator: destination::global().cloned(),
        }
    }

    /// A selector backed by an explicit estimator (tests, or a local store).
    pub fn with_estimator(estimator: Arc<RelayDestinationEstimator>) -> Self {
        Self {
            estimator: Some(estimator),
        }
    }
}

impl Default for DestinationAwareSelector {
    fn default() -> Self {
        Self::new()
    }
}

impl RouteSelector for DestinationAwareSelector {
    fn select(
        &self,
        game_server: SocketAddrV4,
        available_proxies: &[ProxyNode],
    ) -> Result<SelectedRoute, RouteError> {
        let nearest = NearestSelector::new();
        let healthy: Vec<&ProxyNode> = available_proxies
            .iter()
            .filter(|p| p.health == ProxyHealth::Healthy)
            .collect();
        if healthy.is_empty() {
            return Err(RouteError::AllUnhealthy);
        }
        let Some(estimator) = self.estimator.as_ref() else {
            return nearest.select(game_server, available_proxies);
        };

        let destination = *game_server.ip();
        let dest_region = estimator.destination_region(destination);

        let measured: Vec<Option<DestinationEstimate>> = healthy
            .iter()
            .map(|p| estimator.estimate(p.data_addr, destination))
            .collect();

        // Measured evidence for this destination wins; an unmeasured relay
        // falls back to a region prior when both regions are known, and relays
        // with neither rely on the pooled borrow below.
        let legs: Vec<Option<DestLeg>> = healthy
            .iter()
            .zip(measured.iter())
            .map(|(proxy, estimate)| {
                let leg = match estimate {
                    Some(est) => DestLeg {
                        dest_leg_us: est.dest_leg_us,
                        confidence: est.confidence,
                        penalty_us: measured_penalty_us(est.uncertainty_us),
                        provenance: Provenance::Measured,
                    },
                    None => {
                        let prior =
                            regions::prior_dest_leg_us(&proxy.region, dest_region.as_deref()?)?;
                        DestLeg {
                            dest_leg_us: prior,
                            confidence: PRIOR_CONFIDENCE,
                            penalty_us: PRIOR_UNCERTAINTY_US,
                            provenance: Provenance::Prior,
                        }
                    }
                };
                Some(leg)
            })
            .collect();

        let has_signal = legs.iter().any(Option::is_some);
        let pooled = estimator.pooled_estimate();
        if !has_signal && pooled.is_none() {
            return nearest.select(game_server, available_proxies);
        }

        let borrow: DestLeg = match &pooled {
            Some(est) => DestLeg {
                dest_leg_us: est.dest_leg_us,
                confidence: est.confidence * 0.5,
                penalty_us: BORROWED_UNCERTAINTY_US,
                provenance: Provenance::Borrowed,
            },
            None => {
                let known: Vec<u64> = legs.iter().flatten().map(|leg| leg.dest_leg_us).collect();
                DestLeg {
                    dest_leg_us: median_u64(&known).unwrap_or(0),
                    confidence: PRIOR_CONFIDENCE * 0.5,
                    penalty_us: BORROWED_UNCERTAINTY_US,
                    provenance: Provenance::Borrowed,
                }
            }
        };

        let mut scored: Vec<Candidate<'_>> = healthy
            .iter()
            .zip(legs.iter())
            .map(|(proxy, leg)| {
                let client_leg = proxy.latency_us.unwrap_or(u64::MAX);
                let leg = leg.unwrap_or(borrow);
                Candidate {
                    proxy,
                    score_us: risk_adjusted_score(client_leg, leg.dest_leg_us, leg.penalty_us),
                    provenance: leg.provenance,
                    confidence: leg.confidence,
                    client_leg_us: client_leg,
                }
            })
            .collect();
        scored.sort_by(compare_candidates);

        let confidence_sum: f64 = scored.iter().map(|c| c.confidence).sum();
        let confidence = (0.4 + 0.5 * confidence_sum / scored.len() as f64).clamp(0.3, 0.9);

        let primary = scored[0].proxy.clone();
        let backups: Vec<ProxyNode> = scored[1..].iter().map(|c| c.proxy.clone()).collect();

        Ok(SelectedRoute {
            primary,
            backups,
            confidence,
            strategy: RouteStrategy::DestinationAware,
        })
    }

    fn feedback(&mut self, _proxy_id: &str, _observed_latency_us: u64) {
        // Samples are recorded through `route::destination` observers, which
        // are keyed by relay address rather than the selector's proxy id.
    }

    fn strategy(&self) -> RouteStrategy {
        RouteStrategy::DestinationAware
    }
}

fn median_u64(values: &[u64]) -> Option<u64> {
    if values.is_empty() {
        return None;
    }
    let mut sorted = values.to_vec();
    sorted.sort_unstable();
    Some(sorted[sorted.len() / 2])
}

fn measured_penalty_us(uncertainty_us: u64) -> u64 {
    uncertainty_us.min(MEASURED_UNCERTAINTY_CAP_US)
}

fn risk_adjusted_score(client_leg_us: u64, dest_leg_us: u64, penalty_us: u64) -> u64 {
    client_leg_us
        .saturating_add(dest_leg_us)
        .saturating_add(penalty_us)
}

fn compare_candidates(a: &Candidate<'_>, b: &Candidate<'_>) -> std::cmp::Ordering {
    let a_band = a.score_us / PROVENANCE_TIE_BAND_US;
    let b_band = b.score_us / PROVENANCE_TIE_BAND_US;
    a_band
        .cmp(&b_band)
        .then(a.provenance.cmp(&b.provenance))
        .then(b.confidence.total_cmp(&a.confidence))
        .then(a.client_leg_us.cmp(&b.client_leg_us))
}

// ── Nearest Selector (MVP Default) ──────────────────────────

/// Nearest-proxy selector - picks the proxy with lowest measured latency.
/// This is the default MVP strategy.
pub struct NearestSelector;

impl NearestSelector {
    pub fn new() -> Self {
        Self
    }
}

impl RouteSelector for NearestSelector {
    fn select(
        &self,
        _game_server: SocketAddrV4,
        available_proxies: &[ProxyNode],
    ) -> Result<SelectedRoute, RouteError> {
        // Filter to healthy proxies
        let healthy: Vec<&ProxyNode> = available_proxies
            .iter()
            .filter(|p| p.health == ProxyHealth::Healthy)
            .collect();

        if healthy.is_empty() {
            return Err(RouteError::AllUnhealthy);
        }

        // Sort by latency (lowest first), with unknown latency last
        let mut sorted = healthy.clone();
        sorted.sort_by_key(|p| p.latency_us.unwrap_or(u64::MAX));

        let primary = sorted[0].clone();
        let backups: Vec<ProxyNode> = sorted[1..].iter().map(|p| (*p).clone()).collect();

        Ok(SelectedRoute {
            primary,
            backups,
            confidence: 0.8,
            strategy: RouteStrategy::Nearest,
        })
    }

    fn feedback(&mut self, _proxy_id: &str, _observed_latency_us: u64) {
        // NearestSelector doesn't learn - ML selector does
    }

    fn strategy(&self) -> RouteStrategy {
        RouteStrategy::Nearest
    }
}

// ── ML Selector (AI-Powered) ────────────────────────────────

/// ML-powered route selector using the trained linfa model.
///
/// Maintains per-proxy LatencyTrackers to compute features from
/// live network measurements, then uses the ML model to predict
/// which proxy will provide the lowest latency.
pub struct MlSelector {
    /// The loaded ML model.
    model: RouteModel,
    /// Per-proxy latency trackers keyed by proxy ID.
    trackers: HashMap<String, LatencyTracker>,
    /// Per-proxy metadata (hop count, AS path len, distance).
    proxy_meta: HashMap<String, ProxyMeta>,
    /// Window size for latency trackers.
    tracker_window: usize,
    /// Number of predictions made.
    prediction_count: u64,
    /// Total inference time (microseconds) for averaging.
    total_inference_us: u64,
}

/// Static metadata about a proxy (doesn't change per-packet).
#[derive(Debug, Clone)]
pub struct ProxyMeta {
    /// Number of network hops (from traceroute).
    pub hop_count: u32,
    /// BGP AS path length.
    pub bgp_as_path_len: u32,
    /// Geographic distance in km.
    pub geographic_distance_km: f64,
}

impl Default for ProxyMeta {
    fn default() -> Self {
        Self {
            hop_count: 10,
            bgp_as_path_len: 5,
            geographic_distance_km: 1000.0,
        }
    }
}

impl MlSelector {
    /// Create a new ML selector with a pre-loaded model.
    pub fn new(model: RouteModel, tracker_window: usize) -> Self {
        Self {
            model,
            trackers: HashMap::new(),
            proxy_meta: HashMap::new(),
            tracker_window,
            prediction_count: 0,
            total_inference_us: 0,
        }
    }

    /// Create an ML selector that trains on synthetic data.
    pub fn with_synthetic_training(tracker_window: usize) -> Result<Self, crate::error::MlError> {
        let mut model = RouteModel::new();
        let report = model.train_and_load()?;
        tracing::info!(
            "ML selector initialized with synthetic model: MAE={:.2}ms, R²={:.4}",
            report.mae_ms,
            report.r_squared
        );
        Ok(Self::new(model, tracker_window))
    }

    /// Set metadata for a proxy (call after discovering proxy info).
    pub fn set_proxy_meta(&mut self, proxy_id: &str, meta: ProxyMeta) {
        self.proxy_meta.insert(proxy_id.to_string(), meta);
    }

    /// Get or create a latency tracker for a proxy.
    fn get_tracker(&mut self, proxy_id: &str) -> &mut LatencyTracker {
        let window = self.tracker_window;
        self.trackers
            .entry(proxy_id.to_string())
            .or_insert_with(|| LatencyTracker::new(window))
    }

    /// Build features for all healthy proxies.
    fn build_features(&self, proxies: &[ProxyNode]) -> Vec<(String, NetworkFeatures)> {
        proxies
            .iter()
            .filter(|p| p.health == ProxyHealth::Healthy || p.health == ProxyHealth::Degraded)
            .map(|proxy| {
                let meta = self.proxy_meta.get(&proxy.id).cloned().unwrap_or_default();

                let tracker = self.trackers.get(&proxy.id);

                let features = if let Some(tracker) = tracker {
                    extract_features(
                        tracker,
                        meta.hop_count,
                        meta.bgp_as_path_len,
                        proxy.load,
                        meta.geographic_distance_km,
                    )
                } else {
                    // No historical data yet - use proxy's reported latency
                    NetworkFeatures {
                        current_latency_ms: proxy
                            .latency_us
                            .map(|us| us as f64 / 1000.0)
                            .unwrap_or(100.0),
                        historical_p50_ms: proxy
                            .latency_us
                            .map(|us| us as f64 / 1000.0)
                            .unwrap_or(100.0),
                        historical_p95_ms: proxy
                            .latency_us
                            .map(|us| us as f64 / 1000.0 * 1.5)
                            .unwrap_or(150.0),
                        jitter_ms: 5.0,
                        hop_count: meta.hop_count,
                        time_of_day: chrono::Local::now()
                            .format("%H")
                            .to_string()
                            .parse()
                            .unwrap_or(12),
                        day_of_week: chrono::Local::now()
                            .format("%u")
                            .to_string()
                            .parse::<u8>()
                            .unwrap_or(1)
                            - 1,
                        bgp_as_path_len: meta.bgp_as_path_len,
                        packet_loss_pct: 0.01,
                        proxy_load: proxy.load,
                        geographic_distance_km: meta.geographic_distance_km,
                    }
                };

                (proxy.id.clone(), features)
            })
            .collect()
    }

    /// Average inference time in microseconds.
    pub fn avg_inference_us(&self) -> f64 {
        if self.prediction_count == 0 {
            return 0.0;
        }
        self.total_inference_us as f64 / self.prediction_count as f64
    }
}

impl RouteSelector for MlSelector {
    fn select(
        &self,
        _game_server: SocketAddrV4,
        available_proxies: &[ProxyNode],
    ) -> Result<SelectedRoute, RouteError> {
        if available_proxies.is_empty() {
            return Err(RouteError::NoProxies);
        }

        let healthy: Vec<_> = available_proxies
            .iter()
            .filter(|p| p.health == ProxyHealth::Healthy || p.health == ProxyHealth::Degraded)
            .cloned()
            .collect();

        if healthy.is_empty() {
            return Err(RouteError::AllUnhealthy);
        }

        // If model isn't loaded, fall back to nearest
        if !self.model.is_loaded() {
            tracing::warn!("ML model not loaded - falling back to nearest selector");
            let nearest = NearestSelector::new();
            return nearest.select(_game_server, available_proxies);
        }

        // Build features and predict
        let features = self.build_features(&healthy);

        match self.model.predict(&features) {
            Ok(prediction) => {
                let ranked = prediction.ranked_proxies();

                if ranked.is_empty() {
                    return Err(RouteError::NoProxies);
                }

                // Find the primary proxy (lowest predicted latency)
                let primary_id = ranked[0].0;
                let primary = healthy
                    .iter()
                    .find(|p| p.id == primary_id)
                    .cloned()
                    .ok_or(RouteError::NoProxies)?;

                // Build backup list from remaining ranked proxies
                let backups: Vec<ProxyNode> = ranked[1..]
                    .iter()
                    .filter_map(|(id, _)| healthy.iter().find(|p| p.id == *id).cloned())
                    .collect();

                tracing::debug!(
                    "ML route selected: {} (pred={:.1}ms, conf={:.2}, infer={}μs)",
                    primary.id,
                    ranked[0].1,
                    prediction.confidence,
                    prediction.inference_time_us
                );

                Ok(SelectedRoute {
                    primary,
                    backups,
                    confidence: prediction.confidence,
                    strategy: RouteStrategy::MlPredicted,
                })
            }
            Err(e) => {
                tracing::warn!("ML prediction failed ({}), falling back to nearest", e);
                let nearest = NearestSelector::new();
                nearest.select(_game_server, available_proxies)
            }
        }
    }

    fn feedback(&mut self, proxy_id: &str, observed_latency_us: u64) {
        let latency_ms = observed_latency_us as f64 / 1000.0;
        let tracker = self.get_tracker(proxy_id);
        tracker.record(latency_ms);

        tracing::trace!(
            "ML feedback: {} = {:.1}ms (window: {}, p50: {:.1}ms)",
            proxy_id,
            latency_ms,
            tracker.sample_count(),
            tracker.p50()
        );
    }

    fn strategy(&self) -> RouteStrategy {
        RouteStrategy::MlPredicted
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn make_test_proxies() -> Vec<ProxyNode> {
        vec![
            ProxyNode {
                id: "proxy-us-east".into(),
                data_addr: "10.0.1.1:4434".parse().unwrap(),
                control_addr: "10.0.1.1:4433".parse().unwrap(),
                region: "us-east".into(),
                health: ProxyHealth::Healthy,
                latency_us: Some(25_000),
                load: 0.3,
            },
            ProxyNode {
                id: "proxy-eu-west".into(),
                data_addr: "10.0.2.1:4434".parse().unwrap(),
                control_addr: "10.0.2.1:4433".parse().unwrap(),
                region: "eu-west".into(),
                health: ProxyHealth::Healthy,
                latency_us: Some(45_000),
                load: 0.5,
            },
            ProxyNode {
                id: "proxy-asia-se".into(),
                data_addr: "10.0.3.1:4434".parse().unwrap(),
                control_addr: "10.0.3.1:4433".parse().unwrap(),
                region: "asia-se".into(),
                health: ProxyHealth::Healthy,
                latency_us: Some(80_000),
                load: 0.2,
            },
        ]
    }

    #[test]
    fn test_nearest_selector() {
        let selector = NearestSelector::new();
        let proxies = make_test_proxies();
        let game_server: SocketAddrV4 = "1.2.3.4:27015".parse().unwrap();

        let result = selector.select(game_server, &proxies).unwrap();
        assert_eq!(result.primary.id, "proxy-us-east"); // Lowest latency
        assert_eq!(result.backups.len(), 2);
        assert_eq!(result.strategy, RouteStrategy::Nearest);
    }

    #[test]
    fn test_nearest_all_unhealthy() {
        let selector = NearestSelector::new();
        let proxies: Vec<ProxyNode> = make_test_proxies()
            .into_iter()
            .map(|mut p| {
                p.health = ProxyHealth::Unhealthy;
                p
            })
            .collect();
        let game_server: SocketAddrV4 = "1.2.3.4:27015".parse().unwrap();

        let result = selector.select(game_server, &proxies);
        assert!(result.is_err());
    }

    #[test]
    fn test_ml_selector_fallback_without_model() {
        // MlSelector without a loaded model should fall back to nearest
        let model = RouteModel::new(); // Not loaded
        let selector = MlSelector::new(model, 100);
        let proxies = make_test_proxies();
        let game_server: SocketAddrV4 = "1.2.3.4:27015".parse().unwrap();

        let result = selector.select(game_server, &proxies).unwrap();
        // Should still work via fallback
        assert_eq!(result.primary.id, "proxy-us-east");
    }

    #[test]
    fn test_ml_selector_feedback() {
        let model = RouteModel::new();
        let mut selector = MlSelector::new(model, 100);

        // Record some feedback
        for i in 0..20 {
            selector.feedback("proxy-us-east", 25_000 + i * 100);
        }

        // Tracker should have data
        let tracker = selector.trackers.get("proxy-us-east").unwrap();
        assert_eq!(tracker.sample_count(), 20);
        assert!(tracker.is_ready());
    }

    fn node(id: &str, addr: &str, latency_us: u64) -> ProxyNode {
        ProxyNode {
            id: id.into(),
            data_addr: addr.parse().unwrap(),
            control_addr: addr.parse().unwrap(),
            region: "test".into(),
            health: ProxyHealth::Healthy,
            latency_us: Some(latency_us),
            load: 0.0,
        }
    }

    fn feed_legs(
        estimator: &RelayDestinationEstimator,
        addr: SocketAddrV4,
        destination: Ipv4Addr,
        client_us: u64,
        tunnelled_us: u64,
    ) {
        for _ in 0..super::destination::MIN_DEST_SAMPLES {
            estimator.observe_client_leg(addr, client_us);
            estimator.observe_tunnelled(addr, destination, tunnelled_us);
        }
    }

    fn feed_varied(
        estimator: &RelayDestinationEstimator,
        addr: SocketAddrV4,
        destination: Ipv4Addr,
        client_us: u64,
        tunnelled_us: &[u64],
    ) {
        for &sample in tunnelled_us {
            estimator.observe_client_leg(addr, client_us);
            estimator.observe_tunnelled(addr, destination, sample);
        }
    }

    fn node_in_region(id: &str, addr: &str, latency_us: u64, region: &str) -> ProxyNode {
        let mut proxy = node(id, addr, latency_us);
        proxy.region = region.into();
        proxy
    }

    #[test]
    fn test_destination_aware_prefers_better_whole_path() {
        // Relay A is closer to the client but far from the game server.
        // Relay B is farther from the client but nearly on top of the server.
        let near_client: SocketAddrV4 = "10.0.1.1:4434".parse().unwrap();
        let near_server: SocketAddrV4 = "10.0.2.1:4434".parse().unwrap();
        let destination: Ipv4Addr = "1.2.3.4".parse().unwrap();
        let estimator = Arc::new(RelayDestinationEstimator::new());
        feed_legs(&estimator, near_client, destination, 10_000, 100_000); // dest 90ms
        feed_legs(&estimator, near_server, destination, 40_000, 50_000); // dest 10ms

        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = vec![
            node("near-client", "10.0.1.1:4434", 10_000),
            node("near-server", "10.0.2.1:4434", 40_000),
        ];

        let result = selector
            .select("1.2.3.4:27015".parse().unwrap(), &proxies)
            .unwrap();
        assert_eq!(
            result.primary.id, "near-server",
            "the relay closest to the game server must win once the destination leg is known"
        );
        assert_eq!(result.strategy, RouteStrategy::DestinationAware);
        assert_eq!(result.backups[0].id, "near-client");
    }

    #[test]
    fn test_destination_aware_falls_back_to_nearest_without_signal() {
        let estimator = Arc::new(RelayDestinationEstimator::new());
        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = make_test_proxies();

        let result = selector
            .select("1.2.3.4:27015".parse().unwrap(), &proxies)
            .unwrap();
        assert_eq!(result.primary.id, "proxy-us-east");
        assert_eq!(result.strategy, RouteStrategy::Nearest);
    }

    #[test]
    fn test_destination_aware_without_estimator_is_nearest() {
        let selector = DestinationAwareSelector { estimator: None };
        let proxies = make_test_proxies();
        let result = selector
            .select("1.2.3.4:27015".parse().unwrap(), &proxies)
            .unwrap();
        assert_eq!(result.primary.id, "proxy-us-east");
        assert_eq!(result.strategy, RouteStrategy::Nearest);
    }

    #[test]
    fn test_destination_aware_tie_break_keeps_input_order() {
        let a: SocketAddrV4 = "10.0.1.1:4434".parse().unwrap();
        let b: SocketAddrV4 = "10.0.2.1:4434".parse().unwrap();
        let destination: Ipv4Addr = "1.2.3.4".parse().unwrap();
        let estimator = Arc::new(RelayDestinationEstimator::new());
        feed_legs(&estimator, a, destination, 10_000, 60_000);
        feed_legs(&estimator, b, destination, 10_000, 60_000);
        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = vec![
            node("first", "10.0.1.1:4434", 10_000),
            node("second", "10.0.2.1:4434", 10_000),
        ];

        let result = selector
            .select("1.2.3.4:27015".parse().unwrap(), &proxies)
            .unwrap();
        assert_eq!(
            result.primary.id, "first",
            "equal scores must keep the input order, matching the nearest selector"
        );
    }

    #[test]
    fn test_samples_do_not_contaminate_across_servers() {
        // "far" is farther from the client but has a great measured path to
        // server A. Selecting for server B must not reuse server A's samples:
        // B has no signal, so the nearest relay wins.
        let far: SocketAddrV4 = "10.0.2.1:4434".parse().unwrap();
        let near: SocketAddrV4 = "10.0.1.1:4434".parse().unwrap();
        let server_a: Ipv4Addr = "1.2.3.4".parse().unwrap();
        let server_b: SocketAddrV4 = "5.6.7.8:27015".parse().unwrap();
        let estimator = Arc::new(RelayDestinationEstimator::new());
        feed_legs(&estimator, far, server_a, 40_000, 50_000); // dest 10ms
        feed_legs(&estimator, near, server_a, 10_000, 100_000); // dest 90ms

        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = vec![
            node("near", "10.0.1.1:4434", 10_000),
            node("far", "10.0.2.1:4434", 40_000),
        ];

        let for_a = selector
            .select(SocketAddrV4::new(server_a, 27015), &proxies)
            .unwrap();
        assert_eq!(
            for_a.primary.id, "far",
            "server A's measured path decides the route to server A"
        );

        let for_b = selector.select(server_b, &proxies).unwrap();
        assert_eq!(
            for_b.primary.id, "near",
            "server A's samples must not decide the route to server B"
        );
    }

    #[test]
    fn test_region_prior_lets_an_unmeasured_relay_win_but_yields_to_measurement() {
        let destination: Ipv4Addr = "203.0.113.10".parse().unwrap();
        let game_server = SocketAddrV4::new(destination, 27015);
        let lax: SocketAddrV4 = "10.0.1.1:4434".parse().unwrap();
        let near: SocketAddrV4 = "10.0.4.1:4434".parse().unwrap();

        let estimator = Arc::new(RelayDestinationEstimator::new());
        estimator.set_destination_region(destination, "oceania");
        // Lax is nearest to the client but has a measured, poor path to this
        // server; Sydney is farther from the client but in the server's region.
        feed_legs(&estimator, lax, destination, 5_000, 95_000); // dest 90ms

        let selector = DestinationAwareSelector::with_estimator(estimator.clone());
        let proxies = vec![
            node_in_region("sydney", "10.0.3.1:4434", 30_000, "ap-southeast-2"),
            node_in_region("lax", "10.0.1.1:4434", 5_000, "us-west"),
        ];

        // Sydney: 30ms client + 10ms intra-region prior = 40ms.
        // Lax: 5ms client + 90ms measured = 95ms.
        let result = selector.select(game_server, &proxies).unwrap();
        assert_eq!(
            result.primary.id, "sydney",
            "the region prior must let an unmeasured relay beat a worse measured one"
        );

        // A relay with a genuinely better measurement still wins.
        feed_legs(&estimator, near, destination, 1_000, 6_000); // dest 5ms
        let proxies = vec![
            node_in_region("sydney", "10.0.3.1:4434", 30_000, "ap-southeast-2"),
            node_in_region("lax", "10.0.1.1:4434", 5_000, "us-west"),
            node_in_region("near", "10.0.4.1:4434", 1_000, "oceania"),
        ];
        let result = selector.select(game_server, &proxies).unwrap();
        assert_eq!(
            result.primary.id, "near",
            "solid measurement must beat the region prior when it is genuinely closer"
        );
    }

    #[test]
    fn test_unknown_destination_region_keeps_previous_behaviour() {
        // Resolvable relay regions are not enough: with no destination region
        // there is no prior, so the measured relay wins exactly as before.
        let destination: Ipv4Addr = "203.0.113.20".parse().unwrap();
        let game_server = SocketAddrV4::new(destination, 27015);
        let near_client: SocketAddrV4 = "10.0.1.1:4434".parse().unwrap();
        let near_server: SocketAddrV4 = "10.0.2.1:4434".parse().unwrap();
        let estimator = Arc::new(RelayDestinationEstimator::new());
        feed_legs(&estimator, near_client, destination, 10_000, 100_000);
        feed_legs(&estimator, near_server, destination, 40_000, 50_000);
        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = vec![
            node_in_region("near-client", "10.0.1.1:4434", 10_000, "us-east"),
            node_in_region("near-server", "10.0.2.1:4434", 40_000, "us-east"),
        ];
        let result = selector.select(game_server, &proxies).unwrap();
        assert_eq!(result.primary.id, "near-server");

        // With no measurement and no resolvable relay region, the nearest
        // selector is used unchanged even when the destination region is known.
        let empty = Arc::new(RelayDestinationEstimator::new());
        empty.set_destination_region(destination, "oceania");
        let selector = DestinationAwareSelector::with_estimator(empty);
        let proxies = vec![
            node("a", "10.0.1.1:4434", 10_000),
            node("b", "10.0.2.1:4434", 40_000),
        ];
        let result = selector.select(game_server, &proxies).unwrap();
        assert_eq!(result.primary.id, "a");
        assert_eq!(result.strategy, RouteStrategy::Nearest);
    }

    #[test]
    fn test_worked_case_scores_match_the_expected_penalties() {
        assert_eq!(measured_penalty_us(20_000), 20_000);
        assert_eq!(measured_penalty_us(2_000), 2_000);
        assert_eq!(measured_penalty_us(500_000), MEASURED_UNCERTAINTY_CAP_US);
        // measured(30ms, u=20ms) scores 50ms; measured(35ms, u=2ms) scores 37ms.
        assert_eq!(
            risk_adjusted_score(0, 30_000, measured_penalty_us(20_000)),
            50_000
        );
        assert_eq!(
            risk_adjusted_score(0, 35_000, measured_penalty_us(2_000)),
            37_000
        );
        // prior(10ms) scores 25ms; measured(30ms, u=2ms) scores 32ms.
        assert_eq!(risk_adjusted_score(0, 10_000, PRIOR_UNCERTAINTY_US), 25_000);
        assert_eq!(
            risk_adjusted_score(0, 30_000, measured_penalty_us(2_000)),
            32_000
        );
        assert!(Provenance::Measured < Provenance::Borrowed);
        assert!(Provenance::Borrowed < Provenance::Prior);
    }

    #[test]
    fn test_worked_case_solid_measurement_beats_noisy() {
        let destination: Ipv4Addr = "1.2.3.4".parse().unwrap();
        let estimator = Arc::new(RelayDestinationEstimator::new());
        // Noisy: client 10ms, dest 20ms, u=20ms -> point 30ms, score 50ms.
        feed_varied(
            &estimator,
            "10.0.1.1:4434".parse().unwrap(),
            destination,
            10_000,
            &[
                10_000, 10_000, 10_000, 30_000, 30_000, 50_000, 50_000, 50_000,
            ],
        );
        // Solid: client 10ms, dest 25ms, u=2ms -> point 35ms, score 37ms.
        feed_varied(
            &estimator,
            "10.0.2.1:4434".parse().unwrap(),
            destination,
            10_000,
            &[
                33_000, 33_000, 33_000, 35_000, 35_000, 37_000, 37_000, 37_000,
            ],
        );

        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = vec![
            node("noisy", "10.0.1.1:4434", 10_000),
            node("solid", "10.0.2.1:4434", 10_000),
        ];

        let result = selector
            .select(SocketAddrV4::new(destination, 27015), &proxies)
            .unwrap();
        assert_eq!(
            result.primary.id, "solid",
            "a tighter measured estimate must beat a lower-point but noisy one"
        );
        assert_eq!(result.backups[0].id, "noisy");
    }

    #[test]
    fn test_worked_case_prior_stays_eligible() {
        let destination: Ipv4Addr = "203.0.113.10".parse().unwrap();
        let game_server = SocketAddrV4::new(destination, 27015);
        let estimator = Arc::new(RelayDestinationEstimator::new());
        estimator.set_destination_region(destination, "oceania");
        // Measured: client 10ms, dest 20ms, u=2ms -> score 32ms.
        feed_varied(
            &estimator,
            "10.0.1.1:4434".parse().unwrap(),
            destination,
            10_000,
            &[
                28_000, 28_000, 28_000, 30_000, 30_000, 32_000, 32_000, 32_000,
            ],
        );

        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = vec![
            node_in_region("sydney", "10.0.3.1:4434", 0, "ap-southeast-2"),
            node_in_region("near", "10.0.1.1:4434", 10_000, "us-west"),
        ];

        let result = selector.select(game_server, &proxies).unwrap();
        assert_eq!(
            result.primary.id, "sydney",
            "a genuinely closer unmeasured relay must not be frozen out by its prior"
        );
    }

    #[test]
    fn test_noisy_measurement_cannot_freeze_out_the_region_prior() {
        let destination: Ipv4Addr = "203.0.113.10".parse().unwrap();
        let game_server = SocketAddrV4::new(destination, 27015);
        let estimator = Arc::new(RelayDestinationEstimator::new());
        estimator.set_destination_region(destination, "oceania");
        // Lax: point 35ms but u=25ms -> score 60ms. Sydney prior: point 40ms,
        // score 55ms. The noisy alternative must not bury the prior.
        feed_varied(
            &estimator,
            "10.0.1.1:4434".parse().unwrap(),
            destination,
            5_000,
            &[5_000, 5_000, 5_000, 35_000, 35_000, 55_000, 55_000, 55_000],
        );

        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = vec![
            node_in_region("sydney", "10.0.3.1:4434", 30_000, "ap-southeast-2"),
            node_in_region("lax", "10.0.1.1:4434", 5_000, "us-west"),
        ];

        let result = selector.select(game_server, &proxies).unwrap();
        assert_eq!(
            result.primary.id, "sydney",
            "a coarse but stable prior must outrank a noisier measured path"
        );
    }

    #[test]
    fn test_prior_within_tie_band_loses_to_measured() {
        let destination: Ipv4Addr = "203.0.113.10".parse().unwrap();
        let game_server = SocketAddrV4::new(destination, 27015);
        let estimator = Arc::new(RelayDestinationEstimator::new());
        estimator.set_destination_region(destination, "oceania");
        // Lax measured: client 5ms, dest 15ms, u=5ms -> score 25ms.
        feed_varied(
            &estimator,
            "10.0.1.1:4434".parse().unwrap(),
            destination,
            5_000,
            &[
                15_000, 15_000, 15_000, 20_000, 20_000, 25_000, 25_000, 25_000,
            ],
        );

        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies = vec![
            node_in_region("sydney", "10.0.3.1:4434", 0, "ap-southeast-2"),
            node_in_region("lax", "10.0.1.1:4434", 5_000, "us-west"),
        ];

        let result = selector.select(game_server, &proxies).unwrap();
        assert_eq!(
            result.primary.id, "lax",
            "measured evidence must beat a prior of the same nominal score"
        );
    }

    #[test]
    fn test_destination_aware_all_unhealthy() {
        let estimator = Arc::new(RelayDestinationEstimator::new());
        let selector = DestinationAwareSelector::with_estimator(estimator);
        let proxies: Vec<ProxyNode> = make_test_proxies()
            .into_iter()
            .map(|mut p| {
                p.health = ProxyHealth::Unhealthy;
                p
            })
            .collect();
        assert!(selector
            .select("1.2.3.4:27015".parse().unwrap(), &proxies)
            .is_err());
    }
}
