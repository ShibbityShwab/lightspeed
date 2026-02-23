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
//! This adds ~25% bandwidth overhead (K=4) vs ExitLag's 200-300%.
//!
//! ## Usage
//!
//! ```
//! lightspeed --game cs2 --proxy 149.28.84.139:4434 --game-server 192.168.1.1:27015
//! lightspeed --fec --proxy 149.28.84.139:4434 --game-server 192.168.1.1:27015
//! ```

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::BytesMut;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};

use lightspeed_protocol::{
    FecDecoder, FecEncoder, FecHeader, TunnelHeader, FEC_HEADER_SIZE, HEADER_SIZE,
};

/// Get current timestamp in microseconds.
fn now_us() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
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
            stats: Arc::new(RedirectStats::new()),
        }
    }

    /// Enable FEC with the given block size.
    pub fn with_fec(mut self, k_size: u8) -> Self {
        self.fec_enabled = true;
        self.fec_k = k_size.max(2).min(16);
        self
    }

    fn next_seq(&self) -> u16 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    /// Run the redirect proxy. This is the main event loop.
    ///
    /// Spawns two tasks:
    /// - **Outbound**: Game → Local Socket → Tunnel → Proxy → Game Server
    /// - **Inbound**: Game Server → Proxy → Tunnel → Local Socket → Game
    pub async fn run(&self) -> anyhow::Result<()> {
        // Bind the local socket where the game will send traffic
        let local_addr = SocketAddrV4::new(Ipv4Addr::LOCALHOST, self.local_port);
        let local_socket = Arc::new(UdpSocket::bind(local_addr).await?);
        info!("🎮 Local game socket bound to {}", local_addr);

        // Bind the tunnel socket for proxy communication
        let tunnel_addr = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);
        let tunnel_socket = Arc::new(UdpSocket::bind(tunnel_addr).await?);
        info!(
            "🌐 Tunnel socket bound to {}",
            tunnel_socket.local_addr()?
        );

        let game_server = self.game_server;
        let proxy_addr = self.proxy_addr;
        let stats = Arc::clone(&self.stats);
        let fec_enabled = self.fec_enabled;
        let fec_k = self.fec_k;

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
            let tunnel_socket = Arc::clone(&tunnel_socket);
            let game_client_addr = Arc::clone(&game_client_addr);
            let stats = Arc::clone(&stats);

            let seq_counter = Arc::new(AtomicU16::new(0));
            let seq = Arc::clone(&seq_counter);

            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];

                // FEC encoder (only if enabled)
                let mut fec_encoder = if fec_enabled {
                    Some(FecEncoder::new(fec_k))
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
                        // ── FEC mode: encode with FEC header ────────────
                        let block_id = encoder.block_id();
                        let index = encoder.current_index();

                        // Build FEC data packet:
                        // [TunnelHeader v2 20B][FecHeader 4B][game_payload]
                        let header = TunnelHeader::new_fec(seq_num, now_us(), orig_src, game_server);
                        let fec_hdr = FecHeader::data(block_id, index, fec_k);

                        let mut pkt_buf = BytesMut::with_capacity(
                            HEADER_SIZE + FEC_HEADER_SIZE + payload.len(),
                        );
                        // Encode tunnel header inline
                        pkt_buf.extend_from_slice(&header.encode());
                        fec_hdr.encode(&mut pkt_buf);
                        pkt_buf.extend_from_slice(payload);

                        // Feed payload into FEC encoder (XOR accumulation)
                        let parity = encoder.add_packet(payload);

                        // Send the data packet
                        match tunnel_socket.send_to(&pkt_buf, proxy_addr).await {
                            Ok(sent) => {
                                stats.packets_to_proxy.fetch_add(1, Ordering::Relaxed);
                                stats.bytes_to_proxy.fetch_add(sent as u64, Ordering::Relaxed);
                                trace!(seq = seq_num, fec_block = block_id, fec_idx = index, "Game → Proxy (FEC data)");
                            }
                            Err(e) => {
                                warn!("Tunnel send error: {}", e);
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }

                        // If block is complete, send parity packet
                        if let Some(parity_bytes) = parity {
                            let parity_seq = seq.fetch_add(1, Ordering::Relaxed);
                            // block_id was incremented by add_packet, so use block_id (captured before)
                            let parity_header = TunnelHeader::new_fec(
                                parity_seq, now_us(), orig_src, game_server,
                            );
                            let parity_fec = FecHeader::parity(block_id, fec_k);

                            let mut parity_buf = BytesMut::with_capacity(
                                HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len(),
                            );
                            parity_buf.extend_from_slice(&parity_header.encode());
                            parity_fec.encode(&mut parity_buf);
                            parity_buf.extend_from_slice(&parity_bytes);

                            match tunnel_socket.send_to(&parity_buf, proxy_addr).await {
                                Ok(sent) => {
                                    stats.packets_to_proxy.fetch_add(1, Ordering::Relaxed);
                                    stats.bytes_to_proxy.fetch_add(sent as u64, Ordering::Relaxed);
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
                        let header = TunnelHeader::new(seq_num, now_us(), orig_src, game_server);
                        let packet = header.encode_with_payload(payload);

                        match tunnel_socket.send_to(&packet, proxy_addr).await {
                            Ok(sent) => {
                                stats.packets_to_proxy.fetch_add(1, Ordering::Relaxed);
                                stats.bytes_to_proxy.fetch_add(sent as u64, Ordering::Relaxed);
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
            let tunnel_socket = Arc::clone(&tunnel_socket);
            let game_client_addr = Arc::clone(&game_client_addr);
            let stats = Arc::clone(&stats);

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
                    let (len, _from_addr) = match tunnel_socket.recv_from(&mut buf).await {
                        Ok(r) => r,
                        Err(e) => {
                            warn!("Tunnel socket recv error: {}", e);
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }
                    };

                    stats.packets_from_proxy.fetch_add(1, Ordering::Relaxed);
                    stats.bytes_from_proxy.fetch_add(len as u64, Ordering::Relaxed);

                    // Decode the LightSpeed header
                    let (header, payload) =
                        match TunnelHeader::decode_with_payload(&buf[..len]) {
                            Ok(r) => r,
                            Err(e) => {
                                debug!("Invalid tunnel response: {}", e);
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                        };

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

                    if header.has_fec() && fec_decoder.is_some() {
                        // ── FEC mode: decode FEC header from payload ────
                        if payload.len() < FEC_HEADER_SIZE {
                            debug!("FEC packet too short");
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                            continue;
                        }

                        let mut fec_slice: &[u8] = &payload[..FEC_HEADER_SIZE];
                        let fec_hdr = match FecHeader::decode(&mut fec_slice) {
                            Some(h) => h,
                            None => {
                                debug!("Invalid FEC header in response");
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                        };

                        let game_data = &payload[FEC_HEADER_SIZE..];
                        let decoder = fec_decoder.as_mut().unwrap();

                        if fec_hdr.is_parity() {
                            // Parity packet — try to recover a lost data packet
                            let parity_data = bytes::Bytes::copy_from_slice(game_data);
                            if let Some((_idx, recovered)) =
                                decoder.receive_parity(&fec_hdr, parity_data)
                            {
                                // Recovered a lost packet! Forward to game
                                stats.fec_recovered.fetch_add(1, Ordering::Relaxed);
                                info!(
                                    block = fec_hdr.block_id,
                                    recovered_len = recovered.len(),
                                    "🔧 FEC recovered lost packet!"
                                );
                                match local_socket.send_to(&recovered, game_addr).await {
                                    Ok(_) => {
                                        stats.packets_to_game.fetch_add(1, Ordering::Relaxed);
                                    }
                                    Err(e) => {
                                        warn!("Local socket send error: {}", e);
                                        stats.errors.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        } else {
                            // Data packet — forward to game and track for FEC
                            let data_bytes = bytes::Bytes::copy_from_slice(game_data);
                            decoder.receive_data(&fec_hdr, data_bytes);

                            match local_socket.send_to(game_data, game_addr).await {
                                Ok(_) => {
                                    stats.packets_to_game.fetch_add(1, Ordering::Relaxed);
                                    trace!(payload_len = game_data.len(), "Proxy → Game (FEC data)");
                                }
                                Err(e) => {
                                    warn!("Local socket send error: {}", e);
                                    stats.errors.fetch_add(1, Ordering::Relaxed);
                                }
                            }
                        }

                        // Periodic GC of expired FEC blocks
                        gc_counter += 1;
                        if gc_counter % 100 == 0 {
                            decoder.gc();
                        }
                    } else {
                        // ── Non-FEC mode: forward payload directly ──────
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
            let tunnel_socket = Arc::clone(&tunnel_socket);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                let mut seq: u16 = 60000; // Start high to not conflict with data seqs
                loop {
                    interval.tick().await;
                    let header = TunnelHeader::keepalive(seq, now_us());
                    let packet = header.encode();
                    let _ = tunnel_socket.send_to(&packet, proxy_addr).await;
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

                    if from_game > 0 || from_proxy > 0 {
                        if fec_enabled {
                            let parity = stats.fec_parity_sent.load(Ordering::Relaxed);
                            let recovered = stats.fec_recovered.load(Ordering::Relaxed);
                            info!(
                                "📊 Game→Proxy: {} pkts ({} bytes) | Proxy→Game: {} pkts ({} bytes) | FEC: {} parity sent, {} recovered | Errors: {}",
                                to_proxy, bytes_out, to_game, bytes_in, parity, recovered, errors
                            );
                        } else {
                            info!(
                                "📊 Game→Proxy: {} pkts ({} bytes) | Proxy→Game: {} pkts ({} bytes) | Errors: {}",
                                to_proxy, bytes_out, to_game, bytes_in, errors
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

        // Wait for shutdown
        tokio::signal::ctrl_c().await?;
        info!("⚡ Shutdown signal received");

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
        info!(
            "   Errors: {}",
            stats.errors.load(Ordering::Relaxed)
        );

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_redirect_creates_sockets() {
        let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let proxy = SocketAddrV4::new(Ipv4Addr::new(149, 28, 84, 139), 4434);
        let redirect = UdpRedirect::new(0, game_server, proxy);

        assert_eq!(redirect.game_server, game_server);
        assert_eq!(redirect.proxy_addr, proxy);
        assert!(!redirect.fec_enabled);
    }

    #[tokio::test]
    async fn test_redirect_with_fec() {
        let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let proxy = SocketAddrV4::new(Ipv4Addr::new(149, 28, 84, 139), 4434);
        let redirect = UdpRedirect::new(0, game_server, proxy).with_fec(4);

        assert!(redirect.fec_enabled);
        assert_eq!(redirect.fec_k, 4);
    }

    #[tokio::test]
    async fn test_redirect_sequence_increments() {
        let game_server = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let proxy = SocketAddrV4::new(Ipv4Addr::new(149, 28, 84, 139), 4434);
        let redirect = UdpRedirect::new(0, game_server, proxy);

        assert_eq!(redirect.next_seq(), 0);
        assert_eq!(redirect.next_seq(), 1);
        assert_eq!(redirect.next_seq(), 2);
    }
}
