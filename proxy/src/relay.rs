//! # Proxy Packet Relay
//!
//! Core relay engine: receives tunnel packets from clients, strips header,
//! forwards to game server, receives response, re-wraps, returns to client.
//!
//! ## Security
//!
//! The relay enforces multiple security layers:
//! 1. **Authentication**: Validates (IP + session_token) per-packet
//! 2. **Rate limiting**: Per-client PPS and BPS limits
//! 3. **Abuse detection**: Amplification and reflection detection
//! 4. **Destination validation**: Blocks forwarding to private/internal IPs

use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicU16, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};

use lightspeed_protocol::TunnelHeader;

use super::abuse::{AbuseCheckResult, AbuseDetector};
use super::auth::Authenticator;
use super::metrics::ProxyMetrics;
use super::rate_limit::{RateLimitResult, RateLimiter};

/// Get current timestamp in microseconds (wrapping u32).
fn now_us() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

/// Tracks an active client tunnel session.
#[derive(Debug)]
pub struct ClientSession {
    /// Client's address (for sending responses back).
    pub client_addr: SocketAddrV4,
    /// Game server this client is connected to.
    pub game_server: SocketAddrV4,
    /// Outbound socket for forwarding to game server.
    pub outbound_socket: Arc<UdpSocket>,
    /// Packets relayed in this session.
    pub packets_relayed: u64,
    /// Bytes relayed in this session.
    pub bytes_relayed: u64,
    /// Session start time.
    pub started_at: Instant,
    /// Last activity time.
    pub last_activity: Instant,
    /// Response sequence counter.
    pub response_seq: AtomicU16,
}

/// The relay engine — manages all active tunnel sessions.
pub struct RelayEngine {
    /// Active client sessions indexed by client address.
    sessions: Arc<RwLock<HashMap<SocketAddrV4, Arc<ClientSession>>>>,
    /// Maximum concurrent sessions.
    max_sessions: usize,
    /// Session timeout (no activity).
    session_timeout: Duration,
}

impl RelayEngine {
    /// Create a new relay engine.
    pub fn new(max_sessions: usize) -> Self {
        Self {
            sessions: Arc::new(RwLock::new(HashMap::new())),
            max_sessions,
            session_timeout: Duration::from_secs(300), // 5 min
        }
    }

    /// Get the number of active sessions.
    pub async fn active_sessions(&self) -> usize {
        self.sessions.read().await.len()
    }

    /// Check if we can accept a new session.
    pub async fn can_accept(&self) -> bool {
        self.sessions.read().await.len() < self.max_sessions
    }

    /// Get or create a session for a client.
    ///
    /// If the client already has an active session, returns it.
    /// Otherwise, creates a new outbound socket and session.
    pub async fn get_or_create_session(
        &self,
        client_addr: SocketAddrV4,
        game_server: SocketAddrV4,
    ) -> anyhow::Result<Arc<ClientSession>> {
        // Fast path: check existing session
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&client_addr) {
                return Ok(Arc::clone(session));
            }
        }

        // Slow path: create new session
        if !self.can_accept().await {
            anyhow::bail!("Max sessions ({}) reached", self.max_sessions);
        }

        // Bind a new outbound socket for this client's game traffic
        // Use 0.0.0.0:0 to get an ephemeral port
        let outbound_socket = UdpSocket::bind("0.0.0.0:0").await?;

        info!(
            client = %client_addr,
            game_server = %game_server,
            outbound_port = %outbound_socket.local_addr()?,
            "New client session created"
        );

        let session = Arc::new(ClientSession {
            client_addr,
            game_server,
            outbound_socket: Arc::new(outbound_socket),
            packets_relayed: 0,
            bytes_relayed: 0,
            started_at: Instant::now(),
            last_activity: Instant::now(),
            response_seq: AtomicU16::new(0),
        });

        let mut sessions = self.sessions.write().await;
        sessions.insert(client_addr, Arc::clone(&session));

        Ok(session)
    }

    /// Get a shared reference to the sessions map (for the response listener).
    pub fn sessions(&self) -> Arc<RwLock<HashMap<SocketAddrV4, Arc<ClientSession>>>> {
        Arc::clone(&self.sessions)
    }

    /// Clean up expired sessions.
    pub async fn cleanup_expired(&self) -> usize {
        let timeout = self.session_timeout;
        let mut sessions = self.sessions.write().await;
        let before = sessions.len();
        sessions.retain(|addr, session| {
            let keep = session.last_activity.elapsed() < timeout;
            if !keep {
                info!(client = %addr, "Session expired after {:?}", session.started_at.elapsed());
            }
            keep
        });
        before - sessions.len()
    }
}

