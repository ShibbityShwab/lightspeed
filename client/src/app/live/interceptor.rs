//! `--start-interceptor` mode on the shared core (post-W2 task-57).
//!
//! The shipped flow is preserved line for line: resolve the game, build the
//! interceptor config (a live process scan seeds the server routes), print the
//! discovered routes / port range, print the backend, start, print the active
//! lines, wait for Ctrl+C, then stop and print the teardown outcome. Every
//! command now goes through the shared [`SessionCore`] / [`AppCore`], and every
//! user-visible line is a pure builder pinned byte-for-byte in the tests.
//!
//! `--server-addr` comes through `InterceptorRequest::with_server_override`, and
//! the resolved `--adaptive-fec` policy through `with_adaptive`, so the engine
//! applies exactly what the CLI resolved (the shipped engine derived
//! `enabled = fec`). The engine's own start line is suppressed
//! (`announce_start = false`) because the CLI prints its own activation lines.

use std::net::SocketAddrV4;
use std::time::Duration;

use tracing::{info, warn};

use super::LiveSession;
use crate::app::runtime::SessionCore;
use crate::engine::{InterceptorFacts, InterceptorRequest};

// ── The shipped lines, as pure builders (byte-for-byte parity) ──────────────

/// The game line: name and the profile's own port range (debug format).
pub fn game_line(game_name: &str, ports: (u16, u16)) -> String {
    format!("🎮 Game: {} (ports: {:?})", game_name, ports)
}

/// The path-MTU line: effective cap and the accepted clamp.
pub fn path_mtu_line(cap: usize) -> String {
    format!(
        "📏 Client-to-relay path-MTU cap: {} bytes (clamped into [{}, {}])",
        cap,
        crate::tunnel::pmtud::MIN_PATH_MTU,
        crate::tunnel::pmtud::HARD_CAP_PATH_MTU
    )
}

/// The known-server override line.
pub fn server_override_line(server: SocketAddrV4) -> String {
    format!("📍 Server override: {}", server)
}

/// The "discovered N routes" header.
pub fn discovered_header(count: usize) -> String {
    format!("📍 Discovered {} server route(s):", count)
}

/// One discovered route line (`local → remote`, as shipped).
pub fn route_line(local: SocketAddrV4, remote: SocketAddrV4) -> String {
    format!("   {} → {}", local, remote)
}

/// The no-routes fallback lines: auto-detection notice plus the port range.
pub fn no_routes_lines(port_range: (u16, u16)) -> Vec<String> {
    vec![
        "📍 No server routes, interceptor will use port-range auto-detection".to_string(),
        format!("   Port range: {}-{}", port_range.0, port_range.1),
    ]
}

/// The backend line.
pub fn backend_line(platform: &str) -> String {
    format!("🔌 Interceptor backend: {}", platform)
}

/// The pre-start line.
pub fn starting_line() -> String {
    "⚡ Starting interceptor...".to_string()
}

/// The activation lines: MITM notice, proxy and the Ctrl+C hint.
pub fn active_lines(game_name: &str, platform: &str, relay: SocketAddrV4) -> Vec<String> {
    vec![
        format!(
            "✅ Interceptor active, MITM-ing {} traffic via {}",
            game_name, platform
        ),
        format!("   Proxy: {}", relay),
        "   Press Ctrl+C to stop".to_string(),
    ]
}

/// The Ctrl+C shutdown line.
pub fn shutdown_line() -> String {
    "🛑 Shutting down interceptor...".to_string()
}

/// The successful teardown line.
pub fn teardown_ok_line() -> String {
    "✅ Interceptor stopped, firewall rules removed".to_string()
}

/// The teardown warning when the platform handles linger (same text as watch).
pub fn teardown_warning() -> String {
    super::watch::teardown_warning()
}

/// The backend-unavailable error the shipped mode returned.
pub fn unavailable_error(platform: &str, reason: &str) -> String {
    format!("{} interceptor unavailable: {}", platform, reason)
}

/// The start-failure error the shipped mode returned.
pub fn start_failed_error(platform: &str, reason: &str) -> String {
    format!("Failed to start {} interceptor: {}", platform, reason)
}

/// The interceptor request the mode builds for this invocation.
fn interceptor_request(session: &LiveSession) -> InterceptorRequest {
    InterceptorRequest::new(&session.game_key, session.relay, session.fec, session.fec_k)
        .with_adaptive(session.adaptive)
        .with_server_override(session.server_override)
        // The CLI prints its own activation lines; the engine must not add one.
        .with_announce_start(false)
}

/// Print the plan's discovery facts, in the shipped order.
fn print_plan_facts(facts: &InterceptorFacts) {
    info!("{}", path_mtu_line(facts.path_mtu_cap));
    if let Some(server) = facts.server_override {
        info!("{}", server_override_line(server));
    }
    if !facts.routes.is_empty() {
        info!("{}", discovered_header(facts.routes.len()));
        for route in &facts.routes {
            info!("{}", route_line(route.local, route.remote));
        }
    } else {
        for line in no_routes_lines(facts.port_range) {
            info!("{}", line);
        }
    }
    info!("{}", backend_line(facts.platform));
}

// ── The mode ────────────────────────────────────────────────────────────────

