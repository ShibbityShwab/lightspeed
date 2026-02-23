//! # Online Learning Manager
//!
//! Orchestrates the online learning loop: collect live data → retrain
//! model → swap in new model → continue collecting. This closes the
//! feedback loop between route selection and actual measured performance.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────┐     ┌──────────────┐     ┌───────────────┐
//! │ Live Probes/     │────▶│ RouteCollect │────▶│ RouteModel    │
//! │ Tunnel Packets   │     │ (collector)  │     │ (retrained)   │
//! └─────────────────┘     └──────────────┘     └───────────────┘
//!                                                      │
//!                          ┌──────────────┐            │
//!                          │ RouteSelector│◀───────────┘
//!                          │ (uses model) │
//!                          └──────────────┘
//! ```
//!
//! ## Data Persistence
//!
//! Measurements are saved to `~/.lightspeed/measurements.json` between sessions.
//! The trained model is saved to `~/.lightspeed/model.bin`.

use std::path::PathBuf;

use super::collector::RouteCollector;
use super::RouteModel;
use crate::error::MlError;

/// Configuration for the online learning system.
#[derive(Debug, Clone)]
pub struct OnlineLearningConfig {
    /// Minimum new samples before triggering retrain.
    pub retrain_threshold: usize,
    /// Maximum measurements to store per proxy.
    pub max_per_proxy: usize,
    /// Exponential decay factor for sample weighting.
    pub decay_factor: f64,
    /// Directory for persisting data and models.
    pub data_dir: PathBuf,
    /// Whether to mix synthetic data with live data during retraining.
    pub mix_synthetic: bool,
    /// Ratio of synthetic samples to include (0.0 = none, 1.0 = equal weight).
    pub synthetic_ratio: f64,
}

impl Default for OnlineLearningConfig {
    fn default() -> Self {
        let data_dir = dirs_data_dir().unwrap_or_else(|| PathBuf::from(".lightspeed"));
        Self {
            retrain_threshold: 50,
            max_per_proxy: 500,
            decay_factor: 0.95,
            data_dir,
            mix_synthetic: true,
            synthetic_ratio: 0.3,
        }
    }
}

/// Get the platform-appropriate data directory.
fn dirs_data_dir() -> Option<PathBuf> {
    #[cfg(target_os = "windows")]
    {
        std::env::var("APPDATA").ok().map(|p| PathBuf::from(p).join("lightspeed"))
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var("HOME").ok().map(|p| PathBuf::from(p).join(".lightspeed"))
    }
    #[cfg(target_os = "macos")]
    {
        std::env::var("HOME").ok().map(|p| PathBuf::from(p).join(".lightspeed"))
    }
    #[cfg(not(any(target_os = "windows", target_os = "linux", target_os = "macos")))]
    {
        None
    }
}

/// Manages the online learning lifecycle.
///
/// Coordinates between the data collector, model trainer, and route
/// selector to continuously improve route predictions from live data.
pub struct OnlineLearner {
    /// Live data collector.
    pub collector: RouteCollector,
    /// Current ML model.
    pub model: RouteModel,
    /// Configuration.
    config: OnlineLearningConfig,
    /// Number of times the model has been retrained.
    retrain_count: u32,
    /// Whether the system is initialized.
    initialized: bool,
}

impl OnlineLearner {
    /// Create a new online learner with default configuration.
    pub fn new() -> Self {
        Self::with_config(OnlineLearningConfig::default())
    }

    /// Create a new online learner with custom configuration.
    pub fn with_config(config: OnlineLearningConfig) -> Self {
        let collector = RouteCollector::new(config.max_per_proxy)
            .with_retrain_threshold(config.retrain_threshold)
            .with_decay(config.decay_factor);

        Self {
            collector,
            model: RouteModel::new(),
            config,
            retrain_count: 0,
            initialized: false,
        }
    }

