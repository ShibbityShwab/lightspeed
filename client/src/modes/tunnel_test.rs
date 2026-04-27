//! `--test-tunnel` mode: send test packets to a proxy and verify the round-trip.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::time::Duration;

use tracing::{info, warn};

use crate::error;
use crate::tunnel::relay::UdpRelay;

/// Run a tunnel test: send 5 test packets to `proxy_addr` and report results.
pub async fn run_tunnel_test(mut relay: UdpRelay, proxy_addr: SocketAddrV4) -> anyhow::Result<()> {
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
            Ok(sent) => info!("  ✓ Sent test packet #{} ({} bytes)", i + 1, sent),
            Err(e) => warn!("  ✗ Failed to send test packet #{}: {}", i + 1, e),
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }

    // Wait for responses (up to 3 s)
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
            Err(error::TunnelError::Timeout(_)) => break,
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
