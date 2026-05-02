//! WinDivert-based active packet redirect engine (Windows only).
//!
//! # Why WinDivert instead of pcap
//!
//! pcap is a **passive observer** — it copies packets but the originals still
//! travel the direct game-client→server path.  The game session is unaffected
//! and in-game ping reflects the direct RTT, not the tunnel RTT.
//!
//! WinDivert is a **kernel-mode interceptor** — it calls `WinDivertOpen` with
//! a filter and when the kernel matches a packet it is held at the driver layer
//! until the userspace process either re-injects it (`WinDivertSend`) or drops
//! it by never calling send.  This lets us:
//!
//! 1. **Intercept outbound** game-UDP packets going to the server.
//! 2. **Drop** the original (do NOT re-inject).
//! 3. Wrap game payload in a `TunnelHeader` and forward to the LightSpeed proxy.
//! 4. Receive the proxy response on a regular UDP socket.
//! 5. Unwrap the response, **inject a spoofed** UDP packet with `src = real_server`
//!    so the game client receives it as if it came direct from the server.
//!
//! Result: the game session travels entirely through the optimised tunnel and
//! F1-console ping in Rust reflects the tunnel RTT (≈ proxy RTT), not the
//! slow direct path.
//!
//! # Requirements
//! - Windows only (WinDivert is a Windows kernel driver).
//! - `WinDivert64.sys` and `WinDivert.dll` must be in the same directory as
//!   the running executable.  Obtain from <https://reqrypt.org/windivert.html>.
//! - The process must run as **Administrator**.
//! - `windivert-redirect` Cargo feature must be enabled.
//!
//! # Anti-cheat compatibility
//! Rust/EAC, CS2/VAC, and Valorant/Vanguard all permit WinDivert-style
//! network drivers in the same way that ExitLag, WTFast, and NoPing work.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::oneshot;

// ─────────────────────────────────────────────────────────────────────────────
//  Live-stat counters shared with the engine snapshot
// ─────────────────────────────────────────────────────────────────────────────

