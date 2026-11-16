//! Cross-crate ALPN interop: the real client offer/fallback against a real
//! `ControlServer`.
//!
//! This is tier 2 of docs/quic-alpn-rollout.md §6.4 (plan §13 row C). The unit
//! tests in `client/src/quic/mod.rs` and `proxy/src/control.rs` prove each side
//! of the ALPN contract in isolation; this file proves the shipped
//! `lightspeed-client` `ControlClient` and the shipped
//! `lightspeed-proxy` `ControlServer` agree on it over real QUIC handshakes
//! on loopback.
//!
//! The three tests are the compatibility-matrix rows that carry the rollout
//! order (docs/quic-alpn-rollout.md §3):
//!
//! - row C -> C' (`an_off_relay_rescues_the_new_client_with_the_empty_offer_retry`):
//!   a new client offers `lightspeed-control/1`, an `off` relay rejects the
//!   handshake with TLS alert 120 (`no_application_protocol`), and the client's
//!   one-shot empty-offer retry registers end to end. This is plan falsifier F2.
//! - row D (`a_required_relay_negotiates_the_shared_name`): the same client
//!   against a `required` relay registers and the client observes
//!   `lightspeed-control/1` as the negotiated protocol.
//! - row B (`a_required_relay_rejects_an_alpn_less_client_with_120`): a raw
//!   quinn client that offers no ALPN is rejected with `crypto(0x78)` -- the
//!   population a `required` flip breaks, proven across the crate boundary.
//!
//! The test binary runs in the `QUIC Integration Tests` CI job:
//! `cargo test -p lightspeed-client -p lightspeed-proxy --features quic --tests`
//! (.github/workflows/ci.yml). It lives in `proxy/tests/` and drives the client
//! through `lightspeed_client::test_support`, the `#[doc(hidden)]` module that
//! exists for integration tests because the client's `quic` module is
//! crate-private; no policy knob is added to the client for it.

#![cfg(feature = "quic")]

use std::net::SocketAddr;
use std::sync::{Arc, Once};
use std::time::Duration;

use lightspeed_client::test_support::{install_crypto_provider, ControlClient};
use lightspeed_protocol::{game_id, QUIC_CONTROL_ALPN_V1};
use lightspeed_proxy::auth::Authenticator;
use lightspeed_proxy::config::{ControlAlpn, ProxyConfig};
use lightspeed_proxy::control::{ControlServer, ControlState};
use lightspeed_proxy::metrics::{ControlHandshakeOutcome, ProxyMetrics};
use tokio::sync::{watch, RwLock};

/// Bound on one connect+register so a stuck test fails fast instead of
/// spending the CI job's 15-minute budget.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(20);

/// How long the relay-side counters may take to reflect a handshake the client
/// already completed: the accept task records the outcome asynchronously.
const COUNTER_TIMEOUT: Duration = Duration::from_secs(10);

/// One shared per-process setup: a temp TLS dir (so parallel `ControlServer`
/// binds share one generated certificate) and a temp `HOME` (so the client's
/// TOFU pin store is per-run and an ephemeral port reused by a later run cannot
/// hit a stale pin).
static TEST_ENV: Once = Once::new();

fn isolate_test_env() {
    TEST_ENV.call_once(|| {
        // The proxy dev-dependency pulls in lightspeed-client with `quic`, so
        // this test binary can see both rustls providers; pin ring before any
        // config builder runs or rustls panics on auto-detection.
        install_crypto_provider();

        let base =
            std::env::temp_dir().join(format!("lightspeed-alpn-interop-{}", std::process::id()));
        let tls = base.join("tls");
        let home = base.join("home");
        std::fs::create_dir_all(&tls).expect("create the TLS temp dir");
        std::fs::create_dir_all(&home).expect("create the test HOME");
        std::env::set_var("LIGHTSPEED_TLS_DIR", &tls);
        std::env::set_var("HOME", &home);

        // Generate and persist the cert once, serially; parallel tests then only
        // read the pair.
        let primer = ControlServer::bind(loopback(), control_state(ControlAlpn::Off))
            .expect("prime the control-plane TLS certificate");
        drop(primer);
    });
}

