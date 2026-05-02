//! Linux traffic interceptor using nftables / iptables REDIRECT.
//!
//! ## How it works
//!
//! 1. [`scan_for_games`](super::process_scanner::scan_for_games) discovers the game PID
//!    and its active server connection (e.g. `1.2.3.4:28015`).
//! 2. An iptables (or nftables) REDIRECT rule is added:
//!    `OUTPUT -p udp -d <server_ip> --dport <server_port> -j REDIRECT --to-port <local>`
//! 3. We bind a UDP socket on `<local>`.
//! 4. The kernel redirected packets arrive on our socket with the **game's original
//!    source port** preserved in `recvfrom`.
//! 5. We build a `TunnelHeader` encoding `src = game_src`, `dst = known_server` and
//!    forward to the LightSpeed proxy.
//! 6. Proxy responses are forwarded back to `game_src` via a raw UDP send.
//!
//! ## Requires
//! - Root / `CAP_NET_ADMIN`.
//! - `iptables` or `nft` in `$PATH`.
//! - Linux kernel ≥ 3.x.

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::traits::{InterceptorConfig, InterceptorCounters, InterceptorHandle, TrafficInterceptor};

// ─────────────────────────────────────────────────────────────────────────────
//  Struct
// ─────────────────────────────────────────────────────────────────────────────

pub struct NftablesInterceptor;

impl NftablesInterceptor {
    pub fn new() -> Self { Self }
}

impl Default for NftablesInterceptor {
    fn default() -> Self { Self }
}

impl TrafficInterceptor for NftablesInterceptor {
    fn platform_name(&self) -> &'static str { "nftables/iptables" }

    fn check_availability(&self) -> Result<(), String> {
        // Prefer nft; fall back to iptables.
        if which("nft").is_some() || which("iptables").is_some() {
            Ok(())
        } else {
            Err("Neither 'nft' nor 'iptables' found in PATH. Install nftables or iptables.".into())
        }
    }