/// Shared atomic counters filled by the WinDivert redirect task.
#[derive(Debug, Default)]
pub struct WinDivertStats {
    /// Game packets intercepted and forwarded to the proxy (outbound).
    pub packets_intercepted: AtomicU64,
    /// Bytes intercepted.
    pub bytes_intercepted: AtomicU64,
    /// Proxy responses received.
    pub packets_from_proxy: AtomicU64,
    /// Spoofed packets injected back to the game.
    pub packets_injected: AtomicU64,
    /// Bytes injected back to the game.
    pub bytes_injected: AtomicU64,
    /// Errors during intercept or inject.
    pub errors: AtomicU64,
    /// Auto-detected game server address. Set once the first game packet is seen.
    /// `None` means no traffic seen yet (still waiting for game to connect).
    pub detected_server: std::sync::Mutex<Option<SocketAddrV4>>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Raw IP / UDP packet helpers (no external deps)
// ─────────────────────────────────────────────────────────────────────────────

/// Parse a raw IPv4 packet (starting at the IP header) and return the UDP
/// source/destination addresses and the UDP payload slice.
///
/// Returns `None` if the packet is too short, not IPv4, or not UDP.
pub fn parse_ipv4_udp(raw: &[u8]) -> Option<(SocketAddrV4, SocketAddrV4, &[u8])> {
    if raw.len() < 20 {
        return None;
    }
    // IP version check + protocol
    if (raw[0] >> 4) != 4 {
        return None; // not IPv4
    }
    if raw[9] != 17 {
        return None; // not UDP
    }
    let ihl = ((raw[0] & 0x0f) as usize) * 4;
    if raw.len() < ihl + 8 {
        return None;
    }
    let src_ip = Ipv4Addr::new(raw[12], raw[13], raw[14], raw[15]);
    let dst_ip = Ipv4Addr::new(raw[16], raw[17], raw[18], raw[19]);
    let src_port = u16::from_be_bytes([raw[ihl], raw[ihl + 1]]);
    let dst_port = u16::from_be_bytes([raw[ihl + 2], raw[ihl + 3]]);
    let payload = &raw[ihl + 8..];
    Some((
        SocketAddrV4::new(src_ip, src_port),
        SocketAddrV4::new(dst_ip, dst_port),
        payload,
    ))
}

/// Build a raw IPv4+UDP packet from scratch.
///
/// IP and UDP checksums are intentionally zeroed — WinDivert will recalculate
/// them when re-injecting if the `CHECKSUM` flag is NOT set in the flags
/// argument to `WinDivertSend` (i.e., pass `0` for `flags` so WinDivert
/// recalculates).  Alternatively call `WinDivertHelperCalcChecksums` before
/// injecting.
pub fn build_ipv4_udp(src: SocketAddrV4, dst: SocketAddrV4, payload: &[u8]) -> Vec<u8> {
    let udp_len = 8u16 + payload.len() as u16;
    let total_len = 20u16 + udp_len;

    let mut pkt = Vec::with_capacity(total_len as usize);

    // ── IPv4 header (20 bytes, no options) ───────────────────────────────
    pkt.push(0x45); // version=4, IHL=5
    pkt.push(0x00); // DSCP/ECN
    pkt.extend_from_slice(&total_len.to_be_bytes()); // total length
    pkt.extend_from_slice(&[0x00, 0x00]); // identification
    pkt.extend_from_slice(&[0x40, 0x00]); // flags=DF, fragment offset=0
    pkt.push(64); // TTL
    pkt.push(17); // protocol = UDP
    pkt.extend_from_slice(&[0x00, 0x00]); // header checksum (zeroed; WinDivert fills)
    pkt.extend_from_slice(&src.ip().octets()); // src IP
    pkt.extend_from_slice(&dst.ip().octets()); // dst IP

    // ── UDP header (8 bytes) ─────────────────────────────────────────────
    pkt.extend_from_slice(&src.port().to_be_bytes());
    pkt.extend_from_slice(&dst.port().to_be_bytes());
    pkt.extend_from_slice(&udp_len.to_be_bytes());
    pkt.extend_from_slice(&[0x00, 0x00]); // UDP checksum (zeroed; WinDivert fills)

    // ── Payload ──────────────────────────────────────────────────────────
    pkt.extend_from_slice(payload);
    pkt
}

// ─────────────────────────────────────────────────────────────────────────────
//  WinDivert redirect implementation — only compiled with the feature flag
// ─────────────────────────────────────────────────────────────────────────────

/// Configuration for the WinDivert redirect session.
#[derive(Clone, Debug)]
pub struct WinDivertConfig {
    /// Real game server address.
    ///
    /// `Some(addr)` — specific server mode (filter targets exactly this IP:port).
    /// `None`       — auto-detect mode (broad port-range filter; server IP is learned
    ///                from the first intercepted outbound packet, ExitLag-style).
    pub server_addr: Option<SocketAddrV4>,
    /// UDP port range used in the WinDivert filter.
    ///
    /// In auto-detect mode (`server_addr = None`) this is the game's known UDP port
    /// range (e.g. 28015–28017 for Rust).  In manual mode it is used as a secondary
    /// safety gate; set both to the same port for a single-port filter.
    pub port_range: (u16, u16),
    /// LightSpeed proxy address (we redirect outbound packets here).
    pub proxy_addr: SocketAddrV4,
    /// FEC enabled?
    pub fec_enabled: bool,
    /// FEC block size K.
    pub fec_k: u8,
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
mod inner {
    use super::*;
    use std::collections::HashMap;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use tokio::net::UdpSocket;