/// Run the main relay loop on the data plane socket.
///
/// This is the hot path — every game packet goes through here.
/// It receives tunnel packets from clients, validates auth + abuse checks,
/// strips the header, and forwards the raw payload to the game server.
pub async fn run_relay_inbound(
    data_socket: Arc<UdpSocket>,
    engine: Arc<RelayEngine>,
    rate_limiter: Arc<tokio::sync::Mutex<RateLimiter>>,
    authenticator: Arc<RwLock<Authenticator>>,
    abuse_detector: Arc<tokio::sync::Mutex<AbuseDetector>>,
    metrics: Arc<ProxyMetrics>,
) -> anyhow::Result<()> {
    let mut buf = vec![0u8; 2048];

    info!("Relay inbound loop started");

    loop {
        // Receive next packet from a client
        let (len, addr) = match data_socket.recv_from(&mut buf).await {
            Ok(result) => result,
            Err(e) => {
                warn!("Data socket recv error: {}", e);
                continue;
            }
        };

        // We only support IPv4
        let client_addr = match addr {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => {
                trace!("Ignoring IPv6 packet");
                continue;
            }
        };

        // Rate limit check
        {
            let mut rl = rate_limiter.lock().await;
            match rl.check(client_addr, len as u64) {
                RateLimitResult::Allowed => {}
                RateLimitResult::PacketRateExceeded => {
                    trace!(client = %client_addr, "Rate limited (PPS)");
                    metrics.record_drop();
                    continue;
                }
                RateLimitResult::BandwidthExceeded => {
                    trace!(client = %client_addr, "Rate limited (BPS)");
                    metrics.record_drop();
                    continue;
                }
            }
        }

        // Decode tunnel header
        let (header, payload) = match TunnelHeader::decode_with_payload(&buf[..len]) {
            Ok(result) => result,
            Err(e) => {
                debug!(client = %client_addr, error = %e, "Invalid tunnel packet");
                metrics.record_drop();
                continue;
            }
        };

        // ── Security: Authentication check ──────────────────────────
        {
            let auth = authenticator.read().await;
            if !auth.validate(client_addr.ip(), header.session_token) {
                debug!(
                    client = %client_addr,
                    token = header.session_token,
                    "Unauthorized: invalid IP or session token"
                );
                metrics.record_drop();
                continue;
            }
        }

        // Handle control packets
        if header.is_keepalive() {
            trace!(client = %client_addr, seq = header.sequence, "Keepalive received");
            // Echo keepalive back to client (preserve session token)
            let response = TunnelHeader::keepalive(header.sequence, now_us())
                .with_session_token(header.session_token);
            let response_bytes = response.encode();
            let _ = data_socket.send_to(&response_bytes, client_addr).await;
            continue;
        }

        if header.is_fin() {
            info!(client = %client_addr, "Client sent FIN — closing session");
            // Remove session
            let sessions_lock = engine.sessions();
            let mut sessions = sessions_lock.write().await;
            sessions.remove(&client_addr);
            continue;
        }

        // Get the original destination (game server) from the header
        let game_server = header.orig_dst_addr();

        // ── Security: Abuse detection (includes destination validation) ──
        {
            let mut abuse = abuse_detector.lock().await;
            match abuse.record_inbound(*client_addr.ip(), game_server, len as u64) {
                AbuseCheckResult::Allowed => {}
                AbuseCheckResult::PrivateDestination => {
                    debug!(
                        client = %client_addr,
                        dest = %game_server,
                        "Blocked: private/internal destination"
                    );
                    metrics.record_drop();
                    continue;
                }
                AbuseCheckResult::Banned => {
                    trace!(client = %client_addr, "Blocked: client is banned");
                    metrics.record_drop();
                    continue;
                }
                AbuseCheckResult::ReflectionDetected => {
                    warn!(client = %client_addr, "Blocked: reflection attack detected");
                    metrics.record_drop();
                    continue;
                }
                AbuseCheckResult::AmplificationDetected => {
                    warn!(client = %client_addr, "Blocked: amplification detected");
                    metrics.record_drop();
                    continue;
                }
            }
        }

        // Get or create session for this client
        let session = match engine.get_or_create_session(client_addr, game_server).await {
            Ok(s) => s,
            Err(e) => {
                warn!(client = %client_addr, error = %e, "Failed to create session");
                metrics.record_drop();
                continue;
            }
        };

        // Forward the raw game payload to the game server
        match session.outbound_socket.send_to(payload, game_server).await {
            Ok(sent) => {
                metrics.record_relay(sent as u64);
                trace!(
                    client = %client_addr,
                    game_server = %game_server,
                    seq = header.sequence,
                    payload_len = payload.len(),
                    "Forwarded to game server"
                );

                // Record outbound for abuse tracking
                let mut abuse = abuse_detector.lock().await;
                abuse.record_outbound(*client_addr.ip(), sent as u64);
            }
            Err(e) => {
                debug!(
                    client = %client_addr,
                    game_server = %game_server,
                    error = %e,
                    "Failed to forward to game server"
                );
                metrics.record_drop();
            }
        }
    }
}

