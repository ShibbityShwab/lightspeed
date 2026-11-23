//! Status math and route rules — moved out of the GUI at W2-1 S3a.
//!
//! Everything here is a pure decision over facts: the tray projection, the
//! action slot, the session counters, the relay-measured leg readers and the
//! starvation-aware ranking filters. The presentation layer may format these
//! results but never re-derives them.
//!
//! What stayed behind on purpose: copy builders (`i18n` lookups) and anything
//! that depends on `RelayHistory`'s storage. The history moves at S4; until
//! then the GUI implements [`CarryLookup`] for its type, so the starvation
//! filter itself is shared while the storage stays put.

use std::net::{Ipv4Addr, SocketAddrV4};

use crate::app::lifecycle::{RegistrationOutcome, SessionCloseReason};
use crate::{RelayCandidate, ScoredRelay};

use super::config::ProxyEntry;
use super::discovery::RelaySample;
use super::dto::{
    CarryLookup, CarryVerdict, FallbackRanking, MeasuredServerLeg, RelayedPair, ServerLegState,
    ServerPing, TrayState,
};

/// How old a direct-ping sample may be before the surface stops showing it and
/// the ranking stops using it as a triangle bound. One rule for both: a stale
/// ping displayed as current and a stale ping shaping the choice are the same
/// defect.
pub const SERVER_PING_FRESH_SECS: u64 = 60;

/// How many distinct relays may be on record as starving one server before the
/// chooser stops choosing automatically.
pub const MAX_RELAYS_TRIED: usize = 3;

// ── Tray ───────────────────────────────────────────────────────────────────

/// The tray state for the current frame.
///
/// The order is the honesty rule the tray carries: an error is an error, a
/// session whose relay is not returning the game's traffic is
/// [`TrayState::RelayStarved`] and never [`TrayState::Optimizing`], and only a
/// session with no active warning and traffic flowing may claim optimization.
/// `warning_active` is expected to be false when not boosting; it is still
/// checked against `boosting` so a stale warning can never paint a green icon
/// over a stopped tunnel.
pub fn tray_state_for(
    has_error: bool,
    warning_active: bool,
    boosting: bool,
    connected: bool,
) -> TrayState {
    if has_error {
        TrayState::Error
    } else if warning_active && boosting {
        TrayState::RelayStarved
    } else if boosting {
        TrayState::Optimizing
    } else if connected {
        TrayState::Connected
    } else {
        TrayState::Disconnected
    }
}

// ── Action slot ────────────────────────────────────────────────────────────

/// The decision behind the one action slot. Copy lives in the front end; the
/// rule (which label, enabled or not) lives here.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionSlot {
    /// A tunnel is running: the slot stops it.
    StopOptimizing,
    /// The process cannot relay game traffic; the slot asks for elevation.
    RestartElevated,
    /// The boost can be started now.
    StartReady,
    /// The boost cannot start yet (a precondition is unmet).
    StartBlocked,
}

/// The one action slot, as a pure function of the gates.
///
/// - boosting → [`ActionSlot::StopOptimizing`].
/// - unelevated **and able to boost otherwise** (connected, relay selected) →
///   [`ActionSlot::RestartElevated`]. The elevation invite is an answer to
///   "this would run if it had rights", so it must not appear on a window that
///   could not start the boost anyway (spec M1, §2.2 row 7: "the elevation
///   invite appears only when a relay is selected and the boost can otherwise
///   start"). Unelevated-but-not-ready is [`ActionSlot::StartBlocked`].
/// - elevated, connected, with a relay → [`ActionSlot::StartReady`].
/// - otherwise → [`ActionSlot::StartBlocked`].
pub fn action_slot(is_admin: bool, boosting: bool, connected: bool, has_relay: bool) -> ActionSlot {
    if boosting {
        ActionSlot::StopOptimizing
    } else if !is_admin {
        if connected && has_relay {
            ActionSlot::RestartElevated
        } else {
            ActionSlot::StartBlocked
        }
    } else if connected && has_relay {
        ActionSlot::StartReady
    } else {
        ActionSlot::StartBlocked
    }
}

// ── Relay-switch hysteresis ────────────────────────────────────────────────

/// Whether a re-race winner is enough faster to justify switching relays.
///
/// Returns `true` only when `winner_rtt_ms + margin_ms` is strictly less than
/// `current_rtt_ms`; a tie or a worse winner keeps the current relay. The
/// addition saturates, so extreme inputs cannot wrap around and produce a
/// spurious switch. Moved from the GUI's `race_watch.rs` at W2-3a: the rule is
/// session logic, not presentation.
pub fn decide_switch(current_rtt_ms: u64, winner_rtt_ms: u64, margin_ms: u64) -> bool {
    winner_rtt_ms.saturating_add(margin_ms) < current_rtt_ms
}

// ── Session counters ───────────────────────────────────────────────────────

/// The counter pair for whichever interception mode is running. Redirect wins
/// when both are set because it is the older, narrower mode.
pub fn mode_counter(
    redirect_active: bool,
    redirect_value: u64,
    interceptor_active: bool,
    interceptor_value: u64,
) -> Option<u64> {
    if redirect_active {
        Some(redirect_value)
    } else if interceptor_active {
        Some(interceptor_value)
    } else {
        None
    }
}

/// The `Relayed` row's sent counter for the running mode.
pub fn routed_count(
    redirect_active: bool,
    redirect_pkts: u64,
    interceptor_active: bool,
    interceptor_sent: u64,
) -> Option<u64> {
    mode_counter(
        redirect_active,
        redirect_pkts,
        interceptor_active,
        interceptor_sent,
    )
}

/// The `Relayed` row's returned counter for the running mode.
pub fn returned_count(
    redirect_active: bool,
    redirect_pkts_in: u64,
    interceptor_active: bool,
    interceptor_from_proxy: u64,
) -> Option<u64> {
    mode_counter(
        redirect_active,
        redirect_pkts_in,
        interceptor_active,
        interceptor_from_proxy,
    )
}

/// Counters shown relative to where this session started.
///
/// Saturating, because the engine's counters can restart below the baseline when
/// the interceptor is rebuilt; a plain subtraction would wrap to billions.
pub fn session_delta(sent: u64, returned: u64, base: Option<(u64, u64)>) -> (u64, u64) {
    match base {
        Some((base_sent, base_returned)) => (
            sent.saturating_sub(base_sent),
            returned.saturating_sub(base_returned),
        ),
        None => (sent, returned),
    }
}

/// This poll's counter deltas, or `None` when the interval forwarded nothing.
///
/// The skip is the point: while traffic is held on the direct path the reply
/// counter still ticks from packets already in flight, and a "0 forwards, N
/// replies" interval says nothing about whether the relay carries the server.
pub fn history_delta(last: (u64, u64), now: (u64, u64)) -> Option<(u64, u64)> {
    let forwards = now.0.saturating_sub(last.0);
    let replies = now.1.saturating_sub(last.1);
    (forwards > 0).then_some((forwards, replies))
}

