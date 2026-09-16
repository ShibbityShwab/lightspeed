//! Filesystem paths and OS integration helpers.
//!
//! The GUI has no console, so the two artifacts a user may need to inspect
//! (the trace log and the proxy config) get a single source of truth here,
//! together with a cross-platform "open in the OS viewer" helper.

use std::path::{Path, PathBuf};

/// Per-machine GUI data directory (`%LOCALAPPDATA%\Lightspeed` on Windows,
/// `~/.local/share/Lightspeed` elsewhere). Holds the trace log and the
/// single-instance lock file.
pub fn data_dir() -> PathBuf {
    dirs::data_local_dir()
        .unwrap_or_default()
        .join("Lightspeed")
}

/// `gui-trace.log` — the tracing sink wired up in `main`.
pub fn log_file() -> PathBuf {
    data_dir().join("gui-trace.log")
}

/// User-editable config directory (`%APPDATA%\light-speed` on Windows,
/// `~/.config/light-speed` elsewhere).
pub fn config_dir() -> PathBuf {
    dirs::config_dir().unwrap_or_default().join("light-speed")
}

/// `config.toml` — persisted proxy list + selected relay.
pub fn config_file() -> PathBuf {
    config_dir().join("config.toml")
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
    match std::process::Command::new(program).arg(path).spawn() {
        Ok(_) => tracing::info!("Opened {} in OS viewer", path.display()),
        Err(e) => tracing::warn!("Failed to open {}: {e}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::{config_file, log_file};

    #[test]
    fn log_file_lives_under_the_data_dir() {
        assert!(log_file().ends_with("Lightspeed/gui-trace.log"));
    }

    #[test]
    fn config_file_lives_under_the_config_dir() {
        assert!(config_file().ends_with("light-speed/config.toml"));
    }
}
