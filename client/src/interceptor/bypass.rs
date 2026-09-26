//! Pure, I/O-free "do no harm" bypass gate.
//!
//! The client picks the relay nearest to **itself** and then tunnels for the
//! life of the locked game server. For domestic sessions that relay is often a
//! continent away from the server, so `client -> relay -> server` is longer
//! than `client -> server`. This gate decides, per game server, whether the
//! chosen relay actually helps; when it does not, the caller should let the
//! game connect directly.
//!
//! ## Safety comes first
//!
//! A wrong decision must never be worse than a slightly suboptimal relay. The
//! gate therefore:
//!
//! - **Fails open to the relay** on any ambiguity (missing samples, unhealthy
//!   relay, config parse failure). Relay is the status quo; an unproven session
//!   is never moved off it by a malfunctioning gate.
//! - Has a **hard per-server flip cap** ([`MAX_FLIPS_PER_SERVER`]) and a dwell
//!   time, so it cannot thrash mid-match.
//! - Never asks a diverting backend (Linux nftables, macOS pfctl) to tear down
//!   a live flow: deleting a redirect rule does not unhook conntrack, and a
//!   teardown would blackhole the game. `Direct` on those backends means *do
//!   not install the rule*, decided before capture begins.
//! - Defaults to [`BypassMode::DryRun`]: it measures and logs the decision it
//!   would make, and changes nothing. See the module docs in `config.rs` and
//!   the rollback section in `docs/`.
//!
//! This module mirrors `order.rs` and `rotation.rs`: no I/O, `now` passed in,
//! fully unit-testable without root.

use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::sync::atomic::Ordering;
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

use super::traits::InterceptorCounters;

// ─────────────────────────────────────────────────────────────────────────────
//  Tunables
// ─────────────────────────────────────────────────────────────────────────────

/// A first hop that already costs as much as the whole direct path cannot win.
/// `client_relay >= direct_icmp - PRECHECK_MARGIN_MS` means Direct. Zero is the
/// conservative default; a positive margin is even more conservative.
pub const PRECHECK_MARGIN_MS: f32 = 0.0;

/// `direct - relayed` at or above this keeps the relay.
pub const RELAY_KEEP_ADVANTAGE_MS: f32 = 5.0;

/// `direct - relayed` at or below this (the relay is at least this much worse)
/// is the evidence needed to move to Direct. Asymmetric on purpose: switching
/// to Direct is the risky direction, so it needs stronger evidence than
/// staying on the relay.
pub const DIRECT_DECIDE_ADVANTAGE_MS: f32 = -8.0;

/// Relay jitter must be at most this fraction of direct jitter to count as a
/// "materially smoother" relay.
pub const JITTER_KEEP_RATIO: f32 = 0.6;

/// Absolute jitter difference (ms) that also counts as materially smoother.
pub const JITTER_KEEP_MARGIN_MS: f32 = 5.0;

/// The jitter/loss override tolerates the relay being up to this much slower.
pub const JITTER_TOLERANCE_MS: f32 = 10.0;

/// Direct loss at or above this, with the relay delivering better, keeps FEC.
pub const LOSS_KEEP_RATIO: f32 = 0.02;

/// EMA smoothing factor for `advantage_ms`.
pub const EMA_ALPHA: f32 = 0.3;

/// Minimum time since a server was first seen before a probation verdict.
pub const PROBATION_MIN_DURATION: Duration = Duration::from_secs(20);

/// Minimum shadow-direct samples before a like-for-like comparison.
pub const MIN_SHADOW_SAMPLES: u32 = 3;

/// Minimum relayed samples before a stable p50.
pub const MIN_RELAYED_SAMPLES: u32 = 32;

/// After any decision, do not re-evaluate for this long.
pub const DWELL: Duration = Duration::from_secs(120);

/// Hard cap on Relay<->Direct transitions per server per process. After this
/// the state is latched and never changes again for the session.
pub const MAX_FLIPS_PER_SERVER: u32 = 2;

/// Re-evaluate at most once per this interval.
pub const EVAL_INTERVAL: Duration = Duration::from_secs(10);

/// Consecutive evaluation windows that must agree before Relay -> Direct.
pub const DIRECT_CONFIRMATIONS: u8 = 2;

// ─────────────────────────────────────────────────────────────────────────────
//  Config
// ─────────────────────────────────────────────────────────────────────────────

/// How the bypass gate is allowed to act.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BypassMode {
    /// Measure and decide; act only when the evidence is strong and safe.
    Auto,
    /// Force the relay. Complete rollback; no code change needed.
    Never,
    /// Force Direct. Testing aid; not a normal user setting.
    Always,
    /// Measure, log, and count, but always keep the relay. **Default.**
    #[default]
    DryRun,
}

impl BypassMode {
    /// Parse a user-facing mode string.
    pub fn parse(s: &str) -> Option<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "auto" => Some(Self::Auto),
            "never" => Some(Self::Never),
            "always" => Some(Self::Always),
            "dry_run" | "dry-run" => Some(Self::DryRun),
            _ => None,
        }
    }

    /// Stable string form used in config, logs, and status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Never => "never",
            Self::Always => "always",
            Self::DryRun => "dry_run",
        }
    }
}

/// Runtime tuning for the gate. The defaults are the spec's starting points.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BypassConfig {
    /// Acting mode.
    pub mode: BypassMode,
    /// Relay-worse threshold (ms) to prefer Direct.
    pub margin_ms: f32,
    /// Relay-better threshold (ms) to keep the relay.
    pub keep_ms: f32,
    /// Minimum time since first seen before a verdict.
    pub probation: Duration,
    /// Quiet period after a decision.
    pub dwell: Duration,
    /// Minimum interval between evaluations.
    pub eval_interval: Duration,
    /// Hard per-server flip cap.
    pub max_flips: u32,
}

impl Default for BypassConfig {
    fn default() -> Self {
        Self {
            mode: BypassMode::DryRun,
            margin_ms: -DIRECT_DECIDE_ADVANTAGE_MS,
            keep_ms: RELAY_KEEP_ADVANTAGE_MS,
            probation: PROBATION_MIN_DURATION,
            dwell: DWELL,
            eval_interval: EVAL_INTERVAL,
            max_flips: MAX_FLIPS_PER_SERVER,
        }
    }
}

