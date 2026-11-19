//! Relay-measured relay-to-server leg: an unprivileged ICMP echo prober.
//!
//! Design: `docs/design/relay-server-leg.md`. The relay is one endpoint of the
//! relay→game-server leg, so it can measure that leg with ICMP, whose sequence
//! number attributes the reply exactly. The blocking unknown was resolved by a
//! canary on `relay-syd-1`: unprivileged ICMP (`SOCK_DGRAM`/`IPPROTO_ICMP`)
//! works with `net.ipv4.ping_group_range = 61184 65519`, so no `CAP_NET_RAW` is
//! needed. This module never opens a raw socket.
//!
//! ## The gates are the deliverable
//!
//! A relay that probes addresses a client names is a scanning and amplification
//! surface, so every gate below is load-bearing rather than defensive polish:
//!
//! * Only an address a **live session has already forwarded at least
//!   [`FORWARD_THRESHOLD`] packets to** is ever registered. Targets come from
//!   the data path and nowhere else.
//! * Only a **public unicast** address is probed; private, loopback, link-local,
//!   CGNAT, benchmarking and reserved ranges are refused ([`is_probeable`]).
//! * One burst per server per [`PROBE_INTERVAL`] (10 s), and a cached result may
//!   not be probed again inside [`CACHE_TTL`] (30 s) even when a hundred
//!   sessions name the same server. A cached measurement older than
//!   [`CACHE_TTL`] is never reported.
//! * A global budget bounds bursts per second and concurrent bursts, whether or
//!   not the per-server cache would allow more ([`ServerLegConfig`]).
//! * The prober's own egress is charged through the relay's egress budget guard
//!   before every echo is sent ([`BudgetGuard::try_charge_probe`]); when the
//!   guard denies a charge the rest of that burst is skipped and counted, never
//!   sent. Probe traffic therefore counts against the same ceiling as relayed
//!   traffic instead of being invisible to it.
//! * `EPERM`/`EACCES`/`EAGAIN`/timeouts degrade to "unavailable" - the prober
//!   returns `None`, never panics and never fabricates a figure.
//!
//! ## Shape
//!
//! One burst is [`BURST_ECHOES`] echoes; the burst median feeds a per-server
//! history of the last [`HISTORY_CAPACITY`] medians, and jitter is the mean
//! absolute delta between consecutive medians (the same reducer the client's
//! direct series uses). A burst with fewer than [`MIN_ECHO_REPLIES`] plausible
//! replies produces no measurement: the server blocks ICMP, and the honest
//! answer is "unavailable".

use std::collections::{HashMap, VecDeque};
use std::net::Ipv4Addr;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

use lightspeed_protocol::control::ControlMessage;

use crate::budget::BudgetGuard;
use crate::metrics::ProxyMetrics;

/// Packets a live session must have forwarded to its game server before that
/// address may be probed. Below this the relay has not committed enough traffic
/// to the destination to justify a probe.
pub const FORWARD_THRESHOLD: u64 = 20;

/// Egress charged to the budget for one probe echo.
///
/// The on-wire cost is one ICMP echo request (20 B IPv4 header, 8 B ICMP
/// header, plus payload), under 64 B. The estimate deliberately rounds up: the
/// budget is a ceiling, and under-counting measurement traffic is the failure
/// this charge exists to prevent.
pub const ECHO_EGRESS_BYTES: u64 = 64;

/// Echo requests per burst.
pub const BURST_ECHOES: usize = 3;

/// Plausible replies a burst needs before its median is meaningful.
pub const MIN_ECHO_REPLIES: usize = 2;

/// Burst cadence per active server.
pub const PROBE_INTERVAL: Duration = Duration::from_secs(10);

/// How long a measurement stays reportable. A cached measurement older than
/// this is "unavailable" until the next burst lands.
pub const CACHE_TTL: Duration = Duration::from_secs(30);

/// Burst medians kept per server for the jitter figure.
pub const HISTORY_CAPACITY: usize = 16;

/// Minimum interval between reports for one control session.
pub const REPORT_INTERVAL: Duration = Duration::from_secs(5);

/// How long a registered interest stays live without a refreshed forward.
pub const INTEREST_TTL: Duration = Duration::from_secs(60);

/// Per-echo timeout.
pub const ECHO_TIMEOUT: Duration = Duration::from_secs(2);

/// Timer resolution of the background prober thread.
pub const PROBE_TICK: Duration = Duration::from_secs(1);

/// A round trip faster than this was answered locally, not over the public
/// internet. The same physical floor the client applies to a direct ICMP ping.
pub const MIN_PLAUSIBLE_US: u32 = 1_000;

/// Largest believable passive relay→server app round trip (5 s).
///
/// A passive sample is the already-recorded gap between forwarding a game packet
/// and reading the first response on that session socket. A gap above this is a
/// stalled socket or a response to something else, not a game-server round trip
/// anyone plays on; the client applies the same ceiling to its own samples.
pub const MAX_PASSIVE_US: u32 = 5_000_000;

/// Passive app-RTT samples kept per live session.
///
/// One sample per forward/response exchange is frequent (every answered game
/// packet), so a short window is enough to reject a single outlier by median and
/// still describe the recent path. It matches the ICMP `HISTORY_CAPACITY`.
pub const PASSIVE_WINDOW: usize = 16;

/// Upper bound on distinct control-session interests tracked at once, so a
/// client population cannot grow the map without bound.
const MAX_INTERESTS: usize = 4_096;

/// Monotonic clock, injectable so cache and budget behaviour is testable.
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

/// One injectable echo prober.
///
/// The real implementation is the unprivileged datagram socket below; tests
/// script replies so the cache, budget and jitter maths need no network.
pub trait EchoProber: Send + Sync {
    /// Send one echo request to `target` and return the round trip in
    /// microseconds, or `None` when no matching reply arrived before
    /// [`ECHO_TIMEOUT`] (or ICMP is not permitted in this environment).
    fn echo(&self, target: Ipv4Addr, seq: u16) -> Option<u32>;
}

/// Real unprivileged ICMP echo prober.
///
/// On Linux this is `SOCK_DGRAM`/`IPPROTO_ICMP`, which needs only
/// `net.ipv4.ping_group_range` to cover the relay's GID - never `CAP_NET_RAW`.
/// Every socket error (including `EPERM`/`EACCES` when the sysctl does not cover
/// the service account) returns `None`, which callers surface as
/// "unavailable". The relay fleet is Linux; other hosts compile to a prober
/// that reports unavailable, so the module still builds and is testable there.
pub struct SystemEchoProber;

impl EchoProber for SystemEchoProber {
    fn echo(&self, target: Ipv4Addr, seq: u16) -> Option<u32> {
        system_echo(target, seq)
    }
}

#[cfg(target_os = "linux")]
fn system_echo(target: Ipv4Addr, seq: u16) -> Option<u32> {
    linux::echo_once(target, seq)
}

#[cfg(not(target_os = "linux"))]
fn system_echo(_target: Ipv4Addr, _seq: u16) -> Option<u32> {
    None
}

#[cfg(target_os = "linux")]
mod linux {
    //! The real unprivileged ICMP socket. `libc` is already a proxy dependency
    //! on Linux, so this adds no crate.

    use std::net::Ipv4Addr;
    use std::time::Instant;

    use super::{build_echo_request, is_echo_reply, ECHO_TIMEOUT, MIN_PLAUSIBLE_US};

    /// Send one echo request on a fresh unprivileged ICMP datagram socket.
    ///
    /// A socket per echo keeps the prober stateless: bursts are 3 echoes every
    /// 10 s per server, so the setup cost is irrelevant next to the round trip.
    /// Returns `None` for any socket error, including the `EPERM`/`EACCES` a
    /// relay whose GID is outside `ping_group_range` sees - never panics.
    pub(super) fn echo_once(target: Ipv4Addr, seq: u16) -> Option<u32> {
        unsafe {
            let fd = libc::socket(libc::AF_INET, libc::SOCK_DGRAM, libc::IPPROTO_ICMP);
            if fd < 0 {
                return None;
            }
            let result = echo_once_fd(fd, target, seq);
            libc::close(fd);
            result
        }
    }

