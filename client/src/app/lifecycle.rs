//! Lifecycle instrumentation v1 (WinDivert stability reset, P0-1).
//!
//! **Behaviour-neutral by contract.** This module records what the process did;
//! it never decides what the process does next. No retry policy, pin semantics,
//! command behaviour or user-facing copy reads these events. If the sink cannot
//! be opened every call is a no-op and the product behaves exactly as before.
//!
//! Why it exists (`docs/audit/stability-verification-2026-11-23.md` §5,
//! `wat/evidence/2026-11-23/stability-audit/03-instrumentation-and-open-questions.md`
//! §1–3): the 2026-11-23 field trace could not answer who stopped capture, which
//! binary produced a segment, whether a registration was superseded, or what the
//! UI showed. A prose trace cannot answer those; a stable, typed event stream can.
//!
//! Two sinks, one record:
//!
//! 1. `lifecycle.jsonl` — one JSON object per line, stable field names, no ANSI,
//!    no prose, written next to the trace log. This is the machine contract the
//!    P0-6 harness reads.
//! 2. `tracing` — one `event=<name> key=value …` line in the existing trace, so
//!    support can still read a human log.
//!
//! Privacy: events carry identifiers the product already logs (handle ids,
//! sessions, relay/server addresses, reason codes). They never carry packet
//! payloads, tokens, credentials or personal data; the typed event structs below
//! have no such fields by construction. The support bundle writes local evidence
//! files only.
//!
//! ## Named gaps in v1
//!
//! * **WFP enumeration is deferred.** The bundle records the app's own handle
//!   counters and `sc.exe query WinDivert`; it does not run `netsh wfp show
//!   state`. Adding that (bounded, read-only) is part of the P0-6 harness work,
//!   not this behaviour-neutral patch.
//! * **`windivert_version` is best-effort.** The shipped identity is the two
//!   file SHA-256 values; the numeric driver version would need PE version
//!   resources and is left `None` rather than guessed.
//! * **UI screenshot is not captured**; the `ui_state` event is the semantic
//!   dump v1 records.

use std::collections::VecDeque;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Map, Value};

/// The JSONL sink's file name inside the data directory.
pub const LIFECYCLE_FILE: &str = "lifecycle.jsonl";
/// The unclean-shutdown marker. Present at start = the previous run did not
/// reach a clean shutdown; removed only by [`shutdown`] with `clean = true`.
pub const UNCLEAN_MARKER: &str = "lifecycle.unclean";
/// How many recent events the support bundle carries.
pub const EVENT_RING_CAPACITY: usize = 256;
/// Minimum spacing between automatic support bundles.
pub const SUPPORT_BUNDLE_MIN_INTERVAL: Duration = Duration::from_secs(60);
/// Build identity: the release version this binary was compiled from.
pub const BUILD_VERSION: &str = env!("CARGO_PKG_VERSION");
/// Build identity: the commit the binary was compiled from. Set by
/// `client/build.rs`; `"unknown"` when the build had no git available.
pub const BUILD_COMMIT: &str = match option_env!("LIGHTSPEED_BUILD_COMMIT") {
    Some(commit) => commit,
    None => "unknown",
};

// ─────────────────────────────────────────────────────────────────────────────
//  Closed vocabularies
// ─────────────────────────────────────────────────────────────────────────────
//
// These are the *only* spellings a harness may see. A new value is a schema
// change, not a free-form string: that is what makes the P0-6 assertions
// (T10/T13/T15) mechanically checkable instead of prose-parsed.

/// Who caused a transition.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum Actor {
    User,
    Auto,
    Retry,
    #[default]
    Internal,
    System,
}

impl Actor {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Auto => "auto",
            Self::Retry => "retry",
            Self::Internal => "internal",
            Self::System => "system",
        }
    }
}

/// Why interception started or stopped. Closed set per the instrumentation
/// note §2.2; `Unknown` exists only for call sites that genuinely do not know.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum InterceptReason {
    UserStart,
    UserStop,
    PresenceStart,
    PresenceStop,
    RelaySwitch,
    ServerTransition,
    StaleServer,
    WatchdogDemote,
    ErrorRetry,
    AutoRecover,
    Quit,
    Update,
    ElevationHandoff,
    ForceKillRecovery,
    #[default]
    Unknown,
}

impl InterceptReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::UserStart => "user_start",
            Self::UserStop => "user_stop",
            Self::PresenceStart => "presence_start",
            Self::PresenceStop => "presence_stop",
            Self::RelaySwitch => "relay_switch",
            Self::ServerTransition => "server_transition",
            Self::StaleServer => "stale_server",
            Self::WatchdogDemote => "watchdog_demote",
            Self::ErrorRetry => "error_retry",
            Self::AutoRecover => "auto_recover",
            Self::Quit => "quit",
            Self::Update => "update",
            Self::ElevationHandoff => "elevation_handoff",
            Self::ForceKillRecovery => "force_kill_recovery",
            Self::Unknown => "unknown",
        }
    }
}

/// A WinDivert handle role, mirroring the interceptor's `HandleKind`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HandleKindName {
    Pin,
    #[default]
    Intercept,
    Inject,
    Shadow,
    Legacy,
}

impl HandleKindName {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pin => "pin",
            Self::Intercept => "intercept",
            Self::Inject => "inject",
            Self::Shadow => "shadow",
            Self::Legacy => "legacy",
        }
    }
}

/// The result of one handle-open attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum OpenResult {
    #[default]
    Ok,
    Failed,
}

/// Why the game-presence state changed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum PresenceCause {
    ProcessStarted,
    ProcessExited,
    NameMatch,
    #[default]
    ScanError,
    Disabled,
    ManualPick,
}

impl PresenceCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ProcessStarted => "process_started",
            Self::ProcessExited => "process_exited",
            Self::NameMatch => "name_match",
            Self::ScanError => "scan_error",
            Self::Disabled => "disabled",
            Self::ManualPick => "manual_pick",
        }
    }
}