impl BypassConfig {
    /// Resolve the effective config from CLI overrides and the `[route]`
    /// values. CLI flags win; an unparseable config mode falls back to the safe
    /// `dry_run` rather than `auto`.
    pub fn resolve(
        no_bypass: bool,
        force_direct: bool,
        dry_run: bool,
        config_mode: &str,
        margin_ms: f32,
        keep_ms: f32,
        dwell_s: u64,
        probation_s: u64,
    ) -> Self {
        let mode = if no_bypass {
            BypassMode::Never
        } else if force_direct {
            BypassMode::Always
        } else if dry_run {
            BypassMode::DryRun
        } else {
            BypassMode::parse(config_mode).unwrap_or_default()
        };
        Self {
            mode,
            margin_ms,
            keep_ms,
            dwell: Duration::from_secs(dwell_s),
            probation: Duration::from_secs(probation_s),
            ..Self::default()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Decision types
// ─────────────────────────────────────────────────────────────────────────────

/// The action the caller should take.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BypassDecision {
    /// Keep tunnelling through the relay (the status quo, and every fail-open).
    Relay,
    /// Let the game connect directly; do not tunnel this server.
    Direct,
    /// Not enough evidence; make no change.
    Hold,
}

/// Whether the action layer may enact a computed pre-gate `Direct`.
///
/// This is the single place the "`dry_run` changes nothing" contract is
/// enforced for the pre-install path, so every backend that gates an install
/// calls it rather than re-deriving the rule. Only `auto` and `always` enact;
/// a non-`Direct` computation is never enacted.
pub fn enact_pre_gate(mode: BypassMode, computed: BypassDecision) -> bool {
    computed == BypassDecision::Direct && matches!(mode, BypassMode::Auto | BypassMode::Always)
}

/// Whether the action layer may enact a computed steady-state `Direct`.
///
/// A steady-state `Direct` would mean taking a live, diverting flow off the
/// tunnel. No backend can do that safely mid-session today: deleting a redirect
/// does not unhook conntrack (Linux/macOS), and the WinDivert filter is not
/// per-server removable. This stays `false` until a backend implements a safe
/// boundary action, so a computed `Direct` is measured and logged but never
/// tears down live traffic.
pub fn enact_steady_state(_mode: BypassMode, _computed: BypassDecision) -> bool {
    false
}

/// Per-server state machine position.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BypassState {
    /// Never observed before.
    Unseen,
    /// Observed, but no rule is installed and no verdict exists yet.
    Probing,
    /// Relay is carrying traffic; both metrics are accruing.
    RelayProbation,
    /// Relay proven helpful; steady-state slow re-check.
    RelayKeep,
    /// Direct proven better; the action layer honours this at a safe boundary.
    DirectPreferred,
}

impl BypassState {
    /// Stable string form for logs and status.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unseen => "unseen",
            Self::Probing => "probing",
            Self::RelayProbation => "relay_probation",
            Self::RelayKeep => "relay_keep",
            Self::DirectPreferred => "direct_preferred",
        }
    }
}

/// Everything the gate needs for one evaluation. All measurement fields are
/// `Option` so a missing sample fails open rather than fabricating a verdict.
#[derive(Debug, Clone, Copy)]
pub struct BypassInputs {
    /// Monotonic time.
    pub now: Instant,
    /// The locked game server.
    pub server: SocketAddrV4,
    /// Shadow-direct application p50 (like-for-like with `relay_app_p50_ms`).
    pub direct_app_p50_ms: Option<f32>,
    /// Relayed application p50.
    pub relay_app_p50_ms: Option<f32>,
    /// Shadow-direct jitter.
    pub direct_app_jitter_ms: Option<f32>,
    /// Relayed jitter.
    pub relay_app_jitter_ms: Option<f32>,
    /// ICMP direct median, fallback for the pre-gate only.
    pub direct_icmp_p50_ms: Option<f32>,
    /// Client -> relay RTT (one keepalive probe).
    pub client_relay_p50_ms: Option<f32>,
    /// Direct application loss ratio, `1 - replies / attempts`.
    pub direct_loss_ratio: Option<f32>,
    /// Relay unrecovered loss ratio (FEC does not hide this from the game).
    pub relay_unrecovered_loss_ratio: Option<f32>,
    /// Shadow-direct samples in the window.
    pub shadow_samples: u32,
    /// Relayed samples in the window.
    pub relay_samples: u32,
    /// Whether the relay leg looks healthy (packets actually flowing).
    pub relay_healthy: bool,
    /// Whether FEC is enabled for this session.
    pub fec_enabled: bool,
    /// Active configuration.
    pub config: BypassConfig,
}

/// The gate's answer for one evaluation.
#[derive(Debug, Clone, Copy)]
pub struct BypassOutcome {
    /// The effective action, already downgraded to `Relay` in `dry_run`.
    pub decision: BypassDecision,
    /// What the gate would have done without the `dry_run` downgrade.
    pub computed: BypassDecision,
    /// The per-server state after this evaluation.
    pub state: BypassState,
    /// Short, human-readable reason (stable for logs and tests).
    pub reason: &'static str,
    /// Smoothed `direct - relayed` advantage, when computable.
    pub advantage_ms: Option<f32>,
    /// True when the `dry_run` downgrade was applied.
    pub dry_run: bool,
    /// True when this evaluation changed the state.
    pub flipped: bool,
    /// True when this call reached the decision logic, past sample, probation,
    /// interval, and dwell gating. A gated call is an evaluation, not a
    /// decision.
    pub evaluated: bool,
    /// Total flips recorded for this server.
    pub total_flips: u32,
}

#[derive(Debug)]
struct ServerState {
    state: BypassState,
    first_seen: Instant,
    last_eval: Option<Instant>,
    last_change: Option<Instant>,
    advantage_ema: Option<f32>,
    direct_votes: u8,
    flips: u32,
}

impl ServerState {
    fn new(now: Instant) -> Self {
        Self {
            state: BypassState::Probing,
            first_seen: now,
            last_eval: None,
            last_change: None,
            advantage_ema: None,
            direct_votes: 0,
            flips: 0,
        }
    }
}

/// Per-server, pure decision tracker.
#[derive(Debug)]
pub struct BypassTracker {
    config: BypassConfig,
    servers: HashMap<SocketAddrV4, ServerState>,
}

impl BypassTracker {
    /// Create a tracker with the given config.
    pub fn new(config: BypassConfig) -> Self {
        Self {
            config,
            servers: HashMap::new(),
        }
    }

