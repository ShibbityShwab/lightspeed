use super::*;

pub(super) const RELAY_REPLY_TIMEOUT: Duration = Duration::from_secs(8);

/// Minimum intercepted packets before the no-reply watchdog may fire.
///
/// The share test needs a sample to mean anything. At two packets, one late
/// reply reads as starvation and one missing reply reads as 50% loss, so a
/// sparse moment at match start took healthy sessions direct. Twenty packets is
/// under a second of a live match and several seconds of a quiet one.
pub(super) const RELAY_REPLY_MIN_INTERCEPTED: u64 = 20;

/// Returned datagrams a window needs before a zero injection count is called a
/// local fault rather than a quiet relay.
pub(super) const RELAY_REPLY_MIN_RETURNED: u64 = 5;

/// Denominator of the return share that counts as a working return path: one
/// reply in eight.
///
/// Game UDP traffic is asymmetric. A live healthy relay returned 13 datagrams
/// for 54 intercepted (24.1%) while the relayed path was 150 ms faster than
/// direct, and the old quarter-share bar called it starved and dropped the
/// session to the slower path. One reply in eight is the floor, not a target:
/// above it the return path works, and only below it can a window say otherwise.
///
/// The value lives in `crate::relay_history` because the history's "carrying"
/// bar and this watchdog's fault floor are one fact; the history's docs carry
/// the full reasoning.
pub(super) const RELAY_REPLY_SHARE_DENOMINATOR: u64 =
    crate::relay_history::RETURN_SHARE_DENOMINATOR;

/// Consecutive judged windows without a working return path before the relay is
/// condemned.
///
/// One window is a sample, not evidence: a loading screen, a relay restart or a
/// router stall can starve eight seconds on a relay that works. The verdict
/// needs the same fault twice in a row, and each window is judged on its own
/// counters so the second one is new evidence.
pub(super) const RELAY_REPLY_STREAK: u32 = 2;

/// One window of evidence about the current relay and game server.
///
/// The interceptor counters are cumulative, so a window starts from a baseline.
/// It is judged only once it has run for [`RELAY_REPLY_TIMEOUT`] and carried
/// enough traffic to mean anything; the baseline is then re-taken, so the next
/// judgement is independent evidence rather than the same counters re-read.
pub(super) struct RelayReplyWatch {
    pub(super) server: std::net::SocketAddrV4,
    /// When the current window started.
    pub(super) window_started: std::time::Instant,
    pub(super) baseline_intercepted: u64,
    pub(super) baseline_injected: u64,
    /// Decoded, non-keepalive data replies when the window started. Keepalive
    /// echoes are excluded upstream: they never reach the injector, so counting
    /// them would report the local path as broken for a relay that only echoed
    /// our own probes.
    pub(super) baseline_reply_data: u64,
    /// This server's own counters when the window started, so the corroboration
    /// covers the same window the relay-wide counters do.
    pub(super) baseline_flow_sent: u64,
    pub(super) baseline_flow_replied: u64,
    /// Consecutive judged windows that showed no working return path.
    pub(super) streak: ReplyFaultStreak,
}

/// A server this watchdog took direct, and when it may try the relay again.
///
/// Without this, one transient fault - a relay restart, a stall during a
/// control-plane reconnect - kept a session on the direct path for the rest of
/// the match, even when the relay was demonstrably carrying replies again: a
/// live match showed 157 sent and 162 back while the header still read DIRECT.
#[derive(Clone, Copy, Debug)]
pub(super) struct ReplyFallback {
    pub(super) server: std::net::SocketAddrV4,
    /// The relay this chain abandoned. A re-test releases the tunnel for the
    /// interceptor that is already running, so it measures whatever relay that
    /// interceptor carries; once the session is on another relay the chain is
    /// dropped rather than re-pointed, because re-pointing restarts interception
    /// under the player.
    pub(super) relay: Option<std::net::SocketAddrV4>,
    /// Whether every window in the chain so far returned zero datagrams from the
    /// relay. A low return share never sets this, and only a confirmed
    /// zero-return chain may be remembered as a relay failure.
    pub(super) zero_return: bool,
    /// Retries already made for this server.
    pub(super) retries: u32,
    /// Earliest moment for the next one.
    pub(super) due: std::time::Instant,
}