fn loopback() -> SocketAddr {
    "127.0.0.1:0".parse().expect("loopback address")
}

/// A relay state in one ALPN policy, with auth enforced (the shipped default).
fn control_state(mode: ControlAlpn) -> Arc<ControlState> {
    let mut config = ProxyConfig::default();
    config.network.control_alpn = mode;
    Arc::new(ControlState::new(
        config,
        Arc::new(RwLock::new(Authenticator::new(true))),
        Arc::new(ProxyMetrics::new()),
        None,
    ))
}

/// A real relay: bound `ControlServer`, its accept loop running, one ALPN mode.
struct TestRelay {
    addr: SocketAddr,
    metrics: Arc<ProxyMetrics>,
    shutdown_tx: watch::Sender<bool>,
    task: tokio::task::JoinHandle<()>,
}

impl TestRelay {
    async fn start(mode: ControlAlpn) -> Self {
        isolate_test_env();
        let state = control_state(mode);
        let metrics = Arc::clone(&state.metrics);
        let server = ControlServer::bind(loopback(), state).expect("bind the control server");
        let addr = server.local_addr().expect("bound control address");
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let task = tokio::spawn(server.run(shutdown_rx));
        Self {
            addr,
            metrics,
            shutdown_tx,
            task,
        }
    }

    async fn shutdown(self) {
        let _ = self.shutdown_tx.send(true);
        let _ = tokio::time::timeout(Duration::from_secs(5), self.task).await;
    }
}

/// Wait (bounded) until the relay has counted exactly `want` handshakes with
/// `outcome`, returning the observed total.
async fn wait_for_outcome(
    metrics: &ProxyMetrics,
    outcome: ControlHandshakeOutcome,
    want: u64,
) -> u64 {
    let deadline = tokio::time::Instant::now() + COUNTER_TIMEOUT;
    loop {
        let got = metrics.control_handshake_count(outcome);
        if got == want {
            return got;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "timed out waiting for {} control handshake(s) with outcome={}, saw {}",
            want,
            outcome.as_str(),
            got
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
}

/// Row C -> C': a new client's versioned offer is rejected by a relay that
/// predates the rollout, and the one-shot empty-offer retry still registers.
///
/// This is the compatibility guarantee the client-first order rests on
/// (docs/quic-alpn-rollout.md §3, plan falsifier F2). The relay counters prove
/// both handshakes happened: one `rejected_no_application_protocol`, one
/// `no_alpn`. If this test fails, the client cannot self-heal against the
/// installed relay base.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_off_relay_rescues_the_new_client_with_the_empty_offer_retry() {
    let relay = TestRelay::start(ControlAlpn::Off).await;
    let mut client = ControlClient::new().expect("create the control client");

    tokio::time::timeout(
        CONNECT_TIMEOUT,
        client.connect_with_destination(relay.addr, game_id::UNKNOWN, None),
    )
    .await
    .expect("row C -> C': the fallback handshake must not hang")
    .expect("row C -> C': the empty-offer retry must rescue an off relay");

    assert!(
        client.is_connected(),
        "row C': the retry must leave a live control connection"
    );
    assert!(
        client.session_id().is_some(),
        "row C': the retry must register a session"
    );
    assert!(
        client.session_token().is_some(),
        "row C': registration must issue a data-plane token"
    );
    assert!(
        client.negotiated_alpn().is_none(),
        "row C': an off relay accepts the retry with no ALPN, got {:?}",
        client.negotiated_alpn()
    );

    wait_for_outcome(
        &relay.metrics,
        ControlHandshakeOutcome::RejectedNoApplicationProtocol,
        1,
    )
    .await;
    wait_for_outcome(&relay.metrics, ControlHandshakeOutcome::NoAlpn, 1).await;
    assert_eq!(
        relay
            .metrics
            .control_handshake_count(ControlHandshakeOutcome::NegotiatedV1),
        0,
        "row C': no handshake against an off relay may be counted as negotiated"
    );

    let _ = client.disconnect().await;
    relay.shutdown().await;
}

/// Row D: a required relay and a new client agree on `lightspeed-control/1`,
/// and the client can read the negotiated value back (what stage A added
/// `ControlClient::negotiated_alpn()` for).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_required_relay_negotiates_the_shared_name_and_registers() {
    let relay = TestRelay::start(ControlAlpn::Required).await;
    let mut client = ControlClient::new().expect("create the control client");

    tokio::time::timeout(
        CONNECT_TIMEOUT,
        client.connect_with_destination(relay.addr, game_id::UNKNOWN, None),
    )
    .await
    .expect("row D: the first-attempt handshake must not hang")
    .expect("row D: a required relay must accept a client that offers the shared name");

    assert!(
        client.is_connected(),
        "row D: the handshake must leave a live control connection"
    );
    assert!(
        client.session_id().is_some(),
        "row D: negotiation must be followed by a registration"
    );
    assert_eq!(
        client.negotiated_alpn(),
        Some(QUIC_CONTROL_ALPN_V1),
        "row D: the client must observe the negotiated control ALPN"
    );

    wait_for_outcome(&relay.metrics, ControlHandshakeOutcome::NegotiatedV1, 1).await;
    assert_eq!(
        relay
            .metrics
            .control_handshake_count(ControlHandshakeOutcome::NoAlpn),
        0,
        "row D: the first attempt must succeed, so no ALPN-less handshake is counted"
    );
    assert_eq!(
        relay
            .metrics
            .control_handshake_count(ControlHandshakeOutcome::RejectedNoApplicationProtocol),
        0,
        "row D: a required relay must not reject the shared offer, so the fallback never runs"
    );

    let _ = client.disconnect().await;
    relay.shutdown().await;
}

