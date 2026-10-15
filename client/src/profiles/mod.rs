//! The user-profile store: one TOML file, parsed once per process into a
//! cache, with an explicit [`ProfileStore::reload`] for the editor.
//!
//! The schema and every validation rule live in [`schema`]; this module owns
//! the file, the cache, and the `--list-games` rendering. Nothing here touches
//! the built-in registry: a user profile is additive, and built-ins still
//! outrank it (see [`schema::document_diagnostics`]).
//!
//! Load is tolerant and reporting; save is strict. Both go through the same
//! checks, so a document the loader skipped or warned about is refused by the
//! saver with the same message.

use std::path::{Path, PathBuf};
use std::sync::{Arc, OnceLock, RwLock};

pub mod schema;

pub use schema::{
    load_from_str, save_to_file, Diagnostic, LoadedProfiles, SaveError, UserGameProfile, UserServer,
};

// Test-only re-exports. `PortRange` is the type this module's own tests build
// their fixtures with, and `Severity` is what the `games` test module compares
// a diagnostic against. They are gated because `main.rs` compiles this module
// as a private `mod profiles`: a re-export the binary never uses is an
// `unused_imports` error there under `-D warnings`. Every other schema item
// stays public at `profiles::schema`.
#[cfg(test)]
pub use schema::{PortRange, Severity};

/// Where the store reads from and whether it reads at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoreConfig {
    /// Explicit file (`--profiles <path>`), else the per-OS default location.
    pub path: Option<PathBuf>,
    /// `false` for `--no-user-profiles`: the file is never read.
    pub enabled: bool,
}

impl Default for StoreConfig {
    /// Enabled, default location.
    fn default() -> Self {
        Self {
            path: None,
            enabled: true,
        }
    }
}

impl StoreConfig {
    pub fn enabled() -> Self {
        Self::default()
    }

    pub fn disabled() -> Self {
        Self {
            path: None,
            enabled: false,
        }
    }

    pub fn with_path(path: impl Into<PathBuf>) -> Self {
        Self {
            path: Some(path.into()),
            enabled: true,
        }
    }

    /// The CLI surface in one place: `--no-user-profiles` wins over `--profiles`.
    pub fn from_cli_flags(profiles: Option<&str>, no_user_profiles: bool) -> Self {
        if no_user_profiles {
            return Self::disabled();
        }
        match profiles {
            Some(path) => Self::with_path(path),
            None => Self::enabled(),
        }
    }
}

/// The cached profiles document.
///
/// The cache holds an `Arc` so a reader can hand the snapshot out and keep
/// working while a [`ProfileStore::reload`] swaps in the next one.
#[derive(Debug)]
pub struct ProfileStore {
    path: Option<PathBuf>,
    enabled: bool,
    loaded: RwLock<Arc<LoadedProfiles>>,
}

impl ProfileStore {
    /// A store that never reads a file: `--no-user-profiles`.
    pub fn disabled() -> Self {
        Self {
            path: None,
            enabled: false,
            loaded: RwLock::new(Arc::new(LoadedProfiles::default())),
        }
    }

    /// Load from one explicit file: `--profiles <path>`, and every test.
    pub fn load_from(path: impl Into<PathBuf>) -> Self {
        Self::at(Some(path.into()))
    }

    /// Load from the per-OS default location ([`crate::paths::profiles_file`]).
    pub fn load_default() -> Self {
        Self::at(None)
    }

    fn at(path: Option<PathBuf>) -> Self {
        let store = Self {
            path,
            enabled: true,
            loaded: RwLock::new(Arc::new(LoadedProfiles::default())),
        };
        store.reload();
        store
    }

    /// The file this store reads: the explicit path, else the per-OS default.
    /// `None` for a disabled store or a host with no config dir.
    pub fn path(&self) -> Option<PathBuf> {
        if !self.enabled {
            return None;
        }
        Some(
            self.path
                .clone()
                .unwrap_or_else(|| crate::paths::profiles_file().unwrap_or_default()),
        )
        .filter(|p| !p.as_os_str().is_empty())
    }

    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// The current snapshot. Cheap: one `Arc` clone.
    pub fn snapshot(&self) -> Arc<LoadedProfiles> {
        match self.loaded.read() {
            Ok(guard) => Arc::clone(&guard),
            // A panicking writer cannot leave a torn document: the lock only
            // ever holds a fully-built `Arc`. Recover rather than propagate.
            Err(poisoned) => Arc::clone(&poisoned.into_inner()),
        }
    }

    /// The loaded games.
    pub fn games(&self) -> Vec<UserGameProfile> {
        self.snapshot().games.clone()
    }

