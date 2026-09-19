//! # Client Authentication
//!
//! Token-keyed authentication for tunnel clients. Authorization is granted on
//! the QUIC control plane at Register and checked per-packet on the data plane.
//!
//! ## Strategy
//! 1. Client connects via QUIC and sends Register
//! 2. Proxy assigns a random session token (u32), returned in RegisterAck
//! 3. Client includes the token in every tunnel header's `session_token` field
//! 4. Proxy validates (principal + optional bound port + token + expiry) per packet
//!
//! ## Reconnect and NAT isolation
//!
//! Entries are keyed by token, not by IP, so two clients behind the same NAT
//! hold independent authorizations and closing one does not revoke the other.
//! A same-principal re-registration demotes the previous token to a short
//! [`PREVIOUS_TOKEN_GRACE`] window, and a transport close revokes with a grace
//! window sized for a client reconnect. [`Authenticator::sweep`] drops expired
//! entries.
//!
//! ## Security Properties
//! - **Principal binding**: the entry records the IP observed at registration
//! - **Optional port binding**: a client reporting a data port is pinned to it
//! - **Expiry**: every entry has a deadline; a live control connection refreshes it
//! - **Fail closed**: a full table refuses new tokens rather than evicting
//!
//! ## Limitations
//! - 32-bit token space (4 billion values), infeasible to brute-force
//! - No per-packet crypto (game packets are latency-sensitive)

use std::collections::HashMap;
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

/// Lifetime of a freshly issued token.
pub const TOKEN_TTL: Duration = Duration::from_secs(300);

/// Grace window for the previous token after a same-principal re-registration.
pub const PREVIOUS_TOKEN_GRACE: Duration = Duration::from_secs(30);

/// Grace window applied when a registration's transport closes. Sized so a
/// client can reconnect and re-register without a data-plane outage.
pub const TRANSPORT_REVOKE_GRACE: Duration = Duration::from_secs(120);

/// Grace window applied on an explicit `Disconnect`.
pub const EXPLICIT_REVOKE_GRACE: Duration = Duration::from_secs(10);

/// Hard cap on tracked tokens. A full table refuses new tokens (fail closed),
/// mirroring the rate limiter's table discipline.
pub const MAX_AUTH_ENTRIES: usize = 65_536;

/// One authorized data-plane token.
#[derive(Debug, Clone, Copy)]
pub struct AuthEntry {
    principal: Ipv4Addr,
    bound_port: Option<u16>,
    expires_at: Instant,
}

/// Token-based authenticator for the data plane.
///
/// Thread-safe when wrapped in `Arc<RwLock<Authenticator>>`.
/// The read path (`validate`) is the hot path, called per-packet.
/// The write path (`authorize`/`revoke`) is cold, only on QUIC events.
pub struct Authenticator {
    /// Authorized tokens, keyed by the token itself.
    tokens: HashMap<u32, AuthEntry>,
    /// Whether authentication is enforced.
    /// When false, all packets are allowed (backward-compatible dev mode).
    require_auth: bool,
    /// Hard cap on tracked entries; a full table fails closed.
    max_entries: usize,
}

impl Authenticator {
    /// Create a new authenticator.
    pub fn new(require_auth: bool) -> Self {
        Self {
            tokens: HashMap::new(),
            require_auth,
            max_entries: MAX_AUTH_ENTRIES,
        }
    }

    /// Test-only: create an authenticator with a bounded token table.
    #[cfg(test)]
    fn with_max_entries(require_auth: bool, max_entries: usize) -> Self {
        Self {
            tokens: HashMap::new(),
            require_auth,
            max_entries,
        }
    }

    /// Authorize a token for `principal`.
    ///
    /// `data_port` is the client's data-plane source port, or 0 when the client
    /// does not report one (the token is then bound to the principal only).
    /// Any existing entries for the same principal are capped to a short
    /// [`PREVIOUS_TOKEN_GRACE`] window so a reconnecting client can overlap.
    /// Fails closed when the table is full and the token is new.
    pub fn authorize(&mut self, principal: Ipv4Addr, data_port: u16, token: u32, now: Instant) {
        if !self.tokens.contains_key(&token) && self.tokens.len() >= self.max_entries {
            tracing::warn!(
                principal = %principal,
                "Auth token table full, refusing to authorize new token"
            );
            return;
        }

        let previous_deadline = now + PREVIOUS_TOKEN_GRACE;
        for entry in self.tokens.values_mut() {
            if entry.principal == principal && entry.expires_at > previous_deadline {
                entry.expires_at = previous_deadline;
            }
        }

        self.tokens.insert(
            token,
            AuthEntry {
                principal,
                bound_port: (data_port != 0).then_some(data_port),
                expires_at: now + TOKEN_TTL,
            },
        );
        tracing::info!(principal = %principal, token = token, "Authorized data-plane token");
    }

