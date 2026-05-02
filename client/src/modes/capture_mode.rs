//! `--capture` mode: pcap-based bidirectional packet capture and injection.
//!
//! Captures game UDP packets from the network interface, tunnels them through
//! the LightSpeed proxy (with optional FEC), and injects proxy responses back
//! to the game client at the raw-socket level.
//!
//! ## Known Limitations
//!
//! ### Why pcap captures bidirectional traffic (and why we filter it)
//! pcap's BPF filter matches on port numbers regardless of direction.
//! A filter like `udp port 28015` will match both:
//!   - OUTBOUND: `src=192.168.x.x:GAME_PORT → dst=SERVER:28015`  (we want this)
//!   - INBOUND:  `src=SERVER:28015 → dst=192.168.x.x:GAME_PORT`  (we DON'T want this)
//!
//! If the inbound packets are tunneled, the proxy receives them with
//! `orig_dst_addr = 192.168.x.x` (a private/RFC1918 IP) and drops them via
//! the PrivateDestination abuse check — causing ~50% of all tunneled packets
//! to be silently dropped at the proxy. We filter them out here first.
//!
//! ### Windows Firewall and tunnel inbound
//! The tunnel socket binds to an ephemeral port (0.0.0.0:0). Windows Defender
//! Firewall blocks unsolicited inbound UDP on ephemeral ports by default, even
//! when the application sent an outbound packet first (NAT hole punch). We add
//! a temporary inbound rule for the process while capture is active.
//!
//! ### pcap is passive — it cannot redirect traffic
//! pcap observes a copy of each packet. The original packet still travels the
//! direct path (client → game server). The tunnel creates a *parallel* path
//! that the game client never uses for its active session. Actual ping
//! optimisation requires WinDivert (Windows) or nftables/iptables redirect
//! (Linux) to intercept and drop the original packet.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use tracing::info;

use crate::games::GameConfig;
use crate::ml;

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Returns `true` for RFC1918 private ranges, loopback, and link-local.
///
/// Used to filter out the *inbound* half of pcap captures: pcap BPF filters
/// match on port regardless of direction, so both outbound (src=local,
/// dst=server) and inbound (src=server, dst=local private IP) packets are
/// captured. Only outbound packets should be tunneled; inbound packets would
/// arrive at the proxy with `orig_dst = 192.168.x.x` and be dropped by the
/// PrivateDestination abuse check anyway.
#[inline]
fn is_private_ipv4(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_loopback()                          // 127.0.0.0/8
        || ip.is_link_local()                 // 169.254.0.0/16
        || a == 10                            // 10.0.0.0/8
        || (a == 172 && (16..=31).contains(&b)) // 172.16.0.0/12
        || (a == 192 && b == 168) // 192.168.0.0/16
}

// ─────────────────────────────────────────────────────────────────────────────
//  Windows Firewall helpers (no-ops on non-Windows)
// ─────────────────────────────────────────────────────────────────────────────

/// Name used for the temporary Windows Firewall inbound allow rule.
const FIREWALL_RULE_NAME: &str = "LightSpeed Tunnel Inbound";

/// Add a Windows Firewall inbound UDP allow rule for this process.
///
/// The tunnel socket binds to an ephemeral port. Windows Defender Firewall
/// blocks unsolicited inbound UDP even when a NAT hole-punch exists, so
/// proxy responses never reach the tunnel socket. This rule lets them through
/// while capture is active. The rule is removed in `remove_firewall_rule`.
///
/// Requires the process to be running as Administrator.
#[cfg(target_os = "windows")]
fn add_firewall_rule() {
    let exe = std::env::current_exe().unwrap_or_default();
    let exe_str = exe.to_string_lossy();
    let output = std::process::Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={}", FIREWALL_RULE_NAME),
            "protocol=UDP",
            "dir=in",
            "action=allow",
            &format!("program={}", exe_str),
        ])
        .output();
    match output {
        Ok(o) if o.status.success() => {
            tracing::info!("🔓 Windows Firewall: added inbound UDP allow rule for LightSpeed");
        }
        Ok(o) => {
            tracing::warn!(
                "Windows Firewall rule add may have failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => {
            tracing::warn!("Failed to run netsh (need Administrator?): {}", e);
        }
    }
}

/// Remove the Windows Firewall rule added by [`add_firewall_rule`].
#[cfg(target_os = "windows")]
fn remove_firewall_rule() {
    let _ = std::process::Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={}", FIREWALL_RULE_NAME),
        ])
        .output();
    tracing::info!("🔒 Windows Firewall: removed LightSpeed inbound rule");
}

