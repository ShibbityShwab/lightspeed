//! # Route Selector Implementations
//!
//! Concrete route selection strategies for the CLI's `--route-strategy` seam:
//! - **DestinationAwareSelector** - the engine's canonical whole-path ranking
//!   ([`super::relay_choice`]) behind the [`RouteSelector`] interface. There is
//!   exactly one relay ranking rule in the engine and this is its CLI face.
//! - **NearestSelector** - picks the proxy with the lowest measured client leg
//!   (the explicit opt-out of destination awareness)
//! - **MlSelector** - AI-powered route prediction using the linfa model

use std::collections::HashMap;
use std::net::SocketAddrV4;

use super::relay_choice::{self, DestLegSource, RelayCandidate};
use super::{ProxyHealth, ProxyNode, RouteSelector, RouteStrategy, SelectedRoute};
use crate::error::RouteError;
use crate::ml::features::{extract_features, LatencyTracker, NetworkFeatures};
use crate::ml::RouteModel;

/// Advisory confidence the CLI reports for a whole-path choice, decided by the
/// weakest claim the winning estimate makes.
///
/// The ranking never reads this back: the canonical key is the order, and
/// [`SelectedRoute::confidence`] is metadata for the caller's log line.
fn confidence_for(source: DestLegSource) -> f64 {
    match source {
        DestLegSource::Measured => 0.8,
        DestLegSource::GeometricFloor => 0.7,
        DestLegSource::RegionPrior => 0.5,
        DestLegSource::Unknown => 0.3,
    }
}

/// Describe one probed proxy as a candidate for the canonical ranking.
///
/// `ProxyNode::id` is the registry node id when discovery supplied the relay (the
/// CLI's registry path passes it through), so [`relay_choice`] can use the
/// operator's own relay coordinates instead of the coarse region centroid; a
/// configured or manually probed relay keeps whatever label it had, which then
/// resolves by region or not at all. The probe reports a median only, so there is
/// no spread to feed: the canonical key's jitter tie-break stays neutral and the
/// relay address is the final key.
fn relay_candidate(proxy: &ProxyNode) -> RelayCandidate {
    RelayCandidate {
        relay: proxy.data_addr,
        region: Some(proxy.region.clone()),
        node_id: Some(proxy.id.clone()),
        client_leg_us: proxy.latency_us,
        client_jitter_us: 0,
    }
}

/// Destination-aware selector: exactly the engine's canonical whole-path ranking.
///
/// The ranking rule lives in [`super::relay_choice`] and nowhere else. This type
/// is only the [`RouteSelector`] adapter the CLI's `--route-strategy` seam needs:
/// it turns the probed [`ProxyNode`] list into ranking candidates, keeps the
/// engine's order (client leg + relay-to-server estimate + that estimate's own
/// uncertainty, jitter as tie-break, address last), and maps the result back onto
/// [`SelectedRoute`].
///
/// When no candidate has any destination evidence - no measured leg and no
/// resolvable region - the strategy is exactly the nearest-leg race, and reports
/// [`RouteStrategy::Nearest`] so the reroute loop keeps its conservative switch
/// policy. That is the long-standing contract of this strategy, not a ranking
/// rule of its own.
pub struct DestinationAwareSelector;

impl DestinationAwareSelector {
    /// The canonical ranking, with no configuration of its own: the engine owns
    /// the rule and the rule reads the process-wide destination estimator.
    pub fn new() -> Self {
        Self
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
        let healthy: Vec<&ProxyNode> = available_proxies
            .iter()
            .filter(|p| p.health == ProxyHealth::Healthy)
            .collect();
        if healthy.is_empty() {
            return Err(RouteError::AllUnhealthy);
        }

        let candidates: Vec<RelayCandidate> = healthy.iter().map(|p| relay_candidate(p)).collect();
        let ranked = relay_choice::rank(*game_server.ip(), &candidates);

        // No destination evidence anywhere: the strategy is exactly the
        // nearest-leg race, and says so, so the caller's switch policy stays
        // conservative.
        if ranked
            .iter()
            .all(|scored| scored.dest_source == DestLegSource::Unknown)
        {
            return NearestSelector::new().select(game_server, available_proxies);
        }

        // The engine's order, mapped back onto the caller's nodes. A healthy
        // relay the ranking cannot score (no measured client leg) stays eligible
        // as a last backup, which is where the old rule priced it as `u64::MAX`.
        let mut ordered: Vec<ProxyNode> = Vec::with_capacity(healthy.len());
        for scored in &ranked {
            if let Some(node) = healthy.iter().find(|p| p.data_addr == scored.relay) {
                ordered.push((*node).clone());
            }
        }
        for node in &healthy {
            if !ordered.iter().any(|p| p.data_addr == node.data_addr) {
                ordered.push((*node).clone());
            }
        }
        let Some(primary) = ordered.first().cloned() else {
            return NearestSelector::new().select(game_server, available_proxies);
        };
        let confidence = ranked
            .first()
            .map_or(0.3, |scored| confidence_for(scored.dest_source));
        let backups: Vec<ProxyNode> = ordered[1..].to_vec();

        Ok(SelectedRoute {
            primary,
            backups,
            confidence,
            strategy: RouteStrategy::DestinationAware,
        })
    }

