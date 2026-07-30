//! `--start-interceptor` mode: live OOP TrafficInterceptor MITM.
//!
//! Discovers the game process, creates the best available interceptor
//! for the current OS, installs kernel-level redirect rules, and tunnels
//! game traffic through the LightSpeed proxy.
//!
//! ## Platform behavior
//!
//! | OS      | Backend        | Privileges needed          |
//! |---------|----------------|----------------------------|
//! | Linux   | nftables/iptables | root or CAP_NET_ADMIN    |
//! | macOS   | pfctl            | root                      |
//! | Windows | WinDivert        | Administrator             |
//!
//! ## Usage
//!
//! ```text
//! lightspeed --start-interceptor --game rust --proxy 1.2.3.4:4434
//! lightspeed --start-interceptor --game rust --proxy 1.2.3.4:4434 --server-addr 5.6.7.8:28015
//! ```

use std::net::{Ipv4Addr, SocketAddrV4};

use tracing::info;

use crate::interceptor::{build_config_for_game, create_interceptor, Route, TransportProtocol};

/// Start the OOP TrafficInterceptor for a game and run until interrupted.
///
/// This is the live MITM path — it installs real kernel redirect rules
/// and forwards intercepted traffic to the proxy. Press Ctrl+C to stop
/// and clean up.
pub async fn run_intercept_mode(
    game_key: &str,
    proxy_addr: SocketAddrV4,
    fec: bool,
    fec_k: u8,
    server_addr: Option<SocketAddrV4>,
) -> anyhow::Result<()> {
    // ── Resolve game profile ────────────────────────────────────────
    let game = crate::games::detect_game(game_key)?;
    let game_name = game.name().to_string();
    info!("🎮 Game: {} (ports: {:?})", game_name, game.ports());

    // ── Build interceptor config (runs ProcessScanner) ──────────────
    let mut config =
        build_config_for_game(game.as_ref(), proxy_addr, fec, fec_k).ok_or_else(|| {
            anyhow::anyhow!(
                "Failed to build interceptor config for '{}'. \
             Is the game running and connected to a server?",
                game_key
            )
        })?;

    // ── Server address override (for testing without a running game) ─
    if let Some(addr) = server_addr {
        config.initial_routes.push(Route {
            local: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0),
            remote: addr,
            proto: TransportProtocol::Udp,
        });
        info!("📍 Server override: {}", addr);
    }

    // ── Log discovered routes ───────────────────────────────────────
    let discovered = config.initial_routes.len();
    if discovered > 0 {
        info!("📍 Discovered {} server route(s):", discovered);
        for r in &config.initial_routes {
            info!("   {} → {}", r.local, r.remote);
        }
    } else {
        info!("📍 No server routes — interceptor will use port-range auto-detection");
        info!(
            "   Port range: {}-{}",
            config.port_range.0, config.port_range.1
        );
    }

    // ── Create and start the interceptor ────────────────────────────
    let interceptor = create_interceptor();
    let platform = interceptor.platform_name();
    info!("🔌 Interceptor backend: {}", platform);

    interceptor
        .check_availability()
        .map_err(|e| anyhow::anyhow!("{} interceptor unavailable: {}", platform, e))?;

    info!("⚡ Starting interceptor...");
    let mut handle = interceptor
        .start(config)
        .map_err(|e| anyhow::anyhow!("Failed to start {} interceptor: {}", platform, e))?;

    info!(
        "✅ Interceptor active — MITM-ing {} traffic via {}",
        game_name, platform
    );
    info!("   Proxy: {}", proxy_addr);
    info!("   Press Ctrl+C to stop");

    // ── Wait for Ctrl+C ─────────────────────────────────────────────
    tokio::signal::ctrl_c().await?;
    info!("🛑 Shutting down interceptor...");

    // ── Cleanup ─────────────────────────────────────────────────────
    handle.stop();
    info!("✅ Interceptor stopped — firewall rules removed");

    Ok(())
}
