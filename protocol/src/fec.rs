//! # Forward Error Correction (FEC)
//!
//! XOR-based FEC for LightSpeed tunnel packets. This is our core
//! competitive advantage over ExitLag's brute-force packet duplication.
//!
//! ## How It Works
//!
//! Packets are grouped into blocks of K data packets. For each block,
//! we generate P parity packets using XOR. Any single lost packet
//! within a block can be recovered from the remaining K-1 data packets
//! plus 1 parity packet.
//!
//! ## Bandwidth Comparison
//!
//! ```text
//! ExitLag (duplication):  Send every packet on 2-3 paths → 2-3x bandwidth
//! LightSpeed (FEC):       Send K data + P parity           → (K+P)/K bandwidth
//!                         Default K=4, P=1                 → 1.25x bandwidth
//! ```
//!
//! ## Multipath Integration
//!
//! In multipath mode, data packets go on the primary (fastest) path,
//! parity packets go on the secondary path. This means:
//! - If primary path drops a packet → recover from parity on secondary
//! - If secondary path drops → no impact (data is complete on primary)
//! - Total bandwidth: ~1.25x (vs ExitLag's 3x)
//!
//! ## FEC Header Extension (4 bytes, appended after TunnelHeader when FLAG_FEC is set)
//!
//! ```text
//!  0       1       2       3
//! +-------+-------+-------+-------+
//! | BlkHi | BlkLo | Index | KSize |
//! +-------+-------+-------+-------+
//! ```
//!
//! - `block_id` (u16): Identifies the FEC block (wraps at 65535)
//! - `index` (u8): Position within block (0..K-1 = data, K = parity)
//! - `k_size` (u8): Number of data packets in this block

use bytes::{Buf, BufMut, Bytes, BytesMut};

/// FEC header extension size in bytes.
pub const FEC_HEADER_SIZE: usize = 4;

/// Default block size (number of data packets per FEC block).
pub const DEFAULT_BLOCK_SIZE: u8 = 4;

/// Maximum block size supported.
pub const MAX_BLOCK_SIZE: u8 = 16;

/// Maximum packet payload size for FEC (MTU - headers).
/// The internal parity scratch-buffer uses this size; the **emitted** parity
/// packet is always smaller — only `max_payload_len_in_block + 2` bytes —
/// because we track the maximum actual payload length and only XOR that prefix.
pub const FEC_MAX_PAYLOAD: usize = 1400;

/// Ring-buffer capacity for the FEC decoder.
/// Must be large enough that two live blocks never share the same slot
/// (i.e., block IDs never differ by exactly `RING_CAPACITY` simultaneously).
/// 64 slots covers ~256 ms of block IDs at 250 blocks/sec.
const DECODER_RING_CAPACITY: usize = 64;

/// FEC header extension, present when FLAG_FEC is set in TunnelHeader.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FecHeader {
    /// Block identifier (monotonically increasing, wraps at u16::MAX).
    pub block_id: u16,
    /// Index within block: 0..k-1 for data packets, k..k+p-1 for parity.
    pub index: u8,
    /// Number of data packets in this block (K).
    pub k_size: u8,
}

impl FecHeader {
    /// Create a new FEC header for a data packet.
    pub fn data(block_id: u16, index: u8, k_size: u8) -> Self {
        debug_assert!(index < k_size, "Data index must be < k_size");
        Self {
            block_id,
            index,
            k_size,
        }
    }

    /// Create a new FEC header for a parity packet.
    pub fn parity(block_id: u16, k_size: u8) -> Self {
        Self {
            block_id,
            index: k_size, // parity index = K
            k_size,
        }
    }

    /// Returns true if this is a parity (repair) packet.
    pub fn is_parity(&self) -> bool {
        self.index >= self.k_size
    }

    /// Encode to bytes (4 bytes).
    pub fn encode(&self, buf: &mut BytesMut) {
        buf.put_u16(self.block_id);
        buf.put_u8(self.index);
        buf.put_u8(self.k_size);
    }

    /// Decode from bytes.
    pub fn decode(buf: &mut &[u8]) -> Option<Self> {
        if buf.remaining() < FEC_HEADER_SIZE {
            return None;
        }
        let block_id = buf.get_u16();
        let index = buf.get_u8();
        let k_size = buf.get_u8();

        if k_size == 0 || k_size > MAX_BLOCK_SIZE {
            return None;
        }

        Some(Self {
            block_id,
            index,
            k_size,
        })
    }
}

