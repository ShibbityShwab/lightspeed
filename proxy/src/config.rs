//! # Proxy Server Configuration

use serde::{Deserialize, Serialize};
use std::path::Path;

/// Top-level proxy configuration.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProxyConfig {
    /// Server identity.
    #[serde(default)]
    pub server: ServerConfig,

    /// Network bind configuration.
    #[serde(default)]
    pub network: NetworkConfig,

    /// Security settings.
    #[serde(default)]
    pub security: SecurityConfig,

    /// Rate limiting settings.
    #[serde(default)]
    pub rate_limit: RateLimitConfig,

    /// Per-relay egress budget guard (opt-in, disabled by default).
    #[serde(default)]
    pub budget: BudgetConfig,

    /// Metrics settings.
    #[serde(default)]
    pub metrics: MetricsConfig,

    /// Proxy-side IP-to-country aggregation settings.
    #[serde(default)]
    pub geo: GeoConfig,
}

/// Network bind configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// UDP data plane port.
    #[serde(default = "default_data_port")]
    pub data_port: u16,

    /// QUIC control plane port.
    #[serde(default = "default_control_port")]
    pub control_port: u16,

    /// HTTP health/metrics port.
    #[serde(default = "default_health_port")]
    pub health_port: u16,

    /// Enable the TCP listener on the data plane port for UDP-restricted
    /// networks. TCP and UDP can coexist on the same port number.
    #[serde(default = "default_true")]
    pub tcp_enabled: bool,

    /// Maximum concurrent TCP client connections.
    #[serde(default = "default_tcp_max_connections")]
    pub tcp_max_connections: usize,

    /// TCP read timeout in seconds (idle connections are closed).
    #[serde(default = "default_tcp_read_timeout")]
    pub tcp_read_timeout_secs: u64,
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

    /// Public IP of this relay. When set, a session whose destination is this
    /// address is a self-tunnel and is excluded from geo aggregation.
    #[serde(default)]
    pub public_ip: Option<String>,
}

/// Proxy-side IP-to-country aggregation configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoConfig {
    /// Aggregate proxy-observed session country pairs into metrics.
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Path to a MaxMind DB (MMDB) country database. The file is read fully
    /// into memory, so it may be replaced on disk while the proxy runs.
    #[serde(default = "default_geo_mmdb_path")]
    pub mmdb_path: String,
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

    /// Maximum aggregate packets per second across all source ports for one
    /// client IP. Bounds the per-IP tier, which protects against source-port
    /// rotation that defeats the per-flow tier.
    #[serde(default = "default_pps_per_ip")]
    pub max_pps_per_ip: u64,

    /// Maximum aggregate bandwidth (bytes/sec) across all source ports for one
    /// client IP.
    #[serde(default = "default_bps_per_ip")]
    pub max_bps_per_ip: u64,
}

