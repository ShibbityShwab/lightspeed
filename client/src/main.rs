//! # LightSpeed Client
//!
//! Zero-cost global network optimizer for multiplayer games.
//! Captures game UDP packets and tunnels them through optimally-selected
//! proxy nodes to reduce latency via better routing paths.

// Modules marked #[allow(dead_code)] contain scaffolded interfaces
// for future integration (game capture pipeline, ML routing, etc.)
#[allow(dead_code)]
mod capture;
#[allow(dead_code)]
mod config;
#[allow(dead_code)]
mod error;
#[allow(dead_code)]
mod games;
#[allow(dead_code)]
mod ml;
#[allow(dead_code)]
mod quic;
#[allow(dead_code)]
mod redirect;
#[allow(dead_code)]
mod route;
#[allow(dead_code)]
mod tunnel;
#[allow(dead_code)]
mod warp;

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tracing::{info, warn};

use route::{ProxyHealth, ProxyNode, RouteSelector, SelectedRoute};
use route::selector::NearestSelector;
use tunnel::relay::UdpRelay;

/// LightSpeed — Reduce your ping. Free. Forever.
#[derive(Parser, Debug)]
#[command(name = "lightspeed", version, about, long_about = None)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "lightspeed.toml")]
    config: String,

    /// Game to optimize (fortnite, cs2, dota2)
    #[arg(short, long)]
    game: Option<String>,

    /// Proxy server address (host:port). If omitted, auto-selects from config.
    #[arg(short, long)]
    proxy: Option<String>,

    /// Enable verbose logging
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    /// Dry run — show what would happen without capturing packets
    #[arg(long, default_value_t = false)]
    dry_run: bool,

    /// Run tunnel test mode — send test packets to verify proxy connectivity
    #[arg(long, default_value_t = false)]
    test_tunnel: bool,

    /// Test QUIC control plane — connect, register, ping, disconnect
    #[arg(long, default_value_t = false)]
    test_control: bool,

    /// Game server address (ip:port) — enables redirect mode
    /// Traffic to this server is tunneled through the proxy
    #[arg(short = 's', long)]
    game_server: Option<String>,

    /// Local port for redirect mode (default: same as game server port)
    #[arg(long)]
    local_port: Option<u16>,

    /// Enable Forward Error Correction (FEC) for packet loss recovery.
    /// Adds ~25% bandwidth overhead but can recover any single lost packet
    /// per block of K. Much more efficient than ExitLag's packet duplication.
    #[arg(long, default_value_t = false)]
    fec: bool,

    /// FEC block size: number of data packets per parity packet (2-16, default 4).
    /// Lower K = more redundancy but more overhead. K=4 means 25% overhead.
    #[arg(long, default_value_t = 4)]
    fec_k: u8,

    /// Enable Cloudflare WARP for improved routing (5-10ms savings)
    /// Automatically connects WARP on startup and restores on shutdown
    #[arg(short = 'w', long, default_value_t = false)]
    warp: bool,

    /// Disable WARP even if previously enabled
    #[arg(long, default_value_t = false)]
    no_warp: bool,

    /// Show WARP status and exit
    #[arg(long, default_value_t = false)]
    warp_status: bool,

    /// Route selection strategy: nearest, ml (default: from config or nearest)
    #[arg(long)]
    route_strategy: Option<String>,

    /// Probe all configured proxies and display latencies, then exit
    #[arg(long, default_value_t = false)]
    probe_proxies: bool,

    /// Run comprehensive live integration test against configured proxies.
    /// Tests health, route selection, keepalive echo, data relay, and FEC.
    #[arg(long, default_value_t = false)]
    live_test: bool,

    /// Echo server address for live data relay testing (e.g., 149.28.144.74:9999).
    /// Required for data relay and FEC phases of --live-test.
    #[arg(long)]
    echo_server: Option<String>,

    /// List available network interfaces for packet capture, then exit.
    #[arg(long, default_value_t = false)]
    list_interfaces: bool,

    /// Enable pcap capture mode (alternative to redirect mode).
    /// Captures game packets directly from the network interface.
    /// Requires the pcap-capture feature and elevated privileges.
    #[arg(long, default_value_t = false)]
    capture: bool,

    /// Network interface for capture mode (e.g., "eth0", "Ethernet").
    /// If omitted, uses the system default interface.
    #[arg(long)]
    interface: Option<String>,
}