/// The outcome of one control-plane registration attempt.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum RegistrationOutcome {
    Ok,
    #[default]
    Fail,
    Timeout,
    Superseded,
}

/// Why a registration session ended (P0-3's ledger vocabulary; P0-1 only
/// records it when the caller knows it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionCloseReason {
    SessionReplaced,
    ClientQuit,
    Timeout,
    Unknown,
}

/// One relay-reply window's verdict (the existing judged-window line).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum WatchdogVerdict {
    #[default]
    Healthy,
    NoReplyFromRelay,
    InjectionFailed,
    TooFewReplies,
}

impl WatchdogVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Healthy => "healthy",
            Self::NoReplyFromRelay => "no_reply_from_relay",
            Self::InjectionFailed => "injection_failed",
            Self::TooFewReplies => "too_few_replies",
        }
    }
}

/// What the watchdog did about a verdict.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum WatchdogAction {
    #[default]
    Keep,
    Demote,
    SwitchDirect,
}

impl WatchdogAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keep => "keep",
            Self::Demote => "demote",
            Self::SwitchDirect => "switch_direct",
        }
    }
}

/// The subsystem an error record belongs to (P0-3's source taxonomy, used here
/// for the render-side `ui_error` event).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSource {
    Capture,
    Route,
    Registration,
    Config,
    Game,
}

/// What happened to the visible error card.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum UiErrorOp {
    #[default]
    Show,
    Clear,
    Replace,
}

impl UiErrorOp {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Show => "show",
            Self::Clear => "clear",
            Self::Replace => "replace",
        }
    }
}

/// Why the process is shutting down.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
#[derive(Default)]
pub enum ShutdownReason {
    Quit,
    Update,
    Panic,
    ElevationHandoff,
    Kill,
    #[default]
    Unknown,
}

impl ShutdownReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Quit => "quit",
            Self::Update => "update",
            Self::Panic => "panic",
            Self::ElevationHandoff => "elevation_handoff",
            Self::Kill => "kill",
            Self::Unknown => "unknown",
        }
    }
}

/// What the previous process left behind, read from the marker file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PrevShutdown {
    Clean,
    Unclean,
    Unknown,
}

impl PrevShutdown {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Clean => "clean",
            Self::Unclean => "unclean",
            Self::Unknown => "unknown",
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Typed events
// ─────────────────────────────────────────────────────────────────────────────
//
// Every struct is a flat JSON object. Field names are the schema; `serde`
// renames are deliberate and tested. Optional fields may be omitted or null.

/// `event=windivert_open` — one record per attempt (a retried open keeps its id).
#[derive(Clone, Debug, Default, Serialize)]
pub struct HandleOpen {
    pub id: u64,
    pub kind: HandleKindName,
    pub attempt: u32,
    pub layer: String,
    pub priority: i16,
    pub flags: String,
    pub filter: String,
    pub reason: InterceptReason,
    pub actor: Actor,
    pub command_id: String,
    pub pin_active: bool,
    pub pin_id: Option<u64>,
    pub elapsed_us: u64,
    pub ms_since_last_close: Option<u64>,
    pub fwp_code: Option<String>,
    pub owned_handles: String,
    pub result: OpenResult,
    pub snapshot_id: Option<String>,
}

/// `event=windivert_close` — the matching close for a handle id.
#[derive(Clone, Debug, Default, Serialize)]
pub struct HandleClose {
    pub id: u64,
    pub kind: HandleKindName,
    pub close_age_ms: u64,
    pub result: CloseResult,
    pub pin_active: bool,
    pub owned_handles: String,
}

/// The result of a handle close.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CloseResult {
    #[default]
    Ok,
    Failed,
}

/// `event=interceptor_state` — one record per capture-plane transition.
#[derive(Clone, Debug, Default, Serialize)]
pub struct InterceptorState {
    pub from: String,
    pub to: String,
    pub reason: InterceptReason,
    pub actor: Actor,
    pub command_id: String,
    pub pin_id: Option<u64>,
    pub pin_active: bool,
    pub handle_ids: String,
    pub locked_server: Option<String>,
    pub relay: Option<String>,
    pub duration_ms: u64,
    pub attempt: Option<u32>,
    pub max_attempts: Option<u32>,
    pub prev_event_id: Option<u64>,
}

/// `event=control_registration` — one record per registration attempt/session.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ControlRegistration {
    pub attempt: u32,
    pub outcome: RegistrationOutcome,
    pub session: Option<u32>,
    pub relay: Option<String>,
    pub node: Option<String>,
    pub region: Option<String>,
    pub reason: String,
    pub prev_session: Option<u32>,
    pub superseded: bool,
    pub close_reason: Option<SessionCloseReason>,
    pub generation: Option<u64>,
}

/// `event=presence` — one record per presence-state change.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Presence {
    pub from: String,
    pub to: String,
    pub cause: PresenceCause,
    pub scan_ms: u64,
    pub latch: Option<String>,
}

/// `event=watchdog` — the structured form of the judged-window line.
#[derive(Clone, Debug, Default, Serialize)]
pub struct Watchdog {
    pub verdict: WatchdogVerdict,
    pub streak: u32,
    pub fault: Option<String>,
    pub action: WatchdogAction,
    pub relay: Option<String>,
    pub server: Option<String>,
    pub intercepted: u64,
    pub injected: u64,
    pub from_relay: u64,
}

/// `event=ui_error` — the visible error card's show/clear/replace lifecycle.
#[derive(Clone, Debug, Default, Serialize)]
pub struct UiError {
    pub op: UiErrorOp,
    pub source: Option<ErrorSource>,
    pub kind: String,
    pub detail_key: Option<String>,
    pub source_command: Option<String>,
    pub visible_primary_state: Option<String>,
    pub reason_code: Option<String>,
    pub cleared_by: Option<String>,
    pub age_ms: Option<u64>,
}

