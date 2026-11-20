//! Status DTO v2 — the one snapshot the presentation layer consumes.
//!
//! WF-081 W2-1 stage S3a (`docs/design/gui-core-boundary.md` §5.3, spec §2).
//! [`AppStatus`] is assembled once per frame from the engine snapshot, the
//! app's derived route facts and the watchdog's published warning. The GUI's
//! presentation adapter (`client-gui/src/app/route_surface.rs`) maps it to the
//! word/rows model and **derives no state of its own**; the shell renders that.
//!
//! Boundary rules this module encodes:
//! - one field path per contract row (`docs/design/gui-refresh-spec.md` §2); a
//!   stale field path is amended, never shimmed;
//! - a fact the client cannot observe gets `None`, never a fabricated zero (see
//!   [`WatchdogStatus::streak`]);
//! - the warn-only model has no direct/retry/rejection state: the only session
//!   warning is [`StarvationWarning`], and it never moves the route.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use crate::{EngineStatus, ScoredRelay};

// ── Engine / route facts ────────────────────────────────────────────────────

/// Whether a connect attempt is in flight.
///
/// The engine's own facts drive this: a relay address has been chosen, the
/// keepalive is not up, registration has not completed and has not failed. Any
/// of those ends the attempt (`docs/design/gui-refresh-spec.md` §2.1/§2.2 row
/// 2: `control_registered`/`registration_error` end the attempt). A user
/// disconnect clears the chosen address on the app side, so a disconnected idle
/// window does not report `CONNECTING`.
pub fn connecting_fact(
    connected: bool,
    control_registered: bool,
    registration_error: Option<&str>,
    relay_chosen: bool,
) -> bool {
    relay_chosen && !connected && !control_registered && registration_error.is_none()
}

/// Whether the destination-aware chooser is working on the locked server.
///
/// Choosing is a sub-state of HOLDING (amendment A4), never its own word: the
/// engine withholds tunnelling until a relay is chosen for the server the run
/// just locked, and this says that specific work is still in progress.
pub fn choosing_fact(holding: bool, server_locked: bool, relay_chosen_for_server: bool) -> bool {
    holding && server_locked && !relay_chosen_for_server
}

/// What left this host and what the relay returned, for the session on screen.
///
/// One source pair (amendment A5): whichever interception mode is carrying the
/// traffic owns both counters. Redirect wins when both are active because it is
/// the older, narrower mode. The pair is derived once here so the word and the
/// `Relayed` row can never contradict each other.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RelayedPair {
    /// Datagrams this session handed to the relay.
    pub sent: u64,
    /// Datagrams the relay sent back for this session.
    pub returned: u64,
}

impl RelayedPair {
    /// The one counter pair for the session, or `None` when no tunnel mode is
    /// carrying traffic.
    pub fn for_session(
        redirect_active: bool,
        redirect_sent: u64,
        redirect_returned: u64,
        interceptor_active: bool,
        interceptor_sent: u64,
        interceptor_returned: u64,
    ) -> Option<Self> {
        if redirect_active {
            Some(Self {
                sent: redirect_sent,
                returned: redirect_returned,
            })
        } else if interceptor_active {
            Some(Self {
                sent: interceptor_sent,
                returned: interceptor_returned,
            })
        } else {
            None
        }
    }

    /// The interceptor's per-server flow pair when it has one for `server`
    /// (amendment A5: the per-server flow is the first source, the window
    /// counters the fallback).
    pub fn for_flow(server: Ipv4Addr, flows: &[(Ipv4Addr, u64, u64)]) -> Option<Self> {
        flows
            .iter()
            .find(|(ip, _, _)| *ip == server)
            .map(|(_, sent, returned)| Self {
                sent: *sent,
                returned: *returned,
            })
    }

    /// Whether the session is known to be starving: it forwarded traffic and
    /// the relay returned none of it. This is the `Relayed` row's warning flag,
    /// not the watchdog's verdict.
    pub fn starving(self) -> bool {
        self.sent > 0 && self.returned == 0
    }
}

