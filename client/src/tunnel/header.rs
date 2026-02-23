//! # LightSpeed Tunnel Header
//!
//! Re-exports the shared protocol header from `lightspeed_protocol`.
//! All header types and constants are defined in the protocol crate
//! so they can be shared between client and proxy.

// Re-export everything — consumers can use `crate::tunnel::header::*`
#[allow(unused_imports)]
pub use lightspeed_protocol::*;
