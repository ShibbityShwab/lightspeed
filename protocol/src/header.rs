//! # LightSpeed Tunnel Header
//!
//! Encode/decode the LightSpeed tunnel protocol header.
//! This header wraps original game UDP packets for transit through proxy nodes.
//!
//! ## Header Format (20 bytes)
//!
//! ```text
//!  0                   1                   2                   3
//!  0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |  Ver  | Flags | Session Token |         Sequence Number       |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                      Timestamp (μs)                           |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                    Original Source IP                          |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |                   Original Dest IP                            |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! |   Orig SrcPort  |   Orig DstPort  |
//! +-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
//! ```
//!
//! ## Session Token (Security)
//!
//! The session token byte is assigned by the proxy during QUIC registration.
//! Clients MUST include their assigned token in every data-plane packet.
//! The proxy validates this per-packet to prevent unauthorized relay usage.
//! While only 8 bits, it provides defense-in-depth alongside IP-based auth.

use bytes::{Buf, BufMut, Bytes, BytesMut};
// Note: BufMut import kept for encode_with_payload; Buf for decode.
use std::net::{Ipv4Addr, SocketAddrV4};
use thiserror::Error;

/// Current protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

/// Protocol version indicating FEC header follows the tunnel header.
/// When a packet uses version 2, a 4-byte FEC header is present
/// immediately after the 20-byte tunnel header.
pub const PROTOCOL_VERSION_FEC: u8 = 2;

/// Header size in bytes.
pub const HEADER_SIZE: usize = 20;

/// Maximum payload size (MTU - IP header - UDP header - tunnel header).
pub const MAX_PAYLOAD_SIZE: usize = 1400 - 20 - 8 - HEADER_SIZE;

/// Header flags.
pub mod flags {
    /// Keepalive packet (no payload).
    pub const KEEPALIVE: u8 = 0b0000_0001;
    /// Handshake packet.
    pub const HANDSHAKE: u8 = 0b0000_0010;
    /// Fin — close tunnel gracefully.
    pub const FIN: u8 = 0b0000_0100;
    /// Fragment — packet is part of a fragmented message.
    pub const FRAGMENT: u8 = 0b0000_1000;
    // NOTE: Bits 4-7 are reserved for future use. FEC uses a separate
    // header extension signaled via the version/type field, not flags,
    // to maintain backward compatibility with v1 proxies.
}

/// Errors from decoding a tunnel header.
#[derive(Error, Debug)]
pub enum DecodeError {
    #[error("Buffer too small: got {got} bytes, need {need}")]
    BufferTooSmall { got: usize, need: usize },

    #[error("Unsupported protocol version: {version} (expected {expected})")]
    UnsupportedVersion { version: u8, expected: u8 },
}

/// The LightSpeed tunnel header.
#[derive(Debug, Clone, PartialEq)]
pub struct TunnelHeader {
    /// Protocol version (4 bits, currently 1).
    pub version: u8,
    /// Header flags (4 bits).
    pub flags: u8,
    /// Session token assigned by proxy during QUIC registration.
    /// Used for per-packet authentication on the data plane.
    pub session_token: u8,
    /// Packet sequence number (for ordering and dedup).
    pub sequence: u16,
    /// Timestamp in microseconds (for latency measurement).
    pub timestamp_us: u32,
    /// Original source IPv4 address.
    pub orig_src_ip: Ipv4Addr,
    /// Original destination IPv4 address.
    pub orig_dst_ip: Ipv4Addr,
    /// Original source port.
    pub orig_src_port: u16,
    /// Original destination port.
    pub orig_dst_port: u16,
}

