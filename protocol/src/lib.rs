//! # LightSpeed Protocol
//!
//! Shared tunnel protocol definitions used by both the client and proxy crates.
//! Contains the LightSpeed header format, encode/decode logic, protocol constants,
//! and control-plane message definitions.

pub mod control;
pub mod fec;
pub mod framing;
pub mod header;
pub mod tcp_ext;
pub mod telemetry;

pub use header::{
    flags, max_game_payload, max_tunnel_datagram, DecodeError, TunnelHeader, CONSERVATIVE_PATH_MTU,
    FEC_PARITY_TRAILER_SIZE, HEADER_SIZE, MAX_PAYLOAD_SIZE, OUTER_IPV4_HEADER_SIZE,
    OUTER_UDP_HEADER_SIZE, PROTOCOL_VERSION, PROTOCOL_VERSION_FEC, PROTOCOL_VERSION_TCP,
};

pub use tcp_ext::{
    build_tcp_packet, decode_tcp_payload, derive_syn_cookie, tcp_flags, AckProgress, Tcp5Tuple,
    TcpExtHeader, TcpReliable, TcpReliableReceiver, TcpReliableSender, TCP_EXT_HEADER_SIZE,
    TCP_RETRANSMIT_WINDOW,
};

pub use fec::{
    build_fec_data_packet, build_fec_parity_packet, decode_fec_payload, FecDecoder, FecEncoder,
    FecHeader, FecStats, DEFAULT_BLOCK_SIZE, FEC_HEADER_SIZE, FEC_MAX_PAYLOAD, MAX_BLOCK_SIZE,
};

pub use control::{caps, disconnect_reason, game_id, ControlDecodeError, ControlMessage};
pub use telemetry::TelemetryReport;
