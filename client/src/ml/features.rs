//! # Feature Extraction
//!
//! Extracts ML features from network metrics for route prediction.
//! Includes a LatencyTracker for computing historical statistics
//! and a feature extractor that builds NetworkFeatures from live data.

use std::collections::VecDeque;

/// Network features used as input to the ML model.
/// 11 features total — must match FEATURE_COUNT.
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
    pub fn to_array(&self) -> [f64; Self::FEATURE_COUNT] {
        [
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

/// Tracks latency measurements for a single proxy, computing rolling
/// statistics (p50, p95, jitter) from a sliding window.
#[derive(Debug, Clone)]
pub struct LatencyTracker {
    /// Sliding window of recent latency measurements (ms).
    window: VecDeque<f64>,
    /// Maximum window size.
    max_size: usize,
    /// Running sum for fast mean calculation.
    running_sum: f64,
    /// Last measurement for jitter calculation.
    last_value: Option<f64>,
    /// Exponential moving average of jitter.
    ema_jitter: f64,
    /// Packet send count (for loss tracking).
    packets_sent: u64,
    /// Packet recv count (for loss tracking).
    packets_recv: u64,
}

impl LatencyTracker {
    /// Create a new tracker with the given window size.
    pub fn new(window_size: usize) -> Self {
        Self {
            window: VecDeque::with_capacity(window_size),
            max_size: window_size,
            running_sum: 0.0,
            last_value: None,
            ema_jitter: 0.0,
            packets_sent: 0,
            packets_recv: 0,
        }
    }

    /// Record a new latency measurement.
    pub fn record(&mut self, latency_ms: f64) {
        // Update jitter (RFC 3550 style: EMA of absolute differences)
        if let Some(last) = self.last_value {
            let diff = (latency_ms - last).abs();
            self.ema_jitter = self.ema_jitter + (diff - self.ema_jitter) / 16.0;
        }
        self.last_value = Some(latency_ms);

        // Update sliding window
        if self.window.len() >= self.max_size {
            if let Some(removed) = self.window.pop_front() {
                self.running_sum -= removed;
            }
        }
        self.window.push_back(latency_ms);
        self.running_sum += latency_ms;

        self.packets_recv += 1;
    }

    /// Record a sent packet (for loss tracking).
    pub fn record_send(&mut self) {
        self.packets_sent += 1;
    }

    /// Get the current latency (most recent measurement).
    pub fn current(&self) -> Option<f64> {
        self.window.back().copied()
    }

    /// Compute the mean latency.
    pub fn mean(&self) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        self.running_sum / self.window.len() as f64
    }

    /// Compute the p-th percentile (0-100).
    pub fn percentile(&self, p: f64) -> f64 {
        if self.window.is_empty() {
            return 0.0;
        }
        let mut sorted: Vec<f64> = self.window.iter().copied().collect();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let idx = ((p / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// Get the p50 (median) latency.
    pub fn p50(&self) -> f64 {
        self.percentile(50.0)
    }

    /// Get the p95 latency.
    pub fn p95(&self) -> f64 {
        self.percentile(95.0)
    }

    /// Get the current jitter (EMA of inter-packet latency differences).
    pub fn jitter(&self) -> f64 {
        self.ema_jitter
    }

    /// Get the packet loss percentage.
    pub fn packet_loss_pct(&self) -> f64 {
        if self.packets_sent == 0 {
            return 0.0;
        }
        let lost = self.packets_sent.saturating_sub(self.packets_recv);
        lost as f64 / self.packets_sent as f64
    }

    /// Number of samples in the window.
    pub fn sample_count(&self) -> usize {
        self.window.len()
    }

    /// Whether we have enough data for reliable statistics.
    pub fn is_ready(&self) -> bool {
        self.window.len() >= 10
    }
}

/// Extract NetworkFeatures from a proxy's current state.
///
/// This is the bridge between live network metrics and ML input.
pub fn extract_features(
    tracker: &LatencyTracker,
    hop_count: u32,
    bgp_as_path_len: u32,
    proxy_load: f64,
    geographic_distance_km: f64,
) -> NetworkFeatures {
    let now = chrono::Local::now();

    NetworkFeatures {
        current_latency_ms: tracker.current().unwrap_or(0.0),
        historical_p50_ms: tracker.p50(),
        historical_p95_ms: tracker.p95(),
        jitter_ms: tracker.jitter(),
        hop_count,
        time_of_day: now.format("%H").to_string().parse().unwrap_or(12),
        day_of_week: now.format("%u").to_string().parse::<u8>().unwrap_or(1) - 1, // chrono: Mon=1
        bgp_as_path_len,
        packet_loss_pct: tracker.packet_loss_pct(),
        proxy_load,
        geographic_distance_km,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_features_to_array() {
        let f = NetworkFeatures {
            current_latency_ms: 25.0,
            historical_p50_ms: 28.0,
            historical_p95_ms: 45.0,
            jitter_ms: 3.0,
            hop_count: 12,
            time_of_day: 20,
            day_of_week: 3,
            bgp_as_path_len: 5,
            packet_loss_pct: 0.01,
            proxy_load: 0.35,
            geographic_distance_km: 500.0,
        };
        let arr = f.to_array();
        assert_eq!(arr.len(), NetworkFeatures::FEATURE_COUNT);
        assert_eq!(arr[0], 25.0);
        assert_eq!(arr[4], 12.0);
    }

    #[test]
    fn test_latency_tracker_basic() {
        let mut t = LatencyTracker::new(100);
        for i in 1..=100 {
            t.record(i as f64);
        }
        assert_eq!(t.sample_count(), 100);
        assert_eq!(t.current(), Some(100.0));
        assert!((t.mean() - 50.5).abs() < 0.01);
        assert!(t.p50() >= 49.0 && t.p50() <= 51.0);
        assert!(t.p95() >= 94.0 && t.p95() <= 96.0);
    }

    #[test]
    fn test_latency_tracker_sliding_window() {
        let mut t = LatencyTracker::new(10);
        for i in 1..=20 {
            t.record(i as f64);
        }
        // Window should only contain last 10 values (11-20)
        assert_eq!(t.sample_count(), 10);
        assert_eq!(t.current(), Some(20.0));
        assert!((t.mean() - 15.5).abs() < 0.01);
    }

    #[test]
    fn test_jitter_calculation() {
        let mut t = LatencyTracker::new(100);
        // Constant latency = zero jitter
        for _ in 0..50 {
            t.record(20.0);
        }
        assert!(t.jitter() < 0.1);

        // Variable latency = non-zero jitter
        let mut t2 = LatencyTracker::new(100);
        for i in 0..50 {
            let val = if i % 2 == 0 { 20.0 } else { 40.0 };
            t2.record(val);
        }
        assert!(t2.jitter() > 5.0);
    }

    #[test]
    fn test_packet_loss() {
        let mut t = LatencyTracker::new(100);
        for _ in 0..100 {
            t.record_send();
        }
        for _ in 0..95 {
            t.record(20.0); // Only 95 received
        }
        assert!((t.packet_loss_pct() - 0.05).abs() < 0.001);
    }
}