#[cfg(not(target_os = "windows"))]
fn add_firewall_rule() {}
#[cfg(not(target_os = "windows"))]
fn remove_firewall_rule() {}

/// Run the pcap capture mode.
///
/// # Parameters
/// * `game`        — game profile (name, ports, BPF filter, etc.)
/// * `proxy_addr`  — tunnel endpoint address
/// * `proxy_id`    — human-readable proxy identifier for online learning
/// * `proxy_region`— region string for online learning records
/// * `online_learner` — shared online-learning state
/// * `keepalive_timestamps` — shared seq → send-time map for RTT measurement
/// * `fec_enabled` — whether to wrap packets with FEC headers
/// * `fec_k`       — FEC block size (data packets per parity packet)
/// * `interface`   — optional NIC name to capture on (auto-detect if `None`)
///
/// Live stat handles filled by the capture task once it is running.
/// Engine holds a `Arc<Mutex<Option<CaptureStatHandles>>>` slot; the task fills it.
///
/// Fields are read from `engine.rs` (bin target), so suppress dead_code in lib target.
#[allow(dead_code)]
pub struct CaptureStatHandles {
    /// Game → Proxy captured packets.
    pub outbound_packets: Arc<std::sync::atomic::AtomicU64>,
    /// Game → Proxy captured bytes.
    pub outbound_bytes: Arc<std::sync::atomic::AtomicU64>,
    /// Injector stats (from_proxy, injected, errors, fec_recovered).
    pub injector_stats: Arc<crate::capture::injector::InjectorStats>,
}

/// Slot type passed into `run_capture_mode_with_shutdown` so the engine can read live stats.
pub type CaptureStatSlot = Arc<std::sync::Mutex<Option<CaptureStatHandles>>>;

#[allow(clippy::too_many_arguments)]
pub async fn run_capture_mode(
    game: &dyn GameConfig,
    proxy_addr: SocketAddrV4,
    proxy_id: String,
    proxy_region: String,
    online_learner: Arc<tokio::sync::Mutex<ml::online::OnlineLearner>>,
    keepalive_timestamps: Arc<tokio::sync::Mutex<HashMap<u16, std::time::Instant>>>,
    fec_enabled: bool,
    fec_k: u8,
    interface: Option<String>,
) -> anyhow::Result<()> {
    run_capture_mode_inner(
        game,
        proxy_addr,
        proxy_id,
        proxy_region,
        online_learner,
        keepalive_timestamps,
        fec_enabled,
        fec_k,
        interface,
        None,
        None,
    )
    .await
}