/// The session counter pair with the session's starting baseline folded in.
pub fn session_pair(
    redirect_active: bool,
    redirect_sent: u64,
    redirect_returned: u64,
    interceptor_active: bool,
    interceptor_sent: u64,
    interceptor_returned: u64,
    base: Option<(u64, u64)>,
) -> Option<RelayedPair> {
    let pair = RelayedPair::for_session(
        redirect_active,
        redirect_sent,
        redirect_returned,
        interceptor_active,
        interceptor_sent,
        interceptor_returned,
    )?;
    let (sent, returned) = session_delta(pair.sent, pair.returned, base);
    Some(RelayedPair { sent, returned })
}

// ── Direct ping / triangle ─────────────────────────────────────────────────

/// The direct-ping measurement in microseconds, only while it is fresh enough to
/// be treated as evidence about the path.
pub fn fresh_direct_us(ping: Option<(f32, u64)>) -> Option<u64> {
    let (ms, age) = ping?;
    // A non-finite value (NaN/inf) is not evidence either; spelling it this way
    // rejects it explicitly instead of through a negated float comparison.
    if age > SERVER_PING_FRESH_SECS || !ms.is_finite() || ms <= 0.0 {
        return None;
    }
    Some((f64::from(ms) * 1000.0).round() as u64)
}

/// The direct-ping record, only when it belongs to `server`.
///
/// Every consumer that describes a specific server's path asks through here: a
/// sample measured against another server is not evidence about this one. This
/// is the whole fix for the cross-server floor race observed live (task-24 §4):
/// the server-following poll can run before the health poll, so for one frame
/// the stored record can still describe the previous server, and the ranking
/// must read that as "direct ping not measured", not as this server's floor.
pub fn ping_for_server(ping: Option<ServerPing>, server: Ipv4Addr) -> Option<ServerPing> {
    ping.filter(|record| record.server == server)
}

/// The direct ping in microseconds for `server`, only when the record belongs
/// to that server and is fresh.
pub fn direct_us_for(ping: Option<ServerPing>, server: Ipv4Addr) -> Option<u64> {
    let record = ping_for_server(ping, server)?;
    fresh_direct_us(Some((record.ms, record.age_secs)))
}

/// The direct-ping record after one poll for `server`.
pub fn refreshed_server_ping(server: Ipv4Addr, sample: Option<(f32, u64)>) -> Option<ServerPing> {
    sample.map(|(ms, age_secs)| ServerPing {
        server,
        ms,
        age_secs,
    })
}

/// Compare a whole-path estimate with the measured direct ping.
///
/// `Some((faster, delta_us))`; the only place the window or the trace may call a
/// relayed path "better than direct".
pub fn compare_to_direct(total_leg_us: u64, direct_us: Option<u64>) -> Option<(bool, u64)> {
    let direct = direct_us?;
    if total_leg_us < direct {
        Some((true, direct - total_leg_us))
    } else {
        Some((false, total_leg_us - direct))
    }
}

/// The triangle-inequality floor a fresh direct ping proves for the relay in
/// use: `direct - client leg`, microseconds.
pub fn early_triangle_floor(
    ping: Option<ServerPing>,
    server: Ipv4Addr,
    client_leg_us: Option<u64>,
) -> Option<u64> {
    let direct = direct_us_for(ping, server)?;
    let client = client_leg_us?;
    Some(direct.saturating_sub(client))
}

/// The triangle-inequality verdict for the relay in use, when it holds.
pub fn relay_cannot_beat_direct(
    client_leg_us: Option<u64>,
    ping: Option<ServerPing>,
    server: Ipv4Addr,
) -> Option<(u64, u64)> {
    let client = client_leg_us?;
    let direct = direct_us_for(ping, server)?;
    (client >= direct).then_some((client, direct))
}

// ── Relay-measured server leg ──────────────────────────────────────────────

/// The v1 estimator's published server-leg measurement for `(relay, server)`.
pub fn measured_server_leg(relay: SocketAddrV4, server: Ipv4Addr) -> Option<MeasuredServerLeg> {
    let estimator = crate::destination_estimator()?;
    let estimate = estimator.estimate(relay, server)?;
    Some(MeasuredServerLeg {
        dest_leg_us: estimate.dest_leg_us,
        samples: estimate.samples,
        age: estimator.measurement_age(relay, server),
    })
}

/// The relay's own ICMP measurement of the relay→server leg for the pair.
pub fn relay_measured_server_leg(
    relay: SocketAddrV4,
    server: Ipv4Addr,
) -> Option<crate::ServerLegReport> {
    crate::server_leg_report(relay, server)
}

/// The relay's passive app-RTT report for the pair.
pub fn relay_measured_app_server_leg(
    relay: SocketAddrV4,
    server: Ipv4Addr,
) -> Option<crate::ServerLegAppReport> {
    crate::server_leg_app_report(relay, server)
}

/// The relay's TTL-traceroute report for the pair.
pub fn relay_measured_trace(
    relay: SocketAddrV4,
    server: Ipv4Addr,
) -> Option<crate::ServerTraceReport> {
    crate::server_leg_trace(relay, server)
}

/// The scored row for `relay` in a ranking, if the relay was a candidate at all.
pub fn scored_for_relay(scored: &[ScoredRelay], relay: SocketAddrV4) -> Option<&ScoredRelay> {
    scored.iter().find(|candidate| candidate.relay == relay)
}

/// The route surface's state for the relay-to-server leg of the relay in use.
///
/// Precedence is deliberate and single: a published estimator measurement, else
/// the ranking's own row (triangle floor = a proof, prior = an estimate), with a
/// stronger floor from a ping that landed after the choice allowed to win over
/// the prior (it re-ranks nothing).
pub fn server_leg_state(
    scored: Option<&ScoredRelay>,
    measured: Option<MeasuredServerLeg>,
    early_floor_us: Option<u64>,
) -> ServerLegState {
    use crate::DestinationLegSource;
    if let Some(measured) = measured {
        return ServerLegState::Measured(measured);
    }
    let Some(scored) = scored else {
        return match early_floor_us {
            Some(floor_us) => ServerLegState::Bounded { floor_us },
            None => ServerLegState::Unknown,
        };
    };
    match scored.dest_source {
        DestinationLegSource::Measured => ServerLegState::Unknown,
        DestinationLegSource::GeometricFloor => ServerLegState::Bounded {
            floor_us: scored.dest_leg_us,
        },
        DestinationLegSource::RegionPrior => match early_floor_us {
            // A proof stronger than the estimate is shown as the proof; the
            // estimate is never dressed up as one.
            Some(floor_us) if floor_us > scored.dest_leg_us => ServerLegState::Bounded { floor_us },
            _ => ServerLegState::Prior {
                prior_us: scored.dest_leg_us,
            },
        },
        DestinationLegSource::Unknown => match early_floor_us {
            Some(floor_us) => ServerLegState::Bounded { floor_us },
            None => ServerLegState::Unknown,
        },
    }
}

// ── Relay identity / client legs ───────────────────────────────────────────

