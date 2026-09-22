//! Persistent GUI configuration (`config.toml`).
//!
//! Stores the proxy/relay list and the selected relay. Loading is tolerant: a
//! missing or malformed file yields defaults instead of an error, and the file
//! is only ever written by an explicit user action or a successful discovery
//! refresh - never on startup with placeholder data.

use std::net::SocketAddrV4;
use std::path::Path;

use toml::{Table, Value};

/// A proxy relay entry shown in the Boost Server selector and Proxy Manager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyEntry {
    pub addr: SocketAddrV4,
    pub label: String,
    /// Registry node id (`relay-lax-1`) for discovered relays; `None` for
    /// user-added custom proxies.
    pub node_id: Option<String>,
}

impl ProxyEntry {
    /// A user-added proxy (kept across discovery refreshes).
    pub fn custom(addr: SocketAddrV4, label: impl Into<String>) -> Self {
        Self {
            addr,
            label: label.into(),
            node_id: None,
        }
    }

    /// A relay discovered through the signed registry.
    pub fn discovered(
        node_id: impl Into<String>,
        addr: SocketAddrV4,
        label: impl Into<String>,
    ) -> Self {
        Self {
            addr,
            label: label.into(),
            node_id: Some(node_id.into()),
        }
    }

    /// Whether this entry came from the registry (and may be refreshed).
    pub fn is_discovered(&self) -> bool {
        self.node_id.is_some()
    }
}

/// Persisted GUI state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GuiConfig {
    /// Selected relay address (`"ip:port"`), if any.
    pub selected: Option<String>,
    pub proxies: Vec<ProxyEntry>,
    /// When true, the GUI connects to the lowest-latency relay it can reach and
    /// reconsiders on each discovery, so newly added relays are picked up.
    pub auto_select: bool,
    /// When true, share anonymous aggregate latency statistics with the relay.
    /// On by default; no IP address, identifier, or packet content is sent.
    pub share_latency_stats: bool,
}

impl Default for GuiConfig {
    fn default() -> Self {
        Self {
            selected: None,
            proxies: Vec::new(),
            auto_select: true,
            share_latency_stats: true,
        }
    }
}

/// Load the config at `path`, falling back to defaults on any error.
pub fn load(path: &Path) -> GuiConfig {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse(&text) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!("Ignoring malformed config {}: {e}", path.display());
                GuiConfig::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => GuiConfig::default(),
        Err(e) => {
            tracing::warn!("Could not read config {}: {e}", path.display());
            GuiConfig::default()
        }
    }
}

/// Parse a `config.toml` body. Unknown keys are ignored; entries with an
/// unparseable `addr` are skipped rather than failing the whole file.
pub fn parse(text: &str) -> Result<GuiConfig, String> {
    let table: Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let selected = table
        .get("selected")
        .and_then(Value::as_str)
        .map(str::to_string);
    let auto_select = table
        .get("auto_select")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let share_latency_stats = table
        .get("share_latency_stats")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let mut proxies = Vec::new();
    if let Some(items) = table.get("proxies").and_then(Value::as_array) {
        for item in items {
            let Some(entry) = item.as_table() else {
                continue;
            };
            let Some(addr_str) = entry.get("addr").and_then(Value::as_str) else {
                continue;
            };
            let Ok(addr) = addr_str.parse::<SocketAddrV4>() else {
                tracing::warn!("Skipping proxy with invalid address: {addr_str}");
                continue;
            };
            let label = entry
                .get("label")
                .and_then(Value::as_str)
                .unwrap_or(addr_str)
                .to_string();
            let node_id = entry
                .get("node_id")
                .and_then(Value::as_str)
                .map(str::to_string);
            proxies.push(ProxyEntry {
                addr,
                label,
                node_id,
            });
        }
    }
    Ok(GuiConfig {
        selected,
        proxies,
        auto_select,
        share_latency_stats,
    })
}

/// Serialize a config to TOML.
pub fn render(config: &GuiConfig) -> String {
    let mut table = Table::new();
    if let Some(selected) = &config.selected {
        table.insert("selected".into(), Value::String(selected.clone()));
    }
    table.insert("auto_select".into(), Value::Boolean(config.auto_select));
    table.insert(
        "share_latency_stats".into(),
        Value::Boolean(config.share_latency_stats),
    );
    let entries: Vec<Value> = config
        .proxies
        .iter()
        .map(|proxy| {
            let mut entry = Table::new();
            entry.insert("label".into(), Value::String(proxy.label.clone()));
            entry.insert("addr".into(), Value::String(proxy.addr.to_string()));
            if let Some(node_id) = &proxy.node_id {
                entry.insert("node_id".into(), Value::String(node_id.clone()));
            }
            Value::Table(entry)
        })
        .collect();
    table.insert("proxies".into(), Value::Array(entries));
    toml::to_string(&table).unwrap_or_default()
}

/// Write the config, creating the parent directory when needed.
pub fn save(path: &Path, config: &GuiConfig) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, render(config)).map_err(|e| e.to_string())?;
    tracing::info!("Saved config to {}", path.display());
    Ok(())
}

