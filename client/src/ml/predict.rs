//! # ML Inference Engine
//!
//! Runs real-time route prediction using the loaded linfa model.
//! Inference must complete in < 1ms to avoid adding latency.

use super::features::NetworkFeatures;
use crate::error::MlError;

/// Route prediction result.
#[derive(Debug, Clone)]
pub struct RoutePrediction {
    /// Proxy ID → predicted latency in milliseconds.
    pub scores: Vec<(String, f64)>,
    /// Confidence of the prediction (0.0 - 1.0).
    pub confidence: f64,
    /// Inference time in microseconds.
    pub inference_time_us: u64,
}

impl RoutePrediction {
    /// Get the proxy with the lowest predicted latency.
    pub fn best_proxy(&self) -> Option<(&str, f64)> {
        self.scores
            .iter()
            .min_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(id, score)| (id.as_str(), *score))
    }

    /// Get proxies sorted by predicted latency (lowest first).
    pub fn ranked_proxies(&self) -> Vec<(&str, f64)> {
        let mut sorted: Vec<_> = self.scores.iter().map(|(id, s)| (id.as_str(), *s)).collect();
        sorted.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal));
        sorted
    }
}

/// Predict route latencies using the loaded Random Forest ensemble.
///
/// For each proxy, takes its NetworkFeatures and predicts the expected
/// latency through that proxy. Returns predictions for all proxies
/// so the selector can choose the best one.
#[cfg(feature = "ml")]
pub fn predict_route(
    features_per_proxy: &[(String, NetworkFeatures)],
    model_bytes: &[u8],
) -> Result<RoutePrediction, MlError> {
    use linfa::prelude::*;
    use linfa_trees::DecisionTree;
    use ndarray::Array2;
    use std::time::Instant;

    let start = Instant::now();

    // Deserialize the ensemble
    let trees: Vec<DecisionTree<f64, f64>> = bincode::deserialize(model_bytes)
        .map_err(|e| MlError::PredictionFailed(format!("Model deserialization failed: {}", e)))?;

    if trees.is_empty() {
        return Err(MlError::PredictionFailed("Empty model ensemble".into()));
    }

    let n_proxies = features_per_proxy.len();
    let n_features = NetworkFeatures::FEATURE_COUNT;

    // Build feature matrix for all proxies at once
    let mut features_flat = Vec::with_capacity(n_proxies * n_features);
    for (_, features) in features_per_proxy {
        features_flat.extend_from_slice(&features.to_array());
    }

    let feature_matrix =
        Array2::from_shape_vec((n_proxies, n_features), features_flat).map_err(|e| {
            MlError::PredictionFailed(format!("Feature matrix error: {}", e))
        })?;

    // Ensemble prediction: average of all trees
    let mut sum_predictions = ndarray::Array1::zeros(n_proxies);
    for tree in &trees {
        let preds = tree.predict(&feature_matrix);
        sum_predictions = sum_predictions + &preds;
    }
    let predictions = sum_predictions / trees.len() as f64;

    // Build result
    let scores: Vec<(String, f64)> = features_per_proxy
        .iter()
        .zip(predictions.iter())
        .map(|((proxy_id, _), &pred)| (proxy_id.clone(), pred.max(0.0)))
        .collect();

    // Confidence based on prediction spread (lower spread = higher confidence)
    let confidence = if scores.len() > 1 {
        let min = scores.iter().map(|s| s.1).fold(f64::MAX, f64::min);
        let max = scores.iter().map(|s| s.1).fold(f64::MIN, f64::max);
        let spread = max - min;
        // High spread = clear winner = high confidence
        (spread / (min + 1.0)).min(1.0).max(0.1)
    } else {
        0.5
    };

    let inference_time_us = start.elapsed().as_micros() as u64;

    Ok(RoutePrediction {
        scores,
        confidence,
        inference_time_us,
    })
}

/// Fallback prediction without ML — uses a simple heuristic.
///
/// Weighted score: 0.6 * current_latency + 0.2 * p50 + 0.1 * jitter + 0.1 * load_penalty
#[cfg(not(feature = "ml"))]
pub fn predict_route(
    features_per_proxy: &[(String, NetworkFeatures)],
    _model_bytes: &[u8],
) -> Result<RoutePrediction, MlError> {
    use std::time::Instant;
    let start = Instant::now();

    let scores: Vec<(String, f64)> = features_per_proxy
        .iter()
        .map(|(proxy_id, f)| {
            let score = f.current_latency_ms * 0.6
                + f.historical_p50_ms * 0.2
                + f.jitter_ms * 10.0 * 0.1
                + f.proxy_load * 50.0 * 0.1;
            (proxy_id.clone(), score)
        })
        .collect();

    let inference_time_us = start.elapsed().as_micros() as u64;

    Ok(RoutePrediction {
        scores,
        confidence: 0.3, // Low confidence for heuristic
        inference_time_us,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_route_prediction_ranking() {
        let pred = RoutePrediction {
            scores: vec![
                ("us-east".into(), 25.0),
                ("eu-west".into(), 45.0),
                ("asia-se".into(), 80.0),
            ],
            confidence: 0.8,
            inference_time_us: 50,
        };

        assert_eq!(pred.best_proxy().unwrap().0, "us-east");

        let ranked = pred.ranked_proxies();
        assert_eq!(ranked[0].0, "us-east");
        assert_eq!(ranked[1].0, "eu-west");
        assert_eq!(ranked[2].0, "asia-se");
    }

    #[test]
    fn test_heuristic_fallback() {
        let features = vec![
            (
                "fast-proxy".into(),
                NetworkFeatures {
                    current_latency_ms: 10.0,
                    historical_p50_ms: 12.0,
                    jitter_ms: 1.0,
                    proxy_load: 0.2,
                    ..Default::default()
                },
            ),
            (
                "slow-proxy".into(),
                NetworkFeatures {
                    current_latency_ms: 50.0,
                    historical_p50_ms: 55.0,
                    jitter_ms: 8.0,
                    proxy_load: 0.8,
                    ..Default::default()
                },
            ),
        ];

        // Even without ml feature, the heuristic fallback should work
        let result = predict_route(&features, &[]);
        assert!(result.is_ok());

        let pred = result.unwrap();
        assert_eq!(pred.best_proxy().unwrap().0, "fast-proxy");
    }
}
