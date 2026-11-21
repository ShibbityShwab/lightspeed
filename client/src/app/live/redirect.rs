//! `--game-server` redirect mode on the shared core (post-W2 task-57).
//!
//! The shipped flow is preserved line for line: the mode banner (and optional
//! game facts), server/local-port/relay lines, the FEC line, then the shipped
//! redirect runtime lines (bind, tunnel, Ctrl+C hint, configure hint, periodic
//! stats, shutdown line and the final-stats block). Registration goes through the
//! shared session (destination-aware), the redirect itself through
//! [`AppCore::start_redirect_with_options`], and the per-packet stats through the
//! engine's [`crate::RedirectCounters`] when the mode reports.
//!
//! The redirect runtime prints its own bind/tunnel/stats lines (same code the GUI
//! uses); the adopted mode only pins the lines it owns and the shutdown text the
//! runtime must print (`⚡ Shutdown signal received`, the shipped CLI wording).

use std::net::SocketAddrV4;
use std::time::{Duration, Instant};

use tracing::{info, warn};

use super::LiveSession;
use crate::app::runtime::SessionCore;

/// The shutdown line the shipped CLI redirect prints (the engine runtime accepts
/// the caller's wording so the migration stays byte-identical).
pub const SHUTDOWN_LINE: &str = "⚡ Shutdown signal received";

/// One `--game-server` redirect invocation resolved by the CLI dispatch.
pub struct RedirectRun {
    /// The adopted live session (destination = the game server address).
    pub session: LiveSession,
    /// The real game server the redirect targets.
    pub game_server: SocketAddrV4,
    /// Local UDP port the game should connect to.
    pub local_port: u16,
    /// Whether the TCP client↔relay leg is enabled (`--tcp`).
    pub tcp: bool,
    /// Optional game banner facts: display name, anti-cheat, typical PPS.
    pub game_banner: Option<(String, String, u32)>,
}

// ── The shipped lines the CLI owns, as pure builders (byte-for-byte) ────────

/// The mode banner.
pub fn starting_line() -> String {
    "🚀 Starting redirect mode".to_string()
}

/// The optional game facts lines (name, anti-cheat, typical packets/sec).
pub fn game_lines(name: &str, anti_cheat: &str, typical_pps: u32) -> Vec<String> {
    vec![
        format!("   Game:        {} (anti-cheat: {})", name, anti_cheat),
        format!("   Typical PPS: ~{} packets/sec", typical_pps),
    ]
}

/// The game-server line.
pub fn server_line(server: SocketAddrV4) -> String {
    format!("   Game server: {}", server)
}

/// The local-port line.
pub fn local_port_line(local_port: u16) -> String {
    format!("   Local port:  127.0.0.1:{}", local_port)
}

/// The relay line.
pub fn proxy_line(proxy: SocketAddrV4) -> String {
    format!("   Proxy:       {}", proxy)
}

/// The FEC line: adaptive when the policy is on, else fixed when `--fec` is set.
pub fn fec_line(adaptive: crate::engine::AdaptivePolicy, fec: bool, fec_k: u8) -> Option<String> {
    if adaptive.enabled {
        let config = crate::tunnel::adaptive::AdaptiveConfig {
            enabled: adaptive.enabled,
            k_size: adaptive.k_size,
            max_overhead_pct: adaptive.max_overhead_pct,
            ..Default::default()
        };
        Some(format!(
            "   FEC:         adaptive (K={}, ceiling {}%)",
            config.effective_k(),
            config.overhead_pct()
        ))
    } else if fec {
        let overhead = 100 / u32::from(fec_k.clamp(1, u8::MAX));
        Some(format!(
            "   FEC:         enabled (K={}, ~{}% overhead)",
            fec_k, overhead
        ))
    } else {
        None
    }
}

// ── The mode ────────────────────────────────────────────────────────────────

