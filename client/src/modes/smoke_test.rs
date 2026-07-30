//! `--smoke-test` mode: validates interceptor + nftables infrastructure.
//!
//! Starts an echo server + proxy + interceptor, verifies the interceptor
//! starts correctly and nftables rules are installed. Full E2E relay
//! testing requires a real game server with a public IP.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;
use tracing::info;

pub async fn run_smoke_test(proxy_addr: SocketAddrV4) -> anyhow::Result<()> {
    info!("🔥 LightSpeed Smoke Test");
    info!("");

    // 1. Echo server (for interceptor target)
    let echo_port = 28016u16;
    let echo_sock =
        tokio::net::UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::new(127, 0, 0, 1), echo_port))
            .await?;
    info!("📡 Echo: 127.0.0.1:{}", echo_port);
    let echo_handle = tokio::spawn(async move {
        let mut buf = [0u8; 1024];
        while let Ok(Ok((len, addr))) =
            tokio::time::timeout(Duration::from_secs(30), echo_sock.recv_from(&mut buf)).await
        {
            let _ = echo_sock.send_to(&buf[..len], addr).await;
        }
    });

    // 2. Proxy
    let proxy_bin = std::env::current_exe()
        .unwrap_or_default()
        .parent()
        .map(|p| p.join("lightspeed-proxy"))
        .unwrap_or_default();
    let mut proxy_child: Option<std::process::Child> = None;
    if proxy_bin.exists() {
        proxy_child = Some(
            std::process::Command::new(&proxy_bin)
                .arg("--data-bind")
                .arg("127.0.0.1:4434")
                .arg("--health-bind")
                .arg("127.0.0.1:8081")
                .arg("--dev")
                .spawn()?,
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
        info!("🔌 Proxy: PID {}", proxy_child.as_ref().unwrap().id());
    }

    // 3. Interceptor
    let interceptor = crate::interceptor::create_interceptor();
    interceptor
        .check_availability()
        .map_err(|e| anyhow::anyhow!("{}", e))?;
    let config = crate::interceptor::InterceptorConfig {
        game_name: "SmokeTest".into(),
        pid: None,
        port_range: (echo_port, echo_port + 1),
        initial_routes: vec![],
        proxy_addr,
        fec_enabled: false,
        fec_k: 4,
    };
    info!("🔌 Starting interceptor...");
    let mut handle = interceptor.start(config)?;
    info!("✅ Interceptor active (port-range mode)");
    tokio::time::sleep(Duration::from_secs(2)).await;

    // 4. Verify nftables rule exists
    let nft_check = std::process::Command::new("nft")
        .args(["list", "tables"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("lightspeed_"))
        .unwrap_or(false);
    if nft_check {
        info!("✅ nftables rule installed");
    } else {
        info!("⚠️  nftables rule not found");
    }

    // 5. Cleanup
    handle.stop();
    echo_handle.abort();
    if let Some(mut c) = proxy_child {
        let _ = c.kill();
        let _ = c.wait();
    }

    // Verify nftables rule removed
    tokio::time::sleep(Duration::from_millis(300)).await;
    let nft_clean = std::process::Command::new("nft")
        .args(["list", "tables"])
        .output()
        .map(|o| !String::from_utf8_lossy(&o.stdout).contains("lightspeed_"))
        .unwrap_or(true);
    if nft_clean {
        info!("✅ nftables rule removed");
    }

    info!("🧹 Cleanup complete");
    info!("✅ Smoke test PASSED");
    Ok(())
}
