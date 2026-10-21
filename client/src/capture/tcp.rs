//! Client-side TCP terminator shared by the capture engines.
//!
//! The game's TCP handshake is answered locally with a spoofed SYN-ACK; the
//! game's payload is carried through the tunnel's version-5 datagram path, and
//! the server's ordered bytes are injected back as spoofed TCP segments.
//!
//! The state machine has no WinDivert dependency, so both capture engines can
//! use it: the legacy redirect engine (`windivert_redirect`) and the
//! process-first interceptor (`interceptor::windows`). The wire helpers it
//! needs (`build_ipv4_tcp`) live beside the UDP helpers in
//! [`super::windivert_redirect`].

use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use bytes::BytesMut;
use lightspeed_protocol::{
    build_fec_parity_packet, build_tcp_packet, derive_syn_cookie, tcp_flags, FecEncoder, FecHeader,
    Tcp5Tuple, TcpExtHeader, TcpReliable, TunnelHeader, PROTOCOL_VERSION_TCP, TCP_EXT_HEADER_SIZE,
};
use tokio::net::UdpSocket;

use super::windivert_redirect::build_ipv4_tcp;

/// A TCP connection with no traffic for this long is torn down with RST.
pub const TCP_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

/// Counters the terminator updates.
#[derive(Debug, Default)]
pub struct TcpTerminatorStats {
    /// Game TCP SYNs answered with a spoofed SYN-ACK.
    pub syn_answered: AtomicU64,
    /// Ordered server-to-game TCP payloads injected back to the game.
    pub data_injected: AtomicU64,
    /// Tunnel segments retransmitted by the reliability layer.
    pub retransmits: AtomicU64,
}

/// A tunnel segment the terminator wants to emit on the client to proxy leg.
pub struct TcpSegment {
    pub tcp_seq: u32,
    pub tcp_ack: u32,
    pub flags: u8,
    pub payload: Vec<u8>,
}

/// Result of feeding a game-side TCP segment into the terminator.
#[derive(Default)]
pub struct TcpGameOutcome {
    /// Spoofed server-to-game frames to inject back into the stack.
    pub inject: Vec<Vec<u8>>,
    /// Tunnel segments to wrap as v5 and send to the proxy.
    pub segments: Vec<TcpSegment>,
    /// True when this segment opened a connection (SYN answered).
    pub answered_syn: bool,
}

/// Result of feeding a tunnel-side v5 packet into the terminator.
#[derive(Default)]
pub struct TcpInboundOutcome {
    /// Spoofed server-to-game frames to inject back into the stack.
    pub inject: Vec<Vec<u8>>,
}

/// Result of the terminator's idle sweep.
#[derive(Default)]
pub struct TcpIdleOutcome {
    /// Spoofed server-to-game RST frames to inject.
    pub inject: Vec<Vec<u8>>,
    /// Tunnel RST segments to send to the proxy.
    pub segments: Vec<(Tcp5Tuple, TcpSegment)>,
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
    /// Next sequence number in the tunnel's client-to-proxy stream.
    next_tunnel_seq: u32,
    /// Sequenced-reliable channel shared with the proxy connector.
    reliable: TcpReliable,
    last_activity: Instant,
}

/// The client-side TCP terminator: answers the game's handshake locally and
/// splices the game's stream through the v5 tunnel.
pub struct TcpTerminator {
    conns: HashMap<Tcp5Tuple, TcpConn>,
    cookie_key: [u8; 32],
    stats: Arc<TcpTerminatorStats>,
}

impl TcpTerminator {
    pub fn new(stats: Arc<TcpTerminatorStats>) -> Self {
        Self {
            conns: HashMap::new(),
            cookie_key: syn_cookie_key(),
            stats,
        }
    }

    /// Feed a game-side TCP segment, returning the frames/segments to emit.
    #[allow(clippy::too_many_arguments)]
    pub fn on_game_segment(
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
            self.stats.syn_answered.fetch_add(1, Ordering::Relaxed);
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
    pub fn on_tunnel(
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
            return outcome; // no local connection, drop (the relay rejected it)
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
            self.stats.data_injected.fetch_add(1, Ordering::Relaxed);
        }

        outcome
    }