/// A server the watchdog gave up on for a relay, as the window reads it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RelayRejection {
    /// The game server whose traffic the relay did not return.
    pub(super) server: std::net::SocketAddrV4,
    /// The relay the chain abandoned, when it was known at the time.
    pub(super) relay: Option<std::net::SocketAddrV4>,
    /// Whether every window of the chain returned zero datagrams from the relay.
    /// Only a confirmed zero-return chain is remembered as a failure; a chain
    /// that did see replies is a low share, which is normal for game UDP.
    pub(super) zero_return: bool,
}

/// The relay the watchdog is re-testing for a server, or waiting to re-test.
///
/// Published so the window can refuse to move the live session onto another
/// relay while the re-test runs: a background re-rank under a fallback is how a
/// relay re-test turned into LAX -> EWR -> FRA hopping on a live run. `None`
/// means no chain is open for any server, and background switches are allowed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RetestTarget {
    pub(super) server: std::net::SocketAddrV4,
    /// The relay being re-tested, when it was known.
    pub(super) relay: Option<std::net::SocketAddrV4>,
}

/// How long the watchdog waits before re-testing a relay it abandoned, in the
/// order the attempts are made. After the last one the session stays direct
/// until the server changes or the run restarts: a relay that failed three
/// spaced tests is not going to carry this server.
pub(super) const RELAY_RETRY_BACKOFF: [Duration; 2] =
    [Duration::from_secs(30), Duration::from_secs(120)];

/// Whole seconds until a pending re-test, for the route line. Floors at one so
/// the text never reads "retry in 0s" just before the relay is promoted.
pub(super) fn retry_seconds_left(
    due: Option<std::time::Instant>,
    now: std::time::Instant,
) -> Option<u64> {
    Some(due?.saturating_duration_since(now).as_secs().max(1))
}

/// The wait before retry number `retries + 1`, or `None` once they are spent.
pub(super) fn retry_delay(retries: u32) -> Option<Duration> {
    RELAY_RETRY_BACKOFF.get(retries as usize).copied()
}

/// Why the relay path was abandoned, for the trace.
///
/// The three causes need different answers from the reader, so the log must not
/// collapse them into one sentence: the first is the relay's or the game
/// server's fault, the second is ours, and the third is a relay that forwards
/// but barely answers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum RelayReplyFault {
    /// The relay produced no datagram for this server at all.
    NoReplyFromRelay,
    /// The relay returned datagrams and the client injected none of them, so
    /// the local injection path is the fault, not the relay.
    InjectionFailed,
    /// The relay returned something, but too little of the traffic it carried.
    TooFewReplies,
}

/// Consecutive judged windows that said the return path is not working.
///
/// The live false positive that dropped a relay 150 ms faster than direct was a
/// single 8 s window at a 24.1% return share. A relay is only condemned once the
/// same fault holds for [`RELAY_REPLY_STREAK`] windows in a row; any healthy or
/// inconclusive window clears the streak.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(super) struct ReplyFaultStreak {
    pub(super) count: u32,
    pub(super) last: Option<RelayReplyFault>,
}

impl ReplyFaultStreak {
    /// Fold in one judged window and return the fault once the streak is long
    /// enough to act on.
    pub(super) fn observe(&mut self, window: Option<RelayReplyFault>) -> Option<RelayReplyFault> {
        match window {
            None => {
                self.count = 0;
                self.last = None;
                None
            }
            Some(fault) => {
                if self.last == Some(fault) {
                    self.count = self.count.saturating_add(1);
                } else {
                    self.count = 1;
                    self.last = Some(fault);
                }
                (self.count >= RELAY_REPLY_STREAK).then_some(fault)
            }
        }
    }
}

