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

use bytes::Bytes;
use lightspeed_protocol::{
    build_fec_data_packet, build_fec_parity_packet, decode_fec_payload, FecDecoder, FecEncoder,
    FecHeader, TunnelHeader,
};
use tokio::net::UdpSocket;

use crate::error::TunnelError;
use crate::tunnel::adaptive::{AdaptiveConfig, AdaptiveFec, AdaptiveStats};
use crate::tunnel::budget::{self, PayloadFit};
use crate::tunnel::pacer::{Pacer, DEFAULT_CEILING_BPS};
use crate::tunnel::transport::TunnelTransport;

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
    /// Game payloads forwarded with fragmentation allowed for exceeding the
    /// conservative tunnel budget.
    pub payloads_over_budget: AtomicU64,
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
            payloads_over_budget: AtomicU64::new(0),
        }
    }
}

/// UDP relay — sends tunnel packets to proxy and receives responses.
///
/// Supports optional FEC for packet loss recovery. When FEC is enabled:
/// - Outbound: packets are grouped into blocks, XOR parity generated
/// - Inbound: packets are tracked, lost packets recovered from parity
pub struct UdpRelay {
    /// The transport for tunnel traffic (UDP or TCP).
    transport: Option<TunnelTransport>,
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
    /// Adaptive FEC/duplication controller, when the opt-in mode is enabled.
    adaptive: Option<AdaptiveFec>,
    /// Previous response RTT, for the jitter estimate feeding the controller.
    last_rtt_us: Option<u32>,
    /// Conservative outbound pacer. Bounds the burst rate and backs off on
    /// measured loss; see [`crate::tunnel::pacer`].
    pacer: Pacer,
}

impl UdpRelay {
    /// Create a new UDP relay bound to the specified address.
    pub fn new(local_addr: SocketAddrV4) -> Self {
        Self {
            transport: None,
            local_addr,
            recv_buf_size: 2048,
            sequence: AtomicU16::new(0),
            stats: Arc::new(RelayStats::new()),
            fec_encoder: None,
            fec_decoder: None,
            adaptive: None,
            last_rtt_us: None,
            pacer: Pacer::with_ceiling(DEFAULT_CEILING_BPS, DEFAULT_CEILING_BPS),
        }
    }

    /// The outbound pacer for observability and loss feedback.
    pub fn pacer(&self) -> &Pacer {
        &self.pacer
    }

    /// The outbound pacer, mutably, for loss feedback.
    pub fn pacer_mut(&mut self) -> &mut Pacer {
        &mut self.pacer
    }

    /// Enable FEC with the given block size (K data packets per parity).
    pub fn with_fec(mut self, k_size: u8) -> Self {
        self.fec_encoder = Some(FecEncoder::new(k_size));
        self.fec_decoder = Some(FecDecoder::new());
        self
    }

    /// Enable adaptive FEC and loss-gated duplication.
    ///
    /// This is the opt-in mode. On a clean link the encoder accumulates its
    /// blocks but emits no parity, and the multipath spread is collapsed to the
    /// single current relay. Parity returns only when loss is observed and is
    /// hard-bounded by [`AdaptiveConfig::max_overhead_pct`]. Calling this also
    /// enables the FEC encoder/decoder at the effective block size.
    pub fn with_adaptive_fec(mut self, cfg: AdaptiveConfig) -> Self {
        if cfg.enabled {
            let k = cfg.effective_k();
            self.fec_encoder = Some(FecEncoder::new(k));
            self.fec_decoder = Some(FecDecoder::new());
            self.adaptive = Some(AdaptiveFec::new(cfg));
            // Start clean: duplication is earned only by measured loss/jitter.
            crate::session::set_duplication_allowed(false);
        }
        self
    }

    /// The adaptive controller, when the opt-in mode is enabled.
    pub fn adaptive(&self) -> Option<&AdaptiveFec> {
        self.adaptive.as_ref()
    }

    /// Snapshot of the adaptive controller's parity/duplication state.
    pub fn adaptive_stats(&self) -> Option<AdaptiveStats> {
        self.adaptive.as_ref().map(AdaptiveFec::stats)
    }

