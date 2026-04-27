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
//! ## Platform Notes
//!
//! - **Windows**: Uses raw packet injection via `pcap` to accurately spoof the
//!   source IP address. This ensures the game client accepts the packet as if
//!   it came directly from the game server.
//! - **Linux/macOS**: Fallback to standard UDP socket with `SO_REUSEADDR`.

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use tokio::net::UdpSocket;

#[cfg(feature = "pcap-capture")]
use pcap::{Active, Capture, Device};
#[cfg(feature = "pcap-capture")]
use std::sync::Mutex;

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
pub struct PacketInjector {
    /// UDP socket fallback for standard responses.
    socket: Arc<UdpSocket>,
    /// Pcap capture handle for raw packet injection (Windows primarily).
    #[cfg(feature = "pcap-capture")]
    pcap_handle: Option<Arc<Mutex<Capture<Active>>>>,
    /// Stats tracking.
    pub stats: Arc<InjectorStats>,
}

impl PacketInjector {
    /// Create a new packet injector.
    pub async fn new() -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        Ok(Self {
            socket: Arc::new(socket),
            #[cfg(feature = "pcap-capture")]
            pcap_handle: None,
            stats: Arc::new(InjectorStats::new()),
        })
    }

    /// Create a packet injector with a specific pcap interface for raw injection.
    #[cfg(feature = "pcap-capture")]
    pub async fn with_interface(interface_name: &str) -> std::io::Result<Self> {
        let socket = UdpSocket::bind("0.0.0.0:0").await?;

        let devices = Device::list()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        let device = devices
            .into_iter()
            .find(|d| d.name == interface_name)
            .ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Interface {} not found", interface_name),
                )
            })?;

        let cap = Capture::from_device(device)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?
            .promisc(false)
            .snaplen(65535)
            .immediate_mode(true)
            .open()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

        tracing::info!("Pcap injector bound to interface: {}", interface_name);

        Ok(Self {
            socket: Arc::new(socket),
            pcap_handle: Some(Arc::new(Mutex::new(cap))),
            stats: Arc::new(InjectorStats::new()),
        })
    }

    /// Inject a response packet to the game client.
    ///
    /// If pcap is enabled and configured, it will construct a raw Ethernet frame
    /// to accurately spoof the source IP and port (the game server's address).
    /// Otherwise, it falls back to a standard UDP socket.
    #[allow(unused_variables)]
    pub async fn inject(
        &self,
        payload: &[u8],
        game_client: SocketAddrV4,
        server_addr: SocketAddrV4,
        mac_src: [u8; 6],
        mac_dst: [u8; 6],
    ) -> Result<usize, std::io::Error> {
        #[cfg(feature = "pcap-capture")]
        if let Some(ref handle) = self.pcap_handle {
            #[rustfmt::skip]
            let raw_packet = Self::build_raw_packet(payload, game_client, server_addr, mac_src, mac_dst);
            let mut cap = handle.lock().unwrap();
            cap.sendpacket(raw_packet)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))?;

            self.stats.packets_injected.fetch_add(1, Ordering::Relaxed);
            self.stats.bytes_injected.fetch_add(payload.len() as u64, Ordering::Relaxed);
            return Ok(payload.len());
        }

        // Fallback to standard UDP socket
        let sent = self.socket.send_to(payload, game_client).await?;
        self.stats.packets_injected.fetch_add(1, Ordering::Relaxed);
        self.stats.bytes_injected.fetch_add(sent as u64, Ordering::Relaxed);
        Ok(sent)
    }

    /// Construct a raw Ethernet + IPv4 + UDP packet.
    #[cfg(feature = "pcap-capture")]
    fn build_raw_packet(
        payload: &[u8],
        dst_addr: SocketAddrV4,
        src_addr: SocketAddrV4,
        mac_src: [u8; 6],
        mac_dst: [u8; 6],
    ) -> Vec<u8> {
        let mut packet = Vec::with_capacity(14 + 20 + 8 + payload.len());

        // ── Ethernet Header (14 bytes) ────────────────
        packet.extend_from_slice(&mac_dst);
        packet.extend_from_slice(&mac_src);
        packet.extend_from_slice(&[0x08, 0x00]); // EtherType: IPv4

        // ── IPv4 Header (20 bytes) ────────────────────
        let ip_len = (20 + 8 + payload.len()) as u16;
        let mut ipv4 = vec![
            0x45, 0x00, // Version, IHL, TOS
            (ip_len >> 8) as u8, (ip_len & 0xFF) as u8, // Total Length
            0x00, 0x00, // Identification
            0x00, 0x00, // Flags, Fragment Offset
            64, 17, // TTL, Protocol (UDP)
            0x00, 0x00, // Checksum (placeholder)
        ];
        ipv4.extend_from_slice(&src_addr.ip().octets());
        ipv4.extend_from_slice(&dst_addr.ip().octets());

        // Calculate IPv4 checksum
        let mut sum = 0u32;
        for i in (0..20).step_by(2) {
            sum += u16::from_be_bytes([ipv4[i], ipv4[i + 1]]) as u32;
        }
        while sum >> 16 != 0 {
            sum = (sum & 0xFFFF) + (sum >> 16);
        }
        let checksum = !(sum as u16);
        ipv4[10] = (checksum >> 8) as u8;
        ipv4[11] = (checksum & 0xFF) as u8;
        packet.extend_from_slice(&ipv4);

        // ── UDP Header (8 bytes) ──────────────────────
        let udp_len = (8 + payload.len()) as u16;
        let udp = vec![
            (src_addr.port() >> 8) as u8, (src_addr.port() & 0xFF) as u8,
            (dst_addr.port() >> 8) as u8, (dst_addr.port() & 0xFF) as u8,
            (udp_len >> 8) as u8, (udp_len & 0xFF) as u8,
            0x00, 0x00, // Checksum (optional in IPv4, leaving 0)
        ];
        packet.extend_from_slice(&udp);

        // ── Payload ───────────────────────────────────
        packet.extend_from_slice(payload);

        packet
    }

    /// Get a clone of the socket Arc (for use in spawned tasks).
    pub fn socket(&self) -> Arc<UdpSocket> {
        Arc::clone(&self.socket)
    }
}

// SAFETY: Our use of the Mutex makes the Send/Sync requirement safe for the pcap handle.
#[cfg(feature = "pcap-capture")]
unsafe impl Send for PacketInjector {}
#[cfg(feature = "pcap-capture")]
unsafe impl Sync for PacketInjector {}
