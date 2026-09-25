//! macOS traffic interceptor using pfctl (`pf`).
//!
//! ## How it works
//!
//! 1. [`scan_for_games`](super::process_scanner::scan_for_games) discovers the game PID
//!    and its active server connection (e.g. `1.2.3.4:28015`).
//! 2. A `pf` anchor anchor with an `rdr-to` rule is loaded:
//!    `rdr pass on lo0 proto udp from any to <srv_ip> port <srv_port> -> 127.0.0.1 port <local>`
//! 3. We bind a UDP socket on `<local>` and receive redirected game packets.
//! 4. We build a `TunnelHeader(src=game_src, dst=server)` and forward to proxy.
//! 5. Proxy responses are injected back to `game_src`.
//!
//! ## Requires
//! - macOS 10.7+ (pf included by default).
//! - Root (`sudo`).
//! - `pfctl` in `$PATH`.
//!
//! ## Note on `rdr` and `lo0`
//! macOS `pf` redirects on the OUTPUT path are done via the `lo0` anchor.
//! The `rdr-to` rule intercepts packets before they leave the loopback interface,
//! which is sufficient when game and LightSpeed both run on the same machine.
//!
//! ## Shadow-direct sampling
//!
//! macOS has the same destination-only redirect that Linux solves with `SO_MARK`,
//! so the direct re-send probe needs an exemption or its own copy would be
//! captured by the `rdr` rule and never reach the server. macOS `pf` has no
//! per-socket mark, so the interceptor binds the probe socket first and emits a
//! `no rdr` rule keyed on its source port *ahead of* the `rdr` rule (see
//! [`super::pf_rules`]); the probe is admitted only after that rule loads. If the
//! probe cannot bind, or no port is available, direct sampling is disabled and no
//! copy is ever sent. The sampler is therefore enabled exactly where it can work.

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::pf_rules::anchor_script;
use super::teardown::TeardownAck;
use super::traits::{
    InterceptorConfig, InterceptorCounters, InterceptorHandle, PlatformTeardown, TrafficInterceptor,
};

// ─────────────────────────────────────────────────────────────────────────────
//  Shadow-direct probe
// ─────────────────────────────────────────────────────────────────────────────

/// Dedicated probe socket for the macOS shadow-direct sampler.
///
/// Sends an extra copy of the game's own bytes to the server from a socket that
/// a pf `no rdr` rule exempts from the destination-only redirect, then times the
/// reply. Bound non-blocking and sent from with `try_send_to`, so a probe can
/// never park the interceptor's hot path. A probe is only kept when the
/// exemption rule loaded; otherwise no copy is ever sent.
struct MacShadowProbe {
    socket: tokio::net::UdpSocket,
    server: AtomicU32,
}

impl MacShadowProbe {
    fn bind() -> std::io::Result<Self> {
        let std_socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        std_socket.set_nonblocking(true)?;
        Ok(Self {
            socket: tokio::net::UdpSocket::from_std(std_socket)?,
            server: AtomicU32::new(0),
        })
    }

    fn local_port(&self) -> Option<u16> {
        self.socket.local_addr().ok().map(|addr| addr.port())
    }

    fn send_copy(&self, payload: &[u8], server: SocketAddrV4) -> std::io::Result<()> {
        self.server
            .store(u32::from(*server.ip()), Ordering::Release);
        self.socket
            .try_send_to(payload, std::net::SocketAddr::V4(server))
            .map(|_| ())
    }

    /// Spawn the reply reader; it pairs only the last probed server's reply.
    fn spawn_reader(self: &Arc<Self>, running: Arc<AtomicBool>) {
        let probe = Arc::clone(self);
        tokio::spawn(async move {
            let mut buf = vec![0u8; 65535];
            while running.load(Ordering::Relaxed) {
                match tokio::time::timeout(
                    Duration::from_millis(500),
                    probe.socket.recv_from(&mut buf),
                )
                .await
                {
                    Ok(Ok((_len, std::net::SocketAddr::V4(src)))) => {
                        let expected = probe.server.load(Ordering::Acquire);
                        if expected != 0 && u32::from(*src.ip()) == expected {
                            crate::latency::record_shadow_inbound(*src.ip());
                        }
                    }
                    Ok(Ok((_len, std::net::SocketAddr::V6(_)))) => {}
                    Ok(Err(e)) => tracing::debug!("macOS shadow-direct probe recv error: {e}"),
                    Err(_) => {}
                }
            }
        });
    }
}

