//! # QUIC Control Plane Server
//!
//! Accepts QUIC connections from clients, handles registration, health probes,
//! and session management. Runs alongside the UDP data-plane relay.
//!
//! ## Security
//! - Generates random session IDs (unpredictable)
//! - Generates random session tokens for data-plane auth
//! - Wires client authorization into the shared Authenticator
//!
//! Gated behind `--features quic`.

#[cfg(feature = "quic")]
mod inner {
    use std::collections::HashMap;
    use std::net::{Ipv4Addr, SocketAddr};
    use std::sync::Arc;
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    use quinn::crypto::rustls::QuicServerConfig;
    use tokio::sync::RwLock;
    use tracing::{debug, info, warn};

    use lightspeed_protocol::control::{disconnect_reason, ControlMessage};
    use lightspeed_protocol::PROTOCOL_VERSION;

    use crate::auth::Authenticator;
    use crate::config::ProxyConfig;

    // ── Types ───────────────────────────────────────────────────────

    /// A connected client session on the control plane.
    #[derive(Debug, Clone)]
    pub struct ClientSession {
        /// Session ID assigned at registration (random).
        pub session_id: u32,
        /// Data-plane session token (random u8).
        pub session_token: u8,
        /// Client remote address.
        pub remote_addr: SocketAddr,
        /// Game the client is optimizing.
        pub game: u8,
        /// When the session was created.
        pub connected_at: Instant,
        /// Last activity timestamp.
        pub last_seen: Instant,
    }

    /// Shared state across all client handlers.
    pub struct ControlState {
        /// Active client sessions keyed by session ID.
        pub sessions: RwLock<HashMap<u32, ClientSession>>,
        /// Server configuration.
        pub config: ProxyConfig,
        /// Shared authenticator for data-plane auth.
        pub authenticator: Arc<RwLock<Authenticator>>,
        /// When the server started.
        pub started_at: Instant,
    }

    impl ControlState {
        pub fn new(config: ProxyConfig, authenticator: Arc<RwLock<Authenticator>>) -> Self {
            Self {
                sessions: RwLock::new(HashMap::new()),
                config,
                authenticator,
                started_at: Instant::now(),
            }
        }

        /// Generate a random, unique session ID.
        fn generate_session_id(&self) -> u32 {
            // Use random IDs to prevent prediction/enumeration
            rand::random::<u32>()
        }

        /// Current number of active sessions.
        pub async fn active_count(&self) -> usize {
            self.sessions.read().await.len()
        }
    }

    // ── TLS ─────────────────────────────────────────────────────────

    /// Generate a self-signed certificate for the QUIC server (dev / MVP).
    ///
    /// SECURITY: MVP only. Production deployments MUST use proper PKI
    /// with certificates issued by a trusted CA.
    fn generate_self_signed_cert() -> anyhow::Result<(
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )> {
        let key_pair = rcgen::KeyPair::generate()?;
        let cert_params = rcgen::CertificateParams::new(vec!["lightspeed-proxy".into()])?;
        let cert = cert_params.self_signed(&key_pair)?;
        let cert_der = rustls::pki_types::CertificateDer::from(cert.der().to_vec());
        let key_der =
            rustls::pki_types::PrivateKeyDer::try_from(key_pair.serialize_der()).map_err(|e| {
                anyhow::anyhow!("Failed to serialize private key: {}", e)
            })?;
        Ok((vec![cert_der], key_der))
    }

    /// Build a quinn `ServerConfig` with a self-signed certificate.
    fn build_server_config() -> anyhow::Result<quinn::ServerConfig> {
        let (certs, key) = generate_self_signed_cert()?;

        let rustls_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;

        let quic_crypto = QuicServerConfig::try_from(rustls_config)?;
        let mut server_config = quinn::ServerConfig::with_crypto(Arc::new(quic_crypto));

        // Tune transport for control plane (low bandwidth, low latency)
        let mut transport = quinn::TransportConfig::default();
        transport.keep_alive_interval(Some(Duration::from_secs(10)));
        transport.max_idle_timeout(Some(Duration::from_secs(60).try_into()?));
        server_config.transport_config(Arc::new(transport));

        Ok(server_config)
    }

    // ── Helpers ─────────────────────────────────────────────────────

    /// Extract IPv4 address from a SocketAddr (for auth binding).
    fn extract_ipv4(addr: &SocketAddr) -> Option<Ipv4Addr> {
        match addr {
            SocketAddr::V4(v4) => Some(*v4.ip()),
            SocketAddr::V6(v6) => {
                // Handle IPv4-mapped IPv6 addresses (::ffff:a.b.c.d)
                if let Some(v4) = v6.ip().to_ipv4_mapped() {
                    Some(v4)
                } else {
                    None
                }
            }
        }
    }

    // ── Server ──────────────────────────────────────────────────────

