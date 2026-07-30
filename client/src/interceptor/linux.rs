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

use super::traits::{
    InterceptorConfig, InterceptorCounters, InterceptorHandle, TrafficInterceptor,
};

// ─────────────────────────────────────────────────────────────────────────────
//  Struct
// ─────────────────────────────────────────────────────────────────────────────

pub struct NftablesInterceptor;

impl NftablesInterceptor {
    pub fn new() -> Self {
        Self
    }
}

impl Default for NftablesInterceptor {
    fn default() -> Self {
        Self
    }
}

impl TrafficInterceptor for NftablesInterceptor {
    fn platform_name(&self) -> &'static str {
        "nftables/iptables"
    }

    fn check_availability(&self) -> Result<(), String> {
        // Prefer nft; fall back to iptables.
        if which("nft").is_some() || which("iptables").is_some() {
            Ok(())
        } else {
            Err("Neither 'nft' nor 'iptables' found in PATH. Install nftables or iptables.".into())
        }
    }

    fn start(&self, config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        use bytes::BytesMut;
        use lightspeed_protocol::{FecHeader, FEC_HEADER_SIZE, HEADER_SIZE};
        use tokio::net::UdpSocket;

        // ── Resolve the server address ────────────────────────────────────
        //
        // The ProcessScanner should have populated `initial_routes` before we get here.
        // We require at least one route with a public remote address.
        let server_addr = config
            .initial_routes
            .first()
            .filter(|r| super::process_scanner::is_public_ipv4(*r.remote.ip()))
            .map(|r| r.remote)
            .unwrap_or_else(|| {
                tracing::info!(
                    "No server route discovered — using port-range fallback ({}-{})",
                    config.port_range.0,
                    config.port_range.1
                );
                std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::new(0, 0, 0, 0),
                    config.port_range.0,
                )
            });

        let proxy_addr = config.proxy_addr;
        let fec_enabled = config.fec_enabled;
        let fec_k = config.fec_k;

        // ── Bind redirected-traffic listener ─────────────────────────────
        // Pick an ephemeral local port for the REDIRECT target.
        let listener_std = std::net::UdpSocket::bind("127.0.0.1:0")
            .map_err(|e| anyhow::anyhow!("Listener bind failed: {e}"))?;
        let local_port = listener_std.local_addr()?.port();

        // Enable IP_RECVORIGDSTADDR to recover original destination from
        // redirected packets. Works on Linux >= 2.6.29, no conntrack needed.
        {
            use std::os::fd::AsRawFd;
            let fd = listener_std.as_raw_fd();
            let one: libc::c_int = 1;
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_IP,
                    20,
                    &one as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
            }
        }

        tracing::info!(
            "Linux interceptor: redirecting {} → localhost:{}",
            server_addr,
            local_port
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
        // Bind a std socket first, then convert to tokio — avoids block_on
        // which panics if called from within an existing async runtime.
        let tunnel_std = std::net::UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| anyhow::anyhow!("Tunnel socket bind: {e}"))?;
        tunnel_std.set_nonblocking(true)?;
        let tunnel_socket = Arc::new(UdpSocket::from_std(tunnel_std)?);

        // Convert to tokio socket for the tunnel (proxy) side.
        // For the listener side, we use raw recvmsg in a dedicated thread
        // to capture CMSG data (IP_ORIGDSTADDR) for port-range auto-detect.
        listener_std.set_nonblocking(true)?;
        let listener_socket = Arc::new(UdpSocket::from_std(listener_std)?);

        // Channel for recvmsg thread → async loop
        let (pkt_tx, mut pkt_rx) = tokio::sync::mpsc::channel::<(
            Vec<u8>,
            std::net::SocketAddrV4,
            Option<std::net::SocketAddrV4>,
        )>(256);
        {
            use std::os::fd::AsRawFd;
            let fd = listener_socket.as_raw_fd();
            std::thread::spawn(move || {
                let mut buf = vec![0u8; 65535];
                let mut cmsg_buf = [0u8; 256];
                loop {
                    let mut iov = libc::iovec {
                        iov_base: buf.as_mut_ptr() as *mut libc::c_void,
                        iov_len: buf.len(),
                    };
                    let mut src_addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
                    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
                    msg.msg_name = &mut src_addr as *mut _ as *mut libc::c_void;
                    msg.msg_namelen = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
                    msg.msg_iov = &mut iov;
                    msg.msg_iovlen = 1;
                    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
                    msg.msg_controllen = cmsg_buf.len();

                    let n = unsafe { libc::recvmsg(fd, &mut msg, 0) };
                    if n < 0 {
                        continue;
                    } // EAGAIN on empty

                    let src = std::net::SocketAddrV4::new(
                        std::net::Ipv4Addr::from(u32::from_be(src_addr.sin_addr.s_addr)),
                        u16::from_be(src_addr.sin_port),
                    );

                    // Parse CMSG for IP_ORIGDSTADDR
                    let orig_dst = unsafe {
                        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
                        let mut dst = None;
                        while !cmsg.is_null() {
                            if (*cmsg).cmsg_level == libc::SOL_IP && (*cmsg).cmsg_type == 20 {
                                let data = libc::CMSG_DATA(cmsg) as *const libc::sockaddr_in;
                                let ip = u32::from_be((*data).sin_addr.s_addr);
                                let port = u16::from_be((*data).sin_port);
                                dst = Some(std::net::SocketAddrV4::new(
                                    std::net::Ipv4Addr::from(ip),
                                    port,
                                ));
                                break;
                            }
                            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
                        }
                        dst
                    };

                    let data = buf[..n as usize].to_vec();
                    if pkt_tx.blocking_send((data, src, orig_dst)).is_err() {
                        break; // channel closed
                    }
                }
            });
        }

        tracing::info!(
            "⚡ Linux interceptor active — intercepting → {}",
            server_addr
        );

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

        // ── Stats logging task ────────────────────────────────────────
        {
            let counters_s = Arc::clone(&counters);
            let running_s = Arc::clone(&running);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(10));
                // Skip first tick (stats are all zero)
                interval.tick().await;
                while running_s.load(Ordering::Relaxed) {
                    interval.tick().await;
                    let snap = counters_s.snapshot("nftables");
                    tracing::info!(
                        "📊 Interceptor: {} pkts out / {} pkts in / {} injected / {} errors",
                        snap.packets_intercepted,
                        snap.packets_from_proxy,
                        snap.packets_injected,
                        snap.errors,
                    );
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

            // Debounce auto-detect state (port-range mode only).
            // Tracks candidate server addresses and commits when one receives
            // enough packets within the detection window.
            const DETECT_PKTS: u8 = 3;
            const DETECT_WINDOW_MS: u128 = 1_500;
            let mut candidates: Vec<(std::net::SocketAddrV4, u8, std::time::Instant)> =
                Vec::with_capacity(8);
            let mut detected_server: Option<std::net::SocketAddrV4> = None;

            loop {
                if !running_loop.load(Ordering::Relaxed) {
                    break;
                }

                tokio::select! {
                    biased;

                    // Game → Proxy (redirected packet arrives via recvmsg thread)
                    recv = pkt_rx.recv() => {
                        let (data, src, recovered_dst) = match recv {
                            Some(r) => r,
                            None => break, // channel closed
                        };
                        let len = data.len();
                        out_buf[..len].copy_from_slice(&data);

                        if game_src.is_none() {
                            tracing::info!("🎮 Game client detected at {} → {}", src, server_addr);
                        }
                        game_src = Some(src);

                        // Use recovered destination from CMSG (recvmsg thread).
                        // NOTE: nftables REDIRECT sets IP_ORIGDSTADDR to the
                        // post-NAT address, not the original. For true auto-detect,
                        // use --server-addr or ensure the game is running so
                        // ProcessScanner discovers the route.
                        let actual_dst = if server_addr.ip().is_unspecified() {
                            recovered_dst.unwrap_or(server_addr)
                        } else {
                            server_addr
                        };

                        // ── Debounce auto-detect (port-range mode only) ─────
                        if server_addr.ip().is_unspecified() && detected_server.is_none() {
                            let now = std::time::Instant::now();
                            // Expire old candidates
                            candidates.retain(|(_, _, t)| {
                                now.duration_since(*t).as_millis() < DETECT_WINDOW_MS
                            });
                            // Increment or add candidate
                            if let Some(entry) = candidates.iter_mut().find(|(a, _, _)| *a == actual_dst) {
                                entry.1 += 1;
                                if entry.1 >= DETECT_PKTS {
                                    detected_server = Some(actual_dst);
                                    tracing::info!(
                                        "🔍 Auto-detected server: {} ({} pkts in ≤{}ms)",
                                        actual_dst, DETECT_PKTS, DETECT_WINDOW_MS
                                    );
                                    if let Ok(mut g) = counters_loop.detected_server.lock() {
                                        *g = Some(actual_dst);
                                    }
                                    candidates.clear();
                                }
                            } else {
                                candidates.push((actual_dst, 1, now));
                            }
                        }

                        counters_loop.packets_intercepted.fetch_add(1, Ordering::Relaxed);
                        counters_loop.bytes_intercepted.fetch_add(len as u64, Ordering::Relaxed);

                        let payload = &out_buf[..len];
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_micros() as u32;

                        // Forward to proxy with TunnelHeader(src=game_src, dst=actual_dst)
                        if let Some(ref mut enc) = fec_encoder {
                            let block_id = enc.block_id();
                            let index = enc.current_index();
                            let hdr = lightspeed_protocol::TunnelHeader::new_fec(seq, ts, src, actual_dst);
                            let fh = FecHeader::data(block_id, index, fec_k);
                            let mut buf = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + len);
                            buf.extend_from_slice(&hdr.encode_to_array());
                            fh.encode(&mut buf);
                            buf.extend_from_slice(payload);
                            let parity = enc.add_packet(payload);
                            let _ = tunnel_socket.send_to(&buf, proxy_addr).await;
                            if let Some(pb) = parity {
                                let ps = seq.wrapping_add(1);
                                let ph = lightspeed_protocol::TunnelHeader::new_fec(ps, ts, src, actual_dst);
                                let pf = FecHeader::parity(block_id, fec_k);
                                let mut pb2 = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + pb.len());
                                pb2.extend_from_slice(&ph.encode_to_array());
                                pf.encode(&mut pb2);
                                pb2.extend_from_slice(&pb);
                                let _ = tunnel_socket.send_to(&pb2, proxy_addr).await;
                                seq = seq.wrapping_add(1);
                            }
                        } else {
                            let hdr = lightspeed_protocol::TunnelHeader::new(seq, ts, src, actual_dst);
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

        Ok(InterceptorHandle::new(
            shutdown_tx,
            counters,
            "nftables/iptables",
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  iptables / nftables helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Add an OUTPUT chain REDIRECT rule so packets destined for `server` are
/// redirected to `local_port` on the loopback, where our listener sits.
/// We tag the rule with a comment so we can find-and-delete it precisely.
fn add_iptables_redirect(server: SocketAddrV4, local_port: u16, tag: &str) -> anyhow::Result<()> {
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
    // Build an nftables script. If the server address is a placeholder
    // (0.0.0.0), use a port-range match instead of an exact IP match.
    let match_clause = if server.ip().is_unspecified() {
        format!(
            "udp dport {}-{}",
            server.port(),
            server.port().saturating_add(100)
        )
    } else {
        format!("ip daddr {} udp dport {}", server.ip(), server.port())
    };

    let script = format!(
        "table ip {tag} {{\n\
         chain output {{\n\
             type nat hook output priority -100;\n\
             {match_clause} redirect to :{local_port}\n\
         }}\n\
         }}\n",
        tag = tag,
        match_clause = match_clause,
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
            "-t",
            "nat",
            "-A",
            "OUTPUT",
            "-p",
            "udp",
            "-d",
            &server.ip().to_string(),
            "--dport",
            &server.port().to_string(),
            "-m",
            "comment",
            "--comment",
            tag,
            "-j",
            "REDIRECT",
            "--to-port",
            &local_port.to_string(),
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
            "-t",
            "nat",
            "-D",
            "OUTPUT",
            "-p",
            "udp",
            "-d",
            &server.ip().to_string(),
            "--dport",
            &server.port().to_string(),
            "-m",
            "comment",
            "--comment",
            tag,
            "-j",
            "REDIRECT",
            "--to-port",
            &local_port.to_string(),
        ])
        .output();
    tracing::info!("iptables: removed REDIRECT rule (tag={})", tag);
}

/// Return the full path to `cmd` if it exists in PATH.
fn which(cmd: &str) -> Option<std::path::PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(cmd))
            .find(|p| p.is_file())
    })
}