/// Run `--start-interceptor` until Ctrl+C: start the interceptor once, print the
/// shipped discovery/active lines, then stop it and report the teardown.
pub async fn run(core: &mut SessionCore, session: LiveSession) -> anyhow::Result<()> {
    // Shared session bootstrap (telemetry, registration, relay publication and
    // the parity-preserved rerouting loop) runs before any mode line, exactly as
    // the shipped dispatch registered first.
    super::start_session(core, &session).await?;

    let game = crate::games::resolve(&session.game_key)?;
    let game_name = game.name().to_string();
    info!("{}", game_line(&game_name, game.ports()));

    let plan = core
        .commands_mut()
        .plan_boost(interceptor_request(&session))
        .map_err(|error| {
            anyhow::anyhow!(
                "Failed to build interceptor config for '{}'",
                error.detail.unwrap_or_else(|| session.game_key.clone())
            )
        })?;

    let platform = plan.facts().platform.to_string();
    let available = plan.facts().available.clone();
    print_plan_facts(plan.facts());
    if let Err(reason) = available {
        return Err(anyhow::anyhow!("{}", unavailable_error(&platform, &reason)));
    }

    info!("{}", starting_line());
    core.commands_mut()
        .start_planned_boost(plan)
        .map_err(|error| {
            anyhow::anyhow!(
                "{}",
                start_failed_error(
                    &platform,
                    &error
                        .detail
                        .unwrap_or_else(|| "interceptor start failed".to_string())
                )
            )
        })?;

    for line in active_lines(&game_name, &platform, session.relay) {
        info!("{}", line);
    }

    tokio::signal::ctrl_c().await?;
    info!("{}", shutdown_line());

    let stopped = core
        .commands_mut()
        .stop_boost_keep_registration(Duration::from_secs(3))
        .unwrap_or(false);
    if stopped {
        info!("{}", teardown_ok_line());
    } else {
        warn!("{}", teardown_warning());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AdaptivePolicy;
    use std::net::{Ipv4Addr, SocketAddrV4};

    fn session() -> LiveSession {
        LiveSession {
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

    // ── Parity fixtures: the shipped `--start-interceptor` text ────────────

    #[test]
    fn the_discovery_lines_are_the_shipped_ones() {
        let local = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
        let remote = SocketAddrV4::new(Ipv4Addr::new(91, 199, 82, 166), 28015);
        assert_eq!(
            game_line("Rust", (28015, 28017)),
            "🎮 Game: Rust (ports: (28015, 28017))"
        );
        assert_eq!(
            path_mtu_line(1492),
            format!(
                "📏 Client-to-relay path-MTU cap: 1492 bytes (clamped into [{}, {}])",
                crate::tunnel::pmtud::MIN_PATH_MTU,
                crate::tunnel::pmtud::HARD_CAP_PATH_MTU
            )
        );
        assert_eq!(
            server_override_line(remote),
            "📍 Server override: 91.199.82.166:28015"
        );
        assert_eq!(discovered_header(1), "📍 Discovered 1 server route(s):");
        assert_eq!(
            route_line(local, remote),
            "   0.0.0.0:0 → 91.199.82.166:28015"
        );
        assert_eq!(
            no_routes_lines((28015, 28017)),
            vec![
                "📍 No server routes, interceptor will use port-range auto-detection".to_string(),
                "   Port range: 28015-28017".to_string(),
            ]
        );
    }

    #[test]
    fn the_start_and_teardown_lines_are_the_shipped_ones() {
        let relay = SocketAddrV4::new(Ipv4Addr::new(45, 32, 72, 1), 4434);
        assert_eq!(
            backend_line("WinDivert"),
            "🔌 Interceptor backend: WinDivert"
        );
        assert_eq!(starting_line(), "⚡ Starting interceptor...");
        assert_eq!(
            active_lines("Rust", "WinDivert", relay),
            vec![
                "✅ Interceptor active, MITM-ing Rust traffic via WinDivert".to_string(),
                "   Proxy: 45.32.72.1:4434".to_string(),
                "   Press Ctrl+C to stop".to_string(),
            ]
        );
        assert_eq!(shutdown_line(), "🛑 Shutting down interceptor...");
        assert_eq!(
            teardown_ok_line(),
            "✅ Interceptor stopped, firewall rules removed"
        );
        assert_eq!(
            teardown_warning(),
            "⚠️  Interceptor teardown did not complete within 3s; platform filter/handles may still be closing"
        );
        assert_eq!(
            unavailable_error("WinDivert", "access denied"),
            "WinDivert interceptor unavailable: access denied"
        );
        assert_eq!(
            start_failed_error("WinDivert", "access denied"),
            "Failed to start WinDivert interceptor: access denied"
        );
    }

    #[test]
    fn the_request_carries_the_policy_override_and_suppresses_the_engine_line() {
        let mut opts = session();
        let server = SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 7), 28015);
        opts.server_override = Some(server);
        let request = interceptor_request(&opts);
        assert_eq!(request.game_key, "rust");
        assert_eq!(request.relay, opts.relay);
        assert!(request.fec && request.fec_k == 6);
        assert_eq!(request.adaptive, opts.adaptive);
        assert_eq!(request.server_override, Some(server));
        assert_eq!(request.port_range_override, None);
        assert!(!request.announce_start);
    }
}