    fn start(&self, config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        use tokio::net::UdpSocket;
        use lightspeed_protocol::{FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};
        use bytes::BytesMut;

        // ── Resolve the server address ────────────────────────────────────
        //
        // The ProcessScanner should have populated `initial_routes` before we get here.
        // We require at least one route with a public remote address.
        let server_addr = config.initial_routes.first()
            .filter(|r| super::process_scanner::is_public_ipv4(*r.remote.ip()))
            .map(|r| r.remote)
            .ok_or_else(|| anyhow::anyhow!(
                "Linux interceptor requires a pre-discovered server route.\n\
                 Run ProcessScanner first and wait until the game is connected to a server."
            ))?;

        let proxy_addr = config.proxy_addr;
        let fec_enabled = config.fec_enabled;
        let fec_k = config.fec_k;

        // ── Bind redirected-traffic listener ─────────────────────────────
        // Pick an ephemeral local port for the REDIRECT target.
        let listener_std = std::net::UdpSocket::bind("127.0.0.1:0")
            .map_err(|e| anyhow::anyhow!("Listener bind failed: {e}"))?;
        let local_port = listener_std.local_addr()?.port();
        tracing::info!(
            "Linux interceptor: redirecting {} → localhost:{}",
            server_addr, local_port
        );

        // ── Install iptables REDIRECT rule ────────────────────────────────
        let rule_tag = format!("lightspeed_{}", local_port);
        add_iptables_redirect(server_addr, local_port, &rule_tag)?;

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

        // ── Tunnel socket (to/from proxy) ─────────────────────────────────
        let tunnel_socket = Arc::new(
            tokio::runtime::Handle::current()
                .block_on(UdpSocket::bind("0.0.0.0:0"))
                .map_err(|e| anyhow::anyhow!("Tunnel socket bind: {e}"))?,
        );

        // Convert the std socket to a tokio async socket
        listener_std.set_nonblocking(true)?;
        let listener_socket = Arc::new(UdpSocket::from_std(listener_std)?);

        tracing::info!("⚡ Linux interceptor active — intercepting → {}", server_addr);

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

        // ── Main async loop ───────────────────────────────────────────────
        let counters_loop = Arc::clone(&counters);
        let running_loop = Arc::clone(&running);
        let rule_tag_owned = rule_tag.clone();

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

            // Maps game ephemeral port → source SocketAddrV4 (for routing responses back).
            let mut game_src: Option<SocketAddrV4> = None;

            loop {
                if !running_loop.load(Ordering::Relaxed) { break; }

                tokio::select! {
                    biased;

                    // Game → Proxy (redirected outbound packet arrives on listener)
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
                            _ => continue, // IPv6 not supported
                        };

                        if game_src.is_none() {
                            tracing::info!("🎮 Game client detected at {} → {}", src, server_addr);
                        }
                        game_src = Some(src);

                        counters_loop.packets_intercepted.fetch_add(1, Ordering::Relaxed);
                        counters_loop.bytes_intercepted.fetch_add(len as u64, Ordering::Relaxed);

                        let payload = &out_buf[..len];
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_micros() as u32;

                        // Forward to proxy with TunnelHeader(src=game_src, dst=server_addr)
                        if let Some(ref mut enc) = fec_encoder {
                            let block_id = enc.block_id();
                            let index = enc.current_index();
                            let hdr = lightspeed_protocol::TunnelHeader::new_fec(seq, ts, src, server_addr);
                            let fh = FecHeader::data(block_id, index, fec_k);
                            let mut buf = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + len);
                            buf.extend_from_slice(&hdr.encode_to_array());
                            fh.encode(&mut buf);
                            buf.extend_from_slice(payload);
                            let parity = enc.add_packet(payload);
                            let _ = tunnel_socket.send_to(&buf, proxy_addr).await;
                            if let Some(pb) = parity {
                                let ps = seq.wrapping_add(1);
                                let ph = lightspeed_protocol::TunnelHeader::new_fec(ps, ts, src, server_addr);
                                let pf = FecHeader::parity(block_id, fec_k);
                                let mut pb2 = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + pb.len());
                                pb2.extend_from_slice(&ph.encode_to_array());
                                pf.encode(&mut pb2);
                                pb2.extend_from_slice(&pb);
                                let _ = tunnel_socket.send_to(&pb2, proxy_addr).await;
                                seq = seq.wrapping_add(1);
                            }
                        } else {
                            let hdr = lightspeed_protocol::TunnelHeader::new(seq, ts, src, server_addr);
                            let pkt = hdr.encode_with_payload(payload);
                            let _ = tunnel_socket.send_to(&pkt, proxy_addr).await;
                        }
                        seq = seq.wrapping_add(1);
                    }

                    // Proxy → Game (inject response back to game)
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
                                // Send response back to game. The game expects this
                                // to come FROM the server's IP:port — on Linux we
                                // can send from a raw socket with spoofed src, but
                                // the simpler approach (sending from tunnel socket) works
                                // if the game doesn't validate the source IP strictly.
                                //
                                // For strict source-IP spoofing, use a raw IP socket
                                // (requires CAP_NET_RAW). Here we use the listener socket
                                // which will deliver from 127.0.0.1 — sufficient when
                                // the game is on the local machine.
                                match listener_socket.send_to(&d, dest).await {
                                    Ok(_) => {
                                        counters_loop.packets_injected.fetch_add(1, Ordering::Relaxed);
                                        counters_loop.bytes_injected.fetch_add(d.len() as u64, Ordering::Relaxed);
                                    }
                                    Err(e) => {
                                        tracing::warn!("Linux inject error: {e}");
                                        counters_loop.errors.fetch_add(1, Ordering::Relaxed);
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // ── Cleanup: remove iptables rule ─────────────────────────────
            remove_iptables_redirect(server_addr, local_port, &rule_tag_owned);
            tracing::info!("Linux interceptor loop exiting");
        });

        Ok(InterceptorHandle::new(shutdown_tx, counters, "nftables/iptables"))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  iptables / nftables helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Add an OUTPUT chain REDIRECT rule so packets destined for `server` are
