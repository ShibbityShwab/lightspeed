//! # Data-Plane Session Tokens
//!
//! Holds the session token(s) assigned by the proxy during QUIC registration.
//! The token is stamped into every outbound data-plane packet header so the
//! proxy can authenticate the client when `require_auth` is enabled.
//!
//! A single-path client has one default token. A multipath client sends to
//! several relays, each of which authorizes its own token, so an explicit token
//! can be registered per relay address. Lookups are a lock-free scan over fixed
//! atomic slots, keeping the per-packet read path free of any mutex. The
//! default is `0`, which the proxy accepts only when `require_auth = false`
//! (unregistered dev mode).

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

use crate::route::multipath::MultipathState;

/// Maximum per-path token slots. Multipath uses at most three relays plus the
/// current proxy; eight leaves headroom while keeping the lock-free scan cheap.
const MAX_TOKEN_PATHS: usize = 8;

/// Process-global token store: one single-path default plus per-relay entries.
///
/// Reads are lock-free atomic loads over fixed slots (no mutex on the packet
/// path); `write_lock` only serializes writers among themselves.
struct TokenStore {
    /// The single-path default (0 = unregistered).
    default: AtomicU32,
    /// Packed relay address `ip << 16 | port`, 0 = empty. Published with
    /// `Release` after `tokens`, so an `Acquire` match also observes its token.
    paths: [AtomicU64; MAX_TOKEN_PATHS],
    /// Per-path tokens, parallel to `paths`. Zero tokens are never stored.
    tokens: [AtomicU32; MAX_TOKEN_PATHS],
    /// Serializes writers only; readers never lock.
    write_lock: Mutex<()>,
}

impl TokenStore {
    const fn new() -> Self {
        Self {
            default: AtomicU32::new(0),
            paths: [const { AtomicU64::new(0) }; MAX_TOKEN_PATHS],
            tokens: [const { AtomicU32::new(0) }; MAX_TOKEN_PATHS],
            write_lock: Mutex::new(()),
        }
    }

    /// Pack a relay address with the same encoding as `CURRENT_PROXY`.
    fn pack(addr: SocketAddrV4) -> u64 {
        (u64::from(u32::from(*addr.ip())) << 16) | u64::from(addr.port())
    }

    fn default_token(&self) -> u32 {
        self.default.load(Ordering::Relaxed)
    }

    fn set_default(&self, token: u32) {
        self.default.store(token, Ordering::Relaxed);
    }

    fn set_path(&self, addr: SocketAddrV4, token: u32) {
        if token == 0 {
            return;
        }
        let packed = Self::pack(addr);
        let _writer = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (path, slot) in self.paths.iter().zip(self.tokens.iter()) {
            if path.load(Ordering::Relaxed) == packed {
                slot.store(token, Ordering::Relaxed);
                return;
            }
        }
        for (path, slot) in self.paths.iter().zip(self.tokens.iter()) {
            if path.load(Ordering::Relaxed) == 0 {
                slot.store(token, Ordering::Relaxed);
                path.store(packed, Ordering::Release);
                return;
            }
        }
        // All slots busy (unreachable at MAX_TOKEN_PATHS = 8): evict slot 0,
        // clearing the key first so readers never pair it with the new token.
        self.paths[0].store(0, Ordering::Release);
        self.tokens[0].store(token, Ordering::Relaxed);
        self.paths[0].store(packed, Ordering::Release);
    }

    fn path(&self, addr: SocketAddrV4) -> u32 {
        let packed = Self::pack(addr);
        for (path, slot) in self.paths.iter().zip(self.tokens.iter()) {
            if path.load(Ordering::Acquire) == packed {
                return slot.load(Ordering::Relaxed);
            }
        }
        self.default_token()
    }

    fn clear_path(&self, addr: SocketAddrV4) {
        let packed = Self::pack(addr);
        let _writer = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (path, slot) in self.paths.iter().zip(self.tokens.iter()) {
            if path.load(Ordering::Relaxed) == packed {
                path.store(0, Ordering::Release);
                slot.store(0, Ordering::Relaxed);
                return;
            }
        }
    }

