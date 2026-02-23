//! # UDP Relay
//!
//! Manages the UDP sockets for sending wrapped packets to proxies
//! and receiving responses. This is the client-side data plane.
//!
//! Supports optional FEC (Forward Error Correction) for packet loss recovery.
//! When FEC is enabled, outbound packets are grouped and XOR parity packets
//! are generated. Inbound packets are tracked and lost packets can be recovered.

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::{Bytes, BytesMut};
use tokio::net::UdpSocket;

use crate::error::TunnelError;
use lightspeed_protocol::{
    FecDecoder, FecEncoder, FecHeader, TunnelHeader, FEC_HEADER_SIZE, HEADER_SIZE,
};

/// Get current timestamp in microseconds since epoch.
fn now_us() -> u32 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_micros() as u32
}

/// Shared tunnel statistics, updated atomically.
#[derive(Debug)]
pub struct RelayStats {
    pub packets_sent: AtomicU64,
    pub packets_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub send_errors: AtomicU64,
    pub recv_errors: AtomicU64,
    pub fec_parity_sent: AtomicU64,
    pub fec_recovered: AtomicU64,
}

impl RelayStats {
    pub fn new() -> Self {
        Self {
            packets_sent: AtomicU64::new(0),
            packets_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            send_errors: AtomicU64::new(0),
            recv_errors: AtomicU64::new(0),
            fec_parity_sent: AtomicU64::new(0),
            fec_recovered: AtomicU64::new(0),
        }
    }
}

/// UDP relay — sends tunnel packets to proxy and receives responses.
///
/// Supports optional FEC for packet loss recovery. When FEC is enabled:
/// - Outbound: packets are grouped into blocks, XOR parity generated
/// - Inbound: packets are tracked, lost packets recovered from parity
pub struct UdpRelay {
    /// The bound UDP socket for tunnel traffic.
    socket: Option<Arc<UdpSocket>>,
    /// Local bind address.
    local_addr: SocketAddrV4,
    /// Receive buffer size.
    recv_buf_size: usize,
    /// Monotonic sequence counter.
    sequence: AtomicU16,
    /// Relay statistics.
    pub stats: Arc<RelayStats>,
    /// FEC encoder (outbound), if FEC is enabled.
    fec_encoder: Option<FecEncoder>,
    /// FEC decoder (inbound), if FEC is enabled.
    fec_decoder: Option<FecDecoder>,
}

impl UdpRelay {
    /// Create a new UDP relay bound to the specified address.
    pub fn new(local_addr: SocketAddrV4) -> Self {
        Self {
            socket: None,
            local_addr,
            recv_buf_size: 2048,
            sequence: AtomicU16::new(0),
            stats: Arc::new(RelayStats::new()),
            fec_encoder: None,
            fec_decoder: None,
        }
    }

    /// Enable FEC with the given block size (K data packets per parity).
    pub fn with_fec(mut self, k_size: u8) -> Self {
        self.fec_encoder = Some(FecEncoder::new(k_size));
        self.fec_decoder = Some(FecDecoder::new());
        self
    }

    /// Check if FEC is enabled.
    pub fn fec_enabled(&self) -> bool {
        self.fec_encoder.is_some()
    }

    /// Bind the UDP socket.
    pub async fn bind(&mut self) -> Result<(), TunnelError> {
        let socket = UdpSocket::bind(self.local_addr).await?;

        // Set socket buffer sizes for low-latency gaming traffic
        tracing::info!("UDP relay bound to {}", socket.local_addr()?);
        self.socket = Some(Arc::new(socket));
        Ok(())
    }

    /// Get a clone of the socket Arc (for spawning recv tasks).
    pub fn socket(&self) -> Option<Arc<UdpSocket>> {
        self.socket.clone()
    }

    /// Get the next sequence number.
    pub fn next_sequence(&self) -> u16 {
        self.sequence.fetch_add(1, Ordering::Relaxed)
    }

