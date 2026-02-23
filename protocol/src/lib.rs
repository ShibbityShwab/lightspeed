//! # LightSpeed Protocol
//!
//! Shared tunnel protocol definitions used by both the client and proxy crates.
//! Contains the LightSpeed header format, encode/decode logic, protocol constants,
//! and control-plane message definitions.

pub mod control;
pub mod fec;
pub mod header;

pub use header::{
    flags, DecodeError, TunnelHeader, HEADER_SIZE, MAX_PAYLOAD_SIZE, PROTOCOL_VERSION,
};

pub use fec::{
    FecDecoder, FecEncoder, FecHeader, FecStats, DEFAULT_BLOCK_SIZE, FEC_HEADER_SIZE,
    FEC_MAX_PAYLOAD, MAX_BLOCK_SIZE,
};

pub use control::{disconnect_reason, game_id, ControlDecodeError, ControlMessage};
