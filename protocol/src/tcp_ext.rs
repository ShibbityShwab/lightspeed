//! # TCP Extension Header (Protocol Version 5)
//!
//! The sequenced-reliable delivery layer that rides inside the existing UDP
//! tunnel for TCP-terminated games (first target: Minecraft Java Edition).
//!
//! A v5 packet is `TunnelHeader` (24 B) + `FecHeader` (4 B) + `TcpExtHeader`
//! (12 B) + game stream payload. FEC stays underneath the reliable layer and
//! treats the `TcpExtHeader` as opaque protected bytes; anything FEC cannot
//! recover is retransmitted by [`TcpReliable`].
//!
//! TCP-in-TCP is forbidden: `TcpExtHeader` packets must never ride a
//! `TunnelTransport::Tcp` leg, because nesting two congestion controllers and
//! two retransmission loops amplifies loss into a retransmission storm.

use std::collections::{BTreeMap, VecDeque};
use std::net::Ipv4Addr;
use std::time::{Duration, Instant};

use bytes::{Buf, BufMut, Bytes, BytesMut};

use crate::fec::{FecHeader, FEC_HEADER_SIZE};
use crate::header::{TunnelHeader, HEADER_SIZE};

/// Size of the TCP extension header in bytes.
pub const TCP_EXT_HEADER_SIZE: usize = 12;

/// Maximum number of segments the sender keeps unacknowledged in flight and
/// the receiver keeps buffered out-of-order. This bounds per-connection memory
/// (64 x ~1.4 KB <= ~92 KB).
pub const TCP_RETRANSMIT_WINDOW: usize = 64;

/// Flags byte inside [`TcpExtHeader`].
pub mod tcp_flags {
    /// Synchronize: opens a connection.
    pub const SYN: u8 = 0x01;
    /// Acknowledge: the `tcp_ack` field is significant.
    pub const ACK: u8 = 0x02;
    /// Finish: no more data from the sender.
    pub const FIN: u8 = 0x04;
    /// Reset: tear the connection down immediately.
    pub const RST: u8 = 0x08;
    /// Push: deliver the payload immediately.
    pub const PSH: u8 = 0x10;
}

/// The 12-byte TCP extension header carried immediately after [`FecHeader`] in
/// every v5 packet.
///
/// Wire layout (all numerics big-endian): `tcp_seq` u32, `tcp_ack` u32,
/// `flags` u8, `window` u8, `reserved` 2 bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TcpExtHeader {
    /// Per-connection, per-direction stream sequence number, counting stream
    /// bytes per RFC 793 (SYN consumes sequence 1; the first data byte is 2).
    pub tcp_seq: u32,
    /// Cumulative ACK for the opposite direction.
    pub tcp_ack: u32,
    /// [`tcp_flags`] bits.
    pub flags: u8,
    /// Reserved for flow control (zero in v1).
    pub window: u8,
    /// Reserved, must be zero on the wire.
    pub reserved: [u8; 2],
}

impl TcpExtHeader {
    /// Append the header to `buf` (exactly [`TCP_EXT_HEADER_SIZE`] bytes).
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u32(self.tcp_seq);
        buf.put_u32(self.tcp_ack);
        buf.put_u8(self.flags);
        buf.put_u8(self.window);
        buf.put_slice(&self.reserved);
    }

    /// Decode a header from the front of `buf`, advancing it by 12 bytes.
    pub fn decode(buf: &mut &[u8]) -> Option<Self> {
        if buf.remaining() < TCP_EXT_HEADER_SIZE {
            return None;
        }
        let tcp_seq = buf.get_u32();
        let tcp_ack = buf.get_u32();
        let flags = buf.get_u8();
        let window = buf.get_u8();
        let mut reserved = [0u8; 2];
        buf.copy_to_slice(&mut reserved);
        Some(Self {
            tcp_seq,
            tcp_ack,
            flags,
            window,
            reserved,
        })
    }
}

