//! Client-side traceroute: the server-location probe, not a relay probe.
//!
//! The client's own trace goes to the game server along the same path the game
//! would take, so it says something about **where the server is**, not about how
//! good a relay is. A relay traces a server to measure the second hop
//! (relay→server); that is a different vantage and lives in the relay binary.
//! This module is deliberately server-side-only and never feeds relay ranking.
//!
//! The useful output is the last hop that answers. That hop is the closest
//! network point to the game server the client can observe from its own
//! vantage; its RTT is **not** the game's ping, and its prefix is only a
//! location *hint*. Naming a city would need hop-geolocation data (a licensed
//! database or an owner geofeed), which the product does not have. What this
//! module can add honestly is the provider/hosting network name when the last
//! hop's prefix is already in one of the shipped R1 indexes
//! ([`crate::server_place`]).
//!
//! Mechanics:
//!
//! * spawn the OS tool on a worker thread — `tracert` on Windows (which has no
//!   `-q`; every hop is probed three times) and `traceroute -n -q 1` elsewhere;
//! * bound the run to [`TRACE_DEADLINE`] and kill a child still running at the
//!   deadline, keeping whatever hops it printed before then;
//! * one trace per (server, relay session), with a 30-minute server-keyed cache
//!   and at most one run in flight;
//! * never aim a probe at a private, loopback, link-local, shared or reserved
//!   address;
//! * log one stable [`info_line`] per accepted trace, so the trace log can be
//!   parsed without a screenshot.

use std::collections::HashMap;
use std::io::Read;
use std::net::Ipv4Addr;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{Receiver, TryRecvError};
use std::sync::Arc;
use std::time::{Duration, Instant};

/// How many hops the tool may walk (`tracert -h`, `traceroute -m`).
pub const MAX_HOPS: u8 = 20;

/// Hard wall-clock bound for one tool run; a child still running is killed.
const TRACE_DEADLINE: Duration = Duration::from_secs(30);

/// How long a finished route is reused before it is refreshed. Server location
/// moves far more slowly than a match, so re-tracing on every relay switch
/// would spend probes to learn the same answer.
const CACHE_TTL: Duration = Duration::from_secs(30 * 60);

/// How often the bounded runner checks a child for exit.
const POLL_INTERVAL: Duration = Duration::from_millis(10);

/// One TTL row of a trace: the hop number and, when a probe answered, the
/// address and the best RTT seen at that TTL.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceHop {
    /// TTL (hop number) the probe was sent with.
    pub ttl: u8,
    /// Best measured RTT for this hop, in whole ms.
    pub rtt_ms: Option<u32>,
    /// Address that answered at this TTL; `None` is a `*` row.
    pub ip: Option<Ipv4Addr>,
}

/// What the client can honestly say about the path to the game server.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientRoute {
    /// Number of hops walked: the highest TTL the trace produced a row for.
    pub hops: u8,
    /// The last hop that answered, if any. This is a location hint, not a
    /// measurement of the game server's own RTT.
    pub last_hop: Option<Ipv4Addr>,
    /// That hop's best RTT, in whole ms.
    pub last_hop_rtt_ms: Option<u32>,
    /// Whether a hop answered with the target address: the trace reached the
    /// server rather than stopping at a filter.
    pub complete: bool,
}

/// Why a client trace produced no route.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceFailure {
    /// The platform's trace tool could not be started (not installed, blocked).
    Spawn(String),
    /// The deadline passed and no hop had answered yet; the child was killed.
    TimedOut,
    /// The tool finished without printing a single hop row.
    NoOutput,
    /// Waiting for or reading the child process failed.
    Io(String),
    /// The address is not a legal probe target (private/loopback/reserved).
    Refused,
}

/// What one trace produced, after [`GuiTrace`] collects it.
type TraceResult = Result<ClientRoute, TraceFailure>;

/// Whether `target` is a legal probe target: public unicast IPv4 only.
///
/// A trace must never be aimed at the loopback interface, a LAN address, or a
/// shared/reserved range: such a "game server" is local or private, and its
/// path is neither publishable nor useful as a location hint. Documentation
/// ranges (TEST-NET) stay allowed — they are not special-use at the IP layer —
/// which is also what the fixtures below use.
pub fn probe_allowed(target: Ipv4Addr) -> bool {
    if target.is_unspecified()
        || target.is_loopback()
        || target.is_private()
        || target.is_link_local()
        || target.is_multicast()
        || target.is_broadcast()
    {
        return false;
    }
    let [first, second, ..] = target.octets();
    // "This network" (0.0.0.0/8) and the reserved 240.0.0.0/4 block.
    if first == 0 || first >= 240 {
        return false;
    }
    // Shared address space (RFC 6598), which `is_private` does not cover.
    if first == 100 && (64..=127).contains(&second) {
        return false;
    }
    // Benchmarking ranges (RFC 2544); never a product target.
    if first == 198 && matches!(second, 18 | 19) {
        return false;
    }
    true
}