    /// The loaded servers.
    pub fn servers(&self) -> Vec<UserServer> {
        self.snapshot().servers.clone()
    }

    /// Re-read the file and report every diagnostic it produced.
    ///
    /// A missing file is normal (a fresh install has none): that is an empty
    /// document, not an error. Any other read failure is reported and leaves
    /// the store empty rather than stale.
    pub fn reload(&self) -> Arc<LoadedProfiles> {
        let next = if self.enabled {
            match self.path() {
                Some(path) => match std::fs::read_to_string(&path) {
                    Ok(text) => load_from_str(&text),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        tracing::debug!(
                            "profiles: no file at {} - built-in games only",
                            path.display()
                        );
                        LoadedProfiles::default()
                    }
                    Err(e) => {
                        let mut doc = LoadedProfiles::default();
                        doc.diagnostics.push(Diagnostic::skipped(
                            path.display().to_string(),
                            format!("cannot read profiles file: {e}"),
                        ));
                        doc
                    }
                },
                None => LoadedProfiles::default(),
            }
        } else {
            LoadedProfiles::default()
        };
        next.report();

        let next = Arc::new(next);
        match self.loaded.write() {
            Ok(mut guard) => *guard = Arc::clone(&next),
            Err(poisoned) => *poisoned.into_inner() = Arc::clone(&next),
        }
        next
    }

    /// Validate and write the document, then reload the cache.
    ///
    /// Nothing is written when any check fails; the errors are the same
    /// messages the loader reports (built-in collisions, bad ranges, caps).
    pub fn save(
        &self,
        games: &[UserGameProfile],
        servers: &[UserServer],
    ) -> Result<(), Vec<SaveError>> {
        let Some(path) = self.path() else {
            return Err(vec![SaveError::new(
                "file",
                "no profiles path is configured (--no-user-profiles)",
            )]);
        };
        save_to_file(&path, games, servers)?;
        self.reload();
        Ok(())
    }
}

/// The process-wide store.
static STORE: OnceLock<ProfileStore> = OnceLock::new();

/// Initialise the process-wide store. The first caller wins, so the CLI and any
/// library embedding it observe the same cache for the life of the process.
pub fn init(config: &StoreConfig) -> &'static ProfileStore {
    STORE.get_or_init(|| {
        if !config.enabled {
            ProfileStore::disabled()
        } else {
            match &config.path {
                Some(path) => ProfileStore::load_from(path.clone()),
                None => ProfileStore::load_default(),
            }
        }
    })
}

/// The process-wide store, loading the default location on first use.
pub fn global() -> &'static ProfileStore {
    STORE.get_or_init(ProfileStore::load_default)
}

/// Re-read the process-wide store's file.
pub fn reload_global() -> Arc<LoadedProfiles> {
    global().reload()
}