/// Parse a proxy address string into SocketAddrV4.
fn parse_proxy_addr(s: &str) -> anyhow::Result<SocketAddrV4> {
    // Try parsing as ip:port
    if let Ok(addr) = s.parse::<SocketAddrV4>() {
        return Ok(addr);
    }
    // Try parsing as host:port with DNS resolution
    use std::net::ToSocketAddrs;
    let addrs: Vec<_> = s.to_socket_addrs()?.collect();
    for addr in &addrs {
        if let std::net::SocketAddr::V4(v4) = addr {
            return Ok(*v4);
        }
    }
    anyhow::bail!("Could not resolve proxy address: {}", s);
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Initialize tracing
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();

    info!("⚡ LightSpeed v{} starting", env!("CARGO_PKG_VERSION"));

    // Load configuration
    let config = config::Config::load(&cli.config).unwrap_or_else(|e| {
        warn!("Config not found ({}), using defaults", e);
        config::Config::default()
    });

    // Detect or select game (optional in redirect mode)
    let game: Option<Box<dyn games::GameConfig>> = match cli.game.as_deref() {
        Some(name) => {
            info!("Game selected: {}", name);
            Some(games::detect_game(name)?)
        }
        None => {
            if cli.game_server.is_some() {
                // Redirect mode doesn't require game detection
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

    // --list-interfaces: show network interfaces and exit
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
                info!("   • {} [{}]{} — {}", iface.name, status, kind, iface.description);
            }
        }
        return Ok(());
    }

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
    // ISP routing inefficiencies. Free 5-10ms improvement on most paths.
    //
    // Usage: lightspeed --warp --proxy 149.28.84.139:4434 --game-server ...

    let mut warp_manager = warp::WarpManager::new();

    // --warp-status: show WARP status and exit
    if cli.warp_status {
        if !warp_manager.is_installed() {
            info!("🌐 Cloudflare WARP: Not installed");
            info!("   Install: {}", warp::install_instructions());
        } else {
            let warp_info = warp_manager.info();
            info!("🌐 Cloudflare WARP Status");
            info!("   Status:   {}", warp_info.status);
            info!("   Protocol: {}", warp_info.protocol.unwrap_or_else(|| "unknown".into()));
            info!("   Mode:     {}", warp_info.mode.unwrap_or_else(|| "unknown".into()));

            // Check routing for known proxy IPs
            let proxy_ips = vec![
                Ipv4Addr::new(149, 28, 84, 139),   // Vultr LA
                Ipv4Addr::new(149, 28, 144, 74),    // Vultr SGP
            ];
            warp_manager.print_summary(&proxy_ips);

            if let Some(stats) = warp_manager.tunnel_stats() {
                info!("   Tunnel stats:\n{}", stats);
            }
        }
        return Ok(());
    }

    // --warp: enable WARP for improved routing
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
        // Auto-detect WARP (don't connect, just report status)
        let status = warp_manager.status();
        match status {
            warp::WarpStatus::Connected => {
                info!("🌐 WARP detected and connected — traffic uses NTT backbone");
            }
            warp::WarpStatus::Disconnected => {
                info!("🌐 WARP installed but disconnected. Use --warp to enable (saves 5-10ms)");
            }
            warp::WarpStatus::NotInstalled => {
                // Silent — don't nag about WARP if not installed
            }
            _ => {}
        }
    }

    // ── Proxy selection: manual, config-based, or auto-routed ─────
    //
    // Priority:
    // 1. --proxy flag (explicit, highest priority)
    // 2. config.proxy.servers + RouteSelector (probe & pick best)
    // 3. Default localhost for development

    let proxy_addr = if let Some(ref proxy_str) = cli.proxy {
        // Explicit proxy — use it directly
        let addr = parse_proxy_addr(proxy_str)?;
        info!("🌐 Proxy (explicit): {}", addr);
        addr
    } else if !config.proxy.servers.is_empty() {
        // Multiple proxies configured — probe and select best
        let strategy = cli.route_strategy.as_deref()
            .unwrap_or(config.route.strategy.as_str());

        info!("🔍 Probing {} configured proxies (strategy: {})...", config.proxy.servers.len(), strategy);

        let game_server_addr = cli.game_server.as_ref()
            .and_then(|s| parse_proxy_addr(s).ok())
            .unwrap_or_else(|| SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0));

        let selected = select_best_proxy(
            &config.proxy.servers,
            config.proxy.data_port,
            game_server_addr,
            strategy,
        ).await?;

        info!("🌐 Proxy (auto-selected): {} [{}] — {:.1}ms latency, strategy: {:?}",
            selected.primary.data_addr,
            selected.primary.id,
            selected.primary.latency_us.unwrap_or(0) as f64 / 1000.0,
            selected.strategy,
        );
        if !selected.backups.is_empty() {
            info!("   Backups: {}",
                selected.backups.iter()
                    .map(|p| format!("{} ({:.1}ms)", p.id, p.latency_us.unwrap_or(0) as f64 / 1000.0))
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }

        selected.primary.data_addr
    } else {
        // Default proxy for development/testing
        let default = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 4434);
        warn!("No proxy specified, using default: {}", default);
        default
    };

    // --probe-proxies: show all proxy latencies and exit
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
                let latency = node.latency_us
                    .map(|us| format!("{:.1}ms", us as f64 / 1000.0))
                    .unwrap_or_else(|| "timeout".into());
                info!("   {} {} ({}) — {}", status, node.id, node.data_addr, latency);
            }
        }
        return Ok(());
    }

    // --live-test: comprehensive integration test against live proxies
    if cli.live_test {
        let echo_server = cli.echo_server.as_ref()
            .and_then(|s| parse_proxy_addr(s).ok());
        return run_live_test(&config, Some(proxy_addr), echo_server, cli.fec, cli.fec_k).await;
    }

    // Initialize UDP relay
    let bind_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
    let mut relay = UdpRelay::new(bind_addr);
    relay.bind().await?;
    let stats = Arc::clone(&relay.stats);

    if cli.test_tunnel {
        return run_tunnel_test(relay, proxy_addr).await;
    }

    if cli.test_control {
        return run_control_test(proxy_addr, &config).await;
    }

    // ── Online Learning ──────────────────────────────────────────
    //
    // Initialize the ML online learner to collect live RTT data from
    // keepalive probes. The learner persists measurements across sessions
    // and retrains the route model when enough new data accumulates.
    //
    // Data flow: keepalive echo RTT → RouteCollector → retrain → RouteModel

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
            Err(e) => {
                warn!("🧠 Online learning init failed (non-fatal): {}", e);
            }
        }
        Arc::new(tokio::sync::Mutex::new(learner))
    };

    // Shared map: keepalive seq → send Instant, used to compute RTT on echo
    let keepalive_timestamps: Arc<tokio::sync::Mutex<HashMap<u16, std::time::Instant>>> =
        Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    // ── Redirect mode: local UDP proxy ────────────────────────────
    //
    // When --game-server is specified, run in redirect mode:
    //   Game → localhost:port → LightSpeed → Proxy → Game Server
    //
    // This is the primary game integration mode.
    if let Some(ref server_str) = cli.game_server {
        let game_server_addr = parse_proxy_addr(server_str)?;

        // Use game-specific redirect port if available, otherwise match server port
        let local_port = cli.local_port.unwrap_or_else(|| {
            if let Some(ref game) = game {
                game.redirect_port()
            } else {
                game_server_addr.port()
            }
        });

        info!("🚀 Starting redirect mode");
        if let Some(ref game) = game {
            info!("   Game:        {} (anti-cheat: {})", game.name(), game.anti_cheat());
            info!("   Typical PPS: ~{} packets/sec", game.typical_pps());
        }
        info!("   Game server: {}", game_server_addr);
        info!("   Local port:  127.0.0.1:{}", local_port);
        info!("   Proxy:       {}", proxy_addr);

        let mut redirect_proxy = redirect::UdpRedirect::new(local_port, game_server_addr, proxy_addr);
        if cli.fec {
            info!("   FEC:         enabled (K={}, ~{}% overhead)", cli.fec_k, 100 / cli.fec_k as u32);
            redirect_proxy = redirect_proxy.with_fec(cli.fec_k);
        }
        return redirect_proxy.run().await;
    }

    // ── Capture mode: pcap-based packet sniffing ──────────────────
    //
    // When --capture is specified with a game, capture packets directly
    // from the network interface and forward through the proxy.
    // This is more transparent than redirect mode but requires:
    //   1. pcap-capture feature enabled
    //   2. Elevated privileges (admin/root)
    //   3. Npcap (Windows) or libpcap (Linux/macOS)
    if cli.capture {
        let game_ref = game.as_ref().ok_or_else(|| {
            anyhow::anyhow!("Capture mode requires a game. Use --game <name> or ensure a game is running.")
        })?;

        info!("🔍 Starting capture mode");
        info!("   Game:      {} (anti-cheat: {})", game_ref.name(), game_ref.anti_cheat());
        info!("   Ports:     {:?}", game_ref.ports());
        info!("   Proxy:     {}", proxy_addr);
        if let Some(ref iface) = cli.interface {
            info!("   Interface: {}", iface);
        } else {
            info!("   Interface: (auto-detect)");
        }

        let filter = game_ref.build_capture_filter();
        info!("   BPF filter: {}", filter.bpf);

        // Create capture backend
        let mut cap_backend = if let Some(ref iface) = cli.interface {
            match capture::create_capture_on(iface) {
                Ok(c) => c,
                Err(e) => {
                    anyhow::bail!("Failed to create capture on '{}': {}", iface, e);
                }
            }
        } else {
            match capture::create_default_capture() {
                Ok(c) => c,
                Err(e) => {
                    anyhow::bail!("Failed to create capture backend: {}\n   \
                        Ensure pcap-capture feature is enabled: cargo build --features pcap-capture", e);
                }
            }
        };

        // Start capture
        cap_backend.start(&filter).map_err(|e| {
            anyhow::anyhow!("Capture start failed: {}\n   \
                You may need to run with elevated privileges (admin/root).", e)
        })?;

        info!("⚡ Capture active — sniffing {} traffic on ports {:?}",
            game_ref.name(), game_ref.ports());
        info!("   Captured packets will be forwarded through proxy {}", proxy_addr);

        // Create tunnel socket (shared between outbound capture and inbound injection)
        let tunnel_socket = Arc::new(tokio::net::UdpSocket::bind("0.0.0.0:0").await?);
        let fec_enabled = cli.fec;
        let fec_k = cli.fec_k;

        // Create packet injector for bidirectional response delivery
        let injector = capture::injector::PacketInjector::new().await?;
        let injector_stats = Arc::clone(&injector.stats);
        let injector_socket = injector.socket();

        // Track the game client's source address (learned from first outbound packet)
        let game_client_addr: Arc<tokio::sync::RwLock<Option<SocketAddrV4>>> =
            Arc::new(tokio::sync::RwLock::new(None));

        if fec_enabled {
            info!("   FEC:       enabled (K={}, ~{}% overhead)", fec_k, 100 / fec_k as u32);
        }
        info!("   Mode:      bidirectional (capture + inject)");
        info!("   Press Ctrl+C to stop\n");

        // Set up Ctrl+C handler
        let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let running_flag = Arc::clone(&running);
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            running_flag.store(false, Ordering::Relaxed);
        });

        // ── Outbound capture stats (shared with capture loop) ────
        let outbound_packets = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let outbound_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let start_time = std::time::Instant::now();

        // ── Inbound task: Proxy → Injector → Game ────────────────
        //
        // Receives tunnel-wrapped responses from the proxy, decodes them
        // (including FEC recovery), and injects the game payload back to
        // the game client's local address. Also processes keepalive echoes
        // for online learning RTT measurement.
        let inbound_handle = {
            let tunnel_socket = Arc::clone(&tunnel_socket);
            let game_client_addr = Arc::clone(&game_client_addr);
            let running = Arc::clone(&running);
            let injector_stats_ref = Arc::clone(&injector_stats);
            let ka_timestamps = Arc::clone(&keepalive_timestamps);
            let learner_ref = Arc::clone(&online_learner);
            let cap_proxy_id = proxy_id.clone();
            let cap_proxy_region = proxy_region.clone();

            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];
                let mut fec_decoder = if fec_enabled {
                    Some(lightspeed_protocol::FecDecoder::new())
                } else {
                    None
                };
                let mut gc_counter: u32 = 0;

                while running.load(Ordering::Relaxed) {
                    let recv_result = tokio::time::timeout(
                        Duration::from_millis(100),
                        tunnel_socket.recv_from(&mut buf),
                    ).await;

                    let (len, _from) = match recv_result {
                        Ok(Ok(r)) => r,
                        Ok(Err(e)) => {
                            tracing::debug!("Tunnel recv error: {}", e);
                            continue;
                        }
                        Err(_) => continue, // Timeout — check running flag
                    };

                    injector_stats_ref.packets_from_proxy.fetch_add(1, Ordering::Relaxed);

                    // Decode tunnel header
                    let (header, payload) = match lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len]) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::debug!("Invalid tunnel response: {}", e);
                            continue;
                        }
                    };

                    // Process keepalive echoes — compute RTT for online learning
                    if header.is_keepalive() {
                        let rtt_us = {
                            let mut ts_map = ka_timestamps.lock().await;
                            ts_map.remove(&header.sequence)
                                .map(|send_time| send_time.elapsed().as_micros() as u64)
                        };
                        if let Some(rtt) = rtt_us {
                            let latency_ms = rtt as f64 / 1000.0;
                            tracing::trace!(
                                seq = header.sequence,
                                rtt_ms = latency_ms,
                                "Keepalive echo: {:.1}ms", latency_ms,
                            );
                            let mut learner = learner_ref.lock().await;
                            learner.record_and_maybe_retrain(
                                &cap_proxy_id, &cap_proxy_region,
                                latency_ms, 0.0, 0.0, 0.0,
                            );
                        } else {
                            tracing::trace!("Keepalive echo received (no send timestamp)");
                        }
                        continue;
                    }

                    // Get the game client's address (learned from outbound capture)
                    let game_addr = {
                        let addr = game_client_addr.read().await;
                        *addr
                    };
                    let game_addr = match game_addr {
                        Some(addr) => addr,
                        None => {
                            tracing::debug!("Response received but no game client captured yet");
                            continue;
                        }
                    };

                    // Handle FEC if enabled
                    let game_payload: Option<bytes::Bytes> = if header.has_fec() && fec_decoder.is_some() {
                        if payload.len() < lightspeed_protocol::FEC_HEADER_SIZE {
                            tracing::debug!("FEC packet too short");
                            continue;
                        }

                        let mut fec_slice: &[u8] = &payload[..lightspeed_protocol::FEC_HEADER_SIZE];
                        let fec_hdr = match lightspeed_protocol::FecHeader::decode(&mut fec_slice) {
                            Some(h) => h,
                            None => {
                                tracing::debug!("Invalid FEC header in response");
                                continue;
                            }
                        };

                        let game_data = &payload[lightspeed_protocol::FEC_HEADER_SIZE..];
                        let decoder = fec_decoder.as_mut().unwrap();

                        if fec_hdr.is_parity() {
                            let parity_data = bytes::Bytes::copy_from_slice(game_data);
                            if let Some((_idx, recovered)) = decoder.receive_parity(&fec_hdr, parity_data) {
                                injector_stats_ref.fec_recovered.fetch_add(1, Ordering::Relaxed);
                                tracing::info!(
                                    block = fec_hdr.block_id,
                                    recovered_len = recovered.len(),
                                    "🔧 FEC recovered lost packet"
                                );
                                Some(recovered)
                            } else {
                                None // Parity consumed, no recovery needed
                            }
                        } else {
                            let data_bytes = bytes::Bytes::copy_from_slice(game_data);
                            decoder.receive_data(&fec_hdr, data_bytes.clone());
                            Some(data_bytes)
                        }
                    } else {
                        // Non-FEC: payload is the game data directly
                        Some(bytes::Bytes::copy_from_slice(payload))
                    };

                    // Inject the response back to the game
                    if let Some(data) = game_payload {
                        if !data.is_empty() {
                            match injector_socket.send_to(&data, game_addr).await {
                                Ok(sent) => {
                                    injector_stats_ref.packets_injected.fetch_add(1, Ordering::Relaxed);
                                    injector_stats_ref.bytes_injected.fetch_add(sent as u64, Ordering::Relaxed);
                                    tracing::trace!(
                                        payload_len = data.len(),
                                        dst = %game_addr,
                                        "Proxy → Game (injected)"
                                    );
                                }
                                Err(e) => {
                                    injector_stats_ref.inject_errors.fetch_add(1, Ordering::Relaxed);
                                    tracing::warn!("Inject to game failed: {}", e);
                                }
                            }
                        }
                    }

                    // Periodic FEC GC
                    gc_counter += 1;
                    if gc_counter % 100 == 0 {
                        if let Some(ref mut dec) = fec_decoder {
                            dec.gc();
                        }
                    }
                }
            })
        };

        // ── Keepalive task (with RTT timestamp recording) ────────
        let keepalive_handle = {
            let tunnel_socket = Arc::clone(&tunnel_socket);
            let running = Arc::clone(&running);
            let ka_timestamps = Arc::clone(&keepalive_timestamps);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                let mut ka_seq: u16 = 60000;
                while running.load(Ordering::Relaxed) {
                    interval.tick().await;
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32;
                    let header = lightspeed_protocol::TunnelHeader::keepalive(ka_seq, ts);
                    if tunnel_socket.send_to(&header.encode(), proxy_addr).await.is_ok() {
                        let mut ts_map = ka_timestamps.lock().await;
                        ts_map.insert(ka_seq, std::time::Instant::now());
                        // Evict entries older than 30s to prevent unbounded growth
                        ts_map.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                    }
                    ka_seq = ka_seq.wrapping_add(1);
                }
            })
        };

        // ── Stats logger task ────────────────────────────────────
        let stats_handle = {
            let out_pkts = Arc::clone(&outbound_packets);
            let out_bytes = Arc::clone(&outbound_bytes);
            let inj_stats = Arc::clone(&injector_stats);
            let running = Arc::clone(&running);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(10));
                while running.load(Ordering::Relaxed) {
                    interval.tick().await;
                    let cap = out_pkts.load(Ordering::Relaxed);
                    let cap_b = out_bytes.load(Ordering::Relaxed);
                    let inj = inj_stats.packets_injected.load(Ordering::Relaxed);
                    let inj_b = inj_stats.bytes_injected.load(Ordering::Relaxed);
                    let from_proxy = inj_stats.packets_from_proxy.load(Ordering::Relaxed);
                    let recovered = inj_stats.fec_recovered.load(Ordering::Relaxed);
                    let errors = inj_stats.inject_errors.load(Ordering::Relaxed);

                    if cap > 0 || from_proxy > 0 {
                        if fec_enabled {
                            info!(
                                "📊 Out: {} pkts ({} B) | In: {} from proxy → {} injected ({} B) | FEC recovered: {} | Errors: {}",
                                cap, cap_b, from_proxy, inj, inj_b, recovered, errors
                            );
                        } else {
                            info!(
                                "📊 Out: {} pkts ({} B) | In: {} from proxy → {} injected ({} B) | Errors: {}",
                                cap, cap_b, from_proxy, inj, inj_b, errors
                            );
                        }
                    }
                }
            })
        };

        // ── Outbound capture loop: Game → pcap → Tunnel → Proxy ──
        //
        // Captures game packets from the network interface, wraps them
        // in LightSpeed headers (with optional FEC), and forwards to proxy.
        let mut seq: u16 = 0;
        let mut fec_encoder = if fec_enabled {
            Some(lightspeed_protocol::FecEncoder::new(fec_k))
        } else {
            None
        };

        while running.load(Ordering::Relaxed) {
            match cap_backend.next_packet() {
                Ok(pkt) => {
                    outbound_packets.fetch_add(1, Ordering::Relaxed);
                    outbound_bytes.fetch_add(pkt.payload.len() as u64, Ordering::Relaxed);

                    // Learn the game client's source address
                    {
                        let mut addr = game_client_addr.write().await;
                        if addr.is_none() {
                            info!("🎮 Game client detected at {} → {}", pkt.src, pkt.dst);
                        }
                        *addr = Some(pkt.src);
                    }

                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32;

                    if let Some(ref mut encoder) = fec_encoder {
                        // FEC mode: wrap with FEC header
                        use bytes::BytesMut;
                        use lightspeed_protocol::{FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};

                        let block_id = encoder.block_id();
                        let index = encoder.current_index();

                        let header = lightspeed_protocol::TunnelHeader::new_fec(
                            seq, ts, pkt.src, pkt.dst,
                        );
                        let fec_hdr = FecHeader::data(block_id, index, fec_k);

                        let mut pkt_buf = BytesMut::with_capacity(
                            HEADER_SIZE + FEC_HEADER_SIZE + pkt.payload.len(),
                        );
                        pkt_buf.extend_from_slice(&header.encode());
                        fec_hdr.encode(&mut pkt_buf);
                        pkt_buf.extend_from_slice(&pkt.payload);

                        let parity = encoder.add_packet(&pkt.payload);
                        let _ = tunnel_socket.send_to(&pkt_buf, proxy_addr).await;

                        // Send parity when block completes
                        if let Some(parity_bytes) = parity {
                            let parity_seq = seq.wrapping_add(1);
                            let parity_header = lightspeed_protocol::TunnelHeader::new_fec(
                                parity_seq, ts, pkt.src, pkt.dst,
                            );
                            let parity_fec = FecHeader::parity(block_id, fec_k);
                            let mut parity_buf = BytesMut::with_capacity(
                                HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len(),
                            );
                            parity_buf.extend_from_slice(&parity_header.encode());
                            parity_fec.encode(&mut parity_buf);
                            parity_buf.extend_from_slice(&parity_bytes);
                            let _ = tunnel_socket.send_to(&parity_buf, proxy_addr).await;
                            seq = seq.wrapping_add(1); // Extra seq for parity
                        }
                    } else {
                        // Non-FEC: simple tunnel header + payload
                        let header = lightspeed_protocol::TunnelHeader::new(
                            seq, ts, pkt.src, pkt.dst,
                        );
                        let packet = header.encode_with_payload(&pkt.payload);
                        let _ = tunnel_socket.send_to(&packet, proxy_addr).await;
                    }

                    tracing::trace!(
                        seq = seq,
                        src = %pkt.src,
                        dst = %pkt.dst,
                        payload_len = pkt.payload.len(),
                        "Captured → Proxy"
                    );

                    seq = seq.wrapping_add(1);
                }
                Err(e) => {
                    let err_str = format!("{}", e);
                    if !err_str.contains("Timeout") && !err_str.contains("timeout") {
                        tracing::debug!("Capture error: {}", e);
                    }
                }
            }
        }

        // ── Shutdown ─────────────────────────────────────────────
        let _ = cap_backend.stop();
        inbound_handle.abort();
        keepalive_handle.abort();
        stats_handle.abort();

        let elapsed = start_time.elapsed();
        let out_total = outbound_packets.load(Ordering::Relaxed);
        let out_bytes_total = outbound_bytes.load(Ordering::Relaxed);
        let inj_total = injector_stats.packets_injected.load(Ordering::Relaxed);
        let inj_bytes_total = injector_stats.bytes_injected.load(Ordering::Relaxed);
        let from_proxy_total = injector_stats.packets_from_proxy.load(Ordering::Relaxed);
        let fec_recovered_total = injector_stats.fec_recovered.load(Ordering::Relaxed);
        let inject_errors = injector_stats.inject_errors.load(Ordering::Relaxed);

        info!("\n⚡ Capture stopped");
        info!("📊 Final stats:");
        info!("   Duration:        {:.1}s", elapsed.as_secs_f64());
        info!("   ── Outbound (Game → Proxy) ──");
        info!("   Captured:        {} packets, {} bytes", out_total, out_bytes_total);
        if elapsed.as_secs() > 0 && out_total > 0 {
            info!("   Avg PPS:         {:.0}", out_total as f64 / elapsed.as_secs_f64());
        }
        info!("   ── Inbound (Proxy → Game) ──");
        info!("   From proxy:      {} packets", from_proxy_total);
        info!("   Injected:        {} packets, {} bytes", inj_total, inj_bytes_total);
        if fec_enabled {
            info!("   FEC recovered:   {} packets", fec_recovered_total);
        }
        info!("   Inject errors:   {}", inject_errors);

        // Save online learning state
        {
            let learner = online_learner.lock().await;
            learner.save_state();
            let summary = learner.summary();
            info!("🧠 Online learning: {} total measurements, {} retrains",
                summary.total_measurements, summary.retrain_count);
        }

        return Ok(());
    }

    // If a game was specified but no --game-server, show setup instructions
    if let Some(ref game) = game {
        info!("📋 {} setup instructions:", game.name());
        for line in game.redirect_instructions().lines() {
            info!("   {}", line);
        }
        info!("");
        info!("   Example: lightspeed --game {} --game-server <SERVER_IP>:{} --proxy {}",
            cli.game.as_deref().unwrap_or("unknown"),
            game.redirect_port(),
            proxy_addr,
        );
        info!("");
    }

    // ── Keepalive mode (no game server specified) ─────────────────
    //
    // Maintains proxy session with keepalives and logs stats.
    // Use --game-server to enable full redirect mode.

    info!("⚡ LightSpeed tunnel active — keepalive mode");
    info!("   Use --game-server <ip:port> for full redirect mode");
    info!("   (Full packet capture requires --features pcap-capture)");

    // Spawn keepalive sender (with RTT timestamp recording for online learning)
    let keepalive_handle = {
        let relay_socket = relay.socket().expect("socket bound");
        let proxy = proxy_addr;
        let ka_timestamps = Arc::clone(&keepalive_timestamps);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            let mut seq: u16 = 0;
            loop {
                interval.tick().await;
                let header = lightspeed_protocol::TunnelHeader::keepalive(
                    seq,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32,
                );
                let packet = header.encode();
                match relay_socket.send_to(&packet, proxy).await {
                    Ok(_) => {
                        let mut ts_map = ka_timestamps.lock().await;
                        ts_map.insert(seq, std::time::Instant::now());
                        // Evict entries older than 30s
                        ts_map.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                        tracing::trace!(seq = seq, "Sent keepalive to proxy");
                    }
                    Err(e) => {
                        tracing::warn!("Keepalive send failed: {}", e);
                    }
                }
                seq = seq.wrapping_add(1);
            }
        })
    };

    // Spawn response receiver (computes RTT for online learning + logs)
    let recv_handle = {
        let stats = Arc::clone(&stats);
        let relay_socket = relay.socket().expect("socket bound");
        let ka_timestamps = Arc::clone(&keepalive_timestamps);
        let learner_ref = Arc::clone(&online_learner);
        let ka_proxy_id = proxy_id.clone();
        let ka_proxy_region = proxy_region.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            loop {
                match relay_socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        match lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len]) {
                            Ok((header, payload)) => {
                                stats.packets_received.fetch_add(1, Ordering::Relaxed);
                                stats
                                    .bytes_received
                                    .fetch_add(len as u64, Ordering::Relaxed);

                                if header.is_keepalive() {
                                    // Compute RTT and feed into online learner
                                    let rtt_us = {
                                        let mut ts_map = ka_timestamps.lock().await;
                                        ts_map.remove(&header.sequence)
                                            .map(|send_time| send_time.elapsed().as_micros() as u64)
                                    };
                                    if let Some(rtt) = rtt_us {
                                        let latency_ms = rtt as f64 / 1000.0;
                                        tracing::trace!(
                                            seq = header.sequence,
                                            from = %addr,
                                            rtt_ms = latency_ms,
                                            "Keepalive echo: {:.1}ms", latency_ms,
                                        );
                                        let mut learner = learner_ref.lock().await;
                                        learner.record_and_maybe_retrain(
                                            &ka_proxy_id, &ka_proxy_region,
                                            latency_ms, 0.0, 0.0, 0.0,
                                        );
                                    } else {
                                        tracing::trace!(
                                            seq = header.sequence,
                                            from = %addr,
                                            "Keepalive echo received"
                                        );
                                    }
                                } else {
                                    tracing::debug!(
                                        seq = header.sequence,
                                        payload_len = payload.len(),
                                        from = %addr,
                                        "Tunnel response received"
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::debug!(error = %e, "Invalid response packet");
                            }
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Recv error: {}", e);
                    }
                }
            }
        })
    };

    // Spawn stats logger
    let stats_handle = {
        let stats = Arc::clone(&stats);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;
                let sent = stats.packets_sent.load(Ordering::Relaxed);
                let recv = stats.packets_received.load(Ordering::Relaxed);
                let bytes_out = stats.bytes_sent.load(Ordering::Relaxed);
                let bytes_in = stats.bytes_received.load(Ordering::Relaxed);
                info!(
                    sent = sent,
                    recv = recv,
                    bytes_out = bytes_out,
                    bytes_in = bytes_in,
                    "📊 Tunnel stats"
                );
            }
        })
    };

    info!("⚡ Press Ctrl+C to stop");

    // Wait for shutdown
    tokio::signal::ctrl_c().await?;
    info!("⚡ Shutdown signal received");

    keepalive_handle.abort();
    recv_handle.abort();
    stats_handle.abort();

    // Save online learning state
    {
        let learner = online_learner.lock().await;
        learner.save_state();
        let summary = learner.summary();
        info!("🧠 Online learning: {} total measurements, {} retrains",
            summary.total_measurements, summary.retrain_count);
    }

    info!("⚡ LightSpeed shut down cleanly");
    Ok(())
}

