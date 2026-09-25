//! Phase 0 cross-cutting integration tests: control plane → data plane.
//!
//! Each test runs the real proxy [`ControlServer`] (QUIC) wired to the real
//! [`relay::run_relay_inbound`] loop through one shared [`Authenticator`], so a
//! token issued by a registration is exactly the token the data plane validates.
//!
//! All traffic is loopback. The "game server" is a local UDP socket and the
//! abuse detector runs in dev mode so a private (loopback) destination is
//! allowed; destination validation is orthogonal to the auth/grace behavior
//! under test.
//!
//! Clone/reset/revoke timing is observed through deterministic `Instant` seams:
//! the auth table compares deadlines against a caller-supplied `now`, so a test
//! can look into a grace window without sleeping it out.

#![cfg(feature = "quic")]

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Once};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lightspeed_protocol::control::{disconnect_reason, game_id, ControlMessage};
use lightspeed_protocol::{TunnelHeader, PROTOCOL_VERSION};
use lightspeed_proxy::abuse::{AbuseConfig, AbuseDetector};
use lightspeed_proxy::auth::{Authenticator, PREVIOUS_TOKEN_GRACE, TRANSPORT_REVOKE_GRACE};
use lightspeed_proxy::config::{ProxyConfig, RateLimitConfig};
use lightspeed_proxy::control::{ControlServer, ControlState};
use lightspeed_proxy::metrics::ProxyMetrics;
use lightspeed_proxy::rate_limit::RateLimiter;
use lightspeed_proxy::relay::{self, RelayEngine};
use tokio::net::UdpSocket;
use tokio::sync::{watch, RwLock};

// ── TLS and QUIC client helpers ─────────────────────────────────────

static TLS_ONCE: Once = Once::new();