    unsafe fn echo_once_fd(fd: libc::c_int, target: Ipv4Addr, seq: u16) -> Option<u32> {
        // Bound every receive; a missing reply is a timeout, not a hang.
        let timeout = libc::timeval {
            tv_sec: ECHO_TIMEOUT.as_secs() as libc::time_t,
            tv_usec: ECHO_TIMEOUT.subsec_micros() as libc::suseconds_t,
        };
        if libc::setsockopt(
            fd,
            libc::SOL_SOCKET,
            libc::SO_RCVTIMEO,
            (&timeout as *const libc::timeval).cast(),
            std::mem::size_of::<libc::timeval>() as libc::socklen_t,
        ) != 0
        {
            return None;
        }

        let addr_len = std::mem::size_of::<libc::sockaddr_in>() as libc::socklen_t;
        let mut local: libc::sockaddr_in = std::mem::zeroed();
        local.sin_family = libc::AF_INET as libc::sa_family_t;
        local.sin_port = 0;
        local.sin_addr.s_addr = u32::from_ne_bytes(Ipv4Addr::UNSPECIFIED.octets());
        if libc::bind(fd, (&local as *const libc::sockaddr_in).cast(), addr_len) != 0 {
            return None;
        }

        // The kernel rewrites the ICMP identifier with the socket's local port
        // for ping sockets; read it so the request is well-formed for a raw
        // socket too, though a ping socket replies with the rewritten id.
        let ident = {
            let mut bound: libc::sockaddr_in = std::mem::zeroed();
            let mut bound_len = addr_len;
            if libc::getsockname(
                fd,
                (&mut bound as *mut libc::sockaddr_in).cast(),
                &mut bound_len,
            ) != 0
            {
                0
            } else {
                u16::from_be(bound.sin_port)
            }
        };

        let packet = build_echo_request(ident, seq);
        let mut dest: libc::sockaddr_in = std::mem::zeroed();
        dest.sin_family = libc::AF_INET as libc::sa_family_t;
        dest.sin_port = 0;
        dest.sin_addr.s_addr = u32::from_ne_bytes(target.octets());
        let sent_at = Instant::now();
        let sent = libc::sendto(
            fd,
            packet.as_ptr().cast(),
            packet.len(),
            0,
            (&dest as *const libc::sockaddr_in).cast(),
            addr_len,
        );
        if sent < 0 {
            // EPERM/EACCES (no ping_group_range coverage), EAGAIN, ENETUNREACH,
            // EHOSTUNREACH: all "unavailable", never a panic.
            return None;
        }

        let mut buf = [0u8; 1_500];
        // A few receives: unrelated ICMP can arrive on the socket, and a stale
        // reply to an earlier echo must not be paired with this one.
        for _ in 0..4 {
            let mut from: libc::sockaddr_in = std::mem::zeroed();
            let mut from_len = addr_len;
            let len = libc::recvfrom(
                fd,
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
                (&mut from as *mut libc::sockaddr_in).cast(),
                &mut from_len,
            );
            if len < 0 {
                // SO_RCVTIMEO expiry (EAGAIN/EWOULDBLOCK) or any other error.
                return None;
            }
            if from.sin_addr.s_addr.to_ne_bytes() != target.octets() {
                continue;
            }
            let len = len as usize;
            if is_echo_reply(&buf[..len], seq) {
                let elapsed = sent_at.elapsed().as_micros();
                let elapsed = elapsed.min(u128::from(u32::MAX)) as u32;
                return (elapsed >= MIN_PLAUSIBLE_US).then_some(elapsed);
            }
            // Overall deadline: `SO_RCVTIMEO` bounds each receive, but a stream
            // of unmatched replies must not hold the prober thread for a whole
            // burst. Past the echo timeout there is no measurement.
            if sent_at.elapsed() >= ECHO_TIMEOUT {
                return None;
            }
        }
        None
    }
}

/// Echo payload, kept small so the probe cannot be mistaken for a data packet.
///
/// The packet format below is used by the Linux socket and by unit tests on
/// every host; on a non-Linux build (where the socket reports unavailable) it
/// is deliberately kept compiled and exercised rather than deleted.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
const ECHO_PAYLOAD: &[u8] = b"lightspeed-server-leg";

/// Build one ICMP echo request (type 8) with the given identifier and sequence.
///
/// The checksum is filled in here for a raw socket; on an unprivileged
/// `SOCK_DGRAM`/`IPPROTO_ICMP` socket the kernel rewrites the identifier to the
/// socket's local port and recomputes the checksum, so a request built by this
/// function is valid in both modes. Kept outside the Linux-only socket module so
/// its wire format is unit-tested on every host.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn build_echo_request(ident: u16, seq: u16) -> [u8; 8 + ECHO_PAYLOAD.len()] {
    let mut packet = [0u8; 8 + ECHO_PAYLOAD.len()];
    packet[0] = 8; // ICMP echo request
    packet[4..6].copy_from_slice(&ident.to_be_bytes());
    packet[6..8].copy_from_slice(&seq.to_be_bytes());
    packet[8..].copy_from_slice(ECHO_PAYLOAD);
    let sum = checksum(&packet);
    packet[2..4].copy_from_slice(&sum.to_be_bytes());
    packet
}

/// Whether `buf` is an echo reply carrying `seq`.
///
/// A datagram ICMP socket receives the ICMP message without the IPv4 header; the
/// optional-header branch keeps the parser honest if a reply ever arrives with
/// one (and is what a raw-socket fallback would produce).
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn is_echo_reply(buf: &[u8], seq: u16) -> bool {
    let icmp = if buf.len() >= 20 && buf[0] >> 4 == 4 {
        let header_len = usize::from(buf[0] & 0x0f) * 4;
        if buf.len() < header_len + 8 {
            return false;
        }
        &buf[header_len..]
    } else {
        buf
    };
    icmp.len() >= 8 && icmp[0] == 0 && u16::from_be_bytes([icmp[6], icmp[7]]) == seq
}

/// Internet checksum (RFC 1071), one's complement of the 16-bit ones-complement
/// sum. Also used by the reply parser's tests to prove a reply built from a
/// request has its checksum recomputed correctly.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
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

/// Tunables for the prober. Defaults bound the fleet's added egress: one burst
/// of 3 echoes per 10 s per active server, at most two bursts a second, never
/// more than three in flight at once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerLegConfig {
    /// Master switch. Default on; an operator sets `[server_leg] enabled = false`
    /// or `LIGHTSPEED_SERVER_LEG_DISABLED=1` to turn probing off for this relay.
    pub enabled: bool,
    /// Global cap on bursts started per second (one burst = [`BURST_ECHOES`]).
    pub max_bursts_per_second: u32,
    /// Global cap on concurrent bursts.
    pub max_concurrent: usize,
    /// Burst cadence per active server.
    pub probe_interval: Duration,
    /// How long a cached measurement stays reportable.
    pub cache_ttl: Duration,
}

impl Default for ServerLegConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_bursts_per_second: 2,
            max_concurrent: 3,
            probe_interval: PROBE_INTERVAL,
            cache_ttl: CACHE_TTL,
        }
    }
}

/// One server's cached measurement.
#[derive(Debug, Clone, Copy)]
struct Measurement {
    /// Median of the latest burst, microseconds.
    median_us: u32,
    /// When the latest burst completed.
    measured_at: Instant,
}

/// What one probe tick did, for tests and tracing.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProbeTick {
    /// Bursts started this tick.
    pub bursts: u64,
    /// Echo requests actually sent across those bursts.
    pub probes: u64,
    /// Bursts that produced no usable measurement (ICMP blocked or timed out).
    pub unavailable: u64,
    /// Active servers left unprobed because a cap applied.
    pub skipped: u64,
    /// Bursts cut short because the egress budget refused the next echo.
    pub denied: u64,
}

/// How one echo burst ended.
#[derive(Debug, Clone, Copy, Default)]
struct Burst {
    /// The burst median, when enough plausible replies came back.
    measurement: Option<Measurement>,
    /// Echo requests sent in this burst.
    probes: u64,
    /// Whether the egress budget stopped the burst before it finished.
    denied: bool,
}

/// One live data-plane session's interest in its game server's leg.
#[derive(Debug, Clone, Copy)]
struct Interest {
    /// Client IP the report is for (diagnostics and pruning only; routing is by
    /// the control-plane session token).
    client_ip: Ipv4Addr,
    /// The game server this session forwards to.
    server: Ipv4Addr,
    /// Last forward that refreshed this interest.
    last_seen: Instant,
    /// When this session was last sent a report, for [`REPORT_INTERVAL`].
    last_report: Option<Instant>,
}

/// One session's passive relay→server app-RTT window.
///
/// The samples are the gaps between a forward the data path already timestamped
/// (`pending_forward_us`) and the first response read on that session's socket.
/// Recording one adds no packet and takes no extra measurement: it is a
/// timestamp difference the relay already computes. The window only exists for a
/// token with a live [`Interest`], and its `server` must still match that
/// interest, so a passive figure can never be reported for a server the session
/// has not forwarded to.
#[derive(Debug, Clone)]
struct PassiveWindow {
    /// The game server these samples were measured against.
    server: Ipv4Addr,
    /// Accepted samples, oldest first.
    samples: VecDeque<u32>,
    /// When the newest sample was recorded (the cache-age input).
    updated_at: Instant,
    /// When this session was last sent a passive report.
    last_report: Option<Instant>,
}