    /// Retransmit any segments whose RTO elapsed or whose duplicate-ACK
    /// count reached the fast-retransmit threshold.
    pub fn poll_retransmit(&mut self, now: Instant) -> Vec<(Tcp5Tuple, TcpSegment)> {
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
                self.stats.retransmits.fetch_add(1, Ordering::Relaxed);
            }
        }
        segments
    }

    /// Tear down connections idle for [`TCP_IDLE_TIMEOUT`] with RST on both
    /// ends (mirrors the proxy's UDP session-timeout posture).
    pub fn evict_idle(&mut self, now: Instant) -> TcpIdleOutcome {
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

fn tcp_tunnel_header(seq: u16, ts: u32, src: SocketAddrV4, dst: SocketAddrV4) -> TunnelHeader {
    let mut hdr = TunnelHeader::new_fec(seq, ts, src, dst)
        .with_session_token(crate::session::session_token());
    hdr.version = PROTOCOL_VERSION_TCP;
    hdr
}

/// Wrap a terminator segment as a v5 datagram (FEC + TCP extension) and send
/// it to the proxy, advancing the shared tunnel datagram sequence.
#[allow(clippy::too_many_arguments)]
pub async fn send_tcp_segment(
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

/// Deterministic per-5-minute-epoch SYN-cookie key. A live connection stores
/// its cookie, so an epoch rotation never invalidates an in-flight handshake;
/// the epoch is quantised so a restart inside the same window still derives
/// the same cookie for the same 5-tuple.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capture::windivert_redirect::parse_ipv4_tcp;
    use lightspeed_protocol::tcp_flags;
    use std::net::Ipv4Addr;

    fn fixture() -> (
        TcpTerminator,
        Tcp5Tuple,
        SocketAddrV4,
        SocketAddrV4,
        Arc<TcpTerminatorStats>,
    ) {
        let stats = Arc::new(TcpTerminatorStats::default());
        let term = TcpTerminator::new(Arc::clone(&stats));
        let tuple = Tcp5Tuple {
            src_ip: Ipv4Addr::new(192, 0, 2, 10),
            src_port: 50000,
            dst_ip: Ipv4Addr::new(203, 0, 113, 7),
            dst_port: 443,
        };
        let game = SocketAddrV4::new(tuple.src_ip, tuple.src_port);
        let server = SocketAddrV4::new(tuple.dst_ip, tuple.dst_port);
        (term, tuple, game, server, stats)
    }

    #[test]
    fn a_syn_is_answered_locally_and_opened_on_the_tunnel() {
        let (mut term, tuple, game, server, stats) = fixture();
        let out = term.on_game_segment(tuple, game, server, tcp_flags::SYN, 1000, 0, &[]);
        assert!(out.answered_syn);
        assert_eq!(out.segments.len(), 1);
        assert_eq!(out.segments[0].flags, tcp_flags::SYN);
        assert_eq!(out.segments[0].tcp_seq, 1);
        assert_eq!(out.inject.len(), 1);
        let (src, dst, flags, _seq, _ack, _payload) =
            parse_ipv4_tcp(&out.inject[0]).expect("spoofed SYN-ACK");
        assert_eq!(src, server);
        assert_eq!(dst, game);
        assert_eq!(flags, tcp_flags::SYN | tcp_flags::ACK);
        assert_eq!(stats.syn_answered.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn the_handshake_ack_establishes_the_connection_and_data_is_sequenced() {
        let (mut term, tuple, game, server, _stats) = fixture();
        let syn = term.on_game_segment(tuple, game, server, tcp_flags::SYN, 1000, 0, &[]);
        let (_, _, _, cookie, _, _) = parse_ipv4_tcp(&syn.inject[0]).expect("spoofed SYN-ACK");
        let ack = cookie.wrapping_add(1);
        let data = term.on_game_segment(tuple, game, server, tcp_flags::ACK, 1001, ack, b"hello");
        assert_eq!(data.segments.len(), 1);
        assert_eq!(&data.segments[0].payload[..], b"hello");
        assert_eq!(data.inject.len(), 1);
        assert_eq!(
            parse_ipv4_tcp(&data.inject[0]).expect("spoofed ACK").2,
            tcp_flags::ACK
        );
    }

    #[test]
    fn a_tunnel_reply_is_injected_as_ordered_server_bytes() {
        let (mut term, tuple, game, server, stats) = fixture();
        let syn = term.on_game_segment(tuple, game, server, tcp_flags::SYN, 1000, 0, &[]);
        let (_, _, _, cookie, _, _) = parse_ipv4_tcp(&syn.inject[0]).expect("spoofed SYN-ACK");
        let _ = term.on_game_segment(
            tuple,
            game,
            server,
            tcp_flags::ACK,
            1001,
            cookie.wrapping_add(1),
            b"hello",
        );
        let ext = TcpExtHeader {
            tcp_seq: 2,
            tcp_ack: 7,
            flags: tcp_flags::ACK | tcp_flags::PSH,
            window: 0,
            reserved: [0, 0],
        };
        let inbound = term.on_tunnel(tuple, server, game, &ext, b"world");
        assert_eq!(inbound.inject.len(), 1);
        let (src, dst, flags, _seq, _ack, payload) =
            parse_ipv4_tcp(&inbound.inject[0]).expect("injected data");
        assert_eq!(src, server);
        assert_eq!(dst, game);
        assert_eq!(flags, tcp_flags::ACK | tcp_flags::PSH);
        assert_eq!(payload, b"world");
        assert_eq!(stats.data_injected.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn idle_connections_are_reset_on_both_ends() {
        let (mut term, tuple, game, server, _stats) = fixture();
        term.on_game_segment(tuple, game, server, tcp_flags::SYN, 1000, 0, &[]);
        let out = term.evict_idle(Instant::now() + TCP_IDLE_TIMEOUT + Duration::from_secs(1));
        assert_eq!(out.inject.len(), 1);
        assert_eq!(out.segments.len(), 1);
        assert_eq!(out.segments[0].0, tuple);
        assert_eq!(out.segments[0].1.flags, tcp_flags::RST);
    }
}
