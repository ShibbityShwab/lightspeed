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

pub(crate) mod fingerprint;

#[cfg(feature = "quic")]
mod pinning;

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

    /// Build a quinn client config that pins the proxy's self-signed
    /// certificate (trust-on-first-use) and verifies the TLS signature.
    fn build_client_config(addr: SocketAddr) -> Result<quinn::ClientConfig, QuicError> {
        let crypto = rustls::ClientConfig::builder()
            .dangerous()
            .with_custom_certificate_verifier(Arc::new(super::pinning::TofuVerifier::new(addr)))
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
        session_token: Option<u32>,
        /// Proxy node ID.
        node_id: Option<String>,
        /// Proxy region.
        region: Option<String>,
        /// Whether the proxy advertised control-plane telemetry in its ack.
        telemetry_quic: bool,
    }

    impl ControlClient {
        /// Create a new control plane client.
        pub fn new() -> Result<Self, QuicError> {
            // Bind to any available port for the client endpoint. The default
            // client config (with the proxy's pinned certificate) is set per
            // connection in `connect` since it depends on the target address.
            let endpoint = quinn::Endpoint::client("0.0.0.0:0".parse().unwrap())
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;

            Ok(Self {
                endpoint,
                connection: None,
                remote_addr: None,
                session_id: None,
                session_token: None,
                node_id: None,
                region: None,
                telemetry_quic: false,
            })
        }

        /// Connect to a proxy's control plane and register.
        pub async fn connect(&mut self, addr: SocketAddr, game: u8) -> Result<(), QuicError> {
            info!("Connecting QUIC control plane to {}", addr);

            self.endpoint
                .set_default_client_config(build_client_config(addr)?);

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
                data_port: 0,
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
                    telemetry_quic,
                }) => {
                    info!(
                        "Registered with proxy: session={}, token={}, node={}, region={}",
                        session_id, session_token, node_id, region
                    );
                    self.session_id = Some(session_id);
                    self.session_token = Some(session_token);
                    self.node_id = Some(node_id);
                    self.region = Some(region);
                    self.telemetry_quic = telemetry_quic;
                    crate::session::set_session_token(session_token);
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
                .map(|c| c.close_reason().is_none())
                .unwrap_or(false)
        }

        /// Get the session ID assigned by the proxy.
        pub fn session_id(&self) -> Option<u32> {
            self.session_id
        }

        /// Get the data-plane session token (for tunnel header auth).
        pub fn session_token(&self) -> Option<u32> {
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

        /// Whether the proxy accepts telemetry on the control connection.
        pub fn supports_quic_telemetry(&self) -> bool {
            self.telemetry_quic
        }

        /// Get the remote proxy address.
        pub fn remote_addr(&self) -> Option<SocketAddr> {
            self.remote_addr
        }

        /// The live QUIC connection, if any. Used by the supervisor to race
        /// `Connection::closed()` against the keepalive ping.
        pub fn connection(&self) -> Option<&quinn::Connection> {
            self.connection.as_ref()
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

        pub fn session_token(&self) -> Option<u32> {
            None
        }

        pub fn node_id(&self) -> Option<&str> {
            None
        }

        pub fn region(&self) -> Option<&str> {
            None
        }

        pub fn supports_quic_telemetry(&self) -> bool {
            false
        }

        pub fn remote_addr(&self) -> Option<SocketAddr> {
            self.remote_addr
        }
    }
}

pub use inner::ControlClient;

// ── Supervised control-plane registration ───────────────────────────

#[cfg(feature = "quic")]
mod supervisor {
    use std::collections::HashMap;
    use std::net::{SocketAddr, SocketAddrV4};
    use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
    use std::sync::{Arc, Mutex, MutexGuard, OnceLock};
    use std::time::Duration;

    use tokio::sync::watch;
    use tokio::task::JoinHandle;
    use tracing::{debug, info, warn};

    use lightspeed_protocol::control::ControlMessage;

    use super::inner::ControlClient;
    use crate::error::QuicError;