/// Row B across the crate boundary: an ALPN-less client (the shape of every
/// build before the rollout) cannot handshake with a required relay; the relay
/// closes with TLS alert 120 / `crypto(0x78)`. This is why the relay flip
/// cannot precede the client release.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_required_relay_rejects_an_alpn_less_client_with_120() {
    let relay = TestRelay::start(ControlAlpn::Required).await;
    let endpoint = alpn_less_client_endpoint();

    let error = endpoint
        .connect(relay.addr, "lightspeed-proxy")
        .expect("start the QUIC handshake")
        .await
        .expect_err("row B: a required relay must reject an ALPN-less client");
    assert_alert_120(&error);

    wait_for_outcome(
        &relay.metrics,
        ControlHandshakeOutcome::RejectedNoApplicationProtocol,
        1,
    )
    .await;
    assert_eq!(
        relay
            .metrics
            .control_handshake_count(ControlHandshakeOutcome::NegotiatedV1),
        0,
        "row B: a refused handshake must not be counted as negotiated"
    );

    drop(endpoint);
    relay.shutdown().await;
}

/// Assert the peer closed the handshake with TLS alert 120
/// (`no_application_protocol`). The accepting side reports it as a transport
/// error, a peer that received the alert reports a connection close; both
/// shapes mean the same alert (same helper as the row-B matrix unit test).
fn assert_alert_120(error: &quinn::ConnectionError) {
    let code = match error {
        quinn::ConnectionError::TransportError(error) => error.code,
        quinn::ConnectionError::ConnectionClosed(close) => close.error_code,
        other => panic!("row B: expected TLS alert 120, got {other:?}"),
    };
    assert_eq!(
        code,
        quinn::TransportErrorCode::crypto(0x78),
        "row B: expected crypto(0x78) / TLS alert 120 (no_application_protocol), got {error:?}"
    );
}

/// A raw quinn client in the pre-rollout shape: it offers no ALPN at all.
///
/// The certificate is not what these tests are about, so it is accepted; the
/// ALPN offer is the only variable. This deliberately does not use
/// `ControlClient`: row B is about a client that predates the offer.
fn alpn_less_client_endpoint() -> quinn::Endpoint {
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
    // No `alpn_protocols` assignment: an empty list sends no ALPN extension.
    let quic_crypto =
        quinn::crypto::rustls::QuicClientConfig::try_from(crypto).expect("QUIC client crypto");
    let mut endpoint = quinn::Endpoint::client(loopback()).expect("client endpoint");
    endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(quic_crypto)));
    endpoint
}