/// A connection's endpoints: the game's source and destination address/port.
///
/// The game's ephemeral source port disambiguates concurrent connections, so
/// the four fields together form the connection key. The tunnel is per-client,
/// so the client's UDP source port is deliberately not part of the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct Tcp5Tuple {
    pub src_ip: Ipv4Addr,
    pub src_port: u16,
    pub dst_ip: Ipv4Addr,
    pub dst_port: u16,
}

/// Build a v5 data packet: `[TunnelHeader][FecHeader][TcpExtHeader][payload]`.
pub fn build_tcp_packet(
    header: &TunnelHeader,
    fec: &FecHeader,
    tcp_ext: &TcpExtHeader,
    payload: &[u8],
) -> BytesMut {
    let mut buf = BytesMut::with_capacity(
        HEADER_SIZE + FEC_HEADER_SIZE + TCP_EXT_HEADER_SIZE + payload.len(),
    );
    buf.extend_from_slice(&header.encode_to_array());
    fec.encode(&mut buf);
    tcp_ext.encode(&mut buf);
    buf.extend_from_slice(payload);
    buf
}

/// Split a v5 payload slice (the bytes after the `FecHeader`) into the TCP
/// extension header and the remaining game stream payload.
pub fn decode_tcp_payload(payload_slice: &[u8]) -> Option<(TcpExtHeader, &[u8])> {
    let mut buf: &[u8] = payload_slice;
    let tcp_ext = TcpExtHeader::decode(&mut buf)?;
    Some((tcp_ext, buf))
}

/// Derive the client-local SYN-ACK ISN (a SYN cookie) for a connection.
///
/// `HMAC-SHA256(key_epoch, src_ip || src_port || dst_ip || dst_port)`
/// truncated to 32 bits. Deterministic per 5-tuple and key epoch, so the
/// terminator can validate the game's handshake ACK with zero half-open state.
pub fn derive_syn_cookie(key: &[u8; 32], tuple: &Tcp5Tuple) -> u32 {
    let mut message = [0u8; 12];
    message[0..4].copy_from_slice(&tuple.src_ip.octets());
    message[4..6].copy_from_slice(&tuple.src_port.to_be_bytes());
    message[6..10].copy_from_slice(&tuple.dst_ip.octets());
    message[10..12].copy_from_slice(&tuple.dst_port.to_be_bytes());
    let mac = hmac_sha256(key, &message);
    u32::from_be_bytes([mac[0], mac[1], mac[2], mac[3]])
}

// ────────────────────────────────────────────────────────────
// Sequenced-reliable delivery
// ────────────────────────────────────────────────────────────

const INITIAL_RTO: Duration = Duration::from_millis(200);
const MAX_RTO: Duration = Duration::from_secs(15);
const FAST_RETRANSMIT_THRESHOLD: u32 = 3;

/// True when `a` is strictly after `b` in the u32 sequence-number space.
#[inline]
fn seq_after(a: u32, b: u32) -> bool {
    a != b && (a.wrapping_sub(b) as i32) > 0
}

/// True when `a` is at or after `b` in the u32 sequence-number space.
#[inline]
fn seq_ge(a: u32, b: u32) -> bool {
    (a.wrapping_sub(b) as i32) >= 0
}

/// A data segment queued on the sender side, awaiting acknowledgment.
#[derive(Debug, Clone)]
struct PendingSegment {
    seq: u32,
    payload: Bytes,
    sent_at: Instant,
    retransmits: u32,
}

/// Result of feeding a cumulative ACK into the sender.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AckProgress {
    /// The ACK advanced past prior ACKs: acknowledged segments are dropped
    /// and the RTO is reset.
    New,
    /// A duplicate ACK: after three of them the sender fast-retransmits.
    Duplicate,
}

