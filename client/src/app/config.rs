//! Persistent application configuration (`config.toml`) — the shared model.
//!
//! MOVED from `client-gui/src/config.rs` at WF-081 W2-1 stage S1
//! (`docs/design/gui-core-boundary.md` §5.1/§6). The model, the parser, the
//! serializer, the tolerant loader and the reset/merge helpers are engine/app
//! logic; the GUI keeps only the file path (`client-gui/src/paths.rs`) and a
//! re-export shell. The CLI will adopt the same model at S6.
//!
//! Loading is tolerant by contract: a missing or malformed file yields defaults
//! instead of an error, and the file is only ever written by an explicit user
//! action or a successful discovery refresh — never on startup with placeholder
//! data. The tolerant loader's diagnostic is [`ConfigRecovery`], which is the
//! fact behind the GUI's `error.config` sentence (§2.1/§2.8); a missing file is
//! a first run, not a recovery.
//!
//! Legacy keys: the file written by older builds carries
//! `retry_after_fallback`, retired with the automatic direct fallback on
//! 2026-11-20. The parser ignores unknown keys (deliberately, so the retired key
//! is tolerated on read) and the serializer never writes it again. A test pins
//! both directions.
//!
//! ## Canonical and legacy files (S6c / owner Q10)
//!
//! The canonical per-user file is `lightspeed.toml` in the config directory both
//! front ends resolve ([`ConfigPaths`]). The GUI's pre-unification file
//! `config.toml` is imported **read-only**: it is never written, renamed or
//! deleted, the canonical values win on a per-key basis, and it only fills keys
//! the canonical file does not set. [`load_unified`] is the one reader both front
//! ends use; [`save_canonical`] is the only writer. `retry_after_fallback` keeps
//! its tolerated-on-read / never-written contract across the merge (the key sits
//! in the merged table only as an ignored unknown).

use std::net::SocketAddrV4;
use std::path::{Path, PathBuf};

use toml::{Table, Value};

/// A proxy relay entry shown in the relay selector and the proxy manager.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProxyEntry {
    /// The relay's advertised data-plane address.
    pub addr: SocketAddrV4,
    /// The label the selector shows (registry-friendly label or the user's).
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

/// The persisted application configuration.
///
/// One model for every front end (GUI today, CLI at S6). The fields stay flat
/// at S1 so the serialized file is byte-identical to the GUI's shipped
/// `config.toml`; the `RoutePrefs`/`GamePrefs`/`UiPrefs` grouping in the
/// boundary's DTO sketch arrives with the code-only unification at S5, where a
/// reader/writer both change at once.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppConfig {
    /// Selected relay address (`"ip:port"`), if any.
    pub selected: Option<String>,
    /// Every relay the selector can choose from.
    pub proxies: Vec<ProxyEntry>,
    /// When true, the GUI connects to the lowest-latency relay it can reach and
    /// reconsiders on each discovery, so newly added relays are picked up.
    pub auto_select: bool,
    /// When true, share anonymous aggregate latency statistics with the relay.
    /// On by default; no IP address, identifier, or packet content is sent.
    pub share_latency_stats: bool,
    /// When true, the GUI follows the auto-detected game instead of a pinned
    /// pick — the Game field's "Auto (detect)" entry, on like the relay's.
    pub game_auto: bool,
    /// The pinned game registry key (used when `game_auto` is false).
    pub selected_game: Option<String>,
    /// The Game field's "Manual / Custom…" mode: relay an explicit server.
    pub manual_mode: bool,
    /// UI language: a BCP-47 tag (`"en"`, `"de"`, `"pt-BR"`), or `None` to
    /// follow the operating system locale on every start.
    pub language: Option<String>,
    /// The Manual / Custom mode's server field, persisted so a typed address is
    /// still there after a restart. Empty means nothing was entered.
    pub manual_server: String,
    /// The Manual / Custom mode's port-range field, persisted for the same
    /// reason. Empty means the profile's default range is used.
    pub manual_port_range: String,
    /// Effective path-MTU cap for the tunnel, in bytes (1280-1500). The default
    /// 1492 is the PPPoE-safe clamp; lower it when the local path is smaller
    /// (VPN, DPI shim such as Zapret, reduced-MTU adapter), which otherwise
    /// fails with `WSAEMSGSIZE` (Windows 10040). The data plane also lowers it
    /// automatically when the OS rejects a send.
    pub tunnel_mtu: usize,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            selected: None,
            proxies: Vec::new(),
            auto_select: true,
            share_latency_stats: true,
            game_auto: true,
            selected_game: None,
            manual_mode: false,
            language: None,
            manual_server: String::new(),
            manual_port_range: String::new(),
            tunnel_mtu: 1492,
        }
    }
}