/// The relay in use and the facts the route card can honestly state about it.
#[derive(Clone, Debug, Default)]
pub struct RelayStatus {
    /// The relay the surface is describing (the one in use/selected), if any.
    pub relay: Option<SocketAddrV4>,
    /// The ranking that chose it, best first. Empty until a choice was made.
    pub ranked: Vec<ScoredRelay>,
    /// The game server the current route is for.
    pub server: Option<Ipv4Addr>,
    /// The direct (ICMP) ping for that server, with its age.
    pub server_ping: Option<ServerPing>,
    /// How the relay-to-server leg was obtained for the relay in use.
    pub server_leg: ServerLegState,
    /// A connect attempt is in flight ([`connecting_fact`]).
    pub connecting: bool,
    /// The destination-aware chooser is working ([`choosing_fact`]).
    pub choosing: bool,
    /// The triangle-inequality verdict for the relay in use, when it holds:
    /// `(client_leg_us, direct_us)` with `client_leg_us >= direct_us`.
    pub cannot_beat_direct: Option<(u64, u64)>,
    /// The (relay, server) carry memory for this route, when both are known.
    pub carry: Option<CarryVerdict>,
    /// The no-position fallback ranking used for this route, when it was used.
    pub fallback: Option<FallbackRanking>,
}

/// The watchdog's published state.
///
/// S3a carries the warning (the only session warning the warn-only model has)
/// and the streak evidence **when the client publishes it**. The GUI's watchdog
/// thread keeps the streak counter internally until the S4 move publishes it
/// with the rest of the watchdog, so [`Self::streak`] is `None` until then: it
/// is the honest "not observed by this client yet", never a fabricated zero.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WatchdogStatus {
    /// The active starvation warning, if the return path is judged faulty.
    pub warning: Option<StarvationWarning>,
    /// Consecutive judged windows that carried the same fault, once published.
    pub streak: Option<u32>,
}

/// A confirmed starvation finding: the client observed that this relay is not
/// returning this game server's traffic.
///
/// It states the client's observation only; it never claims the relay's intent
/// and never changes the route (warn-only, owner decision 2026-11-20).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StarvationWarning {
    /// The game server whose traffic is not coming back.
    pub server: SocketAddrV4,
    /// The relay that was carrying it, when the relay was known at that moment.
    pub relay: Option<SocketAddrV4>,
    /// The confirmed fault behind the warning.
    pub fault: ReturnPathFault,
}

/// Why the return path was judged faulty. One variant per distinct remedy.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ReturnPathFault {
    /// The relay produced no datagram for this server at all.
    NoReplyFromRelay,
    /// The relay returned datagrams and the client injected none of them: the
    /// local injection path is the fault, not the relay.
    InjectionFailed,
    /// The relay returned something, but too little of the traffic it carried.
    TooFewReplies,
}

/// What the relay history remembers about one `(relay, server)` pair, for the
/// starvation filter that runs before a ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarryVerdict {
    /// The record proves the relay carried this exact server recently.
    Proven,
    /// The watchdog's finding (or the starvation signature) says it did not.
    Starved,
    /// No evidence either way.
    Unproven,
}

/// The carry memory the starvation filter reads.
///
/// The full `RelayHistory` (and its persistence) moves at S4; until then the
/// GUI implements this seam for its history type, so the *rule* lives here and
/// the storage stays where it is. One trait, so no second copy of the filter.
pub trait CarryLookup {
    /// What the record says about `relay` carrying `server`.
    fn carry_verdict(&self, relay: SocketAddrV4, server: Ipv4Addr) -> CarryVerdict;
}

impl<F> CarryLookup for F
where
    F: Fn(SocketAddrV4, Ipv4Addr) -> CarryVerdict,
{
    fn carry_verdict(&self, relay: SocketAddrV4, server: Ipv4Addr) -> CarryVerdict {
        self(relay, server)
    }
}

// ── Server leg (measured + estimator) ──────────────────────────────────────

/// A published quiet-flow server-leg measurement: the value, the samples behind
/// it, and the age of the newest one when the estimator reports an age.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MeasuredServerLeg {
    /// Relay-to-server round trip, microseconds.
    pub dest_leg_us: u64,
    /// Samples behind the published estimate.
    pub samples: usize,
    /// Age of the newest contributing sample; `None` when the estimator
    /// publishes an estimate without an age, which the surface must not invent.
    pub age: Option<Duration>,
}

/// The measured direct (ICMP) ping, and the game server it was measured for.
///
/// Every consumer must ask for the ping **of the server it is describing** and
/// treat a foreign record as "not measured".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ServerPing {
    /// The game server this sample was measured against.
    pub server: Ipv4Addr,
    /// Median ICMP round trip, milliseconds.
    pub ms: f32,
    /// Age of the newest burst sample, seconds.
    pub age_secs: u64,
}

/// What the route card can honestly say about the relay-to-server leg for the
/// relay in use. Exactly one value drives the estimator `Server leg` row.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ServerLegState {
    /// The v1 quiet-flow estimator published an estimate for this exact pair.
    Measured(MeasuredServerLeg),
    /// The prior was below the triangle bound: the leg is at least this.
    Bounded {
        /// Proven lower bound, microseconds.
        floor_us: u64,
    },
    /// The ranking used the geometric region prior.
    Prior {
        /// Prior value, microseconds.
        prior_us: u64,
    },
    /// No measurement and no usable region prior.
    #[default]
    Unknown,
}

