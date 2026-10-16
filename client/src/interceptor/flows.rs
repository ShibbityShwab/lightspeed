//! Process-scoped flow attribution: "which flows does this process own right
//! now", and what to do with each packet of them.
//!
//! The owner's direction is *pick a process, relay everything it sends*. That
//! splits into two jobs; this module owns the first one. **Attribution** answers
//! which flows the target process owns. **Steering** - a static WinDivert
//! network-layer filter, rebuilt only when the flow set changes - acts on the
//! answer and lives elsewhere (`windows.rs`). This module is pure and I/O-free:
//! it takes `now` as an explicit parameter, opens no handle, and never issues a
//! syscall, so its whole state machine is deterministically unit-testable on
//! every platform - exactly the property [`super::order`] was built for.
//!
//! # The state machine
//!
//! [`FlowKey`] identifies one flow: local socket, remote socket, transport.
//! [`FlowMap`] holds at most [`MAX_TRACKED_ENTRIES`] of them and answers one
//! question per observed packet through [`FlowMap::observe`]:
//! [`FlowVerdict::Tunnel`] (relay it) or [`FlowVerdict::Direct`] (re-inject it
//! on the direct path unchanged).
//!
//! Entries arrive two ways:
//!
//! 1. **Seeded** from the scanner's routes ([`FlowMap::seed`]). A scanner route
//!    is an observation of a live connection, so a seeded entry locks at once:
//!    the debounce would only delay an answer we already have.
//! 2. **Attributed** by an [`AttributionSource`] ([`FlowMap::attribute`]) or by
//!    a local-port hint plus the debounce ([`FlowMap::hint_port`]).
//!
//! A packet whose flow has no entry is **UNATTRIBUTED**: it takes the direct
//! path and is counted. It is never dropped and never silently relayed.
//!
//! # Fail-open (the invariant this module exists to keep)
//!
//! Every path out of [`FlowMap::observe`] is either [`FlowVerdict::Tunnel`] or
//! [`FlowVerdict::Direct`]; there is no third outcome and no drop. A flow that
//! is over the tunnel cap, or that cannot be attributed at all, is returned as
//! [`DirectReason::Unattributed`] and recorded in [`FlowReport::unattributed_flows`]
//! so the status line can say `N flows via <backend>, M UNATTRIBUTED`. The game
//! is never broken by our bookkeeping, and a boost that did not capture what it
//! claimed has to say so.
//!
//! # The cap (owner gate OQ1)
//!
//! The relay bans a *source IP* that reaches more than
//! [`RELAY_MAX_DESTINATIONS_PER_WINDOW`] distinct destinations in 10 s, and the
//! ban lasts 3600 s - so its blast radius is every concurrent built-in session
//! on that address and every device behind a shared NAT. The client therefore
//! caps what it touches: at most [`MAX_TUNNELED_FLOWS`] simultaneously tunneled
//! flows, warned from [`WARN_TUNNELED_FLOWS`], over a hard ceiling of
//! [`MAX_TRACKED_ENTRIES`] tracked entries (a memory bound - entries tracked
//! past the tunnel cap still fail open to the direct path).
//!
//! The arithmetic is written down at the constants, checked at compile time for
//! the intra-crate relations, and pinned to the relay's own default by the
//! cross-crate guard `client/tests/abuse_guard.rs`. That test reads
//! [`MAX_TUNNELED_FLOWS`] here and `default_max_destinations()` in
//! `proxy/src/config.rs`, so raising the cap to the relay's threshold - or
//! lowering the relay's threshold to the cap - fails it with both numbers named.
//! [`RELAY_MAX_DESTINATIONS_PER_WINDOW`] mirrors the relay's
//! `SecurityConfig::max_destinations_per_window` (`default_max_destinations()`,
//! `10`, in `proxy/src/config.rs`, window 10 s, `ban_duration_secs` 3600), which
//! the client crate cannot depend on; the guard test also holds this mirror equal
//! to the relay's declared default, so if the relay's threshold moves, the mirror
//! moves with it.
//!
//! # Attribution sources: which WinDivert layer wins, and why
//!
//! Two WinDivert layers carry a `ProcessId`, and both sit behind the one
//! [`AttributionSource`] trait:
//!
//! | Layer  | Events                                   | Sees                                             | Blind to                                    |
//! |--------|------------------------------------------|--------------------------------------------------|---------------------------------------------|
//! | FLOW   | `ESTABLISHED` / `DELETED`, 5-tuple + PID | every flow that starts while the handle is open  | flows established *before* the handle opened |
//! | SOCKET | `SOCKET_CONNECT` / `SOCKET_BIND`, PID    | the connect attempt *before* the first datagram  | nothing that a live socket will not repeat   |
//!
//! [`SourceKind::Auto`] resolves to the **SOCKET** layer, and that is the
//! runtime default. The reason is the target case, not a preference: a boost
//! normally starts *after* the game, so the flows that matter already exist,
//! and a FLOW handle opened at attach time can never report them
//! (`WINDIVERT_LAYER::FLOW` only emits activity it observes after the open).
//! SOCKET emits the connection attempt before the first datagram leaves, so it
//! sees the flows the FLOW layer misses. FLOW stays fully selectable
//! ([`SourceKind::Flow`]) and stays available as a secondary signal: it is the
//! exact, event-driven per-flow signal for flows that begin while the handle is
//! open, and it also reports `DELETED`. The choice is one config value
//! ([`SourceKind`]) and [`select_source`] turns it into the implementation.
//!
//! On any build that cannot host a WinDivert layer
//! ([`WIN_DIVERT_LAYERS_AVAILABLE`] is false - non-Windows, or the
//! `windivert-redirect` feature is off) [`select_source`] returns
//! [`NoopAttributionSource`], which reports nothing; attribution then falls back
//! entirely to scanner-seeded routes and local-port hints.
//!
//! # Known limits, stated honestly
//!
//! - **The FLOW layer cannot see flows established before the handle opened.**
//!   A boost started after the game launch is the common case, so FLOW alone is
//!   never sufficient; it is why SOCKET is the default and why the scanner
//!   seeds the map.
//! - **`GetExtendedUdpTable` carries no remote endpoint.** The owner-PID UDP
//!   table maps a PID to a *local port* and nothing more, and two processes can
//!   share that port. So a UDP hint cannot be trusted into a tunnel on its own:
//!   it enters the map through [`FlowMap::hint_port`] and only locks after the
//!   same N-packet debounce every other unseeded entry uses
//!   ([`TrackerConfig::detect_pkts`] packets inside [`TrackerConfig::detect_window`]).
//!   `GetExtendedTcpTable` does carry the remote endpoint, so a TCP observation
//!   can be seeded directly.
//! - **A mid-session start therefore relies on three things together:**
//!   scanner-seeded routes (the `ProcessScanner` route list, authoritative when
//!   present), local-port hints plus the debounce (UDP, when only the socket
//!   owner is known), and the SOCKET layer's at-attach connect events. Where all
//!   three are silent, the flow is UNATTRIBUTED and goes direct, visibly.
//! - **IPv4 only.** Every interception path in the tree is IPv4 today
//!   (`SocketAddrV4`), so a flow key cannot name an IPv6 flow.
//!
//! # What this reuses (not re-invented here)
//!
//! - [`super::order::ServerTracker`] is the per-flow tracker, exactly as
//!   `order.rs` uses it: it owns the N-packet debounce
//!   ([`TrackerConfig::detect_pkts`] within [`TrackerConfig::detect_window`]),
//!   the single-candidate lock, the empty-payload rule (a zero-length datagram
//!   never advances detection), and [`super::order::Decision`]. This module adds
//!   no second detection state enum.
//! - [`super::order::TrackerConfig`]'s defaults (3 packets / 1500 ms / 5 s) are
//!   reused unchanged, so a flow locks on the same evidence a single-server
//!   interceptor always has.
//! - This module enforces the 5 s **staleness drop** itself, from an entry's
//!   `last_seen`, because a per-flow tracker is always fed its own remote so its
//!   internal stale branch never fires; [`FLOW_STALE_TIMEOUT`] is kept equal to
//!   `TrackerConfig::stale_timeout` and a test holds them together.
//! - [`super::traits::Route`] and [`crate::games::TransportProto`] are the
//!   scanner's and profile's own types - the seeded path consumes the scanner's
//!   output directly rather than a parallel route type.
//! - The cap/warning numbers are the OQ1 decisions
//!   (`wat/state/decisions.md`, 2026-10-07).