/// Why the tolerant loader could not use the file it found.
///
/// The GUI turns this into the localized `error.config` sentence and keeps the
/// verbatim reason behind `Details` (§2.8): a recovery is a user-visible fact,
/// not a silent default. `path` is the file that was attempted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigRecovery {
    /// The config file that could not be read or parsed.
    pub path: PathBuf,
    /// The verbatim diagnostic (parse error or I/O error).
    pub reason: String,
}

/// A configuration plus the diagnostic of a tolerant load.
///
/// `recovery` is `None` when the file loaded cleanly or does not exist yet: a
/// first run is not a recovery, and must not raise a settings-error sentence.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadedAppConfig {
    /// The configuration to run with (defaults when recovery is `Some`).
    pub config: AppConfig,
    /// The diagnostic when defaults were substituted for an unreadable file.
    pub recovery: Option<ConfigRecovery>,
}

/// Load the config at `path`, falling back to defaults on any error.
pub fn load(path: &Path) -> AppConfig {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse(&text) {
            Ok(config) => config,
            Err(e) => {
                tracing::warn!("Ignoring malformed config {}: {e}", path.display());
                AppConfig::default()
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => AppConfig::default(),
        Err(e) => {
            tracing::warn!("Could not read config {}: {e}", path.display());
            AppConfig::default()
        }
    }
}

/// Load the config at `path`, keeping the recovery diagnostic the GUI surfaces.
///
/// Same tolerated behaviour as [`load`], but the caller learns *why* defaults
/// were used so the window can say so (`error.config`) instead of looking like
/// a fresh install. A missing file is a first run: `recovery` stays `None`.
pub fn load_or_default(path: &Path) -> LoadedAppConfig {
    match std::fs::read_to_string(path) {
        Ok(text) => match parse(&text) {
            Ok(config) => LoadedAppConfig {
                config,
                recovery: None,
            },
            Err(reason) => {
                tracing::warn!("Ignoring malformed config {}: {reason}", path.display());
                LoadedAppConfig {
                    config: AppConfig::default(),
                    recovery: Some(ConfigRecovery {
                        path: path.to_path_buf(),
                        reason,
                    }),
                }
            }
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => LoadedAppConfig {
            config: AppConfig::default(),
            recovery: None,
        },
        Err(e) => {
            tracing::warn!("Could not read config {}: {e}", path.display());
            LoadedAppConfig {
                config: AppConfig::default(),
                recovery: Some(ConfigRecovery {
                    path: path.to_path_buf(),
                    reason: e.to_string(),
                }),
            }
        }
    }
}

// ── Canonical + legacy config files (WF-081 W2-3b S6c / boundary S5) ───────
//
// One canonical per-user file is read/written through this model; the GUI's
// pre-unification file is imported **read-only** (Q10), never deleted and never
// mutated. The merge is per key: canonical wins, legacy fills the unset keys.

/// The canonical per-user config file name.
///
/// It is the CLI's native file name in the per-OS config directory both front
/// ends resolve (`crate::paths::config_dir`), so "one config file" is literally
/// one name in one directory.
pub const CANONICAL_FILE_NAME: &str = "lightspeed.toml";

/// The GUI's pre-unification file name (legacy, read-only import).
pub const LEGACY_FILE_NAME: &str = "config.toml";

/// The canonical file's metadata table (`[meta]`).
pub const META_TABLE: &str = "meta";

/// The `[meta]` key recording an explicit "Reset to defaults" decision (S6d).
///
/// `legacy_import = false` in the canonical file means the user explicitly chose
/// defaults: the read-only legacy GUI file must not be imported again. It is the
/// only metadata key this module reads.
pub const LEGACY_IMPORT_META_KEY: &str = "legacy_import";

/// Whether a parsed canonical table records the explicit-defaults decision.
///
/// Only an explicit boolean `false` counts. A missing table/key, a malformed
/// value or `legacy_import = true` all keep the ordinary import precedence
/// unchanged (legacy fills unset keys).
fn legacy_import_disabled(table: &Table) -> bool {
    table
        .get(META_TABLE)
        .and_then(Value::as_table)
        .and_then(|meta| meta.get(LEGACY_IMPORT_META_KEY))
        .and_then(Value::as_bool)
        == Some(false)
}

/// The pair of config files a front end reads: the canonical file it writes and
/// the legacy GUI file it only imports.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigPaths {
    /// Canonical file: written by the GUI, read first, wins on conflicts.
    pub canonical: PathBuf,
    /// Legacy GUI file: imported read-only; never written, never deleted.
    pub legacy: PathBuf,
}

impl ConfigPaths {
    /// The pair inside one per-user config directory.
    pub fn in_dir(dir: &Path) -> Self {
        Self {
            canonical: dir.join(CANONICAL_FILE_NAME),
            legacy: dir.join(LEGACY_FILE_NAME),
        }
    }
}

/// One config file as read by the tolerant loader, before any merge.
enum FileTable {
    Missing,
    Parsed(Table),
    Unusable(String),
}

fn read_table(path: &Path) -> FileTable {
    match std::fs::read_to_string(path) {
        Ok(text) => match toml::from_str::<Table>(&text) {
            Ok(table) => FileTable::Parsed(table),
            Err(error) => FileTable::Unusable(error.to_string()),
        },
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => FileTable::Missing,
        Err(error) => FileTable::Unusable(error.to_string()),
    }
}

/// Load the canonical config, importing the legacy GUI file read-only (Q10).
///
/// Precedence is per key: the legacy file is the base table, the canonical table
/// overrides it, and defaults apply only to keys neither file carries — exactly
/// "canonical wins, legacy fills unset". Recovery rules:
///
/// * neither file exists → defaults, no recovery (a first run is not an error);
/// * canonical exists but is unusable → defaults + [`ConfigRecovery`] naming the
///   canonical file; the legacy file is **not** mixed in, so a broken canonical
///   file can never be silently half-applied;
/// * legacy unusable → logged and ignored (it is a retired, read-only migration
///   source and must never block startup);
/// * legacy alone → its values load as the config (the one-time migration read).
///
/// **Explicit defaults (S6d).** When the canonical file carries the
/// `[meta] legacy_import = false` marker (written by
/// [`reset_to_defaults`] and preserved by [`save_canonical`]), the legacy import
/// is skipped **entirely**: the canonical file alone is the config. That is what
/// makes Reset a real reset without ever touching the legacy file.
///
/// This function only ever **reads** the legacy file; no function in this module
/// writes, renames or deletes it.
pub fn load_unified(paths: &ConfigPaths) -> LoadedAppConfig {
    let canonical_table = match read_table(&paths.canonical) {
        FileTable::Missing => None,
        FileTable::Parsed(table) => Some(table),
        FileTable::Unusable(reason) => {
            tracing::warn!(
                "Ignoring unusable canonical config {}: {reason}",
                paths.canonical.display()
            );
            return LoadedAppConfig {
                config: AppConfig::default(),
                recovery: Some(ConfigRecovery {
                    path: paths.canonical.clone(),
                    reason,
                }),
            };
        }
    };
    // A canonical file that records the explicit-defaults decision suppresses the
    // legacy import entirely; the legacy file is then not even read.
    let import_legacy = canonical_table
        .as_ref()
        .map(|table| !legacy_import_disabled(table))
        .unwrap_or(true);
    let legacy_table = if import_legacy {
        match read_table(&paths.legacy) {
            FileTable::Missing => None,
            FileTable::Parsed(table) => Some(table),
            FileTable::Unusable(reason) => {
                tracing::warn!(
                    "Ignoring unusable legacy config {}: {reason}",
                    paths.legacy.display()
                );
                None
            }
        }
    } else {
        None
    };
    let merged = match (canonical_table, legacy_table) {
        (None, None) => {
            return LoadedAppConfig {
                config: AppConfig::default(),
                recovery: None,
            }
        }
        (Some(canonical), None) => canonical,
        (None, Some(legacy)) => legacy,
        (Some(canonical), Some(legacy)) => {
            let mut merged = legacy;
            for (key, value) in canonical {
                merged.insert(key, value);
            }
            merged
        }
    };
    LoadedAppConfig {
        config: parse_table(merged),
        recovery: None,
    }
}

/// Load the shared config from a per-user config directory.
///
/// Convenience for front ends that resolve the directory through
/// [`crate::paths::config_dir`] (the CLI) rather than keeping a path pair.
pub fn load_unified_in_dir(dir: &Path) -> LoadedAppConfig {
    load_unified(&ConfigPaths::in_dir(dir))
}

/// Write the canonical file only; the legacy GUI file is never touched.
///
/// The canonical write is the migration step: after it, the values it carries
/// override the legacy read for every key it emits. The legacy file stays on
/// disk exactly as the user left it (Q10: preserved).
///
/// An existing explicit-defaults marker (`[meta] legacy_import = false`) is
/// **preserved** across an ordinary settings save: once the user has reset, a
/// later settings change must not silently re-enable the legacy import (S6d).
pub fn save_canonical(paths: &ConfigPaths, config: &AppConfig) -> Result<(), String> {
    let explicit_defaults = matches!(
        read_table(&paths.canonical),
        FileTable::Parsed(table) if legacy_import_disabled(&table)
    );
    write_config(&paths.canonical, config, explicit_defaults)
}

/// "Reset to defaults" — a real reset (S6d).
///
/// Writes the canonical file as typed defaults **plus** the
/// `[meta] legacy_import = false` marker, so the read-only legacy GUI file can
/// never re-populate the settings on the next load. The legacy file itself is
/// never written, renamed or deleted (Q10). A missing/unreadable canonical file
/// is irrelevant: the reset writes a fresh one.
pub fn reset_to_defaults(paths: &ConfigPaths) -> Result<(), String> {
    write_config(&paths.canonical, &AppConfig::default(), true)
}

/// Write `config` to `path`, creating the parent directory, optionally appending
/// the explicit-defaults marker.
fn write_config(path: &Path, config: &AppConfig, explicit_defaults: bool) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut body = render(config);
    if explicit_defaults {
        if !body.ends_with('\n') {
            body.push('\n');
        }
        body.push('[');
        body.push_str(META_TABLE);
        body.push_str("]\n");
        body.push_str(LEGACY_IMPORT_META_KEY);
        body.push_str(" = false\n");
    }
    std::fs::write(path, body).map_err(|e| e.to_string())?;
    tracing::info!("Saved config to {}", path.display());
    Ok(())
}

