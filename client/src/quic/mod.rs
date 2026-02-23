//! # QUIC Control Plane
//!
//! Manages the reliable control channel between client and proxy nodes
//! using QUIC (via quinn). The control plane handles:
//! - Proxy discovery and registration
//! - Health checking and latency probing
//! - Configuration synchronization
//! - Route negotiation
//!
//! The data plane (game packets) uses raw UDP for minimum latency.
//! Only control messages use QUIC for reliability.

pub mod discovery;
pub mod health;

// ── Feature-gated real implementation ───────────────────────────────

#[cfg(feature = "quic")]
mod inner {
    use std::net::SocketAddr;
    use std::sync::Arc;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use quinn::crypto::rustls::QuicClientConfig;
    use tracing::{debug, info};

    use lightspeed_protocol::control::ControlMessage;
    use lightspeed_protocol::PROTOCOL_VERSION;

    use crate::error::QuicError;

    /// Skip server certificate verification (MVP / dev mode).
    /// In production this would verify the proxy's certificate.
    #[derive(Debug)]
    struct SkipServerVerification;

    impl rustls::client::danger::ServerCertVerifier for SkipServerVerification {
        fn verify_server_cert(
            &self,
            _end_entity: &rustls::pki_types::CertificateDer<'_>,
            _intermediates: &[rustls::pki_types::CertificateDer<'_>],
            _server_name: &rustls::pki_types::ServerName<'_>,
            _ocsp_response: &[u8],
            _now: rustls::pki_types::UnixTime,
        ) -> Result<rustls::client::danger::ServerCertVerified, rustls::Error> {
            Ok(rustls::client::danger::ServerCertVerified::assertion())
        }

        fn verify_tls12_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn verify_tls13_signature(
            &self,
            _message: &[u8],
            _cert: &rustls::pki_types::CertificateDer<'_>,
            _dss: &rustls::DigitallySignedStruct,
        ) -> Result<rustls::client::danger::HandshakeSignatureValid, rustls::Error> {
            Ok(rustls::client::danger::HandshakeSignatureValid::assertion())
        }

        fn supported_verify_schemes(&self) -> Vec<rustls::SignatureScheme> {
            rustls::crypto::ring::default_provider()
                .signature_verification_algorithms
                .supported_schemes()
        }
    }

    /// Build a quinn client config that skips certificate verification (MVP).
    fn build_client_config() -> Result<quinn::ClientConfig, QuicError> {
        let crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(SkipServerVerification))
            .with_no_client_auth();

        let quic_crypto =
            QuicClientConfig::try_from(crypto).map_err(|e| QuicError::Tls(e.to_string()))?;

        let mut client_config = quinn::ClientConfig::new(Arc::new(quic_crypto));

        // Tune transport
        let mut transport = quinn::TransportConfig::default();
        transport.keep_alive_interval(Some(Duration::from_secs(10)));
        transport.max_idle_timeout(Some(
            Duration::from_secs(60)
                .try_into()
                .map_err(|e: quinn::VarIntBoundsExceeded| QuicError::Tls(e.to_string()))?,
        ));
        client_config.transport_config(Arc::new(transport));

        Ok(client_config)
    }

    /// QUIC control plane client (real implementation).
    pub struct ControlClient {
        /// Quinn endpoint for outgoing connections.
        endpoint: quinn::Endpoint,
        /// Active connection to the proxy.
        connection: Option<quinn::Connection>,
        /// Remote proxy address.
        remote_addr: Option<SocketAddr>,
        /// Session ID assigned by the proxy.
        session_id: Option<u32>,
        /// Data-plane session token (for tunnel header auth).
        session_token: Option<u8>,
        /// Proxy node ID.
        node_id: Option<String>,
        /// Proxy region.
        region: Option<String>,
    }

    impl ControlClient {
        /// Create a new control plane client.
        pub fn new() -> Result<Self, QuicError> {
            // Bind to any available port for the client endpoint
            let mut endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            endpoint.set_default_client_config(build_client_config()?);

            Ok(Self {
                endpoint,
                connection: None,
                remote_addr: None,
                session_id: None,
                session_token: None,
                node_id: None,
                region: None,
            })
        }

        /// Connect to a proxy's control plane and register.
        pub async fn connect(&mut self, addr: SocketAddr, game: u8) -> Result<(), QuicError> {
            info!("Connecting QUIC control plane to {}", addr);

            let conn = self
                .endpoint
                .connect(addr, "lightspeed-proxy")
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            info!("QUIC connection established to {}", addr);

            // Open a bidirectional stream for registration
            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            // Send Register message
            let register = ControlMessage::Register {
                protocol_version: PROTOCOL_VERSION,
                game,
            };
            register
                .write_to(&mut send)
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            // Read RegisterAck
            let response = ControlMessage::read_from(&mut recv)
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            match response {
                Some(ControlMessage::RegisterAck {
                    session_id,
                    session_token,
                    node_id,
                    region,
                }) => {
                    info!(
                        "Registered with proxy: session={}, token={}, node={}, region={}",
                        session_id, session_token, node_id, region
                    );
                    self.session_id = Some(session_id);
                    self.session_token = Some(session_token);
                    self.node_id = Some(node_id);
                    self.region = Some(region);
                }
                Some(ControlMessage::Disconnect { reason }) => {
                    return Err(QuicError::ConnectionFailed(format!(
                        "Proxy rejected registration (reason={})",
                        reason
                    )));
                }
                other => {
                    return Err(QuicError::ConnectionFailed(format!(
                        "Unexpected response: {:?}",
                        other
                    )));
                }
            }

            // Finish the registration stream
            send.finish()
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            self.connection = Some(conn);
            self.remote_addr = Some(addr);
            Ok(())
        }