/// Whether a pending re-test still points at the relay the chain abandoned.
///
/// The re-test releases the tunnel of the interceptor that is already running,
/// so it measures whatever relay that interceptor carries. If the session was
/// moved - the user picked a relay, the app reconnected - the chain no longer
/// describes a re-test of the abandoned relay: re-enabling it would either test
/// a relay that was never condemned or move the session back (restarting
/// interception mid-session). An unknown relay on either side keeps the chain:
/// the release at worst proves a working relay, and dropping it would leave a
/// session direct with no way back.
pub(super) fn retest_is_about(
    abandoned: Option<std::net::SocketAddrV4>,
    current: Option<std::net::SocketAddrV4>,
) -> bool {
    match (abandoned, current) {
        (Some(abandoned), Some(current)) => abandoned == current,
        _ => true,
    }
}

/// What asked for a background relay change; the no-reply re-test chain treats
/// the two sources differently.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum RelayChangeSource {
    /// The destination-aware ranking. It knows the client legs, the carry history
    /// and whether the server position is known, and its starvation demotion is
    /// what ends a starvation loop, so a re-test chain must never veto it.
    DestinationRanking,
    /// The client-leg re-race. It ranks on the half of the path that says nothing
    /// about the server, so a chain still vetoes a move it would make.
    ClientLegRace,
}

/// Whether a background relay change may move the live session onto `target`.
///
/// A watchdog fallback opens a re-test of one relay. The destination ranking may
/// always move the session: leaving the relay that is starving it is exactly the
/// move a starvation loop needs, and the chain is dropped as stale on the
/// watchdog's next tick once the session is elsewhere. The client-leg re-race has
/// no server evidence, so while the chain is open for the locked server only the
/// relay being re-tested is legal for it - that is what keeps one relay's
/// re-test from becoming LAX -> EWR -> FRA hopping.
pub(super) fn relay_switch_allowed(
    source: RelayChangeSource,
    chain: Option<RetestTarget>,
    locked_server: Option<std::net::Ipv4Addr>,
    target: std::net::SocketAddrV4,
) -> bool {
    match source {
        RelayChangeSource::DestinationRanking => true,
        RelayChangeSource::ClientLegRace => match chain {
            Some(chain) if Some(*chain.server.ip()) == locked_server => chain.relay == Some(target),
            _ => true,
        },
    }
}

/// What one judged window says about the return path, if anything.
///
/// The return path works when at least one datagram in eight comes back and is
/// injected. That is the rule, not a share target: game UDP is asymmetric - a
/// live healthy relay returned 13 for 54 intercepted (24.1%) while 150 ms faster
/// than the direct path - so a share above the floor is normal and not a fault.
/// A window below it only contributes to [`ReplyFaultStreak`]; one window never
/// condemns a relay. `None` also covers a window that carried too little traffic
/// to judge (the caller does not observe those at all).
///
/// `from_proxy_since` separates a silent relay from a broken local injection
/// path. Both are reported as a fallback, but only the first is the relay's
/// fault, and a user asking which side failed needs the log to say so. It
/// counts decoded, non-keepalive data replies: keepalive echoes and undecodable
/// frames never reach the injector, so counting them would blame the local path
/// for a relay that only echoed our own probes.
pub(super) fn relay_reply_fault(
    elapsed: Duration,
    intercepted_since: u64,
    injected_since: u64,
    from_proxy_since: u64,
    flow: Option<(u64, u64)>,
) -> Option<RelayReplyFault> {
    if elapsed < RELAY_REPLY_TIMEOUT
        || intercepted_since < RELAY_REPLY_MIN_INTERCEPTED
        // One reply in eight coming back and reaching the game means the return
        // path works. A higher share is not required to keep a relay.
        || injected_since.saturating_mul(RELAY_REPLY_SHARE_DENOMINATOR) >= intercepted_since
    {
        return None;
    }
    // The window counts every destination the interceptor carries; the flow pair
    // counts this server alone. When the server's own share is healthy, the
    // window is seeing other traffic (or a mid-session relay switch) and dropping
    // to direct would abandon a relay that is working. Live evidence: a window
    // read 152 intercepted / 16 returned while the server's own counters were
    // 528 sent / 520 replied - 98.5%.
    if let Some((sent, replied)) = flow {
        if sent >= RELAY_REPLY_MIN_INTERCEPTED
            && replied.saturating_mul(RELAY_REPLY_SHARE_DENOMINATOR) >= sent
        {
            return None;
        }
    }
    if from_proxy_since == 0 {
        Some(RelayReplyFault::NoReplyFromRelay)
    } else if injected_since == 0 && from_proxy_since >= RELAY_REPLY_MIN_RETURNED {
        // One lone datagram proves nothing about the injector: a relay that
        // barely answers looks the same as one that answers and is dropped.
        // Only a real run of returns with zero injections is a local fault -
        // the live run that mislabelled itself had exactly one.
        Some(RelayReplyFault::InjectionFailed)
    } else {
        Some(RelayReplyFault::TooFewReplies)
    }
}

