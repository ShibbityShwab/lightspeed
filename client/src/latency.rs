//! Direct (ICMP) and relayed (tunnel) latency measurement.
//!
//! `direct_p50_ms` is the median ICMP echo RTT to the detected game server
//! over the un-relayed path. `relayed_p50_ms` is the median round trip of
//! actual tunnelled game traffic (client → relay → game server → relay →
//! client). Their difference is LightSpeed's core value metric, `saved_ms`.
//!
//! `direct_app_p50_ms` is the median round trip of a small sample of the game's
//! own packets on the direct, un-relayed path, timed against the server's
//! reply. It is game-packet-to-game-packet with `relayed_p50_ms`, so the two are
//! directly comparable, unlike the ICMP-based `direct_p50_ms`, which stays as
//! the estimate.
//!
//! ## Measurement vs. reporting
//!
//! Local measurement is **independent of telemetry reporting**. Installing the
//! process-wide tracker ([`install_measurement`], or `telemetry::install` on
//! the reporting path) declares that a feature needing measurement (the
//! interceptor or tunnel) is running; from then on the hooks below record. The
//! telemetry enable flag only governs the **reporting** side: while it is off,
//! measurements stay entirely local and no report is built or sent (enforced in
//! `telemetry::TelemetryCollector::flush`).
//!
//! This split is deliberate. The direct, relayed, and shadow-direct samples
//! feed local routing and a do-no-harm bypass gate, so they must not be
//! silently disabled by opting out of reporting. Nothing measured here reaches
//! the relay until telemetry is enabled.
//!
//! ### Shadow-direct is an explicit, documented exception
//!
//! The shadow-direct sampler re-injects a copy of the game's own packet onto
//! the direct path. That is extra traffic on the game-server connection even
//! when telemetry is off, so it is an explicit decision rather than a side
//! effect: it runs whenever the interceptor is running, is rate-limited to one
//! packet per server per [`SHADOW_SAMPLE_INTERVAL`], and, because the hook is
//! only invoked from the interceptor, never fires when the interceptor is
//! stopped. Its samples are not reported unless telemetry is on.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};

use crate::telemetry::TelemetryCollector;

/// Number of ICMP probes in one direct burst.
pub const DIRECT_PROBE_COUNT: usize = 5;
/// Minimum plausible direct RTT in milliseconds. A faster reply was answered
/// locally rather than over the public internet and must never be stored.
pub const MIN_PLAUSIBLE_DIRECT_MS: f32 = 1.0;
/// Minimum plausible replies in a burst before a median is meaningful.
pub const MIN_DIRECT_SAMPLES: usize = 3;
/// How long a stored direct median remains pairable with a relayed window.
pub const DIRECT_MEDIAN_TTL: Duration = Duration::from_secs(300);
/// Timeout for each ICMP probe reply.
pub const DIRECT_PROBE_TIMEOUT: Duration = Duration::from_secs(2);
/// Minimum interval between direct bursts for the same server.
pub const DIRECT_BURST_INTERVAL: Duration = Duration::from_secs(60);
/// Rolling window capacity for relayed RTT samples.
pub const RELAYED_RING_CAPACITY: usize = 256;
/// How long a shadow-direct sample waits for the server's reply before it is
/// discarded, so a stale timestamp cannot pair with a much later packet.
pub const SHADOW_DIRECT_TIMEOUT: Duration = Duration::from_secs(5);
/// Minimum interval between shadow-direct samples for the same server. At most
/// one of the game's own packets per server per interval is timed directly.
pub const SHADOW_SAMPLE_INTERVAL: Duration = Duration::from_secs(30);
/// How long a stored shadow-direct median remains reportable.
pub const SHADOW_DIRECT_TTL: Duration = Duration::from_secs(300);
/// Rolling window capacity for shadow-direct application RTT samples.
pub const SHADOW_RING_CAPACITY: usize = 256;

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