struct Inner {
    /// Active interests keyed by data-plane session token.
    interests: HashMap<u32, Interest>,
    /// Latest measurement per game server.
    measurements: HashMap<Ipv4Addr, Measurement>,
    /// Recent burst medians per game server, newest last (jitter input).
    history: HashMap<Ipv4Addr, VecDeque<u32>>,
    /// When each server was last attempted, successful or not, so a blocked
    /// server is retried on the cadence instead of on every tick.
    last_attempt: HashMap<Ipv4Addr, Instant>,
    /// Start of the current one-second global-budget window.
    budget_start: Instant,
    /// Bursts started in the current budget window.
    budget_used: u32,
    /// Bursts started and not yet finished (this implementation probes
    /// sequentially, so it never exceeds one, but the cap is enforced).
    in_flight: usize,
    /// Lifetime counters, mirrored into [`ProxyMetrics`] when one is attached.
    bursts: u64,
    probes: u64,
    unavailable: u64,
    skipped: u64,
    budget_denied: u64,
    /// Passive app-RTT windows keyed by data-plane session token. A window is
    /// only created for a token that already has an [`Interest`], and only while
    /// it names that interest's server.
    passive: HashMap<u32, PassiveWindow>,
    /// Lifetime accepted passive samples (diagnostics and tests).
    passive_samples: u64,
}

/// The relay-wide server-leg prober: per-server cache, global budget, and the
/// interest map the data path and the control plane share.
pub struct ServerLegRegistry {
    config: ServerLegConfig,
    clock: Arc<dyn Clock>,
    prober: Arc<dyn EchoProber>,
    metrics: Option<Arc<ProxyMetrics>>,
    /// Optional egress budget guard. When attached, every echo is charged
    /// through it before it is sent, so probe traffic counts against the same
    /// ceiling as relayed traffic. `None` (the default) leaves probes unmetered
    /// but still counted.
    budget: Option<Arc<BudgetGuard>>,
    inner: Mutex<Inner>,
}

impl ServerLegRegistry {
    /// Build a registry over the injected prober and clock.
    pub fn new(
        config: ServerLegConfig,
        clock: Arc<dyn Clock>,
        prober: Arc<dyn EchoProber>,
    ) -> Self {
        let budget_start = clock.now();
        Self {
            config,
            clock,
            prober,
            metrics: None,
            budget: None,
            inner: Mutex::new(Inner {
                interests: HashMap::new(),
                measurements: HashMap::new(),
                history: HashMap::new(),
                last_attempt: HashMap::new(),
                budget_start,
                budget_used: 0,
                in_flight: 0,
                bursts: 0,
                probes: 0,
                unavailable: 0,
                skipped: 0,
                budget_denied: 0,
                passive: HashMap::new(),
                passive_samples: 0,
            }),
        }
    }

    /// Attach the process metrics collector.
    pub fn with_metrics(mut self, metrics: Arc<ProxyMetrics>) -> Self {
        self.metrics = Some(metrics);
        self
    }

    /// Attach the relay's egress budget guard.
    ///
    /// Probe egress is then charged through the guard before each echo, so the
    /// relay's own measurement traffic is metered against the same ceiling as
    /// the data plane. Omitted by default (probes are still counted, just not
    /// charged).
    pub fn with_budget(mut self, budget: Arc<BudgetGuard>) -> Self {
        self.budget = Some(budget);
        self
    }