    pub(super) const PING_INTERVAL: Duration = Duration::from_secs(15);
    pub(super) const MIN_BACKOFF: Duration = Duration::from_millis(250);
    pub(super) const MAX_BACKOFF: Duration = Duration::from_secs(5);

    /// Time budget for one best-effort telemetry send on the control
    /// connection. Telemetry must never delay the control plane or gameplay.
    const TELEMETRY_SEND_TIMEOUT: Duration = Duration::from_secs(2);

    /// First-attempt outcome awaited by [`register_session`](super::register_session).
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub(super) enum Initial {
        Pending,
        Done(u32),
    }

    /// A live control connection plus the capabilities the relay advertised in
    /// its registration ack. Published and cleared as one unit so a telemetry
    /// send can never observe a connection without its capability.
    #[derive(Clone)]
    struct ControlTarget {
        connection: quinn::Connection,
        telemetry_quic: bool,
    }

    /// Observable handle for a running per-relay supervisor.
    #[derive(Clone)]
    pub struct SupervisorHandle {
        /// Monotonic id; a newer supervisor for the same relay has a higher one.
        pub generation: u64,
        /// Set once the supervisor has been asked to stop.
        pub stopped: Arc<AtomicBool>,
        /// Most recent non-zero token the relay issued (0 until registered).
        pub current_token: Arc<AtomicU32>,
        /// Live connection + capability while registered; cleared on disconnect
        /// or reconnect. Telemetry sends on this same connection.
        target: Arc<Mutex<Option<ControlTarget>>>,
    }

    struct Entry {
        handle: SupervisorHandle,
        initial_rx: watch::Receiver<Initial>,
        stop_tx: watch::Sender<bool>,
        _join: JoinHandle<()>,
    }

    static REGISTRY: OnceLock<Mutex<HashMap<SocketAddrV4, Entry>>> = OnceLock::new();
    static NEXT_GENERATION: AtomicU64 = AtomicU64::new(1);

    fn lock_registry() -> MutexGuard<'static, HashMap<SocketAddrV4, Entry>> {
        REGISTRY
            .get_or_init(|| Mutex::new(HashMap::new()))
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// A supervisor may only write state while it is still the live one for its
    /// relay, which makes a stopped or superseded task a no-op.
    fn generation_is_current(data_addr: SocketAddrV4, generation: u64) -> bool {
        lock_registry().get(&data_addr).is_some_and(|entry| {
            entry.handle.generation == generation && !entry.handle.stopped.load(Ordering::Acquire)
        })
    }

    /// Exponential backoff with downward jitter, bounded to
    /// `[MIN_BACKOFF, MAX_BACKOFF]`.
    pub(super) struct Backoff {
        base: Duration,
    }

    impl Backoff {
        pub(super) fn new() -> Self {
            Self { base: MIN_BACKOFF }
        }

        pub(super) fn reset(&mut self) {
            self.base = MIN_BACKOFF;
        }

        /// Advance the exponential schedule and return the previous base.
        pub(super) fn advance(&mut self) -> Duration {
            let base = self.base;
            self.base = (self.base * 2).min(MAX_BACKOFF);
            base
        }

        /// Next delay: the exponential base jittered uniformly down to half,
        /// clamped to the configured floor and ceiling.
        pub(super) fn next_delay(&mut self) -> Duration {
            let base = self.advance();
            let low = (base / 2).max(MIN_BACKOFF);
            let high = base.max(MIN_BACKOFF);
            if high <= low {
                return low;
            }
            let span_ms = (high - low).as_millis() as u64 + 1;
            low + Duration::from_millis(rand::random::<u64>() % span_ms)
        }
    }