/// Parse a `config.toml` body. Unknown keys are ignored; entries with an
/// unparseable `addr` are skipped rather than failing the whole file.
pub fn parse(text: &str) -> Result<AppConfig, String> {
    let table: Table = toml::from_str(text).map_err(|e| e.to_string())?;
    Ok(parse_table(table))
}

/// Interpret an already-merged TOML table as an [`AppConfig`].
///
/// Separate from [`parse`] so the canonical/legacy merge ([`load_unified`]) can
/// combine two raw tables *before* defaults are applied — that is what makes
/// "canonical wins, legacy fills unset" per key rather than per file.
fn parse_table(table: Table) -> AppConfig {
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
    let manual_mode = table
        .get("manual_mode")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    // Absent means "follow the OS locale", so an empty or malformed value is
    // treated as absent rather than as a broken tag.
    let language = table
        .get("language")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_string);
    // A blank field is the same as an absent one: the app falls back to the
    // profile default rather than to an unusable empty string.
    let manual_server = table
        .get("manual_server")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();
    let manual_port_range = table
        .get("manual_port_range")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string();

    // A hand-edited path-MTU cap is clamped into the range the tunnel supports;
    // an absent key keeps the PPPoE-safe 1492 default.
    let tunnel_mtu = table
        .get("tunnel_mtu")
        .and_then(Value::as_integer)
        .map(|value| value.clamp(1280, 1500) as usize)
        .unwrap_or(1492);

    let game_auto = table
        .get("game_auto")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let selected_game = table
        .get("selected_game")
        .and_then(Value::as_str)
        .map(str::to_string);

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
    AppConfig {
        selected,
        proxies,
        auto_select,
        share_latency_stats,
        game_auto,
        selected_game,
        manual_mode,
        language,
        manual_server,
        manual_port_range,
        tunnel_mtu,
    }
}

