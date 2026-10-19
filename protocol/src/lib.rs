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

use std::time::Duration;

/// How long a relay keeps a data-plane session token valid.
///
/// The relay side uses the same constant, so the client's rotation interval
/// can be checked against the lifetime it has to stay ahead of.
pub const SESSION_TOKEN_TTL: Duration = Duration::from_secs(300);

/// How often a client rotates its data-plane session token.
///
/// Must stay comfortably below [`SESSION_TOKEN_TTL`]: rotating at 40% of the
/// lifetime leaves room for a lost exchange and a reconnect before the old
/// token expires. The previous token stays valid for a short grace window
/// after a rotation, so in-flight packets are never dropped.
pub const SESSION_TOKEN_ROTATION: Duration = Duration::from_secs(120);

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn token_rotation_leaves_room_before_the_token_expires() {
        assert!(SESSION_TOKEN_ROTATION < SESSION_TOKEN_TTL);
        assert!(
            SESSION_TOKEN_ROTATION.as_secs() * 2 <= SESSION_TOKEN_TTL.as_secs(),
            "rotation must happen at no more than half the token lifetime"
        );
    }
}