/// The relay's measured display facts for the pair on screen.
///
/// Every field is already pair-bound and validated by `lightspeed-client`; the
/// surface states them and claims nothing further.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ServerLegStatus {
    /// The relay's own ICMP measurement of the relay→server leg.
    pub icmp: Option<crate::ServerLegReport>,
    /// The relay's passive app-RTT sample (display-only peer reference).
    pub app: Option<crate::ServerLegAppReport>,
    /// The relay's TTL traceroute (placement anchor, not a ping).
    pub trace: Option<crate::ServerTraceReport>,
    /// `L + S` as a lower bound for the relayed path, milliseconds, when both
    /// terms are measured ([`expected_path_ms`]).
    pub expected_path_ms: Option<f64>,
}

/// The one whole-path figure the two measured legs support: `L + S`.
///
/// `L` (client→relay keepalive, ms) and `S` (relay-measured server leg, µs) are
/// both measured, so their sum is a **floor** on the relayed path, not the path.
/// `None` while no fresh client leg exists, because a bare `S` cannot become a
/// path total without it.
pub fn expected_path_ms(client_leg_ms: f64, server_leg_us: u32) -> Option<f64> {
    if !client_leg_ms.is_finite() || client_leg_ms <= 0.0 {
        return None;
    }
    let total_us = (client_leg_ms * 1000.0).round() as u64 + u64::from(server_leg_us);
    Some(total_us as f64 / 1000.0)
}

/// One no-position fallback ranking.
#[derive(Clone, Debug)]
pub struct FallbackRanking {
    /// The engine's order for the surviving candidates, best first.
    pub ranked: Vec<ScoredRelay>,
    /// Candidates removed because their record says the relay starves this server.
    pub skipped_starved: usize,
    /// Candidates kept because their record proves they carry this server; zero
    /// when none was proven and the whole pool was ranked.
    pub proven: usize,
}

// ── Location ───────────────────────────────────────────────────────────────

/// Which evidence rung placed the server. Mutually exclusive; display-only
/// except [`Self::Cloud`], whose exact point may feed the chooser.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LocationRung {
    /// Provider-published cloud region (exact).
    Cloud,
    /// Operator geofeed entry, shown as-is (display-only, no dot).
    Geofeed,
    /// Hosting network; the free databases are not trusted for it.
    Hosting,
    /// Country-level estimate from IP.
    Country,
    /// No source could place it.
    #[default]
    Unknown,
}

/// The operator-geofeed provenance, when the ladder used that rung.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GeofeedProvenance<'a> {
    /// Short operator name.
    pub provider: &'a str,
    /// ISO 3166-1 alpha-2 country code.
    pub country: &'a str,
    /// Region/subdivision, when published.
    pub region: Option<&'a str>,
    /// City, when published.
    pub city: Option<&'a str>,
    /// Public feed URL.
    pub source: &'a str,
    /// Feed generation date (`YYYY-MM-DD`).
    pub as_of: &'a str,
}

/// The honest location ladder for the server on screen (amendment A8).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LocationStatus {
    /// The strongest evidence rung.
    pub rung: LocationRung,
    /// Provider name for [`LocationRung::Cloud`].
    pub provider: Option<&'static str>,
    /// Provider region for [`LocationRung::Cloud`].
    pub region: Option<&'static str>,
    /// ASN for [`LocationRung::Hosting`].
    pub asn: Option<u32>,
    /// Network name for [`LocationRung::Hosting`].
    pub hosting: Option<&'static str>,
    /// Country code for [`LocationRung::Country`].
    pub country: Option<&'static str>,
    /// Geofeed provenance for [`LocationRung::Geofeed`].
    pub geofeed: Option<GeofeedProvenance<'static>>,
    /// The map point the surface may draw (`None` = no dot).
    pub display_point: Option<(f32, f32)>,
    /// The cloud-exact point the chooser may rank on; deliberately separate
    /// from [`Self::display_point`] (A8: only cloud-exact may steer).
    pub chooser_point: Option<(f64, f64)>,
}

// ── Route trace ────────────────────────────────────────────────────────────

/// The client-vantage route trace row: hops and the last responding router.
///
/// It is a placement anchor, not the game's ping and not a city label.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClientRoute {
    /// Number of hops walked.
    pub hops: u8,
    /// The last hop that answered, if any.
    pub last_hop: Option<Ipv4Addr>,
    /// That hop's best RTT, milliseconds.
    pub last_hop_rtt_ms: Option<u32>,
    /// Whether a hop answered with the target address.
    pub complete: bool,
}