/// Whether one window belongs to a confirmed zero-return chain.
///
/// Such a chain is the only evidence that may be remembered as a relay failure.
/// It requires the relay to have returned nothing at all AND the client to have
/// injected nothing: a `TooFewReplies` window (normal asymmetric game traffic),
/// or any window that injected even one datagram, can never make a relay look
/// dead.
pub(super) fn zero_return_window(
    fault: RelayReplyFault,
    from_proxy_since: u64,
    injected_since: u64,
) -> bool {
    fault == RelayReplyFault::NoReplyFromRelay && from_proxy_since == 0 && injected_since == 0
}

/// Whether a confirmed fault is the starvation finding the relay history must
/// remember for this (relay, server) pair.
///
/// Only `NoReplyFromRelay` qualifies. `InjectionFailed` is the local path's
/// fault, and `TooFewReplies` is a low share, which asymmetric game UDP
/// produces normally - neither says anything about the relay's path to the
/// server. The finding is published once per chain (`starting_chain`): the
/// following windows of the same chain repeat it, and the chain already owns
/// the re-test cadence.
pub(super) fn starvation_verdict(fault: RelayReplyFault, starting_chain: bool) -> bool {
    starting_chain && fault == RelayReplyFault::NoReplyFromRelay
}

/// A (server, relay) pair the watchdog has just found starving, and the relay it
/// saw (`None` when the relay was not known at that moment).
///
/// The GUI owns the relay history (the watchdog thread cannot borrow `self`), so
/// the finding is published here for the GUI's next poll: the verdict is written,
/// the relay is demoted for the next ranking, and the session leaves it while the
/// watchdog's hold still makes the move free. `server` is `SocketAddrV4` so the
/// GUI keys the record on the full address the watchdog judged, exactly like
/// [`RelayRejection`].
pub(super) type StarvationVerdict = (std::net::SocketAddrV4, Option<std::net::SocketAddrV4>);

/// The trace line for a fault. Kept beside the decision so the wording cannot
/// drift from the counters it quotes.
#[allow(clippy::too_many_arguments)]
pub(super) fn relay_reply_fault_message(
    fault: RelayReplyFault,
    intercepted_since: u64,
    injected_since: u64,
    from_proxy_since: u64,
    errors: u64,
    last_error: Option<&str>,
    send_errors: u64,
    last_send_error: Option<&str>,
) -> String {
    match fault {
        RelayReplyFault::NoReplyFromRelay => {
            // A local send failure explains the silence without the relay being
            // at fault: the reported 10040 case sent nothing and then blamed the
            // return path. Name the OS error instead of accusing the relay.
            if send_errors > 0 {
                return format!(
                    "The relay cannot be the cause: {send_errors} outbound datagram(s) never \
                     left this machine (last: {}); this is a local path-MTU or socket failure. \
                     Switching direct (intercepted={intercepted_since} injected={injected_since} \
                     errors={errors} last_error={})",
                    last_send_error.unwrap_or("unknown"),
                    last_error.unwrap_or("none")
                );
            }
            format!(
                "Relay return path is starving the session; switching it direct \
                 (intercepted={intercepted_since} injected={injected_since} from_relay=0 \
                 errors={errors} send_errors={send_errors} last_error={})",
                last_error.unwrap_or("none")
            )
        }
        RelayReplyFault::InjectionFailed => format!(
            "The relay returned {from_proxy_since} datagram(s) and none could be injected, \
             so this is the local injection path, not the relay; switching direct \
             (intercepted={intercepted_since} injected={injected_since} from_relay={from_proxy_since} \
             errors={errors} send_errors={send_errors} last_error={})",
            last_error.unwrap_or("none")
        ),
        RelayReplyFault::TooFewReplies => format!(
            "Relay return path is starving the session; switching it direct \
             (intercepted={intercepted_since} injected={injected_since} from_relay={from_proxy_since} \
             errors={errors} send_errors={send_errors} last_error={})",
            last_error.unwrap_or("none")
        ),
    }
}