// ────────────────────────────────────────────────────────────
// FEC Encoder
// ────────────────────────────────────────────────────────────

/// FEC encoder: accumulates data packets and generates parity.
///
/// ## Parity wire format
///
/// ```text
/// [ XOR of payload bytes (max_payload_len bytes) ][ lengths_xor (2 bytes, BE) ]
/// ```
///
/// `max_payload_len` is the maximum payload size seen across all K packets in
/// the current block.  The decoder reads `parity.len() - 2` to find how many
/// data bytes are present, and the last two bytes to recover the original
/// packet length.  This allows parity packets to be as small as the largest
/// game packet + 2 bytes (often 64–768 B) instead of always 1400 B.
#[derive(Debug)]
pub struct FecEncoder {
    /// Current block ID.
    block_id: u16,
    /// Block size (K data packets per block).
    k_size: u8,
    /// Accumulated data packets in current block.
    data_packets: Vec<Bytes>,
    /// Running XOR parity scratch buffer (always FEC_MAX_PAYLOAD bytes).
    parity: Vec<u8>,
    /// Maximum payload length seen in the current block — determines emit length.
    parity_len: usize,
    /// Running XOR of all packet lengths (stored separately so we know the
    /// emit length before writing it into the parity content bytes).
    lengths_xor: u16,
}

impl FecEncoder {
    /// Create a new FEC encoder with the given block size.
    pub fn new(k_size: u8) -> Self {
        let k = k_size.clamp(2, MAX_BLOCK_SIZE);
        Self {
            block_id: 0,
            k_size: k,
            data_packets: Vec::with_capacity(k as usize),
            parity: vec![0u8; FEC_MAX_PAYLOAD],
            parity_len: 0,
            lengths_xor: 0,
        }
    }

    /// Get the current block ID.
    pub fn block_id(&self) -> u16 {
        self.block_id
    }

    /// Get the current index within the block.
    pub fn current_index(&self) -> u8 {
        self.data_packets.len() as u8
    }

    /// Emit the parity for the current block state.
    ///
    /// Format: `parity[..parity_len] || lengths_xor as 2 BE bytes`
    fn emit_parity(&self) -> Bytes {
        let emit_len = self.parity_len + 2;
        let mut out = BytesMut::with_capacity(emit_len);
        out.put_slice(&self.parity[..self.parity_len]);
        out.put_u16(self.lengths_xor);
        out.freeze()
    }

    /// Add a data packet to the current block and XOR into parity.
    ///
    /// Returns `Some(parity_bytes)` when the block is complete (K packets received),
    /// or `None` if the block is still accumulating.
    ///
    /// The returned parity is `max_payload_len + 2` bytes — much smaller than
    /// the old fixed 1400-byte buffer for typical game packets (64–512 B).
    pub fn add_packet(&mut self, payload: &[u8]) -> Option<Bytes> {
        let len = payload.len().min(FEC_MAX_PAYLOAD);

        // XOR payload into running parity (only the actual payload bytes)
        for (p, &b) in self.parity[..len].iter_mut().zip(payload.iter()) {
            *p ^= b;
        }

        // Track the maximum payload length seen in this block
        if len > self.parity_len {
            self.parity_len = len;
        }

        // XOR this packet's length into the lengths accumulator
        self.lengths_xor ^= payload.len() as u16;

        self.data_packets.push(Bytes::copy_from_slice(payload));

        if self.data_packets.len() >= self.k_size as usize {
            // Block complete — emit compact parity and reset
            let parity = self.emit_parity();
            self.reset_block();
            Some(parity)
        } else {
            None
        }
    }

    /// Force-flush an incomplete block (e.g., on timeout).
    /// Returns parity for the partial block if any packets were accumulated.
    pub fn flush(&mut self) -> Option<(u16, u8, Bytes)> {
        if self.data_packets.is_empty() {
            return None;
        }
        let block_id = self.block_id;
        let actual_k = self.data_packets.len() as u8;
        let parity = self.emit_parity();
        self.reset_block();
        Some((block_id, actual_k, parity))
    }