/// redirected to `local_port` on the loopback, where our listener sits.
/// We tag the rule with a comment so we can find-and-delete it precisely.
fn add_iptables_redirect(
    server: SocketAddrV4,
    local_port: u16,
    tag: &str,
) -> anyhow::Result<()> {
    // Try nftables first, fall back to iptables.
    if which("nft").is_some() {
        return add_nft_redirect(server, local_port, tag);
    }
    add_ipt_redirect(server, local_port, tag)
}

fn remove_iptables_redirect(server: SocketAddrV4, local_port: u16, tag: &str) {
    if which("nft").is_some() {
        remove_nft_redirect(tag);
    } else {
        remove_ipt_redirect(server, local_port, tag);
    }
}

/// nftables: create a temporary table + chain + rule.
fn add_nft_redirect(server: SocketAddrV4, local_port: u16, tag: &str) -> anyhow::Result<()> {
    // Build an nftables script that adds a nat OUTPUT chain rule.
    let script = format!(
        "table ip {tag} {{\n\
         chain output {{\n\
             type nat hook output priority -100;\n\
             ip daddr {srv_ip} udp dport {srv_port} redirect to :{local_port}\n\
         }}\n\
         }}\n",
        tag = tag,
        srv_ip = server.ip(),
        srv_port = server.port(),
        local_port = local_port,
    );
    let out = std::process::Command::new("nft")
        .arg("-f")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            child.stdin.as_mut().unwrap().write_all(script.as_bytes())?;
            child.wait_with_output()
        })
        .map_err(|e| anyhow::anyhow!("nft failed: {e}"))?;

    if !out.status.success() {
        anyhow::bail!(
            "nft rule add failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    tracing::info!("nftables: added redirect table '{}'", tag);
    Ok(())
}

fn remove_nft_redirect(tag: &str) {
    let _ = std::process::Command::new("nft")
        .args(["delete", "table", "ip", tag])
        .output();
    tracing::info!("nftables: removed redirect table '{}'", tag);
}

/// iptables legacy: add a REDIRECT rule in nat OUTPUT.
fn add_ipt_redirect(server: SocketAddrV4, local_port: u16, tag: &str) -> anyhow::Result<()> {
    let out = std::process::Command::new("iptables")
        .args([
            "-t", "nat", "-A", "OUTPUT",
            "-p", "udp",
            "-d", &server.ip().to_string(),
            "--dport", &server.port().to_string(),
            "-m", "comment", "--comment", tag,
            "-j", "REDIRECT", "--to-port", &local_port.to_string(),
        ])
        .output()
        .map_err(|e| anyhow::anyhow!("iptables failed: {e}"))?;

    if !out.status.success() {
        anyhow::bail!(
            "iptables rule add failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
    tracing::info!("iptables: added REDIRECT rule (tag={})", tag);
    Ok(())
}

fn remove_ipt_redirect(server: SocketAddrV4, local_port: u16, tag: &str) {
    let _ = std::process::Command::new("iptables")
        .args([
            "-t", "nat", "-D", "OUTPUT",
            "-p", "udp",
            "-d", &server.ip().to_string(),
            "--dport", &server.port().to_string(),
            "-m", "comment", "--comment", tag,
            "-j", "REDIRECT", "--to-port", &local_port.to_string(),
        ])
        .output();
    tracing::info!("iptables: removed REDIRECT rule (tag={})", tag);
}

/// Return the full path to `cmd` if it exists in PATH.
fn which(cmd: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH")
        .and_then(|path| {
            std::env::split_paths(&path)
                .map(|dir| dir.join(cmd))
                .find(|p| p.is_file())
        })
}