    /// Ensure exactly one supervisor exists for `data_addr`, returning the
    /// receiver that reports its first registration outcome.
    pub(super) fn ensure_supervisor(
        data_addr: SocketAddrV4,
        control_port: u16,
    ) -> watch::Receiver<Initial> {
        let mut registry = lock_registry();
        if let Some(entry) = registry.get(&data_addr) {
            if !entry.handle.stopped.load(Ordering::Acquire) {
                return entry.initial_rx.clone();
            }
        }

        let generation = NEXT_GENERATION.fetch_add(1, Ordering::Relaxed);
        let stopped = Arc::new(AtomicBool::new(false));
        let current_token = Arc::new(AtomicU32::new(0));
        let target = Arc::new(Mutex::new(None));
        let (initial_tx, initial_rx) = watch::channel(Initial::Pending);
        let (stop_tx, stop_rx) = watch::channel(false);

        let handle = SupervisorHandle {
            generation,
            stopped: Arc::clone(&stopped),
            current_token: Arc::clone(&current_token),
            target: Arc::clone(&target),
        };
        let join = tokio::spawn(supervisor_loop(
            data_addr,
            control_port,
            generation,
            stopped,
            current_token,
            target,
            initial_tx,
            stop_rx,
        ));

        registry.insert(
            data_addr,
            Entry {
                handle,
                initial_rx: initial_rx.clone(),
                stop_tx,
                _join: join,
            },
        );
        initial_rx
    }

    /// Await the first registration outcome for `data_addr`, spawning the
    /// supervisor if needed. Returns the initial token, or `None` if the first
    /// attempt failed (the supervisor keeps retrying in the background).
    pub(super) async fn register_and_wait(
        data_addr: SocketAddrV4,
        control_port: u16,
    ) -> Option<u32> {
        let mut initial_rx = ensure_supervisor(data_addr, control_port);
        if matches!(*initial_rx.borrow(), Initial::Pending) {
            let _ = initial_rx.changed().await;
        }
        let outcome = *initial_rx.borrow();
        match outcome {
            Initial::Done(token) if token != 0 => Some(token),
            _ => None,
        }
    }

    /// Spawn a supervisor for `data_addr` without waiting for the first result.
    pub fn ensure_registration(data_addr: SocketAddrV4, control_port: u16) {
        let _ = ensure_supervisor(data_addr, control_port);
    }

    /// Stop the supervisor for `data_addr`, if one is running.
    pub fn stop_supervisor(data_addr: SocketAddrV4) -> bool {
        match lock_registry().remove(&data_addr) {
            Some(entry) => {
                entry.handle.stopped.store(true, Ordering::Release);
                let _ = entry.stop_tx.send(true);
                true
            }
            None => false,
        }
    }

    /// Observe the supervisor for `data_addr`, if one is running.
    pub fn supervisor_handle(data_addr: SocketAddrV4) -> Option<SupervisorHandle> {
        lock_registry()
            .get(&data_addr)
            .map(|entry| entry.handle.clone())
    }

    /// Whether a live supervisor is managing `data_addr`.
    pub fn is_supervised(data_addr: SocketAddrV4) -> bool {
        supervisor_handle(data_addr).is_some_and(|handle| !handle.stopped.load(Ordering::Acquire))
    }

    /// The most recent token a supervisor's relay issued, if any.
    pub fn supervised_token(data_addr: SocketAddrV4) -> Option<u32> {
        let token = supervisor_handle(data_addr)?
            .current_token
            .load(Ordering::Acquire);
        (token != 0).then_some(token)
    }

