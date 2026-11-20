//! Pure, I/O-free server-tracking state machine for WinDivert interception.
//!
//! The Windows interceptor has to answer one question for every captured
//! outbound packet: *should this packet be tunnelled to the proxy, or
//! re-injected onto the wire untouched?* Historically that decision was
//! duplicated as ad-hoc mutable state inside two blocking receive loops
//! ([`crate::interceptor::windows`] and
//! [`crate::capture::windivert_redirect`]), and the duplication carried a
//! live bug: a server pre-seeded from `initial_routes.first()` was never
//! considered stale because `last_server_pkt` stayed `None` until a packet
//! matched its exact address, so the interceptor locked onto a dead lobby IP
//! forever and passed live-match traffic through untunnelled.
//!
//! This module is the single source of truth for that decision. It performs no
//! I/O and takes `now` as an explicit parameter, so the full state machine is
//! deterministically testable on every platform (the WinDivert loops
//! themselves are Windows-only and cannot run under CI on Linux).
//!
//! ## Flow-aware candidates (the second live bug)
//!
//! The port-scoped path (the built-in game path) captures every host UDP
//! datagram whose destination port falls inside the game profile's range. The
//! original detector treated every captured destination as a candidate, so
//! after five seconds of match silence any unrelated high-port flow could take
//! the lock: the field session re-locked to an AWS endpoint that was not the
//! game server and then starved the relay (`NoReplyFromRelay`) for it. The
//! same code path had no notion of private destinations either.
//!
//! Detection is therefore **flow-aware**: the caller reports, per packet, what
//! the OS socket table proves about the packet's owner ([`FlowOwner`]) and,
//! when the app layer has that knowledge, the destination's hosting-network
//! class ([`ProviderClass`]). The state machine then:
//!
//! - never locks a non-public destination ([`is_lockable_destination`]);
//! - only treats a packet as a candidate when it is attributable to the game
//!   process, or (attribution lagging/unavailable) when it is on the pinned
//!   server's own hosting network;
//! - keeps the pin when a stale lock sees an unrelated flow: going quiet is not
//!   proof the match moved, so a stale lock is replaced only by a candidate the
//!   capture can attribute to the game process (or, when no process is known,
//!   one on the same hosting network);
//! - clears the pin when the caller reports the game process restarted under a
//!   new PID ([`ServerTracker::set_game_pid`]);
//! - prefers the candidate on the pinned server's hosting network when several
//!   attributable candidates compete, bounded so preference can never turn
//!   into a detector that never locks.
//!
//! The attribution sources behind [`FlowOwner`] are the OS socket table
//! (`GetExtendedUdpTable`, which names the PID bound to a local port) and the
//! app-installed provider classifier ([`install_server_provider_lookup`]). The
//! WinDivert network layer the interceptor reads cannot match or report a
//! process id itself; this module never pretends otherwise, and a capture with
//! no attribution at all keeps the historical rule (documented on
//! [`FlowOwner`]).

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::RwLock;
use std::time::{Duration, Instant};

// ─────────────────────────────────────────────────────────────────────────────
//  Public types
// ─────────────────────────────────────────────────────────────────────────────

/// What the caller should do with the packet that was just observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    /// Forward this packet to the locked `server` through the tunnel.
    Tunnel(SocketAddrV4),
    /// Re-inject the packet unchanged; no interesting state transition.
    PassThrough,
    /// Entering a fresh detection window: re-inject and keep accumulating.
    StartDetection,
    /// Sample this packet as a like-for-like direct application RTT probe.
    ///
    /// The caller re-injects the game's OWN packet UNCHANGED onto the direct
    /// path: no synthetic packet is generated and no payload is rewritten,
    /// which is the anti-cheat safety property. The sampler times the reply
    /// on a sniff handle, so from the game's point of view nothing changed.
    /// Distinct from [`PassThrough`](Decision::PassThrough) so the detection
    /// state machine's semantics stay untouched.
    ShadowDirect,
    /// The previous lock expired (server went stale); the caller must tear down
    /// per-session cached state (e.g. the inject interface cache) before this
    /// packet, which is re-injected and begins a new detection window.
    ResetToDetection,
}

/// Tuning for the detection and staleness timers.
#[derive(Debug, Clone, Copy)]
pub struct TrackerConfig {
    /// Packets to one destination required before locking onto it.
    pub detect_pkts: u8,
    /// Detection window: candidate counts older than this are discarded.
    pub detect_window: Duration,
    /// Silence (from either the last packet or the lock time) after which a
    /// locked server is considered gone.
    pub stale_timeout: Duration,
}

impl Default for TrackerConfig {
    fn default() -> Self {
        // Matches the historical WinDivert constants: 3 packets within 1500 ms
        // lock a server; 5 s of silence releases it.
        Self {
            detect_pkts: 3,
            detect_window: Duration::from_millis(1_500),
            stale_timeout: Duration::from_secs(5),
        }
    }
}

/// The most candidate endpoints tracked at once.
///
/// Bounded so a busy host cannot grow the detector's memory no matter what the
/// capture filter produces. Admission is already gated on ownership/network, so
/// this only ever holds the game's own flows (a title can run several).
const MAX_CANDIDATES: usize = 8;

/// Who the capture can prove sent a captured packet.
///
/// The Windows network layer hands us the packet, not its process. The OS
/// socket table (`GetExtendedUdpTable`) does answer "which PID binds this local
/// port", which the caller turns into one of these three values per packet.
/// [`Unknown`](FlowOwner::Unknown) is deliberately not a synonym for
/// [`Foreign`](FlowOwner::Foreign): the table can lag a freshly opened socket,
/// and a caller with no process target at all has no owners to compare. The
/// state machine treats a may-be-ours packet as unproven rather than hostile,
/// unless no process attribution is expected at all - then it keeps the
/// historical acceptance rule so the Manual/Custom path does not regress.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlowOwner {
    /// The live socket table attributes this packet's local port to the
    /// detected game process (and to no other process).
    Game,
    /// The live socket table attributes the local port to another process only.
    Foreign,
    /// Nothing proves either way: no process known, table miss, or a port
    /// shared by several processes.
    Unknown,
}

/// Coarse hosting-network class of a public IPv4 address.
///
/// The detector only ever compares two classes for **equality** ("is this
/// candidate on the pinned server's network?"); it never interprets the value,
/// so the app layer is free to tag its own indexes any way it likes. The GUI
/// tags cloud-provider ranges and hosting ASNs (see
/// [`install_server_provider_lookup`]).
pub type ProviderClass = u64;