    /// The active config.
    pub fn config(&self) -> BypassConfig {
        self.config
    }

    /// Tier-1 pre-gate: a sufficient *direct* proof that needs no capture.
    ///
    /// If the client->relay first hop alone already costs as much as the whole
    /// direct path, the tunnel cannot win (`relay->server >= 0`). This is the
    /// only check that is safe before a NAT rule exists, and it can never
    /// wrongly bypass a helpful relay. Absent data fails open to `Relay`.
    pub fn pre_gate(&mut self, inp: &BypassInputs) -> BypassDecision {
        if inp.config.mode == BypassMode::Never {
            return BypassDecision::Relay;
        }
        if inp.config.mode == BypassMode::Always {
            return BypassDecision::Direct;
        }
        match (inp.client_relay_p50_ms, inp.direct_icmp_p50_ms) {
            (Some(client_relay), Some(direct_icmp))
                if client_relay >= direct_icmp - PRECHECK_MARGIN_MS =>
            {
                if let Some(state) = self.servers.get_mut(&inp.server) {
                    state.state = BypassState::DirectPreferred;
                }
                BypassDecision::Direct
            }
            _ => BypassDecision::Relay,
        }
    }

    /// Full evaluation, including probation and the steady-state re-check.
    pub fn evaluate(&mut self, inp: &BypassInputs) -> BypassOutcome {
        let state = self
            .servers
            .entry(inp.server)
            .or_insert_with(|| ServerState::new(inp.now));

        // Forced modes short-circuit before any measurement is needed.
        match inp.config.mode {
            BypassMode::Never => {
                state.state = BypassState::RelayKeep;
                return outcome(
                    BypassDecision::Relay,
                    state,
                    "bypass disabled",
                    None,
                    false,
                    false,
                    false,
                );
            }
            BypassMode::Always => {
                state.state = BypassState::DirectPreferred;
                return outcome(
                    BypassDecision::Direct,
                    state,
                    "forced direct",
                    None,
                    false,
                    false,
                    false,
                );
            }
            BypassMode::Auto | BypassMode::DryRun => {}
        }

        let dry_run = inp.config.mode == BypassMode::DryRun;

        if !inp.relay_healthy {
            state.state = BypassState::Probing;
            return finish(
                dry_run,
                BypassDecision::Relay,
                "relay unhealthy",
                None,
                state,
                false,
                false,
            );
        }

        let Some(relay) = inp.relay_app_p50_ms else {
            state.state = BypassState::Probing;
            return finish(
                dry_run,
                BypassDecision::Relay,
                "no relayed samples",
                None,
                state,
                false,
                false,
            );
        };
        let Some(direct) = inp.direct_app_p50_ms else {
            state.state = BypassState::RelayProbation;
            return finish(
                dry_run,
                BypassDecision::Relay,
                "no shadow-direct samples",
                None,
                state,
                false,
                false,
            );
        };

        if inp.shadow_samples < MIN_SHADOW_SAMPLES || inp.relay_samples < MIN_RELAYED_SAMPLES {
            state.state = BypassState::RelayProbation;
            return finish(
                dry_run,
                BypassDecision::Relay,
                "insufficient samples",
                None,
                state,
                false,
                false,
            );
        }

        if inp.now.saturating_duration_since(state.first_seen) < inp.config.probation {
            state.state = BypassState::RelayProbation;
            return finish(
                dry_run,
                BypassDecision::Relay,
                "probation not elapsed",
                None,
                state,
                false,
                false,
            );
        }

        // Latched after the flip cap: never move again this session.
        if state.flips >= inp.config.max_flips {
            return finish(
                dry_run,
                current_action(state.state),
                "flip cap reached",
                state.advantage_ema,
                state,
                false,
                false,
            );
        }

        // At most one evaluation per interval; also honours the dwell time.
        if let Some(last) = state.last_eval {
            if inp.now.saturating_duration_since(last) < inp.config.eval_interval {
                return finish(
                    dry_run,
                    current_action(state.state),
                    "evaluation interval",
                    state.advantage_ema,
                    state,
                    false,
                    false,
                );
            }
        }
        if let Some(changed) = state.last_change {
            if inp.now.saturating_duration_since(changed) < inp.config.dwell {
                return finish(
                    dry_run,
                    current_action(state.state),
                    "dwell",
                    state.advantage_ema,
                    state,
                    false,
                    false,
                );
            }
        }
        state.last_eval = Some(inp.now);

        let advantage = direct - relay;
        let ema = match state.advantage_ema {
            Some(prev) => EMA_ALPHA * advantage + (1.0 - EMA_ALPHA) * prev,
            None => advantage,
        };
        state.advantage_ema = Some(ema);

        // Jitter override: a latency-neutral relay that is much smoother is
        // worth keeping (rubber-banding matters more than a few ms).
        let materially_smoother = match (inp.direct_app_jitter_ms, inp.relay_app_jitter_ms) {
            (Some(direct_jitter), Some(relay_jitter)) => {
                relay_jitter <= direct_jitter * JITTER_KEEP_RATIO
                    || direct_jitter - relay_jitter >= JITTER_KEEP_MARGIN_MS
            }
            _ => false,
        };
        if materially_smoother && ema >= -JITTER_TOLERANCE_MS {
            let flipped = transition(state, BypassState::RelayKeep, inp.now);
            return finish(
                dry_run,
                BypassDecision::Relay,
                "jitter override",
                Some(ema),
                state,
                flipped,
                true,
            );
        }

        // Loss override: with FEC on and the direct path lossy, the relay's
        // unrecovered loss is what the game feels. Keep it even on a wash.
        let loss_override = inp.fec_enabled
            && matches!(
                (inp.direct_loss_ratio, inp.relay_unrecovered_loss_ratio),
                (Some(direct_loss), Some(relay_loss))
                    if direct_loss >= LOSS_KEEP_RATIO && relay_loss < direct_loss
            );
        if loss_override {
            let flipped = transition(state, BypassState::RelayKeep, inp.now);
            return finish(
                dry_run,
                BypassDecision::Relay,
                "loss override",
                Some(ema),
                state,
                flipped,
                true,
            );
        }

        // Hysteresis on the smoothed advantage.
        if ema >= inp.config.keep_ms {
            let flipped = transition(state, BypassState::RelayKeep, inp.now);
            state.direct_votes = 0;
            return finish(
                dry_run,
                BypassDecision::Relay,
                "relay advantage",
                Some(ema),
                state,
                flipped,
                true,
            );
        }

        if ema <= -inp.config.margin_ms {
            if state.state == BypassState::DirectPreferred {
                // Already direct and still winning: hold.
                return finish(
                    dry_run,
                    BypassDecision::Direct,
                    "direct preferred",
                    Some(ema),
                    state,
                    false,
                    true,
                );
            }
            state.direct_votes = state.direct_votes.saturating_add(1);
            if state.direct_votes >= DIRECT_CONFIRMATIONS {
                state.direct_votes = 0;
                let flipped = transition(state, BypassState::DirectPreferred, inp.now);
                return finish(
                    dry_run,
                    BypassDecision::Direct,
                    "direct confirmed",
                    Some(ema),
                    state,
                    flipped,
                    true,
                );
            }
            // First agreeing window only; no state change yet.
            return finish(
                dry_run,
                BypassDecision::Relay,
                "direct window 1 of 2",
                Some(ema),
                state,
                false,
                true,
            );
        }

        // Grey zone: keep the relay. Going back to the relay is always safe,
        // so it needs only one window, and it is still a tracked transition.
        state.direct_votes = 0;
        let flipped = transition(state, BypassState::RelayKeep, inp.now);
        finish(
            dry_run,
            BypassDecision::Relay,
            "grey zone",
            Some(ema),
            state,
            flipped,
            true,
        )
    }
}

