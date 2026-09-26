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

use std::net::Ipv4Addr;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use serde::{Deserialize, Serialize};
use thiserror::Error;

// ── Message type tags ───────────────────────────────────────────────

const MSG_PING: u8 = 0x01;
const MSG_PONG: u8 = 0x02;
const MSG_REGISTER: u8 = 0x03;
const MSG_REGISTER_ACK: u8 = 0x04;
const MSG_DISCONNECT: u8 = 0x05;
const MSG_SERVER_INFO: u8 = 0x06;
const MSG_TELEMETRY: u8 = 0x07;

// ── Game IDs ────────────────────────────────────────────────────────

/// Well-known game identifiers used in registration.
pub mod game_id {
    pub const UNKNOWN: u8 = 0;
    pub const FORTNITE: u8 = 1;
    pub const CS2: u8 = 2;
    pub const DOTA2: u8 = 3;
    /// Facepunch's Rust (the survival game, not the language).
    pub const RUST: u8 = 4;
    /// Riot Games' Valorant (5v5 tactical shooter).
    pub const VALORANT: u8 = 5;
    /// Respawn / EA's Apex Legends (battle-royale).
    pub const APEX: u8 = 6;
    /// Blizzard Entertainment's Overwatch 2 (5v5 hero shooter).
    pub const OVERWATCH2: u8 = 7;
    /// Riot Games' League of Legends (team MOBA).
    pub const LOL: u8 = 8;
    /// Krafton's PUBG: Battlegrounds (battle-royale).
    pub const PUBG: u8 = 9;
    /// Valve's Counter-Strike: Global Offensive (legacy build).
    pub const CSGO: u8 = 10;
    /// Nexon's MapleStory (side-scrolling MMORPG).
    pub const MAPLESTORY: u8 = 11;
    /// HoYoverse's Genshin Impact (open-world action RPG).
    pub const GENSHIN: u8 = 12;
    /// Psyonix's Rocket League (vehicular soccer).
    pub const ROCKETLEAGUE: u8 = 13;
    /// Wargaming's World of Tanks (vehicle combat MMO).
    pub const WOT: u8 = 14;
    /// Behaviour Interactive's Dead by Daylight (asymmetric horror).
    pub const DEADBYDAYLIGHT: u8 = 15;
    /// Reissad Studio's Bodycam (first-person shooter).
    pub const BODYCAM: u8 = 16;
    /// Roblox Corporation's Roblox (user-generated game platform).
    pub const ROBLOX: u8 = 17;
    /// The Indie Stone's Project Zomboid (isometric survival).
    pub const ZOMBOID: u8 = 18;
    /// BULKHEAD's WARDOGS (100-player tactical FPS, Unreal Engine 5).
    pub const WARDDOGS: u8 = 19;

    /// Canonical CLI key mapped to its wire id, ordered by ascending id.
    ///
    /// Keys byte-match the client's canonical `--game` CLI strings. [`UNKNOWN`]
    /// (0) is reserved and therefore has no entry here.
    pub const GAME_IDS: &[(&str, u8)] = &[
        ("fortnite", FORTNITE),
        ("cs2", CS2),
        ("dota2", DOTA2),
        ("rust", RUST),
        ("valorant", VALORANT),
        ("apex", APEX),
        ("ow2", OVERWATCH2),
        ("lol", LOL),
        ("pubg", PUBG),
        ("csgo", CSGO),
        ("maplestory", MAPLESTORY),
        ("genshin", GENSHIN),
        ("rocketleague", ROCKETLEAGUE),
        ("wot", WOT),
        ("deadbydaylight", DEADBYDAYLIGHT),
        ("bodycam", BODYCAM),
        ("roblox", ROBLOX),
        ("zomboid", ZOMBOID),
        ("wardogs", WARDDOGS),
    ];

    /// Resolve a CLI game key to its wire id, or [`UNKNOWN`] when absent.
    pub fn id_for_key(key: &str) -> u8 {
        GAME_IDS
            .iter()
            .find_map(|(candidate, id)| (*candidate == key).then_some(*id))
            .unwrap_or(UNKNOWN)
    }