    /// Whether probing is enabled for this relay.
    pub fn enabled(&self) -> bool {
        self.config.enabled
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Register or refresh a session's interest in `server`.
    ///
    /// Called by the data path once a session has forwarded at least
    /// [`FORWARD_THRESHOLD`] packets. `forwarded` is the session's own count so
    /// the threshold is enforced here too, not only at the call site: no
    /// address reaches the prober before that many packets have been relayed to
    /// it. A non-public address is refused and counted.
    pub fn note_forward(&self, token: u32, client_ip: Ipv4Addr, server: Ipv4Addr, forwarded: u64) {
        if !self.config.enabled {
            self.count_skip();
            return;
        }
        if forwarded < FORWARD_THRESHOLD {
            return;
        }
        if !is_probeable(server) {
            self.count_skip();
            return;
        }
        let now = self.clock.now();
        let mut inner = self.lock();
        if !inner.interests.contains_key(&token) && inner.interests.len() >= MAX_INTERESTS {
            self.record_skip_inner(&mut inner);
            return;
        }
        inner
            .interests
            .entry(token)
            .and_modify(|interest| {
                interest.client_ip = client_ip;
                interest.server = server;
                interest.last_seen = now;
            })
            .or_insert(Interest {
                client_ip,
                server,
                last_seen: now,
                last_report: None,
            });
    }

    /// Record one passive relay→server app-RTT sample for `token`/`server`.
    ///
    /// The caller supplies the already-computed gap between the forward time the
    /// data path recorded and the first response read on that session socket, so
    /// this adds no packet and no new measurement. A sample is accepted only
    /// when all of the following hold:
    ///
    /// * probing is enabled for this relay;
    /// * the gap is inside `[`[`MIN_PLAUSIBLE_US`]`, `[`MAX_PASSIVE_US`]`]` - a
    ///   zero or absurd difference is not a measurement;
    /// * the token has a live interest **in this exact server** (an interest is
    ///   only created by forwarding), so a passive figure can never be reported
    ///   for a server the session has not forwarded to;
    /// * the per-session window exists (bounded, oldest evicted first).
    ///
    /// Each accepted sample is counted in `server_leg_passive_samples`; rejected
    /// gaps are not, so the series reads as reportable samples rather than
    /// observed responses.
    pub fn record_passive(&self, token: u32, server: Ipv4Addr, rtt_us: u32) {
        if !self.config.enabled {
            return;
        }
        if !(MIN_PLAUSIBLE_US..=MAX_PASSIVE_US).contains(&rtt_us) {
            return;
        }
        let now = self.clock.now();
        {
            let mut inner = self.lock();
            let Some(interest_server) = inner.interests.get(&token).map(|interest| interest.server)
            else {
                return;
            };
            if interest_server != server {
                return;
            }
            let window = inner.passive.entry(token).or_insert_with(|| PassiveWindow {
                server,
                samples: VecDeque::new(),
                updated_at: now,
                last_report: None,
            });
            if window.server != server {
                // The session's interest moved to another game server: the old
                // window described a different pair and must never be mixed with
                // the new one. Reset rather than carrying samples across.
                window.server = server;
                window.samples.clear();
                window.last_report = None;
            }
            if window.samples.len() >= PASSIVE_WINDOW {
                window.samples.pop_front();
            }
            window.samples.push_back(rtt_us);
            window.updated_at = now;
            inner.passive_samples += 1;
        }
        if let Some(metrics) = &self.metrics {
            metrics.record_server_leg_passive_sample();
        }
    }

    /// Run one probe tick: prune dead interests, then start due bursts within
    /// the global budget and concurrency caps.
    ///
    /// Probes run synchronously on the calling thread; the production caller is
    /// the dedicated prober thread, so a slow/blocked reply costs a probe
    /// timeout and never the async runtime.
    pub fn probe_due(&self) -> ProbeTick {
        let mut tick = ProbeTick::default();
        if !self.config.enabled {
            return tick;
        }
        let now = self.clock.now();
        let due = {
            let mut inner = self.lock();
            inner.prune(now);
            inner.due_servers(now, &self.config)
        };

        // Both caps bound one tick: the per-second budget and the concurrency
        // allowance (this implementation probes sequentially, so its actual
        // concurrency is one; the cap still bounds how many bursts a single
        // sweep may start, which is the point of a concurrency limit here).
        let max_started = self
            .config
            .max_bursts_per_second
            .min(self.config.max_concurrent as u32);
        let mut started = 0u32;
        for server in due {
            let proceed = {
                let mut inner = self.lock();
                inner.refill_budget(now);
                let within_budget = inner.budget_used < self.config.max_bursts_per_second;
                if within_budget && started < max_started {
                    inner.budget_used += 1;
                    inner.in_flight += 1;
                    inner.last_attempt.insert(server, now);
                    true
                } else {
                    inner.skipped += 1;
                    tick.skipped += 1;
                    false
                }
            };
            if !proceed {
                continue;
            }
            started += 1;
            let burst = self.probe_burst(server);
            tick.bursts += 1;
            tick.probes += burst.probes;
            if burst.denied {
                // A budget-denied burst is a probe opportunity the egress guard
                // skipped; it is counted as both a denial and a skip.
                tick.denied += 1;
                tick.skipped += 1;
            }
            let mut inner = self.lock();
            inner.in_flight = inner.in_flight.saturating_sub(1);
            inner.bursts += 1;
            inner.probes += burst.probes;
            if burst.denied {
                inner.budget_denied += 1;
                inner.skipped += 1;
            }
            match burst.measurement {
                Some(measurement) => {
                    inner.measurements.insert(server, measurement);
                    let history = inner.history.entry(server).or_default();
                    if history.len() >= HISTORY_CAPACITY {
                        history.pop_front();
                    }
                    history.push_back(measurement.median_us);
                }
                // A budget-denied burst sends too little to say ICMP is
                // unavailable; it is counted as a denial and a skip instead.
                None if !burst.denied => {
                    inner.unavailable += 1;
                    tick.unavailable += 1;
                }
                None => {}
            }
        }

        let (bursts, probes, unavailable, skipped, budget_denied) = {
            let inner = self.lock();
            (
                inner.bursts,
                inner.probes,
                inner.unavailable,
                inner.skipped,
                inner.budget_denied,
            )
        };
        if let Some(metrics) = &self.metrics {
            metrics.set_server_leg_counters(bursts, unavailable, skipped, probes, budget_denied);
        }
        tick
    }

    /// Run one echo burst and reduce it to a measurement, or `None` when the
    /// server answered too few echoes.
    ///
    /// Each echo is charged to the egress budget before it is sent. A refusal
    /// ends the burst immediately (no further echo is sent) and is reported in
    /// [`Burst::denied`]; a burst that still collected enough replies before
    /// the refusal keeps its measurement.
    fn probe_burst(&self, server: Ipv4Addr) -> Burst {
        let mut burst = Burst::default();
        let mut replies = Vec::with_capacity(BURST_ECHOES);
        for seq in 0..BURST_ECHOES {
            if let Some(budget) = &self.budget {
                let data_used = self
                    .metrics
                    .as_ref()
                    .map_or(0, |metrics| metrics.bytes_relayed.load(Ordering::Relaxed));
                if !budget.try_charge_probe(
                    data_used,
                    ECHO_EGRESS_BYTES,
                    super::budget::current_month_key(),
                ) {
                    burst.denied = true;
                    break;
                }
            }
            burst.probes += 1;
            if let Some(rtt_us) = self.prober.echo(server, seq as u16) {
                if rtt_us >= MIN_PLAUSIBLE_US {
                    replies.push(rtt_us);
                }
            }
        }
        if replies.len() < MIN_ECHO_REPLIES {
            return burst;
        }
        replies.sort_unstable();
        let median_us = median_of_sorted(&replies);
        let measured_at = self.clock.now();
        burst.measurement = Some(Measurement {
            median_us,
            measured_at,
        });
        burst
    }

    /// Reports that are due to be pushed to their control sessions.
    ///
    /// Two independent report types ride the same interest:
    ///
    /// * [`ControlMessage::ServerLeg`] - the ICMP burst median, only while it is
    ///   younger than the cache TTL;
    /// * [`ControlMessage::ServerLegApp`] - the median of the session's passive
    ///   app-RTT window, only while its newest sample is younger than the cache
    ///   TTL and its server still matches the interest.
    ///
    /// Each session receives at most one report of each type per
    /// [`REPORT_INTERVAL`]. Both messages carry the age at send time, so the
    /// client can refuse a stale figure even if the push is delayed. The ICMP
    /// cache TTL and report interval therefore bound the passive report too.
    pub fn due_pushes(&self) -> Vec<(u32, ControlMessage)> {
        if !self.config.enabled {
            return Vec::new();
        }
        let now = self.clock.now();
        let mut inner = self.lock();
        let cache_ttl = self.config.cache_ttl;
        let mut pushes = Vec::new();
        let tokens: Vec<u32> = inner.interests.keys().copied().collect();
        for token in tokens {
            let Some(interest) = inner.interests.get(&token).copied() else {
                continue;
            };

            // ICMP report: unchanged rules.
            if let Some(measurement) = inner.measurements.get(&interest.server).copied() {
                let age = now.saturating_duration_since(measurement.measured_at);
                let due = age <= cache_ttl
                    && !interest
                        .last_report
                        .is_some_and(|last| now.saturating_duration_since(last) < REPORT_INTERVAL);
                if due {
                    let jitter_us = inner.jitter_for(interest.server).unwrap_or(0);
                    let samples = inner
                        .history
                        .get(&interest.server)
                        .map_or(1, |history| history.len().min(u8::MAX as usize) as u8);
                    let age_ms = age.as_millis().min(u128::from(u16::MAX)) as u16;
                    if let Some(entry) = inner.interests.get_mut(&token) {
                        entry.last_report = Some(now);
                        pushes.push((
                            token,
                            ControlMessage::ServerLeg {
                                server: interest.server,
                                median_us: measurement.median_us,
                                jitter_us,
                                samples,
                                age_ms,
                            },
                        ));
                    }
                }
            }

            // Passive app-RTT report: its own window, same cache TTL and rate
            // bound. A window whose server no longer matches the interest (the
            // session switched servers) is never reported.
            let app_due = inner.passive.get(&token).and_then(|window| {
                if window.server != interest.server || window.samples.is_empty() {
                    return None;
                }
                let age = now.saturating_duration_since(window.updated_at);
                if age > cache_ttl {
                    return None;
                }
                if window
                    .last_report
                    .is_some_and(|last| now.saturating_duration_since(last) < REPORT_INTERVAL)
                {
                    return None;
                }
                let mut samples: Vec<u32> = window.samples.iter().copied().collect();
                samples.sort_unstable();
                Some((
                    median_of_sorted(&samples),
                    window.samples.len().min(u8::MAX as usize) as u8,
                    age,
                ))
            });
            if let Some((passive_median_us, passive_samples, age)) = app_due {
                if let Some(window) = inner.passive.get_mut(&token) {
                    window.last_report = Some(now);
                }
                pushes.push((
                    token,
                    ControlMessage::ServerLegApp {
                        server: interest.server,
                        passive_median_us,
                        passive_samples,
                        age_ms: age.as_millis().min(u128::from(u16::MAX)) as u16,
                    },
                ));
            }
        }
        pushes
    }

    /// The cached measurement for `server`, if it is still inside the cache TTL.
    /// Exposed for the relay's diagnostics and tests.
    pub fn cached_measurement(&self, server: Ipv4Addr) -> Option<(u32, u32)> {
        let now = self.clock.now();
        let inner = self.lock();
        inner
            .measurements
            .get(&server)
            .filter(|measurement| {
                now.saturating_duration_since(measurement.measured_at) <= self.config.cache_ttl
            })
            .map(|measurement| (measurement.median_us, inner.jitter_for(server).unwrap_or(0)))
    }

    /// Number of live interests (tests and diagnostics).
    pub fn interest_count(&self) -> usize {
        self.lock().interests.len()
    }

    /// Lifetime bursts started.
    pub fn burst_count(&self) -> u64 {
        self.lock().bursts
    }

    /// Lifetime echo requests sent.
    pub fn probe_count(&self) -> u64 {
        self.lock().probes
    }

    /// Lifetime accepted passive app-RTT samples (tests and diagnostics).
    pub fn passive_sample_count(&self) -> u64 {
        self.lock().passive_samples
    }

    /// Lifetime bursts cut short because the egress budget refused an echo.
    pub fn budget_denied_count(&self) -> u64 {
        self.lock().budget_denied
    }

    /// Lifetime bursts that produced no measurement.
    pub fn unavailable_count(&self) -> u64 {
        self.lock().unavailable
    }

    /// Lifetime skipped opportunities (address refused, budget, capacity).
    pub fn skip_count(&self) -> u64 {
        self.lock().skipped
    }

    /// Attach the probe thread to `Arc<Self>`; returns `None` when probing is
    /// disabled. The thread runs until the process exits, which is the only
    /// shutdown a relay's probe cadence needs.
    pub fn spawn_prober(self: Arc<Self>) -> Option<std::thread::JoinHandle<()>> {
        if !self.enabled() {
            return None;
        }
        std::thread::Builder::new()
            .name("server-leg-prober".to_string())
            .spawn(move || loop {
                self.probe_due();
                std::thread::sleep(PROBE_TICK);
            })
            .ok()
    }

    fn count_skip(&self) {
        self.lock().skipped += 1;
    }

    fn record_skip_inner(&self, inner: &mut Inner) {
        inner.skipped += 1;
    }
}

impl Inner {
    /// Drop interests with no forward inside [`INTEREST_TTL`], then drop the
    /// per-server state nothing references any more so the maps stay bounded by
    /// the live session set rather than by every server ever seen.
    fn prune(&mut self, now: Instant) {
        let ttl = INTEREST_TTL;
        self.interests
            .retain(|_, interest| now.saturating_duration_since(interest.last_seen) <= ttl);
        let live: std::collections::HashSet<Ipv4Addr> = self
            .interests
            .values()
            .map(|interest| interest.server)
            .collect();
        self.measurements.retain(|server, _| live.contains(server));
        self.history.retain(|server, _| live.contains(server));
        self.last_attempt.retain(|server, _| live.contains(server));
        // A passive window survives only while its session interest exists and
        // still names the same server, so the map is bounded by the live session
        // set and can never serve a stale pair.
        let live_pairs: std::collections::HashSet<(u32, Ipv4Addr)> = self
            .interests
            .iter()
            .map(|(token, interest)| (*token, interest.server))
            .collect();
        self.passive
            .retain(|token, window| live_pairs.contains(&(*token, window.server)));
    }

