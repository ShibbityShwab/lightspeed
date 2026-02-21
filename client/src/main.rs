//! # LightSpeed Client
//!
//! Zero-cost global network optimizer for multiplayer games.
//! Captures game UDP packets and tunnels them through optimally-selected
//! proxy nodes to reduce latency via better routing paths.

mod config;
mod tunnel;
mod route;
mod capture;
mod quic;
mod ml;
mod games;
mod error;

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

    // Detect or select game
    let game = match cli.game.as_deref() {
        Some(name) => {
            info!("Game selected: {}", name);
            games::detect_game(name)?
        }
        None => {
            info!("Auto-detecting running game...");
            games::auto_detect()?
        }
    };

    info!(
        "🎮 Targeting game: {} (ports: {:?})",
        game.name(),
        game.ports()
    );

    if cli.dry_run {
        info!("Dry run mode — showing configuration and exiting");
        info!("Config: {:?}", config);
        info!("Game: {}", game.name());
        return Ok(());
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

    // ── Main tunnel loop ──────────────────────────────────────────
    //
    // For now (without pcap capture), we run in "passthrough" mode:
    // - Keepalive loop to maintain proxy session
    // - Stats logging
    // - Waiting for pcap capture integration (Step 3)
    //
    // Once capture is wired in, the flow will be:
    //   CaptureTask → channel → TunnelSendTask → proxy
    //   proxy → TunnelRecvTask → channel → InjectTask → game

    info!("⚡ LightSpeed tunnel active — keepalive mode");
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
                                stats.bytes_received.fetch_add(len as u64, Ordering::Relaxed);

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
async fn run_tunnel_test(relay: UdpRelay, proxy_addr: SocketAddrV4) -> anyhow::Result<()> {
    info!("🧪 Running tunnel test to {}", proxy_addr);

    let test_src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
    let test_dst = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 7777);
    let test_payload = b"LightSpeed tunnel test packet";

    // Send 5 test packets
    for i in 0..5u16 {
        match relay.send_to_proxy(test_payload, test_src, test_dst, proxy_addr).await {
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

    info!("   ✓ Connected! session={:?} node={:?} region={:?}",
        client.session_id(),
        client.node_id(),
        client.region(),
    );

    // Ping test (5 pings)
    info!("   Step 2: Sending 5 pings...");
    let mut rtts = Vec::new();
    for i in 0..5 {
        match client.ping().await {
            Ok(rtt_us) => {
                info!("   ✓ Ping #{}: {}μs ({:.2}ms)", i + 1, rtt_us, rtt_us as f64 / 1000.0);
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
        info!("   📊 RTT: avg={}μs min={}μs max={}μs ({} samples)",
            avg, min, max, rtts.len()
        );
    }

    // Disconnect
    info!("   Step 3: Disconnecting...");
    client.disconnect().await
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
