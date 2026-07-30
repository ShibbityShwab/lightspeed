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
    /// List supported games with their default ports, then exit.
    #[arg(long, default_value_t = false)]
    pub list_games: bool,

    /// Write a default lightspeed.toml config file to the current directory.
    #[arg(long, default_value_t = false)]
    pub write_config: bool,

    /// Run environment checks (nftables/iptables, proxy, game detection).
    /// Exits 0 if all pass, 1 otherwise.
    #[arg(long, default_value_t = false)]
    pub check: bool,

    /// Show detailed system state (OS, interceptor, games, nftables rules).
    #[arg(long, default_value_t = false)]
    pub status: bool,

    /// Run an interactive demonstration of LightSpeed architecture and latency projections.
    #[arg(long, default_value_t = false)]
    pub demo: bool,

    /// Run a full E2E smoke test (starts echo server + interceptor).
    /// Validates the entire pipeline. Requires root for nftables.
    #[arg(long, default_value_t = false)]
    pub smoke_test: bool,

    /// Watch for game process and auto-start interceptor when detected.
    /// Requires --game and --proxy. Ctrl+C to stop.
    #[arg(long, default_value_t = false)]
    pub watch: bool,

    /// Run latency benchmark comparing direct vs LightSpeed routing.
    /// Requires --target and --proxy.
    #[arg(long, default_value_t = false)]
    pub benchmark: bool,

    /// Target server for --benchmark (ip:port).
    #[arg(long)]
    pub target: Option<String>,

    /// Enable pcap capture mode (alternative to redirect mode).
    /// Captures game packets directly from the network interface.
    /// Requires the pcap-capture feature and elevated privileges.
    #[arg(long, default_value_t = false)]
    pub capture: bool,

    /// Network interface for capture mode (e.g., "eth0", "Ethernet").
    /// If omitted, uses the system default interface.
    #[arg(long)]
    pub interface: Option<String>,

    /// Run the OOP TrafficInterceptor in test mode.
    /// Discovers the game process via ProcessScanner and reports the
    /// available interceptor backend, discovered routes, and availability.
    /// Does not start the interceptor (use --game + redirect mode for that).
    #[arg(long, default_value_t = false)]
    pub intercept: bool,

    /// Start the OOP TrafficInterceptor in live MITM mode.
    /// Requires --game and --proxy. Installs kernel-level redirect rules
    /// (nftables/iptables/pfctl/WinDivert) and tunnels game traffic.
    /// Press Ctrl+C to stop and clean up firewall rules.
    /// Requires elevated privileges: root on Linux/macOS, Admin on Windows.
    #[arg(long, default_value_t = false)]
    pub start_interceptor: bool,

    /// Override the game server address for interceptor mode.
    /// Useful for testing when the game is not running.
    /// Format: ip:port (e.g., 1.2.3.4:28015)
    #[arg(long)]
    pub server_addr: Option<String>,

    /// Scan for game processes and display their UDP routes, then exit.
    /// Useful for debugging game detection before starting the interceptor.
    #[arg(long, default_value_t = false)]
    pub scan_processes: bool,

    /// Enable opt-in anonymous telemetry.
    /// Sends aggregated latency stats (p50/p95/p99, jitter, FEC) to the proxy
    /// every 15 min. No IP address or PII is ever sent. See docs/privacy.md.
    #[arg(long, default_value_t = false)]
    pub telemetry: bool,

    /// Disable telemetry even if enabled in the config file.
    #[arg(long, default_value_t = false)]
    pub no_telemetry: bool,
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;

    #[test]
    fn test_default_values() {
        let cli = Cli::try_parse_from(["lightspeed"]).unwrap();
        assert_eq!(cli.config, "lightspeed.toml");
        assert!(!cli.verbose);
        assert!(!cli.dry_run);
        assert!(!cli.test_tunnel);
        assert!(!cli.test_control);
        assert!(!cli.fec);
        assert_eq!(cli.fec_k, 4);
        assert!(!cli.warp);
        assert!(!cli.no_warp);
        assert!(!cli.warp_status);
        assert!(!cli.probe_proxies);
        assert!(!cli.live_test);
        assert!(!cli.list_interfaces);
        assert!(!cli.list_games);
        assert!(!cli.write_config);
        assert!(!cli.check);
        assert!(!cli.status);
        assert!(!cli.demo);
        assert!(!cli.smoke_test);
        assert!(!cli.watch);
        assert!(!cli.benchmark);
        assert!(!cli.capture);
        assert!(!cli.intercept);
        assert!(!cli.start_interceptor);
        assert!(!cli.scan_processes);
        assert!(!cli.telemetry);
        assert!(!cli.no_telemetry);
        assert!(cli.game.is_none());
        assert!(cli.proxy.is_none());
        assert!(cli.game_server.is_none());
        assert!(cli.local_port.is_none());
        assert!(cli.route_strategy.is_none());
        assert!(cli.echo_server.is_none());
        assert!(cli.interface.is_none());
    }

    #[test]
    fn test_short_flags() {
        let cli = Cli::try_parse_from(["lightspeed", "-v"]).unwrap();
        assert!(cli.verbose);

        let cli = Cli::try_parse_from(["lightspeed", "-g", "fortnite"]).unwrap();
        assert_eq!(cli.game.as_deref(), Some("fortnite"));

        let cli = Cli::try_parse_from(["lightspeed", "-p", "10.0.0.1:4434"]).unwrap();
        assert_eq!(cli.proxy.as_deref(), Some("10.0.0.1:4434"));

        let cli = Cli::try_parse_from(["lightspeed", "-s", "192.168.1.1:27015"]).unwrap();
        assert_eq!(cli.game_server.as_deref(), Some("192.168.1.1:27015"));

        let cli = Cli::try_parse_from(["lightspeed", "-w"]).unwrap();
        assert!(cli.warp);
    }

    #[test]
    fn test_long_flags() {
        let cli = Cli::try_parse_from([
            "lightspeed",
            "--verbose",
            "--dry-run",
            "--test-tunnel",
            "--test-control",
            "--fec",
            "--no-warp",
            "--warp-status",
            "--probe-proxies",
            "--live-test",
            "--list-interfaces",
            "--list-games",
            "--write-config",
            "--check",
            "--status",
            "--demo",
            "--smoke-test",
            "--watch",
            "--benchmark",
            "--capture",
            "--intercept",
            "--start-interceptor",
            "--scan-processes",
            "--telemetry",
            "--no-telemetry",
        ])
        .unwrap();
        assert!(cli.verbose);
        assert!(cli.dry_run);
        assert!(cli.test_tunnel);
        assert!(cli.test_control);
        assert!(cli.fec);
        assert!(cli.no_warp);
        assert!(cli.warp_status);
        assert!(cli.probe_proxies);
        assert!(cli.live_test);
        assert!(cli.list_interfaces);
        assert!(cli.list_games);
        assert!(cli.write_config);
        assert!(cli.check);
        assert!(cli.status);
        assert!(cli.demo);
        assert!(cli.smoke_test);
        assert!(cli.watch);
        assert!(cli.benchmark);
        assert!(cli.capture);
        assert!(cli.intercept);
        assert!(cli.start_interceptor);
        assert!(cli.scan_processes);
        assert!(cli.telemetry);
        assert!(cli.no_telemetry);
    }

    #[test]
    fn test_long_flags_with_values() {
        let cli = Cli::try_parse_from([
            "lightspeed",
            "--config",
            "custom.toml",
            "--game",
            "cs2",
            "--proxy",
            "10.0.0.1:4434",
            "--game-server",
            "1.2.3.4:7777",
            "--local-port",
            "8888",
            "--fec-k",
            "8",
            "--route-strategy",
            "ml",
            "--echo-server",
            "10.0.0.1:9999",
            "--interface",
            "eth0",
        ])
        .unwrap();
        assert_eq!(cli.config, "custom.toml");
        assert_eq!(cli.game.as_deref(), Some("cs2"));
        assert_eq!(cli.proxy.as_deref(), Some("10.0.0.1:4434"));
        assert_eq!(cli.game_server.as_deref(), Some("1.2.3.4:7777"));
        assert_eq!(cli.local_port, Some(8888));
        assert_eq!(cli.fec_k, 8);
        assert_eq!(cli.route_strategy.as_deref(), Some("ml"));
        assert_eq!(cli.echo_server.as_deref(), Some("10.0.0.1:9999"));
        assert_eq!(cli.interface.as_deref(), Some("eth0"));
    }

    #[test]
    fn test_game_values() {
        for game in &["fortnite", "cs2", "dota2", "rust"] {
            let cli = Cli::try_parse_from(["lightspeed", "-g", game]).unwrap();
            assert_eq!(cli.game.as_deref(), Some(*game));
        }
    }

    #[test]
    fn test_fec_k_boundaries() {
        // default is 4
        let cli = Cli::try_parse_from(["lightspeed"]).unwrap();
        assert_eq!(cli.fec_k, 4);

        let cli = Cli::try_parse_from(["lightspeed", "--fec-k", "2"]).unwrap();
        assert_eq!(cli.fec_k, 2);

        let cli = Cli::try_parse_from(["lightspeed", "--fec-k", "16"]).unwrap();
        assert_eq!(cli.fec_k, 16);
    }

    #[test]
    fn test_combined_mode_flags() {
        // Redirect mode with game server
        let cli = Cli::try_parse_from([
            "lightspeed",
            "-g",
            "fortnite",
            "-s",
            "10.0.0.1:7777",
            "--fec",
            "--warp",
            "--route-strategy",
            "nearest",
        ])
        .unwrap();
        assert_eq!(cli.game.as_deref(), Some("fortnite"));
        assert_eq!(cli.game_server.as_deref(), Some("10.0.0.1:7777"));
        assert!(cli.fec);
        assert!(cli.warp);
        assert_eq!(cli.route_strategy.as_deref(), Some("nearest"));
    }

    #[test]
    fn test_capture_mode() {
        let cli = Cli::try_parse_from([
            "lightspeed",
            "-g",
            "cs2",
            "--capture",
            "--interface",
            "Ethernet",
        ])
        .unwrap();
        assert_eq!(cli.game.as_deref(), Some("cs2"));
        assert!(cli.capture);
        assert_eq!(cli.interface.as_deref(), Some("Ethernet"));
    }

    #[test]
    fn test_live_test_mode() {
        let cli = Cli::try_parse_from([
            "lightspeed",
            "--live-test",
            "--echo-server",
            "10.0.0.1:9999",
            "--proxy",
            "10.0.0.1:4434",
        ])
        .unwrap();
        assert!(cli.live_test);
        assert_eq!(cli.echo_server.as_deref(), Some("10.0.0.1:9999"));
        assert_eq!(cli.proxy.as_deref(), Some("10.0.0.1:4434"));
    }

    #[test]
    fn test_warp_flags() {
        let cli = Cli::try_parse_from(["lightspeed", "-w"]).unwrap();
        assert!(cli.warp);
        assert!(!cli.no_warp);

        let cli = Cli::try_parse_from(["lightspeed", "--no-warp"]).unwrap();
        assert!(!cli.warp);
        assert!(cli.no_warp);

        let cli = Cli::try_parse_from(["lightspeed", "--warp-status"]).unwrap();
        assert!(cli.warp_status);
    }

    #[test]
    fn test_telemetry_flags() {
        let cli = Cli::try_parse_from(["lightspeed", "--telemetry"]).unwrap();
        assert!(cli.telemetry);
        assert!(!cli.no_telemetry);

        let cli = Cli::try_parse_from(["lightspeed", "--no-telemetry"]).unwrap();
        assert!(!cli.telemetry);
        assert!(cli.no_telemetry);
    }

    #[test]
    fn test_route_strategies() {
        for strategy in &["nearest", "ml", "multipath"] {
            let cli = Cli::try_parse_from(["lightspeed", "--route-strategy", strategy]).unwrap();
            assert_eq!(cli.route_strategy.as_deref(), Some(*strategy));
        }
    }

    // ── parse_proxy_addr tests ───────────────────────────────────────

    #[test]
    fn test_parse_proxy_addr_ipv4() {
        let addr = parse_proxy_addr("192.168.1.1:4434").unwrap();
        assert_eq!(addr.ip().octets(), [192, 168, 1, 1]);
        assert_eq!(addr.port(), 4434);
    }

    #[test]
    fn test_parse_proxy_addr_loopback() {
        let addr = parse_proxy_addr("127.0.0.1:8080").unwrap();
        assert_eq!(addr.ip().octets(), [127, 0, 0, 1]);
        assert_eq!(addr.port(), 8080);
    }

    #[test]
    fn test_parse_proxy_addr_invalid_ip() {
        let result = parse_proxy_addr("not-an-ip:4434");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_proxy_addr_missing_port() {
        let result = parse_proxy_addr("192.168.1.1");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_proxy_addr_invalid_port() {
        let result = parse_proxy_addr("192.168.1.1:99999");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_proxy_addr_empty() {
        let result = parse_proxy_addr("");
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_proxy_addr_garbage() {
        let result = parse_proxy_addr("garbage");
        assert!(result.is_err());
    }
}