/// The socket operations an echo prober needs, abstracted so tests can script
/// replies (including their source addresses) without a network.
trait ProbeSocket {
    /// Send one echo request to `target`.
    fn send(&self, request: &[u8], target: Ipv4Addr) -> std::io::Result<()>;
    /// Receive one datagram, returning its length and source address.
    fn recv(&self, buf: &mut [std::mem::MaybeUninit<u8>]) -> std::io::Result<(usize, SocketAddr)>;
}

/// Real datagram socket backing [`SystemIcmpProber`].
struct UdpProbeSocket(Socket);

impl ProbeSocket for UdpProbeSocket {
    fn send(&self, request: &[u8], target: Ipv4Addr) -> std::io::Result<()> {
        let dest: SockAddr = SocketAddr::V4(SocketAddrV4::new(target, 0)).into();
        self.0.send_to(request, &dest).map(|_| ())
    }

    fn recv(&self, buf: &mut [std::mem::MaybeUninit<u8>]) -> std::io::Result<(usize, SocketAddr)> {
        let (len, source) = self.0.recv_from(buf)?;
        let source = source
            .as_socket()
            .ok_or_else(|| std::io::Error::other("ICMP source was not an IP address"))?;
        Ok((len, source))
    }
}

fn probe_once(target: Ipv4Addr, seq: u16) -> Option<f32> {
    let socket = icmp_socket().ok()?;
    socket.set_read_timeout(Some(DIRECT_PROBE_TIMEOUT)).ok()?;
    let bind: SockAddr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0)).into();
    socket.bind(&bind).ok()?;

    let ident = icmp_ident(&socket);
    probe_with(&UdpProbeSocket(socket), target, ident, seq)
}

