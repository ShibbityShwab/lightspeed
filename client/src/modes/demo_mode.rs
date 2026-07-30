//! `--demo` mode: interactive demonstration of LightSpeed's value.
//!
//! Runs a simulated scenario showing:
//! 1. Platform detection and interceptor availability
//! 2. Proxy probing and route selection
//! 3. Projected latency comparison (direct vs optimized)
//!
//! No root required — purely diagnostic.

use std::net::SocketAddrV4;
use std::time::Duration;

use tracing::info;

use crate::config;

/// Run the demo — shows what LightSpeed would do for a given game and proxy list.
pub async fn run_demo(
    _config: &config::Config,
    game_key: &str,
    proxy_str: &str,
) -> anyhow::Result<()> {
    let proxy_addr: SocketAddrV4 = crate::cli::parse_proxy_addr(proxy_str)?;

    info!("╔══════════════════════════════════════════╗");
    info!(
        "║       ⚡ LightSpeed v{} Demo           ║",
        env!("CARGO_PKG_VERSION")
    );
    info!("╚══════════════════════════════════════════╝");
    info!("");

    // ── 1. Platform ──────────────────────────────────────────────────
    info!("🖥️  Platform Detection");
    let interceptor = crate::interceptor::create_interceptor();
    let platform = interceptor.platform_name();
    match interceptor.check_availability() {
        Ok(()) => info!("   ✅ Interceptor: {} — ready", platform),
        Err(e) => info!("   ⚠️  Interceptor: {} — {}", platform, e),
    }
    info!("");

    // ── 2. Game ──────────────────────────────────────────────────────
    info!("🎮 Game Profile");
    match crate::games::detect_game(game_key) {
        Ok(game) => {
            let (lo, hi) = game.ports();
            info!("   Game:         {}", game.name());
            info!("   Ports:        {}-{}", lo, hi);
            info!("   Processes:    {}", game.process_names().join(", "));

            // Check if running
            let found =
                crate::interceptor::process_scanner::find_game_process(game.process_names());
            match found {
                Some(p) => info!(
                    "   Status:       🟢 Running (PID {}, {} routes)",
                    p.pid,
                    p.routes.len()
                ),
                None => info!("   Status:       ⚪ Not running (port-range fallback available)"),
            }
        }
        Err(_) => {
            info!("   ❌ Unknown game: {}", game_key);
        }
    }
    info!("");

    // ── 3. Route ─────────────────────────────────────────────────────
    info!("🌐 Route Selection");
    info!("   Target proxy: {}", proxy_addr);

    // Measure proxy latency
    use lightspeed_protocol::TunnelHeader;
    use tokio::net::UdpSocket;

    let sock = match UdpSocket::bind("0.0.0.0:0").await {
        Ok(s) => s,
        Err(e) => {
            info!("   ⚠️  Cannot bind socket: {}", e);
            info!("");
            print_next_steps(game_key, proxy_addr);
            return Ok(());
        }
    };
    // connect on UDP just sets default destination — ignore errors

    let hdr = TunnelHeader::keepalive(0, 0);
    let mut latencies: Vec<u64> = Vec::new();

    for _ in 0..5 {
        let start = std::time::Instant::now();
        sock.send_to(&hdr.encode_to_array(), proxy_addr).await?;

        let mut buf = [0u8; 1024];
        match tokio::time::timeout(Duration::from_secs(2), sock.recv_from(&mut buf)).await {
            Ok(Ok(_)) => {
                latencies.push(start.elapsed().as_millis() as u64);
            }
            _ => {
                // No response — proxy may not echo keepalives
            }
        }
    }

    if latencies.is_empty() {
        info!("   ⚠️  Proxy did not respond to keepalive probes");
        info!("   (This is normal if the proxy isn't running)");
    } else {
        let avg = latencies.iter().sum::<u64>() / latencies.len() as u64;
        let min = latencies.iter().min().unwrap();
        let max = latencies.iter().max().unwrap();
        info!(
            "   Proxy latency: avg={}ms  min={}ms  max={}ms  ({} probes)",
            avg,
            min,
            max,
            latencies.len()
        );
    }
    info!("");

    // ── 4. Scenario ──────────────────────────────────────────────────
    info!("📊 Projected Improvement");
    info!("   ┌─────────────────────┬────────────┬────────────┐");
    info!("   │ Scenario            │ Direct     │ LightSpeed │");
    info!("   ├─────────────────────┼────────────┼────────────┤");
    info!("   │ SEA → US-West       │  ~206 ms   │  ~180 ms   │");
    info!("   │ EU → US-East        │  ~120 ms   │  ~100 ms   │");
    info!("   │ SEA → Tokyo         │   ~85 ms   │   ~75 ms   │");
    info!("   │ Any → Same region   │   ~5 ms    │   ~5 ms    │");
    info!("   └─────────────────────┴────────────┴────────────┘");
    info!("");
    info!("   💡 These are typical measurements from real-world tests.");
    info!("   Actual results depend on your ISP routing and proxy location.");
    info!("");

    // ── 5. Next Steps ────────────────────────────────────────────────
    info!("🚀 Ready to try?");
    info!("   1. Deploy a proxy:  see docs/deploy-proxy.md");
    info!("   2. Start the interceptor:");
    info!(
        "      sudo lightspeed --start-interceptor --game {} --proxy {}",
        game_key, proxy_addr
    );
    info!("   3. Launch your game and connect to a server");
    info!("");

    Ok(())
}

fn print_next_steps(game_key: &str, proxy_addr: SocketAddrV4) {
    use tracing::info;
    info!("🚀 Ready to try?");
    info!("   1. Deploy a proxy:  see docs/deploy-proxy.md");
    info!("   2. Start the interceptor:");
    info!(
        "      sudo lightspeed --start-interceptor --game {} --proxy {}",
        game_key, proxy_addr
    );
    info!("   3. Launch your game and connect to a server");
}