/// The `--list-games` body: the built-in section first, then `Your profiles`
/// when any exist. Pure, so the output shape is testable without a binary.
///
/// `source` is the profiles file the entries came from, shown under the user
/// section so `--profiles <path>` is discoverable.
pub fn render_list_games(
    builtins: &[String],
    profiles: &[UserGameProfile],
    source: Option<&Path>,
) -> Vec<String> {
    let mut lines = Vec::with_capacity(builtins.len() + profiles.len() + 4);
    lines.push("🎮 Built-in games:".to_string());
    lines.extend(builtins.iter().cloned());

    if !profiles.is_empty() {
        lines.push(String::new());
        lines.push("   Your profiles:".to_string());
        for g in profiles {
            let ports = g
                .udp_ports
                .map(|p| p.to_string())
                .unwrap_or_else(|| "none".to_string());
            let processes = if g.process_names.is_empty() {
                "no process names".to_string()
            } else {
                g.process_names.join(", ")
            };
            lines.push(format!(
                "   {:<12} ports {} - {} ({})",
                g.key, ports, processes, g.name
            ));
        }
        if let Some(path) = source {
            lines.push(format!("   from {}", path.display()));
        }
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const SAMPLE: &str = r#"
version = 1

[[games]]
name = "My Game"
udp_ports = "27015-27050"
process_names = ["MyGame.exe"]
"#;

    const SAMPLE_TWO: &str = r#"
version = 1

[[games]]
name = "My Game"
udp_ports = "27015-27050"

[[games]]
name = "Second Game"
key = "second"
udp_ports = 28015
"#;

    fn temp_dir(label: &str) -> PathBuf {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!(
            "lightspeed-profiles-{}-{}-{}",
            std::process::id(),
            label,
            n
        ));
        std::fs::create_dir_all(&dir).expect("create temp dir");
        dir
    }

    // ── store: load / reload ────────────────────────────────────────

    #[test]
    fn loads_a_file_then_reloads_it() {
        let dir = temp_dir("reload");
        let path = dir.join("profiles.toml");
        std::fs::write(&path, SAMPLE).unwrap();

        let store = ProfileStore::load_from(&path);
        assert!(store.is_enabled());
        assert_eq!(store.path().as_deref(), Some(path.as_path()));
        let first = store.snapshot();
        assert_eq!(first.games.len(), 1);
        assert_eq!(first.games[0].key, "my-game");
        assert_eq!(
            first.games[0].udp_ports.map(|p| p.to_string()).as_deref(),
            Some("27015-27050")
        );

        // The editor saves; the next resolve must see it without a restart.
        std::fs::write(&path, SAMPLE_TWO).unwrap();
        let second = store.reload();
        assert_eq!(second.games.len(), 2);
        assert_eq!(store.games().len(), 2);
        assert_eq!(store.games()[1].key, "second");
        assert_eq!(
            store.games()[1].udp_ports,
            PortRange::new(28015, 28015).ok()
        );

        // Reloading the same file twice is stable (and leak-free: the previous
        // Arc is dropped, nothing is interned).
        let third = store.reload();
        assert_eq!(third.games, second.games);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_missing_file_is_an_empty_document() {
        let dir = temp_dir("missing");
        let store = ProfileStore::load_from(dir.join("nope.toml"));
        assert!(store.is_enabled());
        assert!(store.snapshot().is_empty());
        assert!(store.snapshot().diagnostics.is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_malformed_file_is_reported_and_leaves_no_games() {
        let dir = temp_dir("malformed");
        let path = dir.join("profiles.toml");
        std::fs::write(&path, "not = = toml").unwrap();

        let store = ProfileStore::load_from(&path);
        assert!(store.games().is_empty());
        assert_eq!(store.snapshot().diagnostics.len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── --no-user-profiles ──────────────────────────────────────────

    #[test]
    fn store_config_maps_the_cli_flags() {
        assert_eq!(
            StoreConfig::from_cli_flags(None, false),
            StoreConfig::enabled()
        );
        assert_eq!(
            StoreConfig::from_cli_flags(Some("custom/profiles.toml"), false),
            StoreConfig::with_path("custom/profiles.toml")
        );
        assert_eq!(
            StoreConfig::from_cli_flags(None, true),
            StoreConfig::disabled()
        );
        // --no-user-profiles wins over an explicit path.
        assert_eq!(
            StoreConfig::from_cli_flags(Some("custom/profiles.toml"), true),
            StoreConfig::disabled()
        );
    }

    #[test]
    fn a_disabled_store_ignores_the_file_and_refuses_to_save() {
        let dir = temp_dir("disabled");
        let path = dir.join("profiles.toml");
        std::fs::write(&path, SAMPLE).unwrap();

        let store = ProfileStore::disabled();
        assert!(!store.is_enabled());
        assert!(store.path().is_none());
        assert!(store.snapshot().is_empty());
        assert!(store.reload().is_empty(), "reload must not read the file");
        assert!(store.save(&[], &[]).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── save through the store ──────────────────────────────────────

    #[test]
    fn save_round_trips_a_valid_document() {
        let dir = temp_dir("save");
        let path = dir.join("profiles.toml");
        let store = ProfileStore::load_from(&path);

        let game = UserGameProfile {
            name: "My Game".to_string(),
            key: "my-game".to_string(),
            process_names: vec!["MyGame.exe".to_string()],
            udp_ports: PortRange::new(27015, 27050).ok(),
            dynamic_server: true,
            notes: Some("community server".to_string()),
        };
        store
            .save(std::slice::from_ref(&game), &[])
            .expect("save a valid document");
        assert_eq!(store.games(), vec![game.clone()]);

        // A fresh store sees the same document.
        let reopened = ProfileStore::load_from(&path);
        assert_eq!(reopened.games(), vec![game]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn save_refuses_a_builtin_collision_and_writes_nothing() {
        let dir = temp_dir("save-collision");
        let path = dir.join("profiles.toml");
        std::fs::write(&path, SAMPLE).unwrap();
        let store = ProfileStore::load_from(&path);
        assert_eq!(store.games().len(), 1);

        let bad = UserGameProfile {
            name: "Sneaky".to_string(),
            key: "minecraft".to_string(),
            process_names: vec![],
            udp_ports: PortRange::new(27015, 27015).ok(),
            dynamic_server: false,
            notes: None,
        };
        let errors = store
            .save(&[bad], &[])
            .expect_err("save must refuse a built-in key");
        assert!(
            errors
                .iter()
                .any(|e| e.message.contains("reserved by built-in game 'Minecraft'")),
            "{errors:?}"
        );

        // The file on disk is untouched, and so is the cache.
        assert_eq!(std::fs::read_to_string(&path).unwrap(), SAMPLE);
        assert_eq!(store.games().len(), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    // ── --list-games ────────────────────────────────────────────────

    #[test]
    fn render_list_games_prints_builtins_then_your_profiles() {
        let builtins =
            vec!["   Fortnite     ports 5225-6500 - FortniteClient-Win64-Shipping.exe".to_string()];
        let profiles = vec![UserGameProfile {
            name: "My Game".to_string(),
            key: "my-game".to_string(),
            process_names: vec!["MyGame.exe".to_string()],
            udp_ports: PortRange::new(27015, 27050).ok(),
            dynamic_server: false,
            notes: None,
        }];
        let source = Path::new("/tmp/profiles.toml");
        let lines = render_list_games(&builtins, &profiles, Some(source));
        let body = lines.join("\n");

        let builtin_at = lines
            .iter()
            .position(|l| l.contains("Built-in games"))
            .expect("built-in section");
        let user_at = lines
            .iter()
            .position(|l| l.contains("Your profiles"))
            .expect("user section");
        assert!(builtin_at < user_at, "built-ins come first:\n{body}");
        assert!(body.contains("Fortnite"), "{body}");
        assert!(body.contains("my-game"), "{body}");
        assert!(body.contains("27015-27050"), "{body}");
        assert!(body.contains("MyGame.exe"), "{body}");
        assert!(body.contains("(My Game)"), "{body}");
        assert!(body.contains("from /tmp/profiles.toml"), "{body}");
    }

    #[test]
    fn render_list_games_omits_your_profiles_when_there_are_none() {
        let builtins = vec!["   Rust         ports 28015-28099 - RustClient.exe".to_string()];
        let lines = render_list_games(&builtins, &[], None);
        assert!(lines.iter().any(|l| l.contains("Built-in games")));
        assert!(!lines.iter().any(|l| l.contains("Your profiles")));
        assert_eq!(lines.len(), 2);
    }

    #[test]
    fn render_list_games_shows_a_profile_with_only_ports() {
        let profiles = vec![UserGameProfile {
            name: "Ports Only".to_string(),
            key: "ports-only".to_string(),
            process_names: vec![],
            udp_ports: PortRange::new(30000, 30000).ok(),
            dynamic_server: false,
            notes: None,
        }];
        let lines = render_list_games(&[], &profiles, None);
        let body = lines.join("\n");
        assert!(body.contains("ports-only"), "{body}");
        assert!(body.contains("30000"), "{body}");
        assert!(body.contains("no process names"), "{body}");
    }

    // ── reload stability ────────────────────────────────────────────

    /// Two reloads of the same file keep the names and keys byte-stable, and
    /// the store drops the superseded snapshot instead of stranding it: after
    /// the second reload the first handle is the sole owner of its document.
    #[test]
    fn reload_twice_keeps_names_and_keys_stable_with_no_leak_path() {
        let dir = temp_dir("reload-twice");
        let path = dir.join("profiles.toml");
        std::fs::write(&path, SAMPLE_TWO).unwrap();

        let store = ProfileStore::load_from(&path);

        let first = store.reload();
        // One reference is the store's own copy, one is the handle returned.
        assert_eq!(std::sync::Arc::strong_count(&first), 2);

        let second = store.reload();
        // The store swapped to the new snapshot and released the old one, so
        // the first handle is now its only owner - there is no leak path that
        // keeps a superseded document (or its names) alive.
        assert_eq!(
            std::sync::Arc::strong_count(&first),
            1,
            "the store must drop the snapshot it superseded"
        );
        assert_eq!(std::sync::Arc::strong_count(&second), 2);

        // Names and keys are stable across the second reload.
        assert_eq!(second.games.len(), 2);
        assert_eq!(second.games, first.games);
        assert_eq!(
            second
                .games
                .iter()
                .map(|g| g.key.as_str())
                .collect::<Vec<_>>(),
            vec!["my-game", "second"]
        );
        assert_eq!(
            second
                .games
                .iter()
                .map(|g| g.name.as_str())
                .collect::<Vec<_>>(),
            vec!["My Game", "Second Game"]
        );

        // Dropping the handles leaves the cache readable and unchanged.
        drop(first);
        drop(second);
        assert_eq!(
            store
                .games()
                .iter()
                .map(|g| g.key.clone())
                .collect::<Vec<_>>(),
            vec!["my-game", "second"]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