/// Remove the persisted config ("Reset to defaults"). A missing file is fine.
pub fn reset(path: &Path) -> Result<(), String> {
    match std::fs::remove_file(path) {
        Ok(()) => {
            tracing::info!("Removed config {}", path.display());
            Ok(())
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.to_string()),
    }
}

/// Replace registry-discovered relays with a fresh set, keeping user-added
/// custom proxies untouched.
pub fn merge_discovered(existing: &[ProxyEntry], discovered: Vec<ProxyEntry>) -> Vec<ProxyEntry> {
    let mut merged = discovered;
    merged.extend(existing.iter().filter(|p| !p.is_discovered()).cloned());
    merged
}

#[cfg(test)]
mod tests {
    use super::{load, merge_discovered, parse, render, reset, save, GuiConfig, ProxyEntry};
    use std::net::SocketAddrV4;

    fn addr(s: &str) -> SocketAddrV4 {
        s.parse().expect("test address")
    }

    fn sample() -> GuiConfig {
        GuiConfig {
            selected: Some("207.246.106.36:4434".into()),
            proxies: vec![
                ProxyEntry::discovered(
                    "relay-lax-1",
                    addr("207.246.106.36:4434"),
                    "LAX - Los Angeles",
                ),
                ProxyEntry::custom(addr("127.0.0.1:4434"), "Local proxy"),
            ],
            auto_select: true,
            share_latency_stats: true,
        }
    }

    #[test]
    fn config_round_trips_through_toml() {
        let parsed = parse(&render(&sample())).expect("rendered config parses");
        assert_eq!(parsed, sample());
    }

    #[test]
    fn config_parse_tolerates_missing_and_unknown_keys() {
        let parsed = parse("selected = \"1.2.3.4:4434\"\nunknown = 7\n").expect("parse");
        assert_eq!(parsed.selected.as_deref(), Some("1.2.3.4:4434"));
        assert!(parsed.proxies.is_empty());
    }

    #[test]
    fn config_parse_skips_invalid_proxy_addresses() {
        let parsed = parse(
            "[[proxies]]\nlabel = \"bad\"\naddr = \"not-an-addr\"\n\n\
             [[proxies]]\nlabel = \"good\"\naddr = \"5.6.7.8:4434\"\n",
        )
        .expect("parse");
        assert_eq!(parsed.proxies.len(), 1);
        assert_eq!(parsed.proxies[0].addr, addr("5.6.7.8:4434"));
        assert_eq!(parsed.proxies[0].node_id, None);
    }

    #[test]
    fn config_parse_rejects_malformed_toml() {
        assert!(parse("this is not = toml [").is_err());
    }

    #[test]
    fn config_load_missing_file_yields_defaults() {
        let path = std::env::temp_dir().join(format!(
            "lightspeed-config-missing-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        assert_eq!(load(&path), GuiConfig::default());
    }

    #[test]
    fn config_save_then_load_round_trips() {
        let path = std::env::temp_dir().join(format!(
            "lightspeed-config-roundtrip-{}.toml",
            std::process::id()
        ));
        let _ = std::fs::remove_file(&path);
        save(&path, &sample()).expect("save");
        assert_eq!(load(&path), sample());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn config_reset_removes_file_and_is_idempotent() {
        let path = std::env::temp_dir().join(format!(
            "lightspeed-config-reset-{}.toml",
            std::process::id()
        ));
        save(&path, &sample()).expect("save");
        assert!(path.exists());
        reset(&path).expect("reset");
        assert!(!path.exists());
        reset(&path).expect("reset again");
    }

    #[test]
    fn config_auto_select_defaults_to_true_and_parses_false() {
        assert!(
            parse("selected = \"1.2.3.4:4434\"\n")
                .expect("parse")
                .auto_select
        );
        assert!(!parse("auto_select = false\n").expect("parse").auto_select);
        assert!(parse("auto_select = true\n").expect("parse").auto_select);
    }

    #[test]
    fn config_share_latency_stats_defaults_on_and_parses_false() {
        assert!(
            parse("selected = \"1.2.3.4:4434\"\n")
                .expect("parse")
                .share_latency_stats
        );
        assert!(
            !parse("share_latency_stats = false\n")
                .expect("parse")
                .share_latency_stats
        );
        assert!(
            parse("share_latency_stats = true\n")
                .expect("parse")
                .share_latency_stats
        );
    }

    #[test]
    fn merge_discovered_refreshes_relays_and_keeps_custom_entries() {
        let existing = vec![
            ProxyEntry::discovered("relay-old", addr("9.9.9.9:4434"), "Old relay"),
            ProxyEntry::custom(addr("127.0.0.1:4434"), "Local proxy"),
        ];
        let discovered = vec![ProxyEntry::discovered(
            "relay-lax-1",
            addr("207.246.106.36:4434"),
            "LAX - Los Angeles",
        )];
        let merged = merge_discovered(&existing, discovered);
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0].node_id.as_deref(), Some("relay-lax-1"));
        assert_eq!(merged[1].label, "Local proxy");
        assert!(!merged[1].is_discovered());
    }
}
