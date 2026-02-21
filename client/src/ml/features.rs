//! # Feature Extraction
//!
//! Extracts ML features from network metrics for route prediction.

/// Network features used as input to the ML model.
#[derive(Debug, Clone, Default)]
pub struct NetworkFeatures {
    /// Current measured latency to proxy (ms).
    pub current_latency_ms: f64,
    /// Historical p50 latency (ms).
    pub historical_p50_ms: f64,
    /// Historical p95 latency (ms).
    pub historical_p95_ms: f64,
    /// Current jitter (ms).
    pub jitter_ms: f64,
    /// Number of network hops to proxy.
    pub hop_count: u32,
    /// Hour of day (0-23).
    pub time_of_day: u8,
    /// Day of week (0-6, Monday=0).
    pub day_of_week: u8,
    /// BGP AS path length.
    pub bgp_as_path_len: u32,
    /// Recent packet loss percentage.
    pub packet_loss_pct: f64,
    /// Proxy load (0.0 - 1.0).
    pub proxy_load: f64,
    /// Geographic distance to proxy (km).
    pub geographic_distance_km: f64,
}

impl NetworkFeatures {
    /// Convert features to a flat f64 array for linfa input.
    pub fn to_array(&self) -> Vec<f64> {
        vec![
            self.current_latency_ms,
            self.historical_p50_ms,
            self.historical_p95_ms,
            self.jitter_ms,
            self.hop_count as f64,
            self.time_of_day as f64,
            self.day_of_week as f64,
            self.bgp_as_path_len as f64,
            self.packet_loss_pct,
            self.proxy_load,
            self.geographic_distance_km,
        ]
    }

    /// Number of features.
    pub const FEATURE_COUNT: usize = 11;

    /// Feature names (for debugging and model inspection).
    pub const FEATURE_NAMES: [&'static str; 11] = [
        "current_latency_ms",
        "historical_p50_ms",
        "historical_p95_ms",
        "jitter_ms",
        "hop_count",
        "time_of_day",
        "day_of_week",
        "bgp_as_path_len",
        "packet_loss_pct",
        "proxy_load",
        "geographic_distance_km",
    ];
}
