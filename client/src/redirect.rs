//! # Local UDP Redirect
//!
//! Transparent local UDP proxy that captures game traffic and tunnels it
//! through the LightSpeed proxy network. This is the primary game integration
//! mode that works without pcap/Npcap or driver-level packet capture.
//!
//! ## How it works
//!
//! 1. Binds a local UDP socket on `127.0.0.1:<local_port>`
//! 2. Game connects to `127.0.0.1:<local_port>` instead of the real server
//! 3. All outbound UDP is wrapped in LightSpeed headers → sent to proxy
//! 4. Proxy forwards raw payload to the real game server
//! 5. Game server responds → proxy wraps response → sent back here
//! 6. Response is unwrapped and delivered to the game on the local socket
//!
//! ## FEC Mode (--fec)
//!
//! When FEC is enabled, packets are grouped into blocks of K. For each block,
//! an XOR parity packet is generated and sent alongside data. If any single
//! packet in a block is lost in transit, it can be recovered from the parity.
//! This adds ~25% bandwidth overhead at the default K=4.
//!
//! ## Usage
//!
//! ```text
//! lightspeed --game cs2 --proxy YOUR_PROXY_IP:4434 --game-server 192.168.1.1:27015
//! lightspeed --fec --proxy YOUR_PROXY_IP:4434 --game-server 192.168.1.1:27015
//! ```

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use lightspeed_protocol::{
    build_fec_data_packet, build_fec_parity_packet, decode_fec_payload, FecDecoder, FecEncoder,
    FecHeader, TunnelHeader, FEC_HEADER_SIZE,
};
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};

use crate::tunnel::adaptive::{AdaptiveConfig, InlineFecGate};
use crate::tunnel::budget::{self, PayloadFit};
use crate::tunnel::transport::TunnelTransport;

/// Get current timestamp in microseconds.
fn now_us() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

/// Whether a decoded FEC payload is a parity packet. Used to tell an FEC
/// recovery (parity that yielded data) from an ordinary data packet, because
/// `decode_fec_payload` returns `Some` for both.
fn is_fec_parity(payload: &[u8]) -> bool {
    if payload.len() < FEC_HEADER_SIZE {
        return false;
    }
    let mut slice: &[u8] = &payload[..FEC_HEADER_SIZE];
    FecHeader::decode(&mut slice)
        .map(|h| h.is_parity())
        .unwrap_or(false)
}

/// Statistics for the redirect proxy.
#[derive(Debug)]
pub struct RedirectStats {
    pub packets_to_proxy: AtomicU64,
    pub packets_from_proxy: AtomicU64,
    pub bytes_to_proxy: AtomicU64,
    pub bytes_from_proxy: AtomicU64,
    pub packets_to_game: AtomicU64,
    pub packets_from_game: AtomicU64,
    pub errors: AtomicU64,
    pub fec_parity_sent: AtomicU64,
    pub fec_recovered: AtomicU64,
    /// Current adaptive parity-to-data ratio in basis points (2500 = 1/4).
    /// Zero while adaptive FEC is off or parity is suppressed.
    pub adaptive_parity_ratio_bp: AtomicU64,
    /// Game payloads forwarded with fragmentation allowed for exceeding the
    /// conservative tunnel budget.
    pub payloads_over_budget: AtomicU64,
}

impl RedirectStats {
    pub fn new() -> Self {
        Self {
            packets_to_proxy: AtomicU64::new(0),
            packets_from_proxy: AtomicU64::new(0),
            bytes_to_proxy: AtomicU64::new(0),
            bytes_from_proxy: AtomicU64::new(0),
            packets_to_game: AtomicU64::new(0),
            packets_from_game: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            fec_parity_sent: AtomicU64::new(0),
            fec_recovered: AtomicU64::new(0),
            adaptive_parity_ratio_bp: AtomicU64::new(0),
            payloads_over_budget: AtomicU64::new(0),
        }
    }
}

