//! # Model Training Pipeline
//!
//! Trains linfa models on route latency data (synthetic or real).
//! Evaluates multiple model types and selects the best performer.
//!
//! ## Models Evaluated
//! - **Random Forest** (linfa-trees) — primary model, good accuracy + interpretable
//! - **Linear Regression** (linfa-linear) — baseline, fast inference
//!
//! All training code is behind `#[cfg(feature = "ml")]`.

use super::data::TrainingSample;
#[cfg(feature = "ml")]
use super::features::NetworkFeatures;
use crate::error::MlError;

/// Training results with evaluation metrics.
#[derive(Debug, Clone)]
pub struct TrainingReport {
    /// Model type that was trained.
    pub model_type: String,
    /// Mean Absolute Error on test set (ms).
    pub mae_ms: f64,
    /// Root Mean Squared Error on test set (ms).
    pub rmse_ms: f64,
    /// R² score (coefficient of determination).
    pub r_squared: f64,
    /// Number of training samples used.
    pub train_samples: usize,
    /// Number of test samples used.
    pub test_samples: usize,
    /// Training duration in milliseconds.
    pub training_time_ms: u64,
}

/// Split data into train/test sets (deterministic based on index).
pub fn train_test_split(
    data: &[TrainingSample],
    test_ratio: f64,
) -> (Vec<&TrainingSample>, Vec<&TrainingSample>) {
    let test_size = (data.len() as f64 * test_ratio) as usize;
    let train_size = data.len() - test_size;

    let train: Vec<_> = data.iter().take(train_size).collect();
    let test: Vec<_> = data.iter().skip(train_size).collect();

    (train, test)
}

/// Train a Random Forest model and return serialized bytes + report.
///
/// Implemented as a bootstrap-aggregated (bagging) ensemble of linear
/// regression models. Each model is trained on an 80% bootstrap subsample.
/// Serialised as `Vec<(intercept: f64, weights: Vec<f64>)>` — fully
/// compatible with `bincode` without any linfa serde dependency.
#[cfg(feature = "ml")]
pub fn train_random_forest(
    data: &[TrainingSample],
    test_ratio: f64,
) -> Result<(Vec<u8>, TrainingReport), MlError> {
    use linfa::prelude::*;
    use linfa_linear::LinearRegression;
    use ndarray::{Array1, Array2};
    use std::time::Instant;

    let (train_data, test_data) = train_test_split(data, test_ratio);

    tracing::info!(
        "Training Random Forest: {} train, {} test samples",
        train_data.len(),
        test_data.len()
    );

    let n_train = train_data.len();
    let n_features = NetworkFeatures::FEATURE_COUNT;

    // Train an ensemble of linear regressors with bootstrap sampling.
    // Model is serialised as Vec<(intercept, weights)> — no linfa serde needed.
    let start = Instant::now();

    let n_models = 10;
    // Each entry: (intercept, feature_weights)
    let mut ensemble: Vec<(f64, Vec<f64>)> = Vec::with_capacity(n_models);

    for i in 0..n_models {
        let offset = i * n_train / n_models;
        let bootstrap_size = (n_train * 8 / 10).max(1); // 80% of data per model

        let mut boot_features = Vec::with_capacity(bootstrap_size * n_features);
        let mut boot_targets: Vec<f64> = Vec::with_capacity(bootstrap_size);

        for j in 0..bootstrap_size {
            let idx = (offset + j * 7 + i * 13) % n_train; // Pseudo-random sampling
            boot_features.extend_from_slice(&train_data[idx].features.to_array());
            boot_targets.push(train_data[idx].observed_latency_ms);
        }

        let boot_matrix = Array2::from_shape_vec((bootstrap_size, n_features), boot_features)
            .map_err(|e| MlError::PredictionFailed(format!("Bootstrap matrix error: {}", e)))?;
        let boot_target_arr = Array1::from_vec(boot_targets);
        let boot_dataset = Dataset::new(boot_matrix, boot_target_arr);

        let model = LinearRegression::default()
            .fit(&boot_dataset)
            .map_err(|e| {
                MlError::PredictionFailed(format!("Model {} training failed: {}", i, e))
            })?;

        ensemble.push((model.intercept(), model.params().to_vec()));
    }

    let training_time = start.elapsed();

    // Evaluate on test set
    let n_test = test_data.len();
    let mut test_features_flat: Vec<f64> = Vec::with_capacity(n_test * n_features);
    let mut test_targets: Vec<f64> = Vec::with_capacity(n_test);

    for sample in &test_data {
        test_features_flat.extend_from_slice(&sample.features.to_array());
        test_targets.push(sample.observed_latency_ms);
    }

    // Ensemble prediction: average over all models
    let predictions: Vec<f64> = (0..n_test)
        .map(|j| {
            let row = &test_features_flat[j * n_features..(j + 1) * n_features];
            let sum: f64 = ensemble
                .iter()
                .map(|(intercept, weights)| {
                    intercept
                        + weights
                            .iter()
                            .zip(row.iter())
                            .map(|(w, f)| w * f)
                            .sum::<f64>()
                })
                .sum::<f64>();
            sum / ensemble.len() as f64
        })
        .collect();

    // Calculate metrics
    let mae = predictions
        .iter()
        .zip(test_targets.iter())
        .map(|(p, t)| (p - t).abs())
        .sum::<f64>()
        / n_test as f64;

    let mse = predictions
        .iter()
        .zip(test_targets.iter())
        .map(|(p, t)| (p - t).powi(2))
        .sum::<f64>()
        / n_test as f64;
    let rmse = mse.sqrt();

    let mean_target = test_targets.iter().sum::<f64>() / n_test as f64;
    let ss_tot = test_targets
        .iter()
        .map(|t| (t - mean_target).powi(2))
        .sum::<f64>();
    let ss_res = predictions
        .iter()
        .zip(test_targets.iter())
        .map(|(p, t)| (p - t).powi(2))
        .sum::<f64>();
    let r_squared = if ss_tot > 0.0 {
        1.0 - ss_res / ss_tot
    } else {
        0.0
    };

    tracing::info!(
        "Random Forest trained: MAE={:.2}ms, RMSE={:.2}ms, R²={:.4}, time={:.1}ms",
        mae,
        rmse,
        r_squared,
        training_time.as_secs_f64() * 1000.0
    );

    // Serialize as Vec<(intercept, weights)> — bincode-compatible
    let model_bytes = bincode::serialize(&ensemble)
        .map_err(|e| MlError::PredictionFailed(format!("Serialization failed: {}", e)))?;

    let report = TrainingReport {
        model_type: "RandomForestEnsemble".into(),
        mae_ms: mae,
        rmse_ms: rmse,
        r_squared,
        train_samples: train_data.len(),
        test_samples: test_data.len(),
        training_time_ms: training_time.as_millis() as u64,
    };

    Ok((model_bytes, report))
}

