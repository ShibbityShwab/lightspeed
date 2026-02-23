//! # Proxy Server Configuration

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level proxy configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Server identity.
    #[serde(default)]
    pub server: ServerConfig,

    /// Security settings.
    #[serde(default)]
    pub security: SecurityConfig,

    /// Rate limiting settings.
    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    /// Metrics settings.
    #[serde(default)]
    pub metrics: MetricsConfig,
}

/// Server identity and general settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    /// Unique node ID.
    #[serde(default = "default_node_id")]
    pub node_id: String,

    /// Region identifier (e.g., "us-east", "eu-west").
    #[serde(default = "default_region")]
    pub region: String,

    /// Maximum concurrent client tunnels.
    #[serde(default = "default_max_clients")]
    pub max_clients: usize,
}

/// Rate limiting configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RateLimitConfig {
    /// Maximum packets per second per client.
    #[serde(default = "default_pps_limit")]
    pub max_pps_per_client: u64,

    /// Maximum bandwidth per client (bytes/sec).
    #[serde(default = "default_bps_limit")]
    pub max_bps_per_client: u64,

    /// Maximum total connections.
    #[serde(default = "default_max_connections")]
    pub max_connections: usize,
}

/// Metrics export configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable Prometheus metrics export.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Metrics update interval in seconds.
    #[serde(default = "default_metrics_interval")]
    pub interval_secs: u64,
}

/// Security configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecurityConfig {
    /// Require QUIC registration before accepting data-plane packets.
    /// When false, any client can send tunnel packets (dev mode).
    /// MUST be true in production.
    #[serde(default)]
    pub require_auth: bool,

    /// Maximum amplification ratio before banning (outbound/inbound).
    #[serde(default = "default_amplification_ratio")]
    pub max_amplification_ratio: f64,

    /// Maximum unique destinations per 10-second window per client.
    #[serde(default = "default_max_destinations")]
    pub max_destinations_per_window: usize,

    /// Abuse ban duration in seconds.
    #[serde(default = "default_ban_duration")]
    pub ban_duration_secs: u64,
}

fn default_amplification_ratio() -> f64 {
    2.0
}
fn default_max_destinations() -> usize {
    10
}
fn default_ban_duration() -> u64 {
    3600
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            require_auth: false,
            max_amplification_ratio: default_amplification_ratio(),
            max_destinations_per_window: default_max_destinations(),
            ban_duration_secs: default_ban_duration(),
        }
    }
}

fn default_node_id() -> String {
    "proxy-001".into()
}
fn default_region() -> String {
    "unknown".into()
}
fn default_max_clients() -> usize {
    100
}
fn default_pps_limit() -> u64 {
    1000
}
fn default_bps_limit() -> u64 {
    1_000_000
} // 1 MB/s
fn default_max_connections() -> usize {
    200
}
fn default_true() -> bool {
    true
}
fn default_metrics_interval() -> u64 {
    10
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            security: SecurityConfig::default(),
            rate_limit: RateLimitConfig::default(),
            metrics: MetricsConfig::default(),
        }
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            node_id: default_node_id(),
            region: default_region(),
            max_clients: default_max_clients(),
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_pps_per_client: default_pps_limit(),
            max_bps_per_client: default_bps_limit(),
            max_connections: default_max_connections(),
        }
    }
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval_secs: default_metrics_interval(),
        }
    }
}

impl ProxyConfig {
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let path = Path::new(path);
        if !path.exists() {
            anyhow::bail!("Config file not found: {}", path.display());
        }
        let content = std::fs::read_to_string(path)?;
        let config: ProxyConfig = toml::from_str(&content)?;
        Ok(config)
    }
}
