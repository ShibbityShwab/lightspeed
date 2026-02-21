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
//! |  Ver  | Flags |   Reserved    |         Sequence Number       |
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

use bytes::{Buf, BufMut, Bytes, BytesMut};
use std::net::{Ipv4Addr, SocketAddrV4};
use thiserror::Error;

/// Current protocol version.
pub const PROTOCOL_VERSION: u8 = 1;

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
    /// Reserved byte (must be 0).
    pub reserved: u8,
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
    pub fn new(
        sequence: u16,
        timestamp_us: u32,
        src: SocketAddrV4,
        dst: SocketAddrV4,
    ) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            flags: 0,
            reserved: 0,
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
            reserved: 0,
            sequence,
            timestamp_us,
            orig_src_ip: Ipv4Addr::UNSPECIFIED,
            orig_dst_ip: Ipv4Addr::UNSPECIFIED,
            orig_src_port: 0,
            orig_dst_port: 0,
        }
    }

    /// Encode the header into bytes.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::with_capacity(HEADER_SIZE);

        // Byte 0: version (high nibble) | flags (low nibble)
        buf.put_u8((self.version << 4) | (self.flags & 0x0F));
        // Byte 1: reserved
        buf.put_u8(self.reserved);
        // Bytes 2-3: sequence number
        buf.put_u16(self.sequence);
        // Bytes 4-7: timestamp
        buf.put_u32(self.timestamp_us);
        // Bytes 8-11: original source IP
        buf.put_slice(&self.orig_src_ip.octets());
        // Bytes 12-15: original dest IP
        buf.put_slice(&self.orig_dst_ip.octets());
        // Bytes 16-17: original source port
        buf.put_u16(self.orig_src_port);
        // Bytes 18-19: original dest port
        buf.put_u16(self.orig_dst_port);

        buf.freeze()
    }

    /// Encode the header + payload into a single buffer.
    pub fn encode_with_payload(&self, payload: &[u8]) -> Bytes {
        let mut buf = BytesMut::with_capacity(HEADER_SIZE + payload.len());

        // Encode header
        buf.put_u8((self.version << 4) | (self.flags & 0x0F));
        buf.put_u8(self.reserved);
        buf.put_u16(self.sequence);
        buf.put_u32(self.timestamp_us);
        buf.put_slice(&self.orig_src_ip.octets());
        buf.put_slice(&self.orig_dst_ip.octets());
        buf.put_u16(self.orig_src_port);
        buf.put_u16(self.orig_dst_port);

        // Append payload
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

        if version != PROTOCOL_VERSION {
            return Err(DecodeError::UnsupportedVersion {
                version,
                expected: PROTOCOL_VERSION,
            });
        }

        let reserved = buf.get_u8();
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
            reserved,
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

    /// Get the original source socket address.
    pub fn orig_src_addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.orig_src_ip, self.orig_src_port)
    }

    /// Get the original destination socket address.
    pub fn orig_dst_addr(&self) -> SocketAddrV4 {
        SocketAddrV4::new(self.orig_dst_ip, self.orig_dst_port)
    }

    /// Create a response header by swapping source and destination.
    pub fn make_response(&self, sequence: u16, timestamp_us: u32) -> Self {
        Self {
            version: PROTOCOL_VERSION,
            flags: 0,
            reserved: 0,
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
        let header = TunnelHeader::new(1, 1000, src, dst);

        let response = header.make_response(2, 2000);
        assert_eq!(response.orig_src_ip, dst.ip().clone());
        assert_eq!(response.orig_dst_ip, src.ip().clone());
        assert_eq!(response.orig_src_port, dst.port());
        assert_eq!(response.orig_dst_port, src.port());
    }
}