/// The action that matches the current state, used while holding.
fn current_action(state: BypassState) -> BypassDecision {
    match state {
        BypassState::DirectPreferred => BypassDecision::Direct,
        _ => BypassDecision::Relay,
    }
}

/// Move a server to `target`, counting a flip and starting the dwell window
/// only when the state actually changes. Returns whether it changed.
fn transition(state: &mut ServerState, target: BypassState, now: Instant) -> bool {
    if state.state == target {
        return false;
    }
    state.state = target;
    state.flips = state.flips.saturating_add(1);
    state.last_change = Some(now);
    true
}

fn finish(
    dry_run: bool,
    computed: BypassDecision,
    reason: &'static str,
    advantage_ms: Option<f32>,
    server: &mut ServerState,
    flipped: bool,
    evaluated: bool,
) -> BypassOutcome {
    outcome(
        computed,
        server,
        reason,
        advantage_ms,
        dry_run,
        flipped,
        evaluated,
    )
}

fn outcome(
    computed: BypassDecision,
    server: &ServerState,
    reason: &'static str,
    advantage_ms: Option<f32>,
    dry_run: bool,
    flipped: bool,
    evaluated: bool,
) -> BypassOutcome {
    let decision = if dry_run {
        BypassDecision::Relay
    } else {
        computed
    };
    BypassOutcome {
        decision,
        computed,
        state: server.state,
        reason,
        advantage_ms,
        dry_run,
        flipped,
        evaluated,
        total_flips: server.flips,
    }
}

/// Process-wide resolved bypass config, set once from CLI + config before any
/// interceptor starts. Read by [`build_config_for_game`] so every backend
/// receives the same resolved settings without threading a parameter through
/// each start path. Idempotent: the first call wins.
///
/// [`build_config_for_game`]: super::build_config_for_game
static DEFAULT_CONFIG: OnceLock<BypassConfig> = OnceLock::new();

/// Publish the resolved bypass config for interceptor starts. Idempotent.
pub fn set_default_config(config: BypassConfig) {
    let _ = DEFAULT_CONFIG.set(config);
}

/// The resolved bypass config, or the safe `dry_run` default when unset.
pub fn default_config() -> BypassConfig {
    DEFAULT_CONFIG.get().copied().unwrap_or_default()
}

/// Runtime wrapper used by the interceptor backends.
///
/// It pulls the live like-for-like measurements from the process-wide
/// [`crate::latency`] tracker, feeds them to the pure [`BypassTracker`], writes
/// the bypass counters, and logs every decision. The client->relay RTT for the
/// tier-1 pre-gate is passed in by the caller: it is supplied by the keepalive
/// probe when available, and `None` simply leaves the pre-gate inert (it fails
/// open to the relay).
#[derive(Debug)]
pub struct BypassGate {
    tracker: BypassTracker,
    counters: Arc<InterceptorCounters>,
}

impl BypassGate {
    /// Build a gate for one interceptor session.
    pub fn new(config: BypassConfig, counters: Arc<InterceptorCounters>) -> Self {
        Self {
            tracker: BypassTracker::new(config),
            counters,
        }
    }

    /// The configured mode.
    pub fn mode(&self) -> BypassMode {
        self.tracker.config().mode
    }

    /// Whether the gate is explicitly disabled.
    pub fn is_disabled(&self) -> bool {
        self.mode() == BypassMode::Never
    }

    /// Tier-1 pre-gate. Supply the client->relay and direct ICMP medians when
    /// they are known; `None` fails open to the relay.
    pub fn pre_gate(
        &mut self,
        server: SocketAddrV4,
        now: Instant,
        client_relay_p50_ms: Option<f32>,
        direct_icmp_p50_ms: Option<f32>,
        fec_enabled: bool,
    ) -> BypassDecision {
        let inp = BypassInputs {
            now,
            server,
            direct_app_p50_ms: None,
            relay_app_p50_ms: None,
            direct_app_jitter_ms: None,
            relay_app_jitter_ms: None,
            direct_icmp_p50_ms,
            client_relay_p50_ms,
            direct_loss_ratio: None,
            relay_unrecovered_loss_ratio: None,
            shadow_samples: 0,
            relay_samples: 0,
            relay_healthy: true,
            fec_enabled,
            config: self.tracker.config(),
        };
        let decision = self.tracker.pre_gate(&inp);
        let supplied = matches!(self.mode(), BypassMode::Auto | BypassMode::DryRun)
            && client_relay_p50_ms.is_some();
        if supplied {
            self.counters
                .bypass_pre_gate_rtt
                .fetch_add(1, Ordering::Relaxed);
            tracing::debug!(
                server = %server,
                client_relay_ms = ?client_relay_p50_ms,
                direct_icmp_ms = ?direct_icmp_p50_ms,
                decision = ?decision,
                "bypass pre-gate evaluated with a live client-to-relay RTT"
            );
        }
        if decision == BypassDecision::Direct {
            tracing::warn!(
                server = %server,
                client_relay_ms = ?client_relay_p50_ms,
                direct_icmp_ms = ?direct_icmp_p50_ms,
                "bypass pre-gate: first hop alone costs as much as the whole direct path"
            );
        }
        decision
    }

