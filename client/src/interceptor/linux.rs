//! Linux traffic interceptor using nftables / iptables REDIRECT.
//!
//! ## How it works
//!
//! 1. [`scan_for_games`](super::process_scanner::scan_for_games) discovers the
//!    game PID and its active server routes. Only public (non-RFC1918) remote
//!    addresses are candidates.
//! 2. A nat OUTPUT REDIRECT rule matching the **exact** `ip daddr … udp dport …`
//!    of the locked server sends those packets to a local listener.
//! 3. A UDP socket bound to that listener port stays open for the whole
//!    session. The kernel preserves the game's source address in `recvfrom`.
//! 4. We build a `TunnelHeader(src = game_src, dst = locked_server)` and forward
//!    to the LightSpeed proxy. The header destination is always the same value
//!    the installed rule matches, so the two cannot diverge.
//! 5. Proxy responses are injected back **from the listener socket**, which is
//!    what lets conntrack reverse the original NAT and deliver the reply to the
//!    connected game socket as if it came from the real server.
//!
//! Server discovery and rotation are driven by [`RotationTracker`] (see
//! `rotation.rs`): a conservative gate swaps the rule only after the old server
//! has stopped appearing in scans and has been silent, because deleting a
//! REDIRECT rule does not unhook an already-established conntrack flow.
//!
//! ## Requires
//! - Root / `CAP_NET_ADMIN`.
//! - `iptables` or `nft` in `$PATH`.
//! - Linux kernel ≥ 3.x.

use std::io::Write;
use std::net::SocketAddrV4;
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::recovery::RecvBackoff;
use super::rotation::{Action, RotationTracker, ROTATE_SCAN_INTERVAL};
use super::traits::{
    InterceptorConfig, InterceptorCounters, InterceptorHandle, TrafficInterceptor,
};