/// Point every `ControlServer` in this test binary at one temp TLS dir and
/// generate its cert once, serially, so parallel tests only ever read it.
fn isolate_tls_dir() {
    TLS_ONCE.call_once(|| {
        let dir =
            std::env::temp_dir().join(format!("lightspeed-phase0-tls-{}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create TLS temp dir");
        std::env::set_var("LIGHTSPEED_TLS_DIR", &dir);

        // The first bind generates and persists cert.der/key.der. Doing it here
        // means later parallel binds only read the pair.
        let state = Arc::new(ControlState::new(
            ProxyConfig::default(),
            Arc::new(RwLock::new(Authenticator::new(true))),
            Arc::new(ProxyMetrics::new()),
        ));
        let primer = ControlServer::bind("127.0.0.1:0".parse().expect("bind addr"), state)
            .expect("prime the TLS certificate");
        drop(primer);
    });
}

/// A QUIC client endpoint that accepts the proxy's self-signed certificate.
fn make_client_endpoint() -> anyhow::Result<quinn::Endpoint> {
    #[derive(Debug)]
    struct SkipVerify;

    impl rustls::client::danger::ServerCertVerifier for SkipVerify {
        fn verify_server_cert(
            &self,
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &[rustls::pki_types::CertificateDer<'_>],
            _: &rustls::pki_types::ServerName<'_>,
            _: &[u8],
            _: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }
        fn verify_tls12_signature(
            &self,
            _: &[u8],
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn verify_tls13_signature(
            &self,
            _: &[u8],
            _: &rustls::pki_types::CertificateDer<'_>,
            _: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }
        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    let crypto = rustls::ClientConfig::builder()
        .dangerous()
        .with_custom_certificate_verifier(Arc::new(SkipVerify))
        .with_no_client_auth();
    let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)?;
    let client_config = quinn::ClientConfig::new(Arc::new(quic_crypto));

    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse::<SocketAddr>()?)?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

/// A minimal control client: connect, register, close.
struct TestClient {
    _endpoint: quinn::Endpoint,
    conn: quinn::Connection,
}

impl TestClient {
    async fn connect(addr: SocketAddr) -> anyhow::Result<Self> {
        let endpoint = make_client_endpoint()?;
        let conn = endpoint.connect(addr, "lightspeed-proxy")?.await?;
        Ok(Self {
            _endpoint: endpoint,
            conn,
        })
    }

    /// Register `data_port` and return the server-issued data-plane token.
    async fn register(&self, data_port: u16) -> anyhow::Result<u32> {
        let (mut send, mut recv) = self.conn.open_bi().await?;
        ControlMessage::Register {
            protocol_version: PROTOCOL_VERSION,
            game: game_id::CS2,
            data_port,
        }
        .write_to(&mut send)
        .await?;
        match ControlMessage::read_from(&mut recv).await? {
            Some(ControlMessage::RegisterAck { session_token, .. }) => {
                send.finish()?;
                Ok(session_token)
            }
            other => anyhow::bail!("expected RegisterAck, got {other:?}"),
        }
    }

    fn close(&self) {
        self.conn.close(0u32.into(), b"phase0");
    }
}

// ── Proxy harness ───────────────────────────────────────────────────

/// A running relay + control server sharing one authenticator.
struct ProxyHarness {
    metrics: Arc<ProxyMetrics>,
    auth: Arc<RwLock<Authenticator>>,
    data_addr: SocketAddrV4,
    control_addr: SocketAddr,
    shutdown_tx: watch::Sender<bool>,
    server: tokio::task::JoinHandle<()>,
}

impl ProxyHarness {
    async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.server).await;
    }
}

async fn start_proxy() -> ProxyHarness {
    isolate_tls_dir();

    let config = ProxyConfig::default();
    let auth = Arc::new(RwLock::new(Authenticator::new(true)));
    let metrics = Arc::new(ProxyMetrics::new());
    let engine = Arc::new(RelayEngine::new(config.server.max_clients));
    let rate_limiter = Arc::new(tokio::sync::Mutex::new(RateLimiter::new(
        RateLimitConfig::default(),
    )));
    let abuse = Arc::new(tokio::sync::Mutex::new(AbuseDetector::new(AbuseConfig {
        dev_mode: true,
        max_amplification_ratio: 100.0,
        max_destinations_per_window: 100,
        ban_duration_secs: 3600,
        window_secs: 60,
        destination_allowlist: Vec::new(),
    })));

    let data_socket = Arc::new(
        UdpSocket::bind("127.0.0.1:0")
            .await
            .expect("bind relay data socket"),
    );
    let data_addr = to_v4(data_socket.local_addr().expect("relay data addr"));

    {
        let data_socket = Arc::clone(&data_socket);
        let engine = Arc::clone(&engine);
        let rate_limiter = Arc::clone(&rate_limiter);
        let auth = Arc::clone(&auth);
        let abuse = Arc::clone(&abuse);
        let metrics = Arc::clone(&metrics);
        tokio::spawn(async move {
            let _ =
                relay::run_relay_inbound(data_socket, engine, rate_limiter, auth, abuse, metrics)
                    .await;
        });
    }

    let state = Arc::new(ControlState::new(
        config,
        Arc::clone(&auth),
        Arc::clone(&metrics),
    ));
    let server = ControlServer::bind("127.0.0.1:0".parse().expect("control addr"), state)
        .expect("bind control server");
    let control_addr = server.local_addr().expect("control local addr");
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let server = tokio::spawn(server.run(shutdown_rx));

    ProxyHarness {
        metrics,
        auth,
        data_addr,
        control_addr,
        shutdown_tx,
        server,
    }
}

// ── Loopback data-plane helpers ─────────────────────────────────────

/// A local client + game-server pair driving the real data plane.
struct Loopback {
    client: UdpSocket,
    game: UdpSocket,
    relay: SocketAddrV4,
}

impl Loopback {
    async fn new(relay: SocketAddrV4) -> anyhow::Result<Self> {
        Ok(Self {
            client: UdpSocket::bind("127.0.0.1:0").await?,
            game: UdpSocket::bind("127.0.0.1:0").await?,
            relay,
        })
    }

    /// The client's data-plane source port, reported to the control server.
    fn data_port(&self) -> u16 {
        self.client.local_addr().expect("client data addr").port()
    }

    /// Send one authenticated data-plane packet carrying `phase0-<seq>`.
    async fn send(&self, seq: u16, token: u32) {
        let src = to_v4(self.client.local_addr().expect("client data addr"));
        let game = to_v4(self.game.local_addr().expect("game server addr"));
        let payload = format!("phase0-{seq}");
        let packet = TunnelHeader::new(seq, now_us(), src, game)
            .with_session_token(token)
            .encode_with_payload(payload.as_bytes())
            .to_vec();
        self.client
            .send_to(&packet, self.relay)
            .await
            .expect("send data-plane packet");
    }

    /// Assert the relay forwards exactly `phase0-<seq>` to the game server.
    async fn expect_forwarded(&self, seq: u16) {
        let mut buf = [0u8; 512];
        let (n, _) = tokio::time::timeout(Duration::from_secs(5), self.game.recv_from(&mut buf))
            .await
            .expect("relay forwarded no packet in time")
            .expect("game server recv");
        let expected = format!("phase0-{seq}");
        assert_eq!(
            &buf[..n],
            expected.as_bytes(),
            "relay forwarded the wrong payload"
        );
    }
}

fn to_v4(addr: SocketAddr) -> SocketAddrV4 {
    match addr {
        SocketAddr::V4(v4) => v4,
        SocketAddr::V6(_) => panic!("expected an IPv4 loopback address"),
    }
}

fn now_us() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

/// Validate `token` for loopback at the caller-supplied instant.
async fn authorized(auth: &Arc<RwLock<Authenticator>>, port: u16, token: u32, at: Instant) -> bool {
    auth.read()
        .await
        .validate(Ipv4Addr::LOCALHOST, port, token, at)
}

/// Wait (bounded) until auth has rejected at least `at_least` packets.
async fn wait_for_auth_rejections(metrics: &Arc<ProxyMetrics>, at_least: u64) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let got = metrics.auth_rejections.load(Ordering::Relaxed);
        if got >= at_least {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {at_least} auth rejections (got {got})"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Total telemetry reports folded across every aggregation cell.
fn telemetry_report_count(metrics: &Arc<ProxyMetrics>) -> u64 {
    metrics
        .telemetry
        .lock()
        .unwrap()
        .cells
        .values()
        .map(|cell| cell.reports)
        .sum()
}

// ── Tests ───────────────────────────────────────────────────────────

/// A client registers, relays a packet, loses the control connection, and
/// re-registers. Its data-plane packets are accepted under both the old token
/// (inside the transport grace) and the new token, with zero auth rejections.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnect_preserves_auth_and_resumes_data() -> anyhow::Result<()> {
    let h = start_proxy().await;
    let plane = Loopback::new(h.data_addr).await?;
    let data_port = plane.data_port();

    let client_a = TestClient::connect(h.control_addr).await?;
    let token_a = client_a.register(data_port).await?;
    assert!(token_a > 0, "registration must issue a non-zero token");

    plane.send(1, token_a).await;
    plane.expect_forwarded(1).await;
    assert_eq!(h.metrics.auth_rejections.load(Ordering::Relaxed), 0);

    // Lose the control connection; the server revokes token A with the 120 s
    // transport grace rather than instantly.
    client_a.close();

    // Deterministic seam: before the revoke token A's deadline is ~300 s out;
    // after it, ~120 s. A probe at now + 121 s flips valid → invalid exactly
    // when the revoke lands, without sleeping the grace out.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let probe = Instant::now() + TRANSPORT_REVOKE_GRACE + Duration::from_secs(1);
        if !authorized(&h.auth, data_port, token_a, probe).await {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the transport-close revoke was never applied"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }

    // Inside the grace the old token still validates: the data plane survives a
    // transient control-plane loss with no outage.
    assert!(
        authorized(&h.auth, data_port, token_a, Instant::now()).await,
        "the old token must stay valid inside the transport grace"
    );
    plane.send(2, token_a).await;
    plane.expect_forwarded(2).await;

    // Re-register on a fresh control connection; data resumes under the new token.
    let client_b = TestClient::connect(h.control_addr).await?;
    let token_b = client_b.register(data_port).await?;
    assert_ne!(token_b, token_a, "re-registration must issue a fresh token");
    plane.send(3, token_b).await;
    plane.expect_forwarded(3).await;

    assert_eq!(
        h.metrics.auth_rejections.load(Ordering::Relaxed),
        0,
        "no packet may be auth-rejected across the reconnect"
    );

    client_b.close();
    h.shutdown().await;
    Ok(())
}

/// Two clients behind one source IP on distinct source ports both authenticate;
/// revoking one token leaves the other authorized and relaying.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn nat_peers_are_isolated() -> anyhow::Result<()> {
    let h = start_proxy().await;
    let plane_a = Loopback::new(h.data_addr).await?;
    let plane_b = Loopback::new(h.data_addr).await?;
    let port_a = plane_a.data_port();
    let port_b = plane_b.data_port();
    assert_ne!(
        port_a, port_b,
        "the two peers must use distinct source ports"
    );

    let client_a = TestClient::connect(h.control_addr).await?;
    let token_a = client_a.register(port_a).await?;
    let client_b = TestClient::connect(h.control_addr).await?;
    let token_b = client_b.register(port_b).await?;
    assert!(token_a > 0 && token_b > 0 && token_a != token_b);

    plane_a.send(1, token_a).await;
    plane_b.send(1, token_b).await;
    plane_a.expect_forwarded(1).await;
    plane_b.expect_forwarded(1).await;
    assert_eq!(h.metrics.auth_rejections.load(Ordering::Relaxed), 0);

    // Revoke only A. B must remain authorized.
    h.auth
        .write()
        .await
        .revoke(token_a, Instant::now(), Duration::ZERO);

    let future = Instant::now() + Duration::from_secs(1);
    assert!(
        !authorized(&h.auth, port_a, token_a, future).await,
        "the revoked token must be invalid"
    );
    assert!(
        authorized(&h.auth, port_b, token_b, future).await,
        "revoking one NAT peer must not deauthorize the other"
    );

    // Across the real data plane: A is rejected, B still relays.
    plane_a.send(2, token_a).await;
    wait_for_auth_rejections(&h.metrics, 1).await;

    plane_b.send(2, token_b).await;
    plane_b.expect_forwarded(2).await;
    assert_eq!(
        h.metrics.auth_rejections.load(Ordering::Relaxed),
        1,
        "B's packet must not be auth-rejected"
    );

    client_a.close();
    client_b.close();
    h.shutdown().await;
    Ok(())
}

/// After a same-principal re-registration the previous token still validates
/// inside the 30 s grace and is rejected after it; the new token outlives it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn previous_token_accepted_within_grace() -> anyhow::Result<()> {
    let h = start_proxy().await;
    let plane = Loopback::new(h.data_addr).await?;
    let port = plane.data_port();

    let client_old = TestClient::connect(h.control_addr).await?;
    let token_old = client_old.register(port).await?;
    // A second registration from the same principal (loopback) demotes the
    // previous token to a 30 s window.
    let client_new = TestClient::connect(h.control_addr).await?;
    let token_new = client_new.register(port).await?;
    assert_ne!(token_old, token_new);

    let now = Instant::now();
    let after_grace = now + PREVIOUS_TOKEN_GRACE + Duration::from_secs(1);
    assert!(
        authorized(&h.auth, port, token_old, now).await,
        "the previous token must validate inside the 30 s grace"
    );
    assert!(
        !authorized(&h.auth, port, token_old, after_grace).await,
        "the previous token must be rejected after the 30 s grace"
    );
    assert!(
        authorized(&h.auth, port, token_new, after_grace).await,
        "the new token must outlive the previous token's grace"
    );

    // End to end: the old token still relays during the grace window, then the
    // new token relays too.
    plane.send(1, token_old).await;
    plane.expect_forwarded(1).await;
    plane.send(2, token_new).await;
    plane.expect_forwarded(2).await;
    assert_eq!(h.metrics.auth_rejections.load(Ordering::Relaxed), 0);

    client_old.close();
    client_new.close();
    h.shutdown().await;
    Ok(())
}

