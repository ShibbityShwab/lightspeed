//! # Security Integration Tests
//!
//! Tests security enforcement using the REAL relay engine:
//! - Authentication enforcement (accept/reject based on token)
//! - Rate limiting (PPS throttling)
//! - Abuse detection (private destination blocking, reflection detection)
//!
//! These tests spin up `run_relay_inbound` and verify behavior via
//! shared metrics counters (packets_relayed vs packets_dropped).

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use lightspeed_proxy::abuse::{AbuseConfig, AbuseDetector};
use lightspeed_proxy::auth::Authenticator;
use lightspeed_proxy::config::RateLimitConfig;
use lightspeed_proxy::metrics::ProxyMetrics;
use lightspeed_proxy::rate_limit::RateLimiter;
use lightspeed_proxy::relay::{self, RelayEngine};

use lightspeed_protocol::TunnelHeader;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tokio::task::JoinHandle;

// ── Helpers ─────────────────────────────────────────────────────────

fn now_us() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

fn to_v4(addr: SocketAddr) -> SocketAddrV4 {
    match addr {
        SocketAddr::V4(v4) => v4,
        _ => panic!("Expected IPv4"),
    }
}

/// Configuration for a test relay instance.
struct RelayTestConfig {
    require_auth: bool,
    max_pps: u64,
    max_bps: u64,
    abuse_config: AbuseConfig,
}

impl Default for RelayTestConfig {
    fn default() -> Self {
        Self {
            require_auth: false,
            max_pps: 10_000,
            max_bps: 10_000_000,
            abuse_config: AbuseConfig {
                max_amplification_ratio: 100.0,
                max_destinations_per_window: 100,
                ban_duration_secs: 3600,
                window_secs: 60,
            },
        }
    }
}

/// A running relay instance with accessible shared state.
struct TestRelay {
    data_addr: SocketAddrV4,
    metrics: Arc<ProxyMetrics>,
    authenticator: Arc<RwLock<Authenticator>>,
    engine: Arc<RelayEngine>,
    _handle: JoinHandle<()>,
}

/// Spin up a real relay inbound loop with the given config.
async fn start_relay(cfg: RelayTestConfig) -> TestRelay {
    let authenticator = Arc::new(RwLock::new(Authenticator::new(cfg.require_auth)));
    let abuse_detector = Arc::new(tokio::sync::Mutex::new(AbuseDetector::new(
        cfg.abuse_config,
    )));
    let rate_limiter = Arc::new(tokio::sync::Mutex::new(RateLimiter::new(RateLimitConfig {
        max_pps_per_client: cfg.max_pps,
        max_bps_per_client: cfg.max_bps,
        max_connections: 200,
    })));
    let metrics = Arc::new(ProxyMetrics::new());
    let engine = Arc::new(RelayEngine::new(100));

    let data_socket = Arc::new(UdpSocket::bind("127.0.0.1:0").await.unwrap());
    let data_addr = to_v4(data_socket.local_addr().unwrap());

    let handle = {
        let ds = Arc::clone(&data_socket);
        let eng = Arc::clone(&engine);
        let rl = Arc::clone(&rate_limiter);
        let auth = Arc::clone(&authenticator);
        let abuse = Arc::clone(&abuse_detector);
        let met = Arc::clone(&metrics);
        tokio::spawn(async move {
            let _ = relay::run_relay_inbound(ds, eng, rl, auth, abuse, met).await;
        })
    };

    TestRelay {
        data_addr,
        metrics,
        authenticator,
        engine,
        _handle: handle,
    }
}

/// Build a tunnel packet targeting a public IP.
fn make_public_packet(seq: u16, token: u8, src: SocketAddrV4) -> Vec<u8> {
    // Use a real public IP so abuse detector allows it
    let public_dest = SocketAddrV4::new(Ipv4Addr::new(93, 184, 216, 34), 80);
    let header = TunnelHeader::new(seq, now_us(), src, public_dest).with_session_token(token);
    header.encode_with_payload(b"test_payload").to_vec()
}