/// Encode-side half of the reliability layer: the retransmit window, RTO
/// exponential backoff, and fast retransmit.
///
/// The sender keeps up to [`TCP_RETRANSMIT_WINDOW`] unacknowledged segments
/// keyed by `tcp_seq`. A cumulative ACK slides the window; an RTO starting at
/// 200 ms with a 15 s cap drives retransmission, and three duplicate ACKs fire
/// a fast retransmit.
#[derive(Debug)]
pub struct TcpReliableSender {
    inflight: VecDeque<PendingSegment>,
    next_seq: u32,
    rto: Duration,
    last_ack: u32,
    duplicate_acks: u32,
}

impl TcpReliableSender {
    /// Create a sender whose next data byte has sequence `next_seq`. The caller
    /// initialises this per RFC 793 (SYN consumes sequence 1, so data typically
    /// starts at 2).
    pub fn new(next_seq: u32) -> Self {
        Self {
            inflight: VecDeque::new(),
            next_seq,
            rto: INITIAL_RTO,
            last_ack: 0,
            duplicate_acks: 0,
        }
    }

    /// Number of unacknowledged segments currently in flight.
    pub fn inflight(&self) -> usize {
        self.inflight.len()
    }

    /// Whether another segment can be queued without exceeding the window.
    pub fn can_send(&self) -> bool {
        self.inflight.len() < TCP_RETRANSMIT_WINDOW
    }

    /// Queue a payload and return the `tcp_seq` it will carry. Returns `None`
    /// when the in-flight window is full.
    pub fn send(&mut self, payload: &[u8]) -> Option<u32> {
        if !self.can_send() {
            return None;
        }
        let seq = self.next_seq;
        self.next_seq = self.next_seq.wrapping_add(payload.len() as u32);
        self.inflight.push_back(PendingSegment {
            seq,
            payload: Bytes::copy_from_slice(payload),
            sent_at: Instant::now(),
            retransmits: 0,
        });
        Some(seq)
    }

    /// Feed a cumulative ACK. Drops acknowledged segments and resets the RTO
    /// on new progress; counts duplicate ACKs toward fast retransmit.
    pub fn on_ack(&mut self, ack: u32) -> AckProgress {
        if seq_after(ack, self.last_ack) {
            self.last_ack = ack;
            self.duplicate_acks = 0;
            self.rto = INITIAL_RTO;
            while let Some(front) = self.inflight.front() {
                let end = front.seq.wrapping_add(front.payload.len() as u32);
                if seq_ge(ack, end) {
                    self.inflight.pop_front();
                } else {
                    break;
                }
            }
            AckProgress::New
        } else {
            self.duplicate_acks += 1;
            AckProgress::Duplicate
        }
    }

    /// The segment to retransmit right now, if any: the oldest unacknowledged
    /// segment once the RTO has elapsed, or on three duplicate ACKs. Applies
    /// RTO exponential backoff and consumes fast-retransmit pressure.
    pub fn poll_retransmit(&mut self, now: Instant) -> Option<(u32, Bytes)> {
        let oldest = self.inflight.front()?;
        let fast = self.duplicate_acks >= FAST_RETRANSMIT_THRESHOLD;
        let timeout = now.duration_since(oldest.sent_at) >= self.rto;
        if !fast && !timeout {
            return None;
        }

        let (seq, payload) = (oldest.seq, oldest.payload.clone());
        if fast {
            self.duplicate_acks = 0;
        } else {
            self.rto = (self.rto * 2).min(MAX_RTO);
        }
        if let Some(front) = self.inflight.front_mut() {
            front.sent_at = now;
            front.retransmits = front.retransmits.saturating_add(1);
        }
        Some((seq, payload))
    }

    /// Total retransmissions recorded across in-flight segments.
    pub fn retransmits(&self) -> u32 {
        self.inflight.iter().map(|s| s.retransmits).sum()
    }
}

