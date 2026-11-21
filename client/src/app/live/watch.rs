//! `--watch` mode on the shared core (post-W2 task-57).
//!
//! The loop is the shipped one — wait for the game process, start the
//! interceptor, stop it when the game exits or Ctrl+C arrives, and keep
//! watching — but every command now goes through the shared [`SessionCore`] /
//! [`AppCore`], so the CLI and the GUI drive the same engine implementation.
//!
//! Every user-visible line is produced by a pure builder below and pinned
//! byte-for-byte in the tests, so the migration cannot drift the CLI text. The
//! mode owns its teardown warning (the engine's keep-registration stop is
//! silent) and suppresses the engine's canonical start line, because the shipped
//! CLI prints its own activation line.

use std::net::SocketAddrV4;
use std::time::Duration;

use tracing::{info, warn};

use crate::app::runtime::SessionCore;
use crate::engine::{AdaptivePolicy, InterceptorRequest};

use super::reroute;

/// Everything the `--watch` loop needs from the CLI invocation.
pub struct WatchOptions {
    /// Game profile key (`rust`, `cs2`, …).
    pub game_key: String,
    /// Data-plane relay chosen for the session.
    pub relay: SocketAddrV4,
    /// Relay candidates for the (parity-preserved) continuous rerouting loop.
    pub candidates: Vec<(String, String, String)>,
    /// Game server reported to the relay during registration, when known
    /// (`--game-server` / `--server-addr`).
    pub destination: Option<std::net::Ipv4Addr>,
    /// Known server seeded as an interceptor route (`--server-addr`).
    pub server_override: Option<SocketAddrV4>,
    /// Fixed-FEC opt-in.
    pub fec: bool,
    /// Fixed FEC block size.
    pub fec_k: u8,
    /// Adaptive-FEC policy as the CLI resolved it.
    pub adaptive: AdaptivePolicy,
    /// Whether the session reports anonymous telemetry.
    pub telemetry_enabled: bool,
    /// Relay data port (rerouting probe).
    pub data_port: u16,
    /// Relay control port (registration/rerouting).
    pub control_port: u16,
    /// Route strategy name (`nearest`, `destination-aware`, …).
    pub route_strategy: String,
    /// Multipath path count (1 = off).
    pub multipath_max_paths: u8,
    /// Effective client-to-relay path-MTU cap.
    pub path_mtu_cap: usize,
    /// Game server the rerouting loop ranks for (unspecified = none).
    pub game_server: SocketAddrV4,
}

// ── The shipped lines, as pure builders (byte-for-byte parity) ──────────────

/// The four-line watch banner.
pub fn banner_lines(game_name: &str, process_names: &[String], ports: (u16, u16)) -> Vec<String> {
    vec![
        format!("👀 Watching for {}...", game_name),
        format!("   Processes: {}", process_names.join(", ")),
        format!("   Ports:     {}-{}", ports.0, ports.1),
        "   Press Ctrl+C to stop\n".to_string(),
    ]
}

/// The game-detected line (PID and discovered route count).
pub fn detection_line(game_name: &str, pid: impl std::fmt::Display, routes: usize) -> String {
    format!(
        "🎮 {} detected! PID {} with {} routes",
        game_name, pid, routes
    )
}

/// The interceptor-active line.
pub fn active_line(game_name: &str) -> String {
    format!("✅ Interceptor active, optimizing {}\n", game_name)
}

/// The game-exited line.
pub fn exited_line(game_name: &str) -> String {
    format!("👋 {} exited, stopping interceptor\n", game_name)
}

/// The backend-unavailable line (the backend's own reason).
pub fn unavailable_line(reason: &str) -> String {
    format!("❌ Interceptor unavailable: {}", reason)
}

/// The interceptor-start-failed line.
pub fn start_failed_line(reason: &str) -> String {
    format!("❌ Interceptor start failed: {}\n", reason)
}

/// The Ctrl+C line while waiting for a game.
pub fn shutdown_line() -> String {
    "🛑 Shutdown requested, stopping".to_string()
}

/// The Ctrl+C line while intercepting.
pub fn shutdown_interceptor_line() -> String {
    "🛑 Shutdown requested, stopping interceptor...".to_string()
}

/// The teardown warning when the platform handles linger.
pub fn teardown_warning() -> String {
    "⚠️  Interceptor teardown did not complete within 3s; platform filter/handles may still be closing"
        .to_string()
}