// ── Proxy Probing & Route Selection ─────────────────────────────────

/// Probe a single proxy by sending keepalive packets and measuring RTT.
///
/// Sends `num_pings` keepalive packets and returns the median RTT in microseconds.
/// Returns `None` if the proxy doesn't respond within the timeout.
async fn probe_single_proxy(
    addr: SocketAddrV4,
    num_pings: usize,
    timeout_ms: u64,
) -> Option<u64> {
    use lightspeed_protocol::TunnelHeader;
    use tokio::net::UdpSocket;

    let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    let mut rtts = Vec::with_capacity(num_pings);

    for seq in 0..num_pings as u16 {
        let send_time = std::time::Instant::now();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u32;

        let header = TunnelHeader::keepalive(seq, ts);
        let packet = header.encode();

        if socket.send_to(&packet, addr).await.is_err() {
            continue;
        }

        let mut buf = vec![0u8; 128];
        match tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            socket.recv_from(&mut buf),
        )
        .await
        {
            Ok(Ok((len, _))) => {
                if TunnelHeader::decode(&buf[..len]).is_ok() {
                    let rtt = send_time.elapsed().as_micros() as u64;
                    rtts.push(rtt);
                }
            }
            _ => {} // Timeout or error — skip this ping
        }

        // Small delay between pings
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    if rtts.is_empty() {
        return None;
    }

    // Return median RTT
    rtts.sort();
    Some(rtts[rtts.len() / 2])
}

