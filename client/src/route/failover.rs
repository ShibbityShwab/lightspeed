//! # Automatic Failover
//!
//! Handles proxy failure detection and automatic failover to backup proxies.
//! Failover triggers:
//! - Proxy stops responding to keepalive
//! - Latency exceeds threshold
//! - Packet loss exceeds threshold
//! - QUIC health check fails

use std::time::{Duration, Instant};

use super::ProxyNode;

/// Failover configuration.
#[derive(Debug, Clone)]
pub struct FailoverConfig {
    /// Maximum keepalive misses before marking unhealthy.
    pub max_keepalive_misses: u32,
    /// Latency threshold (μs) — failover if exceeded consistently.
    pub latency_threshold_us: u64,
    /// Packet loss threshold (%) — failover if exceeded.
    pub loss_threshold_pct: f64,
    /// Cooldown before retrying a failed proxy.
    pub retry_cooldown: Duration,
    /// Maximum failover attempts before giving up.
    pub max_attempts: usize,
}

impl Default for FailoverConfig {
    fn default() -> Self {
        Self {
            max_keepalive_misses: 3,
            latency_threshold_us: 200_000, // 200ms
            loss_threshold_pct: 5.0,
            retry_cooldown: Duration::from_secs(30),
            max_attempts: 3,
        }
    }
}

/// Tracks failover state for the active connection.
pub struct FailoverState {
    /// Currently active proxy.
    pub active_proxy: Option<ProxyNode>,
    /// Number of consecutive keepalive misses.
    pub keepalive_misses: u32,
    /// Number of failover attempts made.
    pub failover_count: usize,
    /// When the last failover occurred.
    pub last_failover: Option<Instant>,
    /// Proxies that have been tried and failed.
    pub failed_proxies: Vec<(String, Instant)>,
    /// Configuration.
    pub config: FailoverConfig,
}

impl FailoverState {
    /// Create a new failover state with default config.
    pub fn new() -> Self {
        Self {
            active_proxy: None,
            keepalive_misses: 0,
            failover_count: 0,
            last_failover: None,
            failed_proxies: vec![],
            config: FailoverConfig::default(),
        }
    }

    /// Record a keepalive miss.
    pub fn record_keepalive_miss(&mut self) {
        self.keepalive_misses += 1;
    }

    /// Record a successful keepalive.
    pub fn record_keepalive_success(&mut self) {
        self.keepalive_misses = 0;
    }

    /// Check if failover should be triggered.
    pub fn should_failover(&self) -> bool {
        self.keepalive_misses >= self.config.max_keepalive_misses
    }

    /// Check if more failover attempts are allowed.
    pub fn can_failover(&self) -> bool {
        self.failover_count < self.config.max_attempts
    }

    /// Check if a previously failed proxy can be retried.
    pub fn can_retry(&self, proxy_id: &str) -> bool {
        self.failed_proxies
            .iter()
            .find(|(id, _)| id == proxy_id)
            .map(|(_, failed_at)| failed_at.elapsed() >= self.config.retry_cooldown)
            .unwrap_or(true) // Not in failed list = can try
    }
}