// ── Commands / errors ──────────────────────────────────────────────────────

/// A session command the window can issue.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SessionCommand {
    /// Connect to the selected relay.
    SelectRelay,
    /// Drop the control-plane link.
    Disconnect,
    /// Start the boost for the selected game/relay.
    StartBoost,
    /// Stop every tunnel backend.
    StopBoost,
    /// Change the effective path-MTU cap.
    SetPathMtu,
    /// The engine's start-of-match hold.
    Hold,
}

/// The in-flight/last-error state of the command slot.
///
/// This is the action loading state (`STARTING…`) and the start-refusal Error
/// source. Today `start_boost`/`stop_boost` are synchronous, so `in_flight` is
/// set and cleared inside the call; W2-3a's `SessionCommand` adoption makes it
/// span frames. The DTO is the same either way — the shell renders it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct SessionCommandState {
    /// The command currently being issued, if any.
    pub in_flight: Option<SessionCommand>,
    /// The last command that failed (the Error row source).
    pub last_error: Option<AppError>,
}

impl SessionCommandState {
    /// Mark `command` as in flight. The previous error is kept until the
    /// command fails again or succeeds (the error panel does not clear itself).
    pub fn begin(&mut self, command: SessionCommand) {
        self.in_flight = Some(command);
    }

    /// The command finished successfully: clear the in-flight marker.
    pub fn finish(&mut self, command: SessionCommand) {
        if self.in_flight == Some(command) {
            self.in_flight = None;
        }
    }

    /// The command failed: clear the in-flight marker and record the error.
    pub fn fail(&mut self, command: SessionCommand, error: AppError) {
        if self.in_flight == Some(command) {
            self.in_flight = None;
        }
        self.last_error = Some(error);
    }

    /// Clear the recorded error (e.g. a successful retry).
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }
}

/// The error taxonomy the Error row renders (spec §2.8). One kind, one sentence.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AppErrorKind {
    /// Capture/redirect backend failure.
    Capture,
    /// Tunnel/interception failure.
    Tunnel,
    /// Routing or QUIC registration failure.
    Route,
    /// The config file could not be read (or was malformed); defaults are running.
    Config,
    /// Game detection/profile failure.
    Game,
    /// Validation refusal.
    Validation,
    /// Local I/O failure.
    Io,
    /// The platform cannot do what was asked.
    Unsupported,
}

/// One user-facing failure: a taxonomy kind plus the verbatim detail behind
/// `Details`/the log. No `String` alone crosses the boundary as copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppError {
    /// Which sentence the surface leads with.
    pub kind: AppErrorKind,
    /// The verbatim engine/diagnostic message, shown behind the disclosure.
    pub detail: Option<String>,
}

impl AppError {
    /// Map the engine's error *kind name* onto the taxonomy.
    ///
    /// The engine reports typed errors internally but exposes strings at the
    /// boundary (`redirect_error`, `interceptor_error`, …); the mapping is by
    /// the surface that produced it, not by parsing the string.
    pub fn for_kind(kind: AppErrorKind, detail: impl Into<String>) -> Self {
        Self {
            kind,
            detail: Some(detail.into()),
        }
    }

    /// The registry/QUIC registration failure (the Relay row retries it).
    pub fn registration(detail: impl Into<String>) -> Self {
        Self::for_kind(AppErrorKind::Route, detail)
    }

    /// A tunnel/interception failure (capture backends, interceptor start).
    pub fn tunnel(detail: impl Into<String>) -> Self {
        Self::for_kind(AppErrorKind::Tunnel, detail)
    }

    /// The tolerant config loader's diagnostic: defaults are running.
    pub fn config(recovery: &super::config::ConfigRecovery) -> Self {
        Self::for_kind(AppErrorKind::Config, recovery.reason.clone())
    }
}

/// The facts the front end supplies (not the engine): platform capabilities.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    /// Whether the process can relay game traffic (administrator rights).
    pub elevated: bool,
}

/// The tray projection of the current state. `RelayStarved` exists and must
/// never be reported as `Optimizing` (spec §2.2 row 6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayState {
    /// No relay link and no session.
    Disconnected,
    /// The relay link is up but nothing is being relayed.
    Connected,
    /// A tunnel carries traffic and no warning is active.
    Optimizing,
    /// A tunnel carries traffic and the return path is judged faulty.
    RelayStarved,
    /// An error is current.
    Error,
}

// ── The snapshot ───────────────────────────────────────────────────────────

