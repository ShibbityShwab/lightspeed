//! `--test-control` mode: exercise the QUIC control plane end-to-end.

use std::net::{SocketAddr, SocketAddrV4};
use std::time::Duration;

use tracing::{info, warn};

use crate::config;
use crate::quic;

/// Run a QUIC control-plane test: connect → register → ping × 5 → disconnect.
pub async fn run_control_test(
    proxy_addr: SocketAddrV4,
    config: &config::Config,
) -> anyhow::Result<()> {
    info!("🧪 Running QUIC control plane test");

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
            Err(e) => warn!("   ✗ Ping #{} failed: {}", i + 1, e),
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    // RTT stats
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
