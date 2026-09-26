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

    /// Interception backend selection settings.
    #[serde(default)]
    pub interception: InterceptionConfig,
}

/// General application settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    /// Log level (trace, debug, info, warn, error).
    #[serde(default = "default_log_level")]
    pub log_level: String,

    /// Share anonymous aggregate latency statistics with the relay.
    ///
    /// **On by default (opt-out).** Only aggregate RTT percentiles, jitter,
    /// and FEC counters are sent; never an IP address, identifier, token, or
    /// packet content. Set to `false` (or pass `--no-telemetry`) to disable.
    #[serde(default = "default_telemetry")]
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

    /// Mark tunnel packets with DSCP Expedited Forwarding (EF, 46).
    ///
    /// **Off by default.** Only a player's own router or ISP can act on this,
    /// every later hop may ignore or reclassify it, and stamping EF on traffic
    /// without the network's consent can be policed. Enable with `--dscp`.
    /// Ignored on Windows, where marking requires the QWAVE/QoS2 API.
    #[serde(default)]
    pub dscp: bool,

    /// Enable adaptive FEC and loss-gated packet duplication.
    ///
    /// **Off by default.** With this off, FEC keeps the fixed block codec and
    /// multipath duplication behaves as before. With it on, the client sends no
    /// parity on a clean link, returns to low overhead quickly once loss stops,
    /// and duplicates a packet only while loss or jitter is measured.
    #[serde(default)]
    pub adaptive_fec: bool,

    /// Hard ceiling on adaptive FEC parity overhead, in percent (default 25).
    ///
    /// Bounds the effective block size to `ceil(100 / pct)`, so even under
    /// sustained loss the adaptive mode never adds more parity than this.
    #[serde(default = "default_fec_max_overhead_pct")]
    pub fec_max_overhead_pct: u32,

    /// Discover the real client-to-relay path MTU (DPLPMTUD) and raise the
    /// tunnel payload budget when the path supports it.
    ///
    /// **Off by default.** With discovery off the client uses the fixed
    /// conservative clamp unchanged. When on, the already-encrypted QUIC
    /// control plane's bounded DPLPMTUD result raises the effective MTU up to a
    /// 1500-byte hard cap, and any failure or ambiguity falls back to the
    /// clamp. Enable with `--path-mtu-discovery`.
    #[serde(default)]
    pub path_mtu_discovery: bool,
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
    /// Route selection strategy: "destination" (default), "nearest",
    /// "multipath", or "ml". "destination" ranks relays by the whole path;
    /// "nearest" opts into client-proximity selection. Unknown values fall
    /// back to "nearest" with a warning. The "multipath" value selects
    /// destination-aware routing; the multipath spread itself is controlled
    /// by the `multipath` flag below.
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

    /// Do-no-harm bypass gate mode: "dry_run" (default), "auto", "never", or
    /// "always".
    ///
    /// The gate tunnels a game session only when the relay actually helps.
    /// **The default is `dry_run`:** it measures and logs the decision it would
    /// make and changes nothing. `auto` lets it act, but only where acting is
    /// safe (see the module docs in `interceptor::bypass`). `never` is the
    /// complete rollback. `always` forces direct and is for testing only.
    #[serde(default = "default_bypass")]
    pub bypass: String,

    /// Relay-worse threshold in milliseconds: at or below this advantage the
    /// gate may prefer the direct path.
    #[serde(default = "default_bypass_margin_ms")]
    pub bypass_margin_ms: f32,

    /// Relay-better threshold in milliseconds: at or above this advantage the
    /// relay is kept.
    #[serde(default = "default_bypass_keep_ms")]
    pub bypass_keep_ms: f32,

    /// Quiet period after a bypass decision, in seconds.
    #[serde(default = "default_bypass_dwell_s")]
    pub bypass_dwell_s: u64,

    /// Minimum probation time before a verdict, in seconds.
    #[serde(default = "default_bypass_probation_s")]
    pub bypass_probation_s: u64,
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

/// Interception backend selection settings.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterceptionConfig {
    /// Interception mode: "auto", "userspace", or "kernel".
    #[serde(default = "default_interception_mode")]
    pub mode: String,
}

// Default value functions

fn default_log_level() -> String {
    "info".into()
}

fn default_telemetry() -> bool {
    true
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
    "destination".into()
}

fn default_health_check_ms() -> u64 {
    10000
}

fn default_max_failover() -> usize {
    3
}

fn default_bypass() -> String {
    "dry_run".into()
}

fn default_bypass_margin_ms() -> f32 {
    8.0
}

fn default_bypass_keep_ms() -> f32 {
    5.0
}

fn default_bypass_dwell_s() -> u64 {
    120
}

