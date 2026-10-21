//! # LightSpeed `TrafficInterceptor` framework
//!
//! A unified, object-oriented interface for MITM traffic interception across
//! all supported platforms.  The engine calls `create_interceptor()` to obtain
//! the best available backend for the current OS + build configuration.
//!
//! ## Quick usage from the engine
//!
//! ```rust,no_run
//! use lightspeed_client::interceptor::{
//!     create_interceptor, InterceptorConfig, ProcessTarget, Route, TransportProto,
//! };
//!
//! let config = InterceptorConfig {
//!     game_name: "Rust".into(),
//!     dynamic_server: false,                // resolved from the game profile
//!     transport: TransportProto::Udp,       // resolved from the game profile
//!     process_names: vec!["RustClient.exe".to_string()],
//!     // The one way to say "intercept this process"; `None` is the ports path.
//!     process_target: Some(ProcessTarget::by_name("RustClient.exe")),
//!     port_range: (28015, 28017),
//!     initial_routes: vec![],   // or pre-filled from ProcessScanner
//!     proxy_addr: "185.1.2.3:4434".parse().unwrap(),
//!     fec_enabled: false,
//!     fec_k: 4,
//!     adaptive_fec: Default::default(),
//!     bypass: Default::default(),
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
//! | Other   |,                        | Unsupported    |,              |

pub mod bypass;
pub mod flows;
#[cfg(target_os = "linux")]
pub mod linux;
#[cfg(target_os = "macos")]
pub mod macos;
pub mod mock;
pub mod order;
pub mod pf_rules;
pub mod process_scanner;
pub mod recovery;
pub mod rotation;
#[cfg(target_os = "linux")]
mod shadow_probe;
pub mod socket_owners;
pub mod teardown;
pub mod traits;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
pub mod windivert_handle;
#[cfg(target_os = "windows")]
pub mod windows;

// Re-export the most-used types at the module root for ergonomics.
pub use process_scanner::find_game_process;
// The transport kind a resolved profile declares lives in `games`; re-exported
// so a config builder can name the type the config carries. `allow` because the
// binary target's module tree reuses this module without naming the type.
#[allow(unused_imports)]
pub use crate::games::TransportProto;
#[allow(unused_imports)]
pub use traits::{
    InterceptorConfig, InterceptorHandle, ProcessTarget, Route, TrafficInterceptor,
    TransportProtocol, UnsupportedInterceptor,
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
//  Capture policy (the Windows data plane's one switch)
// ─────────────────────────────────────────────────────────────────────────────

/// Which traffic the Windows WinDivert data plane captures at the network layer.
///
/// This is the data plane's ONE policy switch, and [`capture_policy`] is the
/// ONLY place it is derived from a config. Because the choice is a value and not
/// code, a later measurement can flip the default without touching the data
/// plane in `windows.rs`:
///
/// - today the process path derives [`CapturePolicy::ProcessAll`] and the
///   built-in game path derives [`CapturePolicy::PortRange`];
/// - if the benchmark in `docs/design/custom-games-and-process-targeting.md`
///   section 7 clears capture-all for built-ins, only `capture_policy` moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CapturePolicy {
    /// Capture every outbound UDP datagram (minus the exclusions) and attribute
    /// each packet in userspace.
    ///
    /// This is the process-targeted path: the network-layer filter cannot match
    /// a `processId`, so it captures the whole process's UDP and every packet's
    /// flow is looked up in the bounded flow map. Only a flow attributed to the
    /// selected process is relayed; everything else is re-injected on the direct
    /// path, never dropped.
    ProcessAll,
    /// Capture one UDP destination-port range, inclusive: the built-in game
    /// path, unchanged.
    PortRange { lo: u16, hi: u16 },
}

