//! Direct (ICMP) and relayed (tunnel) latency measurement.
//!
//! `direct_p50_ms` is the median ICMP echo RTT to the detected game server
//! over the un-relayed path. `relayed_p50_ms` is the median round trip of
//! actual tunnelled game traffic (client → relay → game server → relay →
//! client). Their difference is LightSpeed's core value metric, `saved_ms`.
//!
//! The direct prober is strictly opt-in: the process-wide tracker is installed
//! only when telemetry is enabled, and a given server is probed at most once
//! per [`DIRECT_BURST_INTERVAL`].

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::telemetry::TelemetryCollector;

/// Number of ICMP probes in one direct burst.
pub const DIRECT_PROBE_COUNT: usize = 3;
/// Timeout for each ICMP probe reply.
pub const DIRECT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Minimum interval between direct bursts for the same server.
pub const DIRECT_BURST_INTERVAL: Duration = Duration::from_secs(60);
/// Rolling window capacity for relayed RTT samples.
pub const RELAYED_RING_CAPACITY: usize = 256;

const ECHO_PAYLOAD: &[u8] = b"lightspeed-ping";

/// One ICMP echo prober, injectable so latency logic stays testable without a
/// network.
pub trait IcmpProber: Send + Sync {
    /// Send one echo request to `target` and return the round trip in ms, or
    /// `None` when no matching reply arrived before the timeout.
    fn probe(&self, target: Ipv4Addr, seq: u16) -> Option<f32>;
}

/// Monotonic clock, injectable so the rate limiter is testable.
pub trait Clock: Send + Sync {
    /// The current monotonic instant.
    fn now(&self) -> Instant;
}

/// Real monotonic clock backed by [`Instant::now`].
pub struct SystemClock;

impl Clock for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
}

/// Real ICMP echo prober: unprivileged datagram sockets on Linux and macOS,
/// raw sockets on Windows (the client already runs elevated for WinDivert).
pub struct SystemIcmpProber;

impl IcmpProber for SystemIcmpProber {
    fn probe(&self, target: Ipv4Addr, seq: u16) -> Option<f32> {
        probe_once(target, seq)
    }
}

#[cfg(windows)]
fn icmp_socket() -> std::io::Result<Socket> {
    Socket::new(Domain::IPV4, Type::RAW, Some(Protocol::ICMPV4))
}

#[cfg(not(windows))]
fn icmp_socket() -> std::io::Result<Socket> {
    Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::ICMPV4))
}

#[cfg(windows)]
fn icmp_ident(_socket: &Socket) -> u16 {
    (std::process::id() & 0xffff) as u16
}

#[cfg(not(windows))]
fn icmp_ident(socket: &Socket) -> u16 {
    socket
        .local_addr()
        .ok()
        .and_then(|addr| addr.as_socket_ipv4())
        .map(|addr| addr.port())
        .unwrap_or(0)
}

fn probe_once(target: Ipv4Addr, seq: u16) -> Option<f32> {
    let socket = icmp_socket().ok()?;
    socket.set_read_timeout(Some(DIRECT_PROBE_TIMEOUT)).ok()?;
    let bind: SockAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).into();
    socket.bind(&bind).ok()?;

    let ident = icmp_ident(&socket);
    let request = build_echo_request(ident, seq);
    let dest: SockAddr = SocketAddr::V4(SocketAddrV4::new(target, 0)).into();

    let sent_at = Instant::now();
    socket.send_to(&request, &dest).ok()?;

    let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 1500];
    for _ in 0..4 {
        let Ok((len, _)) = socket.recv_from(&mut buf) else {
            return None;
        };
        // SAFETY: `recv_from` initialized exactly the first `len` bytes of `buf`.
        let filled = unsafe { std::slice::from_raw_parts(buf.as_ptr().cast::<u8>(), len) };
        if parse_echo_reply(filled) == Some((ident, seq)) {
            return Some(sent_at.elapsed().as_secs_f32() * 1000.0);
        }
        if sent_at.elapsed() >= DIRECT_PROBE_TIMEOUT {
            return None;
        }
    }
    None
}