const RECV_POLL_TIMEOUT_MS: libc::c_int = 200;
const RECV_MAX_RETRIES: u32 = 5;
const RECV_BACKOFF_BASE: Duration = Duration::from_millis(10);
const RECV_BACKOFF_MAX: Duration = Duration::from_millis(500);

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

        let config_proxy = config.proxy_addr;
        let fec_enabled = config.fec_enabled;
        let fec_k = config.fec_k;
        let dynamic = config.dynamic_server();
        let process_names = crate::games::process_names_for_name(&config.game_name);
        let initial_routes: Vec<SocketAddrV4> =
            config.initial_routes.iter().map(|r| r.remote).collect();

        // ── Bind the stable redirected-traffic listener ───────────────────
        let listener_std = std::net::UdpSocket::bind("127.0.0.1:0")
            .map_err(|e| anyhow::anyhow!("Listener bind failed: {e}"))?;
        let local_port = listener_std.local_addr()?.port();
        let rule_tag = format!("lightspeed_{local_port}");

        let mut installer = Installer::new(local_port, rule_tag.clone())?;

        let counters = Arc::new(InterceptorCounters::default());
        let running = Arc::new(AtomicBool::new(true));

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        {
            let running = Arc::clone(&running);
            tokio::spawn(async move {
                let _ = shutdown_rx.await;
                running.store(false, Ordering::Relaxed);
            });
        }

        // ── Tunnel socket (client ↔ proxy) ────────────────────────────────
        let tunnel_std = std::net::UdpSocket::bind("0.0.0.0:0")
            .map_err(|e| anyhow::anyhow!("Tunnel socket bind: {e}"))?;
        tunnel_std.set_nonblocking(true)?;
        let tunnel_socket = Arc::new(UdpSocket::from_std(tunnel_std)?);

        listener_std.set_nonblocking(true)?;
        let listener_socket = Arc::new(UdpSocket::from_std(listener_std)?);

        // ── Raw recvmsg thread for CMSG-free source recovery ──────────────
        let (pkt_tx, mut pkt_rx) = tokio::sync::mpsc::channel::<(Vec<u8>, SocketAddrV4)>(256);
        {
            let recv_socket = Arc::clone(&listener_socket);
            let counters_recv = Arc::clone(&counters);
            std::thread::spawn(move || {
                recv_loop(recv_socket.as_raw_fd(), pkt_tx, counters_recv);
            });
        }

        tracing::info!(
            "Linux interceptor: redirect rule table '{rule_tag}' → 127.0.0.1:{local_port}"
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

        // ── Stats logging task ────────────────────────────────────────────
        {
            let counters_s = Arc::clone(&counters);
            let running_s = Arc::clone(&running);
            tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(10));
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

        // ── Rotation + tunnel main loop ───────────────────────────────────
        let counters_loop = Arc::clone(&counters);
        let running_loop = Arc::clone(&running);
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

            let mut rotation = RotationTracker::new();
            let mut scan_timer = tokio::time::interval_at(
                tokio::time::Instant::now() + ROTATE_SCAN_INTERVAL,
                ROTATE_SCAN_INTERVAL,
            );
            scan_timer.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            // Immediate seed decision: static games may pre-lock a discovered
            // route; dynamic games (e.g. Fortnite) start unlocked and wait for
            // the first scanner interval.
            let seeds = super::order::effective_seeds(dynamic, &initial_routes);
            let action = rotation.on_scan(&seeds, Instant::now());
            apply_action(action, &mut installer, &mut rotation, &counters_loop);

            loop {
                if !running_loop.load(Ordering::Relaxed) {
                    break;
                }

                tokio::select! {
                    biased;

                    _ = scan_timer.tick() => {
                        let candidates = scan_candidates(&process_names, config_proxy);
                        let action = rotation.on_scan(&candidates, Instant::now());
                        tracing::info!(
                            "🔍 rotation scan: candidates={candidates:?} action={action:?}"
                        );
                        apply_action(action, &mut installer, &mut rotation, &counters_loop);
                    }

                    recv = pkt_rx.recv() => {
                        let (data, src) = match recv {
                            Some(r) => r,
                            None => break,
                        };
                        let len = data.len();
                        out_buf[..len].copy_from_slice(&data);

                        if game_src.is_none() {
                            tracing::info!("🎮 Game client detected at {src}");
                        }
                        game_src = Some(src);
                        rotation.note_intercepted_packet(Instant::now());

                        let Some(actual_dst) = installer.current() else {
                            tracing::debug!("redirected packet with no installed rule — dropping");
                            continue;
                        };

                        counters_loop.packets_intercepted.fetch_add(1, Ordering::Relaxed);
                        counters_loop.bytes_intercepted.fetch_add(len as u64, Ordering::Relaxed);

                        let payload = &out_buf[..len];
                        let ts = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_micros() as u32;

                        if let Some(ref mut enc) = fec_encoder {
                            let block_id = enc.block_id();
                            let index = enc.current_index();
                            let hdr = lightspeed_protocol::TunnelHeader::new_fec(seq, ts, src, actual_dst)
                                .with_session_token(crate::session::session_token());
                            let fh = FecHeader::data(block_id, index, fec_k);
                            let mut buf = BytesMut::with_capacity(HEADER_SIZE + FEC_HEADER_SIZE + len);
                            buf.extend_from_slice(&hdr.encode_to_array());
                            fh.encode(&mut buf);
                            buf.extend_from_slice(payload);
                            let parity = enc.add_packet(payload);
                            let _ = tunnel_socket.send_to(&buf, crate::session::current_proxy().unwrap_or(config_proxy)).await;
                            if let Some(pb) = parity {
                                let ps = seq.wrapping_add(1);
                                let ph = lightspeed_protocol::TunnelHeader::new_fec(ps, ts, src, actual_dst)
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
                            let hdr = lightspeed_protocol::TunnelHeader::new(seq, ts, src, actual_dst)
                                .with_session_token(crate::session::session_token());
                            let pkt = hdr.encode_with_payload(payload);
                            let (dests, n) = crate::session::send_destinations(config_proxy);
                            for d in dests.iter().take(n) {
                                let _ = tunnel_socket.send_to(&pkt, *d).await;
                            }
                        }
                        seq = seq.wrapping_add(1);
                    }

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

                        let Some(dest) = game_src else { continue };

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
                                dec.receive_parity(&fh, bytes::Bytes::copy_from_slice(d))
                                    .map(|(_, r)| r)
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
                                // Inject from the listener socket: conntrack
                                // reverse-NAT only rewrites replies sent from
                                // the redirect target port.
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

            installer.teardown();
            publish_detected(&counters_loop, None);
            tracing::info!("Linux interceptor loop exiting");
        });

        Ok(InterceptorHandle::new(
            shutdown_tx,
            counters,
            "nftables/iptables",
            None,
        ))
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Rotation helpers
// ─────────────────────────────────────────────────────────────────────────────

/// Collect the scanner's public remote routes, minus the proxy's own address.
fn scan_candidates(process_names: &[&str], proxy: SocketAddrV4) -> Vec<SocketAddrV4> {
    let mut out = Vec::new();
    for process in super::process_scanner::scan_for_games(process_names) {
        for route in process.routes {
            let remote = route.remote;
            if !super::process_scanner::is_public_ipv4(*remote.ip()) {
                continue;
            }
            if remote.ip() == proxy.ip() {
                continue;
            }
            if !out.contains(&remote) {
                out.push(remote);
            }
        }
    }
    out
}

/// Apply a [`RotationTracker`] decision, re-synchronising the tracker when a
/// rule transaction fails so `tracker.current()` never diverges from the rule.
fn apply_action(
    action: Action,
    installer: &mut Installer,
    rotation: &mut RotationTracker,
    counters: &InterceptorCounters,
) {
    match action {
        Action::Wait | Action::Keep => {}
        Action::Install(server) => match installer.install(server) {
            Ok(()) => publish_detected(counters, Some(server)),
            Err(e) => {
                record_failure(counters, format!("install {server}: {e}"));
                rotation.reset();
            }
        },
        Action::Swap(server) => match installer.swap(server) {
            Ok(()) => publish_detected(counters, Some(server)),
            Err(e) => {
                record_failure(counters, format!("swap {server}: {e}"));
                rotation.reset();
            }
        },
        Action::Teardown => {
            installer.teardown();
            publish_detected(counters, None);
        }
    }
}

fn publish_detected(counters: &InterceptorCounters, server: Option<SocketAddrV4>) {
    let mut guard = counters.detected_server.lock().unwrap();
    if *guard != server {
        *guard = server;
    }
}

fn record_failure(counters: &InterceptorCounters, message: String) {
    tracing::warn!("Linux interceptor rule error: {message}");
    counters.errors.fetch_add(1, Ordering::Relaxed);
    *counters.last_error.lock().unwrap() = Some(message);
}

// ─────────────────────────────────────────────────────────────────────────────
//  Rule installer
// ─────────────────────────────────────────────────────────────────────────────

enum Backend {
    Nft(PathBuf),
    Iptables(PathBuf),
}

/// Owns the single REDIRECT rule for the session and tracks its current match.
///
/// The destination encoded in the tunnel header is read from
/// [`Installer::current`], which is updated only after a rule transaction
/// succeeds, keeping header destination and rule match in lock-step.
struct Installer {
    tag: String,
    local_port: u16,
    backend: Backend,
    current: Option<SocketAddrV4>,
}

impl Installer {
    fn new(local_port: u16, tag: String) -> anyhow::Result<Self> {
        let backend = if let Some(path) = which("nft") {
            Backend::Nft(path)
        } else if let Some(path) = which("iptables") {
            Backend::Iptables(path)
        } else {
            anyhow::bail!("Neither 'nft' nor 'iptables' found in PATH");
        };
        Ok(Self {
            tag,
            local_port,
            backend,
            current: None,
        })
    }

    fn current(&self) -> Option<SocketAddrV4> {
        self.current
    }

    fn install(&mut self, server: SocketAddrV4) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Nft(nft) => {
                nft_run(nft, &nft_install_script(server, self.local_port, &self.tag))?
            }
            Backend::Iptables(ipt) => ipt_add(ipt, server, self.local_port, &self.tag)?,
        }
        self.current = Some(server);
        Ok(())
    }

    fn swap(&mut self, server: SocketAddrV4) -> anyhow::Result<()> {
        match &self.backend {
            Backend::Nft(nft) => {
                nft_run(nft, &nft_swap_script(server, self.local_port, &self.tag))?
            }
            Backend::Iptables(ipt) => {
                if let Some(previous) = self.current {
                    ipt_del(ipt, previous, self.local_port, &self.tag);
                }
                ipt_add(ipt, server, self.local_port, &self.tag)?;
            }
        }
        self.current = Some(server);
        Ok(())
    }

    fn teardown(&mut self) {
        let Some(previous) = self.current.take() else {
            return;
        };
        match &self.backend {
            Backend::Nft(nft) => nft_delete_table(nft, &self.tag),
            Backend::Iptables(ipt) => ipt_del(ipt, previous, self.local_port, &self.tag),
        }
    }
}

