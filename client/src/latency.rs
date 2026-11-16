//! Direct (ICMP) and relayed (tunnel) latency measurement.
//!
//! `direct_p50_ms` is the median ICMP echo RTT to the detected game server
//! over the un-relayed path. `relayed_p50_ms` is the median round trip of
//! actual tunnelled game traffic (client → relay → game server → relay →
//! client). Their difference is LightSpeed's core value metric, `saved_ms`.
//!
//! ## The relayed window is validated, not merely paired
//!
//! A reply on a busy UDP flow cannot be attributed to any particular send: the
//! server does not echo sequence numbers, so "reply minus the most recent
//! outbound" measures one inter-send gap rather than a round trip. On the live
//! ROB RUN session that produced 25 of 25 samples below the client→relay leg
//! (median ratio 0.129), which no whole-path round trip can be. The relayed
//! window is therefore fed only by **quiet-flow** samples (design
//! `docs/server-leg-signal.md` §5.1): an outbound is armed only when the flow
//! has had no inbound for [`PROBE_QUIET_GAP`], and the next inbound is accepted
//! only when
//!
//! * the relay is unchanged between send and reply,
//! * the flight time is at least the measured client→relay keepalive p50 (a
//!   whole-path RTT cannot be shorter than its first leg), and
//! * the flight time is at most [`PROBE_WINDOW`].
//!
//! Anything else is counted ([`ProbeCounters`]) and dropped, never clamped. An
//! accepted sample also feeds the destination-aware routing estimator through
//! `route::destination::observe_tunnelled`, which is what makes
//! `DestLegSource::Measured` reachable.
//!
//! `--diagnose` is the one caller that pairs exactly (it matches the reply
//! payload to its own probe), so it records through a separate diagnostic
//! window ([`LatencyTracker::record_diagnostic_rtt`]) that can never be
//! presented as game-traffic latency.
//!
//! `direct_app_p50_ms` is the median round trip of a small sample of the game's
//! own packets on the direct, un-relayed path, timed against the server's
//! reply. It is game-packet-to-game-packet with `relayed_p50_ms`, so the two are
//! directly comparable, unlike the ICMP-based `direct_p50_ms`, which stays as
//! the estimate.
//!
//! ## A shadow-direct sample is validated against the measured direct path
//!
//! On a streaming flow the next inbound datagram is usually not the reply to
//! the re-injected packet, so the gap it times is an inter-packet interval, not
//! a round trip. The sampler therefore accepts a sample only when all of these
//! hold, and stores nothing otherwise (a rejection is counted and traced, never
//! clamped):
//!
//! * the relay is unchanged between the arming send and the reply (an unknown
//!   relay at either end cannot prove that, so it is a rejection too),
//! * a fresh measured ICMP median exists for the same server: without a
//!   measured direct path there is no physical floor to validate the game-packet
//!   round trip against, and
//! * the sample is at least [`MIN_PLAUSIBLE_DIRECT_MS`] (a public-internet
//!   round trip cannot be sub-millisecond) and at least
//!   `SHADOW_MIN_FRACTION_OF_ICMP` of that ICMP median.
//!
//! The first rule matters because of the live defect this validation exists for:
//! on the task-10 ROB RUN session the very first shadow sample was **0.4248 ms**
//! against a direct ICMP RTT of **283.47 ms** (0.15 %) and it was accepted
//! before any ICMP history existed, because the fraction rule simply did not run
//! for the first sample. A collapsed `direct_app_p50_ms` makes
//! `saved_app = direct - relayed` negative by construction, so it must never
//! reach the window, and the window's median must never be published without a
//! validated relayed term beside it (see [`LatencyTracker::app_saving_report`]).
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

use std::collections::{HashMap, VecDeque};
use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex as StdMutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use socket2::{Domain, Protocol, SockAddr, Socket, Type};
use tracing::debug;

use crate::telemetry::TelemetryCollector;

/// Number of ICMP probes in one direct burst.
pub const DIRECT_PROBE_COUNT: usize = 5;
/// Minimum plausible direct RTT in milliseconds. A faster reply was answered
/// locally rather than over the public internet and must never be stored.
pub const MIN_PLAUSIBLE_DIRECT_MS: f32 = 1.0;
/// Minimum plausible replies in a burst before a median is meaningful.
pub const MIN_DIRECT_SAMPLES: usize = 3;

/// How many burst medians the direct path keeps, for its own jitter figure.
pub const DIRECT_HISTORY_CAPACITY: usize = 16;
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

/// A shadow sample below this fraction of the measured ICMP RTT to the same
/// server is a pairing error, not a fast path. On a streaming flow the "next"
/// inbound datagram is usually not the reply, so a sample can land an
/// inter-packet gap after the outbound: live traffic produced 6.4 ms against an
/// ICMP RTT of 299 ms, which no un-relayed round trip can be.
const SHADOW_MIN_FRACTION_OF_ICMP: f32 = 0.5;

/// Absolute floor for a shadow-direct sample: a round trip to a public game
/// server over the internet cannot be faster than this, the same physical
/// argument as [`MIN_PLAUSIBLE_DIRECT_MS`]. A sub-millisecond "reply" was
/// answered locally (or is another datagram of the flow, not the reply), and
/// must never be stored, reported or clamped. This floor needs no ICMP history,
/// so it also covers the live defect where the very first sample was 0.4248 ms
/// against a 283.47 ms direct path.
const SHADOW_MIN_PLAUSIBLE_MS: f32 = MIN_PLAUSIBLE_DIRECT_MS;

/// Samples needed on both sides before the windows may be compared at all. One
/// sample each is enough to compare two wrong numbers.
const MIN_COMPARABLE_SAMPLES: usize = 8;
/// Minimum interval between shadow-direct samples for the same server. At most
/// one of the game's own packets per server per interval is timed directly.
pub const SHADOW_SAMPLE_INTERVAL: Duration = Duration::from_secs(5);
/// How long a stored shadow-direct median remains reportable.
pub const SHADOW_DIRECT_TTL: Duration = Duration::from_secs(300);
/// Rolling window capacity for shadow-direct application RTT samples.
pub const SHADOW_RING_CAPACITY: usize = 256;
/// How long a client->relay keepalive RTT stays usable as the bypass
/// pre-gate's first-hop input. The interceptor keepalives every 5 s, so a
/// sample older than this means keepalives stopped (relay changed or dropped)
/// and the gate must fail open instead of comparing stale data.
pub const CLIENT_RELAY_TTL: Duration = Duration::from_secs(15);
/// Rolling window capacity for client->relay keepalive RTT samples.
pub const CLIENT_RELAY_RING_CAPACITY: usize = 16;

/// A flow must have had no inbound packet for this long before an outbound
/// packet is armed as an attributable whole-path probe.
///
/// This is deliberately far longer than the inter-send period of real game
/// traffic. Design `docs/server-leg-signal.md` §5.1 proposes 1000 ms, and the
/// live measurement that retired the previous 250 ms gap shows why: the ROB RUN
/// session forwarded 36.5 packets/s (a ~27 ms period), so a 250 ms inbound gap
/// still left about nine of the client's own packets in flight and the next
/// reply could be attributed to any of them. The probe then read 0.157 ms
/// against a 234 ms relay leg (`7f34412`). At 1000 ms a reply to a pre-gap
/// packet is ~36 send intervals old, and one older than [`PROBE_WINDOW`] is
/// rejected as a timeout. Menus, loading screens, lobbies and
/// request/response-style traffic still leave gaps this long, so the sample
/// rate is a measurement question (the F1 falsifier), not a guess.
pub const PROBE_QUIET_GAP: Duration = Duration::from_millis(1000);

/// How long an armed probe waits for its reply.
///
/// This is also the upper physical bound on an accepted whole-path round trip:
/// a reply later than this answers an older send (or a send that never left the
/// host), not the armed one, and is counted as a timeout instead of being
/// clamped. Five seconds matches the largest RTT the destination estimator
/// treats as real.
pub const PROBE_WINDOW: Duration = Duration::from_secs(5);

/// Rolling window capacity for the `--diagnose` window.
///
/// Kept separate from the game-traffic window: a diagnostic probe is an
/// explicit request/response pair, and its value must never be presented as
/// game latency.
pub const DIAGNOSTIC_RING_CAPACITY: usize = 64;

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

/// Keepalive RTT samples for one relay and when the window was last refreshed.
struct ClientRelaySample {
    samples_ms: Vec<f32>,
    measured_at: Instant,
}

#[derive(Default)]
struct Inner {
    direct: HashMap<Ipv4Addr, DirectSample>,
    /// Recent burst medians per server, newest last: the direct path's own
    /// spread, which is what a relay has to beat to be worth its hop.
    direct_history: HashMap<Ipv4Addr, VecDeque<f32>>,
    last_burst: HashMap<Ipv4Addr, Instant>,
    client_relay: HashMap<SocketAddrV4, ClientRelaySample>,
    /// When the last inbound for a server arrived: the quiet-gap test.
    last_inbound: HashMap<Ipv4Addr, Instant>,
    /// The send a reply could be attributed to, once the flow went quiet.
    armed: HashMap<Ipv4Addr, ArmedProbe>,
    /// Validated quiet-flow whole-path samples for one server through one relay.
    relayed: Vec<f32>,
    relayed_server: Option<Ipv4Addr>,
    /// The relay the relayed window belongs to, so changing relay cannot mix
    /// two paths into one median.
    relayed_relay: Option<SocketAddrV4>,
    /// Counters for the quiet-flow probe, exposed through [`ProbeCounters`].
    probe_counters: ProbeCounters,
    /// `--diagnose`'s exact-payload window. Never reported as game latency.
    diagnostic: Vec<f32>,
    diagnostic_server: Option<Ipv4Addr>,
    shadow_pending: HashMap<Ipv4Addr, ShadowPending>,
    shadow_last: HashMap<Ipv4Addr, Instant>,
    shadow: Vec<f32>,
    shadow_server: Option<Ipv4Addr>,
    shadow_measured_at: Option<Instant>,
    shadow_attempts: u32,
    shadow_replies: u32,
    /// Rejection counters for the shadow-direct sampler.
    shadow_counters: ShadowCounters,
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

