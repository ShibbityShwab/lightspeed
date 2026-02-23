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

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use tracing::{info, warn};

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

    /// Proxy server address (host:port)
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

    // Determine proxy address
    let proxy_addr = if let Some(ref proxy_str) = cli.proxy {
        parse_proxy_addr(proxy_str)?
    } else if !config.proxy.servers.is_empty() {
        parse_proxy_addr(&config.proxy.servers[0])?
    } else {
        // Default proxy for development/testing
        let default = SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), 4434);
        warn!("No proxy specified, using default: {}", default);
        default
    };

    info!("🌐 Proxy: {}", proxy_addr);

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

    // Spawn keepalive sender
    let keepalive_handle = {
        let relay_socket = relay.socket().expect("socket bound");
        let proxy = proxy_addr;
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

    // Spawn response receiver (logs keepalive echoes and responses)
    let recv_handle = {
        let stats = Arc::clone(&stats);
        let relay_socket = relay.socket().expect("socket bound");
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
                                    tracing::trace!(
                                        seq = header.sequence,
                                        from = %addr,
                                        "Keepalive echo received"
                                    );
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

    info!("⚡ LightSpeed shut down cleanly");
    Ok(())
}

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