/// Local UDP redirect proxy.
///
/// Sits between the game and the LightSpeed proxy, tunneling all UDP traffic.
pub struct UdpRedirect {
    /// Local port to listen on (game connects here).
    local_port: u16,
    /// The real game server address (proxy will forward to this).
    game_server: SocketAddrV4,
    /// The LightSpeed proxy address.
    proxy_addr: SocketAddrV4,
    /// Sequence counter for LightSpeed headers.
    sequence: AtomicU16,
    /// FEC enabled.
    fec_enabled: bool,
    /// FEC block size (K data packets per parity).
    fec_k: u8,
    /// Adaptive FEC/duplication policy. Disabled by default, which keeps the
    /// fixed `fec_k` block with parity on every block.
    adaptive: AdaptiveConfig,
    /// Use TCP for the client↔proxy leg (UDP-restricted networks).
    tcp: bool,
    /// Stats.
    pub stats: Arc<RedirectStats>,
}

impl UdpRedirect {
    /// Create a new UDP redirect.
    pub fn new(local_port: u16, game_server: SocketAddrV4, proxy_addr: SocketAddrV4) -> Self {
        Self {
            local_port,
            game_server,
            proxy_addr,
            sequence: AtomicU16::new(0),
            fec_enabled: false,
            fec_k: 4,
            adaptive: AdaptiveConfig::default(),
            tcp: false,
            stats: Arc::new(RedirectStats::new()),
        }
    }

    /// Enable FEC with the given block size.
    pub fn with_fec(mut self, k_size: u8) -> Self {
        self.fec_enabled = true;
        self.fec_k = k_size.clamp(2, 16);
        self
    }

    /// Enable adaptive FEC and loss-gated duplication.
    ///
    /// The opt-in mode. This also turns FEC on at the clamped effective block
    /// size. On a clean link no parity is emitted; parity returns only after a
    /// measured FEC recovery and is bounded by [`AdaptiveConfig::max_overhead_pct`].
    pub fn with_adaptive_fec(mut self, cfg: AdaptiveConfig) -> Self {
        if cfg.enabled {
            self.fec_enabled = true;
            self.fec_k = cfg.effective_k();
            self.adaptive = cfg;
        }
        self
    }

    /// Use TCP for the client↔proxy leg instead of UDP.
    pub fn with_tcp(mut self) -> Self {
        self.tcp = true;
        self
    }

    fn next_seq(&self) -> u16 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    /// Run the redirect proxy with an external shutdown signal.
    ///
    /// Identical to [`run`] but terminates when `shutdown` fires instead
    /// of waiting for Ctrl-C.  Used by the GUI engine.
    pub async fn run_with_shutdown(
        &self,
        shutdown: tokio::sync::oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
        self.run_inner(Some(shutdown)).await
    }

    /// Run the redirect proxy. This is the main event loop.
    ///
    /// Spawns two tasks:
    /// - **Outbound**: Game → Local Socket → Tunnel → Proxy → Game Server
    /// - **Inbound**: Game Server → Proxy → Tunnel → Local Socket → Game
    pub async fn run(&self) -> anyhow::Result<()> {
        self.run_inner(None).await
    }

    async fn run_inner(
        &self,
        shutdown: Option<tokio::sync::oneshot::Receiver<()>>,
    ) -> anyhow::Result<()> {
        // Bind the local socket where the game will send traffic
        let local_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.local_port);
        let local_socket = Arc::new(UdpSocket::bind(local_addr).await?);
        info!("🎮 Local game socket bound to {}", local_addr);

        // Establish the tunnel transport for proxy communication (UDP or TCP).
        // The send half is shared by the outbound + keepalive tasks; the read
        // half is owned by the inbound task.
        let mut transport = if self.tcp {
            TunnelTransport::connect_tcp(self.proxy_addr).await?
        } else {
            TunnelTransport::connect_udp(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).await?
        };
        // The UDP sender is created with no peer; point it at the real proxy
        // before the split hands it to the outbound task. TCP's peer is fixed.
        transport.set_proxy(self.proxy_addr);
        let (tunnel_sender, mut tunnel_reader) = transport.split();
        if self.tcp {
            info!("🌐 TCP tunnel connected to {}", self.proxy_addr);
        } else {
            info!("🌐 UDP tunnel bound");
        }