/// Parse the hop rows out of either OS tool's output.
///
/// Both `tracert` and `traceroute` print one line per TTL whose first token is
/// the hop number, but the rest follows the tool, the probe count and the OS
/// language. The parser therefore classifies tokens instead of matching prose:
///
/// * `*` is a probe with no reply;
/// * an IPv4 literal is the responding address;
/// * a number followed by `ms` (or suffixed with `ms`) is one RTT, and the best
///   of the probes on the line is kept;
/// * anything else (`Request timed out.`, `!H`, `[MPLS: …]`, the header and the
///   `Trace complete.` footer) is ignored.
///
/// A line only becomes a hop when it starts with a hop number **and** carries at
/// least one probe field, so a localized header/footer cannot invent a hop.
/// TTLs must be strictly increasing, so a duplicated or out-of-order line is
/// dropped rather than shortening the path.
fn parse_hops(stdout: &str) -> Vec<TraceHop> {
    let mut hops: Vec<TraceHop> = Vec::new();
    for line in stdout.lines() {
        let mut tokens = line.split_whitespace().peekable();
        let Some(ttl) = tokens.next().and_then(|token| token.parse::<u8>().ok()) else {
            continue;
        };
        if ttl == 0 || ttl > MAX_HOPS {
            continue;
        }
        if hops.last().is_some_and(|previous| previous.ttl >= ttl) {
            continue;
        }
        let mut ip: Option<Ipv4Addr> = None;
        let mut rtt: Option<u32> = None;
        let mut measured = false;
        while let Some(token) = tokens.next() {
            if token == "*" {
                measured = true;
                continue;
            }
            if let Ok(address) = token.parse::<Ipv4Addr>() {
                ip = Some(address);
                measured = true;
                continue;
            }
            let Some(value) = rtt_value(token) else {
                continue;
            };
            let has_unit = token.ends_with("ms") || tokens.peek().is_some_and(|next| *next == "ms");
            if !has_unit {
                // A bare number is not an RTT (the footer and headers contain
                // plenty of them); only a number with its unit counts.
                continue;
            }
            if !token.ends_with("ms") {
                // `10 ms`: consume the separate unit token.
                tokens.next();
            }
            let value = rtt_ms(value);
            rtt = Some(rtt.map_or(value, |best| best.min(value)));
            measured = true;
        }
        if !measured {
            continue;
        }
        hops.push(TraceHop {
            ttl,
            rtt_ms: rtt,
            ip,
        });
    }
    hops
}

/// The numeric value of an RTT token: `10`, `10.4`, `<1` or `10ms`.
fn rtt_value(token: &str) -> Option<f64> {
    let numeric = token.strip_suffix("ms").unwrap_or(token);
    let numeric = numeric.strip_prefix('<').unwrap_or(numeric);
    numeric.parse::<f64>().ok()
}

/// Whole milliseconds, never below 1: a sub-millisecond hop still answered and
/// `0 ms` would read as "no measurement".
fn rtt_ms(value: f64) -> u32 {
    value.round().max(1.0).min(u32::MAX as f64) as u32
}

/// Reduce parsed hop rows to what the route card and the log line need.
///
/// The last hop that answered is the location hint. `*` rows say the path was
/// filtered at that TTL, which is most of a game-server path; they still count
/// towards `hops` because they were walked.
pub fn summarize(target: Ipv4Addr, hops: &[TraceHop]) -> ClientRoute {
    let hop_count = hops.iter().map(|hop| hop.ttl).max().unwrap_or(0);
    let last_answered = hops
        .iter()
        .filter(|hop| hop.ip.is_some())
        .max_by_key(|hop| hop.ttl);
    ClientRoute {
        hops: hop_count,
        last_hop: last_answered.and_then(|hop| hop.ip),
        last_hop_rtt_ms: last_answered.and_then(|hop| hop.rtt_ms),
        // The tool's "Trace complete." footer is not evidence: it prints even
        // when the last TTL was a timeout. Only the target answering proves
        // the trace reached it.
        complete: hops.iter().any(|hop| hop.ip == Some(target)),
    }
}

/// The part of a spawned trace process the bounded runner needs. A trait rather
/// than the concrete [`Child`] so the deadline/kill path is tested against a
/// simulated child, with no process and no network involved.
trait TraceChild {
    /// `Some(exit code)` once the process has finished, `None` while running.
    fn try_wait(&mut self) -> std::io::Result<Option<i32>>;
    /// Kill the process.
    fn kill(&mut self) -> std::io::Result<()>;
    /// Wait for exit and collect the stdout captured so far.
    fn finish(&mut self) -> std::io::Result<(i32, Vec<u8>)>;
}

/// The real child behind [`std::process::Child`], with stdout piped.
struct ProcessChild {
    child: Child,
}

impl TraceChild for ProcessChild {
    fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
        Ok(self
            .child
            .try_wait()?
            .map(|status| status.code().unwrap_or(-1)))
    }

    fn kill(&mut self) -> std::io::Result<()> {
        self.child.kill()
    }

    fn finish(&mut self) -> std::io::Result<(i32, Vec<u8>)> {
        let status = self.child.wait()?;
        let mut stdout = Vec::new();
        if let Some(mut pipe) = self.child.stdout.take() {
            pipe.read_to_end(&mut stdout)?;
        }
        Ok((status.code().unwrap_or(-1), stdout))
    }
}

/// What the bounded runner produced.
#[derive(Debug)]
enum ChildOutcome {
    /// The child finished within the deadline; its captured stdout.
    Finished { stdout: String },
    /// The deadline passed and the child was killed; whatever it had printed.
    TimedOut { stdout: String },
    /// Waiting for or reading the child failed.
    Io(String),
}

/// Wait for `child` up to `limit`, killing it at the deadline.
///
/// Stdout is read only after the wait ends. Trace output is a few hundred bytes
/// — far below the pipe buffer — so the child cannot block on it before exiting;
/// if it did, the deadline would kill it and the partial output would be kept.
fn run_bounded(mut child: Box<dyn TraceChild>, limit: Duration) -> ChildOutcome {
    let deadline = Instant::now() + limit;
    let mut timed_out = false;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) => {}
            Err(error) => {
                let _ = child.kill();
                return ChildOutcome::Io(error.to_string());
            }
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            timed_out = true;
            break;
        }
        std::thread::sleep(POLL_INTERVAL);
    }
    let stdout = match child.finish() {
        Ok((_, bytes)) => String::from_utf8_lossy(&bytes).into_owned(),
        Err(error) => return ChildOutcome::Io(error.to_string()),
    };
    if timed_out {
        ChildOutcome::TimedOut { stdout }
    } else {
        ChildOutcome::Finished { stdout }
    }
}

