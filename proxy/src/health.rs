//! # Health Check Endpoint
//!
//! Simple HTTP health check endpoint for monitoring proxy status.
//! Responds to GET /health with proxy status information.

use serde::Serialize;

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// Overall health status.
    pub status: &'static str,
    /// Proxy version.
    pub version: &'static str,
    /// Number of active connections.
    pub active_connections: u64,
    /// Uptime in seconds.
    pub uptime_secs: u64,
    /// Region identifier.
    pub region: String,
}

impl HealthResponse {
    /// Create a healthy response.
    pub fn healthy(active_connections: u64, uptime_secs: u64, region: String) -> Self {
        Self {
            status: "healthy",
            version: env!("CARGO_PKG_VERSION"),
            active_connections,
            uptime_secs,
            region,
        }
    }
}

// TODO (WF-001 Step 3): Implement HTTP health check server
// - Bind to health check port (default 8080)
// - GET /health → HealthResponse as JSON
// - GET /metrics → Prometheus metrics
// - Keep it lightweight (no heavy framework)