impl TunnelHeader {
    /// Create a new tunnel header for a data packet.
    pub fn new(sequence: u16, timestamp_us: u32, src: SocketAddrV4, dst: SocketAddrV4) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            flags: 0,
            session_token: 0,
            sequence,
            timestamp_us,
            orig_src_ip: *src.ip(),
            orig_dst_ip: *dst.ip(),
            orig_src_port: src.port(),
            orig_dst_port: dst.port(),
        }
    }

    /// Create a new tunnel header with FEC enabled (version 2).
    /// The caller must append a 4-byte FEC header after the tunnel header.
    pub fn new_fec(sequence: u16, timestamp_us: u32, src: SocketAddrV4, dst: SocketAddrV4) -> Self {
        Self {
            version: PROTOCOL_VERSION_FEC,
            flags: 0,
            session_token: 0,
            sequence,
            timestamp_us,
            orig_src_ip: *src.ip(),
            orig_dst_ip: *dst.ip(),
            orig_src_port: src.port(),
            orig_dst_port: dst.port(),
        }
    }

    /// Create a keepalive header.
    pub fn keepalive(sequence: u16, timestamp_us: u32) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            flags: flags::KEEPALIVE,
            session_token: 0,
            sequence,
            timestamp_us,
            orig_src_ip: Ipv4Addr::UNSPECIFIED,
            orig_dst_ip: Ipv4Addr::UNSPECIFIED,
            orig_src_port: 0,
            orig_dst_port: 0,
        }
    }

    /// Set the session token (builder pattern).
    ///
    /// The token is assigned by the proxy during QUIC registration
    /// and must be included in every subsequent data-plane packet.
    pub fn with_session_token(mut self, token: u8) -> Self {
        self.session_token = token;
        self
    }

    /// Encode the header into a stack-allocated `[u8; 20]` array — **zero heap allocation**.
    ///
    /// This is the hot-path encoding method. Use it whenever you are about to
    /// extend a `BytesMut` with the header bytes:
    ///
    /// ```ignore
    /// buf.extend_from_slice(&header.encode_to_array());
    /// ```
    ///
    /// For direct `socket.send_to` of a header-only packet (e.g., keepalive),
    /// the array coerces directly to `&[u8]`:
    ///
    /// ```ignore
    /// socket.send_to(&header.encode_to_array(), addr).await?;
    /// ```
    #[inline]
    pub fn encode_to_array(&self) -> [u8; HEADER_SIZE] {
        let mut buf = [0u8; HEADER_SIZE];
        // Byte 0: version (high nibble) | flags (low nibble)
        buf[0] = (self.version << 4) | (self.flags & 0x0F);
        // Byte 1: session token
        buf[1] = self.session_token;
        // Bytes 2-3: sequence number (big-endian)
        let seq = self.sequence.to_be_bytes();
        buf[2] = seq[0];
        buf[3] = seq[1];
        // Bytes 4-7: timestamp (big-endian)
        let ts = self.timestamp_us.to_be_bytes();
        buf[4] = ts[0];
        buf[5] = ts[1];
        buf[6] = ts[2];
        buf[7] = ts[3];
        // Bytes 8-11: original source IP
        let src = self.orig_src_ip.octets();
        buf[8]  = src[0];
        buf[9]  = src[1];
        buf[10] = src[2];
        buf[11] = src[3];
        // Bytes 12-15: original dest IP
        let dst = self.orig_dst_ip.octets();
        buf[12] = dst[0];
        buf[13] = dst[1];
        buf[14] = dst[2];
        buf[15] = dst[3];
        // Bytes 16-17: original source port (big-endian)
        let sp = self.orig_src_port.to_be_bytes();
        buf[16] = sp[0];
        buf[17] = sp[1];
        // Bytes 18-19: original dest port (big-endian)
        let dp = self.orig_dst_port.to_be_bytes();
        buf[18] = dp[0];
        buf[19] = dp[1];
        buf
    }

    /// Encode the header into a heap-allocated `Bytes`.
    ///
    /// **Prefer `encode_to_array()`** in hot paths — it avoids the allocation.
    /// This method is kept for callers that need a `Bytes` return type (e.g.,
    /// legacy code, tests, or cases where `Bytes` is required by an API).
    pub fn encode(&self) -> Bytes {
        Bytes::copy_from_slice(&self.encode_to_array())
    }

    /// Encode the header + payload into a single heap-allocated buffer.
    ///
    /// Performs exactly **one allocation** of `HEADER_SIZE + payload.len()` bytes.
    pub fn encode_with_payload(&self, payload: &[u8]) -> Bytes {
        let mut buf = BytesMut::with_capacity(HEADER_SIZE + payload.len());
        buf.put_slice(&self.encode_to_array());
        buf.put_slice(payload);
        buf.freeze()
    }

    /// Decode a header from bytes.
    pub fn decode(data: &[u8]) -> Result<Self, DecodeError> {
        if data.len() < HEADER_SIZE {
            return Err(DecodeError::BufferTooSmall {
                got: data.len(),
                need: HEADER_SIZE,
            });
        }

        let mut buf = &data[..HEADER_SIZE];

        let ver_flags = buf.get_u8();
        let version = ver_flags >> 4;
        let flags = ver_flags & 0x0F;

        if version != PROTOCOL_VERSION && version != PROTOCOL_VERSION_FEC {
            return Err(DecodeError::UnsupportedVersion {
                version,
                expected: PROTOCOL_VERSION,
            });
        }

        let session_token = buf.get_u8();
        let sequence = buf.get_u16();
        let timestamp_us = buf.get_u32();

        let mut ip_bytes = [0u8; 4];
        buf.copy_to_slice(&mut ip_bytes);
        let orig_src_ip = Ipv4Addr::from(ip_bytes);

        buf.copy_to_slice(&mut ip_bytes);
        let orig_dst_ip = Ipv4Addr::from(ip_bytes);

        let orig_src_port = buf.get_u16();
        let orig_dst_port = buf.get_u16();

        Ok(Self {
            version,
            flags,
            session_token,
            sequence,
            timestamp_us,
            orig_src_ip,
            orig_dst_ip,
            orig_src_port,
            orig_dst_port,
        })
    }

    /// Decode a header and return the remaining payload slice.
    pub fn decode_with_payload(data: &[u8]) -> Result<(Self, &[u8]), DecodeError> {
        let header = Self::decode(data)?;
        let payload = &data[HEADER_SIZE..];
        Ok((header, payload))
    }

    /// Check if this is a keepalive packet.
    pub fn is_keepalive(&self) -> bool {
        self.flags & flags::KEEPALIVE != 0
    }

    /// Check if this is a handshake packet.
    pub fn is_handshake(&self) -> bool {
        self.flags & flags::HANDSHAKE != 0
    }

    /// Check if this is a fin (close) packet.
    pub fn is_fin(&self) -> bool {
        self.flags & flags::FIN != 0
    }

    /// Check if this packet has an FEC header extension (version 2).
    pub fn has_fec(&self) -> bool {
        self.version == PROTOCOL_VERSION_FEC
    }

    /// Get the original source socket address.
    pub fn orig_src_addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.orig_src_ip, self.orig_src_port)
    }

    /// Get the original destination socket address.
    pub fn orig_dst_addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.orig_dst_ip, self.orig_dst_port)
    }

    /// Create a response header by swapping source and destination.
    /// Preserves the protocol version (v1 or v2/FEC).
    pub fn make_response(&self, sequence: u16, timestamp_us: u32) -> Self {
        Self {
            version: self.version,
            flags: 0,
            session_token: self.session_token,
            sequence,
            timestamp_us,
            orig_src_ip: self.orig_dst_ip,
            orig_dst_ip: self.orig_src_ip,
            orig_src_port: self.orig_dst_port,
            orig_dst_port: self.orig_src_port,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_encode_decode_roundtrip() {
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let header = TunnelHeader::new(42, 1_000_000, src, dst);

        let encoded = header.encode();
        assert_eq!(encoded.len(), HEADER_SIZE);

        let decoded = TunnelHeader::decode(&encoded).unwrap();
        assert_eq!(header, decoded);
    }

    #[test]
    fn test_header_with_session_token() {
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let header = TunnelHeader::new(1, 500_000, src, dst).with_session_token(0xAB);

        assert_eq!(header.session_token, 0xAB);

        let encoded = header.encode();
        let decoded = TunnelHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.session_token, 0xAB);
        assert_eq!(header, decoded);
    }

    #[test]
    fn test_header_version_check() {
        let mut data = vec![0u8; HEADER_SIZE];
        data[0] = 0xF0; // Version 15 — invalid
        let result = TunnelHeader::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_header_too_short() {
        let data = vec![0u8; 10]; // Too short
        let result = TunnelHeader::decode(&data);
        assert!(result.is_err());
    }

    #[test]
    fn test_keepalive_header() {
        let header = TunnelHeader::keepalive(1, 500_000);
        assert!(header.is_keepalive());
        assert!(!header.is_handshake());
        assert!(!header.is_fin());
        assert_eq!(header.session_token, 0);
    }

    #[test]
    fn test_keepalive_with_token() {
        let header = TunnelHeader::keepalive(1, 500_000).with_session_token(42);
        assert!(header.is_keepalive());
        assert_eq!(header.session_token, 42);

        let encoded = header.encode();
        let decoded = TunnelHeader::decode(&encoded).unwrap();
        assert_eq!(decoded.session_token, 42);
        assert!(decoded.is_keepalive());
    }

    #[test]
    fn test_flags() {
        let src = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1000);
        let dst = SocketAddrV4::new(Ipv4Addr::LOCALHOST, 2000);
        let mut header = TunnelHeader::new(0, 0, src, dst);

        header.flags = flags::FIN;
        assert!(header.is_fin());

        header.flags = flags::HANDSHAKE;
        assert!(header.is_handshake());
    }

    #[test]
    fn test_encode_with_payload() {
        let src = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 5000);
        let dst = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 6000);
        let header = TunnelHeader::new(1, 100, src, dst);
        let payload = b"hello game data";

        let encoded = header.encode_with_payload(payload);
        assert_eq!(encoded.len(), HEADER_SIZE + payload.len());

        let (decoded, decoded_payload) = TunnelHeader::decode_with_payload(&encoded).unwrap();
        assert_eq!(header, decoded);
        assert_eq!(decoded_payload, payload);
    }

    #[test]
    fn test_make_response() {
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);
        let header = TunnelHeader::new(1, 1000, src, dst).with_session_token(99);

        let response = header.make_response(2, 2000);
        assert_eq!(response.orig_src_ip, *dst.ip());
        assert_eq!(response.orig_dst_ip, *src.ip());
        assert_eq!(response.orig_src_port, dst.port());
        assert_eq!(response.orig_dst_port, src.port());
        // Response should preserve session token
        assert_eq!(response.session_token, 99);
    }
}