/// The shadow-direct sampler's counters as one judged window sees them.
///
/// The four rejection reasons exist only as `debug!` lines inside the engine and
/// the shipped GUI file sink is INFO, so a live run could not tell an
/// ICMP-blocked path (`no_icmp_floor`) from a pairing artefact (`behind_floor`,
/// `timeout`, `relay`) apart: task-21 observed 1 accepted / 204 rejected in
/// 17 minutes with no reason breakdown in the shipped log. This is the
/// operator-facing form of the same counters, carried by the per-window INFO
/// line. Grouping them also keeps [`relay_reply_window_message`] within
/// clippy's argument limit.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(super) struct ShadowWindowCounters {
    /// Samples that passed every validity rule.
    pub(super) accepted: u64,
    /// Total rejected by any rule (the four fields below sum to it).
    pub(super) rejected: u64,
    /// No fresh measured ICMP median existed for the server.
    pub(super) no_icmp_floor: u64,
    /// Faster than the direct path can physically be.
    pub(super) behind_floor: u64,
    /// The reply did not arrive inside the sampler's window.
    pub(super) timeout: u64,
    /// The relay was unknown or changed between send and reply.
    pub(super) stale_relay: u64,
}

impl ShadowWindowCounters {
    /// Read the process-wide sampler counters into the line's grouped form.
    pub(super) fn read() -> Self {
        let counters = lightspeed_client::latency::shadow_counters();
        Self {
            accepted: counters.accepted,
            rejected: counters.rejected(),
            no_icmp_floor: counters.rejected_no_icmp_floor,
            behind_floor: counters.rejected_behind_floor,
            timeout: counters.rejected_timeout,
            stale_relay: counters.rejected_stale_relay,
        }
    }
}