    /// Direct application loss ratio from shadow attempts vs replies, or `None`
    /// until at least one sample was attempted.
    fn shadow_loss_ratio(&self) -> Option<f32> {
        if self.shadow_attempts == 0 {
            return None;
        }
        let replies = self.shadow_replies.min(self.shadow_attempts);
        Some(1.0 - replies as f32 / self.shadow_attempts as f32)
    }

    /// Client->relay p50 for `relay`, using `now` for the freshness check so a
    /// caller already holding the lock cannot deadlock on it.
    fn client_relay_p50_at(&self, relay: SocketAddrV4, now: Instant) -> Option<f32> {
        let sample = self.client_relay.get(&relay)?;
        if sample.samples_ms.is_empty()
            || now.saturating_duration_since(sample.measured_at) >= CLIENT_RELAY_TTL
        {
            return None;
        }
        let mut sorted = sample.samples_ms.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(TelemetryCollector::percentile(&sorted, 50.0))
    }

    /// Median of the `--diagnose` window, if any.
    fn diagnostic_median(&self) -> Option<f32> {
        if self.diagnostic.is_empty() {
            return None;
        }
        let mut sorted = self.diagnostic.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        Some(TelemetryCollector::percentile(&sorted, 50.0))
    }

    /// Count one rejected probe and trace why. Never stores or clamps a value.
    fn reject_probe(
        &mut self,
        server: Ipv4Addr,
        relay: Option<SocketAddrV4>,
        elapsed: Duration,
        reason: ProbeRejection,
    ) {
        reason.count(&mut self.probe_counters);
        debug!(
            server = %server,
            relay = ?relay,
            rtt_ms = elapsed.as_secs_f64() * 1000.0,
            reason = reason.as_str(),
            "Quiet-flow tunnelled RTT rejected"
        );
    }

    /// Count one rejected shadow-direct sample and trace why. Never stores or
    /// clamps a value.
    fn reject_shadow(
        &mut self,
        server: Ipv4Addr,
        elapsed: Duration,
        icmp_floor_ms: Option<f32>,
        reason: ShadowRejection,
    ) {
        reason.count(&mut self.shadow_counters);
        debug!(
            server = %server,
            rtt_ms = elapsed.as_secs_f64() * 1000.0,
            icmp_floor_ms = ?icmp_floor_ms,
            reason = reason.as_str(),
            "Shadow-direct application RTT rejected"
        );
    }
}

/// A same-server, fresh shadow-direct and relayed window, ready for a
/// like-for-like bypass comparison.
///
/// Both windows are keyed by server and come from the same moment, so the
/// difference between the two medians is meaningful. It carries sample counts
/// and the raw jitter/loss so the bypass gate can apply its own thresholds;
/// this type never decides anything. Absent or mismatched windows yield `None`,
/// which the gate treats as fail-open to the relay.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LikeForLike {
    /// The server both windows describe.
    pub server: Ipv4Addr,
    /// Shadow-direct application p50 (ms).
    pub direct_app_p50_ms: f32,
    /// Relayed application p50 (ms).
    pub relay_app_p50_ms: f32,
    /// Shadow-direct jitter (ms), once two samples exist.
    pub direct_app_jitter_ms: Option<f32>,
    /// Relayed jitter (ms), once two samples exist.
    pub relay_app_jitter_ms: Option<f32>,
    /// Direct application loss ratio, once a sample was attempted.
    pub direct_loss_ratio: Option<f32>,
    /// Shadow-direct samples in the current window.
    pub shadow_samples: u32,
    /// Relayed samples in the current window.
    pub relay_samples: u32,
}

/// A physically validated app-level saving pair, ready to be published in one
/// telemetry report.
///
/// The relay exports `saved_app = direct_app_p50_ms - relayed_p50_ms` from a
/// report, so that subtraction is only a measurement when **both** terms are
/// validated windows for the same server measured now. A shadow window alone
/// (the live 0.4248 ms case) or a relayed window alone can never produce this
/// value; while either term is missing, stale or unvalidated the report must
/// omit the app-level pair rather than publish a number that reads as a path
/// saving. The relayed window itself is fed only by the validated quiet-flow
/// probe (see the module docs), so this type is the single publication gate for
/// `direct_app_p50_ms` / `saved_app_pairs`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AppSavingReport {
    /// Validated direct application p50 (ms), from the shadow window.
    pub direct_app_p50_ms: f32,
    /// Paired observations behind the pair: the smaller of the two windows,
    /// which is what the relay weights the saving by.
    pub pairs: u32,
}

/// An accepted, physically validated whole-path tunnelled round trip.
///
/// Returned by [`LatencyTracker::record_inbound`] so the process-wide hooks can
/// feed the routing estimator without the tracker depending on it. `rtt_us` is
/// the true client → relay → server → relay → client figure, not an inter-send
/// gap: every sample behind it passed the relay, floor and window rules.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ValidatedPathSample {
    /// The game server the round trip describes.
    pub server: Ipv4Addr,
    /// The relay that carried it, unchanged between send and reply.
    pub relay: SocketAddrV4,
    /// Whole-path round trip, microseconds.
    pub rtt_us: u64,
    /// Validated samples in the window after this one was added.
    pub samples: u32,
    /// Median of the window after this sample was added, milliseconds.
    pub path_p50_ms: f32,
}

/// Counters for the validated quiet-flow whole-path probe.
///
/// The v1 falsifier (F1) is "fewer than 8 accepted samples per (relay, server)
/// in a 10-minute match means the design is not viable", so the accepted and
/// rejected totals are published here instead of being inferred from an absence
/// of log lines. Every accepted sample and every rejection is also traced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ProbeCounters {
    /// Samples that passed every validity rule and entered the relayed window.
    pub accepted: u64,
    /// Replies faster than the measured client->relay leg. A whole-path round
    /// trip cannot be shorter than its first leg; rejected, never clamped.
    pub rejected_behind_floor: u64,
    /// Replies whose relay differed from the relay in use when the probe was
    /// armed (including a probe armed with no relay known).
    pub rejected_relay_changed: u64,
    /// Replies later than [`PROBE_WINDOW`], or a probe that expired unanswered.
    pub rejected_timeout: u64,
    /// Probes that cannot be validated at all: no fresh client->relay keepalive
    /// sample exists to check the physical floor, or the relay is unknown at
    /// send or reply time.
    pub rejected_stale: u64,
}

impl ProbeCounters {
    /// Total samples rejected by any rule.
    pub fn rejected(&self) -> u64 {
        self.rejected_behind_floor
            .saturating_add(self.rejected_relay_changed)
            .saturating_add(self.rejected_timeout)
            .saturating_add(self.rejected_stale)
    }

    /// Accepted plus rejected: every probe that resolved to a verdict.
    pub fn resolved(&self) -> u64 {
        self.accepted.saturating_add(self.rejected())
    }
}

/// Why a quiet-flow reply could not become a validated sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ProbeRejection {
    /// The reply was faster than the first leg of the path.
    BehindFloor,
    /// The relay changed between the armed send and the reply.
    RelayChanged,
    /// The reply arrived after [`PROBE_WINDOW`] (or never arrived).
    Timeout,
    /// No fresh keepalive floor or no relay attribution, so the sample cannot
    /// be validated.
    Stale,
}

impl ProbeRejection {
    /// The counter this rejection increments.
    fn count(self, counters: &mut ProbeCounters) {
        let slot = match self {
            Self::BehindFloor => &mut counters.rejected_behind_floor,
            Self::RelayChanged => &mut counters.rejected_relay_changed,
            Self::Timeout => &mut counters.rejected_timeout,
            Self::Stale => &mut counters.rejected_stale,
        };
        *slot = slot.saturating_add(1);
    }

    /// The stable name used in the trace line.
    fn as_str(self) -> &'static str {
        match self {
            Self::BehindFloor => "behind_floor",
            Self::RelayChanged => "relay_changed",
            Self::Timeout => "timeout",
            Self::Stale => "stale",
        }
    }
}

/// Counters for the shadow-direct application RTT sampler.
///
/// On a streaming flow the next inbound datagram usually answers something
/// other than the re-injected packet, so a sample is accepted only when it
/// passes the validity rules documented on the module and
/// [`LatencyTracker::record_shadow_inbound`]. The counters make the shortfall
/// visible instead of leaving an empty window unexplained: the live defect was
/// a single accepted sample with no ICMP floor, followed by no accepted sample
/// for the rest of the session, which could not be told apart from "the sampler
/// never ran".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ShadowCounters {
    /// Samples that passed every validity rule and entered the window.
    pub accepted: u64,
    /// Samples rejected because no fresh measured ICMP median exists yet for
    /// the server: without a measured direct path there is no physical floor to
    /// validate a game-packet round trip against.
    pub rejected_no_icmp_floor: u64,
    /// Samples faster than the direct path can physically be: below
    /// [`MIN_PLAUSIBLE_DIRECT_MS`], or below
    /// `SHADOW_MIN_FRACTION_OF_ICMP` of the measured ICMP p50.
    pub rejected_behind_floor: u64,
    /// Samples whose reply never arrived inside [`SHADOW_DIRECT_TIMEOUT`].
    pub rejected_timeout: u64,
    /// Samples whose relay was unknown or changed between the arming send and
    /// the reply, so the pairing with the relayed path would mix two paths.
    pub rejected_stale_relay: u64,
}

impl ShadowCounters {
    /// Total samples rejected by any rule.
    pub fn rejected(&self) -> u64 {
        self.rejected_no_icmp_floor
            .saturating_add(self.rejected_behind_floor)
            .saturating_add(self.rejected_timeout)
            .saturating_add(self.rejected_stale_relay)
    }

    /// Accepted plus rejected: every claimed sample that resolved to a verdict.
    pub fn resolved(&self) -> u64 {
        self.accepted.saturating_add(self.rejected())
    }
}

/// Why a shadow-direct reply could not become a validated sample.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ShadowRejection {
    /// No fresh measured ICMP median exists for the server.
    NoIcmpFloor,
    /// The sample is faster than the direct path can physically be.
    BehindFloor,
    /// The reply did not arrive inside [`SHADOW_DIRECT_TIMEOUT`].
    Timeout,
    /// The relay was unknown or changed between send and reply.
    StaleRelay,
}