fn build_echo_request(ident: u16, seq: u16) -> [u8; 8 + ECHO_PAYLOAD.len()] {
    let mut packet = [0u8; 8 + ECHO_PAYLOAD.len()];
    packet[0] = 8;
    packet[4..6].copy_from_slice(&ident.to_be_bytes());
    packet[6..8].copy_from_slice(&seq.to_be_bytes());
    packet[8..].copy_from_slice(ECHO_PAYLOAD);
    let sum = checksum(&packet);
    packet[2..4].copy_from_slice(&sum.to_be_bytes());
    packet
}

fn parse_echo_reply(buf: &[u8]) -> Option<(u16, u16)> {
    let icmp = if buf.len() >= 20 && buf[0] >> 4 == 4 {
        let header_len = usize::from(buf[0] & 0x0f) * 4;
        if buf.len() < header_len + 8 {
            return None;
        }
        &buf[header_len..]
    } else {
        buf
    };
    if icmp.len() < 8 || icmp[0] != 0 {
        return None;
    }
    Some((
        u16::from_be_bytes([icmp[4], icmp[5]]),
        u16::from_be_bytes([icmp[6], icmp[7]]),
    ))
}

fn checksum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 2 <= data.len() {
        sum += u32::from(u16::from_be_bytes([data[i], data[i + 1]]));
        i += 2;
    }
    if i < data.len() {
        sum += u32::from(u16::from_be_bytes([data[i], 0]));
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

#[derive(Default)]
struct Inner {
    direct_p50_ms: Option<f32>,
    pending: HashMap<Ipv4Addr, Instant>,
    last_burst: HashMap<Ipv4Addr, Instant>,
    relayed: Vec<f32>,
}

/// Opt-in direct + relayed latency tracker.
pub struct LatencyTracker {
    prober: Arc<dyn IcmpProber>,
    clock: Arc<dyn Clock>,
    inner: StdMutex<Inner>,
}

impl LatencyTracker {
    /// Create a tracker over the injected prober and clock.
    pub fn new(prober: Arc<dyn IcmpProber>, clock: Arc<dyn Clock>) -> Self {
        Self {
            prober,
            clock,
            inner: StdMutex::new(Inner::default()),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Record T0 for an outbound tunnelled game packet to `server`, returning
    /// `true` when a rate-limited direct burst is due for that server.
    pub fn note_outbound(&self, server: Ipv4Addr) -> bool {
        let now = self.clock.now();
        let mut inner = self.lock();
        inner.pending.insert(server, now);
        match inner.last_burst.get(&server) {
            Some(last) if now.saturating_duration_since(*last) < DIRECT_BURST_INTERVAL => false,
            _ => {
                inner.last_burst.insert(server, now);
                true
            }
        }
    }

    /// Record a relayed response for `server`. Only responses with a matching
    /// pending outbound are measured, mirroring the proxy's single pending
    /// forward marker.
    pub fn record_inbound(&self, server: Ipv4Addr) {
        let now = self.clock.now();
        let mut inner = self.lock();
        let Some(sent_at) = inner.pending.remove(&server) else {
            return;
        };
        let rtt_ms = now.saturating_duration_since(sent_at).as_secs_f64() * 1000.0;
        if !rtt_ms.is_finite() || rtt_ms <= 0.0 {
            return;
        }
        if inner.relayed.len() >= RELAYED_RING_CAPACITY {
            inner.relayed.remove(0);
        }
        inner.relayed.push(rtt_ms as f32);
    }

    /// Claim the rate-limit slot and run one blocking direct burst.
    pub fn maybe_probe_direct(&self, server: Ipv4Addr) -> Option<f32> {
        if !self.note_outbound(server) {
            return None;
        }
        self.run_direct_burst(server)
    }

    /// Run one direct burst, store the median, and return it. `None` when no
    /// probe produced a reply.
    pub fn run_direct_burst(&self, server: Ipv4Addr) -> Option<f32> {
        let mut replies = Vec::with_capacity(DIRECT_PROBE_COUNT);
        for seq in 0..DIRECT_PROBE_COUNT {
            if let Some(rtt) = self.prober.probe(server, seq as u16) {
                if rtt.is_finite() && rtt >= 0.0 {
                    replies.push(rtt);
                }
            }
        }
        if replies.is_empty() {
            return None;
        }
        replies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = TelemetryCollector::percentile(&replies, 50.0);
        self.lock().direct_p50_ms = Some(median);
        Some(median)
    }

    /// Latest direct median, if a burst has produced one.
    pub fn direct_p50_ms(&self) -> Option<f32> {
        self.lock().direct_p50_ms
    }

    /// Median of the current relayed window, if any.
    pub fn relayed_p50_ms(&self) -> Option<f32> {
        let mut inner = self.lock();
        if inner.relayed.is_empty() {
            return None;
        }
        let mut sorted = std::mem::take(&mut inner.relayed);
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = TelemetryCollector::percentile(&sorted, 50.0);
        inner.relayed = sorted;
        Some(median)
    }

    /// `direct - relayed`, only when both medians exist.
    pub fn saved_ms(&self) -> Option<f32> {
        match (self.direct_p50_ms(), self.relayed_p50_ms()) {
            (Some(direct), Some(relayed)) => Some(direct - relayed),
            _ => None,
        }
    }

    /// Take the values for one telemetry report. The direct median persists
    /// (it refreshes at most once per [`DIRECT_BURST_INTERVAL`]); the relayed
    /// window drains so each report covers a fresh interval.
    pub fn take_report_values(&self) -> (Option<f32>, Option<f32>) {
        let direct = self.direct_p50_ms();
        let relayed = {
            let mut inner = self.lock();
            if inner.relayed.is_empty() {
                None
            } else {
                let mut sorted = std::mem::take(&mut inner.relayed);
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                Some(TelemetryCollector::percentile(&sorted, 50.0))
            }
        };
        (direct, relayed)
    }
}

impl Default for LatencyTracker {
    fn default() -> Self {
        Self::new(Arc::new(SystemIcmpProber), Arc::new(SystemClock))
    }
}

static GLOBAL: OnceLock<Arc<LatencyTracker>> = OnceLock::new();

/// Install the process-wide tracker. Never called unless telemetry is enabled,
/// which is what keeps ICMP probing opt-in.
pub fn install_global(tracker: Arc<LatencyTracker>) {
    let _ = GLOBAL.set(tracker);
}

/// The installed tracker, or `None` when telemetry is disabled.
pub fn global() -> Option<&'static Arc<LatencyTracker>> {
    GLOBAL.get()
}

/// T0 hook: call when an outbound game packet is tunnelled to the relay. Kicks
/// a rate-limited direct burst onto a blocking thread.
pub fn record_outbound(server: Ipv4Addr) {
    let Some(tracker) = global() else {
        return;
    };
    if !tracker.note_outbound(server) {
        return;
    }
    let tracker = Arc::clone(tracker);
    if let Ok(handle) = tokio::runtime::Handle::try_current() {
        handle.spawn_blocking(move || {
            tracker.run_direct_burst(server);
        });
    }
}

/// T1 hook: call when the first relayed response for `server` is decoded.
pub fn record_inbound(server: Ipv4Addr) {
    if let Some(tracker) = global() {
        tracker.record_inbound(server);
    }
}

/// Values for the next telemetry report; `(None, None)` when telemetry is off.
pub fn report_values() -> (Option<f32>, Option<f32>) {
    match global() {
        Some(tracker) => tracker.take_report_values(),
        None => (None, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    struct ScriptedProber {
        replies: StdMutex<Vec<Option<f32>>>,
        calls: AtomicUsize,
    }

    impl ScriptedProber {
        fn new(replies: Vec<Option<f32>>) -> Self {
            Self {
                replies: StdMutex::new(replies),
                calls: AtomicUsize::new(0),
            }
        }
    }

    impl IcmpProber for ScriptedProber {
        fn probe(&self, _target: Ipv4Addr, _seq: u16) -> Option<f32> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let mut replies = self.replies.lock().unwrap();
            if replies.is_empty() {
                None
            } else {
                replies.remove(0)
            }
        }
    }

    struct TestClock {
        base: Instant,
        offset_ms: AtomicU64,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                base: Instant::now(),
                offset_ms: AtomicU64::new(0),
            }
        }

        fn advance(&self, duration: Duration) {
            self.offset_ms
                .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> Instant {
            self.base + Duration::from_millis(self.offset_ms.load(Ordering::Relaxed))
        }
    }

    fn tracker(prober: Arc<dyn IcmpProber>, clock: Arc<dyn Clock>) -> LatencyTracker {
        LatencyTracker::new(prober, clock)
    }

    const SERVER: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 7);

    #[test]
    fn median_selects_the_middle_reply() {
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(30.0),
            Some(10.0),
            Some(20.0),
        ]));
        let t = tracker(prober, Arc::new(TestClock::new()));
        assert_eq!(t.run_direct_burst(SERVER), Some(20.0));
        assert_eq!(t.direct_p50_ms(), Some(20.0));
    }

    #[test]
    fn partial_replies_use_median_of_what_arrived() {
        let prober = Arc::new(ScriptedProber::new(vec![Some(10.0), None, Some(30.0)]));
        let t = tracker(prober, Arc::new(TestClock::new()));
        // percentile(50) of [10, 30] takes the upper-middle sample.
        assert_eq!(t.run_direct_burst(SERVER), Some(30.0));
    }

    #[test]
    fn no_replies_leave_direct_unset() {
        let prober = Arc::new(ScriptedProber::new(vec![None, None, None]));
        let t = tracker(prober, Arc::new(TestClock::new()));
        assert_eq!(t.run_direct_burst(SERVER), None);
        assert_eq!(t.direct_p50_ms(), None);
    }

    #[test]
    fn burst_is_rate_limited_per_server() {
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(20.0),
            Some(20.0),
            Some(20.0),
            Some(20.0),
            Some(20.0),
            Some(20.0),
        ]));
        let clock = Arc::new(TestClock::new());
        let t = tracker(prober.clone(), clock.clone());

        assert_eq!(t.maybe_probe_direct(SERVER), Some(20.0));
        assert_eq!(prober.calls.load(Ordering::Relaxed), DIRECT_PROBE_COUNT);

        clock.advance(Duration::from_secs(30));
        assert_eq!(t.maybe_probe_direct(SERVER), None, "inside the interval");
        assert_eq!(
            prober.calls.load(Ordering::Relaxed),
            DIRECT_PROBE_COUNT,
            "no probes run while rate limited"
        );

        clock.advance(Duration::from_secs(31));
        assert_eq!(t.maybe_probe_direct(SERVER), Some(20.0));
        assert_eq!(prober.calls.load(Ordering::Relaxed), DIRECT_PROBE_COUNT * 2);
    }

    #[test]
    fn saved_ms_is_positive_when_relay_is_faster() {
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(50.0),
            Some(50.0),
            Some(50.0),
        ]));
        let clock = Arc::new(TestClock::new());
        let t = tracker(prober, clock.clone());

        t.run_direct_burst(SERVER);
        t.note_outbound(SERVER);
        clock.advance(Duration::from_millis(20));
        t.record_inbound(SERVER);

        assert_eq!(t.relayed_p50_ms(), Some(20.0));
        assert_eq!(t.saved_ms(), Some(30.0));
    }

    #[test]
    fn inbound_without_pending_outbound_is_ignored() {
        let t = tracker(
            Arc::new(ScriptedProber::new(vec![])),
            Arc::new(TestClock::new()),
        );
        t.record_inbound(SERVER);
        assert_eq!(t.relayed_p50_ms(), None);
    }

    #[test]
    fn saved_ms_requires_both_medians() {
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(50.0),
            Some(50.0),
            Some(50.0),
        ]));
        let t = tracker(prober, Arc::new(TestClock::new()));
        t.run_direct_burst(SERVER);
        assert_eq!(t.saved_ms(), None, "no relayed samples yet");
    }

    #[test]
    fn report_values_drain_relayed_but_keep_direct() {
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(50.0),
            Some(50.0),
            Some(50.0),
        ]));
        let clock = Arc::new(TestClock::new());
        let t = tracker(prober, clock.clone());
        t.run_direct_burst(SERVER);
        t.note_outbound(SERVER);
        clock.advance(Duration::from_millis(35));
        t.record_inbound(SERVER);

        assert_eq!(t.take_report_values(), (Some(50.0), Some(35.0)));
        assert_eq!(t.take_report_values(), (Some(50.0), None));
    }

    #[test]
    fn checksum_of_zero_buffer_is_ones_complement() {
        assert_eq!(checksum(&[0u8; 8]), 0xffff);
    }

    #[test]
    fn parse_echo_reply_handles_bare_and_ip_framed() {
        let mut reply = build_echo_request(0x1234, 7);
        reply[0] = 0;
        assert_eq!(parse_echo_reply(&reply), Some((0x1234, 7)));

        let mut framed = vec![0u8; 20];
        framed[0] = 0x45;
        framed.extend_from_slice(&reply);
        assert_eq!(parse_echo_reply(&framed), Some((0x1234, 7)));
    }
}