/// Derive the capture policy from a config: the ONE place this happens.
///
/// The config value that decides is `process_target` - the single way a caller
/// says "intercept this process". When it is set the capture is process-wide
/// ([`CapturePolicy::ProcessAll`]); every other config, the built-in game path
/// and the Manual/custom ports path alike, keeps the port range it already
/// carries ([`CapturePolicy::PortRange`]).
///
/// An empty [`ProcessTarget`] is not a target: it never widens capture on a
/// config that would otherwise be port-scoped.
pub fn capture_policy(config: &InterceptorConfig) -> CapturePolicy {
    match config.process_target.as_ref() {
        Some(target) if target.is_set() => CapturePolicy::ProcessAll,
        _ => CapturePolicy::PortRange {
            lo: config.port_range.0,
            hi: config.port_range.1,
        },
    }
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
///
/// Carries no Manual port-range override: every caller that has no such field
/// (the CLI paths and the watch/demo modes) goes through this entry point and
/// keeps the profile's own range. The GUI's persisted "Custom Port Range"
/// comes in through [`build_config_for_game_with_port_range`].
///
/// **Precedence.** This is the *ports path*: the config it builds carries no
/// [`ProcessTarget`], so nothing here can pre-empt a process target. A caller
/// that has a picked process builds through
/// [`build_config_for_process_target`], and a target set there wins over every
/// port-range derivation on this path.
pub fn build_config_for_game(
    game: &dyn crate::games::GameConfig,
    proxy_addr: std::net::SocketAddrV4,
    fec_enabled: bool,
    fec_k: u8,
    adaptive_fec: crate::tunnel::adaptive::AdaptiveConfig,
) -> Option<InterceptorConfig> {
    build_config_for_game_with_port_range(game, proxy_addr, fec_enabled, fec_k, adaptive_fec, None)
}

/// [`build_config_for_game`] with a user-supplied port range.
///
/// This is where the Manual / Custom flow's persisted "Custom Port Range"
/// actuates. Precedence, manual first:
///
/// 1. `port_range_override` - the Manual field, as the GUI parsed it.
/// 2. the profile's own UDP range, which [`crate::games::GameConfig::ports`]
///    reports: a user profile returns its `udp_ports` field (or `(0, 0)` when
///    the entry declares none), a built-in the range it ships.
///
/// A range the user supplied is never silently dropped: it either becomes the
/// config's `port_range`, or the parse failed upstream and the field was empty,
/// in which case the profile default stands.
///
/// **Precedence.** This is still the *ports path* - the config it returns never
/// carries a [`ProcessTarget`]. A picked process is expressed only through
/// [`build_config_for_process_target`], whose target wins over the range this
/// function resolves.
pub fn build_config_for_game_with_port_range(
    game: &dyn crate::games::GameConfig,
    proxy_addr: std::net::SocketAddrV4,
    fec_enabled: bool,
    fec_k: u8,
    adaptive_fec: crate::tunnel::adaptive::AdaptiveConfig,
    port_range_override: Option<(u16, u16)>,
) -> Option<InterceptorConfig> {
    let process_names: Vec<String> = game.process_names().iter().map(|s| s.to_string()).collect();
    let process_refs: Vec<&str> = process_names.iter().map(|s| s.as_str()).collect();
    let discovered_routes = match find_game_process(&process_refs) {
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
            p.routes
        }
        None => {
            // The game is not running, so there is nothing to seed from. The
            // caller decides whether that is fatal: [`Self`] is a pure config
            // builder and still returns a config (with empty routes) so a
            // preview or a dry run works, while `engine::start_interceptor`
            // refuses to start interception on this same condition. Keeping the
            // environment check out of here is what lets both behaviours exist
            // without one lying to the other.
            tracing::info!(
                "🎮 ProcessScanner: {} is not running; no routes to seed",
                game.name()
            );
            vec![]
        }
    };

    // Manual-first: a range the user typed beats the profile's own.
    let (lo, hi) = port_range_override.unwrap_or_else(|| game.ports());
    Some(assemble_config(
        game,
        None,
        discovered_routes,
        (lo, hi),
        proxy_addr,
        fec_enabled,
        fec_k,
        adaptive_fec,
    ))
}

