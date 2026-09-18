//! # Upstream Latency Integration Test
//!
//! Proves the proxy records a real latency sample for a genuine round trip:
//! a client packet is forwarded to an echo server, the echo response is read
//! back on the same session socket, and that proxy-observed upstream response
//! lag is recorded in the latency histogram.
//!
//! Before the latency fix `record_latency` had no production caller, so
//! `relay_latency_count` stayed zero even though responses were relayed.

use std::net::{SocketAddr, SocketAddrV4};
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
                dev_mode: false,
                max_amplification_ratio: 100.0,
                max_destinations_per_window: 100,
                ban_duration_secs: 3600,
                window_secs: 60,
                destination_allowlist: Vec::new(),
            },
        }
    }
}

/// A running relay instance with accessible shared state.
struct TestRelay {
    data_addr: SocketAddrV4,
    metrics: Arc<ProxyMetrics>,
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
        _handle: handle,
    }
}

/// Build a tunnel packet targeting `dest`.
fn make_packet(seq: u16, token: u32, src: SocketAddrV4, dest: SocketAddrV4) -> Vec<u8> {
    let header = TunnelHeader::new(seq, now_us(), src, dest).with_session_token(token);
    header.encode_with_payload(b"latency_probe").to_vec()
}

/// A local UDP echo server.  It deliberately waits before echoing so the
/// monotonic round trip is measurably non-zero even on loopback, where a raw
/// round trip can complete in under a microsecond.
async fn start_echo_server() -> (SocketAddrV4, JoinHandle<()>) {
    let socket = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let addr = to_v4(socket.local_addr().unwrap());
    let handle = tokio::spawn(async move {
        let mut buf = [0u8; 2048];
        while let Ok((len, peer)) = socket.recv_from(&mut buf).await {
            tokio::time::sleep(Duration::from_millis(2)).await;
            let _ = socket.send_to(&buf[..len], peer).await;
        }
    });
    (addr, handle)
}

// ── Test ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_upstream_latency_recorded() {
    // dev_mode allows the loopback echo server as a destination.
    let relay = start_relay(RelayTestConfig {
        require_auth: false,
        abuse_config: AbuseConfig {
            dev_mode: true,
            max_amplification_ratio: 100.0,
            max_destinations_per_window: 100,
            ban_duration_secs: 3600,
            window_secs: 60,
            destination_allowlist: Vec::new(),
        },
        ..Default::default()
    })
    .await;

    let (echo_addr, echo_handle) = start_echo_server().await;

    let client = UdpSocket::bind("127.0.0.1:0").await.unwrap();
    let client_addr = to_v4(client.local_addr().unwrap());

    let packet = make_packet(1, 0, client_addr, echo_addr);
    client.send_to(&packet, relay.data_addr).await.unwrap();

    // The wrapped echo response must come back — this is the real round trip.
    let mut buf = [0u8; 2048];
    let received = tokio::time::timeout(Duration::from_secs(2), client.recv_from(&mut buf)).await;
    assert!(
        received.is_ok(),
        "expected a wrapped response from the relay within 2s"
    );
    received.unwrap().unwrap();

    let count = relay.metrics.relay_latency_count.load(Ordering::Relaxed);
    let sum = relay.metrics.relay_latency_sum_us.load(Ordering::Relaxed);

    assert!(
        count >= 1,
        "a real upstream round trip must record a latency sample, got count={count}"
    );
    assert!(
        sum > 0 && sum <= 2_000_000,
        "latency sum must be a sane positive value, got sum={sum}"
    );

    echo_handle.abort();
}
