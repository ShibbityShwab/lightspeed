//! # Client Authentication
//!
//! Lightweight authentication for tunnel clients.
//! Must not add significant latency — auth happens on the QUIC control
//! plane, not per-packet on the data plane.
//!
//! ## Strategy (MVP)
//! - Client authenticates via QUIC handshake
//! - Proxy issues a session token (random bytes)
//! - Client includes token in tunnel header reserved field
//! - Proxy validates token per-packet (fast lookup)

use std::collections::HashSet;
use std::net::SocketAddrV4;

/// Simple token-based authenticator.
pub struct Authenticator {
    /// Set of authorized client addresses (populated by QUIC handshake).
    authorized_clients: HashSet<SocketAddrV4>,
}

impl Authenticator {
    /// Create a new authenticator.
    pub fn new() -> Self {
        Self {
            authorized_clients: HashSet::new(),
        }
    }

    /// Authorize a client (called after successful QUIC handshake).
    pub fn authorize(&mut self, client: SocketAddrV4) {
        tracing::info!("Authorized client: {}", client);
        self.authorized_clients.insert(client);
    }

    /// Revoke a client's authorization.
    pub fn revoke(&mut self, client: &SocketAddrV4) {
        tracing::info!("Revoked client: {}", client);
        self.authorized_clients.remove(client);
    }

    /// Check if a client is authorized (called per-packet — must be fast).
    #[inline]
    pub fn is_authorized(&self, client: &SocketAddrV4) -> bool {
        self.authorized_clients.contains(client)
    }

    /// Number of authorized clients.
    pub fn client_count(&self) -> usize {
        self.authorized_clients.len()
    }
}