    fn reset_block(&mut self) {
        self.block_id = self.block_id.wrapping_add(1);
        self.data_packets.clear();
        self.parity.fill(0);
        self.parity_len = 0;
        self.lengths_xor = 0;
    }
}

// ────────────────────────────────────────────────────────────
// FEC Decoder
// ────────────────────────────────────────────────────────────

/// Tracks received packets for a single FEC block.
#[derive(Debug)]
struct BlockState {
    /// Block ID — used to validate ring-buffer slot ownership.
    block_id: u16,
    /// Which data packet indices have been received.
    received: Vec<Option<Bytes>>,
    /// Parity packet (if received).
    parity: Option<Bytes>,
    /// Block size (K). Retained for debugging.
    #[allow(dead_code)]
    k_size: u8,
    /// Creation time for expiry.
    created: std::time::Instant,
}

impl BlockState {
    fn new(block_id: u16, k_size: u8) -> Self {
        Self {
            block_id,
            received: vec![None; k_size as usize],
            parity: None,
            k_size,
            created: std::time::Instant::now(),
        }
    }

    /// Record a received data packet.
    fn receive_data(&mut self, index: u8, payload: Bytes) {
        if (index as usize) < self.received.len() {
            self.received[index as usize] = Some(payload);
        }
    }

    /// Record a received parity packet.
    fn receive_parity(&mut self, payload: Bytes) {
        self.parity = Some(payload);
    }

    /// Count how many data packets are missing.
    fn missing_count(&self) -> usize {
        self.received.iter().filter(|p| p.is_none()).count()
    }

    /// Try to recover a single missing data packet using parity.
    ///
    /// Expects the new compact parity format:
    /// `[ XOR content (parity_content_len bytes) ][ lengths_xor (2 BE bytes) ]`
    ///
    /// Returns `Some((index, recovered_bytes))` if exactly one packet was
    /// missing and parity was available.
    fn try_recover(&self) -> Option<(u8, Bytes)> {
        // Can only recover if exactly 1 packet is missing and we have parity
        if self.missing_count() != 1 {
            return None;
        }
        let parity = self.parity.as_ref()?;

        // Parity must be at least 2 bytes (lengths_xor trailer)
        if parity.len() < 2 {
            return None;
        }

        // The last 2 bytes are the XOR of all packet lengths
        let parity_content_len = parity.len() - 2;

        // Find the missing index
        let missing_idx = self.received.iter().position(|p| p.is_none()).unwrap();

        // Start length accumulator with the lengths_xor from parity
        let mut lengths_xor =
            u16::from_be_bytes([parity[parity_content_len], parity[parity_content_len + 1]]);

        // Build recovery buffer sized to the parity content
        let mut recovered = vec![0u8; parity_content_len];
        recovered.copy_from_slice(&parity[..parity_content_len]);

        // XOR in all received data packets
        for (i, slot) in self.received.iter().enumerate() {
            if i == missing_idx {
                continue;
            }
            if let Some(data) = slot {
                let xor_len = data.len().min(parity_content_len);
                for (r, &b) in recovered[..xor_len].iter_mut().zip(data.iter()) {
                    *r ^= b;
                }
                // XOR this packet's length into the accumulator
                lengths_xor ^= data.len() as u16;
            }
        }

        // lengths_xor now holds the original length of the missing packet
        let orig_len = lengths_xor as usize;
        if orig_len > parity_content_len {
            return None; // Invalid length — recovery failed
        }

        Some((
            missing_idx as u8,
            Bytes::copy_from_slice(&recovered[..orig_len]),
        ))
    }
}

/// FEC decoder: tracks incoming packets and recovers lost data.
///
/// Uses a fixed-size ring buffer (indexed by `block_id % DECODER_RING_CAPACITY`)
/// instead of a `HashMap`, eliminating hash overhead in the hot path.
/// Typical game sessions run at well under `DECODER_RING_CAPACITY` concurrent
/// blocks, so slot collisions are essentially impossible.
#[derive(Debug)]
pub struct FecDecoder {
    /// Ring buffer of active FEC blocks.
    /// Indexed by `block_id % DECODER_RING_CAPACITY`.
    ring: Vec<Option<BlockState>>,
    /// Maximum age of a block before it's discarded (ms).
    max_age_ms: u64,
    /// Stats: packets recovered via FEC.
    pub recovered_count: u64,
    /// Stats: recovery attempts that failed.
    pub recovery_failures: u64,
}