/// Build an [`InterceptorConfig`] whose interception target is a picked
/// process rather than a port range.
///
/// **Precedence, stated once.** A `process_target` on the config is
/// authoritative: when it is set it wins over every port-range derivation.
/// [`build_config_for_game`] and [`build_config_for_game_with_port_range`]
/// therefore always build a config with no target, and this constructor is the
/// only way one arrives - there is exactly one way to say "this process".
///
/// `target` is already bound to the live process table by
/// [`resolve_process_target`], so its `pid` is the PID live at start time (the
/// image name is the identity, so a relaunch still matches) and `live_routes`
/// are that process's routes. The carried `port_range` is the profile's own and
/// is inert while the target stands.
pub fn build_config_for_process_target(
    game: &dyn crate::games::GameConfig,
    target: ProcessTarget,
    live_routes: Vec<Route>,
    proxy_addr: std::net::SocketAddrV4,
    fec_enabled: bool,
    fec_k: u8,
    adaptive_fec: crate::tunnel::adaptive::AdaptiveConfig,
) -> InterceptorConfig {
    assemble_config(
        game,
        Some(target),
        live_routes,
        game.ports(),
        proxy_addr,
        fec_enabled,
        fec_k,
        adaptive_fec,
    )
}

/// Bind a picked [`ProcessTarget`] to the live process table.
///
/// The image name is the identity: when the target names one, the PID the
/// scanner just observed replaces whatever PID the target carried, so a game
/// relaunched under a new PID rebinds by name instead of pinning a stale
/// number. Returns `None` only when the target names an image and no such
/// process is running - that is "not running", and the caller must report it
/// rather than quietly fall back to a port range.
///
/// A bare-PID target is returned as-is: the name-based scanner cannot confirm
/// or refresh it, so the caller's assertion stands and no port-only fallback
/// is invented behind its back.
pub fn resolve_process_target(
    target: &ProcessTarget,
    live: Option<&traits::ProcessInfo>,
) -> Option<ProcessTarget> {
    match target.name() {
        Some(name) => live.map(|p| ProcessTarget {
            name: Some(name.to_string()),
            pid: Some(p.pid),
        }),
        None => target.pid.map(ProcessTarget::by_pid),
    }
}

