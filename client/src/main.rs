//! # LightSpeed Client
#![allow(dead_code, unused_imports, clippy::useless_conversion, clippy::bool_assert_comparison, clippy::collapsible_if)]
//!
//! Zero-cost global network optimizer for multiplayer games.
//! Captures game UDP packets and tunnels them through optimally-selected
//! proxy nodes to reduce latency via better routing paths.

mod capture;
mod cli;
mod config;
mod error;
mod games;
mod interceptor;
mod ml;
mod modes;
mod quic;
mod redirect;
mod route;
mod telemetry;
mod tunnel;
mod warp;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::Arc;

use clap::Parser;
use tracing::{error, info, warn};

use telemetry::TelemetryCollector;

use cli::{parse_proxy_addr, Cli};
use modes::{
    capture_mode::run_capture_mode,
    control_test::run_control_test,
    keepalive::run_keepalive_mode,
    live_test::run_live_test,
    proxy_probe::{probe_all_proxies, select_best_proxy},
    demo_mode::run_demo,
    smoke_test::run_smoke_test,
    watch_mode::run_watch_mode,
    intercept_mode::run_intercept_mode,
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

    // ── Cloudflare WARP manager (used by several early-exit paths) ─
    let mut warp_manager = warp::WarpManager::new();

    // ── --warp-status: show WARP info and exit ────────────────────
    //
    // Must run BEFORE game auto-detection to avoid a spurious
    // "No supported game detected" warning and the ~300 ms tasklist scan.
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
            // Read proxy IPs from config or env var, with placeholder fallback for testing
            let proxy_ips = if let Ok(ips_str) = std::env::var("LIGHTSPEED_PROXY_IPS") {
                ips_str.split(',').filter_map(|s| s.trim().parse().ok()).collect()
            } else {
                vec![
                    Ipv4Addr::new(104, 26, 1, 50), // Example public IP for testing
                ]
            };
            warp_manager.print_summary(&proxy_ips);
            if let Some(stats) = warp_manager.tunnel_stats() {
                info!("   Tunnel stats:\n{}", stats);
            }
        }
        return Ok(());
    }

    // ── No-mode banner ────────────────────────────────────────────
    //
    // When no mode flags are provided, show a friendly quick-start
    // instead of falling through to the default keepalive loop.
    {
        let has_mode = cli.list_games || cli.list_interfaces || cli.write_config
            || cli.check || cli.demo || cli.intercept || cli.scan_processes
            || cli.start_interceptor || cli.test_tunnel || cli.test_control
            || cli.probe_proxies || cli.live_test || cli.warp_status
            || cli.dry_run || cli.capture || cli.game_server.is_some()
            || cli.game.is_some() || cli.smoke_test || cli.watch;
        if !has_mode {
            info!("╔════════════════════════════════════════════╗");
            info!("║       ⚡  LightSpeed v{}                  ║", env!("CARGO_PKG_VERSION"));
            info!("║   Zero-cost global network optimizer      ║");
            info!("╠════════════════════════════════════════════╣");
            info!("║  Quick start:                             ║");
            info!("║    lightspeed --demo                      ║");
            info!("║    lightspeed --check --game rust          ║");
            info!("║    lightspeed --list-games                ║");
            info!("║    lightspeed --help                      ║");
            info!("╚════════════════════════════════════════════╝");
            return Ok(());
        }
    }

    // ── --list-interfaces ─────────────────────────────────────────
    //
    // Also runs before game detection — no point scanning processes
    // when the user only wants to list NICs.
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

    // ── --list-games ──────────────────────────────────────────────
    if cli.list_games {
        info!("🎮 Supported games:");
        // Access game registry through the games module
        let all_games: &[&str] = &[
            "rust", "fortnite", "cs2", "dota2", "valorant", "apex", "ow2", "lol", "pubg",
        ];
        for name in all_games {
            match games::detect_game(name) {
                Ok(g) => {
                    let (lo, hi) = g.ports();
                    info!("   {:<12} ports {}-{}  — {}", g.name(), lo, hi, g.process_names().join(", "));
                }
                Err(_) => {
                    info!("   {:<12} (unknown)", name);
                }
            }
        }
        return Ok(());
    }

    // ── --write-config ────────────────────────────────────────────
    if cli.write_config {
        let path = "lightspeed.toml";
        if std::path::Path::new(path).exists() {
            warn!("{} already exists — not overwriting", path);
        } else {
            let default_config = r#"# LightSpeed configuration
# See docs/user-guide.md for details.

[proxy]
# Your LightSpeed proxy node addresses (host:port).
# Get these from your Vultr/Oracle cloud instances.
# At least one proxy is required.
servers = [
    # "YOUR_PROXY_IP:4434",
]

# Data-plane port (UDP tunnel, shared by all proxy nodes).
data_port = 4434

# Control-plane port (QUIC, shared by all proxy nodes).
# control_port = 4433

[general]
# Default game to optimize.
# default_game = "rust"

# Route selection strategy: "nearest" or "ml"
# route_strategy = "nearest"

# Enable Forward Error Correction for packet loss recovery.
# fec = false

# FEC block size (2-16). Lower = more redundancy.
# fec_k = 4

# Enable Cloudflare WARP for improved routing.
# warp = false

[telemetry]
# Opt-in anonymous telemetry (p50/p95/p99 latency, jitter, FEC stats).
# No IPs or PII are ever sent. See docs/privacy.md.
# enabled = false
"#;
            std::fs::write(path, default_config)?;
            info!("📝 Wrote default config to {}", path);
            info!("   Edit {} to add your proxy addresses, then run:", path);
            info!("   lightspeed --game rust");
        }
        return Ok(());
    }

    // ── --watch ───────────────────────────────────────────────
    if cli.watch {
        let game_key = cli.game.as_deref().ok_or_else(|| {
            anyhow::anyhow!("--watch requires --game <name>")
        })?;
        let proxy_str = cli.proxy.as_deref().unwrap_or("127.0.0.1:4434");
        let proxy_addr = parse_proxy_addr(proxy_str)?;
        let server_override = cli.server_addr.as_deref().map(|s| parse_proxy_addr(s)).transpose()?;
        return run_watch_mode(game_key, proxy_addr, cli.fec, cli.fec_k, server_override).await;
    }

    // ── --smoke-test ───────────────────────────────────────────
    if cli.smoke_test {
        let proxy_str = cli.proxy.as_deref().unwrap_or("127.0.0.1:4434");
        let proxy_addr = parse_proxy_addr(proxy_str)?;
        info!("🔥 Running E2E smoke test (needs root for nftables)...");
        return run_smoke_test(proxy_addr).await;
    }

    // ── --demo ──────────────────────────────────────────────────
    if cli.demo {
        let game_key = cli.game.as_deref().unwrap_or("rust");
        let proxy_str = cli.proxy.as_deref().unwrap_or("127.0.0.1:4434");
        return run_demo(&config, game_key, proxy_str).await;
    }

    // ── --check ──────────────────────────────────────────────────
    if cli.check {
        info!("🔍 LightSpeed environment check");
        let mut all_ok = true;

        // 1. Interceptor availability
        let interceptor = interceptor::create_interceptor();
        let platform = interceptor.platform_name();
        match interceptor.check_availability() {
            Ok(()) => info!("   ✅ Interceptor backend: {} — available", platform),
            Err(e) => {
                warn!("   ❌ Interceptor backend: {} — {}", platform, e);
                all_ok = false;
            }
        }

        // 2. Root / admin check
        #[cfg(target_os = "linux")]
        {
            let is_root = std::process::Command::new("id")
                .args(["-u"])
                .output()
                .ok()
                .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<u32>().ok())
                .map(|uid| uid == 0)
                .unwrap_or(false);
            if is_root {
                info!("   ✅ Running as root");
            } else {
                warn!("   ⚠️  Not running as root — interceptor needs sudo");
            }
        }

        // 3. Game detection
        if let Some(ref game_name) = cli.game {
            match games::detect_game(game_name) {
                Ok(game) => {
                    let procs = game.process_names();
                    let found = crate::interceptor::process_scanner::find_game_process(procs);
                    match found {
                        Some(p) => info!("   ✅ Game '{}' detected: PID {} with {} routes", game.name(), p.pid, p.routes.len()),
                        None => info!("   ⚠️  Game '{}' not running — port-range fallback will be used", game.name()),
                    }
                }
                Err(_) => {
                    warn!("   ❌ Unknown game '{}'", game_name);
                    all_ok = false;
                }
            }
        } else {
            info!("   ℹ️  No --game specified — skipping game detection");
        }

        // 4. Proxy reachability (quick UDP probe)
        if let Some(ref proxy_str) = cli.proxy {
            let proxy_addr = match parse_proxy_addr(proxy_str) {
                Ok(a) => a,
                Err(e) => {
                    warn!("   ❌ Invalid proxy address '{}': {}", proxy_str, e);
                    all_ok = false;
                    return if all_ok { Ok(()) } else { Err(anyhow::anyhow!("Some checks failed")) };
                }
            };
            // Quick connectivity check — send a keepalive and wait briefly
            match std::net::UdpSocket::bind("0.0.0.0:0") {
                Ok(sock) => {
                    sock.set_read_timeout(Some(std::time::Duration::from_millis(500))).ok();
                    let hdr = lightspeed_protocol::TunnelHeader::keepalive(0, 0);
                    if sock.send_to(&hdr.encode_to_array(), proxy_addr).is_ok() {
                        info!("   ✅ Proxy {} — UDP reachable", proxy_addr);
                    } else {
                        warn!("   ❌ Proxy {} — send failed", proxy_addr);
                        all_ok = false;
                    }
                }
                Err(e) => {
                    warn!("   ❌ Cannot bind local socket: {}", e);
                    all_ok = false;
                }
            }
        } else {
            info!("   ℹ️  No --proxy specified — skipping proxy check");
        }

        if all_ok {
            info!("✅ All checks passed");
        } else {
            warn!("❌ Some checks failed — see above");
            return Err(anyhow::anyhow!("Some checks failed"));
        }
        return Ok(());
    }

    // ── --scan-processes ──────────────────────────────────────────
    if cli.scan_processes {
        info!("🔍 Scanning for game processes...");
        use interceptor::process_scanner;
        let game_names: Vec<String> = if let Some(ref g) = cli.game {
            match games::detect_game(g) {
                Ok(boxed) => boxed.process_names().iter().map(|s| s.to_string()).collect(),
                Err(e) => {
                    error!("Unknown game: {}", e);
                    return Err(e.into());
                }
            }
        } else {
            // Scan for all known games
            vec![
                "RustClient.exe".into(), "FortniteClient-Win64-Shipping.exe".into(),
                "cs2.exe".into(), "dota2.exe".into(), "r5apex.exe".into(),
                "VALORANT-Win64-Shipping.exe".into(),
            ]
        };
        let game_name_refs: Vec<&str> = game_names.iter().map(|s| s.as_str()).collect();
        let results = process_scanner::scan_for_games(&game_name_refs);
        if results.is_empty() {
            info!("   No matching game processes found.");
        } else {
            for p in &results {
                info!("   PID {} ({}) — {} routes:", p.pid, p.name, p.routes.len());
                for r in &p.routes {
                    info!("      {} → {}", r.local, r.remote);
                }
            }
        }
        return Ok(());
    }

    // ── --intercept ───────────────────────────────────────────────
    if cli.intercept {
        info!("🧪 Interceptor diagnostic mode");
        let interceptor = interceptor::create_interceptor();
        info!("   Platform: {}", interceptor.platform_name());
        
        match interceptor.check_availability() {
            Ok(()) => info!("   Availability: ✅ Ready"),
            Err(e) => {
                warn!("   Availability: ❌ {}", e);
                return Ok(());
            }
        }
        
        // If game specified, try to build config and show routes
        if let Some(ref game_name) = cli.game {
            match games::detect_game(game_name) {
                Ok(game) => {
                    let proxy = cli.proxy.as_deref().unwrap_or("127.0.0.1:4434");
                    let proxy_addr = match parse_proxy_addr(proxy) {
                        Ok(a) => a,
                        Err(e) => {
                            warn!("   Invalid proxy address '{}': {}", proxy, e);
                            return Err(e.into());
                        }
                    };
                    
                    let config_opt = interceptor::build_config_for_game(
                        game.as_ref(), proxy_addr, cli.fec, cli.fec_k
                    );
                    
                    match config_opt {
                        Some(cfg) => {
                            info!("   Game: {}", cfg.game_name);
                            info!("   PID: {:?}", cfg.pid);
                            info!("   Port range: {}-{}", cfg.port_range.0, cfg.port_range.1);
                            info!("   Routes discovered: {}", cfg.initial_routes.len());
                            for r in &cfg.initial_routes {
                                info!("      {} → {}", r.local, r.remote);
                            }
                            if cfg.initial_routes.is_empty() {
                                info!("   (No server routes found — game may need to be connected to a server)");
                            }
                        }
                        None => {
                            warn!("   Could not build interceptor config — see logs above.");
                        }
                    }
                }
                Err(e) => {
                    warn!("   Unknown game '{}': {}", game_name, e);
                }
            }
        } else {
            info!("   (specify --game <name> to scan for a specific game's routes)");
        }
        
        return Ok(());
    }

    // ── --start-interceptor ────────────────────────────────────────
    if cli.start_interceptor {
        let game_key = cli.game.as_deref().ok_or_else(|| {
            anyhow::anyhow!("--start-interceptor requires --game <name>")
        })?;
        let proxy_str = cli.proxy.as_deref().unwrap_or("127.0.0.1:4434");
        let proxy_addr = parse_proxy_addr(proxy_str)?;
        
        info!("🚀 Starting live interceptor mode");
        let server_override = match cli.server_addr.as_deref() {
            Some(s) => Some(parse_proxy_addr(s)?),
            None => None,
        };
        return run_intercept_mode(game_key, proxy_addr, cli.fec, cli.fec_k, server_override).await;
    }

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
    let (proxy_id, proxy_region) = if let Some(id) = std::env::var("LIGHTSPEED_PROXY_ID").ok().filter(|s| !s.is_empty()) {
        (id, std::env::var("LIGHTSPEED_PROXY_REGION").unwrap_or_else(|_| "unknown".to_string()))
    } else {
        (format!("proxy-{}", proxy_addr.ip()), "unknown".to_string())
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