    fn reset_all(&self) {
        let _writer = self
            .write_lock
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for (path, slot) in self.paths.iter().zip(self.tokens.iter()) {
            path.store(0, Ordering::Relaxed);
            slot.store(0, Ordering::Relaxed);
        }
        self.default.store(0, Ordering::Relaxed);
    }
}

/// The process-global data-plane token store.
static TOKENS: TokenStore = TokenStore::new();

/// The current relay destination (0 = unset). Packed as `ip << 16 | port` so a
/// single lock-free atomic can be read on the per-packet hot path. The
/// continuous re-routing loop updates this; the interceptor engine reads it.
static CURRENT_PROXY: AtomicU64 = AtomicU64::new(0);

/// Global multipath state (active paths + dedup window).
static MULTIPATH: OnceLock<Mutex<MultipathState>> = OnceLock::new();

const MAX_MULTIPATH_PATHS: usize = 3;
const UNSPECIFIED: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);

fn multipath() -> &'static Mutex<MultipathState> {
    MULTIPATH.get_or_init(|| Mutex::new(MultipathState::new(1024)))
}

/// Set the single-path session token after a successful QUIC registration.
pub fn set_session_token(token: u32) {
    TOKENS.set_default(token);
}

/// Get the single-path session token (0 when unregistered).
pub fn session_token() -> u32 {
    TOKENS.default_token()
}

/// Register the token a specific relay authorized for this client. A zero
/// token is ignored so an unregistered update cannot clobber a valid token.
pub fn set_path_token(path: SocketAddrV4, token: u32) {
    TOKENS.set_path(path, token);
}

/// Get the token for a relay: its explicit per-path token when registered,
/// otherwise the single-path default (0 when unregistered).
pub fn path_token(path: SocketAddrV4) -> u32 {
    TOKENS.path(path)
}

/// Drop the explicit token for a relay so lookups fall back to the default.
pub fn clear_path_token(path: SocketAddrV4) {
    TOKENS.clear_path(path);
}

/// Clear every token, per-path and default. Shutdown only.
pub fn reset_all_tokens() {
    TOKENS.reset_all();
}

/// Set the current relay destination.
pub fn set_current_proxy(addr: SocketAddrV4) {
    let packed = (u64::from(u32::from(*addr.ip())) << 16) | u64::from(addr.port());
    CURRENT_PROXY.store(packed, Ordering::Relaxed);
}

/// Get the current relay destination (None when unset).
pub fn current_proxy() -> Option<SocketAddrV4> {
    let packed = CURRENT_PROXY.load(Ordering::Relaxed);
    if packed == 0 {
        return None;
    }
    let ip = Ipv4Addr::from((packed >> 16) as u32);
    let port = (packed & 0xFFFF) as u16;
    Some(SocketAddrV4::new(ip, port))
}

/// Set the active multipath relay destinations (fewer than 2 disables it).
pub fn set_multipath_paths(addrs: Vec<SocketAddrV4>) {
    multipath().lock().unwrap().set_paths(addrs);
}

/// The active multipath relay destinations (up to 3, UNSPECIFIED = unused).
pub fn multipath_paths() -> [SocketAddrV4; MAX_MULTIPATH_PATHS] {
    let mut out = [UNSPECIFIED; MAX_MULTIPATH_PATHS];
    let m = multipath().lock().unwrap();
    for (i, p) in m.paths().iter().take(MAX_MULTIPATH_PATHS).enumerate() {
        out[i] = *p;
    }
    out
}

/// Record a received response and report whether it is a multipath duplicate
/// (drop it). On the first response, records a win for `source`; on duplicates,
/// records a loss. No-op (returns false) when multipath is inactive.
pub fn multipath_record_response(seq: u16, source: SocketAddr, latency_us: u64) -> bool {
    let source_v4 = match source {
        SocketAddr::V4(v4) => v4,
        SocketAddr::V6(_) => return false,
    };
    let mut m = multipath().lock().unwrap();
    if !m.is_active() {
        return false;
    }
    if m.is_duplicate(seq) {
        m.record_duplicate(source_v4);
        crate::telemetry::paths::record_relay_response(source_v4, latency_us, true);
        return true;
    }
    m.record_win(source_v4, latency_us);
    crate::telemetry::paths::record_relay_response(source_v4, latency_us, false);
    false
}

