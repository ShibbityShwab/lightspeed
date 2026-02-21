//! # Control Plane Messages
//!
//! Binary-encoded messages exchanged over the QUIC control channel between
//! client and proxy. Each message is length-prefixed on the wire:
//!
//! ```text
//! [2 bytes: payload length (big-endian)] [payload bytes]
//! ```
//!
//! Payload format: `[1 byte: message type] [type-specific fields]`

use bytes::{Buf, BufMut, Bytes, BytesMut};
use thiserror::Error;

// ── Message type tags ───────────────────────────────────────────────

const MSG_PING: u8 = 0x01;
const MSG_PONG: u8 = 0x02;
const MSG_REGISTER: u8 = 0x03;
const MSG_REGISTER_ACK: u8 = 0x04;
const MSG_DISCONNECT: u8 = 0x05;
const MSG_SERVER_INFO: u8 = 0x06;

// ── Game IDs ────────────────────────────────────────────────────────

/// Well-known game identifiers used in registration.
pub mod game_id {
    pub const UNKNOWN: u8 = 0;
    pub const FORTNITE: u8 = 1;
    pub const CS2: u8 = 2;
    pub const DOTA2: u8 = 3;
}

/// Disconnect reason codes.
pub mod disconnect_reason {
    pub const NORMAL: u8 = 0;
    pub const TIMEOUT: u8 = 1;
    pub const SERVER_SHUTDOWN: u8 = 2;
    pub const RATE_LIMITED: u8 = 3;
    pub const AUTH_FAILURE: u8 = 4;
}

// ── Control message enum ────────────────────────────────────────────

/// A control-plane message exchanged over QUIC.
#[derive(Debug, Clone, PartialEq)]
pub enum ControlMessage {
    /// Client → Proxy: latency probe.
    Ping {
        /// Client-side timestamp in microseconds (echoed back in Pong).
        timestamp_us: u64,
    },

    /// Proxy → Client: latency probe response.
    Pong {
        /// Echoed client timestamp.
        client_timestamp_us: u64,
        /// Proxy-side timestamp in microseconds.
        server_timestamp_us: u64,
    },

    /// Client → Proxy: register this client session.
    Register {
        /// Client protocol version.
        protocol_version: u8,
        /// Game being optimized (see [`game_id`]).
        game: u8,
    },

    /// Proxy → Client: registration accepted.
    RegisterAck {
        /// Session identifier assigned by the proxy.
        session_id: u32,
        /// Data-plane session token (included in every tunnel header).
        /// Used for per-packet authentication alongside IP-based auth.
        session_token: u8,
        /// Proxy node ID.
        node_id: String,
        /// Proxy geographic region.
        region: String,
    },

    /// Either direction: graceful disconnect.
    Disconnect {
        /// Reason code (see [`disconnect_reason`]).
        reason: u8,
    },

    /// Proxy → Client: current server status.
    ServerInfo {
        /// Server load percentage (0-100).
        load_pct: u8,
        /// Number of active client sessions.
        active_clients: u32,
        /// Maximum client capacity.
        capacity: u32,
    },
}

// ── Errors ──────────────────────────────────────────────────────────

/// Errors from control message encode/decode.
#[derive(Error, Debug)]
pub enum ControlDecodeError {
    #[error("Buffer too small: need {need} bytes, got {got}")]
    BufferTooSmall { got: usize, need: usize },

    #[error("Unknown message type: 0x{0:02X}")]
    UnknownMessageType(u8),

