//! # LightSpeed Client
//!
//! Zero-cost global network optimizer for multiplayer games.
//! Captures game UDP packets and tunnels them through optimally-selected
//! proxy nodes to reduce latency via better routing paths.

// Modules marked #[allow(dead_code)] contain scaffolded interfaces
// for future integration (game capture pipeline, ML routing, etc.)
#[allow(dead_code)]
mod capture;
mod cli;
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod error;
#[allow(dead_code)]
mod games;
#[allow(dead_code)]
mod ml;
mod modes;
#[allow(dead_code)]
mod quic;
#[allow(dead_code)]
mod redirect;
#[allow(dead_code)]
mod route;
#[allow(dead_code)]
mod telemetry;
#[allow(dead_code)]
mod tunnel;
#[allow(dead_code)]
mod warp;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;

use clap::Parser;
use tracing::{info, warn};

use telemetry::TelemetryCollector;

use cli::{parse_proxy_addr, Cli};
use modes::{
    capture_mode::run_capture_mode,
    control_test::run_control_test,
    keepalive::run_keepalive_mode,
    live_test::run_live_test,
    proxy_probe::{probe_all_proxies, select_best_proxy},
    tunnel_test::run_tunnel_test,
};
use route::ProxyHealth;
use tunnel::relay::UdpRelay;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // ── Tracing ───────────────────────────────────────────────────
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();

    info!("⚡ LightSpeed v{} starting", env!("CARGO_PKG_VERSION"));

    // ── Telemetry (opt-in) ────────────────────────────────────────
    //
    // --telemetry enables anonymous aggregated RTT/FEC stats reporting.
    // No PII is ever collected. See docs/privacy.md for full details.
    let telemetry_collector: Option<Arc<TelemetryCollector>> = if cli.telemetry && !cli.no_telemetry
    {
        telemetry::print_disclosure();
        Some(Arc::new(TelemetryCollector::new()))
    } else {
        None
    };

    // ── Configuration ─────────────────────────────────────────────
    let config = config::Config::load(&cli.config).unwrap_or_else(|e| {
        warn!("Config not found ({}), using defaults", e);
        config::Config::default()
    });

    // ── Game detection ────────────────────────────────────────────
    let game: Option<Box<dyn games::GameConfig>> = match cli.game.as_deref() {
        Some(name) => {
            info!("Game selected: {}", name);
            Some(games::detect_game(name)?)
        }
        None => {
            if cli.game_server.is_some() {
                info!("Redirect mode — game detection skipped");
                None
            } else {
                info!("Auto-detecting running game...");
                match games::auto_detect() {
                    Ok(g) => Some(g),
                    Err(e) => {
                        warn!("{}", e);
                        None
                    }
                }
            }
        }
    };

    if let Some(ref game) = game {
        info!(
            "🎮 Targeting game: {} (ports: {:?})",
            game.name(),
            game.ports()
        );
    }

    // ── --list-interfaces ─────────────────────────────────────────
    if cli.list_interfaces {
        info!("🔌 Available network interfaces:");
        let interfaces = capture::list_interfaces();
        if interfaces.is_empty() {
            info!("   (none found — pcap-capture feature may not be enabled)");
            info!("   Rebuild with: cargo build --features pcap-capture");
        } else {
            for iface in &interfaces {
                let status = if iface.is_up { "UP" } else { "DOWN" };
                let kind = if iface.is_loopback { " (loopback)" } else { "" };
                info!(
                    "   • {} [{}]{} — {}",
                    iface.name, status, kind, iface.description
                );
            }
        }
        return Ok(());
    }

    // ── --dry-run ─────────────────────────────────────────────────
    if cli.dry_run {
        info!("Dry run mode — showing configuration and exiting");
        info!("Config: {:?}", config);
        if let Some(ref game) = game {
            info!("Game: {}", game.name());
        }
        return Ok(());
    }

    // ── Cloudflare WARP integration ───────────────────────────────
    //
    // WARP routes traffic through Cloudflare's NTT backbone, bypassing
    // ISP routing inefficiencies. Free 5-10 ms improvement on most paths.
    let mut warp_manager = warp::WarpManager::new();

    // --warp-status: show WARP info and exit
    if cli.warp_status {
        if !warp_manager.is_installed() {
            info!("🌐 Cloudflare WARP: Not installed");
            info!("   Install: {}", warp::install_instructions());
        } else {
            let warp_info = warp_manager.info();
            info!("🌐 Cloudflare WARP Status");
            info!("   Status:   {}", warp_info.status);
            info!(
                "   Protocol: {}",
                warp_info.protocol.unwrap_or_else(|| "unknown".into())
            );
            info!(
                "   Mode:     {}",
                warp_info.mode.unwrap_or_else(|| "unknown".into())
            );
            let proxy_ips = vec![
                Ipv4Addr::new(149, 28, 84, 139), // Vultr LA
                Ipv4Addr::new(149, 28, 144, 74), // Vultr SGP
            ];
            warp_manager.print_summary(&proxy_ips);
            if let Some(stats) = warp_manager.tunnel_stats() {
                info!("   Tunnel stats:\n{}", stats);
            }
        }
        return Ok(());
    }

    if cli.warp && !cli.no_warp {
        if !warp_manager.is_installed() {
            warn!("🌐 WARP requested but not installed!");
            warn!("   Install Cloudflare WARP for 5-10ms latency improvement:");
            warn!("   {}", warp::install_instructions());
            warn!("   Continuing without WARP...");
        } else {
            match warp_manager.connect() {
                Ok(()) => {
                    info!("🌐 WARP enabled — traffic routed through Cloudflare NTT backbone");
                }
                Err(e) => {
                    warn!("🌐 WARP connection failed: {}", e);
                    warn!("   Continuing without WARP...");
                }
            }
        }
    } else if !cli.no_warp {
        match warp_manager.status() {
            warp::WarpStatus::Connected => {
                info!("🌐 WARP detected and connected — traffic uses NTT backbone");
            }
            warp::WarpStatus::Disconnected => {
                info!("🌐 WARP installed but disconnected. Use --warp to enable (saves 5-10ms)");
            }
            warp::WarpStatus::NotInstalled => {}
            _ => {}
        }
    }

    // ── Proxy selection ───────────────────────────────────────────
    //
    // Priority:
    // 1. --proxy flag (explicit, highest priority)
    // 2. config.proxy.servers + RouteSelector (probe & pick best)
    // 3. Default localhost for development
    let proxy_addr = if let Some(ref proxy_str) = cli.proxy {
        let addr = parse_proxy_addr(proxy_str)?;
        info!("🌐 Proxy (explicit): {}", addr);
        addr
    } else if !config.proxy.servers.is_empty() {
        let strategy = cli
            .route_strategy
            .as_deref()
            .unwrap_or(config.route.strategy.as_str());
        info!(
            "🔍 Probing {} configured proxies (strategy: {})...",
            config.proxy.servers.len(),
            strategy
        );
        let game_server_addr = cli
            .game_server
            .as_ref()
            .and_then(|s| parse_proxy_addr(s).ok())
            .unwrap_or_else(|| SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0));

        let selected = select_best_proxy(
            &config.proxy.servers,
            config.proxy.data_port,
            game_server_addr,
            strategy,
        )
        .await?;

        info!(
            "🌐 Proxy (auto-selected): {} [{}] — {:.1}ms latency, strategy: {:?}",
            selected.primary.data_addr,
            selected.primary.id,
            selected.primary.latency_us.unwrap_or(0) as f64 / 1000.0,
            selected.strategy,
        );
        if !selected.backups.is_empty() {
            info!(
                "   Backups: {}",
                selected
                    .backups
                    .iter()
                    .map(|p| format!(
                        "{} ({:.1}ms)",
                        p.id,
                        p.latency_us.unwrap_or(0) as f64 / 1000.0
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        selected.primary.data_addr
    } else {
        let default = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 4434);
        warn!("No proxy specified, using default: {}", default);
        default
    };

    // ── --probe-proxies ───────────────────────────────────────────
    if cli.probe_proxies {
        if config.proxy.servers.is_empty() {
            warn!("No proxies configured in config file");
        } else {
            info!("🔍 Probing all configured proxies...");
            let probes = probe_all_proxies(&config.proxy.servers, config.proxy.data_port).await;
            info!("📊 Proxy Latency Report:");
            for node in &probes {
                let status = match node.health {
                    ProxyHealth::Healthy => "✅",
                    ProxyHealth::Degraded => "⚠️",
                    ProxyHealth::Unhealthy => "❌",
                    ProxyHealth::Unknown => "❓",
                };
                let latency = node
                    .latency_us
                    .map(|us| format!("{:.1}ms", us as f64 / 1000.0))
                    .unwrap_or_else(|| "timeout".into());
                info!(
                    "   {} {} ({}) — {}",
                    status, node.id, node.data_addr, latency
                );
            }
        }
        return Ok(());
    }

    // ── --live-test ───────────────────────────────────────────────
    if cli.live_test {
        let echo_server = cli
            .echo_server
            .as_ref()
            .and_then(|s| parse_proxy_addr(s).ok());
        return run_live_test(&config, Some(proxy_addr), echo_server, cli.fec, cli.fec_k).await;
    }

    // ── Relay socket (shared by tunnel/control tests + keepalive) ─
    let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    let mut relay = UdpRelay::new(bind_addr);
    relay.bind().await?;

    // ── --test-tunnel ─────────────────────────────────────────────
    if cli.test_tunnel {
        return run_tunnel_test(relay, proxy_addr).await;
    }

    // ── --test-control ────────────────────────────────────────────
    if cli.test_control {
        return run_control_test(proxy_addr, &config).await;
    }

    // ── Online learner ────────────────────────────────────────────
    //
    // Collects live RTT data from keepalive probes and retrains the
    // route model when enough new data accumulates.
    let (proxy_id, proxy_region) = match proxy_addr.ip().octets() {
        [149, 28, 84, 139] => ("proxy-lax".to_string(), "us-west-lax".to_string()),
        [149, 28, 144, 74] => ("relay-sgp".to_string(), "asia-sgp".to_string()),
        _ => (format!("proxy-{}", proxy_addr.ip()), "unknown".to_string()),
    };

    let online_learner = {
        let mut learner = ml::online::OnlineLearner::new();
        match learner.initialize() {
            Ok(()) => {
                let summary = learner.summary();
                info!(
                    "🧠 Online learning initialized: {} previous measurements, model: {}",
                    summary.total_measurements, summary.model_version,
                );
            }
            Err(e) => warn!("🧠 Online learning init failed (non-fatal): {}", e),
        }
        Arc::new(tokio::sync::Mutex::new(learner))
    };

    let keepalive_timestamps: Arc<tokio::sync::Mutex<HashMap<u16, std::time::Instant>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    // ── --game-server: redirect mode ──────────────────────────────
    //
    // Game → localhost:port → LightSpeed → Proxy → Game Server
    if let Some(ref server_str) = cli.game_server {
        let game_server_addr = parse_proxy_addr(server_str)?;
        let local_port = cli.local_port.unwrap_or_else(|| {
            if let Some(ref game) = game {
                game.redirect_port()
            } else {
                game_server_addr.port()
            }
        });

        info!("🚀 Starting redirect mode");
        if let Some(ref game) = game {
            info!(
                "   Game:        {} (anti-cheat: {})",
                game.name(),
                game.anti_cheat()
            );
            info!("   Typical PPS: ~{} packets/sec", game.typical_pps());
        }
        info!("   Game server: {}", game_server_addr);
        info!("   Local port:  127.0.0.1:{}", local_port);
        info!("   Proxy:       {}", proxy_addr);

        let mut redirect_proxy =
            redirect::UdpRedirect::new(local_port, game_server_addr, proxy_addr);
        if cli.fec {
            info!(
                "   FEC:         enabled (K={}, ~{}% overhead)",
                cli.fec_k,
                100 / cli.fec_k as u32
            );
            redirect_proxy = redirect_proxy.with_fec(cli.fec_k);
        }
        return redirect_proxy.run().await;
    }

    // ── --capture: pcap mode ──────────────────────────────────────
    if cli.capture {
        let game_ref = game.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "Capture mode requires a game. Use --game <name> or ensure a game is running."
            )
        })?;
        return run_capture_mode(
            game_ref.as_ref(),
            proxy_addr,
            proxy_id,
            proxy_region,
            online_learner,
            keepalive_timestamps,
            cli.fec,
            cli.fec_k,
            cli.interface,
        )
        .await;
    }

    // ── Game setup instructions ───────────────────────────────────
    if let Some(ref game) = game {
        info!("📋 {} setup instructions:", game.name());
        for line in game.redirect_instructions().lines() {
            info!("   {}", line);
        }
        info!("");
        info!(
            "   Example: lightspeed --game {} --game-server <SERVER_IP>:{} --proxy {}",
            cli.game.as_deref().unwrap_or("unknown"),
            game.redirect_port(),
            proxy_addr,
        );
        info!("");
    }

    // ── Spawn periodic telemetry flush (every 15 min) ─────────────
    if let Some(ref tc) = telemetry_collector {
        let proxy_host = format!("{}:{}", proxy_addr.ip(), 8080);
        // TelemetryCollector is Arc-backed; .clone() shares the same ring buffer.
        telemetry::spawn_periodic_flush(tc.as_ref().clone(), proxy_host, 0, "".to_string());
    }

    // ── Keepalive mode ────────────────────────────────────────────
    run_keepalive_mode(
        relay,
        proxy_addr,
        proxy_id,
        proxy_region,
        online_learner,
        keepalive_timestamps,
        telemetry_collector,
    )
    .await
}