/// The interceptor request the mode builds for one detected game session.
fn interceptor_request(options: &WatchOptions) -> InterceptorRequest {
    InterceptorRequest::new(&options.game_key, options.relay, options.fec, options.fec_k)
        .with_adaptive(options.adaptive)
        .with_server_override(options.server_override)
        // The CLI prints its own activation line; the engine must not add one.
        .with_announce_start(false)
}

// ── The loop ────────────────────────────────────────────────────────────────

/// Run `--watch` until Ctrl+C, polling for the game process and keeping the
/// interceptor active exactly while the game runs.
pub async fn run(core: &mut SessionCore, options: WatchOptions) -> anyhow::Result<()> {
    let game = crate::games::resolve(&options.game_key)?;
    let game_name = game.name().to_string();
    let process_names: Vec<String> = game.process_names().iter().map(|s| s.to_string()).collect();
    let process_refs: Vec<&str> = process_names.iter().map(|s| s.as_str()).collect();
    let (lo, hi) = game.ports();

    // Telemetry: the adopted session reports through the library collector, so
    // the samples the library data plane records are the ones actually sent.
    {
        let engine = core.engine();
        let mut engine = engine
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        engine.set_telemetry_enabled(options.telemetry_enabled);
        engine.set_telemetry_game(&options.game_key);
    }

    // Initial registration: the shipped path registered with the known game
    // server and published the relay unconditionally (even when registration
    // failed), because the interceptor still needs a current relay for its data
    // plane. The engine switch publishes only on success, so mirror the
    // unconditional publish explicitly.
    let switch = core
        .commands_mut()
        .switch_relay(options.relay, options.destination)
        .map_err(|error| {
            anyhow::anyhow!(
                "{}",
                error
                    .detail
                    .clone()
                    .unwrap_or_else(|| "relay registration failed".to_string())
            )
        })?;
    if switch.registered().await.is_none() {
        crate::session::set_current_proxy(options.relay);
    }

    // Continuous rerouting: same construction and first-tick behaviour as the
    // shipped caller (see `app::live::reroute` module notes).
    let _reroute = reroute::spawn(
        std::sync::Arc::clone(&core.engine()),
        options.candidates.clone(),
        options.data_port,
        options.control_port,
        options.game_server,
        options.route_strategy.clone(),
        options.multipath_max_paths,
        options.path_mtu_cap,
    );

    for line in banner_lines(&game_name, &process_names, (lo, hi)) {
        info!("{}", line);
    }

    // Registered once so every wait below can race the same signal future.
    let mut ctrl_c = Box::pin(tokio::signal::ctrl_c());

    enum State {
        Polling,
        Intercepting,
    }
    let mut state = State::Polling;
    let mut active = false;

    loop {
        match state {
            State::Polling => {
                match crate::interceptor::process_scanner::find_game_process(&process_refs) {
                    Some(process) => {
                        info!(
                            "{}",
                            detection_line(&game_name, process.pid, process.routes.len())
                        );

                        let plan = match core
                            .commands_mut()
                            .plan_boost(interceptor_request(&options))
                        {
                            Ok(plan) => plan,
                            Err(error) => {
                                let reason = error
                                    .detail
                                    .clone()
                                    .unwrap_or_else(|| "interceptor planning failed".to_string());
                                info!("{}", unavailable_line(&reason));
                                return Err(anyhow::anyhow!("{}", reason));
                            }
                        };
                        if let Err(reason) = plan.facts().available.clone() {
                            info!("{}", unavailable_line(&reason));
                            return Err(anyhow::anyhow!("{}", reason));
                        }

                        match core.commands_mut().start_planned_boost(plan) {
                            Ok(()) => {
                                info!("{}", active_line(&game_name));
                                active = true;
                                state = State::Intercepting;
                            }
                            Err(error) => {
                                let reason = error
                                    .detail
                                    .clone()
                                    .unwrap_or_else(|| "interceptor start failed".to_string());
                                info!("{}", start_failed_line(&reason));
                                tokio::select! {
                                    _ = &mut ctrl_c => {
                                        info!("{}", shutdown_line());
                                        return Ok(());
                                    }
                                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                                }
                            }
                        }
                    }
                    None => {
                        tokio::select! {
                            _ = &mut ctrl_c => {
                                info!("{}", shutdown_line());
                                return Ok(());
                            }
                            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                        }
                    }
                }
            }
            State::Intercepting => {
                tokio::select! {
                    _ = &mut ctrl_c => {
                        info!("{}", shutdown_interceptor_line());
                        stop_interceptor(core, &mut active);
                        return Ok(());
                    }
                    _ = tokio::time::sleep(Duration::from_secs(3)) => {}
                }

                if crate::interceptor::process_scanner::find_game_process(&process_refs).is_none() {
                    info!("{}", exited_line(&game_name));
                    stop_interceptor(core, &mut active);
                    state = State::Polling;
                }
            }
        }
    }
}

