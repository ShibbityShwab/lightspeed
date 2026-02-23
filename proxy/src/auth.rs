//! # Client Authentication
//!
//! Token-based authentication for tunnel clients.
//! Auth happens on the QUIC control plane, not per-packet on the data plane.
//!
//! ## Strategy
//! 1. Client connects via QUIC and sends Register
//! 2. Proxy assigns a random session token (u8) and returns it in RegisterAck
//! 3. Client includes the token in every tunnel header's `session_token` field
//! 4. Proxy validates (IP + token) per-packet (fast HashMap lookup)
//!
//! ## Security Properties
//! - **IP binding**: Client IP is recorded at registration time
//! - **Token binding**: Random 8-bit token prevents trivial IP spoofing
//! - **Defense in depth**: Combined with rate limiting and abuse detection
//!
//! ## Limitations (MVP)
//! - 8-bit token space (256 values) — sufficient alongside IP check
//! - NAT: Multiple clients behind same NAT share an IP (documented risk)
//! - No per-packet crypto (game packets are latency-sensitive)

use std::collections::HashMap;
use std::net::Ipv4Addr;

/// Token-based authenticator for the data plane.
///
/// Thread-safe when wrapped in `Arc<RwLock<Authenticator>>`.
/// The read path (`validate`) is the hot path — called per-packet.
/// The write path (`authorize`/`revoke`) is cold — only on QUIC events.
pub struct Authenticator {
    /// Map of authorized client IPs to their assigned session tokens.
    tokens: HashMap<Ipv4Addr, u8>,
    /// Whether authentication is enforced.
    /// When false, all packets are allowed (backward-compatible dev mode).
    require_auth: bool,
}

impl Authenticator {
    /// Create a new authenticator.
    pub fn new(require_auth: bool) -> Self {
        Self {
            tokens: HashMap::new(),
            require_auth,
        }
    }

    /// Authorize a client with a specific session token.
    /// Called after successful QUIC registration.
    pub fn authorize(&mut self, ip: Ipv4Addr, token: u8) {
        tracing::info!(ip = %ip, token = token, "Authorized client for data plane");
        self.tokens.insert(ip, token);
    }

    /// Revoke a client's authorization.
    /// Called on QUIC disconnect or session timeout.
    pub fn revoke(&mut self, ip: &Ipv4Addr) {
        if self.tokens.remove(ip).is_some() {
            tracing::info!(ip = %ip, "Revoked client data plane authorization");
        }
    }

    /// Validate a packet's (IP, token) pair.
    /// This is the **hot path** — called for every data plane packet.
    ///
    /// Returns `true` if:
    /// - Auth is disabled (`require_auth = false`), OR
    /// - The client IP is authorized AND the token matches
    #[inline]
    pub fn validate(&self, ip: &Ipv4Addr, token: u8) -> bool {
        if !self.require_auth {
            return true;
        }
        self.tokens
            .get(ip)
            .map_or(false, |&expected| expected == token)
    }

    /// Check if a client IP is authorized (ignoring token).
    #[inline]
    pub fn is_authorized(&self, ip: &Ipv4Addr) -> bool {
        if !self.require_auth {
            return true;
        }
        self.tokens.contains_key(ip)
    }

    /// Generate a random session token for a new client.
    pub fn generate_token() -> u8 {
        rand::random::<u8>()
    }

    /// Number of authorized clients.
    pub fn client_count(&self) -> usize {
        self.tokens.len()
    }

    /// Whether auth enforcement is enabled.
    pub fn is_enforced(&self) -> bool {
        self.require_auth
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authorize_and_validate() {
        let mut auth = Authenticator::new(true);
        let ip = Ipv4Addr::new(192, 168, 1, 100);
        let token = 42u8;

        assert!(!auth.validate(&ip, token));
        auth.authorize(ip, token);
        assert!(auth.validate(&ip, token));
        assert!(!auth.validate(&ip, 99)); // Wrong token
        assert_eq!(auth.client_count(), 1);
    }

    #[test]
    fn test_revoke() {
        let mut auth = Authenticator::new(true);
        let ip = Ipv4Addr::new(10, 0, 0, 1);
        auth.authorize(ip, 55);
        assert!(auth.validate(&ip, 55));
        auth.revoke(&ip);
        assert!(!auth.validate(&ip, 55));
        assert_eq!(auth.client_count(), 0);
    }

    #[test]
    fn test_auth_disabled() {
        let auth = Authenticator::new(false);
        let ip = Ipv4Addr::new(1, 2, 3, 4);
        // Any IP/token combo should pass when auth is disabled
        assert!(auth.validate(&ip, 0));
        assert!(auth.validate(&ip, 255));
    }

    #[test]
    fn test_multiple_clients() {
        let mut auth = Authenticator::new(true);
        let ip1 = Ipv4Addr::new(10, 0, 0, 1);
        let ip2 = Ipv4Addr::new(10, 0, 0, 2);
        auth.authorize(ip1, 10);
        auth.authorize(ip2, 20);
        assert!(auth.validate(&ip1, 10));
        assert!(auth.validate(&ip2, 20));
        assert!(!auth.validate(&ip1, 20)); // ip1 with ip2's token
        assert_eq!(auth.client_count(), 2);
    }

    #[test]
    fn test_generate_token() {
        // Just verify it doesn't panic and produces values
        let t1 = Authenticator::generate_token();
        let t2 = Authenticator::generate_token();
        // Extremely unlikely to be equal, but not impossible
        let _ = (t1, t2);
    }
}