impl FecDecoder {
    /// Create a new FEC decoder.
    pub fn new() -> Self {
        Self {
            ring: (0..DECODER_RING_CAPACITY).map(|_| None).collect(),
            max_age_ms: 500, // 500 ms max block age
            recovered_count: 0,
            recovery_failures: 0,
        }
    }

    /// Get the ring index for a given `block_id`.
    #[inline]
    fn ring_idx(block_id: u16) -> usize {
        (block_id as usize) % DECODER_RING_CAPACITY
    }

    /// Ensure the ring slot for `block_id` holds a fresh `BlockState`,
    /// evicting any stale block that happens to share the slot.
    fn ensure_block(&mut self, block_id: u16, k_size: u8) -> &mut BlockState {
        let idx = Self::ring_idx(block_id);
        match &mut self.ring[idx] {
            Some(b) if b.block_id == block_id => {}
            slot => {
                *slot = Some(BlockState::new(block_id, k_size));
            }
        }
        self.ring[idx].as_mut().unwrap()
    }

    /// Process an incoming data packet. Returns the payload as-is.
    /// Internally tracks the packet for potential future recovery.
    pub fn receive_data(&mut self, fec: &FecHeader, payload: Bytes) -> Bytes {
        let block = self.ensure_block(fec.block_id, fec.k_size);
        block.receive_data(fec.index, payload.clone());
        payload
    }

    /// Process an incoming parity packet. Does NOT return the parity
    /// as application data — instead, checks if we can now recover
    /// a missing data packet.
    ///
    /// Returns `Some((index, recovered_payload))` if a packet was recovered.
    pub fn receive_parity(&mut self, fec: &FecHeader, payload: Bytes) -> Option<(u8, Bytes)> {
        let idx = Self::ring_idx(fec.block_id);

        // Ensure the slot holds the correct block, then store parity
        match &mut self.ring[idx] {
            Some(b) if b.block_id == fec.block_id => {
                b.receive_parity(payload);
            }
            slot => {
                let mut b = BlockState::new(fec.block_id, fec.k_size);
                b.receive_parity(payload);
                *slot = Some(b);
            }
        }

        // Try recovery — result is fully owned (no borrow from self.ring)
        let result = self.ring[idx].as_ref().and_then(|b| b.try_recover());

        match result {
            Some(r) => {
                self.recovered_count += 1;
                self.ring[idx] = None;
                Some(r)
            }
            None => {
                let all_received = self.ring[idx]
                    .as_ref()
                    .map(|b| b.missing_count() == 0)
                    .unwrap_or(false);
                if all_received {
                    self.ring[idx] = None;
                }
                None
            }
        }
    }

    /// Check if a specific data packet was lost and can be recovered
    /// now that we have enough information.
    pub fn try_recover_block(&mut self, block_id: u16) -> Option<(u8, Bytes)> {
        let idx = Self::ring_idx(block_id);
        // Only attempt if the slot holds the right block
        if self.ring[idx].as_ref().map(|b| b.block_id) != Some(block_id) {
            return None;
        }
        let result = self.ring[idx].as_ref().and_then(|b| b.try_recover());
        match result {
            Some(r) => {
                self.recovered_count += 1;
                self.ring[idx] = None;
                Some(r)
            }
            None => {
                self.recovery_failures += 1;
                None
            }
        }
    }

    /// Garbage-collect expired blocks.
    pub fn gc(&mut self) {
        let now = std::time::Instant::now();
        let max_age = std::time::Duration::from_millis(self.max_age_ms);
        for slot in &mut self.ring {
            if let Some(ref block) = slot {
                if now.duration_since(block.created) >= max_age {
                    *slot = None;
                }
            }
        }
    }

    /// Get recovery statistics.
    pub fn stats(&self) -> FecStats {
        FecStats {
            active_blocks: self.ring.iter().filter(|s| s.is_some()).count(),
            recovered_count: self.recovered_count,
            recovery_failures: self.recovery_failures,
        }
    }
}

impl Default for FecDecoder {
    fn default() -> Self {
        Self::new()
    }
}