/// Probe all configured proxy servers and return ProxyNode list with measured latencies.
async fn probe_all_proxies(servers: &[String], data_port: u16) -> Vec<ProxyNode> {
    let mut nodes = Vec::new();

    // Probe all proxies concurrently
    let mut handles = Vec::new();
    for (i, server_str) in servers.iter().enumerate() {
        let addr = match parse_proxy_addr(server_str) {
            Ok(a) => {
                // If the server string doesn't have a port, use the config data_port
                if !server_str.contains(':') {
                    SocketAddrV4::new(*a.ip(), data_port)
                } else {
                    a
                }
            }
            Err(e) => {
                warn!("Cannot resolve proxy {}: {}", server_str, e);
                continue;
            }
        };

        let id = format!("proxy-{}", i);
        let server_str_clone = server_str.clone();
        handles.push(tokio::spawn(async move {
            let latency = probe_single_proxy(addr, 3, 2000).await;
            (id, addr, server_str_clone, latency)
        }));
    }

    for handle in handles {
        if let Ok((id, addr, _server_str, latency)) = handle.await {
            let health = match latency {
                Some(us) if us < 500_000 => ProxyHealth::Healthy,    // < 500ms
                Some(_) => ProxyHealth::Degraded,                      // > 500ms
                None => ProxyHealth::Unhealthy,                        // No response
            };

            nodes.push(ProxyNode {
                id,
                data_addr: addr,
                control_addr: SocketAddrV4::new(*addr.ip(), 4433),
                region: "unknown".into(),
                health,
                latency_us: latency,
                load: 0.0,
            });
        }
    }

    nodes
}

