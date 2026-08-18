//! `--benchmark` mode: direct vs LightSpeed latency comparison.

use std::net::SocketAddrV4;
use std::time::{Duration, Instant};
use tracing::info;

const PROBES: usize = 10;

pub async fn run_benchmark(target: SocketAddrV4, proxy: SocketAddrV4) -> anyhow::Result<()> {
    use lightspeed_protocol::TunnelHeader;
    use tokio::net::UdpSocket;

    info!("📊 LightSpeed Latency Benchmark");
    info!("   Target: {}  Proxy: {}", target, proxy);
    info!("");

    let sock = UdpSocket::bind("0.0.0.0:0").await?;
    let hdr = TunnelHeader::keepalive(0, 0).with_session_token(crate::session::session_token());

    // Direct
    info!("🔴 Direct route:");
    let mut direct = Vec::new();
    for _ in 0..PROBES {
        let start = Instant::now();
        let _ = sock.send_to(&hdr.encode_to_array(), target).await;
        let mut buf = [0u8; 1024];
        if let Ok(Ok(_)) =
            tokio::time::timeout(Duration::from_secs(3), sock.recv_from(&mut buf)).await
        {
            direct.push(start.elapsed().as_millis() as u64)
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    stats("   Direct", &direct);

    // LightSpeed (via proxy keepalive echo)
    info!("🟢 LightSpeed route:");
    let mut ls = Vec::new();
    for _ in 0..PROBES {
        let start = Instant::now();
        let _ = sock.send_to(&hdr.encode_to_array(), proxy).await;
        let mut buf = [0u8; 1024];
        if let Ok(Ok(_)) =
            tokio::time::timeout(Duration::from_secs(3), sock.recv_from(&mut buf)).await
        {
            ls.push(start.elapsed().as_millis() as u64)
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    stats("   LightSpeed", &ls);

    info!("");
    if direct.len() >= 3 && ls.len() >= 3 {
        let da = direct.iter().sum::<u64>() / direct.len() as u64;
        let la = ls.iter().sum::<u64>() / ls.len() as u64;
        info!("┌──────────────┬──────────┬──────────┐");
        info!("│ Metric       │ Direct   │ LightSpeed│");
        info!("├──────────────┼──────────┼──────────┤");
        info!("│ Avg          │ {:>6} ms │ {:>6} ms │", da, la);
        info!(
            "│ Min          │ {:>6} ms │ {:>6} ms │",
            direct.iter().min().unwrap(),
            ls.iter().min().unwrap()
        );
        info!(
            "│ Max          │ {:>6} ms │ {:>6} ms │",
            direct.iter().max().unwrap(),
            ls.iter().max().unwrap()
        );
        if la < da {
            let save = da - la;
            info!("│ Saving       │    —     │ {:>6} ms │", save);
            info!(
                "│ Improvement  │    —     │   {:>5}% │",
                (save as f64 / da as f64 * 100.0) as u64
            );
        }
        info!("└──────────────┴──────────┴──────────┘");
    } else {
        info!("⚠️  Not enough responses. Is the target/proxy reachable?");
    }
    Ok(())
}

fn stats(label: &str, lats: &[u64]) {
    if lats.is_empty() {
        info!("{label}: no response");
        return;
    }
    let avg = lats.iter().sum::<u64>() / lats.len() as u64;
    info!(
        "{label}: avg={avg}ms min={}ms max={}ms ({} probes)",
        lats.iter().min().unwrap(),
        lats.iter().max().unwrap(),
        lats.len()
    );
}
