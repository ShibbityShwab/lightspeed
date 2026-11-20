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

/// The active starvation warning: the relay is judged not to be returning this
/// server's traffic, and the window and tray must say so.
///
/// The owner-approved 2026-11-20 behaviour is **warn-only**: this finding never
/// moves the session. The earlier automatic direct fallback (and its re-test
/// budget) is gone because it silently took the route from the player and
/// flapped a live session between relay and direct; the user keeps the choice
/// through STOP OPTIMIZING or the relay picker. The finding is still published
/// to the relay history, so the bad relay is demoted for future ranking.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct RelayStarvationWarning {
    /// The game server whose traffic the relay is not returning.
    pub(super) server: std::net::SocketAddrV4,
    /// The relay that was carrying it, when the relay was known at that moment.
    pub(super) relay: Option<std::net::SocketAddrV4>,
    /// The confirmed fault that raised the warning, so the banner can say
    /// whether the relay or the local injection path is the one failing.
    pub(super) fault: RelayReplyFault,
}

/// A (server, relay) pair the watchdog has just found starving, and the relay it
/// saw (`None` when the relay was not known at that moment).
///
/// The GUI owns the relay history (the watchdog thread cannot borrow `self`), so
/// the finding is published here for the GUI's next poll: the verdict is written
/// and the relay is demoted for the next ranking. The session keeps relaying;
/// the warning has already told the player what is happening. `server` is
/// `SocketAddrV4` so the GUI keys the record on the full address the watchdog
/// judged.
pub(super) type StarvationVerdict = (std::net::SocketAddrV4, Option<std::net::SocketAddrV4>);

/// Why the relay path was judged faulty, for the trace and the banner.
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
/// The live false positive that condemned a relay 150 ms faster than direct was a
/// single 8 s window at a 24.1% return share. A relay is only warned about once
/// the same fault holds for [`RELAY_REPLY_STREAK`] windows in a row; any healthy
/// or inconclusive window clears the streak.
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

/// Whether a judged window proves the return path works again.
///
/// Only a window with no fault at all closes an active warning: the relay is
/// carrying the game server again. A window that saw a fault but has not yet
/// reached [`RELAY_REPLY_STREAK`] is **not** evidence that the relay recovered -
/// it is the first half of the evidence for the next warning, so the warning
/// stays up.
pub(super) fn return_path_recovered(window: Option<RelayReplyFault>) -> bool {
    window.is_none()
}

/// What one judged window does to the warning, as a pure decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum WindowEvent {
    /// Nothing to report: a healthy session, or the first fault window of a
    /// streak that has not reached the threshold yet.
    None,
    /// The confirmed fault is active: the warning is raised or extended.
    Warned,
    /// A window with a working return path cleared an active warning.
    Recovered,
}

/// The full effect of one judged window on the starvation state.
///
/// The watchdog thread applies this plan; the tests pin it. Keeping the decision
/// pure is what makes "a confirmed starvation window warns and never switches
/// the route" testable without an engine: the plan carries every effect a window
/// can have, and [`Self::switch_to_direct`] exists only so that contract is
/// explicit and a future change cannot reintroduce the automatic fallback
/// silently.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct WatchdogPlan {
    /// The warning active after this window (`None` once the return path works
    /// again).
    pub(super) warning: Option<RelayStarvationWarning>,
    /// Whether the GUI must be told to demote the (server, relay) pair. Once per
    /// starvation episode, not once per window.
    pub(super) publish_verdict: bool,
    /// Whether this window wants the session moved to the direct path. Always
    /// `false` in the warn-only mode the owner approved on 2026-11-20: the
    /// watchdog never takes the route from the player.
    pub(super) switch_to_direct: bool,
    /// What the trace should say about this window.
    pub(super) event: WindowEvent,
}

impl WatchdogPlan {
    /// A plan that changes nothing but keeps (or clears) the current warning.
    fn unchanged(warning: Option<RelayStarvationWarning>, event: WindowEvent) -> Self {
        Self {
            warning,
            publish_verdict: false,
            switch_to_direct: false,
            event,
        }
    }
}

/// Whether a confirmed fault is the starvation finding the relay history must
/// remember for this (relay, server) pair.
///
/// Only `NoReplyFromRelay` qualifies. `InjectionFailed` is the local path's
/// fault, and `TooFewReplies` is a low share, which asymmetric game UDP
/// produces normally - neither says anything about the relay's path to the
/// server. The finding is published once per episode (`previous`): the following
/// windows of the same episode repeat it, and the demotion is already written.
/// A different confirmed fault after an earlier one is a new finding.
pub(super) fn starvation_verdict(
    fault: RelayReplyFault,
    previous: Option<RelayReplyFault>,
) -> bool {
    fault == RelayReplyFault::NoReplyFromRelay
        && previous != Some(RelayReplyFault::NoReplyFromRelay)
}

