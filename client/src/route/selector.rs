//! # Route Selector Implementations
//!
//! Concrete route selection strategies:
//! - **NearestSelector** — picks proxy with lowest measured latency (MVP default)
//! - **MlSelector** — AI-powered route prediction using linfa model

use std::collections::HashMap;
use std::net::SocketAddrV4;

use crate::error::RouteError;
use crate::ml::features::{extract_features, LatencyTracker, NetworkFeatures};
use crate::ml::RouteModel;
use super::{ProxyHealth, ProxyNode, RouteStrategy, SelectedRoute, RouteSelector};

// ── Nearest Selector (MVP Default) ──────────────────────────

/// Nearest-proxy selector — picks the proxy with lowest measured latency.
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
        // NearestSelector doesn't learn — ML selector does
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
                let meta = self
                    .proxy_meta
                    .get(&proxy.id)
                    .cloned()
                    .unwrap_or_default();

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
                    // No historical data yet — use proxy's reported latency
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
            tracing::warn!("ML model not loaded — falling back to nearest selector");
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
}
