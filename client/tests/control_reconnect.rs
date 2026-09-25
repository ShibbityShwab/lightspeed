//! End-to-end tests for the supervised, reconnectable control plane.
//!
//! Each test runs a minimal in-test QUIC control server (quinn + a self-signed
//! rcgen certificate; the client trust-on-first-use pins it) that speaks
//! `lightspeed_protocol::ControlMessage`. Only runs with `--features quic`.

#![cfg(feature = "quic")]

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, Once};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use lightspeed_client::test_support::{
    is_supervised, path_token, register_session, session_token, stop_supervisor,
};
use lightspeed_client::LightSpeedEngine;
use lightspeed_protocol::control::ControlMessage;

/// The first token the server issues (token B is 1001, and so on).
const FIRST_TOKEN: u32 = 1000;

static HOME_ONCE: Once = Once::new();

/// Point the TOFU fingerprint store at a fresh per-run temp HOME so ephemeral
/// port reuse between runs cannot leave a stale pin behind.
fn isolate_fingerprint_store() {
    HOME_ONCE.call_once(|| {
        let unique = format!(
            "lightspeed-control-test-{}-{}",
            std::process::id(),
            now_us()
        );
        let dir = std::env::temp_dir().join(unique);
        std::fs::create_dir_all(&dir).expect("create test HOME");
        std::env::set_var("HOME", dir);
    });
}

fn now_us() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u64
}

fn server_endpoint() -> quinn::Endpoint {
    let key_pair = rcgen::KeyPair::generate().expect("generate key");
    let params =
        rcgen::CertificateParams::new(vec!["lightspeed-proxy".to_string()]).expect("cert params");
    let cert = params.self_signed(&key_pair).expect("self-signed cert");
    let cert_der = rustls::pki_types::CertificateDer::from(cert.der().to_vec());
    let key_der =
        rustls::pki_types::PrivateKeyDer::try_from(key_pair.serialize_der()).expect("key der");

    let rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)
        .expect("server config");
    let quic_crypto =
        quinn::crypto::rustls::QuicServerConfig::try_from(rustls_config).expect("quic config");
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_crypto));

    quinn::Endpoint::server(server_config, "127.0.0.1:0".parse().unwrap())
        .expect("bind server endpoint")
}

/// A minimal control server that answers Register with an incrementing token
/// and Ping with Pong.
struct TestServer {
    _endpoint: quinn::Endpoint,
    control_port: u16,
    registrations: Arc<AtomicUsize>,
    tokens: Arc<Mutex<Vec<u32>>>,
}

impl TestServer {
    fn start(close_first_after_ack: bool) -> Self {
        let endpoint = server_endpoint();
        let control_port = endpoint.local_addr().expect("local addr").port();
        let registrations = Arc::new(AtomicUsize::new(0));
        let tokens = Arc::new(Mutex::new(Vec::new()));

        let accept_endpoint = endpoint.clone();
        let accept_registrations = Arc::clone(&registrations);
        let accept_tokens = Arc::clone(&tokens);
        tokio::spawn(async move {
            let mut connection_index = 0usize;
            while let Some(incoming) = accept_endpoint.accept().await {
                let index = connection_index;
                connection_index += 1;
                let registrations = Arc::clone(&accept_registrations);
                let tokens = Arc::clone(&accept_tokens);
                tokio::spawn(async move {
                    if let Ok(connection) = incoming.await {
                        serve_connection(
                            connection,
                            index,
                            close_first_after_ack,
                            registrations,
                            tokens,
                        )
                        .await;
                    }
                });
            }
        });

        Self {
            _endpoint: endpoint,
            control_port,
            registrations,
            tokens,
        }
    }

    fn registration_count(&self) -> usize {
        self.registrations.load(Ordering::SeqCst)
    }

    fn last_token(&self) -> u32 {
        self.tokens
            .lock()
            .expect("token list")
            .last()
            .copied()
            .unwrap_or(0)
    }
}