        let game_server = self.game_server;
        let proxy_addr = self.proxy_addr;
        let stats = Arc::clone(&self.stats);
        let fec_enabled = self.fec_enabled;
        let fec_k = self.fec_k;
        let gate = Arc::new(std::sync::Mutex::new(InlineFecGate::new(
            self.adaptive,
            fec_k,
        )));
        let gate_k = gate.lock().unwrap().block_k();

        // Track the game client's address (set on first packet received)
        let game_client_addr: Arc<RwLock<Option<std::net::SocketAddr>>> =
            Arc::new(RwLock::new(None));

        if fec_enabled {
            info!(
                "⚡ Redirect active (FEC K={}): Game → 127.0.0.1:{} → Proxy {} → Server {}",
                fec_k, self.local_port, proxy_addr, game_server
            );
        } else {
            info!(
                "⚡ Redirect active: Game → 127.0.0.1:{} → Proxy {} → Server {}",
                self.local_port, proxy_addr, game_server
            );
        }

        // ── Outbound task: Game → Proxy ─────────────────────────────
        let outbound_handle = {
            let local_socket = Arc::clone(&local_socket);
            let tunnel_sender = tunnel_sender.clone();
            let game_client_addr = Arc::clone(&game_client_addr);
            let stats = Arc::clone(&stats);
            let gate = Arc::clone(&gate);

            let seq_counter = Arc::new(AtomicU16::new(0));
            let seq = Arc::clone(&seq_counter);

            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];

                // FEC encoder (only if enabled)
                let mut fec_encoder = if fec_enabled {
                    Some(FecEncoder::new(gate_k))
                } else {
                    None
                };

