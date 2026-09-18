//! Windows `WinDivertInterceptor` — kernel-level traffic interception via WinDivert.
//!
//! This is the gold-standard implementation on Windows.  WinDivert is a signed
//! NDIS lightweight filter driver that holds each matching packet in the
//! kernel until userspace either re-injects or drops it — identical in
//! principle to how ExitLag / WTFast / NoPing operate.
//!
//! ## Improvements over legacy `windivert_redirect.rs`
//!
//! - Implements the OOP [`TrafficInterceptor`] trait.
//! - The network-layer WinDivert filter matches by port range (WinDivert only
//!   supports `processId` at the Flow layer, not the Network layer), and always
//!   excludes the proxy's own address so the client's tunnel traffic is never
//!   intercepted.
//! - Routes pre-seeded from [`ProcessScanner`] let the engine skip the
//!   debounce accumulation window: interception starts with the first packet.
//! - Stale-server timeout unchanged (5 s) — still auto-resets on map change.
//!
//! ## Requirements
//! - Windows only.
//! - `windivert-redirect` Cargo feature must be enabled.
//! - `WinDivert64.sys` + `WinDivert.dll` must sit next to the exe.
//! - Process must run as **Administrator**.

// Imports needed by both the full WinDivert impl and the stub:
use super::traits::{InterceptorConfig, InterceptorHandle, TrafficInterceptor};

// Imports only needed when WinDivert is actually compiled in:
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::order::{effective_seeds, Decision, ServerTracker, TrackerConfig};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::recovery::RecvBackoff;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::teardown::{recv_should_break, TeardownAck};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::traits::{InterceptorCounters, PlatformTeardown};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use super::windivert_handle::{is_fwp_in_use, OwnedHandle};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use crate::capture::windivert_redirect::{build_ipv4_udp, parse_ipv4_udp};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use std::net::SocketAddrV4;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use std::sync::atomic::{AtomicBool, Ordering};
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use std::sync::Arc;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use std::time::Duration;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use windivert_sys::address::WINDIVERT_ADDRESS;
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
use windivert_sys::{WinDivertFlags, WinDivertLayer, WinDivertShutdownMode};

// ─────────────────────────────────────────────────────────────────────────────
//  Struct
// ─────────────────────────────────────────────────────────────────────────────

/// Windows traffic interceptor backed by WinDivert.
///
/// Wraps the kernel-driver intercept+inject pattern in a clean OOP shell.
/// Obtain one through the module-level factory `create_interceptor()`.
pub struct WinDivertInterceptor;