    #[error("Invalid UTF-8 in string field: {0}")]
    InvalidUtf8(#[from] std::string::FromUtf8Error),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}

// ── Encode ──────────────────────────────────────────────────────────

impl ControlMessage {
    /// Encode the message into bytes (without the length prefix).
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(64);
        match self {
            Self::Ping { timestamp_us } => {
                buf.put_u8(MSG_PING);
                buf.put_u64(*timestamp_us);
            }
            Self::Pong {
                client_timestamp_us,
                server_timestamp_us,
            } => {
                buf.put_u8(MSG_PONG);
                buf.put_u64(*client_timestamp_us);
                buf.put_u64(*server_timestamp_us);
            }
            Self::Register {
                protocol_version,
                game,
            } => {
                buf.put_u8(MSG_REGISTER);
                buf.put_u8(*protocol_version);
                buf.put_u8(*game);
            }
            Self::RegisterAck {
                session_id,
                session_token,
                node_id,
                region,
            } => {
                buf.put_u8(MSG_REGISTER_ACK);
                buf.put_u32(*session_id);
                buf.put_u8(*session_token);
                put_short_string(&mut buf, node_id);
                put_short_string(&mut buf, region);
            }
            Self::Disconnect { reason } => {
                buf.put_u8(MSG_DISCONNECT);
                buf.put_u8(*reason);
            }
            Self::ServerInfo {
                load_pct,
                active_clients,
                capacity,
            } => {
                buf.put_u8(MSG_SERVER_INFO);
                buf.put_u8(*load_pct);
                buf.put_u32(*active_clients);
                buf.put_u32(*capacity);
            }
        }
        buf.freeze()
    }

    /// Encode the message with a 2-byte length prefix (for framing on a stream).
    pub fn encode_framed(&self) -> Bytes {
        let payload = self.encode();
        let mut buf = BytesMut::with_capacity(2 + payload.len());
        buf.put_u16(payload.len() as u16);
        buf.put_slice(&payload);
        buf.freeze()
    }
}

// ── Decode ──────────────────────────────────────────────────────────

impl ControlMessage {
    /// Decode a message from bytes (without the length prefix).
    pub fn decode(data: &[u8]) -> Result<Self, ControlDecodeError> {
        if data.is_empty() {
            return Err(ControlDecodeError::BufferTooSmall { got: 0, need: 1 });
        }

        let mut buf = &data[..];
        let msg_type = buf.get_u8();

        match msg_type {
            MSG_PING => {
                ensure_remaining(&buf, 8)?;
                let timestamp_us = buf.get_u64();
                Ok(Self::Ping { timestamp_us })
            }
            MSG_PONG => {
                ensure_remaining(&buf, 16)?;
                let client_timestamp_us = buf.get_u64();
                let server_timestamp_us = buf.get_u64();
                Ok(Self::Pong {
                    client_timestamp_us,
                    server_timestamp_us,
                })
            }
            MSG_REGISTER => {
                ensure_remaining(&buf, 2)?;
                let protocol_version = buf.get_u8();
                let game = buf.get_u8();
                Ok(Self::Register {
                    protocol_version,
                    game,
                })
            }
            MSG_REGISTER_ACK => {
                ensure_remaining(&buf, 5)?;
                let session_id = buf.get_u32();
                let session_token = buf.get_u8();
                let node_id = get_short_string(&mut buf)?;
                let region = get_short_string(&mut buf)?;
                Ok(Self::RegisterAck {
                    session_id,
                    session_token,
                    node_id,
                    region,
                })
            }
            MSG_DISCONNECT => {
                ensure_remaining(&buf, 1)?;
                let reason = buf.get_u8();
                Ok(Self::Disconnect { reason })
            }
            MSG_SERVER_INFO => {
                ensure_remaining(&buf, 9)?;
                let load_pct = buf.get_u8();
                let active_clients = buf.get_u32();
                let capacity = buf.get_u32();
                Ok(Self::ServerInfo {
                    load_pct,
                    active_clients,
                    capacity,
                })
            }
            other => Err(ControlDecodeError::UnknownMessageType(other)),
        }
    }

