//! Regression: the client reports its game-server destination at registration
//! and caches the region the relay resolves, so the region prior can rank a
//! relay that has never carried traffic to that server. A registration without
//! a destination must leave the region cache untouched.

#![cfg(feature = "quic")]

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use lightspeed_client::test_support::{
    estimated_destination_region, register_session, register_session_with_destination,
    report_destination, set_current_proxy, stop_supervisor,
};
use lightspeed_protocol::control::ControlMessage;

const TOKEN: u32 = 0x3333;
const DESTINATION: Ipv4Addr = Ipv4Addr::new(104, 26, 1, 50);
const UNSEEN_DESTINATION: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 9);

fn install_crypto_provider() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

/// A minimal control-plane server that records the destination of every
/// registration and, when `dest_region` is set, returns it in the ack.
async fn spawn_fake_proxy(
    dest_region: Option<&'static str>,
    seen: Arc<Mutex<Vec<Option<Ipv4Addr>>>>,
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
                    if let Ok(Some(ControlMessage::Register { destination, .. })) =
                        ControlMessage::read_from(&mut recv).await
                    {
                        seen.lock().expect("destination log").push(destination);
                        let ack = ControlMessage::RegisterAck {
                            session_id: 1,
                            session_token: TOKEN,
                            node_id: "fake".into(),
                            region: "test".into(),
                            telemetry_quic: false,
                            dest_region: dest_region.map(str::to_string),
                        };
                        let _ = ack.write_to(&mut send).await;
                        let _ = send.finish();
                    }
                }
            });
        }
    });

    Ok(endpoint)
}

/// The relay records the destination only after the client's registration
/// returns, so wait briefly for the relay task to observe it.
async fn first_seen(seen: &Arc<Mutex<Vec<Option<Ipv4Addr>>>>) -> Option<Ipv4Addr> {
    for _ in 0..100 {
        if let Some(first) = seen.lock().expect("destination log").first() {
            return *first;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the relay never observed a registration");
}

async fn wait_for_seen(
    seen: &Arc<Mutex<Vec<Option<Ipv4Addr>>>>,
    wanted: Option<Ipv4Addr>,
) -> Option<Ipv4Addr> {
    for _ in 0..250 {
        if seen.lock().expect("destination log").contains(&wanted) {
            return wanted;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the relay never observed a registration for {wanted:?}");
}

async fn wait_for_region(destination: Ipv4Addr) -> Option<String> {
    for _ in 0..250 {
        let region = estimated_destination_region(destination);
        if region.is_some() {
            return region;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    estimated_destination_region(destination)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registration_reports_destination_and_caches_the_resolved_region() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!("ls-quic-dest-{}", std::process::id()));
    std::fs::create_dir_all(&home)?;
    std::env::set_var("HOME", &home);
    install_crypto_provider();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let endpoint = spawn_fake_proxy(Some("AU"), Arc::clone(&seen)).await?;
    let port = endpoint.local_addr()?.port();
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 45_010);
    let _ = stop_supervisor(addr);

    assert_eq!(
        register_session_with_destination(addr, port, Some(DESTINATION)).await,
        Some(TOKEN),
    );
    assert_eq!(
        first_seen(&seen).await,
        Some(DESTINATION),
        "the relay must receive the game-server destination"
    );
    assert_eq!(
        estimated_destination_region(DESTINATION).as_deref(),
        Some("oceania"),
        "the AU ack must be cached as the oceania region"
    );
    assert!(stop_supervisor(addr));

    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn registration_without_a_destination_leaves_the_region_unset() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!("ls-quic-nodest-{}", std::process::id()));
    std::fs::create_dir_all(&home)?;
    std::env::set_var("HOME", &home);
    install_crypto_provider();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let endpoint = spawn_fake_proxy(Some("AU"), Arc::clone(&seen)).await?;
    let port = endpoint.local_addr()?.port();
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 45_011);
    let _ = stop_supervisor(addr);

    assert_eq!(register_session(addr, port).await, Some(TOKEN));
    assert_eq!(
        first_seen(&seen).await,
        None,
        "a destination-less registration must report no destination"
    );
    assert_eq!(
        estimated_destination_region(UNSEEN_DESTINATION),
        None,
        "no region must be cached without a destination"
    );
    assert!(stop_supervisor(addr));

    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn destination_known_after_registration_is_backfilled() -> anyhow::Result<()> {
    let home = std::env::temp_dir().join(format!("ls-quic-backfill-{}", std::process::id()));
    std::fs::create_dir_all(&home)?;
    std::env::set_var("HOME", &home);
    install_crypto_provider();

    let seen = Arc::new(Mutex::new(Vec::new()));
    let endpoint = spawn_fake_proxy(Some("AU"), Arc::clone(&seen)).await?;
    let port = endpoint.local_addr()?.port();
    let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 45_012);
    let backfill = Ipv4Addr::new(198, 51, 100, 20);
    let _ = stop_supervisor(addr);

    // Registered before the game server was known: nothing is cached.
    assert_eq!(
        register_session_with_destination(addr, port, None).await,
        Some(TOKEN)
    );
    assert_eq!(first_seen(&seen).await, None);
    assert_eq!(estimated_destination_region(backfill), None);

    // The server becomes known later (the interceptor reports each packet's
    // destination), so the relay is re-registered with it.
    set_current_proxy(addr);
    report_destination(backfill);
    assert_eq!(
        wait_for_seen(&seen, Some(backfill)).await,
        Some(backfill),
        "backfill must re-register the relay with the newly known destination"
    );
    assert_eq!(
        wait_for_region(backfill).await.as_deref(),
        Some("oceania"),
        "the re-registration ack must cache the destination region"
    );
    assert!(stop_supervisor(addr));
    set_current_proxy(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0));

    let _ = std::fs::remove_dir_all(&home);
    Ok(())
}