    /// Revoke a token, retaining it for `grace` so an in-flight reconnect can
    /// still validate. Sweeping later removes the tombstone.
    pub fn revoke(&mut self, token: u32, now: Instant, grace: Duration) {
        if let Some(entry) = self.tokens.get_mut(&token) {
            entry.expires_at = now + grace;
            tracing::info!(
                token = token,
                grace_secs = grace.as_secs(),
                "Revoked data-plane token"
            );
        }
    }

    /// Extend a live token's expiry to a full [`TOKEN_TTL`] from `now`.
    /// Called on control-plane activity so a connected client never expires.
    pub fn refresh(&mut self, token: u32, now: Instant) {
        if let Some(entry) = self.tokens.get_mut(&token) {
            entry.expires_at = now + TOKEN_TTL;
        }
    }

    /// Remove every token for `principal` immediately (abuse ban, no grace).
    pub fn ban(&mut self, principal: Ipv4Addr) {
        let before = self.tokens.len();
        self.tokens.retain(|_, entry| entry.principal != principal);
        let removed = before - self.tokens.len();
        if removed > 0 {
            tracing::warn!(
                principal = %principal,
                removed = removed,
                "Banned principal, revoked data-plane tokens"
            );
        }
    }

    /// Drop every entry whose deadline is at or before `now`.
    pub fn sweep(&mut self, now: Instant) -> usize {
        let before = self.tokens.len();
        self.tokens.retain(|_, entry| entry.expires_at > now);
        before - self.tokens.len()
    }

    /// Validate a packet's (principal, data_port, token) triple.
    /// This is the **hot path**, called for every data plane packet.
    ///
    /// Returns `true` if:
    /// - Auth is disabled (`require_auth = false`), OR
    /// - The token is known, belongs to `principal`, matches the bound port
    ///   (when one was recorded), and has not expired.
    #[inline]
    pub fn validate(&self, principal: Ipv4Addr, data_port: u16, token: u32, now: Instant) -> bool {
        if !self.require_auth {
            return true;
        }
        self.tokens.get(&token).is_some_and(|entry| {
            entry.principal == principal
                && match entry.bound_port {
                    Some(bound) => bound == data_port,
                    None => true,
                }
                && entry.expires_at > now
        })
    }

    /// Generate a random session token for a new client.
    pub fn generate_token() -> u32 {
        rand::random::<u32>()
    }

    /// Number of tracked tokens (live and not-yet-swept).
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

    fn ip(last: u8) -> Ipv4Addr {
        Ipv4Addr::new(10, 0, 0, last)
    }

    fn t0() -> Instant {
        Instant::now()
    }

    #[test]
    fn test_authorize_and_validate() {
        let mut auth = Authenticator::new(true);
        let principal = Ipv4Addr::new(192, 168, 1, 100);
        let now = t0();

        assert!(!auth.validate(principal, 0, 42, now));
        auth.authorize(principal, 0, 42, now);
        assert!(auth.validate(principal, 0, 42, now));
        assert!(!auth.validate(principal, 0, 99, now));
        assert_eq!(auth.client_count(), 1);
    }

    #[test]
    fn test_revoke_sets_finite_grace() {
        let mut auth = Authenticator::new(true);
        let principal = ip(1);
        let now = t0();
        auth.authorize(principal, 0, 55, now);
        assert!(auth.validate(principal, 0, 55, now));

        let grace = Duration::from_secs(90);
        auth.revoke(55, now, grace);
        assert!(auth.validate(principal, 0, 55, now + Duration::from_secs(89)));
        assert!(!auth.validate(principal, 0, 55, now + Duration::from_secs(91)));
    }

    #[test]
    fn test_auth_disabled() {
        let auth = Authenticator::new(false);
        let principal = ip(4);
        let now = t0();
        assert!(auth.validate(principal, 0, 0, now));
        assert!(auth.validate(principal, 1234, 255, now));
    }

    #[test]
    fn test_multiple_clients_distinct_principals() {
        let mut auth = Authenticator::new(true);
        let now = t0();
        auth.authorize(ip(1), 0, 10, now);
        auth.authorize(ip(2), 0, 20, now);
        assert!(auth.validate(ip(1), 0, 10, now));
        assert!(auth.validate(ip(2), 0, 20, now));
        assert!(!auth.validate(ip(1), 0, 20, now));
        assert_eq!(auth.client_count(), 2);
    }

    #[test]
    fn test_generate_token() {
        let t1 = Authenticator::generate_token();
        let t2 = Authenticator::generate_token();
        let _ = (t1, t2);
    }

    #[test]
    fn two_nat_clients_same_ip_distinct_tokens_both_validate() {
        let mut auth = Authenticator::new(true);
        let now = t0();
        let shared = ip(1);

        auth.authorize(shared, 40_001, 111, now);
        auth.authorize(shared, 40_002, 222, now);

        assert!(auth.validate(shared, 40_001, 111, now));
        assert!(auth.validate(shared, 40_002, 222, now));
        assert_eq!(auth.client_count(), 2);
    }