    /// Resolve a wire id back to its CLI game key, if it maps to a real game.
    pub fn key_for_id(id: u8) -> Option<&'static str> {
        GAME_IDS
            .iter()
            .find_map(|(key, candidate)| (*candidate == id).then_some(*key))
    }
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
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
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
        /// Client's data-plane source port, or 0 when the client does not
        /// report one. A non-zero value binds the session to that port.
        #[serde(default)]
        data_port: u16,
        /// The game server the client intends to reach, when known. The relay
        /// resolves it against its local MMDB so the ack can carry the
        /// destination region for the client's region prior. Pre-upgrade
        /// clients omit this trailing field.
        #[serde(default)]
        destination: Option<Ipv4Addr>,
    },

    /// Proxy → Client: registration accepted.
    RegisterAck {
        /// Session identifier assigned by the proxy.
        session_id: u32,
        /// Data-plane session token (included in every tunnel header).
        /// Used for per-packet authentication alongside IP-based auth.
        session_token: u32,
        /// Proxy node ID.
        node_id: String,
        /// Proxy geographic region.
        region: String,
        /// Whether this proxy accepts [`ControlMessage::Telemetry`] on the
        /// control connection. Pre-upgrade proxies omit this trailing byte, so
        /// it decodes as `false` and the client falls back to HTTP.
        #[serde(default)]
        telemetry_quic: bool,
        /// Region of the destination the client registered, as resolved by the
        /// relay's MMDB (an ISO 3166-1 alpha-2 country code, or `None` when the
        /// relay has no database or cannot place the address). Pre-upgrade
        /// relays omit this trailing field.
        #[serde(default)]
        dest_region: Option<String>,
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

    /// Client → Proxy: an anonymised telemetry report.
    ///
    /// Carries the same JSON schema as the HTTP `/telemetry` body so the proxy
    /// can feed both transports through one parse/validate/aggregate path.
    Telemetry {
        /// JSON-encoded report body, capped at
        /// [`crate::telemetry::MAX_TELEMETRY_BODY`] bytes.
        report_json: Vec<u8>,
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
                data_port,
                destination,
            } => {
                buf.put_u8(MSG_REGISTER);
                buf.put_u8(*protocol_version);
                buf.put_u8(*game);
                // The data port and destination are trailing extensions. The
                // port is emitted when non-zero, or when a destination follows
                // so the leading u16 stays unambiguously the port. The
                // destination is four octets when the client knows it.
                if *data_port != 0 || destination.is_some() {
                    buf.put_u16(*data_port);
                }
                if let Some(ip) = destination {
                    buf.put_slice(&ip.octets());
                }
            }
            Self::RegisterAck {
                session_id,
                session_token,
                node_id,
                region,
                telemetry_quic,
                dest_region,
            } => {
                buf.put_u8(MSG_REGISTER_ACK);
                buf.put_u32(*session_id);
                buf.put_u32(*session_token);
                put_short_string(&mut buf, node_id);
                put_short_string(&mut buf, region);
                // Trailing extensions: the telemetry capability byte, then the
                // destination region. The byte is emitted when telemetry is
                // supported or when a region follows, so a client that reads
                // only the byte still sees the capability and ignores the rest.
                if *telemetry_quic || dest_region.is_some() {
                    buf.put_u8(u8::from(*telemetry_quic));
                }
                if let Some(dest_region) = dest_region {
                    put_short_string(&mut buf, dest_region);
                }
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
            Self::Telemetry { report_json } => {
                buf.put_u8(MSG_TELEMETRY);
                buf.put_u32(report_json.len() as u32);
                buf.put_slice(report_json);
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

        let mut buf = data;
        let msg_type = buf.get_u8();

        match msg_type {
            MSG_PING => {
                ensure_remaining(buf, 8)?;
                let timestamp_us = buf.get_u64();
                Ok(Self::Ping { timestamp_us })
            }
            MSG_PONG => {
                ensure_remaining(buf, 16)?;
                let client_timestamp_us = buf.get_u64();
                let server_timestamp_us = buf.get_u64();
                Ok(Self::Pong {
                    client_timestamp_us,
                    server_timestamp_us,
                })
            }
            MSG_REGISTER => {
                ensure_remaining(buf, 2)?;
                let protocol_version = buf.get_u8();
                let game = buf.get_u8();
                // data_port and destination are optional: a client that
                // predates the extensions sends no trailing bytes and defaults
                // to no port (principal-only binding) and no destination.
                let data_port = if buf.len() >= 2 { buf.get_u16() } else { 0 };
                let destination = if buf.len() >= 4 {
                    Some(Ipv4Addr::from(buf.get_u32()))
                } else {
                    None
                };
                Ok(Self::Register {
                    protocol_version,
                    game,
                    data_port,
                    destination,
                })
            }
            MSG_REGISTER_ACK => {
                ensure_remaining(buf, 8)?;
                let session_id = buf.get_u32();
                let session_token = buf.get_u32();
                let node_id = get_short_string(&mut buf)?;
                let region = get_short_string(&mut buf)?;
                // The telemetry capability and destination region are optional
                // trailing extensions: a pre-upgrade proxy sends neither, so
                // telemetry decodes as unsupported and the region as unknown.
                let telemetry_quic = !buf.is_empty() && buf.get_u8() != 0;
                let dest_region = if buf.is_empty() {
                    None
                } else {
                    Some(get_short_string(&mut buf)?)
                };
                Ok(Self::RegisterAck {
                    session_id,
                    session_token,
                    node_id,
                    region,
                    telemetry_quic,
                    dest_region,
                })
            }
            MSG_DISCONNECT => {
                ensure_remaining(buf, 1)?;
                let reason = buf.get_u8();
                Ok(Self::Disconnect { reason })
            }
            MSG_SERVER_INFO => {
                ensure_remaining(buf, 9)?;
                let load_pct = buf.get_u8();
                let active_clients = buf.get_u32();
                let capacity = buf.get_u32();
                Ok(Self::ServerInfo {
                    load_pct,
                    active_clients,
                    capacity,
                })
            }
            MSG_TELEMETRY => {
                ensure_remaining(buf, 4)?;
                let len = buf.get_u32() as usize;
                ensure_remaining(buf, len)?;
                let report_json = buf[..len].to_vec();
                Ok(Self::Telemetry { report_json })
            }
            other => Err(ControlDecodeError::UnknownMessageType(other)),
        }
    }

    /// Read one length-prefixed message from a QUIC recv stream.
    ///
    /// Returns `None` if the stream is cleanly finished.
    #[cfg(feature = "quic")]
    pub async fn read_from(
        recv: &mut quinn::RecvStream,
    ) -> Result<Option<Self>, ControlDecodeError> {
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
            data_port: 41234,
            destination: Some(Ipv4Addr::new(104, 26, 1, 50)),
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_register_legacy_wire_form_decodes_without_data_port() {
        // A client that predates the data_port and destination extensions sends
        // only the type, protocol version, and game id.
        let legacy = [MSG_REGISTER, 1, game_id::CS2];
        let decoded = ControlMessage::decode(&legacy).unwrap();
        assert_eq!(
            decoded,
            ControlMessage::Register {
                protocol_version: 1,
                game: game_id::CS2,
                data_port: 0,
                destination: None,
            }
        );
    }

    #[test]
    fn test_register_zero_data_port_keeps_short_wire_form() {
        let msg = ControlMessage::Register {
            protocol_version: 1,
            game: game_id::RUST,
            data_port: 0,
            destination: None,
        };
        assert_eq!(msg.encode().len(), 3, "zero data_port must stay lenient");

        let msg = ControlMessage::Register {
            protocol_version: 1,
            game: game_id::RUST,
            data_port: 4434,
            destination: None,
        };
        assert_eq!(msg.encode().len(), 5, "non-zero data_port is appended");
    }

    #[test]
    fn test_register_destination_reserves_the_port_field() {
        // A destination-only registration still writes the port u16 (as zero)
        // so the decoder never reads the leading destination octet as a port.
        let msg = ControlMessage::Register {
            protocol_version: 1,
            game: game_id::RUST,
            data_port: 0,
            destination: Some(Ipv4Addr::new(8, 8, 8, 8)),
        };
        let encoded = msg.encode();
        assert_eq!(
            encoded.len(),
            9,
            "type, version, game, zero port, destination"
        );
        assert_eq!(
            ControlMessage::decode(&encoded).unwrap(),
            msg,
            "destination must survive the reserved port field"
        );
    }

    #[test]
    fn test_register_ack_roundtrip() {
        let msg = ControlMessage::RegisterAck {
            session_id: 42,
            session_token: 0xAB,
            node_id: "proxy-sea-001".into(),
            region: "sea".into(),
            telemetry_quic: true,
            dest_region: Some("AU".into()),
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_register_ack_legacy_wire_form_has_no_telemetry() {
        // A pre-upgrade proxy emits no trailing capability byte or region.
        let legacy = ControlMessage::RegisterAck {
            session_id: 42,
            session_token: 0xAB,
            node_id: "proxy-sea-001".into(),
            region: "sea".into(),
            telemetry_quic: false,
            dest_region: None,
        }
        .encode();
        let decoded = ControlMessage::decode(&legacy).unwrap();
        assert_eq!(
            decoded,
            ControlMessage::RegisterAck {
                session_id: 42,
                session_token: 0xAB,
                node_id: "proxy-sea-001".into(),
                region: "sea".into(),
                telemetry_quic: false,
                dest_region: None,
            },
            "an ack without trailing extensions must decode as unsupported and regionless"
        );
    }

    #[test]
    fn test_register_ack_dest_region_survives_a_false_capability() {
        // A relay that resolves a region but does not accept QUIC telemetry
        // still has to emit the capability byte so the region can follow.
        let msg = ControlMessage::RegisterAck {
            session_id: 7,
            session_token: 0xCD,
            node_id: "proxy-syd-001".into(),
            region: "ap-southeast-2".into(),
            telemetry_quic: false,
            dest_region: Some("APAC".into()),
        };
        let decoded = ControlMessage::decode(&msg.encode()).unwrap();
        assert_eq!(decoded, msg);

        // The capability byte is present even though the region is absent.
        let telemetry_only = ControlMessage::RegisterAck {
            session_id: 7,
            session_token: 0xCD,
            node_id: "proxy-syd-001".into(),
            region: "ap-southeast-2".into(),
            telemetry_quic: true,
            dest_region: None,
        };
        assert_eq!(
            ControlMessage::decode(&telemetry_only.encode()).unwrap(),
            telemetry_only
        );
    }

    #[test]
    fn test_register_and_ack_serde_defaults_accept_missing_extensions() {
        let register: ControlMessage =
            serde_json::from_str(r#"{"Register":{"protocol_version":1,"game":2}}"#).unwrap();
        assert_eq!(
            register,
            ControlMessage::Register {
                protocol_version: 1,
                game: 2,
                data_port: 0,
                destination: None,
            }
        );

        let ack: ControlMessage = serde_json::from_str(
            r#"{"RegisterAck":{"session_id":1,"session_token":2,"node_id":"n","region":"r"}}"#,
        )
        .unwrap();
        assert_eq!(
            ack,
            ControlMessage::RegisterAck {
                session_id: 1,
                session_token: 2,
                node_id: "n".into(),
                region: "r".into(),
                telemetry_quic: false,
                dest_region: None,
            }
        );
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
    fn test_telemetry_roundtrip() {
        let msg = ControlMessage::Telemetry {
            report_json: br#"{"game_id":2,"sample_count":10}"#.to_vec(),
        };
        let encoded = msg.encode();
        let decoded = ControlMessage::decode(&encoded).unwrap();
        assert_eq!(msg, decoded);
    }

    #[test]
    fn test_telemetry_truncated_body_is_rejected() {
        // Length prefix claims 8 bytes but only 3 follow.
        let mut framed = vec![MSG_TELEMETRY];
        framed.extend_from_slice(&8u32.to_be_bytes());
        framed.extend_from_slice(b"abc");
        assert!(matches!(
            ControlMessage::decode(&framed),
            Err(ControlDecodeError::BufferTooSmall { .. })
        ));
    }

    #[test]
    fn test_framed_encoding() {
        let msg = ControlMessage::Ping { timestamp_us: 999 };
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

    #[test]
    fn test_game_ids_unique_and_stable() {
        assert_eq!(game_id::GAME_IDS.len(), 19, "every real game needs one id");

        // Each id 1..=19 must appear exactly once (0 stays reserved for UNKNOWN).
        let mut seen = [0u8; 20];
        for (key, id) in game_id::GAME_IDS.iter().copied() {
            assert!(
                (1..=19).contains(&id),
                "key {key:?} has out-of-range id {id}"
            );
            assert_eq!(seen[id as usize], 0, "duplicate id {id}");
            seen[id as usize] += 1;
        }
        for id in 1..=19u8 {
            assert_eq!(seen[id as usize], 1, "id {id} missing or duplicated");
        }

        assert_eq!(game_id::FORTNITE, 1);
        assert_eq!(game_id::CS2, 2);
        assert_eq!(game_id::DOTA2, 3);
        assert_eq!(game_id::RUST, 4);
        assert_eq!(game_id::VALORANT, 5);
        assert_eq!(game_id::APEX, 6);
        assert_eq!(game_id::OVERWATCH2, 7);
        assert_eq!(game_id::LOL, 8);
        assert_eq!(game_id::PUBG, 9);
        assert_eq!(game_id::CSGO, 10);
        assert_eq!(game_id::MAPLESTORY, 11);
        assert_eq!(game_id::GENSHIN, 12);
        assert_eq!(game_id::ROCKETLEAGUE, 13);
        assert_eq!(game_id::WOT, 14);
        assert_eq!(game_id::DEADBYDAYLIGHT, 15);
        assert_eq!(game_id::BODYCAM, 16);
        assert_eq!(game_id::ROBLOX, 17);
        assert_eq!(game_id::ZOMBOID, 18);
        assert_eq!(game_id::WARDDOGS, 19);
    }

    #[test]
    fn test_id_for_key_roundtrip() {
        for (key, id) in game_id::GAME_IDS.iter().copied() {
            assert_eq!(game_id::id_for_key(key), id, "id_for_key({key:?})");
            assert_eq!(game_id::key_for_id(id), Some(key), "key_for_id({id})");
        }
        assert_eq!(game_id::id_for_key("minecraft"), game_id::UNKNOWN);
        assert!(game_id::key_for_id(250).is_none());
    }

    #[test]
    fn test_game_ids_prefix_unchanged() {
        // The nine original games keep the exact ids they shipped with.
        let legacy: &[(&str, u8)] = &[
            ("fortnite", game_id::FORTNITE),
            ("cs2", game_id::CS2),
            ("dota2", game_id::DOTA2),
            ("rust", game_id::RUST),
            ("valorant", game_id::VALORANT),
            ("apex", game_id::APEX),
            ("ow2", game_id::OVERWATCH2),
            ("lol", game_id::LOL),
            ("pubg", game_id::PUBG),
        ];
        for (key, id) in legacy.iter().copied() {
            assert_eq!(game_id::id_for_key(key), id);
            assert_eq!(game_id::key_for_id(id), Some(key));
        }
        // They are also the first nine entries, in ascending id order.
        for (index, &(key, id)) in game_id::GAME_IDS.iter().take(9).enumerate() {
            assert_eq!(id, (index as u8) + 1);
            assert_eq!(key, legacy[index].0);
        }
    }
}
