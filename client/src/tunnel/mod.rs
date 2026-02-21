//! # Tunnel Engine
//!
//! Orchestrates the UDP tunnel: captures outbound game packets, wraps them
//! in the LightSpeed header, sends to proxy, receives responses, unwraps,
//! and delivers back to the game.

pub mod header;
pub mod capture;
pub mod relay;

use std::net::SocketAddrV4;

use bytes::Bytes;


/// A wrapped tunnel packet ready for sending to the proxy.
#[derive(Debug, Clone)]
pub struct TunnelPacket {
    /// The encoded header + payload.
    pub data: Bytes,
    /// Which proxy to send this to.
    pub proxy_addr: SocketAddrV4,
}

/// Tunnel engine state — tracks the lifecycle of a tunnel connection.
pub struct TunnelEngineState {
    /// Whether the tunnel is active.
    pub active: bool,
    /// Target proxy address.
    pub proxy_addr: Option<SocketAddrV4>,
    /// Tunnel statistics.
    pub stats: TunnelStats,
}

impl TunnelEngineState {
    /// Create a new inactive tunnel engine state.
    pub fn new() -> Self {
        Self {
            active: false,
            proxy_addr: None,
            stats: TunnelStats::default(),
        }
    }

    /// Check if the tunnel is currently active.
    pub fn is_active(&self) -> bool {
        self.active
    }
}

/// Tunnel performance statistics.
#[derive(Debug, Clone, Default)]
pub struct TunnelStats {
    /// Total packets sent through tunnel.
    pub packets_sent: u64,
    /// Total packets received from tunnel.
    pub packets_received: u64,
    /// Total bytes sent.
    pub bytes_sent: u64,
    /// Total bytes received.
    pub bytes_received: u64,
    /// Current sequence number.
    pub current_sequence: u16,
    /// Estimated round-trip time in microseconds.
    pub rtt_us: u64,
    /// Packet loss percentage (0.0 - 100.0).
    pub packet_loss_pct: f64,
}