/// Like [`run_capture_mode`] but accepts an external shutdown oneshot for GUI-driven stop,
/// an optional slot the function fills with live stat handles once running,
/// and an optional pre-built BPF filter string (avoids re-running netstat inside the task).
///
/// Used from `engine.rs` (bin target), so suppress dead_code in lib target.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)]
pub async fn run_capture_mode_with_shutdown(
    game: &dyn GameConfig,
    proxy_addr: SocketAddrV4,
    proxy_id: String,
    proxy_region: String,
    online_learner: Arc<tokio::sync::Mutex<ml::online::OnlineLearner>>,
    keepalive_timestamps: Arc<tokio::sync::Mutex<HashMap<u16, std::time::Instant>>>,
    fec_enabled: bool,
    fec_k: u8,
    interface: Option<String>,
    shutdown_rx: tokio::sync::oneshot::Receiver<()>,
    stat_slot: Option<CaptureStatSlot>,
) -> anyhow::Result<()> {
    run_capture_mode_inner(
        game,
        proxy_addr,
        proxy_id,
        proxy_region,
        online_learner,
        keepalive_timestamps,
        fec_enabled,
        fec_k,
        interface,
        Some(shutdown_rx),
        stat_slot,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn run_capture_mode_inner(
    game: &dyn GameConfig,
    proxy_addr: SocketAddrV4,
    proxy_id: String,
    proxy_region: String,
    online_learner: Arc<tokio::sync::Mutex<ml::online::OnlineLearner>>,
    keepalive_timestamps: Arc<tokio::sync::Mutex<HashMap<u16, std::time::Instant>>>,
    fec_enabled: bool,
    fec_k: u8,
    interface: Option<String>,
    shutdown_rx: Option<tokio::sync::oneshot::Receiver<()>>,
    stat_slot: Option<CaptureStatSlot>,
) -> anyhow::Result<()> {
    use bytes::BytesMut;
    use lightspeed_protocol::{FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};

    info!("🔍 Starting capture mode");
    info!(
        "   Game:      {} (anti-cheat: {})",
        game.name(),
        game.anti_cheat()
    );
    info!("   Ports:     {:?}", game.ports());
    info!("   Proxy:     {}", proxy_addr);
    if let Some(ref iface) = interface {
        info!("   Interface: {}", iface);
    } else {
        info!("   Interface: (auto-detect)");
    }

    let filter = game.build_capture_filter();
    info!("   BPF filter: {}", filter.bpf);

    // Create capture backend
    let mut cap_backend = if let Some(ref iface) = interface {
        match crate::capture::create_capture_on(iface) {
            Ok(c) => c,
            Err(e) => anyhow::bail!("Failed to create capture on '{}': {}", iface, e),
        }
    } else {
        match crate::capture::create_default_capture() {
            Ok(c) => c,
            Err(e) => anyhow::bail!(
                "Failed to create capture backend: {}\n   \
                 Ensure pcap-capture feature is enabled: cargo build --features pcap-capture",
                e
            ),
        }
    };

    // Start capture
    cap_backend.start(&filter).map_err(|e| {
        anyhow::anyhow!(
            "Capture start failed: {}\n   \
             You may need to run with elevated privileges (admin/root).",
            e
        )
    })?;

    // Add Windows Firewall inbound allow rule so proxy responses reach the
    // tunnel socket. Without this, Windows Defender Firewall drops inbound
    // UDP on ephemeral ports even when the app sent an outbound packet first.
    add_firewall_rule();

    info!(
        "⚡ Capture active — sniffing {} traffic on ports {:?}",
        game.name(),
        game.ports()
    );
    info!(
        "   Captured packets will be forwarded through proxy {}",
        proxy_addr
    );

    // Shared tunnel socket for outbound capture and inbound injection
    let tunnel_socket = Arc::new(tokio::net::UdpSocket::bind("0.0.0.0:0").await?);

    // Create packet injector for bidirectional response delivery
    #[cfg(feature = "pcap-capture")]
    let injector = if let Some(iface) = cap_backend.interface_name() {
        crate::capture::injector::PacketInjector::with_interface(iface).await?
    } else {
        crate::capture::injector::PacketInjector::new().await?
    };
    #[cfg(not(feature = "pcap-capture"))]
    let injector = crate::capture::injector::PacketInjector::new().await?;

    let injector = Arc::new(injector);
    let injector_stats = Arc::clone(&injector.stats);

    // Track the game client's source address, server address, and MACs
    type CaptureMeta =
        Arc<tokio::sync::RwLock<Option<(SocketAddrV4, SocketAddrV4, [u8; 6], [u8; 6])>>>;
    let capture_meta: CaptureMeta = Arc::new(tokio::sync::RwLock::new(None));

    if fec_enabled {
        info!(
            "   FEC:       enabled (K={}, ~{}% overhead)",
            fec_k,
            100 / fec_k as u32
        );
    }
    info!("   Mode:      bidirectional (capture + inject)");
    info!("   Press Ctrl+C to stop\n");

    // Ctrl+C / external shutdown flag
    let running = Arc::new(std::sync::atomic::AtomicBool::new(true));
    {
        let running_ctrl = Arc::clone(&running);
        tokio::spawn(async move {
            let _ = tokio::signal::ctrl_c().await;
            running_ctrl.store(false, Ordering::Relaxed);
        });
    }
    // Also respond to GUI-driven shutdown (engine calls stop_capture).
    if let Some(rx) = shutdown_rx {
        let running_gui = Arc::clone(&running);
        tokio::spawn(async move {
            let _ = rx.await;
            running_gui.store(false, Ordering::Relaxed);
        });
    }

    // Shared outbound counters
    let outbound_packets = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let outbound_bytes = Arc::new(std::sync::atomic::AtomicU64::new(0));
    let start_time = std::time::Instant::now();

    // Fill the engine's stat slot so it can poll live counters via snapshot().
    if let Some(ref slot) = stat_slot {
        if let Ok(mut guard) = slot.lock() {
            *guard = Some(CaptureStatHandles {
                outbound_packets: Arc::clone(&outbound_packets),
                outbound_bytes: Arc::clone(&outbound_bytes),
                injector_stats: Arc::clone(&injector_stats),
            });
        }
    }

    // ── Inbound task: Proxy → Injector → Game ────────────────────
    let inbound_handle = {
        let tunnel_socket = Arc::clone(&tunnel_socket);
        let capture_meta = Arc::clone(&capture_meta);
        let running = Arc::clone(&running);
        let injector = Arc::clone(&injector);
        let injector_stats_ref = Arc::clone(&injector_stats);
        let ka_timestamps = Arc::clone(&keepalive_timestamps);
        let learner_ref = Arc::clone(&online_learner);
        let cap_proxy_id = proxy_id.clone();
        let cap_proxy_region = proxy_region.clone();

        tokio::spawn(async move {
            let mut buf = vec![0u8; 2048];
            let mut fec_decoder = if fec_enabled {
                Some(lightspeed_protocol::FecDecoder::new())
            } else {
                None
            };
            let mut gc_counter: u32 = 0;

            while running.load(Ordering::Relaxed) {
                let recv_result = tokio::time::timeout(
                    Duration::from_millis(100),
                    tunnel_socket.recv_from(&mut buf),
                )
                .await;

                let (len, _from) = match recv_result {
                    Ok(Ok(r)) => r,
                    Ok(Err(e)) => {
                        tracing::debug!("Tunnel recv error: {}", e);
                        continue;
                    }
                    Err(_) => continue, // Timeout — check running flag
                };

                injector_stats_ref
                    .packets_from_proxy
                    .fetch_add(1, Ordering::Relaxed);

                // Decode tunnel header
                let (header, payload) =
                    match lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len]) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::debug!("Invalid tunnel response: {}", e);
                            continue;
                        }
                    };

                // Process keepalive echoes — measure RTT for online learning
                if header.is_keepalive() {
                    let rtt_us = {
                        let mut ts_map = ka_timestamps.lock().await;
                        ts_map
                            .remove(&header.sequence)
                            .map(|send_time| send_time.elapsed().as_micros() as u64)
                    };
                    if let Some(rtt) = rtt_us {
                        let latency_ms = rtt as f64 / 1000.0;
                        tracing::trace!(
                            seq = header.sequence,
                            rtt_ms = latency_ms,
                            "Keepalive echo: {:.1}ms",
                            latency_ms,
                        );
                        let mut learner = learner_ref.lock().await;
                        learner.record_and_maybe_retrain(
                            &cap_proxy_id,
                            &cap_proxy_region,
                            latency_ms,
                            0.0,
                            0.0,
                            0.0,
                        );
                    } else {
                        tracing::trace!("Keepalive echo received (no send timestamp)");
                    }
                    continue;
                }

                // Get the game client's metadata (learned from outbound capture)
                let meta = {
                    let meta = capture_meta.read().await;
                    *meta
                };
                let (game_client, server_addr, mac_dst, mac_src) = match meta {
                    Some((c, s, ms, md)) => (c, s, md, ms), // swap MACs for injection
                    None => {
                        tracing::debug!("Response received but no game client captured yet");
                        continue;
                    }
                };

                // Handle FEC if enabled
                let game_payload: Option<bytes::Bytes> = if header.has_fec() {
                    if payload.len() < lightspeed_protocol::FEC_HEADER_SIZE {
                        tracing::debug!("FEC packet too short");
                        continue;
                    }

                    let mut fec_slice: &[u8] = &payload[..lightspeed_protocol::FEC_HEADER_SIZE];
                    let fec_hdr = match lightspeed_protocol::FecHeader::decode(&mut fec_slice) {
                        Some(h) => h,
                        None => {
                            tracing::debug!("Invalid FEC header in response");
                            continue;
                        }
                    };

                    let game_data = &payload[lightspeed_protocol::FEC_HEADER_SIZE..];

                    if let Some(decoder) = fec_decoder.as_mut() {
                        if fec_hdr.is_parity() {
                            let parity_data = bytes::Bytes::copy_from_slice(game_data);
                            if let Some((_idx, recovered)) =
                                decoder.receive_parity(&fec_hdr, parity_data)
                            {
                                injector_stats_ref
                                    .fec_recovered
                                    .fetch_add(1, Ordering::Relaxed);
                                tracing::info!(
                                    block = fec_hdr.block_id,
                                    recovered_len = recovered.len(),
                                    "🔧 FEC recovered lost packet"
                                );
                                Some(recovered)
                            } else {
                                None // Parity consumed, no recovery needed
                            }
                        } else {
                            let data_bytes = bytes::Bytes::copy_from_slice(game_data);
                            decoder.receive_data(&fec_hdr, data_bytes.clone());
                            Some(data_bytes)
                        }
                    } else {
                        None
                    }
                } else {
                    // Non-FEC: payload is the game data directly
                    Some(bytes::Bytes::copy_from_slice(payload))
                };

                // Inject the response back to the game client
                if let Some(data) = game_payload {
                    if !data.is_empty() {
                        match injector
                            .inject(&data, game_client, server_addr, mac_src, mac_dst)
                            .await
                        {
                            Ok(_sent) => {
                                tracing::trace!(
                                    payload_len = data.len(),
                                    dst = %game_client,
                                    "Proxy → Game (injected)"
                                );
                            }
                            Err(e) => {
                                tracing::warn!("Inject to game failed: {}", e);
                            }
                        }
                    }
                }

                // Periodic FEC GC
                gc_counter += 1;
                if gc_counter.is_multiple_of(100) {
                    if let Some(ref mut dec) = fec_decoder {
                        dec.gc();
                    }
                }
            }
        })
    };

    // ── Keepalive task (with RTT timestamp recording) ─────────────
    let keepalive_handle = {
        let tunnel_socket = Arc::clone(&tunnel_socket);
        let running = Arc::clone(&running);
        let ka_timestamps = Arc::clone(&keepalive_timestamps);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(5));
            let mut ka_seq: u16 = 60000;
            while running.load(Ordering::Relaxed) {
                interval.tick().await;
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u32;
                let header = lightspeed_protocol::TunnelHeader::keepalive(ka_seq, ts);
                if tunnel_socket
                    .send_to(&header.encode_to_array(), proxy_addr)
                    .await
                    .is_ok()
                {
                    let mut ts_map = ka_timestamps.lock().await;
                    ts_map.insert(ka_seq, std::time::Instant::now());
                    // Evict entries older than 30 s to prevent unbounded growth
                    ts_map.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                }
                ka_seq = ka_seq.wrapping_add(1);
            }
        })
    };

    // ── Stats logger task ─────────────────────────────────────────
    let stats_handle = {
        let out_pkts = Arc::clone(&outbound_packets);
        let out_bytes = Arc::clone(&outbound_bytes);
        let inj_stats = Arc::clone(&injector_stats);
        let running = Arc::clone(&running);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(10));
            while running.load(Ordering::Relaxed) {
                interval.tick().await;
                let cap = out_pkts.load(Ordering::Relaxed);
                let cap_b = out_bytes.load(Ordering::Relaxed);
                let inj = inj_stats.packets_injected.load(Ordering::Relaxed);
                let inj_b = inj_stats.bytes_injected.load(Ordering::Relaxed);
                let from_proxy = inj_stats.packets_from_proxy.load(Ordering::Relaxed);
                let recovered = inj_stats.fec_recovered.load(Ordering::Relaxed);
                let errors = inj_stats.inject_errors.load(Ordering::Relaxed);

                if cap > 0 || from_proxy > 0 {
                    if fec_enabled {
                        info!(
                            "📊 Out: {} pkts ({} B) | In: {} from proxy → {} injected ({} B) | FEC recovered: {} | Errors: {}",
                            cap, cap_b, from_proxy, inj, inj_b, recovered, errors
                        );
                    } else {
                        info!(
                            "📊 Out: {} pkts ({} B) | In: {} from proxy → {} injected ({} B) | Errors: {}",
                            cap, cap_b, from_proxy, inj, inj_b, errors
                        );
                    }
                }
            }
        })
    };

    // ── Outbound capture loop: Game → pcap → Tunnel → Proxy ──────
    let mut seq: u16 = 0;
    let mut fec_encoder = if fec_enabled {
        Some(lightspeed_protocol::FecEncoder::new(fec_k))
    } else {
        None
    };

    while running.load(Ordering::Relaxed) {
        match cap_backend.next_packet() {
            Ok(pkt) => {
                // Skip inbound game-server→client packets captured by pcap's
                // bidirectional BPF match. Their dst is a private/RFC1918 IP;
                // the proxy would drop them via PrivateDestination anyway and
                // counting them would inflate outbound_packets by ~50%.
                if is_private_ipv4(*pkt.dst.ip()) {
                    continue;
                }

                outbound_packets.fetch_add(1, Ordering::Relaxed);
                outbound_bytes.fetch_add(pkt.payload.len() as u64, Ordering::Relaxed);

                // Learn the game client's source address
                {
                    let mut meta = capture_meta.write().await;
                    if meta.is_none() {
                        info!("🎮 Game client detected at {} → {}", pkt.src, pkt.dst);
                    }
                    *meta = Some((pkt.src, pkt.dst, pkt.mac_src, pkt.mac_dst));
                }

                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_micros() as u32;

                if let Some(ref mut encoder) = fec_encoder {
                    // FEC mode: wrap with FEC header
                    let block_id = encoder.block_id();
                    let index = encoder.current_index();

                    let header =
                        lightspeed_protocol::TunnelHeader::new_fec(seq, ts, pkt.src, pkt.dst);
                    let fec_hdr = FecHeader::data(block_id, index, fec_k);

                    let mut pkt_buf =
                        BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + pkt.payload.len());
                    pkt_buf.extend_from_slice(&header.encode_to_array());
                    fec_hdr.encode(&mut pkt_buf);
                    pkt_buf.extend_from_slice(&pkt.payload);

                    let parity = encoder.add_packet(&pkt.payload);
                    let _ = tunnel_socket.send_to(&pkt_buf, proxy_addr).await;

                    // Send parity when block completes
                    if let Some(parity_bytes) = parity {
                        let parity_seq = seq.wrapping_add(1);
                        let parity_header = lightspeed_protocol::TunnelHeader::new_fec(
                            parity_seq, ts, pkt.src, pkt.dst,
                        );
                        let parity_fec = FecHeader::parity(block_id, fec_k);
                        let mut parity_buf = BytesMut::with_capacity(
                            HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len(),
                        );
                        parity_buf.extend_from_slice(&parity_header.encode_to_array());
                        parity_fec.encode(&mut parity_buf);
                        parity_buf.extend_from_slice(&parity_bytes);
                        let _ = tunnel_socket.send_to(&parity_buf, proxy_addr).await;
                        seq = seq.wrapping_add(1); // extra seq for parity
                    }
                } else {
                    // Non-FEC: simple tunnel header + payload
                    let header = lightspeed_protocol::TunnelHeader::new(seq, ts, pkt.src, pkt.dst);
                    let packet = header.encode_with_payload(&pkt.payload);
                    let _ = tunnel_socket.send_to(&packet, proxy_addr).await;
                }

                tracing::trace!(
                    seq = seq,
                    src = %pkt.src,
                    dst = %pkt.dst,
                    payload_len = pkt.payload.len(),
                    "Captured → Proxy"
                );

                seq = seq.wrapping_add(1);
            }
            Err(e) => {
                let err_str = format!("{}", e);
                if !err_str.contains("Timeout") && !err_str.contains("timeout") {
                    tracing::debug!("Capture error: {}", e);
                }
            }
        }
    }

    // ── Shutdown ──────────────────────────────────────────────────
    let _ = cap_backend.stop();
    // Clean up the temporary Windows Firewall rule added at startup.
    remove_firewall_rule();
    inbound_handle.abort();
    keepalive_handle.abort();
    stats_handle.abort();

    let elapsed = start_time.elapsed();
    let out_total = outbound_packets.load(Ordering::Relaxed);
    let out_bytes_total = outbound_bytes.load(Ordering::Relaxed);
    let inj_total = injector_stats.packets_injected.load(Ordering::Relaxed);
    let inj_bytes_total = injector_stats.bytes_injected.load(Ordering::Relaxed);
    let from_proxy_total = injector_stats.packets_from_proxy.load(Ordering::Relaxed);
    let fec_recovered_total = injector_stats.fec_recovered.load(Ordering::Relaxed);
    let inject_errors = injector_stats.inject_errors.load(Ordering::Relaxed);

    info!("\n⚡ Capture stopped");
    info!("📊 Final stats:");
    info!("   Duration:        {:.1}s", elapsed.as_secs_f64());
    info!("   ── Outbound (Game → Proxy) ──");
    info!(
        "   Captured:        {} packets, {} bytes",
        out_total, out_bytes_total
    );
    if elapsed.as_secs() > 0 && out_total > 0 {
        info!(
            "   Avg PPS:         {:.0}",
            out_total as f64 / elapsed.as_secs_f64()
        );
    }
    info!("   ── Inbound (Proxy → Game) ──");
    info!("   From proxy:      {} packets", from_proxy_total);
    info!(
        "   Injected:        {} packets, {} bytes",
        inj_total, inj_bytes_total
    );
    if fec_enabled {
        info!("   FEC recovered:   {} packets", fec_recovered_total);
    }
    info!("   Inject errors:   {}", inject_errors);

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

    Ok(())
}
