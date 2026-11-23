//! The warn-only return-path watchdog - moved out of the GUI at WF-081 W2-3a.
//!
//! This module owns the whole starvation judgement: the judged-window fault
//! classification, the per-(server, relay) fault streak, the pure
//! [`WatchdogPlan`] that turns a window into warning/publish/clear effects, the
//! relay-history demotion, and the background thread that reads the engine's
//! interceptor counters and publishes a [`WatchdogSnapshot`].
//!
//! **Behaviour frozen** (`docs/design/gui-core-boundary.md` Â§6, rule 8; owner
//! decision 2026-11-20): the module is warn-only. A confirmed no-return window
//! raises a warning, the session keeps relaying, the failing pair is demoted in
//! the relay history, and a fully healthy judged window clears the warning.
//! [`WatchdogPlan::switch_to_direct`] is always `false`; nothing here may move
//! the route. The GUI keeps only the thread wiring, the publication read and
//! the presentation of the published facts.
//!
//! The fault/presentation types are the shared DTO's: [`ReturnPathFault`] and
//! [`StarvationWarning`] are the same facts the shell renders, so the S3a
//! mapping layer between a GUI enum and the DTO is gone.

use std::net::SocketAddrV4;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

use crate::LightSpeedEngine;

pub use super::dto::{ReturnPathFault, StarvationWarning};
use super::history::{RelayHistory, RETURN_SHARE_DENOMINATOR};
use super::lifecycle;

/// Lock a mutex, recovering a poisoned lock instead of panicking.
///
/// The watchdog thread must survive a poisoned engine lock long enough to stop
/// cleanly; a panic here would kill the only owner of the starvation state.
fn lock_or_recover<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

pub const RELAY_REPLY_TIMEOUT: Duration = Duration::from_secs(8);

/// Minimum intercepted packets before the no-reply watchdog may fire.
///
/// The share test needs a sample to mean anything. At two packets, one late
/// reply reads as starvation and one missing reply reads as 50% loss, so a
/// sparse moment at match start took healthy sessions direct. Twenty packets is
/// under a second of a live match and several seconds of a quiet one.
pub const RELAY_REPLY_MIN_INTERCEPTED: u64 = 20;

/// Returned datagrams a window needs before a zero injection count is called a
/// local fault rather than a quiet relay.
pub const RELAY_REPLY_MIN_RETURNED: u64 = 5;

/// Denominator of the return share that counts as a working return path: one
/// reply in eight.
///
/// Game UDP traffic is asymmetric. A live healthy relay returned 13 datagrams
/// for 54 intercepted (24.1%) while the relayed path was 150 ms faster than
/// direct, and the old quarter-share bar called it starved and dropped the
/// session to the slower path. One reply in eight is the floor, not a target:
/// above it the return path works, and only below it can a window say otherwise.
///
/// The value lives in `super::history` because the history's "carrying"
/// bar and this watchdog's fault floor are one fact; the history's docs carry
/// the full reasoning.
pub const RELAY_REPLY_SHARE_DENOMINATOR: u64 = RETURN_SHARE_DENOMINATOR;

/// Consecutive judged windows without a working return path before the relay is
/// condemned.
///
/// One window is a sample, not evidence: a loading screen, a relay restart or a
/// router stall can starve eight seconds on a relay that works. The verdict
/// needs the same fault twice in a row, and each window is judged on its own
/// counters so the second one is new evidence.
pub const RELAY_REPLY_STREAK: u32 = 2;

/// One window of evidence about the current relay and game server.
///
/// The interceptor counters are cumulative, so a window starts from a baseline.
/// It is judged only once it has run for [`RELAY_REPLY_TIMEOUT`] and carried
/// enough traffic to mean anything; the baseline is then re-taken, so the next
/// judgement is independent evidence rather than the same counters re-read.
pub struct RelayReplyWatch {
    pub server: std::net::SocketAddrV4,
    /// When the current window started.
    pub window_started: std::time::Instant,
    pub baseline_intercepted: u64,
    pub baseline_injected: u64,
    /// Decoded, non-keepalive data replies when the window started. Keepalive
    /// echoes are excluded upstream: they never reach the injector, so counting
    /// them would report the local path as broken for a relay that only echoed
    /// our own probes.
    pub baseline_reply_data: u64,
    /// This server's own counters when the window started, so the corroboration
    /// covers the same window the relay-wide counters do.
    pub baseline_flow_sent: u64,
    pub baseline_flow_replied: u64,
    /// Consecutive judged windows that showed no working return path.
    pub streak: ReplyFaultStreak,
}