    /// Send a game packet through the tunnel to a proxy.
    ///
    /// When FEC is enabled, packets are grouped and parity is generated.
    /// Wraps the raw game payload in a LightSpeed header and sends it.
    pub async fn send_to_proxy(
        &mut self,
        payload: &[u8],
        orig_src: SocketAddrV4,
        orig_dst: SocketAddrV4,
        proxy_addr: SocketAddrV4,
    ) -> Result<usize, TunnelError> {
        let socket = self.socket.as_ref().ok_or(TunnelError::NotConnected)?;

        if self.fec_encoder.is_some() {
            // ── FEC mode: encode with FEC header ────────────────
            // Get sequence numbers upfront to avoid borrow conflicts
            let seq = self.next_sequence();

            let encoder = self.fec_encoder.as_mut().unwrap();
            let block_id = encoder.block_id();
            let index = encoder.current_index();
            let k_size = index.max(2); // k_size for FEC header

            // Build FEC data packet: [TunnelHeader v2][FecHeader][payload]
            let header = TunnelHeader::new_fec(seq, now_us(), orig_src, orig_dst);
            let fec_hdr = FecHeader::data(block_id, index, k_size);

            let mut pkt_buf =
                BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + payload.len());
            pkt_buf.extend_from_slice(&header.encode());
            fec_hdr.encode(&mut pkt_buf);
            pkt_buf.extend_from_slice(payload);

            // Feed payload into FEC encoder (XOR accumulation)
            let parity = encoder.add_packet(payload);

            // Send the data packet
            let sent = socket.send_to(&pkt_buf, proxy_addr).await?;
            self.stats.packets_sent.fetch_add(1, Ordering::Relaxed);
            self.stats
                .bytes_sent
                .fetch_add(sent as u64, Ordering::Relaxed);

            tracing::trace!(
                seq = seq,
                fec_block = block_id,
                fec_idx = index,
                payload_len = payload.len(),
                "Sent FEC data packet"
            );

            // If block is complete, send parity packet
            if let Some(parity_bytes) = parity {
                let parity_seq = self.next_sequence();
                let parity_header =
                    TunnelHeader::new_fec(parity_seq, now_us(), orig_src, orig_dst);
                let parity_fec = FecHeader::parity(block_id, k_size);

                let mut parity_buf = BytesMut::with_capacity(
                    HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len(),
                );
                parity_buf.extend_from_slice(&parity_header.encode());
                parity_fec.encode(&mut parity_buf);
                parity_buf.extend_from_slice(&parity_bytes);

                let parity_sent = socket.send_to(&parity_buf, proxy_addr).await?;
                self.stats.packets_sent.fetch_add(1, Ordering::Relaxed);
                self.stats
                    .bytes_sent
                    .fetch_add(parity_sent as u64, Ordering::Relaxed);
                self.stats.fec_parity_sent.fetch_add(1, Ordering::Relaxed);

                tracing::trace!(
                    seq = parity_seq,
                    fec_block = block_id,
                    "Sent FEC parity packet"
                );
            }

            Ok(sent)
        } else {
            // ── Non-FEC mode: original behavior ─────────────────
            let seq = self.next_sequence();
            let header = TunnelHeader::new(seq, now_us(), orig_src, orig_dst);
            let packet = header.encode_with_payload(payload);

            let sent = socket.send_to(&packet, proxy_addr).await?;

            self.stats.packets_sent.fetch_add(1, Ordering::Relaxed);
            self.stats
                .bytes_sent
                .fetch_add(sent as u64, Ordering::Relaxed);

            tracing::trace!(
                seq = seq,
                payload_len = payload.len(),
                proxy = %proxy_addr,
                "Sent tunnel packet"
            );

            Ok(sent)
        }
    }

    /// Send a keepalive to the proxy.
    pub async fn send_keepalive(&self, proxy_addr: SocketAddrV4) -> Result<(), TunnelError> {
        let socket = self.socket.as_ref().ok_or(TunnelError::NotConnected)?;

        let seq = self.next_sequence();
        let header = TunnelHeader::keepalive(seq, now_us());
        let packet = header.encode();

        socket.send_to(&packet, proxy_addr).await?;
        tracing::trace!(seq = seq, "Sent keepalive");
        Ok(())
    }

    /// Receive a tunnel-wrapped packet from a proxy.
    ///
    /// When FEC is enabled, tracks received packets and attempts recovery
    /// of lost packets from parity data.
    ///
    /// Returns the decoded header, payload, and the proxy address it came from.
    /// For FEC parity packets that trigger recovery, returns the recovered data.
    pub async fn recv_from_proxy(
        &mut self,
    ) -> Result<(TunnelHeader, Bytes, SocketAddrV4), TunnelError> {
        let socket = self.socket.as_ref().ok_or(TunnelError::NotConnected)?;

        let mut buf = vec![0u8; self.recv_buf_size];
        let (len, addr) = socket.recv_from(&mut buf).await?;

        // Decode header
        let (header, payload_slice) = TunnelHeader::decode_with_payload(&buf[..len])?;

        // Convert addr to SocketAddrV4
        let proxy_addr = match addr {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => {
                return Err(TunnelError::Relay("IPv6 not supported yet".into()));
            }
        };

        // Update stats
        self.stats.packets_received.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes_received
            .fetch_add(len as u64, Ordering::Relaxed);

        // Measure RTT from timestamp
        let now = now_us();
        let rtt_us = now.wrapping_sub(header.timestamp_us);
        tracing::trace!(
            seq = header.sequence,
            payload_len = payload_slice.len(),
            rtt_us = rtt_us,
            "Received tunnel response"
        );

        // Handle FEC if enabled and packet has FEC flag
        if let Some(ref mut decoder) = self.fec_decoder {
            if header.has_fec() && payload_slice.len() >= FEC_HEADER_SIZE {
                let mut fec_slice: &[u8] = &payload_slice[..FEC_HEADER_SIZE];
                if let Some(fec_hdr) = FecHeader::decode(&mut fec_slice) {
                    let game_data = &payload_slice[FEC_HEADER_SIZE..];

                    if fec_hdr.is_parity() {
                        // Parity packet — try to recover
                        let parity_data = Bytes::copy_from_slice(game_data);
                        if let Some((_idx, recovered)) =
                            decoder.receive_parity(&fec_hdr, parity_data)
                        {
                            self.stats.fec_recovered.fetch_add(1, Ordering::Relaxed);
                            tracing::info!(
                                block = fec_hdr.block_id,
                                recovered_len = recovered.len(),
                                "🔧 FEC recovered lost packet"
                            );
                            return Ok((header, recovered, proxy_addr));
                        }
                        // Parity consumed, no recovery needed — return empty
                        return Ok((header, Bytes::new(), proxy_addr));
                    } else {
                        // Data packet — track for FEC and return payload
                        let data_bytes = Bytes::copy_from_slice(game_data);
                        decoder.receive_data(&fec_hdr, data_bytes.clone());
                        return Ok((header, data_bytes, proxy_addr));
                    }
                }
            }
        }

        // Non-FEC or FEC header not present
        let payload = Bytes::copy_from_slice(payload_slice);
        Ok((header, payload, proxy_addr))
    }

    /// Receive with a timeout.
    pub async fn recv_with_timeout(
        &mut self,
        timeout: Duration,
    ) -> Result<(TunnelHeader, Bytes, SocketAddrV4), TunnelError> {
        tokio::time::timeout(timeout, self.recv_from_proxy())
            .await
            .map_err(|_| TunnelError::Timeout(timeout.as_millis() as u64))?
    }

    /// Flush any partial FEC block (e.g., on shutdown or timeout).
    pub async fn flush_fec(&mut self, proxy_addr: SocketAddrV4) -> Result<(), TunnelError> {
        if let Some(ref mut encoder) = self.fec_encoder {
            if let Some((block_id, _k, parity_bytes)) = encoder.flush() {
                let socket = self.socket.as_ref().ok_or(TunnelError::NotConnected)?;
                let seq = self.next_sequence();
                let dummy_addr = SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 0);
                let header = TunnelHeader::new_fec(seq, now_us(), dummy_addr, dummy_addr);
                let fec_hdr = FecHeader::parity(block_id, 0);

                let mut buf = BytesMut::with_capacity(
                    HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len(),
                );
                buf.extend_from_slice(&header.encode());
                fec_hdr.encode(&mut buf);
                buf.extend_from_slice(&parity_bytes);

                socket.send_to(&buf, proxy_addr).await?;
                self.stats.fec_parity_sent.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Get FEC statistics.
    pub fn fec_stats(&self) -> Option<lightspeed_protocol::FecStats> {
        self.fec_decoder.as_ref().map(|d| d.stats())
    }

    /// Close the relay socket.
    pub fn close(&mut self) {
        self.socket = None;
    }
}