    /// Run the WinDivert active redirect loop.
    ///
    /// Spawns two `spawn_blocking` threads:
    /// - **intercept thread**: WinDivert recv loop — captures outbound Game→Server
    ///   packets, extracts UDP payload, sends via an mpsc channel.
    /// - **inject thread**: WinDivert send loop — receives assembled IP+UDP frames
    ///   from an mpsc channel and injects them into the IP stack.
    ///
    /// An async task in between handles the proxy tunnel socket bidirectionally.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_windivert_redirect(
        cfg: WinDivertConfig,
        stats: Arc<WinDivertStats>,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
        use bytes::BytesMut;
        use lightspeed_protocol::{FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};
        use windivert::address::WinDivertAddress;
        use windivert::layer::NetworkLayer;
        use windivert::prelude::{WinDivert, WinDivertFlags, WinDivertPacket};

        let pre_known_server = cfg.server_addr;
        let proxy_addr = cfg.proxy_addr;
        let (port_lo, port_hi) = cfg.port_range;

        tracing::info!("🔀 WinDivert redirect active");
        match pre_known_server {
            Some(s) => tracing::info!("   Game server : {} (manual)", s),
            None => tracing::info!(
                "   Game server : auto-detect (port range {}-{})",
                port_lo,
                port_hi
            ),
        }
        tracing::info!("   Proxy       : {}", proxy_addr);

        // ── WinDivert filter expressions ─────────────────────────────────
        //
        // Manual mode  → specific IP:port filter (tight, zero false positives)
        // Auto-detect  → broad port-range filter; software side re-injects any
        //                non-game-server packets that slip through.
        let out_filter = match pre_known_server {
            Some(s) => format!(
                "udp and outbound and ip.DstAddr == {} and udp.DstPort == {}",
                s.ip(),
                s.port()
            ),
            None => {
                if port_lo == port_hi {
                    format!("udp and outbound and udp.DstPort == {}", port_lo)
                } else {
                    format!(
                        "udp and outbound and udp.DstPort >= {} and udp.DstPort <= {}",
                        port_lo, port_hi
                    )
                }
            }
        };

        tracing::info!("   Outbound WinDivert filter: {}", out_filter);

        // Open WinDivert handle for outbound interception
        let wd_intercept =
            WinDivert::network(&out_filter, 0, WinDivertFlags::new()).map_err(|e| {
                anyhow::anyhow!(
                    "WinDivert open failed (need Administrator + WinDivert64.sys): {}",
                    e
                )
            })?;

        // Open WinDivert handle for injection (no filter — only used for sending).
        // IMPORTANT: do NOT set sniff flag here — sniff makes the handle read-only
        // and WinDivertSend() will silently fail, meaning game never gets responses.
        let wd_inject = WinDivert::network("false", 0, WinDivertFlags::new())
            .map_err(|e| anyhow::anyhow!("WinDivert inject handle open failed: {}", e))?;

        // Wrap handles in Arc so they can be moved into spawn_blocking closures
        let wd_intercept = Arc::new(wd_intercept);
        let wd_inject = Arc::new(wd_inject);

        // Channels between blocking WinDivert threads and async tunnel task
        // intercept_tx: (game_src, game_dst, payload_vec)
        //   game_dst is the true destination the game packet was heading to —
        //   this is the server addr (auto-learned or pre-configured).
        let (intercept_tx, mut intercept_rx) =
            tokio::sync::mpsc::channel::<(SocketAddrV4, SocketAddrV4, Vec<u8>)>(256);
        // inject_tx: raw IPv4+UDP bytes to inject back into the stack
        let (inject_tx, inject_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);

        // Shared interface-address cache: populated by the intercept thread from
        // the first captured outbound packet.  The inject thread clones this address
        // (with Outbound=false) so spoofed server→game responses are delivered on
        // the correct network adapter.  A zeroed address (IfIdx=0) is almost never
        // the LAN adapter, causing WinDivert to silently drop injected packets.
        let if_addr_cache: Arc<std::sync::Mutex<Option<WinDivertAddress<NetworkLayer>>>> =
            Arc::new(std::sync::Mutex::new(None));

        let running = Arc::new(AtomicBool::new(true));

        // ── Shutdown watcher ─────────────────────────────────────────────
        {
            let running = Arc::clone(&running);
            tokio::spawn(async move {
                let _ = shutdown_rx.await;
                running.store(false, Ordering::Relaxed);
            });
        }

