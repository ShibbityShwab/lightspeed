//! # Live Data Collector
//!
//! Records real-world route measurements during game sessions, building
//! a training dataset from live traffic that replaces/supplements synthetic data.
//!
//! ## How It Works
//!
//! 1. During a game session, each proxy probe or tunnel packet measures RTT
//! 2. `RouteCollector` records these as `TrainingSample` structs
//! 3. Samples are stored in memory with exponential decay weighting
//! 4. When `min_samples` threshold is reached, retraining can be triggered
//! 5. Data persists to disk in JSON for cross-session learning
//!
//! ## Usage
//!
//! ```no_run
//! use lightspeed_client::ml::collector::RouteCollector;
//!
//! let mut collector = RouteCollector::new(100);
//! // ... record measurements during game session ...
//! collector.record_measurement("proxy-lax", 204.8, 0.3, 0.0, 0.2);
//! if collector.should_retrain() {
//!     let samples = collector.training_samples();
//!     // retrain model with samples
//! }
//! ```

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use super::data::TrainingSample;
use super::features::{LatencyTracker, NetworkFeatures};

/// A single recorded measurement from live traffic.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct LiveMeasurement {
    /// Proxy identifier (e.g., "proxy-lax", "relay-sgp").
    pub proxy_id: String,
    /// Proxy region (e.g., "us-west-lax", "asia-sgp").
    pub proxy_region: String,
    /// Measured round-trip latency in milliseconds.
    pub latency_ms: f64,
    /// Measured jitter in milliseconds.
    pub jitter_ms: f64,
    /// Measured packet loss percentage (0.0 - 1.0).
    pub packet_loss_pct: f64,
    /// Proxy load at time of measurement (0.0 - 1.0).
    pub proxy_load: f64,
    /// Timestamp (Unix epoch seconds).
    pub timestamp: u64,
    /// Hour of day when measured (0-23).
    pub time_of_day: u8,
    /// Day of week when measured (0-6, Mon=0).
    pub day_of_week: u8,
}

/// Collects live route measurements and manages training data.
pub struct RouteCollector {
    /// All measurements, indexed by proxy_id.
    measurements: HashMap<String, Vec<LiveMeasurement>>,
    /// Per-proxy latency trackers for rolling statistics.
    trackers: HashMap<String, LatencyTracker>,
    /// Maximum measurements to keep per proxy.
    max_per_proxy: usize,
    /// Minimum total samples before retraining is worthwhile.
    min_samples_for_retrain: usize,
    /// Samples recorded since last retrain.
    samples_since_retrain: usize,
    /// Exponential decay factor (0.0-1.0). Higher = more weight on recent data.
    decay_factor: f64,
    /// Total measurements recorded (lifetime).
    total_recorded: u64,
}

impl RouteCollector {
    /// Create a new collector.
    ///
    /// `max_per_proxy`: maximum measurements to keep per proxy (older ones are dropped).
    pub fn new(max_per_proxy: usize) -> Self {
        Self {
            measurements: HashMap::new(),
            trackers: HashMap::new(),
            max_per_proxy,
            min_samples_for_retrain: 50,
            samples_since_retrain: 0,
            decay_factor: 0.95,
            total_recorded: 0,
        }
    }

    /// Set the minimum samples threshold before retraining is recommended.
    pub fn with_retrain_threshold(mut self, threshold: usize) -> Self {
        self.min_samples_for_retrain = threshold;
        self
    }

    /// Set the exponential decay factor for weighting samples.
    pub fn with_decay(mut self, decay: f64) -> Self {
        self.decay_factor = decay.clamp(0.5, 1.0);
        self
    }