/// A registered control client observes `Disconnect { SERVER_SHUTDOWN }` when
/// the server is asked to stop.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn graceful_shutdown_announces_disconnect() -> anyhow::Result<()> {
    let h = start_proxy().await;
    let client = TestClient::connect(h.control_addr).await?;
    let token = client.register(0).await?;
    assert!(token > 0);

    // Deterministic seam: flip the shutdown watch; no sleep.
    h.shutdown_tx.send(true)?;

    // The server opens a fresh server-initiated stream carrying the Disconnect.
    let (_send, mut recv) = tokio::time::timeout(Duration::from_secs(5), client.conn.accept_bi())
        .await
        .expect("server opened no shutdown stream in time")?;
    let msg = tokio::time::timeout(Duration::from_secs(5), ControlMessage::read_from(&mut recv))
        .await
        .expect("no shutdown message in time")?;
    assert_eq!(
        msg,
        Some(ControlMessage::Disconnect {
            reason: disconnect_reason::SERVER_SHUTDOWN,
        })
    );

    client.close();
    let _ = tokio::time::timeout(Duration::from_secs(5), h.server).await;
    Ok(())
}

/// A well-formed report body matching the HTTP `/telemetry` schema.
const VALID_TELEMETRY_JSON: &[u8] = br#"{"game_id":2,"client_country":"US","p50_ms":30.0,"p95_ms":50.0,"p99_ms":80.0,"jitter_ms":2.0,"sample_count":100,"fec_recoveries":1,"fec_losses":0,"client_version":"1.6.9"}"#;