    async fn supervisor_loop(
        data_addr: SocketAddrV4,
        control_port: u16,
        generation: u64,
        stopped: Arc<AtomicBool>,
        current_token: Arc<AtomicU32>,
        target: Arc<Mutex<Option<ControlTarget>>>,
        initial_tx: watch::Sender<Initial>,
        mut stop_rx: watch::Receiver<bool>,
    ) {
        let control_addr = SocketAddr::V4(SocketAddrV4::new(*data_addr.ip(), control_port));
        let mut backoff = Backoff::new();
        let mut first_attempt = true;

        loop {
            if stopped.load(Ordering::Acquire) || !generation_is_current(data_addr, generation) {
                break;
            }

            match connect_and_register(control_addr).await {
                Ok(client) => {
                    let token = client.session_token().unwrap_or(0);
                    let live = client.connection().cloned();
                    let published = live.clone().map(|connection| ControlTarget {
                        connection,
                        telemetry_quic: client.supports_quic_telemetry(),
                    });
                    if !publish_target(data_addr, generation, &target, published) {
                        break;
                    }
                    if token != 0 {
                        if !publish_token(data_addr, generation, &current_token, token) {
                            break;
                        }
                        backoff.reset();
                        info!(relay = %data_addr, token, "control-plane registered");
                    }
                    if first_attempt {
                        let _ = initial_tx.send_replace(Initial::Done(token));
                        first_attempt = false;
                    }
                    let hold = hold_connection(&client, live, &mut stop_rx).await;
                    clear_target(data_addr, generation, &target);
                    if hold == HoldOutcome::Stopped {
                        break;
                    }
                }
                Err(error) => {
                    warn!(relay = %data_addr, %error, "control-plane registration failed; retrying");
                    if first_attempt {
                        let _ = initial_tx.send_replace(Initial::Done(0));
                        first_attempt = false;
                    }
                }
            }

            let delay = backoff.next_delay();
            debug!(relay = %data_addr, delay_ms = delay.as_millis(), "control-plane reconnect backoff");
            if sleep_or_stop(delay, &stopped, &mut stop_rx).await {
                break;
            }
        }

        clear_target(data_addr, generation, &target);
        if first_attempt {
            let _ = initial_tx.send_replace(Initial::Done(0));
        }
        debug!(relay = %data_addr, "control-plane supervisor stopped");
    }

    /// Publish the connection + capability only while this task is still the
    /// live supervisor for its relay. Returns `false` if it has been stopped or
    /// superseded.
    fn publish_target(
        data_addr: SocketAddrV4,
        generation: u64,
        slot: &Mutex<Option<ControlTarget>>,
        target: Option<ControlTarget>,
    ) -> bool {
        if !generation_is_current(data_addr, generation) {
            return false;
        }
        *slot
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) = target;
        true
    }

    /// Clear the published target, unless a newer supervisor owns the slot.
    fn clear_target(data_addr: SocketAddrV4, generation: u64, slot: &Mutex<Option<ControlTarget>>) {
        if generation_is_current(data_addr, generation) {
            *slot
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner) = None;
        }
    }

    /// The live control connection and advertised capability for `data_addr`.
    fn supervised_target(data_addr: SocketAddrV4) -> Option<ControlTarget> {
        let handle = supervisor_handle(data_addr)?;
        if handle.stopped.load(Ordering::Acquire) {
            return None;
        }
        let target = handle
            .target
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        target
    }

    /// Send one telemetry report on the relay's live control connection.
    ///
    /// Best-effort and time-bounded: a missing connection, a relay that did not
    /// advertise control-plane telemetry, or any write error returns `Err` so
    /// the caller can fall back to HTTP. It never retries and never blocks the
    /// keepalive or registration streams, which use separate QUIC streams.
    pub(super) async fn send_telemetry(
        data_addr: SocketAddrV4,
        report_json: &[u8],
    ) -> Result<(), QuicError> {
        if report_json.len() > lightspeed_protocol::telemetry::MAX_TELEMETRY_BODY {
            return Err(QuicError::ConnectionFailed(
                "telemetry body exceeds cap".into(),
            ));
        }
        let target = supervised_target(data_addr)
            .ok_or_else(|| QuicError::ConnectionFailed("no live control connection".into()))?;
        if !target.telemetry_quic {
            return Err(QuicError::ConnectionFailed(
                "relay does not accept control-plane telemetry".into(),
            ));
        }
        let message = ControlMessage::Telemetry {
            report_json: report_json.to_vec(),
        };
        let send = async {
            let (mut send, _recv) = target
                .connection
                .open_bi()
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            message
                .write_to(&mut send)
                .await
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            send.finish()
                .map_err(|e| QuicError::ConnectionFailed(e.to_string()))?;
            Ok::<(), QuicError>(())
        };
        tokio::time::timeout(TELEMETRY_SEND_TIMEOUT, send)
            .await
            .map_err(|_| QuicError::ConnectionFailed("telemetry send timed out".into()))?
    }

    /// Publish a fresh token unless this task has been stopped or superseded.
    /// A zero token is never published, so a valid token is never clobbered.
    fn publish_token(
        data_addr: SocketAddrV4,
        generation: u64,
        current_token: &AtomicU32,
        token: u32,
    ) -> bool {
        if token == 0 || !generation_is_current(data_addr, generation) {
            return false;
        }
        current_token.store(token, Ordering::Release);
        crate::session::set_path_token(data_addr, token);
        true
    }

    async fn connect_and_register(control_addr: SocketAddr) -> Result<ControlClient, QuicError> {
        let mut client = ControlClient::new()?;
        client
            .connect(control_addr, lightspeed_protocol::game_id::UNKNOWN)
            .await?;
        Ok(client)
    }

    #[derive(PartialEq, Eq)]
    enum HoldOutcome {
        Stopped,
        Disconnected,
    }

    async fn hold_connection(
        client: &ControlClient,
        connection: Option<quinn::Connection>,
        stop_rx: &mut watch::Receiver<bool>,
    ) -> HoldOutcome {
        let Some(connection) = connection else {
            return HoldOutcome::Disconnected;
        };
        let mut interval = tokio::time::interval(PING_INTERVAL);
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        interval.tick().await;

        loop {
            tokio::select! {
                _ = stop_requested(stop_rx) => return HoldOutcome::Stopped,
                _ = connection.closed() => {
                    warn!("control-plane connection closed; reconnecting");
                    return HoldOutcome::Disconnected;
                }
                _ = interval.tick() => {
                    if client.ping().await.is_err() {
                        warn!("control-plane ping failed; reconnecting");
                        return HoldOutcome::Disconnected;
                    }
                }
            }
        }
    }

    async fn stop_requested(stop_rx: &mut watch::Receiver<bool>) {
        let _ = stop_rx.wait_for(|stop| *stop).await;
    }

    async fn sleep_or_stop(
        delay: Duration,
        stopped: &AtomicBool,
        stop_rx: &mut watch::Receiver<bool>,
    ) -> bool {
        if stopped.load(Ordering::Acquire) {
            return true;
        }
        tokio::select! {
            _ = tokio::time::sleep(delay) => false,
            _ = stop_requested(stop_rx) => true,
        }
    }
}

