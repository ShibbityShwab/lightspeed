//! LightSpeed client - library API for GUI integration.
//!
//! NOTE: `dead_code` is allowed because this library exposes a wide internal
//! API surface (`pub(crate)`) that is used by the binary (`main.rs`) but not
//! always through the public library path. Items may appear dead from the
//! library's perspective while being actively used by the binary crate.
#![allow(dead_code)]

pub mod engine;
pub mod games;
pub mod interceptor;

pub use engine::{EngineStatus, LightSpeedEngine};
pub use registry::{discover_relays, RelayInfo};

pub(crate) mod capture;
pub(crate) mod cli;
pub(crate) mod config;
pub(crate) mod error;
pub mod latency;
pub(crate) mod ml;
pub(crate) mod modes;
pub mod process;
pub(crate) mod quic;
pub(crate) mod redirect;
pub mod registry;
pub(crate) mod route;
pub(crate) mod session;
pub mod telemetry;
pub(crate) mod tunnel;
pub(crate) mod warp;

/// Control-plane surface exposed for integration tests only: `quic` and
/// `session` are crate-private, so `client/tests/` needs this narrow re-export
/// to drive a real registration and observe the per-relay token.
#[cfg(feature = "quic")]
#[doc(hidden)]
pub mod test_support {
    pub use crate::quic::{
        is_supervised, register_session, register_session_with_destination, report_destination,
        stop_supervisor,
    };
    pub use crate::session::{path_token, session_token};

    /// Install the process-wide rustls `CryptoProvider` for a test binary.
    ///
    /// The shipped binaries pin `ring` early (see `client-gui/src/main.rs`),
    /// but a test binary never runs `main`. In a `cargo test --workspace` run
    /// the GUI's tree is part of the unified feature set, which puts BOTH
    /// `ring` and `aws-lc-rs` in the graph; rustls then cannot auto-detect a
    /// provider and panics on the first QUIC call
    /// (`rustls/src/crypto/mod.rs`, "Could not automatically determine the
    /// process-level CryptoProvider").
    ///
    /// Tests that build a QUIC client or server must call this once before
    /// doing so. It is idempotent - `install_default` ignores a lost race - so
    /// any number of test threads and binaries may call it concurrently.
    pub fn install_crypto_provider() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    /// Set the process-global current relay, driving [`report_destination`].
    pub fn set_current_proxy(addr: std::net::SocketAddrV4) {
        crate::session::set_current_proxy(addr);
    }

    /// The region the process-wide destination estimator holds for `ip`.
    pub fn estimated_destination_region(ip: std::net::Ipv4Addr) -> Option<String> {
        crate::route::destination::ensure_global().destination_region(ip)
    }

    /// Whether one telemetry body would be accepted on the control plane.
    pub async fn control_telemetry_accepted(
        data_addr: std::net::SocketAddrV4,
        report_json: &[u8],
    ) -> bool {
        crate::quic::send_telemetry(data_addr, report_json)
            .await
            .is_ok()
    }
}