/// Every discovered relay's measured client leg, as the chooser consumes it.
pub type RelayLegs = Vec<(SocketAddrV4, Option<RelaySample>)>;

/// Pair measured client-leg samples back onto the proxy entries they belong to.
///
/// A client-leg measurement is taken against a relay's `/health` endpoint, while
/// the relay's tunnel address is the advertised data port. Pairing happens on
/// the IP, which the health endpoint preserves; the result is keyed by the
/// entry's data address so nothing downstream sees a health port.
pub fn pair_relay_legs(
    entries: &[ProxyEntry],
    measured: &[(SocketAddrV4, Option<RelaySample>)],
) -> Vec<(SocketAddrV4, Option<RelaySample>)> {
    entries
        .iter()
        .filter_map(|entry| {
            measured
                .iter()
                .find(|(addr, _)| addr.ip() == entry.addr.ip())
                .map(|(_, latency)| (entry.addr, *latency))
        })
        .collect()
}

/// The proxy entry a measured relay endpoint belongs to.
pub fn proxy_index_for(proxies: &[ProxyEntry], addr: SocketAddrV4) -> Option<usize> {
    proxies
        .iter()
        .position(|entry| same_relay_endpoint(entry.addr, addr))
}

/// Whether two endpoints name the same relay. The `/health` probe uses a
/// different port than the data plane, so only the IP can be compared.
pub fn same_relay_endpoint(a: SocketAddrV4, b: SocketAddrV4) -> bool {
    a.ip() == b.ip()
}

// ── Starvation filter + no-position fallback ranking ──────────────────────

/// The candidate pool after the starvation demotion, and how many relays it
/// dropped.
///
/// A relay is dropped when its record for this exact server says
/// [`CarryVerdict::Starved`]. If every candidate is starved they are still the
/// only relays available, so the unfiltered list stands and nothing is reported
/// as skipped: an empty candidate list would silently disable auto-selection.
pub fn without_starved(
    server: Ipv4Addr,
    candidates: &[RelayCandidate],
    carry: &impl CarryLookup,
) -> (Vec<RelayCandidate>, usize) {
    let survivors: Vec<RelayCandidate> = candidates
        .iter()
        .filter(|candidate| carry.carry_verdict(candidate.relay, server) != CarryVerdict::Starved)
        .cloned()
        .collect();
    if survivors.is_empty() {
        return (candidates.to_vec(), 0);
    }
    let skipped = candidates.len() - survivors.len();
    (survivors, skipped)
}

/// The candidate pool for a server whose position is unknown, restricted to
/// relays with a proven carry record for that exact server when any exist.
pub fn prefer_proven(
    server: Ipv4Addr,
    candidates: Vec<RelayCandidate>,
    carry: &impl CarryLookup,
) -> (Vec<RelayCandidate>, usize) {
    let proven: Vec<RelayCandidate> = candidates
        .iter()
        .filter(|candidate| carry.carry_verdict(candidate.relay, server) == CarryVerdict::Proven)
        .cloned()
        .collect();
    if proven.is_empty() {
        return (candidates, 0);
    }
    let proven_count = proven.len();
    (proven, proven_count)
}

/// Rank candidates for a destination whose position is unknown.
///
/// When there is no exact server position the engine's region prior is a guess
/// about the half of the path the client cannot measure, so it cannot be
/// trusted to order relays. What is real evidence: a recorded carry record
/// proving the relay carried this server, then the measured client leg, then
/// client jitter, then the numerical relay address. Starved relays are removed
/// first, and when any candidate is proven only the proven ones are ranked. The
/// region/position hints are dropped from the candidates (they feed nothing but
/// the prior); a *measured* destination leg is still used because the estimator
/// is keyed by `(relay, server)`.
pub fn rank_without_destination(
    server: Ipv4Addr,
    candidates: &[RelayCandidate],
    direct_us: Option<u64>,
    carry: &impl CarryLookup,
) -> FallbackRanking {
    let (candidates, skipped_starved) = without_starved(server, candidates, carry);
    let (candidates, proven) = prefer_proven(server, candidates, carry);
    let measured_only: Vec<RelayCandidate> = candidates
        .into_iter()
        .map(|candidate| RelayCandidate {
            region: None,
            node_id: None,
            ..candidate
        })
        .collect();
    FallbackRanking {
        ranked: crate::rank_relays_with_direct(server, &measured_only, direct_us),
        skipped_starved,
        proven,
    }
}

/// The `Relayed` row value and its starving flag, from one source pair.
///
/// The pair comes from [`RelayedPair::for_session`] (one mode, both counters);
/// this is the one place the flag is decided, so the word and the row cannot
/// disagree about whether the relay is returning anything.
pub fn relayed_row_value(sent: u64, returned: u64) -> (String, bool) {
    (
        format!("{sent} sent \u{00b7} {returned} returned"),
        RelayedPair { sent, returned }.starving(),
    )
}

/// Whether the start-of-match hold has outlived its fallback on a live run.
///
/// The fallback exists so a hold that never identifies the game server cannot
/// strand a session on the direct path. It must never release a run that is not
/// intercepting: a stopped run has no tunnelling gate, and a stale timestamp
/// left over from the previous session is not a hold.
pub fn stale_hold_due(
    held_since: Option<std::time::Instant>,
    intercepting: bool,
    fallback: std::time::Duration,
) -> bool {
    held_since.is_some_and(|since| intercepting && since.elapsed() >= fallback)
}

// ── Registration state machine (P0-3 / roadmap D4) ─────────────────────────
//
// The control plane is the one shared-status race the 2026-11-23 trace proved:
// a newly learned game server retires the supervisor that the start is awaiting,
// the retired generation's late result arrives as `Done(0)`, and the shipped
// code maps it to a session-wide registration failure. The replacement's
// success had no writer at all, so the ERROR card outlived the condition while
// traffic relayed (`docs/audit/game-transition-state-machine-2026-11-23.md`
// §3.5, invariant I5).
//
// The settlement (`docs/design/windivert-stability-roadmap-2026-11-23.md` D4):
// every registration result belongs to a generation. Only the current generation
// may publish; a retired one is discarded as `superseded`, never mapped to a
// failure, and counted. The rules are pure so the race is testable without a
// relay, a driver or a thread.

/// What applying one registration result did.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RegistrationApplied {
    /// The result belongs to the current generation and is now the state.
    Published,
    /// A retired generation (or an explicit supersede): discarded and counted,
    /// never projected onto a failure.
    Superseded,
}

/// One registration result as the transport reported it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistrationAttempt {
    /// The relay in display form (`"1.2.3.4:4434"`).
    pub relay: Option<String>,
    /// The game server the registration reported, when one was known.
    pub destination: Option<Ipv4Addr>,
    /// The supervisor/registration generation this result belongs to.
    pub generation: u64,
    /// What the attempt did.
    pub outcome: RegistrationOutcome,
    /// The relay-issued session token on success.
    pub session: Option<u32>,
    /// Why the session ended, when the result closes one.
    pub close_reason: Option<SessionCloseReason>,
}