/// `event=ui.word` — the live word changed.
#[derive(Clone, Debug, Default, Serialize)]
pub struct UiWord {
    pub from: String,
    pub to: String,
    pub reason: String,
    pub boosting: bool,
}

/// `event=ui_state` — the semantic screen dump (no screenshot in v1).
#[derive(Clone, Debug, Default, Serialize)]
pub struct UiState {
    pub action_slot: String,
    pub relay: Option<String>,
    pub registration: Option<String>,
    pub error: Option<String>,
    pub boosting: bool,
    pub locked_server: Option<String>,
    pub relay_rtt_ms: Option<f64>,
}

/// `event=shutdown` — one record at process exit (clean flag included).
#[derive(Clone, Debug, Default, Serialize)]
pub struct Shutdown {
    pub reason: ShutdownReason,
    pub clean: bool,
    pub handle_ids_closed: String,
    pub pin_released: bool,
}

// ─────────────────────────────────────────────────────────────────────────────
//  gui.action — the capture-crossing action audit (P0-4 / T15)
// ─────────────────────────────────────────────────────────────────────────────

/// Who initiated a capture-crossing action (`gui.action.source`).
///
/// Closed set: the audit's `button|tray|shortcut|menu` user sources plus the two
/// non-user flows the GUI has — `auto` (game-presence) and `system` (Quit,
/// update install and the elevation handoff). A new source is a schema change,
/// which is what makes T15 mechanically checkable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionSource {
    Button,
    Tray,
    Shortcut,
    Auto,
    System,
}

impl ActionSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Button => "button",
            Self::Tray => "tray",
            Self::Shortcut => "shortcut",
            Self::Auto => "auto",
            Self::System => "system",
        }
    }
}

/// The capture-plane state a `gui.action` record names as `from`/`to`.
///
/// These are the audit's P2 states, not the route/presentation word: a route
/// warning must never look like a capture transition. `Shutdown` exists for the
/// actions that hand the process away (Quit/update/elevation), where the capture
/// plane is being left for good rather than merely stopped.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CaptureState {
    Capturing,
    Idle,
    Fault,
    Shutdown,
}

impl CaptureState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Capturing => "capturing",
            Self::Idle => "idle",
            Self::Fault => "fault_latched",
            Self::Shutdown => "shutdown",
        }
    }

    /// Classify an engine's capture plane before/after an action.
    ///
    /// `fault` is the latched fatal capture fault (`EngineStatus::capture_fault`),
    /// not a route/registration error: only a dead/degraded capture run may read
    /// as `Fault`.
    pub fn of(active: bool, fault: bool) -> Self {
        if active {
            Self::Capturing
        } else if fault {
            Self::Fault
        } else {
            Self::Idle
        }
    }
}

/// The capture-crossing actions the trace must be able to attribute (T15).
///
/// `Optimize` covers every user/presence start; `Restart` is only the explicit
/// restart path, so a plain start can never be mistaken for a teardown.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GuiActionKind {
    Optimize,
    Stop,
    Restart,
    Quit,
    UpdateInstall,
    ElevationHandoff,
}

impl GuiActionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Optimize => "optimize",
            Self::Stop => "stop",
            Self::Restart => "restart",
            Self::Quit => "quit",
            Self::UpdateInstall => "update_install",
            Self::ElevationHandoff => "elevation_handoff",
        }
    }
}

/// One capture-crossing action, emitted exactly once per invocation.
///
/// This is the record that makes the 2026-11-23 trace answerable: the 06:49 Stop
/// had no actor, so the next one can be attributed from the trace alone
/// (`docs/audit/game-transition-state-machine-2026-11-23.md` F6/I14/T15).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GuiAction<'a> {
    /// The capture-crossing action.
    pub action: GuiActionKind,
    /// Who initiated it.
    pub source: ActionSource,
    /// The capture-plane state the action found.
    pub from: CaptureState,
    /// The capture-plane state it left (equal to `from` when it was refused).
    pub to: CaptureState,
    /// The P0-1 command id the matching `interceptor_state` intent carries.
    pub command_id: &'a str,
}

/// The JSON body of a `gui.action` record (pure; tested).
pub fn gui_action_json(action: &GuiAction<'_>) -> Value {
    json!({
        "action": action.action.as_str(),
        "source": action.source.as_str(),
        "from": action.from.as_str(),
        "to": action.to.as_str(),
        "command_id": action.command_id,
    })
}

/// Emit exactly one `gui.action` record for one capture-crossing invocation.
///
/// The caller emits it once per action, after the command attempt, with the
/// capture state it actually left. A refused start therefore records one action
/// whose `from == to` (nothing was torn down), not two.
pub fn record_gui_action(action: &GuiAction<'_>) {
    record("gui.action", gui_action_json(action));
}

// ─────────────────────────────────────────────────────────────────────────────
//  Identity
// ─────────────────────────────────────────────────────────────────────────────

/// The immutable build/host identity stamped into every record's base and the
/// `app_start` event (A12 consumes it).
#[derive(Clone, Debug, Serialize)]
pub struct Identity {
    pub version: String,
    pub commit: String,
    pub windivert_dll_sha256: Option<String>,
    pub windivert_sys_sha256: Option<String>,
    pub windivert_version: Option<String>,
    pub elevated: bool,
    pub host_os: String,
    pub boot_id: Option<String>,
}

impl Identity {
    /// `"1.12.5@6e30a65"` — the short `build` value on every record.
    pub fn build(&self) -> String {
        format!("{}@{}", self.version, self.commit)
    }
}

/// Facts returned by [`init`] so the caller can log them without re-deriving.
#[derive(Clone, Debug, Serialize)]
pub struct StartupFacts {
    pub identity: Identity,
    pub app_session: u64,
    pub prev_shutdown: PrevShutdown,
}

/// Configuration for [`init`]. The data directory is the only required piece;
/// a front end without a log directory (the CLI) simply never calls it.
#[derive(Clone, Debug)]
pub struct LifecycleConfig {
    pub data_dir: PathBuf,
    /// Override the process session id (tests and harnesses).
    pub app_session: Option<u64>,
}