/// Per-relay egress budget guard configuration.
///
/// Disabled by default: with `enabled = false` the relay behaves exactly as it
/// did before this guard existed. When enabled, the guard watches the relay's
/// cumulative egress (bytes sent to game servers plus bytes sent back to
/// clients) and can warn at a soft threshold and refuse new sessions at a hard
/// threshold. Existing sessions are never interrupted.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetConfig {
    /// Master switch.
    #[serde(default)]
    pub enabled: bool,

    /// Cumulative egress byte ceiling since the process started. `0` disables
    /// the flat ceiling.
    #[serde(default)]
    pub max_bytes: u64,

    /// Monthly egress allowance in bytes, for metered plans billed per
    /// calendar month. `0` disables it. When both a flat ceiling and a monthly
    /// allowance are set, whichever has the higher utilization is binding.
    #[serde(default)]
    pub monthly_bytes: u64,

    /// Soft threshold as a percentage of the binding limit. At or above it the
    /// relay logs a warning and flips the soft-exceeded metric once. `0`
    /// disables the soft warning.
    #[serde(default = "default_soft_threshold_pct")]
    pub soft_threshold_pct: u8,

    /// Hard threshold as a percentage of the binding limit. At or above it the
    /// relay refuses new sessions while existing sessions keep relaying. `0`
    /// disables the hard stop.
    #[serde(default = "default_hard_threshold_pct")]
    pub hard_threshold_pct: u8,
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
    #[serde(default = "default_true")]
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

    /// Optional destination allowlist (CIDR strings, e.g. "104.26.0.0/16").
    /// Empty = allow any public destination. Non-empty = only relay to these
    /// prefixes. Recommended for community relays to prevent open-relay abuse.
    #[serde(default)]
    pub destination_allowlist: Vec<String>,
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
            require_auth: true,
            max_amplification_ratio: default_amplification_ratio(),
            max_destinations_per_window: default_max_destinations(),
            ban_duration_secs: default_ban_duration(),
            destination_allowlist: Vec::new(),
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
fn default_data_port() -> u16 {
    4434
}
fn default_control_port() -> u16 {
    4433
}
fn default_health_port() -> u16 {
    8080
}
fn default_pps_limit() -> u64 {
    1000
}
fn default_bps_limit() -> u64 {
    1_000_000
} // 1 MB/s
fn default_pps_per_ip() -> u64 {
    5000
}
fn default_bps_per_ip() -> u64 {
    5_000_000
} // 5 MB/s
fn default_soft_threshold_pct() -> u8 {
    80
}
fn default_hard_threshold_pct() -> u8 {
    100
}
fn default_true() -> bool {
    true
}
fn default_metrics_interval() -> u64 {
    10
}
fn default_tcp_max_connections() -> usize {
    256
}
fn default_tcp_read_timeout() -> u64 {
    10
}
fn default_geo_mmdb_path() -> String {
    "/opt/lightspeed/geoip/dbip-country-lite.mmdb".into()
}

/// Whether an environment flag value means "on".
///
/// Empty and the usual falsy spellings are off; any other value is on.
fn env_flag_truthy(raw: &str) -> bool {
    !matches!(
        raw.trim().to_ascii_lowercase().as_str(),
        "" | "0" | "false" | "no" | "off"
    )
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            node_id: default_node_id(),
            region: default_region(),
            max_clients: default_max_clients(),
            public_ip: None,
        }
    }
}

impl Default for GeoConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mmdb_path: default_geo_mmdb_path(),
        }
    }
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self {
            data_port: default_data_port(),
            control_port: default_control_port(),
            health_port: default_health_port(),
            tcp_enabled: true,
            tcp_max_connections: default_tcp_max_connections(),
            tcp_read_timeout_secs: default_tcp_read_timeout(),
        }
    }
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_pps_per_client: default_pps_limit(),
            max_bps_per_client: default_bps_limit(),
            max_pps_per_ip: default_pps_per_ip(),
            max_bps_per_ip: default_bps_per_ip(),
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