impl RegistrationAttempt {
    /// A successful registration by `generation`.
    pub fn ok(generation: u64, session: u32, relay: impl Into<String>) -> Self {
        Self {
            relay: Some(relay.into()),
            generation,
            outcome: RegistrationOutcome::Ok,
            session: Some(session),
            ..Default::default()
        }
    }

    /// A failed first attempt by `generation`.
    pub fn failed(generation: u64, relay: impl Into<String>) -> Self {
        Self {
            relay: Some(relay.into()),
            generation,
            outcome: RegistrationOutcome::Fail,
            ..Default::default()
        }
    }

    /// A retired result that is not a failure for presentation (I5).
    pub fn superseded(
        generation: u64,
        relay: impl Into<String>,
        close_reason: SessionCloseReason,
    ) -> Self {
        Self {
            relay: Some(relay.into()),
            generation,
            outcome: RegistrationOutcome::Superseded,
            close_reason: Some(close_reason),
            ..Default::default()
        }
    }
}

/// One row of the registration ledger (D4 item 2).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistrationRecord {
    /// The relay this registration was for.
    pub relay: Option<String>,
    /// The game server reported to the relay, when one was known.
    pub destination: Option<Ipv4Addr>,
    /// The generation the record belongs to.
    pub generation: u64,
    /// The relay-issued session token, when the attempt produced one.
    pub session: Option<u32>,
    /// The outcome the record stands for (`Superseded` for discarded results).
    pub outcome: RegistrationOutcome,
    /// The session this record replaced, when one was live.
    pub prev_session: Option<u32>,
    /// Whether the result was discarded as retired.
    pub superseded: bool,
    /// Why the session this record describes ended, when it has.
    pub close_reason: Option<SessionCloseReason>,
}

/// The registration state the shared status publishes (D4 items 1–2).
///
/// At most one session is active. Every session token that reaches the status is
/// resolvable to a [`RegistrationRecord`] here, so a card or a tool can name the
/// registration a token came from.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct RegistrationStatus {
    /// The newest generation the ledger has accepted.
    pub generation: u64,
    /// The one active session, when the current generation registered.
    pub session: Option<u32>,
    /// Whether the current generation is registered.
    pub registered: bool,
    /// Retired results discarded, never mapped to a failure (T11's counter).
    pub superseded_total: u64,
    /// Why the most recent session ended, when one has.
    pub close_reason: Option<SessionCloseReason>,
    /// Bounded history, oldest first (the active session is always inside).
    pub ledger: Vec<RegistrationRecord>,
}

impl RegistrationStatus {
    /// How many ledger rows are kept. Older rows are dropped; an active session
    /// is always the newest Ok row, so it is never the one dropped.
    pub const LEDGER_CAPACITY: usize = 32;

    /// Apply one registration result.
    ///
    /// `superseded` is the transport's verdict (a newer supervisor generation
    /// exists for the relay). A result is also discarded when its generation is
    /// older than the newest one already accepted: an old result never publishes
    /// over a newer one, whatever it says. Both discard paths are the same
    /// thing to the user (`superseded`, never a failure).
    pub fn apply(
        &mut self,
        mut attempt: RegistrationAttempt,
        superseded: bool,
    ) -> RegistrationApplied {
        // An `Ok` without a session cannot be a registration; record it as the
        // failure it is instead of claiming a session that does not exist.
        if attempt.outcome == RegistrationOutcome::Ok && attempt.session.is_none() {
            attempt.outcome = RegistrationOutcome::Fail;
        }
        let stale = attempt.generation < self.generation;
        if superseded || stale || attempt.outcome == RegistrationOutcome::Superseded {
            self.superseded_total = self.superseded_total.saturating_add(1);
            self.push(RegistrationRecord {
                relay: attempt.relay,
                destination: attempt.destination,
                generation: attempt.generation,
                session: attempt.session,
                outcome: RegistrationOutcome::Superseded,
                prev_session: self.session,
                superseded: true,
                close_reason: Some(
                    attempt
                        .close_reason
                        .unwrap_or(SessionCloseReason::SessionReplaced),
                ),
            });
            return RegistrationApplied::Superseded;
        }
        self.generation = attempt.generation;
        let prev_session = self.session;
        match attempt.outcome {
            RegistrationOutcome::Ok => {
                if let Some(previous) = prev_session {
                    if Some(previous) != attempt.session {
                        self.close_session(
                            previous,
                            attempt
                                .close_reason
                                .unwrap_or(SessionCloseReason::SessionReplaced),
                        );
                    }
                }
                self.registered = true;
                self.session = attempt.session;
                self.close_reason = None;
            }
            _ => {
                // Only a current generation's failure may withdraw a session,
                // and a failure is not a close: it records no close reason of
                // its own unless the transport supplied one.
                self.registered = false;
                self.session = None;
                if let Some(reason) = attempt.close_reason {
                    self.close_reason = Some(reason);
                }
            }
        }
        self.push(RegistrationRecord {
            relay: attempt.relay,
            destination: attempt.destination,
            generation: attempt.generation,
            session: attempt.session,
            outcome: attempt.outcome,
            prev_session,
            superseded: false,
            close_reason: attempt.close_reason,
        });
        RegistrationApplied::Published
    }

    /// A same-source healthy result at the same or a newer generation clears a
    /// stale failure (I5/I6). Returns whether anything was published.
    ///
    /// This is the replacement-supervisor path: the destination report retires
    /// the start's supervisor, and the replacement registers without anybody
    /// awaiting it. The next snapshot observes it here, so the newer success has
    /// a writer instead of leaving the retired generation's failure on screen.
    pub fn note_healthy(&mut self, generation: u64, session: u32, relay: Option<String>) -> bool {
        if generation < self.generation {
            return false;
        }
        if self.registered && self.session == Some(session) && generation == self.generation {
            return false;
        }
        let attempt = RegistrationAttempt {
            relay,
            generation,
            outcome: RegistrationOutcome::Ok,
            session: Some(session),
            ..Default::default()
        };
        self.apply(attempt, false) == RegistrationApplied::Published
    }

    /// The active session closed (Stop/Quit/replaced): no failure is recorded.
    /// Returns the session that closed, if one was live.
    pub fn note_closed(&mut self, reason: SessionCloseReason) -> Option<u32> {
        let session = self.session?;
        self.registered = false;
        self.session = None;
        self.close_reason = Some(reason);
        self.close_session(session, reason);
        Some(session)
    }

    /// Record a candidate registration that is **not** the session in use.
    ///
    /// An in-session relay switch registers a candidate on another relay; a
    /// failed candidate keeps the live session (invariant I4's "stay on the old
    /// relay"), so it is a ledger row only. Only a success promotes through
    /// [`Self::apply`]; a failure here never withdraws the active session.
    pub fn note_candidate(&mut self, attempt: RegistrationAttempt) {
        self.push(RegistrationRecord {
            relay: attempt.relay,
            destination: attempt.destination,
            generation: attempt.generation,
            session: attempt.session,
            outcome: attempt.outcome,
            prev_session: None,
            superseded: false,
            close_reason: attempt.close_reason,
        });
    }

