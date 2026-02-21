//! # LightSpeed Protocol
//!
//! Shared tunnel protocol definitions used by both the client and proxy crates.
//! Contains the LightSpeed header format, encode/decode logic, and protocol constants.

pub mod header;

pub use header::{
    flags, DecodeError, TunnelHeader, HEADER_SIZE, MAX_PAYLOAD_SIZE, PROTOCOL_VERSION,
};
