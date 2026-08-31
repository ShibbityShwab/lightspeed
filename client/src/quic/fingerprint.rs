//! Per-address proxy certificate fingerprint store.
//!
//! Shared by the TOFU verifier (trust-on-first-use) and the registry pre-pin
//! path (trust-from-start). Kept free of TLS/rustls dependencies so it is
//! usable regardless of the `quic` feature.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;

pub(crate) fn store_path() -> PathBuf {
    if let Some(home) = std::env::var_os("HOME") {
        return PathBuf::from(home).join(".lightspeed-proxy-fingerprints");
    }
    PathBuf::from(".lightspeed-proxy-fingerprints")
}

pub fn load_fingerprints() -> HashMap<String, String> {
    std::fs::read_to_string(store_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub fn save_fingerprint(addr: &str, fp: &str) {
    let mut map = load_fingerprints();
    map.insert(addr.to_string(), fp.to_string());
    if let Ok(json) = serde_json::to_string_pretty(&map) {
        let _ = std::fs::write(store_path(), json);
    }
}

/// Pre-pin a proxy's expected certificate fingerprint before first connect,
/// so the certificate is verified against this value rather than accepted on
/// first use. Used for registry-discovered relays.
pub fn pre_pin(addr: SocketAddr, fingerprint: &str) {
    save_fingerprint(&addr.to_string(), fingerprint);
}

/// The previously pinned fingerprint for an address, if any.
pub fn expected_fingerprint(addr: &str) -> Option<String> {
    load_fingerprints().get(addr).cloned()
}
