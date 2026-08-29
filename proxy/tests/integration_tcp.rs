//! # TCP Relay Integration Tests
//!
//! Tests the client→proxy TCP leg of the data plane: length-prefixed framing,
//! relay to a game (echo) server, authentication enforcement, and frame-size
//! limits.  All traffic flows through the real [`relay::run_tcp_inbound`].

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use lightspeed_protocol::framing::{read_frame, write_frame, MAX_FRAME_SIZE};
use lightspeed_protocol::TunnelHeader;

use lightspeed_proxy::abuse::{AbuseConfig, AbuseDetector};
use lightspeed_proxy::auth::Authenticator;
use lightspeed_proxy::config::RateLimitConfig;
use lightspeed_proxy::metrics::ProxyMetrics;
use lightspeed_proxy::rate_limit::RateLimiter;
use lightspeed_proxy::relay::{self, RelayEngine};

use tokio::io::AsyncWriteExt;
use tokio::net::{TcpStream, UdpSocket};
use tokio::sync::RwLock;

fn now_us() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

fn to_v4(addr: SocketAddr) -> SocketAddrV4 {
    match addr {
        SocketAddr::V4(v4) => v4,
        _ => panic!("expected IPv4"),
    }
}

/// A running TCP relay instance with accessible shared state.
struct TestTcpRelay {
    tcp_addr: SocketAddrV4,
    metrics: Arc<ProxyMetrics>,
    engine: Arc<RelayEngine>,
}

async fn start_tcp_relay(require_auth: bool, dev_mode: bool) -> TestTcpRelay {
    let authenticator = Arc::new(RwLock::new(Authenticator::new(require_auth)));
    let abuse_detector = Arc::new(tokio::sync::Mutex::new(AbuseDetector::new(AbuseConfig {
        dev_mode,
        max_amplification_ratio: 100.0,
        max_destinations_per_window: 100,
        ban_duration_secs: 3600,
        window_secs: 60,
        destination_allowlist: Vec::new(),
    })));
    let rate_limiter = Arc::new(tokio::sync::Mutex::new(RateLimiter::new(RateLimitConfig {
        max_pps_per_client: 10_000,
        max_bps_per_client: 10_000_000,
        max_connections: 200,
    })));
    let metrics = Arc::new(ProxyMetrics::new());
    let engine = Arc::new(RelayEngine::new(100));

    // Reserve and release a port so `run_tcp_inbound` can bind the same one.
    let probe = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let tcp_addr = to_v4(probe.local_addr().unwrap());
    drop(probe);

    let eng = Arc::clone(&engine);
    let rl = Arc::clone(&rate_limiter);
    let auth = Arc::clone(&authenticator);
    let abuse = Arc::clone(&abuse_detector);
    let met = Arc::clone(&metrics);

    tokio::spawn(async move {
        let _ = relay::run_tcp_inbound(
            std::net::SocketAddr::V4(tcp_addr),
            eng,
            rl,
            auth,
            abuse,
            met,
            256,
            Duration::from_secs(10),
        )
        .await;
    });

    TestTcpRelay {
        tcp_addr,
        metrics,
        engine,
    }
}

async fn connect_with_retry(addr: SocketAddrV4) -> TcpStream {
    for _ in 0..50 {
        if let Ok(stream) = TcpStream::connect(addr).await {
            return stream;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("failed to connect to TCP relay");
}

#[tokio::test]
async fn test_tcp_relay_roundtrip() {
    let relay = start_tcp_relay(false, true).await;

    // UDP echo server acting as the game server.
    let echo = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let echo_addr = to_v4(echo.local_addr().unwrap());
    let echo_task = tokio::spawn({
        let echo = Arc::clone(&echo);
        async move {
            let mut buf = vec![0u8; 2048];
            while let Ok((len, addr)) = echo.recv_from(&mut buf).await {
                let _ = echo.send_to(&buf[..len], addr).await;
            }
        }
    });

    let mut stream = connect_with_retry(relay.tcp_addr).await;
    let client_addr = to_v4(stream.local_addr().unwrap());

    let header = TunnelHeader::new(1, now_us(), client_addr, echo_addr);
    let packet = header.encode_with_payload(b"tcp relay roundtrip");
    write_frame(&mut stream, &packet).await.unwrap();

    let mut buf = Vec::new();
    let n = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut stream, &mut buf))
        .await
        .expect("timeout waiting for framed response")
        .unwrap()
        .unwrap();

    let (resp_header, resp_payload) = TunnelHeader::decode_with_payload(&buf[..n]).unwrap();
    assert_eq!(resp_payload, b"tcp relay roundtrip");
    assert_eq!(resp_header.orig_src_addr(), echo_addr);
    assert_eq!(resp_header.orig_dst_addr(), client_addr);

    echo_task.abort();
}

#[tokio::test]
async fn test_tcp_unauthenticated_rejected() {
    let relay = start_tcp_relay(true, true).await;

    let mut stream = connect_with_retry(relay.tcp_addr).await;
    let client_addr = to_v4(stream.local_addr().unwrap());

    // Token 0 with require_auth=true and no registration — must be dropped.
    let public_dest = SocketAddrV4::new(Ipv4Addr::new(93, 184, 216, 34), 80);
    let header = TunnelHeader::new(1, now_us(), client_addr, public_dest);
    let packet = header.encode_with_payload(b"unauthorized");
    write_frame(&mut stream, &packet).await.unwrap();

    tokio::time::sleep(Duration::from_millis(300)).await;

    assert_eq!(
        relay.metrics.packets_relayed.load(Ordering::Relaxed),
        0,
        "no packets should be relayed without auth"
    );
    assert!(
        relay.metrics.packets_dropped.load(Ordering::Relaxed) >= 1,
        "unauthenticated packet should be dropped"
    );
}

#[tokio::test]
async fn test_tcp_oversized_frame_disconnected() {
    let relay = start_tcp_relay(false, true).await;

    let mut stream = connect_with_retry(relay.tcp_addr).await;

    // Send a length prefix larger than the cap (no payload follows).
    let huge = (MAX_FRAME_SIZE as u32 + 1).to_be_bytes();
    stream.write_all(&huge).await.unwrap();

    // The relay must reject the frame and close the connection. Reading back
    // yields EOF (or a reset), never a valid frame.
    let mut buf = Vec::new();
    let result = tokio::time::timeout(Duration::from_secs(2), read_frame(&mut stream, &mut buf))
        .await
        .expect("timeout — relay should have closed the connection");
    match result {
        Ok(None) | Err(_) => {}
        Ok(Some(_)) => panic!("received a frame despite oversized length"),
    }
    assert_eq!(relay.engine.active_sessions().await, 0);
}