/// Decode-side half of the reliability layer: a reorder window that emits the
/// in-order stream and a cumulative ACK.
///
/// The receiver buffers up to [`TCP_RETRANSMIT_WINDOW`] out-of-order segments
/// keyed by `tcp_seq`, emits a cumulative ACK immediately (no delayed-ACK
/// timer), and drops segments older than the window so the sender retransmits
/// them.
#[derive(Debug)]
pub struct TcpReliableReceiver {
    reorder: BTreeMap<u32, Bytes>,
    next_expected: u32,
}

impl TcpReliableReceiver {
    /// Create a receiver expecting its next byte at sequence `next_expected`.
    pub fn new(next_expected: u32) -> Self {
        Self {
            reorder: BTreeMap::new(),
            next_expected,
        }
    }

    /// The cumulative ACK to advertise for this stream.
    pub fn ack(&self) -> u32 {
        self.next_expected
    }

    /// Deliver a segment. Returns the in-order payloads now available, in
    /// sequence order. Out-of-order segments within the window are buffered;
    /// segments older than the window are dropped so the sender retransmits.
    pub fn on_segment(&mut self, seq: u32, payload: &[u8]) -> Vec<Bytes> {
        let mut ordered = Vec::new();

        // Too far ahead of the reorder window: drop, the sender retransmits.
        if seq_ge(
            seq,
            self.next_expected
                .wrapping_add(TCP_RETRANSMIT_WINDOW as u32),
        ) {
            return ordered;
        }

        if seq == self.next_expected {
            ordered.push(Bytes::copy_from_slice(payload));
            self.next_expected = self.next_expected.wrapping_add(payload.len() as u32);
            loop {
                let next = match self.reorder.keys().next().copied() {
                    Some(key) if key == self.next_expected => key,
                    _ => break,
                };
                let data = self.reorder.remove(&next).expect("key is present");
                self.next_expected = self.next_expected.wrapping_add(data.len() as u32);
                ordered.push(data);
            }
        } else if seq_after(seq, self.next_expected) {
            // Out of order but within the window: buffer it.
            self.reorder
                .entry(seq)
                .or_insert_with(|| Bytes::copy_from_slice(payload));
        }

        ordered
    }
}

/// One direction's sequenced-reliable channel, bundling the encode-side sender
/// and decode-side receiver halves so client and proxy share one
/// implementation.
#[derive(Debug)]
pub struct TcpReliable {
    pub sender: TcpReliableSender,
    pub receiver: TcpReliableReceiver,
}

impl TcpReliable {
    /// Create a direction's reliability layer.
    ///
    /// `next_seq` is the next stream byte the sender assigns; `next_expected`
    /// is the first byte the receiver still expects. The caller initialises
    /// both per RFC 793 (SYN consumes sequence 1, so data typically starts at
    /// 2).
    pub fn new(next_seq: u32, next_expected: u32) -> Self {
        Self {
            sender: TcpReliableSender::new(next_seq),
            receiver: TcpReliableReceiver::new(next_expected),
        }
    }
}

// ────────────────────────────────────────────────────────────
// HMAC-SHA256 (self-contained; the protocol crate has no hash deps)
// ────────────────────────────────────────────────────────────

const SHA256_K: [u32; 64] = [
    0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
    0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
    0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
    0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
    0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
    0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
    0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
    0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
];