/// Serialize a config to TOML.
pub fn render(config: &AppConfig) -> String {
    let mut table = Table::new();
    if let Some(selected) = &config.selected {
        table.insert("selected".into(), Value::String(selected.clone()));
    }
    table.insert("auto_select".into(), Value::Boolean(config.auto_select));
    table.insert("game_auto".into(), Value::Boolean(config.game_auto));
    table.insert("manual_mode".into(), Value::Boolean(config.manual_mode));
    if let Some(game) = &config.selected_game {
        table.insert("selected_game".into(), Value::String(game.clone()));
    }
    table.insert(
        "share_latency_stats".into(),
        Value::Boolean(config.share_latency_stats),
    );
    if let Some(language) = &config.language {
        table.insert("language".into(), Value::String(language.clone()));
    }
    if !config.manual_server.is_empty() {
        table.insert(
            "manual_server".into(),
            Value::String(config.manual_server.clone()),
        );
    }
    if !config.manual_port_range.is_empty() {
        table.insert(
            "manual_port_range".into(),
            Value::String(config.manual_port_range.clone()),
        );
    }
    table.insert(
        "tunnel_mtu".into(),
        Value::Integer(config.tunnel_mtu as i64),
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
pub fn save(path: &Path, config: &AppConfig) -> Result<(), String> {
    write_config(path, config, false)
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
    use super::{
        load, load_or_default, load_unified, load_unified_in_dir, merge_discovered, parse, render,
        reset, reset_to_defaults, save, save_canonical, AppConfig, ConfigPaths, ProxyEntry,
        CANONICAL_FILE_NAME, LEGACY_FILE_NAME, LEGACY_IMPORT_META_KEY, META_TABLE,
    };
    use std::net::SocketAddrV4;
    use std::path::PathBuf;

    /// A unique scratch directory for one test, removed first if a previous run
    /// left it behind. The gate points `TEMP`/`TMP` inside the workspace, so the
    /// low-integrity test binary can write here (see `lightspeed-windows`).
    fn scratch(tag: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("lightspeed-config-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("scratch dir");
        dir
    }

    fn read(path: &std::path::Path) -> String {
        std::fs::read_to_string(path).expect("read fixture")
    }

    fn addr(s: &str) -> SocketAddrV4 {
        s.parse().expect("test address")
    }

    fn sample() -> AppConfig {
        AppConfig {
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
            game_auto: true,
            selected_game: None,
            manual_mode: false,
            language: Some("de".into()),
            manual_server: "203.0.113.7:28015".into(),
            manual_port_range: "27000-27050".into(),
            tunnel_mtu: 1400,
        }
    }

    fn temp_path(tag: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!(
            "lightspeed-appconfig-{tag}-{}.toml",
            std::process::id()
        ))
    }

    #[test]
    fn tunnel_mtu_is_clamped_from_the_config_file() {
        let lowered = parse("tunnel_mtu = 900\n").expect("parse");
        assert_eq!(
            lowered.tunnel_mtu, 1280,
            "a cap below the floor is raised to it"
        );

        let raised = parse("tunnel_mtu = 9000\n").expect("parse");
        assert_eq!(
            raised.tunnel_mtu, 1500,
            "a cap above the Ethernet hard cap is refused"
        );

        let absent = parse("").expect("parse");
        assert_eq!(absent.tunnel_mtu, 1492, "the default is the PPPoE clamp");
    }

    #[test]
    fn config_round_trips_through_toml() {
        let parsed = parse(&render(&sample())).expect("rendered config parses");
        assert_eq!(parsed, sample());
    }

    #[test]
    fn config_round_trips_without_a_pinned_language() {
        let mut config = sample();
        config.language = None;
        let parsed = parse(&render(&config)).expect("rendered config parses");
        assert_eq!(parsed.language, None);
    }

    /// Discussion #146: the Manual / Custom fields must still be there after a
    /// restart. `sample()` carries both, so the round-trip test above covers the
    /// written case; this covers the untouched case, where an empty field must
    /// not be written out and must parse back as empty rather than as missing.
    #[test]
    fn an_untouched_manual_entry_is_neither_written_nor_lost() {
        let mut config = sample();
        config.manual_server = String::new();
        config.manual_port_range = String::new();
        let text = render(&config);
        assert!(!text.contains("manual_server"), "{text}");
        assert!(!text.contains("manual_port_range"), "{text}");
        let parsed = parse(&text).expect("config without the fields parses");
        assert_eq!(parsed.manual_server, "");
        assert_eq!(parsed.manual_port_range, "");
    }

    #[test]
    fn config_parse_tolerates_missing_and_unknown_keys() {
        let parsed = parse("selected = \"1.2.3.4:4434\"\nunknown = 7\n").expect("parse");
        assert_eq!(parsed.selected.as_deref(), Some("1.2.3.4:4434"));
        assert!(parsed.proxies.is_empty());
    }

    /// The automatic direct fallback and its re-test setting were removed on
    /// 2026-11-20. A config file written by an older build still carries
    /// `retry_after_fallback`; the key must be ignored rather than failing the
    /// whole load, and a later save must not write it back.
    #[test]
    fn config_from_before_the_warn_only_mode_still_loads() {
        let parsed = parse("retry_after_fallback = false\nshare_latency_stats = false\n")
            .expect("a config with the retired key parses");
        assert!(
            !parsed.share_latency_stats,
            "the keys around the retired one still load"
        );
        let rendered = render(&parsed);
        assert!(
            !rendered.contains("retry_after_fallback"),
            "the retired key must not be written back: {rendered}"
        );
    }

    /// The tolerant loader keeps the diagnostic the GUI needs for `error.config`;
    /// a first run (no file) is not a recovery and must not raise a sentence.
    #[test]
    fn load_or_default_reports_a_recovery_only_for_an_unusable_file() {
        let missing = temp_path("missing");
        let _ = std::fs::remove_file(&missing);
        let loaded = load_or_default(&missing);
        assert_eq!(loaded.config, AppConfig::default());
        assert_eq!(loaded.recovery, None, "a first run is not a settings error");

        let broken = temp_path("broken");
        std::fs::write(&broken, "this is not = toml [").expect("write");
        let loaded = load_or_default(&broken);
        assert_eq!(loaded.config, AppConfig::default());
        let recovery = loaded.recovery.expect("a malformed file is a recovery");
        assert_eq!(recovery.path, broken);
        assert!(
            !recovery.reason.is_empty(),
            "the diagnostic must say why defaults were used"
        );
        let _ = std::fs::remove_file(&broken);
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
    fn config_language_absent_means_follow_the_system_locale() {
        let parsed = parse("auto_select = true\n").expect("parse");
        assert_eq!(parsed.language, None);
    }

    #[test]
    fn config_language_blank_is_treated_as_absent() {
        for body in ["language = \"\"\n", "language = \"   \"\n"] {
            let parsed = parse(body).expect("parse");
            assert_eq!(parsed.language, None, "{body:?} should not pin a language");
        }
    }

    #[test]
    fn config_load_missing_file_yields_defaults() {
        let path = temp_path("load-missing");
        let _ = std::fs::remove_file(&path);
        assert_eq!(load(&path), AppConfig::default());
    }

    #[test]
    fn config_save_then_load_round_trips() {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);
        save(&path, &sample()).expect("save");
        assert_eq!(load(&path), sample());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn config_reset_removes_file_and_is_idempotent() {
        let path = temp_path("reset");
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

    // ── Canonical + legacy fixture matrix (S6c / Q10) ──────────────────────

    /// The pair carries the canonical and legacy names in the given directory.
    #[test]
    fn config_paths_use_the_canonical_and_legacy_names() {
        let dir = scratch("names");
        let paths = ConfigPaths::in_dir(&dir);
        assert_eq!(
            paths.canonical.file_name().and_then(|n| n.to_str()),
            Some(CANONICAL_FILE_NAME)
        );
        assert_eq!(
            paths.legacy.file_name().and_then(|n| n.to_str()),
            Some(LEGACY_FILE_NAME)
        );
    }

    /// `old-only`: the legacy GUI file loads as the config, is byte-identical
    /// after the read, and no canonical file is created by a read.
    #[test]
    fn legacy_only_config_is_imported_read_only() {
        let dir = scratch("legacy-only");
        let paths = ConfigPaths::in_dir(&dir);
        let legacy_body = "selected = \"207.246.106.36:4434\"\nshare_latency_stats = false\n\
                           tunnel_mtu = 1400\nretry_after_fallback = true\n";
        std::fs::write(&paths.legacy, legacy_body).expect("write legacy");

        let loaded = load_unified(&paths);
        assert!(
            loaded.recovery.is_none(),
            "a legacy import is not a recovery"
        );
        assert_eq!(
            loaded.config.selected.as_deref(),
            Some("207.246.106.36:4434")
        );
        assert!(!loaded.config.share_latency_stats);
        assert_eq!(loaded.config.tunnel_mtu, 1400);
        assert_eq!(
            read(&paths.legacy),
            legacy_body,
            "the legacy file must be read-only (byte-identical after the import)"
        );
        assert!(
            !paths.canonical.exists(),
            "reading must not create the canonical file"
        );
    }

    /// `new-only`: the canonical file loads; the legacy path is not required.
    #[test]
    fn canonical_only_config_loads() {
        let dir = scratch("canonical-only");
        let paths = ConfigPaths::in_dir(&dir);
        std::fs::write(
            &paths.canonical,
            "auto_select = false\nshare_latency_stats = true\ntunnel_mtu = 1450\n",
        )
        .expect("write canonical");

        let loaded = load_unified(&paths);
        assert!(loaded.recovery.is_none());
        assert!(!loaded.config.auto_select);
        assert!(loaded.config.share_latency_stats);
        assert_eq!(loaded.config.tunnel_mtu, 1450);
        assert!(!paths.legacy.exists());
    }

    /// `both`: per-key precedence — canonical wins where it sets a key, legacy
    /// fills every key the canonical file does not carry (Q10).
    #[test]
    fn both_files_canonical_wins_per_key_and_legacy_fills_unset() {
        let dir = scratch("both");
        let paths = ConfigPaths::in_dir(&dir);
        std::fs::write(
            &paths.legacy,
            "selected = \"1.1.1.1:4434\"\nshare_latency_stats = false\n\
             tunnel_mtu = 1400\nlanguage = \"de\"\ngame_auto = false\n",
        )
        .expect("write legacy");
        std::fs::write(
            &paths.canonical,
            "selected = \"2.2.2.2:4434\"\nshare_latency_stats = true\ngame_auto = true\n",
        )
        .expect("write canonical");

        let loaded = load_unified(&paths);
        assert!(loaded.recovery.is_none());
        assert_eq!(
            loaded.config.selected.as_deref(),
            Some("2.2.2.2:4434"),
            "canonical wins on a key it sets"
        );
        assert!(loaded.config.share_latency_stats, "canonical wins");
        assert!(loaded.config.game_auto, "canonical wins");
        assert_eq!(
            loaded.config.tunnel_mtu, 1400,
            "legacy fills a key the canonical file does not set"
        );
        assert_eq!(
            loaded.config.language.as_deref(),
            Some("de"),
            "legacy fills a key the canonical file does not set"
        );
    }

    /// A broken canonical file is a recovery and must not be mixed with legacy
    /// values (no silently half-applied config).
    #[test]
    fn malformed_canonical_is_a_recovery_and_never_mixes_legacy() {
        let dir = scratch("malformed-canonical");
        let paths = ConfigPaths::in_dir(&dir);
        std::fs::write(&paths.legacy, "selected = \"1.1.1.1:4434\"\n").expect("write legacy");
        std::fs::write(&paths.canonical, "this is not = = valid {{ toml\n")
            .expect("write malformed canonical");

        let loaded = load_unified(&paths);
        let recovery = loaded.recovery.expect("malformed canonical is a recovery");
        assert_eq!(recovery.path, paths.canonical);
        assert!(!recovery.reason.is_empty());
        assert_eq!(loaded.config, AppConfig::default());
        assert_eq!(
            loaded.config.selected, None,
            "the legacy value must not leak into a broken-canonical load"
        );
    }

    /// A broken legacy file is ignored (it is a retired read-only source and must
    /// not block startup); the canonical file still loads cleanly.
    #[test]
    fn malformed_legacy_is_ignored_and_canonical_loads() {
        let dir = scratch("malformed-legacy");
        let paths = ConfigPaths::in_dir(&dir);
        std::fs::write(&paths.legacy, "{ not toml").expect("write malformed legacy");
        std::fs::write(&paths.canonical, "share_latency_stats = false\n").expect("write canonical");

        let loaded = load_unified(&paths);
        assert!(
            loaded.recovery.is_none(),
            "a retired legacy file must never raise the GUI settings error"
        );
        assert!(!loaded.config.share_latency_stats);
    }

    /// Neither file (first run) is defaults and **not** a recovery.
    #[test]
    fn no_files_is_a_first_run_not_a_recovery() {
        let dir = scratch("none");
        let loaded = load_unified_in_dir(&dir);
        assert!(loaded.recovery.is_none());
        assert_eq!(loaded.config, AppConfig::default());
    }

    /// The writer touches the canonical file only: the legacy file keeps its
    /// bytes, the retired key is never re-written, and the canonical file
    /// round-trips the merged values.
    #[test]
    fn save_canonical_never_touches_or_rewrites_the_legacy_file() {
        let dir = scratch("save");
        let paths = ConfigPaths::in_dir(&dir);
        let legacy_body = "selected = \"1.1.1.1:4434\"\nretry_after_fallback = true\n\
                           language = \"de\"\n";
        std::fs::write(&paths.legacy, legacy_body).expect("write legacy");

        let loaded = load_unified(&paths);
        save_canonical(&paths, &loaded.config).expect("save canonical");

        assert_eq!(read(&paths.legacy), legacy_body);
        assert!(paths.canonical.exists(), "the migration writes canonical");
        let written = read(&paths.canonical);
        assert!(
            !written.contains("retry_after_fallback"),
            "the retired key must never be written: {written}"
        );
        assert!(written.contains("language = \"de\""), "{written}");
        let reparsed = parse(&written).expect("canonical round-trips");
        assert_eq!(reparsed.selected, loaded.config.selected);
        assert_eq!(reparsed.language, loaded.config.language);

        // Second load prefers the canonical file and keeps the same values.
        let reloaded = load_unified(&paths);
        assert!(reloaded.recovery.is_none());
        assert_eq!(reloaded.config.selected, loaded.config.selected);
    }

    // ── Explicit defaults / Reset (S6d) ────────────────────────────────────

    /// Reset writes the marker; the next load returns typed defaults even with a
    /// legacy file full of values present, and the legacy bytes are untouched.
    #[test]
    fn reset_to_defaults_is_a_real_reset_with_a_legacy_file_present() {
        let dir = scratch("reset");
        let paths = ConfigPaths::in_dir(&dir);
        let legacy_body = "selected = \"1.1.1.1:4434\"\nshare_latency_stats = false\n\
                           language = \"de\"\n";
        std::fs::write(&paths.legacy, legacy_body).expect("write legacy");

        // Before the reset the legacy file is imported (ordinary precedence).
        assert_eq!(
            load_unified(&paths).config.selected.as_deref(),
            Some("1.1.1.1:4434")
        );

        reset_to_defaults(&paths).expect("reset");
        let canonical = read(&paths.canonical);
        assert!(
            canonical.contains(&format!("[{META_TABLE}]")),
            "reset writes the meta table: {canonical}"
        );
        assert!(
            canonical.contains(&format!("{LEGACY_IMPORT_META_KEY} = false")),
            "reset writes the explicit-defaults marker: {canonical}"
        );

        let loaded = load_unified(&paths);
        assert_eq!(
            loaded.config,
            AppConfig::default(),
            "reset returns typed defaults, not the legacy values"
        );
        assert!(loaded.recovery.is_none());
        assert_eq!(
            read(&paths.legacy),
            legacy_body,
            "the legacy file is never touched by a reset"
        );
    }

    /// The reset decision survives a later ordinary settings save: the marker is
    /// preserved and the legacy file still does not re-import.
    #[test]
    fn reset_marker_survives_a_later_settings_save() {
        let dir = scratch("reset-roundtrip");
        let paths = ConfigPaths::in_dir(&dir);
        std::fs::write(
            &paths.legacy,
            "selected = \"1.1.1.1:4434\"\nshare_latency_stats = false\n",
        )
        .expect("write legacy");
        reset_to_defaults(&paths).expect("reset");

        let mut changed = load_unified(&paths).config;
        assert_eq!(changed, AppConfig::default());
        changed.language = Some("ru".to_string());
        changed.tunnel_mtu = 1400;
        save_canonical(&paths, &changed).expect("save after reset");

        let written = read(&paths.canonical);
        assert!(
            written.contains(&format!("{LEGACY_IMPORT_META_KEY} = false")),
            "the marker must survive an ordinary settings save: {written}"
        );
        let reloaded = load_unified(&paths);
        assert_eq!(reloaded.config.language.as_deref(), Some("ru"));
        assert_eq!(reloaded.config.tunnel_mtu, 1400);
        assert_eq!(
            reloaded.config.selected, None,
            "the legacy selection must not re-import after a reset"
        );
        assert!(
            reloaded.config.share_latency_stats,
            "the legacy consent bit must not re-import after a reset"
        );
    }

    /// Ordinary precedence is unchanged: only an explicit boolean `false`
    /// suppresses the import; a missing/true/malformed marker keeps it.
    #[test]
    fn only_an_explicit_false_marker_suppresses_the_import() {
        for canonical in [
            format!("[{META_TABLE}]\n{LEGACY_IMPORT_META_KEY} = true\n"),
            format!("[{META_TABLE}]\n{LEGACY_IMPORT_META_KEY} = \"no\"\n"),
            "[meta]\nother = 1\n".to_string(),
            "auto_select = false\n".to_string(),
        ] {
            let dir = scratch("marker-not-false");
            let paths = ConfigPaths::in_dir(&dir);
            std::fs::write(&paths.legacy, "selected = \"1.1.1.1:4434\"\n").expect("write legacy");
            std::fs::write(&paths.canonical, &canonical).expect("write canonical");
            assert_eq!(
                load_unified(&paths).config.selected.as_deref(),
                Some("1.1.1.1:4434"),
                "canonical {canonical:?} must not suppress the legacy import"
            );
        }
    }
}
