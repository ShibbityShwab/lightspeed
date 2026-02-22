//! # ML Route Prediction
//!
//! Machine learning models for intelligent route selection using linfa.
//! The ML system predicts which proxy will provide the lowest latency
//! for a given game server, based on current network conditions.
//!
//! ## Architecture
//!
//! ```text
//! Network Metrics → Feature Extraction → linfa Model → Route Score
//!       ↑                                                   ↓
//!    Feedback ← Actual Latency ← Selected Route ← Route Selector
//! ```
//!
//! ## Feature Gate
//!
//! The full ML pipeline requires `--features ml`. Without it, a heuristic
//! fallback is used that still provides reasonable route selection.

pub mod data;
pub mod features;
pub mod predict;
pub mod trainer;

use crate::error::MlError;

/// ML model wrapper for route prediction.
///
/// Manages the lifecycle of the trained model: load from disk,
/// hold in memory for fast inference, and provide version info.
pub struct RouteModel {
    /// Serialized model bytes (bincode-encoded linfa ensemble).
    model_bytes: Vec<u8>,
    /// Whether a model is loaded and ready.
    loaded: bool,
    /// Model version/identifier.
    version: String,
    /// Model type (e.g., "RandomForestEnsemble").
    model_type: String,
}

impl RouteModel {
    /// Create a new (unloaded) model.
    pub fn new() -> Self {
        Self {
            model_bytes: Vec::new(),
            loaded: false,
            version: String::new(),
            model_type: String::new(),
        }
    }

    /// Check if a model is loaded.
    pub fn is_loaded(&self) -> bool {
        self.loaded
    }

    /// Get model version.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Get model type.
    pub fn model_type(&self) -> &str {
        &self.model_type
    }

    /// Get the raw model bytes for inference.
    pub fn model_bytes(&self) -> &[u8] {
        &self.model_bytes
    }

    /// Load a pre-trained model from file.
    pub fn load(&mut self, path: &str) -> Result<(), MlError> {
        let path = std::path::Path::new(path);
        if !path.exists() {
            return Err(MlError::ModelNotFound(path.display().to_string()));
        }

        let bytes = std::fs::read(path).map_err(|e| {
            MlError::PredictionFailed(format!("Failed to read model file: {}", e))
        })?;

        // Validate minimum size (a valid bincode model should be > 100 bytes)
        if bytes.len() < 100 {
            return Err(MlError::PredictionFailed(
                "Model file too small — likely corrupted".into(),
            ));
        }

        self.model_bytes = bytes;
        self.loaded = true;
        self.version = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string();
        self.model_type = "RandomForestEnsemble".into();

        tracing::info!(
            "ML model loaded: {} ({} bytes, version: {})",
            self.model_type,
            self.model_bytes.len(),
            self.version
        );

        Ok(())
    }

    /// Load a model directly from bytes (e.g., after training).
    pub fn load_from_bytes(&mut self, bytes: Vec<u8>, version: &str, model_type: &str) {
        self.model_bytes = bytes;
        self.loaded = true;
        self.version = version.to_string();
        self.model_type = model_type.to_string();

        tracing::info!(
            "ML model loaded from memory: {} ({} bytes, version: {})",
            self.model_type,
            self.model_bytes.len(),
            self.version
        );
    }

    /// Save the loaded model to disk.
    pub fn save(&self, path: &str) -> Result<(), MlError> {
        if !self.loaded {
            return Err(MlError::NotLoaded);
        }

        // Create parent directory if needed
        if let Some(parent) = std::path::Path::new(path).parent() {
            std::fs::create_dir_all(parent).map_err(|e| {
                MlError::PredictionFailed(format!("Failed to create directory: {}", e))
            })?;
        }

        std::fs::write(path, &self.model_bytes).map_err(|e| {
            MlError::PredictionFailed(format!("Failed to write model file: {}", e))
        })?;

        tracing::info!("ML model saved to {} ({} bytes)", path, self.model_bytes.len());
        Ok(())
    }

    /// Predict latencies for a set of proxies using the loaded model.
    pub fn predict(
        &self,
        features_per_proxy: &[(String, features::NetworkFeatures)],
    ) -> Result<predict::RoutePrediction, MlError> {
        if !self.loaded {
            return Err(MlError::NotLoaded);
        }

        predict::predict_route(features_per_proxy, &self.model_bytes)
    }

    /// Train a new model from data and load it.
    ///
    /// This is a convenience method that generates synthetic data,
    /// trains both model types, picks the best, and loads it.
    pub fn train_and_load(&mut self) -> Result<trainer::TrainingReport, MlError> {
        tracing::info!("Generating synthetic training data...");
        let config = data::SyntheticConfig::default();
        let samples = data::generate_synthetic_data(&config);
        tracing::info!("Generated {} training samples", samples.len());

        // Train random forest (primary model)
        let (rf_bytes, rf_report) = trainer::train_random_forest(&samples, 0.2)?;

        tracing::info!(
            "Random Forest: MAE={:.2}ms, R²={:.4}",
            rf_report.mae_ms,
            rf_report.r_squared
        );

        // Load the best model
        self.load_from_bytes(
            rf_bytes,
            "synthetic-v1",
            &rf_report.model_type,
        );

        Ok(rf_report)
    }
}

impl Default for RouteModel {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_model_lifecycle() {
        let model = RouteModel::new();
        assert!(!model.is_loaded());
        assert_eq!(model.version(), "");
    }

    #[test]
    fn test_load_nonexistent() {
        let mut model = RouteModel::new();
        let result = model.load("/nonexistent/model.bin");
        assert!(result.is_err());
    }

    #[test]
    fn test_predict_without_load() {
        let model = RouteModel::new();
        let features = vec![(
            "test".into(),
            features::NetworkFeatures::default(),
        )];
        let result = model.predict(&features);
        assert!(result.is_err());
    }
}