impl ShadowRejection {
    /// The counter this rejection increments.
    fn count(self, counters: &mut ShadowCounters) {
        let slot = match self {
            Self::NoIcmpFloor => &mut counters.rejected_no_icmp_floor,
            Self::BehindFloor => &mut counters.rejected_behind_floor,
            Self::Timeout => &mut counters.rejected_timeout,
            Self::StaleRelay => &mut counters.rejected_stale_relay,
        };
        *slot = slot.saturating_add(1);
    }

    /// The stable name used in the trace line.
    fn as_str(self) -> &'static str {
        match self {
            Self::NoIcmpFloor => "no_icmp_floor",
            Self::BehindFloor => "behind_floor",
            Self::Timeout => "timeout",
            Self::StaleRelay => "stale_relay",
        }
    }
}

/// One re-injected game packet that the server's reply could pair with, and the
/// relay in use when it was sent.
#[derive(Debug, Clone, Copy)]
struct ShadowPending {
    /// When the sampled packet was re-injected onto the direct path.
    sent_at: Instant,
    /// The relay in use at that instant (`None` when no relay is known, which
    /// cannot be shown to be unchanged and so fails validation unless the reply
    /// also sees no relay).
    relay: Option<SocketAddrV4>,
}

/// One send that a single inbound packet could be attributed to.
#[derive(Debug, Clone, Copy)]
struct ArmedProbe {
    /// When the probe's send left the client.
    sent_at: Instant,
    /// The relay in use at that instant, as `session::current_proxy` reported
    /// it. `None` means the send cannot be attributed and must be rejected.
    relay: Option<SocketAddrV4>,
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
    ///
    /// Also maintains the quiet-flow attribution probe: while the flow has had
    /// no inbound for [`PROBE_QUIET_GAP`], the newest send is the one a reply
    /// can be attributed to (design §5.1), so it is kept armed with the relay in
    /// use at that instant. A busy flow cannot have a reply attributed at all,
    /// so nothing is armed and the next inbound is not measured.
    pub fn note_outbound(&self, server: Ipv4Addr) -> bool {
        self.note_outbound_from(server, crate::session::current_proxy())
    }

    /// [`LatencyTracker::note_outbound`] with the relay supplied, so tests do
    /// not depend on the process-wide current-relay state.
    fn note_outbound_from(&self, server: Ipv4Addr, relay: Option<SocketAddrV4>) -> bool {
        let now = self.clock.now();
        let mut inner = self.lock();
        Self::expire_armed(&mut inner, server, now);
        let quiet = inner
            .last_inbound
            .get(&server)
            .is_none_or(|last| now.saturating_duration_since(*last) >= PROBE_QUIET_GAP);
        if quiet {
            inner.armed.insert(
                server,
                ArmedProbe {
                    sent_at: now,
                    relay,
                },
            );
        }
        match inner.last_burst.get(&server) {
            Some(last) if now.saturating_duration_since(*last) < DIRECT_BURST_INTERVAL => false,
            _ => {
                inner.last_burst.insert(server, now);
                true
            }
        }
    }

    /// Drop an armed probe that has waited longer than [`PROBE_WINDOW`], so a
    /// reply much later cannot pair with it, and count the timeout once.
    fn expire_armed(inner: &mut Inner, server: Ipv4Addr, now: Instant) {
        let expired = inner
            .armed
            .get(&server)
            .is_some_and(|armed| now.saturating_duration_since(armed.sent_at) > PROBE_WINDOW);
        if !expired {
            return;
        }
        if let Some(armed) = inner.armed.remove(&server) {
            let elapsed = now.saturating_duration_since(armed.sent_at);
            inner.reject_probe(server, armed.relay, elapsed, ProbeRejection::Timeout);
        }
    }

    /// Record a relayed response for `server`.
    ///
    /// A response becomes a whole-path sample only when it can be attributed to
    /// the armed quiet-flow send and physically validated:
    ///
    /// * the relay is unchanged between send and reply,
    /// * the flight time is at least the measured client->relay keepalive p50,
    /// * the flight time is no more than [`PROBE_WINDOW`].
    ///
    /// Every failure is counted and traced and stores nothing; nothing is
    /// clamped. Only accepted samples enter the relayed window that
    /// [`LatencyTracker::relayed_p50_ms`] and [`LatencyTracker::like_for_like`]
    /// report, so the old busy-flow pairing can no longer produce a figure.
    ///
    /// Returns the accepted sample so the process-wide hook can feed the
    /// destination-aware estimator; the tracker itself stays free of routing.
    pub fn record_inbound(&self, server: Ipv4Addr) -> Option<ValidatedPathSample> {
        self.record_inbound_from(server, crate::session::current_proxy())
    }

    /// [`LatencyTracker::record_inbound`] with the relay supplied, so tests do
    /// not depend on the process-wide current-relay state.
    fn record_inbound_from(
        &self,
        server: Ipv4Addr,
        relay_now: Option<SocketAddrV4>,
    ) -> Option<ValidatedPathSample> {
        let now = self.clock.now();
        let mut inner = self.lock();
        inner.last_inbound.insert(server, now);
        let Some(armed) = inner.armed.remove(&server) else {
            // No quiet-flow send: the reply cannot be attributed, so there is no
            // verdict and no counter (there was no probe to reject).
            return None;
        };
        let elapsed = now.saturating_duration_since(armed.sent_at);
        if elapsed > PROBE_WINDOW {
            inner.reject_probe(server, armed.relay, elapsed, ProbeRejection::Timeout);
            return None;
        }
        let (Some(armed_relay), Some(relay_now)) = (armed.relay, relay_now) else {
            // The relay is unknown at one end, so "unchanged" cannot be shown:
            // never assume it.
            inner.reject_probe(server, armed.relay, elapsed, ProbeRejection::Stale);
            return None;
        };
        if armed_relay != relay_now {
            inner.reject_probe(
                server,
                Some(armed_relay),
                elapsed,
                ProbeRejection::RelayChanged,
            );
            return None;
        }
        let Some(client_leg_ms) = inner.client_relay_p50_at(armed_relay, now) else {
            // The physical floor needs a fresh client->relay leg; without one a
            // sample cannot be validated and is rejected rather than clipped.
            inner.reject_probe(server, Some(armed_relay), elapsed, ProbeRejection::Stale);
            return None;
        };
        let rtt_ms = elapsed.as_secs_f64() * 1000.0;
        if !rtt_ms.is_finite() || rtt_ms < f64::from(client_leg_ms) {
            inner.reject_probe(
                server,
                Some(armed_relay),
                elapsed,
                ProbeRejection::BehindFloor,
            );
            return None;
        }
        if inner.relayed_server != Some(server) || inner.relayed_relay != Some(armed_relay) {
            inner.relayed.clear();
            inner.relayed_server = Some(server);
            inner.relayed_relay = Some(armed_relay);
        }
        if inner.relayed.len() >= RELAYED_RING_CAPACITY {
            inner.relayed.remove(0);
        }
        inner.relayed.push(rtt_ms as f32);
        inner.probe_counters.accepted = inner.probe_counters.accepted.saturating_add(1);
        let samples = inner.relayed.len() as u32;
        let path_p50_ms = inner.relayed_median().unwrap_or_default();
        debug!(
            server = %server,
            relay = %armed_relay,
            rtt_ms = rtt_ms,
            samples = samples,
            path_p50_ms = path_p50_ms,
            "Quiet-flow tunnelled RTT accepted"
        );
        Some(ValidatedPathSample {
            server,
            relay: armed_relay,
            rtt_us: u64::try_from(elapsed.as_micros()).unwrap_or(u64::MAX),
            samples,
            path_p50_ms,
        })
    }

    /// Record one **explicitly correlated** diagnostic round trip for `server`.
    ///
    /// `--diagnose` sends a synthetic probe and only calls this once the reply
    /// payload matches its own, so the attribution the game-traffic rule has to
    /// infer is already exact. That is why it does not need the quiet gap or the
    /// physical floor - and why it lives in its own window: a diagnostic value
    /// is never reported as game-traffic latency
    /// ([`LatencyTracker::relayed_p50_ms`] does not read it) and a game-traffic
    /// sample never appears here.
    pub fn record_diagnostic_rtt(&self, server: Ipv4Addr, rtt_ms: f32) {
        if !rtt_ms.is_finite() || rtt_ms <= 0.0 {
            return;
        }
        let mut inner = self.lock();
        if inner.diagnostic_server != Some(server) {
            inner.diagnostic.clear();
            inner.diagnostic_server = Some(server);
        }
        if inner.diagnostic.len() >= DIAGNOSTIC_RING_CAPACITY {
            inner.diagnostic.remove(0);
        }
        inner.diagnostic.push(rtt_ms);
    }

    /// Median of the `--diagnose` window for the current server, if any.
    pub fn diagnostic_relayed_p50_ms(&self) -> Option<f32> {
        self.lock().diagnostic_median()
    }

    /// Samples in the `--diagnose` window.
    pub fn diagnostic_relayed_samples(&self) -> u32 {
        self.lock().diagnostic.len() as u32
    }

    /// Snapshot of the quiet-flow probe counters.
    pub fn probe_counters(&self) -> ProbeCounters {
        self.lock().probe_counters
    }

    /// Jitter of the direct (ICMP) burst medians for `server`.
    ///
    /// A relay can only ever offer steadiness, never a shorter round trip, so this
    /// is the figure that has to exist for that claim to be checkable at all.
    /// `None` until two bursts have been recorded.
    pub fn direct_jitter_ms(&self, server: Ipv4Addr) -> Option<f32> {
        let inner = self.lock();
        let history = inner.direct_history.get(&server)?;
        let samples: Vec<f32> = history.iter().copied().collect();
        (samples.len() >= 2).then(|| ring_jitter(&samples))
    }

    /// The last direct (ICMP) median for `server`, with how long ago it was taken.
    ///
    /// This is the one latency figure on the client that is a real correlated
    /// round trip: an ICMP echo carries its own sequence, so the reply genuinely
    /// answers the request. It measures client to game server directly and says
    /// nothing about the relayed path, which is why the age comes with it.
    pub fn direct_sample_ms(&self, server: Ipv4Addr) -> Option<(f32, Duration)> {
        let now = self.clock.now();
        let inner = self.lock();
        inner.direct.get(&server).map(|sample| {
            (
                sample.median_ms,
                now.saturating_duration_since(sample.measured_at),
            )
        })
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
        {
            let mut inner = self.lock();
            inner.direct.insert(
                server,
                DirectSample {
                    median_ms: median,
                    measured_at,
                },
            );
            let history = inner.direct_history.entry(server).or_default();
            if history.len() >= DIRECT_HISTORY_CAPACITY {
                history.pop_front();
            }
            history.push_back(median);
        }
        Some(median)
    }