/// Build a tunnel packet targeting a private IP.
fn make_private_dest_packet(seq: u16, token: u8, src: SocketAddrV4, dest_ip: Ipv4Addr) -> Vec<u8> {
    let private_dest = SocketAddrV4::new(dest_ip, 22);
    let header = TunnelHeader::new(seq, now_us(), src, private_dest).with_session_token(token);
    header.encode_with_payload(b"test_payload").to_vec()
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_auth_rejects_unauthenticated() {
    let relay = start_relay(RelayTestConfig {
        require_auth: true,
        ..Default::default()
    })
    .await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    // No auth registered — send 5 packets, all should be dropped
    for seq in 0..5u16 {
        let packet = make_public_packet(seq, 0, client_addr);
        client.send_to(&packet, relay.data_addr).await.unwrap();
    }

    // Give the relay time to process
    tokio::time::sleep(Duration::from_millis(200)).await;

    let dropped = relay.metrics.packets_dropped.load(Ordering::Relaxed);
    let relayed = relay.metrics.packets_relayed.load(Ordering::Relaxed);

    assert_eq!(relayed, 0, "No packets should be relayed without auth");
    assert!(
        dropped >= 5,
        "All 5 packets should be dropped, got {}",
        dropped
    );
}

#[tokio::test]
async fn test_auth_accepts_valid_token() {
    let relay = start_relay(RelayTestConfig {
        require_auth: true,
        ..Default::default()
    })
    .await;

    // Authorize localhost with token 42
    {
        let mut auth = relay.authenticator.write().await;
        auth.authorize(Ipv4Addr::LOCALHOST, 42);
    }

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    // Send 3 packets with correct token
    for seq in 0..3u16 {
        let packet = make_public_packet(seq, 42, client_addr);
        client.send_to(&packet, relay.data_addr).await.unwrap();
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    let relayed = relay.metrics.packets_relayed.load(Ordering::Relaxed);
    let dropped = relay.metrics.packets_dropped.load(Ordering::Relaxed);

    assert!(
        relayed >= 3,
        "All 3 packets should be relayed, got {}",
        relayed
    );
    assert_eq!(dropped, 0, "No packets should be dropped");
    assert!(
        relay.engine.active_sessions().await >= 1,
        "Session should be created"
    );
}

#[tokio::test]
async fn test_invalid_token_rejected() {
    let relay = start_relay(RelayTestConfig {
        require_auth: true,
        ..Default::default()
    })
    .await;

    // Authorize with token 42
    {
        let mut auth = relay.authenticator.write().await;
        auth.authorize(Ipv4Addr::LOCALHOST, 42);
    }

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    // Send with WRONG token (99 instead of 42)
    for seq in 0..3u16 {
        let packet = make_public_packet(seq, 99, client_addr);
        client.send_to(&packet, relay.data_addr).await.unwrap();
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    let dropped = relay.metrics.packets_dropped.load(Ordering::Relaxed);
    let relayed = relay.metrics.packets_relayed.load(Ordering::Relaxed);

    assert_eq!(relayed, 0, "Wrong-token packets should not be relayed");
    assert!(
        dropped >= 3,
        "All wrong-token packets should be dropped, got {}",
        dropped
    );
}

#[tokio::test]
async fn test_rate_limit_drops_excess() {
    let relay = start_relay(RelayTestConfig {
        require_auth: false,
        max_pps: 5,       // Very low limit
        max_bps: 100_000, // High enough to not interfere
        ..Default::default()
    })
    .await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    // Send 15 packets rapidly — only 5 should be allowed per window
    for seq in 0..15u16 {
        let packet = make_public_packet(seq, 0, client_addr);
        client.send_to(&packet, relay.data_addr).await.unwrap();
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    let dropped = relay.metrics.packets_dropped.load(Ordering::Relaxed);
    let relayed = relay.metrics.packets_relayed.load(Ordering::Relaxed);

    // At most 5 should be relayed (rate limit window is 1 second)
    assert!(
        relayed <= 5,
        "Should relay at most 5 packets, got {}",
        relayed
    );
    assert!(
        dropped >= 10,
        "Should drop at least 10 packets, got {}",
        dropped
    );
}

#[tokio::test]
async fn test_private_destination_blocked() {
    let relay = start_relay(RelayTestConfig {
        require_auth: false,
        ..Default::default()
    })
    .await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    // Test various private destinations
    let private_ips = [
        Ipv4Addr::new(127, 0, 0, 1),   // Loopback
        Ipv4Addr::new(10, 0, 0, 1),    // RFC 1918
        Ipv4Addr::new(192, 168, 1, 1), // RFC 1918
        Ipv4Addr::new(172, 16, 0, 1),  // RFC 1918
        Ipv4Addr::new(169, 254, 1, 1), // Link-local
    ];

    for (i, &ip) in private_ips.iter().enumerate() {
        let packet = make_private_dest_packet(i as u16, 0, client_addr, ip);
        client.send_to(&packet, relay.data_addr).await.unwrap();
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    let dropped = relay.metrics.packets_dropped.load(Ordering::Relaxed);
    let relayed = relay.metrics.packets_relayed.load(Ordering::Relaxed);

    assert_eq!(relayed, 0, "No private-dest packets should be relayed");
    assert!(
        dropped >= private_ips.len() as u64,
        "All {} private-dest packets should be dropped, got {}",
        private_ips.len(),
        dropped
    );
}

#[tokio::test]
async fn test_reflection_detection_bans_client() {
    let relay = start_relay(RelayTestConfig {
        require_auth: false,
        abuse_config: AbuseConfig {
            max_destinations_per_window: 3, // Very low threshold
            max_amplification_ratio: 100.0,
            ban_duration_secs: 3600,
            window_secs: 60,
        },
        ..Default::default()
    })
    .await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    // Send to 5 different public destinations — should trigger reflection detection after 3
    for i in 1..=5u8 {
        let dest = SocketAddrV4::new(Ipv4Addr::new(93, 184, 216, i), 80);
        let header = TunnelHeader::new(i as u16, now_us(), client_addr, dest);
        let packet = header.encode_with_payload(b"reflect_test");
        client
            .send_to(&packet.to_vec(), relay.data_addr)
            .await
            .unwrap();
        // Small delay to ensure ordering
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    tokio::time::sleep(Duration::from_millis(200)).await;

    let dropped = relay.metrics.packets_dropped.load(Ordering::Relaxed);

    // First 3 allowed, 4th triggers ban + is dropped, 5th is banned
    assert!(
        dropped >= 2,
        "At least 2 packets should be dropped after reflection threshold, got {}",
        dropped
    );
}

#[tokio::test]
async fn test_malformed_packet_dropped() {
    let relay = start_relay(RelayTestConfig::default()).await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();

    // Send garbage data (too short to be a valid header)
    let garbage = b"not a tunnel packet";
    client.send_to(garbage, relay.data_addr).await.unwrap();

    // Send packet with invalid version
    let mut bad_version = vec![0u8; 20];
    bad_version[0] = 0xF0; // Version 15
    client.send_to(&bad_version, relay.data_addr).await.unwrap();

    // Send too-short packet
    client.send_to(&[1, 2, 3], relay.data_addr).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    let dropped = relay.metrics.packets_dropped.load(Ordering::Relaxed);
    assert!(
        dropped >= 3,
        "All malformed packets should be dropped, got {}",
        dropped
    );
}

#[tokio::test]
async fn test_relay_engine_session_creation() {
    // Verify sessions are created through the real relay path
    let relay = start_relay(RelayTestConfig {
        require_auth: false,
        ..Default::default()
    })
    .await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    assert_eq!(
        relay.engine.active_sessions().await,
        0,
        "No sessions initially"
    );

    // Send a packet — should create a session
    let packet = make_public_packet(1, 0, client_addr);
    client.send_to(&packet, relay.data_addr).await.unwrap();

    tokio::time::sleep(Duration::from_millis(200)).await;

    assert!(
        relay.engine.active_sessions().await >= 1,
        "Session should be created after first packet"
    );
}
