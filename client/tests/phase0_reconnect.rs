//! Phase 0 end-to-end test: every active multipath relay obtains its own
//! per-path registration and data-plane token.
//!
//! Drives the real supervised registration path
//! (`lightspeed_client::test_support::register_session`) against two minimal
//! in-test QUIC control servers. Only runs with `--features quic`.

#![cfg(feature = "quic")]

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Once};
use std::time::{SystemTime, UNIX_EPOCH};

use lightspeed_client::test_support::{path_token, register_session, stop_supervisor};
use lightspeed_protocol::control::ControlMessage;

static HOME_ONCE: Once = Once::new();

/// Point the TOFU fingerprint store at a fresh per-run temp HOME so ephemeral
/// port reuse cannot leave a stale pin behind.
fn isolate_fingerprint_store() {
    HOME_ONCE.call_once(|| {
        let unique = format!("lightspeed-phase0-{}-{}", std::process::id(), now_us());
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

/// A minimal control server that answers Register with a fresh token.
struct TestServer {
    _endpoint: quinn::Endpoint,
    control_port: u16,
    registrations: Arc<AtomicUsize>,
}

impl TestServer {
    fn start(first_token: u32) -> Self {
        let endpoint = server_endpoint();
        let control_port = endpoint.local_addr().expect("local addr").port();
        let registrations = Arc::new(AtomicUsize::new(0));

        let accept_endpoint = endpoint.clone();
        let accept_registrations = Arc::clone(&registrations);
        tokio::spawn(async move {
            while let Some(incoming) = accept_endpoint.accept().await {
                let registrations = Arc::clone(&accept_registrations);
                tokio::spawn(async move {
                    if let Ok(connection) = incoming.await {
                        serve_connection(connection, first_token, registrations).await;
                    }
                });
            }
        });

        Self {
            _endpoint: endpoint,
            control_port,
            registrations,
        }
    }

    fn registration_count(&self) -> usize {
        self.registrations.load(Ordering::SeqCst)
    }
}

async fn serve_connection(
    connection: quinn::Connection,
    first_token: u32,
    registrations: Arc<AtomicUsize>,
) {
    loop {
        let Ok((mut send, mut recv)) = connection.accept_bi().await else {
            return;
        };
        while let Ok(Some(message)) = ControlMessage::read_from(&mut recv).await {
            match message {
                ControlMessage::Register { .. } => {
                    let seq = registrations.fetch_add(1, Ordering::SeqCst) as u32;
                    let ack = ControlMessage::RegisterAck {
                        session_id: 1 + seq,
                        session_token: first_token + seq,
                        node_id: "phase0-relay".to_string(),
                        region: "loopback".to_string(),
                    };
                    let _ = ack.write_to(&mut send).await;
                    let _ = send.finish();
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

/// Two active multipath paths each register exactly once and publish their own
/// relay's per-path token.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn every_active_path_registered() {
    isolate_fingerprint_store();
    let server_a = TestServer::start(1000);
    let server_b = TestServer::start(2000);
    let path_a = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 41301);
    let path_b = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 41302);
    let _ = stop_supervisor(path_a);
    let _ = stop_supervisor(path_b);

    let token_a = register_session(path_a, server_a.control_port)
        .await
        .expect("path A must register");
    let token_b = register_session(path_b, server_b.control_port)
        .await
        .expect("path B must register");

    assert!(token_a > 0 && token_b > 0, "both paths must receive tokens");
    assert_ne!(token_a, token_b, "each path holds its own relay's token");
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
    assert_eq!(path_token(path_a), token_a, "path A token is published");
    assert_eq!(path_token(path_b), token_b, "path B token is published");

    let _ = stop_supervisor(path_a);
    let _ = stop_supervisor(path_b);
}
