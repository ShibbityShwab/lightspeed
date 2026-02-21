//! # LightSpeed Protocol
//!
//! Shared tunnel protocol definitions used by both the client and proxy crates.
//! Contains the LightSpeed header format, encode/decode logic, protocol constants,
//! and control-plane message definitions.

pub mod header;
pub mod control;

pub use header::{
    flags, DecodeError, TunnelHeader, HEADER_SIZE, MAX_PAYLOAD_SIZE, PROTOCOL_VERSION,
};

pub use control::{
    game_id, disconnect_reason, ControlDecodeError, ControlMessage,
};