use std::collections::{HashMap, HashSet};
use std::net::SocketAddrV4;
use std::time::{Duration, Instant};

use super::order::{Decision, ServerTracker, TrackerConfig};
use super::traits::{Route, TransportProtocol};
use crate::games::TransportProto;

// ─────────────────────────────────────────────────────────────────────────────
//  The cap and warning arithmetic (owner gate OQ1)
// ─────────────────────────────────────────────────────────────────────────────

/// The relay's own abuse threshold: distinct destinations inside a 10 s window
/// that trip its source-IP ban for 3600 s.
///
/// Mirrors `SecurityConfig::max_destinations_per_window`
/// (`default_max_destinations()`, `10`, in `proxy/src/config.rs`). The client
/// crate cannot depend on the proxy crate, so the number is mirrored here, and
/// `client/tests/abuse_guard.rs` reads the relay's declared default to hold this
/// mirror equal to it - as well as holding [`MAX_TUNNELED_FLOWS`] strictly below
/// it.
pub const RELAY_MAX_DESTINATIONS_PER_WINDOW: usize = 10;

/// Simultaneously *tunneled* flows the client admits (OQ1: cap 8).
///
/// The relay rule this number exists to stay under: a source IP that reaches
/// more than [`RELAY_MAX_DESTINATIONS_PER_WINDOW`] distinct destinations in 10 s
/// is banned for 3600 s (`max_destinations_per_window` and `ban_duration_secs`
/// in `proxy/src/config.rs`), and that ban would take every session behind the
/// same NAT down with it. Distinct destinations in flight at once therefore stay
/// 2 below that threshold, the headroom OQ1 accepted on an UNMEASURED benchmark
/// (B-1 needs elevation); `client/tests/abuse_guard.rs` reads both numbers back
/// and fails if this cap ever reaches the relay's.
pub const MAX_TUNNELED_FLOWS: usize = 8;

/// Tunneled flows at which the status line starts warning (OQ1: warn at 6).
///
/// Two slots stay free when the warning appears, so the user hears about the
/// relay's 3600 s source-IP ban risk before [`MAX_TUNNELED_FLOWS`] - itself held
/// under `max_destinations_per_window` (`proxy/src/config.rs`) by
/// `client/tests/abuse_guard.rs` - actually bites.
pub const WARN_TUNNELED_FLOWS: usize = 6;

/// Hard ceiling on tracked entries, tunneled or not (a memory bound).
///
/// An entry past the tunnel cap is still tracked so its flow is visible in the
/// report, but it never tunnels; the map itself stops growing at this number
/// and further flows fail open to the direct path.
pub const MAX_TRACKED_ENTRIES: usize = 64;

/// The 5 s of silence after which a flow entry is dropped.
///
/// Equal to `TrackerConfig::stale_timeout`; [`tests::flow_stale_timeout_matches_tracker`]
/// holds the two together.
pub const FLOW_STALE_TIMEOUT: Duration = Duration::from_secs(5);

/// Whether this build can host the two WinDivert attribution layers.
pub const WIN_DIVERT_LAYERS_AVAILABLE: bool =
    cfg!(all(target_os = "windows", feature = "windivert-redirect"));

// The warning must fire before the cap does, and the cap must fit the tracked
// ceiling. Written as `if`/`panic!` rather than `assert!` so the const guard
// does not trip `clippy::assertions_on_constants`.
//
// The cap-versus-relay relation is deliberately NOT asserted here: a const
// failure can interpolate neither number into its message, and it would stop the
// build before any test could run. That relation belongs to
// `client/tests/abuse_guard.rs`, which reads the relay's declared default and
// names both values when they drift.
const _: () = {
    if WARN_TUNNELED_FLOWS >= MAX_TUNNELED_FLOWS {
        panic!("WARN_TUNNELED_FLOWS must fire before MAX_TUNNELED_FLOWS");
    }
    if MAX_TUNNELED_FLOWS > MAX_TRACKED_ENTRIES {
        panic!("MAX_TUNNELED_FLOWS must not exceed MAX_TRACKED_ENTRIES");
    }
};

// ─────────────────────────────────────────────────────────────────────────────
//  Flow identity
// ─────────────────────────────────────────────────────────────────────────────

/// One flow, identified by both endpoints and the transport.
///
/// One entry per flow, not one per destination: a game talking to the same
/// server from two sockets gets two keys, and neither steals the other's lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowKey {
    /// The game's local socket.
    pub local: SocketAddrV4,
    /// The server's socket.
    pub remote: SocketAddrV4,
    /// The transport carried.
    pub kind: TransportProto,
}

impl FlowKey {
    /// Build a flow key.
    pub const fn new(local: SocketAddrV4, remote: SocketAddrV4, kind: TransportProto) -> Self {
        Self {
            local,
            remote,
            kind,
        }
    }

    /// The local port, which is what a `GetExtendedUdpTable` hint can name.
    pub fn local_port(&self) -> u16 {
        self.local.port()
    }
}

impl std::fmt::Display for FlowKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} -> {}", self.kind, self.local, self.remote)
    }
}