/// Whether a datagram received on the redirect listener came from the probe
/// socket itself, i.e. the destination-only redirect captured the probe's copy.
fn is_probe_echo(probe_port: Option<u16>, src: SocketAddrV4) -> bool {
    probe_port == Some(src.port())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Struct
// ─────────────────────────────────────────────────────────────────────────────

pub struct PfInterceptor;

impl PfInterceptor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for PfInterceptor {
    fn default() -> Self {
        Self
    }
}

impl TrafficInterceptor for PfInterceptor {
    fn platform_name(&self) -> &'static str {
        "pfctl"
    }

    fn check_availability(&self) -> Result<(), String> {
        if !std::path::Path::new("/sbin/pfctl").exists()
            && std::process::Command::new("/sbin/pfctl")
                .arg("-s")
                .arg("info")
                .output()
                .map(|o| !o.status.success())
                .unwrap_or(true)
        {
            return Err("pfctl not available. macOS 10.7+ required. Run as root.".into());
        }
        Ok(())
    }

    fn start(&self, config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        use bytes::BytesMut;
        use lightspeed_protocol::{FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};
        use tokio::net::UdpSocket;

        // ── Resolve the server address ────────────────────────────────────
        let server_addr = config
            .initial_routes
            .first()
            .filter(|r| super::process_scanner::is_public_ipv4(*r.remote.ip()))
            .map(|r| r.remote)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "macOS interceptor requires a pre-discovered server route.\n\
                     Run ProcessScanner first and wait for the game to connect."
                )
            })?;

        let config_proxy = config.proxy_addr;
        let fec_enabled = config.fec_enabled;
        let fec_k = config.fec_k;

        // ── Bind listener socket ──────────────────────────────────────────
        let listener_std = std::net::UdpSocket::bind("127.0.0.1:0")
            .map_err(|e| anyhow::anyhow!("Listener bind failed: {e}"))?;
        let local_port = listener_std.local_addr()?.port();

        // ── Anchor name is unique per session ─────────────────────────────
        let anchor = format!("lightspeed_{}", local_port);

        tracing::info!(
            "macOS pf interceptor: adding anchor '{}' → redirect {} to :{}",
            anchor,
            server_addr,
            local_port
        );

        // Enable pf if not already running, then add the anchor rule.
        enable_pf()?;

        // Bind the shadow-direct probe BEFORE loading the anchor: its source
        // port is what the `no rdr` exemption is keyed on, so the port must be
        // known when the rule is emitted. A bind failure disables direct
        // sampling instead of risking a copy the redirect would capture.
        let shadow_probe = match MacShadowProbe::bind() {
            Ok(probe) => Some(Arc::new(probe)),
            Err(e) => {
                tracing::debug!("macOS shadow-direct probe disabled: {e}");
                None
            }
        };
        let shadow_probe_port = shadow_probe.as_ref().and_then(|p| p.local_port());
        let shadow_probe = match (shadow_probe, shadow_probe_port) {
            (Some(probe), Some(_)) => Some(probe),
            (Some(_), None) => {
                tracing::debug!("macOS shadow-direct probe disabled: no bound port");
                None
            }
            (None, _) => None,
        };

        // The anchor carries the probe's `no rdr` exemption, so a probe kept
        // past this point can escape the redirect. A bind failure dropped it
        // above, so no unexempted copy is ever sent.
        add_pf_anchor(&anchor, server_addr, local_port, shadow_probe_port)?;

        let counters = Arc::new(InterceptorCounters::default());
        {
            let mut g = counters.detected_server.lock().unwrap();
            *g = Some(server_addr);
        }

        let running = Arc::new(AtomicBool::new(true));

        if let Some(ref probe) = shadow_probe {
            probe.spawn_reader(Arc::clone(&running));
            tracing::debug!(
                "macOS shadow-direct probe on port {:?} with pf no-rdr exemption",
                shadow_probe_port
            );
        }

        // ── Shutdown handler ──────────────────────────────────────────────
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        {
            let running = Arc::clone(&running);
            tokio::spawn(async move {
                let _ = shutdown_rx.await;
                running.store(false, Ordering::Relaxed);
            });
        }

        // The tunnel task removes the pf anchor when it exits, so
        // `stop_and_wait` must wait for its acknowledgement before the caller
        // lets the process exit.
        let (teardown_tx, teardown_ack) = TeardownAck::new(1);

        // ── Sockets ───────────────────────────────────────────────────────
        let tunnel_std = std::net::UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| anyhow::anyhow!("Tunnel socket bind: {e}"))?;
        tunnel_std.set_nonblocking(true)?;
        let tunnel_socket = Arc::new(UdpSocket::from_std(tunnel_std)?);
        if let Err(e) = crate::tunnel::transport::set_dont_fragment(&tunnel_socket) {
            tracing::warn!(
                "macOS pf interceptor: could not set don't-fragment on tunnel socket: {e}"
            );
        }
        if let Err(e) = crate::tunnel::qos::apply(&tunnel_socket) {
            tracing::warn!("macOS pf interceptor: could not set DSCP on tunnel socket: {e}");
        }

        listener_std.set_nonblocking(true)?;
        let listener_socket = Arc::new(UdpSocket::from_std(listener_std)?);

        tracing::info!("⚡ macOS pf interceptor active");

        // ── Keepalive task ────────────────────────────────────────────────
        {
            let ts = Arc::clone(&tunnel_socket);
            let running_ka = Arc::clone(&running);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(5));
                let mut seq: u16 = 60000;
                while running_ka.load(Ordering::Relaxed) {
                    interval.tick().await;
                    let now_us = std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_micros() as u32;
                    let hdr = lightspeed_protocol::TunnelHeader::keepalive(seq, now_us)
                        .with_session_token(crate::session::session_token());
                    let _ = ts
                        .send_to(
                            &hdr.encode_to_array(),
                            crate::session::current_proxy().unwrap_or(config_proxy),
                        )
                        .await;
                    seq = seq.wrapping_add(1);
                }
            });
        }

        // ── Main loop ─────────────────────────────────────────────────────
        let counters_loop = Arc::clone(&counters);
        let running_loop = Arc::clone(&running);
        let anchor_owned = anchor.clone();

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
            let mut out_buf = vec![0u8; 65535];
            let mut in_buf = vec![0u8; 65535];
            let mut game_src: Option<SocketAddrV4> = None;

            loop {
                if !running_loop.load(Ordering::Relaxed) {
                    break;
                }

                tokio::select! {
                    biased;

                    // Game → Proxy
                    recv = tokio::time::timeout(
                        Duration::from_millis(100),
                        listener_socket.recv_from(&mut out_buf),
                    ) => {
                        let (len, from) = match recv {
                            Ok(Ok(r)) => r,
                            _ => continue,
                        };
                        let src = match from {
                            std::net::SocketAddr::V4(v4) => v4,
                            _ => continue,
                        };

                        // The destination-only `rdr` rule can capture the
                        // probe's own copy; drop it before it enters the tunnel.
                        if is_probe_echo(shadow_probe_port, src) {
                            tracing::debug!(
                                "macOS interceptor: ignored redirected shadow-direct probe from {src}"
                            );
                            continue;
                        }

                        if game_src.is_none() {
                            tracing::info!("🎮 Game client at {} → {}", src, server_addr);
                        }
                        game_src = Some(src);

                        counters_loop.packets_intercepted.fetch_add(1, Ordering::Relaxed);
                        counters_loop.bytes_intercepted.fetch_add(len as u64, Ordering::Relaxed);

                        let payload = &out_buf[..len];
                        let ts_us = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_micros() as u32;

                        if let crate::tunnel::budget::PayloadFit::OverBudget {
                            payload_len,
                            budget,
                        } = crate::tunnel::budget::classify(len, fec_encoder.is_some())
                        {
                            counters_loop.payloads_over_budget.fetch_add(1, Ordering::Relaxed);
                            tracing::warn!(
                                payload_len = payload_len,
                                budget = budget,
                                "macOS pf interceptor: forwarding oversized game payload; the kernel may fragment it"
                            );
                        }

                        let shadow_due =
                            crate::latency::record_shadow_outbound(*server_addr.ip());
                        let tunnel_send = async {
                        if let Some(ref mut enc) = fec_encoder {
                            let block_id = enc.block_id();
                            let index = enc.current_index();
                            let hdr = lightspeed_protocol::TunnelHeader::new_fec(seq, ts_us, src, server_addr)
                                .with_session_token(crate::session::session_token());
                            let fh = FecHeader::data(block_id, index, fec_k);
                            let mut buf = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + len);
                            buf.extend_from_slice(&hdr.encode_to_array());
                            fh.encode(&mut buf);
                            buf.extend_from_slice(payload);
                            let parity = enc.add_packet(payload);
                            crate::latency::record_outbound(*server_addr.ip());
                            let _ = crate::tunnel::transport::send_datagram(
                                &tunnel_socket,
                                &buf,
                                crate::session::current_proxy().unwrap_or(config_proxy),
                            )
                            .await;
                            if let Some(pb) = parity {
                                let ps = seq.wrapping_add(1);
                                let ph = lightspeed_protocol::TunnelHeader::new_fec(ps, ts_us, src, server_addr)
                                    .with_session_token(crate::session::session_token());
                                let pf2 = FecHeader::parity(block_id, fec_k);
                                let mut pb2 = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + pb.len());
                                pb2.extend_from_slice(&ph.encode_to_array());
                                pf2.encode(&mut pb2);
                                pb2.extend_from_slice(&pb);
                                let _ = crate::tunnel::transport::send_datagram(
                                    &tunnel_socket,
                                    &pb2,
                                    crate::session::current_proxy().unwrap_or(config_proxy),
                                )
                                .await;
                                seq = seq.wrapping_add(1);
                            }
                        } else {
                            let hdr = lightspeed_protocol::TunnelHeader::new(seq, ts_us, src, server_addr)
                                .with_session_token(crate::session::session_token());
                            let pkt = hdr.encode_with_payload(payload);
                            crate::latency::record_outbound(*server_addr.ip());
                            let (dests, n) = crate::session::send_destinations(config_proxy);
                            for d in dests.iter().take(n) {
                                let _ = crate::tunnel::transport::send_datagram(
                                    &tunnel_socket,
                                    &pkt,
                                    *d,
                                )
                                    .await;
                            }
                        }
                        };
                        // The game's packet is tunnelled first and unchanged.
                        // Only afterwards, when the sampler claimed a sample, is
                        // an EXTRA copy of its own bytes sent direct.
                        tunnel_send.await;
                        if shadow_due {
                            if let Some(ref probe) = shadow_probe {
                                if let Err(e) = probe.send_copy(payload, server_addr) {
                                    tracing::debug!("macOS shadow-direct probe send failed: {e}");
                                }
                            }
                        }
                        seq = seq.wrapping_add(1);
                    }

                    // Proxy → Game
                    resp = tokio::time::timeout(
                        Duration::from_millis(50),
                        tunnel_socket.recv_from(&mut in_buf),
                    ) => {
                        let (len, src_addr) = match resp {
                            Ok(Ok(r)) => r,
                            _ => continue,
                        };

                        if !crate::session::is_known_proxy(src_addr, config_proxy) {
                            continue;
                        }

                        counters_loop.packets_from_proxy.fetch_add(1, Ordering::Relaxed);

                        let (header, payload) = match lightspeed_protocol::TunnelHeader::decode_with_payload(&in_buf[..len]) {
                            Ok(r) => r,
                            Err(_) => continue,
                        };

                        if header.is_keepalive() { continue; }

                        crate::latency::record_inbound(*header.orig_src_addr().ip());

                        let dest = match game_src {
                            Some(gs) => gs,
                            None => continue,
                        };

                        let data: Option<bytes::Bytes> = if header.has_fec() {
                            if payload.len() < FEC_HEADER_SIZE { continue; }
                            let mut sl: &[u8] = &payload[..FEC_HEADER_SIZE];
                            let fh = match lightspeed_protocol::FecHeader::decode(&mut sl) {
                                Some(h) => h,
                                None => continue,
                            };
                            let d = &payload[FEC_HEADER_SIZE..];
                            let dec = fec_decoders.entry(src_addr).or_default();
                            if fh.is_parity() {
                                let recovered = dec
                                    .receive_parity(&fh, bytes::Bytes::copy_from_slice(d))
                                    .map(|(_, r)| r);
                                if recovered.is_some() {
                                    crate::telemetry::paths::record_relay_recovery(src_addr);
                                }
                                recovered
                            } else {
                                let b = bytes::Bytes::copy_from_slice(d);
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

                        if let Some(d) = data {
                            if !d.is_empty() {
                                match listener_socket.send_to(&d, dest).await {
                                    Ok(_) => {
                                        counters_loop.packets_injected.fetch_add(1, Ordering::Relaxed);
                                        counters_loop.bytes_injected.fetch_add(d.len() as u64, Ordering::Relaxed);
                                    }
                                    Err(e) => {
                                        tracing::warn!("macOS inject error: {e}");
                                        counters_loop.errors.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Cleanup
            remove_pf_anchor(&anchor_owned);
            tracing::info!("macOS pf interceptor loop exiting");
            let _ = teardown_tx.send(());
        });

        Ok(InterceptorHandle::new(
            shutdown_tx,
            counters,
            "pfctl",
            Some(PlatformTeardown::new(|| {}, teardown_ack)),
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  pfctl helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Ensure pf is enabled (`pfctl -e`). Idempotent — if already enabled the
/// error "pf already enabled" is suppressed.
fn enable_pf() -> anyhow::Result<()> {
    let out = std::process::Command::new("/sbin/pfctl")
        .args(["-e"])
        .output()
        .map_err(|e| anyhow::anyhow!("pfctl enable failed: {e}"))?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        if !stderr.contains("already enabled") {
            tracing::warn!("pfctl -e: {stderr}");
        }
    }
    Ok(())
}

/// Load an anchor rule that redirects `server` UDP to `local_port`.
///
/// The anchor is `/etc/pf.anchors/lightspeed_<port>` and is referenced from a
/// temporary pf.conf line added via `pfctl -a <anchor> -f -`. When `probe_port`
/// is `Some`, a `no rdr` exemption for the shadow-direct probe's source port is
/// emitted ahead of the redirect so only the probe escapes it.
fn add_pf_anchor(
    anchor: &str,
    server: SocketAddrV4,
    local_port: u16,
    probe_port: Option<u16>,
) -> anyhow::Result<()> {
    let rules = anchor_script(server, local_port, probe_port);

    let out = std::process::Command::new("/sbin/pfctl")
        .args(["-a", anchor, "-f", "-"])
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.as_mut().unwrap().write_all(rules.as_bytes())?;
            child.wait_with_output()
        })
        .map_err(|e| anyhow::anyhow!("pfctl anchor load failed: {e}"))?;

    if !out.status.success() {
        anyhow::bail!(
            "pfctl anchor load failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    tracing::info!(
        "pf: loaded anchor '{}' → redirect {} to :{}",
        anchor,
        server,
        local_port
    );
    Ok(())
}

/// Remove a named pf anchor by flushing it.
fn remove_pf_anchor(anchor: &str) {
    // Flush the anchor rules (empty input removes all rules in anchor).
    let _ = std::process::Command::new("/sbin/pfctl")
        .args(["-a", anchor, "-F", "all"])
        .output();
    tracing::info!("pf: flushed anchor '{}'", anchor);
}
