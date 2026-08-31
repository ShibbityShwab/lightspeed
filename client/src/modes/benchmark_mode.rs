//! `--benchmark` mode: direct vs LightSpeed latency comparison.
//!
//! Sends interleaved probes to an echo server (`--target`) both directly and
//! tunnelled through the proxy, then compares medians so first-mile jitter
//! (e.g. on a hotspot) affects both routes equally.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
use tracing::info;

const PROBES: usize = 30;

pub async fn run_benchmark(target: SocketAddrV4, proxy: SocketAddrV4) -> anyhow::Result<()> {
    use lightspeed_protocol::TunnelHeader;
    use tokio::net::UdpSocket;

    info!("📊 LightSpeed Latency Benchmark");
    info!("   Echo target: {}  Proxy: {}", target, proxy);
    info!("   {} interleaved probes per route (median reported)", PROBES);
    info!("");

    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    let token = crate::session::session_token();
    let local = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);

    let mut direct = Vec::with_capacity(PROBES);
    let mut relay = Vec::with_capacity(PROBES);

    // Interleave direct and via-relay probes so first-mile drift affects both
    // routes equally, then compare medians rather than means.
    for i in 0..PROBES {
        let seq = i as u16;
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u32;
        let mut buf = [0u8; 2048];

        // Direct: keepalive straight to the echo server.
        let start = Instant::now();
        let keepalive = TunnelHeader::keepalive(seq, ts).with_session_token(token);
        let _ = sock.send_to(&keepalive.encode_to_array(), target).await;
        if let Ok(Ok(_)) =
            tokio::time::timeout(Duration::from_secs(3), sock.recv_from(&mut buf)).await
        {
            direct.push(start.elapsed().as_micros() as u64);
        }

        // Via relay: full tunnel packet whose destination is the echo server.
        let start = Instant::now();
        let tunnel = TunnelHeader::new(seq, ts, local, target).with_session_token(token);
        let packet = tunnel.encode_with_payload(b"benchmark");
        let _ = sock.send_to(&packet, proxy).await;
        if let Ok(Ok(_)) =
            tokio::time::timeout(Duration::from_secs(3), sock.recv_from(&mut buf)).await
        {
            relay.push(start.elapsed().as_micros() as u64);
        }

        tokio::time::sleep(Duration::from_millis(150)).await;
    }

    info!("");
    report("Direct", &direct);
    report("LightSpeed (via relay)", &relay);

    if direct.len() >= 5 && relay.len() >= 5 {
        let dm = median(&direct);
        let rm = median(&relay);
        info!("");
        info!("┌───────────────────────┬───────────┬───────────┐");
        info!("│ Median                │ {:>7} ms │ {:>7} ms │", dm / 1000, rm / 1000);
        info!(
            "│ Packet loss           │ {:>7}/{} │ {:>7}/{} │",
            PROBES - direct.len(),
            PROBES,
            PROBES - relay.len(),
            PROBES
        );
        if rm < dm {
            info!("│ Saving                │     -     │ {:>7} ms │", (dm - rm) / 1000);
        } else if dm < rm {
            info!("│ Penalty               │     -     │ {:>7} ms │", (rm - dm) / 1000);
        }
        info!("└───────────────────────┴───────────┴───────────┘");
    } else {
        info!("⚠️  Not enough responses. Is the echo server reachable (run tools/echo_server.py)?");
    }
    Ok(())
}

fn median(v: &[u64]) -> u64 {
    if v.is_empty() {
        return 0;
    }
    let mut s = v.to_vec();
    s.sort_unstable();
    let mid = s.len() / 2;
    if s.len().is_multiple_of(2) {
        (s[mid - 1] + s[mid]) / 2
    } else {
        s[mid]
    }
}

fn report(label: &str, lats: &[u64]) {
    if lats.is_empty() {
        info!("{label}: no response");
        return;
    }
    let mut v = lats.to_vec();
    v.sort_unstable();
    let p90 = v[(v.len() - 1) * 90 / 100];
    info!(
        "{label}: median={}ms min={}ms p90={}ms ({}/{} received)",
        median(&v) / 1000,
        v[0] / 1000,
        p90 / 1000,
        lats.len(),
        PROBES
    );
}
