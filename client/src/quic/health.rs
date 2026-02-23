//! # Proxy Health Checking
//!
//! Periodically probes proxy nodes to measure latency and detect failures.
//! Health data feeds into the route selector for optimal proxy selection.
//!
//! Uses the QUIC control plane for ping/pong probes when the `quic` feature
//! is enabled; otherwise falls back to simple UDP echo timing.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::time::{Duration, Instant};

use tracing::{debug, info, warn};

use crate::error::QuicError;
use crate::quic::ControlClient;
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
    /// Recent results per proxy (keyed by proxy ID).
    results: HashMap<String, Vec<HealthCheckResult>>,
    /// Maximum results to retain per proxy.
    max_history: usize,
}

impl HealthChecker {
    /// Create a new health checker with default settings.
    pub fn new() -> Self {
        Self {
            interval: Duration::from_secs(10),
            timeout: Duration::from_secs(5),
            failure_threshold: 3,
            results: HashMap::new(),
            max_history: 20,
        }
    }

    /// Create a health checker with custom settings.
    pub fn with_config(interval: Duration, timeout: Duration, failure_threshold: u32) -> Self {
        Self {
            interval,
            timeout,
            failure_threshold,
            results: HashMap::new(),
            max_history: 20,
        }
    }

    /// Perform a single health check against a proxy using an existing control client.
    pub async fn check_with_client(
        &mut self,
        proxy: &ProxyNode,
        client: &ControlClient,
    ) -> HealthCheckResult {
        let start = Instant::now();

        let result = tokio::time::timeout(self.timeout, client.ping()).await;

        let (rtt, healthy) = match result {
            Ok(Ok(rtt_us)) => {
                let rtt = Duration::from_micros(rtt_us);
                debug!("Health check OK for {}: {}μs", proxy.id, rtt_us);
                (rtt, true)
            }
            Ok(Err(e)) => {
                warn!("Health check failed for {}: {}", proxy.id, e);
                (start.elapsed(), false)
            }
            Err(_) => {
                warn!("Health check timed out for {}", proxy.id);
                (self.timeout, false)
            }
        };

        let check = HealthCheckResult {
            proxy_id: proxy.id.clone(),
            rtt,
            healthy,
            checked_at: Instant::now(),
        };

        // Store result
        let history = self
            .results
            .entry(proxy.id.clone())
            .or_insert_with(Vec::new);
        history.push(check.clone());
        if history.len() > self.max_history {
            history.remove(0);
        }

        check
    }

    /// Perform a standalone health check (creates its own connection).
    pub async fn check_standalone(&mut self, proxy: &ProxyNode, game: u8) -> HealthCheckResult {
        let control_addr = SocketAddr::V4(proxy.control_addr);
        let start = Instant::now();

        let mut client = match ControlClient::new() {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to create control client for {}: {}", proxy.id, e);
                return HealthCheckResult {
                    proxy_id: proxy.id.clone(),
                    rtt: start.elapsed(),
                    healthy: false,
                    checked_at: Instant::now(),
                };
            }
        };

        // Connect + register + ping
        let result = tokio::time::timeout(self.timeout, async {
            client.connect(control_addr, game).await?;
            let rtt_us = client.ping().await?;
            client.disconnect().await?;
            Ok::<u64, QuicError>(rtt_us)
        })
        .await;

        let (rtt, healthy) = match result {
            Ok(Ok(rtt_us)) => (Duration::from_micros(rtt_us), true),
            Ok(Err(e)) => {
                warn!("Standalone health check failed for {}: {}", proxy.id, e);
                (start.elapsed(), false)
            }
            Err(_) => {
                warn!("Standalone health check timed out for {}", proxy.id);
                (self.timeout, false)
            }
        };

        let check = HealthCheckResult {
            proxy_id: proxy.id.clone(),
            rtt,
            healthy,
            checked_at: Instant::now(),
        };

        let history = self
            .results
            .entry(proxy.id.clone())
            .or_insert_with(Vec::new);
        history.push(check.clone());
        if history.len() > self.max_history {
            history.remove(0);
        }

        check
    }

    /// Determine proxy health from recent check results.
    pub fn evaluate_health(&self, proxy_id: &str) -> ProxyHealth {
        let results = match self.results.get(proxy_id) {
            Some(r) => r,
            None => return ProxyHealth::Unknown,
        };

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

    /// Get the average RTT for a proxy (from successful checks only).
    pub fn average_rtt(&self, proxy_id: &str) -> Option<Duration> {
        let results = self.results.get(proxy_id)?;
        let successful: Vec<_> = results.iter().filter(|r| r.healthy).collect();
        if successful.is_empty() {
            return None;
        }
        let total: Duration = successful.iter().map(|r| r.rtt).sum();
        Some(total / successful.len() as u32)
    }

    /// Get the latest RTT for a proxy.
    pub fn latest_rtt(&self, proxy_id: &str) -> Option<Duration> {
        self.results
            .get(proxy_id)?
            .iter()
            .rev()
            .find(|r| r.healthy)
            .map(|r| r.rtt)
    }

    /// Get all results for a proxy.
    pub fn get_history(&self, proxy_id: &str) -> &[HealthCheckResult] {
        self.results
            .get(proxy_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Clear all stored results.
    pub fn clear(&mut self) {
        self.results.clear();
    }
}