/// Select the best proxy from configured servers using the specified strategy.
///
/// Probes all proxies, builds ProxyNode list, and runs the RouteSelector.
async fn select_best_proxy(
    servers: &[String],
    data_port: u16,
    game_server: SocketAddrV4,
    strategy: &str,
) -> anyhow::Result<SelectedRoute> {
    let nodes = probe_all_proxies(servers, data_port).await;

    if nodes.is_empty() {
        anyhow::bail!("No proxy servers could be resolved");
    }

    let healthy_count = nodes.iter().filter(|n| n.health == ProxyHealth::Healthy).count();
    if healthy_count == 0 {
        warn!("⚠️  No healthy proxies found, trying degraded nodes...");
    }

    // Select route using the configured strategy
    let selector: Box<dyn RouteSelector> = match strategy {
        "ml" => {
            match route::selector::MlSelector::with_synthetic_training(100) {
                Ok(ml) => {
                    info!("   Using ML route selector");
                    Box::new(ml)
                }
                Err(e) => {
                    warn!("   ML selector failed ({}), falling back to nearest", e);
                    Box::new(NearestSelector::new())
                }
            }
        }
        _ => {
            Box::new(NearestSelector::new())
        }
    };

    selector.select(game_server, &nodes)
        .map_err(|e| anyhow::anyhow!("Route selection failed: {}", e))
}

