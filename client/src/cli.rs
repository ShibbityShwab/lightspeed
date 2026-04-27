//! CLI argument definitions and address parsing helpers.

use std::net::SocketAddrV4;

use clap::Parser;

/// LightSpeed — Reduce your ping. Free. Forever.
#[derive(Parser, Debug)]
#[command(name = "lightspeed", version, about, long_about = None)]
pub struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "lightspeed.toml")]
    pub config: String,

    /// Game to optimize (fortnite, cs2, dota2, rust)
    #[arg(short, long)]
    pub game: Option<String>,

    /// Proxy server address (host:port). If omitted, auto-selects from config.
    #[arg(short, long)]
    pub proxy: Option<String>,

    /// Enable verbose logging
    #[arg(short, long, default_value_t = false)]
    pub verbose: bool,

    /// Dry run — show what would happen without capturing packets
    #[arg(long, default_value_t = false)]
    pub dry_run: bool,

    /// Run tunnel test mode — send test packets to verify proxy connectivity
    #[arg(long, default_value_t = false)]
    pub test_tunnel: bool,

    /// Test QUIC control plane — connect, register, ping, disconnect
    #[arg(long, default_value_t = false)]
    pub test_control: bool,

    /// Game server address (ip:port) — enables redirect mode.
    /// Traffic to this server is tunneled through the proxy.
    #[arg(short = 's', long)]
    pub game_server: Option<String>,

    /// Local port for redirect mode (default: same as game server port)
    #[arg(long)]
    pub local_port: Option<u16>,

    /// Enable Forward Error Correction (FEC) for packet loss recovery.
    /// Adds ~25% bandwidth overhead but can recover any single lost packet
    /// per block of K. Much more efficient than ExitLag's packet duplication.
    #[arg(long, default_value_t = false)]
    pub fec: bool,

    /// FEC block size: number of data packets per parity packet (2-16, default 4).
    /// Lower K = more redundancy but more overhead. K=4 means 25% overhead.
    #[arg(long, default_value_t = 4)]
    pub fec_k: u8,

    /// Enable Cloudflare WARP for improved routing (5-10ms savings).
    /// Automatically connects WARP on startup and restores on shutdown.
    #[arg(short = 'w', long, default_value_t = false)]
    pub warp: bool,

    /// Disable WARP even if previously enabled
    #[arg(long, default_value_t = false)]
    pub no_warp: bool,

    /// Show WARP status and exit
    #[arg(long, default_value_t = false)]
    pub warp_status: bool,

    /// Route selection strategy: nearest, ml (default: from config or nearest)
    #[arg(long)]
    pub route_strategy: Option<String>,

    /// Probe all configured proxies and display latencies, then exit
    #[arg(long, default_value_t = false)]
    pub probe_proxies: bool,

    /// Run comprehensive live integration test against configured proxies.
    /// Tests health, route selection, keepalive echo, data relay, and FEC.
    #[arg(long, default_value_t = false)]
    pub live_test: bool,

    /// Echo server address for live data relay testing (e.g., YOUR_PROXY_IP:9999).
    /// Required for data relay and FEC phases of --live-test.
    #[arg(long)]
    pub echo_server: Option<String>,

    /// List available network interfaces for packet capture, then exit.
    #[arg(long, default_value_t = false)]
    pub list_interfaces: bool,

    /// Enable pcap capture mode (alternative to redirect mode).
    /// Captures game packets directly from the network interface.
    /// Requires the pcap-capture feature and elevated privileges.
    #[arg(long, default_value_t = false)]
    pub capture: bool,

    /// Network interface for capture mode (e.g., "eth0", "Ethernet").
    /// If omitted, uses the system default interface.
    #[arg(long)]
    pub interface: Option<String>,
}

/// Parse a proxy address string into `SocketAddrV4`.
///
/// Accepts `ip:port` directly or performs DNS resolution for `host:port`.
pub fn parse_proxy_addr(s: &str) -> anyhow::Result<SocketAddrV4> {
    // Try parsing as ip:port first (fast path)
    if let Ok(addr) = s.parse::<SocketAddrV4>() {
        return Ok(addr);
    }
    // Fall back to DNS resolution for hostname:port
    use std::net::ToSocketAddrs;
    let addrs: Vec<_> = s.to_socket_addrs()?.collect();
    for addr in &addrs {
        if let std::net::SocketAddr::V4(v4) = addr {
            return Ok(*v4);
        }
    }
    anyhow::bail!("Could not resolve proxy address: {}", s);
}
