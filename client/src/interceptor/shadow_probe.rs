//! Linux shadow-direct probe socket: a like-for-like direct application RTT.
//!
//! Windows re-injects the sampled game packet UNCHANGED onto the direct path
//! (WinDivert `send` with a sniff inbound handle). Linux cannot do that: the
//! interceptor's `nftables`/`iptables` `nat OUTPUT REDIRECT` sends every
//! matching game packet to the local listener before userspace sees it, so the
//! original packet is already committed to the tunnel. Instead, when the shadow
//! sampler claims a sample, Linux sends an EXTRA copy of the game's OWN bytes to
//! the real server from this dedicated probe socket and times the reply. The
//! game's original packet is still tunnelled exactly as before; this is an
//! additional packet, never a diversion and never a modification.
//!
//! Anti-cheat safety: the probe is one extra packet per server per 30s (the
//! tracker's [`SHADOW_SAMPLE_INTERVAL`](crate::latency::SHADOW_SAMPLE_INTERVAL))
//! carrying the game's own bytes verbatim. Nothing is invented, and the game's
//! flow is never diverted, dropped, modified, or delayed.
//!
//! ## Redirect exemption
//!
//! The installed `nat OUTPUT` rule matches on destination IP:port only, so
//! without an exemption this probe would itself be redirected to the local
//! listener and never reach the server. The probe socket therefore carries
//! [`PROBE_MARK`] via `SO_MARK`, and the interceptor installs a rule that
//! returns marked packets *before* the redirect, so only the probe escapes.
//! Setting `SO_MARK` needs `CAP_NET_ADMIN` (which the interceptor already needs
//! for nft/iptables); when it fails, probing is disabled rather than sending a
//! copy that would be captured. [`ShadowProbe::local_port`] and the
//! interceptor's drop guard remain as defence in depth.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::os::fd::AsRawFd;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;

/// How long the reply reader waits for a datagram before re-checking `running`.
/// The real pairing deadline is the tracker's `SHADOW_DIRECT_TIMEOUT`; this only
/// bounds shutdown latency.
const PROBE_POLL_TIMEOUT_MS: libc::c_int = 500;

/// Socket mark (`SO_MARK`) that exempts the probe from the interceptor's
/// destination-only `nat OUTPUT` redirect. `0x4c53` is ASCII "LS". The
/// interceptor emits `meta mark 0x4c53 return` (nft) or
/// `-m mark --mark 0x4c53 -j RETURN` (iptables) ahead of the redirect rule, so
/// only the probe escapes to the real server.
pub(crate) const PROBE_MARK: u32 = 0x4c53;

/// Set `SO_MARK` on `fd`. Needs `CAP_NET_ADMIN`; `EPERM` here means the process
/// cannot mark sockets, so the probe must be disabled rather than sent, since an
/// unmarked probe would be captured by the interceptor's own redirect.
fn set_socket_mark(fd: libc::c_int, mark: u32) -> std::io::Result<()> {
    let value = mark;
    // SAFETY: `value` is a live `u32` and the length is exactly its size, so
    // `setsockopt` reads only initialised bytes for the duration of the call.
    let rc = unsafe {
        libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_MARK,
            std::ptr::addr_of!(value).cast::<libc::c_void>(),
            std::mem::size_of::<u32>() as libc::socklen_t,
        )
    };
    if rc == 0 {
        Ok(())
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Read back `SO_MARK` from `fd`. Used by tests to prove the mark is actually
/// present on the socket the probe will send from.
#[cfg(test)]
fn read_socket_mark(fd: libc::c_int) -> std::io::Result<u32> {
    let mut value: u32 = 0;
    let mut len = std::mem::size_of::<u32>() as libc::socklen_t;
    // SAFETY: `value` and `len` are valid out-pointers for `len` bytes and the
    // kernel only writes within them.
    let rc = unsafe {
        libc::getsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_MARK,
            std::ptr::addr_of_mut!(value).cast::<libc::c_void>(),
            &mut len,
        )
    };
    if rc == 0 {
        Ok(value)
    } else {
        Err(std::io::Error::last_os_error())
    }
}