/// Decide what one judged window means for the warning and the relay history.
///
/// `confirmed` is the fault [`ReplyFaultStreak::observe`] returned (the streak
/// threshold was reached), `window` is this window's own verdict. A confirmed
/// fault raises (or refreshes) the warning and may publish the demotion finding;
/// a healthy window clears the warning; a lone fault window changes nothing.
/// The plan never asks for a direct switch - see [`WatchdogPlan::switch_to_direct`].
pub(super) fn plan_return_path_window(
    current: Option<&RelayStarvationWarning>,
    confirmed: Option<RelayReplyFault>,
    window: Option<RelayReplyFault>,
    server: std::net::SocketAddrV4,
    relay: Option<std::net::SocketAddrV4>,
) -> WatchdogPlan {
    if let Some(fault) = confirmed {
        let warning = RelayStarvationWarning {
            server,
            relay,
            fault,
        };
        let publish_verdict = starvation_verdict(fault, current.map(|warning| warning.fault));
        return WatchdogPlan {
            warning: Some(warning),
            publish_verdict,
            // Warn-only: the session keeps the route the player chose. The
            // traffic still flows (or fails) through the relay; the window, the
            // tray and the log are the surfaces that explain it.
            switch_to_direct: false,
            event: WindowEvent::Warned,
        };
    }
    if return_path_recovered(window) && current.is_some() {
        return WatchdogPlan::unchanged(None, WindowEvent::Recovered);
    }
    WatchdogPlan::unchanged(current.copied(), WindowEvent::None)
}

/// Write the watchdog's starvation finding into the relay history.
///
/// This is the load-bearing half of the warn-only mode: the route never changes,
/// but the next ranking must already know that this relay did not carry this
/// server. It lives here, beside the detection, so the demotion path is
/// exercised by a test without an engine.
pub(super) fn apply_starvation_verdict(
    history: &mut crate::relay_history::RelayHistory,
    server: std::net::SocketAddrV4,
    relay: std::net::SocketAddrV4,
) {
    history.record_starvation(relay, *server.ip());
}

/// What one judged window says about the return path, if anything.
///
/// The return path works when at least one datagram in eight comes back and is
/// injected. That is the rule, not a share target: game UDP is asymmetric - a
/// live healthy relay returned 13 for 54 intercepted (24.1%) while 150 ms faster
/// than the direct path - so a share above the floor is normal and not a fault.
/// A window below it only contributes to [`ReplyFaultStreak`]; one window never
/// raises a warning. `None` also covers a window that carried too little traffic
/// to judge (the caller does not observe those at all).
///
/// `from_proxy_since` separates a silent relay from a broken local injection
/// path. The log names which side failed, because a user asking for the reason
/// needs the answer. It counts decoded, non-keepalive data replies: keepalive
/// echoes and undecodable frames never reach the injector, so counting them
/// would blame the local path for a relay that only echoed our own probes.
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
    // window is seeing other traffic (or a mid-session relay switch) and warning
    // about it would accuse a relay that is working. Live evidence: a window
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