/// A (server, relay) pair the watchdog has just found starving, and the relay it
/// saw (`None` when the relay was not known at that moment).
///
/// The GUIs relay history is written by the consumer (the watchdog thread cannot
/// borrow it), so the finding is published here for the next poll: the verdict is
/// written and the relay is demoted for the next ranking. The session keeps
/// relaying; the warning has already told the player what is happening. `server`
/// is `SocketAddrV4` so the consumer keys the record on the full address the
/// watchdog judged.
pub type StarvationVerdict = (SocketAddrV4, Option<SocketAddrV4>);

/// Consecutive judged windows that said the return path is not working.
///
/// The live false positive that condemned a relay 150 ms faster than direct was a
/// single 8 s window at a 24.1% return share. A relay is only warned about once
/// the same fault holds for [`RELAY_REPLY_STREAK`] windows in a row; any healthy
/// or inconclusive window clears the streak.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ReplyFaultStreak {
    pub count: u32,
    pub last: Option<ReturnPathFault>,
}

impl ReplyFaultStreak {
    /// Fold in one judged window and return the fault once the streak is long
    /// enough to act on.
    pub fn observe(&mut self, window: Option<ReturnPathFault>) -> Option<ReturnPathFault> {
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
pub fn return_path_recovered(window: Option<ReturnPathFault>) -> bool {
    window.is_none()
}

/// What one judged window does to the warning, as a pure decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WindowEvent {
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
pub struct WatchdogPlan {
    /// The warning active after this window (`None` once the return path works
    /// again).
    pub warning: Option<StarvationWarning>,
    /// Whether the GUI must be told to demote the (server, relay) pair. Once per
    /// starvation episode, not once per window.
    pub publish_verdict: bool,
    /// Whether this window wants the session moved to the direct path. Always
    /// `false` in the warn-only mode the owner approved on 2026-11-20: the
    /// watchdog never takes the route from the player.
    pub switch_to_direct: bool,
    /// What the trace should say about this window.
    pub event: WindowEvent,
}

impl WatchdogPlan {
    /// A plan that changes nothing but keeps (or clears) the current warning.
    fn unchanged(warning: Option<StarvationWarning>, event: WindowEvent) -> Self {
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
pub fn starvation_verdict(fault: ReturnPathFault, previous: Option<ReturnPathFault>) -> bool {
    fault == ReturnPathFault::NoReplyFromRelay
        && previous != Some(ReturnPathFault::NoReplyFromRelay)
}

/// Decide what one judged window means for the warning and the relay history.
///
/// `confirmed` is the fault [`ReplyFaultStreak::observe`] returned (the streak
/// threshold was reached), `window` is this window's own verdict. A confirmed
/// fault raises (or refreshes) the warning and may publish the demotion finding;
/// a healthy window clears the warning; a lone fault window changes nothing.
/// The plan never asks for a direct switch - see [`WatchdogPlan::switch_to_direct`].
pub fn plan_return_path_window(
    current: Option<&StarvationWarning>,
    confirmed: Option<ReturnPathFault>,
    window: Option<ReturnPathFault>,
    server: std::net::SocketAddrV4,
    relay: Option<std::net::SocketAddrV4>,
) -> WatchdogPlan {
    if let Some(fault) = confirmed {
        let warning = StarvationWarning {
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
pub fn apply_starvation_verdict(
    history: &mut RelayHistory,
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
pub fn relay_reply_fault(
    elapsed: Duration,
    intercepted_since: u64,
    injected_since: u64,
    from_proxy_since: u64,
    flow: Option<(u64, u64)>,
) -> Option<ReturnPathFault> {
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
        Some(ReturnPathFault::NoReplyFromRelay)
    } else if injected_since == 0 && from_proxy_since >= RELAY_REPLY_MIN_RETURNED {
        // One lone datagram proves nothing about the injector: a relay that
        // barely answers looks the same as one that answers and is dropped.
        // Only a real run of returns with zero injections is a local fault -
        // the live run that mislabelled itself had exactly one.
        Some(ReturnPathFault::InjectionFailed)
    } else {
        Some(ReturnPathFault::TooFewReplies)
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
pub fn relay_reply_fault_message(
    fault: ReturnPathFault,
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
        ReturnPathFault::NoReplyFromRelay => {
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
        ReturnPathFault::InjectionFailed => format!(
            "The relay returned {from_proxy_since} datagram(s) and none could be injected, \
             so this is the local injection path, not the relay; {KEEP} \
             (intercepted={intercepted_since} injected={injected_since} from_relay={from_proxy_since} \
             errors={errors} send_errors={send_errors} last_error={})",
            last_error.unwrap_or("none")
        ),
        ReturnPathFault::TooFewReplies => format!(
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
pub struct ShadowWindowCounters {
    /// Samples that passed every validity rule.
    pub accepted: u64,
    /// Total rejected by any rule (the four fields below sum to it).
    pub rejected: u64,
    /// No fresh measured ICMP median existed for the server.
    pub no_icmp_floor: u64,
    /// Faster than the direct path can physically be.
    pub behind_floor: u64,
    /// The reply did not arrive inside the sampler's window.
    pub timeout: u64,
    /// The relay was unknown or changed between send and reply.
    pub stale_relay: u64,
}

impl ShadowWindowCounters {
    /// Read the process-wide sampler counters into the line's grouped form.
    pub fn read() -> Self {
        let counters = crate::latency::shadow_counters();
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
pub fn relay_reply_window_message(
    intercepted: u64,
    injected: u64,
    from_relay: u64,
    probe: (u64, u64),
    shadow: ShadowWindowCounters,
    streak: &ReplyFaultStreak,
    window: Option<ReturnPathFault>,
) -> String {
    let share = if intercepted == 0 {
        "n/a".to_string()
    } else {
        format!("{:.1}%", 100.0 * injected as f64 / intercepted as f64)
    };
    let verdict = match window {
        None => "healthy",
        Some(ReturnPathFault::NoReplyFromRelay) => "no_reply_from_relay",
        Some(ReturnPathFault::InjectionFailed) => "injection_failed",
        Some(ReturnPathFault::TooFewReplies) => "too_few_replies",
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
pub fn warning_is_about(
    warning: &StarvationWarning,
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
pub fn warning_message_key(fault: ReturnPathFault) -> &'static str {
    match fault {
        ReturnPathFault::InjectionFailed => "route.injection_failed_warning",
        ReturnPathFault::NoReplyFromRelay | ReturnPathFault::TooFewReplies => {
            "route.relay_not_returning"
        }
    }
}

/// What the watchdog thread publishes for the rest of the app to read.
///
/// One published value instead of the old three separate `Arc`s: the warning,
/// the live streak evidence and the one-shot demotion finding are the same
/// judgement and must be read coherently.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct WatchdogSnapshot {
    /// The active starvation warning, if the return path is judged faulty.
    pub warning: Option<StarvationWarning>,
    /// Consecutive judged windows that showed the same fault. `Some` only while
    /// a streak is live (`count > 0`); `None` when the last judged window was
    /// healthy or nothing has been judged. Never a fabricated value (R16).
    pub streak: Option<u32>,
    /// The first confirmed `NoReplyFromRelay` of an episode, for the relay-history
    /// demotion. The consumer takes it once; it is not a route command.
    pub verdict: Option<StarvationVerdict>,
}

/// Fold one judged window's plan and streak into the published snapshot.
///
/// The single place that turns a [`WatchdogPlan`] into published facts; the
/// thread calls it and the tests pin it without a thread or an engine. The
/// streak is `Some` exactly when the streak counter is non-zero, so a shell can
/// never read a streak the watchdog does not have.
pub fn apply_plan(
    snapshot: &mut WatchdogSnapshot,
    plan: &WatchdogPlan,
    streak_count: u32,
    server: SocketAddrV4,
    relay: Option<SocketAddrV4>,
) {
    snapshot.warning = plan.warning;
    snapshot.streak = (streak_count > 0).then_some(streak_count);
    if plan.publish_verdict {
        snapshot.verdict = Some((server, relay));
    }
}

/// Clear the published warning and streak (no interceptor, nothing locked, the
/// tunnel is held direct, or the warning belonged to a different session).
pub fn clear_publication(publication: &Arc<Mutex<WatchdogSnapshot>>) {
    let mut snapshot = lock_or_recover(publication);
    snapshot.warning = None;
    snapshot.streak = None;
}

/// Spawn the no-reply return-path watchdog on a background thread.
///
/// It must not run in the UI loop: a relay or interceptor restart can block
/// that thread for seconds, which is exactly when the watchdog is needed. This
/// thread only reads the interceptor counters, so it keeps working while the UI
/// thread is busy.
///
/// It is the only owner of the starvation state. The publication carries the
/// warning the window and tray show, the live streak (R16) and the one-shot
/// demotion finding the consumer writes into the relay history. It never calls
/// `hold_tunnelling`: the session keeps relaying, and the player decides
/// whether to stop or switch relays.
pub fn spawn_relay_reply_watchdog(
    engine: Arc<Mutex<LightSpeedEngine>>,
    quit: Arc<std::sync::atomic::AtomicBool>,
    publication: Arc<Mutex<WatchdogSnapshot>>,
) {
    std::thread::spawn(move || {
        let mut watch: Option<RelayReplyWatch> = None;
        let mut active: Option<StarvationWarning> = None;
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
                    clear_publication(&publication);
                }
                continue;
            };
            // A different server is a different session; the old warning's
            // history says nothing about it. Moving the session off the relay
            // invalidates the pair just the same.
            let relay_now = crate::current_relay();
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
                clear_publication(&publication);
            }
            if health.tunnelling_enabled != Some(true) {
                // The session is held direct while a relay is chosen (a normal
                // few seconds at match start), or not relaying at all. Nothing
                // is flowing through a relay, so there is no return path to
                // judge and no warning to keep.
                watch = None;
                if active.take().is_some() {
                    clear_publication(&publication);
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
            let probe = crate::latency::probe_counters();
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
            // P0-1: the structured twin of the judged-window line. The verdict,
            // streak and counters are the same values, so a harness can assert
            // on fields instead of parsing prose. Log only; the warn-only policy
            // above is unchanged.
            let verdict = match window {
                None => lifecycle::WatchdogVerdict::Healthy,
                Some(ReturnPathFault::NoReplyFromRelay) => {
                    lifecycle::WatchdogVerdict::NoReplyFromRelay
                }
                Some(ReturnPathFault::InjectionFailed) => {
                    lifecycle::WatchdogVerdict::InjectionFailed
                }
                Some(ReturnPathFault::TooFewReplies) => lifecycle::WatchdogVerdict::TooFewReplies,
            };
            lifecycle::emit(
                "watchdog",
                &lifecycle::Watchdog {
                    verdict,
                    streak: current.streak.count,
                    fault: confirmed.map(|fault| format!("{fault:?}")),
                    action: if confirmed.is_some() {
                        lifecycle::WatchdogAction::Demote
                    } else {
                        lifecycle::WatchdogAction::Keep
                    },
                    relay: relay_now.map(|relay| relay.to_string()),
                    server: Some(server.to_string()),
                    intercepted: intercepted_since,
                    injected: injected_since,
                    from_relay: from_proxy_since,
                },
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
                            confirmed.unwrap_or(ReturnPathFault::NoReplyFromRelay),
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
            {
                // Publish before the GUI's next poll: the warning and live
                // streak for the surfaces, and the one-shot finding the poll
                // writes into the relay history (demoting the pair for the next
                // ranking).
                let mut snapshot = lock_or_recover(&publication);
                apply_plan(
                    &mut snapshot,
                    &plan,
                    current.streak.count,
                    server,
                    relay_now,
                );
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
            watch = Some(current);
        }
    });
}
// ── Relay-health judgement (S4: moved from the GUI's relay_choice.rs) ──────
//
// A second, narrower judgement: did the datagrams the client decided to relay
// actually leave the machine? It reads the tunnel's own counters, never the
// relay's, and it informs a suggestion - it never switches anything.

/// How long a run must have been relaying before the relay verdict starts
/// sampling. A match takes several seconds to open its flow, and judging it
/// before then would call a healthy relay broken.
pub const RELAY_HEALTH_SETTLE: Duration = Duration::from_secs(10);

/// The window each relay verdict is measured over. One second is long enough
/// for a game's packet rate to make a lost share visible and short enough that
/// a fault is reported while the player is still in the match.
pub const RELAY_HEALTH_WINDOW: Duration = Duration::from_secs(1);

/// Share of a window's relayed datagrams that may fail to leave before the
/// relay is called broken. UDP games tolerate loss; a fifth of the game's
/// traffic not leaving the machine for the whole window is a fault, not jitter.
pub const RELAY_HEALTH_MAX_UNSENT_SHARE: f64 = 0.2;

/// The tunnel counters as of the last relay-verdict sample.
#[derive(Clone, Copy, Debug)]
pub struct RelayHealthBase {
    pub at: std::time::Instant,
    pub intercepted: u64,
    pub sent: u64,
}

/// What one window of tunnel counters says about the relay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RelayHealthVerdict {
    /// Nothing was decided to relay during the window: menus, or a hold.
    Idle,
    /// Every datagram decided to relay left the machine.
    Healthy,
    /// A share of them did not leave the machine.
    Unsent { unsent: u64, decided: u64 },
}

impl RelayHealthVerdict {
    pub fn sentence(&self) -> String {
        match self {
            Self::Idle => "no traffic to judge".to_string(),
            // The client sees these counters: intercepted datagrams against
            // datagrams the tunnel sent. It cannot see what the relay did with
            // them afterwards, so the sentence must not claim the relay's
            // forwarding is healthy - only this machine's outbound leg.
            Self::Healthy => {
                "every relayed datagram left for the relay; the outbound leg is clean".to_string()
            }
            Self::Unsent { unsent, decided } => format!(
                "{unsent} of {decided} relayed datagrams never left for the relay in the last \
                 second; if the match feels like packet loss, switch relay"
            ),
        }
    }
}

/// Whether a relay change may be applied to the live tunnel now.
///
/// The tunnel reads the current relay on every packet, so a change applied while
/// datagrams are flowing goes to a relay that may not have this session yet. A
/// hold withholds tunnelling, and that is the one state in which the switch is
/// safe; a backend with no gate never tunnels at all, so nothing is at risk.
///
/// A test pins it, which is why it is not dead code in a release build.
#[cfg_attr(not(test), allow(dead_code))]
pub fn switch_is_safe(tunnelling_enabled: Option<bool>) -> bool {
    tunnelling_enabled != Some(true)
}

/// Judge one window of tunnel counters.
///
/// The comparison is on *movement*: a run that relayed a million packets and
/// then started failing must not be excused by its healthy past, so the caller
/// passes the counters as of the previous sample, not the totals.
pub fn relay_health_verdict(
    prev_intercepted: u64,
    prev_sent: u64,
    intercepted: u64,
    sent: u64,
) -> RelayHealthVerdict {
    let decided = intercepted.saturating_sub(prev_intercepted);
    let left = sent.saturating_sub(prev_sent).min(decided);
    if decided == 0 {
        return RelayHealthVerdict::Idle;
    }
    let unsent = decided - left;
    let share = unsent as f64 / decided as f64;
    if share > RELAY_HEALTH_MAX_UNSENT_SHARE {
        RelayHealthVerdict::Unsent { unsent, decided }
    } else {
        RelayHealthVerdict::Healthy
    }
}

/// Hysteresis for a connected re-race: a winner must beat the relay in use by
/// this many milliseconds before the window switches away from it.
pub const RELAY_SWITCH_MARGIN_MS: u64 = 20;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::dto::StarvationWarning;

    fn addr(s: &str) -> SocketAddrV4 {
        s.parse().expect("test address")
    }

    // ── The warn-only make-or-break tests (W2-3a (a)) ──────────────────────
    // These moved out of `client-gui/src/app/tests.rs` *before* the thread, so
    // the frozen raise/clear rules are pinned in the shared crate where the
    // decision lives. `switch_to_direct` stays `false`: the mutation anchor for
    // "the automatic direct fallback cannot come back" is unchanged.

    #[test]
    fn a_warning_clears_only_after_a_healthy_window() {
        // The warn-only replacement for the old retry-chain close rule. A
        // warning stays up until a window proves the return path works again;
        // the first fault window of a new streak is not that proof. Closing the
        // warning there was the 2026-11-20 flap: "the relay is returning traffic
        // again" after a window that returned nothing.
        let server = addr("198.51.100.23:36703");
        let relay = addr("192.0.2.10:4434");
        let warning = StarvationWarning {
            server,
            relay: Some(relay),
            fault: ReturnPathFault::NoReplyFromRelay,
        };
        let active = Some(&warning);
        let fault = ReturnPathFault::NoReplyFromRelay;
        let first = plan_return_path_window(active, None, Some(fault), server, Some(relay));
        assert_eq!(
            first.event,
            WindowEvent::None,
            "a first fault window is not recovery"
        );
        assert_eq!(
            first.warning,
            Some(warning),
            "a fault window keeps the warning up"
        );
        let healthy = plan_return_path_window(active, None, None, server, Some(relay));
        assert_eq!(healthy.event, WindowEvent::Recovered);
        assert_eq!(healthy.warning, None, "a working return path clears it");
        assert!(!healthy.switch_to_direct);
    }

    #[test]
    fn a_zero_return_window_warns_only_after_the_streak() {
        let zero = relay_reply_fault(RELAY_REPLY_TIMEOUT, 68, 0, 0, None);
        assert_eq!(zero, Some(ReturnPathFault::NoReplyFromRelay));
        let mut streak = ReplyFaultStreak::default();
        assert_eq!(
            streak.observe(zero),
            None,
            "one 8 s window of no return path is not evidence enough"
        );
        assert_eq!(
            streak.observe(zero),
            Some(ReturnPathFault::NoReplyFromRelay),
            "the configured streak of consecutive zero-return windows"
        );
        // A healthy window in between proves the absence was not sustained.
        let mut streak = ReplyFaultStreak::default();
        assert_eq!(streak.observe(zero), None);
        assert_eq!(streak.observe(None), None);
        assert_eq!(
            streak.observe(zero),
            None,
            "the streak restarted, so this is the first window again"
        );
        assert_eq!(
            streak.observe(zero),
            Some(ReturnPathFault::NoReplyFromRelay)
        );
    }

    #[test]
    fn a_warning_belongs_to_its_server_and_relay() {
        let server = addr("198.51.100.23:36703");
        let relay = addr("192.0.2.10:4434");
        let warning = StarvationWarning {
            server,
            relay: Some(relay),
            fault: ReturnPathFault::NoReplyFromRelay,
        };
        assert!(
            warning_is_about(&warning, server, Some(relay)),
            "the pair the watchdog judged is the pair the warning is about"
        );
        assert!(
            !warning_is_about(&warning, server, Some(addr("192.0.2.11:4434"))),
            "a new relay is a new session; the old finding says nothing about it"
        );
        assert!(
            !warning_is_about(&warning, addr("198.51.100.7:36704"), Some(relay)),
            "a new game server is a new session"
        );
        assert!(
            warning_is_about(&warning, server, None),
            "an unknown relay cannot prove the warning is stale"
        );
    }

    #[test]
    fn a_confirmed_starvation_window_warns_but_never_switches_to_direct() {
        // The owner-approved warn-only mode. Two consecutive 8 s judged windows
        // with enough traffic and zero returned datagrams are the existing
        // starvation condition; the plan must raise the warning and publish the
        // demotion finding, and must never ask for the session to be taken
        // direct. This is the mutation anchor: re-enabling the automatic switch
        // (setting `switch_to_direct` on the confirmed branch) fails this test.
        let server = addr("198.51.100.23:36703");
        let relay = addr("192.0.2.10:4434");
        let mut streak = ReplyFaultStreak::default();
        let window = relay_reply_fault(RELAY_REPLY_TIMEOUT, 60, 0, 0, None);
        assert_eq!(window, Some(ReturnPathFault::NoReplyFromRelay));
        assert_eq!(streak.observe(window), None, "one window is a sample");
        let confirmed = streak
            .observe(window)
            .expect("the second consecutive window confirms starvation");
        let plan = plan_return_path_window(None, Some(confirmed), window, server, Some(relay));
        let warning = plan
            .warning
            .expect("a confirmed starvation window raises the warning");
        assert_eq!(warning.server, server);
        assert_eq!(warning.relay, Some(relay));
        assert_eq!(warning.fault, ReturnPathFault::NoReplyFromRelay);
        assert_eq!(plan.event, WindowEvent::Warned);
        assert!(
            plan.publish_verdict,
            "the finding still reaches the relay-history demotion path"
        );
        assert!(
            !plan.switch_to_direct,
            "warn-only: the watchdog must never take the route from the player"
        );
        assert_eq!(
            warning_message_key(warning.fault),
            "route.relay_not_returning",
            "the route-card banner blames the relay only when the relay is at fault"
        );
    }

    #[test]
    fn the_starvation_verdict_still_demotes_the_relay_for_future_ranking() {
        // The load-bearing half of the warn-only mode: the route never changes,
        // but the finding must still be written into the relay history so the
        // next ranking does not re-select the relay that just starved the
        // session.
        use std::net::Ipv4Addr;
        let server = Ipv4Addr::new(198, 51, 100, 23);
        let relay = addr("192.0.2.10:4434");
        let mut history = RelayHistory::default();
        apply_starvation_verdict(&mut history, SocketAddrV4::new(server, 36703), relay);
        assert_eq!(
            history.carry_verdict(relay, server),
            crate::app::history::CarryVerdict::Starved,
            "the next ranking must demote a relay that did not return the traffic"
        );
        assert!(
            !history.is_starved(relay, server),
            "the finding is the starvation signature, not the retired exhausted-chain verdict"
        );
    }

    // ── R16: the streak is published for real ────────────────────────────

    #[test]
    fn the_published_streak_is_the_counted_streak_and_clears_with_the_warning() {
        // The watchdog thread publishes what it actually counted. A shell can
        // never read a fabricated streak: the value walks 1 -> 2 -> None with
        // the judged windows, and never exists without a warning episode.
        let server = addr("198.51.100.23:36703");
        let relay = addr("192.0.2.10:4434");
        let window = relay_reply_fault(RELAY_REPLY_TIMEOUT, 60, 0, 0, None);
        let mut streak = ReplyFaultStreak::default();
        let mut snapshot = WatchdogSnapshot::default();
        assert_eq!(snapshot.streak, None, "nothing judged yet");

        // Window 1: a fault, not confirmed. The streak is real evidence even
        // though no warning exists yet; it is published as the actual count.
        let confirmed = streak.observe(window);
        let plan = plan_return_path_window(None, confirmed, window, server, Some(relay));
        assert_eq!(plan.event, WindowEvent::None);
        apply_plan(&mut snapshot, &plan, streak.count, server, Some(relay));
        assert_eq!(snapshot.warning, None);
        assert_eq!(snapshot.streak, Some(1), "one judged fault window");
        assert_eq!(snapshot.verdict, None, "no demotion before confirmation");

        // Window 2: same fault again -> confirmed, warning raised, streak 2.
        let confirmed = streak.observe(window);
        let plan = plan_return_path_window(
            snapshot.warning.as_ref(),
            confirmed,
            window,
            server,
            Some(relay),
        );
        assert_eq!(plan.event, WindowEvent::Warned);
        assert!(!plan.switch_to_direct, "warn-only");
        apply_plan(&mut snapshot, &plan, streak.count, server, Some(relay));
        assert_eq!(snapshot.streak, Some(2), "two consecutive judged windows");
        assert!(snapshot.warning.is_some(), "the warning is raised");
        assert!(
            snapshot.verdict.is_some(),
            "the confirmed no-reply finding is published once for the demotion"
        );
        let warning = snapshot.warning.expect("warning");
        assert_eq!(warning.fault, ReturnPathFault::NoReplyFromRelay);

        // Window 3: healthy -> warning clears and the streak is gone (None, not
        // a stale 2 and not a fabricated 0).
        let confirmed = streak.observe(None);
        let plan = plan_return_path_window(
            snapshot.warning.as_ref(),
            confirmed,
            None,
            server,
            Some(relay),
        );
        assert_eq!(plan.event, WindowEvent::Recovered);
        apply_plan(&mut snapshot, &plan, streak.count, server, Some(relay));
        assert_eq!(snapshot.warning, None);
        assert_eq!(snapshot.streak, None, "no streak after a healthy window");
    }

    #[test]
    fn a_different_fault_restarts_the_published_streak() {
        let server = addr("198.51.100.23:36703");
        let mut streak = ReplyFaultStreak::default();
        let mut snapshot = WatchdogSnapshot::default();
        let no_reply = relay_reply_fault(RELAY_REPLY_TIMEOUT, 60, 0, 0, None);
        let injector = relay_reply_fault(RELAY_REPLY_TIMEOUT, 600, 0, 40, None);
        let confirmed = streak.observe(no_reply);
        let plan = plan_return_path_window(None, confirmed, no_reply, server, None);
        apply_plan(&mut snapshot, &plan, streak.count, server, None);
        assert_eq!(snapshot.streak, Some(1));
        let confirmed = streak.observe(injector);
        assert_eq!(confirmed, None, "a different fault restarts the count");
        let plan = plan_return_path_window(None, confirmed, injector, server, None);
        apply_plan(&mut snapshot, &plan, streak.count, server, None);
        assert_eq!(
            snapshot.streak,
            Some(1),
            "the published count restarted too"
        );
    }

    // ── Relay-health judgement (S4: tests moved from the GUI's relay_choice) ──

    #[test]
    fn a_gate_off_is_the_only_safe_moment_for_a_relay_switch() {
        // The tunnel reads the current relay on every packet. Pointing it at a
        // relay the new session is not registered with yet would send a live
        // match's datagrams into a void, so a relay change has to be staged
        // until the handover is finished. What makes it safe is the state the
        // hold already sets: tunnelling withheld.
        let live_match = Some(true);
        let held = Some(false);
        let gate_absent = None;
        assert!(
            !switch_is_safe(live_match),
            "a relay switch during a live match must be staged, not applied"
        );
        assert!(
            switch_is_safe(held),
            "with traffic held, the switch is what the tunnel sees"
        );
        assert!(
            switch_is_safe(gate_absent),
            "a backend with no gate is never relaying, so nothing is at risk"
        );
    }

    #[test]
    fn relay_health_reads_movement_not_totals() {
        // A run that relayed a healthy million packets and then started failing
        // must be reported as failing: the verdict is on the window's movement.
        assert_eq!(
            relay_health_verdict(1_000_000, 1_000_000, 1_000_010, 1_000_002),
            RelayHealthVerdict::Unsent {
                unsent: 8,
                decided: 10
            },
            "80% of this window never left, despite the healthy history"
        );
        assert_eq!(
            relay_health_verdict(0, 0, 100, 100),
            RelayHealthVerdict::Healthy
        );
        assert_eq!(
            relay_health_verdict(0, 0, 100, 0),
            RelayHealthVerdict::Unsent {
                unsent: 100,
                decided: 100
            },
            "nothing leaving at all is the loudest case, not an idle one"
        );
    }

    #[test]
    fn relay_health_tolerates_loss_and_a_still_flow() {
        // UDP games lose the odd datagram; one in ten is within the tolerance.
        assert_eq!(
            relay_health_verdict(0, 0, 100, 90),
            RelayHealthVerdict::Healthy
        );
        // No new packets decided: menus, or a hold. This must not read as a fault.
        assert_eq!(
            relay_health_verdict(500, 500, 500, 500),
            RelayHealthVerdict::Idle
        );
        // A sent count running ahead of the decided count cannot happen, and
        // must not underflow into a wild "unsent" number if it ever does.
        assert_eq!(
            relay_health_verdict(0, 50, 10, 60),
            RelayHealthVerdict::Healthy
        );
    }

    #[test]
    fn clearing_the_publication_drops_the_warning_and_streak_but_keeps_a_pending_verdict() {
        let server = addr("198.51.100.23:36703");
        let mut snapshot = WatchdogSnapshot {
            warning: Some(StarvationWarning {
                server,
                relay: None,
                fault: ReturnPathFault::NoReplyFromRelay,
            }),
            streak: Some(2),
            verdict: Some((server, None)),
        };
        let publication = Arc::new(Mutex::new(WatchdogSnapshot::default()));
        {
            let mut published = lock_or_recover(&publication);
            *published = snapshot;
        }
        clear_publication(&publication);
        snapshot = *lock_or_recover(&publication);
        assert_eq!(snapshot.warning, None);
        assert_eq!(snapshot.streak, None);
        assert!(
            snapshot.verdict.is_some(),
            "the one-shot demotion finding is not lost to a clear"
        );
    }
}