/// Dedicated UDP socket for shadow-direct probes, shared by all samples, plus
/// the server its last probe targeted.
pub(crate) struct ShadowProbe {
    socket: std::net::UdpSocket,
    marked: bool,
    server: AtomicU32,
}

impl ShadowProbe {
    /// Bind the probe socket on an ephemeral port, mark it with [`PROBE_MARK`]
    /// so the interceptor's redirect rule exempts it, and make it non-blocking
    /// so a probe send can never park the interceptor's hot path.
    pub(crate) fn bind() -> std::io::Result<Self> {
        let socket = std::net::UdpSocket::bind("0.0.0.0:0")?;
        socket.set_nonblocking(true)?;
        let marked = match set_socket_mark(socket.as_raw_fd(), PROBE_MARK) {
            Ok(()) => true,
            Err(e) => {
                tracing::debug!(
                    "shadow-direct probe cannot set SO_MARK ({e}); disabling direct sampling"
                );
                false
            }
        };
        Ok(Self {
            socket,
            marked,
            server: AtomicU32::new(0),
        })
    }

    /// Whether this probe can bypass the interceptor's redirect. Only a socket
    /// that accepted [`PROBE_MARK`] may be sent from; an unmarked probe would be
    /// captured by the interceptor's own rule.
    pub(crate) fn is_usable(&self) -> bool {
        self.marked
    }

    /// The local port the probe socket is bound to. The interceptor uses this to
    /// recognise its own probe when the nat rule redirects the copy back to the
    /// listener.
    pub(crate) fn local_port(&self) -> Option<u16> {
        self.socket.local_addr().ok().map(|addr| addr.port())
    }

    /// Send an extra copy of the game's own bytes to `server`, remembering it so
    /// only that server's reply is accepted.
    pub(crate) fn send_copy(&self, payload: &[u8], server: SocketAddrV4) -> std::io::Result<()> {
        self.server
            .store(u32::from(*server.ip()), Ordering::Release);
        self.socket.send_to(payload, server).map(|_| ())
    }

    /// Wait up to `timeout_ms` for one datagram, returning the source IP only
    /// when it matches the last probed server.
    fn poll_reply(
        &self,
        buf: &mut [u8],
        timeout_ms: libc::c_int,
    ) -> std::io::Result<Option<Ipv4Addr>> {
        let mut pollfd = libc::pollfd {
            fd: self.socket.as_raw_fd(),
            events: libc::POLLIN,
            revents: 0,
        };
        // SAFETY: `pollfd` is POD, the fd stays open for the whole call, and the
        // timeout is a plain integer. `poll` only reads/writes `pollfd`.
        let ready = unsafe { libc::poll(&mut pollfd, 1, timeout_ms) };
        if ready <= 0 {
            return Ok(None);
        }
        match self.socket.recv_from(buf) {
            Ok((_len, SocketAddr::V4(src))) => {
                let expected = self.server.load(Ordering::Acquire);
                if expected != 0 && u32::from(*src.ip()) == expected {
                    Ok(Some(*src.ip()))
                } else {
                    Ok(None)
                }
            }
            Ok((_len, SocketAddr::V6(_))) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => Ok(None),
            Err(e) => Err(e),
        }
    }

    /// Spawn the reply reader. It hands the server IP to `on_reply` for every
    /// matching reply and exits once `running` clears.
    pub(crate) fn spawn_reader<F>(self: &Arc<Self>, running: Arc<AtomicBool>, on_reply: F)
    where
        F: Fn(Ipv4Addr) + Send + 'static,
    {
        let probe = Arc::clone(self);
        std::thread::spawn(move || {
            let mut buf = vec![0u8; 65535];
            while running.load(Ordering::Relaxed) {
                match probe.poll_reply(&mut buf, PROBE_POLL_TIMEOUT_MS) {
                    Ok(Some(server)) => on_reply(server),
                    Ok(None) => {}
                    Err(e) => tracing::debug!("shadow-direct probe recv error: {e}"),
                }
            }
        });
    }
}

