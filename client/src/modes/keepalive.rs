//! Keepalive (idle) mode — maintains a proxy session with periodic pings.
//!
//! Used when no `--game-server` is specified.  Sends keepalive packets every
//! 5 s, measures round-trip latency for the online learner, and prints tunnel
//! stats every 15 s until Ctrl+C.

use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tracing::{info, warn};

use crate::ml;
use crate::tunnel::relay::UdpRelay;

/// Run the keepalive (idle) mode.
///
/// Blocks until Ctrl+C, then saves the online-learning state and returns.
pub async fn run_keepalive_mode(
    relay: UdpRelay,
    proxy_addr: SocketAddrV4,
    proxy_id: String,
    proxy_region: String,
    online_learner: Arc<tokio::sync::Mutex<ml::online::OnlineLearner>>,
    keepalive_timestamps: Arc<tokio::sync::Mutex<HashMap<u16, std::time::Instant>>>,
) -> anyhow::Result<()> {
    let stats = Arc::clone(&relay.stats);

    info!("⚡ LightSpeed tunnel active — keepalive mode");
    info!("   Use --game-server <ip:port> for full redirect mode");
    info!("   (Full packet capture requires --features pcap-capture)");

    // ── Keepalive sender ──────────────────────────────────────────
    let keepalive_handle = {
        let relay_socket = relay.socket().expect("socket bound");
        let ka_timestamps = Arc::clone(&keepalive_timestamps);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            let mut seq: u16 = 0;
            loop {
                interval.tick().await;
                let header = lightspeed_protocol::TunnelHeader::keepalive(
                    seq,
                    std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32,
                );
                let packet = header.encode_to_array();
                match relay_socket.send_to(&packet, proxy_addr).await {
                    Ok(_) => {
                        let mut ts_map = ka_timestamps.lock().await;
                        ts_map.insert(seq, std::time::Instant::now());
                        // Evict entries older than 30 s
                        ts_map.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                        tracing::trace!(seq = seq, "Sent keepalive to proxy");
                    }
                    Err(e) => warn!("Keepalive send failed: {}", e),
                }
                seq = seq.wrapping_add(1);
            }
        })
    };

    // ── Response receiver (computes RTT for online learning) ──────
    let recv_handle = {
        let stats = Arc::clone(&stats);
        let relay_socket = relay.socket().expect("socket bound");
        let ka_timestamps = Arc::clone(&keepalive_timestamps);
        let learner_ref = Arc::clone(&online_learner);
        let ka_proxy_id = proxy_id.clone();
        let ka_proxy_region = proxy_region.clone();
        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            loop {
                match relay_socket.recv_from(&mut buf).await {
                    Ok((len, addr)) => {
                        match lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len]) {
                            Ok((header, payload)) => {
                                stats.packets_received.fetch_add(1, Ordering::Relaxed);
                                stats.bytes_received.fetch_add(len as u64, Ordering::Relaxed);

                                if header.is_keepalive() {
                                    let rtt_us = {
                                        let mut ts_map = ka_timestamps.lock().await;
                                        ts_map.remove(&header.sequence).map(|send_time| {
                                            send_time.elapsed().as_micros() as u64
                                        })
                                    };
                                    if let Some(rtt) = rtt_us {
                                        let latency_ms = rtt as f64 / 1000.0;
                                        tracing::trace!(
                                            seq = header.sequence,
                                            from = %addr,
                                            rtt_ms = latency_ms,
                                            "Keepalive echo: {:.1}ms",
                                            latency_ms,
                                        );
                                        let mut learner = learner_ref.lock().await;
                                        learner.record_and_maybe_retrain(
                                            &ka_proxy_id,
                                            &ka_proxy_region,
                                            latency_ms,
                                            0.0,
                                            0.0,
                                            0.0,
                                        );
                                    } else {
                                        tracing::trace!(
                                            seq = header.sequence,
                                            from = %addr,
                                            "Keepalive echo received"
                                        );
                                    }
                                } else {
                                    tracing::debug!(
                                        seq = header.sequence,
                                        payload_len = payload.len(),
                                        from = %addr,
                                        "Tunnel response received"
                                    );
                                }
                            }
                            Err(e) => tracing::debug!(error = %e, "Invalid response packet"),
                        }
                    }
                    Err(e) => warn!("Recv error: {}", e),
                }
            }
        })
    };

    // ── Stats logger ──────────────────────────────────────────────
    let stats_handle = {
        let stats = Arc::clone(&stats);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(15));
            loop {
                interval.tick().await;
                let sent = stats.packets_sent.load(Ordering::Relaxed);
                let recv = stats.packets_received.load(Ordering::Relaxed);
                let bytes_out = stats.bytes_sent.load(Ordering::Relaxed);
                let bytes_in = stats.bytes_received.load(Ordering::Relaxed);
                info!(
                    sent = sent,
                    recv = recv,
                    bytes_out = bytes_out,
                    bytes_in = bytes_in,
                    "📊 Tunnel stats"
                );
            }
        })
    };

    info!("⚡ Press Ctrl+C to stop");
    tokio::signal::ctrl_c().await?;
    info!("⚡ Shutdown signal received");

    keepalive_handle.abort();
    recv_handle.abort();
    stats_handle.abort();

    // Save online learning state
    {
        let learner = online_learner.lock().await;
        learner.save_state();
        let summary = learner.summary();
        info!(
            "🧠 Online learning: {} total measurements, {} retrains",
            summary.total_measurements, summary.retrain_count
        );
    }

    info!("⚡ LightSpeed shut down cleanly");
    Ok(())
}
