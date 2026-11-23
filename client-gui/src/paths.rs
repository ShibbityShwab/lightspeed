//! Filesystem paths and OS integration helpers.
//!
//! The GUI has no console, so the two artifacts a user may need to inspect
//! (the trace log and the proxy config) get a single source of truth here,
//! together with a cross-platform "open in the OS viewer" helper.

use std::path::{Path, PathBuf};

use lightspeed_client::app::config::ConfigPaths;

/// Per-machine GUI data directory (`%LOCALAPPDATA%\Lightspeed` on Windows,
/// `~/.local/share/Lightspeed` elsewhere). Holds the trace log and the
/// single-instance lock file.
///
/// `LIGHTSPEED_DATA_DIR` overrides it wholesale. That is the supported way to
/// run the GUI against a disposable profile for evidence/automation: the
/// Windows known-folder API ignores `LOCALAPPDATA`/`APPDATA` overrides, so
/// setting those alone does **not** isolate a run.
pub fn data_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LIGHTSPEED_DATA_DIR") {
        return PathBuf::from(dir);
    }
    dirs::data_local_dir()
        .unwrap_or_default()
        .join("Lightspeed")
}

/// `gui-trace.log` - the tracing sink wired up in `main`.
pub fn log_file() -> PathBuf {
    data_dir().join("gui-trace.log")
}

/// `gui-crash.log`: panic/fatal records written by [`crate::crash`].
/// `relay-history.json`: what each relay did for each game server, across
/// sessions. Machine state, not user configuration.
pub fn relay_history_file() -> PathBuf {
    data_dir().join("relay-history.json")
}

pub fn crash_log() -> PathBuf {
    data_dir().join("gui-crash.log")
}

/// User-editable config directory (`%APPDATA%\light-speed` on Windows,
/// `~/.config/light-speed` elsewhere).
///
/// `LIGHTSPEED_CONFIG_DIR` overrides it wholesale for a disposable profile;
/// see [`data_dir`] for why overriding `APPDATA` is not enough on Windows.
pub fn config_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("LIGHTSPEED_CONFIG_DIR") {
        return PathBuf::from(dir);
    }
    dirs::config_dir().unwrap_or_default().join("light-speed")
}

/// The shared config files (WF-081 W2-3b S6c / owner Q10).
///
/// The canonical file (`lightspeed.toml`, the same name the CLI uses) is the one
/// the GUI reads and writes; the pre-unification GUI file (`config.toml`) is
/// imported **read-only** and never written, renamed or deleted. Both live in the
/// per-user config directory above.
pub fn config_paths() -> ConfigPaths {
    let dir = config_dir();
    ConfigPaths::in_dir(&dir)
}

/// Open `path` in the OS default handler (Explorer, Finder, xdg-open).
///
/// Failures are logged, never propagated: a missing handler must not take the
/// GUI down or panic.
pub fn open_in_os(path: &Path) {
    let program = if cfg!(target_os = "windows") {
        "explorer"
    } else if cfg!(target_os = "macos") {
        "open"
    } else {
        "xdg-open"
    };
    match crate::platform::silent_command(program).arg(path).spawn() {
        Ok(_) => tracing::info!("Opened {} in OS viewer", path.display()),
        Err(e) => tracing::warn!("Failed to open {}: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{config_paths, crash_log, log_file};

    #[test]
    fn log_file_lives_under_the_data_dir() {
        assert!(log_file().ends_with("Lightspeed/gui-trace.log"));
    }

    #[test]
    fn config_paths_are_canonical_and_legacy_in_the_config_dir() {
        let paths = config_paths();
        assert!(paths.canonical.ends_with("light-speed/lightspeed.toml"));
        assert!(paths.legacy.ends_with("light-speed/config.toml"));
    }

    #[test]
    fn crash_log_lives_under_the_data_dir() {
        assert!(crash_log().ends_with("Lightspeed/gui-crash.log"));
    }
}