    /// Evaluate one server from explicit inputs.
    ///
    /// The gate's own config is authoritative: `inp.config` is overwritten so a
    /// caller cannot bypass the resolved mode. Writes the bypass counters and
    /// logs every transition.
    pub fn evaluate_inputs(&mut self, inp: &BypassInputs) -> BypassOutcome {
        let mut inp = *inp;
        inp.config = self.tracker.config();
        let out = self.tracker.evaluate(&inp);

        self.counters
            .bypass_evaluations
            .fetch_add(1, Ordering::Relaxed);
        if out.evaluated {
            self.counters
                .bypass_decisions
                .fetch_add(1, Ordering::Relaxed);
        }
        match out.computed {
            BypassDecision::Relay => &self.counters.bypass_relay,
            BypassDecision::Direct => &self.counters.bypass_direct,
            BypassDecision::Hold => &self.counters.bypass_hold,
        }
        .fetch_add(1, Ordering::Relaxed);

        if out.flipped {
            self.counters.bypass_flips.fetch_add(1, Ordering::Relaxed);
            tracing::info!(
                server = %inp.server,
                state = out.state.as_str(),
                computed = ?out.computed,
                advantage_ms = ?out.advantage_ms,
                reason = out.reason,
                dry_run = out.dry_run,
                total_flips = out.total_flips,
                "bypass gate transition"
            );
        } else if out.computed == BypassDecision::Direct || out.dry_run {
            tracing::debug!(
                server = %inp.server,
                state = out.state.as_str(),
                computed = ?out.computed,
                advantage_ms = ?out.advantage_ms,
                reason = out.reason,
                dry_run = out.dry_run,
                "bypass gate decision"
            );
        }
        out
    }

    /// Full probation/steady-state evaluation from the live measurements.
    ///
    /// Uses only the same-server like-for-like window: when the shadow-direct
    /// and relayed samples do not describe this `server` (different server,
    /// stale, or empty), every application measurement is left `None` and the
    /// gate fails open to the relay. The ICMP direct median is never compared
    /// against the relayed path here.
    pub fn evaluate(
        &mut self,
        server: SocketAddrV4,
        now: Instant,
        relay_healthy: bool,
        fec_enabled: bool,
    ) -> BypassOutcome {
        let paired = crate::latency::like_for_like().filter(|p| p.server == *server.ip());
        let inp = BypassInputs {
            now,
            server,
            direct_app_p50_ms: paired.map(|p| p.direct_app_p50_ms),
            relay_app_p50_ms: paired.map(|p| p.relay_app_p50_ms),
            direct_app_jitter_ms: paired.and_then(|p| p.direct_app_jitter_ms),
            relay_app_jitter_ms: paired.and_then(|p| p.relay_app_jitter_ms),
            direct_icmp_p50_ms: None,
            client_relay_p50_ms: None,
            direct_loss_ratio: paired.and_then(|p| p.direct_loss_ratio),
            relay_unrecovered_loss_ratio: None,
            shadow_samples: paired.map_or(0, |p| p.shadow_samples),
            relay_samples: paired.map_or(0, |p| p.relay_samples),
            relay_healthy,
            fec_enabled,
            config: self.tracker.config(),
        };
        self.evaluate_inputs(&inp)
    }