    /// Latest direct median across servers, if a burst has produced one.
    /// The most recently measured direct median across all servers.
    pub fn direct_p50_ms(&self) -> Option<f32> {
        self.lock().latest_direct()
    }

    /// Record one client->relay keepalive RTT (ms) for `relay`. Non-finite and
    /// non-positive samples are dropped, and each relay keeps its own window.
    pub fn record_client_relay_rtt(&self, relay: SocketAddrV4, rtt_ms: f32) {
        if !rtt_ms.is_finite() || rtt_ms <= 0.0 {
            return;
        }
        let now = self.clock.now();
        let mut inner = self.lock();
        let sample = inner
            .client_relay
            .entry(relay)
            .or_insert_with(|| ClientRelaySample {
                samples_ms: Vec::with_capacity(CLIENT_RELAY_RING_CAPACITY),
                measured_at: now,
            });
        if sample.samples_ms.len() >= CLIENT_RELAY_RING_CAPACITY {
            sample.samples_ms.remove(0);
        }
        sample.samples_ms.push(rtt_ms);
        sample.measured_at = now;
    }

    /// Median client->relay RTT (ms) for `relay`, only while a keepalive
    /// sample arrived within [`CLIENT_RELAY_TTL`].
    pub fn client_relay_p50_ms(&self, relay: SocketAddrV4) -> Option<f32> {
        let now = self.clock.now();
        self.lock().client_relay_p50_at(relay, now)
    }

    /// Claim the shadow-direct sampling slot for `server`, capturing the relay
    /// in use at this instant. Returns `true` when a sample is due: the caller
    /// must re-inject the game's own packet unchanged onto the direct path. At
    /// most one sample per server per [`SHADOW_SAMPLE_INTERVAL`]. Uses its own
    /// map, so it never touches the relayed probe's `armed` entry.
    pub fn record_shadow_outbound(&self, server: Ipv4Addr) -> bool {
        self.record_shadow_outbound_from(server, crate::session::current_proxy())
    }

    /// [`LatencyTracker::record_shadow_outbound`] with the relay supplied, so
    /// tests do not depend on the process-wide current-relay state.
    fn record_shadow_outbound_from(&self, server: Ipv4Addr, relay: Option<SocketAddrV4>) -> bool {
        let now = self.clock.now();
        let mut inner = self.lock();
        if let Some(pending) = inner.shadow_pending.get(&server).copied() {
            if now.saturating_duration_since(pending.sent_at) >= SHADOW_DIRECT_TIMEOUT {
                inner.shadow_pending.remove(&server);
                let elapsed = now.saturating_duration_since(pending.sent_at);
                inner.reject_shadow(server, elapsed, None, ShadowRejection::Timeout);
            }
        }
        match inner.shadow_last.get(&server) {
            Some(last) if now.saturating_duration_since(*last) < SHADOW_SAMPLE_INTERVAL => false,
            _ => {
                inner.shadow_last.insert(server, now);
                inner.shadow_pending.insert(
                    server,
                    ShadowPending {
                        sent_at: now,
                        relay,
                    },
                );
                inner.shadow_attempts = inner.shadow_attempts.saturating_add(1);
                true
            }
        }
    }

    /// Record the server's reply to a shadow-direct sample.
    ///
    /// A reply becomes a sample only when it can be physically validated
    /// against the direct path:
    ///
    /// * it pairs with a pending sample inside [`SHADOW_DIRECT_TIMEOUT`],
    /// * the relay is unchanged between the arming send and the reply (an
    ///   unknown relay at either end cannot show that, so it is a rejection too),
    /// * a fresh measured ICMP median exists for the server, and
    /// * the round trip is at least [`MIN_PLAUSIBLE_DIRECT_MS`] and at least
    ///   `SHADOW_MIN_FRACTION_OF_ICMP` of that median.
    ///
    /// Every failure is counted ([`ShadowCounters`]) and traced and stores
    /// nothing; nothing is clamped. Only accepted samples enter the window that
    /// [`LatencyTracker::shadow_direct_p50_ms`] and
    /// [`LatencyTracker::like_for_like`] report, so a collapsed pairing can no
    /// longer become a direct-app median.
    pub fn record_shadow_inbound(&self, server: Ipv4Addr) {
        self.record_shadow_inbound_from(server, crate::session::current_proxy());
    }