/// Run the response listener for a single client session.
///
/// Listens on the session's outbound socket for responses from the game
/// server, wraps them in a LightSpeed header, and sends them back to
/// the client via the data plane socket.
pub async fn run_session_response_listener(
    session: Arc<ClientSession>,
    data_socket: Arc<UdpSocket>,
    metrics: Arc<ProxyMetrics>,
) {
    let mut buf = vec![0u8; 2048];

    loop {
        // Receive response from game server
        let (len, _game_addr) = match session.outbound_socket.recv_from(&mut buf).await {
            Ok(result) => result,
            Err(e) => {
                debug!(
                    client = %session.client_addr,
                    error = %e,
                    "Outbound socket recv error"
                );
                break;
            }
        };

        let payload = &buf[..len];

        // Wrap response in a LightSpeed header (swap src/dst from original)
        let seq = session.response_seq.fetch_add(1, Ordering::Relaxed);
        let response_header = TunnelHeader::new(
            seq,
            now_us(),
            session.game_server,    // game server is now the "source"
            session.client_addr,    // client's original IP is the "destination"
        );

        let packet = response_header.encode_with_payload(payload);

        // Send back to client via the data plane socket
        match data_socket.send_to(&packet, session.client_addr).await {
            Ok(sent) => {
                metrics.record_relay(sent as u64);
                trace!(
                    client = %session.client_addr,
                    seq = seq,
                    payload_len = len,
                    "Sent response to client"
                );
            }
            Err(e) => {
                debug!(
                    client = %session.client_addr,
                    error = %e,
                    "Failed to send response to client"
                );
            }
        }
    }
}

/// Periodically clean up expired sessions, stale abuse data, and start
/// response listeners for new sessions.
pub async fn run_session_manager(
    engine: Arc<RelayEngine>,
    data_socket: Arc<UdpSocket>,
    abuse_detector: Arc<tokio::sync::Mutex<AbuseDetector>>,
    metrics: Arc<ProxyMetrics>,
) {
    let mut known_sessions: HashMap<SocketAddrV4, tokio::task::JoinHandle<()>> = HashMap::new();
    let mut interval = tokio::time::interval(Duration::from_secs(5));

    loop {
        interval.tick().await;

        // Clean up expired sessions
        let removed = engine.cleanup_expired().await;
        if removed > 0 {
            info!("Cleaned up {} expired sessions", removed);
        }

        // Clean up abuse detector state
        {
            let mut abuse = abuse_detector.lock().await;
            abuse.cleanup();
        }

        // Remove join handles for sessions that no longer exist
        let sessions_lock = engine.sessions();
        let active = sessions_lock.read().await;
        known_sessions.retain(|addr, handle| {
            if !active.contains_key(addr) {
                handle.abort();
                false
            } else {
                true
            }
        });

        // Start response listeners for new sessions
        for (addr, session) in active.iter() {
            if !known_sessions.contains_key(addr) {
                let session = Arc::clone(session);
                let data_socket = Arc::clone(&data_socket);
                let metrics = Arc::clone(&metrics);

                let handle = tokio::spawn(async move {
                    run_session_response_listener(session, data_socket, metrics).await;
                });

                known_sessions.insert(*addr, handle);
            }
        }

        let session_count = active.len();
        drop(active);

        if session_count > 0 {
            debug!(
                sessions = session_count,
                listeners = known_sessions.len(),
                "Session manager tick"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[tokio::test]
    async fn test_relay_engine_session_lifecycle() {
        let engine = RelayEngine::new(10);

        assert_eq!(engine.active_sessions().await, 0);
        assert!(engine.can_accept().await);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        let session = engine.get_or_create_session(client, server).await.unwrap();
        assert_eq!(session.client_addr, client);
        assert_eq!(session.game_server, server);
        assert_eq!(engine.active_sessions().await, 1);

        // Getting same client again should return existing session
        let session2 = engine.get_or_create_session(client, server).await.unwrap();
        assert_eq!(engine.active_sessions().await, 1);
        assert_eq!(session.client_addr, session2.client_addr);
    }

    #[tokio::test]
    async fn test_relay_engine_max_sessions() {
        let engine = RelayEngine::new(1);

        let client1 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1000);
        let client2 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 2000);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        engine.get_or_create_session(client1, server).await.unwrap();
        assert!(engine.get_or_create_session(client2, server).await.is_err());
    }

    #[tokio::test]
    async fn test_inbound_decode_and_forward() {
        // Test that we can encode a tunnel packet and decode it
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let header = TunnelHeader::new(1, now_us(), src, dst).with_session_token(42);
        let payload = b"game data payload";

        let packet = header.encode_with_payload(payload);

        // Simulate what the relay does: decode
        let (decoded_header, decoded_payload) =
            TunnelHeader::decode_with_payload(&packet).unwrap();

        assert_eq!(decoded_header.orig_src_addr(), src);
        assert_eq!(decoded_header.orig_dst_addr(), dst);
        assert_eq!(decoded_header.session_token, 42);
        assert_eq!(decoded_payload, payload);
    }
}