                loop {
                    // Receive from the game client
                    let (len, from_addr) = match local_socket.recv_from(&mut buf).await {
                        Ok(r) => r,
                        Err(e) => {
                            warn!("Local socket recv error: {}", e);
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };

                    let payload = &buf[..len];

                    if let PayloadFit::OverBudget {
                        payload_len,
                        budget,
                    } = budget::classify(len, fec_encoder.is_some())
                    {
                        stats.payloads_over_budget.fetch_add(1, Ordering::Relaxed);
                        warn!(
                            payload_len = payload_len,
                            budget = budget,
                            "Forwarding oversized game payload; the kernel may fragment it"
                        );
                    }

                    // Remember the game client's address
                    {
                        let mut addr = game_client_addr.write().await;
                        if addr.is_none() {
                            info!("🎮 Game client connected from {}", from_addr);
                        }
                        *addr = Some(from_addr);
                    }

                    stats.packets_from_game.fetch_add(1, Ordering::Relaxed);

                    let seq_num = seq.fetch_add(1, Ordering::Relaxed);

                    let orig_src = match from_addr {
                        std::net::SocketAddr::V4(v4) => v4,
                        _ => SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                    };

                    if let Some(ref mut encoder) = fec_encoder {
                        let block_id = encoder.block_id();
                        let index = encoder.current_index();
                        let k_size = encoder.k_size();
                        let header =
                            TunnelHeader::new_fec(seq_num, now_us(), orig_src, game_server)
                                .with_session_token(crate::session::session_token());
                        let fec_hdr = FecHeader::data(block_id, index, k_size);
                        let pkt_buf = build_fec_data_packet(&header, &fec_hdr, payload);
                        let parity = encoder.add_packet(payload);

                        // Same gate as the relay: record the completed block and
                        // drop its parity while the link is clean.
                        let (emit_parity, ratio_bp) = {
                            let mut g = gate.lock().unwrap();
                            let emit = g.emit_parity();
                            if parity.is_some() {
                                g.record_block(emit);
                            }
                            (emit, g.parity_ratio_bp())
                        };
                        stats
                            .adaptive_parity_ratio_bp
                            .store(ratio_bp, Ordering::Relaxed);

                        crate::latency::record_outbound(*game_server.ip());
                        let send_result = if budget::datagram_fits(pkt_buf.len()) {
                            tunnel_sender.send(&pkt_buf).await
                        } else {
                            tunnel_sender.send_may_fragment(&pkt_buf).await
                        };
                        match send_result {
                            Ok(sent) => {
                                stats.packets_to_proxy.fetch_add(1, Ordering::Relaxed);
                                stats
                                    .bytes_to_proxy
                                    .fetch_add(sent as u64, Ordering::Relaxed);
                                trace!(
                                    seq = seq_num,
                                    fec_block = block_id,
                                    fec_idx = index,
                                    "Game → Proxy (FEC data)"
                                );
                            }
                            Err(e) => {
                                warn!("Tunnel send error: {}", e);
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }

                        if let Some(parity_bytes) = parity.filter(|_| emit_parity) {
                            let parity_seq = seq.fetch_add(1, Ordering::Relaxed);
                            let parity_header =
                                TunnelHeader::new_fec(parity_seq, now_us(), orig_src, game_server)
                                    .with_session_token(crate::session::session_token());
                            let parity_fec = FecHeader::parity(block_id, k_size);
                            let parity_buf =
                                build_fec_parity_packet(&parity_header, &parity_fec, &parity_bytes);

                            let parity_result = if budget::datagram_fits(parity_buf.len()) {
                                tunnel_sender.send(&parity_buf).await
                            } else {
                                tunnel_sender.send_may_fragment(&parity_buf).await
                            };
                            match parity_result {
                                Ok(sent) => {
                                    stats.packets_to_proxy.fetch_add(1, Ordering::Relaxed);
                                    stats
                                        .bytes_to_proxy
                                        .fetch_add(sent as u64, Ordering::Relaxed);
                                    stats.fec_parity_sent.fetch_add(1, Ordering::Relaxed);
                                    trace!(seq = parity_seq, fec_block = block_id, "Parity sent");
                                }
                                Err(e) => {
                                    warn!("Parity send error: {}", e);
                                    stats.errors.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }
                    } else {
                        // ── Non-FEC mode: original behavior ─────────────
                        let header = TunnelHeader::new(seq_num, now_us(), orig_src, game_server)
                            .with_session_token(crate::session::session_token());
                        let packet = header.encode_with_payload(payload);

                        crate::latency::record_outbound(*game_server.ip());
                        let send_result = if budget::datagram_fits(packet.len()) {
                            tunnel_sender.send(&packet).await
                        } else {
                            tunnel_sender.send_may_fragment(&packet).await
                        };
                        match send_result {
                            Ok(sent) => {
                                stats.packets_to_proxy.fetch_add(1, Ordering::Relaxed);
                                stats
                                    .bytes_to_proxy
                                    .fetch_add(sent as u64, Ordering::Relaxed);
                                trace!(seq = seq_num, payload_len = len, "Game → Proxy");
                            }
                            Err(e) => {
                                warn!("Tunnel send error: {}", e);
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            })
        };

        // ── Inbound task: Proxy → Game ──────────────────────────────
        let inbound_handle = {
            let local_socket = Arc::clone(&local_socket);
            let game_client_addr = Arc::clone(&game_client_addr);
            let stats = Arc::clone(&stats);
            let gate = Arc::clone(&gate);

            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];

                // FEC decoder (only if enabled)
                let mut fec_decoder = if fec_enabled {
                    Some(FecDecoder::new())
                } else {
                    None
                };

                let mut gc_counter: u32 = 0;

                loop {
                    // Receive from the proxy
                    let len = match tunnel_reader.recv(&mut buf).await {
                        Ok(Some(n)) => n,
                        Ok(None) => {
                            debug!("Tunnel closed by proxy");
                            break;
                        }
                        Err(e) => {
                            if tunnel_reader.is_tcp() {
                                debug!("Tunnel socket recv error: {}", e);
                                break;
                            }
                            warn!("Tunnel socket recv error: {}", e);
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };

                    stats.packets_from_proxy.fetch_add(1, Ordering::Relaxed);
                    stats
                        .bytes_from_proxy
                        .fetch_add(len as u64, Ordering::Relaxed);

                    // Decode the LightSpeed header
                    let (header, payload) = match TunnelHeader::decode_with_payload(&buf[..len]) {
                        Ok(r) => r,
                        Err(e) => {
                            debug!("Invalid tunnel response: {}", e);
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };

                    if !header.is_keepalive() {
                        crate::latency::record_inbound(*header.orig_src_addr().ip());
                    }

                    // Get the game client address
                    let game_addr = {
                        let addr = game_client_addr.read().await;
                        *addr
                    };

                    let game_addr = match game_addr {
                        Some(addr) => addr,
                        None => {
                            debug!("Response received but no game client connected yet");
                            continue;
                        }
                    };

                    if header.has_fec() {
                        if let Some(decoder) = fec_decoder.as_mut() {
                            let is_parity = is_fec_parity(payload);
                            let decoded = decode_fec_payload(payload, decoder);
                            let recovered = is_parity && decoded.is_some();
                            let ratio_bp = {
                                let mut g = gate.lock().unwrap();
                                g.observe_response(recovered, header.timestamp_us, now_us());
                                g.parity_ratio_bp()
                            };
                            stats
                                .adaptive_parity_ratio_bp
                                .store(ratio_bp, Ordering::Relaxed);

                            if let Some(data) = decoded {
                                stats.fec_recovered.fetch_add(1, Ordering::Relaxed);
                                info!(
                                    block = header.sequence,
                                    recovered_len = data.len(),
                                    "🔧 FEC recovered lost packet!"
                                );
                                match local_socket.send_to(&data, game_addr).await {
                                    Ok(_) => {
                                        stats.packets_to_game.fetch_add(1, Ordering::Relaxed);
                                    }
                                    Err(e) => {
                                        warn!("Local socket send error: {}", e);
                                        stats.errors.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }

                            // Periodic GC of expired FEC blocks
                            gc_counter += 1;
                            if gc_counter.is_multiple_of(100) {
                                decoder.gc();
                            }
                        } // end if let Some(decoder)
                    } else {
                        // ── Non-FEC mode: forward payload directly ──────
                        {
                            let mut g = gate.lock().unwrap();
                            g.observe_response(false, header.timestamp_us, now_us());
                            stats
                                .adaptive_parity_ratio_bp
                                .store(g.parity_ratio_bp(), Ordering::Relaxed);
                        }
                        match local_socket.send_to(payload, game_addr).await {
                            Ok(_) => {
                                stats.packets_to_game.fetch_add(1, Ordering::Relaxed);
                                trace!(payload_len = payload.len(), "Proxy → Game");
                            }
                            Err(e) => {
                                warn!("Local socket send error: {}", e);
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            })
        };

        // ── Keepalive task ──────────────────────────────────────────
        let keepalive_handle = {
            let tunnel_sender = tunnel_sender.clone();
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                let mut seq: u16 = 60000; // Start high to not conflict with data seqs
                loop {
                    interval.tick().await;
                    let header = TunnelHeader::keepalive(seq, now_us())
                        .with_session_token(crate::session::session_token());
                    let packet = header.encode();
                    let _ = tunnel_sender.send(&packet).await;
                    seq = seq.wrapping_add(1);
                }
            })
        };

        // ── Stats logger ────────────────────────────────────────────
        let stats_handle = {
            let stats = Arc::clone(&self.stats);
            let fec_enabled = self.fec_enabled;
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(10));
                loop {
                    interval.tick().await;
                    let to_proxy = stats.packets_to_proxy.load(Ordering::Relaxed);
                    let from_proxy = stats.packets_from_proxy.load(Ordering::Relaxed);
                    let to_game = stats.packets_to_game.load(Ordering::Relaxed);
                    let from_game = stats.packets_from_game.load(Ordering::Relaxed);
                    let bytes_out = stats.bytes_to_proxy.load(Ordering::Relaxed);
                    let bytes_in = stats.bytes_from_proxy.load(Ordering::Relaxed);
                    let errors = stats.errors.load(Ordering::Relaxed);
                    let over_budget = stats.payloads_over_budget.load(Ordering::Relaxed);

                    if from_game > 0 || from_proxy > 0 {
                        if fec_enabled {
                            let parity = stats.fec_parity_sent.load(Ordering::Relaxed);
                            let recovered = stats.fec_recovered.load(Ordering::Relaxed);
                            info!(
                                "📊 Game→Proxy: {} pkts ({} bytes) | Proxy→Game: {} pkts ({} bytes) | FEC: {} parity sent, {} recovered | Errors: {} | Over-budget forwarded: {}",
                                to_proxy, bytes_out, to_game, bytes_in, parity, recovered, errors, over_budget
                            );
                        } else {
                            info!(
                                "📊 Game→Proxy: {} pkts ({} bytes) | Proxy→Game: {} pkts ({} bytes) | Errors: {} | Over-budget forwarded: {}",
                                to_proxy, bytes_out, to_game, bytes_in, errors, over_budget
                            );
                        }
                    }
                }
            })
        };

        info!("⚡ Press Ctrl+C to stop");
        info!(
            "📌 Configure your game to connect to: 127.0.0.1:{}",
            self.local_port
        );

        // Wait for shutdown — either GUI oneshot or Ctrl-C
        match shutdown {
            Some(rx) => {
                let _ = rx.await;
                info!("⚡ Redirect shutdown requested");
            }
            None => {
                tokio::signal::ctrl_c().await?;
                info!("⚡ Shutdown signal received");
            }
        }

        outbound_handle.abort();
        inbound_handle.abort();
        keepalive_handle.abort();
        stats_handle.abort();

        let stats = &self.stats;
        info!("📊 Final stats:");
        info!(
            "   Game → Proxy: {} packets, {} bytes",
            stats.packets_to_proxy.load(Ordering::Relaxed),
            stats.bytes_to_proxy.load(Ordering::Relaxed)
        );
        info!(
            "   Proxy → Game: {} packets, {} bytes",
            stats.packets_to_game.load(Ordering::Relaxed),
            stats.bytes_from_proxy.load(Ordering::Relaxed)
        );
        if self.fec_enabled {
            info!(
                "   FEC: {} parity sent, {} packets recovered",
                stats.fec_parity_sent.load(Ordering::Relaxed),
                stats.fec_recovered.load(Ordering::Relaxed),
            );
        }
        info!("   Errors: {}", stats.errors.load(Ordering::Relaxed));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_redirect_creates_sockets() {
        let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let proxy = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 4434);
        let redirect = UdpRedirect::new(0, game_server, proxy);

        assert_eq!(redirect.game_server, game_server);
        assert_eq!(redirect.proxy_addr, proxy);
        assert!(!redirect.fec_enabled);
    }

    #[tokio::test]
    async fn test_redirect_with_fec() {
        let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let proxy = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 4434);
        let redirect = UdpRedirect::new(0, game_server, proxy).with_fec(4);

        assert!(redirect.fec_enabled);
        assert_eq!(redirect.fec_k, 4);
    }

    #[tokio::test]
    async fn test_redirect_sequence_increments() {
        let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let proxy = SocketAddrV4::new(Ipv4Addr::new(192, 0, 2, 1), 4434);
        let redirect = UdpRedirect::new(0, game_server, proxy);

        assert_eq!(redirect.next_seq(), 0);
        assert_eq!(redirect.next_seq(), 1);
        assert_eq!(redirect.next_seq(), 2);
    }

    fn v4(addr: std::net::SocketAddr) -> SocketAddrV4 {
        match addr {
            std::net::SocketAddr::V4(a) => a,
            std::net::SocketAddr::V6(_) => panic!("expected IPv4"),
        }
    }

    async fn free_udp_port() -> u16 {
        let probe = UdpSocket::bind("127.0.0.1:0").await.unwrap();
        v4(probe.local_addr().unwrap()).port()
    }

    fn fec_data_packet(seq: u16, fec: &FecHeader, payload: &[u8]) -> Vec<u8> {
        let src = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        let dst = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1);
        let hdr = TunnelHeader::new_fec(seq, now_us(), src, dst)
            .with_session_token(crate::session::session_token());
        build_fec_data_packet(&hdr, fec, payload).to_vec()
    }

    fn fec_parity_packet(seq: u16, fec: &FecHeader, parity: &[u8]) -> Vec<u8> {
        let src = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0);
        let dst = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1);
        let hdr = TunnelHeader::new_fec(seq, now_us(), src, dst)
            .with_session_token(crate::session::session_token());
        build_fec_parity_packet(&hdr, fec, parity).to_vec()
    }

