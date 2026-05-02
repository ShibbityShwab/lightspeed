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

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::traits::{
    InterceptorConfig, InterceptorCounters, InterceptorHandle, TrafficInterceptor,
};

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
            && std::process::Command::new("pfctl")
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

        let proxy_addr = config.proxy_addr;
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
        add_pf_anchor(&anchor, server_addr, local_port)?;

        let counters = Arc::new(InterceptorCounters::default());
        {
            let mut g = counters.detected_server.lock().unwrap();
            *g = Some(server_addr);
        }

        let running = Arc::new(AtomicBool::new(true));

        // ── Shutdown handler ──────────────────────────────────────────────
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        {
            let running = Arc::clone(&running);
            tokio::spawn(async move {
                let _ = shutdown_rx.await;
                running.store(false, Ordering::Relaxed);
            });
        }

        // ── Sockets ───────────────────────────────────────────────────────
        let tunnel_socket = Arc::new(
            tokio::runtime::Handle::current()
                .block_on(UdpSocket::bind("0.0.0.0:0"))
                .map_err(|e| anyhow::anyhow!("Tunnel socket bind: {e}"))?,
        );

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
                    let hdr = lightspeed_protocol::TunnelHeader::keepalive(seq, now_us);
                    let _ = ts.send_to(&hdr.encode_to_array(), proxy_addr).await;
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
            let mut fec_decoder = if fec_enabled {
                Some(lightspeed_protocol::FecDecoder::new())
            } else {
                None
            };

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

                        if let Some(ref mut enc) = fec_encoder {
                            let block_id = enc.block_id();
                            let index = enc.current_index();
                            let hdr = lightspeed_protocol::TunnelHeader::new_fec(seq, ts_us, src, server_addr);
                            let fh = FecHeader::data(block_id, index, fec_k);
                            let mut buf = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + len);
                            buf.extend_from_slice(&hdr.encode_to_array());
                            fh.encode(&mut buf);
                            buf.extend_from_slice(payload);
                            let parity = enc.add_packet(payload);
                            let _ = tunnel_socket.send_to(&buf, proxy_addr).await;
                            if let Some(pb) = parity {
                                let ps = seq.wrapping_add(1);
                                let ph = lightspeed_protocol::TunnelHeader::new_fec(ps, ts_us, src, server_addr);
                                let pf2 = FecHeader::parity(block_id, fec_k);
                                let mut pb2 = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + pb.len());
                                pb2.extend_from_slice(&ph.encode_to_array());
                                pf2.encode(&mut pb2);
                                pb2.extend_from_slice(&pb);
                                let _ = tunnel_socket.send_to(&pb2, proxy_addr).await;
                                seq = seq.wrapping_add(1);
                            }
                        } else {
                            let hdr = lightspeed_protocol::TunnelHeader::new(seq, ts_us, src, server_addr);
                            let pkt = hdr.encode_with_payload(payload);
                            let _ = tunnel_socket.send_to(&pkt, proxy_addr).await;
                        }
                        seq = seq.wrapping_add(1);
                    }

                    // Proxy → Game
                    resp = tokio::time::timeout(
                        Duration::from_millis(50),
                        tunnel_socket.recv_from(&mut in_buf),
                    ) => {
                        let (len, _) = match resp {
                            Ok(Ok(r)) => r,
                            _ => continue,
                        };

                        counters_loop.packets_from_proxy.fetch_add(1, Ordering::Relaxed);

                        let (header, payload) = match lightspeed_protocol::TunnelHeader::decode_with_payload(&in_buf[..len]) {
                            Ok(r) => r,
                            Err(_) => continue,
                        };

                        if header.is_keepalive() { continue; }

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
                            if let Some(ref mut dec) = fec_decoder {
                                if fh.is_parity() {
                                    dec.receive_parity(&fh, bytes::Bytes::copy_from_slice(d)).map(|(_, r)| r)
                                } else {
                                    let b = bytes::Bytes::copy_from_slice(d);
                                    dec.receive_data(&fh, b.clone());
                                    Some(b)
                                }
                            } else { None }
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
        });

        Ok(InterceptorHandle::new(shutdown_tx, counters, "pfctl"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  pfctl helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Ensure pf is enabled (`pfctl -e`). Idempotent — if already enabled the
/// error "pf already enabled" is suppressed.
fn enable_pf() -> anyhow::Result<()> {
    let out = std::process::Command::new("pfctl")
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
/// temporary pf.conf line added via `pfctl -a <anchor> -f -`.
fn add_pf_anchor(anchor: &str, server: SocketAddrV4, local_port: u16) -> anyhow::Result<()> {
    // pf rdr rule — 127.0.0.1 is the loopback; game and LightSpeed both local.
    let rules = format!(
        "rdr pass proto udp from any to {srv_ip} port {srv_port} -> 127.0.0.1 port {local_port}\n",
        srv_ip = server.ip(),
        srv_port = server.port(),
        local_port = local_port,
    );

    let out = std::process::Command::new("pfctl")
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
    let _ = std::process::Command::new("pfctl")
        .args(["-a", anchor, "-F", "all"])
        .output();
    tracing::info!("pf: flushed anchor '{}'", anchor);
}