// ── Test & Diagnostic Functions ─────────────────────────────────────

/// Run a tunnel test: send test packets to the proxy and verify round-trip.
async fn run_tunnel_test(mut relay: UdpRelay, proxy_addr: SocketAddrV4) -> anyhow::Result<()> {
    info!("🧪 Running tunnel test to {}", proxy_addr);

    let test_src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
    let test_dst = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 7777);
    let test_payload = b"LightSpeed tunnel test packet";

    // Send 5 test packets
    for i in 0..5u16 {
        match relay
            .send_to_proxy(test_payload, test_src, test_dst, proxy_addr)
            .await
        {
            Ok(sent) => {
                info!("  ✓ Sent test packet #{} ({} bytes)", i + 1, sent);
            }
            Err(e) => {
                warn!("  ✗ Failed to send test packet #{}: {}", i + 1, e);
            }
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Wait for responses
    info!("  Waiting for responses...");
    let timeout = Duration::from_secs(3);
    let start = std::time::Instant::now();
    let mut received = 0;

    while start.elapsed() < timeout {
        match relay.recv_with_timeout(Duration::from_millis(500)).await {
            Ok((header, payload, from)) => {
                received += 1;
                info!(
                    "  ✓ Response #{}: seq={} payload={} bytes from {}",
                    received,
                    header.sequence,
                    payload.len(),
                    from
                );
            }
            Err(error::TunnelError::Timeout(_)) => {
                // Timeout is expected if no game server is listening
                break;
            }
            Err(e) => {
                warn!("  ✗ Recv error: {}", e);
                break;
            }
        }
    }

    let sent = relay.stats.packets_sent.load(Ordering::Relaxed);
    let bytes_sent = relay.stats.bytes_sent.load(Ordering::Relaxed);

    info!("🧪 Tunnel test complete:");
    info!("   Packets sent:     {}", sent);
    info!("   Bytes sent:       {}", bytes_sent);
    info!("   Responses:        {}", received);

    if received > 0 {
        info!("   ✅ Tunnel is working!");
    } else {
        info!("   ⚠️  No responses (expected if no game server at test destination)");
        info!("   The proxy received and forwarded the packets successfully");
        info!("   if you see session logs on the proxy side.");
    }

    Ok(())
}

/// Run a QUIC control plane test: connect, register, ping, disconnect.
async fn run_control_test(proxy_addr: SocketAddrV4, config: &config::Config) -> anyhow::Result<()> {
    use std::net::SocketAddr;

    info!("🧪 Running QUIC control plane test");

    // Control plane is on the QUIC port (default 4433), data plane on proxy_addr
    let control_port = config.proxy.quic_port;
    let control_addr = SocketAddr::V4(SocketAddrV4::new(*proxy_addr.ip(), control_port));

    info!("   Control plane: {}", control_addr);

    // Create control client
    let mut client = quic::ControlClient::new()
        .map_err(|e| anyhow::anyhow!("Failed to create QUIC client: {}", e))?;

    // Connect and register
    info!("   Step 1: Connecting and registering...");
    let game_id = lightspeed_protocol::game_id::UNKNOWN;
    client
        .connect(control_addr, game_id)
        .await
        .map_err(|e| anyhow::anyhow!("QUIC connect failed: {}", e))?;

    info!(
        "   ✓ Connected! session={:?} token={:?} node={:?} region={:?}",
        client.session_id(),
        client.session_token(),
        client.node_id(),
        client.region(),
    );

    // Ping test (5 pings)
    info!("   Step 2: Sending 5 pings...");
    let mut rtts = Vec::new();
    for i in 0..5 {
        match client.ping().await {
            Ok(rtt_us) => {
                info!(
                    "   ✓ Ping #{}: {}μs ({:.2}ms)",
                    i + 1,
                    rtt_us,
                    rtt_us as f64 / 1000.0
                );
                rtts.push(rtt_us);
            }
            Err(e) => {
                warn!("   ✗ Ping #{} failed: {}", i + 1, e);
            }
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // Stats
    if !rtts.is_empty() {
        let avg = rtts.iter().sum::<u64>() / rtts.len() as u64;
        let min = *rtts.iter().min().unwrap();
        let max = *rtts.iter().max().unwrap();
        info!(
            "   📊 RTT: avg={}μs min={}μs max={}μs ({} samples)",
            avg,
            min,
            max,
            rtts.len()
        );
    }

    // Disconnect
    info!("   Step 3: Disconnecting...");
    client
        .disconnect()
        .await
        .map_err(|e| anyhow::anyhow!("Disconnect failed: {}", e))?;
    info!("   ✓ Disconnected");

    info!("🧪 QUIC control plane test complete");
    if rtts.len() == 5 {
        info!("   ✅ Control plane is working!");
    } else {
        info!("   ⚠️  {} of 5 pings succeeded", rtts.len());
    }

    Ok(())
}

// ── Live Integration Test ───────────────────────────────────────────

/// Run comprehensive live integration test against configured proxies.
///
/// Tests 5 phases:
/// 1. Proxy health check (keepalive probe)
/// 2. Route selection (auto-select best proxy)
/// 3. Keepalive echo (10 packets per proxy, latency stats)
/// 4. Data relay (requires echo server)
/// 5. FEC relay (requires echo server + --fec)
async fn run_live_test(
    config: &config::Config,
    explicit_proxy: Option<SocketAddrV4>,
    echo_server: Option<SocketAddrV4>,
    fec_enabled: bool,
    fec_k: u8,
) -> anyhow::Result<()> {
    use lightspeed_protocol::TunnelHeader;
    use tokio::net::UdpSocket;

    info!("🧪 LightSpeed Live Integration Test");
    info!("══════════════════════════════════════════════════════");

    let servers = &config.proxy.servers;
    let data_port = config.proxy.data_port;

    // Build list of proxies to test
    let proxy_addrs: Vec<(String, SocketAddrV4)> = if !servers.is_empty() {
        servers.iter().enumerate().filter_map(|(i, s)| {
            parse_proxy_addr(s).ok().map(|addr| {
                let addr = if !s.contains(':') {
                    SocketAddrV4::new(*addr.ip(), data_port)
                } else {
                    addr
                };
                (format!("proxy-{}", i), addr)
            })
        }).collect()
    } else if let Some(addr) = explicit_proxy {
        vec![("proxy-0".into(), addr)]
    } else {
        anyhow::bail!("No proxies configured. Add servers to config or use --proxy");
    };

    if proxy_addrs.is_empty() {
        anyhow::bail!("No proxy addresses could be resolved");
    }

    let mut total_pass = 0u32;
    let mut total_fail = 0u32;
    let mut total_skip = 0u32;

    // ── Phase 1: Proxy Health Check ─────────────────────────────
    info!("\n📡 Phase 1: Proxy Health Check");
    info!("──────────────────────────────────────────────────────");

    let nodes = probe_all_proxies(
        &proxy_addrs.iter().map(|(_, a)| a.to_string()).collect::<Vec<_>>(),
        data_port,
    ).await;

    let mut healthy_nodes: Vec<&ProxyNode> = Vec::new();
    for (i, node) in nodes.iter().enumerate() {
        let label = if i < proxy_addrs.len() { &proxy_addrs[i].0 } else { &node.id };
        match node.health {
            ProxyHealth::Healthy => {
                let ms = node.latency_us.unwrap_or(0) as f64 / 1000.0;
                info!("  ✅ {} ({}) — {:.1}ms [Healthy]", label, node.data_addr, ms);
                healthy_nodes.push(node);
                total_pass += 1;
            }
            ProxyHealth::Degraded => {
                let ms = node.latency_us.unwrap_or(0) as f64 / 1000.0;
                warn!("  ⚠️  {} ({}) — {:.1}ms [Degraded]", label, node.data_addr, ms);
                healthy_nodes.push(node);
                total_pass += 1;
            }
            _ => {
                warn!("  ❌ {} ({}) — TIMEOUT [Unhealthy]", label, node.data_addr);
                total_fail += 1;
            }
        }
    }

    if healthy_nodes.is_empty() {
        info!("\n❌ All proxies unreachable — cannot continue");
        info!("══════════════════════════════════════════════════════");
        return Ok(());
    }

    // ── Phase 2: Route Selection ────────────────────────────────
    info!("\n🔀 Phase 2: Route Selection");
    info!("──────────────────────────────────────────────────────");

    if nodes.len() >= 2 {
        let dummy_gs = SocketAddrV4::new(Ipv4Addr::new(0, 0, 0, 0), 0);
        let selector = NearestSelector::new();
        match selector.select(dummy_gs, &nodes) {
            Ok(route) => {
                let ms = route.primary.latency_us.unwrap_or(0) as f64 / 1000.0;
                info!("  Strategy:  {:?}", route.strategy);
                info!("  Selected:  {} ({:.1}ms)", route.primary.id, ms);
                if !route.backups.is_empty() {
                    let backups: Vec<String> = route.backups.iter()
                        .map(|b| format!("{} ({:.1}ms)", b.id, b.latency_us.unwrap_or(0) as f64 / 1000.0))
                        .collect();
                    info!("  Backups:   {}", backups.join(", "));
                }
                info!("  ✅ Route selection working");
                total_pass += 1;
            }
            Err(e) => {
                warn!("  ❌ Route selection failed: {}", e);
                total_fail += 1;
            }
        }
    } else {
        info!("  ⏭️  Only 1 proxy — route selection not applicable");
        total_skip += 1;
    }

    // ── Phase 3: Keepalive Echo (detailed) ──────────────────────
    info!("\n💓 Phase 3: Keepalive Echo (10 packets each)");
    info!("──────────────────────────────────────────────────────");

    for (_i, (label, addr)) in proxy_addrs.iter().enumerate() {
        let socket = match UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                warn!("  ❌ {} — socket bind failed: {}", label, e);
                total_fail += 1;
                continue;
            }
        };

        let num_pings = 10;
        let mut rtts = Vec::with_capacity(num_pings);

        for seq in 0..num_pings as u16 {
            let send_time = std::time::Instant::now();
            let ts = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u32;

            let header = TunnelHeader::keepalive(seq, ts);
            let packet = header.encode();

            if socket.send_to(&packet, addr).await.is_err() {
                continue;
            }

            let mut buf = vec![0u8; 128];
            match tokio::time::timeout(
                Duration::from_millis(3000),
                socket.recv_from(&mut buf),
            ).await {
                Ok(Ok((len, _))) => {
                    if TunnelHeader::decode(&buf[..len]).is_ok() {
                        rtts.push(send_time.elapsed().as_micros() as u64);
                    }
                }
                _ => {}
            }

            tokio::time::sleep(Duration::from_millis(50)).await;
        }

        if rtts.is_empty() {
            warn!("  ❌ {} — 0/{} keepalives echoed", label, num_pings);
            total_fail += 1;
        } else {
            rtts.sort();
            let avg = rtts.iter().sum::<u64>() as f64 / rtts.len() as f64 / 1000.0;
            let min = *rtts.first().unwrap() as f64 / 1000.0;
            let max = *rtts.last().unwrap() as f64 / 1000.0;
            let jitter = if rtts.len() > 1 {
                let diffs: Vec<f64> = rtts.windows(2)
                    .map(|w| (w[1] as f64 - w[0] as f64).abs() / 1000.0)
                    .collect();
                diffs.iter().sum::<f64>() / diffs.len() as f64
            } else {
                0.0
            };

            info!(
                "  ✅ {}: {}/{} received, avg={:.1}ms, min={:.1}ms, max={:.1}ms, jitter={:.1}ms",
                label, rtts.len(), num_pings, avg, min, max, jitter
            );
            total_pass += 1;
        }
    }

    // ── Phase 4: Data Relay ─────────────────────────────────────
    info!("\n📦 Phase 4: Data Relay");
    info!("──────────────────────────────────────────────────────");

    if let Some(echo_addr) = echo_server {
        for (label, proxy) in &proxy_addrs {
            let socket = match UdpSocket::bind("0.0.0.0:0").await {
                Ok(s) => s,
                Err(e) => {
                    warn!("  ❌ {} — socket bind failed: {}", label, e);
                    total_fail += 1;
                    continue;
                }
            };

            let local_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12345);
            let num_packets = 5;
            let mut rtts = Vec::new();
            let mut payload_matches = 0u32;

            for seq in 0..num_packets as u16 {
                let payload = format!("LIGHTSPEED_LIVE_TEST_{}", seq);
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u32;

                let header = TunnelHeader::new(seq, ts, local_addr, echo_addr);
                let packet = header.encode_with_payload(payload.as_bytes());

                let send_time = std::time::Instant::now();
                if socket.send_to(&packet, proxy).await.is_err() {
                    continue;
                }

                let mut buf = vec![0u8; 2048];
                match tokio::time::timeout(
                    Duration::from_millis(5000),
                    socket.recv_from(&mut buf),
                ).await {
                    Ok(Ok((len, _))) => {
                        let rtt = send_time.elapsed().as_micros() as u64;
                        rtts.push(rtt);

                        match TunnelHeader::decode_with_payload(&buf[..len]) {
                            Ok((_hdr, resp_payload)) => {
                                if resp_payload == payload.as_bytes() {
                                    payload_matches += 1;
                                }
                            }
                            Err(_) => {}
                        }
                    }
                    _ => {}
                }

                tokio::time::sleep(Duration::from_millis(100)).await;
            }

            if rtts.is_empty() {
                warn!("  ❌ {} → echo({}): 0/{} responses", label, echo_addr, num_packets);
                total_fail += 1;
            } else {
                let avg = rtts.iter().sum::<u64>() as f64 / rtts.len() as f64 / 1000.0;
                let min = *rtts.iter().min().unwrap() as f64 / 1000.0;
                let max = *rtts.iter().max().unwrap() as f64 / 1000.0;
                let status = if payload_matches == rtts.len() as u32 { "✅" } else { "⚠️" };
                info!(
                    "  {} {} → echo({}): {}/{} received, {}/{} payload match, avg={:.1}ms min={:.1}ms max={:.1}ms",
                    status, label, echo_addr, rtts.len(), num_packets,
                    payload_matches, rtts.len(), avg, min, max
                );
                if payload_matches > 0 {
                    total_pass += 1;
                } else {
                    total_fail += 1;
                }
            }
        }
    } else {
        info!("  ⏭️  Skipped — no echo server configured");
        info!("     Use --echo-server <ip:port> to test data relay");
        info!("     (Run tools/echo_server.py on a Vultr node first)");
        total_skip += 1;
    }

    // ── Phase 5: FEC Relay ──────────────────────────────────────
    info!("\n🔧 Phase 5: FEC Relay");
    info!("──────────────────────────────────────────────────────");

    if let Some(echo_addr) = echo_server {
        if fec_enabled {
            use lightspeed_protocol::{FecEncoder, FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};
            use bytes::BytesMut;

            for (label, proxy) in &proxy_addrs {
                let socket = match UdpSocket::bind("0.0.0.0:0").await {
                    Ok(s) => s,
                    Err(e) => {
                        warn!("  ❌ {} — socket bind failed: {}", label, e);
                        total_fail += 1;
                        continue;
                    }
                };

                let local_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 12345);
                let mut encoder = FecEncoder::new(fec_k);
                let mut responses = 0u32;
                let num_data_packets = fec_k as u16 * 2; // 2 full FEC blocks

                for seq in 0..num_data_packets {
                    let payload = format!("FEC_TEST_{}_{}", label, seq);
                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32;

                    let block_id = encoder.block_id();
                    let index = encoder.current_index();

                    let header = TunnelHeader::new_fec(seq, ts, local_addr, echo_addr);
                    let fec_hdr = FecHeader::data(block_id, index, fec_k);

                    let mut pkt = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + payload.len());
                    pkt.extend_from_slice(&header.encode());
                    fec_hdr.encode(&mut pkt);
                    pkt.extend_from_slice(payload.as_bytes());

                    let parity = encoder.add_packet(payload.as_bytes());

                    let _ = socket.send_to(&pkt, proxy).await;

                    // Send parity when block completes
                    if let Some(parity_bytes) = parity {
                        let parity_header = TunnelHeader::new_fec(
                            seq + 1000, ts, local_addr, echo_addr,
                        );
                        let parity_fec = FecHeader::parity(block_id, fec_k);
                        let mut parity_pkt = BytesMut::with_capacity(
                            HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len(),
                        );
                        parity_pkt.extend_from_slice(&parity_header.encode());
                        parity_fec.encode(&mut parity_pkt);
                        parity_pkt.extend_from_slice(&parity_bytes);
                        let _ = socket.send_to(&parity_pkt, proxy).await;
                    }

                    tokio::time::sleep(Duration::from_millis(50)).await;
                }

                // Collect responses
                let deadline = std::time::Instant::now() + Duration::from_secs(5);
                let mut buf = vec![0u8; 2048];
                while std::time::Instant::now() < deadline {
                    match tokio::time::timeout(
                        Duration::from_millis(500),
                        socket.recv_from(&mut buf),
                    ).await {
                        Ok(Ok((len, _))) => {
                            if TunnelHeader::decode(&buf[..len]).is_ok() {
                                responses += 1;
                            }
                        }
                        _ => break,
                    }
                }

                let blocks = num_data_packets / fec_k as u16;
                let total_sent = num_data_packets + blocks; // data + parity
                if responses > 0 {
                    info!(
                        "  ✅ {} FEC(K={}): sent {} data + {} parity, received {} responses",
                        label, fec_k, num_data_packets, blocks, responses
                    );
                    total_pass += 1;
                } else {
                    warn!(
                        "  ❌ {} FEC(K={}): sent {} packets, 0 responses",
                        label, fec_k, total_sent
                    );
                    total_fail += 1;
                }
            }
        } else {
            info!("  ⏭️  Skipped — FEC not enabled (use --fec to test)");
            total_skip += 1;
        }
    } else {
        info!("  ⏭️  Skipped — no echo server configured");
        total_skip += 1;
    }

    // ── Summary ─────────────────────────────────────────────────
    info!("\n══════════════════════════════════════════════════════");
    info!("📊 Live Integration Test Summary");
    info!("──────────────────────────────────────────────────────");
    info!("  Proxies tested:     {}", proxy_addrs.len());
    info!("  Healthy:            {}/{}", healthy_nodes.len(), nodes.len());
    if let Some(best) = nodes.iter().filter(|n| n.latency_us.is_some()).min_by_key(|n| n.latency_us) {
        info!("  Best latency:       {:.1}ms ({})", best.latency_us.unwrap() as f64 / 1000.0, best.id);
    }
    info!("  ────────────────────────────────────────────");
    info!("  ✅ Passed:  {}", total_pass);
    info!("  ❌ Failed:  {}", total_fail);
    info!("  ⏭️  Skipped: {}", total_skip);
    info!("──────────────────────────────────────────────────────");

    if total_fail == 0 {
        info!("  🎉 All tests passed! Live infrastructure verified.");
    } else {
        warn!("  ⚠️  {} test(s) failed — check proxy status", total_fail);
    }

    info!("══════════════════════════════════════════════════════");
    Ok(())
}