/// Snapshot of per-path multipath stats (wins, total, EMA latency).
pub fn multipath_stats() -> Vec<(SocketAddrV4, crate::route::multipath::PathStats)> {
    let m = multipath().lock().unwrap();
    m.stats().iter().map(|(a, s)| (*a, s.clone())).collect()
}

/// Whether a received datagram's source is one of the active relay
/// destinations (multipath paths or the current proxy). Used to reject
/// datagrams injected by off-path hosts.
pub fn is_known_proxy(source: SocketAddr, config_fallback: SocketAddrV4) -> bool {
    let source_v4 = match source {
        SocketAddr::V4(v4) => v4,
        SocketAddr::V6(_) => return false,
    };
    let (dests, n) = send_destinations(config_fallback);
    dests.iter().take(n).any(|d| *d == source_v4)
}

/// Relay destinations for an outbound packet: the multipath spread when two or
/// more paths are active, otherwise the single current relay. Returns
/// `(destinations, count)`; only the first `count` entries are valid.
pub fn send_destinations(
    config_fallback: SocketAddrV4,
) -> ([SocketAddrV4; MAX_MULTIPATH_PATHS], usize) {
    let paths = multipath_paths();
    let active = paths.iter().filter(|p| **p != UNSPECIFIED).count();
    if active >= 2 {
        (paths, active)
    } else {
        (
            [
                current_proxy().unwrap_or(config_fallback),
                UNSPECIFIED,
                UNSPECIFIED,
            ],
            1,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The test harness runs `#[test]`s in parallel and the token store is
    /// process-global, so tests that mutate it take this lock to stay
    /// deterministic.
    static TOKEN_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn token_test_guard() -> std::sync::MutexGuard<'static, ()> {
        TOKEN_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    #[test]
    fn test_session_token_set_get() {
        let _guard = token_test_guard();
        assert_eq!(session_token(), 0);
        set_session_token(0xAB);
        assert_eq!(session_token(), 0xAB);
        set_session_token(0);
        assert_eq!(session_token(), 0);
    }

    #[test]
    fn per_path_token_roundtrip() {
        let _guard = token_test_guard();
        reset_all_tokens();
        set_session_token(0xAA);
        let path_a = SocketAddrV4::new(Ipv4Addr::new(45, 32, 72, 7), 4434);
        let path_b = SocketAddrV4::new(Ipv4Addr::new(45, 32, 72, 8), 4434);

        set_path_token(path_a, 0x11);
        set_path_token(path_b, 0x22);

        // Two paths hold distinct tokens.
        assert_eq!(path_token(path_a), 0x11);
        assert_eq!(path_token(path_b), 0x22);
        // Per-path writes leave the single-path default untouched.
        assert_eq!(session_token(), 0xAA);

        clear_path_token(path_a);
        // A path with no explicit entry falls back to the single-path default.
        assert_eq!(path_token(path_a), 0xAA);
        assert_eq!(path_token(path_b), 0x22);

        reset_all_tokens();
        assert_eq!(path_token(path_a), 0);
        assert_eq!(session_token(), 0);
    }

    #[test]
    fn zero_does_not_overwrite_valid() {
        let _guard = token_test_guard();
        reset_all_tokens();
        let path = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 5), 4434);

        set_path_token(path, 0x1234);
        assert_eq!(path_token(path), 0x1234);

        // A zero token means "unregistered" and must never clobber a valid one.
        set_path_token(path, 0);
        assert_eq!(path_token(path), 0x1234);
    }

    #[test]
    fn test_current_proxy_roundtrip() {
        assert_eq!(current_proxy(), None);
        let addr = SocketAddrV4::new(Ipv4Addr::new(45, 32, 72, 7), 4434);
        set_current_proxy(addr);
        assert_eq!(current_proxy(), Some(addr));
        set_current_proxy(SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 1));
        assert_eq!(
            current_proxy(),
            Some(SocketAddrV4::new(Ipv4Addr::new(1, 2, 3, 4), 1))
        );
    }
}