/// The trace line for every judged return-path window, healthy or not.
///
/// The fault lines above only exist when the watchdog acts, so a run that
/// stayed relayed could only defend itself by the absence of a false positive
/// (QA finding F1). One line per judgement makes that proof direct: the
/// counters the decision read, the share they produce, the verdict, and the
/// streak the next window will extend. Judgements are at least
/// [`RELAY_REPLY_TIMEOUT`] apart by construction - the baselines are re-taken
/// after each one - so the 2 Hz poll cannot turn this into a flood.
///
/// `share` is the ratio the 1-in-8 floor judges, `injected / intercepted`; a
/// window that carried no traffic at all prints `n/a` and is never judged in
/// practice.
///
/// The sampler fields are the engine's validated-sample counters: the
/// quiet-flow probe (`latency::probe_counters`) and the shadow-direct app-RTT
/// sampler (`latency::shadow_counters`). They are here because each sampler's
/// own accepted/rejected lines are `debug!` and the shipped GUI file sink is
/// INFO, so without this line the falsifiers (accepted samples per match, and
/// *why* samples were rejected) would be unmeasurable in a released build. The
/// shadow rejection is broken down by reason because the reason is the whole
/// diagnosis: `no_icmp=204` is an ICMP-blocked path, `floor=204` is a pairing
/// artefact, `timeout=204` is a lost reply and `relay=204` a path that changed.
pub(super) fn relay_reply_window_message(
    intercepted: u64,
    injected: u64,
    from_relay: u64,
    probe: (u64, u64),
    shadow: ShadowWindowCounters,
    streak: &ReplyFaultStreak,
    window: Option<RelayReplyFault>,
) -> String {
    let share = if intercepted == 0 {
        "n/a".to_string()
    } else {
        format!("{:.1}%", 100.0 * injected as f64 / intercepted as f64)
    };
    let verdict = match window {
        None => "healthy",
        Some(RelayReplyFault::NoReplyFromRelay) => "no_reply_from_relay",
        Some(RelayReplyFault::InjectionFailed) => "injection_failed",
        Some(RelayReplyFault::TooFewReplies) => "too_few_replies",
    };
    let (probe_accepted, probe_rejected) = probe;
    format!(
        "Relay reply window judged: intercepted={intercepted} injected={injected} \
         from_relay={from_relay} share={share} streak={} verdict={verdict} \
         probe_accepted={probe_accepted} probe_rejected={probe_rejected} \
         shadow_accepted={} shadow_rejected={}(no_icmp={},floor={},timeout={},relay={})",
        streak.count,
        shadow.accepted,
        shadow.rejected,
        shadow.no_icmp_floor,
        shadow.behind_floor,
        shadow.timeout,
        shadow.stale_relay,
    )
}

