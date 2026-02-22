//! # Training Data Pipeline
//!
//! Defines the data schema for route metrics and provides a synthetic
//! data generator for training when real proxy data isn't available yet.
//!
//! ## Data Flow
//! ```text
//! Live Probes / Synthetic → TrainingSample → Dataset → linfa training
//! ```

use super::features::NetworkFeatures;
use rand::Rng;

/// A single training sample: features + observed latency outcome.
#[derive(Debug, Clone)]
pub struct TrainingSample {
    /// Input features describing network conditions.
    pub features: NetworkFeatures,
    /// The proxy region this sample is for.
    pub proxy_region: String,
    /// Actual observed latency through this proxy (ms) — the label.
    pub observed_latency_ms: f64,
}

/// Configuration for synthetic data generation.
#[derive(Debug, Clone)]
pub struct SyntheticConfig {
    /// Number of samples to generate.
    pub num_samples: usize,
    /// Proxy regions to simulate.
    pub regions: Vec<RegionProfile>,
    /// Random seed for reproducibility.
    pub seed: u64,
}

/// Profile describing a proxy region's typical network characteristics.
#[derive(Debug, Clone)]
pub struct RegionProfile {
    /// Region identifier (e.g., "us-east").
    pub region: String,
    /// Base latency in ms (ideal conditions).
    pub base_latency_ms: f64,
    /// Latency variance (standard deviation).
    pub latency_std_ms: f64,
    /// Typical hop count range.
    pub hop_range: (u32, u32),
    /// Typical AS path length range.
    pub as_path_range: (u32, u32),
    /// Geographic distance in km.
    pub distance_km: f64,
    /// Peak hour latency multiplier (1.0 = no change).
    pub peak_multiplier: f64,
    /// Base packet loss percentage.
    pub base_loss_pct: f64,
}

impl Default for SyntheticConfig {
    fn default() -> Self {
        Self {
            num_samples: 10_000,
            seed: 42,
            regions: vec![
                // US-East (Ashburn) — good for NA players
                RegionProfile {
                    region: "us-east".into(),
                    base_latency_ms: 25.0,
                    latency_std_ms: 8.0,
                    hop_range: (8, 14),
                    as_path_range: (3, 6),
                    distance_km: 500.0,
                    peak_multiplier: 1.3,
                    base_loss_pct: 0.02,
                },
                // EU-West (Frankfurt) — good for EU players
                RegionProfile {
                    region: "eu-west".into(),
                    base_latency_ms: 30.0,
                    latency_std_ms: 10.0,
                    hop_range: (10, 16),
                    as_path_range: (4, 7),
                    distance_km: 6000.0,
                    peak_multiplier: 1.4,
                    base_loss_pct: 0.03,
                },
                // Asia-SE (Singapore) — good for SEA players
                RegionProfile {
                    region: "asia-se".into(),
                    base_latency_ms: 45.0,
                    latency_std_ms: 15.0,
                    hop_range: (12, 20),
                    as_path_range: (5, 9),
                    distance_km: 12000.0,
                    peak_multiplier: 1.5,
                    base_loss_pct: 0.05,
                },
            ],
        }
    }
}

/// Generate synthetic training data simulating realistic network conditions.
///
/// The generator models:
/// - Time-of-day effects (peak hours = higher latency)
/// - Day-of-week patterns (weekends slightly higher)
/// - Load-dependent latency (higher load = more latency)
/// - Distance correlation (farther = higher base latency)
/// - Packet loss impact on effective latency
/// - Jitter as a function of congestion
pub fn generate_synthetic_data(config: &SyntheticConfig) -> Vec<TrainingSample> {
    let mut rng = rand::rngs::StdRng::seed_from_u64(config.seed);
    let mut samples = Vec::with_capacity(config.num_samples * config.regions.len());

    let samples_per_region = config.num_samples / config.regions.len();

    for region in &config.regions {
        for _ in 0..samples_per_region {
            let sample = generate_sample(&mut rng, region);
            samples.push(sample);
        }
    }

    samples
}