/// Run one echo exchange over an abstract socket. Returns the round trip in ms
/// when a matching reply arrives before the timeout.
fn probe_with<S: ProbeSocket>(socket: &S, target: Ipv4Addr, ident: u16, seq: u16) -> Option<f32> {
    let request = build_echo_request(ident, seq);
    let sent_at = Instant::now();
    socket.send(&request, target).ok()?;

    let mut buf = [std::mem::MaybeUninit::<u8>::uninit(); 1500];
    for _ in 0..4 {
        let Ok((len, source)) = socket.recv(&mut buf) else {
            return None;
        };
        if !reply_source_matches(source, target) {
            if sent_at.elapsed() >= DIRECT_PROBE_TIMEOUT {
                return None;
            }
            continue;
        }
        // SAFETY: `recv` initialized exactly the first `len` bytes of `buf`.
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

/// A reply is only ours when it came from the host we probed. Accepting a reply
/// from any host lets a locally-answered flow masquerade as a WAN round trip.
fn reply_source_matches(source: SocketAddr, target: Ipv4Addr) -> bool {
    matches!(source, SocketAddr::V4(v4) if *v4.ip() == target)
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

/// A direct-burst median together with the moment it was measured.
struct DirectSample {
    median_ms: f32,
    measured_at: Instant,
}

#[derive(Default)]
struct Inner {
    direct: HashMap<Ipv4Addr, DirectSample>,
    pending: HashMap<Ipv4Addr, Instant>,
    last_burst: HashMap<Ipv4Addr, Instant>,
    relayed: Vec<f32>,
    relayed_server: Option<Ipv4Addr>,
    shadow_pending: HashMap<Ipv4Addr, Instant>,
    shadow_last: HashMap<Ipv4Addr, Instant>,
    shadow: Vec<f32>,
    shadow_server: Option<Ipv4Addr>,
    shadow_measured_at: Option<Instant>,
    shadow_attempts: u32,
    shadow_replies: u32,
}

impl Inner {
    /// The direct median for `server`, only while it is within
    /// [`DIRECT_MEDIAN_TTL`] of measurement.
    fn fresh_direct(&self, server: Ipv4Addr, now: Instant) -> Option<f32> {
        self.direct
            .get(&server)
            .filter(|sample| now.saturating_duration_since(sample.measured_at) < DIRECT_MEDIAN_TTL)
            .map(|sample| sample.median_ms)
    }

    /// The most recently measured direct median across all servers.
    fn latest_direct(&self) -> Option<f32> {
        self.direct
            .values()
            .max_by_key(|sample| sample.measured_at)
            .map(|sample| sample.median_ms)
    }

    /// Median of the current single-server relayed window, if any.
    fn relayed_median(&self) -> Option<f32> {
        if self.relayed.is_empty() {
            return None;
        }
        let mut sorted = self.relayed.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(TelemetryCollector::percentile(&sorted, 50.0))
    }

    /// Median of the current single-server shadow-direct window, if any.
    fn shadow_median(&self) -> Option<f32> {
        if self.shadow.is_empty() {
            return None;
        }
        let mut sorted = self.shadow.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(TelemetryCollector::percentile(&sorted, 50.0))
    }
}

/// Mean of consecutive absolute RTT deltas in arrival order.
fn ring_jitter(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    let deltas: f32 = samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
    deltas / (samples.len() - 1) as f32
}

/// Direct, relayed, and shadow-direct latency tracker.
///
/// Local measurement is independent of telemetry reporting; see the module
/// docs. The tracker stores samples only; nothing here sends them anywhere.
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
        if inner.relayed_server != Some(server) {
            inner.relayed.clear();
            inner.relayed_server = Some(server);
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

    /// Run one direct burst, store the median, and return it. Replies faster
    /// than [`MIN_PLAUSIBLE_DIRECT_MS`] are discarded, and fewer than
    /// [`MIN_DIRECT_SAMPLES`] plausible replies yield `None` rather than a
    /// fabricated p50.
    pub fn run_direct_burst(&self, server: Ipv4Addr) -> Option<f32> {
        let mut replies = Vec::with_capacity(DIRECT_PROBE_COUNT);
        for seq in 0..DIRECT_PROBE_COUNT {
            if let Some(rtt) = self.prober.probe(server, seq as u16) {
                if rtt.is_finite() && rtt >= MIN_PLAUSIBLE_DIRECT_MS {
                    replies.push(rtt);
                }
            }
        }
        if replies.len() < MIN_DIRECT_SAMPLES {
            return None;
        }
        replies.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        let median = TelemetryCollector::percentile(&replies, 50.0);
        let measured_at = self.clock.now();
        self.lock().direct.insert(
            server,
            DirectSample {
                median_ms: median,
                measured_at,
            },
        );
        Some(median)
    }

    /// Latest direct median across servers, if a burst has produced one.
    pub fn direct_p50_ms(&self) -> Option<f32> {
        self.lock().latest_direct()
    }

    /// Claim the shadow-direct sampling slot for `server`. Returns `true` when
    /// a sample is due: the caller must re-inject the game's own packet
    /// unchanged onto the direct path. At most one sample per server per
    /// [`SHADOW_SAMPLE_INTERVAL`]. Uses its own map, so it never touches the
    /// relayed `pending` entry.
    pub fn record_shadow_outbound(&self, server: Ipv4Addr) -> bool {
        let now = self.clock.now();
        let mut inner = self.lock();
        if let Some(sent_at) = inner.shadow_pending.get(&server).copied() {
            if now.saturating_duration_since(sent_at) >= SHADOW_DIRECT_TIMEOUT {
                inner.shadow_pending.remove(&server);
            }
        }
        match inner.shadow_last.get(&server) {
            Some(last) if now.saturating_duration_since(*last) < SHADOW_SAMPLE_INTERVAL => false,
            _ => {
                inner.shadow_last.insert(server, now);
                inner.shadow_pending.insert(server, now);
                inner.shadow_attempts = inner.shadow_attempts.saturating_add(1);
                true
            }
        }
    }

    /// Record the server's reply to a shadow-direct sample. Pairs only with a
    /// pending sample inside [`SHADOW_DIRECT_TIMEOUT`]; anything older is
    /// discarded rather than measured.
    pub fn record_shadow_inbound(&self, server: Ipv4Addr) {
        let now = self.clock.now();
        let mut inner = self.lock();
        let Some(sent_at) = inner.shadow_pending.remove(&server) else {
            return;
        };
        if now.saturating_duration_since(sent_at) >= SHADOW_DIRECT_TIMEOUT {
            return;
        }
        let rtt_ms = now.saturating_duration_since(sent_at).as_secs_f64() * 1000.0;
        if !rtt_ms.is_finite() || rtt_ms <= 0.0 {
            return;
        }
        if inner.shadow_server != Some(server) {
            inner.shadow.clear();
            inner.shadow_server = Some(server);
            inner.shadow_attempts = 0;
            inner.shadow_replies = 0;
        }
        if inner.shadow.len() >= SHADOW_RING_CAPACITY {
            inner.shadow.remove(0);
        }
        inner.shadow.push(rtt_ms as f32);
        inner.shadow_measured_at = Some(now);
        inner.shadow_replies = inner.shadow_replies.saturating_add(1);
    }

    /// Median direct application RTT, only while the shadow window was
    /// measured within [`SHADOW_DIRECT_TTL`].
    pub fn shadow_direct_p50_ms(&self) -> Option<f32> {
        let now = self.clock.now();
        let inner = self.lock();
        let measured_at = inner.shadow_measured_at?;
        if now.saturating_duration_since(measured_at) >= SHADOW_DIRECT_TTL {
            return None;
        }
        inner.shadow_median()
    }

    /// Median of the current relayed window, if any.
    pub fn relayed_p50_ms(&self) -> Option<f32> {
        self.lock().relayed_median()
    }

    /// Jitter of the shadow-direct window, or `None` before two samples.
    pub fn shadow_direct_jitter_ms(&self) -> Option<f32> {
        let inner = self.lock();
        (inner.shadow.len() >= 2).then(|| ring_jitter(&inner.shadow))
    }

    /// Jitter of the relayed window, or `None` before two samples.
    pub fn relayed_jitter_ms(&self) -> Option<f32> {
        let inner = self.lock();
        (inner.relayed.len() >= 2).then(|| ring_jitter(&inner.relayed))
    }

    /// Direct application loss ratio from shadow attempts vs replies, or
    /// `None` until at least one sample was attempted.
    pub fn shadow_direct_loss_ratio(&self) -> Option<f32> {
        let inner = self.lock();
        if inner.shadow_attempts == 0 {
            return None;
        }
        let replies = inner.shadow_replies.min(inner.shadow_attempts);
        Some(1.0 - replies as f32 / inner.shadow_attempts as f32)
    }

    /// Number of shadow-direct samples in the current window.
    pub fn shadow_direct_samples(&self) -> u32 {
        self.lock().shadow.len() as u32
    }

    /// Number of relayed samples in the current window.
    pub fn relayed_samples(&self) -> u32 {
        self.lock().relayed.len() as u32
    }

    /// `direct - relayed`, only when a fresh direct median and the relayed
    /// window are from the same server. Mixing a direct median from one server
    /// or moment with a relayed window from another is not a saving.
    pub fn saved_ms(&self) -> Option<f32> {
        let now = self.clock.now();
        let inner = self.lock();
        let server = inner.relayed_server?;
        let direct = inner.fresh_direct(server, now)?;
        let relayed = inner.relayed_median()?;
        Some(direct - relayed)
    }

    /// Current report values without draining the relayed window.
    pub fn peek_report_values(&self) -> (Option<f32>, Option<f32>) {
        let now = self.clock.now();
        let inner = self.lock();
        let direct = match inner.relayed_server {
            Some(server) => inner.fresh_direct(server, now),
            None => None,
        };
        (direct, inner.relayed_median())
    }

    /// Drain the relayed window after a report carrying it was accepted.
    pub fn commit_report_values(&self) {
        self.lock().relayed.clear();
    }

    /// Take the values for one telemetry report. The direct median is reported
    /// only when it is fresh and belongs to the same server as the relayed
    /// window; the relayed window drains so each report covers a fresh interval.
    pub fn take_report_values(&self) -> (Option<f32>, Option<f32>) {
        let values = self.peek_report_values();
        self.commit_report_values();
        values
    }
}

impl Default for LatencyTracker {
    fn default() -> Self {
        Self::new(Arc::new(SystemIcmpProber), Arc::new(SystemClock))
    }
}

static GLOBAL: OnceLock<Arc<LatencyTracker>> = OnceLock::new();

/// Process-wide gate for whether local latency measurement is running.
///
/// This is separate from [`crate::telemetry::is_enabled`], which governs
/// **reporting**. Measurement feeds local routing and a do-no-harm bypass gate,
/// so it runs whenever a feature that needs it (the interceptor/tunnel) is
/// active, regardless of whether telemetry is opted in.
static MEASURING: AtomicBool = AtomicBool::new(false);

/// Whether local latency measurement is currently active.
pub fn is_measuring() -> bool {
    MEASURING.load(Ordering::Relaxed)
}

/// Enable or disable local latency measurement. Never touches telemetry.
pub fn set_measuring(measuring: bool) {
    MEASURING.store(measuring, Ordering::Relaxed);
}

/// Install the process-wide tracker. Idempotent: the first call wins.
///
/// Installing a tracker means a measurement feature is active, so this also
/// enables measurement. It does **not** enable telemetry reporting.
pub fn install_global(tracker: Arc<LatencyTracker>) {
    let _ = GLOBAL.set(tracker);
    set_measuring(true);
}

/// Declare that a feature needs local measurement and install the process-wide
/// tracker if it is not already present. Idempotent, and independent of
/// telemetry: call this from every tunnel/interceptor start path.
pub fn install_measurement() {
    if global().is_none() {
        install_global(Arc::new(LatencyTracker::default()));
    }
    set_measuring(true);
}

/// The installed tracker, or `None` when measurement was never installed.
pub fn global() -> Option<&'static Arc<LatencyTracker>> {
    GLOBAL.get()
}

/// T0 hook: call when an outbound game packet is tunnelled to the relay. Kicks
/// a rate-limited direct burst onto a blocking thread.
///
/// Gated on local measurement ([`is_measuring`]), not on telemetry: a muted
/// client still measures locally. The burst's result is not reported anywhere.
pub fn record_outbound(server: Ipv4Addr) {
    crate::route::destination::note_outbound(server);
    if !is_measuring() {
        return;
    }
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
/// Gated on local measurement, not on telemetry, so a muted client still
/// records the relayed RTT it needs for local decisions.
pub fn record_inbound(server: Ipv4Addr) {
    crate::route::destination::note_inbound(server);
    if !is_measuring() {
        return;
    }
    if let Some(tracker) = global() {
        tracker.record_inbound(server);
    }
}

/// Shadow-direct hook: ask whether a sample is due for `server`. The caller
/// re-injects the game's own packet unchanged on the direct path (never a
/// synthetic packet).
///
/// **Explicit decision:** this is local measurement, so it runs whenever the
/// interceptor is running, even with `--no-telemetry`. The re-injected packet
/// goes only to the game server, unchanged, at most once per server per
/// [`SHADOW_SAMPLE_INTERVAL`], and never to the relay; the sample is not
/// reported unless telemetry is on. Because the hook is only called from the
/// interceptor, it does not fire while the interceptor is stopped. Returns
/// `false` when measurement is not active.
pub fn record_shadow_outbound(server: Ipv4Addr) -> bool {
    if !is_measuring() {
        return false;
    }
    match global() {
        Some(tracker) => tracker.record_shadow_outbound(server),
        None => false,
    }
}

/// Shadow-direct hook: record the server's reply to a sampled packet. Gated on
/// local measurement, not on telemetry.
pub fn record_shadow_inbound(server: Ipv4Addr) {
    if !is_measuring() {
        return;
    }
    if let Some(tracker) = global() {
        tracker.record_shadow_inbound(server);
    }
}

/// Median direct application RTT for the next telemetry report; `None` when no
/// fresh shadow sample exists.
///
/// Reads local measurement only. Whether a report is allowed to use it is
/// decided by the reporting gate in `telemetry::TelemetryCollector::flush`.
pub fn shadow_direct_p50_ms() -> Option<f32> {
    global().and_then(|tracker| tracker.shadow_direct_p50_ms())
}

/// Median relayed application RTT, or `None` when the window is empty.
pub fn relayed_p50_ms() -> Option<f32> {
    global().and_then(|tracker| tracker.relayed_p50_ms())
}

/// Jitter of the direct application path, if a window exists.
pub fn shadow_direct_jitter_ms() -> Option<f32> {
    global().and_then(|tracker| tracker.shadow_direct_jitter_ms())
}

/// Jitter of the relayed application path, if a window exists.
pub fn relayed_jitter_ms() -> Option<f32> {
    global().and_then(|tracker| tracker.relayed_jitter_ms())
}

/// Shadow-direct loss ratio (1 - replies/attempts) for the bypass gate.
pub fn shadow_direct_loss_ratio() -> Option<f32> {
    global().and_then(|tracker| tracker.shadow_direct_loss_ratio())
}

/// Shadow-direct sample count of the current window.
pub fn shadow_direct_samples() -> u32 {
    global().map_or(0, |tracker| tracker.shadow_direct_samples())
}

/// Relayed sample count of the current window.
pub fn relayed_samples() -> u32 {
    global().map_or(0, |tracker| tracker.relayed_samples())
}

/// Values for the next telemetry report; `(None, None)` when no tracker is
/// installed. Reads local measurement; this is not the reporting gate.
pub fn report_values() -> (Option<f32>, Option<f32>) {
    match global() {
        Some(tracker) => tracker.take_report_values(),
        None => (None, None),
    }
}

/// Values for the next telemetry report without draining the relayed window;
/// `(None, None)` when no tracker is installed.
pub fn peek_report_values() -> (Option<f32>, Option<f32>) {
    match global() {
        Some(tracker) => tracker.peek_report_values(),
        None => (None, None),
    }
}

/// Commit values returned by [`peek_report_values`] after a successful send.
pub fn commit_report_values() {
    if let Some(tracker) = global() {
        tracker.commit_report_values();
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

    /// Scripted [`ProbeSocket`] that returns canned datagrams (with their
    /// source addresses) instead of touching the network.
    struct ScriptedSocket {
        replies: StdMutex<Vec<(SocketAddr, Vec<u8>)>>,
        sends: AtomicUsize,
    }

    impl ProbeSocket for ScriptedSocket {
        fn send(&self, _request: &[u8], _target: Ipv4Addr) -> std::io::Result<()> {
            self.sends.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn recv(
            &self,
            buf: &mut [std::mem::MaybeUninit<u8>],
        ) -> std::io::Result<(usize, SocketAddr)> {
            let mut replies = self.replies.lock().unwrap();
            if replies.is_empty() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::TimedOut,
                    "no scripted reply",
                ));
            }
            let (source, bytes) = replies.remove(0);
            for (slot, byte) in buf.iter_mut().zip(bytes.iter()) {
                slot.write(*byte);
            }
            Ok((bytes.len(), source))
        }
    }

    fn echo_reply_bytes(ident: u16, seq: u16) -> Vec<u8> {
        let mut reply = build_echo_request(ident, seq);
        reply[0] = 0;
        reply.to_vec()
    }

    fn tracker(prober: Arc<dyn IcmpProber>, clock: Arc<dyn Clock>) -> LatencyTracker {
        LatencyTracker::new(prober, clock)
    }

    const SERVER: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 7);
    const OTHER_SERVER: Ipv4Addr = Ipv4Addr::new(198, 51, 100, 9);

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
    fn no_replies_leave_direct_unset() {
        let prober = Arc::new(ScriptedProber::new(vec![None, None, None]));
        let t = tracker(prober, Arc::new(TestClock::new()));
        assert_eq!(t.run_direct_burst(SERVER), None);
        assert_eq!(t.direct_p50_ms(), None);
    }

    #[test]
    fn below_minimum_replies_leave_direct_unset() {
        let prober = Arc::new(ScriptedProber::new(vec![Some(10.0), None, Some(30.0)]));
        let t = tracker(prober, Arc::new(TestClock::new()));
        assert_eq!(
            t.run_direct_burst(SERVER),
            None,
            "a median of fewer than three replies is not a p50"
        );
    }

    #[test]
    fn all_implausible_burst_returns_none() {
        let prober = Arc::new(ScriptedProber::new(vec![Some(0.1), Some(0.5), Some(0.9)]));
        let t = tracker(prober, Arc::new(TestClock::new()));
        assert_eq!(t.run_direct_burst(SERVER), None);
        assert_eq!(t.direct_p50_ms(), None);
    }

    #[test]
    fn filtered_burst_uses_remaining_median() {
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(0.5),
            Some(10.0),
            Some(0.2),
            Some(50.0),
            Some(30.0),
        ]));
        let t = tracker(prober, Arc::new(TestClock::new()));
        assert_eq!(
            t.run_direct_burst(SERVER),
            Some(30.0),
            "sub-millisecond replies are discarded, the rest form the median"
        );
    }

    #[test]
    fn burst_is_rate_limited_per_server() {
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(20.0);
            DIRECT_PROBE_COUNT * 2
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
    fn stale_direct_is_not_reported() {
        let prober = Arc::new(ScriptedProber::new(vec![Some(50.0); MIN_DIRECT_SAMPLES]));
        let clock = Arc::new(TestClock::new());
        let t = tracker(prober, clock.clone());
        t.run_direct_burst(SERVER);
        t.note_outbound(SERVER);
        clock.advance(Duration::from_millis(20));
        t.record_inbound(SERVER);

        clock.advance(Duration::from_secs(301));
        let (direct, relayed) = t.take_report_values();
        assert_eq!(direct, None, "a stale direct median must not be reported");
        assert_eq!(relayed, Some(20.0));
    }

    #[test]
    fn direct_from_other_server_is_not_reported() {
        let prober = Arc::new(ScriptedProber::new(vec![Some(50.0); MIN_DIRECT_SAMPLES]));
        let clock = Arc::new(TestClock::new());
        let t = tracker(prober, clock.clone());
        t.run_direct_burst(SERVER);
        t.note_outbound(OTHER_SERVER);
        clock.advance(Duration::from_millis(20));
        t.record_inbound(OTHER_SERVER);

        let (direct, relayed) = t.take_report_values();
        assert_eq!(
            direct, None,
            "direct from a different server must not be reported"
        );
        assert_eq!(relayed, Some(20.0));
    }

    #[test]
    fn saved_ms_rejects_other_server_direct() {
        let prober = Arc::new(ScriptedProber::new(vec![Some(50.0); MIN_DIRECT_SAMPLES]));
        let clock = Arc::new(TestClock::new());
        let t = tracker(prober, clock.clone());
        t.run_direct_burst(SERVER);
        t.note_outbound(OTHER_SERVER);
        clock.advance(Duration::from_millis(20));
        t.record_inbound(OTHER_SERVER);

        assert_eq!(
            t.saved_ms(),
            None,
            "direct from a different server must not yield saved"
        );
    }

    #[test]
    fn saved_ms_rejects_stale_direct() {
        let prober = Arc::new(ScriptedProber::new(vec![Some(50.0); MIN_DIRECT_SAMPLES]));
        let clock = Arc::new(TestClock::new());
        let t = tracker(prober, clock.clone());
        t.run_direct_burst(SERVER);
        t.note_outbound(SERVER);
        clock.advance(Duration::from_millis(20));
        t.record_inbound(SERVER);

        clock.advance(Duration::from_secs(301));
        assert_eq!(
            t.saved_ms(),
            None,
            "a stale direct median must not yield saved"
        );
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
    fn reply_from_other_host_is_ignored() {
        let target = Ipv4Addr::new(198, 51, 100, 10);
        let (ident, seq) = (0x1234, 7);
        let other = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(198, 51, 100, 11), 0));
        let socket = ScriptedSocket {
            replies: StdMutex::new(vec![(other, echo_reply_bytes(ident, seq)); 4]),
            sends: AtomicUsize::new(0),
        };

        assert_eq!(
            probe_with(&socket, target, ident, seq),
            None,
            "a reply from a different host must be ignored"
        );
        assert_eq!(socket.sends.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn reply_from_target_host_is_accepted() {
        let target = Ipv4Addr::new(198, 51, 100, 10);
        let (ident, seq) = (0x1234, 7);
        let source = SocketAddr::V4(SocketAddrV4::new(target, 0));
        let socket = ScriptedSocket {
            replies: StdMutex::new(vec![(source, echo_reply_bytes(ident, seq))]),
            sends: AtomicUsize::new(0),
        };

        assert!(
            probe_with(&socket, target, ident, seq).is_some(),
            "a reply from the probe target must be accepted"
        );
    }

    // ── Shadow-direct application RTT sampler ───────────────────────────────

    #[test]
    fn shadow_sample_is_rate_limited_to_one_per_server_per_interval() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        assert!(t.record_shadow_outbound(SERVER), "the first sample is due");
        assert!(
            !t.record_shadow_outbound(SERVER),
            "a second sample inside the interval must be refused"
        );

        clock.advance(Duration::from_secs(29));
        assert!(
            !t.record_shadow_outbound(SERVER),
            "still inside the interval"
        );

        clock.advance(Duration::from_secs(1));
        assert!(t.record_shadow_outbound(SERVER), "due again at 30s");

        assert!(
            t.record_shadow_outbound(OTHER_SERVER),
            "each server has its own sampling slot"
        );
    }

    #[test]
    fn shadow_sample_does_not_clobber_relayed_pending() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        t.note_outbound(SERVER);
        assert!(t.record_shadow_outbound(SERVER));

        clock.advance(Duration::from_millis(25));
        t.record_inbound(SERVER);
        assert_eq!(t.relayed_p50_ms(), Some(25.0));

        clock.advance(Duration::from_millis(5));
        t.record_shadow_inbound(SERVER);
        assert_eq!(t.shadow_direct_p50_ms(), Some(30.0));
        assert_eq!(
            t.relayed_p50_ms(),
            Some(25.0),
            "the shadow sample must not disturb the relayed window"
        );
    }

    #[test]
    fn shadow_median_selects_the_middle_sample() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        for rtt_ms in [10u64, 30, 20] {
            assert!(t.record_shadow_outbound(SERVER));
            clock.advance(Duration::from_millis(rtt_ms));
            t.record_shadow_inbound(SERVER);
            clock.advance(Duration::from_secs(30));
        }

        assert_eq!(t.shadow_direct_p50_ms(), Some(20.0));
    }

    #[test]
    fn shadow_reply_after_timeout_is_not_paired() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        assert!(t.record_shadow_outbound(SERVER));
        clock.advance(SHADOW_DIRECT_TIMEOUT + Duration::from_millis(1));
        t.record_shadow_inbound(SERVER);

        assert_eq!(
            t.shadow_direct_p50_ms(),
            None,
            "a reply past the timeout must not pair with the sample"
        );
    }

    #[test]
    fn shadow_stale_pending_is_cleared_for_the_next_sample() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        assert!(t.record_shadow_outbound(SERVER));
        clock.advance(SHADOW_SAMPLE_INTERVAL + SHADOW_DIRECT_TIMEOUT + Duration::from_millis(1));

        assert!(
            t.record_shadow_outbound(SERVER),
            "the stale pending must not block the next sample"
        );
        clock.advance(Duration::from_millis(40));
        t.record_shadow_inbound(SERVER);

        assert_eq!(t.shadow_direct_p50_ms(), Some(40.0));
    }

    #[test]
    fn shadow_median_expires_after_ttl() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        assert!(t.record_shadow_outbound(SERVER));
        clock.advance(Duration::from_millis(15));
        t.record_shadow_inbound(SERVER);
        assert_eq!(t.shadow_direct_p50_ms(), Some(15.0));

        clock.advance(SHADOW_DIRECT_TTL);
        assert_eq!(
            t.shadow_direct_p50_ms(),
            None,
            "a shadow median older than the TTL must not be reported"
        );
    }

    #[test]
    fn shadow_inbound_without_pending_is_ignored() {
        let t = tracker(
            Arc::new(ScriptedProber::new(vec![])),
            Arc::new(TestClock::new()),
        );
        t.record_shadow_inbound(SERVER);
        assert_eq!(t.shadow_direct_p50_ms(), None);
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
