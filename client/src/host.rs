//! Host facts that are necessarily platform-conditional.
//!
//! The shared application core (`client/src/app/`) must build the same on every
//! target (`client/tests/gui_boundary.rs` G5), so the two Windows-only identity
//! facts the lifecycle record needs live here instead: whether this process is
//! elevated, and a boot id that changes on reboot.
//!
//! Both are read-only observations used only by the lifecycle identity block.
//! Nothing in the product's behaviour branches on them.

/// Whether the current process token is elevated.
///
/// Used only to stamp the lifecycle identity (`elevated=true|false`); a wrong
/// answer would mislabel a support record, never change capture behaviour.
pub fn elevated() -> bool {
    #[cfg(windows)]
    {
        #[link(name = "shell32")]
        extern "system" {
            fn IsUserAnAdmin() -> i32;
        }
        // SAFETY: [Category 8 - FFI] `IsUserAnAdmin` takes no arguments and only
        // reads the current process token.
        unsafe { IsUserAnAdmin() != 0 }
    }
    #[cfg(not(windows))]
    {
        false
    }
}

/// An opaque boot id: a hex value derived from `now - uptime`.
///
/// Two runs on the same boot share the id (so a trace gap can be told apart from
/// a reboot); a reboot changes it. `None` where the platform exposes no uptime.
pub fn boot_id() -> Option<String> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        extern "system" {
            fn GetTickCount64() -> u64;
        }
        // SAFETY: [Category 8 - FFI] `GetTickCount64` takes no arguments and is
        // always callable.
        let uptime_ms = unsafe { GetTickCount64() };
        let boot_ms = now_unix_ms().saturating_sub(uptime_ms);
        Some(format!("{boot_ms:x}"))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// OS and architecture, as a stable identity string (`windows x86_64`).
pub fn description() -> String {
    format!("{} {}", std::env::consts::OS, std::env::consts::ARCH)
}

fn now_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

/// The read-only `sc.exe query WinDivert` result for the support bundle.
#[derive(Clone, Debug)]
pub struct ServiceQuery {
    pub exit_code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

/// Run the read-only service query; `None` where the platform has no SCM.
///
/// The support bundle uses this to record what the kernel service looked like
/// when an open failed. It changes no state.
pub fn windivert_service_query() -> Option<ServiceQuery> {
    #[cfg(windows)]
    {
        match std::process::Command::new("sc.exe")
            .args(["query", "WinDivert"])
            .output()
        {
            Ok(output) => Some(ServiceQuery {
                exit_code: output.status.code(),
                stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
                stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            }),
            Err(_) => None,
        }
    }
    #[cfg(not(windows))]
    {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_host_description_names_os_and_arch() {
        let described = description();
        assert!(described.contains(std::env::consts::OS));
        assert!(described.contains(std::env::consts::ARCH));
    }

    #[test]
    fn a_boot_id_is_a_stable_hex_value_on_windows() {
        match boot_id() {
            Some(id) => {
                assert!(!id.is_empty());
                assert!(id.chars().all(|ch| ch.is_ascii_hexdigit()));
                // Reading it twice in one process must not change the boot id.
                assert_eq!(Some(id), boot_id());
            }
            None => {
                // Non-Windows: the fact is unavailable, not invented.
            }
        }
    }
}
