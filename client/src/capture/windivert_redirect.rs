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
//! For TCP games (Minecraft Java Edition) the same engine runs a **terminator**:
//! it answers the game's TCP handshake locally with a spoofed SYN-ACK, splices
//! the game's stream payload into the tunnel's version-5 datagram path, and
//! injects ordered server bytes back as spoofed TCP segments. See
//! `docs/tcp-tunnel-design.md`.
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
//! network drivers in the same way other packet-capture tools do.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use tokio::sync::oneshot;

use crate::games::TransportProto;

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
    /// Game TCP SYNs answered with a spoofed SYN-ACK (TCP termination).
    pub tcp_syn_answered: AtomicU64,
    /// Ordered server→game TCP payloads injected back to the game.
    pub tcp_data_injected: AtomicU64,
    /// Tunnel segments retransmitted by the TCP reliability layer.
    pub tcp_retransmits: AtomicU64,
    /// Auto-detected game server address. Set once the first game packet is seen.
    /// `None` means no traffic seen yet (still waiting for game to connect).
    pub detected_server: std::sync::Mutex<Option<SocketAddrV4>>,
    /// Human-readable description of the most recent fatal error, if any.
    pub last_error: std::sync::Mutex<Option<String>>,
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
    // IHL counts 32-bit words and must be at least 5 (RFC 791). Without the
    // lower bound a packet with IHL 0 passes both length checks above and every
    // field then reads at the wrong offset - the ports come out of the IP
    // header itself and the payload is mis-framed - so a malformed datagram
    // would route on garbage instead of being rejected.
    let ihl = ((raw[0] & 0x0f) as usize) * 4;
    if ihl < 20 || raw.len() < ihl + 8 {
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
//  Raw IP / TCP packet helpers
// ─────────────────────────────────────────────────────────────────────────────

/// A parsed IPv4 TCP segment: source, destination, flags, sequence number,
/// acknowledgement number, and the payload slice.
pub type ParsedIpv4Tcp<'a> = (SocketAddrV4, SocketAddrV4, u8, u32, u32, &'a [u8]);

/// Parse a raw IPv4 packet and return the TCP source/destination addresses,
/// the TCP flags byte, the sequence/acknowledgement numbers, and the payload.
///
/// Uses the same IHL validation as [`parse_ipv4_udp`] (replacing the protocol
/// test with TCP's `6`), then validates the TCP data-offset nibble and reads
/// the fixed header fields.
pub fn parse_ipv4_tcp(raw: &[u8]) -> Option<ParsedIpv4Tcp<'_>> {
    if raw.len() < 20 {
        return None;
    }
    if (raw[0] >> 4) != 4 {
        return None; // not IPv4
    }
    if raw[9] != 6 {
        return None; // not TCP
    }
    let ihl = ((raw[0] & 0x0f) as usize) * 4;
    // The TCP header is at least 20 bytes, so the data-offset nibble and the
    // sequence/ack numbers are only reachable past that floor.
    if ihl < 20 || raw.len() < ihl + 20 {
        return None;
    }
    let src_ip = Ipv4Addr::new(raw[12], raw[13], raw[14], raw[15]);
    let dst_ip = Ipv4Addr::new(raw[16], raw[17], raw[18], raw[19]);
    let src_port = u16::from_be_bytes([raw[ihl], raw[ihl + 1]]);
    let dst_port = u16::from_be_bytes([raw[ihl + 2], raw[ihl + 3]]);
    let seq = u32::from_be_bytes([raw[ihl + 4], raw[ihl + 5], raw[ihl + 6], raw[ihl + 7]]);
    let ack = u32::from_be_bytes([raw[ihl + 8], raw[ihl + 9], raw[ihl + 10], raw[ihl + 11]]);
    let data_offset = (raw[ihl + 12] >> 4) as usize;
    if data_offset < 5 {
        return None; // RFC 793 requires a data offset of at least 5
    }
    let tcp_header_len = data_offset * 4;
    if raw.len() < ihl + tcp_header_len {
        return None;
    }
    let flags = raw[ihl + 13];
    let payload = &raw[ihl + tcp_header_len..];
    Some((
        SocketAddrV4::new(src_ip, src_port),
        SocketAddrV4::new(dst_ip, dst_port),
        flags,
        seq,
        ack,
        payload,
    ))
}

