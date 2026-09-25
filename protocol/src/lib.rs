//! # LightSpeed Protocol
//!
//! Shared tunnel protocol definitions used by both the client and proxy crates.
//! Contains the LightSpeed header format, encode/decode logic, protocol constants,
//! and control-plane message definitions.

pub mod control;
pub mod fec;
pub mod framing;
pub mod header;
pub mod telemetry;

pub use header::{
    flags, max_game_payload, max_tunnel_datagram, DecodeError, TunnelHeader, CONSERVATIVE_PATH_MTU,
    FEC_PARITY_TRAILER_SIZE, HEADER_SIZE, MAX_PAYLOAD_SIZE, OUTER_IPV4_HEADER_SIZE,
    OUTER_UDP_HEADER_SIZE, PROTOCOL_VERSION, PROTOCOL_VERSION_FEC,
};

pub use fec::{
    build_fec_data_packet, build_fec_parity_packet, decode_fec_payload, FecDecoder, FecEncoder,
    FecHeader, FecStats, DEFAULT_BLOCK_SIZE, FEC_HEADER_SIZE, FEC_MAX_PAYLOAD, MAX_BLOCK_SIZE,
};

pub use control::{disconnect_reason, game_id, ControlDecodeError, ControlMessage};
pub use telemetry::TelemetryReport;