/// Given: a registered client on the authenticated QUIC control connection.
/// When: it sends an anonymised telemetry report as a control message. Then:
/// the proxy ingests it on the encrypted path and folds it into the same
/// `(game, country)` cell the HTTP endpoint feeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn telemetry_travels_over_control_plane() -> anyhow::Result<()> {
    let h = start_proxy().await;
    let client = TestClient::connect(h.control_addr).await?;
    let token = client.register(0).await?;
    assert!(token > 0, "registration must issue a non-zero token");

    let (mut send, _recv) = client.conn.open_bi().await?;
    ControlMessage::Telemetry {
        report_json: VALID_TELEMETRY_JSON.to_vec(),
    }
    .write_to(&mut send)
    .await?;
    send.finish()?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while telemetry_report_count(&h.metrics) < 1 {
        assert!(
            Instant::now() < deadline,
            "telemetry was not ingested over the control plane"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }

    {
        let agg = h.metrics.telemetry.lock().unwrap();
        let cell = agg.cells.values().next().expect("one telemetry cell");
        assert_eq!(cell.reports, 1, "one report must be folded");
        assert_eq!(cell.samples, 100, "sample_count must be carried unchanged");
        assert_eq!(cell.p50_sum_ms, 30.0, "p50 must be carried unchanged");
    }

    client.close();
    h.shutdown().await;
    Ok(())
}