/// The trace line for a confirmed fault. Kept beside the decision so the wording
/// cannot drift from the counters it quotes.
///
/// Every arm ends with the same statement of the mode: the relay is kept and the
/// player is warned, because the automatic direct fallback was removed on
/// 2026-11-20. The log is the evidence that the decision was a warning and not a
/// route change.
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
    const KEEP: &str = "keeping the relay and warning the player (no automatic path switch)";
    match fault {
        RelayReplyFault::NoReplyFromRelay => {
            // A local send failure explains the silence without the relay being
            // at fault: the reported 10040 case sent nothing and then blamed the
            // return path. Name the OS error instead of accusing the relay.
            if send_errors > 0 {
                return format!(
                    "The relay cannot be the cause: {send_errors} outbound datagram(s) never \
                     left this machine (last: {}); this is a local path-MTU or socket failure. \
                     {KEEP} (intercepted={intercepted_since} injected={injected_since} \
                     errors={errors} last_error={})",
                    last_send_error.unwrap_or("unknown"),
                    last_error.unwrap_or("none")
                );
            }
            format!(
                "Relay return path is starving the session; {KEEP} \
                 (intercepted={intercepted_since} injected={injected_since} from_relay=0 \
                 errors={errors} send_errors={send_errors} last_error={})",
                last_error.unwrap_or("none")
            )
        }
        RelayReplyFault::InjectionFailed => format!(
            "The relay returned {from_proxy_since} datagram(s) and none could be injected, \
             so this is the local injection path, not the relay; {KEEP} \
             (intercepted={intercepted_since} injected={injected_since} from_relay={from_proxy_since} \
             errors={errors} send_errors={send_errors} last_error={})",
            last_error.unwrap_or("none")
        ),
        RelayReplyFault::TooFewReplies => format!(
            "Relay return path is returning too little of the traffic it carries; {KEEP} \
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

/// Whether a warning belongs to the session it was raised for.
///
/// A warning is per (server, relay) pair. A new game server is a new session, and
/// a relay the session was moved off cannot be judged by the old warning; either
/// one clears it. An unknown relay on either side keeps the warning: the pair is
/// unchanged as far as this process can tell, and dropping it would hide a fault
/// the player may still be feeling.
pub(super) fn warning_is_about(
    warning: &RelayStarvationWarning,
    server: std::net::SocketAddrV4,
    relay: Option<std::net::SocketAddrV4>,
) -> bool {
    warning.server == server
        && match (warning.relay, relay) {
            (Some(abandoned), Some(current)) => abandoned == current,
            _ => true,
        }
}

/// The banner text key for a confirmed fault.
///
/// The relay faults keep the same sentence; an injection failure is the local
/// path's fault, so it must not blame the relay.
#[allow(dead_code)] // the seam owns the warning copy in W2-2; kept for the S4 move
pub(super) fn warning_message_key(fault: RelayReplyFault) -> &'static str {
    match fault {
        RelayReplyFault::InjectionFailed => "route.injection_failed_warning",
        RelayReplyFault::NoReplyFromRelay | RelayReplyFault::TooFewReplies => {
            "route.relay_not_returning"
        }
    }
}

/// Spawn the no-reply return-path watchdog on a background thread.
///
/// It must not run in the UI loop: a relay or interceptor restart can block
/// that thread for seconds, which is exactly when the watchdog is needed. This
/// thread only reads the interceptor counters, so it keeps working while the UI
/// thread is busy.
///
/// It is the only owner of the starvation state. `starved` publishes the first
/// confirmed `NoReplyFromRelay` of an episode, so the GUI can demote the pair for
/// future ranking; `warning` publishes the warning the window and the tray show
/// while the relay is not returning the session's traffic. It never calls
/// `hold_tunnelling`: the session keeps relaying, and the player decides whether
/// to stop or switch relays.
pub(super) fn spawn_relay_reply_watchdog(
    engine: Arc<Mutex<LightSpeedEngine>>,
    quit: QuitFlag,
    starved: Arc<Mutex<Option<StarvationVerdict>>>,
    warning: Arc<Mutex<Option<RelayStarvationWarning>>>,
) {
    std::thread::spawn(move || {
        let mut watch: Option<RelayReplyWatch> = None;
        let mut active: Option<RelayStarvationWarning> = None;
        while !quit.load(std::sync::atomic::Ordering::Relaxed) {
            std::thread::sleep(Duration::from_millis(500));
            let health = {
                let engine = lock_or_recover(&engine);
                engine.interceptor_health()
            };
            let Some((health, server)) =
                health.and_then(|health| health.detected_server.map(|server| (health, server)))
            else {
                // No interceptor, or nothing locked yet: neither a window nor a
                // warning can describe anything.
                watch = None;
                if active.take().is_some() {
                    *lock_or_recover(&warning) = None;
                }
                continue;
            };
            // A different server is a different session; the old warning's
            // history says nothing about it. Moving the session off the relay
            // invalidates the pair just the same.
            let relay_now = lightspeed_client::current_relay();
            if active
                .as_ref()
                .is_some_and(|warning| !warning_is_about(warning, server, relay_now))
            {
                tracing::info!(
                    server = %server,
                    relay = ?relay_now,
                    "The starvation warning belongs to a different session; clearing it"
                );
                active = None;
                *lock_or_recover(&warning) = None;
            }
            if health.tunnelling_enabled != Some(true) {
                // The session is held direct while a relay is chosen (a normal
                // few seconds at match start), or not relaying at all. Nothing
                // is flowing through a relay, so there is no return path to
                // judge and no warning to keep.
                watch = None;
                if active.take().is_some() {
                    *lock_or_recover(&warning) = None;
                    tracing::info!(
                        server = %server,
                        "The relay is not carrying traffic; pausing the return-path watchdog"
                    );
                }
                continue;
            }
            let now = std::time::Instant::now();
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
            let confirmed = current.streak.observe(window);
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

            let plan =
                plan_return_path_window(active.as_ref(), confirmed, window, server, relay_now);
            match plan.event {
                WindowEvent::Warned => {
                    tracing::warn!(
                        server = %server,
                        fault = ?confirmed,
                        "{}",
                        relay_reply_fault_message(
                            confirmed.unwrap_or(RelayReplyFault::NoReplyFromRelay),
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
                WindowEvent::Recovered => {
                    tracing::info!(
                        server = %server,
                        "The relay is returning this server's traffic again; clearing the \
                         starvation warning"
                    );
                }
                WindowEvent::None => {}
            }
            if plan.publish_verdict {
                // Publish before the GUI's next poll; the poll writes the
                // starvation signature into the relay history, demoting the pair
                // for the next ranking.
                *lock_or_recover(&starved) = Some((server, relay_now));
            }
            if plan.switch_to_direct {
                // Warn-only mode. A plan that asks for a route change is a bug:
                // the session belongs to the player.
                tracing::error!(
                    server = %server,
                    "Refusing an automatic direct switch: the return-path watchdog is warn-only"
                );
            }
            active = plan.warning;
            *lock_or_recover(&warning) = active;
            watch = Some(current);
        }
    });
}