/// Turn the bounded runner's output into hop rows, keeping a partial trace that
/// printed hops before the deadline and failing only when nothing was printed.
fn hops_from_outcome(outcome: ChildOutcome) -> Result<Vec<TraceHop>, TraceFailure> {
    let (stdout, timed_out) = match outcome {
        ChildOutcome::Finished { stdout } => (stdout, false),
        ChildOutcome::TimedOut { stdout } => (stdout, true),
        ChildOutcome::Io(error) => return Err(TraceFailure::Io(error)),
    };
    let hops = parse_hops(&stdout);
    if hops.is_empty() {
        return Err(if timed_out {
            TraceFailure::TimedOut
        } else {
            TraceFailure::NoOutput
        });
    }
    Ok(hops)
}

/// The platform's trace command for `target`.
///
/// Windows `tracert` has no `-q`: every hop is probed three times, which is why
/// the parser keeps the best RTT of the three. `-w 1000` bounds each probe and
/// the 30 s wall clock bounds the run even when several hops time out (three
/// one-second probes per hop). Elsewhere `traceroute -n -q 1` keeps one numeric
/// probe per hop at the same one-second budget.
#[cfg(windows)]
fn platform_trace_command(target: Ipv4Addr) -> Command {
    let max_hops = MAX_HOPS.to_string();
    let target = target.to_string();
    let mut command = crate::platform::silent_command("tracert");
    command.args(["-d", "-h", max_hops.as_str(), "-w", "1000", target.as_str()]);
    command
}

/// See the Windows variant above.
#[cfg(not(windows))]
fn platform_trace_command(target: Ipv4Addr) -> Command {
    let max_hops = MAX_HOPS.to_string();
    let target = target.to_string();
    let mut command = crate::platform::silent_command("traceroute");
    command.args([
        "-n",
        "-m",
        max_hops.as_str(),
        "-q",
        "1",
        "-w",
        "1",
        target.as_str(),
    ]);
    command
}

/// Run one bounded trace to `target` and return its TTL rows.
///
/// This is the on-demand entry point: it spawns the platform tool, waits at
/// most [`TRACE_DEADLINE`], kills it on overrun and parses what was printed. It
/// blocks its caller for up to the deadline, which is why the GUI runs it on a
/// worker thread.
pub fn trace_hops(target: Ipv4Addr) -> Result<Vec<TraceHop>, TraceFailure> {
    // Defence in depth: the scheduler refuses these too, but the probe must
    // never leave the machine for a private address even if a caller forgets.
    if !probe_allowed(target) {
        return Err(TraceFailure::Refused);
    }
    let mut command = platform_trace_command(target);
    command
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let child = command
        .spawn()
        .map_err(|error| TraceFailure::Spawn(error.to_string()))?;
    let outcome = run_bounded(Box::new(ProcessChild { child }), TRACE_DEADLINE);
    hops_from_outcome(outcome)
}

/// Produces one trace. Production runs the OS tool; tests substitute a
/// controlled runner so cache and single-flight behaviour is deterministic and
/// never touches the network.
trait TraceRunner: Send + Sync {
    fn run(&self, target: Ipv4Addr) -> TraceResult;
}

/// The production runner: the platform tool, bounded and parsed.
struct OsTraceRunner;

impl TraceRunner for OsTraceRunner {
    fn run(&self, target: Ipv4Addr) -> TraceResult {
        let hops = trace_hops(target)?;
        Ok(summarize(target, &hops))
    }
}

/// A server-keyed cache of finished routes, valid for [`CACHE_TTL`].
#[derive(Default)]
struct TraceCache {
    entries: HashMap<Ipv4Addr, (Instant, ClientRoute)>,
}

impl TraceCache {
    fn get(&self, server: Ipv4Addr, now: Instant) -> Option<ClientRoute> {
        let (at, route) = self.entries.get(&server)?;
        (now.duration_since(*at) < CACHE_TTL).then_some(*route)
    }

    fn insert(&mut self, server: Ipv4Addr, route: ClientRoute, now: Instant) {
        // Expire on write so a long-lived window does not accumulate one entry
        // per match forever.
        self.entries
            .retain(|_, (at, _)| now.duration_since(*at) < CACHE_TTL);
        self.entries.insert(server, (now, route));
    }
}

/// A trace that finished since the previous tick, produced exactly once so the
/// GUI logs it exactly once.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AcceptedTrace {
    /// The game server the trace ran against.
    pub server: Ipv4Addr,
    /// What the trace learned.
    pub route: ClientRoute,
}

/// What one scheduler tick produced.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TraceTick {
    /// A fresh route for the locked server, when the cache has one.
    pub route: Option<ClientRoute>,
    /// Set on the tick that collects a finished run; log it exactly once.
    pub accepted: Option<AcceptedTrace>,
}

struct InFlight {
    server: Ipv4Addr,
    session: u64,
    rx: Receiver<TraceResult>,
}

#[derive(Clone, Copy)]
struct Attempted {
    server: Ipv4Addr,
    session: u64,
    at: Instant,
}