/// The interceptor's published counters, projected so the DTO does not leak the
/// engine's internal health type (which is not cloneable/presentable).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InterceptorFacts {
    /// The server the interceptor has locked, when it has one.
    pub detected_server: Option<SocketAddrV4>,
    /// Datagrams intercepted from the game.
    pub packets_intercepted: u64,
    /// Datagrams injected back to the game.
    pub packets_injected: u64,
    /// Game-data replies received from a known relay and decoded (non-keepalive).
    pub packets_reply_data: u64,
    /// The interceptor's cumulative error count.
    pub errors: u64,
    /// The interceptor's last recorded error, verbatim.
    pub last_error: Option<String>,
}

/// One status snapshot: everything the presentation layer may render.
///
/// Assembled once per frame by the app facade; the GUI adapter derives no state
/// from anything else. Every field is a fact, and a fact the client cannot
/// observe is `None` (never a zero).
#[derive(Clone, Debug)]
pub struct AppStatus {
    /// The engine's published status snapshot.
    pub engine: EngineStatus,
    /// The interceptor's health counters, when a backend is running.
    pub interceptor: Option<InterceptorFacts>,
    /// The relay and the route facts for it.
    pub relay: RelayStatus,
    /// The watchdog's published warning/streak.
    pub watchdog: WatchdogStatus,
    /// The location ladder for the server on screen.
    pub location: LocationStatus,
    /// The relay-measured leg rows for the pair on screen.
    pub server_leg: ServerLegStatus,
    /// The client-vantage route trace, when one exists.
    pub route_trace: Option<ClientRoute>,
    /// `run_boosting()`: some tunnel backend carries the game's traffic.
    pub boosting: bool,
    /// The one session counter pair, when a backend is carrying traffic.
    pub relayed: Option<RelayedPair>,
    /// The current failure, if any (session error, config recovery, engine).
    pub error: Option<AppError>,
    /// The command slot's in-flight/last-error state.
    pub session: SessionCommandState,
    /// The platform capability facts.
    pub capabilities: Capabilities,
    /// The tray projection of this same snapshot.
    pub tray: TrayState,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::config::ConfigRecovery;
    use std::path::PathBuf;

    #[test]
    fn a_session_command_is_in_flight_until_it_finishes_or_fails() {
        let mut session = SessionCommandState::default();
        session.begin(SessionCommand::StartBoost);
        assert_eq!(session.in_flight, Some(SessionCommand::StartBoost));
        session.finish(SessionCommand::StartBoost);
        assert_eq!(session.in_flight, None);
        assert_eq!(session.last_error, None);

        session.begin(SessionCommand::StartBoost);
        let error = AppError::tunnel("WSAEMSGSIZE");
        session.fail(SessionCommand::StartBoost, error.clone());
        assert_eq!(session.in_flight, None);
        assert_eq!(session.last_error, Some(error));

        // A later success does not hide the recorded error: the Error row keeps
        // its Details until the user retries (there is no silent clearing).
        session.begin(SessionCommand::StartBoost);
        session.finish(SessionCommand::StartBoost);
        assert!(session.last_error.is_some());
        session.clear_error();
        assert_eq!(session.last_error, None);
    }

    #[test]
    fn a_finish_for_another_command_does_not_clear_this_one() {
        let mut session = SessionCommandState::default();
        session.begin(SessionCommand::StartBoost);
        session.finish(SessionCommand::StopBoost);
        assert_eq!(
            session.in_flight,
            Some(SessionCommand::StartBoost),
            "only the command that began may finish"
        );
    }

    #[test]
    fn the_config_recovery_becomes_the_config_error_sentence() {
        let recovery = ConfigRecovery {
            path: PathBuf::from("C:/Users/x/AppData/Roaming/light-speed/config.toml"),
            reason: "expected an equals, found a newline".to_string(),
        };
        let error = AppError::config(&recovery);
        assert_eq!(error.kind, AppErrorKind::Config);
        assert_eq!(
            error.detail.as_deref(),
            Some("expected an equals, found a newline"),
            "the verbatim diagnostic stays behind Details"
        );
    }

    #[test]
    fn the_relayed_pair_reads_the_per_server_flow_first() {
        let server = Ipv4Addr::new(203, 0, 113, 7);
        let flows = [(server, 54u64, 0u64)];
        assert_eq!(
            RelayedPair::for_flow(server, &flows),
            Some(RelayedPair {
                sent: 54,
                returned: 0
            })
        );
        assert_eq!(
            RelayedPair::for_flow(Ipv4Addr::new(203, 0, 113, 9), &flows),
            None,
            "another server's flow is not this server's pair"
        );
    }
}
