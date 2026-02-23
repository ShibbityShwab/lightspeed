//! # QUIC Control Plane Integration Test
//!
//! Tests the full client ↔ proxy QUIC control plane flow:
//! 1. Start proxy QUIC control server
//! 2. Client connects and registers
//! 3. Client sends pings and measures RTT
//! 4. Client disconnects gracefully
//!
//! Only runs when compiled with `--features quic`.

#![cfg(feature = "quic")]

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lightspeed_protocol::control::{disconnect_reason, game_id, ControlMessage};
use lightspeed_protocol::PROTOCOL_VERSION;

// ── Helpers ─────────────────────────────────────────────────────────

/// Generate a self-signed certificate and build a quinn server endpoint.
fn make_server_endpoint(bind: SocketAddr) -> anyhow::Result<quinn::Endpoint> {
    let key_pair = rcgen::KeyPair::generate()?;
    let cert_params = rcgen::CertificateParams::new(vec!["lightspeed-proxy".into()])?;
    let cert = cert_params.self_signed(&key_pair)?;
    let cert_der = rustls::pki_types::CertificateDer::from(cert.der().to_vec());
    let key_der = rustls::pki_types::PrivateKeyDer::try_from(key_pair.serialize_der())
        .map_err(|e| anyhow::anyhow!("{}", e))?;

    let rustls_config = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)?;

    let quic_crypto = quinn::crypto::rustls::QuicServerConfig::try_from(rustls_config)?;
    let server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_crypto));
    let endpoint = quinn::Endpoint::server(server_config, bind)?;
    Ok(endpoint)
}

/// Build a quinn client endpoint that skips cert verification.
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

    let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse()?)?;
    endpoint.set_default_client_config(client_config);
    Ok(endpoint)
}

/// Simple server handler: process one stream's worth of messages.
async fn serve_one_stream(
    mut send: quinn::SendStream,
    mut recv: quinn::RecvStream,
) -> anyhow::Result<()> {
    while let Some(msg) = ControlMessage::read_from(&mut recv).await? {
        let response = match msg {
            ControlMessage::Ping { timestamp_us } => {
                let now_us = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64;
                Some(ControlMessage::Pong {
                    client_timestamp_us: timestamp_us,
                    server_timestamp_us: now_us,
                })
            }
            ControlMessage::Register { game, .. } => Some(ControlMessage::RegisterAck {
                session_id: 42,
                session_token: 0xAB,
                node_id: "test-proxy".into(),
                region: "test-region".into(),
            }),
            ControlMessage::Disconnect { .. } => None,
            _ => None,
        };
        if let Some(resp) = response {
            resp.write_to(&mut send).await?;
        }
    }
    send.finish()?;
    Ok(())
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_register_and_ping() -> anyhow::Result<()> {
    // Start server on a random port
    let server = make_server_endpoint("127.0.0.1:0".parse()?)?;
    let server_addr = server.local_addr()?;

    // Spawn server accept loop
    let server_handle = tokio::spawn(async move {
        if let Some(incoming) = server.accept().await {
            let conn = incoming.await.unwrap();
            // Accept streams until connection closes
            loop {
                match conn.accept_bi().await {
                    Ok((send, recv)) => {
                        tokio::spawn(async move {
                            let _ = serve_one_stream(send, recv).await;
                        });
                    }
                    Err(_) => break,
                }
            }
        }
    });

    // Give server a moment to start
    tokio::time::sleep(Duration::from_millis(50)).await;

    // --- Client side ---
    let client_ep = make_client_endpoint()?;
    let conn = client_ep.connect(server_addr, "lightspeed-proxy")?.await?;

    // Register
    {
        let (mut send, mut recv) = conn.open_bi().await?;
        let register = ControlMessage::Register {
            protocol_version: PROTOCOL_VERSION,
            game: game_id::CS2,
        };
        register.write_to(&mut send).await?;

        let response = ControlMessage::read_from(&mut recv).await?;
        match response {
            Some(ControlMessage::RegisterAck {
                session_id,
                session_token,
                node_id,
                region,
            }) => {
                assert_eq!(session_id, 42);
                assert_eq!(session_token, 0xAB);
                assert_eq!(node_id, "test-proxy");
                assert_eq!(region, "test-region");
            }
            other => panic!("Expected RegisterAck, got: {:?}", other),
        }
        send.finish()?;
    }

    // Ping (3 times)
    for _ in 0..3 {
        let (mut send, mut recv) = conn.open_bi().await?;
        let now_us = SystemTime::now().duration_since(UNIX_EPOCH)?.as_micros() as u64;

        let ping = ControlMessage::Ping {
            timestamp_us: now_us,
        };
        ping.write_to(&mut send).await?;
        send.finish()?;

        let response = ControlMessage::read_from(&mut recv).await?;
        match response {
            Some(ControlMessage::Pong {
                client_timestamp_us,
                server_timestamp_us,
            }) => {
                assert_eq!(client_timestamp_us, now_us);
                assert!(server_timestamp_us > 0);
            }
            other => panic!("Expected Pong, got: {:?}", other),
        }
    }

    // Disconnect
    {
        let (mut send, _recv) = conn.open_bi().await?;
        let msg = ControlMessage::Disconnect {
            reason: disconnect_reason::NORMAL,
        };
        msg.write_to(&mut send).await?;
        send.finish()?;
    }

    conn.close(0u32.into(), b"test done");
    server_handle.abort();

    Ok(())
}

#[tokio::test]
async fn test_message_roundtrip_encoding() {
    // Test that all message types survive encode → decode
    let messages = vec![
        ControlMessage::Ping {
            timestamp_us: 123456789,
        },
        ControlMessage::Pong {
            client_timestamp_us: 100,
            server_timestamp_us: 200,
        },
        ControlMessage::Register {
            protocol_version: PROTOCOL_VERSION,
            game: game_id::FORTNITE,
        },
        ControlMessage::RegisterAck {
            session_id: 999,
            session_token: 42,
            node_id: "node-abc".into(),
            region: "us-east".into(),
        },
        ControlMessage::Disconnect {
            reason: disconnect_reason::SERVER_SHUTDOWN,
        },
        ControlMessage::ServerInfo {
            load_pct: 50,
            active_clients: 42,
            capacity: 100,
        },
    ];

    for msg in messages {
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded, "Roundtrip failed for {:?}", msg);
    }
}
