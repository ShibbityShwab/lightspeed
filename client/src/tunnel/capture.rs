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
    /// Source MAC address (from Ethernet frame).
    pub mac_src: [u8; 6],
    /// Destination MAC address (from Ethernet frame).
    pub mac_dst: [u8; 6],
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
        // Build BPF filter string for pcap.
        // Use `udp port X` for a single port, `udp portrange X-Y` for a range.
        let port_expr = if port_range.0 == port_range.1 {
            format!("udp port {}", port_range.0)
        } else {
            format!("udp portrange {}-{}", port_range.0, port_range.1)
        };

        let bpf = if server_ips.is_empty() {
            port_expr
        } else {
            let ip_filter: Vec<String> =
                server_ips.iter().map(|ip| format!("host {}", ip)).collect();
            format!("{} and ({})", port_expr, ip_filter.join(" or "))
        };

        Self {
            server_ips,
            port_range,
            bpf,
        }
    }

    /// Create a filter that matches any of a discrete set of ports.
    ///
    /// Used when netstat returns multiple candidate UDP ports for the game
    /// process — we OR all of them together so the right one is always captured.
    pub fn new_multi_port(server_ips: Vec<std::net::Ipv4Addr>, ports: Vec<u16>) -> Self {
        assert!(!ports.is_empty(), "ports list must not be empty");

        let port_expr = if ports.len() == 1 {
            format!("udp port {}", ports[0])
        } else {
            let parts: Vec<String> = ports.iter().map(|p| format!("port {}", p)).collect();
            format!("udp and ({})", parts.join(" or "))
        };

        let bpf = if server_ips.is_empty() {
            port_expr
        } else {
            let ip_filter: Vec<String> =
                server_ips.iter().map(|ip| format!("host {}", ip)).collect();
            format!("{} and ({})", port_expr, ip_filter.join(" or "))
        };

        // Store the first port as representative for port_range
        let p0 = ports[0];
        Self {
            server_ips,
            port_range: (p0, p0),
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

    /// Get the name of the active interface, if any.
    fn interface_name(&self) -> Option<&str>;
}
