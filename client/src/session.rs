//! # Data-Plane Session Token
//!
//! Holds the session token assigned by the proxy during QUIC registration.
//! The token is stamped into every outbound data-plane packet header so the
//! proxy can authenticate the client when `require_auth` is enabled.
//!
//! A process-global atomic is used because a client process has exactly one
//! active proxy session: the control plane sets it once after registration and
//! every data-plane send reads it. It defaults to `0`, which the proxy accepts
//! only when `require_auth = false` (unregistered dev mode).

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::{AtomicU64, AtomicU8, Ordering};

/// The current data-plane session token (0 = unregistered).
static SESSION_TOKEN: AtomicU8 = AtomicU8::new(0);

/// The current relay destination (0 = unset). Packed as `ip << 16 | port` so a
/// single lock-free atomic can be read on the per-packet hot path. The
/// continuous re-routing loop updates this; the interceptor engine reads it.
static CURRENT_PROXY: AtomicU64 = AtomicU64::new(0);

/// Set the session token after a successful QUIC registration.
pub fn set_session_token(token: u8) {
    SESSION_TOKEN.store(token, Ordering::Relaxed);
}

/// Get the current session token (0 when unregistered).
pub fn session_token() -> u8 {
    SESSION_TOKEN.load(Ordering::Relaxed)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_session_token_set_get() {
        assert_eq!(session_token(), 0);
        set_session_token(0xAB);
        assert_eq!(session_token(), 0xAB);
        set_session_token(0);
        assert_eq!(session_token(), 0);
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