async fn serve_connection(
    connection: quinn::Connection,
    connection_index: usize,
    close_first_after_ack: bool,
    registrations: Arc<AtomicUsize>,
    tokens: Arc<Mutex<Vec<u32>>>,
) {
    loop {
        let Ok((mut send, mut recv)) = connection.accept_bi().await else {
            return;
        };
        while let Ok(Some(message)) = ControlMessage::read_from(&mut recv).await {
            match message {
                ControlMessage::Register { .. } => {
                    let seq = registrations.fetch_add(1, Ordering::SeqCst) as u32;
                    let token = FIRST_TOKEN + seq;
                    tokens.lock().expect("token list").push(token);
                    let ack = ControlMessage::RegisterAck {
                        session_id: 1 + seq,
                        session_token: token,
                        node_id: "test-relay".to_string(),
                        region: "test".to_string(),
                        telemetry_quic: false,
                    };
                    let _ = ack.write_to(&mut send).await;
                    let _ = send.finish();
                    if close_first_after_ack && connection_index == 0 {
                        // Give the client time to read the ack, then sever the
                        // control connection to force a supervised reconnect.
                        tokio::time::sleep(Duration::from_millis(200)).await;
                        connection.close(0u32.into(), b"control-loss");
                        return;
                    }
                }
                ControlMessage::Ping { timestamp_us } => {
                    let pong = ControlMessage::Pong {
                        client_timestamp_us: timestamp_us,
                        server_timestamp_us: now_us(),
                    };
                    let _ = pong.write_to(&mut send).await;
                }
                _ => {}
            }
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reconnects_after_control_loss_and_preserves_token() {
    isolate_fingerprint_store();
    let server = TestServer::start(true);
    let data_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 41001);

    let token_a = register_session(data_addr, server.control_port)
        .await
        .expect("initial registration must return a token");
    assert!(token_a > 0, "initial token must be non-zero");
    assert_eq!(path_token(data_addr), token_a);

    let deadline = Instant::now() + Duration::from_secs(10);
    while Instant::now() < deadline {
        assert_ne!(
            path_token(data_addr),
            0,
            "a transient control-plane loss must never zero the per-path token"
        );
        assert_ne!(
            session_token(),
            0,
            "a transient control-plane loss must never zero the session token"
        );
        let latest = server.last_token();
        if latest != 0 && latest != token_a {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let token_b = server.last_token();
    assert!(
        token_b > 0 && token_b != token_a,
        "the supervisor must reconnect and register a fresh token (got {token_b}, first {token_a})"
    );
    assert_eq!(server.registration_count(), 2);

    let deadline = Instant::now() + Duration::from_secs(10);
    while path_token(data_addr) != token_b && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        path_token(data_addr),
        token_b,
        "the client must adopt the reconnected relay's token"
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registers_every_active_path() {
    isolate_fingerprint_store();
    let server_a = TestServer::start(false);
    let server_b = TestServer::start(false);
    let path_a = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 41101);
    let path_b = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 41102);

    let token_a = register_session(path_a, server_a.control_port)
        .await
        .expect("path A must register");
    let token_b = register_session(path_b, server_b.control_port)
        .await
        .expect("path B must register");

    assert!(token_a > 0 && token_b > 0);
    assert_eq!(
        server_a.registration_count(),
        1,
        "path A registers exactly once"
    );
    assert_eq!(
        server_b.registration_count(),
        1,
        "path B registers exactly once"
    );
    assert_eq!(path_token(path_a), token_a);
    assert_eq!(path_token(path_b), token_b);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn supervisor_stopped_on_disconnect() {
    isolate_fingerprint_store();
    let server = TestServer::start(false);
    let data_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 41201);
    let _ = stop_supervisor(data_addr);

    let mut engine = LightSpeedEngine::new(tokio::runtime::Handle::current());
    engine.set_control_port(server.control_port);
    engine.connect(data_addr);

    let deadline = Instant::now() + Duration::from_secs(10);
    while path_token(data_addr) == 0 && Instant::now() < deadline {
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        server.registration_count(),
        1,
        "the engine must register on its configured control port"
    );
    let issued = path_token(data_addr);
    assert!(
        issued > 0,
        "engine registration must publish the relay's per-path token"
    );
    assert!(
        is_supervised(data_addr),
        "connect must leave a live control-plane supervisor"
    );

    engine.disconnect();

    assert!(
        !is_supervised(data_addr),
        "a normal disconnect must stop the relay's supervisor"
    );
    assert_eq!(
        path_token(data_addr),
        issued,
        "a normal disconnect must not zero the per-path token"
    );
    assert_ne!(
        session_token(),
        0,
        "a normal disconnect must not zero the session token"
    );

    let _ = stop_supervisor(data_addr);
}