#[cfg(feature = "quic")]
pub use supervisor::SupervisorHandle;

#[cfg(feature = "quic")]
pub fn ensure_registration(data_addr: std::net::SocketAddrV4, control_port: u16) {
    supervisor::ensure_registration(data_addr, control_port);
}

#[cfg(feature = "quic")]
pub fn stop_supervisor(data_addr: std::net::SocketAddrV4) -> bool {
    supervisor::stop_supervisor(data_addr)
}

#[cfg(feature = "quic")]
pub fn is_supervised(data_addr: std::net::SocketAddrV4) -> bool {
    supervisor::is_supervised(data_addr)
}

#[cfg(feature = "quic")]
pub fn supervised_token(data_addr: std::net::SocketAddrV4) -> Option<u32> {
    supervisor::supervised_token(data_addr)
}

#[cfg(feature = "quic")]
pub fn supervisor_handle(data_addr: std::net::SocketAddrV4) -> Option<SupervisorHandle> {
    supervisor::supervisor_handle(data_addr)
}

/// Send one telemetry report over the relay's live control connection.
///
/// Returns `Err` when the `quic` feature is disabled or no supervised control
/// connection is live, so callers fall back to the HTTP endpoint.
#[cfg(feature = "quic")]
pub async fn send_telemetry(
    data_addr: std::net::SocketAddrV4,
    report_json: &[u8],
) -> Result<(), crate::error::QuicError> {
    supervisor::send_telemetry(data_addr, report_json).await
}

/// Telemetry over QUIC is unavailable without the `quic` feature.
#[cfg(not(feature = "quic"))]
pub async fn send_telemetry(
    _data_addr: std::net::SocketAddrV4,
    _report_json: &[u8],
) -> Result<(), crate::error::QuicError> {
    Err(crate::error::QuicError::ConnectionFailed(
        "quic feature disabled".into(),
    ))
}

#[cfg(not(feature = "quic"))]
pub fn ensure_registration(_data_addr: std::net::SocketAddrV4, _control_port: u16) {}