    /// Record a new live measurement for a proxy.
    pub fn record_measurement(
        &mut self,
        proxy_id: &str,
        proxy_region: &str,
        latency_ms: f64,
        jitter_ms: f64,
        packet_loss_pct: f64,
        proxy_load: f64,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let timestamp = now.as_secs();

        let now_chrono = chrono::Local::now();
        let time_of_day: u8 = now_chrono.format("%H").to_string().parse().unwrap_or(12);
        let day_of_week: u8 = now_chrono.format("%u").to_string().parse::<u8>().unwrap_or(1) - 1;

        let measurement = LiveMeasurement {
            proxy_id: proxy_id.to_string(),
            proxy_region: proxy_region.to_string(),
            latency_ms,
            jitter_ms,
            packet_loss_pct,
            proxy_load,
            timestamp,
            time_of_day,
            day_of_week,
        };

        // Update latency tracker
        let tracker = self.trackers
            .entry(proxy_id.to_string())
            .or_insert_with(|| LatencyTracker::new(200));
        tracker.record(latency_ms);
        tracker.record_send(); // For loss tracking

        // Store measurement
        let proxy_measurements = self.measurements
            .entry(proxy_id.to_string())
            .or_insert_with(Vec::new);

        proxy_measurements.push(measurement);

        // Evict oldest if over capacity
        if proxy_measurements.len() > self.max_per_proxy {
            proxy_measurements.remove(0);
        }

        self.samples_since_retrain += 1;
        self.total_recorded += 1;

        tracing::trace!(
            proxy = proxy_id,
            latency_ms = latency_ms,
            total = self.total_recorded,
            "Recorded live measurement"
        );
    }

    /// Record a latency probe result (simpler interface for keepalive probes).
    pub fn record_probe(&mut self, proxy_id: &str, proxy_region: &str, rtt_us: u64) {
        let latency_ms = rtt_us as f64 / 1000.0;
        self.record_measurement(proxy_id, proxy_region, latency_ms, 0.0, 0.0, 0.0);
    }

    /// Check if we have enough new data to justify retraining.
    pub fn should_retrain(&self) -> bool {
        self.samples_since_retrain >= self.min_samples_for_retrain
    }

    /// Reset the retrain counter (call after retraining).
    pub fn mark_retrained(&mut self) {
        self.samples_since_retrain = 0;
    }

    /// Get total measurements recorded.
    pub fn total_recorded(&self) -> u64 {
        self.total_recorded
    }

    /// Get measurements count per proxy.
    pub fn per_proxy_counts(&self) -> HashMap<String, usize> {
        self.measurements.iter()
            .map(|(k, v)| (k.clone(), v.len()))
            .collect()
    }

    /// Convert all stored measurements into `TrainingSample` format for training.
    ///
    /// Applies exponential decay weighting — recent samples appear more times
    /// in the output, effectively giving them more weight during training.
    pub fn training_samples(&self) -> Vec<TrainingSample> {
        let mut samples = Vec::new();

        for (proxy_id, measurements) in &self.measurements {
            let tracker = self.trackers.get(proxy_id);
            let n = measurements.len();

            for (i, m) in measurements.iter().enumerate() {
                // Exponential decay: weight = decay^(n - i - 1)
                // More recent samples (higher i) get weight closer to 1.0
                let age = (n - i - 1) as f64;
                let weight = self.decay_factor.powf(age);

                // Include sample if weight is above threshold (0.1)
                if weight < 0.1 {
                    continue;
                }

                let features = NetworkFeatures {
                    current_latency_ms: m.latency_ms,
                    historical_p50_ms: tracker.map(|t| t.p50()).unwrap_or(m.latency_ms),
                    historical_p95_ms: tracker.map(|t| t.p95()).unwrap_or(m.latency_ms * 1.5),
                    jitter_ms: m.jitter_ms,
                    hop_count: 0,  // Not available from live probes
                    time_of_day: m.time_of_day,
                    day_of_week: m.day_of_week,
                    bgp_as_path_len: 0,  // Not available from live probes
                    packet_loss_pct: m.packet_loss_pct,
                    proxy_load: m.proxy_load,
                    geographic_distance_km: 0.0,  // Could be estimated from region
                };

                let sample = TrainingSample {
                    features,
                    proxy_region: m.proxy_region.clone(),
                    observed_latency_ms: m.latency_ms,
                };

                // For high-weight samples, include multiple times (effective weighting)
                let repeats = (weight * 3.0).ceil() as usize;
                for _ in 0..repeats.max(1) {
                    samples.push(sample.clone());
                }
            }
        }

        samples
    }

