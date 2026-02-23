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
/// Parity is computed over fixed-size buffers, padded with zeros.
pub const FEC_MAX_PAYLOAD: usize = 1400;

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
#[derive(Debug)]
pub struct FecEncoder {
    /// Current block ID.
    block_id: u16,
    /// Block size (K data packets per block).
    k_size: u8,
    /// Accumulated data packets in current block.
    data_packets: Vec<Bytes>,
    /// Running XOR parity (computed incrementally).
    parity: Vec<u8>,
}

impl FecEncoder {
    /// Create a new FEC encoder with the given block size.
    pub fn new(k_size: u8) -> Self {
        let k = k_size.min(MAX_BLOCK_SIZE).max(2);
        Self {
            block_id: 0,
            k_size: k,
            data_packets: Vec::with_capacity(k as usize),
            parity: vec![0u8; FEC_MAX_PAYLOAD],
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

    /// Add a data packet to the current block and XOR into parity.
    ///
    /// Returns `Some(parity_bytes)` when the block is complete (K packets received),
    /// or `None` if the block is still accumulating.
    pub fn add_packet(&mut self, payload: &[u8]) -> Option<Bytes> {
        // XOR payload into running parity
        let len = payload.len().min(FEC_MAX_PAYLOAD);
        for i in 0..len {
            self.parity[i] ^= payload[i];
        }
        // Also XOR the length (2 bytes, big-endian) at a fixed position
        // so we can recover the original packet length
        let len_bytes = (payload.len() as u16).to_be_bytes();
        let len_offset = FEC_MAX_PAYLOAD - 2;
        self.parity[len_offset] ^= len_bytes[0];
        self.parity[len_offset + 1] ^= len_bytes[1];

        self.data_packets.push(Bytes::copy_from_slice(payload));

        if self.data_packets.len() >= self.k_size as usize {
            // Block complete — emit parity and reset
            let parity = Bytes::copy_from_slice(&self.parity);
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
        let parity = Bytes::copy_from_slice(&self.parity);
        self.reset_block();
        Some((block_id, actual_k, parity))
    }

    fn reset_block(&mut self) {
        self.block_id = self.block_id.wrapping_add(1);
        self.data_packets.clear();
        self.parity.fill(0);
    }
}

// ────────────────────────────────────────────────────────────
// FEC Decoder
// ────────────────────────────────────────────────────────────

/// Tracks received packets for a single FEC block.
#[derive(Debug)]
struct BlockState {
    /// Which data packet indices have been received.
    received: Vec<Option<Bytes>>,
    /// Parity packet (if received).
    parity: Option<Bytes>,
    /// Block size (K). Used in debug output.
    #[allow(dead_code)]
    k_size: u8,
    /// Creation time for expiry.
    created: std::time::Instant,
}

impl BlockState {
    fn new(k_size: u8) -> Self {
        Self {
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
    /// Returns `Some((index, recovered_bytes))` if exactly one packet
    /// was missing and parity was available.
    fn try_recover(&self) -> Option<(u8, Bytes)> {
        // Can only recover if exactly 1 packet is missing and we have parity
        if self.missing_count() != 1 {
            return None;
        }
        let parity = self.parity.as_ref()?;

        // Find the missing index
        let missing_idx = self
            .received
            .iter()
            .position(|p| p.is_none())
            .unwrap();

        // XOR all received data packets + parity to recover the missing one
        let mut recovered = vec![0u8; FEC_MAX_PAYLOAD];

        // Start with parity
        let parity_len = parity.len().min(FEC_MAX_PAYLOAD);
        recovered[..parity_len].copy_from_slice(&parity[..parity_len]);

        // XOR in all received data packets
        for (i, slot) in self.received.iter().enumerate() {
            if i == missing_idx {
                continue;
            }
            if let Some(data) = slot {
                let len = data.len().min(FEC_MAX_PAYLOAD);
                for j in 0..len {
                    recovered[j] ^= data[j];
                }
                // Also XOR the length
                let len_bytes = (data.len() as u16).to_be_bytes();
                let len_offset = FEC_MAX_PAYLOAD - 2;
                recovered[len_offset] ^= len_bytes[0];
                recovered[len_offset + 1] ^= len_bytes[1];
            }
        }

        // Extract original length from recovered data
        let len_offset = FEC_MAX_PAYLOAD - 2;
        let orig_len =
            u16::from_be_bytes([recovered[len_offset], recovered[len_offset + 1]]) as usize;

        if orig_len > FEC_MAX_PAYLOAD - 2 {
            return None; // Invalid length, recovery failed
        }

        Some((missing_idx as u8, Bytes::copy_from_slice(&recovered[..orig_len])))
    }
}

/// FEC decoder: tracks incoming packets and recovers lost data.
#[derive(Debug)]
pub struct FecDecoder {
    /// Active FEC blocks, keyed by block_id.
    blocks: std::collections::HashMap<u16, BlockState>,
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
            blocks: std::collections::HashMap::new(),
            max_age_ms: 500, // 500ms max block age
            recovered_count: 0,
            recovery_failures: 0,
        }
    }

    /// Process an incoming data packet. Returns the payload as-is.
    /// Internally tracks the packet for potential future recovery.
    pub fn receive_data(&mut self, fec: &FecHeader, payload: Bytes) -> Bytes {
        let block = self
            .blocks
            .entry(fec.block_id)
            .or_insert_with(|| BlockState::new(fec.k_size));
        block.receive_data(fec.index, payload.clone());
        payload
    }

    /// Process an incoming parity packet. Does NOT return the parity
    /// as application data — instead, checks if we can now recover
    /// a missing data packet.
    ///
    /// Returns `Some((index, recovered_payload))` if a packet was recovered.
    pub fn receive_parity(&mut self, fec: &FecHeader, payload: Bytes) -> Option<(u8, Bytes)> {
        let block = self
            .blocks
            .entry(fec.block_id)
            .or_insert_with(|| BlockState::new(fec.k_size));
        block.receive_parity(payload);

        match block.try_recover() {
            Some(result) => {
                self.recovered_count += 1;
                // Clean up the block
                self.blocks.remove(&fec.block_id);
                Some(result)
            }
            None => {
                if block.missing_count() == 0 {
                    // All packets received, no recovery needed — clean up
                    self.blocks.remove(&fec.block_id);
                }
                None
            }
        }
    }

    /// Check if a specific data packet was lost and can be recovered
    /// now that we have enough information.
    pub fn try_recover_block(&mut self, block_id: u16) -> Option<(u8, Bytes)> {
        let block = self.blocks.get(&block_id)?;
        match block.try_recover() {
            Some(result) => {
                self.recovered_count += 1;
                self.blocks.remove(&block_id);
                Some(result)
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
        self.blocks
            .retain(|_, block| now.duration_since(block.created) < max_age);
    }

    /// Get recovery statistics.
    pub fn stats(&self) -> FecStats {
        FecStats {
            active_blocks: self.blocks.len(),
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
        assert!(result.is_none(), "No recovery needed when all data received");
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
        assert!(result.is_none(), "Cannot recover 2 lost packets with 1 parity");
    }

    #[test]
    fn test_variable_length_packets() {
        let mut encoder = FecEncoder::new(3);

        let packets: Vec<Vec<u8>> = vec![
            vec![1, 2, 3],                             // 3 bytes
            vec![10, 20, 30, 40, 50],                  // 5 bytes
            vec![100, 200],                             // 2 bytes
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
}
