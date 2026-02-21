//! # LightSpeed Client Configuration
//!
//! Manages client configuration from file and defaults.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level client configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    /// General settings.
    #[serde(default)]
    pub general: GeneralConfig,

    /// Tunnel settings.
    #[serde(default)]
    pub tunnel: TunnelConfig,

    /// Proxy settings.
    #[serde(default)]
    pub proxy: ProxyConfig,

    /// Route selection settings.
    #[serde(default)]
    pub route: RouteConfig,

    /// ML model settings.
    #[serde(default)]
    pub ml: MlConfig,
}

/// General application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Log level (trace, debug, info, warn, error).
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Enable telemetry (latency metrics only, opt-in).
    #[serde(default)]
    pub telemetry: bool,

    /// Network interface to capture on (auto-detect if empty).
    #[serde(default)]
    pub interface: Option<String>,
}

/// Tunnel engine settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TunnelConfig {
    /// Keepalive interval in milliseconds.
    #[serde(default = "default_keepalive_ms")]
    pub keepalive_ms: u64,

    /// Connection timeout in milliseconds.
    #[serde(default = "default_timeout_ms")]
    pub timeout_ms: u64,

    /// Maximum packet size (MTU).
    #[serde(default = "default_mtu")]
    pub mtu: usize,
}

/// Proxy connection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// List of known proxy addresses (host:port).
    #[serde(default)]
    pub servers: Vec<String>,

    /// QUIC control plane port.
    #[serde(default = "default_quic_port")]
    pub quic_port: u16,

    /// UDP data plane port.
    #[serde(default = "default_data_port")]
    pub data_port: u16,
}

/// Route selection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RouteConfig {
    /// Route selection strategy: "nearest", "ml", "multipath".
    #[serde(default = "default_strategy")]
    pub strategy: String,

    /// Enable multipath (send on multiple paths, use fastest).
    #[serde(default)]
    pub multipath: bool,

    /// Health check interval in milliseconds.
    #[serde(default = "default_health_check_ms")]
    pub health_check_ms: u64,

    /// Maximum proxy failover attempts.
    #[serde(default = "default_max_failover")]
    pub max_failover: usize,
}

/// ML model settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MlConfig {
    /// Path to pre-trained model file.
    #[serde(default)]
    pub model_path: Option<String>,

    /// Enable online learning (adapt model in real-time).
    #[serde(default)]
    pub online_learning: bool,

    /// Minimum samples before online update.
    #[serde(default = "default_min_samples")]
    pub min_samples: usize,
}

// Default value functions

fn default_log_level() -> String {
    "info".into()
}

fn default_keepalive_ms() -> u64 {
    5000
}

fn default_timeout_ms() -> u64 {
    10000
}

fn default_mtu() -> usize {
    1400
}

fn default_quic_port() -> u16 {
    4433
}

fn default_data_port() -> u16 {
    4434
}

fn default_strategy() -> String {
    "nearest".into()
}

fn default_health_check_ms() -> u64 {
    10000
}

fn default_max_failover() -> usize {
    3
}

fn default_min_samples() -> usize {
    50
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            tunnel: TunnelConfig::default(),
            proxy: ProxyConfig::default(),
            route: RouteConfig::default(),
            ml: MlConfig::default(),
        }
    }
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            telemetry: false,
            interface: None,
        }
    }
}

impl Default for TunnelConfig {
    fn default() -> Self {
        Self {
            keepalive_ms: default_keepalive_ms(),
            timeout_ms: default_timeout_ms(),
            mtu: default_mtu(),
        }
    }
}

impl Default for ProxyConfig {
    fn default() -> Self {
        Self {
            servers: vec![],
            quic_port: default_quic_port(),
            data_port: default_data_port(),
        }
    }
}

impl Default for RouteConfig {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            multipath: false,
            health_check_ms: default_health_check_ms(),
            max_failover: default_max_failover(),
        }
    }
}

impl Default for MlConfig {
    fn default() -> Self {
        Self {
            model_path: None,
            online_learning: false,
            min_samples: default_min_samples(),
        }
    }
}

impl Config {
    /// Load configuration from a TOML file.
    pub fn load(path: &str) -> anyhow::Result<Self> {
        let path = Path::new(path);
        if !path.exists() {
            anyhow::bail!("Config file not found: {}", path.display());
        }
        let content = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to a TOML file.
    pub fn save(&self, path: &str) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}