        // ── Blocking intercept thread ────────────────────────────────────
        let wd_ic = Arc::clone(&wd_intercept);
        let running_ic = Arc::clone(&running);
        let stats_ic = Arc::clone(&stats);
        let if_cache_ic = Arc::clone(&if_addr_cache);
        let itx = intercept_tx;
        tokio::task::spawn_blocking(move || {
            tracing::info!("WinDivert intercept thread started");

            // In auto-detect mode this starts as None and is learned from traffic.
            // In manual mode it is pre-populated and never changes.
            let mut learned_server: Option<SocketAddrV4> = pre_known_server;

            // ── Debounced auto-detect state ───────────────────────────────
            // We require N=3 packets to the SAME destination within T=1500ms
            // before committing.  This prevents locking onto Steam master-server
            // broadcasts on port 28015, which are one-shot queries that never
            // generate a sustained packet stream.
            //
            // Each entry: (candidate_addr, packet_count, first_seen_instant)
            const DETECT_THRESHOLD_PKTS: u8 = 3;
            const DETECT_WINDOW_MS: u128 = 1500;
            let mut candidates: Vec<(SocketAddrV4, u8, Instant)> = Vec::with_capacity(8);

            // ── Stale-server detection ────────────────────────────────────
            // If the locked server goes quiet for SERVER_STALE_SECS seconds
            // (e.g., the user disconnected and joined a different server), we
            // unlock and re-enter Phase 2 auto-detect so traffic to the new
            // server flows through the tunnel automatically.
            const SERVER_STALE_SECS: u64 = 5;
            let mut last_server_pkt: Option<Instant> = None;

            // 65 535 bytes covers the largest possible IPv4 datagram.
            let mut recv_buf = vec![0u8; 65535];

            loop {
                if !running_ic.load(Ordering::Relaxed) {
                    break;
                }
                // recv() is blocking — will return each captured packet.
                match wd_ic.recv(Some(&mut recv_buf)) {
                    Ok(pkt) => {
                        let parsed: Option<(SocketAddrV4, SocketAddrV4, Vec<u8>)> =
                            parse_ipv4_udp(&pkt.data).map(|(src, dst, pl)| (src, dst, pl.to_vec()));

                        match parsed {
                            Some((game_src, game_dst, payload_vec)) => {
                                // ── Cache the interface index on first outbound packet ──
                                // The WinDivertAddress from an intercepted outbound packet
                                // contains the real LAN adapter IfIdx.  We clone it, flip
                                // Outbound=false, and store it so the inject thread can
                                // deliver spoofed server→game responses on the correct
                                // interface instead of the zeroed IfIdx=0 default which
                                // almost always resolves to the wrong adapter and causes
                                // WinDivert to silently drop all injected packets.
                                {
                                    let mut guard = if_cache_ic.lock().unwrap();
                                    if guard.is_none() {
                                        let mut cached = pkt.address.clone();
                                        cached.set_outbound(false); // inbound direction for inject
                                        cached.set_ip_checksum(false); // let WinDivert recompute
                                        cached.set_udp_checksum(false); // let WinDivert recompute
                                        tracing::info!(
                                            "🔗 Cached inject interface: IfIdx={} SubIfIdx={}",
                                            cached.interface_index(),
                                            cached.subinterface_index(),
                                        );
                                        *guard = Some(cached);
                                    }
                                }

                                // ── Phase 1: Server already locked ─────────
                                if let Some(server) = learned_server {
                                    let now = Instant::now();
                                    if game_dst == server {
                                        // It IS game→known_server traffic — tunnel it.
                                        last_server_pkt = Some(now);
                                        stats_ic
                                            .packets_intercepted
                                            .fetch_add(1, Ordering::Relaxed);
                                        stats_ic
                                            .bytes_intercepted
                                            .fetch_add(payload_vec.len() as u64, Ordering::Relaxed);
                                        if itx
                                            .blocking_send((game_src, game_dst, payload_vec))
                                            .is_err()
                                        {
                                            break;
                                        }
                                        continue;
                                    }

                                    // Different destination inside port range.
                                    // Check if the locked server has gone stale.
                                    // `last_server_pkt = None` means server was auto-detected
                                    // but never tunnelled (e.g. manual pre-known mode) — don't
                                    // auto-reset those; they always pass through unchanged.
                                    let is_stale = last_server_pkt
                                        .map(|t| {
                                            now.duration_since(t)
                                                > Duration::from_secs(SERVER_STALE_SECS)
                                        })
                                        .unwrap_or(false);

                                    if is_stale {
                                        // Old server timed out — unlock and re-enter Phase 2
                                        // so traffic to the new server is detected and tunnelled.
                                        tracing::info!(
                                            "🔄 Server {} stale ({}s silence) — re-entering detection",
                                            server, SERVER_STALE_SECS,
                                        );
                                        learned_server = None;
                                        last_server_pkt = None;
                                        candidates.clear();
                                        if let Ok(mut guard) = stats_ic.detected_server.lock() {
                                            *guard = None;
                                        }
                                        // Also reset the interface cache so we re-learn the
                                        // correct network adapter for the new session.
                                        {
                                            let mut cache_guard = if_cache_ic.lock().unwrap();
                                            *cache_guard = None;
                                        }
                                        // Fall through to Phase 2 with this packet.
                                    } else {
                                        // Server still fresh — this is a different destination
                                        // (e.g. Steam master-server query). Pass through unchanged.
                                        let _ = wd_ic.send(&pkt);
                                        continue;
                                    }
                                }

                                // ── Phase 2: Auto-detect — debounce ────────
                                // Pass ALL packets through unchanged until we commit;
                                // the game session continues normally during detection.
                                let _ = wd_ic.send(&pkt);

                                let now = Instant::now();

                                // Expire stale candidates older than the window.
                                candidates.retain(|(_, _, t)| {
                                    now.duration_since(*t).as_millis() < DETECT_WINDOW_MS
                                });

                                // Update or insert candidate entry for game_dst.
                                let mut committed = false;
                                if let Some(entry) =
                                    candidates.iter_mut().find(|(addr, _, _)| *addr == game_dst)
                                {
                                    entry.1 += 1;
                                    if entry.1 >= DETECT_THRESHOLD_PKTS {
                                        committed = true;
                                    }
                                } else {
                                    candidates.push((game_dst, 1, now));
                                }

                                if committed {
                                    // Lock in the server.
                                    learned_server = Some(game_dst);
                                    tracing::info!(
                                        "🔍 Auto-detected game server: {} ({} pkts in ≤{}ms)",
                                        game_dst,
                                        DETECT_THRESHOLD_PKTS,
                                        DETECT_WINDOW_MS,
                                    );
                                    if let Ok(mut guard) = stats_ic.detected_server.lock() {
                                        *guard = Some(game_dst);
                                    }
                                    candidates.clear();
                                    let _ = game_src; // suppress unused warning
                                }
                            }
                            None => {
                                // Non-IPv4/UDP — re-inject unchanged.
                                let _ = wd_ic.send(&pkt);
                            }
                        }
                    }
                    Err(e) => {
                        if running_ic.load(Ordering::Relaxed) {
                            tracing::warn!("WinDivert recv error: {}", e);
                            stats_ic.errors.fetch_add(1, Ordering::Relaxed);
                        }
                        break;
                    }
                }
            }
            tracing::info!("WinDivert intercept thread exiting");
        });