impl LifecycleConfig {
    pub fn new(data_dir: impl Into<PathBuf>) -> Self {
        Self {
            data_dir: data_dir.into(),
            app_session: None,
        }
    }
}

/// The interception intent the interceptor reads when it opens a handle.
///
/// Set by the engine immediately before a capture-plane action, so the handle
/// records carry the reason/actor without threading new parameters through every
/// open call (which would be a behaviour-risk change, not instrumentation).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Intent {
    pub reason: InterceptReason,
    pub actor: Actor,
    pub command_id: String,
}

impl Default for Intent {
    fn default() -> Self {
        Self {
            reason: InterceptReason::Unknown,
            actor: Actor::Internal,
            command_id: String::new(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  The sink
// ─────────────────────────────────────────────────────────────────────────────

/// One open `lifecycle.jsonl` sink plus the bounded event ring.
#[derive(Debug)]
pub struct Lifecycle {
    data_dir: PathBuf,
    file: Option<File>,
    identity: Identity,
    app_session: u64,
    prev_shutdown: PrevShutdown,
    seq: u64,
    ring: VecDeque<Value>,
    last_bundle: Option<Instant>,
}

impl Lifecycle {
    /// Open (or create) the sink in `data_dir` and read/refresh the marker.
    ///
    /// A failure to create or open the file is not an error for the product:
    /// the returned sink simply has no file and every [`Self::record`] call is a
    /// no-op apart from the in-memory ring.
    pub fn open(data_dir: impl Into<PathBuf>, app_session: Option<u64>) -> Self {
        let data_dir = data_dir.into();
        let _ = std::fs::create_dir_all(&data_dir);
        let prev_shutdown = read_marker(&data_dir);
        write_marker(&data_dir);
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(data_dir.join(LIFECYCLE_FILE))
            .ok();
        Self {
            data_dir,
            file,
            identity: Identity::detect(),
            app_session: app_session.unwrap_or_else(now_unix_ms),
            prev_shutdown,
            seq: 0,
            ring: VecDeque::with_capacity(EVENT_RING_CAPACITY),
            last_bundle: None,
        }
    }

    pub fn identity(&self) -> &Identity {
        &self.identity
    }

    pub fn app_session(&self) -> u64 {
        self.app_session
    }

    pub fn prev_shutdown(&self) -> PrevShutdown {
        self.prev_shutdown
    }

    /// Append one record to the JSONL sink, the in-memory ring and the trace.
    pub fn record(&mut self, event: &'static str, fields: Value) {
        self.seq = self.seq.wrapping_add(1);
        let now = now_unix_ms();
        let mut object = Map::new();
        object.insert("ts".to_string(), Value::String(utc_now_rfc3339()));
        object.insert("ts_ms".to_string(), Value::Number(now.into()));
        object.insert(
            "app_session".to_string(),
            Value::Number(self.app_session.into()),
        );
        object.insert("build".to_string(), Value::String(self.identity.build()));
        object.insert("seq".to_string(), Value::Number(self.seq.into()));
        object.insert("event".to_string(), Value::String(event.to_string()));
        if let Value::Object(map) = fields {
            for (key, value) in map {
                object.insert(key, value);
            }
        }
        let value = Value::Object(object);
        let line = match serde_json::to_string(&value) {
            Ok(line) => line,
            Err(_) => return,
        };
        if let Some(file) = self.file.as_mut() {
            let _ = writeln!(file, "{line}");
        }
        if self.ring.len() == EVENT_RING_CAPACITY {
            self.ring.pop_front();
        }
        self.ring.push_back(value.clone());
        // Human trace: keep the shipped `event=` + key=value shape.
        tracing::info!("event={} {}", event, compact_fields(&value));
    }

    /// Emit a typed event. Compilation pins the schema; a serialization failure
    /// (impossible for these structs) is dropped rather than propagated.
    pub fn emit<T: Serialize>(&mut self, event: &'static str, value: &T) {
        if let Ok(value) = serde_json::to_value(value) {
            self.record(event, value);
        }
    }

    /// Write a support bundle if the rate limit allows it.
    ///
    /// Returns the bundle directory name (also useful as the `snapshot_id` the
    /// open-failure record carries) or `None` when rate-limited or unwritable.
    /// Read-only: the bundle never invokes a state-changing command.
    pub fn support_bundle(&mut self, reason: &str, extra: Value) -> Option<String> {
        if let Some(last) = self.last_bundle {
            if last.elapsed() < SUPPORT_BUNDLE_MIN_INTERVAL {
                return None;
            }
        }
        self.last_bundle = Some(Instant::now());
        let name = format!("bundle-{}-{}", now_unix_ms(), sanitize(reason));
        let dir = self.data_dir.join("support").join(&name);
        if std::fs::create_dir_all(&dir).is_err() {
            return None;
        }
        let service = service_query();
        let bundle = json!({
            "ts": utc_now_rfc3339(),
            "reason": reason,
            "app_session": self.app_session,
            "build": self.identity.build(),
            "identity": self.identity,
            "prev_shutdown": self.prev_shutdown,
            "service": service,
            "wfp_snapshot": "deferred_p0_1_v1",
            "extra": extra,
            "events": self.ring.iter().cloned().collect::<Vec<_>>(),
        });
        let _ = std::fs::write(
            dir.join("bundle.json"),
            serde_json::to_string_pretty(&bundle).unwrap_or_default(),
        );
        let tail: Vec<String> = self
            .ring
            .iter()
            .filter_map(|value| serde_json::to_string(value).ok())
            .collect();
        let _ = std::fs::write(dir.join("lifecycle-tail.jsonl"), tail.join("\n"));
        if let Some(service) = bundle.get("service") {
            let _ = std::fs::write(
                dir.join("sc-query.txt"),
                serde_json::to_string_pretty(service).unwrap_or_default(),
            );
        }
        Some(name)
    }

    /// Record a clean shutdown and remove the unclean marker.
    pub fn shutdown(&mut self, event: Shutdown) {
        let clean = event.clean;
        self.emit("shutdown", &event);
        if clean {
            let _ = std::fs::remove_file(self.data_dir.join(UNCLEAN_MARKER));
        }
    }

    /// The most recent events (oldest first), for tests and diagnostics.
    pub fn recent_events(&self) -> Vec<Value> {
        self.ring.iter().cloned().collect()
    }
}

impl Identity {
    /// Detect the identity of this process without touching driver state.
    fn detect() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|exe| exe.parent().map(Path::to_path_buf));
        let (dll, sys) = match exe_dir.as_deref() {
            Some(dir) => (
                sha256_file(&dir.join("WinDivert.dll")),
                sha256_file(&dir.join("WinDivert64.sys")),
            ),
            None => (None, None),
        };
        Self {
            version: BUILD_VERSION.to_string(),
            commit: BUILD_COMMIT.to_string(),
            windivert_dll_sha256: dll,
            windivert_sys_sha256: sys,
            windivert_version: None,
            elevated: crate::host::elevated(),
            host_os: crate::host::description(),
            boot_id: crate::host::boot_id(),
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Process-global sink (one per process)
// ─────────────────────────────────────────────────────────────────────────────

static GLOBAL: OnceLock<Mutex<Option<Lifecycle>>> = OnceLock::new();
static INTENT: Mutex<Intent> = Mutex::new(Intent {
    reason: InterceptReason::Unknown,
    actor: Actor::Internal,
    command_id: String::new(),
});

// ── Test seam ─────────────────────────────────────────────────────────────
//
// The sink is process-global in production (one JSONL per process) and the
// intent is process-global too (the interceptor opens on worker threads). That
// makes a parallel `cargo test` run share one sink and one intent, so an
// integrated test could observe another test's events. This thread-local
// override gives a test its own isolated sink for actions recorded on its own
// thread; production builds compile it away.

#[cfg(test)]
thread_local! {
    static TEST_SINK: std::cell::RefCell<Option<Lifecycle>> =
        const { std::cell::RefCell::new(None) };
    /// Per-test-thread intent (see [`set_intent`]).
    static TEST_INTENT: std::cell::RefCell<Intent> =
        const {
            std::cell::RefCell::new(Intent {
                reason: InterceptReason::Unknown,
                actor: Actor::Internal,
                command_id: String::new(),
            })
        };
}

/// Route a record to the test thread's isolated sink first; production falls
/// back to the process-global sink.
fn with_sink<R>(f: impl FnOnce(&mut Lifecycle) -> R) -> Option<R> {
    #[cfg(test)]
    {
        let taken = TEST_SINK.with(|cell| cell.borrow_mut().take());
        if let Some(mut sink) = taken {
            let result = f(&mut sink);
            TEST_SINK.with(|cell| *cell.borrow_mut() = Some(sink));
            return Some(result);
        }
    }
    with_lifecycle(f)
}

/// A test thread's isolated lifecycle sink, live until the guard drops.
#[cfg(test)]
pub struct TestSinkGuard;

#[cfg(test)]
impl Drop for TestSinkGuard {
    fn drop(&mut self) {
        TEST_SINK.with(|cell| *cell.borrow_mut() = None);
    }
}

/// Install an isolated sink on the calling test thread.
///
/// Actions recorded on other threads (a spawned registration task) still go to
/// the process-global sink, which is why the integrated test drives only
/// same-thread paths.
#[cfg(test)]
pub fn install_test_sink(dir: impl Into<PathBuf>) -> TestSinkGuard {
    let mut sink = Lifecycle::open(dir, Some(1));
    sink.record("app_start", json!({}));
    TEST_SINK.with(|cell| *cell.borrow_mut() = Some(sink));
    TestSinkGuard
}

/// Initialise the process-global sink. Idempotent: a second call is a no-op and
/// returns the facts recorded by the first.
pub fn init(config: LifecycleConfig) -> Option<StartupFacts> {
    let cell = GLOBAL.get_or_init(|| Mutex::new(None));
    let mut guard = lock(cell);
    if guard.is_some() {
        return guard.as_ref().map(|lifecycle| StartupFacts {
            identity: lifecycle.identity().clone(),
            app_session: lifecycle.app_session(),
            prev_shutdown: lifecycle.prev_shutdown(),
        });
    }
    let mut lifecycle = Lifecycle::open(config.data_dir, config.app_session);
    let facts = StartupFacts {
        identity: lifecycle.identity().clone(),
        app_session: lifecycle.app_session(),
        prev_shutdown: lifecycle.prev_shutdown(),
    };
    lifecycle.record(
        "app_start",
        json!({
            "version": facts.identity.version,
            "commit": facts.identity.commit,
            "windivert_dll_sha256": facts.identity.windivert_dll_sha256,
            "windivert_sys_sha256": facts.identity.windivert_sys_sha256,
            "windivert_version": facts.identity.windivert_version,
            "elevated": facts.identity.elevated,
            "host_os": facts.identity.host_os,
            "boot_id": facts.identity.boot_id,
            "prev_shutdown": facts.prev_shutdown,
        }),
    );
    *guard = Some(lifecycle);
    Some(facts)
}

/// Whether the process-global sink is initialised.
pub fn is_initialised() -> bool {
    GLOBAL
        .get()
        .map(|cell| lock(cell).is_some())
        .unwrap_or(false)
}

/// Append one record; a no-op when [`init`] was never called.
pub fn record(event: &'static str, fields: Value) {
    with_sink(|lifecycle| lifecycle.record(event, fields));
}

/// Emit one typed record; a no-op when [`init`] was never called.
pub fn emit<T: Serialize>(event: &'static str, value: &T) {
    with_sink(|lifecycle| lifecycle.emit(event, value));
}

/// Rate-limited support bundle; returns the snapshot id when written.
pub fn support_bundle(reason: &str, extra: Value) -> Option<String> {
    with_sink(|lifecycle| lifecycle.support_bundle(reason, extra)).flatten()
}

/// Record a shutdown and remove the unclean marker when `clean`.
pub fn shutdown(event: Shutdown) {
    with_sink(|lifecycle| lifecycle.shutdown(event));
}

/// The build identity, when initialised.
pub fn identity() -> Option<Identity> {
    with_lifecycle(|lifecycle| lifecycle.identity().clone())
}

/// The current process session id (`0` when uninitialised).
pub fn app_session() -> u64 {
    with_lifecycle(|lifecycle| lifecycle.app_session()).unwrap_or(0)
}

/// Set the intent the next handle open/close records carry.
///
/// Production stores one process-global intent because a capture-plane open runs
/// on a worker thread while the command that asked for it ran on the UI thread.
/// Unit tests use a per-thread intent: libtest runs every test on its own
/// thread, so a suite that exercises commands and lifecycle helpers in parallel
/// cannot have one test's intent observed by another's assertion.
pub fn set_intent(intent: Intent) {
    #[cfg(test)]
    {
        TEST_INTENT.with(|cell| *cell.borrow_mut() = intent);
    }
    #[cfg(not(test))]
    {
        let mut guard = lock_intent();
        *guard = intent;
    }
}

/// Read the current intent (interceptor-side).
pub fn current_intent() -> Intent {
    #[cfg(test)]
    {
        TEST_INTENT.with(|cell| cell.borrow().clone())
    }
    #[cfg(not(test))]
    {
        lock_intent().clone()
    }
}

fn with_lifecycle<R>(f: impl FnOnce(&mut Lifecycle) -> R) -> Option<R> {
    let cell = GLOBAL.get()?;
    let mut guard = lock(cell);
    guard.as_mut().map(f)
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_intent() -> std::sync::MutexGuard<'static, Intent> {
    INTENT
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

// ─────────────────────────────────────────────────────────────────────────────
//  Helpers
// ─────────────────────────────────────────────────────────────────────────────

/// One `key=value` projection of an event object for the human trace.
pub fn compact_fields(value: &Value) -> String {
    let Value::Object(map) = value else {
        return String::new();
    };
    let mut parts = Vec::new();
    for (key, value) in map {
        if matches!(
            key.as_str(),
            "ts" | "ts_ms" | "app_session" | "build" | "seq" | "event"
        ) {
            continue;
        }
        let rendered = match value {
            Value::String(text) if text.is_empty() => String::new(),
            Value::String(text) if !text.contains(' ') => text.clone(),
            other => other.to_string(),
        };
        if rendered.is_empty() {
            parts.push(format!("{key}="));
        } else {
            parts.push(format!("{key}={rendered}"));
        }
    }
    parts.join(" ")
}

fn now_unix_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis() as u64)
        .unwrap_or(0)
}

fn utc_now_rfc3339() -> String {
    time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| now_unix_ms().to_string())
}

fn sanitize(text: &str) -> String {
    let cleaned: String = text
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' {
                ch
            } else {
                '-'
            }
        })
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "event".to_string()
    } else {
        trimmed.chars().take(40).collect()
    }
}