/// The GUI's client-trace worker: one run in flight at most, a server-keyed
/// 30-minute cache, and a once-per-(server, relay-session) retry rule.
pub struct GuiTrace {
    runner: Arc<dyn TraceRunner>,
    cache: TraceCache,
    in_flight: Option<InFlight>,
    /// The last attempt for a (server, relay session), so a failure is not
    /// retried on every frame.
    attempted: Option<Attempted>,
}

impl GuiTrace {
    pub fn new() -> Self {
        Self::from_runner(Arc::new(OsTraceRunner))
    }

    fn from_runner(runner: Arc<dyn TraceRunner>) -> Self {
        Self {
            runner,
            cache: TraceCache::default(),
            in_flight: None,
            attempted: None,
        }
    }

    /// Advance one frame: collect a finished run, then start one when the
    /// locked `server` has no fresh route and nothing else is running.
    ///
    /// `session` is the relay-session identity (the engine's keepalive
    /// generation). A finished route is reused for [`CACHE_TTL`] regardless of
    /// the session, because the client→server path does not change with the
    /// relay: re-tracing after every relay switch would learn the same server
    /// location. The session id only bounds retries after a failed run, so one
    /// dead relay session cannot re-spawn the tool on every frame.
    pub fn update(&mut self, server: Option<Ipv4Addr>, session: u64, now: Instant) -> TraceTick {
        // 1. Collect a run that finished since the previous tick.
        let finished = self
            .in_flight
            .as_ref()
            .and_then(|flight| match flight.rx.try_recv() {
                Ok(result) => Some((flight.server, flight.session, result)),
                Err(TryRecvError::Empty) => None,
                Err(TryRecvError::Disconnected) => Some((
                    flight.server,
                    flight.session,
                    Err(TraceFailure::Io(
                        "client trace worker thread disappeared".to_string(),
                    )),
                )),
            });
        let mut accepted = None;
        if let Some((target, run_session, result)) = finished {
            self.in_flight = None;
            self.attempted = Some(Attempted {
                server: target,
                session: run_session,
                at: now,
            });
            match result {
                Ok(route) => {
                    self.cache.insert(target, route, now);
                    accepted = Some(AcceptedTrace {
                        server: target,
                        route,
                    });
                }
                Err(failure) => {
                    tracing::debug!("client trace for {target} produced no route: {failure:?}");
                }
            }
        }

        let tick = |route| TraceTick { route, accepted };

        // 2. Decide what the card may show for the currently locked server.
        let Some(server) = server else {
            return tick(None);
        };
        if !probe_allowed(server) {
            return tick(None);
        }
        if let Some(route) = self.cache.get(server, now) {
            return tick(Some(route));
        }
        if self.in_flight.is_some() {
            return tick(None);
        }
        let retry_blocked = self.attempted.is_some_and(|attempted| {
            attempted.server == server
                && attempted.session == session
                && now.duration_since(attempted.at) < CACHE_TTL
        });
        if retry_blocked {
            return tick(None);
        }

        // 3. Start one, on its own thread. The UI frame never waits for it.
        let (tx, rx) = std::sync::mpsc::channel();
        let runner = Arc::clone(&self.runner);
        let spawned = std::thread::Builder::new()
            .name("client-trace".to_string())
            .spawn(move || {
                let result = runner.run(server);
                // A closed receiver just means the window shut down first.
                let _ = tx.send(result);
            });
        if spawned.is_ok() {
            self.in_flight = Some(InFlight {
                server,
                session,
                rx,
            });
        } else {
            self.attempted = Some(Attempted {
                server,
                session,
                at: now,
            });
            tracing::debug!("client trace for {server} could not be started");
        }
        tick(None)
    }
}

impl Default for GuiTrace {
    fn default() -> Self {
        Self::new()
    }
}

/// The provider/hosting network name for `address` when one of the shipped R1
/// indexes knows its prefix.
///
/// Deliberately no city or region: a provider region from the cloud feed is a
/// fact, but a city for an arbitrary hop needs hop-geolocation data the product
/// does not license. An operator geofeed match names the operator - the same
/// display-only provenance the route card uses - and never a city for a hop.
#[allow(dead_code)] // S5b moves the trace into the core; W2-2 renders the seam's caption
fn network_label(address: Ipv4Addr) -> Option<String> {
    match crate::server_place::place(address) {
        crate::server_place::Place::Cloud { provider, .. } => Some(provider.to_ascii_uppercase()),
        crate::server_place::Place::Geofeed { provider, .. } => Some(provider.to_string()),
        crate::server_place::Place::Hosting { name, .. } => Some(name.to_string()),
        crate::server_place::Place::Country { .. } | crate::server_place::Place::Unknown => None,
    }
}

/// The route-card caption for a finished trace, localized:
///
/// `Route to server: 12 hops · nearest responding hop 23 ms`
///
/// plus the provider/hosting address when the last responding hop's prefix is
/// in one of the shipped indexes. No city is ever appended — that needs hop
/// geolocation data, and this feature deliberately does not claim it.
#[allow(dead_code)] // see network_label: the W2-2 shell renders the seam's caption
pub fn caption(route: &ClientRoute) -> Option<String> {
    if route.hops == 0 {
        return None;
    }
    let hops = route.hops.to_string();
    let mut caption = crate::i18n::t_with("route.trace_route", &[("hops", hops.as_str())]);
    if let Some(rtt) = route.last_hop_rtt_ms {
        let rtt = rtt.to_string();
        caption.push_str(" \u{00b7} ");
        caption.push_str(&crate::i18n::t_with(
            "route.trace_nearest",
            &[("ms", rtt.as_str())],
        ));
    }
    if let Some(name) = route.last_hop.and_then(network_label) {
        caption.push_str(" \u{00b7} ");
        caption.push_str(&name);
    }
    Some(caption)
}