    /// [`LatencyTracker::record_shadow_inbound`] with the relay supplied, so
    /// tests do not depend on the process-wide current-relay state.
    fn record_shadow_inbound_from(&self, server: Ipv4Addr, relay_now: Option<SocketAddrV4>) {
        let now = self.clock.now();
        let mut inner = self.lock();
        let Some(pending) = inner.shadow_pending.remove(&server) else {
            // No pending re-injected sample: this inbound cannot be attributed
            // to one, so there is no verdict and no counter.
            return;
        };
        let elapsed = now.saturating_duration_since(pending.sent_at);
        if elapsed >= SHADOW_DIRECT_TIMEOUT {
            inner.reject_shadow(server, elapsed, None, ShadowRejection::Timeout);
            return;
        }
        if pending.relay != relay_now {
            // A relay switch during the sample would make the direct window and
            // the relayed window it is compared with describe two different
            // paths; reject rather than mix them.
            inner.reject_shadow(server, elapsed, None, ShadowRejection::StaleRelay);
            return;
        }
        let Some(icmp_p50) = inner.fresh_direct(server, now) else {
            // Without a measured direct path there is no physical floor. This is
            // the live defect: the first sample (0.4248 ms against 283.47 ms)
            // was accepted here because the fraction check silently did not run.
            inner.reject_shadow(server, elapsed, None, ShadowRejection::NoIcmpFloor);
            return;
        };
        let rtt = (elapsed.as_secs_f64() * 1000.0) as f32;
        if !rtt.is_finite() || rtt < SHADOW_MIN_PLAUSIBLE_MS {
            inner.reject_shadow(
                server,
                elapsed,
                Some(icmp_p50),
                ShadowRejection::BehindFloor,
            );
            return;
        }
        // Faster than the direct path can be is a pairing error, not a fast
        // path: the "next" inbound on a streaming flow is usually not the reply.
        if rtt < icmp_p50 * SHADOW_MIN_FRACTION_OF_ICMP {
            inner.reject_shadow(
                server,
                elapsed,
                Some(icmp_p50),
                ShadowRejection::BehindFloor,
            );
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
        inner.shadow.push(rtt);
        inner.shadow_measured_at = Some(now);
        inner.shadow_replies = inner.shadow_replies.saturating_add(1);
        inner.shadow_counters.accepted = inner.shadow_counters.accepted.saturating_add(1);
        debug!(
            server = %server,
            rtt_ms = rtt,
            icmp_floor_ms = icmp_p50,
            samples = inner.shadow.len(),
            "Shadow-direct application RTT accepted"
        );
    }

    /// Median direct application RTT, only while the shadow window was
    /// measured within [`SHADOW_DIRECT_TTL`].
    ///
    /// Only physically validated samples enter the window, so a value returned
    /// here already passed the direct-path floor. It is still **not** a saving:
    /// a report may publish it only through
    /// [`LatencyTracker::app_saving_report`], which additionally requires a
    /// validated relayed window for the same server.
    pub fn shadow_direct_p50_ms(&self) -> Option<f32> {
        let now = self.clock.now();
        let inner = self.lock();
        let measured_at = inner.shadow_measured_at?;
        if now.saturating_duration_since(measured_at) >= SHADOW_DIRECT_TTL {
            return None;
        }
        inner.shadow_median()
    }

    /// Snapshot of the shadow-direct sampler counters.
    pub fn shadow_counters(&self) -> ShadowCounters {
        self.lock().shadow_counters
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
        self.lock().shadow_loss_ratio()
    }

    /// A fresh same-server shadow-direct and relayed window, or `None` when
    /// the two windows describe different servers, the shadow window is stale,
    /// or either window is empty. Returning `None` is the fail-open signal: the
    /// bypass gate must keep relaying rather than compare unrelated data.
    pub fn like_for_like(&self) -> Option<LikeForLike> {
        let now = self.clock.now();
        let inner = self.lock();
        let server = inner.relayed_server?;
        if inner.shadow_server != Some(server) {
            return None;
        }
        let measured_at = inner.shadow_measured_at?;
        if now.saturating_duration_since(measured_at) >= SHADOW_DIRECT_TTL {
            return None;
        }
        if inner.shadow.len() < MIN_COMPARABLE_SAMPLES
            || inner.relayed.len() < MIN_COMPARABLE_SAMPLES
        {
            return None;
        }
        Some(LikeForLike {
            server,
            direct_app_p50_ms: inner.shadow_median()?,
            relay_app_p50_ms: inner.relayed_median()?,
            direct_app_jitter_ms: (inner.shadow.len() >= 2).then(|| ring_jitter(&inner.shadow)),
            relay_app_jitter_ms: (inner.relayed.len() >= 2).then(|| ring_jitter(&inner.relayed)),
            direct_loss_ratio: inner.shadow_loss_ratio(),
            shadow_samples: inner.shadow.len() as u32,
            relay_samples: inner.relayed.len() as u32,
        })
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

    /// Drain the relayed and shadow-direct windows after a report carrying them
    /// was accepted, so the next report measures a fresh paired window instead
    /// of reusing a direct-app median that has already been reported.
    pub fn commit_report_values(&self) {
        let mut inner = self.lock();
        inner.relayed.clear();
        inner.shadow.clear();
        inner.shadow_measured_at = None;
    }

    /// A validated app-level saving pair for the next telemetry report, or
    /// `None` while either term is unvalidated.
    ///
    /// The relay books `saved_app = direct_app_p50_ms - relayed_p50_ms` from a
    /// report, so that subtraction is only a measurement when both terms are
    /// validated windows for the same server measured now: the shadow window
    /// holds only physically validated samples (see
    /// [`LatencyTracker::record_shadow_inbound`]) and the relayed window only
    /// validated quiet-flow samples (see the module docs). Anything else - no
    /// relayed window yet, a server change, a stale shadow window, an emptied
    /// window - returns `None`, and the report must omit `direct_app_p50_ms` /
    /// `saved_app_pairs` rather than publish a number that reads as a path
    /// saving.
    pub fn app_saving_report(&self) -> Option<AppSavingReport> {
        let now = self.clock.now();
        let inner = self.lock();
        let server = inner.relayed_server?;
        if inner.shadow_server != Some(server) {
            return None;
        }
        let measured_at = inner.shadow_measured_at?;
        if now.saturating_duration_since(measured_at) >= SHADOW_DIRECT_TTL {
            return None;
        }
        if inner.relayed.is_empty() {
            // The relayed window drains after every successful report, so a
            // direct-app median measured before that drain has no partner and
            // must not be published alone.
            return None;
        }
        let direct_app_p50_ms = inner.shadow_median()?;
        let pairs = inner.shadow.len().min(inner.relayed.len()) as u32;
        if pairs == 0 {
            return None;
        }
        Some(AppSavingReport {
            direct_app_p50_ms,
            pairs,
        })
    }

    /// Number of paired direct-application and relayed samples behind the
    /// report values: the smaller of the two validated windows when they belong
    /// to the same server and the shadow median is fresh, else zero. Zero means
    /// the report must not publish an app-level pair.
    pub fn saved_app_pairs(&self) -> u32 {
        self.app_saving_report().map_or(0, |app| app.pairs)
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
    if crate::route::destination::note_outbound(server) {
        crate::quic::report_destination(server);
    }
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
///
/// Only a response that the tracker can attribute to an armed quiet-flow send
/// and validate against the client->relay leg becomes a sample; it then feeds
/// the destination-aware routing estimator, which is what makes
/// `DestLegSource::Measured` reachable. Rejected replies are counted and traced
/// inside the tracker and feed nothing.
pub fn record_inbound(server: Ipv4Addr) {
    if !is_measuring() {
        return;
    }
    let Some(tracker) = global() else {
        return;
    };
    if let Some(sample) = tracker.record_inbound(server) {
        crate::route::destination::observe_tunnelled(sample.relay, sample.server, sample.rtt_us);
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
///
/// The reply is accepted only when it can be physically validated against the
/// measured direct path (relay unchanged, a fresh ICMP median exists, and the
/// round trip is at least [`MIN_PLAUSIBLE_DIRECT_MS`] and
/// `SHADOW_MIN_FRACTION_OF_ICMP` of that median); everything else is counted
/// in [`ShadowCounters`] and stored nowhere.
pub fn record_shadow_inbound(server: Ipv4Addr) {
    if !is_measuring() {
        return;
    }
    if let Some(tracker) = global() {
        tracker.record_shadow_inbound(server);
    }
}

/// Median direct application RTT, if a **validated** shadow window exists.
///
/// Reads local measurement only. This value is not a saving: whether a report
/// may publish it (together with the relayed term) is decided by
/// [`app_saving_report`] and applied in `telemetry::TelemetryCollector::build_report`.
pub fn shadow_direct_p50_ms() -> Option<f32> {
    global().and_then(|tracker| tracker.shadow_direct_p50_ms())
}

/// Median relayed application RTT, or `None` when the window is empty.
pub fn relayed_p50_ms() -> Option<f32> {
    global().and_then(|tracker| tracker.relayed_p50_ms())
}

/// Direct (ICMP) ping to `server`, with how long ago it was measured, through the
/// process-wide tracker.
///
/// This is the one figure on the client that is a genuinely correlated round
/// trip: an ICMP echo carries its own sequence, so the reply answers the request
/// that produced it. It measures the client-to-server path directly - with no
/// relay in it - which is why the age travels with the number.
pub fn direct_sample(server: Ipv4Addr) -> Option<(f32, Duration)> {
    global().and_then(|tracker| tracker.direct_sample_ms(server))
}

/// Jitter of the direct path to `server`, from the burst medians so far.
pub fn direct_jitter(server: Ipv4Addr) -> Option<f32> {
    global().and_then(|tracker| tracker.direct_jitter_ms(server))
}

/// Run one direct burst against `server` through the process-wide tracker.
///
/// Blocking: five ICMP probes with timeouts is seconds of work, so call this from
/// a worker thread rather than the UI loop.
pub fn probe_direct(server: Ipv4Addr) -> Option<f32> {
    global().and_then(|tracker| tracker.maybe_probe_direct(server))
}

/// Record one client->relay keepalive RTT (ms) for `relay`.
///
/// Also feeds the destination estimator's client-leg ring: that leg is routing
/// state (the `L2 = L3 - L1` subtraction and the triangle-inequality floor), not
/// telemetry, so it is recorded even when measurement is not installed. The
/// keepalive is the only production source of this figure; a validated
/// quiet-flow sample is rejected as `stale` when it is missing rather than
/// checked against a guessed floor.
pub fn record_client_relay_rtt(relay: SocketAddrV4, rtt_ms: f32) {
    if !rtt_ms.is_finite() || rtt_ms <= 0.0 {
        return;
    }
    let rtt_us = (f64::from(rtt_ms) * 1000.0).round();
    if rtt_us > 0.0 && rtt_us <= u64::MAX as f64 {
        crate::route::destination::observe_client_leg(relay, rtt_us as u64);
    }
    if !is_measuring() {
        return;
    }
    if let Some(tracker) = global() {
        tracker.record_client_relay_rtt(relay, rtt_ms);
    }
}

/// Current quiet-flow probe counters, or all-zero when no tracker is installed.
///
/// These are the v1 falsifier's instrument: a 10-minute live match must produce
/// at least 8 accepted samples per (relay, server), and the rejection reasons
/// explain any shortfall.
pub fn probe_counters() -> ProbeCounters {
    global().map_or_else(ProbeCounters::default, |tracker| tracker.probe_counters())
}

/// Current shadow-direct sampler counters, or all-zero when no tracker is
/// installed.
///
/// These explain why an app-level window is empty or why a sample was not
/// accepted: `rejected_no_icmp_floor` above zero with `accepted == 0` is the
/// live defect's signature (a sample attempted before the first direct burst
/// finished).
pub fn shadow_counters() -> ShadowCounters {
    global().map_or_else(ShadowCounters::default, |tracker| tracker.shadow_counters())
}

/// Median client->relay RTT (ms) for `relay`, or `None` when no keepalive
/// sample arrived within [`CLIENT_RELAY_TTL`]. Reads local measurement.
pub fn client_relay_p50_ms(relay: SocketAddrV4) -> Option<f32> {
    global().and_then(|tracker| tracker.client_relay_p50_ms(relay))
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

/// A fresh same-server shadow-direct and relayed window for the bypass gate, or
/// `None` when the pairing is stale or mismatched (fail open to the relay).
pub fn like_for_like() -> Option<LikeForLike> {
    global().and_then(|tracker| tracker.like_for_like())
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

/// Number of paired direct-application and relayed samples behind the next
/// report. Reads local measurement; this is not the reporting gate.
pub fn saved_app_pairs() -> u32 {
    global().map_or(0, |tracker| tracker.saved_app_pairs())
}

/// A physically validated app-level saving pair for the next telemetry report,
/// or `None` while either term is unvalidated.
///
/// This is **the** publication gate for `direct_app_p50_ms` / `saved_app_pairs`:
/// the report must publish the app-level pair only when this returns `Some`,
/// and otherwise omit both fields so the relay cannot book a `saved_app` the
/// client could not validate.
pub fn app_saving_report() -> Option<AppSavingReport> {
    global().and_then(|tracker| tracker.app_saving_report())
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

    /// A tracker with a fresh measured direct (ICMP) median for every named
    /// server. The shadow sampler validates a game-packet round trip against
    /// that floor, so every test that expects an accepted shadow sample must
    /// establish one first.
    fn tracker_with_direct_floors(
        clock: &Arc<TestClock>,
        floors: &[(Ipv4Addr, f32)],
    ) -> LatencyTracker {
        let mut replies = Vec::new();
        for (_, floor_ms) in floors {
            // One burst makes DIRECT_PROBE_COUNT probe calls for the server.
            replies.extend((0..DIRECT_PROBE_COUNT).map(|_| Some(*floor_ms)));
        }
        let t = tracker(Arc::new(ScriptedProber::new(replies)), clock.clone());
        for (server, floor_ms) in floors {
            assert_eq!(t.run_direct_burst(*server), Some(*floor_ms));
        }
        t
    }

    /// [`tracker_with_direct_floors`] for the one server most shadow tests use.
    fn tracker_with_direct_floor(clock: &Arc<TestClock>, floor_ms: f32) -> LatencyTracker {
        tracker_with_direct_floors(clock, &[(SERVER, floor_ms)])
    }

    /// Re-inject one shadow sample for `server` after `rtt` and record its
    /// reply. Returns whether the sampling slot was claimed.
    fn sample_shadow(
        t: &LatencyTracker,
        clock: &TestClock,
        server: Ipv4Addr,
        rtt: Duration,
    ) -> bool {
        let claimed = t.record_shadow_outbound(server);
        clock.advance(rtt);
        t.record_shadow_inbound(server);
        claimed
    }

    /// One accepted quiet-flow sample: refresh the keepalive floor, wait out the
    /// quiet gap, arm a send, let `rtt_ms` pass, then record the reply. Returns
    /// what the tracker reported for the accepted sample.
    fn accept_sample(
        t: &LatencyTracker,
        clock: &TestClock,
        server: Ipv4Addr,
        relay_addr: SocketAddrV4,
        floor_ms: f32,
        rtt_ms: u64,
    ) -> Option<ValidatedPathSample> {
        t.record_client_relay_rtt(relay_addr, floor_ms);
        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(server, Some(relay_addr));
        clock.advance(Duration::from_millis(rtt_ms));
        t.record_inbound_from(server, Some(relay_addr))
    }

    #[test]
    fn the_direct_sample_carries_the_value_and_its_age() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(
            Arc::new(ScriptedProber::new(vec![Some(300.0)])),
            clock.clone(),
        );
        assert_eq!(t.direct_sample_ms(SERVER), None, "nothing measured yet");
        // One burst of plausible replies, then read it back with its age.
        let prober = ScriptedProber::new(vec![Some(280.0), Some(300.0), Some(320.0), None, None]);
        let t = tracker(Arc::new(prober), clock.clone());
        assert_eq!(t.run_direct_burst(SERVER), Some(300.0));
        clock.advance(Duration::from_secs(7));
        let (ms, age) = t.direct_sample_ms(SERVER).expect("measured");
        assert_eq!(ms, 300.0);
        assert_eq!(age, Duration::from_secs(7), "age travels with the figure");
        assert_eq!(t.direct_sample_ms(OTHER_SERVER), None, "per server");
    }

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
        accept_sample(&t, &clock, SERVER, relay(1), 10.0, 20);

        assert_eq!(t.relayed_p50_ms(), Some(20.0));
        assert_eq!(t.saved_ms(), Some(30.0));
    }

    #[test]
    fn an_unattributed_reply_is_ignored() {
        let t = tracker(
            Arc::new(ScriptedProber::new(vec![])),
            Arc::new(TestClock::new()),
        );
        t.record_inbound(SERVER);
        assert_eq!(t.relayed_p50_ms(), None);
        assert_eq!(
            t.probe_counters(),
            ProbeCounters::default(),
            "a reply with no armed probe is not a rejected probe: there was \
             nothing to attribute it to"
        );
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
        accept_sample(&t, &clock, SERVER, relay(1), 10.0, 20);

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
        accept_sample(&t, &clock, OTHER_SERVER, relay(1), 10.0, 20);

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
        accept_sample(&t, &clock, OTHER_SERVER, relay(1), 10.0, 20);

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
        accept_sample(&t, &clock, SERVER, relay(1), 10.0, 20);

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
        accept_sample(&t, &clock, SERVER, relay(1), 10.0, 35);

        assert_eq!(t.take_report_values(), (Some(50.0), Some(35.0)));
        assert_eq!(t.take_report_values(), (Some(50.0), None));
    }

    // ── Validated quiet-flow whole-path probe (design §5.1) ─────────────────

    #[test]
    fn an_accepted_quiet_flow_sample_reports_its_values() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        let sample = accept_sample(&t, &clock, SERVER, relay(1), 200.0, 260)
            .expect("a reply above the client->relay floor is a whole-path sample");

        assert_eq!(sample.server, SERVER);
        assert_eq!(sample.relay, relay(1));
        assert_eq!(
            sample.rtt_us, 260_000,
            "a real flight time, in microseconds"
        );
        assert_eq!(sample.samples, 1);
        assert_eq!(sample.path_p50_ms, 260.0);
        assert_eq!(t.relayed_samples(), 1);
        assert_eq!(t.relayed_p50_ms(), Some(260.0));
        assert_eq!(t.probe_counters().accepted, 1);
        assert_eq!(t.probe_counters().rejected(), 0);
    }

    #[test]
    fn the_quiet_gap_outlasts_the_live_send_cadence() {
        // The live ROB RUN session forwarded 36.5 packets/s (~27 ms period).
        // The retired 250 ms gap still left ~9 packets in flight, so a reply
        // could be attributed to the wrong send and read 0.157 ms against a
        // 234 ms relay leg. 1000 ms is the design's value and must stay well
        // past that cadence.
        assert_eq!(PROBE_QUIET_GAP, Duration::from_millis(1000));
        assert!(
            PROBE_QUIET_GAP >= Duration::from_millis(36 * 27),
            "the gap must span tens of the live session's send intervals"
        );
        assert!(
            PROBE_WINDOW >= Duration::from_secs(5),
            "the upper bound is the largest physical RTT the estimator accepts"
        );
    }

    #[test]
    fn a_busy_flow_never_arms_a_probe() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 100.0);

        // A reply last arrived, then a send 10 ms later is not attributable.
        t.record_inbound_from(SERVER, Some(relay(1)));
        clock.advance(Duration::from_millis(10));
        t.note_outbound_from(SERVER, Some(relay(1)));
        clock.advance(Duration::from_millis(20));
        assert_eq!(t.record_inbound_from(SERVER, Some(relay(1))), None);
        assert_eq!(t.relayed_samples(), 0, "a busy-flow reply is not a sample");
        assert_eq!(
            t.probe_counters(),
            ProbeCounters::default(),
            "no probe was armed, so nothing was rejected"
        );
    }

    #[test]
    fn a_reply_inside_the_physical_floor_is_rejected_not_clamped() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 200.0);

        // A 20 ms flight time against a 200 ms client->relay leg is impossible
        // for a whole path: the reply cannot have made the round trip.
        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(SERVER, Some(relay(1)));
        clock.advance(Duration::from_millis(20));
        assert_eq!(t.record_inbound_from(SERVER, Some(relay(1))), None);

        assert_eq!(t.relayed_samples(), 0, "nothing may be stored or clamped");
        assert_eq!(t.relayed_p50_ms(), None);
        assert_eq!(t.probe_counters().rejected_behind_floor, 1);
        assert_eq!(t.probe_counters().accepted, 0);
        assert_eq!(t.probe_counters().rejected(), 1, "counted, not silent");
    }

    #[test]
    fn a_reply_after_a_relay_change_is_rejected() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 50.0);
        t.record_client_relay_rtt(relay(2), 50.0);

        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(SERVER, Some(relay(1)));
        clock.advance(Duration::from_millis(120));
        // The session moved to relay 2 before the reply came back: the sample
        // would mix two paths.
        assert_eq!(t.record_inbound_from(SERVER, Some(relay(2))), None);
        assert_eq!(t.relayed_samples(), 0);
        assert_eq!(t.probe_counters().rejected_relay_changed, 1);
    }

    #[test]
    fn a_reply_after_the_probe_window_is_rejected_as_a_timeout() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 50.0);

        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(SERVER, Some(relay(1)));
        clock.advance(PROBE_WINDOW + Duration::from_millis(1));
        // Still inside CLIENT_RELAY_TTL, so this can only be the window rule.
        assert_eq!(t.record_inbound_from(SERVER, Some(relay(1))), None);
        assert_eq!(t.relayed_samples(), 0);
        assert_eq!(t.probe_counters().rejected_timeout, 1);
    }

    #[test]
    fn an_unanswered_probe_expires_and_is_counted_once() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(SERVER, Some(relay(1)));
        clock.advance(PROBE_WINDOW + Duration::from_millis(1));
        // The next outbound notices the unanswered probe is dead.
        t.note_outbound_from(SERVER, Some(relay(1)));
        assert_eq!(t.probe_counters().rejected_timeout, 1);
        // A later inbound must not pair with it nor count it again.
        clock.advance(Duration::from_millis(10));
        assert_eq!(t.record_inbound_from(SERVER, Some(relay(1))), None);
        assert_eq!(t.probe_counters().rejected_timeout, 1);
        assert_eq!(t.relayed_samples(), 0);
    }

    #[test]
    fn a_probe_without_a_keepalive_floor_is_rejected_as_stale() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        // No keepalive sample ever arrived, so the floor cannot be checked.
        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(SERVER, Some(relay(1)));
        clock.advance(Duration::from_millis(300));
        assert_eq!(t.record_inbound_from(SERVER, Some(relay(1))), None);
        assert_eq!(t.relayed_samples(), 0);
        assert_eq!(
            t.probe_counters().rejected_stale,
            1,
            "no L1 means reject, never clip the floor"
        );
    }

    #[test]
    fn an_expired_keepalive_floor_is_stale_too() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 100.0);
        clock.advance(CLIENT_RELAY_TTL + Duration::from_millis(1));

        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(SERVER, Some(relay(1)));
        clock.advance(Duration::from_millis(300));
        assert_eq!(t.record_inbound_from(SERVER, Some(relay(1))), None);
        assert_eq!(t.probe_counters().rejected_stale, 1);
    }

    #[test]
    fn a_probe_armed_without_a_relay_is_rejected_as_stale() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 50.0);

        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(SERVER, None);
        clock.advance(Duration::from_millis(120));
        assert_eq!(t.record_inbound_from(SERVER, Some(relay(1))), None);
        assert_eq!(
            t.probe_counters().rejected_stale,
            1,
            "an unattributable relay must not be assumed unchanged"
        );
    }

    #[test]
    fn the_relayed_median_is_over_the_validated_ring() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        for rtt_ms in [400u64, 200, 300] {
            accept_sample(&t, &clock, SERVER, relay(1), 100.0, rtt_ms);
        }
        assert_eq!(t.relayed_samples(), 3);
        assert_eq!(
            t.relayed_p50_ms(),
            Some(300.0),
            "the median of the validated window, not the newest or the fastest"
        );
    }

    #[test]
    fn changing_relay_starts_a_new_relayed_window() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        accept_sample(&t, &clock, SERVER, relay(1), 100.0, 200);
        accept_sample(&t, &clock, SERVER, relay(1), 100.0, 220);
        assert_eq!(t.relayed_samples(), 2);

        accept_sample(&t, &clock, SERVER, relay(2), 100.0, 300);
        assert_eq!(
            t.relayed_samples(),
            1,
            "samples from two relays must not share one median"
        );
        assert_eq!(t.relayed_p50_ms(), Some(300.0));
    }

    #[test]
    fn changing_server_starts_a_new_relayed_window() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        accept_sample(&t, &clock, SERVER, relay(1), 100.0, 200);
        accept_sample(&t, &clock, OTHER_SERVER, relay(1), 100.0, 400);
        assert_eq!(t.relayed_samples(), 1);
        assert_eq!(t.relayed_p50_ms(), Some(400.0));
    }

    #[test]
    fn the_old_busy_flow_pairing_no_longer_feeds_the_reported_figure() {
        // The removed estimator paired a reply with the most recent outbound on
        // any flow, which measured one inter-send gap (7.33-34.30 ms against a
        // 200+ ms relay leg live). Such a reply must now produce nothing at all.
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 200.0);

        // A busy exchange: sends every 25 ms, replies interleaved.
        t.record_inbound_from(SERVER, Some(relay(1)));
        for _ in 0..4 {
            t.note_outbound_from(SERVER, Some(relay(1)));
            clock.advance(Duration::from_millis(25));
            t.record_inbound_from(SERVER, Some(relay(1)));
            clock.advance(Duration::from_millis(5));
        }

        assert_eq!(
            t.relayed_p50_ms(),
            None,
            "a busy flow has no attributable round trip"
        );
        assert_eq!(t.relayed_samples(), 0);
        assert_eq!(t.saved_ms(), None);
        assert_eq!(
            t.probe_counters(),
            ProbeCounters::default(),
            "none of those replies was a probe, so none is a rejection"
        );
    }

    #[test]
    fn a_flow_that_goes_quiet_after_busy_traffic_still_produces_a_sample() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 120.0);

        // Busy first: nothing attributable.
        t.record_inbound_from(SERVER, Some(relay(1)));
        for _ in 0..3 {
            t.note_outbound_from(SERVER, Some(relay(1)));
            clock.advance(Duration::from_millis(25));
            t.record_inbound_from(SERVER, Some(relay(1)));
            clock.advance(Duration::from_millis(5));
        }
        assert_eq!(t.relayed_samples(), 0);

        // Then the quiet gap: the next send is armed and its reply measured.
        let sample = accept_sample(&t, &clock, SERVER, relay(1), 120.0, 260);
        assert!(sample.is_some());
        assert_eq!(t.relayed_samples(), 1);
        assert_eq!(t.probe_counters().accepted, 1);
        assert_eq!(t.probe_counters().rejected(), 0);
    }

    // ── The `--diagnose` seam is a separate window ──────────────────────────

    #[test]
    fn a_diagnostic_sample_never_appears_in_the_game_traffic_window() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        t.record_diagnostic_rtt(SERVER, 180.0);
        t.record_diagnostic_rtt(SERVER, 220.0);
        assert_eq!(
            t.diagnostic_relayed_p50_ms(),
            Some(220.0),
            "the median reducer used by every window (nearest-rank)"
        );
        assert_eq!(t.diagnostic_relayed_samples(), 2);
        assert_eq!(
            t.relayed_p50_ms(),
            None,
            "a diagnostic value must never be presented as game latency"
        );
        assert_eq!(t.relayed_samples(), 0);
        assert_eq!(t.saved_ms(), None);
    }

    #[test]
    fn a_game_traffic_sample_never_appears_in_the_diagnostic_window() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        accept_sample(&t, &clock, SERVER, relay(1), 100.0, 250);
        assert_eq!(t.relayed_p50_ms(), Some(250.0));
        assert_eq!(t.diagnostic_relayed_p50_ms(), None);
        assert_eq!(t.diagnostic_relayed_samples(), 0);
    }

    #[test]
    fn the_diagnostic_window_is_per_server_and_ignores_implausible_values() {
        let t = tracker(
            Arc::new(ScriptedProber::new(vec![])),
            Arc::new(TestClock::new()),
        );
        t.record_diagnostic_rtt(SERVER, 100.0);
        t.record_diagnostic_rtt(SERVER, f32::NAN);
        t.record_diagnostic_rtt(SERVER, -5.0);
        t.record_diagnostic_rtt(SERVER, 0.0);
        assert_eq!(t.diagnostic_relayed_samples(), 1);

        t.record_diagnostic_rtt(OTHER_SERVER, 300.0);
        assert_eq!(
            t.diagnostic_relayed_samples(),
            1,
            "a new server starts a fresh diagnostic window"
        );
        assert_eq!(t.diagnostic_relayed_p50_ms(), Some(300.0));
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

        clock.advance(SHADOW_SAMPLE_INTERVAL - Duration::from_secs(1));
        assert!(
            !t.record_shadow_outbound(SERVER),
            "still inside the interval"
        );

        clock.advance(Duration::from_secs(1));
        assert!(
            t.record_shadow_outbound(SERVER),
            "due again at the interval"
        );

        assert!(
            t.record_shadow_outbound(OTHER_SERVER),
            "each server has its own sampling slot"
        );
    }

    #[test]
    fn a_shadow_sample_attempted_before_any_icmp_floor_is_not_accepted() {
        // Live defect (task-10 ROB RUN session, 2026-11-16T08:44:56Z): the
        // sampler accepted 0.4248 ms against a direct ICMP RTT of 283.47 ms
        // because its only sanity filter is the `0.5 x ICMP` fraction, and no
        // burst had finished yet, so the first sample had no floor to check.
        // A collapsed median makes `saved_app = direct_app - relayed` negative
        // by construction, so a sample that cannot be physically validated
        // must never enter the window.
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());

        assert!(t.record_shadow_outbound(SERVER));
        clock.advance(Duration::from_millis(300));
        t.record_shadow_inbound(SERVER);

        assert_eq!(
            t.shadow_direct_samples(),
            0,
            "a shadow sample with no measured ICMP floor cannot be validated"
        );
        assert_eq!(
            t.shadow_direct_p50_ms(),
            None,
            "no unvalidated app RTT may be reported"
        );
        assert_eq!(
            t.shadow_counters(),
            ShadowCounters {
                rejected_no_icmp_floor: 1,
                ..ShadowCounters::default()
            },
            "the floorless sample must be counted by reason, not silently dropped"
        );
    }

    #[test]
    fn a_shadow_sample_inside_the_physical_floor_is_accepted() {
        // The positive path: a direct path measured at 100 ms makes a
        // game-packet round trip of 120 ms plausible, so it enters the window.
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 100.0);

        assert!(sample_shadow(
            &t,
            &clock,
            SERVER,
            Duration::from_millis(120)
        ));
        assert_eq!(t.shadow_direct_p50_ms(), Some(120.0));
        assert_eq!(
            t.shadow_counters(),
            ShadowCounters {
                accepted: 1,
                ..ShadowCounters::default()
            }
        );
    }

    #[test]
    fn the_live_collapsed_sample_is_rejected_once_the_icmp_floor_exists() {
        // The same live sample (0.4248 ms) arriving after the 283.47 ms direct
        // burst completed: the absolute floor and the ICMP-relative rule both
        // reject it, and nothing is stored or clamped.
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 283.47);

        assert!(sample_shadow(
            &t,
            &clock,
            SERVER,
            Duration::from_micros(425)
        ));
        assert_eq!(t.shadow_direct_samples(), 0);
        assert_eq!(t.shadow_direct_p50_ms(), None);
        assert_eq!(
            t.shadow_counters().rejected_behind_floor,
            1,
            "a sub-millisecond pairing error is behind the direct-path floor"
        );
        assert_eq!(t.shadow_counters().accepted, 0);
    }

    #[test]
    fn a_sub_millisecond_sample_is_rejected_even_with_a_tiny_icmp_floor() {
        // The absolute floor stands on its own: with a 1.0 ms direct path the
        // ICMP-relative rule would accept a 0.8 ms sample, but no round trip to
        // a public game server over the internet can be that fast.
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 1.0);

        assert!(sample_shadow(
            &t,
            &clock,
            SERVER,
            Duration::from_micros(800)
        ));
        assert_eq!(
            t.shadow_direct_samples(),
            0,
            "a sub-millisecond sample was answered locally, not by the server"
        );
        assert_eq!(t.shadow_counters().rejected_behind_floor, 1);
    }

    #[test]
    fn a_shadow_sample_with_a_changed_relay_is_rejected() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 100.0);

        assert!(t.record_shadow_outbound_from(SERVER, Some(relay(1))));
        clock.advance(Duration::from_millis(120));
        t.record_shadow_inbound_from(SERVER, Some(relay(2)));

        assert_eq!(
            t.shadow_direct_samples(),
            0,
            "a sample spanning a relay switch would mix two paths"
        );
        assert_eq!(t.shadow_counters().rejected_stale_relay, 1);

        // A sample whose relay is unknown at both ends is not a change: it can
        // still be measured locally, it just cannot become a relayed-path pair.
        clock.advance(SHADOW_SAMPLE_INTERVAL);
        assert!(sample_shadow(
            &t,
            &clock,
            SERVER,
            Duration::from_millis(120)
        ));
        assert_eq!(t.shadow_direct_samples(), 1);
    }

    #[test]
    fn shadow_sampling_does_not_disturb_the_validated_window() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 10.0);

        t.record_client_relay_rtt(relay(1), 10.0);
        clock.advance(PROBE_QUIET_GAP);
        t.note_outbound_from(SERVER, Some(relay(1)));
        assert!(t.record_shadow_outbound(SERVER));

        clock.advance(Duration::from_millis(25));
        t.record_inbound_from(SERVER, Some(relay(1)));
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
        let t = tracker_with_direct_floor(&clock, 10.0);

        for rtt_ms in [10u64, 30, 20] {
            assert!(sample_shadow(
                &t,
                &clock,
                SERVER,
                Duration::from_millis(rtt_ms)
            ));
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
        assert_eq!(
            t.shadow_counters().rejected_timeout,
            1,
            "a sample that never got a timely reply is a counted timeout"
        );
    }

    #[test]
    fn shadow_stale_pending_is_cleared_for_the_next_sample() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 10.0);

        assert!(t.record_shadow_outbound(SERVER));
        clock.advance(SHADOW_SAMPLE_INTERVAL + SHADOW_DIRECT_TIMEOUT + Duration::from_millis(1));

        assert!(
            t.record_shadow_outbound(SERVER),
            "the stale pending must not block the next sample"
        );
        assert_eq!(
            t.shadow_counters().rejected_timeout,
            1,
            "the expired pending is counted once, not silently replaced"
        );
        clock.advance(Duration::from_millis(40));
        t.record_shadow_inbound(SERVER);

        assert_eq!(t.shadow_direct_p50_ms(), Some(40.0));
    }

    #[test]
    fn shadow_median_expires_after_ttl() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 10.0);

        assert!(sample_shadow(&t, &clock, SERVER, Duration::from_millis(15)));
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
        assert_eq!(
            t.shadow_counters(),
            ShadowCounters::default(),
            "an inbound with no pending sample is unattributable, not a rejection"
        );
    }

    #[test]
    fn an_unvalidated_shadow_window_is_not_published() {
        // The live-defect shape: a relayed window exists, but every shadow
        // attempt was rejected (here: no ICMP floor yet). The app-level pair
        // must stay absent, so the relay has nothing to subtract.
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        for _ in 0..4 {
            accept_sample(&t, &clock, SERVER, relay(1), 10.0, 25);
        }
        assert!(sample_shadow(
            &t,
            &clock,
            SERVER,
            Duration::from_millis(300)
        ));
        assert_eq!(t.shadow_counters().accepted, 0);
        assert_eq!(t.shadow_counters().rejected_no_icmp_floor, 1);

        assert_eq!(
            t.app_saving_report(),
            None,
            "an unvalidated shadow term must not be published"
        );
        assert_eq!(t.saved_app_pairs(), 0);
        assert_eq!(t.shadow_direct_p50_ms(), None);
    }

    #[test]
    fn an_app_pair_without_a_relayed_window_is_not_published() {
        // A validated direct-app median on its own is still not a saving:
        // without the validated relayed term the report must omit the app pair,
        // or the relay would subtract whatever relayed figure it happens to
        // have and book a saving the client could not measure.
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 40.0);
        assert!(sample_shadow(&t, &clock, SERVER, Duration::from_millis(80)));
        assert_eq!(t.shadow_direct_p50_ms(), Some(80.0));
        assert_eq!(t.relayed_p50_ms(), None);

        assert_eq!(
            t.app_saving_report(),
            None,
            "a direct-app median alone must never be published as an app pair"
        );
        assert_eq!(t.saved_app_pairs(), 0);
    }

    #[test]
    fn a_validated_app_pair_is_published_with_its_pair_count() {
        let clock = Arc::new(TestClock::new());
        // Direct path 40 ms, shadow samples 80 ms: validated.
        let t = tracker_with_direct_floor(&clock, 40.0);
        for _ in 0..3 {
            assert!(sample_shadow(&t, &clock, SERVER, Duration::from_millis(80)));
            clock.advance(Duration::from_secs(6));
        }
        for _ in 0..4 {
            accept_sample(&t, &clock, SERVER, relay(1), 10.0, 25);
        }

        assert_eq!(
            t.app_saving_report(),
            Some(AppSavingReport {
                direct_app_p50_ms: 80.0,
                pairs: 3,
            }),
            "both valid terms publish together, weighted by the smaller window"
        );
        assert_eq!(t.saved_app_pairs(), 3);
    }

    #[test]
    fn a_stale_shadow_window_is_not_published() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 40.0);
        assert!(sample_shadow(&t, &clock, SERVER, Duration::from_millis(80)));
        accept_sample(&t, &clock, SERVER, relay(1), 10.0, 25);
        assert!(t.app_saving_report().is_some());

        clock.advance(SHADOW_DIRECT_TTL);
        assert_eq!(
            t.app_saving_report(),
            None,
            "a stale shadow median must fail the publication gate"
        );
        assert_eq!(t.saved_app_pairs(), 0);
    }

    #[test]
    fn paired_sample_count_matches_samples_and_drains_on_commit() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floors(&clock, &[(SERVER, 10.0), (OTHER_SERVER, 10.0)]);

        for rtt_ms in [10u64, 20, 30] {
            assert!(sample_shadow(
                &t,
                &clock,
                SERVER,
                Duration::from_millis(rtt_ms)
            ));
            clock.advance(Duration::from_secs(30));
        }
        for _ in 0..4 {
            accept_sample(&t, &clock, SERVER, relay(1), 10.0, 25);
        }
        assert_eq!(
            t.saved_app_pairs(),
            3,
            "the pair count is the smaller of the two single-server windows"
        );

        assert!(sample_shadow(
            &t,
            &clock,
            OTHER_SERVER,
            Duration::from_millis(40)
        ));
        assert_eq!(
            t.saved_app_pairs(),
            0,
            "direct-app and relayed windows from different servers are not pairs"
        );

        assert!(sample_shadow(&t, &clock, SERVER, Duration::from_millis(10)));
        assert_eq!(
            t.saved_app_pairs(),
            1,
            "a fresh same-server pair is counted"
        );

        t.commit_report_values();
        assert_eq!(t.saved_app_pairs(), 0, "commit drains the paired window");
        assert_eq!(t.shadow_direct_samples(), 0, "the shadow window drains too");
        assert_eq!(t.relayed_samples(), 0, "the relayed window drains");
        assert_eq!(
            t.shadow_direct_p50_ms(),
            None,
            "no direct-app median is reused after commit"
        );
    }

    // ── Like-for-like bypass pairing ────────────────────────────────────────

    /// A validated whole-path sample and a shadow-direct sample for the same
    /// server, which is the pair the bypass gate compares.
    fn paired_windows(
        t: &LatencyTracker,
        clock: &TestClock,
        server: Ipv4Addr,
        relay_ms: u64,
        shadow_ms: u64,
    ) {
        accept_sample(t, clock, server, relay(1), 10.0, relay_ms);
        assert!(t.record_shadow_outbound(server));
        clock.advance(Duration::from_millis(shadow_ms));
        t.record_shadow_inbound(server);
    }

    #[test]
    fn a_shadow_sample_faster_than_the_direct_path_is_discarded() {
        // Live case: a 6.4 ms "round trip" to a server whose ICMP RTT is 299 ms.
        // On a streaming flow the next inbound datagram is usually not the reply,
        // so the pairing times one inter-packet gap. That must never reach the
        // window, where it would claim direct beats the relay by 290 ms.
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 299.0);

        assert!(t.record_shadow_outbound(SERVER));
        clock.advance(Duration::from_millis(6));
        t.record_shadow_inbound(SERVER);
        assert_eq!(
            t.shadow_direct_samples(),
            0,
            "6 ms against a 299 ms path is a pairing error, not a sample"
        );
        assert_eq!(t.shadow_counters().rejected_behind_floor, 1);

        // A plausible one still lands.
        clock.advance(SHADOW_SAMPLE_INTERVAL);
        assert!(sample_shadow(
            &t,
            &clock,
            SERVER,
            Duration::from_millis(300)
        ));
        assert_eq!(t.shadow_direct_samples(), 1);
        assert_eq!(t.shadow_counters().accepted, 1);
    }

    #[test]
    fn like_for_like_pairs_same_server_shadow_and_relayed_windows() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 40.0);

        // A comparison needs a real window on both sides, not one sample.
        for i in 0..MIN_COMPARABLE_SAMPLES {
            paired_windows(&t, &clock, SERVER, 40 + i as u64, 80 + i as u64);
            clock.advance(Duration::from_secs(6));
        }

        let paired = t
            .like_for_like()
            .expect("same-server shadow and relayed windows must pair");
        assert_eq!(paired.server, SERVER);
        assert_eq!(paired.relay_samples, MIN_COMPARABLE_SAMPLES as u32);
        assert_eq!(paired.shadow_samples, MIN_COMPARABLE_SAMPLES as u32);
        assert!(
            paired.direct_app_p50_ms > paired.relay_app_p50_ms,
            "the shadow-direct median must describe the slower direct path here"
        );
    }

    #[test]
    fn like_for_like_rejects_a_single_server_pairing() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floors(&clock, &[(SERVER, 40.0), (OTHER_SERVER, 40.0)]);

        // A valid relayed window for SERVER...
        accept_sample(&t, &clock, SERVER, relay(1), 10.0, 60);
        // ...and a shadow window for a different server.
        assert!(sample_shadow(
            &t,
            &clock,
            OTHER_SERVER,
            Duration::from_millis(90)
        ));

        assert_eq!(
            t.like_for_like(),
            None,
            "a shadow window from another server must fail open, not pair"
        );
    }

    #[test]
    fn like_for_like_is_none_without_a_relayed_window() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 40.0);

        assert!(sample_shadow(&t, &clock, SERVER, Duration::from_millis(90)));

        assert_eq!(
            t.like_for_like(),
            None,
            "a shadow window alone must not fabricate a comparison"
        );
    }

    #[test]
    fn like_for_like_is_none_when_the_shadow_window_is_stale() {
        let clock = Arc::new(TestClock::new());
        let t = tracker_with_direct_floor(&clock, 40.0);

        for i in 0..MIN_COMPARABLE_SAMPLES {
            paired_windows(&t, &clock, SERVER, 40 + i as u64, 80 + i as u64);
            clock.advance(Duration::from_secs(6));
        }
        assert!(
            t.like_for_like().is_some(),
            "a fresh pair must be usable before it goes stale"
        );

        clock.advance(SHADOW_DIRECT_TTL);
        assert_eq!(
            t.like_for_like(),
            None,
            "a stale shadow window must fail open to the relay"
        );
    }

    // ── Client -> relay keepalive RTT (bypass pre-gate input) ───────────────

    fn relay(n: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, n), 4434)
    }

    #[test]
    fn client_relay_rtt_is_absent_until_a_keepalive_sample_is_recorded() {
        let t = tracker(
            Arc::new(ScriptedProber::new(vec![])),
            Arc::new(TestClock::new()),
        );
        assert_eq!(
            t.client_relay_p50_ms(relay(1)),
            None,
            "an unmeasured relay must not fabricate a first-hop RTT"
        );
    }

    #[test]
    fn fresh_client_relay_rtt_is_published_as_the_p50() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        for rtt in [30.0, 10.0, 20.0] {
            t.record_client_relay_rtt(relay(1), rtt);
        }
        assert_eq!(t.client_relay_p50_ms(relay(1)), Some(20.0));
    }

    #[test]
    fn stale_client_relay_rtt_expires_and_the_gate_fails_open() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 100.0);
        assert_eq!(t.client_relay_p50_ms(relay(1)), Some(100.0));

        clock.advance(CLIENT_RELAY_TTL + Duration::from_millis(1));
        assert_eq!(
            t.client_relay_p50_ms(relay(1)),
            None,
            "a stale keepalive RTT must not drive the pre-gate"
        );
    }

    #[test]
    fn client_relay_rtt_is_tracked_per_relay() {
        let clock = Arc::new(TestClock::new());
        let t = tracker(Arc::new(ScriptedProber::new(vec![])), clock.clone());
        t.record_client_relay_rtt(relay(1), 15.0);
        assert_eq!(t.client_relay_p50_ms(relay(1)), Some(15.0));
        assert_eq!(
            t.client_relay_p50_ms(relay(2)),
            None,
            "a different relay has no sample of its own"
        );
    }

    #[test]
    fn implausible_client_relay_rtt_is_discarded() {
        let t = tracker(
            Arc::new(ScriptedProber::new(vec![])),
            Arc::new(TestClock::new()),
        );
        t.record_client_relay_rtt(relay(1), 0.0);
        t.record_client_relay_rtt(relay(1), f32::NAN);
        assert_eq!(t.client_relay_p50_ms(relay(1)), None);
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
