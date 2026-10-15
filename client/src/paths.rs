//! Per-OS filesystem locations for the client.
//!
//! One implementation for the whole tree: the CLI's `--profiles`/`--no-user-profiles`
//! and the GUI's config path both resolve here, so the two surfaces cannot drift
//! onto different files. The rules match `dirs::config_dir()` - the resolver the
//! GUI already uses - so an existing install keeps the same directory:
//!
//! | OS      | Config directory                                             |
//! |---------|--------------------------------------------------------------|
//! | Windows | `%APPDATA%\light-speed`                                       |
//! | macOS   | `~/Library/Application Support/light-speed`                   |
//! | Linux   | `$XDG_CONFIG_HOME/light-speed` (default `~/.config/light-speed`) |
//!
//! The environment lookup is injected ([`config_dir_for`]) so every rule is
//! testable on any host; [`config_dir`] is the live host's answer.

use std::path::PathBuf;

/// The OS a path is resolved for. Explicit rather than `cfg!` so the rules can
/// be tested on a single host.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    Windows,
    MacOs,
    Linux,
}

/// The platform this binary was built for.
pub fn current_platform() -> Platform {
    if cfg!(target_os = "windows") {
        Platform::Windows
    } else if cfg!(target_os = "macos") {
        Platform::MacOs
    } else {
        Platform::Linux
    }
}

/// The per-user config directory for `platform`, from `env`.
///
/// `None` when the environment carries no usable base (no `HOME`, no
/// `%APPDATA%`): a caller then has no profiles file rather than a relative one
/// written into the current directory.
///
/// On Linux a relative `XDG_CONFIG_HOME` is ignored, as the XDG spec requires.
/// On Windows `%APPDATA%` wins and `%USERPROFILE%\AppData\Roaming` is the
/// fallback for a stripped environment.
pub fn config_dir_for(platform: Platform, env: &dyn Fn(&str) -> Option<String>) -> Option<PathBuf> {
    let base = match platform {
        Platform::Windows => match env("APPDATA").filter(|s| !s.trim().is_empty()) {
            Some(appdata) => PathBuf::from(appdata),
            None => PathBuf::from(env("USERPROFILE").filter(|s| !s.trim().is_empty())?)
                .join("AppData")
                .join("Roaming"),
        },
        Platform::MacOs => PathBuf::from(env("HOME").filter(|s| !s.trim().is_empty())?)
            .join("Library")
            .join("Application Support"),
        Platform::Linux => match env("XDG_CONFIG_HOME")
            .filter(|s| !s.trim().is_empty() && is_posix_absolute(s))
        {
            Some(xdg) => PathBuf::from(xdg),
            None => PathBuf::from(env("HOME").filter(|s| !s.trim().is_empty())?).join(".config"),
        },
    };
    Some(base.join("light-speed"))
}

/// Whether `s` is absolute in the POSIX sense (starts with `/`).
///
/// Deliberately not [`Path::is_absolute`]: that answer depends on the build
/// host, and on Windows `/xdg` is not absolute while on Linux it is. The XDG
/// rule is a Linux rule, so it has to be the Linux answer on every host.
fn is_posix_absolute(s: &str) -> bool {
    s.starts_with('/')
}

/// The config directory on this host.
pub fn config_dir() -> Option<PathBuf> {
    config_dir_for(current_platform(), &|key| std::env::var(key).ok())
}

/// `profiles.toml`, next to the config file in [`config_dir`].
pub fn profiles_file() -> Option<PathBuf> {
    config_dir().map(|dir| dir.join("profiles.toml"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    /// An environment lookup over a fixed table. Never touches the real env, so
    /// every rule holds on every host. Owns its pairs, so no call site has to
    /// keep a table alive just to hold the closure.
    fn env_of<const N: usize>(
        pairs: [(&'static str, &'static str); N],
    ) -> impl Fn(&str) -> Option<String> {
        move |key| {
            pairs
                .iter()
                .find(|(k, _)| *k == key)
                .map(|(_, v)| (*v).to_string())
        }
    }

    /// An environment with none of the keys a path rule may need.
    fn no_env() -> impl Fn(&str) -> Option<String> {
        |_| None
    }

    fn slashed(path: &Path) -> String {
        path.to_string_lossy().replace('\\', "/")
    }

    #[test]
    fn windows_uses_appdata() {
        let env = env_of([("APPDATA", r"C:\Users\me\AppData\Roaming")]);
        let dir = config_dir_for(Platform::Windows, &env).unwrap();
        assert_eq!(slashed(&dir), "C:/Users/me/AppData/Roaming/light-speed");
    }

    #[test]
    fn windows_falls_back_to_userprofile() {
        let env = env_of([("USERPROFILE", r"C:\Users\me")]);
        let dir = config_dir_for(Platform::Windows, &env).unwrap();
        assert_eq!(slashed(&dir), "C:/Users/me/AppData/Roaming/light-speed");

        assert!(config_dir_for(Platform::Windows, &no_env()).is_none());
    }

    #[test]
    fn macos_uses_library_application_support() {
        let env = env_of([("HOME", "/Users/me")]);
        let dir = config_dir_for(Platform::MacOs, &env).unwrap();
        assert_eq!(
            slashed(&dir),
            "/Users/me/Library/Application Support/light-speed"
        );

        assert!(config_dir_for(Platform::MacOs, &no_env()).is_none());
    }

    #[test]
    fn linux_prefers_xdg_config_home() {
        let env = env_of([("HOME", "/home/me"), ("XDG_CONFIG_HOME", "/xdg")]);
        let dir = config_dir_for(Platform::Linux, &env).unwrap();
        assert_eq!(slashed(&dir), "/xdg/light-speed");
    }

    #[test]
    fn linux_defaults_to_dot_config() {
        let env = env_of([("HOME", "/home/me")]);
        let dir = config_dir_for(Platform::Linux, &env).unwrap();
        assert_eq!(slashed(&dir), "/home/me/.config/light-speed");
    }

    #[test]
    fn linux_ignores_a_relative_or_empty_xdg_config_home() {
        for xdg in ["relative/dir", "", "   "] {
            let env = env_of([("HOME", "/home/me"), ("XDG_CONFIG_HOME", xdg)]);
            let dir = config_dir_for(Platform::Linux, &env).unwrap();
            assert_eq!(
                slashed(&dir),
                "/home/me/.config/light-speed",
                "XDG_CONFIG_HOME={xdg:?} must be ignored"
            );
        }

        assert!(config_dir_for(Platform::Linux, &no_env()).is_none());
    }

    #[test]
    fn profiles_file_sits_next_to_the_config_file() {
        let env = env_of([("HOME", "/home/me"), ("XDG_CONFIG_HOME", "/xdg")]);
        let dir = config_dir_for(Platform::Linux, &env).unwrap();
        assert_eq!(
            slashed(&dir.join("profiles.toml")),
            "/xdg/light-speed/profiles.toml"
        );

        // The live host resolves a file under `light-speed` when it has a base.
        if let Some(file) = profiles_file() {
            assert!(file.ends_with(Path::new("light-speed").join("profiles.toml")));
        }
    }
}
