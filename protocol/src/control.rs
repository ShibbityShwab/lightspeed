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
const MSG_ROTATE_REQUEST: u8 = 0x08;
const MSG_ROTATE_ACK: u8 = 0x09;
const MSG_SERVER_LEG: u8 = 0x0A;
const MSG_CLIENT_CAPS: u8 = 0x0B;
const MSG_SERVER_LEG_APP: u8 = 0x0C;
const MSG_SERVER_TRACE: u8 = 0x0D;

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
    /// Mojang's Minecraft, Bedrock Edition (UDP gameplay over 19132/19133).
    pub const MINECRAFT: u8 = 20;
    /// Crytek's Hunt: Showdown 1896 (UDP gameplay over 20000-20099).
    pub const HUNT: u8 = 21;
    /// Mojang's Minecraft, Java Edition (TCP gameplay over 25565).
    pub const MINECRAFT_JAVA: u8 = 22;
    pub const RAINBOWSIX: u8 = 23;
    /// Embark Studios' ARC Raiders (UDP gameplay over 61606-61632).
    pub const ARCRAIDERS: u8 = 24;
    /// Raven Software's Jedi Knight II: Jedi Outcast, played through the NWH
    /// Multiplayer client (UDP gameplay over 28070).
    pub const JEDIOUTCAST: u8 = 25;
    /// CipSoft's Tibia (UDP login server 7171, game world 7172).
    pub const TIBIA: u8 = 26;
    /// Kefir's ROB RUN (UDP gameplay on an ephemeral server port).
    pub const ROBRUN: u8 = 27;
    /// EA Sports' EA SPORTS FC 27 (UDP gameplay over 21000-21050).
    pub const FC27: u8 = 28;
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
        ("minecraft", MINECRAFT),
        ("hunt", HUNT),
        ("minecraft-java", MINECRAFT_JAVA),
        ("rainbowsix", RAINBOWSIX),
        ("arcraiders", ARCRAIDERS),
        ("jedioutcast", JEDIOUTCAST),
        ("tibia", TIBIA),
        ("robrun", ROBRUN),
        ("fc27", FC27),
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

/// Capability bitmap carried in the registration ack's `caps` byte and, as a
/// client capability, in [`ControlMessage::ClientCaps`].
///
/// A one-byte additive bitmap. Pre-upgrade relays omit the ack byte, which
/// decodes as zero, "no TCP"; pre-upgrade clients never send
/// [`ControlMessage::ClientCaps`], so a relay that understands the message
/// treats them as having no capabilities and does not push server-leg reports
/// they cannot consume.
pub mod caps {
    /// The relay supports the TCP tunnel (protocol version 5).
    pub const TCP_TUNNEL: u8 = 0b0000_0001;
    /// The relay can send relay-measured server-leg reports (`MSG_SERVER_LEG`
    /// and `MSG_SERVER_LEG_APP`, both on server-initiated unidirectional
    /// streams). In
    /// [`ControlMessage::ClientCaps`] the same bit means the client can read
    /// them: the relay pushes only to a control session that advertised it, so
    /// a client built before this feature never receives a report stream it
    /// would not consume. That gate is why no version bump is needed.
    pub const SERVER_LEG: u8 = 0b0000_0010;
}

/// Maximum hops a relay-side traceroute may report.
///
/// The relay never sends a probe past this TTL, and the client rejects a
/// reported hop count above it: a trace that claims more is not a path anyone
/// can be shown, it is a malformed or hostile frame.
pub const MAX_SERVER_TRACE_HOPS: u8 = 20;

/// Bit flags carried in [`ControlMessage::ServerTrace`]'s `flags` byte.
pub mod trace_flags {
    /// The destination itself answered, so the trace reached the server rather
    /// than ending at the last router that would reply. A trace without this
    /// bit still has a truthful last hop: the path is bounded by it, not fully
    /// enumerated.
    pub const COMPLETE: u8 = 0b0000_0001;
}

// ── QUIC control-plane ALPN ─────────────────────────────────────────

/// Application protocol name for the QUIC control plane, revision 1.
///
/// Versioned wire identifier: a future revision adds a new name
/// (`lightspeed-control/2`) and keeps `/1` offered while relays in the field
/// still speak it; `/1` is never renamed. The version is independent of the
/// data-plane [`crate::PROTOCOL_VERSION`]: the control plane and the data plane
/// evolve on separate schedules, so a TLS-only change does not force a header
/// bump.
///
/// Both the client and the relay read this one constant, so the two ends of
/// the handshake cannot drift.
pub const QUIC_CONTROL_ALPN_V1: &[u8] = b"lightspeed-control/1";

