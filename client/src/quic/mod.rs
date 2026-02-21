//! # QUIC Control Plane
//!
//! Manages the reliable control channel between client and proxy nodes
//! using QUIC (via quinn). The control plane handles:
//! - Proxy discovery and registration
//! - Health checking and latency probing
//! - Configuration synchronization
//! - Route negotiation
//!
//! The data plane (game packets) uses raw UDP for minimum latency.
//! Only control messages use QUIC for reliability.

pub mod discovery;
pub mod health;

use std::net::SocketAddr;

use crate::error::QuicError;

/// QUIC control plane client.
pub struct ControlClient {
    /// Whether the client is connected.
    connected: bool,
    /// Remote proxy address.
    remote_addr: Option<SocketAddr>,
}

impl ControlClient {
    /// Create a new control plane client.
    pub fn new() -> Self {
        Self {
            connected: false,
            remote_addr: None,
        }
    }

    /// Connect to a proxy's control plane.
    pub async fn connect(&mut self, addr: SocketAddr) -> Result<(), QuicError> {
        tracing::info!("Connecting QUIC control plane to {}", addr);

        // TODO (WF-001 Step 4): Implement QUIC connection
        // 1. Create quinn::Endpoint with client config
        // 2. Configure TLS (self-signed for MVP)
        // 3. Connect to proxy
        // 4. Perform handshake
        // 5. Start health check background task

        self.remote_addr = Some(addr);
        self.connected = true;
        Ok(())
    }

    /// Disconnect from the proxy.
    pub async fn disconnect(&mut self) -> Result<(), QuicError> {
        tracing::info!("Disconnecting QUIC control plane");
        self.connected = false;
        self.remote_addr = None;
        Ok(())
    }

    /// Check if control plane is connected.
    pub fn is_connected(&self) -> bool {
        self.connected
    }
}