fn default_bypass_probation_s() -> u64 {
    20
}

fn default_multipath_max_paths() -> u8 {
    2
}

fn default_fec_max_overhead_pct() -> u32 {
    25
}

fn default_min_samples() -> usize {
    50
}

fn default_interception_mode() -> String {
    "auto".into()
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            log_level: default_log_level(),
            telemetry: default_telemetry(),
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
            dscp: false,
            adaptive_fec: false,
            fec_max_overhead_pct: default_fec_max_overhead_pct(),
            path_mtu_discovery: false,
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
            bypass: default_bypass(),
            bypass_margin_ms: default_bypass_margin_ms(),
            bypass_keep_ms: default_bypass_keep_ms(),
            bypass_dwell_s: default_bypass_dwell_s(),
            bypass_probation_s: default_bypass_probation_s(),
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

impl Default for InterceptionConfig {
    fn default() -> Self {
        Self {
            mode: default_interception_mode(),
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
        assert!(config.general.telemetry);
        assert!(config.general.interface.is_none());
        assert_eq!(config.tunnel.keepalive_ms, 5000);
        assert_eq!(config.tunnel.timeout_ms, 10000);
        assert_eq!(config.tunnel.mtu, 1400);
        assert!(!config.tunnel.dscp);
        assert!(!config.tunnel.adaptive_fec);
        assert_eq!(config.tunnel.fec_max_overhead_pct, 25);
        assert!(!config.tunnel.path_mtu_discovery);
        assert!(config.proxy.servers.is_empty());
        assert_eq!(config.proxy.quic_port, 4433);
        assert_eq!(config.proxy.data_port, 4434);
        assert_eq!(config.route.strategy, "destination");
        assert!(!config.route.multipath);
        assert_eq!(config.route.health_check_ms, 10000);
        assert_eq!(config.route.max_failover, 3);
        assert_eq!(config.route.bypass, "dry_run");
        assert_eq!(config.route.bypass_margin_ms, 8.0);
        assert_eq!(config.route.bypass_keep_ms, 5.0);
        assert_eq!(config.route.bypass_dwell_s, 120);
        assert_eq!(config.route.bypass_probation_s, 20);
        assert!(config.ml.model_path.is_none());
        assert!(!config.ml.online_learning);
        assert_eq!(config.ml.min_samples, 50);
        assert_eq!(config.interception.mode, "auto");
    }

    #[test]
    fn test_bypass_defaults_to_dry_run_and_accepts_every_mode() {
        let defaulted: Config = toml::from_str("[route]\nstrategy = \"nearest\"\n").unwrap();
        assert_eq!(
            defaulted.route.bypass, "dry_run",
            "the bypass gate must be safe (no-op) by default"
        );

        for mode in &["auto", "never", "always", "dry_run"] {
            let toml_str = format!("[route]\nbypass = \"{}\"\n", mode);
            let config: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(config.route.bypass, *mode);
        }
    }

    #[test]
    fn test_sub_config_defaults() {
        let general = GeneralConfig::default();
        assert_eq!(general.log_level, "info");
        assert!(general.telemetry);
        assert!(general.interface.is_none());

        let tunnel = TunnelConfig::default();
        assert_eq!(tunnel.keepalive_ms, 5000);
        assert_eq!(tunnel.timeout_ms, 10000);
        assert_eq!(tunnel.mtu, 1400);
        assert!(!tunnel.dscp);
        assert!(!tunnel.adaptive_fec);
        assert_eq!(tunnel.fec_max_overhead_pct, 25);
        assert!(!tunnel.path_mtu_discovery);

        let proxy = ProxyConfig::default();
        assert!(proxy.servers.is_empty());
        assert_eq!(proxy.quic_port, 4433);
        assert_eq!(proxy.data_port, 4434);

        let route = RouteConfig::default();
        assert_eq!(route.strategy, "destination");
        assert!(!route.multipath);
        assert_eq!(route.health_check_ms, 10000);
        assert_eq!(route.max_failover, 3);
        assert_eq!(route.bypass, "dry_run");
        assert_eq!(route.bypass_margin_ms, 8.0);
        assert_eq!(route.bypass_keep_ms, 5.0);
        assert_eq!(route.bypass_dwell_s, 120);
        assert_eq!(route.bypass_probation_s, 20);

        let ml = MlConfig::default();
        assert!(ml.model_path.is_none());
        assert!(!ml.online_learning);
        assert_eq!(ml.min_samples, 50);

        let interception = InterceptionConfig::default();
        assert_eq!(interception.mode, "auto");
    }

    // ── TOML round-trip tests ──────────────────────────────────────────

    #[test]
    fn test_dscp_defaults_off_and_can_be_enabled() {
        let defaulted: Config = toml::from_str("[tunnel]\nkeepalive_ms = 5000\n").unwrap();
        assert!(!defaulted.tunnel.dscp, "DSCP marking must be opt-in");

        let opted_in: Config = toml::from_str("[tunnel]\ndscp = true\n").unwrap();
        assert!(opted_in.tunnel.dscp);
    }

    #[test]
    fn adaptive_fec_defaults_off_and_accepts_a_ceiling() {
        let defaulted: Config = toml::from_str("[tunnel]\nkeepalive_ms = 5000\n").unwrap();
        assert!(
            !defaulted.tunnel.adaptive_fec,
            "adaptive FEC must be opt-in"
        );
        assert_eq!(defaulted.tunnel.fec_max_overhead_pct, 25);

        let opted_in: Config =
            toml::from_str("[tunnel]\nadaptive_fec = true\nfec_max_overhead_pct = 50\n").unwrap();
        assert!(opted_in.tunnel.adaptive_fec);
        assert_eq!(opted_in.tunnel.fec_max_overhead_pct, 50);
    }

    #[test]
    fn path_mtu_discovery_defaults_off_and_can_be_enabled() {
        let defaulted: Config = toml::from_str("[tunnel]\nkeepalive_ms = 5000\n").unwrap();
        assert!(
            !defaulted.tunnel.path_mtu_discovery,
            "path-MTU discovery must be opt-in so the default is today's clamp"
        );

        let opted_in: Config = toml::from_str("[tunnel]\npath_mtu_discovery = true\n").unwrap();
        assert!(opted_in.tunnel.path_mtu_discovery);
    }

    #[test]
    fn test_telemetry_defaults_on_and_can_be_disabled() {
        // Absent key => on by default (opt-out).
        let defaulted: Config = toml::from_str("[general]\nlog_level = \"info\"\n").unwrap();
        assert!(defaulted.general.telemetry);

        // Explicit opt-out is honoured.
        let opted_out: Config = toml::from_str("[general]\ntelemetry = false\n").unwrap();
        assert!(!opted_out.general.telemetry);

        // Explicit opt-in stays true.
        let opted_in: Config = toml::from_str("[general]\ntelemetry = true\n").unwrap();
        assert!(opted_in.general.telemetry);
    }

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
        assert!(config.general.telemetry); // default preserved (on by default)
        assert_eq!(config.tunnel.keepalive_ms, 10000);
        assert_eq!(config.tunnel.timeout_ms, 10000); // default preserved
        assert_eq!(config.route.strategy, "destination"); // default preserved
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
    fn test_interception_mode_values() {
        for mode in &["auto", "userspace", "kernel"] {
            let toml_str = format!("[interception]\nmode = \"{}\"\n", mode);
            let config: Config = toml::from_str(&toml_str).unwrap();
            assert_eq!(config.interception.mode, *mode);
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

    // ── Shipped example config ─────────────────────────────────────────

    #[test]
    fn test_example_config_parses_with_documented_defaults() {
        let example = include_str!("../lightspeed.example.toml");
        let config: Config = toml::from_str(example).expect("example config must parse");

        assert_eq!(config.general.log_level, "info");
        assert!(config.general.telemetry);
        assert!(config.general.interface.is_none());

        assert_eq!(config.tunnel.keepalive_ms, 5000);
        assert_eq!(config.tunnel.timeout_ms, 10000);
        assert_eq!(config.tunnel.mtu, 1400);
        assert_eq!(config.tunnel.transport, "udp");
        assert!(!config.tunnel.adaptive_fec);
        assert_eq!(config.tunnel.fec_max_overhead_pct, 25);
        assert!(!config.tunnel.path_mtu_discovery);

        assert!(config.proxy.servers.is_empty());
        assert_eq!(config.proxy.quic_port, 4433);
        assert_eq!(config.proxy.data_port, 4434);

        assert!(config.registry.url.is_none());
        assert!(config.registry.operator_key.is_none());

        assert_eq!(config.route.strategy, "destination");
        assert!(!config.route.multipath);
        assert_eq!(config.route.multipath_max_paths, 2);
        assert_eq!(config.route.health_check_ms, 10000);
        assert_eq!(config.route.max_failover, 3);
        assert_eq!(config.route.bypass, "dry_run");
        assert_eq!(config.route.bypass_margin_ms, 8.0);
        assert_eq!(config.route.bypass_keep_ms, 5.0);
        assert_eq!(config.route.bypass_dwell_s, 120);
        assert_eq!(config.route.bypass_probation_s, 20);

        assert!(config.ml.model_path.is_none());
        assert!(!config.ml.online_learning);
        assert_eq!(config.ml.min_samples, 50);

        assert_eq!(config.interception.mode, "auto");
    }
}