    /// Count a pre-gate Direct that the caller either honoured or declined.
    pub fn report_pre_gate(&self, applied: bool) {
        if applied {
            self.counters.bypass_allowed.fetch_add(1, Ordering::Relaxed);
        } else {
            self.counters.bypass_refused.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record whether a computed Direct was actually applied by the caller.
    /// `applied == false` (dry run, an unsafe backend, or a worker that cannot
    /// bypass) counts as a refused bypass while the relay is kept.
    pub fn note_action(&self, outcome: &BypassOutcome, applied: bool) {
        if outcome.computed != BypassDecision::Direct {
            return;
        }
        if applied && outcome.decision == BypassDecision::Direct {
            self.counters.bypass_allowed.fetch_add(1, Ordering::Relaxed);
        } else {
            self.counters.bypass_refused.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// One steady-state gate step for a backend whose capture loop produces
/// shadow-direct samples.
///
/// This is the seam the Windows capture loop calls from its evaluation timer.
/// It counts every evaluation, applies the same-server like-for-like pairing,
/// and returns the outcome. It never acts: a steady-state `Direct` would mean
/// tearing down a live divert, and [`enact_steady_state`] is the single place
/// that contract is enforced.
pub fn steady_state_step(
    gate: &mut BypassGate,
    server: SocketAddrV4,
    now: Instant,
    relay_healthy: bool,
    fec_enabled: bool,
) -> BypassOutcome {
    gate.evaluate(server, now, relay_healthy, fec_enabled)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn server(n: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(203, 0, 113, n), 28015)
    }

    fn auto() -> BypassConfig {
        BypassConfig {
            mode: BypassMode::Auto,
            ..Default::default()
        }
    }

    fn at(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    fn inputs(now: Instant, server: SocketAddrV4, config: BypassConfig) -> BypassInputs {
        BypassInputs {
            now,
            server,
            direct_app_p50_ms: None,
            relay_app_p50_ms: None,
            direct_app_jitter_ms: None,
            relay_app_jitter_ms: None,
            direct_icmp_p50_ms: None,
            client_relay_p50_ms: None,
            direct_loss_ratio: None,
            relay_unrecovered_loss_ratio: None,
            shadow_samples: 0,
            relay_samples: 0,
            relay_healthy: true,
            fec_enabled: false,
            config,
        }
    }

    /// A complete, decision-ready sample: relay 20 ms faster, enough samples.
    fn ready(now: Instant, server: SocketAddrV4, config: BypassConfig) -> BypassInputs {
        let mut inp = inputs(now, server, config);
        inp.direct_app_p50_ms = Some(50.0);
        inp.relay_app_p50_ms = Some(30.0);
        inp.shadow_samples = MIN_SHADOW_SAMPLES;
        inp.relay_samples = MIN_RELAYED_SAMPLES;
        inp
    }

    // ── Pre-gate ────────────────────────────────────────────────────────────

    #[test]
    fn pre_gate_directs_when_first_hop_exceeds_direct_path() {
        let t0 = Instant::now();
        let mut tracker = BypassTracker::new(auto());
        let mut inp = inputs(t0, server(1), auto());
        inp.client_relay_p50_ms = Some(100.0);
        inp.direct_icmp_p50_ms = Some(50.0);
        assert_eq!(tracker.pre_gate(&inp), BypassDecision::Direct);
    }

    #[test]
    fn pre_gate_fails_open_without_client_relay_rtt() {
        let t0 = Instant::now();
        let mut tracker = BypassTracker::new(auto());
        let mut inp = inputs(t0, server(1), auto());
        inp.client_relay_p50_ms = None;
        inp.direct_icmp_p50_ms = Some(50.0);
        assert_eq!(tracker.pre_gate(&inp), BypassDecision::Relay);
    }

    #[test]
    fn pre_gate_keeps_a_helpful_relay() {
        let t0 = Instant::now();
        let mut tracker = BypassTracker::new(auto());
        let mut inp = inputs(t0, server(1), auto());
        inp.client_relay_p50_ms = Some(10.0);
        inp.direct_icmp_p50_ms = Some(80.0);
        assert_eq!(tracker.pre_gate(&inp), BypassDecision::Relay);
    }

    // ── Hysteresis ──────────────────────────────────────────────────────────

    #[test]
    fn hysteresis_keeps_relay_on_a_small_advantage() {
        let t0 = Instant::now();
        let s = server(2);
        let mut tracker = BypassTracker::new(auto());
        // Warm up probation.
        tracker.evaluate(&ready(t0, s, auto()));
        let mut inp = ready(at(t0, 21), s, auto());
        inp.direct_app_p50_ms = Some(41.0);
        inp.relay_app_p50_ms = Some(40.0);
        let out = tracker.evaluate(&inp);
        assert_eq!(out.decision, BypassDecision::Relay);
        assert_eq!(out.state, BypassState::RelayKeep);
    }

    #[test]
    fn hysteresis_requires_two_agreeing_windows_to_go_direct() {
        let t0 = Instant::now();
        let s = server(3);
        let mut tracker = BypassTracker::new(auto());
        tracker.evaluate(&ready(t0, s, auto()));

        let mut inp = ready(at(t0, 21), s, auto());
        inp.direct_app_p50_ms = Some(30.0);
        inp.relay_app_p50_ms = Some(50.0);
        let first = tracker.evaluate(&inp);
        assert_eq!(
            first.decision,
            BypassDecision::Relay,
            "one window must hold"
        );
        assert_eq!(first.state, BypassState::RelayProbation);

        let mut inp2 = inp;
        inp2.now = at(t0, 42);
        let second = tracker.evaluate(&inp2);
        assert_eq!(second.decision, BypassDecision::Direct);
        assert_eq!(second.state, BypassState::DirectPreferred);
    }

    #[test]
    fn dwell_prevents_an_immediate_flip_back() {
        let t0 = Instant::now();
        let s = server(4);
        let mut tracker = BypassTracker::new(auto());
        let mut warm = ready(t0, s, auto());
        warm.direct_app_p50_ms = Some(30.0);
        warm.relay_app_p50_ms = Some(50.0);
        tracker.evaluate(&warm); // probation anchor
        let mut d1 = warm;
        d1.now = at(t0, 21);
        tracker.evaluate(&d1);
        let mut d2 = warm;
        d2.now = at(t0, 42);
        assert_eq!(tracker.evaluate(&d2).decision, BypassDecision::Direct);

        // Relay now much better, but we are inside the dwell period.
        let mut back = ready(at(t0, 60), s, auto());
        back.direct_app_p50_ms = Some(80.0);
        back.relay_app_p50_ms = Some(20.0);
        assert_eq!(
            tracker.evaluate(&back).decision,
            BypassDecision::Direct,
            "dwell must block a flip back"
        );

        // Past the dwell, the relay is restored.
        let mut back2 = back;
        back2.now = at(t0, 200);
        assert_eq!(tracker.evaluate(&back2).decision, BypassDecision::Relay);
    }

    // ── Flip cap ────────────────────────────────────────────────────────────

    #[test]
    fn flip_cap_latches_after_two_flips() {
        let t0 = Instant::now();
        let s = server(5);
        let mut tracker = BypassTracker::new(auto());

        let mut direct = ready(t0, s, auto());
        direct.direct_app_p50_ms = Some(30.0);
        direct.relay_app_p50_ms = Some(50.0);
        tracker.evaluate(&direct);
        let mut d1 = direct;
        d1.now = at(t0, 21);
        tracker.evaluate(&d1);
        let mut d2 = direct;
        d2.now = at(t0, 42);
        let flip1 = tracker.evaluate(&d2);
        assert_eq!(flip1.decision, BypassDecision::Direct);
        assert_eq!(flip1.total_flips, 1);

        // Flip back once the dwell has passed.
        let mut relay = ready(at(t0, 200), s, auto());
        relay.direct_app_p50_ms = Some(90.0);
        relay.relay_app_p50_ms = Some(20.0);
        let flip2 = tracker.evaluate(&relay);
        assert_eq!(flip2.decision, BypassDecision::Relay);
        assert_eq!(flip2.total_flips, 2);

        // The cap is reached: a later strong direct signal must be ignored.
        let mut direct3 = direct;
        direct3.now = at(t0, 400);
        let latched = tracker.evaluate(&direct3);
        assert_eq!(latched.decision, BypassDecision::Relay);
        assert_eq!(latched.reason, "flip cap reached");
        assert_eq!(latched.total_flips, 2);
    }

    // ── Loss / jitter override ──────────────────────────────────────────────

    #[test]
    fn loss_override_keeps_a_latency_neutral_relay_with_fec() {
        let t0 = Instant::now();
        let s = server(6);
        let mut tracker = BypassTracker::new(auto());
        tracker.evaluate(&ready(t0, s, auto()));

        let mut inp = ready(at(t0, 21), s, auto());
        inp.fec_enabled = true;
        inp.direct_app_p50_ms = Some(50.0);
        inp.relay_app_p50_ms = Some(53.0); // relay 3 ms worse: within tolerance
        inp.direct_loss_ratio = Some(0.05);
        inp.relay_unrecovered_loss_ratio = Some(0.001);
        let out = tracker.evaluate(&inp);
        assert_eq!(out.reason, "loss override");
        assert_eq!(out.decision, BypassDecision::Relay);
        assert_eq!(out.state, BypassState::RelayKeep);
    }

    #[test]
    fn loss_override_does_not_fire_below_the_loss_floor() {
        let t0 = Instant::now();
        let s = server(7);
        let mut tracker = BypassTracker::new(auto());
        tracker.evaluate(&ready(t0, s, auto()));

        let mut inp = ready(at(t0, 21), s, auto());
        inp.fec_enabled = true;
        inp.direct_app_p50_ms = Some(30.0);
        inp.relay_app_p50_ms = Some(50.0);
        inp.direct_loss_ratio = Some(0.005); // below LOSS_KEEP_RATIO
        inp.relay_unrecovered_loss_ratio = Some(0.0);
        let out = tracker.evaluate(&inp);
        assert_ne!(out.reason, "loss override");
        assert_ne!(out.state, BypassState::RelayKeep);
    }

    #[test]
    fn jitter_override_keeps_a_smoother_relay() {
        let t0 = Instant::now();
        let s = server(8);
        let mut tracker = BypassTracker::new(auto());
        tracker.evaluate(&ready(t0, s, auto()));

        let mut inp = ready(at(t0, 21), s, auto());
        inp.direct_app_p50_ms = Some(50.0);
        inp.relay_app_p50_ms = Some(53.0);
        inp.direct_app_jitter_ms = Some(20.0);
        inp.relay_app_jitter_ms = Some(5.0);
        let out = tracker.evaluate(&inp);
        assert_eq!(out.reason, "jitter override");
        assert_eq!(out.decision, BypassDecision::Relay);
    }

    // ── Dry run ─────────────────────────────────────────────────────────────

    #[test]
    fn dry_run_computes_direct_but_acts_relay() {
        let t0 = Instant::now();
        let s = server(9);
        let cfg = BypassConfig {
            mode: BypassMode::DryRun,
            ..Default::default()
        };
        let mut tracker = BypassTracker::new(cfg);
        let mut direct = ready(t0, s, cfg);
        direct.direct_app_p50_ms = Some(30.0);
        direct.relay_app_p50_ms = Some(50.0);
        tracker.evaluate(&direct);
        let mut d1 = direct;
        d1.now = at(t0, 21);
        tracker.evaluate(&d1);
        let mut d2 = direct;
        d2.now = at(t0, 42);
        let out = tracker.evaluate(&d2);

        assert_eq!(out.computed, BypassDecision::Direct, "must measure Direct");
        assert_eq!(out.decision, BypassDecision::Relay, "must change nothing");
        assert!(out.dry_run);
    }

    // ── Forced modes / fail-open ────────────────────────────────────────────

    #[test]
    fn never_forces_relay() {
        let t0 = Instant::now();
        let cfg = BypassConfig {
            mode: BypassMode::Never,
            ..Default::default()
        };
        let mut tracker = BypassTracker::new(cfg);
        let out = tracker.evaluate(&ready(t0, server(10), cfg));
        assert_eq!(out.decision, BypassDecision::Relay);
        assert_eq!(out.state, BypassState::RelayKeep);
    }

    #[test]
    fn resolve_lets_cli_flags_win_and_fails_safe_on_garbage() {
        let never = BypassConfig::resolve(true, false, false, "auto", 8.0, 5.0, 120, 20);
        assert_eq!(never.mode, BypassMode::Never, "no-bypass wins over config");

        let force = BypassConfig::resolve(false, true, false, "never", 8.0, 5.0, 120, 20);
        assert_eq!(force.mode, BypassMode::Always);

        let dry = BypassConfig::resolve(false, false, true, "auto", 8.0, 5.0, 120, 20);
        assert_eq!(dry.mode, BypassMode::DryRun);

        let config = BypassConfig::resolve(false, false, false, "auto", 8.0, 5.0, 120, 20);
        assert_eq!(config.mode, BypassMode::Auto);

        let garbage = BypassConfig::resolve(false, false, false, "banana", 8.0, 5.0, 120, 20);
        assert_eq!(
            garbage.mode,
            BypassMode::DryRun,
            "an unparseable mode must fail safe, not auto"
        );
    }

    #[test]
    fn always_forces_direct() {
        let t0 = Instant::now();
        let cfg = BypassConfig {
            mode: BypassMode::Always,
            ..Default::default()
        };
        let mut tracker = BypassTracker::new(cfg);
        let out = tracker.evaluate(&inputs(t0, server(11), cfg));
        assert_eq!(out.decision, BypassDecision::Direct);
    }

    #[test]
    fn missing_relay_samples_fail_open() {
        let t0 = Instant::now();
        let s = server(12);
        let mut tracker = BypassTracker::new(auto());
        let mut inp = inputs(t0, s, auto());
        inp.direct_app_p50_ms = Some(10.0);
        inp.shadow_samples = MIN_SHADOW_SAMPLES;
        let out = tracker.evaluate(&inp);
        assert_eq!(out.decision, BypassDecision::Relay);
        assert_eq!(out.reason, "no relayed samples");
    }

    #[test]
    fn too_few_samples_hold_at_probation() {
        let t0 = Instant::now();
        let s = server(13);
        let mut tracker = BypassTracker::new(auto());
        let mut inp = ready(at(t0, 21), s, auto());
        inp.relay_samples = 10;
        let out = tracker.evaluate(&inp);
        assert_eq!(out.decision, BypassDecision::Relay);
        assert_eq!(out.state, BypassState::RelayProbation);
        assert_eq!(out.reason, "insufficient samples");
    }

    #[test]
    fn unhealthy_relay_fails_open() {
        let t0 = Instant::now();
        let s = server(14);
        let mut tracker = BypassTracker::new(auto());
        let mut inp = ready(at(t0, 21), s, auto());
        inp.relay_healthy = false;
        let out = tracker.evaluate(&inp);
        assert_eq!(out.decision, BypassDecision::Relay);
        assert_eq!(out.reason, "relay unhealthy");
    }

    #[test]
    fn gate_counts_every_allowed_and_refused_bypass() {
        let counters = Arc::new(InterceptorCounters::default());
        let gate = BypassGate::new(auto(), Arc::clone(&counters));

        gate.report_pre_gate(true);
        gate.report_pre_gate(false);

        let direct = BypassOutcome {
            decision: BypassDecision::Direct,
            computed: BypassDecision::Direct,
            state: BypassState::DirectPreferred,
            reason: "direct confirmed",
            advantage_ms: Some(-20.0),
            dry_run: false,
            flipped: true,
            evaluated: true,
            total_flips: 1,
        };
        gate.note_action(&direct, true);
        gate.note_action(
            &BypassOutcome {
                decision: BypassDecision::Relay,
                dry_run: true,
                ..direct
            },
            false,
        );
        gate.note_action(
            &BypassOutcome {
                computed: BypassDecision::Relay,
                decision: BypassDecision::Relay,
                ..direct
            },
            false,
        );

        assert_eq!(counters.bypass_allowed.load(Ordering::Relaxed), 2);
        assert_eq!(counters.bypass_refused.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn pre_gate_counts_only_a_supplied_client_relay_rtt() {
        let counters = Arc::new(InterceptorCounters::default());
        let mut gate = BypassGate::new(auto(), Arc::clone(&counters));

        gate.pre_gate(server(20), Instant::now(), None, Some(50.0), false);
        assert_eq!(
            counters.bypass_pre_gate_rtt.load(Ordering::Relaxed),
            0,
            "a missing first-hop RTT is not real input"
        );

        gate.pre_gate(server(20), Instant::now(), Some(10.0), Some(50.0), false);
        assert_eq!(
            counters.bypass_pre_gate_rtt.load(Ordering::Relaxed),
            1,
            "a supplied first-hop RTT counts as real input"
        );
    }

    #[test]
    fn servers_are_tracked_independently() {
        let t0 = Instant::now();
        let a = server(15);
        let b = server(16);
        let mut tracker = BypassTracker::new(auto());

        let mut direct = ready(t0, a, auto());
        direct.direct_app_p50_ms = Some(30.0);
        direct.relay_app_p50_ms = Some(50.0);
        tracker.evaluate(&direct);
        let mut d1 = direct;
        d1.now = at(t0, 21);
        tracker.evaluate(&d1);
        let mut d2 = direct;
        d2.now = at(t0, 42);
        assert_eq!(tracker.evaluate(&d2).state, BypassState::DirectPreferred);

        // A fresh server has its own, untouched state.
        let fresh = tracker.evaluate(&ready(at(t0, 42), b, auto()));
        assert_eq!(fresh.state, BypassState::RelayProbation);
    }

    #[test]
    fn enact_pre_gate_honours_dry_run_and_never_forces_relay() {
        assert!(
            enact_pre_gate(BypassMode::Auto, BypassDecision::Direct),
            "auto must act on a sufficient direct proof"
        );
        assert!(
            !enact_pre_gate(BypassMode::DryRun, BypassDecision::Direct),
            "dry_run must never change tunneling behavior"
        );
        assert!(!enact_pre_gate(BypassMode::Never, BypassDecision::Direct));
        assert!(!enact_pre_gate(BypassMode::Auto, BypassDecision::Relay));
    }

    // ── Steady-state wiring ─────────────────────────────────────────────────

    #[test]
    fn steady_state_step_invokes_evaluate_and_counts_it() {
        let counters = Arc::new(InterceptorCounters::default());
        let mut gate = BypassGate::new(auto(), Arc::clone(&counters));

        let first = steady_state_step(&mut gate, server(30), Instant::now(), true, false);
        let second = steady_state_step(&mut gate, server(30), Instant::now(), true, false);

        assert_eq!(
            counters.bypass_evaluations.load(Ordering::Relaxed),
            2,
            "every steady-state tick must invoke evaluate and increment the counter"
        );
        assert_eq!(
            counters.bypass_relay.load(Ordering::Relaxed),
            2,
            "a fail-open tick is still counted under the action it chose"
        );
        assert_eq!(
            counters.bypass_decisions.load(Ordering::Relaxed),
            0,
            "an unpairable window is an evaluation, not a decision"
        );
        assert_eq!(first.decision, BypassDecision::Relay);
        assert_eq!(second.decision, BypassDecision::Relay);
        assert!(
            !enact_steady_state(gate.mode(), first.computed),
            "the step must stay measurement-only on every backend"
        );
    }

    #[test]
    fn unpairable_measurements_fail_open_to_the_relay() {
        let counters = Arc::new(InterceptorCounters::default());
        let mut gate = BypassGate::new(auto(), Arc::clone(&counters));

        // ICMP and first-hop data exist, but no same-server shadow-direct pair.
        let mut inp = inputs(at(Instant::now(), 60), server(31), auto());
        inp.direct_icmp_p50_ms = Some(10.0);
        inp.client_relay_p50_ms = Some(5.0);

        let out = gate.evaluate_inputs(&inp);
        assert_eq!(out.decision, BypassDecision::Relay);
        assert!(
            !out.evaluated,
            "an unpairable window is not a decision and must not be acted on"
        );
    }

    #[test]
    fn dry_run_steady_state_computes_direct_but_never_enacts() {
        let counters = Arc::new(InterceptorCounters::default());
        let cfg = BypassConfig {
            mode: BypassMode::DryRun,
            ..Default::default()
        };
        let mut gate = BypassGate::new(cfg, Arc::clone(&counters));
        let t0 = Instant::now();
        let s = server(32);

        let mut warm = ready(t0, s, cfg);
        warm.direct_app_p50_ms = Some(30.0);
        warm.relay_app_p50_ms = Some(50.0);
        gate.evaluate_inputs(&warm);
        let mut d1 = warm;
        d1.now = at(t0, 21);
        gate.evaluate_inputs(&d1);
        let mut d2 = warm;
        d2.now = at(t0, 42);
        let out = gate.evaluate_inputs(&d2);

        assert_eq!(
            out.computed,
            BypassDecision::Direct,
            "the gate must compute Direct"
        );
        assert_eq!(
            out.decision,
            BypassDecision::Relay,
            "dry_run must change nothing"
        );
        assert!(out.dry_run);
        assert_eq!(counters.bypass_direct.load(Ordering::Relaxed), 1);
        assert_eq!(
            counters.bypass_decisions.load(Ordering::Relaxed),
            2,
            "only the two decision-bearing windows count, not the probation warm-up"
        );
        assert!(!enact_steady_state(BypassMode::DryRun, out.computed));
        assert!(
            !enact_steady_state(BypassMode::Auto, out.computed),
            "no backend has a safe live-teardown action yet"
        );
    }
}
