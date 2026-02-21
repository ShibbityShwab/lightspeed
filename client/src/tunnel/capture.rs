//! # Tunnel Packet Capture
//!
//! Captures game UDP packets from the network interface using pcap.
//! This module works with the platform-specific capture backends in `crate::capture`.

use std::net::SocketAddrV4;

use bytes::Bytes;

use crate::error::CaptureError;

/// A raw captured packet with metadata.
#[derive(Debug, Clone)]
pub struct CapturedPacket {
    /// Raw UDP payload (game data).
    pub payload: Bytes,
    /// Source address (game client).
    pub src: SocketAddrV4,
    /// Destination address (game server).
    pub dst: SocketAddrV4,
    /// Capture timestamp in microseconds since epoch.
    pub timestamp_us: u64,
}

/// Filter for selecting which packets to capture.
#[derive(Debug, Clone)]
pub struct CaptureFilter {
    /// Game server IP addresses to intercept.
    pub server_ips: Vec<std::net::Ipv4Addr>,
    /// Game server port range.
    pub port_range: (u16, u16),
    /// BPF filter string (generated from above).
    pub bpf: String,
}

impl CaptureFilter {
    /// Create a new capture filter for the given game servers and ports.
    pub fn new(server_ips: Vec<std::net::Ipv4Addr>, port_range: (u16, u16)) -> Self {
        // Build BPF filter string for pcap
        let bpf = if server_ips.is_empty() {
            format!("udp portrange {}-{}", port_range.0, port_range.1)
        } else {
            let ip_filter: Vec<String> = server_ips
                .iter()
                .map(|ip| format!("host {}", ip))
                .collect();
            format!(
                "udp portrange {}-{} and ({})",
                port_range.0,
                port_range.1,
                ip_filter.join(" or ")
            )
        };

        Self {
            server_ips,
            port_range,
            bpf,
        }
    }
}

/// Trait for packet capture — implemented per platform in `crate::capture`.
pub trait PacketCapture: Send + Sync {
    /// Start capturing packets with the given filter.
    fn start(&mut self, filter: &CaptureFilter) -> Result<(), CaptureError>;

    /// Stop capturing.
    fn stop(&mut self) -> Result<(), CaptureError>;

    /// Get the next captured packet (blocking).
    fn next_packet(&mut self) -> Result<CapturedPacket, CaptureError>;

    /// Check if capture is active.
    fn is_active(&self) -> bool;
}