/// A classifier the app layer installs so detection can tell whether a
/// candidate endpoint is on the same hosting network as the locked server.
///
/// `None` means "no opinion" for that address; the detector then falls back to
/// the weaker rules documented on [`ServerTracker::observe_attributed`].
pub type ProviderLookup = fn(Ipv4Addr) -> Option<ProviderClass>;

/// The process-wide provider classifier, if the app layer installed one.
///
/// A function pointer behind a `RwLock` is the same bridge shape the session
/// globals already use: the detector (`client`) cannot reach the shipped
/// location indexes (`client-gui`), so the app pushes the classifier down once
/// at startup instead of the client carrying a second copy of the data.
static SERVER_PROVIDER_LOOKUP: RwLock<Option<ProviderLookup>> = RwLock::new(None);

/// Install the provider classifier the process detector consults.
///
/// Safe to call more than once; the last classifier wins. Not calling it is
/// valid: detection then never sees a provider class and keeps the pid-based
/// rules only.
pub fn install_server_provider_lookup(lookup: ProviderLookup) {
    let mut slot = SERVER_PROVIDER_LOOKUP
        .write()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    *slot = Some(lookup);
}

/// The installed classifier's answer for `ip`, or `None` when none is installed
/// or it has no opinion.
pub fn server_provider_class(ip: Ipv4Addr) -> Option<ProviderClass> {
    let slot = SERVER_PROVIDER_LOOKUP
        .read()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    (*slot).and_then(|lookup| lookup(ip))
}

/// Whether `dst` may ever be a game server.
///
/// A non-public destination - loopback, RFC 1918 private, link-local, CGNAT,
/// multicast, unspecified, broadcast - is answered on the machine or inside the
/// local network and can never be relayed to. Locking one is always a defect:
/// the relay will starve for it, and the GUI then shows a location for an
/// address that is not a game server at all.
///
/// Test/documentation ranges stay lockable on purpose: this repository's test
/// fixtures use TEST-NET addresses, and they never appear in a real capture.
pub fn is_lockable_destination(dst: SocketAddrV4) -> bool {
    let ip = *dst.ip();
    if ip.is_unspecified()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_multicast()
        || ip.is_broadcast()
    {
        return false;
    }
    let [a, b, _, _] = ip.octets();
    // 0.0.0.0/8 "this network".
    if a == 0 {
        return false;
    }
    // 10.0.0.0/8.
    if a == 10 {
        return false;
    }
    // 100.64.0.0/10 carrier-grade NAT.
    if a == 100 && (64..=127).contains(&b) {
        return false;
    }
    // 172.16.0.0/12.
    if a == 172 && (16..=31).contains(&b) {
        return false;
    }
    // 192.168.0.0/16.
    if a == 192 && b == 168 {
        return false;
    }
    true
}

/// Why a candidate endpoint was refused the lock.
///
/// The caller logs these (the state machine performs no I/O): the field bug was
/// undiagnosable precisely because a lock change and its reason were invisible.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectReason {
    /// Loopback/private/link-local/CGNAT/multicast/unspecified: never a server.
    NonPublicDestination,
    /// The OS socket table proves another process owns the flow.
    ForeignProcess,
    /// A game process is known, but this flow could not be attributed to it.
    UnknownOwner,
    /// The flow is on a known hosting network other than the pinned server's.
    ProviderMismatch,
}

impl std::fmt::Display for RejectReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::NonPublicDestination => "not a public destination",
            Self::ForeignProcess => "owned by another process",
            Self::UnknownOwner => "not attributable to the game process",
            Self::ProviderMismatch => "different hosting network",
        })
    }
}

/// Why a candidate endpoint was accepted as the lock.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcceptReason {
    /// The packet's socket belongs to the detected game process.
    GameProcess,
    /// Same, and the endpoint is on the pinned server's hosting network.
    GameProcessSameNetwork,
    /// Unattributed, but on the pinned server's hosting network.
    ProviderMatch,
    /// No process attribution is expected on this path: the historical rule.
    Unattributed,
}

impl std::fmt::Display for AcceptReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::GameProcess => "game process",
            Self::GameProcessSameNetwork => "game process, same hosting network",
            Self::ProviderMatch => "same hosting network as the previous server",
            Self::Unattributed => "no process attribution on this path",
        })
    }
}

/// One captured packet, as far as detection is concerned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PacketObservation {
    /// The captured destination (the presumed game server).
    pub dst: SocketAddrV4,
    /// Payload length; a zero-length datagram never advances detection.
    pub payload_len: usize,
    /// What the caller can prove about the packet's owner.
    pub owner: FlowOwner,
    /// Hosting-network class of `dst`, when the app can classify it.
    pub provider: Option<ProviderClass>,
}

/// The detector's answer for one packet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Detection {
    /// What the caller should do with the packet.
    pub decision: Decision,
    /// Why this packet's destination was refused as a candidate, when it was.
    pub rejected: Option<RejectReason>,
    /// Why this packet committed a new lock, when it did.
    pub accepted: Option<AcceptReason>,
}