/// Given: a registered control client. When: it sends an oversized telemetry
/// body followed by a valid one on the same stream. Then: the oversized body
/// is dropped by the size cap and only the valid report is aggregated, so the
/// control path is capped exactly like the HTTP endpoint.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn control_plane_telemetry_rejects_oversized_body() -> anyhow::Result<()> {
    let h = start_proxy().await;
    let client = TestClient::connect(h.control_addr).await?;
    let token = client.register(0).await?;
    assert!(token > 0);

    // Just over the HTTP endpoint's 2048-byte body cap.
    let oversized = format!(
        r#"{{"game_id":2,"client_country":"US","p50_ms":30.0,"p95_ms":50.0,"p99_ms":80.0,"jitter_ms":2.0,"sample_count":100,"client_version":"{}"}}"#,
        "x".repeat(2_100)
    );

    // Same stream, so the server processes them in order: oversized first
    // (dropped), then the valid report (aggregated once).
    let (mut send, _recv) = client.conn.open_bi().await?;
    ControlMessage::Telemetry {
        report_json: oversized.into_bytes(),
    }
    .write_to(&mut send)
    .await?;
    ControlMessage::Telemetry {
        report_json: VALID_TELEMETRY_JSON.to_vec(),
    }
    .write_to(&mut send)
    .await?;
    send.finish()?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while telemetry_report_count(&h.metrics) < 1 {
        assert!(Instant::now() < deadline, "valid report was not ingested");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let reports = telemetry_report_count(&h.metrics);
    assert_eq!(
        reports, 1,
        "the oversized body must be dropped, not aggregated"
    );

    client.close();
    h.shutdown().await;
    Ok(())
}