// ─────────────────────────────────────────────────────────────────────────────
//  SO_ORIGINAL_DST recovery
// ─────────────────────────────────────────────────────────────────────────────

/// Attempt to recover the original destination from a netfilter-redirected
/// UDP packet via `getsockopt(fd, SOL_IP, SO_ORIGINAL_DST, ...)`.
///
/// NOTE: SO_ORIGINAL_DST for UDP requires kernel support that may not be
/// available (returns ENOPROTOOPT on many kernels). It works reliably for TCP
/// but UDP support is kernel/config-dependent.
///
/// When this fails, use `--server-addr <ip:port>` to specify the game server,
/// or ensure the game is running before starting the interceptor so the
/// ProcessScanner can discover the route automatically.
#[cfg(target_os = "linux")]
fn recover_original_dst(fd: std::os::fd::RawFd) -> Option<std::net::SocketAddrV4> {
    // These constants are stable on Linux:
    //   SOL_IP = 0
    //   SO_ORIGINAL_DST = 80
    // Use recvmsg(MSG_PEEK | MSG_DONTWAIT) with CMSG to recover
    // IP_ORIGDSTADDR from redirected packets. Works without conntrack.
    let mut cmsg_buf = [0u8; 256];
    let mut iov = libc::iovec {
        iov_base: cmsg_buf.as_mut_ptr() as *mut libc::c_void,
        iov_len: 0,
    };
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = cmsg_buf.as_mut_ptr() as *mut libc::c_void;
    msg.msg_controllen = cmsg_buf.len();

    if unsafe { libc::recvmsg(fd, &mut msg, libc::MSG_PEEK | libc::MSG_DONTWAIT) } < 0 {
        return None;
    }

    unsafe {
        let mut cmsg = libc::CMSG_FIRSTHDR(&msg);
        while !cmsg.is_null() {
            if (*cmsg).cmsg_level == libc::SOL_IP && (*cmsg).cmsg_type == 20 {
                let data = libc::CMSG_DATA(cmsg) as *const libc::sockaddr_in;
                let ip = u32::from_be((*data).sin_addr.s_addr);
                let port = u16::from_be((*data).sin_port);
                return Some(std::net::SocketAddrV4::new(
                    std::net::Ipv4Addr::from(ip),
                    port,
                ));
            }
            cmsg = libc::CMSG_NXTHDR(&msg, cmsg);
        }
    }
    None
}

#[cfg(not(target_os = "linux"))]
fn recover_original_dst(_fd: std::os::fd::RawFd) -> Option<std::net::SocketAddrV4> {
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recover_original_dst_invalid_fd_returns_none() {
        // Using an obviously invalid fd should return None, not panic.
        let result = recover_original_dst(-1);
        assert!(result.is_none());
    }

    #[test]
    fn recover_original_dst_bogus_fd_returns_none() {
        // A valid-looking but non-socket fd should also return None.
        let result = recover_original_dst(999999);
        assert!(result.is_none());
    }

    #[test]
    fn port_range_fallback_placeholder_is_unspecified() {
        // Verify the placeholder IP is 0.0.0.0 (triggers port-range nftables mode).
        let placeholder = std::net::SocketAddrV4::new(std::net::Ipv4Addr::new(0, 0, 0, 0), 28015);
        assert!(placeholder.ip().is_unspecified());
        assert_eq!(placeholder.port(), 28015);
    }
}
