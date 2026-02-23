//! # Packet Injector — Response Path for Capture Mode
//!
//! Delivers proxy responses back to the game client by sending UDP packets
//! that appear to come from the original game server. This completes the
//! bidirectional capture pipeline:
//!
//! ```text
//! OUTBOUND: Game → [pcap capture] → LightSpeed → Proxy → Game Server
//! INBOUND:  Game Server → Proxy → LightSpeed → [injector] → Game
//! ```
//!
//! ## How It Works
//!
//! The game client sends UDP packets to a game server IP:port. We capture
//! those outbound packets (learning the game's local address), tunnel them
//! through the proxy, and receive responses. The injector sends response
//! payloads back to the game's local address using a UDP socket bound to
//! the game server's address — so the game sees responses from the expected
//! source.
//!
//! ## Platform Notes
//!
//! - **Windows**: Uses standard UDP socket. Works because we're sending to
//!   localhost/LAN addresses. Requires admin (same as capture mode).
//! - **Linux**: Standard UDP socket with `SO_REUSEADDR`.
//! - **macOS**: Standard UDP socket with `SO_REUSEADDR`.
//!
//! The "simple" approach works because:
//! 1. The game client is on the same machine
//! 2. We know the game's source address from captured outbound packets
//! 3. We send response payload directly to that address
//! 4. The game receives it on its listening port

use std::net::SocketAddrV4;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use tokio::net::UdpSocket;

/// Statistics for the packet injector.
#[derive(Debug)]
pub struct InjectorStats {
    /// Packets successfully injected back to the game.
    pub packets_injected: AtomicU64,
    /// Bytes injected back to the game.
    pub bytes_injected: AtomicU64,
    /// Injection errors (send failures).
    pub inject_errors: AtomicU64,
    /// Packets received from proxy (total inbound).
    pub packets_from_proxy: AtomicU64,
    /// FEC packets recovered on inbound path.
    pub fec_recovered: AtomicU64,
}

impl InjectorStats {
    pub fn new() -> Self {
        Self {
            packets_injected: AtomicU64::new(0),
            bytes_injected: AtomicU64::new(0),
            inject_errors: AtomicU64::new(0),
            packets_from_proxy: AtomicU64::new(0),
            fec_recovered: AtomicU64::new(0),
        }
    }
}

/// Injects response packets back to the game client.
///
/// Uses a UDP socket to send response payloads to the game client's
/// local address, completing the bidirectional capture pipeline.
pub struct PacketInjector {
    /// UDP socket for sending responses to the game.
    socket: Arc<UdpSocket>,
    /// Stats tracking.
    pub stats: Arc<InjectorStats>,
}

impl PacketInjector {
    /// Create a new packet injector.
    ///
    /// Binds a UDP socket on an ephemeral port. The socket will be used
    /// to send response payloads to the game client.
    pub async fn new() -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(Self {
            socket: Arc::new(socket),
            stats: Arc::new(InjectorStats::new()),
        })
    }

    /// Create a packet injector that mimics a specific source address.
    ///
    /// On platforms that support it, this binds to the game server's address
    /// so the game sees responses from the expected source. Falls back to
    /// an ephemeral port if binding fails (e.g., address already in use).
    pub async fn mimicking_source(game_server: SocketAddrV4) -> std::io::Result<Self> {
        // Try to bind to the game server port (so game sees correct source port)
        // This may fail if the port is already in use, which is fine
        let socket = match UdpSocket::bind(format!("0.0.0.0:{}", game_server.port())).await {
            Ok(s) => {
                tracing::info!(
                    "Injector bound to port {} (mimicking game server source)",
                    game_server.port()
                );
                s
            }
            Err(_) => {
                tracing::debug!(
                    "Could not bind to port {} (in use), using ephemeral port",
                    game_server.port()
                );
                UdpSocket::bind("0.0.0.0:0").await?
            }
        };

        Ok(Self {
            socket: Arc::new(socket),
            stats: Arc::new(InjectorStats::new()),
        })
    }

    /// Inject a response packet to the game client.
    ///
    /// Sends the payload to the game client's address so the game
    /// receives it as a normal UDP packet.
    pub async fn inject(&self, payload: &[u8], game_client: SocketAddrV4) -> Result<usize, std::io::Error> {
        let sent = self.socket.send_to(payload, game_client).await?;
        self.stats.packets_injected.fetch_add(1, Ordering::Relaxed);
        self.stats.bytes_injected.fetch_add(sent as u64, Ordering::Relaxed);
        Ok(sent)
    }

    /// Get a clone of the socket Arc (for use in spawned tasks).
    pub fn socket(&self) -> Arc<UdpSocket> {
        Arc::clone(&self.socket)
    }
}