impl Detection {
    /// A verdict with no candidate reasoning attached.
    const fn plain(decision: Decision) -> Self {
        Self {
            decision,
            rejected: None,
            accepted: None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Candidate {
    addr: SocketAddrV4,
    count: u8,
    first_seen: Instant,
    /// Lower is preferred; see [`ServerTracker::classify`].
    rank: u8,
    provider: Option<ProviderClass>,
    accepted: AcceptReason,
}

/// Server-tracking state machine.
///
/// Feed it every captured outbound packet in order via [`observe`] (ports path)
/// or [`observe_attributed`] (flow-aware paths) and act on the returned
/// [`Decision`].
///
/// [`observe`]: ServerTracker::observe
/// [`observe_attributed`]: ServerTracker::observe_attributed
#[derive(Debug)]
pub struct ServerTracker {
    cfg: TrackerConfig,
    locked: Option<SocketAddrV4>,
    locked_at: Option<Instant>,
    last_server_pkt: Option<Instant>,
    candidates: Vec<Candidate>,
    seeds: Vec<SocketAddrV4>,
    /// Live PID of the detected game process, when the caller resolved one.
    game_pid: Option<u32>,
    /// Hosting-network class of the locked server, when the caller knew it.
    locked_provider: Option<ProviderClass>,
    /// The hosting network a replacement candidate is compared against.
    ///
    /// This is not the lock itself: when a stale lock is released to re-detect,
    /// the pinned server's network must survive the release so the candidates
    /// that follow can still be preferred by network. It is set from the lock on
    /// release and from the winning candidate on commit, and cleared when the
    /// game process restarts (a new process knows nothing about the old match).
    detect_baseline: Option<ProviderClass>,
    /// When a ready candidate is being held back in favour of a better-ranked
    /// one; bounds the hold so preference can never deadlock the detector.
    preferred_since: Option<Instant>,
}

/// Compute the seed routes the tracker should pre-lock onto.
///
/// Games with `dynamic_server() == true` (e.g. Fortnite) rotate through
/// ephemeral AWS addresses, so a discovered route is worthless the moment the
/// match starts: such games must begin in auto-detect and ignore the seeds.
/// Static games keep the legacy behaviour of starting from the discovered
/// routes. The *whole* route list is returned rather than collapsing to
/// `.first()` so callers never encode that assumption themselves.
pub fn effective_seeds(dynamic: bool, initial_routes: &[SocketAddrV4]) -> Vec<SocketAddrV4> {
    if dynamic {
        Vec::new()
    } else {
        initial_routes.to_vec()
    }
}

impl ServerTracker {
    /// Create an unlocked tracker.
    pub fn new(cfg: TrackerConfig) -> Self {
        Self {
            cfg,
            locked: None,
            locked_at: None,
            last_server_pkt: None,
            candidates: Vec::new(),
            seeds: Vec::new(),
            game_pid: None,
            locked_provider: None,
            detect_baseline: None,
            preferred_since: None,
        }
    }

    /// Pre-seed a lock. `routes[0]`, if present, becomes the locked server and
    /// the stale timer is anchored at `now` so a pre-seed that never receives
    /// a packet still expires.
    pub fn seed(&mut self, routes: &[SocketAddrV4], now: Instant) {
        self.seeds = routes.to_vec();
        if let Some(first) = routes.first().copied() {
            self.locked = Some(first);
            self.locked_at = Some(now);
            self.last_server_pkt = None;
            self.locked_provider = None;
            self.detect_baseline = None;
        }
    }

    /// Record the hosting-network class of the pre-seeded lock.
    ///
    /// Only meaningful for a seeded lock (`seed`): a lock earned through
    /// observation stores the class of the candidate that earned it.
    pub fn note_seed_provider(&mut self, provider: Option<ProviderClass>) {
        if self.locked.is_some() && self.locked_at.is_some() {
            self.locked_provider = provider;
            self.detect_baseline = provider;
        }
    }

    /// Tell the tracker which PID is the live game process.
    ///
    /// Two things depend on this: ownership acceptance (`Game` vs `Foreign`),
    /// and game restarts. A PID that differs from the one already recorded is a
    /// restart - the old process is gone, so its pinned server says nothing
    /// about the new process - and the lock plus detection state are cleared.
    /// `None` withdraws the attribution requirement (the caller gave up on
    /// resolving the process) but keeps the lock: losing the ability to
    /// attribute is not proof the match moved.
    ///
    /// Returns `true` when this call detected a restart.
    pub fn set_game_pid(&mut self, pid: Option<u32>) -> bool {
        match pid {
            None => {
                self.game_pid = None;
                false
            }
            Some(new) => match self.game_pid {
                Some(old) if old == new => false,
                Some(_) => {
                    self.game_pid = Some(new);
                    self.release_lock();
                    self.candidates.clear();
                    self.detect_baseline = None;
                    self.preferred_since = None;
                    true
                }
                None => {
                    self.game_pid = Some(new);
                    false
                }
            },
        }
    }

    /// The game PID detection is currently attributing to, if any.
    pub fn game_pid(&self) -> Option<u32> {
        self.game_pid
    }

    /// The currently locked server, if any.
    pub fn locked_server(&self) -> Option<SocketAddrV4> {
        self.locked
    }

    /// The hosting-network class recorded with the current lock, if known.
    pub fn locked_provider(&self) -> Option<ProviderClass> {
        self.locked_provider
    }

    /// Whether a server is currently locked.
    pub fn is_locked(&self) -> bool {
        self.locked.is_some()
    }

    /// The routes supplied to [`seed`], if any.
    ///
    /// [`seed`]: ServerTracker::seed
    pub fn seeded_routes(&self) -> &[SocketAddrV4] {
        &self.seeds
    }

    /// Number of live detection candidates.
    pub fn candidate_count(&self) -> usize {
        self.candidates.len()
    }

    /// Observe one outbound packet and return the routing decision.
    ///
    /// Legacy/ports-context form: no ownership or provider knowledge is
    /// supplied, so this is exactly the historical state machine (a tracker
    /// whose [`set_game_pid`] was never called accepts candidates without
    /// attribution). Non-public destinations are still refused, which is a
    /// safety rule rather than an attribution rule.
    ///
    /// [`set_game_pid`]: ServerTracker::set_game_pid
    pub fn observe(
        &mut self,
        observed_dst: SocketAddrV4,
        now: Instant,
        payload_len: usize,
    ) -> Decision {
        self.observe_attributed(
            PacketObservation {
                dst: observed_dst,
                payload_len,
                owner: FlowOwner::Unknown,
                provider: None,
            },
            now,
        )
        .decision
    }

    /// Observe one outbound packet with the caller's ownership/provider
    /// knowledge and return the routing decision plus the candidate reasoning.
    ///
    /// Decision order, and why the pin survives a quiet match:
    ///
    /// 1. A zero-length datagram never counts ([`Decision::PassThrough`]).
    /// 2. A non-public destination is refused ([`RejectReason::NonPublicDestination`])
    ///    and can neither become a lock nor stay one.
    /// 3. A packet to the locked server is tunnelled.
    /// 4. A packet elsewhere while the lock is fresh is re-injected untouched.
    /// 5. A packet elsewhere once the lock is stale **is only a candidate when
    ///    the capture attributes it to the game process** (or, with no process
    ///    known, when it is on the pinned server's hosting network). Anything
    ///    else is re-injected with a [`RejectReason`] and the lock is kept: a
    ///    quiet server is not proof the match moved, and an unrelated high-port
    ///    flow must never be able to take the session.
    /// 6. Only then is the stale lock released and the normal 3-packets-in-1.5 s
    ///    window run over attributable candidates. Among several candidates the
    ///    one on the pinned server's network is preferred, bounded so a hold can
    ///    never exceed one detection window.
    pub fn observe_attributed(&mut self, obs: PacketObservation, now: Instant) -> Detection {
        if obs.payload_len == 0 {
            return Detection::plain(Decision::PassThrough);
        }

        // (2) A non-public destination is never a game server. Refuse it as a
        // candidate, and if a seed ever locked one, drop that lock: relaying to
        // an address the relay cannot reach only starves the session.
        if !is_lockable_destination(obs.dst) {
            if self.locked == Some(obs.dst) {
                self.release_lock();
            }
            return Detection {
                decision: Decision::PassThrough,
                rejected: Some(RejectReason::NonPublicDestination),
                accepted: None,
            };
        }

        if let Some(server) = self.locked {
            // (3) The pinned server itself: keep tunnelling and refresh it.
            if obs.dst == server {
                self.last_server_pkt = Some(now);
                return Detection::plain(Decision::Tunnel(server));
            }

            let anchor = self.last_server_pkt.or(self.locked_at);
            let stale =
                anchor.is_none_or(|t| now.saturating_duration_since(t) > self.cfg.stale_timeout);
            // (4) The old server is still active: a new destination is a lobby
            // or backend flow, not a replacement. Pass it through untouched.
            if !stale {
                return Detection::plain(Decision::PassThrough);
            }

            // (5) Stale lock: only an attributable candidate may replace it.
            let Some((rank, accepted)) = self.classify(obs.owner, obs.provider) else {
                let rejected = self.reject_reason(obs.owner, obs.provider);
                return Detection {
                    decision: Decision::PassThrough,
                    rejected: Some(rejected),
                    accepted: None,
                };
            };
            // The pinned server's network survives the lock it was learned
            // from: the candidates that follow are compared against it.
            self.detect_baseline = self.locked_provider.or(self.detect_baseline);
            self.release_lock();
            self.candidates.clear();
            self.note_candidate(obs.dst, now, rank, accepted, obs.provider);
            if let Some(winner) = self.take_commit(now) {
                return self.commit(winner, now);
            }
            // The one-shot "the pin went quiet, a replacement is being
            // confirmed" signal: the caller tears down per-session cache state.
            return Detection::plain(Decision::ResetToDetection);
        }

        // (6) Unlocked: accumulate candidates that pass the same gate.
        let Some((rank, accepted)) = self.classify(obs.owner, obs.provider) else {
            let rejected = self.reject_reason(obs.owner, obs.provider);
            return Detection {
                decision: Decision::PassThrough,
                rejected: Some(rejected),
                accepted: None,
            };
        };
        self.expire_candidates(now);
        let started = self.candidates.is_empty();
        self.note_candidate(obs.dst, now, rank, accepted, obs.provider);
        if let Some(winner) = self.take_commit(now) {
            return self.commit(winner, now);
        }
        if started {
            return Detection::plain(Decision::StartDetection);
        }
        Detection::plain(Decision::PassThrough)
    }

    /// The hosting network candidates are compared against: the lock's when
    /// there is one, else the last pinned server's (survives a stale release so
    /// the re-detect window still prefers the established network).
    fn network_baseline(&self) -> Option<ProviderClass> {
        self.locked_provider.or(self.detect_baseline)
    }

    /// Judge one packet as a candidate, or say why it cannot be one.
    ///
    /// Rank order (lower wins; only ever compared within one detection window):
    /// 0 - attributable to the game process (same network when both known, or no
    ///     network knowledge), or unattributed but on the pinned server's
    ///     network in legacy mode;
    /// 1 - attributable to the game process but on a *different* known network.
    ///     Accepted: a title that rotates between hosting networks still has to
    ///     be followed, and the process attribution is the stronger evidence. It
    ///     simply loses a tie to a candidate on the established network;
    /// 2 - last-resort unattributed acceptance in legacy mode.
    fn classify(
        &self,
        owner: FlowOwner,
        provider: Option<ProviderClass>,
    ) -> Option<(u8, AcceptReason)> {
        let baseline = self.network_baseline();
        let same_network = matches!(
            (provider, baseline),
            (Some(candidate), Some(pinned)) if candidate == pinned
        );
        let different_network = matches!(
            (provider, baseline),
            (Some(candidate), Some(pinned)) if candidate != pinned
        );
        match owner {
            FlowOwner::Foreign => None,
            FlowOwner::Game => Some(if same_network {
                (0, AcceptReason::GameProcessSameNetwork)
            } else if different_network {
                (1, AcceptReason::GameProcess)
            } else {
                (0, AcceptReason::GameProcess)
            }),
            FlowOwner::Unknown => {
                if same_network {
                    Some((0, AcceptReason::ProviderMatch))
                } else if different_network {
                    None
                } else if self.game_pid.is_some() {
                    // A process is known and this is not attributable: unproven,
                    // so not a candidate.
                    None
                } else {
                    // No attribution is expected on this path at all: the
                    // historical rule, kept so Manual/Custom detection does not
                    // regress.
                    Some((2, AcceptReason::Unattributed))
                }
            }
        }
    }

    /// The reason [`classify`](Self::classify) refused this packet.
    fn reject_reason(&self, owner: FlowOwner, provider: Option<ProviderClass>) -> RejectReason {
        match owner {
            FlowOwner::Foreign => RejectReason::ForeignProcess,
            FlowOwner::Game => RejectReason::UnknownOwner,
            FlowOwner::Unknown => {
                let different_network = matches!(
                    (provider, self.network_baseline()),
                    (Some(candidate), Some(pinned)) if candidate != pinned
                );
                if different_network {
                    RejectReason::ProviderMismatch
                } else {
                    RejectReason::UnknownOwner
                }
            }
        }
    }

    /// Drop the lock and every timer anchored to it.
    fn release_lock(&mut self) {
        self.locked = None;
        self.locked_at = None;
        self.last_server_pkt = None;
        self.locked_provider = None;
    }

    fn expire_candidates(&mut self, now: Instant) {
        let window = self.cfg.detect_window;
        self.candidates
            .retain(|c| now.saturating_duration_since(c.first_seen) < window);
        if self.candidates.is_empty() {
            self.preferred_since = None;
        }
    }

    /// Count one packet towards `dst`. Returns whether that count reached the
    /// lock threshold (the preference check happens in [`take_commit`]).
    ///
    /// [`take_commit`]: Self::take_commit
    fn note_candidate(
        &mut self,
        dst: SocketAddrV4,
        now: Instant,
        rank: u8,
        accepted: AcceptReason,
        provider: Option<ProviderClass>,
    ) -> bool {
        if let Some(candidate) = self.candidates.iter_mut().find(|c| c.addr == dst) {
            candidate.count = candidate.count.saturating_add(1);
            // Later packets can improve the ranking (a provider table that was
            // not yet warm), never worsen it.
            if rank < candidate.rank {
                candidate.rank = rank;
                candidate.accepted = accepted;
            }
            candidate.provider = candidate.provider.or(provider);
            return candidate.count >= self.cfg.detect_pkts;
        }
        if self.candidates.len() >= MAX_CANDIDATES {
            return false;
        }
        self.candidates.push(Candidate {
            addr: dst,
            count: 1,
            first_seen: now,
            rank,
            provider,
            accepted,
        });
        self.cfg.detect_pkts <= 1
    }

    /// Pick the candidate that should take the lock, if one is ready.
    ///
    /// Preference is a hold, not a veto: a ready candidate waits while a
    /// better-ranked candidate is still live in the same window, but never for
    /// longer than [`TrackerConfig::detect_window`], so a trickling flow cannot
    /// keep the detector from ever locking.
    fn take_commit(&mut self, now: Instant) -> Option<Candidate> {
        let winner = self
            .candidates
            .iter()
            .enumerate()
            .filter(|(_, candidate)| candidate.count >= self.cfg.detect_pkts)
            .min_by_key(|(_, candidate)| (candidate.rank, candidate.first_seen))
            .map(|(index, _)| index)?;

        let best_rank = self.candidates[winner].rank;
        let better_live = self
            .candidates
            .iter()
            .enumerate()
            .any(|(index, candidate)| index != winner && candidate.rank < best_rank);
        if better_live {
            let since = *self.preferred_since.get_or_insert(now);
            if now.saturating_duration_since(since) < self.cfg.detect_window {
                return None;
            }
        }
        self.preferred_since = None;
        Some(self.candidates.swap_remove(winner))
    }

    /// Install `winner` as the lock and report why it was accepted.
    fn commit(&mut self, winner: Candidate, now: Instant) -> Detection {
        self.locked = Some(winner.addr);
        self.locked_at = Some(now);
        self.last_server_pkt = Some(now);
        self.locked_provider = winner.provider;
        self.detect_baseline = winner.provider;
        self.candidates.clear();
        self.preferred_since = None;
        Detection {
            decision: Decision::Tunnel(winner.addr),
            rejected: None,
            accepted: Some(winner.accepted),
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

    /// A neutral, public test destination (TEST-NET-3). Test fixtures in this
    /// repository stay on documentation ranges.
    fn addr(a: u8, port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, a), port)
    }

    fn at(base: Instant, ms: u64) -> Instant {
        base + Duration::from_millis(ms)
    }

    fn tracker() -> ServerTracker {
        ServerTracker::new(TrackerConfig::default())
    }

    /// One packet from `dst`, owned by `owner`, no provider knowledge.
    fn obs(dst: SocketAddrV4, owner: FlowOwner) -> PacketObservation {
        PacketObservation {
            dst,
            payload_len: 100,
            owner,
            provider: None,
        }
    }

    /// One packet from `dst`, owned by `owner`, on `provider`.
    fn obs_on(dst: SocketAddrV4, owner: FlowOwner, provider: u64) -> PacketObservation {
        PacketObservation {
            provider: Some(provider),
            ..obs(dst, owner)
        }
    }

    /// A tracker pinned to `server` by three game-process packets, with the
    /// game process resolved to `pid`. Returns the tracker and the instant the
    /// lock was earned at, so tests can advance time from it.
    fn locked_on(server: SocketAddrV4, pid: u32) -> (ServerTracker, Instant) {
        let t0 = Instant::now();
        let mut tracker = tracker();
        assert!(
            !tracker.set_game_pid(Some(pid)),
            "the first PID resolution is not a restart"
        );
        for i in 0..3 {
            tracker.observe_attributed(obs(server, FlowOwner::Game), at(t0, i * 10));
        }
        assert_eq!(tracker.locked_server(), Some(server));
        (tracker, t0)
    }

    // ── (a) single-server lock after N packets within the window ───────────
    #[test]
    fn three_packets_within_window_lock_and_tunnel() {
        let t0 = Instant::now();
        let a = addr(1, 7777);
        let mut t = tracker();

        assert_eq!(t.observe(a, t0, 100), Decision::StartDetection);
        assert_eq!(t.observe(a, at(t0, 10), 100), Decision::PassThrough);
        assert_eq!(t.observe(a, at(t0, 20), 100), Decision::Tunnel(a));
        assert_eq!(t.locked_server(), Some(a));
        assert!(t.is_locked());

        // Subsequent packets to the locked server keep tunnelling.
        assert_eq!(t.observe(a, at(t0, 30), 100), Decision::Tunnel(a));
    }

    #[test]
    fn packets_spread_past_window_never_lock() {
        let t0 = Instant::now();
        let a = addr(2, 7777);
        let mut t = tracker();

        assert_eq!(t.observe(a, t0, 100), Decision::StartDetection);
        // 2 s apart > 1500 ms window: the count keeps resetting.
        assert_eq!(t.observe(a, at(t0, 2000), 100), Decision::StartDetection);
        assert_eq!(t.observe(a, at(t0, 4000), 100), Decision::StartDetection);
        assert!(!t.is_locked(), "spread-out packets must not lock a server");
    }

    // ── (b) stale after 5 s of silence triggers re-detect ──────────────────
    #[test]
    fn stale_after_silence_triggers_reset_and_redetect() {
        let t0 = Instant::now();
        let a = addr(3, 7777);
        let b = addr(4, 7777);
        let mut t = tracker();

        for i in 0..3 {
            t.observe(a, at(t0, i * 10), 100);
        }
        assert_eq!(t.locked_server(), Some(a));

        // A still sending, then 6 s of silence, then traffic to B.
        assert_eq!(t.observe(a, at(t0, 100), 100), Decision::Tunnel(a));
        assert_eq!(t.observe(b, at(t0, 6100), 100), Decision::ResetToDetection);
        assert!(!t.is_locked(), "stale lock must be released");

        assert_eq!(t.observe(b, at(t0, 6110), 100), Decision::PassThrough);
        assert_eq!(t.observe(b, at(t0, 6120), 100), Decision::Tunnel(b));
        assert_eq!(t.locked_server(), Some(b));
    }

    // ── (c) PRE-SEEDED server that never receives a packet (core bug) ──────
    #[test]
    fn preseeded_server_that_never_packets_goes_stale_and_redetects() {
        let t0 = Instant::now();
        let dead = addr(5, 7777);
        let live = addr(6, 7777);
        let mut t = tracker();
        t.seed(&[dead], t0);

        assert_eq!(t.locked_server(), Some(dead));
        assert!(t.is_locked());

        // No packet ever reaches `dead`. Just 6 s later, traffic to the live
        // server must force a reset rather than passing through forever.
        assert_eq!(
            t.observe(live, at(t0, 6000), 100),
            Decision::ResetToDetection
        );
        assert!(!t.is_locked());

        assert_eq!(t.observe(live, at(t0, 6010), 100), Decision::PassThrough);
        assert_eq!(t.observe(live, at(t0, 6020), 100), Decision::Tunnel(live));
        assert_eq!(t.locked_server(), Some(live));
    }

    #[test]
    fn preseeded_server_tunnels_on_first_matching_packet() {
        let t0 = Instant::now();
        let server = addr(7, 7777);
        let mut t = tracker();
        t.seed(&[server], t0);

        // The stale timer must NOT release a server that is actively talking.
        assert_eq!(t.observe(server, at(t0, 10), 100), Decision::Tunnel(server));
    }

    // ── (d) destination change while the old server is still sending ───────
    #[test]
    fn destination_change_while_old_server_active_passes_through() {
        let t0 = Instant::now();
        let a = addr(8, 7777);
        let b = addr(9, 7777);
        let mut t = tracker();
        for i in 0..3 {
            t.observe(a, at(t0, i * 10), 100);
        }
        assert_eq!(t.locked_server(), Some(a));

        // B appears while A is fresh: pass it through, stay locked on A.
        assert_eq!(t.observe(b, at(t0, 100), 100), Decision::PassThrough);
        assert_eq!(t.locked_server(), Some(a));
        assert_eq!(t.observe(a, at(t0, 200), 100), Decision::Tunnel(a));
        assert_eq!(t.locked_server(), Some(a));
    }

    // ── (e) multi-route candidate set ──────────────────────────────────────
    #[test]
    fn multi_route_candidates_accumulate_independently() {
        let t0 = Instant::now();
        let a = addr(10, 7777);
        let b = addr(11, 7777);
        let mut t = tracker();

        assert_eq!(t.observe(a, t0, 100), Decision::StartDetection);
        assert_eq!(t.observe(b, at(t0, 1), 100), Decision::PassThrough);
        assert_eq!(t.observe(a, at(t0, 2), 100), Decision::PassThrough);
        assert_eq!(t.observe(b, at(t0, 3), 100), Decision::PassThrough);
        // B reaches the threshold first and wins.
        assert_eq!(t.observe(b, at(t0, 4), 100), Decision::Tunnel(b));
        assert_eq!(t.locked_server(), Some(b));

        // A (only 2 packets) is still fresh but is not the locked server.
        assert_eq!(t.observe(a, at(t0, 5), 100), Decision::PassThrough);
        assert_eq!(t.locked_server(), Some(b));
    }

    #[test]
    fn effective_seeds_does_not_collapse_to_first_route() {
        let routes = vec![addr(20, 7777), addr(21, 7777), addr(22, 7777)];
        let seeds = effective_seeds(false, &routes);
        assert_eq!(seeds, routes, "static games keep every discovered route");
    }

    #[test]
    fn effective_seeds_is_empty_for_dynamic_games() {
        let routes = vec![addr(30, 7777), addr(31, 7777)];
        assert!(effective_seeds(true, &routes).is_empty());
        // No routes at all is also empty and harmless.
        assert!(effective_seeds(false, &[]).is_empty());
    }

    #[test]
    fn seeding_multiple_routes_locks_the_first_but_keeps_the_set() {
        let t0 = Instant::now();
        let routes = vec![addr(40, 7777), addr(41, 7777)];
        let mut t = tracker();
        t.seed(&routes, t0);
        assert_eq!(t.locked_server(), Some(routes[0]));
        assert_eq!(t.seeded_routes(), routes.as_slice());
    }

    // ── (f) zero-length / empty payload handling ───────────────────────────
    #[test]
    fn empty_payload_is_passthrough_and_never_counts() {
        let t0 = Instant::now();
        let a = addr(50, 7777);
        let mut t = tracker();

        // Empty datagrams must never advance detection toward a lock.
        for i in 0..5 {
            assert_eq!(t.observe(a, at(t0, i * 10), 0), Decision::PassThrough);
        }
        assert!(!t.is_locked());
        assert_eq!(t.candidate_count(), 0);
    }

    #[test]
    fn empty_payload_to_locked_server_does_not_refresh_staleness() {
        let t0 = Instant::now();
        let a = addr(51, 7777);
        let b = addr(52, 7777);
        let mut t = tracker();
        for i in 0..3 {
            t.observe(a, at(t0, i * 10), 100);
        }
        assert_eq!(t.locked_server(), Some(a));

        // Empty payload at t0+6000 does not count as A activity...
        assert_eq!(t.observe(a, at(t0, 6000), 0), Decision::PassThrough);
        // ...so a real packet to B still sees A as stale.
        assert_eq!(t.observe(b, at(t0, 6010), 100), Decision::ResetToDetection);
    }

    // ── (g) FLOW-AWARE DETECTION: the live mis-lock bug ────────────────────
    //
    // Field evidence 2026-11-20: after a re-arm, the pinned server (a live
    // match) went quiet for ~16 s, an unrelated high-port UDP flow appeared and
    // the client re-locked to it; the relay then starved (`NoReplyFromRelay`).
    // The tests below pin the pin: a foreign flow can never steal a lock, an
    // unproven one never becomes one, and the pinned address survives.

    #[test]
    fn a_new_foreign_flow_cannot_steal_a_stale_lock() {
        let server = addr(101, 36704);
        let (mut t, t0) = locked_on(server, 4242);

        // The match goes quiet: 6 s with no packet to the pinned server.
        let flow = addr(200, 35980);
        let mut detection = t.observe_attributed(obs(flow, FlowOwner::Foreign), at(t0, 6000));
        assert_eq!(
            detection.decision,
            Decision::PassThrough,
            "a foreign flow must not trigger re-detection"
        );
        assert_eq!(detection.rejected, Some(RejectReason::ForeignProcess));
        assert_eq!(
            t.locked_server(),
            Some(server),
            "the stale lock must be kept, not stolen"
        );

        // A flood of the same unrelated flow changes nothing.
        for i in 0..20 {
            detection = t.observe_attributed(obs(flow, FlowOwner::Foreign), at(t0, 6010 + i));
            assert_eq!(detection.decision, Decision::PassThrough);
            assert_eq!(detection.rejected, Some(RejectReason::ForeignProcess));
        }
        assert_eq!(
            t.locked_server(),
            Some(server),
            "the pinned server survives any amount of unrelated traffic"
        );
    }

    #[test]
    fn a_stale_lock_is_replaced_only_by_a_game_process_flow() {
        let server = addr(102, 36704);
        let (mut t, t0) = locked_on(server, 4242);
        let next_server = addr(201, 37000);

        // Unattributed high-port flow: refused, pin kept.
        let unproven =
            t.observe_attributed(obs(addr(202, 35980), FlowOwner::Unknown), at(t0, 6000));
        assert_eq!(unproven.decision, Decision::PassThrough);
        assert_eq!(unproven.rejected, Some(RejectReason::UnknownOwner));
        assert_eq!(t.locked_server(), Some(server));

        // The game process's own new server: accepted after the debounce.
        let first = t.observe_attributed(obs(next_server, FlowOwner::Game), at(t0, 6010));
        assert_eq!(first.decision, Decision::ResetToDetection);
        assert_eq!(first.rejected, None);

        let second = t.observe_attributed(obs(next_server, FlowOwner::Game), at(t0, 6020));
        assert_eq!(second.decision, Decision::PassThrough);

        let third = t.observe_attributed(obs(next_server, FlowOwner::Game), at(t0, 6030));
        assert_eq!(third.decision, Decision::Tunnel(next_server));
        assert_eq!(
            third.accepted,
            Some(AcceptReason::GameProcess),
            "acceptance says why: the game process owns this flow"
        );
        assert_eq!(t.locked_server(), Some(next_server));
    }

    #[test]
    fn a_game_flow_is_accepted_after_a_process_restart() {
        let server = addr(103, 36704);
        let (mut t, t0) = locked_on(server, 1111);
        let restarted_server = addr(203, 37001);

        // The game relaunched under a new PID: old evidence is void.
        assert!(
            t.set_game_pid(Some(2222)),
            "a new PID is a restart and must be reported"
        );
        assert!(
            !t.is_locked(),
            "a lock earned by the old process cannot survive its restart"
        );

        for i in 0..3 {
            t.observe_attributed(
                obs(restarted_server, FlowOwner::Game),
                at(t0, 10_000 + i * 10),
            );
        }
        assert_eq!(t.locked_server(), Some(restarted_server));
    }

    #[test]
    fn a_foreign_flow_never_becomes_a_candidate() {
        let t0 = Instant::now();
        let mut t = tracker();
        assert!(!t.set_game_pid(Some(4242)));
        let unrelated = addr(204, 35980);

        for i in 0..10 {
            let detection =
                t.observe_attributed(obs(unrelated, FlowOwner::Foreign), at(t0, i * 10));
            assert_eq!(detection.decision, Decision::PassThrough);
            assert_eq!(detection.rejected, Some(RejectReason::ForeignProcess));
        }
        assert!(
            !t.is_locked(),
            "a foreign high-port flow must never lock the session"
        );
        assert_eq!(t.candidate_count(), 0);
    }

    #[test]
    fn an_unattributed_flow_never_becomes_a_candidate_when_a_process_is_known() {
        let t0 = Instant::now();
        let mut t = tracker();
        assert!(!t.set_game_pid(Some(4242)));

        for i in 0..10 {
            let detection =
                t.observe_attributed(obs(addr(205, 35980), FlowOwner::Unknown), at(t0, i * 10));
            assert_eq!(detection.rejected, Some(RejectReason::UnknownOwner));
        }
        assert!(!t.is_locked());
    }

    #[test]
    fn a_private_or_loopback_destination_never_locks() {
        let t0 = Instant::now();
        let mut t = tracker();
        assert!(!t.set_game_pid(Some(4242)));

        for ip in [
            Ipv4Addr::LOCALHOST,
            Ipv4Addr::new(10, 1, 2, 3),
            Ipv4Addr::new(172, 16, 5, 5),
            Ipv4Addr::new(192, 168, 1, 10),
            Ipv4Addr::new(169, 254, 1, 1),
            Ipv4Addr::new(100, 64, 0, 1),
            Ipv4Addr::new(224, 0, 0, 1),
            Ipv4Addr::UNSPECIFIED,
        ] {
            let dst = SocketAddrV4::new(ip, 35980);
            for i in 0..5 {
                let detection = t.observe_attributed(obs(dst, FlowOwner::Game), at(t0, i * 10));
                assert_eq!(
                    detection.decision,
                    Decision::PassThrough,
                    "{ip} must never be relayed to"
                );
                assert_eq!(
                    detection.rejected,
                    Some(RejectReason::NonPublicDestination),
                    "{ip} must be refused as a candidate"
                );
            }
            assert!(!t.is_locked(), "{ip} must never become the lock");
        }
        assert_eq!(t.candidate_count(), 0);
    }

    #[test]
    fn a_private_seed_is_dropped_on_the_first_matching_packet() {
        let t0 = Instant::now();
        let private = SocketAddrV4::new(Ipv4Addr::new(192, 168, 1, 50), 37000);
        let mut t = tracker();
        t.seed(&[private], t0);
        assert!(t.is_locked(), "the seed itself is a lock");

        let detection = t.observe_attributed(obs(private, FlowOwner::Game), at(t0, 10));
        assert_eq!(detection.decision, Decision::PassThrough);
        assert_eq!(detection.rejected, Some(RejectReason::NonPublicDestination));
        assert!(!t.is_locked(), "a private seed must not stay locked");
    }

    #[test]
    fn ambiguity_keeps_the_pin() {
        let server = addr(104, 36704);
        let (mut t, t0) = locked_on(server, 4242);

        // Two unattributed candidates appear after the silence: neither is
        // attributable, so neither may take the lock.
        for i in 0..10u64 {
            let dst = addr(if i % 2 == 0 { 206 } else { 207 }, 35980);
            let detection = t.observe_attributed(obs(dst, FlowOwner::Unknown), at(t0, 6000 + i));
            assert_eq!(detection.decision, Decision::PassThrough);
            assert_eq!(detection.rejected, Some(RejectReason::UnknownOwner));
        }
        assert_eq!(
            t.locked_server(),
            Some(server),
            "an ambiguous field of candidates must not move the pin"
        );
    }

    #[test]
    fn a_candidate_on_another_hosting_network_is_refused_when_unattributed() {
        let server = addr(105, 36704);
        let b = addr(208, 35980);
        let t0 = Instant::now();
        let mut t = tracker();
        assert!(!t.set_game_pid(Some(4242)));
        for i in 0..3 {
            t.observe_attributed(obs_on(server, FlowOwner::Game, 199_524), at(t0, i * 10));
        }
        assert_eq!(t.locked_server(), Some(server));
        assert_eq!(t.locked_provider(), Some(199_524));

        // Unattributed, different hosting network: refused, pin kept.
        let detection = t.observe_attributed(obs_on(b, FlowOwner::Unknown, 16_509), at(t0, 6000));
        assert_eq!(detection.decision, Decision::PassThrough);
        assert_eq!(detection.rejected, Some(RejectReason::ProviderMismatch));
        assert_eq!(t.locked_server(), Some(server));

        // Unattributed, same hosting network: accepted as a replacement.
        let same_network = addr(209, 37000);
        let first = t.observe_attributed(
            obs_on(same_network, FlowOwner::Unknown, 199_524),
            at(t0, 6010),
        );
        assert_eq!(first.decision, Decision::ResetToDetection);
        let second = t.observe_attributed(
            obs_on(same_network, FlowOwner::Unknown, 199_524),
            at(t0, 6020),
        );
        assert_eq!(second.decision, Decision::PassThrough);
        let third = t.observe_attributed(
            obs_on(same_network, FlowOwner::Unknown, 199_524),
            at(t0, 6030),
        );
        assert_eq!(third.decision, Decision::Tunnel(same_network));
        assert_eq!(third.accepted, Some(AcceptReason::ProviderMatch));
    }

    #[test]
    fn the_pinned_network_is_preferred_between_competing_candidates() {
        // Pin on network 10. After the silence two game flows appear: B on
        // another known network, C on the pinned one. B reaches 3 packets first;
        // the detector must prefer C, which is the network the match was on.
        let server = addr(106, 36704);
        let other_network = addr(210, 37010);
        let pinned_network = addr(211, 37020);
        let t0 = Instant::now();
        let mut t = tracker();
        assert!(!t.set_game_pid(Some(4242)));
        for i in 0..3 {
            t.observe_attributed(obs_on(server, FlowOwner::Game, 10), at(t0, i * 10));
        }
        assert_eq!(t.locked_server(), Some(server));

        // Interleave so both are live when B hits the threshold.
        t.observe_attributed(obs_on(other_network, FlowOwner::Game, 20), at(t0, 6000));
        t.observe_attributed(obs_on(pinned_network, FlowOwner::Game, 10), at(t0, 6010));
        t.observe_attributed(obs_on(other_network, FlowOwner::Game, 20), at(t0, 6020));
        t.observe_attributed(obs_on(pinned_network, FlowOwner::Game, 10), at(t0, 6030));
        let held = t.observe_attributed(obs_on(other_network, FlowOwner::Game, 20), at(t0, 6040));
        assert_eq!(
            held.decision,
            Decision::PassThrough,
            "the cross-network candidate must wait while a same-network one is live"
        );
        assert_eq!(
            t.locked_server(),
            None,
            "the dead lock was released when an attributable candidate appeared; \
             no replacement is committed while the tie is unresolved"
        );

        let committed =
            t.observe_attributed(obs_on(pinned_network, FlowOwner::Game, 10), at(t0, 6050));
        assert_eq!(committed.decision, Decision::Tunnel(pinned_network));
        assert_eq!(
            committed.accepted,
            Some(AcceptReason::GameProcessSameNetwork),
            "the candidate on the pinned network wins the tie"
        );
        assert_eq!(t.locked_server(), Some(pinned_network));
    }

    #[test]
    fn a_provider_mismatch_still_blocks_a_stale_relock_without_a_process_target() {
        // Manual/Custom detection has no process target, so the historical
        // acceptance rule stands - except that a different known hosting
        // network is refused, which is the remaining guard when the socket
        // table cannot attribute anything.
        let server = addr(107, 36704);
        let t0 = Instant::now();
        let mut t = tracker();
        for i in 0..3 {
            t.observe_attributed(obs_on(server, FlowOwner::Unknown, 199_524), at(t0, i * 10));
        }
        assert_eq!(t.locked_server(), Some(server));
        assert_eq!(t.locked_provider(), Some(199_524));

        let unrelated = addr(212, 35980);
        let detection =
            t.observe_attributed(obs_on(unrelated, FlowOwner::Unknown, 16_509), at(t0, 6000));
        assert_eq!(detection.decision, Decision::PassThrough);
        assert_eq!(detection.rejected, Some(RejectReason::ProviderMismatch));
        assert_eq!(t.locked_server(), Some(server));
    }

    #[test]
    fn withdrawing_the_process_target_keeps_the_lock_and_the_legacy_rule() {
        // The caller relaxes attribution when it never saw a single packet
        // attributed to the resolved PID. Losing attribution must not drop the
        // pin, and detection must fall back to the historical rule rather than
        // refusing everything forever.
        let server = addr(108, 36704);
        let (mut t, t0) = locked_on(server, 4242);

        assert!(!t.set_game_pid(None), "withdrawal is not a restart");
        assert_eq!(t.game_pid(), None);
        assert_eq!(
            t.locked_server(),
            Some(server),
            "losing attribution does not prove the match moved"
        );

        let next = addr(213, 37000);
        let detection = t.observe_attributed(obs(next, FlowOwner::Unknown), at(t0, 6000));
        assert_eq!(
            detection.decision,
            Decision::ResetToDetection,
            "with no attribution expected, detection keeps its legacy rule"
        );
    }

    #[test]
    fn seed_provider_is_recorded_for_a_seeded_lock() {
        let t0 = Instant::now();
        let server = addr(109, 36704);
        let mut t = tracker();
        t.seed(&[server], t0);
        assert_eq!(t.locked_provider(), None);
        t.note_seed_provider(Some(199_524));
        assert_eq!(t.locked_provider(), Some(199_524));

        // Only a locked tracker records it.
        let mut unlocked = tracker();
        unlocked.note_seed_provider(Some(1));
        assert_eq!(unlocked.locked_provider(), None);
    }

    #[test]
    fn a_non_game_pid_change_is_not_a_restart() {
        let server = addr(110, 36704);
        let (mut t, _t0) = locked_on(server, 4242);
        assert!(!t.set_game_pid(Some(4242)), "the same PID is not a restart");
        assert_eq!(t.locked_server(), Some(server));
    }

    #[test]
    fn a_shared_port_is_unproven_not_ours() {
        // The Windows UDP table carries no remote endpoint and two processes can
        // bind one port, so an ambiguous owner must not be accepted as ours.
        // `windows::flow_owner` implements the classification; here we pin the
        // detector side: Unknown stays unproven even for a known PID.
        let t0 = Instant::now();
        let mut t = tracker();
        assert!(!t.set_game_pid(Some(4242)));
        for i in 0..6 {
            let detection =
                t.observe_attributed(obs(addr(214, 35980), FlowOwner::Unknown), at(t0, i * 10));
            assert_eq!(detection.rejected, Some(RejectReason::UnknownOwner));
        }
        assert!(!t.is_locked());
    }
}