    /// Receive up to `n` FEC packets, ignoring keepalives, returning whether
    /// each was parity and the source address of the tunnel peer.
    async fn recv_fec_burst(
        proxy: &UdpSocket,
        n: usize,
        per_packet: Duration,
    ) -> Vec<(bool, SocketAddrV4)> {
        let mut out = Vec::new();
        let mut buf = vec![0u8; 4096];
        while out.len() < n {
            let (len, src) = match tokio::time::timeout(per_packet, proxy.recv_from(&mut buf)).await
            {
                Ok(Ok((l, s))) => (l, v4(s)),
                _ => break,
            };
            let Ok((header, body)) = TunnelHeader::decode_with_payload(&buf[..len]) else {
                continue;
            };
            if !header.has_fec() || body.len() < FEC_HEADER_SIZE {
                continue;
            }
            let mut slice: &[u8] = &body[..FEC_HEADER_SIZE];
            let Some(fh) = FecHeader::decode(&mut slice) else {
                continue;
            };
            out.push((fh.is_parity(), src));
        }
        out
    }

    async fn wait_until(timeout: Duration, mut predicate: impl FnMut() -> bool) -> bool {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            if predicate() {
                return true;
            }
            if tokio::time::Instant::now() >= deadline {
                return false;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    }

    #[test]
    fn redirect_adaptive_disabled_emits_fixed_parity() {
        let _guard = crate::session::token_test_guard();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        rt.block_on(async {
            let proxy = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let proxy_addr = v4(proxy.local_addr().unwrap());
            let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
            let local_port = free_udp_port().await;

            let redirect = UdpRedirect::new(local_port, game_server, proxy_addr).with_fec(4);
            let stats = Arc::clone(&redirect.stats);
            let (tx, rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move { redirect.run_with_shutdown(rx).await });
            tokio::time::sleep(Duration::from_millis(250)).await;

            let game = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let dst = SocketAddrV4::new(Ipv4Addr::LOCALHOST, local_port);
            for _ in 0..4 {
                game.send_to(b"data", dst).await.unwrap();
            }

            let burst = recv_fec_burst(&proxy, 5, Duration::from_millis(700)).await;
            assert_eq!(burst.len(), 5, "one full block plus its parity");
            assert_eq!(
                burst.iter().filter(|(parity, _)| *parity).count(),
                1,
                "an opted-out redirect keeps the fixed P=1 parity"
            );
            assert_eq!(
                stats.adaptive_parity_ratio_bp.load(Ordering::Relaxed),
                0,
                "the observable ratio stays zero without adaptive FEC"
            );

            let _ = tx.send(());
            let _ = task.await;
        });
    }

    #[test]
    fn redirect_adaptive_clean_link_sends_no_parity() {
        let _guard = crate::session::token_test_guard();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        rt.block_on(async {
            let proxy = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let proxy_addr = v4(proxy.local_addr().unwrap());
            let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
            let local_port = free_udp_port().await;

            let cfg = AdaptiveConfig {
                enabled: true,
                ..AdaptiveConfig::default()
            };
            let redirect =
                UdpRedirect::new(local_port, game_server, proxy_addr).with_adaptive_fec(cfg);
            let stats = Arc::clone(&redirect.stats);
            let (tx, rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move { redirect.run_with_shutdown(rx).await });
            tokio::time::sleep(Duration::from_millis(250)).await;

            let game = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let dst = SocketAddrV4::new(Ipv4Addr::LOCALHOST, local_port);
            for _ in 0..4 {
                game.send_to(b"data", dst).await.unwrap();
            }

            let burst = recv_fec_burst(&proxy, 5, Duration::from_millis(700)).await;
            assert_eq!(
                burst.len(),
                4,
                "a clean adaptive link sends only the four data packets"
            );
            assert!(
                burst.iter().all(|(parity, _)| !*parity),
                "a clean adaptive link must send no parity"
            );
            assert_eq!(stats.adaptive_parity_ratio_bp.load(Ordering::Relaxed), 0);

            let _ = tx.send(());
            let _ = task.await;
        });
    }

    #[test]
    fn redirect_adaptive_recovery_reenables_parity() {
        let _guard = crate::session::token_test_guard();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");
        rt.block_on(async {
            let proxy = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let proxy_addr = v4(proxy.local_addr().unwrap());
            let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
            let local_port = free_udp_port().await;

            let cfg = AdaptiveConfig {
                enabled: true,
                ..AdaptiveConfig::default()
            };
            let redirect =
                UdpRedirect::new(local_port, game_server, proxy_addr).with_adaptive_fec(cfg);
            let stats = Arc::clone(&redirect.stats);
            let (tx, rx) = tokio::sync::oneshot::channel();
            let task = tokio::spawn(async move { redirect.run_with_shutdown(rx).await });
            tokio::time::sleep(Duration::from_millis(250)).await;

            let game = UdpSocket::bind("127.0.0.1:0").await.unwrap();
            let dst = SocketAddrV4::new(Ipv4Addr::LOCALHOST, local_port);

            // One clean outbound packet teaches the test the tunnel source address,
            // and must arrive without parity.
            game.send_to(b"prime", dst).await.unwrap();
            let primed = recv_fec_burst(&proxy, 1, Duration::from_secs(2)).await;
            assert_eq!(primed.len(), 1, "the priming packet must arrive");
            assert!(!primed[0].0, "the priming packet must not carry parity");
            let tunnel_src = primed[0].1;

            // A proxy -> client block of K=2 with index 1 lost: the parity packet
            // recovers it, which is the loss signal the inline gate consumes.
            let mut enc = FecEncoder::new(2);
            let block = enc.block_id();
            let idx0 = enc.current_index();
            assert!(enc.add_packet(b"alpha").is_none());
            let parity = enc.add_packet(b"bravo").expect("parity after K=2");
            proxy
                .send_to(
                    &fec_data_packet(1, &FecHeader::data(block, idx0, 2), b"alpha"),
                    tunnel_src,
                )
                .await
                .unwrap();
            proxy
                .send_to(
                    &fec_parity_packet(2, &FecHeader::parity(block, 2), &parity),
                    tunnel_src,
                )
                .await
                .unwrap();

            assert!(
                wait_until(Duration::from_secs(2), || {
                    stats.adaptive_parity_ratio_bp.load(Ordering::Relaxed) == 2_500
                })
                .await,
                "the FEC recovery must turn inline parity back on"
            );

            for _ in 0..4 {
                game.send_to(b"data", dst).await.unwrap();
            }
            let burst = recv_fec_burst(&proxy, 5, Duration::from_millis(700)).await;
            assert_eq!(
                burst.iter().filter(|(parity, _)| *parity).count(),
                1,
                "one parity must follow the measured recovery"
            );

            let _ = tx.send(());
            let _ = task.await;
        });
    }
}
