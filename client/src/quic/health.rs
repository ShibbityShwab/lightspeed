//! # Proxy Health Checking
//!
//! Periodically probes proxy nodes to measure latency and detect failures.
//! Health data feeds into the route selector for optimal proxy selection.

use std::time::{Duration, Instant};

use crate::route::{ProxyHealth, ProxyNode};

/// Health check result for a single probe.
#[derive(Debug, Clone)]
pub struct HealthCheckResult {
    /// Proxy that was checked.
    pub proxy_id: String,
    /// Round-trip time of the health check.
    pub rtt: Duration,
    /// Whether the check succeeded.
    pub healthy: bool,
    /// When the check was performed.
    pub checked_at: Instant,
}

/// Health checker — probes proxies at regular intervals.
pub struct HealthChecker {
    /// How often to check each proxy.
    pub interval: Duration,
    /// Timeout for each health check.
    pub timeout: Duration,
    /// Number of consecutive failures before marking unhealthy.
    pub failure_threshold: u32,
}

impl HealthChecker {
    /// Create a new health checker with default settings.
    pub fn new() -> Self {
        Self {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            failure_threshold: 3,
        }
    }

    /// Perform a single health check against a proxy.
    pub async fn check(&self, _proxy: &ProxyNode) -> HealthCheckResult {
        // TODO (WF-001 Step 4): Implement health check
        // 1. Send QUIC ping to proxy's control plane
        // 2. Measure RTT
        // 3. Update proxy health status

        HealthCheckResult {
            proxy_id: String::new(),
            rtt: Duration::from_millis(0),
            healthy: false,
            checked_at: Instant::now(),
        }
    }

    /// Determine proxy health from recent check results.
    pub fn evaluate_health(&self, results: &[HealthCheckResult]) -> ProxyHealth {
        if results.is_empty() {
            return ProxyHealth::Unknown;
        }

        let recent_failures = results
            .iter()
            .rev()
            .take(self.failure_threshold as usize)
            .filter(|r| !r.healthy)
            .count();

        if recent_failures >= self.failure_threshold as usize {
            ProxyHealth::Unhealthy
        } else if recent_failures > 0 {
            ProxyHealth::Degraded
        } else {
            ProxyHealth::Healthy
        }
    }
}
