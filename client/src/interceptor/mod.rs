//! # LightSpeed `TrafficInterceptor` framework
//!
//! A unified, object-oriented interface for MITM traffic interception across
//! all supported platforms.  The engine calls `create_interceptor()` to obtain
//! the best available backend for the current OS + build configuration.
//!
//! ## Quick usage from the engine
//!
//! ```rust,no_run
//! use lightspeed_client::interceptor::{create_interceptor, InterceptorConfig, Route};
//!
//! let config = InterceptorConfig {
//!     game_name: "Rust".into(),
//!     pid: Some(1234),          // from ProcessScanner
//!     port_range: (28015, 28017),
//!     initial_routes: vec![],   // or pre-filled from ProcessScanner
//!     proxy_addr: "185.1.2.3:4434".parse().unwrap(),
//!     fec_enabled: false,
//!     fec_k: 4,
//! };
//!
//! let interceptor = create_interceptor();
//! interceptor.check_availability().expect("interceptor not available");
//! let handle = interceptor.start(config).expect("failed to start");
//! // handle is dropped on stop
//! ```
//!
//! ## Platform selection
//!
//! | OS      | Feature flag             | Backend        | Precision      |
//! |---------|--------------------------|----------------|----------------|
//! | Windows | `windivert-redirect`     | WinDivert      | PID-level      |
//! | Linux   | (none)                   | nftables/iptables | Port+dest   |
//! | macOS   | (none)                   | pfctl          | Port+dest      |
//! | Other   | —                        | Unsupported    | —              |

#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod mock;
pub mod process_scanner;
pub mod traits;
#[cfg(target_os = "windows")]
pub mod windows;

// Re-export the most-used types at the module root for ergonomics.
pub use process_scanner::find_game_process;
#[allow(unused_imports)]
pub use traits::{
    InterceptorConfig, InterceptorHandle, Route, TrafficInterceptor, TransportProtocol,
    UnsupportedInterceptor,
};

// ─────────────────────────────────────────────────────────────────────────────
//  Factory
// ─────────────────────────────────────────────────────────────────────────────