    /// Check if FEC is enabled.
    pub fn fec_enabled(&self) -> bool {
        self.fec_encoder.is_some()
    }

    /// Feed one link observation to the adaptive controller and publish the
    /// resulting duplication decision to the process-wide gate.
    fn observe_link(&mut self, loss_pct: f64, jitter_ms: f32) {
        let Some(ctrl) = self.adaptive.as_mut() else {
            return;
        };
        ctrl.observe_sample(loss_pct, jitter_ms);
        crate::session::set_duplication_allowed(ctrl.should_duplicate());
    }

    /// Loss percentage a single FEC recovery implies, as one packet lost from a
    /// block of the effective size.
    fn recovery_loss_pct(&self) -> f64 {
        match self.adaptive.as_ref() {
            Some(ctrl) => 100.0 / f64::from(ctrl.effective_k()),
            None => 100.0 / 4.0,
        }
    }

    /// Bind the UDP socket.
    pub async fn bind(&mut self) -> Result<(), TunnelError> {
        let transport = TunnelTransport::connect_udp(self.local_addr).await?;

        // Set socket buffer sizes for low-latency gaming traffic
        tracing::info!("UDP relay bound to {}", transport.local_addr()?);
        self.transport = Some(transport);
        Ok(())
    }

    /// Connect a TCP transport to the proxy (UDP-restricted networks).
    pub async fn connect_tcp(&mut self, proxy_addr: SocketAddrV4) -> Result<(), TunnelError> {
        let transport = TunnelTransport::connect_tcp(proxy_addr).await?;
        tracing::info!("TCP relay connected to {}", proxy_addr);
        self.transport = Some(transport);
        Ok(())
    }

