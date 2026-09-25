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
    use lightspeed_protocol::telemetry::MAX_TELEMETRY_BODY;
    use lightspeed_protocol::PROTOCOL_VERSION;

    use crate::auth::{Authenticator, EXPLICIT_REVOKE_GRACE, TRANSPORT_REVOKE_GRACE};
    use crate::config::ProxyConfig;
    use crate::metrics::ProxyMetrics;

    // ── Types ───────────────────────────────────────────────────────

    /// The data-plane token issued to a QUIC connection, shared across the
    /// connection's streams so keepalive refresh and close-time revoke reach
    /// the same token the Register handler issued.
    type ConnectionToken = Arc<tokio::sync::Mutex<Option<u32>>>;

    /// Bound on distinct source IPs tracked by the control-plane telemetry
    /// limiter, so a client cannot grow the map without bound.
    const MAX_TELEMETRY_RATE_IPS: usize = 65_536;

    /// Telemetry reports accepted per source IP per fixed window.
    const TELEMETRY_REPORTS_PER_WINDOW: u32 = 60;

    /// Fixed-window duration for control-plane telemetry throttling.
    const TELEMETRY_RATE_WINDOW: Duration = Duration::from_secs(60);

    /// Per-IP fixed-window limiter for control-plane telemetry reports.
    ///
    /// The HTTP endpoint relies on the general request handling; the QUIC path
    /// gets an explicit bound so a client cannot flood the aggregator over an
    /// authenticated connection. A new IP past the tracking cap is refused
    /// (fail closed).
    #[derive(Default)]
    struct TelemetryRateLimiter {
        ips: HashMap<Ipv4Addr, (Instant, u32)>,
    }

    impl TelemetryRateLimiter {
        fn allow(&mut self, ip: Ipv4Addr, now: Instant) -> bool {
            if !self.ips.contains_key(&ip) && self.ips.len() >= MAX_TELEMETRY_RATE_IPS {
                return false;
            }
            let entry = self.ips.entry(ip).or_insert((now, 0));
            if now.saturating_duration_since(entry.0) >= TELEMETRY_RATE_WINDOW {
                *entry = (now, 0);
            }
            if entry.1 >= TELEMETRY_REPORTS_PER_WINDOW {
                return false;
            }
            entry.1 += 1;
            true
        }
    }

    /// A connected client session on the control plane.
    #[derive(Debug, Clone)]
    pub struct ClientSession {
        /// Session ID assigned at registration (random).
        pub session_id: u32,
        /// Data-plane session token (random u32).
        pub session_token: u32,
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
        /// Live QUIC connections keyed by stable connection id. Kept so the
        /// server can announce a graceful shutdown to every client before the
        /// endpoint closes.
        pub connections: RwLock<HashMap<usize, quinn::Connection>>,
        /// Server configuration.
        pub config: ProxyConfig,
        /// Shared authenticator for data-plane auth.
        pub authenticator: Arc<RwLock<Authenticator>>,
        /// Shared metrics, including the telemetry aggregator that the HTTP
        /// `/telemetry` endpoint also feeds, so both transports aggregate
        /// identically.
        pub metrics: Arc<ProxyMetrics>,
        /// Per-IP limiter for control-plane telemetry reports.
        telemetry_rate: tokio::sync::Mutex<TelemetryRateLimiter>,
        /// When the server started.
        pub started_at: Instant,
    }

    impl ControlState {
        pub fn new(
            config: ProxyConfig,
            authenticator: Arc<RwLock<Authenticator>>,
            metrics: Arc<ProxyMetrics>,
        ) -> Self {
            Self {
                sessions: RwLock::new(HashMap::new()),
                connections: RwLock::new(HashMap::new()),
                config,
                authenticator,
                metrics,
                telemetry_rate: tokio::sync::Mutex::new(TelemetryRateLimiter::default()),
                started_at: Instant::now(),
            }
        }

        /// Generate a random, collision-free session ID.
        async fn generate_session_id(&self) -> u32 {
            // Random IDs prevent prediction/enumeration; the loop retries on
            // the (birthday-bound) chance of a 32-bit collision with an active
            // session, which would otherwise silently overwrite it.
            loop {
                let id = rand::random::<u32>();
                if !self.sessions.read().await.contains_key(&id) {
                    return id;
                }
            }
        }

        /// Current number of active sessions.
        pub async fn active_count(&self) -> usize {
            self.sessions.read().await.len()
        }
    }

    // ── TLS ─────────────────────────────────────────────────────────

    /// Directory holding the persistent TLS cert and key.
    fn tls_dir() -> std::path::PathBuf {
        std::path::PathBuf::from(
            std::env::var("LIGHTSPEED_TLS_DIR")
                .unwrap_or_else(|_| "/var/lib/lightspeed/tls".to_string()),
        )
    }

    /// Load a persistent self-signed certificate, or generate + store one on
    /// first boot. A stable certificate is required for client trust-on-first-
    /// use pinning: a fresh per-boot cert would change the fingerprint on every
    /// restart and force clients to re-pin (and look like a MITM).
    fn load_or_generate_cert() -> anyhow::Result<(
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )> {
        let dir = tls_dir();
        let cert_path = dir.join("cert.der");
        let key_path = dir.join("key.der");

        if cert_path.exists() && key_path.exists() {
            let cert_der = rustls::pki_types::CertificateDer::from(std::fs::read(&cert_path)?);
            let key_der = rustls::pki_types::PrivateKeyDer::try_from(std::fs::read(&key_path)?)
                .map_err(|e| anyhow::anyhow!("Failed to load private key: {}", e))?;
            return Ok((vec![cert_der], key_der));
        }

        let key_pair = rcgen::KeyPair::generate()?;
        let cert_params = rcgen::CertificateParams::new(vec!["lightspeed-proxy".into()])?;
        let cert = cert_params.self_signed(&key_pair)?;
        let cert_der = rustls::pki_types::CertificateDer::from(cert.der().to_vec());
        let key_der = rustls::pki_types::PrivateKeyDer::try_from(key_pair.serialize_der())
            .map_err(|e| anyhow::anyhow!("Failed to serialize private key: {}", e))?;

        std::fs::create_dir_all(&dir)?;
        std::fs::write(&cert_path, cert_der.as_ref())?;
        std::fs::write(&key_path, key_der.secret_der())?;

        Ok((vec![cert_der], key_der))
    }

    /// Validate TLS assets without binding a port or generating anything.
    ///
    /// Fails when an existing cert/key pair is incomplete or unreadable, or
    /// when first-boot generation would fail because no ancestor of the TLS
    /// directory is writable. Used by `--check`.
    pub fn validate_tls_assets() -> anyhow::Result<()> {
        let dir = tls_dir();
        let cert_path = dir.join("cert.der");
        let key_path = dir.join("key.der");
        let cert_exists = cert_path.exists();
        let key_exists = key_path.exists();

        if cert_exists || key_exists {
            if !cert_exists || !key_exists {
                anyhow::bail!("TLS cert/key are not a matching pair in {}", dir.display());
            }
            std::fs::read(&cert_path)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {}", cert_path.display(), e))?;
            std::fs::read(&key_path)
                .map_err(|e| anyhow::anyhow!("cannot read {}: {}", key_path.display(), e))?;
            return Ok(());
        }

        // First boot generates the pair, so the nearest existing ancestor must
        // be writable for `create_dir_all` + the cert writes to succeed.
        let mut ancestor = dir.as_path();
        while !ancestor.exists() {
            ancestor = ancestor.parent().ok_or_else(|| {
                anyhow::anyhow!("no existing ancestor for TLS dir {}", dir.display())
            })?;
        }
        let probe_path = ancestor.join(".lightspeed-tls-write-probe");
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&probe_path)
        {
            Ok(file) => {
                drop(file);
                let _ = std::fs::remove_file(&probe_path);
                Ok(())
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
            Err(e) => anyhow::bail!(
                "TLS dir ancestor {} is not writable: {}",
                ancestor.display(),
                e
            ),
        }
    }

    /// Build a quinn `ServerConfig` with a self-signed certificate.
    fn build_server_config() -> anyhow::Result<quinn::ServerConfig> {
        // quinn pulls rustls with its aws-lc-rs default while this crate selects
        // ring, so two providers are compiled in and `ServerConfig::builder()`
        // panics on auto-detection. Pin ring as the process-wide provider.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (certs, key) = load_or_generate_cert()?;

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
                v6.ip().to_ipv4_mapped()
            }
        }
    }

    // ── Server ──────────────────────────────────────────────────────

    /// A bound QUIC control-plane endpoint.
    ///
    /// Binding is separate from serving so a caller can detect a bind failure
    /// before committing to run, and so it can drive a graceful shutdown.
    pub struct ControlServer {
        endpoint: quinn::Endpoint,
        state: Arc<ControlState>,
    }

    impl ControlServer {
        /// Bind the QUIC control endpoint on `bind_addr`.
        pub fn bind(bind_addr: SocketAddr, state: Arc<ControlState>) -> anyhow::Result<Self> {
            let server_config = build_server_config()?;
            let endpoint = quinn::Endpoint::server(server_config, bind_addr)?;
            info!(
                "QUIC control plane listening on {} (node={}, auth={})",
                endpoint.local_addr()?,
                state.config.server.node_id,
                if state.config.security.require_auth {
                    "enforced"
                } else {
                    "disabled"
                }
            );
            Ok(Self { endpoint, state })
        }

        /// The actual bound address (resolves port 0 to the assigned port).
        pub fn local_addr(&self) -> std::io::Result<SocketAddr> {
            self.endpoint.local_addr()
        }

        /// Accept control connections until `shutdown` flips to `true`, then
        /// announce a server shutdown to every live connection, close the
        /// endpoint, and return.
        pub async fn run(self, mut shutdown: tokio::sync::watch::Receiver<bool>) {
            // Cap concurrent control connections so idle pre-registration
            // connections can't exhaust task/memory (only max_clients is enforced
            // at Register).
            let conn_limit = self.state.config.server.max_clients.max(1);
            let semaphore = Arc::new(tokio::sync::Semaphore::new(conn_limit));

            loop {
                tokio::select! {
                    biased;
                    _ = shutdown.changed() => break,
                    incoming = self.endpoint.accept() => {
                        let Some(incoming) = incoming else { break; };
                        let state = Arc::clone(&self.state);
                        let semaphore = Arc::clone(&semaphore);
                        tokio::spawn(async move {
                            let Ok(conn) = incoming.await else {
                                return;
                            };
                            let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                                conn.close(0u32.into(), b"at capacity");
                                return;
                            };
                            let remote = conn.remote_address();
                            let conn_id = conn.stable_id();
                            state.connections.write().await.insert(conn_id, conn.clone());
                            info!("QUIC connection from {}", remote);
                            if let Err(e) = handle_connection(conn, Arc::clone(&state)).await {
                                warn!("Client {} connection error: {}", remote, e);
                            }
                            state.connections.write().await.remove(&conn_id);
                            drop(permit);
                        });
                    }
                }
            }

            self.announce_shutdown().await;
            self.endpoint.close(0u32.into(), b"server shutdown");
            info!("QUIC control plane shutting down");
        }

        /// Send `Disconnect { SERVER_SHUTDOWN }` to every live connection.
        ///
        /// Each write is individually time-bounded so one slow client cannot
        /// stall the shutdown; the caller additionally bounds the whole `run`.
        async fn announce_shutdown(&self) {
            let connections: Vec<quinn::Connection> = self
                .state
                .connections
                .read()
                .await
                .values()
                .cloned()
                .collect();
            let count = connections.len();
            let msg = ControlMessage::Disconnect {
                reason: disconnect_reason::SERVER_SHUTDOWN,
            };
            for conn in &connections {
                let opened = tokio::time::timeout(Duration::from_secs(1), conn.open_bi()).await;
                if let Ok(Ok((mut send, _recv))) = opened {
                    let _ =
                        tokio::time::timeout(Duration::from_secs(1), msg.write_to(&mut send)).await;
                    let _ = send.finish();
                }
            }

            // QUIC may drop received-but-undelivered stream data when a
            // CONNECTION_CLOSE arrives, so closing immediately races the
            // client's read of the Disconnect. Wait (bounded) for peers to
            // consume it and close first.
            let drain = async {
                let mut waiting = tokio::task::JoinSet::new();
                for conn in connections {
                    waiting.spawn(async move {
                        conn.closed().await;
                    });
                }
                while waiting.join_next().await.is_some() {}
            };
            let _ = tokio::time::timeout(Duration::from_secs(2), drain).await;
            info!(
                "Announced server shutdown to {} control connection(s)",
                count
            );
        }
    }

    /// Convenience wrapper that runs without a shutdown channel.
    ///
    /// It never triggers the graceful announcement on its own; callers that
    /// need one should hold a [`ControlServer`] and drive [`ControlServer::run`]
    /// with a shutdown receiver.
    pub async fn run_control_server(
        bind_addr: SocketAddr,
        state: Arc<ControlState>,
    ) -> anyhow::Result<()> {
        let (_shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        ControlServer::bind(bind_addr, state)?
            .run(shutdown_rx)
            .await;
        Ok(())
    }

    /// Handle a single client QUIC connection.
    async fn handle_connection(
        conn: quinn::Connection,
        state: Arc<ControlState>,
    ) -> anyhow::Result<()> {
        let remote = conn.remote_address();
        let issued_token: ConnectionToken = Arc::new(tokio::sync::Mutex::new(None));

        // Accept bidirectional streams from the client
        loop {
            match conn.accept_bi().await {
                Ok((send, recv)) => {
                    let state = Arc::clone(&state);
                    let issued_token = Arc::clone(&issued_token);
                    tokio::spawn(async move {
                        if let Err(e) = handle_stream(send, recv, remote, state, issued_token).await
                        {
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

        // Collect every token issued to this connection before dropping its
        // sessions, then revoke with a reconnect grace instead of deleting, so
        // a client that reconnects immediately is not deauthorized mid-flight.
        let mut issued = Vec::new();
        {
            let mut sessions = state.sessions.write().await;
            sessions.retain(|_, s| {
                let keep = s.remote_addr != remote;
                if !keep {
                    issued.push(s.session_token);
                }
                keep
            });
        }
        if let Some(token) = issued_token.lock().await.take() {
            issued.push(token);
        }
        if !issued.is_empty() {
            let now = Instant::now();
            let mut auth = state.authenticator.write().await;
            for token in issued {
                auth.revoke(token, now, TRANSPORT_REVOKE_GRACE);
            }
        }

        Ok(())
    }

    /// Handle a single bidirectional stream.
    async fn handle_stream(
        mut send: quinn::SendStream,
        mut recv: quinn::RecvStream,
        remote: SocketAddr,
        state: Arc<ControlState>,
        issued_token: ConnectionToken,
    ) -> anyhow::Result<()> {
        while let Some(msg) = ControlMessage::read_from(&mut recv).await? {
            let response = process_message(msg, remote, &state, &issued_token).await;
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
        issued_token: &ConnectionToken,
    ) -> Option<ControlMessage> {
        match msg {
            ControlMessage::Ping { timestamp_us } => {
                // A live control connection is evidence the client is still
                // present: extend its data-plane token so it never expires.
                let token = { *issued_token.lock().await };
                if let Some(token) = token {
                    state
                        .authenticator
                        .write()
                        .await
                        .refresh(token, Instant::now());
                }
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
                data_port,
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
                let session_id = state.generate_session_id().await;
                let session_token = Authenticator::generate_token();

                let session = ClientSession {
                    session_id,
                    session_token,
                    remote_addr: remote,
                    game,
                    connected_at: Instant::now(),
                    last_seen: Instant::now(),
                };

                state.sessions.write().await.insert(session_id, session);

                // Authorize this token on the data plane. The register data port
                // pins the token when non-zero; a client that omits it keeps
                // principal-only binding.
                if let Some(ipv4) = extract_ipv4(&remote) {
                    state.authenticator.write().await.authorize(
                        ipv4,
                        data_port,
                        session_token,
                        Instant::now(),
                    );
                    *issued_token.lock().await = Some(session_token);
                    info!(
                        "Registered client {} → session {} token={} (game={}, data_port={})",
                        remote, session_id, session_token, game, data_port
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
                    telemetry_quic: true,
                })
            }

            ControlMessage::Disconnect { reason } => {
                info!("Client {} disconnecting (reason={})", remote, reason);

                // Explicit disconnect gets a short grace; abuse bans use
                // `Authenticator::ban` for immediate removal.
                let token = issued_token.lock().await.take();
                if let Some(token) = token {
                    state.authenticator.write().await.revoke(
                        token,
                        Instant::now(),
                        EXPLICIT_REVOKE_GRACE,
                    );
                }

                None
            }

            ControlMessage::Telemetry { report_json } => {
                if report_json.len() > MAX_TELEMETRY_BODY {
                    warn!(
                        "Telemetry from {} rejected: {} bytes exceeds {} cap",
                        remote,
                        report_json.len(),
                        MAX_TELEMETRY_BODY
                    );
                    return None;
                }
                let Some(ip) = extract_ipv4(&remote) else {
                    debug!("Telemetry from non-IPv4 {} ignored", remote);
                    return None;
                };
                if !state.telemetry_rate.lock().await.allow(ip, Instant::now()) {
                    debug!("Telemetry from {} rate-limited", remote);
                    return None;
                }
                // Same parse, validate, and aggregation path as the HTTP
                // endpoint, so the metric meaning is identical across
                // transports.
                match crate::health::ingest_telemetry(&state.metrics, &report_json, remote.ip()) {
                    Ok(()) => debug!("Telemetry from {} ingested over control plane", remote),
                    Err(e) => warn!("Telemetry from {} rejected: {}", remote, e),
                }
                None
            }

            other => {
                debug!("Unexpected message from {}: {:?}", remote, other);
                None
            }
        }
    }
}

// ── Re-exports ──────────────────────────────────────────────────────

#[cfg(feature = "quic")]
pub use inner::{
    run_control_server, validate_tls_assets, ClientSession, ControlServer, ControlState,
};