/// Create the best available [`TrafficInterceptor`] for the current platform.
///
/// The returned object is heap-allocated and `Send + Sync` so it can be stored
/// in the engine and called from any thread.
///
/// Call [`TrafficInterceptor::check_availability`] before [`TrafficInterceptor::start`]
/// to surface a human-readable error if the backend is unavailable (e.g. missing
/// WinDivert files, missing root, etc.).
pub fn create_interceptor() -> Box<dyn TrafficInterceptor> {
    // Windows + windivert-redirect feature: use WinDivert (gold standard).
    #[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
    {
        tracing::debug!("TrafficInterceptor: selecting WinDivert backend");
        return Box::new(windows::WinDivertInterceptor::new());
    }

    // Windows without the feature: fall through to unsupported.
    #[cfg(all(target_os = "windows", not(feature = "windivert-redirect")))]
    {
        tracing::debug!("TrafficInterceptor: WinDivert feature not enabled");
        return Box::new(UnsupportedInterceptor::new(
            "WinDivert requires the 'windivert-redirect' Cargo feature.\n\
             Rebuild with: cargo build --features windivert-redirect"
                .to_string(),
        ));
    }

    // Linux: nftables / iptables.
    #[cfg(target_os = "linux")]
    {
        tracing::debug!("TrafficInterceptor: selecting nftables/iptables backend");
        return Box::new(linux::NftablesInterceptor::new());
    }

    // macOS: pfctl.
    #[cfg(target_os = "macos")]
    {
        tracing::debug!("TrafficInterceptor: selecting pfctl backend");
        return Box::new(macos::PfInterceptor::new());
    }

    // Fallback for any other OS.
    #[allow(unreachable_code)]
    Box::new(UnsupportedInterceptor::new(
        "No TrafficInterceptor backend available for this OS.".to_string(),
    ))
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers: build an InterceptorConfig from a GameConfig + ProcessScanner
// ─────────────────────────────────────────────────────────────────────────────

/// Build an [`InterceptorConfig`] by combining a game profile with a live
/// process scan.
///
/// This is the "automatic zero-config" entry point: it discovers the running
/// game process, reads its active UDP connections, and returns a config that
/// pre-seeds the interceptor with the real server address so it starts
/// MITM-ing immediately on the first packet.
///
/// Returns `None` if the game process is not currently running.
pub fn build_config_for_game(
    game: &dyn crate::games::GameConfig,
    proxy_addr: std::net::SocketAddrV4,
    fec_enabled: bool,
    fec_k: u8,
) -> Option<InterceptorConfig> {
    let process_names: Vec<&str> = game.process_names().to_vec();
    let info = find_game_process(&process_names);

    let (pid, initial_routes) = match info {
        Some(p) => {
            tracing::info!(
                "🎮 ProcessScanner: found {} (PID={}) with {} routes",
                p.name,
                p.pid,
                p.routes.len()
            );
            for r in &p.routes {
                tracing::info!("   Route: {}", r);
            }
            (Some(p.pid), p.routes)
        }
        None => {
            tracing::info!(
                "🎮 ProcessScanner: {} not running — interceptor will use port-range filter",
                game.name()
            );
            (None, vec![])
        }
    };

    let (lo, hi) = game.ports();
    Some(InterceptorConfig {
        game_name: game.name().to_string(),
        pid,
        port_range: (lo, hi),
        initial_routes,
        proxy_addr,
        fec_enabled,
        fec_k,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
//  Interception mode selection
// ─────────────────────────────────────────────────────────────────────────────

/// How the user wants game traffic intercepted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum InterceptionMode {
    /// Auto-select the best available backend (kernel first, userspace fallback).
    #[default]
    Auto,
    /// Force the userspace pcap capture backend.
    Userspace,
    /// Force the kernel-level MITM backend (nftables/pfctl/WinDivert).
    Kernel,
}

impl InterceptionMode {
    /// Parse a user-facing mode string into an [`InterceptionMode`].
    ///
    /// Accepts `"auto"`, `"userspace"`, and `"kernel"` (case-insensitive, with
    /// surrounding whitespace ignored). Returns a descriptive error for any
    /// other value.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Ok(Self::Auto),
            "userspace" => Ok(Self::Userspace),
            "kernel" => Ok(Self::Kernel),
            other => Err(format!(
                "invalid interception mode \"{other}\": expected \"auto\", \"userspace\", or \"kernel\""
            )),
        }
    }
}

/// Compute the effective interception mode from the CLI flag and config value.
///
/// The `--interception-mode` CLI flag wins over `config.interception.mode`.
/// When neither is explicitly set, `config` carries the default `"auto"` (see
/// [`crate::config::InterceptionConfig`]), which resolves to [`InterceptionMode::Auto`].
pub fn effective_interception_mode(
    cli: Option<&str>,
    config: &str,
) -> Result<InterceptionMode, String> {
    match cli {
        Some(value) => InterceptionMode::parse(value),
        None => InterceptionMode::parse(config),
    }
}

/// The concrete backend selected after resolving an [`InterceptionMode`] against
/// live probe results.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResolvedMode {
    Kernel,
    Userspace,
}

/// Resolve a requested [`InterceptionMode`] into a concrete [`ResolvedMode`]
/// given the live availability of each backend.
///
/// Pure function: no I/O, no side effects.
pub fn resolve_mode(
    requested: InterceptionMode,
    kernel_available: bool,
    userspace_available: bool,
) -> Result<ResolvedMode, String> {
    match requested {
        InterceptionMode::Kernel => {
            if kernel_available {
                Ok(ResolvedMode::Kernel)
            } else {
                Err("kernel interception unavailable".to_string())
            }
        }
        InterceptionMode::Userspace => {
            if userspace_available {
                Ok(ResolvedMode::Userspace)
            } else {
                Err("userspace interception unavailable".to_string())
            }
        }
        InterceptionMode::Auto => {
            if kernel_available {
                Ok(ResolvedMode::Kernel)
            } else if userspace_available {
                Ok(ResolvedMode::Userspace)
            } else {
                Err("no interception backend available".to_string())
            }
        }
    }
}

