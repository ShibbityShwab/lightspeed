//! # Proxy Packet Relay
//!
//! Core relay engine: receives tunnel packets from clients, strips header,
//! forwards to game server, receives response, re-wraps, returns to client.
//!
//! ## FEC Support
//!
//! When a client sends version-2 packets, FEC is active:
//! - **Inbound**: Data packets have FEC header stripped before forwarding.
//!   Parity packets are absorbed (not forwarded) and used for recovery.
//! - **Outbound**: Responses are FEC-encoded with the same K for the client.
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

use bytes::BytesMut;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};

use lightspeed_protocol::{
    FecDecoder, FecEncoder, FecHeader, TunnelHeader, FEC_HEADER_SIZE, HEADER_SIZE,
};

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
    /// Whether this client uses FEC (detected from first v2 packet).
    pub fec_enabled: bool,
    /// FEC block size (K) from the client's packets.
    pub fec_k: u8,
    /// FEC decoder for inbound packets (client → proxy).
    /// Protected by tokio Mutex since it's accessed from the inbound loop.
    pub fec_decoder: tokio::sync::Mutex<FecDecoder>,
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
    /// If the client already has an active session, returns `(session, false)`.
    /// Otherwise, creates a new outbound socket and session, returning `(session, true)`.
    /// The caller should spawn a response listener when `is_new` is true.
    pub async fn get_or_create_session(
        &self,
        client_addr: SocketAddrV4,
        game_server: SocketAddrV4,
        fec_enabled: bool,
        fec_k: u8,
    ) -> anyhow::Result<(Arc<ClientSession>, bool)> {
        // Fast path: check existing session
        {
            let sessions = self.sessions.read().await;
            if let Some(session) = sessions.get(&client_addr) {
                return Ok((Arc::clone(session), false));
            }
        }

        // Slow path: create new session
        if !self.can_accept().await {
            anyhow::bail!("Max sessions ({}) reached", self.max_sessions);
        }

        // Bind a new outbound socket for this client's game traffic
        let outbound_socket = UdpSocket::bind("0.0.0.0:0").await?;

        info!(
            client = %client_addr,
            game_server = %game_server,
            outbound_port = %outbound_socket.local_addr()?,
            fec = fec_enabled,
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
            fec_enabled,
            fec_k,
            fec_decoder: tokio::sync::Mutex::new(FecDecoder::new()),
        });

        let mut sessions = self.sessions.write().await;
        sessions.insert(client_addr, Arc::clone(&session));

        Ok((session, true))
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
                    metrics.record_rate_limit();
                    continue;
                }
                RateLimitResult::BandwidthExceeded => {
                    trace!(client = %client_addr, "Rate limited (BPS)");
                    metrics.record_drop();
                    metrics.record_rate_limit();
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
                metrics.record_auth_rejection();
                continue;
            }
        }

        // Handle control packets (keepalive always uses v1)
        if header.is_keepalive() {
            trace!(client = %client_addr, seq = header.sequence, "Keepalive received");
            let response = TunnelHeader::keepalive(header.sequence, now_us())
                .with_session_token(header.session_token);
            let response_bytes = response.encode();
            let _ = data_socket.send_to(&response_bytes, client_addr).await;
            continue;
        }

        if header.is_fin() {
            info!(client = %client_addr, "Client sent FIN — closing session");
            let sessions_lock = engine.sessions();
            let mut sessions = sessions_lock.write().await;
            sessions.remove(&client_addr);
            continue;
        }

        // Get the original destination (game server) from the header
        let game_server = header.orig_dst_addr();
        let is_fec = header.has_fec();

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
                    metrics.record_abuse_block();
                    continue;
                }
                AbuseCheckResult::Banned => {
                    trace!(client = %client_addr, "Blocked: client is banned");
                    metrics.record_drop();
                    metrics.record_abuse_block();
                    continue;
                }
                AbuseCheckResult::ReflectionDetected => {
                    warn!(client = %client_addr, "Blocked: reflection attack detected");
                    metrics.record_drop();
                    metrics.record_abuse_block();
                    continue;
                }
                AbuseCheckResult::AmplificationDetected => {
                    warn!(client = %client_addr, "Blocked: amplification detected");
                    metrics.record_drop();
                    metrics.record_abuse_block();
                    continue;
                }
            }
        }

        // ── FEC: Parse FEC header if version 2 ──────────────────────
        let (fec_hdr, game_payload) = if is_fec {
            if payload.len() < FEC_HEADER_SIZE {
                debug!(client = %client_addr, "FEC packet too short");
                metrics.record_drop();
                continue;
            }
            let mut fec_slice: &[u8] = &payload[..FEC_HEADER_SIZE];
            match FecHeader::decode(&mut fec_slice) {
                Some(fh) => (Some(fh), &payload[FEC_HEADER_SIZE..]),
                None => {
                    debug!(client = %client_addr, "Invalid FEC header");
                    metrics.record_drop();
                    continue;
                }
            }
        } else {
            (None, payload)
        };

        // Determine FEC k_size for session creation
        let fec_k = fec_hdr.as_ref().map(|h| h.k_size).unwrap_or(4);

        // Get or create session for this client
        let (session, is_new) = match engine
            .get_or_create_session(client_addr, game_server, is_fec, fec_k)
            .await
        {
            Ok(s) => s,
            Err(e) => {
                warn!(client = %client_addr, error = %e, "Failed to create session");
                metrics.record_drop();
                continue;
            }
        };

        // Immediately spawn response listener for new sessions
        if is_new {
            metrics.record_session_created();
            let session_clone = Arc::clone(&session);
            let data_socket_clone = Arc::clone(&data_socket);
            let metrics_clone = Arc::clone(&metrics);
            tokio::spawn(async move {
                run_session_response_listener(session_clone, data_socket_clone, metrics_clone)
                    .await;
            });
            info!(client = %client_addr, fec = is_fec, "Response listener spawned immediately");
        }

        // ── Handle FEC parity packets ───────────────────────────────
        if let Some(ref fh) = fec_hdr {
            if fh.is_parity() {
                // Parity packet: do NOT forward to game server.
                // Feed into FEC decoder for potential recovery.
                metrics.record_fec_parity();
                let parity_data = bytes::Bytes::copy_from_slice(game_payload);
                let mut decoder = session.fec_decoder.lock().await;
                if let Some((_idx, recovered)) = decoder.receive_parity(fh, parity_data) {
                    // Recovered a lost packet — forward to game server
                    metrics.record_fec_recovery();
                    info!(
                        client = %client_addr,
                        block = fh.block_id,
                        recovered_len = recovered.len(),
                        "🔧 FEC recovered lost packet on proxy"
                    );
                    match session.outbound_socket.send_to(&recovered, game_server).await {
                        Ok(sent) => {
                            metrics.record_relay(sent as u64);
                        }
                        Err(e) => {
                            debug!(client = %client_addr, error = %e, "Failed to forward recovered packet");
                        }
                    }
                }
                // Periodic GC
                decoder.gc();
                continue; // Don't forward parity to game server
            } else {
                // Data packet with FEC: track in decoder, then forward game_payload
                let data_bytes = bytes::Bytes::copy_from_slice(game_payload);
                let mut decoder = session.fec_decoder.lock().await;
                decoder.receive_data(fh, data_bytes);
            }
        }

        // Forward the raw game payload to the game server
        match session.outbound_socket.send_to(game_payload, game_server).await {
            Ok(sent) => {
                metrics.record_relay(sent as u64);
                trace!(
                    client = %client_addr,
                    game_server = %game_server,
                    seq = header.sequence,
                    payload_len = game_payload.len(),
                    fec = is_fec,
                    "Forwarded to game server"
                );

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
///
/// If the client uses FEC, responses are also FEC-encoded.
pub async fn run_session_response_listener(
    session: Arc<ClientSession>,
    data_socket: Arc<UdpSocket>,
    metrics: Arc<ProxyMetrics>,
) {
    let mut buf = vec![0u8; 2048];

    // Create FEC encoder if client supports FEC
    let mut fec_encoder = if session.fec_enabled {
        Some(FecEncoder::new(session.fec_k))
    } else {
        None
    };

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
        let seq = session.response_seq.fetch_add(1, Ordering::Relaxed);

        if let Some(ref mut encoder) = fec_encoder {
            // ── FEC mode: wrap with FEC header ──────────────────────
            let block_id = encoder.block_id();
            let index = encoder.current_index();

            // Build: [TunnelHeader v2][FecHeader data][game_response]
            let response_header = TunnelHeader::new_fec(
                seq,
                now_us(),
                session.game_server,
                session.client_addr,
            );
            let fec_hdr = FecHeader::data(block_id, index, session.fec_k);

            let mut pkt_buf =
                BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + payload.len());
            pkt_buf.extend_from_slice(&response_header.encode());
            fec_hdr.encode(&mut pkt_buf);
            pkt_buf.extend_from_slice(payload);

            // Feed into FEC encoder
            let parity = encoder.add_packet(payload);

            // Send data packet to client
            match data_socket.send_to(&pkt_buf, session.client_addr).await {
                Ok(sent) => {
                    metrics.record_relay(sent as u64);
                    trace!(
                        client = %session.client_addr,
                        seq = seq,
                        fec_block = block_id,
                        fec_idx = index,
                        "Sent FEC response to client"
                    );
                }
                Err(e) => {
                    debug!(
                        client = %session.client_addr,
                        error = %e,
                        "Failed to send FEC response to client"
                    );
                }
            }

            // If block complete, send parity packet
            if let Some(parity_bytes) = parity {
                let parity_seq = session.response_seq.fetch_add(1, Ordering::Relaxed);
                let parity_header = TunnelHeader::new_fec(
                    parity_seq,
                    now_us(),
                    session.game_server,
                    session.client_addr,
                );
                let parity_fec = FecHeader::parity(block_id, session.fec_k);

                let mut parity_buf =
                    BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len());
                parity_buf.extend_from_slice(&parity_header.encode());
                parity_fec.encode(&mut parity_buf);
                parity_buf.extend_from_slice(&parity_bytes);

                match data_socket.send_to(&parity_buf, session.client_addr).await {
                    Ok(sent) => {
                        metrics.record_relay(sent as u64);
                        trace!(
                            client = %session.client_addr,
                            seq = parity_seq,
                            fec_block = block_id,
                            "Sent parity to client"
                        );
                    }
                    Err(e) => {
                        debug!(
                            client = %session.client_addr,
                            error = %e,
                            "Failed to send parity to client"
                        );
                    }
                }
            }
        } else {
            // ── Non-FEC mode: original behavior ─────────────────────
            let response_header = TunnelHeader::new(
                seq,
                now_us(),
                session.game_server,
                session.client_addr,
            );

            let packet = response_header.encode_with_payload(payload);

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
            metrics.record_session_expired(removed as u64);
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

        let (session, is_new) = engine
            .get_or_create_session(client, server, false, 4)
            .await
            .unwrap();
        assert!(is_new);
        assert_eq!(session.client_addr, client);
        assert_eq!(session.game_server, server);
        assert!(!session.fec_enabled);
        assert_eq!(engine.active_sessions().await, 1);

        // Getting same client again should return existing session
        let (session2, is_new2) = engine
            .get_or_create_session(client, server, false, 4)
            .await
            .unwrap();
        assert!(!is_new2);
        assert_eq!(engine.active_sessions().await, 1);
        assert_eq!(session.client_addr, session2.client_addr);
    }

    #[tokio::test]
    async fn test_relay_engine_fec_session() {
        let engine = RelayEngine::new(10);

        let client = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        let (session, is_new) = engine
            .get_or_create_session(client, server, true, 4)
            .await
            .unwrap();
        assert!(is_new);
        assert!(session.fec_enabled);
        assert_eq!(session.fec_k, 4);
    }

    #[tokio::test]
    async fn test_relay_engine_max_sessions() {
        let engine = RelayEngine::new(1);

        let client1 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 1000);
        let client2 = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 2000);
        let server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        engine
            .get_or_create_session(client1, server, false, 4)
            .await
            .unwrap();
        assert!(engine
            .get_or_create_session(client2, server, false, 4)
            .await
            .is_err());
    }

    #[tokio::test]
    async fn test_inbound_decode_and_forward() {
        // Test that we can encode a tunnel packet and decode it
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let header = TunnelHeader::new(1, now_us(), src, dst).with_session_token(42);
        let payload = b"game data payload";

        let packet = header.encode_with_payload(payload);

        let (decoded_header, decoded_payload) = TunnelHeader::decode_with_payload(&packet).unwrap();

        assert_eq!(decoded_header.orig_src_addr(), src);
        assert_eq!(decoded_header.orig_dst_addr(), dst);
        assert_eq!(decoded_header.session_token, 42);
        assert!(!decoded_header.has_fec());
        assert_eq!(decoded_payload, payload);
    }

    #[tokio::test]
    async fn test_fec_packet_decode() {
        // Test FEC v2 packet encoding/decoding
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let header = TunnelHeader::new_fec(1, now_us(), src, dst);

        let fec_hdr = FecHeader::data(0, 2, 4);
        let game_data = b"game data with fec";

        // Build: [TunnelHeader v2][FecHeader][payload]
        let mut pkt = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + game_data.len());
        pkt.extend_from_slice(&header.encode());
        fec_hdr.encode(&mut pkt);
        pkt.extend_from_slice(game_data);

        // Decode
        let (decoded_header, payload) = TunnelHeader::decode_with_payload(&pkt).unwrap();
        assert!(decoded_header.has_fec());

        // Parse FEC header from payload
        let mut fec_slice: &[u8] = &payload[..FEC_HEADER_SIZE];
        let decoded_fec = FecHeader::decode(&mut fec_slice).unwrap();
        assert_eq!(decoded_fec.block_id, 0);
        assert_eq!(decoded_fec.index, 2);
        assert_eq!(decoded_fec.k_size, 4);
        assert!(!decoded_fec.is_parity());

        let decoded_game_data = &payload[FEC_HEADER_SIZE..];
        assert_eq!(decoded_game_data, game_data);
    }
}