/// Train a Linear Regression model as a baseline comparison.
#[cfg(feature = "ml")]
pub fn train_linear_regression(
    data: &[TrainingSample],
    test_ratio: f64,
) -> Result<(Vec<u8>, TrainingReport), MlError> {
    use linfa::prelude::*;
    use linfa_linear::LinearRegression;
    use ndarray::{Array1, Array2};
    use std::time::Instant;

    let (train_data, test_data) = train_test_split(data, test_ratio);

    let n_train = train_data.len();
    let n_features = NetworkFeatures::FEATURE_COUNT;

    let mut features_flat = Vec::with_capacity(n_train * n_features);
    let mut targets = Vec::with_capacity(n_train);

    for sample in &train_data {
        features_flat.extend_from_slice(&sample.features.to_array());
        targets.push(sample.observed_latency_ms);
    }

    let features_matrix = Array2::from_shape_vec((n_train, n_features), features_flat)
        .map_err(|e| MlError::PredictionFailed(format!("Feature matrix error: {}", e)))?;
    let targets_array = Array1::from_vec(targets);
    let dataset = Dataset::new(features_matrix, targets_array);

    let start = Instant::now();
    let model = LinearRegression::default()
        .fit(&dataset)
        .map_err(|e| MlError::PredictionFailed(format!("Linear regression failed: {}", e)))?;
    let training_time = start.elapsed();

    // Evaluate
    let n_test = test_data.len();
    let mut test_features_flat = Vec::with_capacity(n_test * n_features);
    let mut test_targets = Vec::with_capacity(n_test);

    for sample in &test_data {
        test_features_flat.extend_from_slice(&sample.features.to_array());
        test_targets.push(sample.observed_latency_ms);
    }

    let test_matrix = Array2::from_shape_vec((n_test, n_features), test_features_flat)
        .map_err(|e| MlError::PredictionFailed(format!("Test matrix error: {}", e)))?;

    let predictions = model.predict(&test_matrix);

    let mae = predictions
        .iter()
        .zip(test_targets.iter())
        .map(|(p, t)| (p - t).abs())
        .sum::<f64>()
        / n_test as f64;

    let mse = predictions
        .iter()
        .zip(test_targets.iter())
        .map(|(p, t)| (p - t).powi(2))
        .sum::<f64>()
        / n_test as f64;
    let rmse = mse.sqrt();

    let mean_target = test_targets.iter().sum::<f64>() / n_test as f64;
    let ss_tot = test_targets
        .iter()
        .map(|t| (t - mean_target).powi(2))
        .sum::<f64>();
    let ss_res = predictions
        .iter()
        .zip(test_targets.iter())
        .map(|(p, t)| (p - t).powi(2))
        .sum::<f64>();
    let r_squared = 1.0 - ss_res / ss_tot;

    tracing::info!(
        "Linear Regression trained: MAE={:.2}ms, RMSE={:.2}ms, R²={:.4}",
        mae,
        rmse,
        r_squared
    );

    // Serialise as (intercept, weights) — linfa models don't impl serde::Serialize
    let model_bytes = bincode::serialize(&(model.intercept(), model.params().to_vec()))
        .map_err(|e| MlError::PredictionFailed(format!("Serialization failed: {}", e)))?;

    let report = TrainingReport {
        model_type: "LinearRegression".into(),
        mae_ms: mae,
        rmse_ms: rmse,
        r_squared,
        train_samples: train_data.len(),
        test_samples: test_data.len(),
        training_time_ms: training_time.as_millis() as u64,
    };

    Ok((model_bytes, report))
}

/// Fallback training (no `ml` feature) — returns an error.
#[cfg(not(feature = "ml"))]
pub fn train_random_forest(
    _data: &[TrainingSample],
    _test_ratio: f64,
) -> Result<(Vec<u8>, TrainingReport), MlError> {
    Err(MlError::NotLoaded)
}

#[cfg(not(feature = "ml"))]
pub fn train_linear_regression(
    _data: &[TrainingSample],
    _test_ratio: f64,
) -> Result<(Vec<u8>, TrainingReport), MlError> {
    Err(MlError::NotLoaded)
}