    fn feedback(&mut self, _proxy_id: &str, _observed_latency_us: u64) {
        // Samples reach the estimator through the `route::destination` observers,
        // which are keyed by relay address; this adapter has nothing of its own
        // to learn.
    }

    fn strategy(&self) -> RouteStrategy {
        RouteStrategy::DestinationAware
    }
}

// ── Nearest Selector (client-leg race) ──────────────────────

/// Nearest-proxy selector - picks the proxy with the lowest measured client leg.
///
/// This is the explicit opt-out of destination awareness: the CLI's default
/// strategy is `"destination"` (the canonical whole-path ranking), and this one
/// exists for a caller that wants the plain client-to-relay race.
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
    use crate::route::destination;
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

    /// The process-wide estimator the canonical ranking reads, with the
    /// destination region the route-ranking suites rely on.
    ///
    /// Whoever creates the process-wide estimator first must leave it usable for
    /// the tests that follow, because `install_global` is first-wins: setting the
    /// region here keeps the shared state independent of test execution order.
    fn canonical_estimator() -> std::sync::Arc<destination::RelayDestinationEstimator> {
        let estimator = destination::ensure_global();
        estimator.set_destination_region(Ipv4Addr::new(3, 237, 142, 3), "us-east");
        estimator
    }

    /// Feed the process-wide estimator a measured pair, which is how production
    /// learns a relay's destination leg: the quiet-flow probe records the same
    /// leg through this estimator.
    fn feed_global_leg(
        relay: SocketAddrV4,
        destination: Ipv4Addr,
        client_us: u64,
        tunnelled_us: u64,
    ) {
        let estimator = canonical_estimator();
        for _ in 0..destination::MIN_DEST_SAMPLES {
            estimator.observe_client_leg(relay, client_us);
            estimator.observe_tunnelled(relay, destination, tunnelled_us);
        }
    }

    fn node_in_region(id: &str, addr: &str, latency_us: u64, region: &str) -> ProxyNode {
        let mut proxy = node(id, addr, latency_us);
        proxy.region = region.into();
        proxy
    }

    #[test]
    fn test_destination_aware_prefers_better_whole_path() {
        // A relay near the game server beats a relay near the client once the
        // canonical key can see the destination leg: 100 ms whole path against
        // 40 ms + a 10 ms prior (+25 ms uncertainty), through the same estimator
        // production feeds.
        let server_ip = Ipv4Addr::new(198, 51, 100, 9);
        let server = SocketAddrV4::new(server_ip, 27015);
        canonical_estimator().set_destination_region(server_ip, "us-east");
        let near_client: SocketAddrV4 = "10.0.9.31:4434".parse().unwrap();
        let near_server: SocketAddrV4 = "10.0.9.32:4434".parse().unwrap();
        feed_global_leg(near_client, server_ip, 10_000, 100_000); // dest 90 ms
        feed_global_leg(near_server, server_ip, 40_000, 50_000); // dest 10 ms

        let proxies = vec![
            node_in_region("near-client", "10.0.9.31:4434", 10_000, "us-west"),
            node_in_region("near-server", "10.0.9.32:4434", 40_000, "us-east"),
        ];

        let selected = DestinationAwareSelector::new()
            .select(server, &proxies)
            .unwrap();
        assert_eq!(
            selected.primary.id, "near-server",
            "the relay closest to the game server must win once the destination leg is known"
        );
        assert_eq!(selected.strategy, RouteStrategy::DestinationAware);
        assert_eq!(selected.backups[0].id, "near-client");
    }

    #[test]
    fn the_cli_destination_strategy_orders_like_the_canonical_whole_path_key() {
        // The counterfactual that exposed the duplicate truth (task-23): an
        // eu-west relay with a short client leg (148.9 ms) against a us-east
        // relay with a long one (300.1 ms). Their destination priors are 161.5
        // and 10.0 ms, so the whole paths are 335.425 and 335.100 ms - the
        // canonical key prefers the US relay by 325 us. The old second rule put
        // both scores in one 1 ms tie band and then followed the client leg, so
        // it chose the EU relay. There is one ranking rule now.
        let server: SocketAddrV4 = "3.237.142.3:27015".parse().unwrap();
        canonical_estimator();
        let proxies = vec![
            node_in_region("eu", "10.0.9.1:4434", 148_900, "eu-west"),
            node_in_region("us", "10.0.9.9:4434", 300_100, "us-east"),
        ];

        let candidates: Vec<RelayCandidate> = proxies.iter().map(relay_candidate).collect();
        let canonical = relay_choice::rank(*server.ip(), &candidates);
        assert_eq!(canonical.len(), 2, "both candidates must be scoreable");
        assert_eq!(
            canonical[0].relay, proxies[1].data_addr,
            "the engine's key prefers the lower whole path"
        );
        assert!(canonical[0].total_leg_us < canonical[1].total_leg_us);

        let selected = DestinationAwareSelector::new()
            .select(server, &proxies)
            .unwrap();

        assert_eq!(selected.strategy, RouteStrategy::DestinationAware);
        assert_eq!(
            selected.primary.id, "us",
            "the whole-path total decides, not the client leg"
        );
        assert_eq!(
            selected
                .backups
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec!["eu"],
            "the backups follow the same canonical order"
        );
    }

    #[test]
    fn a_measured_destination_leg_governs_the_cli_order() {
        // A measured relay-to-server leg is the strongest evidence the ranking
        // has: a measured 5 ms beats a 10 ms region prior once the prior's 25 ms
        // uncertainty is charged, even with identical client legs.
        let server_ip = Ipv4Addr::new(203, 0, 113, 77);
        let server = SocketAddrV4::new(server_ip, 27015);
        canonical_estimator().set_destination_region(server_ip, "us-east");
        let measured: SocketAddrV4 = "10.0.9.21:4434".parse().unwrap();
        feed_global_leg(measured, server_ip, 100_000, 105_000); // dest 5 ms
        let proxies = vec![
            node_in_region("measured", "10.0.9.21:4434", 100_000, "us-west"),
            node_in_region("prior", "10.0.9.22:4434", 100_000, "us-east"),
        ];

        let candidates: Vec<RelayCandidate> = proxies.iter().map(relay_candidate).collect();
        let canonical = relay_choice::rank(server_ip, &candidates);
        assert_eq!(
            canonical[0].dest_source,
            DestLegSource::Measured,
            "the measured relay must be the one carrying the measurement"
        );
        assert_eq!(canonical[0].relay, measured);

        let selected = DestinationAwareSelector::new()
            .select(server, &proxies)
            .unwrap();
        assert_eq!(selected.primary.id, "measured");
        assert_eq!(selected.strategy, RouteStrategy::DestinationAware);
        assert_eq!(
            selected.confidence, 0.8,
            "a measurement is the strongest claim the CLI can report"
        );
    }

    #[test]
    fn without_a_destination_signal_the_strategy_falls_back_to_nearest() {
        // No destination region and no measured leg: no candidate has destination
        // evidence, so the strategy is exactly the nearest-leg race.
        let proxies = make_test_proxies();
        let selected = DestinationAwareSelector::new()
            .select("1.2.3.4:27015".parse().unwrap(), &proxies)
            .unwrap();
        assert_eq!(selected.primary.id, "proxy-us-east");
        assert_eq!(selected.strategy, RouteStrategy::Nearest);
    }

    #[test]
    fn a_healthy_relay_without_a_client_leg_stays_a_backup() {
        // The canonical ranking cannot score a relay with no measured client leg,
        // but it is healthy and the CLI has always kept it as a last backup.
        let server: SocketAddrV4 = "3.237.142.3:27015".parse().unwrap();
        canonical_estimator();
        let mut blind = node_in_region("blind", "10.0.9.40:4434", 0, "eu-west");
        blind.latency_us = None;
        let proxies = vec![
            node_in_region("us", "10.0.9.9:4434", 300_100, "us-east"),
            node_in_region("eu", "10.0.9.1:4434", 148_900, "eu-west"),
            blind,
        ];

        let selected = DestinationAwareSelector::new()
            .select(server, &proxies)
            .unwrap();
        assert_eq!(selected.primary.id, "us");
        assert_eq!(
            selected
                .backups
                .iter()
                .map(|p| p.id.as_str())
                .collect::<Vec<_>>(),
            vec!["eu", "blind"],
            "an unscoreable healthy relay is still listed, last"
        );
    }

    #[test]
    fn all_unhealthy_is_an_error() {
        let proxies: Vec<ProxyNode> = make_test_proxies()
            .into_iter()
            .map(|mut p| {
                p.health = ProxyHealth::Unhealthy;
                p
            })
            .collect();
        assert!(DestinationAwareSelector::new()
            .select("1.2.3.4:27015".parse().unwrap(), &proxies)
            .is_err());
    }
}