    /// The ledger row for the active session, when there is one.
    pub fn active_record(&self) -> Option<&RegistrationRecord> {
        let session = self.session?;
        self.ledger.iter().rev().find(|record| {
            record.session == Some(session) && record.outcome == RegistrationOutcome::Ok
        })
    }

    fn close_session(&mut self, session: u32, reason: SessionCloseReason) {
        if let Some(record) = self
            .ledger
            .iter_mut()
            .rev()
            .find(|record| record.session == Some(session))
        {
            record.close_reason = Some(reason);
        }
    }

    fn push(&mut self, record: RegistrationRecord) {
        if self.ledger.len() == Self::LEDGER_CAPACITY {
            self.ledger.remove(0);
        }
        self.ledger.push(record);
    }
}

// ── Error presentation (P0-3 / invariants I6–I7) ───────────────────────────

/// A latched capture-plane fault (P0-3, roadmap D4 item 4 / invariant I7).
///
/// The one error that may replace a live capture word. It carries the failure
/// kind and the handle-layer close→open age from the interceptor helper, which
/// is exactly the context the 2026-11-23 trace lacked when `FWP_E_IN_USE`
/// followed a Stop. Only a successful start at a newer epoch clears it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CaptureFault {
    /// The failing step (`windivert_open`, `teardown_incomplete`, …).
    pub kind: String,
    /// The diagnostic the backend produced.
    pub detail: String,
    /// The capture plane's epoch when the fault was recorded.
    pub epoch: u64,
    /// Milliseconds since the last successful handle close, when one is known.
    pub close_age_ms: Option<u64>,
}

