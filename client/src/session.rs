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

use std::sync::atomic::{AtomicU8, Ordering};

/// The current data-plane session token (0 = unregistered).
static SESSION_TOKEN: AtomicU8 = AtomicU8::new(0);

/// Set the session token after a successful QUIC registration.
pub fn set_session_token(token: u8) {
    SESSION_TOKEN.store(token, Ordering::Relaxed);
}

/// Get the current session token (0 when unregistered).
pub fn session_token() -> u8 {
    SESSION_TOKEN.load(Ordering::Relaxed)
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
}