// `TransportProto` (the games/profile enum) derives no `Hash`, and it lives
// outside this module's scope, so the discriminant is hashed explicitly rather
// than widening that type. `PartialEq`/`Eq` stay derived, which keeps key
// equality identical to field equality.
impl std::hash::Hash for FlowKey {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.local.hash(state);
        self.remote.hash(state);
        let kind: u8 = match self.kind {
            TransportProto::Udp => 0,
            TransportProto::Tcp => 1,
        };
        kind.hash(state);
    }
}

/// Build a flow key from the scanner's own route type.
impl From<&Route> for FlowKey {
    fn from(route: &Route) -> Self {
        let kind = match route.proto {
            TransportProtocol::Udp => TransportProto::Udp,
            TransportProtocol::Tcp => TransportProto::Tcp,
        };
        Self {
            local: route.local,
            remote: route.remote,
            kind,
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Per-packet verdict
// ─────────────────────────────────────────────────────────────────────────────

/// Why a packet took the direct path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DirectReason {
    /// No attribution for this flow, or it is over the tunnel cap. Counted as
    /// UNATTRIBUTED so the shortfall is visible.
    Unattributed,
    /// Attributed, but still inside the N-packet debounce window.
    Detecting,
    /// Attributed and locked, but this packet is not to be relayed (e.g. a
    /// zero-length payload, which `order.rs` never counts as activity).
    Passing,
}

/// What the data plane should do with the packet just observed.
///
/// There is deliberately no "drop" variant: fail-open is the invariant, so the
/// only alternatives are relay and re-inject-direct.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowVerdict {
    /// Relay this packet through the tunnel for `key`.
    Tunnel(FlowKey),
    /// Re-inject this packet on the direct path, unchanged.
    Direct(DirectReason),
}

impl FlowVerdict {
    /// Whether this packet is relayed through the tunnel.
    pub const fn is_tunnel(&self) -> bool {
        matches!(self, Self::Tunnel(_))
    }
}

/// A status-line warning that the tunneled-flow count is approaching the cap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapWarning {
    /// Flows currently tunneled.
    pub tunneled: usize,
    /// The cap they are approaching ([`MAX_TUNNELED_FLOWS`] by default).
    pub cap: usize,
    /// Human-readable message for the `interceptor_error` slot.
    pub message: String,
}

/// A snapshot for the status line: what was attributed, and what was not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlowReport {
    /// Flows currently admitted to the tunnel.
    pub tunneled: usize,
    /// Every entry the map tracks, tunneled or not.
    pub tracked: usize,
    /// Distinct flows seen without attribution (or over the tunnel cap).
    pub unattributed_flows: usize,
    /// Packets refused the tunnel because the cap was full.
    pub over_cap_packets: u64,
    /// Present once the tunneled count reaches the warning threshold.
    pub warning: Option<CapWarning>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Attribution sources
// ─────────────────────────────────────────────────────────────────────────────

/// Which WinDivert layer an [`AttributionSource`] reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionLayer {
    /// The WinDivert FLOW layer (`ESTABLISHED` / `DELETED`, 5-tuple + PID).
    Flow,
    /// The WinDivert SOCKET layer (`SOCKET_CONNECT` / `SOCKET_BIND`, PID).
    Socket,
    /// No attribution available (non-Windows, feature off, or disabled).
    None,
}

impl std::fmt::Display for AttributionLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Flow => "flow",
            Self::Socket => "socket",
            Self::None => "none",
        })
    }
}

/// The one config value that picks the attribution source at runtime.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SourceKind {
    /// Prefer the layer that sees the most: the SOCKET layer where it exists.
    #[default]
    Auto,
    /// Force the WinDivert FLOW layer.
    Flow,
    /// Force the WinDivert SOCKET layer.
    Socket,
    /// Attribution off; scanner routes and local-port hints only.
    Off,
}

impl SourceKind {
    /// The layer this selection names, ignoring platform availability.
    ///
    /// `Auto` takes SOCKET because the target case is boost-after-launch: the
    /// flows that matter already exist, and only the SOCKET layer reports a
    /// connection attempt the FLOW handle never saw.
    pub const fn resolved(self) -> AttributionLayer {
        match self {
            Self::Auto | Self::Socket => AttributionLayer::Socket,
            Self::Flow => AttributionLayer::Flow,
            Self::Off => AttributionLayer::None,
        }
    }

    /// The layer actually usable on this build.
    pub const fn effective(self) -> AttributionLayer {
        if WIN_DIVERT_LAYERS_AVAILABLE {
            self.resolved()
        } else {
            AttributionLayer::None
        }
    }
}

/// One observation from an attribution layer: which flow, and whose process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FlowObservation {
    /// The flow the layer observed.
    pub key: FlowKey,
    /// The owning process id the layer reported.
    pub pid: u32,
    /// Which event produced it.
    pub event: AttributionEvent,
}

/// The layer event that produced a [`FlowObservation`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AttributionEvent {
    /// FLOW layer: the flow was established after the handle opened.
    FlowEstablished,
    /// FLOW layer: the flow was deleted.
    FlowDeleted,
    /// SOCKET layer: a connect was attempted, before the first datagram.
    SocketConnect,
    /// SOCKET layer: a bind happened. Carries a local port but no remote.
    SocketBind,
}

/// A runtime-selected attribution source.
///
/// Implementations never block and never fail hard: a source that sees nothing
/// returns nothing, and an unattributed flow fails open in [`FlowMap`].
pub trait AttributionSource {
    /// Which layer this source reads.
    fn layer(&self) -> AttributionLayer;

    /// Record one parsed layer event.
    ///
    /// A source drops events belonging to a different layer rather than
    /// guessing: mis-feed must not fabricate attribution.
    fn observe(&mut self, obs: FlowObservation);

    /// Take the observations accepted since the last call. Never blocks.
    fn drain(&mut self) -> Vec<FlowObservation>;
}

/// The FLOW layer's source shape: `ESTABLISHED` / `DELETED` events, each
/// carrying the 5-tuple and the owning PID.
///
/// Blind to flows established before the handle opened - the layer emits only
/// activity it observes after the open - so it is never the only source.
#[derive(Debug, Default)]
pub struct FlowLayerSource {
    queue: Vec<FlowObservation>,
}

impl AttributionSource for FlowLayerSource {
    fn layer(&self) -> AttributionLayer {
        AttributionLayer::Flow
    }

    fn observe(&mut self, obs: FlowObservation) {
        if matches!(
            obs.event,
            AttributionEvent::FlowEstablished | AttributionEvent::FlowDeleted
        ) {
            self.queue.push(obs);
        }
    }

    fn drain(&mut self) -> Vec<FlowObservation> {
        std::mem::take(&mut self.queue)
    }
}

/// The SOCKET layer's source shape: `SOCKET_CONNECT` / `SOCKET_BIND` events,
/// both PID-capable and emitted *before* the first datagram leaves.
///
/// Because it fires at connect time, it can report a flow the FLOW layer never
/// saw - the reason it is [`SourceKind::Auto`]'s pick. A `BIND` event carries no
/// remote endpoint, so it is useful only as a local-port hint.
#[derive(Debug, Default)]
pub struct SocketLayerSource {
    queue: Vec<FlowObservation>,
}

