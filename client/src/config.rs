//! # LightSpeed Client Configuration
//!
//! Manages client configuration from file and defaults.

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level client configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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

    /// Community registry settings.
    #[serde(default)]
    pub registry: RegistryConfig,

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

    /// Transport for the client→proxy leg: "udp" (default) or "tcp".
    #[serde(default = "default_transport")]
    pub transport: String,
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

/// Community registry settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RegistryConfig {
    /// Registry URL (signed node list). Empty = disabled.
    #[serde(default)]
    pub url: Option<String>,

    /// Operator Ed25519 public key (base64) used to verify the registry.
    #[serde(default)]
    pub operator_key: Option<String>,
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

    /// Maximum number of simultaneous relay paths when multipath is enabled.
    #[serde(default = "default_multipath_max_paths")]
    pub multipath_max_paths: u8,

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

fn default_transport() -> String {
    "udp".into()
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

fn default_multipath_max_paths() -> u8 {
    2
}

fn default_min_samples() -> usize {
    50
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
            transport: default_transport(),
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
            multipath_max_paths: default_multipath_max_paths(),
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

#[cfg(test)]
mod tests {
    use super::*;

    // ── Default value tests ────────────────────────────────────────────

    #[test]
    fn test_config_default() {
        let config = Config::default();
        assert_eq!(config.general.log_level, "info");
        assert!(!config.general.telemetry);
        assert!(config.general.interface.is_none());
        assert_eq!(config.tunnel.keepalive_ms, 5000);
        assert_eq!(config.tunnel.timeout_ms, 10000);
        assert_eq!(config.tunnel.mtu, 1400);
        assert!(config.proxy.servers.is_empty());
        assert_eq!(config.proxy.quic_port, 4433);
        assert_eq!(config.proxy.data_port, 4434);
        assert_eq!(config.route.strategy, "nearest");
        assert!(!config.route.multipath);
        assert_eq!(config.route.health_check_ms, 10000);
        assert_eq!(config.route.max_failover, 3);
        assert!(config.ml.model_path.is_none());
        assert!(!config.ml.online_learning);
        assert_eq!(config.ml.min_samples, 50);
    }

    #[test]
    fn test_sub_config_defaults() {
        let general = GeneralConfig::default();
        assert_eq!(general.log_level, "info");
        assert!(!general.telemetry);
        assert!(general.interface.is_none());

        let tunnel = TunnelConfig::default();
        assert_eq!(tunnel.keepalive_ms, 5000);
        assert_eq!(tunnel.timeout_ms, 10000);
        assert_eq!(tunnel.mtu, 1400);

        let proxy = ProxyConfig::default();
        assert!(proxy.servers.is_empty());
        assert_eq!(proxy.quic_port, 4433);
        assert_eq!(proxy.data_port, 4434);

        let route = RouteConfig::default();
        assert_eq!(route.strategy, "nearest");
        assert!(!route.multipath);
        assert_eq!(route.health_check_ms, 10000);
        assert_eq!(route.max_failover, 3);

        let ml = MlConfig::default();
        assert!(ml.model_path.is_none());
        assert!(!ml.online_learning);
        assert_eq!(ml.min_samples, 50);
    }

    // ── TOML round-trip tests ──────────────────────────────────────────

    #[test]
    fn test_empty_config_toml_roundtrip() {
        let toml_str = "";
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.log_level, "info");
        assert_eq!(config.tunnel.keepalive_ms, 5000);

        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.general.log_level, "info");
    }

    #[test]
    fn test_partial_config_toml() {
        let toml_str = r#"
[general]
log_level = "debug"
interface = "eth0"

[tunnel]
keepalive_ms = 10000
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.log_level, "debug");
        assert_eq!(config.general.interface.as_deref(), Some("eth0"));
        assert!(!config.general.telemetry); // default preserved
        assert_eq!(config.tunnel.keepalive_ms, 10000);
        assert_eq!(config.tunnel.timeout_ms, 10000); // default preserved
        assert_eq!(config.route.strategy, "nearest"); // default preserved
    }

    #[test]
    fn test_full_config_toml_roundtrip() {
        let toml_str = r#"
[general]
log_level = "trace"
telemetry = true
interface = "Ethernet"

[tunnel]
keepalive_ms = 2000
timeout_ms = 5000
mtu = 1200

[proxy]
servers = ["10.0.0.1:4434", "10.0.0.2:4434"]
quic_port = 8443
data_port = 8444

[route]
strategy = "ml"
multipath = true
health_check_ms = 5000
max_failover = 5

[ml]
model_path = "/path/to/model.bin"
online_learning = true
min_samples = 100
"#;
        let config: Config = toml::from_str(toml_str).unwrap();

        assert_eq!(config.general.log_level, "trace");
        assert!(config.general.telemetry);
        assert_eq!(config.general.interface.as_deref(), Some("Ethernet"));
        assert_eq!(config.tunnel.keepalive_ms, 2000);
        assert_eq!(config.tunnel.timeout_ms, 5000);
        assert_eq!(config.tunnel.mtu, 1200);
        assert_eq!(config.proxy.servers, vec!["10.0.0.1:4434", "10.0.0.2:4434"]);
        assert_eq!(config.proxy.quic_port, 8443);
        assert_eq!(config.proxy.data_port, 8444);
        assert_eq!(config.route.strategy, "ml");
        assert!(config.route.multipath);
        assert_eq!(config.route.health_check_ms, 5000);
        assert_eq!(config.route.max_failover, 5);
        assert_eq!(config.ml.model_path.as_deref(), Some("/path/to/model.bin"));
        assert!(config.ml.online_learning);
        assert_eq!(config.ml.min_samples, 100);

        // Round-trip
        let serialized = toml::to_string_pretty(&config).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.general.log_level, "trace");
        assert_eq!(deserialized.route.strategy, "ml");
        assert_eq!(deserialized.ml.min_samples, 100);
    }

    #[test]
    fn test_serialize_preserves_values() {
        let mut config = Config::default();
        config.tunnel.keepalive_ms = 3000;
        config.route.strategy = "multipath".into();

        let serialized = toml::to_string_pretty(&config).unwrap();
        assert!(serialized.contains("keepalive_ms = 3000"));
        assert!(serialized.contains("strategy = \"multipath\""));

        let deserialized: Config = toml::from_str(&serialized).unwrap();
        assert_eq!(deserialized.tunnel.keepalive_ms, 3000);
        assert_eq!(deserialized.route.strategy, "multipath");
    }

    // ── Invalid TOML tests ────────────────────────────────────────────

    #[test]
    fn test_invalid_toml_syntax() {
        let result: Result<Config, _> = toml::from_str("this is not valid toml == ---");
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_field_graceful() {
        // TOML with unknown fields should work (serde ignores by default)
        let toml_str = r#"
[general]
log_level = "info"
unknown_field = "should_be_ignored"

[unknown_section]
foo = "bar"
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_ok());
    }

    #[test]
    fn test_wrong_type_for_field() {
        let toml_str = r#"
[tunnel]
keepalive_ms = "not_a_number"
"#;
        let result: Result<Config, _> = toml::from_str(toml_str);
        assert!(result.is_err());
    }

    // ── File-based load/save tests ─────────────────────────────────────

    #[test]
    fn test_load_nonexistent_file() {
        let result = Config::load("/nonexistent/path/config.toml");
        assert!(result.is_err());
    }

    #[test]
    fn test_save_and_load_roundtrip() {
        let mut config = Config::default();
        config.general.log_level = "debug".into();
        config.tunnel.keepalive_ms = 7500;
        config.proxy.servers = vec!["10.0.0.1:4434".into()];
        config.route.strategy = "ml".into();
        config.ml.online_learning = true;

        let tmp_dir = std::env::temp_dir();
        let tmp_file = tmp_dir.join("lightspeed_test_config.toml");
        let path_str = tmp_file.to_str().unwrap();

        // Save
        config.save(path_str).unwrap();

        // Load
        let loaded = Config::load(path_str).unwrap();

        assert_eq!(loaded.general.log_level, "debug");
        assert_eq!(loaded.tunnel.keepalive_ms, 7500);
        assert_eq!(loaded.proxy.servers, vec!["10.0.0.1:4434"]);
        assert_eq!(loaded.route.strategy, "ml");
        assert!(loaded.ml.online_learning);

        // Clean up
        let _ = std::fs::remove_file(&tmp_file);
    }

    #[test]
    fn test_route_strategy_values() {
        for strategy in &["nearest", "ml", "multipath"] {
            let toml_str = format!("[route]\nstrategy = \"{}\"\n", strategy);
            let config: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(config.route.strategy, *strategy);
        }
    }

    #[test]
    fn test_log_level_values() {
        for level in &["trace", "debug", "info", "warn", "error"] {
            let toml_str = format!("[general]\nlog_level = \"{}\"\n", level);
            let config: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(config.general.log_level, *level);
        }
    }

    #[test]
    fn test_proxy_servers_multiple() {
        let toml_str = r#"
[proxy]
servers = [
    "proxy1.example.com:4434",
    "proxy2.example.com:4434",
    "proxy3.example.com:4434",
]
"#;
        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.proxy.servers.len(), 3);
        assert_eq!(config.proxy.servers[0], "proxy1.example.com:4434");
        assert_eq!(config.proxy.servers[2], "proxy3.example.com:4434");
    }

    #[test]
    fn test_toml_roundtrip_preserves_whitespace_semantics() {
        let original = Config::default();
        let serialized = toml::to_string_pretty(&original).unwrap();
        let deserialized: Config = toml::from_str(&serialized).unwrap();

        // All fields should survive the round-trip
        assert_eq!(deserialized.general.log_level, original.general.log_level);
        assert_eq!(
            deserialized.tunnel.keepalive_ms,
            original.tunnel.keepalive_ms
        );
        assert_eq!(deserialized.proxy.quic_port, original.proxy.quic_port);
        assert_eq!(deserialized.route.strategy, original.route.strategy);
        assert_eq!(deserialized.ml.min_samples, original.ml.min_samples);
    }
}