#[cfg(not(feature = "quic"))]
pub fn stop_supervisor(_data_addr: std::net::SocketAddrV4) -> bool {
    false
}

#[cfg(not(feature = "quic"))]
pub fn is_supervised(_data_addr: std::net::SocketAddrV4) -> bool {
    false
}

#[cfg(not(feature = "quic"))]
pub fn supervised_token(_data_addr: std::net::SocketAddrV4) -> Option<u32> {
    None
}

/// Register with the proxy's control plane to obtain a data-plane session token.
///
/// Best-effort: returns `None` (leaving the token at its default of `0`) when
/// the `quic` feature is disabled, the proxy has no control plane, or the first
/// registration attempt is rejected. The data plane then sends token `0`, which
/// the proxy accepts only when `require_auth = false`.
///
/// The registration is supervised per relay: the returned token is the initial
/// one, and a background task holds the connection open and reconnects with
/// backoff after a control-plane loss, refreshing the per-path token. Repeated
/// calls for the same relay are idempotent and do not spawn duplicate tasks.
pub async fn register_session(data_addr: std::net::SocketAddrV4, control_port: u16) -> Option<u32> {
    register_session_inner(data_addr, control_port).await
}

#[cfg(feature = "quic")]
async fn register_session_inner(
    data_addr: std::net::SocketAddrV4,
    control_port: u16,
) -> Option<u32> {
    supervisor::register_and_wait(data_addr, control_port).await
}

#[cfg(not(feature = "quic"))]
async fn register_session_inner(
    _data_addr: std::net::SocketAddrV4,
    _control_port: u16,
) -> Option<u32> {
    None
}

#[cfg(all(test, feature = "quic"))]
mod supervisor_tests {
    use super::supervisor::{
        ensure_registration, is_supervised, send_telemetry, stop_supervisor, supervisor_handle,
        Backoff, MAX_BACKOFF, MIN_BACKOFF,
    };
    use std::net::{Ipv4Addr, SocketAddrV4};
    use std::time::Duration;

    #[test]
    fn backoff_advances_exponentially_and_caps() {
        let mut backoff = Backoff::new();
        let expected = [
            Duration::from_millis(250),
            Duration::from_millis(500),
            Duration::from_millis(1000),
            Duration::from_millis(2000),
            Duration::from_millis(4000),
            Duration::from_millis(5000),
            Duration::from_millis(5000),
        ];
        for want in expected {
            assert_eq!(backoff.advance(), want);
        }
    }

    #[test]
    fn backoff_delays_stay_bounded_and_reset() {
        let mut backoff = Backoff::new();
        for _ in 0..64 {
            let delay = backoff.next_delay();
            assert!(delay >= MIN_BACKOFF, "delay {delay:?} below floor");
            assert!(delay <= MAX_BACKOFF, "delay {delay:?} above ceiling");
            backoff.reset();
        }
    }

    #[tokio::test]
    async fn registration_is_idempotent_per_relay() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42101);
        let _ = stop_supervisor(addr);
        ensure_registration(addr, 59991);
        let first = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        ensure_registration(addr, 59991);
        let second = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert_eq!(first, second, "a second request must not respawn");
        assert!(is_supervised(addr));
        assert!(stop_supervisor(addr));
        assert!(!is_supervised(addr));
    }

    #[tokio::test]
    async fn restart_after_stop_bumps_generation() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42102);
        let _ = stop_supervisor(addr);
        ensure_registration(addr, 59992);
        let first = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert!(stop_supervisor(addr));
        ensure_registration(addr, 59992);
        let second = supervisor_handle(addr)
            .expect("supervisor registered")
            .generation;
        assert!(second > first, "restart must advance the generation");
        assert!(stop_supervisor(addr));
    }

    /// Given: no live supervised control connection. When: telemetry is sent.
    /// Then: it reports failure so the caller can fall back to HTTP instead of
    /// dropping the report silently.
    #[tokio::test]
    async fn telemetry_without_a_live_connection_errors() {
        let addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 42103);
        let _ = stop_supervisor(addr);
        assert!(
            send_telemetry(addr, b"{}").await.is_err(),
            "no live connection must surface as an error"
        );
    }
}