/// Probe the current build/runtime for backend availability.
///
/// Returns `(kernel_available, userspace_available)`.
pub fn probe_availability() -> (bool, bool) {
    let kernel_available = create_interceptor().check_availability().is_ok();

    #[cfg(feature = "pcap-capture")]
    let userspace_available = crate::capture::create_default_capture().is_ok();

    #[cfg(not(feature = "pcap-capture"))]
    let userspace_available = false;

    (kernel_available, userspace_available)
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interception_mode_default_is_auto() {
        assert_eq!(InterceptionMode::default(), InterceptionMode::Auto);
    }

    #[test]
    fn parse_interception_mode_valid_lowercase() {
        assert_eq!(
            InterceptionMode::parse("auto").unwrap(),
            InterceptionMode::Auto
        );
        assert_eq!(
            InterceptionMode::parse("userspace").unwrap(),
            InterceptionMode::Userspace
        );
        assert_eq!(
            InterceptionMode::parse("kernel").unwrap(),
            InterceptionMode::Kernel
        );
    }

    #[test]
    fn parse_interception_mode_case_and_whitespace_insensitive() {
        assert_eq!(
            InterceptionMode::parse("AUTO").unwrap(),
            InterceptionMode::Auto
        );
        assert_eq!(
            InterceptionMode::parse(" Userspace ").unwrap(),
            InterceptionMode::Userspace
        );
        assert_eq!(
            InterceptionMode::parse("Kernel").unwrap(),
            InterceptionMode::Kernel
        );
    }

    #[test]
    fn parse_interception_mode_invalid_errors() {
        assert!(InterceptionMode::parse("bogus").is_err());
        assert!(InterceptionMode::parse("").is_err());
        assert!(InterceptionMode::parse("   ").is_err());
        assert!(InterceptionMode::parse("user-space").is_err());
    }

    #[test]
    fn effective_interception_mode_cli_overrides_config() {
        assert_eq!(
            effective_interception_mode(Some("kernel"), "auto").unwrap(),
            InterceptionMode::Kernel
        );
        assert_eq!(
            effective_interception_mode(Some("userspace"), "kernel").unwrap(),
            InterceptionMode::Userspace
        );
        assert_eq!(
            effective_interception_mode(Some("auto"), "userspace").unwrap(),
            InterceptionMode::Auto
        );
    }

    #[test]
    fn effective_interception_mode_falls_back_to_config() {
        assert_eq!(
            effective_interception_mode(None, "kernel").unwrap(),
            InterceptionMode::Kernel
        );
        assert_eq!(
            effective_interception_mode(None, "userspace").unwrap(),
            InterceptionMode::Userspace
        );
        assert_eq!(
            effective_interception_mode(None, "auto").unwrap(),
            InterceptionMode::Auto
        );
    }

    #[test]
    fn effective_interception_mode_invalid_cli_errors() {
        assert!(effective_interception_mode(Some("banana"), "auto").is_err());
    }

    #[test]
    fn effective_interception_mode_invalid_config_errors() {
        assert!(effective_interception_mode(None, "banana").is_err());
    }

    #[test]
    fn resolve_mode_truth_table() {
        use InterceptionMode::*;

        let cases: Vec<(InterceptionMode, bool, bool, Result<ResolvedMode, &str>)> = vec![
            // ── Kernel requested: only kernel availability matters ──
            (Kernel, true, true, Ok(ResolvedMode::Kernel)),
            (Kernel, true, false, Ok(ResolvedMode::Kernel)),
            (Kernel, false, true, Err("kernel interception unavailable")),
            (Kernel, false, false, Err("kernel interception unavailable")),
            // ── Userspace requested: only userspace availability matters ──
            (Userspace, true, true, Ok(ResolvedMode::Userspace)),
            (
                Userspace,
                true,
                false,
                Err("userspace interception unavailable"),
            ),
            (Userspace, false, true, Ok(ResolvedMode::Userspace)),
            (
                Userspace,
                false,
                false,
                Err("userspace interception unavailable"),
            ),
            // ── Auto: prefer kernel, fall back to userspace ──
            (Auto, true, true, Ok(ResolvedMode::Kernel)),
            (Auto, true, false, Ok(ResolvedMode::Kernel)),
            (Auto, false, true, Ok(ResolvedMode::Userspace)),
            (Auto, false, false, Err("no interception backend available")),
        ];

        for (requested, kernel, userspace, expected) in cases {
            let actual = resolve_mode(requested, kernel, userspace);
            match expected {
                Ok(m) => assert_eq!(
                    actual,
                    Ok(m),
                    "requested={requested:?} kernel={kernel} userspace={userspace}"
                ),
                Err(e) => assert_eq!(
                    actual,
                    Err(e.to_string()),
                    "requested={requested:?} kernel={kernel} userspace={userspace}"
                ),
            }
        }
    }

    #[test]
    fn create_interceptor_is_always_some() {
        // Should never panic regardless of platform or available features.
        let i = create_interceptor();
        // Platform name must be non-empty.
        assert!(!i.platform_name().is_empty());
    }

    #[test]
    fn build_config_for_unknown_game_process_returns_some() {
        // Even if the process isn't running the config is returned (with empty routes).
        // At runtime, the interceptor falls back to port-range auto-detect mode.
        use crate::games::rust::RustConfig;
        let config =
            build_config_for_game(&RustConfig, "127.0.0.1:4434".parse().unwrap(), false, 4);
        assert!(config.is_some());
        let cfg = config.unwrap();
        assert_eq!(cfg.game_name, "Rust");
        assert_eq!(cfg.port_range, (28015, 28017));
        assert!(!cfg.fec_enabled);
    }

    // ── Mock interceptor integration tests ─────────────────────────

    #[test]
    fn mock_create_interceptor_lifecycle() {
        let interceptor = mock::MockInterceptor::new();
        assert_eq!(interceptor.platform_name(), "mock");
        assert!(interceptor.check_availability().is_ok());

        let config = InterceptorConfig {
            game_name: "Rust".into(),
            pid: Some(9999),
            port_range: (28015, 28017),
            initial_routes: vec![],
            proxy_addr: "127.0.0.1:4434".parse().unwrap(),
            fec_enabled: true,
            fec_k: 4,
        };

        let mut handle = interceptor.start(config.clone()).unwrap();
        assert_eq!(interceptor.start_count(), 1);

        let saved = interceptor.last_config().unwrap();
        assert_eq!(saved.game_name, "Rust");
        assert!(saved.fec_enabled);
        assert_eq!(saved.fec_k, 4);

        handle.stop();
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(interceptor.stop_count(), 1);
    }

    #[test]
    fn mock_unavailable_reports_error() {
        let interceptor = mock::MockInterceptor::unavailable();
        assert!(interceptor.check_availability().is_err());
    }

    #[test]
    fn mock_handle_counters_default_zero() {
        let interceptor = mock::MockInterceptor::new();
        let handle = interceptor
            .start(InterceptorConfig {
                game_name: "Test".into(),
                pid: None,
                port_range: (1, 2),
                initial_routes: vec![],
                proxy_addr: "127.0.0.1:4434".parse().unwrap(),
                fec_enabled: false,
                fec_k: 4,
            })
            .unwrap();

        let snap = handle.snapshot();
        assert_eq!(snap.packets_intercepted, 0);
        assert_eq!(snap.packets_from_proxy, 0);
        assert_eq!(snap.packets_injected, 0);
        assert_eq!(snap.errors, 0);
    }

    #[test]
    fn process_scanner_scan_for_games_empty_input() {
        let results = process_scanner::scan_for_games(&[]);
        assert!(results.is_empty());
    }

    #[test]
    fn process_scanner_find_nonexistent_game() {
        let result = process_scanner::find_game_process(&["nonexistent_game_xyz_123"]);
        assert!(result.is_none());
    }

    #[test]
    fn route_display() {
        use std::net::{Ipv4Addr, SocketAddrV4};
        let r = Route {
            local: SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 5), 54321),
            remote: SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 28015),
            proto: TransportProtocol::Udp,
        };
        let s = r.to_string();
        assert!(s.contains("UDP"));
        assert!(s.contains("1.2.3.4:28015"));
    }
}