    /// Get a clone of the UDP socket (UDP transports only; `None` for TCP).
    pub fn socket(&self) -> Option<Arc<UdpSocket>> {
        match &self.transport {
            Some(TunnelTransport::Udp { socket, .. }) => Some(Arc::clone(socket)),
            _ => None,
        }
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
        // Point the UDP transport at the target proxy (TCP's peer is fixed).
        if let Some(transport) = self.transport.as_mut() {
            transport.set_proxy(proxy_addr);
        }
        let transport = self.transport.as_ref().ok_or(TunnelError::NotConnected)?;

        let fit = budget::classify(payload.len(), self.fec_encoder.is_some());
        if let PayloadFit::OverBudget {
            payload_len,
            budget,
        } = fit
        {
            self.stats
                .payloads_over_budget
                .fetch_add(1, Ordering::Relaxed);
            tracing::warn!(
                payload_len = payload_len,
                budget = budget,
                "Forwarding oversized game payload; the kernel may fragment it on the client-to-relay hop"
            );
        }

        let path_token = crate::session::path_token(proxy_addr);

        // Get sequence numbers upfront to avoid borrow conflicts
        let seq = self.next_sequence();
        if let Some(encoder) = self.fec_encoder.as_mut() {
            // ── FEC mode: encode with FEC header ────────────────
            let block_id = encoder.block_id();
            let index = encoder.current_index();
            let k_size = encoder.k_size();

            let header = TunnelHeader::new_fec(seq, now_us(), orig_src, orig_dst)
                .with_session_token(path_token);
            let fec_hdr = FecHeader::data(block_id, index, k_size);
            let pkt_buf = build_fec_data_packet(&header, &fec_hdr, payload);

            let parity = encoder.add_packet(payload);
            // Adaptive mode: drop the block's parity while the link is clean.
            let emit_parity = self
                .adaptive
                .as_ref()
                .map(|ctrl| ctrl.parity_k().is_some())
                .unwrap_or(true);
            if parity.is_some() {
                if let Some(ctrl) = self.adaptive.as_mut() {
                    ctrl.record_block(emit_parity);
                }
            }

            self.pacer.acquire(pkt_buf.len()).await;
            let sent = if budget::datagram_fits(pkt_buf.len()) {
                transport.send(&pkt_buf).await?
            } else {
                transport.send_may_fragment(&pkt_buf).await?
            };
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

            if let Some(parity_bytes) = parity.filter(|_| emit_parity) {
                let parity_seq = self.next_sequence();
                let parity_header = TunnelHeader::new_fec(parity_seq, now_us(), orig_src, orig_dst)
                    .with_session_token(path_token);
                let parity_fec = FecHeader::parity(block_id, k_size);
                let parity_buf =
                    build_fec_parity_packet(&parity_header, &parity_fec, &parity_bytes);

                self.pacer.acquire(parity_buf.len()).await;
                let parity_sent = if budget::datagram_fits(parity_buf.len()) {
                    transport.send(&parity_buf).await?
                } else {
                    transport.send_may_fragment(&parity_buf).await?
                };
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
            let header =
                TunnelHeader::new(seq, now_us(), orig_src, orig_dst).with_session_token(path_token);
            let packet = header.encode_with_payload(payload);

            self.pacer.acquire(packet.len()).await;
            let sent = if budget::datagram_fits(packet.len()) {
                transport.send(&packet).await?
            } else {
                transport.send_may_fragment(&packet).await?
            };

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
    pub async fn send_keepalive(&mut self, proxy_addr: SocketAddrV4) -> Result<(), TunnelError> {
        if let Some(transport) = self.transport.as_mut() {
            transport.set_proxy(proxy_addr);
        }
        let transport = self.transport.as_ref().ok_or(TunnelError::NotConnected)?;

        let seq = self.next_sequence();
        let path_token = crate::session::path_token(proxy_addr);
        let header = TunnelHeader::keepalive(seq, now_us()).with_session_token(path_token);
        let packet = header.encode_to_array();

        transport.send(&packet).await?;
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
        let mut buf = vec![0u8; self.recv_buf_size];
        let (len, proxy_addr) = {
            let transport = self.transport.as_mut().ok_or(TunnelError::NotConnected)?;
            let len = match transport.recv(&mut buf).await? {
                Some(n) => n,
                None => return Err(TunnelError::Relay("tunnel closed".into())),
            };
            (len, transport.proxy_addr())
        };

        // Decode header
        let (header, payload_slice) = TunnelHeader::decode_with_payload(&buf[..len])?;

        // Update stats
        self.stats.packets_received.fetch_add(1, Ordering::Relaxed);
        self.stats
            .bytes_received
            .fetch_add(len as u64, Ordering::Relaxed);

        // A healthy response nudges the paced rate back up, slowly.
        self.pacer.maybe_recover();

        // Measure RTT from timestamp
        let now = now_us();
        let rtt_us = now.wrapping_sub(header.timestamp_us);
        let jitter_us = self
            .last_rtt_us
            .map(|last| rtt_us.abs_diff(last))
            .unwrap_or(0);
        self.last_rtt_us = Some(rtt_us);
        let jitter_ms = jitter_us as f32 / 1000.0;
        tracing::trace!(
            seq = header.sequence,
            payload_len = payload_slice.len(),
            rtt_us = rtt_us,
            "Received tunnel response"
        );

        // Handle FEC if enabled and packet has FEC flag. The decoder borrow is
        // released before the adaptive controller observes the outcome.
        let (payload, recovered) = match self.fec_decoder.as_mut() {
            Some(decoder) if header.has_fec() => match decode_fec_payload(payload_slice, decoder) {
                Some(data) => (data, true),
                // Parity consumed, no recovery needed: return an empty payload.
                None => (Bytes::new(), false),
            },
            _ => (Bytes::copy_from_slice(payload_slice), false),
        };

        if recovered {
            self.stats.fec_recovered.fetch_add(1, Ordering::Relaxed);
            self.pacer.on_loss();
            self.observe_link(self.recovery_loss_pct(), jitter_ms);
            tracing::info!(
                block = header.sequence,
                recovered_len = payload.len(),
                "🔧 FEC recovered lost packet"
            );
        } else {
            self.observe_link(0.0, jitter_ms);
        }

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
        let emit = self
            .adaptive
            .as_ref()
            .map(|ctrl| ctrl.parity_k().is_some())
            .unwrap_or(true);
        if let Some(ref mut encoder) = self.fec_encoder {
            if let Some((block_id, _k, parity_bytes)) = encoder.flush() {
                if !emit {
                    return Ok(());
                }
                if let Some(transport) = self.transport.as_mut() {
                    transport.set_proxy(proxy_addr);
                }
                let transport = self.transport.as_ref().ok_or(TunnelError::NotConnected)?;
                let seq = self.next_sequence();
                let dummy_addr = SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 0);
                let path_token = crate::session::path_token(proxy_addr);
                let header = TunnelHeader::new_fec(seq, now_us(), dummy_addr, dummy_addr)
                    .with_session_token(path_token);
                let fec_hdr = FecHeader::parity(block_id, 0);
                let buf = build_fec_parity_packet(&header, &fec_hdr, &parity_bytes);

                transport.send(&buf).await?;
                self.stats.fec_parity_sent.fetch_add(1, Ordering::Relaxed);
            }
        }
        Ok(())
    }

    /// Get FEC statistics.
    pub fn fec_stats(&self) -> Option<lightspeed_protocol::FecStats> {
        self.fec_decoder.as_ref().map(|d| d.stats())
    }

    /// Close the relay transport.
    pub fn close(&mut self) {
        self.transport = None;
    }
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::time::Duration;

    use tokio::net::UdpSocket;

    use super::*;

    #[test]
    fn send_to_proxy_stamps_the_destination_path_token() {
        let _guard = crate::session::token_test_guard();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        rt.block_on(async {
            let receiver = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
                .await
                .expect("bind receiver");
            let proxy_addr = match receiver.local_addr().expect("receiver addr") {
                std::net::SocketAddr::V4(v4) => v4,
                std::net::SocketAddr::V6(_) => panic!("expected IPv4"),
            };
            crate::session::set_session_token(0xDEAD_BEEF);
            crate::session::set_path_token(proxy_addr, 0x1234_5678);

            let mut relay = UdpRelay::new(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
            relay.bind().await.expect("bind relay");
            let src = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 40000);
            let dst = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 40001);
            relay
                .send_to_proxy(b"payload", src, dst, proxy_addr)
                .await
                .expect("send through tunnel");

            let mut buf = vec![0u8; 2048];
            let (n, _) = tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buf))
                .await
                .expect("tunnel packet within 2s")
                .expect("receive tunnel packet");
            let (header, _) = TunnelHeader::decode_with_payload(&buf[..n]).expect("decode header");
            assert_eq!(
                header.session_token, 0x1234_5678,
                "the data plane must stamp the destination's per-path token, not the default"
            );

            crate::session::reset_all_tokens();
        });
    }

    #[test]
    fn oversized_payload_is_forwarded_and_counted() {
        let _guard = crate::session::token_test_guard();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        rt.block_on(async {
            let receiver = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
                .await
                .expect("bind receiver");
            let proxy_addr = match receiver.local_addr().expect("receiver addr") {
                std::net::SocketAddr::V4(v4) => v4,
                std::net::SocketAddr::V6(_) => panic!("expected IPv4"),
            };
            crate::session::set_session_token(0xDEAD_BEEF);
            crate::session::set_path_token(proxy_addr, 0x1234_5678);

            let mut relay = UdpRelay::new(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0));
            relay.bind().await.expect("bind relay");

            // 200 bytes over the plain budget: the clamp used to drop this.
            let budget = lightspeed_protocol::max_game_payload(false);
            let payload = vec![0xA5u8; budget + 200];
            let src = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 40000);
            let dst = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 40001);

            let sent = relay
                .send_to_proxy(&payload, src, dst, proxy_addr)
                .await
                .expect("send oversized payload");
            assert!(
                sent > 0,
                "an oversized payload must not be silently dropped (sent={sent})"
            );

            let mut buf = vec![0u8; 4096];
            let (n, _) = tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buf))
                .await
                .expect("oversized tunnel packet within 2s")
                .expect("receive oversized tunnel packet");
            let (_, body) = TunnelHeader::decode_with_payload(&buf[..n]).expect("decode header");
            assert_eq!(body, payload.as_slice(), "payload must arrive unchanged");
            assert_eq!(
                relay.stats.payloads_over_budget.load(Ordering::Relaxed),
                1,
                "an oversized payload must be counted"
            );

            crate::session::reset_all_tokens();
        });
    }

    #[test]
    fn adaptive_fec_sends_no_parity_clean_and_returns_it_under_loss() {
        let _guard = crate::session::token_test_guard();
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("test runtime");

        rt.block_on(async {
            let receiver = UdpSocket::bind(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0))
                .await
                .expect("bind receiver");
            let proxy_addr = match receiver.local_addr().expect("receiver addr") {
                std::net::SocketAddr::V4(v4) => v4,
                std::net::SocketAddr::V6(_) => panic!("expected IPv4"),
            };
            crate::session::set_session_token(0xDEAD_BEEF);
            crate::session::set_path_token(proxy_addr, 0x1234_5678);

            let cfg = crate::tunnel::adaptive::AdaptiveConfig {
                enabled: true,
                ..crate::tunnel::adaptive::AdaptiveConfig::default()
            };
            let mut relay =
                UdpRelay::new(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 0)).with_adaptive_fec(cfg);
            relay.bind().await.expect("bind relay");

            assert!(relay.fec_enabled(), "adaptive FEC must enable the codec");
            let clean = relay.adaptive_stats().expect("adaptive stats");
            assert_eq!(clean.parity_ratio, 0.0, "a new link starts with no parity");
            assert!(!clean.duplicating);
            assert_eq!(clean.effective_k, 4);
            assert_eq!(clean.overhead_bound_pct, 25);

            let src = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 40000);
            let dst = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 2), 40001);

            // One full K=4 block on a clean link: no parity may be emitted.
            for _ in 0..4 {
                relay
                    .send_to_proxy(b"clean", src, dst, proxy_addr)
                    .await
                    .expect("send clean block");
            }
            let mut parity_seen = 0;
            for _ in 0..4 {
                let mut buf = vec![0u8; 2048];
                let (n, _) =
                    tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buf))
                        .await
                        .expect("clean packet within 2s")
                        .expect("receive clean packet");
                let (header, body) =
                    TunnelHeader::decode_with_payload(&buf[..n]).expect("decode header");
                assert!(header.has_fec(), "adaptive mode still uses FEC headers");
                let mut fec_slice: &[u8] = &body[..4];
                let fec = FecHeader::decode(&mut fec_slice).expect("decode FEC header");
                if fec.is_parity() {
                    parity_seen += 1;
                }
            }
            assert_eq!(parity_seen, 0, "a clean link must send no parity");
            assert_eq!(relay.adaptive_stats().unwrap().parity_ratio, 0.0);

            // Loss is observed: the next block must carry parity.
            relay.observe_link(50.0, 0.0);
            assert_eq!(relay.adaptive_stats().unwrap().parity_ratio, 0.25);
            for _ in 0..4 {
                relay
                    .send_to_proxy(b"lossy", src, dst, proxy_addr)
                    .await
                    .expect("send lossy block");
            }
            let mut lossy_parity = 0;
            let mut lossy_data = 0;
            for _ in 0..5 {
                let mut buf = vec![0u8; 2048];
                let (n, _) =
                    tokio::time::timeout(Duration::from_secs(2), receiver.recv_from(&mut buf))
                        .await
                        .expect("lossy packet within 2s")
                        .expect("receive lossy packet");
                let (_, body) =
                    TunnelHeader::decode_with_payload(&buf[..n]).expect("decode header");
                let mut fec_slice: &[u8] = &body[..4];
                let fec = FecHeader::decode(&mut fec_slice).expect("decode FEC header");
                if fec.is_parity() {
                    lossy_parity += 1;
                } else {
                    lossy_data += 1;
                }
            }
            assert_eq!(lossy_data, 4, "all four data packets arrive");
            assert_eq!(lossy_parity, 1, "one parity packet follows the loss");

            let stats = relay.adaptive_stats().unwrap();
            assert_eq!(stats.blocks_data, 2, "two completed blocks");
            assert_eq!(stats.blocks_parity, 1, "one block carried parity");
            assert!(stats.overhead_bound_pct <= 25);

            crate::session::reset_all_tokens();
        });
    }
}