    /// Initialize the online learner.
    ///
    /// 1. Load previous measurements from disk (if available)
    /// 2. Load or train an initial model
    pub fn initialize(&mut self) -> Result<(), MlError> {
        // Ensure data directory exists
        let _ = std::fs::create_dir_all(&self.config.data_dir);

        // Load previous measurements
        let measurements_path = self.measurements_path();
        if measurements_path.exists() {
            match self.collector.load_from_file(measurements_path.to_str().unwrap_or("")) {
                Ok(count) => {
                    tracing::info!("Loaded {} previous measurements", count);
                }
                Err(e) => {
                    tracing::warn!("Could not load previous measurements: {}", e);
                }
            }
        }

        // Try to load saved model
        let model_path = self.model_path();
        if model_path.exists() {
            match self.model.load(model_path.to_str().unwrap_or("")) {
                Ok(()) => {
                    tracing::info!("Loaded saved ML model (version: {})", self.model.version());
                }
                Err(e) => {
                    tracing::warn!("Could not load saved model: {}", e);
                    // Fall through to train initial model
                }
            }
        }

        // If no model loaded, train with synthetic + any existing live data
        if !self.model.is_loaded() {
            tracing::info!("No saved model found — training initial model");
            self.train_initial_model()?;
        }

        self.initialized = true;
        Ok(())
    }

    /// Record a live measurement and check if retraining should occur.
    ///
    /// Returns `true` if a retrain was triggered and completed.
    pub fn record_and_maybe_retrain(
        &mut self,
        proxy_id: &str,
        proxy_region: &str,
        latency_ms: f64,
        jitter_ms: f64,
        packet_loss_pct: f64,
        proxy_load: f64,
    ) -> bool {
        self.collector.record_measurement(
            proxy_id, proxy_region, latency_ms, jitter_ms, packet_loss_pct, proxy_load,
        );

        if self.collector.should_retrain() {
            match self.retrain() {
                Ok(report) => {
                    tracing::info!(
                        "🧠 Online retrain #{}: MAE={:.2}ms, R²={:.4}, {} samples",
                        self.retrain_count,
                        report.mae_ms,
                        report.r_squared,
                        report.train_samples + report.test_samples,
                    );
                    return true;
                }
                Err(e) => {
                    tracing::warn!("Online retrain failed: {}", e);
                }
            }
        }

        false
    }

    /// Record a simple probe measurement.
    pub fn record_probe(&mut self, proxy_id: &str, proxy_region: &str, rtt_us: u64) {
        self.collector.record_probe(proxy_id, proxy_region, rtt_us);
    }

    /// Force a model retrain using collected live data.
    pub fn retrain(&mut self) -> Result<super::trainer::TrainingReport, MlError> {
        let mut samples = self.collector.training_samples();

        if samples.is_empty() {
            return Err(MlError::PredictionFailed("No training data available".into()));
        }

        // Optionally mix in synthetic data for robustness
        if self.config.mix_synthetic && self.config.synthetic_ratio > 0.0 {
            let synthetic_count = (samples.len() as f64 * self.config.synthetic_ratio) as usize;
            if synthetic_count > 0 {
                let synthetic_config = super::data::SyntheticConfig {
                    num_samples: synthetic_count,
                    seed: 42 + self.retrain_count as u64,
                    ..Default::default()
                };
                let synthetic_samples = super::data::generate_synthetic_data(&synthetic_config);
                samples.extend(synthetic_samples);
                tracing::debug!(
                    "Mixed {} live + {} synthetic samples for retraining",
                    samples.len() - synthetic_count,
                    synthetic_count
                );
            }
        }

        // Train new model
        let (model_bytes, report) = super::trainer::train_random_forest(&samples, 0.2)?;

        // Load new model
        let version = format!("online-v{}", self.retrain_count + 1);
        self.model.load_from_bytes(model_bytes, &version, &report.model_type);

        // Save model and measurements
        self.save_state();

        self.retrain_count += 1;
        self.collector.mark_retrained();

        Ok(report)
    }

    /// Save current state (model + measurements) to disk.
    pub fn save_state(&self) {
        // Save model
        let model_path = self.model_path();
        if self.model.is_loaded() {
            if let Err(e) = self.model.save(model_path.to_str().unwrap_or("")) {
                tracing::warn!("Failed to save model: {}", e);
            }
        }

        // Save measurements
        let measurements_path = self.measurements_path();
        if let Err(e) = self.collector.save_to_file(measurements_path.to_str().unwrap_or("")) {
            tracing::warn!("Failed to save measurements: {}", e);
        }
    }

    /// Get the number of times the model has been retrained.
    pub fn retrain_count(&self) -> u32 {
        self.retrain_count
    }