impl Default for BudgetConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            max_bytes: 0,
            monthly_bytes: 0,
            soft_threshold_pct: default_soft_threshold_pct(),
            hard_threshold_pct: default_hard_threshold_pct(),
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

    /// Apply environment overrides on top of the loaded configuration.
    ///
    /// `LIGHTSPEED_GEO_DISABLED` (truthy) disables geo aggregation, and
    /// `LIGHTSPEED_GEO_MMDB_PATH` overrides the database path. Applied after
    /// [`Self::load`] so an operator can toggle geo without editing the file.
    pub fn apply_env_overrides(&mut self) {
        self.apply_env_overrides_with(|key| std::env::var(key).ok());
    }

    /// [`Self::apply_env_overrides`] with an injected environment lookup so
    /// tests never touch process-global environment state.
    pub fn apply_env_overrides_with<F>(&mut self, get: F)
    where
        F: Fn(&str) -> Option<String>,
    {
        if let Some(raw) = get("LIGHTSPEED_GEO_DISABLED") {
            self.geo.enabled = !env_flag_truthy(&raw);
        }
        if let Some(path) = get("LIGHTSPEED_GEO_MMDB_PATH") {
            if !path.trim().is_empty() {
                self.geo.mmdb_path = path;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_defaults() {
        let n = NetworkConfig::default();
        assert_eq!(n.data_port, 4434);
        assert_eq!(n.control_port, 4433);
        assert_eq!(n.health_port, 8080);
    }

    #[test]
    fn test_parse_network_ports() {
        let config: ProxyConfig = toml::from_str(
            r#"
[network]
data_port = 5555
health_port = 9000
"#,
        )
        .unwrap();
        assert_eq!(config.network.data_port, 5555);
        assert_eq!(config.network.health_port, 9000);
        assert_eq!(config.network.control_port, 4433);
    }

    #[test]
    fn test_rate_limit_unknown_key_ignored_and_ip_defaults() {
        // A config file written before `max_connections` was removed must still
        // parse: serde ignores unknown keys.
        let config: ProxyConfig = toml::from_str(
            r#"
[rate_limit]
max_connections = 5
"#,
        )
        .expect("TOML containing the removed max_connections key must still parse");

        assert_eq!(config.rate_limit.max_pps_per_ip, 5000);
        assert_eq!(config.rate_limit.max_bps_per_ip, 5_000_000);
    }

    #[test]
    fn test_parse_geo_section_and_public_ip() {
        let config: ProxyConfig = toml::from_str(
            r#"
[server]
public_ip = "203.0.113.7"

[geo]
enabled   = false
mmdb_path = "/tmp/custom.mmdb"
"#,
        )
        .unwrap();

        assert_eq!(config.server.public_ip.as_deref(), Some("203.0.113.7"));
        assert!(!config.geo.enabled);
        assert_eq!(config.geo.mmdb_path, "/tmp/custom.mmdb");
    }

    #[test]
    fn test_geo_defaults_and_optional_public_ip() {
        let config: ProxyConfig = toml::from_str("").unwrap();

        assert!(config.server.public_ip.is_none());
        assert!(config.geo.enabled);
        assert_eq!(
            config.geo.mmdb_path,
            "/opt/lightspeed/geoip/dbip-country-lite.mmdb"
        );
    }

    #[test]
    fn test_geo_env_overrides() {
        let mut config = ProxyConfig::default();
        config.apply_env_overrides_with(|key| match key {
            "LIGHTSPEED_GEO_DISABLED" => Some("1".to_string()),
            "LIGHTSPEED_GEO_MMDB_PATH" => Some("/tmp/env.mmdb".to_string()),
            _ => None,
        });

        assert!(!config.geo.enabled);
        assert_eq!(config.geo.mmdb_path, "/tmp/env.mmdb");
    }

    #[test]
    fn test_budget_defaults_to_disabled() {
        let config = ProxyConfig::default();
        assert!(!config.budget.enabled);
        assert_eq!(config.budget.max_bytes, 0);
        assert_eq!(config.budget.monthly_bytes, 0);
        assert_eq!(config.budget.soft_threshold_pct, 80);
        assert_eq!(config.budget.hard_threshold_pct, 100);

        let empty: ProxyConfig = toml::from_str("").unwrap();
        assert!(!empty.budget.enabled);
    }

    #[test]
    fn test_parse_budget_section() {
        let config: ProxyConfig = toml::from_str(
            r#"
[budget]
enabled             = true
max_bytes           = 1073741824
monthly_bytes       = 5368709120
soft_threshold_pct  = 75
hard_threshold_pct  = 95
"#,
        )
        .unwrap();

        assert!(config.budget.enabled);
        assert_eq!(config.budget.max_bytes, 1_073_741_824);
        assert_eq!(config.budget.monthly_bytes, 5_368_709_120);
        assert_eq!(config.budget.soft_threshold_pct, 75);
        assert_eq!(config.budget.hard_threshold_pct, 95);
    }
}