        /// Send a ping and measure round-trip time in microseconds.
        pub async fn ping(&self) -> Result<u64, QuicError> {
            let conn = self
                .connection
                .as_ref()
                .ok_or_else(|| QuicError::ConnectionFailed("Not connected".into()))?;

            let (mut send, mut recv) = conn
                .open_bi()
                .await
                .map_err(|e| QuicError::HealthCheckFailed(e.to_string()))?;

            let now_us = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;

            let ping = ControlMessage::Ping {
                timestamp_us: now_us,
            };
            ping.write_to(&mut send)
                .await
                .map_err(|e| QuicError::HealthCheckFailed(e.to_string()))?;
            send.finish()
                .map_err(|e| QuicError::HealthCheckFailed(e.to_string()))?;

            let response = ControlMessage::read_from(&mut recv)
                .await
                .map_err(|e| QuicError::HealthCheckFailed(e.to_string()))?;

            let after_us = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_micros() as u64;

            match response {
                Some(ControlMessage::Pong {
                    client_timestamp_us,
                    ..
                }) => {
                    let rtt_us = after_us.saturating_sub(client_timestamp_us);
                    debug!("Ping RTT: {}μs", rtt_us);
                    Ok(rtt_us)
                }
                other => Err(QuicError::HealthCheckFailed(format!(
                    "Unexpected ping response: {:?}",
                    other
                ))),
            }
        }

        /// Disconnect from the proxy gracefully.
        pub async fn disconnect(&mut self) -> Result<(), QuicError> {
            if let Some(conn) = self.connection.take() {
                // Try to send a Disconnect message
                if let Ok((mut send, _recv)) = conn.open_bi().await {
                    let msg = ControlMessage::Disconnect {
                        reason: lightspeed_protocol::control::disconnect_reason::NORMAL,
                    };
                    let _ = msg.write_to(&mut send).await;
                    let _ = send.finish();
                }
                conn.close(0u32.into(), b"bye");
                info!("Disconnected from proxy");
            }
            self.remote_addr = None;
            self.session_id = None;
            self.session_token = None;
            self.node_id = None;
            self.region = None;
            Ok(())
        }

        /// Check if control plane is connected.
        pub fn is_connected(&self) -> bool {
            self.connection
                .as_ref()
                .map(|c| !c.close_reason().is_some())
                .unwrap_or(false)
        }

        /// Get the session ID assigned by the proxy.
        pub fn session_id(&self) -> Option<u32> {
            self.session_id
        }

        /// Get the data-plane session token (for tunnel header auth).
        pub fn session_token(&self) -> Option<u8> {
            self.session_token
        }

        /// Get the proxy node ID.
        pub fn node_id(&self) -> Option<&str> {
            self.node_id.as_deref()
        }

        /// Get the proxy region.
        pub fn region(&self) -> Option<&str> {
            self.region.as_deref()
        }

        /// Get the remote proxy address.
        pub fn remote_addr(&self) -> Option<SocketAddr> {
            self.remote_addr
        }
    }
}

// ── Fallback stub (no quic feature) ─────────────────────────────────

#[cfg(not(feature = "quic"))]
mod inner {
    use std::net::SocketAddr;

    use crate::error::QuicError;

    /// QUIC control plane client (stub — compile without `quic` feature).
    pub struct ControlClient {
        connected: bool,
        remote_addr: Option<SocketAddr>,
    }

    impl ControlClient {
        pub fn new() -> Result<Self, QuicError> {
            Ok(Self {
                connected: false,
                remote_addr: None,
            })
        }

        pub async fn connect(&mut self, addr: SocketAddr, _game: u8) -> Result<(), QuicError> {
            tracing::info!("QUIC disabled — stub connect to {}", addr);
            self.remote_addr = Some(addr);
            self.connected = true;
            Ok(())
        }

        pub async fn ping(&self) -> Result<u64, QuicError> {
            Ok(0)
        }

        pub async fn disconnect(&mut self) -> Result<(), QuicError> {
            self.connected = false;
            self.remote_addr = None;
            Ok(())
        }

        pub fn is_connected(&self) -> bool {
            self.connected
        }

        pub fn session_id(&self) -> Option<u32> {
            None
        }

        pub fn session_token(&self) -> Option<u8> {
            None
        }

        pub fn node_id(&self) -> Option<&str> {
            None
        }

        pub fn region(&self) -> Option<&str> {
            None
        }

        pub fn remote_addr(&self) -> Option<SocketAddr> {
            self.remote_addr
        }
    }
}

pub use inner::ControlClient;