/// Whether the ERROR word may be rendered for this snapshot (I7).
///
/// While a capture run is live, only a latched capture-plane fault may replace
/// the live word: a route or registration warning is a banner beside the word,
/// because "the card says failed while traffic relays" is the truthfulness
/// defect this rule closes. With no live run there is no state to mask, so any
/// error may take the word.
pub fn error_word_for(has_error: bool, boosting: bool, capture_fault_latched: bool) -> bool {
    if !has_error {
        return false;
    }
    !boosting || capture_fault_latched
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::dto::CarryVerdict;
    use std::collections::HashMap;
    use std::time::Duration;

    fn relay(last: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, last), 4434)
    }

    fn candidate(last: u8, client_leg_us: u64) -> RelayCandidate {
        RelayCandidate {
            relay: relay(last),
            region: Some("test region".to_string()),
            node_id: Some(format!("relay-test-{last}")),
            client_leg_us: Some(client_leg_us),
            client_jitter_us: 1000,
        }
    }

    fn server() -> Ipv4Addr {
        Ipv4Addr::new(203, 0, 113, 7)
    }

    /// A fixture carry memory: no GUI history needed.
    struct Memory(HashMap<(u8, Ipv4Addr), CarryVerdict>);

    impl Memory {
        fn new() -> Self {
            Self(HashMap::new())
        }

        fn set(mut self, last: u8, server: Ipv4Addr, verdict: CarryVerdict) -> Self {
            self.0.insert((last, server), verdict);
            self
        }
    }

    impl CarryLookup for Memory {
        fn carry_verdict(&self, relay: SocketAddrV4, server: Ipv4Addr) -> CarryVerdict {
            self.0
                .get(&(relay.ip().octets()[3], server))
                .copied()
                .unwrap_or(CarryVerdict::Unproven)
        }
    }

    // ── tray ───────────────────────────────────────────────────────────────

    #[test]
    fn the_tray_never_reads_optimizing_while_the_return_path_fails() {
        // Moved from the GUI test cluster at W2-3a (acceptance (a)/(d)). The
        // live honesty defect: the tray claimed "optimizing" in green while the
        // relay returned nothing. Every combination must keep that word away
        // from a failing return path, and a normal boosting session must still
        // say Optimizing.
        for has_error in [false, true] {
            for warning_active in [false, true] {
                for boosting in [false, true] {
                    for connected in [false, true] {
                        let state = tray_state_for(has_error, warning_active, boosting, connected);
                        if has_error {
                            assert_eq!(state, TrayState::Error);
                        } else if warning_active && boosting {
                            assert_eq!(
                                state,
                                TrayState::RelayStarved,
                                "a failing return path needs its own tray state"
                            );
                            assert_ne!(
                                state,
                                TrayState::Optimizing,
                                "the tray must never claim optimization while the return path fails"
                            );
                        } else if boosting {
                            assert_eq!(state, TrayState::Optimizing);
                        } else if connected {
                            assert_eq!(state, TrayState::Connected);
                        } else {
                            assert_eq!(state, TrayState::Disconnected);
                        }
                    }
                }
            }
        }
        // A warning with no traffic flowing is not a running optimization
        // either: the tray falls back to the connection state.
        assert_eq!(
            tray_state_for(false, true, false, true),
            TrayState::Connected
        );
    }

    #[test]
    fn switches_when_the_winner_is_clearly_better() {
        // Moved from the GUI's race_watch.rs at W2-3a (all four cases kept).
        assert!(decide_switch(100, 60, 20));
        assert!(decide_switch(100, 79, 20));
    }

    #[test]
    fn keeps_the_current_relay_within_the_margin() {
        assert!(
            !decide_switch(100, 80, 20),
            "exactly at the margin is a tie"
        );
        assert!(!decide_switch(100, 85, 20));
        assert!(!decide_switch(100, 100, 0));
    }

    #[test]
    fn keeps_the_current_relay_when_the_winner_is_worse() {
        assert!(!decide_switch(50, 90, 5));
        assert!(!decide_switch(50, 51, 0));
    }

    #[test]
    fn extreme_values_do_not_wrap_into_a_switch() {
        assert!(!decide_switch(u64::MAX, u64::MAX, 1));
        assert!(!decide_switch(u64::MAX, u64::MAX - 1, 10));
    }

    // ── action slot ────────────────────────────────────────────────────────

    #[test]
    fn the_action_slot_names_the_three_gates() {
        assert_eq!(
            action_slot(true, true, true, true),
            ActionSlot::StopOptimizing
        );
        assert_eq!(action_slot(true, false, true, true), ActionSlot::StartReady);
        assert_eq!(
            action_slot(true, false, false, true),
            ActionSlot::StartBlocked
        );
        assert_eq!(
            action_slot(true, false, true, false),
            ActionSlot::StartBlocked
        );
    }

    #[test]
    fn the_elevation_invite_needs_a_boost_that_could_otherwise_start() {
        // M1: the invite is an answer to "this would run with rights", so it
        // appears only when connected with a relay.
        assert_eq!(
            action_slot(false, false, true, true),
            ActionSlot::RestartElevated
        );
        assert_eq!(
            action_slot(false, false, false, true),
            ActionSlot::StartBlocked,
            "unelevated but disconnected: there is nothing to elevate for"
        );
        assert_eq!(
            action_slot(false, false, true, false),
            ActionSlot::StartBlocked,
            "unelevated but no relay: there is nothing to elevate for"
        );
        assert_eq!(
            action_slot(false, false, false, false),
            ActionSlot::StartBlocked
        );
        // A running session still offers the stop action first.
        assert_eq!(
            action_slot(false, true, true, true),
            ActionSlot::StopOptimizing
        );
    }

    // ── facts ──────────────────────────────────────────────────────────────

    #[test]
    fn connecting_is_in_flight_until_registration_ends_it() {
        use super::super::dto::connecting_fact;
        assert!(connecting_fact(false, false, None, true));
        assert!(
            !connecting_fact(true, false, None, true),
            "connected ends it"
        );
        assert!(
            !connecting_fact(false, true, None, true),
            "registration ends it"
        );
        assert!(
            !connecting_fact(false, false, Some("refused"), true),
            "a registration error ends it with Error"
        );
        assert!(
            !connecting_fact(false, false, None, false),
            "no relay chosen"
        );
    }

    #[test]
    fn choosing_is_a_hold_sub_state_for_the_locked_server() {
        use super::super::dto::choosing_fact;
        assert!(choosing_fact(true, true, false));
        assert!(!choosing_fact(false, true, false), "not holding");
        assert!(!choosing_fact(true, false, false), "no locked server");
        assert!(
            !choosing_fact(true, true, true),
            "relay chosen for the server ends the choice"
        );
    }

    // ── counters ───────────────────────────────────────────────────────────

    #[test]
    fn relayed_pair_reads_one_mode_and_never_mixes_counters() {
        let redirect = RelayedPair::for_session(true, 7, 3, true, 32, 20);
        assert_eq!(
            redirect,
            Some(RelayedPair {
                sent: 7,
                returned: 3
            }),
            "redirect wins when both are active"
        );
        let interceptor = RelayedPair::for_session(false, 0, 0, true, 32, 20);
        assert_eq!(
            interceptor,
            Some(RelayedPair {
                sent: 32,
                returned: 20
            })
        );
        assert_eq!(RelayedPair::for_session(false, 0, 0, false, 32, 20), None);
    }

    #[test]
    fn a_starving_pair_is_flagged_from_the_same_counters() {
        assert!(RelayedPair {
            sent: 54,
            returned: 0
        }
        .starving());
        assert!(!RelayedPair {
            sent: 54,
            returned: 50
        }
        .starving());
        assert!(!RelayedPair {
            sent: 0,
            returned: 0
        }
        .starving());
    }

    #[test]
    fn the_relayed_row_counts_from_where_the_session_started() {
        assert_eq!(session_delta(180, 95, Some((100, 50))), (80, 45));
        assert_eq!(session_delta(180, 95, None), (180, 95));
        assert_eq!(session_delta(5, 3, Some((100, 50))), (0, 0));
        let pair = session_pair(false, 0, 0, true, 180, 95, Some((100, 50)));
        assert_eq!(
            pair,
            Some(RelayedPair {
                sent: 80,
                returned: 45
            })
        );
    }

    #[test]
    fn counters_come_from_the_running_mode() {
        assert_eq!(routed_count(false, 0, true, 32), Some(32));
        assert_eq!(routed_count(true, 7, true, 32), Some(7));
        assert_eq!(routed_count(false, 0, false, 0), None);
        assert_eq!(returned_count(false, 0, true, 12), Some(12));
        assert_eq!(mode_counter(false, 0, false, 0), None);
    }

    #[test]
    fn an_interval_that_forwarded_nothing_records_nothing() {
        assert_eq!(history_delta((100, 40), (100, 41)), None);
        assert_eq!(history_delta((100, 40), (130, 44)), Some((30, 4)));
        assert_eq!(history_delta((100, 40), (5, 3)), None);
    }

    // ── direct ping / triangle ─────────────────────────────────────────────

    #[test]
    fn a_direct_ping_is_evidence_only_while_fresh_and_for_its_server() {
        let server = Ipv4Addr::new(203, 0, 113, 7);
        let other = Ipv4Addr::new(203, 0, 113, 9);
        let ping = ServerPing {
            server,
            ms: 41.5,
            age_secs: 3,
        };
        assert_eq!(direct_us_for(Some(ping), server), Some(41_500));
        assert_eq!(
            direct_us_for(Some(ping), other),
            None,
            "a ping for another server is not evidence about this one"
        );
        assert_eq!(
            fresh_direct_us(Some((41.5, SERVER_PING_FRESH_SECS))),
            Some(41_500)
        );
        assert_eq!(
            fresh_direct_us(Some((41.5, SERVER_PING_FRESH_SECS + 1))),
            None
        );
        assert_eq!(fresh_direct_us(Some((0.0, 1))), None);
        assert_eq!(fresh_direct_us(None), None);
    }

    #[test]
    fn a_better_than_direct_claim_needs_a_measured_comparison() {
        assert_eq!(compare_to_direct(283_470, None), None);
        assert_eq!(
            compare_to_direct(250_000, Some(283_474)),
            Some((true, 33_474))
        );
        assert_eq!(
            compare_to_direct(368_840, Some(283_474)),
            Some((false, 85_366))
        );
    }

    #[test]
    fn the_early_bound_needs_a_fresh_ping_and_a_measured_client_leg() {
        let server = Ipv4Addr::new(203, 0, 113, 7);
        let ping = ServerPing {
            server,
            ms: 283.47,
            age_secs: 1,
        };
        assert_eq!(
            early_triangle_floor(Some(ping), server, Some(219_170)),
            Some(64_300)
        );
        assert_eq!(early_triangle_floor(None, server, Some(219_170)), None);
        assert_eq!(early_triangle_floor(Some(ping), server, None), None);
    }

    #[test]
    fn the_triangle_verdict_needs_two_fresh_client_measurements() {
        let server = Ipv4Addr::new(203, 0, 113, 7);
        let ping = ServerPing {
            server,
            ms: 283.474,
            age_secs: 1,
        };
        assert_eq!(
            relay_cannot_beat_direct(Some(305_840), Some(ping), server),
            Some((305_840, 283_474))
        );
        assert_eq!(
            relay_cannot_beat_direct(Some(220_170), Some(ping), server),
            None
        );
        assert_eq!(relay_cannot_beat_direct(None, Some(ping), server), None);
    }

    // ── server leg ─────────────────────────────────────────────────────────

    fn scored(dest_leg_us: u64, source: crate::DestinationLegSource) -> ScoredRelay {
        ScoredRelay {
            relay: relay(1),
            total_leg_us: dest_leg_us + 20_000,
            dest_leg_us,
            dest_leg_prior_us: dest_leg_us,
            client_leg_us: 20_000,
            dest_source: source,
            jitter_us: 0,
        }
    }

    #[test]
    fn the_server_leg_state_says_how_the_leg_was_obtained() {
        use crate::DestinationLegSource;
        let published = MeasuredServerLeg {
            dest_leg_us: 41_000,
            samples: 4,
            age: Some(Duration::from_secs(3)),
        };
        assert_eq!(
            server_leg_state(
                Some(&scored(41_000, DestinationLegSource::RegionPrior)),
                Some(published),
                None
            ),
            ServerLegState::Measured(published)
        );
        assert_eq!(
            server_leg_state(
                Some(&scored(41_000, DestinationLegSource::RegionPrior)),
                None,
                None
            ),
            ServerLegState::Prior { prior_us: 41_000 }
        );
        assert_eq!(
            server_leg_state(
                Some(&scored(63_300, DestinationLegSource::GeometricFloor)),
                None,
                None
            ),
            ServerLegState::Bounded { floor_us: 63_300 }
        );
        assert_eq!(
            server_leg_state(Some(&scored(0, DestinationLegSource::Unknown)), None, None),
            ServerLegState::Unknown
        );
        assert_eq!(server_leg_state(None, None, None), ServerLegState::Unknown);
        assert_eq!(
            server_leg_state(None, None, Some(60_000)),
            ServerLegState::Bounded { floor_us: 60_000 }
        );
        // A proof stronger than the prior wins; a weaker one does not.
        assert_eq!(
            server_leg_state(
                Some(&scored(41_000, DestinationLegSource::RegionPrior)),
                None,
                Some(60_000)
            ),
            ServerLegState::Bounded { floor_us: 60_000 }
        );
        assert_eq!(
            server_leg_state(
                Some(&scored(41_000, DestinationLegSource::RegionPrior)),
                None,
                Some(30_000)
            ),
            ServerLegState::Prior { prior_us: 41_000 }
        );
    }

    #[test]
    fn the_surface_describes_the_relay_in_use_not_the_winner() {
        let scored = vec![scored(41_000, crate::DestinationLegSource::RegionPrior)];
        assert!(scored_for_relay(&scored, relay(1)).is_some());
        assert_eq!(scored_for_relay(&scored, relay(9)), None);
    }

    #[test]
    fn expected_path_is_a_floor_and_needs_a_client_leg() {
        use super::super::dto::expected_path_ms;
        assert_eq!(expected_path_ms(20.0, 40_000), Some(60.0));
        assert_eq!(expected_path_ms(0.0, 40_000), None);
        assert_eq!(expected_path_ms(f64::NAN, 40_000), None);
    }

    // ── identity / legs ────────────────────────────────────────────────────

    #[test]
    fn client_legs_pair_onto_data_addresses_not_health_ports() {
        use super::super::discovery::RelaySample;
        let entries = vec![
            ProxyEntry::discovered("relay-a", relay(1), "A"),
            ProxyEntry::custom(relay(2), "B"),
        ];
        let health = SocketAddrV4::new(*relay(1).ip(), 8080);
        let sample = RelaySample {
            median: Duration::from_millis(80),
            jitter: Duration::from_millis(5),
        };
        let legs = pair_relay_legs(&entries, &[(health, Some(sample))]);
        assert_eq!(legs, vec![(relay(1), Some(sample))]);
        assert_eq!(proxy_index_for(&entries, health), Some(0));
    }

    // ── starvation filter ──────────────────────────────────────────────────

    #[test]
    fn a_starved_carry_record_demotes_a_relay_without_a_failed_flag() {
        let server = server();
        let candidates = vec![candidate(1, 20_000), candidate(2, 30_000)];
        let memory = Memory::new().set(1, server, CarryVerdict::Starved);
        let (pool, skipped) = without_starved(server, &candidates, &memory);
        assert_eq!(pool.len(), 1);
        assert_eq!(pool[0].relay, relay(2));
        assert_eq!(skipped, 1);
    }

    #[test]
    fn an_all_starved_pool_stands_rather_than_disabling_selection() {
        let server = server();
        let candidates = vec![candidate(1, 20_000)];
        let memory = Memory::new().set(1, server, CarryVerdict::Starved);
        let (pool, skipped) = without_starved(server, &candidates, &memory);
        assert_eq!(pool.len(), 1, "the only relay available is still offered");
        assert_eq!(skipped, 0);
    }

    #[test]
    fn proven_carry_records_win_when_the_server_position_is_unknown() {
        let server = server();
        let candidates = vec![candidate(1, 20_000), candidate(2, 30_000)];
        let memory = Memory::new().set(2, server, CarryVerdict::Proven);
        let (pool, proven) = prefer_proven(server, candidates.clone(), &memory);
        assert_eq!(proven, 1);
        assert_eq!(pool[0].relay, relay(2));
        let (unproven_pool, proven) = prefer_proven(server, candidates, &Memory::new());
        assert_eq!(proven, 0);
        assert_eq!(unproven_pool.len(), 2, "no proof shrinks nothing");
    }

    #[test]
    fn a_stale_hold_never_releases_a_stopped_run() {
        // The fallback is the one place a hold may end without a server. It must
        // never touch a run that is not intercepting: the timestamp can outlive
        // the run it described (Stop keeps the GUI alive), and releasing a
        // stopped run would be a statement about a gate that does not exist.
        let fallback = Duration::from_secs(120);
        let stale = Some(
            std::time::Instant::now()
                .checked_sub(fallback + Duration::from_secs(1))
                .expect("a clock with a past"),
        );
        assert!(
            !stale_hold_due(stale, false, fallback),
            "a stopped run has no hold to release, however stale the timestamp"
        );
        assert!(
            stale_hold_due(stale, true, fallback),
            "a live run past the fallback is the only release the fallback may make"
        );
        assert!(
            !stale_hold_due(Some(std::time::Instant::now()), true, fallback),
            "a young hold is not stale"
        );
        assert!(
            !stale_hold_due(None, true, fallback),
            "no hold was recorded, so the fallback has nothing to do"
        );
    }

    #[test]
    fn the_no_position_fallback_ranks_measured_client_legs_and_drops_the_prior() {
        let server = server();
        let candidates = vec![candidate(1, 40_000), candidate(2, 20_000)];
        let ranking = rank_without_destination(server, &candidates, None, &Memory::new());
        assert_eq!(ranking.ranked[0].relay, relay(2), "lowest client leg first");
        assert_eq!(ranking.skipped_starved, 0);
        assert_eq!(ranking.proven, 0);
        assert_eq!(
            ranking.ranked[0].dest_source,
            crate::DestinationLegSource::Unknown,
            "the untrusted region prior is not used"
        );
    }

    // ── registration state machine (P0-3 / D4) ─────────────────────────────

    fn relay_name(last: u8) -> String {
        relay(last).to_string()
    }

    /// Register `generation` successfully at `session`.
    fn registered(generation: u64, session: u32) -> RegistrationAttempt {
        RegistrationAttempt::ok(generation, session, relay_name(1))
    }

    #[test]
    fn a_successful_registration_publishes_its_session_and_keeps_one_active() {
        let mut status = RegistrationStatus::default();
        assert_eq!(
            status.apply(registered(1, 1000), false),
            RegistrationApplied::Published
        );
        assert!(status.registered);
        assert_eq!(status.session, Some(1000));
        assert_eq!(status.generation, 1);
        assert_eq!(status.superseded_total, 0);
        let record = status.active_record().expect("the session is resolvable");
        assert_eq!(record.session, Some(1000));
        assert_eq!(record.generation, 1);
        assert_eq!(record.outcome, RegistrationOutcome::Ok);
        assert!(!record.superseded);
        assert_eq!(record.prev_session, None);
    }

    /// **T11** — a retired generation is not a failure.
    ///
    /// The start's supervisor is retired before its first attempt completes; its
    /// late result must be discarded and counted, and it must never withdraw the
    /// live session or publish a failure.
    #[test]
    fn t11_a_superseded_generation_is_discarded_and_counted_never_a_failure() {
        let mut status = RegistrationStatus::default();
        status.apply(registered(1, 1000), false);
        let retired = RegistrationAttempt::failed(1, relay_name(1));

        assert_eq!(
            status.apply(retired, true),
            RegistrationApplied::Superseded,
            "a retired generation's result is superseded"
        );
        assert!(status.registered, "the live session must survive");
        assert_eq!(status.session, Some(1000));
        assert_eq!(status.superseded_total, 1, "superseded accounting");
        let discarded = status
            .ledger
            .last()
            .expect("the discarded result is on record");
        assert!(discarded.superseded);
        assert_eq!(discarded.outcome, RegistrationOutcome::Superseded);
        assert_eq!(
            discarded.close_reason,
            Some(SessionCloseReason::SessionReplaced)
        );

        // ... and the explicit outcome does the same without the flag.
        let mut explicit = RegistrationStatus::default();
        explicit.apply(registered(1, 1001), false);
        assert_eq!(
            explicit.apply(
                RegistrationAttempt::superseded(1, relay_name(1), SessionCloseReason::ClientQuit),
                false
            ),
            RegistrationApplied::Superseded
        );
        assert!(explicit.registered);
        assert_eq!(explicit.superseded_total, 1);
        assert_eq!(
            explicit.ledger.last().and_then(|row| row.close_reason),
            Some(SessionCloseReason::ClientQuit)
        );
    }

    /// **T10** — a stale registration result cannot withdraw a newer session.
    ///
    /// The generation guard is the ledger's own copy of the rule: even a result
    /// the transport labelled current is discarded when a newer generation has
    /// already published. That is what makes the stale ERROR card impossible even
    /// if the transport's supersede signal is late.
    #[test]
    fn t10_a_stale_registration_error_is_discarded_after_a_newer_generation_succeeds() {
        let mut status = RegistrationStatus::default();
        status.apply(registered(1, 1000), false);
        status.apply(registered(2, 2000), false);
        assert_eq!(
            status.session,
            Some(2000),
            "a newer generation replaces the session"
        );
        assert_eq!(
            status.ledger[0].close_reason,
            Some(SessionCloseReason::SessionReplaced),
            "the replaced session's record names why it ended"
        );

        // The retired generation's failure arrives late, labelled current.
        let stale = RegistrationAttempt::failed(1, relay_name(1));
        assert_eq!(
            status.apply(stale, false),
            RegistrationApplied::Superseded,
            "an older generation never publishes over a newer one"
        );
        assert!(status.registered);
        assert_eq!(status.session, Some(2000));
        assert_eq!(status.superseded_total, 1);
    }

    /// A replacement's success has a writer: a session first observed by the
    /// snapshot path (the destination-report re-registration) publishes, and its
    /// success is recorded exactly once.
    #[test]
    fn a_replacement_success_publishes_once_and_clears_the_retired_record() {
        let mut status = RegistrationStatus::default();
        status.apply(RegistrationAttempt::failed(1, relay_name(1)), false);
        assert!(!status.registered);

        assert!(
            status.note_healthy(2, 2000, Some(relay_name(1))),
            "the replacement's success is published"
        );
        assert!(status.registered);
        assert_eq!(status.session, Some(2000));
        assert!(
            !status.note_healthy(2, 2000, Some(relay_name(1))),
            "the same healthy result is idempotent (one record, not one per frame)"
        );
        assert_eq!(status.ledger.len(), 2, "failed then registered");

        // An older generation's healthy result never rolls the state back.
        assert!(!status.note_healthy(1, 999, Some(relay_name(1))));
        assert_eq!(status.session, Some(2000));
    }

    #[test]
    fn a_failed_switch_candidate_keeps_the_active_session() {
        // An in-session relay switch registers a candidate on another relay. A
        // failed candidate keeps the live session (invariant I4): it is a ledger
        // row, never a withdrawal of the session in use.
        let mut status = RegistrationStatus::default();
        status.apply(registered(1, 1000), false);
        status.note_candidate(RegistrationAttempt::failed(2, relay_name(2)));
        assert!(status.registered);
        assert_eq!(status.session, Some(1000));
        assert_eq!(
            status.generation, 1,
            "a candidate does not advance the session's generation"
        );
        assert_eq!(
            status.ledger.last().map(|record| record.outcome),
            Some(RegistrationOutcome::Fail),
            "the candidate failure is on record"
        );
        assert_eq!(status.superseded_total, 0);
    }

    #[test]
    fn a_closed_session_is_not_a_failure_and_stays_resolvable() {
        let mut status = RegistrationStatus::default();
        status.apply(registered(1, 1000), false);
        assert_eq!(
            status.note_closed(SessionCloseReason::ClientQuit),
            Some(1000)
        );
        assert!(!status.registered);
        assert_eq!(status.session, None);
        assert_eq!(status.close_reason, Some(SessionCloseReason::ClientQuit));
        assert_eq!(
            status.ledger.last().and_then(|row| row.close_reason),
            Some(SessionCloseReason::ClientQuit),
            "the closed session's record names why it ended"
        );
        assert_eq!(status.superseded_total, 0, "a close is not a supersede");
        assert_eq!(
            status.note_closed(SessionCloseReason::ClientQuit),
            None,
            "nothing is closed twice"
        );
    }

    #[test]
    fn the_ledger_is_bounded_and_keeps_the_active_session() {
        let mut status = RegistrationStatus::default();
        for generation in 1..=(RegistrationStatus::LEDGER_CAPACITY as u64 + 5) {
            status.apply(registered(generation, 1000 + generation as u32), false);
        }
        assert_eq!(
            status.ledger.len(),
            RegistrationStatus::LEDGER_CAPACITY,
            "history is bounded"
        );
        assert!(
            status.active_record().is_some(),
            "the active session survives"
        );
        assert_eq!(
            status.ledger.last().and_then(|record| record.session),
            status.session
        );
    }

    // ── error presentation (I7 / T13 unit level) ───────────────────────────

    #[test]
    fn t13_a_non_fatal_error_never_takes_the_word_from_a_live_capture() {
        // The observed defect: word=ERROR while traffic relayed. A live capture
        // keeps its word and the error is a banner beside it.
        assert!(!error_word_for(true, true, false));
        // The one error that may mask a live capture is the latched fault.
        assert!(error_word_for(true, true, true));
        // With no live run there is nothing to mask: the error owns the word.
        assert!(error_word_for(true, false, false));
        // No error, no Error word, whatever the fault field says.
        assert!(!error_word_for(false, true, true));
    }
}