    /// Run the QUIC control plane server.
    ///
    /// Accepts connections and spawns a handler task for each client.
    pub async fn run_control_server(
        bind_addr: SocketAddr,
        state: Arc<ControlState>,
    ) -> anyhow::Result<()> {
        let server_config = build_server_config()?;
        let endpoint = quinn::Endpoint::server(server_config, bind_addr)?;

        info!(
            "QUIC control plane listening on {} (node={}, auth={})",
            bind_addr,
            state.config.server.node_id,
            if state.config.security.require_auth { "enforced" } else { "disabled" }
        );

        while let Some(incoming) = endpoint.accept().await {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                match incoming.await {
                    Ok(conn) => {
                        let remote = conn.remote_address();
                        info!("QUIC connection from {}", remote);
                        if let Err(e) = handle_connection(conn, state).await {
                            warn!("Client {} connection error: {}", remote, e);
                        }
                    }
                    Err(e) => {
                        warn!("Failed to accept QUIC connection: {}", e);
                    }
                }
            });
        }

        info!("QUIC control plane shutting down");
        Ok(())
    }

    /// Handle a single client QUIC connection.
    async fn handle_connection(
        conn: quinn::Connection,
        state: Arc<ControlState>,
    ) -> anyhow::Result<()> {
        let remote = conn.remote_address();
        let mut session_id: Option<u32> = None;

        // Accept bidirectional streams from the client
        loop {
            match conn.accept_bi().await {
                Ok((send, recv)) => {
                    let sid = session_id;
                    let state = Arc::clone(&state);
                    let remote = remote;
                    tokio::spawn(async move {
                        if let Err(e) = handle_stream(send, recv, remote, sid, state).await {
                            debug!("Stream handler error for {}: {}", remote, e);
                        }
                    });
                }
                Err(quinn::ConnectionError::ApplicationClosed(_)) => {
                    info!("Client {} closed connection gracefully", remote);
                    break;
                }
                Err(e) => {
                    warn!("Connection error from {}: {}", remote, e);
                    break;
                }
            }
        }

        // Clean up session and revoke data-plane auth
        if let Some(sid) = session_id {
            state.sessions.write().await.remove(&sid);
            info!("Removed session {} for {}", sid, remote);
        }

        // Revoke data-plane authorization for this client's IP
        if let Some(ipv4) = extract_ipv4(&remote) {
            let mut auth = state.authenticator.write().await;
            auth.revoke(&ipv4);
        }

        Ok(())
    }

    /// Handle a single bidirectional stream.
    async fn handle_stream(
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
        remote: SocketAddr,
        _session_id: Option<u32>,
        state: Arc<ControlState>,
    ) -> anyhow::Result<()> {
        while let Some(msg) = ControlMessage::read_from(&mut recv).await? {
            let response = process_message(msg, remote, &state).await;
            if let Some(resp) = response {
                resp.write_to(&mut send).await?;
            }
        }
        send.finish()?;
        Ok(())
    }

    /// Process one control message and optionally return a response.
    async fn process_message(
        msg: ControlMessage,
        remote: SocketAddr,
        state: &ControlState,
    ) -> Option<ControlMessage> {
        match msg {
            ControlMessage::Ping { timestamp_us } => {
                let now_us = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u64;
                debug!("Ping from {} (ts={})", remote, timestamp_us);
                Some(ControlMessage::Pong {
                    client_timestamp_us: timestamp_us,
                    server_timestamp_us: now_us,
                })
            }

            ControlMessage::Register {
                protocol_version,
                game,
            } => {
                if protocol_version != PROTOCOL_VERSION {
                    warn!(
                        "Client {} has protocol version {} (expected {})",
                        remote, protocol_version, PROTOCOL_VERSION
                    );
                }

                // Check capacity
                let current = state.active_count().await;
                if current >= state.config.server.max_clients {
                    warn!(
                        "Rejecting client {} — at capacity ({}/{})",
                        remote, current, state.config.server.max_clients
                    );
                    return Some(ControlMessage::Disconnect {
                        reason: disconnect_reason::RATE_LIMITED,
                    });
                }

                // Generate random session ID and token
                let session_id = state.generate_session_id();
                let session_token = Authenticator::generate_token();

                let session = ClientSession {
                    session_id,
                    session_token,
                    remote_addr: remote,
                    game,
                    connected_at: Instant::now(),
                    last_seen: Instant::now(),
                };

                state
                    .sessions
                    .write()
                    .await
                    .insert(session_id, session);

                // Authorize client's IP on the data plane
                if let Some(ipv4) = extract_ipv4(&remote) {
                    let mut auth = state.authenticator.write().await;
                    auth.authorize(ipv4, session_token);
                    info!(
                        "Registered client {} → session {} token={} (game={})",
                        remote, session_id, session_token, game
                    );
                } else {
                    warn!(
                        "Client {} has non-IPv4 address, data-plane auth skipped",
                        remote
                    );
                }

                Some(ControlMessage::RegisterAck {
                    session_id,
                    session_token,
                    node_id: state.config.server.node_id.clone(),
                    region: state.config.server.region.clone(),
                })
            }

            ControlMessage::Disconnect { reason } => {
                info!(
                    "Client {} disconnecting (reason={})",
                    remote, reason
                );

                // Revoke data-plane auth
                if let Some(ipv4) = extract_ipv4(&remote) {
                    let mut auth = state.authenticator.write().await;
                    auth.revoke(&ipv4);
                }

                None
            }

            other => {
                debug!(
                    "Unexpected message from {}: {:?}",
                    remote, other
                );
                None
            }
        }
    }
}

// ── Re-exports ──────────────────────────────────────────────────────

#[cfg(feature = "quic")]
pub use inner::{run_control_server, ClientSession, ControlState};