    /// Check if the system is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized
    }

    /// Get a summary of the online learner's state.
    pub fn summary(&self) -> OnlineLearnerSummary {
        OnlineLearnerSummary {
            initialized: self.initialized,
            model_loaded: self.model.is_loaded(),
            model_version: self.model.version().to_string(),
            retrain_count: self.retrain_count,
            total_measurements: self.collector.total_recorded(),
            per_proxy_counts: self.collector.per_proxy_counts(),
            should_retrain: self.collector.should_retrain(),
        }
    }

    // ── Private helpers ──────────────────────────────────────────

    fn measurements_path(&self) -> PathBuf {
        self.config.data_dir.join("measurements.json")
    }

    fn model_path(&self) -> PathBuf {
        self.config.data_dir.join("model.bin")
    }

    fn train_initial_model(&mut self) -> Result<(), MlError> {
        let live_samples = self.collector.training_samples();

        if live_samples.len() >= 30 {
            // Enough live data — train primarily on that
            tracing::info!("Training initial model from {} live samples", live_samples.len());
            let (model_bytes, report) = super::trainer::train_random_forest(&live_samples, 0.2)?;
            self.model.load_from_bytes(model_bytes, "live-initial", &report.model_type);
        } else {
            // Not enough live data — use synthetic + whatever live we have
            tracing::info!("Training initial model with synthetic data + {} live samples", live_samples.len());
            let report = self.model.train_and_load()?;
            tracing::info!(
                "Initial model trained: MAE={:.2}ms, R²={:.4}",
                report.mae_ms,
                report.r_squared
            );
        }

        // Save the initial model
        let model_path = self.model_path();
        if self.model.is_loaded() {
            let _ = self.model.save(model_path.to_str().unwrap_or(""));
        }

        Ok(())
    }
}

impl Default for OnlineLearner {
    fn default() -> Self {
        Self::new()
    }
}

/// Summary of the online learner's current state.
#[derive(Debug)]
pub struct OnlineLearnerSummary {
    pub initialized: bool,
    pub model_loaded: bool,
    pub model_version: String,
    pub retrain_count: u32,
    pub total_measurements: u64,
    pub per_proxy_counts: std::collections::HashMap<String, usize>,
    pub should_retrain: bool,
}

impl std::fmt::Display for OnlineLearnerSummary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Online Learning Status:")?;
        writeln!(f, "  Initialized:    {}", self.initialized)?;
        writeln!(f, "  Model loaded:   {}", self.model_loaded)?;
        writeln!(f, "  Model version:  {}", self.model_version)?;
        writeln!(f, "  Retrain count:  {}", self.retrain_count)?;
        writeln!(f, "  Total samples:  {}", self.total_measurements)?;
        writeln!(f, "  Need retrain:   {}", self.should_retrain)?;
        for (proxy, count) in &self.per_proxy_counts {
            writeln!(f, "  {}: {} measurements", proxy, count)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn test_config() -> OnlineLearningConfig {
        OnlineLearningConfig {
            retrain_threshold: 5,
            max_per_proxy: 100,
            decay_factor: 0.95,
            data_dir: PathBuf::from("test_data_online"),
            mix_synthetic: false, // Faster tests
            synthetic_ratio: 0.0,
        }
    }

    #[test]
    fn test_create_learner() {
        let learner = OnlineLearner::with_config(test_config());
        assert!(!learner.is_initialized());
        assert_eq!(learner.retrain_count(), 0);
    }

    #[test]
    fn test_record_measurements() {
        let mut learner = OnlineLearner::with_config(test_config());

        for i in 0..3 {
            learner.collector.record_measurement(
                "proxy-lax", "us-west-lax",
                200.0 + i as f64, 0.3, 0.0, 0.2,
            );
        }

        assert_eq!(learner.collector.total_recorded(), 3);
    }

    #[test]
    fn test_summary() {
        let mut learner = OnlineLearner::with_config(test_config());
        learner.collector.record_measurement(
            "proxy-lax", "us-west-lax", 204.8, 0.3, 0.0, 0.2,
        );

        let summary = learner.summary();
        assert!(!summary.initialized);
        assert!(!summary.model_loaded);
        assert_eq!(summary.total_measurements, 1);
        assert!(!summary.should_retrain);

        // Test Display trait
        let display = format!("{}", summary);
        assert!(display.contains("Online Learning Status"));
    }

    #[test]
    fn test_data_dir_creation() {
        let config = test_config();
        let learner = OnlineLearner::with_config(config.clone());
        assert_eq!(learner.measurements_path(), config.data_dir.join("measurements.json"));
        assert_eq!(learner.model_path(), config.data_dir.join("model.bin"));

        // Cleanup
        let _ = std::fs::remove_dir_all("test_data_online");
    }
}