    #[test]
    fn revoke_one_token_leaves_the_other_valid() {
        let mut auth = Authenticator::new(true);
        let now = t0();
        let shared = ip(1);
        auth.authorize(shared, 40_001, 111, now);
        auth.authorize(shared, 40_002, 222, now);

        auth.revoke(222, now, Duration::from_secs(90));

        assert!(auth.validate(shared, 40_001, 111, now + Duration::from_secs(20)));
        assert!(auth.validate(shared, 40_002, 222, now + Duration::from_secs(89)));
        assert!(!auth.validate(shared, 40_002, 222, now + Duration::from_secs(91)));
    }

    #[test]
    fn previous_token_valid_within_window_then_rejected() {
        let mut auth = Authenticator::new(true);
        let shared = ip(1);
        let t_old = t0();
        auth.authorize(shared, 0, 111, t_old);

        let t_new = t_old + Duration::from_secs(100);
        auth.authorize(shared, 0, 222, t_new);

        let within = t_new + PREVIOUS_TOKEN_GRACE - Duration::from_secs(1);
        let after = t_new + PREVIOUS_TOKEN_GRACE + Duration::from_secs(1);
        assert!(auth.validate(shared, 0, 111, within));
        assert!(!auth.validate(shared, 0, 111, after));
        assert!(auth.validate(shared, 0, 222, after));
    }

    #[test]
    fn token_from_different_principal_rejected() {
        let mut auth = Authenticator::new(true);
        let now = t0();
        auth.authorize(ip(1), 0, 111, now);
        assert!(!auth.validate(ip(2), 0, 111, now));
    }

    #[test]
    fn bound_port_mismatch_rejected() {
        let mut auth = Authenticator::new(true);
        let now = t0();
        auth.authorize(ip(1), 40_000, 111, now);

        assert!(auth.validate(ip(1), 40_000, 111, now));
        assert!(!auth.validate(ip(1), 40_001, 111, now));
        // A zero data port means "no port binding was ever established".
        assert!(!auth.validate(ip(1), 0, 111, now));
    }

    #[test]
    fn sweep_drops_expired_entries() {
        let mut auth = Authenticator::new(true);
        let now = t0();
        auth.authorize(ip(1), 0, 111, now);
        auth.authorize(ip(2), 0, 222, now);

        auth.sweep(now + TOKEN_TTL - Duration::from_secs(1));
        assert_eq!(auth.client_count(), 2);

        auth.sweep(now + TOKEN_TTL + Duration::from_secs(1));
        assert_eq!(auth.client_count(), 0);
        assert!(!auth.validate(ip(1), 0, 111, now + TOKEN_TTL + Duration::from_secs(2)));
    }

    #[test]
    fn refresh_keeps_open_connection_valid() {
        let mut auth = Authenticator::new(true);
        let principal = ip(1);
        let now = t0();
        auth.authorize(principal, 0, 111, now);

        let refreshed_at = now + Duration::from_secs(250);
        auth.refresh(111, refreshed_at);

        let past_original_ttl = now + TOKEN_TTL + Duration::from_secs(10);
        assert!(auth.validate(principal, 0, 111, past_original_ttl));
        assert!(auth.validate(
            principal,
            0,
            111,
            refreshed_at + TOKEN_TTL - Duration::from_secs(1)
        ));
        assert!(!auth.validate(
            principal,
            0,
            111,
            refreshed_at + TOKEN_TTL + Duration::from_secs(1)
        ));
    }

    #[test]
    fn ban_removes_principal_entries_immediately() {
        let mut auth = Authenticator::new(true);
        let now = t0();
        auth.authorize(ip(1), 0, 111, now);
        auth.authorize(ip(1), 0, 222, now);
        auth.authorize(ip(2), 0, 333, now);

        auth.ban(ip(1));

        assert!(!auth.validate(ip(1), 0, 111, now));
        assert!(!auth.validate(ip(1), 0, 222, now));
        assert!(auth.validate(ip(2), 0, 333, now));
        assert_eq!(auth.client_count(), 1);
    }

    #[test]
    fn authorize_fails_closed_when_table_full() {
        let mut auth = Authenticator::with_max_entries(true, 2);
        let now = t0();
        auth.authorize(ip(1), 0, 111, now);
        auth.authorize(ip(2), 0, 222, now);

        auth.authorize(ip(3), 0, 333, now);
        assert!(!auth.validate(ip(3), 0, 333, now));
        assert_eq!(auth.client_count(), 2);

        auth.authorize(ip(1), 0, 111, now + Duration::from_secs(1));
        assert_eq!(auth.client_count(), 2);
    }
}