/// `Some("unclean")` when the marker exists, `Some("clean")` otherwise.
fn read_marker(data_dir: &Path) -> PrevShutdown {
    if data_dir.join(UNCLEAN_MARKER).exists() {
        PrevShutdown::Unclean
    } else {
        PrevShutdown::Clean
    }
}

fn write_marker(data_dir: &Path) {
    let marker = data_dir.join(UNCLEAN_MARKER);
    let body = format!("started_at_ms={}\n", now_unix_ms());
    let _ = std::fs::write(marker, body);
}

/// Read-only `sc.exe query WinDivert` projection for the support bundle.
///
/// The platform call lives in `crate::host` so `client/src/app/` stays portable
/// (gui_boundary G5). A host without an SCM records the fact instead of guessing.
fn service_query() -> Value {
    match crate::host::windivert_service_query() {
        Some(query) => json!({
            "available": true,
            "exit_code": query.exit_code,
            "stdout": query.stdout,
            "stderr": query.stderr,
        }),
        None => json!({ "available": false, "reason": "no scm on this host" }),
    }
}

/// SHA-256 of a file, lowercase hex; `None` when it cannot be read.
pub fn sha256_file(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    Some(sha256_hex(&bytes))
}

/// SHA-256 of bytes, lowercase hex.
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = ring::digest::digest(&ring::digest::SHA256, bytes);
    digest
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "lightspeed-lifecycle-{tag}-{}-{}",
            std::process::id(),
            now_unix_ms()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    fn read_lines(dir: &Path) -> Vec<Value> {
        let text = std::fs::read_to_string(dir.join(LIFECYCLE_FILE)).unwrap_or_default();
        text.lines()
            .filter(|line| !line.trim().is_empty())
            .map(|line| serde_json::from_str(line).expect("valid JSON per line"))
            .collect()
    }

    #[test]
    fn records_are_one_json_object_per_line_with_the_base_fields() {
        let dir = temp_dir("schema");
        let mut lifecycle = Lifecycle::open(&dir, Some(42));
        let open = HandleOpen {
            id: 7,
            kind: HandleKindName::Intercept,
            attempt: 1,
            layer: "network".to_string(),
            priority: 0,
            flags: "0x0000".to_string(),
            filter: "udp".to_string(),
            reason: InterceptReason::UserStart,
            actor: Actor::User,
            command_id: "cmd-1".to_string(),
            pin_active: true,
            pin_id: Some(3),
            elapsed_us: 1200,
            ms_since_last_close: None,
            fwp_code: None,
            owned_handles: "intercept:7".to_string(),
            result: OpenResult::Ok,
            snapshot_id: None,
        };
        lifecycle.emit("windivert_open", &open);
        let lines = read_lines(&dir);
        assert_eq!(lines.len(), 1);
        let event = &lines[0];
        for key in [
            "ts",
            "ts_ms",
            "app_session",
            "build",
            "seq",
            "event",
            "id",
            "kind",
            "reason",
            "actor",
            "pin_active",
        ] {
            assert!(event.get(key).is_some(), "missing base/event field {key}");
        }
        assert_eq!(event["event"], "windivert_open");
        assert_eq!(event["app_session"].as_u64(), Some(42));
        assert_eq!(event["kind"], "intercept");
        assert_eq!(event["reason"], "user_start");
        assert_eq!(event["actor"], "user");
        assert_eq!(event["result"], "ok");
        assert_eq!(event["seq"].as_u64(), Some(1));
    }

    #[test]
    fn closed_vocabularies_serialize_to_the_documented_spellings() {
        assert_eq!(
            serde_json::to_string(&InterceptReason::ForceKillRecovery).unwrap(),
            "\"force_kill_recovery\""
        );
        assert_eq!(
            serde_json::to_string(&InterceptReason::StaleServer).unwrap(),
            "\"stale_server\""
        );
        assert_eq!(
            serde_json::to_string(&PresenceCause::ProcessExited).unwrap(),
            "\"process_exited\""
        );
        assert_eq!(
            serde_json::to_string(&UiErrorOp::Replace).unwrap(),
            "\"replace\""
        );
        assert_eq!(
            serde_json::to_string(&WatchdogVerdict::NoReplyFromRelay).unwrap(),
            "\"no_reply_from_relay\""
        );
        assert_eq!(
            serde_json::to_string(&ShutdownReason::ElevationHandoff).unwrap(),
            "\"elevation_handoff\""
        );
        assert_eq!(
            serde_json::to_string(&HandleKindName::Shadow).unwrap(),
            "\"shadow\""
        );
    }

    #[test]
    fn the_event_ring_is_bounded_and_keeps_the_newest_events() {
        let dir = temp_dir("ring");
        let mut lifecycle = Lifecycle::open(&dir, None);
        for index in 0..(EVENT_RING_CAPACITY + 10) {
            lifecycle.record("presence", json!({ "index": index }));
        }
        let events = lifecycle.recent_events();
        assert_eq!(events.len(), EVENT_RING_CAPACITY);
        assert_eq!(events[0]["index"].as_u64(), Some(10));
        assert_eq!(
            events[EVENT_RING_CAPACITY - 1]["index"].as_u64(),
            Some((EVENT_RING_CAPACITY + 9) as u64)
        );
    }

    #[test]
    fn the_unclean_marker_turns_a_crash_into_prev_shutdown_unclean() {
        let dir = temp_dir("marker");
        {
            let mut first = Lifecycle::open(&dir, None);
            assert_eq!(first.prev_shutdown(), PrevShutdown::Clean);
            first.record("app_start", json!({}));
            // No clean shutdown: the marker stays behind.
        }
        let mut second = Lifecycle::open(&dir, None);
        assert_eq!(second.prev_shutdown(), PrevShutdown::Unclean);
        second.shutdown(Shutdown {
            reason: ShutdownReason::Quit,
            clean: true,
            handle_ids_closed: "2,3,4".to_string(),
            pin_released: true,
        });
        assert!(!dir.join(UNCLEAN_MARKER).exists());
        let third = Lifecycle::open(&dir, None);
        assert_eq!(third.prev_shutdown(), PrevShutdown::Clean);
    }

    #[test]
    fn the_support_bundle_is_rate_limited_and_read_only() {
        let dir = temp_dir("bundle");
        let mut lifecycle = Lifecycle::open(&dir, None);
        lifecycle.record("app_start", json!({}));
        let first =
            lifecycle.support_bundle("windivert_open_failure", json!({"fwp": "0x8032000A"}));
        assert!(first.is_some(), "the first bundle must be written");
        let second = lifecycle.support_bundle("windivert_open_failure", json!({}));
        assert_eq!(second, None, "a second bundle inside the window is refused");
        let name = first.unwrap();
        let bundle_path = dir.join("support").join(&name).join("bundle.json");
        let bundle: Value =
            serde_json::from_str(&std::fs::read_to_string(bundle_path).unwrap()).unwrap();
        assert_eq!(bundle["reason"], "windivert_open_failure");
        assert_eq!(bundle["wfp_snapshot"], "deferred_p0_1_v1");
        assert!(bundle["events"].as_array().is_some());
        assert!(bundle["identity"]["version"].as_str().is_some());
        // The service query may be unavailable off Windows, but the key exists.
        assert!(bundle.get("service").is_some());
    }

    #[test]
    fn records_carry_no_payload_or_token_fields() {
        let dir = temp_dir("privacy");
        let mut lifecycle = Lifecycle::open(&dir, None);
        lifecycle.record("presence", json!({"to": "robrun"}));
        let text = std::fs::read_to_string(dir.join(LIFECYCLE_FILE)).unwrap();
        for banned in ["payload", "token", "password", "secret", "packet_hex"] {
            assert!(
                !text.contains(banned),
                "the lifecycle schema must not contain {banned}"
            );
        }
    }

    #[test]
    fn compact_fields_skips_the_base_and_quotes_only_when_needed() {
        let value = json!({
            "ts": "2026-11-23T00:00:00Z",
            "event": "presence",
            "seq": 1,
            "to": "robrun",
            "from": "none",
            "why": "two words",
        });
        let rendered = compact_fields(&value);
        assert!(!rendered.contains("ts="));
        assert!(!rendered.contains("event="));
        assert!(rendered.contains("to=robrun"));
        assert!(rendered.contains("why=\"two words\""));
    }

    #[test]
    fn sha256_matches_a_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn sanitize_keeps_bundle_names_path_safe() {
        assert_eq!(
            sanitize("windivert open/failure!"),
            "windivert-open-failure"
        );
        assert_eq!(sanitize("///"), "event");
    }

    #[test]
    fn intent_round_trips_without_touching_capture_policy() {
        set_intent(Intent {
            reason: InterceptReason::UserStart,
            actor: Actor::User,
            command_id: "cmd-9".to_string(),
        });
        let intent = current_intent();
        assert_eq!(intent.reason, InterceptReason::UserStart);
        assert_eq!(intent.actor, Actor::User);
        assert_eq!(intent.command_id, "cmd-9");
        set_intent(Intent::default());
    }

    #[test]
    fn identity_names_the_build_even_when_files_are_absent() {
        let identity = Identity::detect();
        assert_eq!(identity.version, BUILD_VERSION);
        assert!(!identity.commit.is_empty());
        // The DLL/sys hashes are present in an installed tree and absent in a
        // bare test run; either way the field exists and is a 64-char hex string
        // when present.
        for hash in [
            identity.windivert_dll_sha256.as_deref(),
            identity.windivert_sys_sha256.as_deref(),
        ]
        .into_iter()
        .flatten()
        {
            assert_eq!(hash.len(), 64);
            assert!(hash.chars().all(|ch| ch.is_ascii_hexdigit()));
        }
    }

    // ── gui.action — capture-crossing action audit (P0-4 / T15) ───────────

    #[test]
    fn a_gui_action_record_is_exactly_the_documented_five_fields() {
        let action = GuiAction {
            action: GuiActionKind::Optimize,
            source: ActionSource::Button,
            from: CaptureState::Idle,
            to: CaptureState::Capturing,
            command_id: "StartBoost",
        };
        let json = gui_action_json(&action);
        let mut keys: Vec<&str> = json
            .as_object()
            .expect("gui.action is one object")
            .keys()
            .map(String::as_str)
            .collect();
        keys.sort_unstable();
        assert_eq!(
            keys,
            vec!["action", "command_id", "from", "source", "to"],
            "the schema is the audit's five fields, not a second event shape"
        );
        assert_eq!(json["action"], "optimize");
        assert_eq!(json["source"], "button");
        assert_eq!(json["from"], "idle");
        assert_eq!(json["to"], "capturing");
        assert_eq!(json["command_id"], "StartBoost");
    }

    #[test]
    fn the_gui_action_vocabularies_serialize_to_the_documented_spellings() {
        // T15/T11 closed sets: a new spelling is a schema change.
        for (kind, expected) in [
            (GuiActionKind::Optimize, "optimize"),
            (GuiActionKind::Stop, "stop"),
            (GuiActionKind::Restart, "restart"),
            (GuiActionKind::Quit, "quit"),
            (GuiActionKind::UpdateInstall, "update_install"),
            (GuiActionKind::ElevationHandoff, "elevation_handoff"),
        ] {
            assert_eq!(kind.as_str(), expected);
        }
        for (source, expected) in [
            (ActionSource::Button, "button"),
            (ActionSource::Tray, "tray"),
            (ActionSource::Shortcut, "shortcut"),
            (ActionSource::Auto, "auto"),
            (ActionSource::System, "system"),
        ] {
            assert_eq!(source.as_str(), expected);
        }
        for (state, expected) in [
            (CaptureState::Capturing, "capturing"),
            (CaptureState::Idle, "idle"),
            (CaptureState::Fault, "fault_latched"),
            (CaptureState::Shutdown, "shutdown"),
        ] {
            assert_eq!(state.as_str(), expected);
        }
        assert_eq!(
            CaptureState::of(true, true),
            CaptureState::Capturing,
            "a live run is capturing even if a stale fault is still latched"
        );
        assert_eq!(CaptureState::of(false, true), CaptureState::Fault);
        assert_eq!(CaptureState::of(false, false), CaptureState::Idle);
    }

    #[test]
    fn each_record_gui_action_call_emits_exactly_one_line() {
        let dir = temp_dir("gui-action");
        let _sink = install_test_sink(dir.clone());
        let cases = [
            (
                GuiActionKind::Optimize,
                ActionSource::Button,
                CaptureState::Idle,
                CaptureState::Capturing,
                "StartBoost",
            ),
            (
                GuiActionKind::Optimize,
                ActionSource::Auto,
                CaptureState::Idle,
                CaptureState::Capturing,
                "StartBoost",
            ),
            (
                GuiActionKind::Stop,
                ActionSource::Tray,
                CaptureState::Capturing,
                CaptureState::Idle,
                "StopBoost",
            ),
            (
                GuiActionKind::Restart,
                ActionSource::Button,
                CaptureState::Capturing,
                CaptureState::Capturing,
                "StartBoost",
            ),
            (
                GuiActionKind::Quit,
                ActionSource::System,
                CaptureState::Capturing,
                CaptureState::Shutdown,
                "quit",
            ),
            (
                GuiActionKind::UpdateInstall,
                ActionSource::System,
                CaptureState::Capturing,
                CaptureState::Shutdown,
                "update",
            ),
            (
                GuiActionKind::ElevationHandoff,
                ActionSource::System,
                CaptureState::Capturing,
                CaptureState::Shutdown,
                "elevation_handoff",
            ),
            // A refused start: exactly one record, and the untouched transition
            // (from == to) is what proves no teardown happened.
            (
                GuiActionKind::Optimize,
                ActionSource::Button,
                CaptureState::Capturing,
                CaptureState::Capturing,
                "StartBoost",
            ),
        ];
        for (kind, source, from, to, command_id) in cases {
            record_gui_action(&GuiAction {
                action: kind,
                source,
                from,
                to,
                command_id,
            });
        }
        let events: Vec<Value> = read_lines(&dir)
            .into_iter()
            .filter(|event| event["event"] == "gui.action")
            .collect();
        assert_eq!(
            events.len(),
            cases.len(),
            "one record per invocation, including the refused start"
        );
        for (event, (kind, source, from, to, command_id)) in events.iter().zip(cases) {
            assert_eq!(event["action"], kind.as_str());
            assert_eq!(event["source"], source.as_str());
            assert_eq!(event["from"], from.as_str());
            assert_eq!(event["to"], to.as_str());
            assert_eq!(event["command_id"], command_id);
        }
    }
}