/// 16-bit one's-complement sum (RFC 1071), folded to 16 bits.
fn ones_complement_sum(bytes: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < bytes.len() {
        sum += u16::from_be_bytes([bytes[i], bytes[i + 1]]) as u32;
        i += 2;
    }
    if i < bytes.len() {
        sum += (bytes[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    sum as u16
}

/// The TCP checksum pseudo-header (RFC 793), 12 bytes.
fn tcp_pseudo_header(src: Ipv4Addr, dst: Ipv4Addr, tcp_len: u16) -> [u8; 12] {
    let mut p = [0u8; 12];
    p[0..4].copy_from_slice(&src.octets());
    p[4..8].copy_from_slice(&dst.octets());
    p[9] = 6; // protocol = TCP
    p[10..12].copy_from_slice(&tcp_len.to_be_bytes());
    p
}

/// TCP checksum over the IPv4 pseudo-header + the TCP segment (whose checksum
/// field is zero).
fn tcp_checksum(src: Ipv4Addr, dst: Ipv4Addr, segment: &[u8]) -> u16 {
    let pseudo = tcp_pseudo_header(src, dst, segment.len() as u16);
    let mut sum = ones_complement_sum(&pseudo) as u32 + ones_complement_sum(segment) as u32;
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Build a raw IPv4+TCP segment from scratch.
///
/// Unlike the UDP builder, the TCP checksum is computed here over the IPv4
/// pseudo-header: a zero TCP checksum is **not** the valid "no checksum"
/// sentinel that UDP allows, so the OS stack silently drops a zero-checksum
/// segment and the game hangs in its handshake. The IPv4 header checksum is
/// also populated for the same reason.
pub fn build_ipv4_tcp(
    src: SocketAddrV4,
    dst: SocketAddrV4,
    seq: u32,
    ack: u32,
    flags: u8,
    payload: &[u8],
) -> Vec<u8> {
    let tcp_len = 20u16 + payload.len() as u16;
    let total_len = 20u16 + tcp_len;

    let mut pkt = Vec::with_capacity(total_len as usize);

    // ── IPv4 header (20 bytes, no options) ───────────────────────────────
    pkt.push(0x45); // version=4, IHL=5
    pkt.push(0x00); // DSCP/ECN
    pkt.extend_from_slice(&total_len.to_be_bytes()); // total length
    pkt.extend_from_slice(&[0x00, 0x00]); // identification
    pkt.extend_from_slice(&[0x40, 0x00]); // flags=DF, fragment offset=0
    pkt.push(64); // TTL
    pkt.push(6); // protocol = TCP
    pkt.extend_from_slice(&[0x00, 0x00]); // header checksum (filled below)
    pkt.extend_from_slice(&src.ip().octets()); // src IP
    pkt.extend_from_slice(&dst.ip().octets()); // dst IP

    let ip_checksum = !ones_complement_sum(&pkt[..20]);
    pkt[10..12].copy_from_slice(&ip_checksum.to_be_bytes());

    // ── TCP header (20 bytes, data offset 5) ─────────────────────────────
    pkt.extend_from_slice(&src.port().to_be_bytes());
    pkt.extend_from_slice(&dst.port().to_be_bytes());
    pkt.extend_from_slice(&seq.to_be_bytes());
    pkt.extend_from_slice(&ack.to_be_bytes());
    pkt.push(0x50); // data offset = 5 (20 bytes), reserved = 0
    pkt.push(flags);
    pkt.extend_from_slice(&64240u16.to_be_bytes()); // window
    pkt.extend_from_slice(&[0x00, 0x00]); // checksum (filled below)
    pkt.extend_from_slice(&[0x00, 0x00]); // urgent pointer

    // ── Payload ──────────────────────────────────────────────────────────
    pkt.extend_from_slice(payload);

    let checksum = tcp_checksum(*src.ip(), *dst.ip(), &pkt[20..]);
    pkt[36..38].copy_from_slice(&checksum.to_be_bytes());

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
    ///                from the first intercepted outbound packet).
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
    /// Which IP transport the game's traffic uses. Defaults to UDP; the
    /// Minecraft Java Edition profile passes [`TransportProto::Tcp`] to enable
    /// the client-side TCP terminator path.
    pub transport: TransportProto,
}

#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]
mod inner {
    use super::*;
    use bytes::BytesMut;
    use std::collections::{HashMap, HashSet};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::time::{Duration, Instant};

    use tokio::net::UdpSocket;

    use crate::interceptor::order::{Decision, ServerTracker, TrackerConfig};
    use crate::interceptor::recovery::RecvBackoff;
    use crate::interceptor::teardown::{recv_should_break, TeardownAck};
    use crate::interceptor::windivert_handle::{is_fwp_in_use, OwnedHandle};
    use lightspeed_protocol::{
        build_fec_data_packet, build_fec_parity_packet, build_tcp_packet, decode_fec_payload,
        decode_tcp_payload, derive_syn_cookie, tcp_flags, FecDecoder, FecEncoder, FecHeader,
        Tcp5Tuple, TcpExtHeader, TcpReliable, TunnelHeader, PROTOCOL_VERSION_TCP,
        TCP_EXT_HEADER_SIZE,
    };
    use windivert_sys::address::WINDIVERT_ADDRESS;
    use windivert_sys::{WinDivertFlags, WinDivertLayer, WinDivertShutdownMode};

    /// Upper bound on the owner-thread acknowledgement wait during teardown.
    const TEARDOWN_TIMEOUT: Duration = Duration::from_secs(2);

    /// A TCP connection with no traffic for this long is torn down with RST.
    const TCP_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

    /// Retransmit/ACK polling cadence. The RTO starts at 200 ms, so 50 ms
    /// granularity is comfortably finer than the shortest timer.
    const TCP_RETRANSMIT_TICK: Duration = Duration::from_millis(50);

    /// Deterministic teardown for the redirect's two WinDivert handles.
    ///
    /// Dropping the guard stops both owner threads (running flag plus
    /// `WinDivertShutdown`), waits for their acks, and closes the handles. It
    /// runs on every exit path, including `?` returns before the tunnel loop,
    /// so a failed setup cannot leak WFP state.
    struct RedirectTeardown {
        intercept: Arc<OwnedHandle>,
        inject: Arc<OwnedHandle>,
        running: Arc<AtomicBool>,
        ack: TeardownAck,
    }

    impl Drop for RedirectTeardown {
        fn drop(&mut self) {
            self.running.store(false, Ordering::Relaxed);
            let _ = self.intercept.shutdown(WinDivertShutdownMode::Both);
            let _ = self.inject.shutdown(WinDivertShutdownMode::Both);
            if !self.ack.wait(TEARDOWN_TIMEOUT) {
                tracing::warn!(
                    "WinDivert redirect: owner threads did not stop within {TEARDOWN_TIMEOUT:?}; \
                     leaving the handles open so a thread still inside a WinDivert call cannot \
                     race a recycled handle"
                );
                return;
            }
            let _ = self.intercept.close();
            let _ = self.inject.close();
        }
    }

    /// Log a WinDivert open failure, calling out stale WFP state when the OS
    /// reports `FWP_E_IN_USE` so the fix (cold shutdown or killing the stale
    /// process) is visible next to the raw error.
    fn log_open_failure(err: &std::io::Error) {
        if is_fwp_in_use(err) {
            tracing::error!(
                "❌ WinDivert open failed: {err}. A previous LightSpeed run left its \
                 WFP filter/callout registered; do a full cold shutdown or close the \
                 stale LightSpeed process, then try again."
            );
        } else {
            tracing::error!("❌ WinDivert open failed: {err}");
        }
    }

    /// A packet captured by the intercept thread, tagged with the transport it
    /// belongs to so the tunnel task can route UDP and TCP differently.
    enum InterceptEvent {
        Udp {
            game_src: SocketAddrV4,
            game_dst: SocketAddrV4,
            payload: Vec<u8>,
        },
        Tcp {
            game_src: SocketAddrV4,
            game_dst: SocketAddrV4,
            flags: u8,
            seq: u32,
            ack: u32,
            payload: Vec<u8>,
        },
    }

    /// A tunnel segment the terminator wants to emit on the client→proxy leg.
    struct TcpSegment {
        tcp_seq: u32,
        tcp_ack: u32,
        flags: u8,
        payload: Vec<u8>,
    }

    /// Result of feeding a game-side TCP segment into the terminator.
    #[derive(Default)]
    struct TcpGameOutcome {
        /// Spoofed server→game frames to inject back into the stack.
        inject: Vec<Vec<u8>>,
        /// Tunnel segments to wrap as v5 and send to the proxy.
        segments: Vec<TcpSegment>,
        /// True when this segment opened a connection (SYN answered).
        answered_syn: bool,
    }

    /// Result of feeding a tunnel-side v5 packet into the terminator.
    #[derive(Default)]
    struct TcpInboundOutcome {
        /// Spoofed server→game frames to inject back into the stack.
        inject: Vec<Vec<u8>>,
    }

    /// Result of the terminator's idle sweep.
    #[derive(Default)]
    struct TcpIdleOutcome {
        /// Spoofed server→game RST frames to inject.
        inject: Vec<Vec<u8>>,
        /// Tunnel RST segments to send to the proxy.
        segments: Vec<(Tcp5Tuple, TcpSegment)>,
    }

    /// Connection states shared by the terminator (and mirrored by the
    /// proxy-side connector).
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum TcpState {
        SynRcvd,
        Established,
        FinWait1,
    }

    /// Per-5-tuple connection state owned by the terminator.
    struct TcpConn {
        state: TcpState,
        /// Next sequence number we expect from the game (RFC 793).
        client_seq: u32,
        /// Next sequence number we present to the game as the server.
        server_seq: u32,
        /// Next sequence number in the tunnel's client→proxy stream.
        next_tunnel_seq: u32,
        /// Sequenced-reliable channel shared with the proxy connector.
        reliable: TcpReliable,
        last_activity: Instant,
    }

    /// The client-side TCP terminator: answers the game's handshake locally and
    /// splices the game's stream through the v5 tunnel.
    struct TcpTerminator {
        conns: HashMap<Tcp5Tuple, TcpConn>,
        cookie_key: [u8; 32],
        stats: Arc<WinDivertStats>,
    }

    impl TcpTerminator {
        fn new(stats: Arc<WinDivertStats>) -> Self {
            Self {
                conns: HashMap::new(),
                cookie_key: syn_cookie_key(),
                stats,
            }
        }

        /// Feed a game-side TCP segment, returning the frames/segments to emit.
        #[allow(clippy::too_many_arguments)]
        fn on_game_segment(
            &mut self,
            tuple: Tcp5Tuple,
            game: SocketAddrV4,
            server: SocketAddrV4,
            flags: u8,
            seq: u32,
            ack: u32,
            payload: &[u8],
        ) -> TcpGameOutcome {
            let mut outcome = TcpGameOutcome::default();

            // RST tears the connection down in both directions, no FIN dance.
            if flags & tcp_flags::RST != 0 {
                self.conns.remove(&tuple);
                outcome.segments.push(TcpSegment {
                    tcp_seq: 0,
                    tcp_ack: 0,
                    flags: tcp_flags::RST,
                    payload: Vec::new(),
                });
                return outcome;
            }

            // SYN opens a connection: answer the game locally and signal the proxy.
            if flags & tcp_flags::SYN != 0 && !self.conns.contains_key(&tuple) {
                let cookie = derive_syn_cookie(&self.cookie_key, &tuple);
                self.conns.insert(
                    tuple,
                    TcpConn {
                        state: TcpState::SynRcvd,
                        client_seq: seq.wrapping_add(1),
                        server_seq: cookie.wrapping_add(1),
                        next_tunnel_seq: 2,
                        reliable: TcpReliable::new(2, 2),
                        last_activity: Instant::now(),
                    },
                );
                outcome.inject.push(build_ipv4_tcp(
                    server,
                    game,
                    cookie,
                    seq.wrapping_add(1),
                    tcp_flags::SYN | tcp_flags::ACK,
                    &[],
                ));
                outcome.answered_syn = true;
                self.stats.tcp_syn_answered.fetch_add(1, Ordering::Relaxed);
                // Tunnel SYN: SYN consumes sequence 1 in the tunnel stream, so
                // the first data byte will be 2.
                outcome.segments.push(TcpSegment {
                    tcp_seq: 1,
                    tcp_ack: 0,
                    flags: tcp_flags::SYN,
                    payload: Vec::new(),
                });
                return outcome;
            }

            let Some(conn) = self.conns.get_mut(&tuple) else {
                return outcome; // untracked segment (e.g. a retransmitted SYN)
            };
            conn.last_activity = Instant::now();

            if conn.state == TcpState::SynRcvd {
                // The game's handshake ACK must validate the SYN cookie before
                // any data is accepted.
                if flags & tcp_flags::ACK == 0 || ack != conn.server_seq {
                    return outcome;
                }
                conn.state = TcpState::Established;
            }

            // Data: hand the game payload to the reliable sender and ACK the
            // game's bytes locally so its send window advances.
            if !payload.is_empty() {
                let tcp_ack = conn.reliable.receiver.ack();
                if let Some(tcp_seq) = conn.reliable.sender.send(payload) {
                    conn.next_tunnel_seq = tcp_seq.wrapping_add(payload.len() as u32);
                    outcome.segments.push(TcpSegment {
                        tcp_seq,
                        tcp_ack,
                        flags: tcp_flags::ACK | tcp_flags::PSH,
                        payload: payload.to_vec(),
                    });
                }
                conn.client_seq = conn.client_seq.wrapping_add(payload.len() as u32);
                outcome.inject.push(build_ipv4_tcp(
                    server,
                    game,
                    conn.server_seq,
                    conn.client_seq,
                    tcp_flags::ACK,
                    &[],
                ));
            }

            // FIN: ACK locally, drain, and propagate to the proxy.
            if flags & tcp_flags::FIN != 0 {
                conn.client_seq = conn.client_seq.wrapping_add(1); // FIN consumes 1
                conn.state = TcpState::FinWait1;
                outcome.inject.push(build_ipv4_tcp(
                    server,
                    game,
                    conn.server_seq,
                    conn.client_seq,
                    tcp_flags::ACK,
                    &[],
                ));
                outcome.segments.push(TcpSegment {
                    tcp_seq: conn.next_tunnel_seq,
                    tcp_ack: conn.reliable.receiver.ack(),
                    flags: tcp_flags::FIN,
                    payload: Vec::new(),
                });
            }

            outcome
        }

        /// Feed a tunnel-side v5 packet, returning frames to inject into the game.
        fn on_tunnel(
            &mut self,
            tuple: Tcp5Tuple,
            server: SocketAddrV4,
            game: SocketAddrV4,
            tcp_ext: &TcpExtHeader,
            payload: &[u8],
        ) -> TcpInboundOutcome {
            let mut outcome = TcpInboundOutcome::default();

            // A tunnel RST tears the connection down immediately.
            if tcp_ext.flags & tcp_flags::RST != 0 {
                self.conns.remove(&tuple);
                outcome
                    .inject
                    .push(build_ipv4_tcp(server, game, 0, 0, tcp_flags::RST, &[]));
                return outcome;
            }

            let Some(conn) = self.conns.get_mut(&tuple) else {
                return outcome; // no local connection — drop (the relay rejected it)
            };
            conn.last_activity = Instant::now();

            // The peer finished sending; close toward the game.
            if tcp_ext.flags & tcp_flags::FIN != 0 {
                outcome.inject.push(build_ipv4_tcp(
                    server,
                    game,
                    conn.server_seq,
                    conn.client_seq,
                    tcp_flags::FIN | tcp_flags::ACK,
                    &[],
                ));
                conn.server_seq = conn.server_seq.wrapping_add(1); // FIN consumes 1
                conn.state = TcpState::FinWait1;
                return outcome;
            }

            // The cumulative ACK acknowledges our sent data (and may drive
            // retransmission on the next poll).
            let _ = conn.reliable.sender.on_ack(tcp_ext.tcp_ack);

            // Deliver ordered server bytes to the game as spoofed TCP segments.
            let ordered = conn.reliable.receiver.on_segment(tcp_ext.tcp_seq, payload);
            for chunk in ordered {
                if chunk.is_empty() {
                    continue;
                }
                outcome.inject.push(build_ipv4_tcp(
                    server,
                    game,
                    conn.server_seq,
                    conn.client_seq,
                    tcp_flags::ACK | tcp_flags::PSH,
                    &chunk,
                ));
                conn.server_seq = conn.server_seq.wrapping_add(chunk.len() as u32);
                self.stats.tcp_data_injected.fetch_add(1, Ordering::Relaxed);
            }

            outcome
        }

        /// Retransmit any segments whose RTO elapsed or whose duplicate-ACK
        /// count reached the fast-retransmit threshold.
        fn poll_retransmit(&mut self, now: Instant) -> Vec<(Tcp5Tuple, TcpSegment)> {
            let mut segments = Vec::new();
            for (tuple, conn) in self.conns.iter_mut() {
                while let Some((tcp_seq, payload)) = conn.reliable.sender.poll_retransmit(now) {
                    segments.push((
                        *tuple,
                        TcpSegment {
                            tcp_seq,
                            tcp_ack: conn.reliable.receiver.ack(),
                            flags: tcp_flags::ACK | tcp_flags::PSH,
                            payload: payload.to_vec(),
                        },
                    ));
                    self.stats.tcp_retransmits.fetch_add(1, Ordering::Relaxed);
                }
            }
            segments
        }

        /// Tear down connections idle for [`TCP_IDLE_TIMEOUT`] with RST on both
        /// ends (mirrors the proxy's UDP session-timeout posture).
        fn evict_idle(&mut self, now: Instant) -> TcpIdleOutcome {
            let stale: Vec<Tcp5Tuple> = self
                .conns
                .iter()
                .filter(|(_, conn)| now.duration_since(conn.last_activity) > TCP_IDLE_TIMEOUT)
                .map(|(tuple, _)| *tuple)
                .collect();
            let mut outcome = TcpIdleOutcome::default();
            for tuple in stale {
                self.conns.remove(&tuple);
                let server = SocketAddrV4::new(tuple.dst_ip, tuple.dst_port);
                let game = SocketAddrV4::new(tuple.src_ip, tuple.src_port);
                outcome
                    .inject
                    .push(build_ipv4_tcp(server, game, 0, 0, tcp_flags::RST, &[]));
                outcome.segments.push((
                    tuple,
                    TcpSegment {
                        tcp_seq: 0,
                        tcp_ack: 0,
                        flags: tcp_flags::RST,
                        payload: Vec::new(),
                    },
                ));
            }
            outcome
        }
    }

    /// Deterministic per-5-minute-epoch SYN-cookie key. A live connection
    /// stores its cookie, so an epoch rotation never invalidates an in-flight
    /// handshake; the epoch is quantised so a restart inside the same window
    /// still derives the same cookie for the same 5-tuple.
    fn syn_cookie_key() -> [u8; 32] {
        let epoch = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            / 300;
        let mut key = [0u8; 32];
        key[..8].copy_from_slice(&epoch.to_be_bytes());
        key
    }

    /// Current wall-clock time in microseconds (u32), the tunnel's timestamp.
    fn now_us() -> u32 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u32
    }

    /// Build a version-5 tunnel header (FEC + TCP extensions follow).
    fn tcp_tunnel_header(seq: u16, ts: u32, src: SocketAddrV4, dst: SocketAddrV4) -> TunnelHeader {
        let mut hdr = TunnelHeader::new_fec(seq, ts, src, dst)
            .with_session_token(crate::session::session_token());
        hdr.version = PROTOCOL_VERSION_TCP;
        hdr
    }

    /// Wrap a terminator segment as a v5 datagram (FEC + TCP extension) and send
    /// it to the proxy, advancing the shared tunnel datagram sequence.
    #[allow(clippy::too_many_arguments)]
    async fn send_tcp_segment(
        socket: &UdpSocket,
        proxy: SocketAddrV4,
        encoder: &mut FecEncoder,
        seq: &mut u16,
        ts: u32,
        src: SocketAddrV4,
        dst: SocketAddrV4,
        segment: &TcpSegment,
    ) {
        let header = tcp_tunnel_header(*seq, ts, src, dst);
        let block_id = encoder.block_id();
        let index = encoder.current_index();
        let k = encoder.k_size();
        let fec = FecHeader::data(block_id, index, k);
        let tcp_ext = TcpExtHeader {
            tcp_seq: segment.tcp_seq,
            tcp_ack: segment.tcp_ack,
            flags: segment.flags,
            window: 0,
            reserved: [0, 0],
        };

        // FEC protects the TCP extension bytes too, so feed the encoder exactly
        // the bytes that follow the FEC header on the wire.
        let mut protected = BytesMut::with_capacity(TCP_EXT_HEADER_SIZE + segment.payload.len());
        tcp_ext.encode(&mut protected);
        protected.extend_from_slice(&segment.payload);

        let packet = build_tcp_packet(&header, &fec, &tcp_ext, &segment.payload);
        let _ = socket.send_to(&packet, proxy).await;
        *seq = seq.wrapping_add(1);

        if let Some(parity) = encoder.add_packet(&protected) {
            let parity_seq = *seq;
            let parity_header = tcp_tunnel_header(parity_seq, ts, src, dst);
            let parity_fec = FecHeader::parity(block_id, k);
            let parity_packet = build_fec_parity_packet(&parity_header, &parity_fec, &parity);
            let _ = socket.send_to(&parity_packet, proxy).await;
            *seq = seq.wrapping_add(1);
        }
    }

    /// Run the WinDivert active redirect loop.
    ///
    /// Spawns two `spawn_blocking` threads:
    /// - **intercept thread**: WinDivert recv loop — captures outbound Game→Server
    ///   packets, extracts UDP payload (or TCP segment), sends via an mpsc channel.
    /// - **inject thread**: WinDivert send loop — receives assembled IP frames
    ///   from an mpsc channel and injects them into the IP stack.
    ///
    /// An async task in between handles the proxy tunnel socket bidirectionally.
    #[allow(clippy::too_many_arguments)]
    pub async fn run_windivert_redirect(
        cfg: WinDivertConfig,
        stats: Arc<WinDivertStats>,
        shutdown_rx: oneshot::Receiver<()>,
    ) -> anyhow::Result<()> {
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
        //
        // The proto token follows `config.Transport`: UDP keeps today's
        // `udp and outbound …` grammar verbatim, while a TCP profile emits the
        // `tcp and outbound …` equivalent so the kernel matches TCP segments.
        let (proto, port_field) = match cfg.transport {
            TransportProto::Udp => ("udp", "udp.DstPort"),
            TransportProto::Tcp => ("tcp", "tcp.DstPort"),
        };
        let out_filter = match pre_known_server {
            Some(s) => format!(
                "{proto} and outbound and ip.DstAddr == {} and {port_field} == {}",
                s.ip(),
                s.port()
            ),
            None => {
                if port_lo == port_hi {
                    format!("{proto} and outbound and {port_field} == {}", port_lo)
                } else {
                    format!(
                        "{proto} and outbound and {port_field} >= {} and {port_field} <= {}",
                        port_lo, port_hi
                    )
                }
            }
        };

        tracing::info!("   Outbound WinDivert filter: {}", out_filter);

        // Open WinDivert handle for outbound interception
        let wd_intercept = match OwnedHandle::open(
            &out_filter,
            WinDivertLayer::Network,
            0,
            WinDivertFlags::new(),
        ) {
            Ok(h) => Arc::new(h),
            Err(e) => {
                log_open_failure(&e);
                return Err(anyhow::anyhow!(
                    "WinDivert open failed (need Administrator + WinDivert64.sys): {e}"
                ));
            }
        };

        // Open WinDivert handle for injection (no filter — only used for sending).
        // IMPORTANT: do NOT set sniff flag here — sniff makes the handle read-only
        // and WinDivertSend() will silently fail, meaning game never gets responses.
        let wd_inject =
            match OwnedHandle::open("false", WinDivertLayer::Network, 0, WinDivertFlags::new()) {
                Ok(h) => Arc::new(h),
                Err(e) => {
                    log_open_failure(&e);
                    let _ = wd_intercept.close();
                    return Err(anyhow::anyhow!("WinDivert inject handle open failed: {e}"));
                }
            };

        // Channels between blocking WinDivert threads and async tunnel task
        // intercept_tx: one [`InterceptEvent`] per captured packet.
        let (intercept_tx, mut intercept_rx) = tokio::sync::mpsc::channel::<InterceptEvent>(256);
        // inject_tx: raw IPv4 frames (UDP or TCP) to inject back into the stack
        let (inject_tx, inject_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);

        // Shared interface-address cache: populated by the intercept thread from
        // the first captured outbound packet.  The inject thread clones this address
        // (with Outbound=false) so spoofed server→game responses are delivered on
        // the correct network adapter.  A zeroed address (IfIdx=0) is almost never
        // the LAN adapter, causing WinDivert to silently drop injected packets.
        let if_addr_cache: Arc<std::sync::Mutex<Option<WINDIVERT_ADDRESS>>> =
            Arc::new(std::sync::Mutex::new(None));

        let running = Arc::new(AtomicBool::new(true));

        let (ack_tx, teardown_ack) = TeardownAck::new(2);
        // Released on every exit path, including early `?` returns below.
        let _teardown = RedirectTeardown {
            intercept: Arc::clone(&wd_intercept),
            inject: Arc::clone(&wd_inject),
            running: Arc::clone(&running),
            ack: teardown_ack,
        };

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
        let ack_ic = ack_tx.clone();
        let intercept_transport = cfg.transport;
        tokio::task::spawn_blocking(move || {
            tracing::info!("WinDivert intercept thread started");

            const RECV_MAX_RETRIES: u32 = 5;
            const RECV_BACKOFF_BASE: Duration = Duration::from_millis(10);
            const RECV_BACKOFF_MAX: Duration = Duration::from_millis(400);

            let mut tracker = ServerTracker::new(TrackerConfig::default());
            if let Some(server) = pre_known_server {
                tracker.seed(std::slice::from_ref(&server), Instant::now());
            }
            let mut reported_server: Option<SocketAddrV4> = pre_known_server;
            let mut backoff =
                RecvBackoff::new(RECV_MAX_RETRIES, RECV_BACKOFF_BASE, RECV_BACKOFF_MAX);

            // 65 535 bytes covers the largest possible IPv4 datagram.
            let mut recv_buf = vec![0u8; 65535];

            loop {
                if !running_ic.load(Ordering::Relaxed) {
                    break;
                }
                // recv() is blocking — will return each captured packet.
                match wd_ic.recv(&mut recv_buf) {
                    Ok((len, addr)) => {
                        backoff.on_success();
                        let data = &recv_buf[..len];
                        let parsed = if intercept_transport == TransportProto::Tcp {
                            parse_ipv4_tcp(data).map(|(src, dst, flags, seq, ack, payload)| {
                                InterceptEvent::Tcp {
                                    game_src: src,
                                    game_dst: dst,
                                    flags,
                                    seq,
                                    ack,
                                    payload: payload.to_vec(),
                                }
                            })
                        } else {
                            parse_ipv4_udp(data).map(|(src, dst, payload)| InterceptEvent::Udp {
                                game_src: src,
                                game_dst: dst,
                                payload: payload.to_vec(),
                            })
                        };

                        match parsed {
                            Some(event) => {
                                let (game_dst, payload_len) = match &event {
                                    InterceptEvent::Udp {
                                        game_dst, payload, ..
                                    } => (*game_dst, payload.len()),
                                    InterceptEvent::Tcp {
                                        game_dst, payload, ..
                                    } => (*game_dst, payload.len()),
                                };

                                // ── Cache the interface index on first outbound packet ──
                                // The address from an intercepted outbound packet contains
                                // the real LAN adapter IfIdx.  We copy it, set
                                // Outbound=false, and store it so the inject thread can
                                // deliver spoofed server→game responses on the correct
                                // interface instead of the zeroed IfIdx=0 default which
                                // almost always resolves to the wrong adapter and causes
                                // WinDivert to silently drop all injected packets.
                                {
                                    let mut guard = if_cache_ic
                                        .lock()
                                        .unwrap_or_else(|poisoned| poisoned.into_inner());
                                    if guard.is_none() {
                                        let mut cached = addr;
                                        cached.set_outbound(false); // inbound direction for inject
                                        cached.set_ipchecksum(false); // let WinDivert recompute
                                        cached.set_udpchecksum(false); // let WinDivert recompute

                                        // SAFETY: [Category 8 - FFI] `addr` came from a
                                        // successful network-layer `WinDivertRecv`, so the
                                        // driver initialized the `Network` union member.
                                        // These read two plain `u32`s from that
                                        // `#[repr(C)]` union.
                                        let if_idx =
                                            unsafe { cached.union_field.Network.interface_id };
                                        let sub_if_idx =
                                            unsafe { cached.union_field.Network.subinterface_id };
                                        tracing::info!(
                                            "🔗 Cached inject interface: IfIdx={if_idx} SubIfIdx={sub_if_idx}"
                                        );
                                        *guard = Some(cached);
                                    }
                                }

                                // TCP control segments (SYN/FIN/ACK/RST) are zero-payload
                                // but still carry a detection signal, so count them as one
                                // byte for the tracker instead of being dropped as empty.
                                let detect_len = if matches!(event, InterceptEvent::Tcp { .. }) {
                                    payload_len.max(1)
                                } else {
                                    payload_len
                                };

                                match tracker.observe(game_dst, Instant::now(), detect_len) {
                                    Decision::Tunnel(server) => {
                                        if reported_server != Some(server) {
                                            reported_server = Some(server);
                                            if let Ok(mut guard) = stats_ic.detected_server.lock() {
                                                *guard = Some(server);
                                            }
                                            tracing::info!("🔍 Locked game server: {}", server);
                                        }
                                        stats_ic
                                            .packets_intercepted
                                            .fetch_add(1, Ordering::Relaxed);
                                        stats_ic
                                            .bytes_intercepted
                                            .fetch_add(payload_len as u64, Ordering::Relaxed);
                                        if itx.blocking_send(event).is_err() {
                                            break;
                                        }
                                    }
                                    Decision::PassThrough
                                    | Decision::StartDetection
                                    | Decision::ShadowDirect => {
                                        let _ = wd_ic.send(data, &addr);
                                    }
                                    Decision::ResetToDetection => {
                                        tracing::info!("🔄 Locked server stale — re-detecting");
                                        reported_server = None;
                                        if let Ok(mut guard) = stats_ic.detected_server.lock() {
                                            *guard = None;
                                        }
                                        {
                                            let mut cache_guard = if_cache_ic
                                                .lock()
                                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                                            *cache_guard = None;
                                        }
                                        let _ = wd_ic.send(data, &addr);
                                    }
                                }
                            }
                            None => {
                                // Non-IPv4 / non-matching transport — re-inject unchanged.
                                let _ = wd_ic.send(data, &addr);
                            }
                        }
                    }
                    Err(e) => {
                        let raw = e.raw_os_error().unwrap_or(0);
                        if recv_should_break(running_ic.load(Ordering::Relaxed), raw) {
                            break;
                        }
                        stats_ic.errors.fetch_add(1, Ordering::Relaxed);
                        match backoff.on_failure() {
                            Some(delay) => {
                                tracing::warn!(
                                    "WinDivert recv error (retry {}/{} in {:?}): {}",
                                    backoff.consecutive_failures(),
                                    RECV_MAX_RETRIES,
                                    delay,
                                    e
                                );
                                std::thread::sleep(delay);
                            }
                            None => {
                                let msg = format!(
                                    "WinDivert recv failed {} times consecutively: {}",
                                    backoff.consecutive_failures(),
                                    e
                                );
                                tracing::error!("{}", msg);
                                if let Ok(mut guard) = stats_ic.last_error.lock() {
                                    *guard = Some(msg);
                                }
                                break;
                            }
                        }
                    }
                }
            }
            // The owner closes the handle only after every owner thread sends
            // this ack, so the thread must not touch it afterwards.
            let _ = ack_ic.send(());
            tracing::info!("WinDivert intercept thread exiting");
        });

        // ── Blocking inject thread ───────────────────────────────────────
        let wd_inj = Arc::clone(&wd_inject);
        let running_inj = Arc::clone(&running);
        let stats_inj = Arc::clone(&stats);
        let if_cache_inj = Arc::clone(&if_addr_cache);
        let ack_inj = ack_tx.clone();
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
                            let guard = if_cache_inj
                                .lock()
                                .unwrap_or_else(|poisoned| poisoned.into_inner());
                            match guard.as_ref() {
                                Some(a) => *a,
                                None => {
                                    tracing::debug!(
                                        "inject: interface not cached yet, using zeroed addr"
                                    );
                                    WINDIVERT_ADDRESS::default()
                                }
                            }
                        };
                        match wd_inj.send(&raw_pkt, &addr) {
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
            // The owner closes the handle only after every owner thread sends
            // this ack, so the thread must not touch it afterwards.
            let _ = ack_inj.send(());
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
                    let hdr = lightspeed_protocol::TunnelHeader::keepalive(ka_seq, now_us)
                        .with_session_token(crate::session::session_token());
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

        // ── TCP terminator state ─────────────────────────────────────────
        // v5 always carries FEC (the lossy leg needs it under the reliability
        // layer), so the terminator keeps its own encoder/decoder regardless of
        // the UDP `fec_enabled` flag.
        let mut tcp_terminator =
            (cfg.transport == TransportProto::Tcp).then(|| TcpTerminator::new(Arc::clone(&stats)));
        let mut tcp_fec_encoder =
            (cfg.transport == TransportProto::Tcp).then(|| FecEncoder::new(cfg.fec_k));
        let mut tcp_fec_decoder = (cfg.transport == TransportProto::Tcp).then(FecDecoder::new);
        let mut tcp_fw_ports: HashSet<u16> = HashSet::new();
        let mut tcp_tick = tokio::time::interval(TCP_RETRANSMIT_TICK);
        tcp_tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

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
                    let event = match maybe_pkt {
                        Some(e) => e,
                        None => break, // intercept thread exited
                    };

                    match event {
                        InterceptEvent::Udp { game_src, game_dst, payload } => {
                            // Learn / update game source address
                            if game_src_learned.is_none() {
                                tracing::info!("🎮 Game client detected at {}", game_src);
                            }
                            game_src_learned = Some(game_src);

                            // Update active_server from each packet (handles reconnects).
                            active_server = Some(game_dst);

                            let ts = now_us();

                            if let Some(ref mut encoder) = fec_encoder {
                                let block_id = encoder.block_id();
                                let index = encoder.current_index();
                                let hdr = lightspeed_protocol::TunnelHeader::new_fec(
                                    seq, ts, game_src, game_dst,
                                )
                                .with_session_token(crate::session::session_token());
                                let fec_hdr = FecHeader::data(block_id, index, cfg.fec_k);
                                let pkt_buf = build_fec_data_packet(&hdr, &fec_hdr, &payload);
                                let parity = encoder.add_packet(&payload);
                                crate::latency::record_outbound(*game_dst.ip());
                                let _ = tunnel_socket.send_to(&pkt_buf, proxy_addr).await;
                                if let Some(parity_bytes) = parity {
                                    let ps = seq.wrapping_add(1);
                                    let ph = lightspeed_protocol::TunnelHeader::new_fec(
                                        ps, ts, game_src, game_dst,
                                    )
                                    .with_session_token(crate::session::session_token());
                                    let pfec = FecHeader::parity(block_id, cfg.fec_k);
                                    let pb = build_fec_parity_packet(&ph, &pfec, &parity_bytes);
                                    let _ = tunnel_socket.send_to(&pb, proxy_addr).await;
                                    seq = seq.wrapping_add(1);
                                }
                            } else {
                                let hdr = lightspeed_protocol::TunnelHeader::new(
                                    seq, ts, game_src, game_dst,
                                )
                                .with_session_token(crate::session::session_token());
                                let pkt_bytes = hdr.encode_with_payload(&payload);
                                crate::latency::record_outbound(*game_dst.ip());
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

                        InterceptEvent::Tcp { game_src, game_dst, flags, seq: game_seq, ack: game_ack, payload } => {
                            let ts = now_us();
                            let tuple = Tcp5Tuple {
                                src_ip: *game_src.ip(),
                                src_port: game_src.port(),
                                dst_ip: *game_dst.ip(),
                                dst_port: game_dst.port(),
                            };
                            let term = tcp_terminator
                                .as_mut()
                                .expect("TCP terminator is present for Tcp transport");
                            let outcome = term.on_game_segment(
                                tuple, game_src, game_dst, flags, game_seq, game_ack, &payload,
                            );

                            for frame in outcome.inject {
                                stats.bytes_injected.fetch_add(frame.len() as u64, Ordering::Relaxed);
                                if inject_tx.try_send(frame).is_err() {
                                    tracing::warn!("Inject channel full — dropping TCP segment");
                                    stats.errors.fetch_add(1, Ordering::Relaxed);
                                }
                            }

                            let encoder = tcp_fec_encoder
                                .as_mut()
                                .expect("TCP FEC encoder is present for Tcp transport");
                            for segment in &outcome.segments {
                                send_tcp_segment(
                                    &tunnel_socket, proxy_addr, encoder, &mut seq, ts,
                                    game_src, game_dst, segment,
                                )
                                .await;
                            }

                            if outcome.answered_syn && tcp_fw_ports.insert(game_src.port()) {
                                add_tcp_firewall_rule(*game_dst.ip(), game_src.port());
                            }

                            tracing::trace!(
                                tcp_flags = flags,
                                dst = %game_dst,
                                payload_len = payload.len(),
                                "WD: TCP Game → Proxy"
                            );
                        }
                    }
                }

                // ── TCP retransmit / idle sweep ──────────────────────────
                _ = tcp_tick.tick() => {
                    let Some(term) = tcp_terminator.as_mut() else { continue; };
                    let now = Instant::now();
                    let ts = now_us();

                    let retransmits = term.poll_retransmit(now);
                    {
                        let encoder = tcp_fec_encoder
                            .as_mut()
                            .expect("TCP FEC encoder is present for Tcp transport");
                        for (tuple, segment) in retransmits {
                            let src = SocketAddrV4::new(tuple.src_ip, tuple.src_port);
                            let dst = SocketAddrV4::new(tuple.dst_ip, tuple.dst_port);
                            send_tcp_segment(
                                &tunnel_socket, proxy_addr, encoder, &mut seq, ts,
                                src, dst, &segment,
                            )
                            .await;
                        }
                    }

                    let idle = term.evict_idle(now);
                    for frame in idle.inject {
                        if inject_tx.try_send(frame).is_err() {
                            tracing::warn!("Inject channel full — dropping TCP RST");
                            stats.errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                    {
                        let encoder = tcp_fec_encoder
                            .as_mut()
                            .expect("TCP FEC encoder is present for Tcp transport");
                        for (tuple, segment) in idle.segments {
                            let src = SocketAddrV4::new(tuple.src_ip, tuple.src_port);
                            let dst = SocketAddrV4::new(tuple.dst_ip, tuple.dst_port);
                            send_tcp_segment(
                                &tunnel_socket, proxy_addr, encoder, &mut seq, ts,
                                src, dst, &segment,
                            )
                            .await;
                        }
                    }
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

                    if !header.is_keepalive() {
                        crate::latency::record_inbound(*header.orig_src_addr().ip());
                    }

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

                    // ── TCP (v5): run the terminator on the ordered stream ─
                    if header.has_tcp() {
                        // FEC-decode the protected bytes first (the FEC layer
                        // treats the TCP extension as opaque), then split the
                        // extension header from the game stream payload.
                        let protected = match tcp_fec_decoder
                            .as_mut()
                            .and_then(|dec| decode_fec_payload(payload, dec))
                        {
                            Some(p) => p,
                            None => continue,
                        };
                        let Some((tcp_ext, game_payload)) = decode_tcp_payload(&protected) else {
                            continue;
                        };

                        // make_response swaps src/dst, so on the inbound leg the
                        // header's source is the server and its destination the game.
                        let server = header.orig_src_addr();
                        let game = header.orig_dst_addr();
                        let tuple = Tcp5Tuple {
                            src_ip: *game.ip(),
                            src_port: game.port(),
                            dst_ip: *server.ip(),
                            dst_port: server.port(),
                        };
                        let term = tcp_terminator
                            .as_mut()
                            .expect("TCP terminator is present for Tcp transport");
                        let outcome = term.on_tunnel(tuple, server, game, &tcp_ext, game_payload);
                        for frame in outcome.inject {
                            stats.bytes_injected.fetch_add(frame.len() as u64, Ordering::Relaxed);
                            if inject_tx.try_send(frame).is_err() {
                                tracing::warn!("Inject channel full — dropping TCP segment");
                                stats.errors.fetch_add(1, Ordering::Relaxed);
                            }
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
                        if let Some(ref mut dec) = fec_decoder {
                            decode_fec_payload(payload, dec)
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
        for port in tcp_fw_ports.drain() {
            remove_tcp_firewall_rule(port);
        }
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

        if let Some(msg) = stats.last_error.lock().ok().and_then(|guard| guard.clone()) {
            anyhow::bail!(msg);
        }

        Ok(())
    }

    // ── Firewall helpers for tunnel socket inbound ──────────────────────────

    const FW_RULE_BASE: &str = "LightSpeed WinDivert Tunnel";

    fn add_windivert_firewall_rule(port: u16) {
        let exe = std::env::current_exe().unwrap_or_default();
        let name = format!("{} {}", FW_RULE_BASE, port);
        let _ = crate::process::silent_command("netsh")
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
        let _ = crate::process::silent_command("netsh")
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

    /// Allow the spoofed server→game SYN-ACK and data segments that the
    /// terminator injects toward the game, which Windows Firewall would
    /// otherwise drop (they carry the real server's IP as source).
    fn add_tcp_firewall_rule(server_ip: Ipv4Addr, game_port: u16) {
        let name = format!("{} TCP {}", FW_RULE_BASE, game_port);
        let _ = crate::process::silent_command("netsh")
            .args([
                "advfirewall",
                "firewall",
                "add",
                "rule",
                &format!("name={}", name),
                "protocol=TCP",
                "dir=in",
                "action=allow",
                &format!("remoteip={}", server_ip),
                &format!("localport={}", game_port),
            ])
            .output();
        tracing::info!(
            "🔓 Firewall: added inbound TCP allow rule ({}:{})",
            server_ip,
            game_port
        );
    }

    fn remove_tcp_firewall_rule(game_port: u16) {
        let name = format!("{} TCP {}", FW_RULE_BASE, game_port);
        let _ = crate::process::silent_command("netsh")
            .args([
                "advfirewall",
                "firewall",
                "delete",
                "rule",
                &format!("name={}", name),
            ])
            .output();
        tracing::info!(
            "🔒 Firewall: removed inbound TCP allow rule (port {})",
            game_port
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

#[cfg(test)]
mod tests {
    use super::{build_ipv4_tcp, parse_ipv4_tcp, parse_ipv4_udp};
    use std::net::{Ipv4Addr, SocketAddrV4};

    /// A minimal valid IPv4+UDP datagram with the given IHL nibble.
    ///
    /// Padded to at least 20 bytes so the fixture itself is well formed even
    /// when the IHL it advertises is not; the parser is what should reject it.
    fn datagram(ihl_words: u8, port: u16) -> Vec<u8> {
        let ihl = (ihl_words as usize) * 4;
        let mut p = vec![0u8; (ihl + 8).max(20)];
        p[0] = 0x40 | ihl_words;
        p[9] = 17; // UDP
        p[12..16].copy_from_slice(&[192, 168, 1, 2]);
        p[16..20].copy_from_slice(&[10, 0, 0, 1]);
        if p.len() >= ihl + 8 {
            p[ihl..ihl + 2].copy_from_slice(&port.to_be_bytes());
            p[ihl + 2..ihl + 4].copy_from_slice(&7777u16.to_be_bytes());
        }
        p
    }

    #[test]
    fn parses_a_well_formed_datagram() {
        let packet = datagram(5, 30000);
        let (src, dst, payload) = parse_ipv4_udp(&packet).expect("should parse");
        assert_eq!(src.port(), 30000);
        assert_eq!(dst.port(), 7777);
        assert!(payload.is_empty(), "no payload in this fixture");
    }

    #[test]
    fn rejects_an_ihl_below_the_minimum() {
        // IHL 0 is malformed (RFC 791 requires >= 5). It satisfies every length
        // check, so without an explicit lower bound the port would be read out
        // of the IP header itself and a bad datagram would parse as valid.
        assert!(
            parse_ipv4_udp(&datagram(0, 30000)).is_none(),
            "IHL 0 must be rejected, not parsed"
        );
        assert!(
            parse_ipv4_udp(&datagram(4, 30000)).is_none(),
            "IHL 16 is too short"
        );
    }

    #[test]
    fn rejects_truncated_and_non_udp_input() {
        assert!(parse_ipv4_udp(&[]).is_none(), "empty input");
        assert!(
            parse_ipv4_udp(&[0u8; 19]).is_none(),
            "shorter than an IP header"
        );

        let mut tcp = datagram(5, 30000);
        tcp[9] = 6; // TCP
        assert!(parse_ipv4_udp(&tcp).is_none(), "non-UDP must be rejected");

        let mut v6 = datagram(5, 30000);
        v6[0] = 0x65; // version 6, IHL 5
        assert!(parse_ipv4_udp(&v6).is_none(), "non-IPv4 must be rejected");

        // Header claims 20 bytes and UDP needs 8, so 24 is the floor.
        let truncated = datagram(5, 30000)[..27].to_vec();
        assert!(parse_ipv4_udp(&truncated).is_none());
    }

    /// Minimal IPv4 (IHL 5, protocol 6) + 20-byte TCP header with data offset
    /// 5, SYN set, seq `0x01020304`, and a 3-byte payload.
    fn tcp_fixture() -> Vec<u8> {
        let mut p = vec![0u8; 20 + 20 + 3];
        p[0] = 0x45; // version=4, IHL=5
        p[9] = 6; // TCP
        p[12..16].copy_from_slice(&[192, 168, 1, 2]);
        p[16..20].copy_from_slice(&[10, 0, 0, 1]);
        // TCP header
        p[20..22].copy_from_slice(&12345u16.to_be_bytes()); // src port
        p[22..24].copy_from_slice(&25565u16.to_be_bytes()); // dst port
        p[24..28].copy_from_slice(&0x0102_0304u32.to_be_bytes()); // seq
        p[28..32].copy_from_slice(&0x0506_0708u32.to_be_bytes()); // ack
        p[32] = 0x50; // data offset 5 (20 bytes), reserved 0
        p[33] = 0x02; // SYN flag
        p[34..36].copy_from_slice(&64240u16.to_be_bytes()); // window
        p[36..38].copy_from_slice(&[0x00, 0x00]); // checksum
        p[38..40].copy_from_slice(&[0x00, 0x00]); // urgent pointer
        p[40..43].copy_from_slice(b"XYZ"); // 3-byte payload
        p
    }

    #[test]
    fn parse_ipv4_tcp_reads_data_offset() {
        let packet = tcp_fixture();
        let (src, dst, flags, seq, ack, payload) = parse_ipv4_tcp(&packet).expect("should parse");
        assert_eq!(src.port(), 12345);
        assert_eq!(dst.port(), 25565);
        assert_eq!(flags, 0x02, "SYN flag must be read");
        assert_eq!(seq, 0x0102_0304, "sequence number must be read");
        assert_eq!(ack, 0x0506_0708, "acknowledgement number must be read");
        assert_eq!(payload, b"XYZ", "payload must follow the data offset");
    }

    #[test]
    fn parse_ipv4_tcp_rejects_non_tcp_and_short_headers() {
        assert!(parse_ipv4_tcp(&[]).is_none(), "empty input");

        let mut udp = tcp_fixture();
        udp[9] = 17; // UDP
        assert!(parse_ipv4_tcp(&udp).is_none(), "non-TCP must be rejected");

        let mut short_data_offset = tcp_fixture();
        short_data_offset[32] = 0x40; // data offset 4 (< 5) is invalid
        assert!(parse_ipv4_tcp(&short_data_offset).is_none());

        // A header claiming more TCP bytes than the packet holds is rejected.
        let mut long_data_offset = tcp_fixture();
        long_data_offset[32] = 0x60; // data offset 6 (24 bytes) past the packet
        assert!(parse_ipv4_tcp(&long_data_offset).is_none());
    }

    #[test]
    fn build_ipv4_tcp_computes_a_real_tcp_checksum() {
        let src = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 100), 12345);
        let dst = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 25565);
        let mut pkt = build_ipv4_tcp(src, dst, 0x0102_0304, 0x0506_0708, 0x18, b"MC");

        assert_eq!(pkt.len(), 20 + 20 + 2);
        assert_eq!(pkt[9], 6, "protocol must be TCP");

        // The TCP checksum is computed over the IPv4 pseudo-header + segment;
        // a zero value is not a valid "no checksum" for TCP and would be
        // silently dropped by the OS stack.
        let tcp_checksum = u16::from_be_bytes([pkt[36], pkt[37]]);
        assert_ne!(tcp_checksum, 0, "the TCP checksum must be populated");

        let ip_checksum = u16::from_be_bytes([pkt[10], pkt[11]]);
        assert_ne!(ip_checksum, 0, "the IPv4 header checksum must be populated");

        // Independently re-derive the TCP checksum and assert byte equality.
        // The checksum is computed over the segment with its checksum field
        // zeroed, so clear it before re-summing the returned packet.
        pkt[36..38].copy_from_slice(&[0x00, 0x00]);
        let mut sum: u32 = 0;
        let acc = |bytes: &[u8], sum: &mut u32| {
            let mut i = 0;
            while i + 1 < bytes.len() {
                *sum += u16::from_be_bytes([bytes[i], bytes[i + 1]]) as u32;
                i += 2;
            }
            if i < bytes.len() {
                *sum += (bytes[i] as u32) << 8;
            }
        };
        let mut pseudo = [0u8; 12];
        pseudo[0..4].copy_from_slice(&src.ip().octets());
        pseudo[4..8].copy_from_slice(&dst.ip().octets());
        pseudo[9] = 6;
        pseudo[10..12].copy_from_slice(&22u16.to_be_bytes()); // 20 + 2
        acc(&pseudo, &mut sum);
        acc(&pkt[20..], &mut sum);
        while sum >> 16 != 0 {
            sum = (sum & 0xffff) + (sum >> 16);
        }
        assert_eq!(tcp_checksum, !(sum as u16));
    }
}