/// FIPS 180-4 SHA-256, returning the 32-byte digest.
fn sha256(message: &[u8]) -> [u8; 32] {
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];

    // Pad: append 0x80, zeroes, then the 64-bit bit length.
    let bit_len = (message.len() as u64) * 8;
    let mut data = Vec::with_capacity(((message.len() / 64) + 2) * 64);
    data.extend_from_slice(message);
    data.push(0x80);
    while data.len() % 64 != 56 {
        data.push(0);
    }
    data.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in data.as_chunks::<64>().0 {
        let mut w = [0u32; 64];
        for (i, word) in w.iter_mut().enumerate().take(16) {
            let base = i * 4;
            *word = u32::from_be_bytes([
                chunk[base],
                chunk[base + 1],
                chunk[base + 2],
                chunk[base + 3],
            ]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }

        let (mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut hh) =
            (h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]);
        for (&k, &wv) in SHA256_K.iter().zip(w.iter()) {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let ch = (e & f) ^ ((!e) & g);
            let temp1 = hh
                .wrapping_add(s1)
                .wrapping_add(ch)
                .wrapping_add(k)
                .wrapping_add(wv);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let maj = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(maj);
            hh = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }

        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
        h[5] = h[5].wrapping_add(f);
        h[6] = h[6].wrapping_add(g);
        h[7] = h[7].wrapping_add(hh);
    }

    let mut out = [0u8; 32];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// RFC 2104 HMAC-SHA256.
fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    const BLOCK: usize = 64;

    let mut k = [0u8; BLOCK];
    if key.len() > BLOCK {
        let hashed = sha256(key);
        k[..32].copy_from_slice(&hashed);
    } else {
        k[..key.len()].copy_from_slice(key);
    }

    let mut ipad = [0x36u8; BLOCK];
    let mut opad = [0x5cu8; BLOCK];
    for i in 0..BLOCK {
        ipad[i] ^= k[i];
        opad[i] ^= k[i];
    }

    let mut inner = Vec::with_capacity(BLOCK + message.len());
    inner.extend_from_slice(&ipad);
    inner.extend_from_slice(message);
    let inner_hash = sha256(&inner);

    let mut outer = Vec::with_capacity(BLOCK + 32);
    outer.extend_from_slice(&opad);
    outer.extend_from_slice(&inner_hash);
    sha256(&outer)
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fec::{FecHeader, FEC_HEADER_SIZE};
    use crate::header::{DecodeError, TunnelHeader, PROTOCOL_VERSION, PROTOCOL_VERSION_FEC};
    use bytes::Bytes;
    use std::net::Ipv4Addr;

    /// The exact 42-byte v5 wire fixture from the design doc's field values.
    ///
    /// Byte 0 is `0x50` (version 5 in the high nibble, flags 0 in the low
    /// nibble), matching `TunnelHeader::encode_to_array`.
    fn v5_fixture() -> [u8; 42] {
        [
            0x50, 0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x2A, 0x00, 0x0F, 0x42, 0x40, 0xC0, 0xA8, 0x01,
            0x64, 0x68, 0x1A, 0x01, 0x32, 0x30, 0x39, 0x63, 0xDD, 0x00, 0x00, 0x00, 0x00, 0x04,
            0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x02, 0x01, 0x00, 0x00, 0x00, 0x4D, 0x43,
        ]
    }

    #[test]
    fn tcp_v5_wire_layout_roundtrip() {
        let fixture = v5_fixture();

        // The pin: old code rejects v5 here, so this expect() fails before any
        // other assertion until version-5 decode acceptance lands.
        let (header, payload) =
            TunnelHeader::decode_with_payload(&fixture).expect("v5 must decode");

        assert_eq!(header.version, 5);
        assert_eq!(header.flags, 0);
        assert_eq!(header.session_token, 0xDEAD_BEEF);
        assert_eq!(header.sequence, 42);
        assert_eq!(header.timestamp_us, 1_000_000);
        assert_eq!(header.orig_src_ip, Ipv4Addr::new(192, 168, 1, 100));
        assert_eq!(header.orig_dst_ip, Ipv4Addr::new(104, 26, 1, 50));
        assert_eq!(header.orig_src_port, 12345);
        assert_eq!(header.orig_dst_port, 25565);
        assert!(header.has_fec());
        assert!(header.has_tcp());

        // FEC header follows the tunnel header.
        let mut fec_slice: &[u8] = &payload[..FEC_HEADER_SIZE];
        let fec = FecHeader::decode(&mut fec_slice).expect("fec header must decode");
        assert_eq!(fec.block_id, 0);
        assert_eq!(fec.index, 0);
        assert_eq!(fec.k_size, 4);

        // TCP extension header + game stream payload.
        let (tcp_ext, game_payload) =
            decode_tcp_payload(&payload[FEC_HEADER_SIZE..]).expect("tcp ext must decode");
        assert_eq!(tcp_ext.tcp_seq, 1);
        assert_eq!(tcp_ext.tcp_ack, 2);
        assert_eq!(tcp_ext.flags, tcp_flags::SYN);
        assert_eq!(tcp_ext.window, 0);
        assert_eq!(tcp_ext.reserved, [0, 0]);
        assert_eq!(game_payload, b"MC");

        // Byte-identical re-encode.
        let reencoded = build_tcp_packet(&header, &fec, &tcp_ext, game_payload);
        assert_eq!(reencoded.as_ref(), fixture.as_slice());
    }

    #[test]
    fn unsupported_version_pins_old_accepted_set() {
        let fixture = v5_fixture();
        let version = fixture[0] >> 4;
        assert_eq!(version, 5);

        // Reproduce the old code's accepted-set gate verbatim and assert the
        // exact rejection an old relay emits for the new wire format. This is
        // the backward-compatibility contract: old relays reject and drop v5.
        let result = if version != PROTOCOL_VERSION && version != PROTOCOL_VERSION_FEC {
            Err::<(), DecodeError>(DecodeError::UnsupportedVersion {
                version,
                expected: PROTOCOL_VERSION,
            })
        } else {
            Ok(())
        };

        assert!(
            matches!(
                result,
                Err(DecodeError::UnsupportedVersion {
                    version: 5,
                    expected: 3
                })
            ),
            "old relays must reject v5 as UnsupportedVersion{{version: 5, expected: 3}}"
        );
    }

    #[test]
    fn syn_cookie_is_deterministic_and_keyed() {
        let tuple = Tcp5Tuple {
            src_ip: Ipv4Addr::new(192, 168, 1, 100),
            src_port: 12345,
            dst_ip: Ipv4Addr::new(104, 26, 1, 50),
            dst_port: 25565,
        };
        let key = [0x42u8; 32];

        let cookie_a = derive_syn_cookie(&key, &tuple);
        let cookie_b = derive_syn_cookie(&key, &tuple);
        assert_eq!(cookie_a, cookie_b, "same key and tuple must agree");

        let mut other_key = key;
        other_key[0] ^= 0xFF;
        assert_ne!(
            derive_syn_cookie(&other_key, &tuple),
            cookie_a,
            "a different key epoch must change the cookie"
        );

        let other_tuple = Tcp5Tuple {
            src_port: 12346,
            ..tuple
        };
        assert_ne!(
            derive_syn_cookie(&key, &other_tuple),
            cookie_a,
            "a different tuple must change the cookie"
        );
    }

    #[test]
    fn tcp_reliable_acks_retransmits_and_reorders() {
        // Encode side: window advances on cumulative ACK, dup ACKs count up.
        let mut reliable = TcpReliable::new(2, 2);
        assert_eq!(reliable.sender.send(b"hello"), Some(2));
        assert_eq!(reliable.sender.send(b"world"), Some(7));
        assert_eq!(reliable.sender.inflight(), 2);

        assert_eq!(reliable.sender.on_ack(7), AckProgress::New);
        assert_eq!(reliable.sender.inflight(), 1);
        assert_eq!(reliable.sender.on_ack(7), AckProgress::Duplicate);

        // Decode side: out-of-order is buffered and flushed in order.
        let mut receiver = TcpReliableReceiver::new(2);
        assert!(receiver.on_segment(7, b"world").is_empty());
        assert_eq!(
            receiver.on_segment(2, b"hello"),
            vec![Bytes::from_static(b"hello"), Bytes::from_static(b"world")]
        );
        assert_eq!(receiver.ack(), 12);
    }
}