fn nft_install_script(server: SocketAddrV4, local_port: u16, tag: &str) -> String {
    format!(
        "table ip {tag} {{\n chain output {{\n  type nat hook output priority -100; policy accept;\n  ip daddr {ip} udp dport {port} redirect to :{local_port}\n }}\n}}\n",
        ip = server.ip(),
        port = server.port(),
    )
}

fn nft_swap_script(server: SocketAddrV4, local_port: u16, tag: &str) -> String {
    format!(
        "flush chain ip {tag} output\n\
         add rule ip {tag} output ip daddr {ip} udp dport {port} redirect to :{local_port}\n",
        ip = server.ip(),
        port = server.port(),
    )
}

fn nft_run(nft: &Path, script: &str) -> anyhow::Result<()> {
    let mut child = std::process::Command::new(nft)
        .arg("-f")
        .arg("-")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|e| anyhow::anyhow!("nft failed to start: {e}"))?;
    {
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| anyhow::anyhow!("nft stdin unavailable"))?;
        stdin.write_all(script.as_bytes())?;
    }
    let out = child.wait_with_output()?;
    if !out.status.success() {
        anyhow::bail!("nft: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(())
}

fn nft_delete_table(nft: &Path, tag: &str) {
    let removed = std::process::Command::new(nft)
        .args(["delete", "table", "ip", tag])
        .output()
        .map(|out| out.status.success())
        .unwrap_or(false);
    if removed {
        tracing::info!("nftables: removed redirect table '{tag}'");
    }
}

fn ipt_rule_args(server: SocketAddrV4, local_port: u16, tag: &str, op: &str) -> Vec<String> {
    vec![
        "-t".into(),
        "nat".into(),
        op.into(),
        "OUTPUT".into(),
        "-p".into(),
        "udp".into(),
        "-d".into(),
        server.ip().to_string(),
        "--dport".into(),
        server.port().to_string(),
        "-m".into(),
        "comment".into(),
        "--comment".into(),
        tag.to_string(),
        "-j".into(),
        "REDIRECT".into(),
        "--to-port".into(),
        local_port.to_string(),
    ]
}

fn ipt_add(ipt: &Path, server: SocketAddrV4, local_port: u16, tag: &str) -> anyhow::Result<()> {
    let out = std::process::Command::new(ipt)
        .args(ipt_rule_args(server, local_port, tag, "-A"))
        .output()
        .map_err(|e| anyhow::anyhow!("iptables failed to start: {e}"))?;
    if !out.status.success() {
        anyhow::bail!(
            "iptables rule add failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        );
    }
    tracing::info!("iptables: added REDIRECT rule (tag={tag})");
    Ok(())
}

fn ipt_del(ipt: &Path, server: SocketAddrV4, local_port: u16, tag: &str) {
    let _ = std::process::Command::new(ipt)
        .args(ipt_rule_args(server, local_port, tag, "-D"))
        .output();
    tracing::info!("iptables: removed REDIRECT rule (tag={tag})");
}

/// Return the full path to `cmd` if it exists in PATH.
fn which(cmd: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(cmd))
            .find(|p| p.is_file())
    })
}