/// The one line logged per accepted trace. The field order is part of the
/// contract:
///
/// `CLIENT_TRACE server=<ip> hops=<n> last_hop=<ip|none> last_hop_rtt_ms=<n|none> complete=<true|false> at_unix_ms=<ms>`
pub fn info_line(server: Ipv4Addr, route: &ClientRoute, at_unix_ms: u128) -> String {
    let last_hop = route
        .last_hop
        .map_or_else(|| "none".to_string(), |ip| ip.to_string());
    let last_hop_rtt_ms = route
        .last_hop_rtt_ms
        .map_or_else(|| "none".to_string(), |ms| ms.to_string());
    format!(
        "CLIENT_TRACE server={server} hops={} last_hop={last_hop} \
         last_hop_rtt_ms={last_hop_rtt_ms} complete={} at_unix_ms={at_unix_ms}",
        route.hops, route.complete
    )
}

/// Milliseconds since the Unix epoch, for the trace log line.
pub fn now_unix_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_millis())
}

/// Parse a [`info_line`] back into its fields; the round-trip test proves the
/// formatter and the documented contract stay identical.
#[cfg(test)]
fn parse_info_line(line: &str) -> Option<(Ipv4Addr, ClientRoute, u128)> {
    let rest = line.strip_prefix("CLIENT_TRACE ")?;
    let mut server = None;
    let mut route = ClientRoute {
        hops: 0,
        last_hop: None,
        last_hop_rtt_ms: None,
        complete: false,
    };
    let mut at_unix_ms = None;
    for field in rest.split(' ') {
        let (key, value) = field.split_once('=')?;
        match key {
            "server" => server = Some(value.parse().ok()?),
            "hops" => route.hops = value.parse().ok()?,
            "last_hop" => {
                route.last_hop = if value == "none" {
                    None
                } else {
                    Some(value.parse().ok()?)
                }
            }
            "last_hop_rtt_ms" => {
                route.last_hop_rtt_ms = if value == "none" {
                    None
                } else {
                    Some(value.parse().ok()?)
                }
            }
            "complete" => route.complete = value.parse().ok()?,
            "at_unix_ms" => at_unix_ms = Some(value.parse().ok()?),
            // Fail closed on an unknown field: the contract is stable.
            _ => return None,
        }
    }
    Some((server?, route, at_unix_ms?))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::mpsc::channel;
    use std::sync::Mutex;

    fn ip(text: &str) -> Ipv4Addr {
        text.parse().expect("test address")
    }

    fn route_fixture() -> ClientRoute {
        ClientRoute {
            hops: 4,
            last_hop: Some(ip("203.0.113.9")),
            last_hop_rtt_ms: Some(23),
            complete: false,
        }
    }

    // ── Parser ─────────────────────────────────────────────────────────────

    /// A real-format `tracert -d -h 20 -w 1000` capture: three probes per hop,
    /// a `*`/`Request timed out.` row, and the header/footer lines.
    const TRACERT_OUTPUT: &str = "\
Tracing route to 192.0.2.10 over a maximum of 20 hops

  1    <1 ms    <1 ms    <1 ms  10.20.30.1 
  2     4 ms     5 ms     4 ms  198.51.100.1 
  3     *        *        *     Request timed out.
  4     8 ms     9 ms     8 ms  203.0.113.1 
  5    12 ms    13 ms    12 ms  192.0.2.10 

Trace complete.
";

    /// A real-format `traceroute -n -m 20 -q 1 -w 1` capture, including a
    /// filtered hop (`*`) and an annotation (`!H`) the parser must ignore.
    const TRACEROUTE_OUTPUT: &str = "\
traceroute to 192.0.2.10 (192.0.2.10), 20 hops max, 60 byte packets
 1  10.20.30.1  0.512 ms
 2  * 
 3  198.51.100.1  7.004 ms !H
 4  192.0.2.10  11.842 ms
";

    /// Every TTL timed out: walked hops, but no responding hop at all.
    const TRACERT_ALL_TIMEOUTS: &str = "\
Tracing route to 192.0.2.10 over a maximum of 3 hops

  1     *        *        *     Request timed out.
  2     *        *        *     Request timed out.
  3     *        *        *     Request timed out.

Trace complete.
";

    #[test]
    fn tracert_output_parses_rtts_stars_and_trailing_lines() {
        let hops = parse_hops(TRACERT_OUTPUT);
        assert_eq!(
            hops.len(),
            5,
            "header/footer must not parse as hops: {hops:?}"
        );
        assert_eq!(
            hops[0],
            TraceHop {
                ttl: 1,
                rtt_ms: Some(1),
                ip: Some(ip("10.20.30.1"))
            }
        );
        assert_eq!(
            hops[2],
            TraceHop {
                ttl: 3,
                rtt_ms: None,
                ip: None
            },
            "a `*`/`Request timed out.` row is a walked hop with no reply"
        );
        assert_eq!(
            hops[4],
            TraceHop {
                ttl: 5,
                rtt_ms: Some(12),
                ip: Some(ip("192.0.2.10"))
            },
            "the best of the three probes is kept"
        );
    }

    #[test]
    fn traceroute_output_parses_numeric_hop_lines() {
        let hops = parse_hops(TRACEROUTE_OUTPUT);
        assert_eq!(hops.len(), 4, "the header must not parse: {hops:?}");
        // `0.512 ms` is whole-millisecond 1, never 0.
        assert_eq!(hops[0].rtt_ms, Some(1));
        assert_eq!(hops[1].rtt_ms, None);
        assert_eq!(hops[1].ip, None);
        assert_eq!(hops[2].ip, Some(ip("198.51.100.1")));
        assert_eq!(
            hops[3],
            TraceHop {
                ttl: 4,
                rtt_ms: Some(12),
                ip: Some(ip("192.0.2.10"))
            }
        );
    }

    #[test]
    fn a_hop_keeps_the_best_rtt_of_its_probes() {
        let hops = parse_hops("  1    17 ms    24 ms     9 ms  203.0.113.7 \n");
        assert_eq!(
            hops,
            vec![TraceHop {
                ttl: 1,
                rtt_ms: Some(9),
                ip: Some(ip("203.0.113.7"))
            }]
        );
    }

    #[test]
    fn malformed_and_out_of_order_lines_are_dropped() {
        // A localized header that starts with a number but has no probe field,
        // and a duplicate TTL, must not change the path.
        let hops = parse_hops(
            "20 Hops maximal\n  1    <1 ms  10.20.30.1\n  1     2 ms  10.20.30.1\n  2     *\n",
        );
        assert_eq!(hops.len(), 2, "{hops:?}");
        assert_eq!(hops[0].ttl, 1);
        assert_eq!(hops[1].ttl, 2);
    }

    #[test]
    fn the_last_responding_hop_is_the_location_hint() {
        let route = summarize(ip("192.0.2.10"), &parse_hops(TRACERT_OUTPUT));
        assert_eq!(route.hops, 5);
        assert_eq!(route.last_hop, Some(ip("192.0.2.10")));
        assert_eq!(route.last_hop_rtt_ms, Some(12));
        assert!(route.complete);
    }

    #[test]
    fn an_incomplete_trace_uses_the_last_hop_that_answered() {
        // The target never answers; the trace stops at 203.0.113.1.
        let hops = parse_hops("  1    <1 ms  10.20.30.1\n  2     8 ms  203.0.113.1\n  3     *\n");
        let route = summarize(ip("192.0.2.10"), &hops);
        assert_eq!(route.hops, 3);
        assert_eq!(route.last_hop, Some(ip("203.0.113.1")));
        assert_eq!(route.last_hop_rtt_ms, Some(8));
        assert!(!route.complete);
    }

    #[test]
    fn an_all_timeout_trace_has_hops_but_no_location_hint() {
        let route = summarize(ip("192.0.2.10"), &parse_hops(TRACERT_ALL_TIMEOUTS));
        assert_eq!(route.hops, 3);
        assert_eq!(route.last_hop, None);
        assert_eq!(route.last_hop_rtt_ms, None);
        assert!(!route.complete);
    }

    // ── Refusal ────────────────────────────────────────────────────────────

    #[test]
    fn private_loopback_and_reserved_targets_are_refused() {
        for target in [
            "0.0.0.0",
            "127.0.0.1",
            "10.0.0.1",
            "172.16.5.4",
            "192.168.1.1",
            "169.254.1.1",
            "100.64.0.1",
            "198.18.0.1",
            "224.0.0.1",
            "240.0.0.1",
            "255.255.255.255",
        ] {
            assert!(!probe_allowed(ip(target)), "{target} must be refused");
        }
    }

    #[test]
    fn public_unicast_targets_are_allowed() {
        for target in ["8.8.8.8", "3.237.221.191", "192.0.2.1", "203.0.113.9"] {
            assert!(probe_allowed(ip(target)), "{target} must be allowed");
        }
    }

    // ── Deadline / kill ────────────────────────────────────────────────────

    #[derive(Default)]
    struct FakeChildState {
        polls: u32,
        killed: bool,
        stdout: Vec<u8>,
    }

    struct FakeChild {
        state: Arc<Mutex<FakeChildState>>,
        exit_after: Option<u32>,
    }

    impl TraceChild for FakeChild {
        fn try_wait(&mut self) -> std::io::Result<Option<i32>> {
            let mut state = self.state.lock().expect("fake child lock");
            state.polls += 1;
            Ok(match self.exit_after {
                Some(exit_after) if state.polls >= exit_after => Some(0),
                _ => None,
            })
        }

        fn kill(&mut self) -> std::io::Result<()> {
            self.state.lock().expect("fake child lock").killed = true;
            Ok(())
        }

        fn finish(&mut self) -> std::io::Result<(i32, Vec<u8>)> {
            let state = self.state.lock().expect("fake child lock");
            Ok((0, state.stdout.clone()))
        }
    }

    fn fake_child(
        stdout: &str,
        exit_after: Option<u32>,
    ) -> (Box<FakeChild>, Arc<Mutex<FakeChildState>>) {
        let state = Arc::new(Mutex::new(FakeChildState {
            polls: 0,
            killed: false,
            stdout: stdout.as_bytes().to_vec(),
        }));
        (
            Box::new(FakeChild {
                state: Arc::clone(&state),
                exit_after,
            }),
            state,
        )
    }

    #[test]
    fn a_child_that_overruns_the_deadline_is_killed() {
        let (child, state) = fake_child("  1    <1 ms  10.20.30.1\n", None);
        let started = Instant::now();
        let outcome = run_bounded(child, Duration::from_millis(60));
        assert!(
            matches!(outcome, ChildOutcome::TimedOut { .. }),
            "expected a timeout, got {outcome:?}"
        );
        assert!(
            state.lock().expect("fake child lock").killed,
            "the overrunning child must be killed"
        );
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "the deadline must bound the wait"
        );
    }

    #[test]
    fn a_child_that_exits_is_read_to_completion() {
        let (child, _state) = fake_child("  1    <1 ms  10.20.30.1\n", Some(2));
        let outcome = run_bounded(child, Duration::from_secs(1));
        match outcome {
            ChildOutcome::Finished { stdout } => {
                assert_eq!(stdout, "  1    <1 ms  10.20.30.1\n");
            }
            other => panic!("expected a finished child, got {other:?}"),
        }
    }

    #[test]
    fn a_timed_out_partial_trace_still_yields_its_hops() {
        let hops = hops_from_outcome(ChildOutcome::TimedOut {
            stdout: TRACERT_ALL_TIMEOUTS.to_string(),
        })
        .expect("a partial trace keeps its hops");
        assert_eq!(hops.len(), 3);
        assert_eq!(summarize(ip("192.0.2.10"), &hops).last_hop, None);
    }

    #[test]
    fn a_trace_with_no_hop_rows_is_a_failure() {
        assert_eq!(
            hops_from_outcome(ChildOutcome::Finished {
                stdout: "Tracing route\n\nTrace complete.\n".to_string()
            }),
            Err(TraceFailure::NoOutput)
        );
        assert_eq!(
            hops_from_outcome(ChildOutcome::TimedOut {
                stdout: String::new()
            }),
            Err(TraceFailure::TimedOut)
        );
    }

    // ── Cache ──────────────────────────────────────────────────────────────

    #[test]
    fn a_fresh_route_is_reused_for_thirty_minutes() {
        let now = Instant::now();
        let mut cache = TraceCache::default();
        let route = route_fixture();
        cache.insert(ip("192.0.2.10"), route, now);
        assert_eq!(cache.get(ip("192.0.2.10"), now), Some(route));
        assert_eq!(
            cache.get(ip("192.0.2.10"), now + Duration::from_secs(29 * 60)),
            Some(route),
            "a route inside the TTL must be reused"
        );
        assert_eq!(
            cache.get(ip("192.0.2.10"), now + Duration::from_secs(31 * 60)),
            None,
            "a route past the TTL must be refreshed"
        );
        assert_eq!(
            cache.get(ip("203.0.113.40"), now),
            None,
            "one server's route is not another server's"
        );
    }

    // ── Scheduler ──────────────────────────────────────────────────────────

    /// A runner the tests control: it counts calls, can block on a gate, and
    /// returns a fixed result. Keeps every scheduler test off the network.
    struct FakeRunner {
        result: Mutex<Option<TraceResult>>,
        calls: Arc<AtomicUsize>,
        finished: Arc<AtomicUsize>,
        gate: Option<Mutex<Receiver<()>>>,
    }

    impl TraceRunner for FakeRunner {
        fn run(&self, _target: Ipv4Addr) -> TraceResult {
            self.calls.fetch_add(1, Ordering::SeqCst);
            if let Some(gate) = &self.gate {
                let receiver = gate.lock().expect("gate lock");
                let _ = receiver.recv();
            }
            let result = self
                .result
                .lock()
                .expect("fake result lock")
                .clone()
                .unwrap_or(Err(TraceFailure::NoOutput));
            self.finished.fetch_add(1, Ordering::SeqCst);
            result
        }
    }

    fn fake_runner(result: TraceResult) -> (Arc<FakeRunner>, Arc<AtomicUsize>, Arc<AtomicUsize>) {
        let calls = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(FakeRunner {
            result: Mutex::new(Some(result)),
            calls: Arc::clone(&calls),
            finished: Arc::clone(&finished),
            gate: None,
        });
        (runner, calls, finished)
    }

    /// Poll the scheduler until a route appears, then return it. `now` is fixed
    /// so the cache TTL cannot expire while the test waits.
    fn wait_for_route(
        trace: &mut GuiTrace,
        server: Ipv4Addr,
        session: u64,
        now: Instant,
    ) -> ClientRoute {
        for _ in 0..500 {
            let tick = trace.update(Some(server), session, now);
            if let Some(route) = tick.route {
                return route;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("the trace never produced a route");
    }

    #[test]
    fn a_server_is_traced_once_and_then_served_from_cache() {
        let (runner, calls, _finished) = fake_runner(Ok(route_fixture()));
        let mut trace = GuiTrace::from_runner(runner);
        let server = ip("192.0.2.10");
        let now = Instant::now();

        let first = trace.update(Some(server), 1, now);
        assert_eq!(first.route, None, "the first tick starts the run");
        assert_eq!(wait_for_route(&mut trace, server, 1, now), route_fixture());
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        let second = trace.update(Some(server), 1, now);
        assert_eq!(
            second.route,
            Some(route_fixture()),
            "the second tick is served from the cache"
        );
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a cached route must not spawn a second trace"
        );
    }

    #[test]
    fn only_one_trace_runs_at_a_time() {
        let (tx, rx) = channel();
        let calls = Arc::new(AtomicUsize::new(0));
        let finished = Arc::new(AtomicUsize::new(0));
        let runner = Arc::new(FakeRunner {
            result: Mutex::new(Some(Ok(route_fixture()))),
            calls: Arc::clone(&calls),
            finished,
            gate: Some(Mutex::new(rx)),
        });
        let mut trace = GuiTrace::from_runner(runner);
        let first = ip("192.0.2.10");
        let second = ip("192.0.2.11");
        let now = Instant::now();

        trace.update(Some(first), 1, now);
        // Wait for the gated runner to actually enter its run before checking
        // that no second one starts.
        let deadline = Instant::now() + Duration::from_secs(2);
        while calls.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "the first run never started");
            std::thread::sleep(Duration::from_millis(5));
        }
        // A different locked server arrives while the first run is gated: one
        // trace at a time means this tick must not start a second one.
        for _ in 0..5 {
            let tick = trace.update(Some(second), 1, now);
            assert_eq!(tick.route, None);
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);

        // Release the first run; its route becomes the served one.
        tx.send(()).expect("release the gated runner");
        drop(tx);
        assert_eq!(wait_for_route(&mut trace, first, 1, now), route_fixture());

        // Only now may the second server start its own trace.
        let tick = trace.update(Some(second), 1, now);
        assert_eq!(tick.route, None);
        let deadline = Instant::now() + Duration::from_secs(2);
        while calls.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < deadline, "the second run never started");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_failed_trace_is_not_retried_in_the_same_relay_session() {
        let (runner, calls, finished) = fake_runner(Err(TraceFailure::NoOutput));
        let mut trace = GuiTrace::from_runner(runner);
        let server = ip("192.0.2.10");
        let now = Instant::now();

        trace.update(Some(server), 7, now);
        // Wait until the worker produced its failure, then tick once to collect
        // it: one failed attempt inside a session is not a reason to respawn
        // the tool on every frame.
        let deadline = Instant::now() + Duration::from_secs(2);
        while finished.load(Ordering::SeqCst) == 0 {
            assert!(Instant::now() < deadline, "the runner never finished");
            std::thread::sleep(Duration::from_millis(5));
        }
        std::thread::sleep(Duration::from_millis(20));
        for _ in 0..10 {
            assert_eq!(trace.update(Some(server), 7, now).route, None);
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "a failed trace must not be retried within the same session"
        );

        // A new relay session is a new attempt. Wait for the worker to start so
        // the count is attributable to this tick and not to scheduling.
        trace.update(Some(server), 8, now);
        let deadline = Instant::now() + Duration::from_secs(2);
        while calls.load(Ordering::SeqCst) < 2 {
            assert!(Instant::now() < deadline, "the new session never traced");
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(calls.load(Ordering::SeqCst), 2);
    }

    #[test]
    fn a_private_server_is_never_probed() {
        let (runner, calls, _finished) = fake_runner(Ok(route_fixture()));
        let mut trace = GuiTrace::from_runner(runner);
        let now = Instant::now();
        for _ in 0..10 {
            assert_eq!(trace.update(Some(ip("192.168.1.5")), 1, now).route, None);
        }
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "a private target must not reach the runner at all"
        );
    }

    // ── Log line ───────────────────────────────────────────────────────────

    #[test]
    fn the_trace_log_line_round_trips() {
        let route = route_fixture();
        let line = info_line(ip("192.0.2.10"), &route, 1_700_000_000_123);
        assert_eq!(
            line,
            "CLIENT_TRACE server=192.0.2.10 hops=4 last_hop=203.0.113.9 \
             last_hop_rtt_ms=23 complete=false at_unix_ms=1700000000123"
        );
        assert_eq!(
            parse_info_line(&line),
            Some((ip("192.0.2.10"), route, 1_700_000_000_123))
        );
    }

    #[test]
    fn an_unanswered_hop_logs_none_not_zero() {
        let route = ClientRoute {
            hops: 3,
            last_hop: None,
            last_hop_rtt_ms: None,
            complete: false,
        };
        let line = info_line(ip("192.0.2.10"), &route, 7);
        assert!(line.contains("last_hop=none"), "{line}");
        assert!(line.contains("last_hop_rtt_ms=none"), "{line}");
        assert_eq!(
            parse_info_line(&line).map(|(_, route, _)| (
                route.last_hop,
                route.last_hop_rtt_ms,
                route.complete
            )),
            Some((None, None, false))
        );
    }

    // ── Caption ────────────────────────────────────────────────────────────

    fn english<T>(body: impl FnOnce() -> T) -> T {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));
        let value = body();
        crate::i18n::set_language(None);
        value
    }

    #[test]
    fn the_caption_states_hops_and_the_nearest_reply() {
        let route = route_fixture();
        assert_eq!(
            english(|| caption(&route)).as_deref(),
            Some("Route to server: 4 hops \u{00b7} nearest responding hop 23 ms")
        );
    }

    #[test]
    fn a_cloud_hop_adds_the_provider_name_but_never_a_city() {
        // 3.237.221.191 is in the shipped AWS us-east-1 cloud range.
        let route = ClientRoute {
            hops: 9,
            last_hop: Some(ip("3.237.221.191")),
            last_hop_rtt_ms: Some(31),
            complete: false,
        };
        assert_eq!(
            english(|| caption(&route)).as_deref(),
            Some("Route to server: 9 hops \u{00b7} nearest responding hop 31 ms \u{00b7} AWS")
        );
    }

    #[test]
    fn a_hosting_hop_adds_the_network_name_but_never_a_city() {
        // The live G-Core Virginia server: the label is the operator, not JP -
        // and never the geofeed's city, which places the server, not a hop.
        let route = ClientRoute {
            hops: 12,
            last_hop: Some(ip("178.107.14.201")),
            last_hop_rtt_ms: Some(40),
            complete: false,
        };
        assert_eq!(
            english(|| caption(&route)).as_deref(),
            Some(
                "Route to server: 12 hops \u{00b7} nearest responding hop 40 ms \u{00b7} \
                 G-Core"
            )
        );
    }

    #[test]
    fn a_timeout_row_has_no_caption_without_any_hop() {
        let route = ClientRoute {
            hops: 0,
            last_hop: None,
            last_hop_rtt_ms: None,
            complete: false,
        };
        assert_eq!(english(|| caption(&route)), None);
    }
}