/// Run the redirect until Ctrl+C, then wait for the runtime's final stats.
pub async fn run(core: &mut SessionCore, spec: RedirectRun) -> anyhow::Result<()> {
    // Shared session bootstrap: telemetry, destination-aware registration, relay
    // publication and the parity-preserved rerouting loop.
    super::start_session(core, &spec.session).await?;

    info!("{}", starting_line());
    if let Some((name, anti_cheat, typical_pps)) = &spec.game_banner {
        for line in game_lines(name, anti_cheat, *typical_pps) {
            info!("{}", line);
        }
    }
    info!("{}", server_line(spec.game_server));
    info!("{}", local_port_line(spec.local_port));
    info!("{}", proxy_line(spec.session.relay));
    if let Some(line) = fec_line(spec.session.adaptive, spec.session.fec, spec.session.fec_k) {
        info!("{}", line);
    }

    let game_name = spec
        .game_banner
        .as_ref()
        .map(|(name, _, _)| name.clone())
        .unwrap_or_else(|| spec.session.game_key.clone());
    let options = crate::engine::RedirectOptions {
        fec: spec.session.fec,
        fec_k: spec.session.fec_k,
        adaptive: spec.session.adaptive,
        tcp: spec.tcp,
        shutdown_notice: Some(SHUTDOWN_LINE),
    };
    core.commands_mut()
        .start_redirect_with_options(
            spec.game_server,
            spec.local_port,
            &game_name,
            spec.session.relay,
            options,
        )
        .map_err(|error| {
            anyhow::anyhow!(
                "{}",
                error
                    .detail
                    .unwrap_or_else(|| "redirect start failed".to_string())
            )
        })?;

    // Wait for Ctrl+C; a runtime failure ends the shipped mode as an error, so
    // surface it the same way instead of waiting for a signal that will not come.
    let mut ctrl_c = Box::pin(tokio::signal::ctrl_c());
    loop {
        if let Some(error) = core.status().redirect_error {
            return Err(anyhow::anyhow!("{error}"));
        }
        tokio::select! {
            _ = &mut ctrl_c => break,
            _ = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    }

    core.commands_mut().stop_boost().map_err(|error| {
        anyhow::anyhow!(
            "{}",
            error
                .detail
                .unwrap_or_else(|| "redirect stop failed".to_string())
        )
    })?;
    // The redirect task prints its shutdown line and final stats before clearing
    // `redirect_active`; wait for that so the process does not exit first.
    let deadline = Instant::now() + Duration::from_secs(3);
    while core.status().redirect_active && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    if core.status().redirect_active {
        warn!("redirect teardown did not complete within 3s");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::AdaptivePolicy;
    use std::net::{Ipv4Addr, SocketAddrV4};

    #[test]
    fn the_mode_banner_lines_are_the_shipped_ones() {
        let server = SocketAddrV4::new(Ipv4Addr::new(91, 199, 82, 166), 28015);
        let relay = SocketAddrV4::new(Ipv4Addr::new(45, 32, 72, 1), 4434);
        assert_eq!(starting_line(), "🚀 Starting redirect mode");
        assert_eq!(
            game_lines("Rust", "EAC", 30),
            vec![
                "   Game:        Rust (anti-cheat: EAC)".to_string(),
                "   Typical PPS: ~30 packets/sec".to_string(),
            ]
        );
        assert_eq!(server_line(server), "   Game server: 91.199.82.166:28015");
        assert_eq!(local_port_line(28016), "   Local port:  127.0.0.1:28016");
        assert_eq!(proxy_line(relay), "   Proxy:       45.32.72.1:4434");
        assert_eq!(SHUTDOWN_LINE, "⚡ Shutdown signal received");
    }

    #[test]
    fn the_fec_line_matches_the_shipped_branches() {
        let policy = AdaptivePolicy {
            enabled: true,
            k_size: 6,
            max_overhead_pct: 12,
        };
        let config = crate::tunnel::adaptive::AdaptiveConfig {
            enabled: true,
            k_size: 6,
            max_overhead_pct: 12,
            ..Default::default()
        };
        assert_eq!(
            fec_line(policy, true, 4),
            Some(format!(
                "   FEC:         adaptive (K={}, ceiling {}%)",
                config.effective_k(),
                config.overhead_pct()
            ))
        );
        assert_eq!(
            fec_line(AdaptivePolicy::default(), true, 4),
            Some("   FEC:         enabled (K=4, ~25% overhead)".to_string())
        );
        assert_eq!(fec_line(AdaptivePolicy::default(), false, 4), None);
    }
}