/// Spawn the no-reply fallback on a background thread.
///
/// It must not run in the UI loop: a relay or interceptor restart can block
/// that thread for seconds, which is exactly when the fallback is needed. This
/// thread only reads the interceptor counters and flips the tunnelling switch,
/// both cheap, so it keeps working while the UI thread is busy.
///
/// It is also the only owner of the fallback chain. `retest` publishes which
/// relay it is re-testing, so the window does not move the session out from
/// under the re-test; `rejected` carries the chain's verdict, including whether
/// it was a confirmed zero-return chain, so the history is written from proof
/// rather than from one low window; `starved` publishes the first confirmed
/// `NoReplyFromRelay` of a chain, so the GUI can demote the pair and leave the
/// relay while the hold still makes that switch free.
pub(super) fn spawn_relay_reply_watchdog(
    engine: Arc<Mutex<LightSpeedEngine>>,
    quit: QuitFlag,
    rejected: Arc<Mutex<Option<RelayRejection>>>,
    retry_enabled: Arc<std::sync::atomic::AtomicBool>,
    retry_at: Arc<Mutex<Option<std::time::Instant>>>,
    retest: Arc<Mutex<Option<RetestTarget>>>,
    starved: Arc<Mutex<Option<StarvationVerdict>>>,
) {
    std::thread::spawn(move || {
        let mut watch: Option<RelayReplyWatch> = None;
        let mut fallback: Option<ReplyFallback> = None;
        while !quit.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(500));
            let health = {
                let engine = lock_or_recover(&engine);
                engine.interceptor_health()
            };
            let Some(health) = health else {
                watch = None;
                fallback = None;
                *lock_or_recover(&retest) = None;
                continue;
            };
            let Some(server) = health.detected_server else {
                watch = None;
                fallback = None;
                *lock_or_recover(&retest) = None;
                continue;
            };
            let now = std::time::Instant::now();
            // A different server is a different session; the old fallback's
            // history says nothing about it.
            if fallback.as_ref().is_some_and(|f| f.server != server) {
                fallback = None;
                // A countdown from the previous server says nothing about this one.
                *lock_or_recover(&retry_at) = None;
                *lock_or_recover(&retest) = None;
                *lock_or_recover(&starved) = None;
            }
            // A chain is about one relay. If the session has been moved off it
            // (a user pick, a reconnect), the chain is stale: re-enabling the
            // tunnel would test a relay that was never condemned, and moving the
            // session back would restart interception mid-session.
            if let Some(chain) = fallback {
                if chain.server == server
                    && !retest_is_about(chain.relay, lightspeed_client::current_relay())
                {
                    tracing::info!(
                        server = %server,
                        relay = ?chain.relay,
                        "Dropped a stale relay re-test: the session is no longer on the relay it abandoned"
                    );
                    watch = None;
                    fallback = None;
                    *lock_or_recover(&retry_at) = None;
                    *lock_or_recover(&retest) = None;
                    *lock_or_recover(&rejected) = None;
                }
            }
            if health.tunnelling_enabled != Some(true) {
                watch = None;
                // No recorded fallback means this is the GUI's hold, which is
                // not ours to lift.
                let Some(abandoned) = fallback else {
                    continue;
                };
                if now < abandoned.due {
                    fallback = Some(abandoned);
                    continue;
                }
                if lock_or_recover(&engine).release_tunnelling() {
                    *lock_or_recover(&rejected) = None;
                    *lock_or_recover(&retry_at) = None;
                    tracing::info!(
                        server = %abandoned.server,
                        attempt = abandoned.retries + 1,
                        "Re-testing the relay for this server after a direct fallback"
                    );
                    fallback = Some(ReplyFallback {
                        retries: abandoned.retries + 1,
                        due: now,
                        ..abandoned
                    });
                } else {
                    fallback = Some(abandoned);
                }
                continue;
            }
            let mut current = match watch.take() {
                Some(watch) if watch.server == server => watch,
                _ => RelayReplyWatch {
                    server,
                    window_started: now,
                    baseline_intercepted: health.packets_intercepted,
                    baseline_injected: health.packets_injected,
                    baseline_reply_data: health.packets_reply_data,
                    baseline_flow_sent: health.flow.map(|(sent, _)| sent).unwrap_or(0),
                    baseline_flow_replied: health.flow.map(|(_, replied)| replied).unwrap_or(0),
                    streak: ReplyFaultStreak::default(),
                },
            };
            let elapsed = now.duration_since(current.window_started);
            let intercepted_since = health
                .packets_intercepted
                .saturating_sub(current.baseline_intercepted);
            // Too short, or too little traffic to judge: keep the window open and
            // accumulating. Judging it would turn a sparse match into a false
            // positive, and observing it would reset a real streak.
            if elapsed < RELAY_REPLY_TIMEOUT || intercepted_since < RELAY_REPLY_MIN_INTERCEPTED {
                watch = Some(current);
                continue;
            }
            let injected_since = health
                .packets_injected
                .saturating_sub(current.baseline_injected);
            let from_proxy_since = health
                .packets_reply_data
                .saturating_sub(current.baseline_reply_data);
            let (flow_sent, flow_replied) = health
                .flow
                .map(|(sent, replied)| {
                    (
                        sent.saturating_sub(current.baseline_flow_sent),
                        replied.saturating_sub(current.baseline_flow_replied),
                    )
                })
                .unwrap_or((0, 0));
            let window = relay_reply_fault(
                elapsed,
                intercepted_since,
                injected_since,
                from_proxy_since,
                Some((flow_sent, flow_replied)),
            );
            let fault = current.streak.observe(window);
            // One line per judgement, healthy or not: direct evidence that the
            // watchdog looked at this window and what it saw, instead of an
            // absence of WARN lines standing in for a verdict. The quiet-flow
            // probe counters ride along because their own trace is debug-level
            // and the shipped file sink is INFO (QA finding: F1 unmeasurable);
            // the shadow sampler's rejection reasons ride along for the same
            // reason (task-21: 1 accepted / 204 rejected, reason unobservable).
            let probe = lightspeed_client::latency::probe_counters();
            let shadow = ShadowWindowCounters::read();
            tracing::info!(
                server = %server,
                "{}",
                relay_reply_window_message(
                    intercepted_since,
                    injected_since,
                    from_proxy_since,
                    (probe.accepted, probe.rejected()),
                    shadow,
                    &current.streak,
                    window,
                )
            );
            // Each judgement starts a fresh window: the next verdict must be new
            // evidence, not the same counters re-read half a second later.
            current.window_started = now;
            current.baseline_intercepted = health.packets_intercepted;
            current.baseline_injected = health.packets_injected;
            current.baseline_reply_data = health.packets_reply_data;
            current.baseline_flow_sent = health.flow.map(|(sent, _)| sent).unwrap_or(0);
            current.baseline_flow_replied = health.flow.map(|(_, replied)| replied).unwrap_or(0);
            let Some(fault) = fault else {
                // The return path works. A chain that was open is over: the relay
                // carries the server again, and keeping the leash would spend an
                // old retry budget on a fault that has not happened.
                if fallback.is_some() {
                    fallback = None;
                    *lock_or_recover(&retry_at) = None;
                    *lock_or_recover(&retest) = None;
                    *lock_or_recover(&starved) = None;
                    tracing::info!(
                        server = %server,
                        "The relay is returning this server's traffic again; the fallback chain is closed"
                    );
                }
                watch = Some(current);
                continue;
            };
            let retries = fallback
                .as_ref()
                .filter(|f| f.server == server)
                .map(|f| f.retries)
                .unwrap_or(0);
            // A finding is published for the first window of a chain only; the
            // following windows of the same chain repeat it and the chain already
            // owns the re-test cadence.
            let starting_chain = fallback.as_ref().is_none_or(|f| f.server != server);
            // Only a chain in which every window returned nothing at all is a
            // confirmed zero-return chain, and only that may become a history
            // verdict. A low share (TooFewReplies), or any window with an
            // injection, never qualifies.
            let zero_return = fallback
                .as_ref()
                .filter(|f| f.server == server)
                .map(|f| f.zero_return)
                .unwrap_or(true)
                && zero_return_window(fault, from_proxy_since, injected_since);
            if lock_or_recover(&engine).hold_tunnelling() {
                // A day out is "never" for a match; it keeps the retry branch
                // armed without another field to carry it.
                let wait = if retry_enabled.load(std::sync::atomic::Ordering::Relaxed) {
                    retry_delay(retries).unwrap_or(Duration::from_secs(60 * 60 * 24))
                } else {
                    // Retries switched off: this is the last word, so the server
                    // is recorded as rejecting the relay straight away.
                    Duration::from_secs(60 * 60 * 24)
                };
                let abandoned_relay = lightspeed_client::current_relay();
                // Publish the finding before the hold, so the GUI's next poll can
                // write the verdict and leave this relay while the hold makes the
                // move free. Only a NoReplyFromRelay chain is a starvation
                // finding; the other faults are not evidence about the relay.
                if starvation_verdict(fault, starting_chain) {
                    *lock_or_recover(&starved) = Some((server, abandoned_relay));
                }
                *lock_or_recover(&rejected) =
                    retry_delay(retries).is_none().then_some(RelayRejection {
                        server,
                        relay: abandoned_relay,
                        zero_return,
                    });
                // Publish the pending retry so the status can say when the
                // route flips back instead of flipping wordlessly.
                *lock_or_recover(&retry_at) = retry_delay(retries)
                    .filter(|_| retry_enabled.load(std::sync::atomic::Ordering::Relaxed))
                    .map(|w| now + w);
                // Publish the chain so the window keeps the session on the relay
                // being re-tested instead of re-ranking the route underneath it.
                *lock_or_recover(&retest) = Some(RetestTarget {
                    server,
                    relay: abandoned_relay,
                });
                fallback = Some(ReplyFallback {
                    server,
                    relay: abandoned_relay,
                    zero_return,
                    retries,
                    due: now + wait,
                });
                tracing::warn!(
                    server = %current.server,
                    fault = ?fault,
                    "{}",
                    relay_reply_fault_message(
                        fault,
                        intercepted_since,
                        injected_since,
                        from_proxy_since,
                        health.errors,
                        health.last_error.as_deref(),
                        health.send_errors,
                        health.last_send_error.as_deref(),
                    )
                );
            }
        }
    });
}
