//! # UDP Relay
//!
//! Manages the UDP sockets for sending wrapped packets to proxies
//! and receiving responses. This is the client-side data plane.

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicU16, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use bytes::Bytes;
use tokio::net::UdpSocket;

use crate::error::TunnelError;
use lightspeed_protocol::TunnelHeader;

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
        }
    }
}

/// UDP relay — sends tunnel packets to proxy and receives responses.
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
        }
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
    /// Wraps the raw game payload in a LightSpeed header and sends it.
    pub async fn send_to_proxy(
        &self,
        payload: &[u8],
        orig_src: SocketAddrV4,
        orig_dst: SocketAddrV4,
        proxy_addr: SocketAddrV4,
    ) -> Result<usize, TunnelError> {
        let socket = self.socket.as_ref().ok_or(TunnelError::NotConnected)?;

        let seq = self.next_sequence();
        let header = TunnelHeader::new(seq, now_us(), orig_src, orig_dst);

        // Encode header + payload into a single buffer (zero-copy where possible)
        let packet = header.encode_with_payload(payload);

        let sent = socket.send_to(&packet, proxy_addr).await?;

        // Update stats
        self.stats.packets_sent.fetch_add(1, Ordering::Relaxed);
        self.stats.bytes_sent.fetch_add(sent as u64, Ordering::Relaxed);

        tracing::trace!(
            seq = seq,
            payload_len = payload.len(),
            proxy = %proxy_addr,
            "Sent tunnel packet"
        );

        Ok(sent)
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
    /// Returns the decoded header, payload, and the proxy address it came from.
    pub async fn recv_from_proxy(&self) -> Result<(TunnelHeader, Bytes, SocketAddrV4), TunnelError> {
        let socket = self.socket.as_ref().ok_or(TunnelError::NotConnected)?;

        let mut buf = vec![0u8; self.recv_buf_size];
        let (len, addr) = socket.recv_from(&mut buf).await?;

        // Decode header (will return DecodeError → TunnelError::Decode if too small)
        let (header, payload_slice) = TunnelHeader::decode_with_payload(&buf[..len])?;
        let payload = Bytes::copy_from_slice(payload_slice);

        // Convert addr to SocketAddrV4
        let proxy_addr = match addr {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => {
                return Err(TunnelError::Relay("IPv6 not supported yet".into()));
            }
        };

        // Update stats
        self.stats.packets_received.fetch_add(1, Ordering::Relaxed);
        self.stats.bytes_received.fetch_add(len as u64, Ordering::Relaxed);

        // Measure RTT from timestamp
        let now = now_us();
        let rtt_us = now.wrapping_sub(header.timestamp_us);
        tracing::trace!(
            seq = header.sequence,
            payload_len = payload.len(),
            rtt_us = rtt_us,
            "Received tunnel response"
        );

        Ok((header, payload, proxy_addr))
    }

    /// Receive with a timeout.
    pub async fn recv_with_timeout(
        &self,
        timeout: Duration,
    ) -> Result<(TunnelHeader, Bytes, SocketAddrV4), TunnelError> {
        tokio::time::timeout(timeout, self.recv_from_proxy())
            .await
            .map_err(|_| TunnelError::Timeout(timeout.as_millis() as u64))?
    }

    /// Close the relay socket.
    pub fn close(&mut self) {
        self.socket = None;
    }
}