/// Assemble a config from already-resolved pieces.
///
/// Both target paths end here, so the resolved flags, the dynamic-server
/// seeding rule and the field set live in exactly one place.
fn assemble_config(
    game: &dyn crate::games::GameConfig,
    process_target: Option<ProcessTarget>,
    discovered_routes: Vec<Route>,
    port_range: (u16, u16),
    proxy_addr: std::net::SocketAddrV4,
    fec_enabled: bool,
    fec_k: u8,
    adaptive_fec: crate::tunnel::adaptive::AdaptiveConfig,
) -> InterceptorConfig {
    // The names the config advertises are the ones it actually targets: the
    // picked image for a process target (which is also what Linux re-scans),
    // the profile's own otherwise.
    let process_names: Vec<String> = match process_target.as_ref().and_then(|t| t.name()) {
        Some(name) => vec![name.to_string()],
        None => game.process_names().iter().map(|s| s.to_string()).collect(),
    };

    // Every resolved flag is read off the profile here, once. Nothing further
    // down the config path may re-derive them from `game_name`.
    let dynamic_server = game.dynamic_server();

    // Seeds go through the single place the dynamic-server rule lives: a
    // rotating profile gets no seed (its first discovered route is a lobby
    // address the match has already left), while a static profile keeps every
    // discovered route. The config carries the result, so no consumer can
    // seed a rotating profile by reading `initial_routes` directly.
    let discovered: Vec<std::net::SocketAddrV4> =
        discovered_routes.iter().map(|r| r.remote).collect();
    let seed_addrs = order::effective_seeds(dynamic_server, &discovered);
    let initial_routes = discovered_routes
        .into_iter()
        .filter(|route| seed_addrs.contains(&route.remote))
        .collect();

    let capture_tcp_ports = {
        let tcp = game.capture_tcp_ports();
        (tcp != (0, 0)).then_some(tcp)
    };

    InterceptorConfig {
        game_name: game.name().to_string(),
        dynamic_server,
        transport: game.transport(),
        capture_tcp_ports,
        process_names,
        process_target,
        port_range,
        initial_routes,
        proxy_addr,
        fec_enabled,
        fec_k,
        adaptive_fec,
        bypass: bypass::default_config(),
    }
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
        let config = build_config_for_game(
            &RustConfig,
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
            Default::default(),
        );
        assert!(config.is_some());
        let cfg = config.unwrap();
        assert_eq!(cfg.game_name, "Rust");
        assert_eq!(cfg.port_range, (28015, 28017));
        assert!(!cfg.fec_enabled);
    }

    #[test]
    fn build_config_for_a_dynamic_profile_carries_the_flag_and_no_seeds() {
        use crate::games::fortnite::FortniteConfig;
        use std::net::SocketAddrV4;

        let cfg = build_config_for_game(
            &FortniteConfig,
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
            Default::default(),
        )
        .expect("a config is built even when the process is not running");

        assert!(
            cfg.dynamic_server,
            "Fortnite rotates through ephemeral server addresses"
        );
        assert_eq!(cfg.transport, TransportProto::Udp);
        assert!(
            cfg.process_names
                .contains(&"FortniteClient-Win64-Shipping.exe".to_string()),
            "process names come from the resolved profile: {:?}",
            cfg.process_names
        );

        // A rotating title must never seed: its first discovered route is a
        // lobby address the match has already left.
        let routes: Vec<SocketAddrV4> = cfg.initial_routes.iter().map(|r| r.remote).collect();
        assert!(
            routes.is_empty(),
            "a dynamic profile receives no seed route, got {routes:?}"
        );
        assert!(order::effective_seeds(cfg.dynamic_server, &routes).is_empty());
    }

    #[test]
    fn build_config_for_a_static_profile_keeps_its_seeds() {
        use crate::games::rust::RustConfig;
        use std::net::SocketAddrV4;

        let cfg = build_config_for_game(
            &RustConfig,
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
            Default::default(),
        )
        .expect("a config is built even when the process is not running");

        assert!(!cfg.dynamic_server, "Rust does not rotate servers");
        assert_eq!(cfg.transport, TransportProto::Udp);
        assert!(
            cfg.process_names.contains(&"RustClient.exe".to_string()),
            "process names come from the resolved profile: {:?}",
            cfg.process_names
        );

        let routes: Vec<SocketAddrV4> = cfg.initial_routes.iter().map(|r| r.remote).collect();
        assert_eq!(order::effective_seeds(cfg.dynamic_server, &routes), routes);

        // Not vacuous when CI has no game running: a static profile keeps a
        // discovered set intact rather than collapsing to its first route.
        let discovered: Vec<SocketAddrV4> = vec![
            "1.2.3.4:28015".parse().unwrap(),
            "5.6.7.8:28015".parse().unwrap(),
        ];
        assert_eq!(order::effective_seeds(false, &discovered), discovered);
    }

    // ── Port-range precedence: Manual > profile udp_ports > ports() ──

    /// A profile-shaped double standing in for the profile tiers.
    ///
    /// A user profile surfaces its `udp_ports` through `ports()` (the
    /// `games` adapter derives exactly that), so `ports()` is the accessor for
    /// both profile tiers: the double is the profile, whatever it declared.
    struct ProfilePorts(u16, u16);

    impl crate::games::GameConfig for ProfilePorts {
        fn name(&self) -> &str {
            "Profile Ports"
        }

        fn process_names(&self) -> Vec<&str> {
            vec!["ProfilePorts.exe"]
        }

        fn ports(&self) -> (u16, u16) {
            (self.0, self.1)
        }

        fn anti_cheat(&self) -> &str {
            "user-defined"
        }

        fn typical_pps(&self) -> u32 {
            60
        }

        fn packet_size_range(&self) -> (usize, usize) {
            (64, 1400)
        }
    }

    /// The chain, manual first: the range the user typed in the Manual /
    /// Custom field beats the profile's own range, and with no Manual range
    /// the profile's own range is used verbatim.
    #[test]
    fn port_range_precedence_is_manual_then_profile_udp_ports_then_ports() {
        let profile = ProfilePorts(27000, 27050);

        // 1. Manual field present: it wins over the profile's own range.
        let manual = build_config_for_game_with_port_range(
            &profile,
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
            Default::default(),
            Some((30000, 30010)),
        )
        .expect("a config is built even when the process is not running");
        assert_eq!(
            manual.port_range,
            (30000, 30010),
            "the Manual field must beat the profile's own range"
        );

        // 2. No Manual field: the profile's own range is used as-is.
        let profile_default = build_config_for_game_with_port_range(
            &profile,
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
            Default::default(),
            None,
        )
        .expect("a config is built even when the process is not running");
        assert_eq!(profile_default.port_range, (27000, 27050));

        // 3. ...and the untouched 5-arg entry point every CLI path uses
        //    reaches the same profile default by passing no override.
        let legacy = build_config_for_game(
            &profile,
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
            Default::default(),
        )
        .expect("a config is built even when the process is not running");
        assert_eq!(legacy.port_range, (27000, 27050));
    }

    /// A blank or malformed Manual field parses to `None` and the profile
    /// default stands: a typo cannot take the filter down with it.
    #[test]
    fn an_absent_range_override_keeps_the_profile_default() {
        let profile = ProfilePorts(28015, 28017);
        let cfg = build_config_for_game_with_port_range(
            &profile,
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
            Default::default(),
            None,
        )
        .expect("a config is built even when the process is not running");
        assert_eq!(cfg.port_range, (28015, 28017));
    }

    // ── Process target: authoritative, name-identified, never degraded ──

    /// A process target is carried verbatim and never degrades to the ports
    /// path: the config that leaves the builder is process-scoped, with the
    /// picked image as the target and the live routes as its seed.
    #[test]
    fn a_process_target_is_kept_and_does_not_degrade_to_port_only() {
        let profile = ProfilePorts(27000, 27050);
        let cfg = build_config_for_process_target(
            &profile,
            ProcessTarget::by_name("MyGame.exe"),
            vec![Route {
                local: "0.0.0.0:0".parse().unwrap(),
                remote: "203.0.113.7:27015".parse().unwrap(),
                proto: TransportProtocol::Udp,
            }],
            "127.0.0.1:4434".parse().unwrap(),
            false,
            4,
            Default::default(),
        );

        let target = cfg
            .process_target
            .as_ref()
            .expect("the config must keep the picked process");
        assert_eq!(target.name(), Some("MyGame.exe"));
        assert_eq!(
            cfg.process_names,
            vec!["MyGame.exe".to_string()],
            "the config targets the picked image, not the profile's own names"
        );
        assert_eq!(
            cfg.initial_routes.len(),
            1,
            "the target's live routes seed the config"
        );
        // The range is still carried for diagnostics, but the target - not the
        // range - is what decides the traffic; nothing here is port-only.
        assert_eq!(cfg.port_range, (27000, 27050));
    }

    /// The image name is the identity, so a relaunch under a new PID rebinds by
    /// name; a name that is not running reports nothing to bind, and a bare PID
    /// is the caller's assertion passed through untouched.
    #[test]
    fn a_process_target_survives_a_pid_change_by_name() {
        let picked = ProcessTarget {
            name: Some("MyGame.exe".to_string()),
            pid: Some(1111),
        };
        let relaunched = crate::interceptor::traits::ProcessInfo {
            pid: 2222,
            name: "MyGame.exe".to_string(),
            routes: vec![],
        };

        let rebound = resolve_process_target(&picked, Some(&relaunched))
            .expect("the name still matches a running process");
        assert_eq!(
            rebound.name(),
            Some("MyGame.exe"),
            "the name is the identity across a relaunch"
        );
        assert_eq!(
            rebound.pid,
            Some(2222),
            "the live PID replaces the stale one"
        );

        // A named target that is not in the live table does not bind: the
        // caller must report it, never fall back to the ports path.
        assert!(resolve_process_target(&picked, None).is_none());

        // A bare PID has no name to rebind on; it is passed through untouched.
        let by_pid = resolve_process_target(&ProcessTarget::by_pid(1111), None)
            .expect("a bare-PID target is the caller's own assertion");
        assert_eq!(by_pid.pid, Some(1111));
        assert_eq!(by_pid.name(), None);
    }

    // ── Mock interceptor integration tests ─────────────────────────

    #[test]
    fn mock_create_interceptor_lifecycle() {
        let interceptor = mock::MockInterceptor::new();
        assert_eq!(interceptor.platform_name(), "mock");
        assert!(interceptor.check_availability().is_ok());

        let config = InterceptorConfig {
            game_name: "Rust".into(),
            dynamic_server: false,
            transport: TransportProto::Udp,
            capture_tcp_ports: None,
            process_names: vec!["RustClient.exe".to_string()],
            process_target: Some(ProcessTarget::by_name("RustClient.exe")),
            port_range: (28015, 28017),
            initial_routes: vec![],
            proxy_addr: "127.0.0.1:4434".parse().unwrap(),
            fec_enabled: true,
            fec_k: 4,
            adaptive_fec: Default::default(),
            bypass: Default::default(),
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
                dynamic_server: false,
                transport: TransportProto::Udp,
                capture_tcp_ports: None,
                process_names: vec!["TestGame.exe".to_string()],
                process_target: None,
                port_range: (1, 2),
                initial_routes: vec![],
                proxy_addr: "127.0.0.1:4434".parse().unwrap(),
                fec_enabled: false,
                fec_k: 4,
                adaptive_fec: Default::default(),
                bypass: Default::default(),
            })
            .unwrap();

        let snap = handle.snapshot();
        assert_eq!(snap.packets_intercepted, 0);
        assert_eq!(snap.packets_from_proxy, 0);
        assert_eq!(snap.packets_injected, 0);
        assert_eq!(snap.errors, 0);
    }

    #[test]
    fn capture_policy_follows_the_process_target_and_the_port_range() {
        let base = InterceptorConfig {
            game_name: "Rust".into(),
            dynamic_server: false,
            transport: TransportProto::Udp,
            capture_tcp_ports: None,
            process_names: vec!["RustClient.exe".to_string()],
            process_target: None,
            port_range: (28015, 28017),
            initial_routes: vec![],
            proxy_addr: "127.0.0.1:4434".parse().unwrap(),
            fec_enabled: false,
            fec_k: 4,
            adaptive_fec: Default::default(),
            bypass: Default::default(),
        };

        // The built-in game path (and the ports path) keeps its range.
        assert_eq!(
            capture_policy(&base),
            CapturePolicy::PortRange {
                lo: 28015,
                hi: 28017
            }
        );

        // A picked process switches capture to the whole process.
        let picked = InterceptorConfig {
            process_target: Some(ProcessTarget::by_name("MyGame.exe")),
            ..base.clone()
        };
        assert_eq!(capture_policy(&picked), CapturePolicy::ProcessAll);

        // A PID-only target is still a target.
        let by_pid = InterceptorConfig {
            process_target: Some(ProcessTarget::by_pid(4242)),
            ..base.clone()
        };
        assert_eq!(capture_policy(&by_pid), CapturePolicy::ProcessAll);

        // An empty target names nothing: the range stands.
        let empty = InterceptorConfig {
            process_target: Some(ProcessTarget::default()),
            ..base
        };
        assert_eq!(
            capture_policy(&empty),
            CapturePolicy::PortRange {
                lo: 28015,
                hi: 28017
            }
        );
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