fn generate_sample(rng: &mut impl Rng, profile: &RegionProfile) -> TrainingSample {
    // Time context
    let time_of_day: u8 = rng.gen_range(0..24);
    let day_of_week: u8 = rng.gen_range(0..7);

    // Peak hour factor: higher latency during evening gaming hours (18-23)
    let peak_factor = if (18..=23).contains(&time_of_day) {
        profile.peak_multiplier
    } else if (12..=17).contains(&time_of_day) {
        1.0 + (profile.peak_multiplier - 1.0) * 0.5
    } else {
        1.0
    };

    // Weekend factor: slightly higher baseline
    let weekend_factor = if day_of_week >= 5 { 1.1 } else { 1.0 };

    // Proxy load (0.0 - 1.0), higher during peak
    let base_load: f64 = rng.gen_range(0.05..0.4);
    let proxy_load = (base_load * peak_factor * weekend_factor).min(0.95);

    // Load-dependent latency increase
    let load_penalty = if proxy_load > 0.7 {
        (proxy_load - 0.7) * 50.0 // Heavy penalty above 70% load
    } else if proxy_load > 0.5 {
        (proxy_load - 0.5) * 15.0
    } else {
        0.0
    };

    // Network hops and AS path
    let hop_count: u32 = rng.gen_range(profile.hop_range.0..=profile.hop_range.1);
    let bgp_as_path_len: u32 = rng.gen_range(profile.as_path_range.0..=profile.as_path_range.1);

    // Packet loss (increases with congestion)
    let packet_loss_pct = profile.base_loss_pct * peak_factor
        + if proxy_load > 0.8 {
            rng.gen_range(0.01..0.05)
        } else {
            0.0
        };

    // Current latency: base + noise + peak + load + loss retransmission
    let noise: f64 = rng.gen_range(-1.0..1.0) * profile.latency_std_ms;
    let loss_retransmission = packet_loss_pct * 100.0; // Each % loss adds ~100ms equivalent
    let current_latency_ms = (profile.base_latency_ms
        + noise
        + (peak_factor - 1.0) * profile.base_latency_ms
        + load_penalty
        + loss_retransmission)
        .max(5.0); // Floor at 5ms

    // Jitter: correlates with congestion and loss
    let jitter_ms = (profile.latency_std_ms * 0.5 * peak_factor
        + rng.gen_range(0.0..3.0)
        + load_penalty * 0.3)
        .max(0.5);

    // Historical stats (smoothed versions of current)
    let historical_p50_ms = profile.base_latency_ms * 1.1 + rng.gen_range(-2.0..2.0);
    let historical_p95_ms = profile.base_latency_ms * 1.8 + rng.gen_range(-3.0..5.0);

    // Geographic distance with small noise
    let geographic_distance_km = profile.distance_km + rng.gen_range(-50.0..50.0);

    let features = NetworkFeatures {
        current_latency_ms,
        historical_p50_ms,
        historical_p95_ms,
        jitter_ms,
        hop_count,
        time_of_day,
        day_of_week,
        bgp_as_path_len,
        packet_loss_pct,
        proxy_load,
        geographic_distance_km,
    };

    // The "true" latency through this proxy — what we're trying to predict.
    // It's the current latency plus some additional real-world noise.
    let observed_latency_ms =
        current_latency_ms + rng.gen_range(-2.0..3.0) + jitter_ms * rng.gen_range(0.0..0.5);

    TrainingSample {
        features,
        proxy_region: profile.region.clone(),
        observed_latency_ms: observed_latency_ms.max(3.0),
    }
}

// Need this import for seed_from_u64
use rand::SeedableRng;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_synthetic_generation() {
        let config = SyntheticConfig {
            num_samples: 300,
            seed: 42,
            ..Default::default()
        };
        let data = generate_synthetic_data(&config);

        // Should produce samples_per_region * num_regions
        assert_eq!(data.len(), 300); // 100 per region × 3 regions

        // All latencies should be positive
        for sample in &data {
            assert!(sample.observed_latency_ms > 0.0);
            assert!(sample.features.current_latency_ms > 0.0);
            assert!(sample.features.proxy_load >= 0.0 && sample.features.proxy_load <= 1.0);
        }
    }

    #[test]
    fn test_region_coverage() {
        let config = SyntheticConfig::default();
        let data = generate_synthetic_data(&config);

        let regions: std::collections::HashSet<_> =
            data.iter().map(|s| s.proxy_region.as_str()).collect();
        assert!(regions.contains("us-east"));
        assert!(regions.contains("eu-west"));
        assert!(regions.contains("asia-se"));
    }

    #[test]
    fn test_peak_hours_higher_latency() {
        let config = SyntheticConfig {
            num_samples: 3000,
            seed: 123,
            ..Default::default()
        };
        let data = generate_synthetic_data(&config);

        let peak: Vec<_> = data
            .iter()
            .filter(|s| s.features.time_of_day >= 18 && s.features.time_of_day <= 23)
            .collect();
        let offpeak: Vec<_> = data
            .iter()
            .filter(|s| s.features.time_of_day < 12)
            .collect();

        let avg_peak: f64 =
            peak.iter().map(|s| s.observed_latency_ms).sum::<f64>() / peak.len() as f64;
        let avg_offpeak: f64 =
            offpeak.iter().map(|s| s.observed_latency_ms).sum::<f64>() / offpeak.len() as f64;

        // Peak hours should have higher average latency
        assert!(
            avg_peak > avg_offpeak,
            "Peak ({:.1}ms) should be > off-peak ({:.1}ms)",
            avg_peak,
            avg_offpeak
        );
    }
}