/// Stop the interceptor but keep the session registered (the mode keeps
/// watching), printing the shipped teardown warning when the platform lingers.
fn stop_interceptor(core: &mut SessionCore, active: &mut bool) {
    if !*active {
        return;
    }
    *active = false;
    let stopped = core
        .commands_mut()
        .stop_boost_keep_registration(Duration::from_secs(3))
        .unwrap_or(false);
    if !stopped {
        warn!("{}", teardown_warning());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn options() -> WatchOptions {
        WatchOptions {
            game_key: "rust".to_string(),
            relay: SocketAddrV4::new(Ipv4Addr::new(45, 32, 72, 1), 4434),
            candidates: vec![],
            destination: None,
            server_override: None,
            fec: true,
            fec_k: 6,
            adaptive: AdaptivePolicy {
                enabled: true,
                k_size: 6,
                max_overhead_pct: 12,
            },
            telemetry_enabled: false,
            data_port: 4434,
            control_port: 4433,
            route_strategy: "nearest".to_string(),
            multipath_max_paths: 1,
            path_mtu_cap: 1492,
            game_server: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
        }
    }

    // ── Parity fixtures: the shipped `--watch` text, byte-for-byte ─────────

    #[test]
    fn the_watch_banner_is_the_shipped_four_lines() {
        let names = vec!["RustClient.exe".to_string(), "Rust.exe".to_string()];
        assert_eq!(
            banner_lines("Rust", &names, (28015, 28017)),
            vec![
                "👀 Watching for Rust...".to_string(),
                "   Processes: RustClient.exe, Rust.exe".to_string(),
                "   Ports:     28015-28017".to_string(),
                "   Press Ctrl+C to stop\n".to_string(),
            ]
        );
    }

    #[test]
    fn the_watch_status_lines_are_the_shipped_ones() {
        assert_eq!(
            detection_line("Rust", 4242u32, 3),
            "🎮 Rust detected! PID 4242 with 3 routes"
        );
        assert_eq!(
            active_line("Rust"),
            "✅ Interceptor active, optimizing Rust\n"
        );
        assert_eq!(
            exited_line("Rust"),
            "👋 Rust exited, stopping interceptor\n"
        );
        assert_eq!(
            unavailable_line("WinDivert interceptor unavailable: access denied"),
            "❌ Interceptor unavailable: WinDivert interceptor unavailable: access denied"
        );
        assert_eq!(
            start_failed_line("Failed to start WinDivert interceptor: access denied"),
            "❌ Interceptor start failed: Failed to start WinDivert interceptor: access denied\n"
        );
        assert_eq!(shutdown_line(), "🛑 Shutdown requested, stopping");
        assert_eq!(
            shutdown_interceptor_line(),
            "🛑 Shutdown requested, stopping interceptor..."
        );
        assert_eq!(
            teardown_warning(),
            "⚠️  Interceptor teardown did not complete within 3s; platform filter/handles may still be closing"
        );
    }

    #[test]
    fn the_request_carries_the_resolved_policy_and_suppresses_the_engine_line() {
        let mut opts = options();
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 7), 28015);
        opts.server_override = Some(server);
        let request = interceptor_request(&opts);
        assert_eq!(request.game_key, "rust");
        assert_eq!(request.relay, opts.relay);
        assert!(request.fec);
        assert_eq!(request.fec_k, 6);
        assert_eq!(
            request.adaptive, opts.adaptive,
            "the caller's adaptive policy must pass through verbatim"
        );
        assert_eq!(request.server_override, Some(server));
        assert_eq!(
            request.port_range_override, None,
            "watch keeps the profile's own port range (no manual override)"
        );
        assert!(
            !request.announce_start,
            "the CLI prints its own activation line; the engine must not add one"
        );
    }
}