    /// Servers that are due for a burst, oldest attempt first: a server with no
    /// cached measurement, or whose last attempt is at least the probe interval
    /// old. The cache TTL is enforced on the report side (`due_pushes` never
    /// sends a measurement older than it) and suppresses duplicate bursts
    /// inside the cadence, so a hundred sessions naming one server still cost
    /// one burst.
    fn due_servers(&self, now: Instant, config: &ServerLegConfig) -> Vec<Ipv4Addr> {
        let mut servers: Vec<Ipv4Addr> = self
            .interests
            .values()
            .filter(|interest| is_probeable(interest.server))
            .map(|interest| interest.server)
            .collect();
        servers.sort_unstable();
        servers.dedup();
        servers.retain(|server| {
            let Some(last) = self.last_attempt.get(server) else {
                return true;
            };
            now.saturating_duration_since(*last) >= config.probe_interval
        });
        servers
    }

    /// Refill the per-second burst budget if the window elapsed.
    fn refill_budget(&mut self, now: Instant) {
        if now.saturating_duration_since(self.budget_start) >= Duration::from_secs(1) {
            self.budget_start = now;
            self.budget_used = 0;
        }
    }

    fn jitter_for(&self, server: Ipv4Addr) -> Option<u32> {
        let history = self.history.get(&server)?;
        if history.len() < 2 {
            return None;
        }
        let samples: Vec<u32> = history.iter().copied().collect();
        let total: u64 = samples
            .windows(2)
            .map(|pair| u64::from(pair[1].abs_diff(pair[0])))
            .sum();
        Some((total / (samples.len() - 1) as u64) as u32)
    }
}

/// Median of already-sorted samples (mean of the middle pair for even counts).
fn median_of_sorted(sorted: &[u32]) -> u32 {
    if sorted.is_empty() {
        return 0;
    }
    let mid = sorted.len() / 2;
    if sorted.len() % 2 == 1 {
        sorted[mid]
    } else {
        ((u64::from(sorted[mid - 1]) + u64::from(sorted[mid])) / 2) as u32
    }
}

/// One passive relay→server app-RTT sample from two monotonic session-clock
/// timestamps, in microseconds.
///
/// `marker_us` is the session-clock time at which the packet awaiting a response
/// was forwarded (already recorded by the data path); `now_us` is the time the
/// first response was read on the same session socket. Returns `None` when no
/// forward was outstanding (`marker_us == 0`), the response is not strictly
/// later than the forward, or the gap is outside
/// `[`[`MIN_PLAUSIBLE_US`]`, `[`MAX_PASSIVE_US`]`]` - a zero or absurd difference
/// is not a measurement the relay may report. The subtraction saturates so a
/// clock slip can never wrap into a plausible-looking figure.
pub fn passive_app_rtt(now_us: u64, marker_us: u64) -> Option<u32> {
    if marker_us == 0 || now_us <= marker_us {
        return None;
    }
    let rtt_us = now_us.saturating_sub(marker_us);
    if rtt_us < u64::from(MIN_PLAUSIBLE_US) || rtt_us > u64::from(MAX_PASSIVE_US) {
        return None;
    }
    Some(rtt_us as u32)
}

/// Whether `addr` may be probed: public unicast only.
///
/// The design makes "never probe a private, loopback, link-local or reserved
/// address" mandatory. `proxy/src/abuse.rs` owns destination validation for the
/// data path; this is the same rule expressed for the prober's fixed-format
/// echo, and the relay only ever registers an address that already passed the
/// data path's `record_inbound` destination check.
pub fn is_probeable(addr: Ipv4Addr) -> bool {
    if addr.is_unspecified()
        || addr.is_broadcast()
        || addr.is_multicast()
        || addr.is_loopback()
        || addr.is_private()
        || addr.is_link_local()
        || addr.is_documentation()
    {
        return false;
    }
    let octets = addr.octets();
    // 0.0.0.0/8 "this network" is not a destination; 240.0.0.0/4 reserved
    // (includes 255.255.255.255).
    if octets[0] == 0 || octets[0] >= 240 {
        return false;
    }
    // 100.64.0.0/10 carrier-grade NAT.
    if octets[0] == 100 && (64..=127).contains(&octets[1]) {
        return false;
    }
    // 198.18.0.0/15 benchmarking.
    if octets[0] == 198 && (octets[1] == 18 || octets[1] == 19) {
        return false;
    }
    // 192.0.0.0/24 IETF protocol assignments, 192.88.99.0/24 6to4 relay.
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 0 {
        return false;
    }
    if octets[0] == 192 && octets[1] == 88 && octets[2] == 99 {
        return false;
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

    /// One scripted echo reply per call, in order. An empty script is a timeout.
    struct ScriptedProber {
        replies: Mutex<VecDeque<Option<u32>>>,
        calls: AtomicUsize,
    }

    impl ScriptedProber {
        fn new(replies: Vec<Option<u32>>) -> Self {
            Self {
                replies: Mutex::new(replies.into()),
                calls: AtomicUsize::new(0),
            }
        }

        fn calls(&self) -> usize {
            self.calls.load(Ordering::Relaxed)
        }
    }

    impl EchoProber for ScriptedProber {
        fn echo(&self, _target: Ipv4Addr, _seq: u16) -> Option<u32> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            self.replies
                .lock()
                .expect("script")
                .pop_front()
                .unwrap_or(None)
        }
    }

    struct TestClock {
        base: Instant,
        offset_us: AtomicU64,
    }

    impl TestClock {
        fn new() -> Self {
            Self {
                base: Instant::now(),
                offset_us: AtomicU64::new(0),
            }
        }

        fn advance(&self, duration: Duration) {
            self.offset_us
                .fetch_add(duration.as_micros() as u64, Ordering::Relaxed);
        }
    }

    impl Clock for TestClock {
        fn now(&self) -> Instant {
            self.base + Duration::from_micros(self.offset_us.load(Ordering::Relaxed))
        }
    }

    /// A public unicast address for each test.
    fn server(last: u8) -> Ipv4Addr {
        Ipv4Addr::new(91, 199, 82, last)
    }

    fn registry(
        config: ServerLegConfig,
        clock: Arc<TestClock>,
        prober: Arc<ScriptedProber>,
    ) -> ServerLegRegistry {
        ServerLegRegistry::new(config, clock, prober)
    }

    /// A guard with a flat byte ceiling and a 100 % hard stop, enabled.
    fn budget(max_bytes: u64) -> Arc<BudgetGuard> {
        Arc::new(BudgetGuard::new(crate::config::BudgetConfig {
            enabled: true,
            max_bytes,
            monthly_bytes: 0,
            soft_threshold_pct: 0,
            hard_threshold_pct: 100,
        }))
    }

    #[test]
    fn a_denied_egress_budget_stops_the_burst() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![Some(40_000); 9]));
        let metrics = Arc::new(ProxyMetrics::new());
        // The data plane has already consumed the whole 1 kB ceiling, so the
        // first 64-byte echo charge would be 106 % and must be refused.
        metrics.bytes_relayed.store(1_000, Ordering::Relaxed);
        let guard = budget(1_000);
        let reg = registry(
            ServerLegConfig::default(),
            Arc::clone(&clock),
            Arc::clone(&prober),
        )
        .with_metrics(Arc::clone(&metrics))
        .with_budget(Arc::clone(&guard));
        reg.note_forward(
            1,
            Ipv4Addr::new(198, 51, 100, 9),
            server(5),
            FORWARD_THRESHOLD,
        );

        let tick = reg.probe_due();
        assert_eq!(tick.bursts, 1);
        assert_eq!(tick.denied, 1, "the burst is denied, not unavailable");
        assert_eq!(tick.unavailable, 0);
        assert_eq!(tick.probes, 0, "no echo is sent once the budget denies");
        assert_eq!(prober.calls(), 0);
        assert_eq!(reg.budget_denied_count(), 1);
        assert_eq!(metrics.server_leg_budget_denied.load(Ordering::Relaxed), 1);

        // The budget is still exhausted on the next cadence: still no echo.
        clock.advance(PROBE_INTERVAL);
        assert_eq!(reg.probe_due().denied, 1);
        assert_eq!(prober.calls(), 0, "a denied burst must never send");
        assert_eq!(reg.budget_denied_count(), 2);
    }

    #[test]
    fn probe_egress_is_charged_and_counted_when_the_budget_allows() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(40_000),
            Some(41_000),
            Some(42_000),
        ]));
        let metrics = Arc::new(ProxyMetrics::new());
        let guard = budget(1_000_000);
        let reg = registry(
            ServerLegConfig::default(),
            Arc::clone(&clock),
            Arc::clone(&prober),
        )
        .with_metrics(Arc::clone(&metrics))
        .with_budget(Arc::clone(&guard));
        reg.note_forward(
            1,
            Ipv4Addr::new(198, 51, 100, 9),
            server(6),
            FORWARD_THRESHOLD,
        );

        let tick = reg.probe_due();
        assert_eq!(tick.probes, BURST_ECHOES as u64);
        assert_eq!(prober.calls(), BURST_ECHOES);
        assert_eq!(
            guard.probe_egress_bytes(),
            BURST_ECHOES as u64 * ECHO_EGRESS_BYTES,
            "every sent echo must be charged to the egress guard"
        );
        assert_eq!(reg.probe_count(), BURST_ECHOES as u64);
        assert_eq!(
            metrics.server_leg_probes.load(Ordering::Relaxed),
            BURST_ECHOES as u64
        );
        assert_eq!(reg.budget_denied_count(), 0);
        // The charged bytes are part of the total the guard accounts against.
        assert_eq!(
            guard.metered_total(metrics.bytes_relayed.load(Ordering::Relaxed)),
            BURST_ECHOES as u64 * ECHO_EGRESS_BYTES
        );
    }

    #[test]
    fn a_tight_probe_budget_saturates_instead_of_oversending() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![Some(40_000); 30]));
        let metrics = Arc::new(ProxyMetrics::new());
        let guard = budget(256);
        let reg = registry(
            ServerLegConfig::default(),
            Arc::clone(&clock),
            Arc::clone(&prober),
        )
        .with_metrics(Arc::clone(&metrics))
        .with_budget(Arc::clone(&guard));
        for index in 0..5u32 {
            reg.note_forward(
                index,
                Ipv4Addr::new(198, 51, 100, 9),
                server(index as u8 + 10),
                FORWARD_THRESHOLD,
            );
        }

        // First burst fits three 64-byte charges (192 B, 75 %); the second
        // burst's first charge would be exactly 100 % and is denied.
        let first = reg.probe_due();
        assert_eq!(first.bursts, 2, "the per-second cap allows two bursts");
        assert_eq!(first.probes, BURST_ECHOES as u64);
        assert_eq!(first.denied, 1);
        assert_eq!(prober.calls(), BURST_ECHOES);
        assert_eq!(guard.probe_egress_bytes(), 192);
        assert!(guard.probe_egress_bytes() < 256);

        // However many registrations and ticks follow, the meter never exceeds
        // the ceiling and no further echo is sent. Interests are refreshed each
        // tick (a live session keeps forwarding), so the TTL never prunes them.
        for _ in 0..10 {
            clock.advance(PROBE_INTERVAL);
            for index in 0..5u32 {
                reg.note_forward(
                    index,
                    Ipv4Addr::new(198, 51, 100, 9),
                    server(index as u8 + 10),
                    FORWARD_THRESHOLD,
                );
            }
            let tick = reg.probe_due();
            assert_eq!(tick.probes, 0, "a saturated budget must not send");
            assert!(tick.denied >= 1, "every remaining burst is denied");
        }
        assert_eq!(prober.calls(), BURST_ECHOES);
        assert!(
            guard.probe_egress_bytes() <= 256,
            "metered probe egress must never exceed the ceiling"
        );
        assert_eq!(reg.budget_denied_count(), 21);
    }

    #[test]
    fn median_and_jitter_are_computed_from_the_burst_medians() {
        let clock = Arc::new(TestClock::new());
        // Burst 1: 100/120/140 ms -> median 120; burst 2: 130/150/170 -> 150;
        // burst 3: 140/160/200 -> 160.
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(100_000),
            Some(120_000),
            Some(140_000),
            Some(130_000),
            Some(150_000),
            Some(170_000),
            Some(140_000),
            Some(160_000),
            Some(200_000),
        ]));
        let reg = registry(
            ServerLegConfig::default(),
            Arc::clone(&clock),
            Arc::clone(&prober),
        );
        reg.note_forward(
            1,
            Ipv4Addr::new(198, 51, 100, 9),
            server(1),
            FORWARD_THRESHOLD,
        );

        assert_eq!(reg.probe_due().bursts, 1);
        assert_eq!(reg.cached_measurement(server(1)), Some((120_000, 0)));

        clock.advance(PROBE_INTERVAL);
        assert_eq!(reg.probe_due().bursts, 1);
        // Two medians so far: |150 - 120| = 30 ms.
        assert_eq!(reg.cached_measurement(server(1)), Some((150_000, 30_000)));

        clock.advance(PROBE_INTERVAL);
        assert_eq!(reg.probe_due().bursts, 1);
        // Jitter is the mean of consecutive deltas: (30 + 10) / 2 = 20 ms.
        assert_eq!(reg.cached_measurement(server(1)), Some((160_000, 20_000)));
        assert_eq!(reg.burst_count(), 3);
        assert_eq!(prober.calls(), 9);
    }

    #[test]
    fn the_cache_prevents_a_second_probe_inside_the_ttl() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(50_000),
            Some(52_000),
            Some(54_000),
        ]));
        let reg = registry(
            ServerLegConfig::default(),
            Arc::clone(&clock),
            Arc::clone(&prober),
        );
        // A hundred sessions to the same server cost one burst, not a hundred.
        for token in 0..100u32 {
            reg.note_forward(
                token,
                Ipv4Addr::new(198, 51, 100, 9),
                server(2),
                FORWARD_THRESHOLD,
            );
        }
        assert_eq!(reg.interest_count(), 100);
        assert_eq!(reg.probe_due().bursts, 1);
        assert_eq!(prober.calls(), BURST_ECHOES);

        // Inside the cache TTL (and inside the burst cadence) no second probe
        // runs, even for a freshly registered session.
        reg.note_forward(
            101,
            Ipv4Addr::new(198, 51, 100, 9),
            server(2),
            FORWARD_THRESHOLD,
        );
        clock.advance(PROBE_INTERVAL - Duration::from_secs(1));
        assert_eq!(reg.probe_due().bursts, 0);
        assert_eq!(prober.calls(), BURST_ECHOES);

        // Once the burst cadence is due the next burst runs; it is still well
        // inside the cache TTL, which is why the *report* is served from the
        // cache rather than being a second probe.
        clock.advance(Duration::from_secs(2));
        assert_eq!(reg.probe_due().bursts, 1);
        assert!(reg.cached_measurement(server(2)).is_some());
    }

    #[test]
    fn the_global_burst_cap_holds_under_many_registrations() {
        let clock = Arc::new(TestClock::new());
        let mut replies = Vec::new();
        for _ in 0..30 {
            replies.push(Some(40_000));
        }
        let prober = Arc::new(ScriptedProber::new(replies));
        let config = ServerLegConfig {
            max_bursts_per_second: 2,
            max_concurrent: 3,
            ..ServerLegConfig::default()
        };
        let reg = registry(config, Arc::clone(&clock), Arc::clone(&prober));
        for index in 0..10u32 {
            reg.note_forward(
                index,
                Ipv4Addr::new(198, 51, 100, 9),
                server(index as u8 + 10),
                FORWARD_THRESHOLD,
            );
        }
        let tick = reg.probe_due();
        assert_eq!(tick.bursts, 2, "only the global budget may start");
        assert_eq!(tick.skipped, 8);
        assert_eq!(prober.calls(), 2 * BURST_ECHOES);

        // The budget refills after a second; the remaining servers get a turn.
        clock.advance(Duration::from_secs(1));
        assert_eq!(reg.probe_due().bursts, 2);
    }

    #[test]
    fn the_concurrency_cap_also_bounds_a_tick() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![Some(40_000); 30]));
        let config = ServerLegConfig {
            max_bursts_per_second: 100,
            max_concurrent: 1,
            ..ServerLegConfig::default()
        };
        let reg = registry(config, Arc::clone(&clock), Arc::clone(&prober));
        for index in 0..5u32 {
            reg.note_forward(
                index,
                Ipv4Addr::new(198, 51, 100, 9),
                server(index as u8 + 20),
                FORWARD_THRESHOLD,
            );
        }
        let tick = reg.probe_due();
        assert_eq!(tick.bursts, 1);
        assert_eq!(tick.skipped, 4);
    }

    #[test]
    fn private_and_reserved_ranges_are_refused() {
        let refused = [
            Ipv4Addr::new(10, 0, 0, 1),
            Ipv4Addr::new(172, 16, 5, 4),
            Ipv4Addr::new(192, 168, 1, 1),
            Ipv4Addr::new(127, 0, 0, 1),
            Ipv4Addr::new(169, 254, 3, 3),
            Ipv4Addr::new(0, 0, 0, 0),
            Ipv4Addr::new(0, 1, 2, 3),
            Ipv4Addr::new(255, 255, 255, 255),
            Ipv4Addr::new(224, 0, 0, 1),
            Ipv4Addr::new(240, 0, 0, 1),
            Ipv4Addr::new(100, 64, 0, 1),
            Ipv4Addr::new(198, 18, 0, 1),
            Ipv4Addr::new(192, 0, 0, 1),
            Ipv4Addr::new(192, 88, 99, 1),
            Ipv4Addr::new(192, 0, 2, 1),
            Ipv4Addr::new(198, 51, 100, 1),
            Ipv4Addr::new(203, 0, 113, 1),
        ];
        for addr in refused {
            assert!(!is_probeable(addr), "{addr} must never be probed");
        }
        for addr in [
            Ipv4Addr::new(91, 199, 82, 166),
            Ipv4Addr::new(8, 8, 8, 8),
            Ipv4Addr::new(45, 32, 72, 7),
        ] {
            assert!(is_probeable(addr), "{addr} is public unicast");
        }

        // A registration for a refused address creates no interest and starts
        // no burst, and is counted as a skip.
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![Some(40_000); 6]));
        let reg = registry(ServerLegConfig::default(), clock, Arc::clone(&prober));
        reg.note_forward(
            1,
            Ipv4Addr::new(198, 51, 100, 9),
            Ipv4Addr::new(192, 168, 1, 10),
            FORWARD_THRESHOLD,
        );
        assert_eq!(reg.interest_count(), 0);
        assert_eq!(reg.skip_count(), 1);
        assert_eq!(reg.probe_due().bursts, 0);
        assert_eq!(prober.calls(), 0);
    }

    #[test]
    fn no_probe_before_the_forward_threshold() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![Some(40_000); 6]));
        let reg = registry(ServerLegConfig::default(), clock, Arc::clone(&prober));
        for forwarded in 0..FORWARD_THRESHOLD {
            reg.note_forward(1, Ipv4Addr::new(198, 51, 100, 9), server(30), forwarded);
        }
        assert_eq!(reg.interest_count(), 0, "no interest before 20 forwards");
        assert_eq!(reg.probe_due().bursts, 0);
        assert_eq!(prober.calls(), 0);

        // The twentieth forward registers interest; the next tick probes.
        reg.note_forward(
            1,
            Ipv4Addr::new(198, 51, 100, 9),
            server(30),
            FORWARD_THRESHOLD,
        );
        assert_eq!(reg.interest_count(), 1);
        assert_eq!(reg.probe_due().bursts, 1);
    }

    #[test]
    fn a_quiet_server_is_unavailable_not_a_fabricated_median() {
        let clock = Arc::new(TestClock::new());
        // ICMP is blocked: every echo times out.
        let prober = Arc::new(ScriptedProber::new(Vec::new()));
        let reg = registry(
            ServerLegConfig::default(),
            Arc::clone(&clock),
            Arc::clone(&prober),
        );
        reg.note_forward(1, Ipv4Addr::new(198, 51, 100, 9), server(40), 20);
        let tick = reg.probe_due();
        assert_eq!(tick.bursts, 1);
        assert_eq!(tick.unavailable, 1);
        assert_eq!(reg.unavailable_count(), 1);
        assert_eq!(reg.cached_measurement(server(40)), None);
        assert!(reg.due_pushes().is_empty(), "unavailable is never reported");

        // A burst with one plausible reply is also not a median.
        let prober = Arc::new(ScriptedProber::new(vec![Some(50_000), None, None]));
        let reg = registry(
            ServerLegConfig::default(),
            Arc::clone(&clock),
            Arc::clone(&prober),
        );
        reg.note_forward(1, Ipv4Addr::new(198, 51, 100, 9), server(41), 20);
        assert_eq!(reg.probe_due().unavailable, 1);
        assert_eq!(reg.cached_measurement(server(41)), None);
    }

    #[test]
    fn reports_are_pushed_at_most_once_per_interval() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(70_000),
            Some(72_000),
            Some(74_000),
        ]));
        let reg = registry(ServerLegConfig::default(), Arc::clone(&clock), prober);
        reg.note_forward(7, Ipv4Addr::new(198, 51, 100, 9), server(50), 20);
        assert_eq!(reg.probe_due().bursts, 1);

        let first = reg.due_pushes();
        assert_eq!(first.len(), 1);
        let (token, message) = &first[0];
        assert_eq!(*token, 7);
        match message {
            ControlMessage::ServerLeg {
                server: reported,
                median_us,
                samples,
                age_ms,
                ..
            } => {
                assert_eq!(*reported, server(50));
                assert_eq!(*median_us, 72_000);
                assert_eq!(*samples, 1);
                assert_eq!(*age_ms, 0);
            }
            other => panic!("expected a server-leg message, got {other:?}"),
        }

        // A second call inside the report interval is suppressed.
        assert!(reg.due_pushes().is_empty());
        clock.advance(REPORT_INTERVAL);
        assert_eq!(reg.due_pushes().len(), 1);
    }

    #[test]
    fn a_measurement_older_than_the_cache_ttl_is_not_reported() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(70_000),
            Some(72_000),
            Some(74_000),
        ]));
        let reg = registry(ServerLegConfig::default(), Arc::clone(&clock), prober);
        reg.note_forward(9, Ipv4Addr::new(198, 51, 100, 9), server(60), 20);
        assert_eq!(reg.probe_due().bursts, 1);
        clock.advance(CACHE_TTL + Duration::from_secs(1));
        // The interest is still live (the client keeps forwarding), but the
        // measurement expired: the row must hide rather than show a stale value.
        assert!(reg.due_pushes().is_empty());
        assert_eq!(reg.cached_measurement(server(60)), None);
    }

    #[test]
    fn a_dead_session_stops_being_probed_and_reported() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![
            Some(70_000),
            Some(72_000),
            Some(74_000),
        ]));
        let reg = registry(ServerLegConfig::default(), Arc::clone(&clock), prober);
        reg.note_forward(11, Ipv4Addr::new(198, 51, 100, 9), server(70), 20);
        assert_eq!(reg.probe_due().bursts, 1);
        clock.advance(INTEREST_TTL + Duration::from_secs(1));
        // The tick is what prunes; after it the dead session's server is gone
        // from the maps and can never be probed or reported again.
        assert_eq!(reg.probe_due().bursts, 0);
        assert_eq!(reg.interest_count(), 0);
        assert!(reg.due_pushes().is_empty());
        // The cache is pruned with the interest, so the maps stay bounded.
        assert_eq!(reg.cached_measurement(server(70)), None);
    }

    #[test]
    fn the_disabled_switch_stops_everything() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(vec![Some(40_000); 6]));
        let config = ServerLegConfig {
            enabled: false,
            ..ServerLegConfig::default()
        };
        let reg = registry(config, clock, prober);
        reg.note_forward(1, Ipv4Addr::new(198, 51, 100, 9), server(80), 20);
        assert_eq!(reg.interest_count(), 0);
        assert_eq!(reg.probe_due().bursts, 0);
        assert!(reg.due_pushes().is_empty());
    }

    #[test]
    fn an_echo_request_has_the_icmp_layout_and_a_valid_checksum() {
        let packet = build_echo_request(0x1234, 7);
        assert_eq!(packet.len(), 8 + ECHO_PAYLOAD.len());
        assert_eq!(packet[0], 8, "ICMP echo request type");
        assert_eq!(&packet[4..6], &[0x12, 0x34], "identifier, network order");
        assert_eq!(&packet[6..8], &[0x00, 0x07], "sequence, network order");
        assert_eq!(&packet[8..], ECHO_PAYLOAD, "payload rides after the header");
        // One's-complement property: a checked packet sums to zero.
        assert_eq!(checksum(&packet), 0);
    }

    #[test]
    fn an_echo_reply_is_only_matched_by_its_own_sequence() {
        let mut reply = build_echo_request(0xABCD, 9);
        reply[0] = 0; // echo reply
        let sum = checksum(&reply);
        reply[2..4].copy_from_slice(&sum.to_be_bytes());
        assert!(is_echo_reply(&reply, 9));
        assert!(
            !is_echo_reply(&reply, 10),
            "a stale reply must not pair with this echo"
        );
        assert!(
            !is_echo_reply(&build_echo_request(0xABCD, 9), 9),
            "a request is not a reply"
        );
        assert!(!is_echo_reply(&reply[..7], 9), "truncated ICMP is rejected");

        // A reply that carries an IPv4 header is accepted too (raw-socket
        // shape); the parser must skip the header rather than read it as ICMP.
        let mut with_ip = vec![0x45u8; 20];
        with_ip.extend_from_slice(&reply);
        assert!(is_echo_reply(&with_ip, 9));
        with_ip[20] = 8;
        assert!(!is_echo_reply(&with_ip, 9));
    }

    // ── Passive app-RTT samples (F4 peer reference) ────────────────

    #[test]
    fn passive_app_rtt_rejects_wrong_direction_and_implausible_gaps() {
        // The direction matters: a response that appears to precede the forward
        // is not a sample. Reversing the subtraction would report 100 ms here.
        assert_eq!(passive_app_rtt(50_000, 150_000), None);
        assert_eq!(passive_app_rtt(120_000, 120_000), None);
        // A zero marker is "no forward outstanding", not a 1 µs round trip.
        assert_eq!(passive_app_rtt(1_000_000, 0), None);
        assert_eq!(passive_app_rtt(1, 0), None);
        // Plausible bounds: at least 1 ms, at most 5 s, both inclusive.
        assert_eq!(passive_app_rtt(1_000, 1), None, "999 µs is not a sample");
        assert_eq!(passive_app_rtt(1_001, 1), Some(1_000));
        assert_eq!(passive_app_rtt(5_000_001, 1), Some(5_000_000));
        assert_eq!(passive_app_rtt(5_000_002, 1), None);
        // A normal exchange: forward at 50 ms, first response at 150 ms.
        assert_eq!(passive_app_rtt(150_000, 50_000), Some(100_000));
    }

    #[test]
    fn a_passive_sample_requires_a_forwarded_server() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(Vec::new()));
        let metrics = Arc::new(ProxyMetrics::new());
        let reg = registry(
            ServerLegConfig::default(),
            Arc::clone(&clock),
            Arc::clone(&prober),
        )
        .with_metrics(Arc::clone(&metrics));

        // No interest: the session has never forwarded to this server, so the
        // sample is not stored, not reported and not counted.
        reg.record_passive(7, server(1), 50_000);
        assert_eq!(reg.passive_sample_count(), 0);
        assert!(reg.due_pushes().is_empty());
        assert_eq!(
            metrics.server_leg_passive_samples.load(Ordering::Relaxed),
            0
        );

        // Once the session has forwarded enough for an interest, its sample is
        // accepted - for that exact token and server only.
        reg.note_forward(
            7,
            Ipv4Addr::new(198, 51, 100, 9),
            server(1),
            FORWARD_THRESHOLD,
        );
        reg.record_passive(7, server(1), 50_000);
        assert_eq!(reg.passive_sample_count(), 1);
        assert_eq!(
            metrics.server_leg_passive_samples.load(Ordering::Relaxed),
            1
        );

        // A different token, a different server, and out-of-bounds gaps are all
        // refused: the counter and the stored window do not move.
        reg.record_passive(8, server(1), 50_000);
        reg.record_passive(7, server(2), 50_000);
        reg.record_passive(7, server(1), 0);
        reg.record_passive(7, server(1), 999);
        reg.record_passive(7, server(1), MAX_PASSIVE_US + 1);
        assert_eq!(reg.passive_sample_count(), 1);
        assert_eq!(
            metrics.server_leg_passive_samples.load(Ordering::Relaxed),
            1
        );
    }

    #[test]
    fn a_passive_report_is_the_median_of_its_window_and_rate_limited() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(Vec::new()));
        let reg = registry(ServerLegConfig::default(), Arc::clone(&clock), prober);
        reg.note_forward(
            3,
            Ipv4Addr::new(198, 51, 100, 9),
            server(10),
            FORWARD_THRESHOLD,
        );
        for rtt_us in [30_000, 50_000, 40_000, 60_000] {
            reg.record_passive(3, server(10), rtt_us);
        }

        let first = reg.due_pushes();
        assert_eq!(
            first.len(),
            1,
            "a passive report exists without any ICMP measurement"
        );
        assert_eq!(first[0].0, 3);
        match &first[0].1 {
            ControlMessage::ServerLegApp {
                server: reported,
                passive_median_us,
                passive_samples,
                age_ms,
            } => {
                assert_eq!(*reported, server(10));
                assert_eq!(*passive_median_us, 45_000, "median of 30/40/50/60 ms");
                assert_eq!(*passive_samples, 4);
                assert_eq!(*age_ms, 0);
            }
            other => panic!("expected a passive server-leg message, got {other:?}"),
        }

        // Same per-session rate bound as the ICMP report.
        assert!(reg.due_pushes().is_empty(), "one report per interval");
        clock.advance(REPORT_INTERVAL);
        assert_eq!(reg.due_pushes().len(), 1);

        // The passive window obeys the same cache TTL: a window with no sample
        // for longer than that is not reported.
        clock.advance(CACHE_TTL + Duration::from_secs(1));
        assert!(reg.due_pushes().is_empty());
    }

    #[test]
    fn the_passive_window_keeps_only_the_most_recent_samples() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(Vec::new()));
        let reg = registry(ServerLegConfig::default(), Arc::clone(&clock), prober);
        reg.note_forward(
            5,
            Ipv4Addr::new(198, 51, 100, 9),
            server(11),
            FORWARD_THRESHOLD,
        );
        for index in 0..PASSIVE_WINDOW as u32 + 4 {
            reg.record_passive(5, server(11), 10_000 + index);
        }
        assert_eq!(
            reg.passive_sample_count(),
            PASSIVE_WINDOW as u64 + 4,
            "the lifetime counter counts every accepted sample"
        );
        let pushes = reg.due_pushes();
        assert_eq!(pushes.len(), 1);
        match &pushes[0].1 {
            ControlMessage::ServerLegApp {
                passive_median_us,
                passive_samples,
                ..
            } => {
                assert_eq!(*passive_samples, PASSIVE_WINDOW as u8);
                // The window holds the newest 16 of 10_000..=10_019, i.e.
                // 10_004..=10_019; the even-count median is (10011+10012)/2.
                assert_eq!(*passive_median_us, 10_011);
            }
            other => panic!("expected a passive server-leg message, got {other:?}"),
        }
    }

    #[test]
    fn a_passive_window_never_mixes_two_game_servers() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(Vec::new()));
        let reg = registry(ServerLegConfig::default(), Arc::clone(&clock), prober);
        reg.note_forward(
            21,
            Ipv4Addr::new(198, 51, 100, 9),
            server(14),
            FORWARD_THRESHOLD,
        );
        reg.record_passive(21, server(14), 40_000);

        // The same session token is re-registered for another server (the
        // client switched game servers). The old window must not accept the new
        // server's samples and must not report for either until a new sample
        // lands; a stale sample for the old pair is never served.
        reg.note_forward(
            21,
            Ipv4Addr::new(198, 51, 100, 9),
            server(15),
            FORWARD_THRESHOLD,
        );
        assert!(reg
            .due_pushes()
            .iter()
            .all(|(_, message)| !matches!(message, ControlMessage::ServerLegApp { .. })));
        reg.record_passive(21, server(15), 60_000);
        assert_eq!(reg.passive_sample_count(), 2);
        let pushes = reg.due_pushes();
        assert_eq!(pushes.len(), 1);
        match &pushes[0].1 {
            ControlMessage::ServerLegApp {
                server: reported,
                passive_median_us,
                passive_samples,
                ..
            } => {
                assert_eq!(*reported, server(15));
                assert_eq!(*passive_median_us, 60_000);
                assert_eq!(*passive_samples, 1, "the old server's sample is dropped");
            }
            other => panic!("expected a passive server-leg message, got {other:?}"),
        }
    }

    #[test]
    fn a_dead_session_drops_its_passive_window() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(Vec::new()));
        let reg = registry(ServerLegConfig::default(), Arc::clone(&clock), prober);
        reg.note_forward(
            9,
            Ipv4Addr::new(198, 51, 100, 9),
            server(12),
            FORWARD_THRESHOLD,
        );
        reg.record_passive(9, server(12), 40_000);
        clock.advance(INTEREST_TTL + Duration::from_secs(1));
        // The probe tick prunes; after it the window and interest are gone.
        assert_eq!(reg.probe_due().bursts, 0);
        assert_eq!(reg.interest_count(), 0);
        assert!(reg.due_pushes().is_empty());
        assert_eq!(
            reg.passive_sample_count(),
            1,
            "the lifetime counter is not a live-state counter"
        );
    }

    #[test]
    fn the_disabled_switch_also_stops_passive_sampling() {
        let clock = Arc::new(TestClock::new());
        let prober = Arc::new(ScriptedProber::new(Vec::new()));
        let config = ServerLegConfig {
            enabled: false,
            ..ServerLegConfig::default()
        };
        let reg = registry(config, clock, prober);
        reg.note_forward(
            1,
            Ipv4Addr::new(198, 51, 100, 9),
            server(80),
            FORWARD_THRESHOLD,
        );
        reg.record_passive(1, server(80), 40_000);
        assert_eq!(reg.passive_sample_count(), 0);
        assert_eq!(reg.interest_count(), 0);
        assert!(reg.due_pushes().is_empty());
    }
}