// ─────────────────────────────────────────────────────────────────────────────
//  Blocking receive loop
// ─────────────────────────────────────────────────────────────────────────────

/// Blocking `recvmsg` loop on a non-blocking listener fd.
///
/// A short `poll` before each `recvmsg` keeps the thread idle instead of
/// busy-spinning on EAGAIN. `RecvBackoff` is reserved for real socket errors;
/// timeouts and WouldBlock are normal.
fn recv_loop(
    fd: libc::c_int,
    tx: tokio::sync::mpsc::Sender<(Vec<u8>, SocketAddrV4)>,
    counters: Arc<InterceptorCounters>,
) {
    let mut backoff = RecvBackoff::new(RECV_MAX_RETRIES, RECV_BACKOFF_BASE, RECV_BACKOFF_MAX);
    let mut buf = vec![0u8; 65535];

    loop {
        if tx.is_closed() {
            break;
        }

        let mut pollfd = libc::pollfd {
            fd,
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pollfd` is POD, the fd is a live UDP socket owned by the
        // caller's Arc clone, and the timeout is a constant.
        let ready = unsafe { libc::poll(&mut pollfd, 1, RECV_POLL_TIMEOUT_MS) };
        if ready < 0 {
            let err = std::io::Error::last_os_error();
            if err.kind() == std::io::ErrorKind::Interrupted {
                continue;
            }
            if !handle_recv_failure(&mut backoff, &counters, format!("poll: {err}")) {
                break;
            }
            continue;
        }
        if ready == 0 || pollfd.revents & libc::POLLIN == 0 {
            continue;
        }

        let mut iov = libc::iovec {
            iov_base: buf.as_mut_ptr() as *mut libc::c_void,
            iov_len: buf.len(),
        };
        // SAFETY: zeroed() on POD sockaddr_in; the kernel overwrites it.
        let mut src_addr: libc::sockaddr_in = unsafe { std::mem::zeroed() };
        // SAFETY: zeroed() on POD msghdr; pointers set below before recvmsg.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_name = &mut src_addr as *mut _ as *mut libc::c_void;
        msg.msg_namelen = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;

        // SAFETY: recvmsg with a valid msghdr and iovec into `buf`.
        let n = unsafe { libc::recvmsg(fd, &mut msg, 0) };
        if n < 0 {
            let err = std::io::Error::last_os_error();
            match err.kind() {
                std::io::ErrorKind::WouldBlock | std::io::ErrorKind::Interrupted => continue,
                _ => {
                    if !handle_recv_failure(&mut backoff, &counters, format!("recvmsg: {err}")) {
                        break;
                    }
                    continue;
                }
            }
        }
        backoff.on_success();

        let src = SocketAddrV4::new(
            std::net::Ipv4Addr::from(u32::from_be(src_addr.sin_addr.s_addr)),
            u16::from_be(src_addr.sin_port),
        );
        let data = buf[..n as usize].to_vec();
        if tx.blocking_send((data, src)).is_err() {
            break;
        }
    }
}

/// Record one real receive failure. Returns `false` once the retry budget is
/// exhausted, after publishing the fatal error to the counters.
fn handle_recv_failure(
    backoff: &mut RecvBackoff,
    counters: &InterceptorCounters,
    message: String,
) -> bool {
    counters.errors.fetch_add(1, Ordering::Relaxed);
    match backoff.on_failure() {
        Some(delay) => {
            std::thread::sleep(delay);
            true
        }
        None => {
            *counters.last_error.lock().unwrap() = Some(message);
            false
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn server() -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 9), 34568)
    }

    #[test]
    fn nft_install_script_matches_exact_ip_without_placeholder() {
        let script = nft_install_script(server(), 40000, "lightspeed_40000");
        assert!(script.contains("ip daddr 203.0.113.9 udp dport 34568 redirect to :40000"));
        assert!(script.contains("table ip lightspeed_40000"));
        assert!(
            !script.contains("0.0.0.0"),
            "exact-IP rule must never match the unspecified address"
        );
        assert!(
            !script.contains("dport 34568-"),
            "exact-IP rule must never use a port range"
        );
    }

    #[test]
    fn nft_swap_script_flushes_then_adds_in_one_transaction() {
        let script = nft_swap_script(server(), 40000, "lightspeed_40000");
        let flush_at = script
            .find("flush chain ip lightspeed_40000 output")
            .unwrap();
        let add_at = script.find("add rule ip lightspeed_40000 output").unwrap();
        assert!(flush_at < add_at);
        assert!(script.contains("ip daddr 203.0.113.9 udp dport 34568"));
        assert!(!script.contains("0.0.0.0"));
    }

    #[test]
    fn ipt_rule_args_are_exact_and_symmetric() {
        let add = ipt_rule_args(server(), 40000, "tag", "-A");
        let del = ipt_rule_args(server(), 40000, "tag", "-D");
        assert!(add.contains(&"-A".to_string()));
        assert!(del.contains(&"-D".to_string()));
        assert!(add.contains(&"203.0.113.9".to_string()));
        assert!(!add.contains(&"0.0.0.0".to_string()));
        assert_eq!(add.len(), del.len());
    }
}