    /// Read one length-prefixed message from a QUIC recv stream.
    ///
    /// Returns `None` if the stream is cleanly finished.
    #[cfg(feature = "quic")]
    pub async fn read_from(recv: &mut quinn::RecvStream) -> Result<Option<Self>, ControlDecodeError> {
        // Read 2-byte length prefix
        let mut len_buf = [0u8; 2];
        match recv.read_exact(&mut len_buf).await {
            Ok(()) => {}
            Err(e) => {
                // Stream finished cleanly → no more messages
                if matches!(e, quinn::ReadExactError::FinishedEarly(_)) {
                    return Ok(None);
                }
                return Err(ControlDecodeError::Io(std::io::Error::new(
                    std::io::ErrorKind::UnexpectedEof,
                    e.to_string(),
                )));
            }
        }
        let len = u16::from_be_bytes(len_buf) as usize;
        if len == 0 {
            return Err(ControlDecodeError::BufferTooSmall { got: 0, need: 1 });
        }

        // Read payload
        let mut payload = vec![0u8; len];
        recv.read_exact(&mut payload).await.map_err(|e| {
            ControlDecodeError::Io(std::io::Error::new(
                std::io::ErrorKind::UnexpectedEof,
                e.to_string(),
            ))
        })?;

        Self::decode(&payload).map(Some)
    }

    /// Write one length-prefixed message to a QUIC send stream.
    #[cfg(feature = "quic")]
    pub async fn write_to(&self, send: &mut quinn::SendStream) -> Result<(), ControlDecodeError> {
        let framed = self.encode_framed();
        send.write_all(&framed).await.map_err(|e| {
            ControlDecodeError::Io(std::io::Error::new(
                std::io::ErrorKind::BrokenPipe,
                e.to_string(),
            ))
        })?;
        Ok(())
    }
}

// ── Helpers ─────────────────────────────────────────────────────────

fn ensure_remaining(buf: &[u8], need: usize) -> Result<(), ControlDecodeError> {
    if buf.len() < need {
        Err(ControlDecodeError::BufferTooSmall {
            got: buf.len(),
            need,
        })
    } else {
        Ok(())
    }
}

/// Write a length-prefixed short string (max 255 bytes).
fn put_short_string(buf: &mut BytesMut, s: &str) {
    let bytes = s.as_bytes();
    let len = bytes.len().min(255) as u8;
    buf.put_u8(len);
    buf.put_slice(&bytes[..len as usize]);
}

/// Read a length-prefixed short string.
fn get_short_string(buf: &mut &[u8]) -> Result<String, ControlDecodeError> {
    ensure_remaining(buf, 1)?;
    let len = buf.get_u8() as usize;
    ensure_remaining(buf, len)?;
    let bytes = &buf[..len];
    let s = String::from_utf8(bytes.to_vec())?;
    buf.advance(len);
    Ok(s)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ping_roundtrip() {
        let msg = ControlMessage::Ping {
            timestamp_us: 123456789,
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_pong_roundtrip() {
        let msg = ControlMessage::Pong {
            client_timestamp_us: 100,
            server_timestamp_us: 200,
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_register_roundtrip() {
        let msg = ControlMessage::Register {
            protocol_version: 1,
            game: game_id::FORTNITE,
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_register_ack_roundtrip() {
        let msg = ControlMessage::RegisterAck {
            session_id: 42,
            session_token: 0xAB,
            node_id: "proxy-sea-001".into(),
            region: "sea".into(),
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_disconnect_roundtrip() {
        let msg = ControlMessage::Disconnect {
            reason: disconnect_reason::NORMAL,
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_server_info_roundtrip() {
        let msg = ControlMessage::ServerInfo {
            load_pct: 75,
            active_clients: 42,
            capacity: 100,
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_framed_encoding() {
        let msg = ControlMessage::Ping {
            timestamp_us: 999,
        };
        let framed = msg.encode_framed();
        // 2-byte len + 1-byte type + 8-byte u64 = 11 total
        assert_eq!(framed.len(), 11);
        let payload_len = u16::from_be_bytes([framed[0], framed[1]]) as usize;
        assert_eq!(payload_len, 9);

        let decoded = ControlMessage::decode(&framed[2..]).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_unknown_message_type() {
        let data = [0xFF, 0x00, 0x00];
        let result = ControlMessage::decode(&data);
        assert!(matches!(
            result,
            Err(ControlDecodeError::UnknownMessageType(0xFF))
        ));
    }

    #[test]
    fn test_empty_buffer() {
        let result = ControlMessage::decode(&[]);
        assert!(matches!(
            result,
            Err(ControlDecodeError::BufferTooSmall { .. })
        ));
    }
}