    /// Get the latency tracker for a specific proxy.
    pub fn tracker(&self, proxy_id: &str) -> Option<&LatencyTracker> {
        self.trackers.get(proxy_id)
    }

    /// Save collected measurements to a JSON file for persistence.
    pub fn save_to_file(&self, path: &str) -> Result<(), std::io::Error> {
        let all_measurements: Vec<&LiveMeasurement> = self.measurements
            .values()
            .flat_map(|v| v.iter())
            .collect();

        let json = serde_json::to_string_pretty(&all_measurements)?;

        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, json)?;
        tracing::info!(
            "Saved {} live measurements to {}",
            all_measurements.len(),
            path
        );
        Ok(())
    }

    /// Load measurements from a JSON file (from previous sessions).
    pub fn load_from_file(&mut self, path: &str) -> Result<usize, std::io::Error> {
        let json = std::fs::read_to_string(path)?;
        let measurements: Vec<LiveMeasurement> = serde_json::from_str(&json)?;
        let count = measurements.len();

        for m in measurements {
            // Update tracker
            let tracker = self.trackers
                .entry(m.proxy_id.clone())
                .or_insert_with(|| LatencyTracker::new(200));
            tracker.record(m.latency_ms);

            // Store measurement
            let proxy_measurements = self.measurements
                .entry(m.proxy_id.clone())
                .or_insert_with(Vec::new);
            proxy_measurements.push(m);
        }

        tracing::info!("Loaded {} live measurements from {}", count, path);
        Ok(count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_collection() {
        let mut collector = RouteCollector::new(100);
        collector.record_measurement("proxy-lax", "us-west-lax", 204.8, 0.3, 0.0, 0.2);
        collector.record_measurement("proxy-lax", "us-west-lax", 205.1, 0.4, 0.0, 0.2);
        collector.record_measurement("relay-sgp", "asia-sgp", 34.0, 0.3, 0.0, 0.1);

        assert_eq!(collector.total_recorded(), 3);
        let counts = collector.per_proxy_counts();
        assert_eq!(counts["proxy-lax"], 2);
        assert_eq!(counts["relay-sgp"], 1);
    }

    #[test]
    fn test_retrain_threshold() {
        let mut collector = RouteCollector::new(1000).with_retrain_threshold(5);
        assert!(!collector.should_retrain());

        for i in 0..5 {
            collector.record_measurement("proxy-lax", "us-west-lax", 200.0 + i as f64, 0.3, 0.0, 0.2);
        }
        assert!(collector.should_retrain());

        collector.mark_retrained();
        assert!(!collector.should_retrain());
    }

    #[test]
    fn test_training_samples_generation() {
        let mut collector = RouteCollector::new(100);
        for i in 0..10 {
            collector.record_measurement(
                "proxy-lax", "us-west-lax",
                200.0 + i as f64, 0.3, 0.0, 0.2,
            );
        }

        let samples = collector.training_samples();
        assert!(!samples.is_empty());
        // All samples should have the proxy region
        for s in &samples {
            assert_eq!(s.proxy_region, "us-west-lax");
            assert!(s.observed_latency_ms >= 200.0);
        }
    }

    #[test]
    fn test_eviction() {
        let mut collector = RouteCollector::new(5);
        for i in 0..10 {
            collector.record_measurement(
                "proxy-lax", "us-west-lax",
                200.0 + i as f64, 0.3, 0.0, 0.2,
            );
        }

        let counts = collector.per_proxy_counts();
        assert_eq!(counts["proxy-lax"], 5); // Should only keep 5
    }

    #[test]
    fn test_probe_shorthand() {
        let mut collector = RouteCollector::new(100);
        collector.record_probe("proxy-lax", "us-west-lax", 204_800); // 204.8ms in microseconds
        assert_eq!(collector.total_recorded(), 1);
    }
}
