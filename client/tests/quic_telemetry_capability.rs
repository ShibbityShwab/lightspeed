//! Regression: a client talking to a pre-upgrade relay (one whose
//! registration ack does not advertise control-plane telemetry) must refuse
//! the QUIC path so the caller falls back to HTTP, while a relay that
//! advertises support receives the report over QUIC.

#![cfg(feature = "quic")]

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::Arc;
use std::time::Duration;

use lightspeed_client::test_support::{
    control_telemetry_accepted, register_session, stop_supervisor,
};
use lightspeed_protocol::control::ControlMessage;

const OLD_PROXY_TOKEN: u32 = 0x1111;
const NEW_PROXY_TOKEN: u32 = 0x2222;

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// A minimal control-plane server that answers registration with `token`,
/// advertising control-plane telemetry only when `advertise` is set, and
/// records any telemetry message it receives.
async fn spawn_fake_proxy(
    token: u32,
    advertise: bool,
    seen: Arc<tokio::sync::Notify>,
) -> anyhow::Result<quinn::Endpoint> {
    let key_pair = rcgen::KeyPair::generate()?;
    let params = rcgen::CertificateParams::new(vec!["lightspeed-proxy".into()])?;
    let cert = params.self_signed(&key_pair)?;
    let cert_der = rustls::pki_types::CertificateDer::from(cert.der().to_vec());
    let key_der = rustls::pki_types::PrivateKeyDer::try_from(key_pair.serialize_der())
        .map_err(|e| anyhow::anyhow!("serialise test key: {e}"))?;

    let tls = rustls::ServerConfig::builder()
        .with_no_client_auth()
        .with_single_cert(vec![cert_der], key_der)?;
    let crypto = quinn::crypto::rustls::QuicServerConfig::try_from(tls)?;
    let endpoint = quinn::Endpoint::server(
        quinn::ServerConfig::with_crypto(Arc::new(crypto)),
        "127.0.0.1:0".parse::<SocketAddr>()?,
    )?;

    let accepting = endpoint.clone();
    tokio::spawn(async move {
        while let Some(incoming) = accepting.accept().await {
            let Ok(conn) = incoming.await else { continue };
            let seen = Arc::clone(&seen);
            tokio::spawn(async move {
                while let Ok((mut send, mut recv)) = conn.accept_bi().await {
                    match ControlMessage::read_from(&mut recv).await {
                        Ok(Some(ControlMessage::Register { .. })) => {
                            let ack = ControlMessage::RegisterAck {
                                session_id: 1,
                                session_token: token,
                                node_id: "fake".into(),
                                region: "test".into(),
                                telemetry_quic: advertise,
                            };
                            let _ = ack.write_to(&mut send).await;
                            let _ = send.finish();
                        }
                        Ok(Some(ControlMessage::Telemetry { .. })) => seen.notify_one(),
                        _ => {}
                    }
                }
            });
        }
    });

    Ok(endpoint)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn telemetry_falls_back_when_relay_lacks_control_telemetry() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!("ls-quic-tel-{}", std::process::id()));
    std::fs::create_dir_all(&home)?;
    std::env::set_var("HOME", &home);
    install_crypto_provider();

    // Pre-upgrade relay: registration ack carries no telemetry capability.
    let old_seen = Arc::new(tokio::sync::Notify::new());
    let old_endpoint = spawn_fake_proxy(OLD_PROXY_TOKEN, false, Arc::clone(&old_seen)).await?;
    let old_port = old_endpoint.local_addr()?.port();
    let old_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 45_001);
    let _ = stop_supervisor(old_addr);
    assert_eq!(
        register_session(old_addr, old_port).await,
        Some(OLD_PROXY_TOKEN),
        "registration must succeed against the legacy relay"
    );
    assert!(
        !control_telemetry_accepted(old_addr, b"{}").await,
        "an unadvertised relay must not receive telemetry over QUIC"
    );
    assert!(stop_supervisor(old_addr));

    // Upgraded relay: registration ack advertises control-plane telemetry.
    let new_seen = Arc::new(tokio::sync::Notify::new());
    let new_endpoint = spawn_fake_proxy(NEW_PROXY_TOKEN, true, Arc::clone(&new_seen)).await?;
    let new_port = new_endpoint.local_addr()?.port();
    let new_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 45_002);
    let _ = stop_supervisor(new_addr);
    assert_eq!(
        register_session(new_addr, new_port).await,
        Some(NEW_PROXY_TOKEN),
        "registration must succeed against the upgraded relay"
    );
    assert!(
        control_telemetry_accepted(new_addr, b"{}").await,
        "an advertising relay must accept telemetry over QUIC"
    );
    tokio::time::timeout(Duration::from_secs(5), new_seen.notified())
        .await
        .expect("upgraded relay did not receive the telemetry message");
    assert!(stop_supervisor(new_addr));

    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}