/// FEC performance statistics.
#[derive(Debug, Clone, Copy)]
pub struct FecStats {
    pub active_blocks: usize,
    pub recovered_count: u64,
    pub recovery_failures: u64,
}

// ────────────────────────────────────────────────────────────
// Tests
// ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fec_header_roundtrip() {
        let hdr = FecHeader::data(42, 2, 4);
        let mut buf = BytesMut::new();
        hdr.encode(&mut buf);
        assert_eq!(buf.len(), FEC_HEADER_SIZE);

        let mut slice: &[u8] = &buf;
        let decoded = FecHeader::decode(&mut slice).unwrap();
        assert_eq!(hdr, decoded);
    }

    #[test]
    fn test_fec_parity_header() {
        let hdr = FecHeader::parity(100, 4);
        assert!(hdr.is_parity());
        assert_eq!(hdr.index, 4);
        assert_eq!(hdr.k_size, 4);
    }

    #[test]
    fn test_xor_recovery_simple() {
        let mut encoder = FecEncoder::new(4);

        let packets: Vec<Vec<u8>> = vec![
            b"Hello, World!".to_vec(),
            b"Game packet 2".to_vec(),
            b"Position data".to_vec(),
            b"Player input!".to_vec(),
        ];

        // Feed packets into encoder
        let mut parity = None;
        for (i, pkt) in packets.iter().enumerate() {
            let result = encoder.add_packet(pkt);
            if i == 3 {
                parity = result; // Block complete, parity emitted
            }
        }

        assert!(parity.is_some(), "Parity should be emitted after K packets");
        let parity_bytes = parity.unwrap();

        // Simulate: packet index 2 ("Position data") is lost
        let mut decoder = FecDecoder::new();

        // Receive packets 0, 1, 3 (skip 2)
        for i in [0u8, 1, 3] {
            let fec = FecHeader::data(0, i, 4);
            decoder.receive_data(&fec, Bytes::copy_from_slice(&packets[i as usize]));
        }

        // Receive parity
        let fec = FecHeader::parity(0, 4);
        let recovered = decoder.receive_parity(&fec, parity_bytes);

        assert!(recovered.is_some(), "Should recover missing packet");
        let (idx, data) = recovered.unwrap();
        assert_eq!(idx, 2, "Should recover index 2");
        assert_eq!(&data[..], b"Position data", "Recovered data should match");
    }

    #[test]
    fn test_no_loss_no_recovery() {
        let mut encoder = FecEncoder::new(2);

        let p1 = b"packet one";
        let p2 = b"packet two";

        encoder.add_packet(p1);
        let parity = encoder.add_packet(p2).unwrap();

        // All packets received — parity triggers cleanup, no recovery
        let mut decoder = FecDecoder::new();
        decoder.receive_data(&FecHeader::data(0, 0, 2), Bytes::from_static(p1));
        decoder.receive_data(&FecHeader::data(0, 1, 2), Bytes::from_static(p2));
        let result = decoder.receive_parity(&FecHeader::parity(0, 2), parity);
        assert!(
            result.is_none(),
            "No recovery needed when all data received"
        );
    }

    #[test]
    fn test_two_losses_cannot_recover() {
        let mut encoder = FecEncoder::new(4);

        let packets: Vec<Vec<u8>> = (0..4).map(|i| format!("pkt{i}").into_bytes()).collect();

        let mut parity = None;
        for pkt in &packets {
            parity = encoder.add_packet(pkt);
        }
        let parity_bytes = parity.unwrap();

        // Lose packets 1 and 3 — two losses, can't recover with 1 parity
        let mut decoder = FecDecoder::new();
        decoder.receive_data(
            &FecHeader::data(0, 0, 4),
            Bytes::copy_from_slice(&packets[0]),
        );
        decoder.receive_data(
            &FecHeader::data(0, 2, 4),
            Bytes::copy_from_slice(&packets[2]),
        );
        let result = decoder.receive_parity(&FecHeader::parity(0, 4), parity_bytes);
        assert!(
            result.is_none(),
            "Cannot recover 2 lost packets with 1 parity"
        );
    }

    #[test]
    fn test_variable_length_packets() {
        let mut encoder = FecEncoder::new(3);

        let packets: Vec<Vec<u8>> = vec![
            vec![1, 2, 3],            // 3 bytes
            vec![10, 20, 30, 40, 50], // 5 bytes
            vec![100, 200],           // 2 bytes
        ];

        let mut parity = None;
        for pkt in &packets {
            parity = encoder.add_packet(pkt);
        }
        let parity_bytes = parity.unwrap();

        // Lose packet 1 (5 bytes)
        let mut decoder = FecDecoder::new();
        decoder.receive_data(
            &FecHeader::data(0, 0, 3),
            Bytes::copy_from_slice(&packets[0]),
        );
        decoder.receive_data(
            &FecHeader::data(0, 2, 3),
            Bytes::copy_from_slice(&packets[2]),
        );

        let recovered = decoder.receive_parity(&FecHeader::parity(0, 3), parity_bytes);
        assert!(recovered.is_some());
        let (idx, data) = recovered.unwrap();
        assert_eq!(idx, 1);
        assert_eq!(&data[..], &[10, 20, 30, 40, 50]);
    }

    #[test]
    fn test_encoder_block_id_increments() {
        let mut encoder = FecEncoder::new(2);
        assert_eq!(encoder.block_id(), 0);

        encoder.add_packet(b"a");
        encoder.add_packet(b"b"); // completes block 0
        assert_eq!(encoder.block_id(), 1);

        encoder.add_packet(b"c");
        encoder.add_packet(b"d"); // completes block 1
        assert_eq!(encoder.block_id(), 2);
    }

    #[test]
    fn test_flush_partial_block() {
        let mut encoder = FecEncoder::new(4);
        encoder.add_packet(b"only one");

        let flushed = encoder.flush();
        assert!(flushed.is_some());
        let (block_id, k, _parity) = flushed.unwrap();
        assert_eq!(block_id, 0);
        assert_eq!(k, 1);
    }

    /// End-to-end FEC pipeline test simulating a full tunnel session:
    /// 1. Encode 3 blocks of 4 packets each with FEC headers
    /// 2. Simulate losing 1 packet per block
    /// 3. Verify all lost packets are recovered by the decoder
    #[test]
    fn test_e2e_fec_pipeline_multi_block() {
        use crate::header::{TunnelHeader, HEADER_SIZE};
        use std::net::{Ipv4Addr, SocketAddrV4};

        let k: u8 = 4;
        let num_blocks: usize = 3;
        let mut encoder = FecEncoder::new(k);
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        // Generate test game packets (varying sizes)
        let game_packets: Vec<Vec<u8>> = (0..num_blocks * k as usize)
            .map(|i| format!("game_packet_{:04}_data_{}", i, "x".repeat(50 + i * 10)).into_bytes())
            .collect();

        // ── SENDER SIDE ──────────────────────────────────────────
        // Build wire-format packets: [TunnelHeader v2][FecHeader][payload]
        let mut wire_packets: Vec<(FecHeader, Vec<u8>)> = Vec::new(); // (fec_hdr, full_wire_packet)
        let mut parity_packets: Vec<(FecHeader, Vec<u8>)> = Vec::new();

        for (i, payload) in game_packets.iter().enumerate() {
            let seq = i as u16;
            let block_id = encoder.block_id();
            let index = encoder.current_index();

            let tunnel_hdr = TunnelHeader::new_fec(seq, 1000 + i as u32, src, dst);
            let fec_hdr = FecHeader::data(block_id, index, k);

            // Build wire packet
            let mut buf = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + payload.len());
            buf.extend_from_slice(&tunnel_hdr.encode());
            fec_hdr.encode(&mut buf);
            buf.extend_from_slice(payload);

            wire_packets.push((fec_hdr, buf.to_vec()));

            // Feed into FEC encoder
            let parity = encoder.add_packet(payload);
            if let Some(parity_bytes) = parity {
                let parity_hdr = FecHeader::parity(block_id, k);
                let parity_tunnel = TunnelHeader::new_fec(seq + 100, 2000, src, dst);
                let mut parity_buf =
                    BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len());
                parity_buf.extend_from_slice(&parity_tunnel.encode());
                parity_hdr.encode(&mut parity_buf);
                parity_buf.extend_from_slice(&parity_bytes);
                parity_packets.push((parity_hdr, parity_buf.to_vec()));
            }
        }

        assert_eq!(wire_packets.len(), num_blocks * k as usize);
        assert_eq!(parity_packets.len(), num_blocks);

        // ── RECEIVER SIDE (simulate packet loss) ─────────────────
        let mut decoder = FecDecoder::new();
        let mut recovered_payloads: Vec<(usize, Vec<u8>)> = Vec::new(); // (original_index, data)
        let mut received_payloads: Vec<(usize, Vec<u8>)> = Vec::new();

        for (block_num, (parity_fec, parity_wire)) in parity_packets.iter().enumerate() {
            let block_start = block_num * k as usize;
            // Lose packet index 1 in each block (simulate network loss)
            let lost_index_in_block: usize = 1;
            let lost_global_index = block_start + lost_index_in_block;

            // Receive data packets (skip the lost one)
            for i in 0..k as usize {
                let global_idx = block_start + i;
                if i == lost_index_in_block {
                    continue; // Simulate loss
                }

                let (fec_hdr, wire_pkt) = &wire_packets[global_idx];
                let game_data = &wire_pkt[HEADER_SIZE + FEC_HEADER_SIZE..];
                let data_bytes = Bytes::copy_from_slice(game_data);
                decoder.receive_data(fec_hdr, data_bytes.clone());
                received_payloads.push((global_idx, game_data.to_vec()));
            }

            // Receive parity packet → should trigger recovery
            let parity_data = &parity_wire[HEADER_SIZE + FEC_HEADER_SIZE..];
            let result = decoder.receive_parity(parity_fec, Bytes::copy_from_slice(parity_data));

            assert!(
                result.is_some(),
                "Block {} should recover lost packet",
                block_num
            );
            let (recovered_idx, recovered_data) = result.unwrap();
            assert_eq!(
                recovered_idx, lost_index_in_block as u8,
                "Should recover index {} in block {}",
                lost_index_in_block, block_num
            );

            recovered_payloads.push((lost_global_index, recovered_data.to_vec()));
        }

        // ── VERIFY ALL PAYLOADS RECOVERED CORRECTLY ──────────────
        assert_eq!(recovered_payloads.len(), num_blocks);
        for (global_idx, recovered) in &recovered_payloads {
            assert_eq!(
                recovered, &game_packets[*global_idx],
                "Recovered payload at index {} doesn't match original",
                global_idx
            );
        }

        // Verify decoder stats
        let stats = decoder.stats();
        assert_eq!(stats.recovered_count, num_blocks as u64);
        assert_eq!(stats.active_blocks, 0, "All blocks should be cleaned up");
    }

    /// Test FEC with realistic game packet sizes (Fortnite-like: 50-500 bytes)
    #[test]
    fn test_fec_realistic_game_packets() {
        let k: u8 = 8; // Realistic block size
        let mut encoder = FecEncoder::new(k);

        // Simulate realistic game packet sizes
        let packets: Vec<Vec<u8>> = vec![
            vec![0xAA; 64],  // Position update (small)
            vec![0xBB; 128], // Movement input
            vec![0xCC; 256], // State sync
            vec![0xDD; 48],  // Keepalive-like
            vec![0xEE; 512], // Large state update
            vec![0xFF; 96],  // Hit registration
            vec![0x11; 200], // Inventory update
            vec![0x22; 384], // Player spawn data
        ];

        let mut parity = None;
        for pkt in &packets {
            parity = encoder.add_packet(pkt);
        }
        let parity_bytes = parity.expect("Should have parity after K packets");

        // Lose the largest packet (index 4, 512 bytes)
        let mut decoder = FecDecoder::new();
        for (i, pkt) in packets.iter().enumerate() {
            if i == 4 {
                continue; // Lost!
            }
            decoder.receive_data(&FecHeader::data(0, i as u8, k), Bytes::copy_from_slice(pkt));
        }

        let result = decoder.receive_parity(&FecHeader::parity(0, k), parity_bytes);
        assert!(result.is_some(), "Should recover lost 512-byte packet");
        let (idx, recovered) = result.unwrap();
        assert_eq!(idx, 4);
        assert_eq!(recovered.len(), 512);
        assert!(
            recovered.iter().all(|&b| b == 0xEE),
            "All bytes should be 0xEE"
        );
    }
}