/// Every ALPN identifier this build offers on the QUIC control plane, most
/// preferred first.
///
/// The return shape is exactly what a rustls `ClientConfig`/`ServerConfig`
/// `alpn_protocols` field takes, so the client offer and the relay's
/// `required` mode are built from the same list. An empty list means "no ALPN
/// extension at all", which is the pre-rollout behaviour.
pub fn quic_control_alpn() -> Vec<Vec<u8>> {
    vec![QUIC_CONTROL_ALPN_V1.to_vec()]
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
        /// Capability bitmap (bit 0 = TCP tunnel support). Additive:
        /// pre-upgrade relays omit this trailing byte, which decodes as zero,
        /// and zero means "no TCP".
        #[serde(default)]
        caps: u8,
    },

    /// Client → Proxy: request a fresh data-plane session token.
    ///
    /// Carries the token the client currently holds so the relay can bind the
    /// rotation to the live session. A relay that predates this message decodes
    /// the type byte as [`ControlDecodeError::UnknownMessageType`] and ignores
    /// the frame, so a client that never rotates sees today's behaviour.
    RotateRequest {
        /// The data-plane session token the client is currently using.
        current_token: u32,
    },

    /// Proxy → Client: session token rotated.
    ///
    /// The old token stays valid only until the client acknowledges it on the
    /// data plane, so the ack names both tokens explicitly.
    RotateAck {
        /// The replacement data-plane session token.
        new_token: u32,
        /// The token this rotation replaces.
        old_token: u32,
    },

    /// Proxy → Client: the relay's own measurement of the relay-to-server leg.
    ///
    /// The relay sends an ICMP echo burst to a game server a live session is
    /// already forwarding to and reports the result over the control plane. The
    /// client cannot measure this leg itself (docs/server-leg-signal.md §4.1),
    /// so this is the only source for it. Additive: a client that predates this
    /// message decodes the type byte as [`ControlDecodeError::UnknownMessageType`]
    /// and ignores the frame, exactly as a relay that predates
    /// [`ControlMessage::RotateRequest`] ignores that one.
    ServerLeg {
        /// The game server address this measurement is for.
        server: Ipv4Addr,
        /// Median of the echo burst, microseconds.
        median_us: u32,
        /// Mean absolute delta between the burst medians in the window,
        /// microseconds. Zero is a legitimate value (a perfectly steady leg).
        jitter_us: u32,
        /// How many burst medians the window holds.
        samples: u8,
        /// Staleness of the measurement at send time, milliseconds.
        age_ms: u16,
    },

    /// Proxy → Client: the relay's passive app-RTT sample for the
    /// relay-to-server leg.
    ///
    /// The relay times every forwarded game-data packet to the first response it
    /// reads back on the same session socket, so this figure costs no added
    /// traffic. It rides next to [`ControlMessage::ServerLeg`] for the same
    /// `(session, server)` pair and is reported on the same server-initiated
    /// stream path; it exists because the ICMP measurement can be blocked while
    /// ordinary game traffic still answers.
    ///
    /// Additive: a client that predates it decodes the type byte as
    /// [`ControlDecodeError::UnknownMessageType`] and ignores the frame. The
    /// same [`caps::SERVER_LEG`] capability gates both report types, because a
    /// build that advertised it can already ignore an unknown frame.
    ServerLegApp {
        /// The game server address this sample is for.
        server: Ipv4Addr,
        /// Median of the relay's recent passive samples, microseconds.
        passive_median_us: u32,
        /// How many passive samples the relay's window holds.
        passive_samples: u8,
        /// Staleness of the newest sample at send time, milliseconds.
        age_ms: u16,
    },

    /// Proxy → Client: the relay's TTL-limited traceroute towards the game
    /// server.
    ///
    /// The relay sends small, empty UDP probes with increasing IP TTL and reads
    /// the ICMP time-exceeded errors from the socket error queue, so it learns
    /// the last router on the relay→server path without `CAP_NET_RAW`. That
    /// router is the placement anchor when the hosting network's geo data is
    /// unknown: the client can say "the path reaches <router>" instead of
    /// nothing. It is **not** a city label by itself - turning a hop address
    /// into a place still needs hop geolocation data the product does not have.
    ///
    /// Additive: a client that predates this message decodes the type byte as
    /// [`ControlDecodeError::UnknownMessageType`] and ignores the frame. It
    /// rides the same server-initiated unidirectional stream path as
    /// [`ControlMessage::ServerLeg`], gated by the same [`caps::SERVER_LEG`]
    /// capability, so an old client is never sent a stream it would not read.
    ServerTrace {
        /// The game server address this trace was for.
        server: Ipv4Addr,
        /// Highest TTL that produced any reply: the distance in hops to
        /// `last_hop`, or to the destination when `complete` is set. Silent
        /// intermediate hops do not lower it.
        hops: u8,
        /// The router that answered at the highest TTL below the destination
        /// (the penultimate router of the measured path). [`Ipv4Addr::UNSPECIFIED`]
        /// means no router answered: nothing may be placed from this trace.
        last_hop: Ipv4Addr,
        /// Round trip to `last_hop`, microseconds. `0` when `last_hop` is
        /// unspecified; a router reply always has a measured RTT.
        last_hop_rtt_us: u32,
        /// Bit flags, see [`trace_flags`].
        flags: u8,
        /// Staleness of the trace at send time, milliseconds.
        age_ms: u16,
    },

    /// Client → Proxy: capabilities this client can consume.
    ///
    /// The relay pushes [`ControlMessage::ServerLeg`] and
    /// [`ControlMessage::ServerLegApp`] reports on server-initiated streams and
    /// must not send them to a client that cannot read them. A client that predates this message never sends it, which the
    /// relay treats as "no capabilities"; a relay that predates it decodes the
    /// type byte as [`ControlDecodeError::UnknownMessageType`] and drops the
    /// stream without touching the connection. Sent once per control
    /// connection after registration, best-effort. Additive, so no version
    /// bump.
    ClientCaps {
        /// Bitmap of [`caps`] the client understands.
        caps: u8,
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
                caps,
            } => {
                buf.put_u8(MSG_REGISTER_ACK);
                buf.put_u32(*session_id);
                buf.put_u32(*session_token);
                put_short_string(&mut buf, node_id);
                put_short_string(&mut buf, region);
                // Trailing extensions: the telemetry capability byte, then the
                // destination region, then the capability bitmap. The telemetry
                // byte doubles as the presence marker for the whole block, so a
                // client that reads only that byte still sees the capability
                // and ignores the rest.
                let caps_present = *caps != 0;
                if *telemetry_quic || dest_region.is_some() || caps_present {
                    buf.put_u8(u8::from(*telemetry_quic));
                }
                // A region slot is always framed when a trailing capability
                // follows, so the fixed-width `caps` byte after the
                // variable-length region stays unambiguous; an absent region is
                // framed as a zero-length string.
                if dest_region.is_some() || caps_present {
                    if let Some(dest_region) = dest_region {
                        put_short_string(&mut buf, dest_region);
                    } else {
                        buf.put_u8(0);
                    }
                }
                if caps_present {
                    buf.put_u8(*caps);
                }
            }
            Self::RotateRequest { current_token } => {
                buf.put_u8(MSG_ROTATE_REQUEST);
                buf.put_u32(*current_token);
            }
            Self::RotateAck {
                new_token,
                old_token,
            } => {
                buf.put_u8(MSG_ROTATE_ACK);
                buf.put_u32(*new_token);
                buf.put_u32(*old_token);
            }
            Self::ServerLeg {
                server,
                median_us,
                jitter_us,
                samples,
                age_ms,
            } => {
                buf.put_u8(MSG_SERVER_LEG);
                buf.put_slice(&server.octets());
                buf.put_u32(*median_us);
                buf.put_u32(*jitter_us);
                buf.put_u8(*samples);
                buf.put_u16(*age_ms);
            }
            Self::ServerLegApp {
                server,
                passive_median_us,
                passive_samples,
                age_ms,
            } => {
                buf.put_u8(MSG_SERVER_LEG_APP);
                buf.put_slice(&server.octets());
                buf.put_u32(*passive_median_us);
                buf.put_u8(*passive_samples);
                buf.put_u16(*age_ms);
            }
            Self::ServerTrace {
                server,
                hops,
                last_hop,
                last_hop_rtt_us,
                flags,
                age_ms,
            } => {
                buf.put_u8(MSG_SERVER_TRACE);
                buf.put_slice(&server.octets());
                buf.put_u8(*hops);
                buf.put_slice(&last_hop.octets());
                buf.put_u32(*last_hop_rtt_us);
                buf.put_u8(*flags);
                buf.put_u16(*age_ms);
            }
            Self::ClientCaps { caps } => {
                buf.put_u8(MSG_CLIENT_CAPS);
                buf.put_u8(*caps);
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
                // The telemetry capability, destination region, and capability
                // bitmap are optional trailing extensions: a pre-upgrade proxy
                // sends none of them, so telemetry decodes as unsupported, the
                // region as unknown, and the caps as zero.
                let telemetry_quic = !buf.is_empty() && buf.get_u8() != 0;
                let dest_region = if buf.is_empty() {
                    None
                } else {
                    match get_short_string(&mut buf)? {
                        region if region.is_empty() => None,
                        region => Some(region),
                    }
                };
                let caps = if buf.is_empty() { 0 } else { buf.get_u8() };
                Ok(Self::RegisterAck {
                    session_id,
                    session_token,
                    node_id,
                    region,
                    telemetry_quic,
                    dest_region,
                    caps,
                })
            }
            MSG_ROTATE_REQUEST => {
                ensure_remaining(buf, 4)?;
                let current_token = buf.get_u32();
                Ok(Self::RotateRequest { current_token })
            }
            MSG_ROTATE_ACK => {
                ensure_remaining(buf, 8)?;
                let new_token = buf.get_u32();
                let old_token = buf.get_u32();
                Ok(Self::RotateAck {
                    new_token,
                    old_token,
                })
            }
            MSG_SERVER_LEG => {
                // Fixed-width payload: IPv4 (4) + median (4) + jitter (4) +
                // samples (1) + age (2). Trailing bytes are ignored, so a later
                // revision may append fields without breaking this reader.
                ensure_remaining(buf, 4 + 4 + 4 + 1 + 2)?;
                let server = Ipv4Addr::from(buf.get_u32());
                let median_us = buf.get_u32();
                let jitter_us = buf.get_u32();
                let samples = buf.get_u8();
                let age_ms = buf.get_u16();
                Ok(Self::ServerLeg {
                    server,
                    median_us,
                    jitter_us,
                    samples,
                    age_ms,
                })
            }
            MSG_SERVER_LEG_APP => {
                // Fixed-width payload: IPv4 (4) + passive median (4) +
                // samples (1) + age (2). Trailing bytes are ignored, so a later
                // revision may append fields without breaking this reader.
                ensure_remaining(buf, 4 + 4 + 1 + 2)?;
                let server = Ipv4Addr::from(buf.get_u32());
                let passive_median_us = buf.get_u32();
                let passive_samples = buf.get_u8();
                let age_ms = buf.get_u16();
                Ok(Self::ServerLegApp {
                    server,
                    passive_median_us,
                    passive_samples,
                    age_ms,
                })
            }
            MSG_SERVER_TRACE => {
                // Fixed-width payload: server IPv4 (4) + hops (1) + last-hop
                // IPv4 (4) + last-hop RTT (4) + flags (1) + age (2). Trailing
                // bytes are ignored, so a later revision may append fields
                // without breaking this reader.
                ensure_remaining(buf, 4 + 1 + 4 + 4 + 1 + 2)?;
                let server = Ipv4Addr::from(buf.get_u32());
                let hops = buf.get_u8();
                let last_hop = Ipv4Addr::from(buf.get_u32());
                let last_hop_rtt_us = buf.get_u32();
                let flags = buf.get_u8();
                let age_ms = buf.get_u16();
                Ok(Self::ServerTrace {
                    server,
                    hops,
                    last_hop,
                    last_hop_rtt_us,
                    flags,
                    age_ms,
                })
            }
            MSG_CLIENT_CAPS => {
                ensure_remaining(buf, 1)?;
                let caps = buf.get_u8();
                Ok(Self::ClientCaps { caps })
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
            caps: caps::TCP_TUNNEL,
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
            caps: 0,
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
                caps: 0,
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
            caps: 0,
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
            caps: 0,
        };
        assert_eq!(
            ControlMessage::decode(&telemetry_only.encode()).unwrap(),
            telemetry_only
        );
    }

    #[test]
    fn test_register_ack_caps_bitmap_is_additive() {
        // A relay advertising TCP sets bit 0; the byte rides after the region.
        let advertised = ControlMessage::RegisterAck {
            session_id: 5,
            session_token: 0xEF,
            node_id: "proxy".into(),
            region: "sea".into(),
            telemetry_quic: true,
            dest_region: Some("AU".into()),
            caps: caps::TCP_TUNNEL,
        };
        assert_eq!(
            ControlMessage::decode(&advertised.encode()).unwrap(),
            advertised
        );

        // The bitmap also survives when the region is absent (framed as an
        // empty string so the trailing caps byte stays unambiguous).
        let no_region = ControlMessage::RegisterAck {
            session_id: 5,
            session_token: 0xEF,
            node_id: "proxy".into(),
            region: "sea".into(),
            telemetry_quic: false,
            dest_region: None,
            caps: caps::TCP_TUNNEL,
        };
        assert_eq!(
            ControlMessage::decode(&no_region.encode()).unwrap(),
            no_region
        );

        // A legacy ack with no caps byte decodes as zero, "no TCP".
        let legacy = ControlMessage::RegisterAck {
            session_id: 5,
            session_token: 0xEF,
            node_id: "proxy".into(),
            region: "sea".into(),
            telemetry_quic: false,
            dest_region: None,
            caps: 0,
        };
        assert_eq!(ControlMessage::decode(&legacy.encode()).unwrap(), legacy);
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
                caps: 0,
            }
        );
    }

    #[test]
    fn test_rotate_request_and_ack_roundtrip() {
        let request = ControlMessage::RotateRequest {
            current_token: 0xDEAD_BEEF,
        };
        assert_eq!(ControlMessage::decode(&request.encode()).unwrap(), request);

        let ack = ControlMessage::RotateAck {
            new_token: 0x0102_0304,
            old_token: 0xDEAD_BEEF,
        };
        let encoded = ack.encode();
        // New token then old token, each a big-endian u32.
        assert_eq!(
            encoded,
            Bytes::from_static(&[
                MSG_ROTATE_ACK,
                0x01,
                0x02,
                0x03,
                0x04,
                0xDE,
                0xAD,
                0xBE,
                0xEF
            ])
        );
        assert_eq!(ControlMessage::decode(&encoded).unwrap(), ack);
    }

    #[test]
    fn test_rotate_frames_reject_truncated_payload_cleanly() {
        // A rotate request needs all four token bytes: three is malformed.
        let truncated_request = [MSG_ROTATE_REQUEST, 0x11, 0x22, 0x33];
        assert!(matches!(
            ControlMessage::decode(&truncated_request),
            Err(ControlDecodeError::BufferTooSmall { got: 3, need: 4 })
        ));

        // A rotate ack needs both tokens: a half-written pair is malformed.
        let truncated_ack = [MSG_ROTATE_ACK, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07];
        assert!(matches!(
            ControlMessage::decode(&truncated_ack),
            Err(ControlDecodeError::BufferTooSmall { got: 7, need: 8 })
        ));

        // A bare type byte is rejected, not silently defaulted to zero.
        assert!(matches!(
            ControlMessage::decode(&[MSG_ROTATE_REQUEST]),
            Err(ControlDecodeError::BufferTooSmall { got: 0, need: 4 })
        ));
    }

    #[test]
    fn test_rotate_request_wire_form_is_pinned_by_hardcoded_bytes() {
        // Hand-built frame: type tag 0x08 (rotate request) followed by the
        // big-endian current token 0x11223344. The bytes are literals on
        // purpose - with no rotate variant the tag is unknown, `decode` returns
        // UnknownMessageType, and this test fails, pinning the whole feature to
        // the wire format.
        let frame = [0x08u8, 0x11, 0x22, 0x33, 0x44];
        let decoded = ControlMessage::decode(&frame).unwrap();
        assert_eq!(
            decoded.encode().as_ref(),
            frame.as_slice(),
            "a rotate request must round-trip its pinned wire bytes"
        );
    }

    #[test]
    fn test_server_leg_roundtrip() {
        let msg = ControlMessage::ServerLeg {
            server: Ipv4Addr::new(91, 199, 82, 166),
            median_us: 148_250,
            jitter_us: 3_500,
            samples: 4,
            age_ms: 1_200,
        };
        let encoded = msg.encode();
        // Type tag + IPv4 + u32 + u32 + u8 + u16 = 16 bytes.
        assert_eq!(encoded.len(), 16);
        assert_eq!(ControlMessage::decode(&encoded).unwrap(), msg);

        // Boundary values survive the fixed-width fields.
        for (median_us, jitter_us, samples, age_ms) in
            [(1, 0, 1, 0), (u32::MAX, u32::MAX, u8::MAX, u16::MAX)]
        {
            let msg = ControlMessage::ServerLeg {
                server: Ipv4Addr::UNSPECIFIED,
                median_us,
                jitter_us,
                samples,
                age_ms,
            };
            assert_eq!(ControlMessage::decode(&msg.encode()).unwrap(), msg);
        }
    }

    #[test]
    fn test_server_leg_wire_form_is_pinned_by_hardcoded_bytes() {
        // Hand-built frame: tag 0x0A, server 91.199.82.166, median 0x0002_4320
        // (148 256 µs), jitter 0x0000_0BB8 (3 000 µs), samples 3, age 0x001F
        // (31 ms). The bytes are literals on purpose - a reordered or resized
        // field fails here instead of silently decoding a different number.
        let frame: [u8; 16] = [
            0x0A, 91, 199, 82, 166, 0x00, 0x02, 0x43, 0x20, 0x00, 0x00, 0x0B, 0xB8, 3, 0x00, 0x1F,
        ];
        let decoded = ControlMessage::decode(&frame).unwrap();
        assert_eq!(
            decoded,
            ControlMessage::ServerLeg {
                server: Ipv4Addr::new(91, 199, 82, 166),
                median_us: 148_256,
                jitter_us: 3_000,
                samples: 3,
                age_ms: 31,
            }
        );
        assert_eq!(decoded.encode().as_ref(), frame.as_slice());
    }

    #[test]
    fn test_server_leg_truncated_and_malformed_payloads_are_rejected() {
        // Every shorter length is malformed: the fixed-width payload needs 15
        // bytes after the tag.
        for len in 0..15usize {
            let mut frame = vec![MSG_SERVER_LEG];
            frame.extend_from_slice(&vec![0u8; len]);
            assert!(
                matches!(
                    ControlMessage::decode(&frame),
                    Err(ControlDecodeError::BufferTooSmall { got, need: 15 }) if got == len
                ),
                "a {len}-byte server-leg payload must be rejected"
            );
        }
        // The full payload decodes.
        let mut frame = vec![MSG_SERVER_LEG];
        frame.extend_from_slice(&[0u8; 15]);
        assert!(ControlMessage::decode(&frame).is_ok());
    }

    #[test]
    fn test_server_leg_trailing_bytes_are_ignored_for_forward_compatibility() {
        // A later revision may append fields; a reader that predates them must
        // decode the frame it understands rather than dropping it.
        let mut frame = vec![MSG_SERVER_LEG];
        frame.extend_from_slice(&Ipv4Addr::new(8, 8, 8, 8).octets());
        frame.extend_from_slice(&12_000u32.to_be_bytes());
        frame.extend_from_slice(&40u32.to_be_bytes());
        frame.push(2);
        frame.extend_from_slice(&5u16.to_be_bytes());
        frame.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(
            ControlMessage::decode(&frame).unwrap(),
            ControlMessage::ServerLeg {
                server: Ipv4Addr::new(8, 8, 8, 8),
                median_us: 12_000,
                jitter_us: 40,
                samples: 2,
                age_ms: 5,
            }
        );
    }

    #[test]
    fn test_server_leg_tag_is_a_new_unknown_to_an_old_reader() {
        // Compatibility claim: a build without `ServerLeg` decodes tag 0x0A as
        // `UnknownMessageType` and ignores the frame. Pinned by asserting the
        // tag is distinct from every tag such a build already knew, and that
        // the unknown-tag path itself still behaves that way.
        assert_eq!(MSG_SERVER_LEG, 0x0A);
        for known in [
            MSG_PING,
            MSG_PONG,
            MSG_REGISTER,
            MSG_REGISTER_ACK,
            MSG_DISCONNECT,
            MSG_SERVER_INFO,
            MSG_TELEMETRY,
            MSG_ROTATE_REQUEST,
            MSG_ROTATE_ACK,
        ] {
            assert_ne!(
                MSG_SERVER_LEG, known,
                "the server-leg tag must not reuse an existing message type"
            );
        }

        let mut payload = vec![MSG_SERVER_LEG];
        payload.extend(std::iter::repeat_n(0u8, 15));
        let decoded = ControlMessage::decode(&payload).expect("a current reader decodes it");
        assert!(matches!(decoded, ControlMessage::ServerLeg { .. }));

        // An unknown tag is still unknown: nothing is defaulted or guessed.
        assert!(matches!(
            ControlMessage::decode(&[0x0E]),
            Err(ControlDecodeError::UnknownMessageType(0x0E))
        ));
    }

    #[test]
    fn test_server_leg_app_roundtrip() {
        let msg = ControlMessage::ServerLegApp {
            server: Ipv4Addr::new(91, 199, 82, 166),
            passive_median_us: 47_250,
            passive_samples: 12,
            age_ms: 900,
        };
        let encoded = msg.encode();
        // Type tag + IPv4 + u32 + u8 + u16 = 12 bytes.
        assert_eq!(encoded.len(), 12);
        assert_eq!(ControlMessage::decode(&encoded).unwrap(), msg);

        // Boundary values survive the fixed-width fields.
        for (passive_median_us, passive_samples, age_ms) in
            [(0, 0, 0), (u32::MAX, u8::MAX, u16::MAX)]
        {
            let msg = ControlMessage::ServerLegApp {
                server: Ipv4Addr::UNSPECIFIED,
                passive_median_us,
                passive_samples,
                age_ms,
            };
            assert_eq!(ControlMessage::decode(&msg.encode()).unwrap(), msg);
        }
    }

    #[test]
    fn test_server_leg_app_wire_form_is_pinned_by_hardcoded_bytes() {
        // Hand-built frame: tag 0x0C, server 91.199.82.166, passive median
        // 0x0000_B798 (47 000 µs), samples 9, age 0x012C (300 ms). The bytes
        // are literals on purpose - a reordered, resized or re-tagged field
        // fails here instead of silently decoding a different number.
        let frame: [u8; 12] = [
            0x0C, 91, 199, 82, 166, 0x00, 0x00, 0xB7, 0x98, 9, 0x01, 0x2C,
        ];
        let decoded = ControlMessage::decode(&frame).unwrap();
        assert_eq!(
            decoded,
            ControlMessage::ServerLegApp {
                server: Ipv4Addr::new(91, 199, 82, 166),
                passive_median_us: 47_000,
                passive_samples: 9,
                age_ms: 300,
            }
        );
        assert_eq!(decoded.encode().as_ref(), frame.as_slice());
    }

    #[test]
    fn test_server_leg_app_truncated_payloads_are_rejected() {
        // Every shorter length is malformed: the fixed-width payload needs 11
        // bytes after the tag.
        for len in 0..11usize {
            let mut frame = vec![MSG_SERVER_LEG_APP];
            frame.extend_from_slice(&vec![0u8; len]);
            assert!(
                matches!(
                    ControlMessage::decode(&frame),
                    Err(ControlDecodeError::BufferTooSmall { got, need: 11 }) if got == len
                ),
                "an {len}-byte server-leg-app payload must be rejected"
            );
        }
        // The full payload decodes.
        let mut frame = vec![MSG_SERVER_LEG_APP];
        frame.extend_from_slice(&[0u8; 11]);
        assert!(ControlMessage::decode(&frame).is_ok());
    }

    #[test]
    fn test_server_leg_app_trailing_bytes_are_ignored_for_forward_compatibility() {
        // A later revision may append fields; this reader must decode the frame
        // it understands rather than dropping it.
        let mut frame = vec![MSG_SERVER_LEG_APP];
        frame.extend_from_slice(&Ipv4Addr::new(8, 8, 8, 8).octets());
        frame.extend_from_slice(&47_000u32.to_be_bytes());
        frame.push(4);
        frame.extend_from_slice(&250u16.to_be_bytes());
        frame.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(
            ControlMessage::decode(&frame).unwrap(),
            ControlMessage::ServerLegApp {
                server: Ipv4Addr::new(8, 8, 8, 8),
                passive_median_us: 47_000,
                passive_samples: 4,
                age_ms: 250,
            }
        );
    }

    #[test]
    fn test_server_leg_app_tag_is_a_new_unknown_to_an_old_reader() {
        // Compatibility claim: a build without `ServerLegApp` decodes tag 0x0C
        // as `UnknownMessageType` and ignores the frame. Pinned by asserting
        // the tag is distinct from every tag such a build already knew, and
        // that the unknown-tag path itself still behaves that way.
        assert_eq!(MSG_SERVER_LEG_APP, 0x0C);
        for known in [
            MSG_PING,
            MSG_PONG,
            MSG_REGISTER,
            MSG_REGISTER_ACK,
            MSG_DISCONNECT,
            MSG_SERVER_INFO,
            MSG_TELEMETRY,
            MSG_ROTATE_REQUEST,
            MSG_ROTATE_ACK,
            MSG_SERVER_LEG,
            MSG_CLIENT_CAPS,
        ] {
            assert_ne!(
                MSG_SERVER_LEG_APP, known,
                "the passive server-leg tag must not reuse an existing message type"
            );
        }

        let mut payload = vec![MSG_SERVER_LEG_APP];
        payload.extend(std::iter::repeat_n(0u8, 11));
        let decoded = ControlMessage::decode(&payload).expect("a current reader decodes it");
        assert!(matches!(decoded, ControlMessage::ServerLegApp { .. }));

        // An unknown tag is still unknown: nothing is defaulted or guessed.
        assert!(matches!(
            ControlMessage::decode(&[0x0E]),
            Err(ControlDecodeError::UnknownMessageType(0x0E))
        ));
    }

    #[test]
    fn test_server_trace_roundtrip() {
        let msg = ControlMessage::ServerTrace {
            server: Ipv4Addr::new(91, 199, 82, 166),
            hops: 12,
            last_hop: Ipv4Addr::new(8, 8, 8, 8),
            last_hop_rtt_us: 14_250,
            flags: trace_flags::COMPLETE,
            age_ms: 900,
        };
        let encoded = msg.encode();
        // Type tag + IPv4 + u8 + IPv4 + u32 + u8 + u16 = 17 bytes.
        assert_eq!(encoded.len(), 17);
        assert_eq!(ControlMessage::decode(&encoded).unwrap(), msg);

        // Boundary values survive the fixed-width fields; the client applies
        // the plausibility rules, the codec only has to round-trip them.
        for (hops, last_hop, last_hop_rtt_us, flags, age_ms) in [
            (0, Ipv4Addr::UNSPECIFIED, 0, 0, 0),
            (
                MAX_SERVER_TRACE_HOPS,
                Ipv4Addr::new(203, 0, 113, 1),
                u32::MAX,
                u8::MAX,
                u16::MAX,
            ),
        ] {
            let msg = ControlMessage::ServerTrace {
                server: Ipv4Addr::UNSPECIFIED,
                hops,
                last_hop,
                last_hop_rtt_us,
                flags,
                age_ms,
            };
            assert_eq!(ControlMessage::decode(&msg.encode()).unwrap(), msg);
        }
    }

    #[test]
    fn test_server_trace_wire_form_is_pinned_by_hardcoded_bytes() {
        // Hand-built frame: tag 0x0D, server 91.199.82.166, hops 12, last hop
        // 8.8.8.8, last-hop RTT 0x0000_37AA (14 250 µs), flags 0x01 (complete),
        // age 0x0384 (900 ms). The bytes are literals on purpose - a reordered,
        // resized or re-tagged field fails here instead of silently decoding a
        // different hop.
        let frame: [u8; 17] = [
            0x0D, 91, 199, 82, 166, 12, 8, 8, 8, 8, 0x00, 0x00, 0x37, 0xAA, 0x01, 0x03, 0x84,
        ];
        let decoded = ControlMessage::decode(&frame).unwrap();
        assert_eq!(
            decoded,
            ControlMessage::ServerTrace {
                server: Ipv4Addr::new(91, 199, 82, 166),
                hops: 12,
                last_hop: Ipv4Addr::new(8, 8, 8, 8),
                last_hop_rtt_us: 14_250,
                flags: trace_flags::COMPLETE,
                age_ms: 900,
            }
        );
        assert_eq!(decoded.encode().as_ref(), frame.as_slice());
    }

    #[test]
    fn test_server_trace_truncated_payloads_are_rejected() {
        // Every shorter length is malformed: the fixed-width payload needs 16
        // bytes after the tag.
        for len in 0..16usize {
            let mut frame = vec![MSG_SERVER_TRACE];
            frame.extend_from_slice(&vec![0u8; len]);
            assert!(
                matches!(
                    ControlMessage::decode(&frame),
                    Err(ControlDecodeError::BufferTooSmall { got, need: 16 }) if got == len
                ),
                "a {len}-byte server-trace payload must be rejected"
            );
        }
        // The full payload decodes.
        let mut frame = vec![MSG_SERVER_TRACE];
        frame.extend_from_slice(&[0u8; 16]);
        assert!(ControlMessage::decode(&frame).is_ok());
    }

    #[test]
    fn test_server_trace_trailing_bytes_are_ignored_for_forward_compatibility() {
        // A later revision may append fields; this reader must decode the frame
        // it understands rather than dropping it.
        let mut frame = vec![MSG_SERVER_TRACE];
        frame.extend_from_slice(&Ipv4Addr::new(198, 51, 100, 9).octets());
        frame.push(7);
        frame.extend_from_slice(&Ipv4Addr::new(203, 0, 113, 1).octets());
        frame.extend_from_slice(&3_250u32.to_be_bytes());
        frame.push(trace_flags::COMPLETE);
        frame.extend_from_slice(&42u16.to_be_bytes());
        frame.extend_from_slice(&[0xDE, 0xAD, 0xBE, 0xEF]);
        assert_eq!(
            ControlMessage::decode(&frame).unwrap(),
            ControlMessage::ServerTrace {
                server: Ipv4Addr::new(198, 51, 100, 9),
                hops: 7,
                last_hop: Ipv4Addr::new(203, 0, 113, 1),
                last_hop_rtt_us: 3_250,
                flags: trace_flags::COMPLETE,
                age_ms: 42,
            }
        );
    }

    #[test]
    fn test_server_trace_tag_is_a_new_unknown_to_an_old_reader() {
        // Compatibility claim: a build without `ServerTrace` decodes tag 0x0D
        // as `UnknownMessageType` and ignores the frame. Pinned by asserting
        // the tag is distinct from every tag such a build already knew, and
        // that the unknown-tag path itself still behaves that way.
        assert_eq!(MSG_SERVER_TRACE, 0x0D);
        for known in [
            MSG_PING,
            MSG_PONG,
            MSG_REGISTER,
            MSG_REGISTER_ACK,
            MSG_DISCONNECT,
            MSG_SERVER_INFO,
            MSG_TELEMETRY,
            MSG_ROTATE_REQUEST,
            MSG_ROTATE_ACK,
            MSG_SERVER_LEG,
            MSG_CLIENT_CAPS,
            MSG_SERVER_LEG_APP,
        ] {
            assert_ne!(
                MSG_SERVER_TRACE, known,
                "the server-trace tag must not reuse an existing message type"
            );
        }

        let mut payload = vec![MSG_SERVER_TRACE];
        payload.extend(std::iter::repeat_n(0u8, 16));
        let decoded = ControlMessage::decode(&payload).expect("a current reader decodes it");
        assert!(matches!(decoded, ControlMessage::ServerTrace { .. }));

        // A tag after this one is still unknown: nothing is defaulted or guessed.
        assert!(matches!(
            ControlMessage::decode(&[0x0E]),
            Err(ControlDecodeError::UnknownMessageType(0x0E))
        ));
    }

    #[test]
    fn test_server_trace_constants_are_pinned() {
        // The hop ceiling is a shared bound: the relay stops at it and the
        // client refuses anything above it. The complete bit is the only flag
        // defined so far.
        assert_eq!(MAX_SERVER_TRACE_HOPS, 20);
        assert_eq!(trace_flags::COMPLETE, 0b0000_0001);
    }

    #[test]
    fn test_client_caps_roundtrip_and_legacy_tolerance() {
        let msg = ControlMessage::ClientCaps {
            caps: caps::SERVER_LEG,
        };
        let encoded = msg.encode();
        assert_eq!(
            encoded,
            Bytes::from_static(&[MSG_CLIENT_CAPS, caps::SERVER_LEG])
        );
        assert_eq!(ControlMessage::decode(&encoded).unwrap(), msg);

        // The caps bitmap is additive: a client may advertise only what it
        // knows, and the zero bitmap is well-formed.
        for bitmap in [0u8, caps::TCP_TUNNEL, u8::MAX] {
            let msg = ControlMessage::ClientCaps { caps: bitmap };
            assert_eq!(ControlMessage::decode(&msg.encode()).unwrap(), msg);
        }

        // A relay that predates the message sees an unknown type and drops the
        // stream without touching the connection; the tag must not reuse one of
        // the tags it already knows.
        assert_eq!(MSG_CLIENT_CAPS, 0x0B);
        for known in [
            MSG_PING,
            MSG_PONG,
            MSG_REGISTER,
            MSG_REGISTER_ACK,
            MSG_DISCONNECT,
            MSG_SERVER_INFO,
            MSG_TELEMETRY,
            MSG_ROTATE_REQUEST,
            MSG_ROTATE_ACK,
            MSG_SERVER_LEG,
            MSG_SERVER_LEG_APP,
        ] {
            assert_ne!(MSG_CLIENT_CAPS, known);
        }
        assert!(matches!(
            ControlMessage::decode(&[0x0E]),
            Err(ControlDecodeError::UnknownMessageType(0x0E))
        ));
    }

    #[test]
    fn test_client_caps_rejects_a_truncated_payload() {
        assert!(matches!(
            ControlMessage::decode(&[MSG_CLIENT_CAPS]),
            Err(ControlDecodeError::BufferTooSmall { got: 0, need: 1 })
        ));
        assert!(ControlMessage::decode(&[MSG_CLIENT_CAPS, 0]).is_ok());
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
        assert_eq!(game_id::GAME_IDS.len(), 28, "every real game needs one id");

        // Each id 1..=28 must appear exactly once (0 stays reserved for UNKNOWN).
        let mut seen = [0u8; 29];
        for (key, id) in game_id::GAME_IDS.iter().copied() {
            assert!(
                (1..=28).contains(&id),
                "key {key:?} has out-of-range id {id}"
            );
            assert_eq!(seen[id as usize], 0, "duplicate id {id}");
            seen[id as usize] += 1;
        }
        for id in 1..=28u8 {
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
        assert_eq!(game_id::MINECRAFT, 20);
        assert_eq!(game_id::HUNT, 21);
        assert_eq!(game_id::MINECRAFT_JAVA, 22);
        assert_eq!(game_id::RAINBOWSIX, 23);
        assert_eq!(game_id::ARCRAIDERS, 24);
        assert_eq!(game_id::JEDIOUTCAST, 25);
        assert_eq!(game_id::TIBIA, 26);
        assert_eq!(game_id::ROBRUN, 27);
        assert_eq!(game_id::FC27, 28);
    }

    #[test]
    fn test_id_for_key_roundtrip() {
        for (key, id) in game_id::GAME_IDS.iter().copied() {
            assert_eq!(game_id::id_for_key(key), id, "id_for_key({key:?})");
            assert_eq!(game_id::key_for_id(id), Some(key), "key_for_id({id})");
        }
        assert_eq!(
            game_id::id_for_key("minecraft-java"),
            game_id::MINECRAFT_JAVA
        );
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

    #[test]
    fn control_alpn_name_is_stable() {
        // A rename is a wire break with no compile error on either side: the
        // client offer and the relay's required list are separate builds, so
        // only this byte-for-byte assertion keeps them in step.
        assert_eq!(QUIC_CONTROL_ALPN_V1, b"lightspeed-control/1");
        assert_eq!(
            quic_control_alpn(),
            vec![b"lightspeed-control/1".to_vec()],
            "the offer is the versioned name, most preferred first"
        );
    }

    #[test]
    fn control_alpn_name_is_rfc7301_conformant() {
        // RFC 7301 §3.1: ProtocolName is opaque<1..2^8-1>.
        let offer = quic_control_alpn();
        let names = std::iter::once(QUIC_CONTROL_ALPN_V1).chain(offer.iter().map(Vec::as_slice));
        for name in names {
            assert!(!name.is_empty(), "an ALPN name must not be empty");
            assert!(name.len() <= 255, "an ALPN name is at most 255 bytes");
            assert!(
                name.iter()
                    .all(|byte| byte.is_ascii_graphic() || *byte == b' '),
                "the name must be printable ASCII, got {name:?}"
            );
            assert!(
                name.iter().all(|byte| *byte != 0),
                "the name must not contain a NUL"
            );
        }
    }
}
