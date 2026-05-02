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

pub mod linux;
pub mod macos;
pub mod process_scanner;
pub mod traits;
pub mod windows;

// Re-export the most-used types at the module root for ergonomics.
pub use process_scanner::{find_game_process, scan_for_games};
pub use traits::{
    InterceptorConfig, InterceptorCounters, InterceptorHandle, InterceptorStats,
    ProcessInfo, Route, TrafficInterceptor, TransportProtocol, UnsupportedInterceptor,
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
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

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
        let config = build_config_for_game(
            &RustConfig,
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
        );
        assert!(config.is_some());
        let cfg = config.unwrap();
        assert_eq!(cfg.game_name, "Rust");
        assert_eq!(cfg.port_range, (28015, 28017));
        assert!(!cfg.fec_enabled);
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