        // ── Blocking inject thread ───────────────────────────────────────
        let wd_inj = Arc::clone(&wd_inject);
        let running_inj = Arc::clone(&running);
        let stats_inj = Arc::clone(&stats);
        let if_cache_inj = Arc::clone(&if_addr_cache);
        tokio::task::spawn_blocking(move || {
            // Use a std mpsc receiver so we can do recv_timeout to check running flag
            tracing::info!("WinDivert inject thread started");
            loop {
                match inject_rx.recv_timeout(Duration::from_millis(100)) {
                    Ok(raw_pkt) => {
                        // Use the interface address cached from the first intercepted
                        // outbound packet so spoofed inbound responses are delivered on
                        // the correct network adapter.  IfIdx=0 (zeroed default) is
                        // almost never the LAN interface and causes WinDivert to silently
                        // drop injected packets, starving the game of server replies.
                        // Fall back to zeroed addr only if no outbound traffic seen yet.
                        let addr = {
                            let guard = if_cache_inj.lock().unwrap();
                            match guard.as_ref() {
                                Some(a) => a.clone(),
                                None => {
                                    tracing::debug!(
                                        "inject: interface not cached yet, using zeroed addr"
                                    );
                                    // SAFETY: zeroed WinDivertAddress is a valid (if suboptimal)
                                    // fallback; the inject will succeed on loopback-capable configs.
                                    unsafe { WinDivertAddress::<NetworkLayer>::new() }
                                }
                            }
                        };
                        let pkt = WinDivertPacket {
                            data: std::borrow::Cow::Owned(raw_pkt),
                            address: addr,
                        };
                        match wd_inj.send(&pkt) {
                            Ok(_) => {
                                stats_inj.packets_injected.fetch_add(1, Ordering::Relaxed);
                            }
                            Err(e) => {
                                tracing::warn!("WinDivert inject error: {}", e);
                                stats_inj.errors.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                        if !running_inj.load(Ordering::Relaxed) {
                            break;
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            tracing::info!("WinDivert inject thread exiting");
        });

        // ── Async tunnel socket ──────────────────────────────────────────
        let tunnel_socket = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
        let tunnel_local_port = tunnel_socket.local_addr()?.port();
        tracing::info!("   Tunnel socket bound on port {}", tunnel_local_port);

        // Add Windows Firewall rule so proxy responses reach our tunnel socket
        add_windivert_firewall_rule(tunnel_local_port);

        // Keepalive timestamps for RTT measurement
        let ka_timestamps: Arc<tokio::sync::Mutex<HashMap<u16, Instant>>> =
            Arc::new(tokio::sync::Mutex::new(HashMap::new()));

        // ── Keepalive task ────────────────────────────────────────────────
        {
            let ts = Arc::clone(&tunnel_socket);
            let running = Arc::clone(&running);
            let ka_ts = Arc::clone(&ka_timestamps);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                let mut ka_seq: u16 = 60000u16;
                while running.load(Ordering::Relaxed) {
                    interval.tick().await;
                    let now_us = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32;
                    let hdr = lightspeed_protocol::TunnelHeader::keepalive(ka_seq, now_us);
                    if ts.send_to(&hdr.encode_to_array(), proxy_addr).await.is_ok() {
                        let mut map = ka_ts.lock().await;
                        map.insert(ka_seq, Instant::now());
                        map.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                    }
                    ka_seq = ka_seq.wrapping_add(1);
                }
            });
        }

        // FEC encoder for outbound
        let mut fec_encoder = if cfg.fec_enabled {
            Some(lightspeed_protocol::FecEncoder::new(cfg.fec_k))
        } else {
            None
        };

        let mut fec_decoder = if cfg.fec_enabled {
            Some(lightspeed_protocol::FecDecoder::new())
        } else {
            None
        };

        let mut seq: u16 = 0;
        let mut buf = vec![0u8; 65535];

        // The client's local source address — learned from the first intercepted
        // outbound packet (game sends from its ephemeral port on local IP).
        let mut game_src_learned: Option<SocketAddrV4> = None;
        // The effective server addr used for TunnelHeader encoding and inject spoofing.
        // Populated from the pre-configured addr or auto-learned on first packet.
        let mut active_server: Option<SocketAddrV4> = pre_known_server;

        tracing::info!("⚡ WinDivert active — waiting for game traffic …");

        loop {
            if !running.load(Ordering::Relaxed) {
                break;
            }

            tokio::select! {
                biased;

                // ── Outbound: intercepted game packet → proxy ─────────────
                maybe_pkt = intercept_rx.recv() => {
                    let (game_src, game_dst, payload) = match maybe_pkt {
                        Some(p) => p,
                        None => break, // intercept thread exited
                    };

                    // Learn / update game source address
                    if game_src_learned.is_none() {
                        tracing::info!("🎮 Game client detected at {}", game_src);
                    }
                    game_src_learned = Some(game_src);

                    // Update active_server from each packet (handles reconnects).
                    active_server = Some(game_dst);

                    let ts = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32;

                    if let Some(ref mut encoder) = fec_encoder {
                        let block_id = encoder.block_id();
                        let index = encoder.current_index();
                        let hdr = lightspeed_protocol::TunnelHeader::new_fec(
                            seq, ts, game_src, game_dst,
                        );
                        let fec_hdr = FecHeader::data(block_id, index, cfg.fec_k);
                        let mut pkt_buf = BytesMut::with_capacity(
                            HEADER_SIZE + FEC_HEADER_SIZE + payload.len(),
                        );
                        pkt_buf.extend_from_slice(&hdr.encode_to_array());
                        fec_hdr.encode(&mut pkt_buf);
                        pkt_buf.extend_from_slice(&payload);
                        let parity = encoder.add_packet(&payload);
                        let _ = tunnel_socket.send_to(&pkt_buf, proxy_addr).await;
                        if let Some(parity_bytes) = parity {
                            let ps = seq.wrapping_add(1);
                            let ph = lightspeed_protocol::TunnelHeader::new_fec(
                                ps, ts, game_src, game_dst,
                            );
                            let pfec = FecHeader::parity(block_id, cfg.fec_k);
                            let mut pb = BytesMut::with_capacity(
                                HEADER_SIZE + FEC_HEADER_SIZE + parity_bytes.len(),
                            );
                            pb.extend_from_slice(&ph.encode_to_array());
                            pfec.encode(&mut pb);
                            pb.extend_from_slice(&parity_bytes);
                            let _ = tunnel_socket.send_to(&pb, proxy_addr).await;
                            seq = seq.wrapping_add(1);
                        }
                    } else {
                        let hdr = lightspeed_protocol::TunnelHeader::new(
                            seq, ts, game_src, game_dst,
                        );
                        let pkt_bytes = hdr.encode_with_payload(&payload);
                        let _ = tunnel_socket.send_to(&pkt_bytes, proxy_addr).await;
                    }

                    tracing::trace!(
                        seq,
                        src = %game_src,
                        payload_len = payload.len(),
                        "WD: Game → Proxy"
                    );
                    seq = seq.wrapping_add(1);
                }

                // ── Inbound: proxy response → inject as spoofed server pkt ─
                recv_res = tokio::time::timeout(
                    Duration::from_millis(50),
                    tunnel_socket.recv_from(&mut buf),
                ) => {
                    let (len, _from) = match recv_res {
                        Ok(Ok(r)) => r,
                        Ok(Err(_)) | Err(_) => continue,
                    };

                    stats.packets_from_proxy.fetch_add(1, Ordering::Relaxed);

                    // Decode tunnel header
                    let (header, payload) = match lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len]) {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::debug!("Invalid tunnel response: {}", e);
                            continue;
                        }
                    };

                    // Handle keepalive echo
                    if header.is_keepalive() {
                        let rtt_us = {
                            let mut map = ka_timestamps.lock().await;
                            map.remove(&header.sequence).map(|t| t.elapsed().as_micros() as u64)
                        };
                        if let Some(rtt) = rtt_us {
                            tracing::trace!("KA RTT: {:.1}ms", rtt as f64 / 1000.0);
                        }
                        continue;
                    }

                    // Need game_src to know where to inject the response
                    let game_src = match game_src_learned {
                        Some(gs) => gs,
                        None => {
                            tracing::debug!("Response from proxy but game src not yet learned");
                            continue;
                        }
                    };

                    // Extract game payload (handle FEC if enabled)
                    let game_data: Option<bytes::Bytes> = if header.has_fec() {
                        if payload.len() < FEC_HEADER_SIZE {
                            continue;
                        }
                        let mut fec_slice: &[u8] = &payload[..FEC_HEADER_SIZE];
                        let fec_hdr = match lightspeed_protocol::FecHeader::decode(&mut fec_slice) {
                            Some(h) => h,
                            None => continue,
                        };
                        let data = &payload[FEC_HEADER_SIZE..];
                        if let Some(ref mut dec) = fec_decoder {
                            if fec_hdr.is_parity() {
                                dec.receive_parity(&fec_hdr, bytes::Bytes::copy_from_slice(data))
                                    .map(|(_, r)| r)
                            } else {
                                let b = bytes::Bytes::copy_from_slice(data);
                                dec.receive_data(&fec_hdr, b.clone());
                                Some(b)
                            }
                        } else {
                            None
                        }
                    } else {
                        Some(bytes::Bytes::copy_from_slice(payload))
                    };

                    if let Some(data) = game_data {
                        if !data.is_empty() {
                            // Build spoofed IP+UDP packet: src=active_server, dst=game_src
                            // WinDivert will fill in the IP/UDP checksums on inject.
                            let spoof_src = match active_server {
                                Some(s) => s,
                                None => continue, // server not yet known — skip
                            };
                            let raw = build_ipv4_udp(spoof_src, game_src, &data);
                            stats.bytes_injected.fetch_add(data.len() as u64, Ordering::Relaxed);
                            // Send to inject thread via sync mpsc (non-blocking send)
                            if inject_tx.try_send(raw).is_err() {
                                tracing::warn!("Inject channel full — dropping packet");
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }

                            tracing::trace!(
                                payload_len = data.len(),
                                dst = %game_src,
                                "WD: Proxy → Game (injected)"
                            );
                        }
                    }
                }
            }
        }

        // ── Shutdown ──────────────────────────────────────────────────────
        running.store(false, Ordering::Relaxed);
        remove_windivert_firewall_rule(tunnel_local_port);
        tracing::info!("WinDivert redirect stopped");

        let ic = stats.packets_intercepted.load(Ordering::Relaxed);
        let fp = stats.packets_from_proxy.load(Ordering::Relaxed);
        let inj = stats.packets_injected.load(Ordering::Relaxed);
        tracing::info!(
            "📊 WinDivert final: intercepted={} from_proxy={} injected={}",
            ic,
            fp,
            inj
        );

        Ok(())
    }