impl WinDivertInterceptor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for WinDivertInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  TrafficInterceptor — full implementation (gated on feature + platform)
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
impl TrafficInterceptor for WinDivertInterceptor {
    fn platform_name(&self) -> &'static str {
        "WinDivert"
    }

    fn check_availability(&self) -> Result<(), String> {
        // Check that WinDivert64.sys and WinDivert.dll are next to the exe.
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(|d| d.to_path_buf()))
            .unwrap_or_default();
        let sys = exe_dir.join("WinDivert64.sys");
        let dll = exe_dir.join("WinDivert.dll");
        if !sys.exists() {
            return Err(format!(
                "WinDivert64.sys not found at {}\n\
                 Download WinDivert 2.x from https://reqrypt.org/windivert.html",
                sys.display()
            ));
        }
        if !dll.exists() {
            return Err(format!("WinDivert.dll not found at {}", dll.display()));
        }
        Ok(())
    }

    fn start(&self, config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        use bytes::BytesMut;
        use lightspeed_protocol::{FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};
        use std::collections::HashMap;
        use std::time::Instant;
        use tokio::net::UdpSocket;

        let (port_lo, port_hi) = config.port_range;
        let config_proxy = config.proxy_addr;
        let dynamic_server = config.dynamic_server();
        let route_set: Vec<SocketAddrV4> = config.initial_routes.iter().map(|r| r.remote).collect();
        let seeds = effective_seeds(dynamic_server, &route_set);
        let pre_known_server: Option<SocketAddrV4> = seeds.first().copied();

        // ── Build the WinDivert outbound filter ───────────────────────────
        //
        // Priority order:
        //   1. PID known  → `outbound and processId == N and udp.DstPort in {ports}`
        //   2. Server IP known (from route scanner) → specific IP:port
        //   3. Port range only → broad filter; debounce auto-detects the real server
        let out_filter = match config.pid {
            Some(pid) => {
                // NOTE: `processId` is only valid for WinDivert Flow/Socket layers,
                // not the Network layer used by `WinDivert::network()`.
                // We log the PID for diagnostics but rely on port constraints +
                // debounce auto-detection to isolate game traffic.
                let port_clause = if port_lo == port_hi {
                    format!("udp.DstPort == {port_lo}")
                } else {
                    format!("udp.DstPort >= {port_lo} and udp.DstPort <= {port_hi}")
                };
                tracing::info!(
                    "🔒 WinDivert filter: PID {} (network layer, port range {}-{})",
                    pid,
                    port_lo,
                    port_hi
                );
                format!("outbound and udp and {}", port_clause)
            }
            None => match pre_known_server {
                Some(s) => {
                    tracing::info!("🔒 WinDivert filter: IP {}:{}", s.ip(), s.port());
                    format!(
                        "udp and outbound and ip.DstAddr == {} and udp.DstPort == {}",
                        s.ip(),
                        s.port()
                    )
                }
                None => {
                    tracing::info!(
                        "🔒 WinDivert filter: port range {}-{} (auto-detect mode)",
                        port_lo,
                        port_hi
                    );
                    if port_lo == port_hi {
                        format!("udp and outbound and udp.DstPort == {port_lo}")
                    } else {
                        format!(
                            "udp and outbound and udp.DstPort >= {port_lo} and udp.DstPort <= {port_hi}"
                        )
                    }
                }
            },
        };

        // Never intercept the client's own tunnel traffic. The broad port-range
        // filter also matches the QUIC control and keepalive packets addressed
        // to the proxy; without this exclusion the auto-detect tracker locks the
        // proxy's own address as the "game server".
        let out_filter = format!("{out_filter} and ip.DstAddr != {}", config_proxy.ip());

        tracing::info!("🔀 WinDivert intercept filter: {}", out_filter);

        // Open intercept handle (captures matching outbound packets).
        tracing::info!("⚡ Opening WinDivert intercept handle...");
        let wd_intercept = match OwnedHandle::open(
            &out_filter,
            WinDivertLayer::Network,
            0,
            WinDivertFlags::new(),
        ) {
            Ok(h) => {
                tracing::info!("✅ WinDivert intercept handle opened successfully");
                Arc::new(h)
            }
            Err(e) => {
                log_open_failure("intercept", &e);
                return Err(anyhow::anyhow!(
                    "WinDivert open failed (need Admin + WinDivert64.sys loaded): {e}"
                ));
            }
        };

        // Open inject handle (no filter — write-only for injecting inbound spoofs).
        tracing::info!("⚡ Opening WinDivert inject handle...");
        let wd_inject =
            match OwnedHandle::open("false", WinDivertLayer::Network, 0, WinDivertFlags::new()) {
                Ok(h) => {
                    tracing::info!("✅ WinDivert inject handle opened successfully");
                    Arc::new(h)
                }
                Err(e) => {
                    log_open_failure("inject", &e);
                    // Close the already-open intercept handle before returning.
                    let _ = wd_intercept.close();
                    return Err(anyhow::anyhow!("WinDivert inject handle failed: {e}"));
                }
            };

        // Both handles are open: any later setup error must release them.
        let mut setup_guard =
            HandleShutdownGuard::new(Arc::clone(&wd_intercept), Arc::clone(&wd_inject));

        // Shared state
        let counters = Arc::new(InterceptorCounters::default());
        let running = Arc::new(AtomicBool::new(true));
        tracing::info!("✅ Shared state created");

        // Pre-seed detected_server if we already know it from the route scanner.
        if let Some(s) = pre_known_server {
            if let Ok(mut g) = counters.detected_server.lock() {
                *g = Some(s);
            }
        }

        // Interface address cache
        let if_addr_cache: Arc<std::sync::Mutex<Option<WINDIVERT_ADDRESS>>> =
            Arc::new(std::sync::Mutex::new(None));

        // Channels
        let (intercept_tx, mut intercept_rx) =
            tokio::sync::mpsc::channel::<(SocketAddrV4, SocketAddrV4, Vec<u8>)>(512);
        let (inject_tx, inject_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(512);
        tracing::info!("✅ Channels created");

        // Each owner thread sends one ack when it can no longer touch its
        // WinDivert handle, so `stop_and_wait` can close deterministically.
        // The third ack is the tunnel task, which also removes the firewall
        // rule, so teardown does not finish before the rule is gone.
        let (ack_tx, teardown_ack) = TeardownAck::new(3);

        // ── Shutdown signal ──────────────────────────────────────────────
        // Keep tokio::sync::oneshot for InterceptorHandle compatibility, but
        // use tokio::runtime::Handle::block_on inside a std::thread so we avoid
        // the tokio::spawn(…) panic when start() is called from a non-tokio thread
        // (e.g. the egui main loop on the GUI).
        tracing::info!("🔌 Creating shutdown signal...");
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        tracing::info!("✅ Shutdown signal created");
        tracing::info!("🔌 Spawning shutdown thread...");
        {
            let running = Arc::clone(&running);
            let rt_handle = tokio::runtime::Handle::current();
            std::thread::spawn(move || {
                tracing::info!("🎯 Shutdown thread started, waiting for signal...");
                rt_handle.block_on(async {
                    let _ = shutdown_rx.await;
                });
                tracing::info!("🛑 Shutdown signal received");
                running.store(false, Ordering::Relaxed);
            });
        }
        tracing::info!("✅ Shutdown thread spawned");

        // ── Intercept thread (blocking) ───────────────────────────────────
        //
        // Reads intercepted outbound packets from WinDivert, auto-detects the
        // game server, then forwards packets to the async tunnel task via mpsc.
        {
            let wd_ic = Arc::clone(&wd_intercept);
            let counters_ic = Arc::clone(&counters);
            let if_cache_ic = Arc::clone(&if_addr_cache);
            let itx = intercept_tx;
            let running_ic = Arc::clone(&running);
            let seeds_ic = seeds;
            let ack_ic = ack_tx.clone();

            tracing::info!("🚀 Spawning intercept thread...");
            tokio::task::spawn_blocking(move || {
                tracing::info!("🎯 Intercept thread started");
                const RECV_MAX_RETRIES: u32 = 5;
                const RECV_BACKOFF_BASE: Duration = Duration::from_millis(10);
                const RECV_BACKOFF_MAX: Duration = Duration::from_millis(400);

                let mut tracker = ServerTracker::new(TrackerConfig::default());
                tracker.seed(&seeds_ic, Instant::now());
                let mut reported_server: Option<SocketAddrV4> = pre_known_server;
                let mut backoff =
                    RecvBackoff::new(RECV_MAX_RETRIES, RECV_BACKOFF_BASE, RECV_BACKOFF_MAX);
                let mut recv_buf = vec![0u8; 65535];

                loop {
                    if !running_ic.load(Ordering::Relaxed) {
                        break;
                    }

                    match wd_ic.recv(&mut recv_buf) {
                        Ok((len, addr)) => {
                            backoff.on_success();
                            let data = &recv_buf[..len];

                            // Cache the interface index on the first outbound
                            // packet. The copy is mutated for injection; `addr`
                            // stays untouched so re-injection keeps the
                            // original outbound direction and checksums.
                            {
                                let mut guard = if_cache_ic
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                if guard.is_none() {
                                    let mut cached = addr;
                                    cached.set_outbound(false);
                                    cached.set_ipchecksum(false);
                                    cached.set_udpchecksum(false);
                                    // SAFETY: [Category 8 - FFI] `addr` came from
                                    // a successful network-layer `WinDivertRecv`,
                                    // so the driver initialized the `Network`
                                    // union member. This reads one plain `u32`
                                    // from that `#[repr(C)]` union.
                                    let if_idx = unsafe { cached.union_field.Network.interface_id };
                                    tracing::info!(
                                        "🔗 WinDivert: cached inject interface IfIdx={if_idx}"
                                    );
                                    *guard = Some(cached);
                                }
                            }

                            let parsed =
                                parse_ipv4_udp(data).map(|(src, dst, pl)| (src, dst, pl.to_vec()));

                            match parsed {
                                Some((game_src, game_dst, payload)) => {
                                    match tracker.observe(game_dst, Instant::now(), payload.len()) {
                                        Decision::Tunnel(server) => {
                                            if reported_server != Some(server) {
                                                reported_server = Some(server);
                                                if let Ok(mut g) =
                                                    counters_ic.detected_server.lock()
                                                {
                                                    *g = Some(server);
                                                }
                                                tracing::info!("🔍 Locked game server: {}", server);
                                            }
                                            counters_ic
                                                .packets_intercepted
                                                .fetch_add(1, Ordering::Relaxed);
                                            counters_ic
                                                .bytes_intercepted
                                                .fetch_add(payload.len() as u64, Ordering::Relaxed);
                                            if itx
                                                .blocking_send((game_src, game_dst, payload))
                                                .is_err()
                                            {
                                                break;
                                            }
                                        }
                                        Decision::PassThrough | Decision::StartDetection => {
                                            let _ = wd_ic.send(data, &addr);
                                        }
                                        Decision::ResetToDetection => {
                                            tracing::info!("🔄 Locked server stale — re-detecting");
                                            reported_server = None;
                                            if let Ok(mut g) = counters_ic.detected_server.lock() {
                                                *g = None;
                                            }
                                            if let Ok(mut c) = if_cache_ic.lock() {
                                                *c = None;
                                            }
                                            let _ = wd_ic.send(data, &addr);
                                        }
                                    }
                                }
                                None => {
                                    // Non-IPv4/UDP — re-inject unchanged.
                                    let _ = wd_ic.send(data, &addr);
                                }
                            }
                        }
                        Err(e) => {
                            let raw = e.raw_os_error().unwrap_or(0);
                            if recv_should_break(running_ic.load(Ordering::Relaxed), raw) {
                                break;
                            }
                            counters_ic.errors.fetch_add(1, Ordering::Relaxed);
                            match backoff.on_failure() {
                                Some(delay) => {
                                    tracing::warn!(
                                        "WinDivert recv error (retry {}/{} in {:?}): {e}",
                                        backoff.consecutive_failures(),
                                        RECV_MAX_RETRIES,
                                        delay
                                    );
                                    std::thread::sleep(delay);
                                }
                                None => {
                                    let msg = format!(
                                        "WinDivert recv failed {} times consecutively: {e}",
                                        backoff.consecutive_failures()
                                    );
                                    tracing::error!("{msg}");
                                    if let Ok(mut g) = counters_ic.last_error.lock() {
                                        *g = Some(msg);
                                    }
                                    break;
                                }
                            }
                        }
                    }
                }
                // The owner closes the handle only after every owner thread
                // sends this ack, so the thread must not touch it afterwards.
                let _ = ack_ic.send(());
                tracing::info!("WinDivert intercept thread exiting");
            });
        }
        tracing::info!("✅ Intercept thread spawned");

        // ── Inject thread (blocking) ─────────────────────────────────────
        {
            tracing::info!("🚀 Spawning inject thread...");
            let wd_inj = Arc::clone(&wd_inject);
            let counters_inj = Arc::clone(&counters);
            let if_cache_inj = Arc::clone(&if_addr_cache);
            let running_inj = Arc::clone(&running);
            let ack_inj = ack_tx.clone();

            tokio::task::spawn_blocking(move || {
                tracing::info!("🎯 Inject thread started");
                loop {
                    match inject_rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(raw) => {
                            let addr = {
                                let guard = if_cache_inj
                                    .lock()
                                    .unwrap_or_else(|poisoned| poisoned.into_inner());
                                match guard.as_ref() {
                                    Some(a) => *a,
                                    None => WINDIVERT_ADDRESS::default(),
                                }
                            };
                            match wd_inj.send(&raw, &addr) {
                                Ok(_) => {
                                    counters_inj
                                        .packets_injected
                                        .fetch_add(1, Ordering::Relaxed);
                                }
                                Err(e) => {
                                    tracing::warn!("WinDivert inject error: {e}");
                                    counters_inj.errors.fetch_add(1, Ordering::Relaxed);
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
                // The owner closes the handle only after every owner thread
                // sends this ack, so the thread must not touch it afterwards.
                let _ = ack_inj.send(());
                tracing::info!("WinDivert inject thread exiting");
            });
        }
        tracing::info!("✅ Inject thread spawned");

        // ── Async tunnel task ─────────────────────────────────────────────
        {
            tracing::info!("🚀 Creating tunnel UDP socket...");
            let counters_t = Arc::clone(&counters);
            let counters_ka = Arc::clone(&counters);
            let _running_t = Arc::clone(&running);

            let tunnel_std = std::net::UdpSocket::bind("0.0.0.0:0")
                .map_err(|e| anyhow::anyhow!("Tunnel socket bind failed: {e}"))?;
            tunnel_std.set_nonblocking(true)?;
            let tunnel_socket = Arc::new(UdpSocket::from_std(tunnel_std)?);
            tracing::info!("✅ Tunnel UDP socket created");
            let tunnel_port = tunnel_socket
                .local_addr()
                .map_err(|e| anyhow::anyhow!("Cannot get tunnel port: {e}"))?
                .port();
            tracing::info!("🔌 Tunnel socket bound to port {}", tunnel_port);

            // Add Windows Firewall rule so proxy responses reach our tunnel socket.
            tracing::info!("🔓 Adding firewall rule for port {}...", tunnel_port);
            add_fw_rule(tunnel_port);
            tracing::info!("✅ Firewall rule added");

            tracing::info!("⚡ WinDivert active — tunnel socket port {}", tunnel_port);

            // Keepalive timestamps
            let ka_ts: Arc<tokio::sync::Mutex<HashMap<u16, Instant>>> =
                Arc::new(tokio::sync::Mutex::new(HashMap::new()));

            // Keepalive sender task
            {
                let ts = Arc::clone(&tunnel_socket);
                let ka = Arc::clone(&ka_ts);
                let running_ka = Arc::clone(&running);
                tokio::spawn(async move {
                    let mut ticker = tokio::time::interval(Duration::from_secs(5));
                    let mut seq: u16 = 60000;
                    while running_ka.load(Ordering::Relaxed) {
                        ticker.tick().await;
                        let now_us = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_micros() as u32;
                        let hdr = lightspeed_protocol::TunnelHeader::keepalive(seq, now_us)
                            .with_session_token(crate::session::session_token());
                        if ts
                            .send_to(
                                &hdr.encode_to_array(),
                                crate::session::current_proxy().unwrap_or(config_proxy),
                            )
                            .await
                            .is_ok()
                        {
                            let mut m = ka.lock().await;
                            m.insert(seq, Instant::now());
                            m.retain(|_, t| t.elapsed() < Duration::from_secs(30));
                        }
                        seq = seq.wrapping_add(1);
                        let _ = counters_ka.packets_from_proxy.load(Ordering::Relaxed);
                        // touch to avoid dead_code warning
                    }
                });
            }

            // Main async loop: forward intercepted packets to proxy, inject responses.
            let fec_enabled = config.fec_enabled;
            let fec_k = config.fec_k;
            let running_loop = Arc::clone(&running);
            let ack_loop = ack_tx.clone();

            tokio::spawn(async move {
                let mut fec_encoder = if fec_enabled {
                    Some(lightspeed_protocol::FecEncoder::new(fec_k))
                } else {
                    None
                };
                let mut fec_decoders: std::collections::HashMap<
                    std::net::SocketAddr,
                    lightspeed_protocol::FecDecoder,
                > = std::collections::HashMap::new();

                let mut seq: u16 = 0;
                let mut buf = vec![0u8; 65535];
                let mut game_src_learned: Option<SocketAddrV4> = None;
                let mut active_server: Option<SocketAddrV4> = pre_known_server;

                loop {
                    if !running_loop.load(Ordering::Relaxed) {
                        break;
                    }

                    tokio::select! {
                        biased;

                        // Intercepted game → proxy
                        maybe = intercept_rx.recv() => {
                            let (game_src, game_dst, payload) = match maybe {
                                Some(p) => p,
                                None => break,
                            };

                            if game_src_learned.is_none() {
                                tracing::info!("🎮 Game client at {}", game_src);
                            }
                            game_src_learned = Some(game_src);
                            active_server = Some(game_dst);

                            let ts = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_micros() as u32;

                            if let Some(ref mut enc) = fec_encoder {
                                let block_id = enc.block_id();
                                let index = enc.current_index();
                                let hdr = lightspeed_protocol::TunnelHeader::new_fec(seq, ts, game_src, game_dst)
                                    .with_session_token(crate::session::session_token());
                                let fh = FecHeader::data(block_id, index, fec_k);
                                let mut buf2 = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + payload.len());
                                buf2.extend_from_slice(&hdr.encode_to_array());
                                fh.encode(&mut buf2);
                                buf2.extend_from_slice(&payload);
                                let parity = enc.add_packet(&payload);
                                let _ = tunnel_socket.send_to(&buf2, crate::session::current_proxy().unwrap_or(config_proxy)).await;
                                if let Some(pb) = parity {
                                    let ps = seq.wrapping_add(1);
                                    let ph = lightspeed_protocol::TunnelHeader::new_fec(ps, ts, game_src, game_dst)
                                        .with_session_token(crate::session::session_token());
                                    let pf = FecHeader::parity(block_id, fec_k);
                                    let mut pb2 = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + pb.len());
                                    pb2.extend_from_slice(&ph.encode_to_array());
                                    pf.encode(&mut pb2);
                                    pb2.extend_from_slice(&pb);
                                    let _ = tunnel_socket.send_to(&pb2, crate::session::current_proxy().unwrap_or(config_proxy)).await;
                                    seq = seq.wrapping_add(1);
                                }
                            } else {
                                let hdr = lightspeed_protocol::TunnelHeader::new(seq, ts, game_src, game_dst)
                                    .with_session_token(crate::session::session_token());
                                let pkt = hdr.encode_with_payload(&payload);
                                let (dests, n) = crate::session::send_destinations(config_proxy);
                                for d in dests.iter().take(n) {
                                    let _ = tunnel_socket.send_to(&pkt, *d).await;
                                }
                            }
                            seq = seq.wrapping_add(1);
                        }

                        // Proxy response → inject as spoofed server packet
                        recv = tokio::time::timeout(Duration::from_millis(50), tunnel_socket.recv_from(&mut buf)) => {
                            let (len, src_addr) = match recv {
                                Ok(Ok(r)) => r,
                                _ => continue,
                            };

                            if !crate::session::is_known_proxy(src_addr, config_proxy) {
                                continue;
                            }

                            counters_t.packets_from_proxy.fetch_add(1, Ordering::Relaxed);

                            let (header, payload) = match lightspeed_protocol::TunnelHeader::decode_with_payload(&buf[..len]) {
                                Ok(r) => r,
                                Err(_) => continue,
                            };

                            if header.is_keepalive() {
                                continue; // keepalive echo — RTT measured by keepalive task
                            }

                            let game_src = match game_src_learned {
                                Some(gs) => gs,
                                None => continue,
                            };

                            let game_data: Option<bytes::Bytes> = if header.has_fec() {
                                if payload.len() < FEC_HEADER_SIZE { continue; }
                                let mut sl: &[u8] = &payload[..FEC_HEADER_SIZE];
                                let fh = match lightspeed_protocol::FecHeader::decode(&mut sl) {
                                    Some(h) => h,
                                    None => continue,
                                };
                                let data = &payload[FEC_HEADER_SIZE..];
                                let dec = fec_decoders.entry(src_addr).or_default();
                                if fh.is_parity() {
                                    dec.receive_parity(&fh, bytes::Bytes::copy_from_slice(data))
                                        .map(|(_, r)| r)
                                } else {
                                    let b = bytes::Bytes::copy_from_slice(data);
                                    dec.receive_data(&fh, b.clone());
                                    if crate::session::multipath_record_response(
                                        header.sequence,
                                        src_addr,
                                        0,
                                    ) {
                                        None
                                    } else {
                                        Some(b)
                                    }
                                }
                            } else if crate::session::multipath_record_response(
                                header.sequence,
                                src_addr,
                                0,
                            ) {
                                None
                            } else {
                                Some(bytes::Bytes::copy_from_slice(payload))
                            };

                            if let Some(data) = game_data {
                                if !data.is_empty() {
                                    let spoof_src = match active_server {
                                        Some(s) => s,
                                        None => continue,
                                    };
                                    let raw = build_ipv4_udp(spoof_src, game_src, &data);
                                    counters_t.bytes_injected.fetch_add(data.len() as u64, Ordering::Relaxed);
                                    if inject_tx.try_send(raw).is_err() {
                                        tracing::warn!("Inject channel full — dropping response");
                                        counters_t.errors.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                    }
                }

                // Cleanup
                remove_fw_rule(tunnel_port);
                // This ack also covers the firewall rule removal, so
                // `stop_and_wait` does not return before the rule is gone.
                let _ = ack_loop.send(());
                tracing::info!("WinDivert tunnel task exiting");
            });
        }

        // `stop()` shuts both directions down: `Recv` releases the intercept
        // thread parked in `recv`, and `Send` releases the inject thread if it
        // is parked in a full `WinDivertSend` queue. Both owner threads then
        // ack, so the session can be waited on and the handles closed
        // deterministically.
        let teardown = PlatformTeardown::new(
            {
                let wd_ic = Arc::clone(&wd_intercept);
                let wd_inj = Arc::clone(&wd_inject);
                move || {
                    if let Err(e) = wd_ic.shutdown(WinDivertShutdownMode::Recv) {
                        tracing::warn!("WinDivert intercept shutdown failed: {e}");
                    }
                    if let Err(e) = wd_inj.shutdown(WinDivertShutdownMode::Send) {
                        tracing::warn!("WinDivert inject shutdown failed: {e}");
                    }
                }
            },
            teardown_ack,
        );

        setup_guard.disarm();
        Ok(InterceptorHandle::new(
            shutdown_tx,
            counters,
            "WinDivert",
            Some(teardown),
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Handle lifecycle helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Shuts both WinDivert handles down unless disarmed.
///
/// `start` opens both handles before it binds the tunnel socket and spawns the
/// async tunnel task. If any of that setup fails, `Drop` shuts the handles
/// down; the owner threads then exit and the last `Arc<OwnedHandle>` drop
/// closes the handle, so a failed start cannot leak WFP state. On success
/// `disarm` hands the handles to the running session.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
struct HandleShutdownGuard {
    intercept: Arc<OwnedHandle>,
    inject: Arc<OwnedHandle>,
    armed: bool,
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
impl HandleShutdownGuard {
    fn new(intercept: Arc<OwnedHandle>, inject: Arc<OwnedHandle>) -> Self {
        Self {
            intercept,
            inject,
            armed: true,
        }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
impl Drop for HandleShutdownGuard {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let _ = self.intercept.shutdown(WinDivertShutdownMode::Both);
        let _ = self.inject.shutdown(WinDivertShutdownMode::Both);
    }
}

/// Log a WinDivert open failure, calling out stale WFP state when the OS
/// reports `FWP_E_IN_USE` so the fix (cold shutdown or killing the stale
/// process) is visible next to the raw error.
#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
fn log_open_failure(handle_role: &str, err: &std::io::Error) {
    if is_fwp_in_use(err) {
        tracing::error!(
            "❌ WinDivert {handle_role} open failed: {err}. A previous LightSpeed run \
             left its WFP filter/callout registered; do a full cold shutdown or close \
             the stale LightSpeed process, then try again."
        );
    } else {
        tracing::error!("❌ WinDivert {handle_role} open failed: {err}");
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Stub for non-Windows or missing feature
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(not(all(target_os = "windows", feature = "windivert-redirect")))]
impl TrafficInterceptor for WinDivertInterceptor {
    fn platform_name(&self) -> &'static str {
        "WinDivert"
    }

    fn check_availability(&self) -> Result<(), String> {
        Err(
            "WinDivert requires Windows + windivert-redirect Cargo feature.\n\
             Build with: cargo build --features windivert-redirect"
                .to_string(),
        )
    }

    fn start(&self, _config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        anyhow::bail!(
            "WinDivert requires Windows + the 'windivert-redirect' feature.\n\
             Build with: cargo build --features windivert-redirect"
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Windows Firewall helpers
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
fn add_fw_rule(port: u16) {
    let exe = std::env::current_exe().unwrap_or_default();
    let name = format!("LightSpeed WinDivert Tunnel {}", port);
    match std::process::Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "add",
            "rule",
            &format!("name={name}"),
            "protocol=UDP",
            "dir=in",
            "action=allow",
            &format!("program={}", exe.to_string_lossy()),
        ])
        .output()
    {
        Ok(o) if o.status.success() => {
            tracing::info!("🔓 Firewall: added inbound UDP rule for port {}", port);
        }
        Ok(o) => {
            tracing::warn!(
                "Firewall rule add may have failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => {
            tracing::warn!(
                "Failed to run netsh for firewall (need Administrator?): {}",
                e
            );
        }
    }
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
fn remove_fw_rule(port: u16) {
    let name = format!("LightSpeed WinDivert Tunnel {}", port);
    match std::process::Command::new("netsh")
        .args([
            "advfirewall",
            "firewall",
            "delete",
            "rule",
            &format!("name={name}"),
        ])
        .output()
    {
        Ok(o) if o.status.success() => {
            tracing::info!("🔒 Firewall: removed WinDivert rule for port {}", port);
        }
        Ok(o) => {
            tracing::warn!(
                "Firewall rule remove may have failed: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        }
        Err(e) => {
            tracing::warn!("Failed to run netsh for firewall removal: {}", e);
        }
    }
}

#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn add_fw_rule(_port: u16) {}
#[cfg(not(target_os = "windows"))]
#[allow(dead_code)]
fn remove_fw_rule(_port: u16) {}