impl AttributionSource for SocketLayerSource {
    fn layer(&self) -> AttributionLayer {
        AttributionLayer::Socket
    }

    fn observe(&mut self, obs: FlowObservation) {
        if matches!(
            obs.event,
            AttributionEvent::SocketConnect | AttributionEvent::SocketBind
        ) {
            self.queue.push(obs);
        }
    }

    fn drain(&mut self) -> Vec<FlowObservation> {
        std::mem::take(&mut self.queue)
    }
}

/// The non-Windows / feature-off / disabled source: reports nothing.
///
/// Selecting it is not an error. Attribution then rests on scanner-seeded
/// routes and local-port hints, and every unattributed flow fails open.
#[derive(Debug, Default)]
pub struct NoopAttributionSource;

impl AttributionSource for NoopAttributionSource {
    fn layer(&self) -> AttributionLayer {
        AttributionLayer::None
    }

    fn observe(&mut self, _obs: FlowObservation) {}

    fn drain(&mut self) -> Vec<FlowObservation> {
        Vec::new()
    }
}

/// Turn the one config value into the source implementation for this build.
///
/// Off Windows, or without `windivert-redirect`, the layer handles cannot
/// exist, so this returns [`NoopAttributionSource`] regardless of `kind`.
pub fn select_source(kind: SourceKind) -> Box<dyn AttributionSource> {
    match kind.effective() {
        AttributionLayer::Flow => Box::new(FlowLayerSource::default()),
        AttributionLayer::Socket => Box::new(SocketLayerSource::default()),
        AttributionLayer::None => Box::new(NoopAttributionSource),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  The bounded flow map
// ─────────────────────────────────────────────────────────────────────────────

/// Tuning for [`FlowMap`].
#[derive(Debug, Clone, Copy)]
pub struct FlowMapConfig {
    /// The per-flow tracker's debounce and window ([`ServerTracker`] config).
    pub tracker: TrackerConfig,
    /// Simultaneously tunneled flows allowed ([`MAX_TUNNELED_FLOWS`]).
    pub max_tunneled: usize,
    /// Tunneled count at which the status line warns ([`WARN_TUNNELED_FLOWS`]).
    pub warn_tunneled: usize,
    /// Tracked-entry ceiling ([`MAX_TRACKED_ENTRIES`]).
    pub max_tracked: usize,
}

impl Default for FlowMapConfig {
    fn default() -> Self {
        Self {
            tracker: TrackerConfig::default(),
            max_tunneled: MAX_TUNNELED_FLOWS,
            warn_tunneled: WARN_TUNNELED_FLOWS,
            max_tracked: MAX_TRACKED_ENTRIES,
        }
    }
}

/// One tracked flow: the reused per-flow tracker plus its bookkeeping.
#[derive(Debug)]
struct FlowEntry {
    /// The reused `order.rs` state machine for this flow's single candidate.
    tracker: ServerTracker,
    /// Last packet seen (the map's own 5 s staleness anchor).
    last_seen: Instant,
    /// The process that owns this flow.
    pid: u32,
    /// Whether this entry currently holds one of the tunnel slots.
    tunneled: bool,
}

impl FlowEntry {
    /// A fresh entry. `seed` locks immediately (an authoritative observation:
    /// a scanner route or a layer event); otherwise the entry must earn its
    /// lock through the debounce.
    fn new(cfg: TrackerConfig, key: FlowKey, pid: u32, seed: bool, now: Instant) -> Self {
        let mut tracker = ServerTracker::new(cfg);
        if seed {
            tracker.seed(&[key.remote], now);
        }
        Self {
            tracker,
            last_seen: now,
            pid,
            tunneled: false,
        }
    }
}

/// The bounded, fail-open flow map.
///
/// See the module docs for the state machine, the cap and the fail-open rule.
#[derive(Debug)]
pub struct FlowMap {
    cfg: FlowMapConfig,
    /// `None` accepts any attributed process (port-scoped mode); `Some(pid)`
    /// is process mode and rejects every flow owned by another process.
    target_pid: Option<u32>,
    entries: HashMap<FlowKey, FlowEntry>,
    /// PID hints from the UDP owner table, keyed by local port. That table has
    /// no remote endpoint, so a hint must earn its lock through the debounce.
    port_hints: HashMap<u16, u32>,

    /// Hinted local ports whose flow locks on its first packet instead of
    /// passing the debounce. Populated only for the target's own sockets.
    locked_hint_ports: std::collections::HashSet<u16>,
    /// Distinct flows that could not be attributed (or were over the cap).
    unattributed: HashSet<FlowKey>,
    over_cap_packets: u64,
}

impl FlowMap {
    /// Create an empty map. `target_pid` is the process mode target, or `None`
    /// to accept any attributed process (port-scoped mode).
    pub fn new(cfg: FlowMapConfig, target_pid: Option<u32>) -> Self {
        Self {
            cfg,
            target_pid,
            entries: HashMap::new(),
            port_hints: HashMap::new(),
            locked_hint_ports: std::collections::HashSet::new(),
            unattributed: HashSet::new(),
            over_cap_packets: 0,
        }
    }

    /// The process this map targets, if any.
    pub fn target_pid(&self) -> Option<u32> {
        self.target_pid
    }

    /// Number of tracked entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is tracked.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Number of entries currently holding a tunnel slot.
    pub fn tunneled_count(&self) -> usize {
        self.entries.values().filter(|e| e.tunneled).count()
    }

    /// The process a tracked flow belongs to, if tracked.
    pub fn pid_of(&self, key: &FlowKey) -> Option<u32> {
        self.entries.get(key).map(|e| e.pid)
    }

    /// Whether `pid` is acceptable to this map.
    fn pid_accepted(&self, pid: u32) -> bool {
        self.target_pid.is_none_or(|target| target == pid)
    }

    /// Seed entries from the scanner's route list.
    ///
    /// A route is an observation of a live connection, so each seeded entry
    /// locks immediately. Routes owned by another process, or past the tracked
    /// ceiling, are ignored (the flow then fails open, visibly).
    pub fn seed(&mut self, pid: u32, routes: &[Route], now: Instant) {
        if !self.pid_accepted(pid) {
            return;
        }
        let tracker_cfg = self.cfg.tracker;
        let max_tracked = self.cfg.max_tracked;
        for route in routes {
            if self.entries.len() >= max_tracked {
                break;
            }
            let key = FlowKey::from(route);
            self.entries
                .entry(key)
                .or_insert_with(|| FlowEntry::new(tracker_cfg, key, pid, true, now));
        }
    }

    /// Record a local-port -> PID hint from the socket owner table.
    ///
    /// Two documented holes make a hint weaker than a route: the UDP table
    /// carries no remote endpoint, and two processes can bind the same port. A
    /// hinted flow therefore starts unlocked and must pass the debounce before
    /// it can tunnel.
    ///
    /// `lock_now` is for the one case where waiting is unsafe: the hint came
    /// from the socket the *target process itself* owns, on a machine where no
    /// layer event source is running. Field verification against a live game
    /// showed the cost of waiting there - the flow map sat at `flows: 0` while
    /// thousands of the game's packets went direct, and the game server saw a
    /// client whose traffic had moved mid-match and dropped the session. When
    /// the local port is the target's own, the ownership question is already
    /// answered, so the flow locks on its first packet instead of the third.
    pub fn hint_port(&mut self, local_port: u16, pid: u32) {
        self.hint_port_locking(local_port, pid, false);
    }

    /// [`Self::hint_port`], optionally locking the flow on its first packet.
    pub fn hint_port_locking(&mut self, local_port: u16, pid: u32, lock_now: bool) {
        if self.port_hints.len() < self.cfg.max_tracked || self.port_hints.contains_key(&local_port)
        {
            self.port_hints.insert(local_port, pid);
        }
        if lock_now {
            self.locked_hint_ports.insert(local_port);
        }
    }

    /// Whether a hinted local port locks its flow on the first packet.
    pub fn hint_locks_immediately(&self, local_port: u16) -> bool {
        self.locked_hint_ports.contains(&local_port)
    }
    /// Apply one layer observation.
    ///
    /// Returns whether it was recorded. A `BIND` observation carries no remote
    /// endpoint, so it becomes a local-port hint; `DELETED` retires the flow;
    /// an established flow or a connect locks its entry at once.
    pub fn attribute(&mut self, obs: FlowObservation, now: Instant) -> bool {
        if !self.pid_accepted(obs.pid) {
            return false;
        }
        match obs.event {
            AttributionEvent::FlowDeleted => self.entries.remove(&obs.key).is_some(),
            AttributionEvent::SocketBind => {
                self.hint_port(obs.key.local_port(), obs.pid);
                true
            }
            AttributionEvent::FlowEstablished | AttributionEvent::SocketConnect => {
                if let Some(entry) = self.entries.get_mut(&obs.key) {
                    entry.last_seen = now;
                    entry.pid = obs.pid;
                    return true;
                }
                let tracker_cfg = self.cfg.tracker;
                if self.entries.len() >= self.cfg.max_tracked {
                    return false;
                }
                self.entries.insert(
                    obs.key,
                    FlowEntry::new(tracker_cfg, obs.key, obs.pid, true, now),
                );
                true
            }
        }
    }

    /// Forget one flow (a layer `DELETED`, or an explicit teardown).
    pub fn retire(&mut self, key: &FlowKey) {
        self.entries.remove(key);
    }

    /// Drop entries silent for [FLOW_STALE_TIMEOUT], returning how many went.
    pub fn expire(&mut self, now: Instant) -> usize {
        let stale = self.cfg.tracker.stale_timeout;
        let before = self.entries.len();
        self.entries
            .retain(|_, entry| now.saturating_duration_since(entry.last_seen) <= stale);
        before - self.entries.len()
    }

    /// Observe one outbound packet and return the routing verdict.
    ///
    /// Fail-open: the only outcomes are [`FlowVerdict::Tunnel`] and
    /// [`FlowVerdict::Direct`].
    pub fn observe(&mut self, key: FlowKey, now: Instant, payload_len: usize) -> FlowVerdict {
        self.expire(now);

        // A packet whose flow is unknown but whose local port is hinted can be
        // attributed. The hint normally starts the flow unlocked so it must earn
        // its tunnel slot through the debounce; a hint the caller marked as the
        // target's own socket locks at once, because waiting there is what cost a
        // live game its session (see `hint_port_locking`).
        let hint = self.port_hints.get(&key.local_port()).copied();
        if let Some(pid) = hint {
            let tracker_cfg = self.cfg.tracker;
            let max_tracked = self.cfg.max_tracked;
            let lock_now = self.locked_hint_ports.contains(&key.local_port());
            if self.pid_accepted(pid) && self.entries.len() < max_tracked {
                self.entries
                    .entry(key)
                    .or_insert_with(|| FlowEntry::new(tracker_cfg, key, pid, lock_now, now));
            }
        }

        let tunneled_now = self.tunneled_count();
        let max_tunneled = self.cfg.max_tunneled;
        // Compute inside the entry borrow, apply afterwards: recording an
        // outcome touches other fields of `self`.
        let outcome = if let Some(entry) = self.entries.get_mut(&key) {
            entry.last_seen = now;
            match entry.tracker.observe(key.remote, now, payload_len) {
                Decision::Tunnel(_) => {
                    if entry.tunneled {
                        Observed::Tunnel
                    } else if tunneled_now < max_tunneled {
                        entry.tunneled = true;
                        Observed::Tunnel
                    } else {
                        entry.tunneled = false;
                        Observed::OverCap
                    }
                }
                Decision::ResetToDetection => {
                    // Unreachable for a self-consistent entry (it is always fed
                    // its own remote), but if a lock were ever released the slot
                    // must be released with it.
                    entry.tunneled = false;
                    Observed::Detecting
                }
                Decision::StartDetection | Decision::PassThrough | Decision::ShadowDirect => {
                    if entry.tracker.is_locked() {
                        Observed::Passing
                    } else {
                        Observed::Detecting
                    }
                }
            }
        } else {
            Observed::Unattributed
        };

        match outcome {
            Observed::Tunnel => FlowVerdict::Tunnel(key),
            Observed::Detecting => FlowVerdict::Direct(DirectReason::Detecting),
            Observed::Passing => FlowVerdict::Direct(DirectReason::Passing),
            Observed::OverCap => {
                self.over_cap_packets += 1;
                self.note_unattributed(key);
                FlowVerdict::Direct(DirectReason::Unattributed)
            }
            Observed::Unattributed => {
                self.note_unattributed(key);
                FlowVerdict::Direct(DirectReason::Unattributed)
            }
        }
    }

    /// Record a flow in the bounded UNATTRIBUTED set.
    fn note_unattributed(&mut self, key: FlowKey) {
        if self.unattributed.len() < self.cfg.max_tracked {
            self.unattributed.insert(key);
        }
    }

    /// A snapshot for the status line.
    pub fn report(&self) -> FlowReport {
        let tunneled = self.tunneled_count();
        let warning = (tunneled >= self.cfg.warn_tunneled).then(|| {
            let cap = self.cfg.max_tunneled;
            CapWarning {
                tunneled,
                cap,
                message: format!(
                    "{tunneled} of {cap} tunneled flows; the relay bans a source past \
                     {RELAY_MAX_DESTINATIONS_PER_WINDOW} destinations in 10 s for 3600 s"
                ),
            }
        });
        FlowReport {
            tunneled,
            tracked: self.entries.len(),
            unattributed_flows: self.unattributed.len(),
            over_cap_packets: self.over_cap_packets,
            warning,
        }
    }
}

/// Internal per-packet outcome, computed under the entry borrow.
#[derive(Debug, Clone, Copy)]
enum Observed {
    Tunnel,
    Detecting,
    Passing,
    OverCap,
    Unattributed,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn addr(a: u8, port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, a), port)
    }

    fn key(a: u8, local_port: u16, remote_port: u16) -> FlowKey {
        FlowKey::new(
            SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, local_port),
            addr(a, remote_port),
            TransportProto::Udp,
        )
    }

    fn obs(key: FlowKey, pid: u32) -> FlowObservation {
        FlowObservation {
            key,
            pid,
            event: AttributionEvent::FlowEstablished,
        }
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    /// A seeded scanner route, as `ProcessScanner` emits it.
    fn route(local_port: u16, a: u8, remote_port: u16) -> Route {
        Route {
            local: SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, local_port),
            remote: addr(a, remote_port),
            proto: TransportProtocol::Udp,
        }
    }

    // ── seeding ────────────────────────────────────────────────────────────
    #[test]
    fn seeded_route_locks_and_tunnels_the_first_packet() {
        let t0 = Instant::now();
        let r = route(50_000, 10, 7777);
        let k = FlowKey::from(&r);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));

        m.seed(4242, std::slice::from_ref(&r), t0);

        assert_eq!(m.len(), 1);
        assert_eq!(m.pid_of(&k), Some(4242), "the route carries its owner");
        assert_eq!(m.observe(k, t0, 100), FlowVerdict::Tunnel(k));
    }

    #[test]
    fn seeding_ignores_routes_owned_by_another_process() {
        let t0 = Instant::now();
        let r = route(50_000, 11, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));

        m.seed(999, std::slice::from_ref(&r), t0);
        assert!(
            m.is_empty(),
            "process mode must not adopt another PID's route"
        );
    }

    #[test]
    fn seed_respects_the_tracked_ceiling() {
        let t0 = Instant::now();
        let routes: Vec<Route> = (0..80u8)
            .map(|i| route(50_000 + i as u16, i, 7777))
            .collect();
        let mut m = FlowMap::new(FlowMapConfig::default(), None);

        m.seed(7, &routes, t0);
        assert_eq!(m.len(), MAX_TRACKED_ENTRIES);
    }

    // ── debounce and lock ──────────────────────────────────────────────────
    #[test]
    fn a_target_owned_hint_locks_on_the_first_packet() {
        // Field-verification regression. A live ARC Raiders match was dropped
        // because the socket-owner table proved the local port belonged to the
        // target process, yet the flow still had to pass the debounce: the map
        // sat at `flows: 0` while thousands of the game's packets went direct,
        // and the server saw a client whose traffic had moved mid-match.
        //
        // When the owner table names the target's own socket, ownership is
        // already settled, so the flow takes its tunnel slot immediately.
        let t0 = Instant::now();
        let k = key(14, 50_010, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));
        m.hint_port_locking(50_010, 4242, true);

        assert!(m.hint_locks_immediately(50_010));
        // The very first packet tunnels - no detection window at all.
        assert_eq!(m.observe(k, t0, 100), FlowVerdict::Tunnel(k));
        assert_eq!(m.tunneled_count(), 1);
        assert_eq!(m.pid_of(&k), Some(4242));
    }

    #[test]
    fn a_hint_from_someone_else_still_passes_the_debounce() {
        // The conservative rule is not weakened globally: a hint that does NOT
        // come from the target's own socket keeps earning its slot through the
        // debounce, because two processes can bind one port.
        let t0 = Instant::now();
        let k = key(15, 50_011, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));
        m.hint_port(50_011, 4242);

        assert!(!m.hint_locks_immediately(50_011));
        assert_eq!(
            m.observe(k, t0, 100),
            FlowVerdict::Direct(DirectReason::Detecting)
        );
        assert_eq!(m.tunneled_count(), 0);
    }

    #[test]
    fn an_immediate_lock_still_rejects_a_foreign_pid() {
        // Locking early must not become a way to attribute someone else's
        // traffic: the PID filter runs before the hint is recorded.
        let t0 = Instant::now();
        let k = key(16, 50_012, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));
        m.hint_port_locking(50_012, 9999, true);

        assert_eq!(
            m.observe(k, t0, 100),
            FlowVerdict::Direct(DirectReason::Unattributed)
        );
        assert_eq!(m.tunneled_count(), 0);
    }

    #[test]
    fn hinted_flow_must_pass_the_debounce_before_it_locks() {
        let t0 = Instant::now();
        let k = key(12, 50_001, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));
        m.hint_port(50_001, 4242);

        // One packet creates the entry, but a hint is not an authoritative
        // observation, so the entry starts in detection.
        assert_eq!(
            m.observe(k, t0, 100),
            FlowVerdict::Direct(DirectReason::Detecting)
        );
        assert_eq!(m.pid_of(&k), Some(4242));
        assert_eq!(m.len(), 1);
        assert_eq!(
            m.observe(k, at(t0, 10), 100),
            FlowVerdict::Direct(DirectReason::Detecting)
        );
        // Third packet inside the window reaches TrackerConfig::detect_pkts.
        assert_eq!(m.observe(k, at(t0, 20), 100), FlowVerdict::Tunnel(k));
    }

    #[test]
    fn unconvincing_hint_never_locks_across_the_window() {
        let t0 = Instant::now();
        let k = key(13, 50_002, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), None);
        m.hint_port(50_002, 7);

        assert_eq!(
            m.observe(k, t0, 100),
            FlowVerdict::Direct(DirectReason::Detecting)
        );
        // 2 s apart: outside the 1500 ms window, the count keeps resetting.
        for i in 1..4 {
            assert_eq!(
                m.observe(k, at(t0, i * 2000), 100),
                FlowVerdict::Direct(DirectReason::Detecting)
            );
        }
        assert_eq!(m.tunneled_count(), 0);
    }

    #[test]
    fn zero_length_payload_never_advances_detection() {
        let t0 = Instant::now();
        let k = key(14, 50_003, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), None);
        m.hint_port(50_003, 7);

        for i in 0..5 {
            assert_eq!(
                m.observe(k, at(t0, i * 10), 0),
                FlowVerdict::Direct(DirectReason::Detecting)
            );
        }
        assert_eq!(
            m.tunneled_count(),
            0,
            "empty datagrams must never lock a flow"
        );
    }

    #[test]
    fn attributed_connect_locks_the_flow_without_a_debounce() {
        let t0 = Instant::now();
        let k = key(15, 50_004, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));

        let recorded = m.attribute(
            FlowObservation {
                key: k,
                pid: 4242,
                event: AttributionEvent::SocketConnect,
            },
            t0,
        );

        assert!(recorded);
        assert_eq!(m.observe(k, t0, 100), FlowVerdict::Tunnel(k));
    }

    #[test]
    fn bind_event_only_becomes_a_local_port_hint() {
        let t0 = Instant::now();
        // A BIND observation carries no remote endpoint.
        let bind = FlowObservation {
            key: key(16, 50_005, 0),
            pid: 4242,
            event: AttributionEvent::SocketBind,
        };
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));

        assert!(m.attribute(bind, t0));
        assert!(m.is_empty(), "a BIND must not create a flow entry");

        let k = key(17, 50_005, 7777);
        assert_eq!(
            m.observe(k, t0, 100),
            FlowVerdict::Direct(DirectReason::Detecting)
        );
        assert_eq!(
            m.observe(k, at(t0, 10), 100),
            FlowVerdict::Direct(DirectReason::Detecting)
        );
        assert_eq!(m.observe(k, at(t0, 20), 100), FlowVerdict::Tunnel(k));
    }

    #[test]
    fn attribute_rejects_another_process_flow() {
        let t0 = Instant::now();
        let k = key(18, 50_006, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));

        assert!(!m.attribute(obs(k, 999), t0));
        assert!(m.is_empty());
        assert_eq!(
            m.observe(k, t0, 100),
            FlowVerdict::Direct(DirectReason::Unattributed)
        );
    }

    // ── staleness ──────────────────────────────────────────────────────────
    #[test]
    fn silent_flow_is_dropped_after_five_seconds() {
        let t0 = Instant::now();
        let r = route(50_007, 19, 7777);
        let k = FlowKey::from(&r);
        let mut m = FlowMap::new(FlowMapConfig::default(), None);
        m.seed(7, std::slice::from_ref(&r), t0);
        assert_eq!(m.observe(k, t0, 100), FlowVerdict::Tunnel(k));

        assert_eq!(
            m.expire(at(t0, 4_000)),
            0,
            "4 s of silence is not stale yet"
        );
        assert_eq!(
            m.expire(at(t0, 6_000)),
            1,
            "6 s of silence expires the entry"
        );
        assert!(m.is_empty());
        assert_eq!(m.tunneled_count(), 0, "the tunnel slot is released with it");
        assert_eq!(
            m.observe(k, at(t0, 6_000), 100),
            FlowVerdict::Direct(DirectReason::Unattributed)
        );
    }

    #[test]
    fn traffic_refreshes_staleness() {
        let t0 = Instant::now();
        let r = route(50_008, 20, 7777);
        let k = FlowKey::from(&r);
        let mut m = FlowMap::new(FlowMapConfig::default(), None);
        m.seed(7, std::slice::from_ref(&r), t0);

        m.observe(k, t0, 100);
        m.observe(k, at(t0, 4_000), 100);
        assert_eq!(
            m.expire(at(t0, 6_000)),
            0,
            "the 4 s packet re-anchored the entry"
        );
        assert_eq!(m.observe(k, at(t0, 6_000), 100), FlowVerdict::Tunnel(k));
    }

    #[test]
    fn flow_stale_timeout_matches_tracker() {
        assert_eq!(FLOW_STALE_TIMEOUT, TrackerConfig::default().stale_timeout);
    }

    // ── the cap (OQ1), the warning and the ceiling ─────────────────────────
    #[test]
    fn relay_cap_and_warning_arithmetic_holds() {
        // Bind through the config (runtime values) so the assertions are about
        // the numbers the map actually uses, not about constants clippy can fold.
        let cfg = FlowMapConfig::default();
        let relay_threshold = RELAY_MAX_DESTINATIONS_PER_WINDOW;
        let cap = cfg.max_tunneled;
        let warn = cfg.warn_tunneled;

        // The client cap must sit below the relay's ban line with headroom.
        assert!(
            cap < relay_threshold,
            "cap {cap} must stay under {relay_threshold}"
        );
        assert_eq!(
            relay_threshold - cap,
            2,
            "headroom under the relay's 10-in-10 s ban"
        );
        // The warning must fire before the cap binds.
        assert!(warn < cap, "warning {warn} must fire before cap {cap}");
        assert_eq!(cap - warn, 2);
        // The tracked ceiling must not be below the tunnel cap.
        assert!(cap <= cfg.max_tracked);
    }

    #[test]
    fn cap_admits_at_most_eight_tunneled_flows() {
        let t0 = Instant::now();
        let mut m = FlowMap::new(FlowMapConfig::default(), None);

        for i in 0..MAX_TUNNELED_FLOWS as u8 {
            let k = key(i, 50_000 + i as u16, 7777);
            assert!(m.attribute(obs(k, 7), t0));
            assert_eq!(m.observe(k, t0, 100), FlowVerdict::Tunnel(k));
        }
        assert_eq!(m.tunneled_count(), MAX_TUNNELED_FLOWS);

        let k9 = key(100, 51_000, 7777);
        assert!(m.attribute(obs(k9, 7), t0));
        assert_eq!(
            m.observe(k9, t0, 100),
            FlowVerdict::Direct(DirectReason::Unattributed),
            "the ninth flow fails open and is reported UNATTRIBUTED"
        );
        let report = m.report();
        assert_eq!(report.tunneled, MAX_TUNNELED_FLOWS);
        assert_eq!(report.over_cap_packets, 1);
        assert_eq!(report.unattributed_flows, 1);
    }

    #[test]
    fn warning_appears_at_six_tunneled_flows() {
        let t0 = Instant::now();
        let mut m = FlowMap::new(FlowMapConfig::default(), None);

        for i in 0..WARN_TUNNELED_FLOWS as u8 - 1 {
            let k = key(i, 50_000 + i as u16, 7777);
            m.attribute(obs(k, 7), t0);
            m.observe(k, t0, 100);
        }
        assert_eq!(m.tunneled_count(), 5);
        assert!(
            m.report().warning.is_none(),
            "no warning below the threshold"
        );

        let k = key(50, 50_050, 7777);
        m.attribute(obs(k, 7), t0);
        assert_eq!(m.observe(k, t0, 100), FlowVerdict::Tunnel(k));

        let warning = m.report().warning.expect("warning at the threshold");
        assert_eq!(warning.tunneled, WARN_TUNNELED_FLOWS);
        assert_eq!(warning.cap, MAX_TUNNELED_FLOWS);
    }

    #[test]
    fn tracked_ceiling_bounds_the_map_at_sixty_four_entries() {
        let t0 = Instant::now();
        let mut m = FlowMap::new(FlowMapConfig::default(), None);

        let mut admitted = 0;
        for i in 0..65u8 {
            if m.attribute(obs(key(i, 50_000 + i as u16, 7777), 7), t0) {
                admitted += 1;
            }
        }
        assert_eq!(m.len(), MAX_TRACKED_ENTRIES);
        assert_eq!(admitted, MAX_TRACKED_ENTRIES);
    }

    #[test]
    fn retiring_a_tunneled_flow_frees_its_slot() {
        let t0 = Instant::now();
        let mut m = FlowMap::new(FlowMapConfig::default(), None);
        for i in 0..MAX_TUNNELED_FLOWS as u8 {
            let k = key(i, 50_000 + i as u16, 7777);
            m.attribute(obs(k, 7), t0);
            m.observe(k, t0, 100);
        }

        let first = key(0, 50_000, 7777);
        m.retire(&first);
        assert_eq!(m.tunneled_count(), MAX_TUNNELED_FLOWS - 1);

        let k9 = key(100, 51_000, 7777);
        m.attribute(obs(k9, 7), t0);
        assert_eq!(m.observe(k9, t0, 100), FlowVerdict::Tunnel(k9));
    }

    #[test]
    fn layer_delete_retires_the_flow() {
        let t0 = Instant::now();
        let k = key(21, 50_009, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), None);
        assert!(m.attribute(obs(k, 7), t0));
        assert_eq!(m.len(), 1);

        assert!(m.attribute(
            FlowObservation {
                key: k,
                pid: 7,
                event: AttributionEvent::FlowDeleted,
            },
            t0
        ));
        assert!(m.is_empty());
    }

    // ── fail-open / UNATTRIBUTED ───────────────────────────────────────────
    #[test]
    fn unattributable_flow_passes_direct_and_is_reported() {
        let t0 = Instant::now();
        let k = key(22, 50_010, 7777);
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));

        let verdict = m.observe(k, t0, 100);
        assert_eq!(verdict, FlowVerdict::Direct(DirectReason::Unattributed));
        assert!(!verdict.is_tunnel());

        let report = m.report();
        assert_eq!(report.unattributed_flows, 1);
        assert_eq!(report.tracked, 0, "an unattributed flow is not tracked");
        assert!(report.warning.is_none());
    }

    #[test]
    fn unattributed_set_is_bounded_and_counts_distinct_flows() {
        let t0 = Instant::now();
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));

        // The same unknown flow observed repeatedly counts once.
        let k = key(23, 50_011, 7777);
        for i in 0..3 {
            m.observe(k, at(t0, i * 10), 100);
        }
        assert_eq!(m.report().unattributed_flows, 1);

        // Distinct unknown flows each count, bounded by the tracked ceiling.
        for i in 0..80u8 {
            m.observe(key(i, 51_000 + i as u16, 7777), at(t0, 100), 100);
        }
        assert_eq!(m.report().unattributed_flows, MAX_TRACKED_ENTRIES);
    }

    // ── source selection ───────────────────────────────────────────────────
    #[test]
    fn auto_prefers_the_socket_layer_and_off_prefers_none() {
        assert_eq!(SourceKind::Auto.resolved(), AttributionLayer::Socket);
        assert_eq!(SourceKind::Socket.resolved(), AttributionLayer::Socket);
        assert_eq!(SourceKind::Flow.resolved(), AttributionLayer::Flow);
        assert_eq!(SourceKind::Off.resolved(), AttributionLayer::None);
    }

    #[test]
    fn select_source_returns_the_effective_layer_for_the_build() {
        for kind in [
            SourceKind::Auto,
            SourceKind::Flow,
            SourceKind::Socket,
            SourceKind::Off,
        ] {
            let source = select_source(kind);
            assert_eq!(
                source.layer(),
                kind.effective(),
                "select_source must match the build's effective layer for {kind:?}"
            );
        }
        // This build (no windivert-redirect in the default feature set) can host
        // no WinDivert layer, so every selection is the no-op.
        if !WIN_DIVERT_LAYERS_AVAILABLE {
            assert_eq!(SourceKind::Auto.effective(), AttributionLayer::None);
        }
    }

    #[test]
    fn noop_source_reports_nothing() {
        let mut source = NoopAttributionSource;
        assert_eq!(source.layer(), AttributionLayer::None);
        source.observe(obs(key(24, 50_012, 7777), 7));
        source.observe(FlowObservation {
            key: key(25, 50_013, 7777),
            pid: 7,
            event: AttributionEvent::SocketConnect,
        });
        assert!(source.drain().is_empty());
    }

    #[test]
    fn flow_source_accepts_only_flow_layer_events() {
        let mut source = FlowLayerSource::default();
        let k = key(26, 50_014, 7777);
        assert_eq!(source.layer(), AttributionLayer::Flow);

        source.observe(FlowObservation {
            key: k,
            pid: 7,
            event: AttributionEvent::SocketConnect,
        });
        assert!(
            source.drain().is_empty(),
            "a SOCKET event is not FLOW evidence"
        );

        source.observe(FlowObservation {
            key: k,
            pid: 7,
            event: AttributionEvent::FlowEstablished,
        });
        assert_eq!(source.drain().len(), 1);
        assert!(source.drain().is_empty(), "draining empties the queue");
    }

    #[test]
    fn socket_source_accepts_only_socket_layer_events() {
        let mut source = SocketLayerSource::default();
        let k = key(27, 50_015, 7777);
        assert_eq!(source.layer(), AttributionLayer::Socket);

        source.observe(FlowObservation {
            key: k,
            pid: 7,
            event: AttributionEvent::FlowEstablished,
        });
        assert!(
            source.drain().is_empty(),
            "a FLOW event is not SOCKET evidence"
        );

        for event in [
            AttributionEvent::SocketConnect,
            AttributionEvent::SocketBind,
        ] {
            source.observe(FlowObservation {
                key: k,
                pid: 7,
                event,
            });
        }
        assert_eq!(source.drain().len(), 2);
    }

    #[test]
    fn drained_observation_attributes_a_flow_end_to_end() {
        let t0 = Instant::now();
        let k = key(28, 50_016, 7777);
        let mut source = SocketLayerSource::default();
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));

        source.observe(FlowObservation {
            key: k,
            pid: 4242,
            event: AttributionEvent::SocketConnect,
        });
        for observation in source.drain() {
            assert!(m.attribute(observation, t0));
        }

        assert_eq!(m.observe(k, t0, 100), FlowVerdict::Tunnel(k));
        assert_eq!(m.pid_of(&k), Some(4242));
    }

    #[test]
    fn fail_open_verdicts_cover_every_path() {
        // The map returns exactly two verdict shapes, and both re-inject or
        // relay: there is no drop variant to reach, by construction.
        let t0 = Instant::now();
        let mut m = FlowMap::new(FlowMapConfig::default(), Some(4242));
        let k = key(29, 50_017, 7777);

        // Unattributable, then hinted-but-not-yet-locked: both direct.
        assert_eq!(
            m.observe(k, t0, 100),
            FlowVerdict::Direct(DirectReason::Unattributed)
        );
        m.hint_port(50_017, 4242);
        assert_eq!(
            m.observe(k, t0, 100),
            FlowVerdict::Direct(DirectReason::Detecting)
        );

        // Locked on the third packet: relayed.
        let _ = m.observe(k, at(t0, 10), 100);
        assert!(m.observe(k, at(t0, 20), 100).is_tunnel());
    }
}