    // ── Firewall helpers for tunnel socket inbound ──────────────────────────

    const FW_RULE_BASE: &str = "LightSpeed WinDivert Tunnel";

    fn add_windivert_firewall_rule(port: u16) {
        let exe = std::env::current_exe().unwrap_or_default();
        let name = format!("{} {}", FW_RULE_BASE, port);
        let _ = std::process::Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={}", name),
                "protocol=UDP",
                "dir=in",
                "action=allow",
                &format!("program={}", exe.to_string_lossy()),
            ])
            .output();
        tracing::info!("🔓 Firewall: added inbound UDP allow rule (port {})", port);
    }

    fn remove_windivert_firewall_rule(port: u16) {
        let name = format!("{} {}", FW_RULE_BASE, port);
        let _ = std::process::Command::new("netsh")
            .args([
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                &format!("name={}", name),
            ])
            .output();
        tracing::info!(
            "🔒 Firewall: removed WinDivert inbound rule (port {})",
            port
        );
    }
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
pub use inner::run_windivert_redirect;

// ── Stub for non-Windows / feature-disabled builds ───────────────────────────

#[cfg(not(all(target_os = "windows", feature = "windivert-redirect")))]
pub async fn run_windivert_redirect(
    _cfg: WinDivertConfig,
    _stats: Arc<WinDivertStats>,
    _shutdown_rx: oneshot::Receiver<()>,
) -> anyhow::Result<()> {
    anyhow::bail!(
        "WinDivert redirect requires Windows + the 'windivert-redirect' feature.\n\
         Build with: cargo build --features windivert-redirect\n\
         Also requires WinDivert64.sys next to the executable."
    )
}
