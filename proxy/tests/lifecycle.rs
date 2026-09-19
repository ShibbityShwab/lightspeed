//! # Proxy lifecycle integration tests
//!
//! Locks the two operational behaviours that make the proxy safe to manage:
//!
//! 1. `--check` validates a configuration without binding the data, control,
//!    or health ports, so it can gate an update while the service is running.
//!    `--bind-check` performs the real binds and is only safe to run while the
//!    service is stopped.
//! 2. On shutdown the control plane announces
//!    `Disconnect { reason: SERVER_SHUTDOWN }` to every live connection before
//!    it closes the QUIC endpoint.

use std::net::{TcpListener, UdpSocket};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

/// Hold the config's three ports and write a matching TOML file.
///
/// The returned sockets stay bound for the duration of the caller's scope, so
/// a regression that binds during `--check` fails with `EADDRINUSE` instead of
/// silently succeeding.
fn hold_ports_and_write_config(
    tag: &str,
) -> anyhow::Result<(UdpSocket, UdpSocket, TcpListener, PathBuf, PathBuf)> {
    let data = UdpSocket::bind("0.0.0.0:0")?;
    let control = UdpSocket::bind("0.0.0.0:0")?;
    let health = TcpListener::bind("0.0.0.0:0")?;

    let dir =
        std::env::temp_dir().join(format!("lightspeed-lifecycle-{tag}-{}", std::process::id()));
    std::fs::create_dir_all(&dir)?;
    // The proxy checks TLS cert/key readability (or first-boot writability).
    // Point it at a temp dir so the test never touches /var/lib.
    let tls = dir.join("tls");
    std::fs::create_dir_all(&tls)?;

    let config = dir.join("proxy.toml");
    let body = format!(
        "[network]\ndata_port = {}\ncontrol_port = {}\nhealth_port = {}\n",
        data.local_addr()?.port(),
        control.local_addr()?.port(),
        health.local_addr()?.port(),
    );
    std::fs::write(&config, body)?;
    Ok((data, control, health, config, tls))
}

/// Run the built proxy binary with `args`, a config, and an isolated TLS dir.
fn run_proxy(args: &[&str], config: &Path, tls: &Path) -> Output {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_lightspeed-proxy"));
    cmd.arg("--config").arg(config);
    cmd.args(args);
    cmd.env("LIGHTSPEED_TLS_DIR", tls);
    cmd.output()
        .expect("failed to spawn the lightspeed-proxy binary")
}

/// `--check` must be exit 0 even while another process holds every port it
/// would otherwise bind. This is what makes it usable as a pre-update gate.
#[test]
fn check_does_not_bind_ports() -> anyhow::Result<()> {
    let (data, control, health, config, tls) = hold_ports_and_write_config("check")?;

    let output = run_proxy(&["--check"], &config, &tls);
    assert!(
        output.status.success(),
        "--check must exit 0 while the data/control/health ports are held.\n\
         status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    // The held sockets must still be ours: --check bound nothing.
    assert!(data.local_addr().is_ok(), "data port was stolen");
    assert!(control.local_addr().is_ok(), "control port was stolen");
    assert!(health.local_addr().is_ok(), "health port was stolen");
    Ok(())
}

/// `--bind-check` performs the real binds, so it must fail while the service
/// (or a test) already holds the ports. This proves the flag is not a no-op.
#[test]
fn bind_check_fails_while_ports_held() -> anyhow::Result<()> {
    let (data, control, health, config, tls) = hold_ports_and_write_config("bind-check")?;

    let output = run_proxy(&["--bind-check"], &config, &tls);
    assert!(
        !output.status.success(),
        "--bind-check must fail while the ports are already held.\n\
         status: {}\nstdout: {}\nstderr: {}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );

    assert!(data.local_addr().is_ok());
    assert!(control.local_addr().is_ok());
    assert!(health.local_addr().is_ok());
    Ok(())
}

#[cfg(feature = "quic")]
mod shutdown {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::Duration;

    use lightspeed_protocol::control::{disconnect_reason, game_id, ControlMessage};
    use lightspeed_protocol::PROTOCOL_VERSION;
    use lightspeed_proxy::auth::Authenticator;
    use lightspeed_proxy::config::ProxyConfig;
    use lightspeed_proxy::control::{ControlServer, ControlState};
    use tokio::sync::{watch, RwLock};

    /// A QUIC client endpoint that accepts the proxy's self-signed cert.
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
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
            {
                Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
            }
            fn verify_tls13_signature(
                &self,
                _: &[u8],
                _: &rustls::pki_types::CertificateDer<'_>,
                _: &rustls::DigitallySignedStruct,
            ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error>
            {
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

    /// A registered control client observes `Disconnect { SERVER_SHUTDOWN }`
    /// when the server is asked to stop.
    #[tokio::test]
    async fn shutdown_sends_server_shutdown() -> anyhow::Result<()> {
        // Keep the generated cert/key out of /var/lib during tests.
        let tls =
            std::env::temp_dir().join(format!("lightspeed-shutdown-tls-{}", std::process::id()));
        std::fs::create_dir_all(&tls)?;
        std::env::set_var("LIGHTSPEED_TLS_DIR", &tls);

        let state = Arc::new(ControlState::new(
            ProxyConfig::default(),
            Arc::new(RwLock::new(Authenticator::new(true))),
        ));
        let server = ControlServer::bind("127.0.0.1:0".parse()?, Arc::clone(&state))?;
        let addr = server.local_addr()?;

        // Deterministic seam: a watch channel requests shutdown, no sleep.
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        let server_handle = tokio::spawn(server.run(shutdown_rx));

        let client = make_client_endpoint()?;
        let conn = client.connect(addr, "lightspeed-proxy")?.await?;

        // Register so this is a live, registered control session. The
        // RegisterAck proves the server recorded the connection before we
        // trigger shutdown, so the announcement cannot race registration.
        {
            let (mut send, mut recv) = conn.open_bi().await?;
            ControlMessage::Register {
                protocol_version: PROTOCOL_VERSION,
                game: game_id::CS2,
                data_port: 0,
            }
            .write_to(&mut send)
            .await?;
            let ack = ControlMessage::read_from(&mut recv).await?;
            assert!(matches!(ack, Some(ControlMessage::RegisterAck { .. })));
            send.finish()?;
        }

        shutdown_tx.send(true)?;

        // The server opens a fresh server-initiated stream carrying the
        // Disconnect. Bounded so a missing announcement fails fast.
        let (_send, mut recv) = tokio::time::timeout(Duration::from_secs(5), conn.accept_bi())
            .await
            .expect("server did not open a shutdown stream in time")?;
        let msg =
            tokio::time::timeout(Duration::from_secs(5), ControlMessage::read_from(&mut recv))
                .await
                .expect("no shutdown message in time")?;
        assert_eq!(
            msg,
            Some(ControlMessage::Disconnect {
                reason: disconnect_reason::SERVER_SHUTDOWN,
            }),
        );

        // A real client reconnects on shutdown; closing lets the server's
        // bounded peer drain finish promptly instead of waiting it out.
        conn.close(0u32.into(), b"shutdown received");

        let _ = tokio::time::timeout(Duration::from_secs(5), server_handle).await;
        Ok(())
    }
}
