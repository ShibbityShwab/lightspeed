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
//! ## Usage
//!
//! ```
//! lightspeed --game cs2 --proxy 149.28.84.139:4434 --game-server 192.168.1.1:27015
//! ```
//!
//! Then configure the game to connect to `127.0.0.1:27015` (same port).

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{debug, info, trace, warn};

use lightspeed_protocol::TunnelHeader;

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
            stats: Arc::new(RedirectStats::new()),
        }
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

        // Track the game client's address (set on first packet received)
        let game_client_addr: Arc<RwLock<Option<std::net::SocketAddr>>> =
            Arc::new(RwLock::new(None));

        info!(
            "⚡ Redirect active: Game → 127.0.0.1:{} → Proxy {} → Server {}",
            self.local_port, proxy_addr, game_server
        );

        // ── Outbound task: Game → Proxy ─────────────────────────────
        let outbound_handle = {
            let local_socket = Arc::clone(&local_socket);
            let tunnel_socket = Arc::clone(&tunnel_socket);
            let game_client_addr = Arc::clone(&game_client_addr);
            let stats = Arc::clone(&stats);
            let _sequence = &self.sequence;

            // We need to move the atomicu16 ref — use a shared counter
            let seq_counter = Arc::new(AtomicU16::new(0));

            let seq = Arc::clone(&seq_counter);
            tokio::spawn(async move {
                let mut buf = vec![0u8; 2048];

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

                    // Remember the game client's address (for sending responses back)
                    {
                        let mut addr = game_client_addr.write().await;
                        if addr.is_none() {
                            info!("🎮 Game client connected from {}", from_addr);
                        }
                        *addr = Some(from_addr);
                    }

                    stats.packets_from_game.fetch_add(1, Ordering::Relaxed);

                    // Build the LightSpeed tunnel header
                    let seq_num = seq.fetch_add(1, Ordering::Relaxed);

                    // Construct the original source (game client) and destination (game server)
                    let orig_src = match from_addr {
                        std::net::SocketAddr::V4(v4) => v4,
                        _ => SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0),
                    };

                    let header =
                        TunnelHeader::new(seq_num, now_us(), orig_src, game_server);

                    let packet = header.encode_with_payload(payload);

                    // Send to proxy
                    match tunnel_socket.send_to(&packet, proxy_addr).await {
                        Ok(sent) => {
                            stats.packets_to_proxy.fetch_add(1, Ordering::Relaxed);
                            stats
                                .bytes_to_proxy
                                .fetch_add(sent as u64, Ordering::Relaxed);
                            trace!(
                                seq = seq_num,
                                payload_len = len,
                                "Game → Proxy"
                            );
                        }
                        Err(e) => {
                            warn!("Tunnel send error: {}", e);
                            stats.errors.fetch_add(1, Ordering::Relaxed);
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

                    // Decode the LightSpeed header
                    let (_header, payload) =
                        match TunnelHeader::decode_with_payload(&buf[..len]) {
                            Ok(r) => r,
                            Err(e) => {
                                debug!("Invalid tunnel response: {}", e);
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                                continue;
                            }
                        };

                    stats
                        .bytes_from_proxy
                        .fetch_add(len as u64, Ordering::Relaxed);

                    // Forward the raw game data back to the game client
                    let game_addr = {
                        let addr = game_client_addr.read().await;
                        *addr
                    };

                    if let Some(game_addr) = game_addr {
                        match local_socket.send_to(payload, game_addr).await {
                            Ok(_) => {
                                stats.packets_to_game.fetch_add(1, Ordering::Relaxed);
                                trace!(
                                    payload_len = payload.len(),
                                    "Proxy → Game"
                                );
                            }
                            Err(e) => {
                                warn!("Local socket send error: {}", e);
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    } else {
                        debug!("Response received but no game client connected yet");
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
                        info!(
                            "📊 Game→Proxy: {} pkts ({} bytes) | Proxy→Game: {} pkts ({} bytes) | Errors: {}",
                            to_proxy, bytes_out, to_game, bytes_in, errors
                        );
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
