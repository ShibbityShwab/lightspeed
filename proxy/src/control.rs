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

    use lightspeed_protocol::control::{caps, disconnect_reason, ControlMessage};
    use lightspeed_protocol::telemetry::MAX_TELEMETRY_BODY;
    use lightspeed_protocol::PROTOCOL_VERSION;
    use lightspeed_protocol::{quic_control_alpn, QUIC_CONTROL_ALPN_V1};

    use crate::auth::{Authenticator, EXPLICIT_REVOKE_GRACE, TRANSPORT_REVOKE_GRACE};
    use crate::config::{ControlAlpn, ProxyConfig};
    use crate::geo::{self, GeoResolver};
    use crate::metrics::{ControlHandshakeOutcome, ProxyMetrics};

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
        /// Geo resolver used to place a client's registered destination, when
        /// the operator has an MMDB enabled.
        pub geo: Option<Arc<dyn GeoResolver>>,
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
            geo: Option<Arc<dyn GeoResolver>>,
        ) -> Self {
            // Publish the startup-time policy on the shared metrics handle: the
            // `/health` endpoint is served from that handle and must report the
            // control-plane ALPN mode without a second copy of `ProxyConfig`.
            metrics.set_control_alpn(config.network.control_alpn);
            Self {
                sessions: RwLock::new(HashMap::new()),
                connections: RwLock::new(HashMap::new()),
                config,
                authenticator,
                metrics,
                geo,
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

    /// Serialises first-boot TLS generation.
    ///
    /// The check-then-generate path below is not re-entrant: two concurrent
    /// callers can both see the files missing, generate different pairs and then
    /// interleave the two writes, leaving one generation's certificate beside
    /// another's key (rustls reports `KeyMismatch`). Production calls it once at
    /// startup, but the proxy's unit tests legitimately bind several control
    /// servers at the same time - which is how this was found by CI's
    /// `QUIC Integration Tests` job on 2026-11-16.
    static TLS_CERT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Load a persistent self-signed certificate, or generate + store one on
    /// first boot. A stable certificate is required for client trust-on-first-
    /// use pinning: a fresh per-boot cert would change the fingerprint on every
    /// restart and force clients to re-pin (and look like a MITM).
    fn load_or_generate_cert() -> anyhow::Result<(
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )> {
        ensure_cert_in(&tls_dir())
    }

    /// The load-or-generate body for one TLS directory, serialised process-wide.
    fn ensure_cert_in(
        dir: &std::path::Path,
    ) -> anyhow::Result<(
        Vec<rustls::pki_types::CertificateDer<'static>>,
        rustls::pki_types::PrivateKeyDer<'static>,
    )> {
        let _guard = TLS_CERT_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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

        std::fs::create_dir_all(dir)?;
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
    ///
    /// `mode` is the control-plane ALPN policy and the only difference the
    /// matrix tests vary:
    /// - [`ControlAlpn::Off`] leaves `alpn_protocols` empty, which is the
    ///   pre-rollout behaviour byte for byte: ALPN-less clients complete the
    ///   handshake and clients that offer ALPN are rejected with alert 120.
    /// - [`ControlAlpn::Required`] advertises this build's control ALPN, so the
    ///   handshake only completes when the client offers the same name.
    fn build_server_config(mode: ControlAlpn) -> anyhow::Result<quinn::ServerConfig> {
        // quinn pulls rustls with its aws-lc-rs default while this crate selects
        // ring, so two providers are compiled in and `ServerConfig::builder()`
        // panics on auto-detection. Pin ring as the process-wide provider.
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (certs, key) = load_or_generate_cert()?;

        let mut rustls_config = rustls::ServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)?;
        if mode == ControlAlpn::Required {
            rustls_config.alpn_protocols = quic_control_alpn();
        }

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

    /// TLS alert `no_application_protocol` (RFC 9001 §8.1; QUIC error 0x0178).
    const ALERT_NO_APPLICATION_PROTOCOL: u8 = 0x78;

    /// Why a QUIC control handshake did not complete.
    ///
    /// Distinct from a connection error after a successful handshake: a
    /// rejection is a client that never got a session, which is exactly the
    /// population `control_alpn = "required"` breaks for pre-ALPN clients.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum HandshakeRejection {
        /// TLS alert 120: no application protocol could be agreed (the client
        /// offered no ALPN, or a revision the relay does not speak).
        NoApplicationProtocol,
        /// Any other handshake failure (timeout, transport error, ...).
        Other,
    }

    impl HandshakeRejection {
        /// Classify a failed `Incoming` handshake.
        ///
        /// The alert reaches the accepting side as a
        /// [`quinn::ConnectionError::TransportError`] carrying the crypto code;
        /// [`quinn::ConnectionError::ConnectionClosed`] is accepted too so a
        /// peer-sent alert classifies the same way instead of falling into
        /// `other`.
        fn classify(error: &quinn::ConnectionError) -> Self {
            let code = match error {
                quinn::ConnectionError::TransportError(error) => Some(error.code),
                quinn::ConnectionError::ConnectionClosed(close) => Some(close.error_code),
                _ => None,
            };
            if code
                == Some(quinn::TransportErrorCode::crypto(
                    ALERT_NO_APPLICATION_PROTOCOL,
                ))
            {
                Self::NoApplicationProtocol
            } else {
                Self::Other
            }
        }

        /// The metric outcome this rejection counts as.
        fn outcome(self) -> ControlHandshakeOutcome {
            match self {
                Self::NoApplicationProtocol => {
                    ControlHandshakeOutcome::RejectedNoApplicationProtocol
                }
                Self::Other => ControlHandshakeOutcome::RejectedOther,
            }
        }

        /// The `reason=` log value.
        fn as_str(self) -> &'static str {
            match self {
                Self::NoApplicationProtocol => "no_application_protocol",
                Self::Other => "other",
            }
        }
    }

    /// Startup label for the control-plane ALPN policy.
    ///
    /// `off` and `required (<name>)` are the only two states an operator can
    /// see; they are the same two values `/health` reports as data.
    fn alpn_startup_label(mode: ControlAlpn) -> String {
        match mode {
            ControlAlpn::Off => "off".to_string(),
            ControlAlpn::Required => {
                format!(
                    "required ({})",
                    String::from_utf8_lossy(QUIC_CONTROL_ALPN_V1)
                )
            }
        }
    }

    /// The control-plane ALPN this connection negotiated, if any.
    ///
    /// `None` is a completed handshake without ALPN (relay `off` + ALPN-less
    /// client), which is counted separately from a negotiation so the relay
    /// can report its client population split.
    fn negotiated_control_alpn(conn: &quinn::Connection) -> Option<Vec<u8>> {
        let protocol = conn
            .handshake_data()?
            .downcast::<quinn::crypto::rustls::HandshakeData>()
            .ok()?
            .protocol?;
        quic_control_alpn()
            .into_iter()
            .find(|offered| offered == &protocol)
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
            let alpn = state.config.network.control_alpn;
            let server_config = build_server_config(alpn)?;
            let endpoint = quinn::Endpoint::server(server_config, bind_addr)?;
            // The single operator self-check for the ALPN stage: "off" is stage 2,
            // "required (<name>)" is stage 3/4 (docs/quic-alpn-rollout.md §7.3).
            info!(
                "QUIC control plane listening on {} (node={}, auth={}, alpn={})",
                endpoint.local_addr()?,
                state.config.server.node_id,
                if state.config.security.require_auth {
                    "enforced"
                } else {
                    "disabled"
                },
                alpn_startup_label(alpn),
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
                            let peer = incoming.remote_address();
                            let conn = match incoming.await {
                                Ok(conn) => conn,
                                Err(error) => {
                                    // A rejected handshake used to be silent. The
                                    // split is what a canary watches: alert 120 is a
                                    // pre-ALPN client cut off by `required` mode.
                                    let rejection = HandshakeRejection::classify(&error);
                                    state
                                        .metrics
                                        .record_control_handshake(rejection.outcome());
                                    warn!(
                                        remote = %peer,
                                        error = %error,
                                        reason = rejection.as_str(),
                                        "QUIC control handshake rejected"
                                    );
                                    return;
                                }
                            };
                            let Ok(permit) = semaphore.clone().try_acquire_owned() else {
                                conn.close(0u32.into(), b"at capacity");
                                return;
                            };
                            let remote = conn.remote_address();
                            let conn_id = conn.stable_id();
                            state.connections.write().await.insert(conn_id, conn.clone());
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
    ///
    /// The handshake already completed here, so this is the one place the
    /// negotiated ALPN is read and counted: `negotiated_v1` and `no_alpn` are
    /// the relay's client-population split, and the value is attached to the
    /// per-connection log so a capture can be matched to a mode.
    async fn handle_connection(
        conn: quinn::Connection,
        state: Arc<ControlState>,
    ) -> anyhow::Result<()> {
        let remote = conn.remote_address();
        let negotiated = negotiated_control_alpn(&conn);
        state
            .metrics
            .record_control_handshake(if negotiated.is_some() {
                ControlHandshakeOutcome::NegotiatedV1
            } else {
                ControlHandshakeOutcome::NoAlpn
            });
        info!(
            "QUIC connection from {} (alpn={})",
            remote,
            negotiated
                .as_deref()
                .map(|protocol| String::from_utf8_lossy(protocol).into_owned())
                .unwrap_or_else(|| "none".to_string())
        );
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
                destination,
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
                        "Rejecting client {} - at capacity ({}/{})",
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
                    dest_region: destination.and_then(|ip| {
                        let geo = state.geo.as_deref()?;
                        geo::destination_region(geo, ip)
                    }),
                    // This proxy implements the v5 TCP splice; advertise it so
                    // clients can offer TCP game profiles.
                    caps: caps::TCP_TUNNEL,
                })
            }

            ControlMessage::RotateRequest { current_token } => {
                let Some(ipv4) = extract_ipv4(&remote) else {
                    warn!("Rotate request from non-IPv4 {} ignored", remote);
                    return None;
                };

                // Only the token this connection was actually issued may be
                // rotated: an unregistered connection holds none, and a client
                // cannot rotate a token it was never given.
                let held = *issued_token.lock().await;
                if held != Some(current_token) {
                    debug!(
                        "Rotate request from {} refused: token {} is not this connection's",
                        remote, current_token
                    );
                    return None;
                }

                // Rotation adds an entry and demotes the presented token, so a
                // full table refuses it exactly as registration does.
                let now = Instant::now();
                let rotated = {
                    let mut auth = state.authenticator.write().await;
                    auth.rotate(ipv4, current_token, now)
                };
                let new_token = rotated?;

                // Move the connection and its session record onto the fresh
                // token so keepalive refresh and close-time revoke both target
                // the token the client will now use.
                *issued_token.lock().await = Some(new_token);
                for session in state.sessions.write().await.values_mut() {
                    if session.remote_addr == remote {
                        session.session_token = new_token;
                    }
                }

                info!(
                    "Rotated session token for {}: {} -> {}",
                    remote, current_token, new_token
                );
                Some(ControlMessage::RotateAck {
                    new_token,
                    old_token: current_token,
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

    #[cfg(test)]
    mod tests {
        use super::*;

        fn state_with_geo(resolver: Option<Arc<dyn GeoResolver>>) -> Arc<ControlState> {
            Arc::new(ControlState::new(
                ProxyConfig::default(),
                Arc::new(RwLock::new(Authenticator::new(false))),
                Arc::new(ProxyMetrics::new()),
                resolver,
            ))
        }

        fn state_with_auth() -> Arc<ControlState> {
            Arc::new(ControlState::new(
                ProxyConfig::default(),
                Arc::new(RwLock::new(Authenticator::new(true))),
                Arc::new(ProxyMetrics::new()),
                None,
            ))
        }

        /// Register over `process_message` and return the issued session token.
        async fn register_for_token(
            state: &ControlState,
            token: &ConnectionToken,
            data_port: u16,
        ) -> u32 {
            let response = process_message(
                ControlMessage::Register {
                    protocol_version: PROTOCOL_VERSION,
                    game: 0,
                    data_port,
                    destination: None,
                },
                "127.0.0.1:40000".parse().unwrap(),
                state,
                token,
            )
            .await;
            match response {
                Some(ControlMessage::RegisterAck { session_token, .. }) => session_token,
                other => panic!("expected RegisterAck, got {other:?}"),
            }
        }

        async fn register(
            state: &ControlState,
            token: &ConnectionToken,
            destination: Ipv4Addr,
        ) -> Option<String> {
            let response = process_message(
                ControlMessage::Register {
                    protocol_version: PROTOCOL_VERSION,
                    game: 0,
                    data_port: 0,
                    destination: Some(destination),
                },
                "127.0.0.1:40000".parse().unwrap(),
                state,
                token,
            )
            .await;
            match response {
                Some(ControlMessage::RegisterAck { dest_region, .. }) => dest_region,
                other => panic!("expected RegisterAck, got {other:?}"),
            }
        }

        #[tokio::test]
        async fn register_ack_carries_the_resolved_destination_region() {
            let resolver: Arc<dyn GeoResolver> = Arc::new(|ip: Ipv4Addr| {
                (ip == Ipv4Addr::new(8, 8, 8, 8)).then(|| "AU".to_string())
            });
            let state = state_with_geo(Some(resolver));
            let token: ConnectionToken = Arc::new(tokio::sync::Mutex::new(None));

            assert_eq!(
                register(&state, &token, Ipv4Addr::new(8, 8, 8, 8))
                    .await
                    .as_deref(),
                Some("AU"),
                "the relay must resolve a known destination through its MMDB"
            );
        }

        #[tokio::test]
        async fn register_ack_omits_the_region_without_a_resolver_or_a_known_address() {
            let token: ConnectionToken = Arc::new(tokio::sync::Mutex::new(None));

            let no_resolver = state_with_geo(None);
            assert_eq!(
                register(&no_resolver, &token, Ipv4Addr::new(8, 8, 8, 8)).await,
                None,
                "a relay without an MMDB must not invent a region"
            );

            let resolver: Arc<dyn GeoResolver> = Arc::new(|_ip: Ipv4Addr| None);
            let unplaceable = state_with_geo(Some(resolver));
            assert_eq!(
                register(&unplaceable, &token, Ipv4Addr::new(9, 9, 9, 9)).await,
                None,
                "an address the MMDB cannot place must leave the region unset"
            );
        }

        #[tokio::test]
        async fn rotate_request_swaps_the_connections_token() {
            let state = state_with_auth();
            let token: ConnectionToken = Arc::new(tokio::sync::Mutex::new(None));
            let remote: SocketAddr = "127.0.0.1:40000".parse().unwrap();

            let old = register_for_token(&state, &token, 0).await;
            let response = process_message(
                ControlMessage::RotateRequest { current_token: old },
                remote,
                &state,
                &token,
            )
            .await;

            let (new, acked_old) = match response {
                Some(ControlMessage::RotateAck {
                    new_token,
                    old_token,
                }) => (new_token, old_token),
                other => panic!("expected RotateAck, got {other:?}"),
            };
            assert_eq!(acked_old, old, "the ack names the superseded token");
            assert_ne!(new, old, "the ack carries a fresh token");
            assert_eq!(
                *token.lock().await,
                Some(new),
                "the connection must hold the token it was just issued"
            );

            let principal = Ipv4Addr::new(127, 0, 0, 1);
            let now = Instant::now();
            {
                let auth = state.authenticator.read().await;
                assert!(auth.validate(principal, 0, new, now), "new token is live");
                assert!(
                    auth.validate(principal, 0, old, now),
                    "old token is in grace"
                );
                assert!(
                    !auth.validate(Ipv4Addr::new(10, 0, 0, 9), 0, new, now),
                    "the fresh token stays bound to the requesting principal"
                );
            }

            let sessions = state.sessions.read().await;
            assert!(
                sessions.values().any(|s| s.session_token == new),
                "the session record must track the rotated token"
            );
        }

        #[tokio::test]
        async fn rotate_without_registration_is_refused() {
            let state = state_with_auth();
            let token: ConnectionToken = Arc::new(tokio::sync::Mutex::new(None));
            let remote: SocketAddr = "127.0.0.1:40000".parse().unwrap();

            let response = process_message(
                ControlMessage::RotateRequest {
                    current_token: 0xABCD_1234,
                },
                remote,
                &state,
                &token,
            )
            .await;

            assert!(
                response.is_none(),
                "an unregistered connection must not rotate"
            );
            assert_eq!(state.authenticator.read().await.client_count(), 0);
        }

        #[tokio::test]
        async fn rotate_with_a_token_this_connection_does_not_hold_is_refused() {
            let state = state_with_auth();
            let token: ConnectionToken = Arc::new(tokio::sync::Mutex::new(None));
            let remote: SocketAddr = "127.0.0.1:40000".parse().unwrap();

            let held = register_for_token(&state, &token, 0).await;
            let response = process_message(
                ControlMessage::RotateRequest {
                    current_token: held ^ 0xFFFF_FFFF,
                },
                remote,
                &state,
                &token,
            )
            .await;

            assert!(response.is_none(), "a foreign token must not rotate");
            assert_eq!(*token.lock().await, Some(held));
            assert!(state.authenticator.read().await.validate(
                Ipv4Addr::new(127, 0, 0, 1),
                0,
                held,
                Instant::now()
            ));
        }

        // ── §6.3 ALPN matrix: the five rows that decide the rollout order ──
        //
        // Each test binds a real `ControlServer` on 127.0.0.1:0 and drives a
        // real quinn client. `alpn_protocols` is the only variable, so a
        // failure names the handshake policy rather than test scaffolding. The
        // rows and their meaning are in docs/quic-alpn-rollout.md §3/§6.3.

        use crate::config::ControlAlpn;

        use std::sync::Once;

        /// Serializes TLS-dir setup across the parallel matrix tests, the same
        /// way the integration suites do: the first bind generates the
        /// self-signed pair, later binds read it.
        static TLS_ONCE: Once = Once::new();

        fn isolate_tls_dir() {
            TLS_ONCE.call_once(|| {
                let dir = std::env::temp_dir().join(format!(
                    "lightspeed-control-alpn-tls-{}",
                    std::process::id()
                ));
                std::fs::create_dir_all(&dir).expect("create TLS temp dir");
                std::env::set_var("LIGHTSPEED_TLS_DIR", &dir);
            });
        }

        /// Regression (CI `QUIC Integration Tests`, 2026-11-16): the parallel
        /// matrix tests bind several control servers at once, so more than one
        /// caller reached the check-then-generate path with an empty TLS
        /// directory. Two callers could each generate a pair and interleave the
        /// writes, leaving one certificate beside another's key and failing the
        /// bind with `KeyMismatch`. Every caller on an empty directory must
        /// observe the single generated pair.
        ///
        /// The barrier makes the race window as wide as it gets; the assertion is
        /// on the invariant (one pair), not on timing, so it is not flaky once
        /// the serialisation exists.
        #[test]
        fn concurrent_first_boot_generation_returns_one_matching_pair() {
            let dir = std::env::temp_dir().join(format!(
                "lightspeed-control-tls-race-{}",
                std::process::id()
            ));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create the race dir");

            let barrier = std::sync::Arc::new(std::sync::Barrier::new(8));
            let mut threads = Vec::new();
            for _ in 0..8 {
                let dir = dir.clone();
                let barrier = std::sync::Arc::clone(&barrier);
                threads.push(std::thread::spawn(move || {
                    barrier.wait();
                    ensure_cert_in(&dir).expect("load or generate the certificate")
                }));
            }
            let pairs: Vec<_> = threads
                .into_iter()
                .map(|thread| thread.join().expect("join the generation thread"))
                .collect();

            let first_cert = pairs[0].0[0].as_ref().to_vec();
            let first_key = pairs[0].1.secret_der().to_vec();
            for (certs, key) in &pairs {
                assert_eq!(
                    certs[0].as_ref(),
                    first_cert.as_slice(),
                    "every concurrent caller must see the one generated certificate"
                );
                assert_eq!(
                    key.secret_der(),
                    first_key.as_slice(),
                    "every concurrent caller must see the one generated key"
                );
                // The pair must actually be usable, which is what KeyMismatch
                // rejected when the writes interleaved.
                rustls::ServerConfig::builder()
                    .with_no_client_auth()
                    .with_single_cert(certs.clone(), key.clone_key())
                    .expect("the returned certificate and key must match");
            }

            let _ = std::fs::remove_dir_all(&dir);
        }

        /// A relay serving on loopback with one ALPN policy.
        struct TestRelay {
            addr: SocketAddr,
            metrics: Arc<ProxyMetrics>,
            shutdown_tx: tokio::sync::watch::Sender<bool>,
            handle: tokio::task::JoinHandle<()>,
        }

        impl TestRelay {
            async fn start(mode: ControlAlpn) -> Self {
                isolate_tls_dir();
                let mut config = ProxyConfig::default();
                config.network.control_alpn = mode;
                let metrics = Arc::new(ProxyMetrics::new());
                let state = Arc::new(ControlState::new(
                    config,
                    Arc::new(RwLock::new(Authenticator::new(true))),
                    Arc::clone(&metrics),
                    None,
                ));
                let server = ControlServer::bind("127.0.0.1:0".parse().expect("bind addr"), state)
                    .expect("bind the control server");
                let addr = server.local_addr().expect("bound control address");
                let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
                let handle = tokio::spawn(server.run(shutdown_rx));
                Self {
                    addr,
                    metrics,
                    shutdown_tx,
                    handle,
                }
            }

            async fn shutdown(self) {
                let _ = self.shutdown_tx.send(true);
                let _ = tokio::time::timeout(Duration::from_secs(5), self.handle).await;
            }
        }

        /// Accepts the relay's self-signed certificate. The certificate is not
        /// what these tests are about; the ALPN offer is.
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

        /// A QUIC client endpoint that accepts the relay certificate; `alpn` is
        /// the only variable between matrix rows.
        fn client_endpoint(alpn: Vec<Vec<u8>>) -> quinn::Endpoint {
            let mut crypto = rustls::ClientConfig::builder()
                .dangerous()
                .with_custom_certificate_verifier(Arc::new(SkipVerify))
                .with_no_client_auth();
            crypto.alpn_protocols = alpn;
            let quic_crypto = quinn::crypto::rustls::QuicClientConfig::try_from(crypto)
                .expect("QUIC client crypto");
            let mut endpoint = quinn::Endpoint::client("127.0.0.1:0".parse().expect("client addr"))
                .expect("client endpoint");
            endpoint.set_default_client_config(quinn::ClientConfig::new(Arc::new(quic_crypto)));
            endpoint
        }

        async fn connect(
            endpoint: &quinn::Endpoint,
            addr: SocketAddr,
        ) -> Result<quinn::Connection, quinn::ConnectionError> {
            endpoint
                .connect(addr, "lightspeed-proxy")
                .expect("start the QUIC handshake")
                .await
        }

        /// The ALPN the connection negotiated, read from the rustls handshake
        /// data on either side.
        fn negotiated(conn: &quinn::Connection) -> Option<Vec<u8>> {
            conn.handshake_data()
                .expect("a completed handshake has handshake data")
                .downcast::<quinn::crypto::rustls::HandshakeData>()
                .expect("rustls handshake data")
                .protocol
        }

        /// Assert the peer closed the handshake with TLS alert 120
        /// (`no_application_protocol`). The accepting side reports it as a
        /// transport error; a peer that received the alert reports it as a
        /// connection close. Both shapes mean the same alert.
        fn assert_alert_120(error: &quinn::ConnectionError) {
            let code = match error {
                quinn::ConnectionError::TransportError(error) => error.code,
                quinn::ConnectionError::ConnectionClosed(close) => close.error_code,
                other => panic!("expected an ALPN handshake failure, got {other:?}"),
            };
            assert_eq!(
                code,
                quinn::TransportErrorCode::crypto(0x78),
                "expected TLS alert 120 (no_application_protocol), got {error:?}"
            );
        }

        /// Register over the established connection and return the response.
        async fn register_over_connection(conn: &quinn::Connection) -> ControlMessage {
            let (mut send, mut recv) = conn.open_bi().await.expect("open a control stream");
            ControlMessage::Register {
                protocol_version: PROTOCOL_VERSION,
                game: 0,
                data_port: 0,
                destination: None,
            }
            .write_to(&mut send)
            .await
            .expect("write Register");
            let response = ControlMessage::read_from(&mut recv)
                .await
                .expect("read the control response")
                .expect("a control response");
            send.finish().expect("finish the control stream");
            response
        }

        /// Wait (bounded) for an accept task to count its handshake outcome.
        async fn wait_for_outcome(metrics: &ProxyMetrics, outcome: ControlHandshakeOutcome) -> u64 {
            let deadline = Instant::now() + Duration::from_secs(5);
            loop {
                let count = metrics.control_handshake_count(outcome);
                if count > 0 {
                    return count;
                }
                assert!(
                    Instant::now() < deadline,
                    "timed out waiting for the {} counter",
                    outcome.as_str()
                );
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        }

        // Row A: today's client against today's relay — no ALPN, handshake
        // completes, Register is answered. This is the behaviour `off` must
        // preserve byte for byte.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn an_off_relay_accepts_an_alpn_less_client() {
            let relay = TestRelay::start(ControlAlpn::Off).await;
            let endpoint = client_endpoint(Vec::new());
            let conn = connect(&endpoint, relay.addr)
                .await
                .expect("row A: an off relay must accept an ALPN-less client");
            assert_eq!(
                negotiated(&conn),
                None,
                "row A: no ALPN is negotiated when both sides offer none"
            );
            match register_over_connection(&conn).await {
                ControlMessage::RegisterAck { .. } => {}
                other => panic!("row A: expected a RegisterAck, got {other:?}"),
            }
            assert_eq!(
                relay.metrics.control_alpn(),
                ControlAlpn::Off,
                "the /health data source must report the mode the relay serves"
            );
            wait_for_outcome(&relay.metrics, ControlHandshakeOutcome::NoAlpn).await;
            assert_eq!(
                relay
                    .metrics
                    .control_handshake_count(ControlHandshakeOutcome::NegotiatedV1),
                0,
                "row A: no handshake may be counted as negotiated"
            );
            conn.close(0u32.into(), b"row A done");
            drop(endpoint);
            relay.shutdown().await;
        }

        // Row C: the new client against an old relay. The relay rejects the
        // offer with alert 120 — the exact failure the client's empty-list
        // fallback exists for. If this ever succeeds, rustls' strict-QUIC
        // branch is not what §3 says and the client fallback is dead code.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn an_off_relay_rejects_an_alpn_offering_client_with_120() {
            let relay = TestRelay::start(ControlAlpn::Off).await;
            let endpoint = client_endpoint(quic_control_alpn());
            let error = match connect(&endpoint, relay.addr).await {
                Ok(_) => panic!("row C: an off relay must reject a client that offers ALPN"),
                Err(error) => error,
            };
            assert_alert_120(&error);
            wait_for_outcome(
                &relay.metrics,
                ControlHandshakeOutcome::RejectedNoApplicationProtocol,
            )
            .await;
            drop(endpoint);
            relay.shutdown().await;
        }

        // Row D: the new client against a required relay — the shared name is
        // negotiated and Register is answered.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_required_relay_negotiates_the_shared_name() {
            let relay = TestRelay::start(ControlAlpn::Required).await;
            let endpoint = client_endpoint(quic_control_alpn());
            let conn = connect(&endpoint, relay.addr)
                .await
                .expect("row D: a required relay must accept a client that offers the shared name");
            assert_eq!(
                negotiated(&conn),
                Some(QUIC_CONTROL_ALPN_V1.to_vec()),
                "row D: the negotiated protocol must be the shared control ALPN"
            );
            match register_over_connection(&conn).await {
                ControlMessage::RegisterAck { .. } => {}
                other => panic!("row D: expected a RegisterAck, got {other:?}"),
            }
            assert_eq!(
                relay.metrics.control_alpn(),
                ControlAlpn::Required,
                "the /health data source must report the configured mode"
            );
            wait_for_outcome(&relay.metrics, ControlHandshakeOutcome::NegotiatedV1).await;
            conn.close(0u32.into(), b"row D done");
            drop(endpoint);
            relay.shutdown().await;
        }

        // Row B: a pre-ALPN client against a required relay. This is why the
        // client build has to ship first: the client cannot recover, it can
        // only be updated.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_required_relay_rejects_an_alpn_less_client_with_120() {
            let relay = TestRelay::start(ControlAlpn::Required).await;
            let endpoint = client_endpoint(Vec::new());
            let error = match connect(&endpoint, relay.addr).await {
                Ok(_) => panic!("row B: a required relay must reject an ALPN-less client"),
                Err(error) => error,
            };
            assert_alert_120(&error);
            wait_for_outcome(
                &relay.metrics,
                ControlHandshakeOutcome::RejectedNoApplicationProtocol,
            )
            .await;
            drop(endpoint);
            relay.shutdown().await;
        }

        // Row E: version skew stays loud. A client that offers only revision 2
        // against a revision-1 relay gets alert 120; the relay never silently
        // downgrades to an ALPN-less session.
        #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
        async fn a_required_relay_rejects_a_different_revision_with_120() {
            let relay = TestRelay::start(ControlAlpn::Required).await;
            let endpoint = client_endpoint(vec![b"lightspeed-control/2".to_vec()]);
            let error = match connect(&endpoint, relay.addr).await {
                Ok(_) => panic!("row E: a required relay must reject an unknown revision"),
                Err(error) => error,
            };
            assert_alert_120(&error);
            wait_for_outcome(
                &relay.metrics,
                ControlHandshakeOutcome::RejectedNoApplicationProtocol,
            )
            .await;
            assert_eq!(
                relay
                    .metrics
                    .control_handshake_count(ControlHandshakeOutcome::NegotiatedV1),
                0,
                "row E: a failed revision negotiation must not be counted as negotiated"
            );
            drop(endpoint);
            relay.shutdown().await;
        }
    }
}

// ── Re-exports ──────────────────────────────────────────────────────

#[cfg(feature = "quic")]
pub use inner::{
    run_control_server, validate_tls_assets, ClientSession, ControlServer, ControlState,
};