/// Tunnel the game's packet first, then, only when a sample was claimed, send an
/// extra direct copy through `probe`.
///
/// `tunnel` performs the proxy send. It runs first and unconditionally, and its
/// result is returned untouched; a missing or failing probe can never change
/// whether or how the game's packet is tunnelled.
pub(crate) async fn tunnel_then_probe<T, F>(
    tunnel: F,
    claimed: bool,
    probe: Option<&ShadowProbe>,
    payload: &[u8],
    server: SocketAddrV4,
) -> T
where
    F: std::future::Future<Output = T>,
{
    let tunnelled = tunnel.await;
    if claimed {
        if let Some(probe) = probe {
            if let Err(e) = probe.send_copy(payload, server) {
                tracing::debug!("shadow-direct probe send failed: {e}");
            }
        }
    }
    tunnelled
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::latency::{Clock, IcmpProber, LatencyTracker};
    use std::net::SocketAddr;
    use std::sync::atomic::AtomicUsize;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    /// Poll `cond` until it holds or `timeout` elapses.
    fn wait_for(mut cond: impl FnMut() -> bool, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if cond() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        cond()
    }

    fn v4(addr: SocketAddr) -> SocketAddrV4 {
        match addr {
            SocketAddr::V4(v4) => v4,
            SocketAddr::V6(_) => panic!("expected an IPv4 address"),
        }
    }

    #[test]
    fn probe_socket_carries_the_mark_or_probing_is_disabled() {
        // Given a bound probe,
        let probe = ShadowProbe::bind().expect("probe bind");
        let mark = read_socket_mark(probe.socket.as_raw_fd());

        // Then either the kernel accepted SO_MARK, the probe is usable, and the
        // socket really carries PROBE_MARK,
        if probe.is_usable() {
            assert_eq!(
                mark.expect("marked socket must expose its mark"),
                PROBE_MARK
            );
        } else {
            // or SO_MARK was denied (no CAP_NET_ADMIN): the probe must report
            // itself unusable so an unmarked, redirectable copy is never sent.
            if let Ok(value) = mark {
                assert_eq!(value, 0, "an unusable probe must not carry a mark");
            }
            assert!(
                probe.local_port().is_some(),
                "the socket is still bound even when probing is disabled"
            );
        }
    }

    #[test]
    fn set_socket_mark_reports_failure_instead_of_claiming_success() {
        // A bad descriptor must surface the OS error, never a silent success.
        assert!(set_socket_mark(-1, PROBE_MARK).is_err());
    }

    /// A local UDP echo server for probe pairing tests.
    struct Echo {
        addr: SocketAddrV4,
        received: Arc<AtomicUsize>,
        stop: Arc<AtomicBool>,
    }

    impl Echo {
        fn start() -> Self {
            let socket = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
            socket
                .set_read_timeout(Some(Duration::from_millis(50)))
                .unwrap();
            let addr = v4(socket.local_addr().unwrap());
            let received = Arc::new(AtomicUsize::new(0));
            let stop = Arc::new(AtomicBool::new(false));
            {
                let received = Arc::clone(&received);
                let stop = Arc::clone(&stop);
                std::thread::spawn(move || {
                    let mut buf = [0u8; 2048];
                    while !stop.load(Ordering::Relaxed) {
                        if let Ok((len, from)) = socket.recv_from(&mut buf) {
                            received.fetch_add(1, Ordering::Relaxed);
                            let _ = socket.send_to(&buf[..len], from);
                        }
                    }
                });
            }
            Self {
                addr,
                received,
                stop,
            }
        }

        fn count(&self) -> usize {
            self.received.load(Ordering::Relaxed)
        }
    }

    impl Drop for Echo {
        fn drop(&mut self) {
            self.stop.store(true, Ordering::Relaxed);
        }
    }

    struct NoopProber;

    impl IcmpProber for NoopProber {
        fn probe(&self, _target: Ipv4Addr, _seq: u16) -> Option<f32> {
            None
        }
    }

    struct ManualClock {
        base: Instant,
        offset_ms: std::sync::atomic::AtomicU64,
    }

    impl ManualClock {
        fn new() -> Self {
            Self {
                base: Instant::now(),
                offset_ms: std::sync::atomic::AtomicU64::new(0),
            }
        }

        fn advance(&self, duration: Duration) {
            self.offset_ms
                .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
        }
    }

    impl Clock for ManualClock {
        fn now(&self) -> Instant {
            self.base + Duration::from_millis(self.offset_ms.load(Ordering::Relaxed))
        }
    }

    #[test]
    fn send_copy_carries_the_game_bytes_and_pairs_the_reply() {
        // Given a local echo and a probe socket,
        let server = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let server_addr = v4(server.local_addr().unwrap());
        let probe = ShadowProbe::bind().unwrap();
        let payload = b"game-own-bytes".to_vec();

        // When the probe sends the game's own bytes,
        probe.send_copy(&payload, server_addr).unwrap();

        // Then the server receives exactly those bytes,
        let mut buf = [0u8; 2048];
        let (len, from) = server.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..len], payload.as_slice());

        // And the server's reply is paired back to it.
        server.send_to(b"reply", from).unwrap();
        assert_eq!(
            probe.poll_reply(&mut buf, 1000).unwrap(),
            Some(*server_addr.ip())
        );
    }

    #[test]
    fn poll_reply_ignores_a_reply_from_another_host() {
        // Given a probe whose last target is not 127.0.0.1,
        let probe = ShadowProbe::bind().unwrap();
        let port = probe.local_port().unwrap();
        let _ = probe.send_copy(b"x", SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 1), 9));

        // When a datagram arrives from 127.0.0.1,
        let injector = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        injector
            .send_to(b"spoofed-reply", ("127.0.0.1", port))
            .unwrap();

        // Then it is ignored rather than measured as the probed server's reply.
        let mut buf = [0u8; 2048];
        assert_eq!(probe.poll_reply(&mut buf, 1000).unwrap(), None);
    }

    #[test]
    fn reader_delivers_a_matching_reply_to_the_hook() {
        // Given a running reader over a local echo,
        let server = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let server_addr = v4(server.local_addr().unwrap());
        let probe = Arc::new(ShadowProbe::bind().unwrap());
        let running = Arc::new(AtomicBool::new(true));
        let (tx, rx) = mpsc::channel();
        probe.spawn_reader(Arc::clone(&running), move |ip| {
            let _ = tx.send(ip);
        });

        // When the probe sends and the server replies,
        probe.send_copy(b"game-own-bytes", server_addr).unwrap();
        let mut buf = [0u8; 2048];
        let (_len, from) = server.recv_from(&mut buf).unwrap();
        server.send_to(b"reply", from).unwrap();

        // Then the reader hands the server IP to the hook.
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(3)).unwrap(),
            *server_addr.ip()
        );
        running.store(false, Ordering::Relaxed);
    }

    #[tokio::test]
    async fn tunnel_runs_and_the_extra_copy_is_sent_when_a_sample_is_claimed() {
        // Given a claimed sample and a local echo,
        let server = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        server
            .set_read_timeout(Some(Duration::from_secs(2)))
            .unwrap();
        let server_addr = v4(server.local_addr().unwrap());
        let probe = ShadowProbe::bind().unwrap();
        let payload = b"game-own-bytes".to_vec();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_t = Arc::clone(&calls);

        // When the outbound path runs,
        let result = tunnel_then_probe(
            async move {
                calls_t.fetch_add(1, Ordering::Relaxed);
                7u8
            },
            true,
            Some(&probe),
            &payload,
            server_addr,
        )
        .await;

        // Then the game's packet still tunnelled and returned its value,
        assert_eq!(result, 7);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "the game's packet must still be tunnelled"
        );

        // And an extra copy of the game's own bytes went out.
        let mut buf = [0u8; 2048];
        let (len, _from) = server.recv_from(&mut buf).unwrap();
        assert_eq!(&buf[..len], payload.as_slice());
    }

    #[tokio::test]
    async fn tunnel_runs_when_no_probe_socket_is_available() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_t = Arc::clone(&calls);
        let result = tunnel_then_probe(
            async move {
                calls_t.fetch_add(1, Ordering::Relaxed);
                1u8
            },
            true,
            None,
            b"game-own-bytes",
            SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, 7), 28015),
        )
        .await;

        assert_eq!(result, 1);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "a missing probe must not stop the tunnel"
        );
    }

    #[tokio::test]
    async fn a_failing_probe_send_does_not_affect_tunnelling() {
        // A limited-broadcast destination needs SO_BROADCAST; without it the
        // send fails, which is exactly the probe-error path.
        let probe = ShadowProbe::bind().unwrap();
        let bad = SocketAddrV4::new(Ipv4Addr::BROADCAST, 9);
        assert!(
            probe.send_copy(b"x", bad).is_err(),
            "broadcast send must fail without SO_BROADCAST"
        );

        let calls = Arc::new(AtomicUsize::new(0));
        let calls_t = Arc::clone(&calls);
        let result = tunnel_then_probe(
            async move {
                calls_t.fetch_add(1, Ordering::Relaxed);
                2u8
            },
            true,
            Some(&probe),
            b"game-own-bytes",
            bad,
        )
        .await;

        assert_eq!(result, 2);
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "a probe send failure must not stop the tunnel"
        );
    }

    #[tokio::test]
    async fn no_probe_is_sent_when_the_gate_declines() {
        let server = std::net::UdpSocket::bind("127.0.0.1:0").unwrap();
        server
            .set_read_timeout(Some(Duration::from_millis(250)))
            .unwrap();
        let server_addr = v4(server.local_addr().unwrap());
        let probe = ShadowProbe::bind().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_t = Arc::clone(&calls);

        tunnel_then_probe(
            async move {
                calls_t.fetch_add(1, Ordering::Relaxed);
            },
            false,
            Some(&probe),
            b"game-own-bytes",
            server_addr,
        )
        .await;

        assert_eq!(calls.load(Ordering::Relaxed), 1);
        let mut buf = [0u8; 64];
        assert!(
            server.recv_from(&mut buf).is_err(),
            "a declined sample must send no probe"
        );
    }

    #[tokio::test]
    async fn shadow_gate_claims_one_copy_per_server_per_interval() {
        // Given a tracker with a manual clock and a local echo,
        let echo = Echo::start();
        let clock = Arc::new(ManualClock::new());
        let tracker = LatencyTracker::new(Arc::new(NoopProber), clock.clone());
        let probe = ShadowProbe::bind().unwrap();
        let payload = b"game-own-bytes".to_vec();
        let server = echo.addr;

        macro_rules! run_once {
            () => {{
                let claimed = tracker.record_shadow_outbound(*server.ip());
                tunnel_then_probe(async {}, claimed, Some(&probe), &payload, server).await;
            }};
        }

        // When: t=0 claims,
        run_once!();
        assert!(wait_for(|| echo.count() >= 1, Duration::from_secs(2)));

        // t=29s does not,
        clock.advance(Duration::from_secs(29));
        run_once!();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(echo.count(), 1, "still inside the sampling interval");

        // t=30s claims again,
        clock.advance(Duration::from_secs(1));
        run_once!();
        assert!(wait_for(|| echo.count() >= 2, Duration::from_secs(2)));

        // and the next interval behaves the same.
        clock.advance(Duration::from_secs(29));
        run_once!();
        std::thread::sleep(Duration::from_millis(100));
        assert_eq!(echo.count(), 2);
        clock.advance(Duration::from_secs(1));
        run_once!();
        assert!(wait_for(|| echo.count() >= 3, Duration::from_secs(2)));
    }
}
