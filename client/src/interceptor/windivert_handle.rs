//! Owned WinDivert handle with explicit, idempotent teardown, a process-wide
//! driver pin, and per-handle lifecycle instrumentation.
//!
//! ## Why the raw handle is owned here
//!
//! The `windivert` 0.6 wrapper keeps its `handle` field private and its
//! `shutdown`/`close` methods take `&mut self`. That makes teardown impossible
//! while another thread is blocked in `recv(&self)`: the only path left is
//! `Arc::try_unwrap`, which silently no-ops while the receive thread still
//! holds a clone, and the receive thread never releases its clone because
//! `recv` blocks forever. The leaked WFP state then makes the next launch fail
//! with `FWP_E_IN_USE` (0x8032000A).
//!
//! This module owns the raw `HANDLE` directly instead. Every operation takes
//! `&self` (the WinDivert FFI accepts the handle by value), so the handle can
//! live in an `Arc` while `shutdown(&self)` unblocks a concurrent blocking
//! `recv(&self)`. No `&mut` aliasing, no `try_unwrap`, no leak.
//!
//! ## The pinned driver (1.12.5 recovery design)
//!
//! [`pin_driver`] opens ONE permanent non-diverting handle - filter `"false"`,
//! normal (not sniff, not send-only) flags - before the first diverting handle
//! and keeps it open for the app lifetime. It keeps the WinDivert driver
//! **referenced** across a normal Stop that closes the intercept/inject/shadow
//! handles. It is not a WFP-state guarantee: the pin owns no WFP object, and the
//! 2026-11-23 field trace has the pin active while the next intercept open still
//! fails `FWP_E_IN_USE`. In the observed environment WinDivert 2.2 keeps the
//! driver loaded after the last close either way, so there is no unload/reload
//! churn for the pin to remove. The pin is closed only by [`release_driver_pin`]
//! (the explicit app-shutdown call, after the diverting handles are closed) or
//! by process exit. It is idempotent and serialized: a second [`pin_driver`]
//! call while a pin is held is a no-op that opens no second handle. This is a
//! *pin*, not a pause gate: the diverting handles are still created and closed
//! by the normal interception lifecycle.
//!
//! ## Restart resilience (P0-2): R1 bounded retry, R2 one pin-cycle
//!
//! A failed diverting open is recovered through two automatic rungs before the
//! remedy is surfaced (roadmap D5):
//!
//! * **R1 - bounded retry.** `FWP_E_IN_USE` is retried with an exponential
//!   backoff (50 ms doubling, capped at 400 ms) for at most 6 attempts inside a
//!   5 s wall-clock budget. The retry runs **while the pin is held too**: a held
//!   pin is a device handle, not a WFP state lock, so it must never suppress the
//!   retry. The 1.12.5 policy required `!pin_active` and turned one transient
//!   into a terminal failure with reboot advice.
//! * **R2 - one pin-cycle per failure episode.** When a diverting open still
//!   fails `FWP_E_IN_USE` after the R1 budget, the pin is released and reopened
//!   once and the diverting open is retried once. The pin's own open never cycles
//!   itself. Attempt ids/numbers and the changed pin id are all in the trace, so
//!   the accounting is visible without a second event schema.
//!
//! R3 (an explicit "Reset interception driver" action) is deliberately **not**
//! implemented and **not** automatic: stopping the shared WinDivert service is an
//! owner decision (OD-1) that is still open. R4 (a full shutdown/reboot) is the
//! last-resort copy, never the first-line remedy. Every retry is bounded by
//! attempts *and* wall clock; the pure `OpenRecovery` state machine owns the
//! decision and is unit-tested with no WinDivert call.
//!
//! ## Instrumentation
//!
//! Every logical open is assigned one monotonic `id` (1, 2, 3, ... per process)
//! and logs one line; the same id is used by the close line:
//!
//! ```text
//! WinDivert handle open: id=3 kind=intercept attempt=1 layer=Network priority=0 flags=0x0000 filter="udp and outbound ..." -> ok
//! WinDivert handle close: id=3 kind=intercept -> ok
//! ```
//!
//! A failed attempt ends its open line with `-> FAILED (<os error>)`; a retried
//! `FWP_E_IN_USE` attempt logs an extra
//! `WinDivert handle open retry: ...` line between attempts. `flags` is the raw
//! WinDivert flag word (`0x0000` normal, `0x0001` sniff, `0x0008` send-only).
//! Only handle metadata is logged - never packet payloads or process data.
//! Process-wide per-kind counters are exposed by [`windivert_handle_stats`].
//!
//! ## Typed lifecycle events (P0-1, behaviour-neutral)
//!
//! In addition to the prose lines above, every open attempt emits one
//! `windivert_open` record and every close one `windivert_close` record through
//! [`crate::app::lifecycle`]. The record carries the id, the role, the attempt,
//! the intent (`reason`/`actor`/`command_id`), whether the pin is held and its
//! id, how long the attempt took, the process-wide gap since the last successful
//! close, the OS status as `0xNNNNNNNN`, and the handles currently believed
//! live (`intercept:2,inject:3`). The same sink is a no-op until a front end
//! initialises it, so the CLI pays nothing; no product decision reads a record.
//!
//! Two details are deliberate:
//!
//! * A retried open keeps its id, so the attempt sequence pairs with the one
//!   close that eventually follows (or with no close, for a failed open).
//! * The pin's own open record reports `pin_active=false` / `pin_id=None`: the
//!   record is emitted when the open returns, before the pin slot stores the
//!   handle, and the events are observations, not predictions. The diverting
//!   handles opened after the pin do carry `pin_active=true` and the pin id.
//!
//! On a failed *diverting* open the failure record's `snapshot_id` names a
//! rate-limited support bundle written by the same sink (identity, `sc query
//! WinDivert`, the recent event ring, handle counters). The shipped failure
//! prose line is extended with `snapshot_id=...`; its error text and the
//! caller's remedy text are unchanged. The pin's own failure does not write a
//! bundle: it is a different episode and bundling it first would consume the
//! sink's rate limit before the session handle's failure could be captured.
//!
//! Windows-only: declared behind
//! `#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]` in
//! `interceptor::mod`, and every dependency it uses is gated with it.

use std::collections::BTreeMap;
use std::ffi::{c_void, CString};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use windivert_sys::address::WINDIVERT_ADDRESS;
use windivert_sys::{
    WinDivertClose, WinDivertFlags, WinDivertLayer, WinDivertOpen, WinDivertRecv, WinDivertSend,
    WinDivertShutdown, WinDivertShutdownMode,
};

use crate::app::lifecycle;

/// The handle type used by `windivert-sys` 0.11.
///
/// It is no longer the `windows` crate's `HANDLE`: 0.11 dropped that dependency
/// entirely (`WinDivertOpen` returns the raw OS handle, the other calls take a
/// `*mut c_void`), so this crate no longer needs to keep its `windows` version
/// in lockstep with windivert's - which is what blocked the windows 0.62 upgrade.
type WinDivertHandle = *mut core::ffi::c_void;

/// `windivert-sys` 0.11 models `BOOL` as a plain `c_int`, so a success test is a
/// comparison rather than the `windows::BOOL::as_bool` helper.
fn succeeded(ok: core::ffi::c_int) -> bool {
    ok != 0
}

// ─────────────────────────────────────────────────────────────────────────────
//  Handle roles and lifecycle instrumentation
// ─────────────────────────────────────────────────────────────────────────────

/// The role a WinDivert handle plays in the backend.
///
/// The kind is what makes the lifecycle counters and log lines attributable:
/// an evidence run can count the permanent pin apart from the per-session
/// intercept/inject/shadow handles, and the legacy `capture::windivert_redirect`
/// backend's handles stay out of the interception counters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HandleKind {
    /// The permanent non-diverting driver pin: filter `"false"`, normal flags.
    Pin,
    /// The one static outbound capture (diverting) handle.
    Intercept,
    /// The inbound spoof injection handle: filter `"false"`, normal flags.
    Inject,
    /// The optional sniff-only shadow-direct inbound handle.
    Shadow,
    /// A handle opened by the legacy `capture::windivert_redirect` backend.
    Legacy,
}

impl HandleKind {
    /// Number of kinds: the length of the counter table.
    const COUNT: usize = 5;

    /// Stable lowercase name used in logs and counter lookups.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pin => "pin",
            Self::Intercept => "intercept",
            Self::Inject => "inject",
            Self::Shadow => "shadow",
            Self::Legacy => "legacy",
        }
    }

    /// Index into the per-kind counter table.
    fn index(self) -> usize {
        match self {
            Self::Pin => 0,
            Self::Intercept => 1,
            Self::Inject => 2,
            Self::Shadow => 3,
            Self::Legacy => 4,
        }
    }

    /// Whether this is a diverting session handle rather than the permanent pin.
    ///
    /// "Diverting handles" is the field trace's term for the handles a session
    /// opens and closes (intercept, inject, shadow, and the legacy redirect
    /// backend's handle). The pin is the one handle opened for the whole app
    /// lifetime with the `"false"` filter; a failed pin open is a different
    /// failure episode from a session handle's stale-WFP open, so the support
    /// bundle is attached to the session handles only.
    fn is_diverting(self) -> bool {
        !matches!(self, Self::Pin)
    }
}

/// A snapshot of the lifecycle counters for one [`HandleKind`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct HandleCounts {
    /// Logical opens attempted (including every bounded `FWP_E_IN_USE` retry).
    pub open_attempts: u64,
    /// Opens that returned a live handle.
    pub open_successes: u64,
    /// Opens that failed (including the final attempt of a retried open).
    pub open_failures: u64,
    /// Close calls that reached `WinDivertClose` (a repeated `close` is a no-op
    /// and is not counted again).
    pub close_attempts: u64,
    /// Closes that `WinDivertClose` accepted.
    pub close_successes: u64,
    /// Closes that `WinDivertClose` rejected.
    pub close_failures: u64,
    /// Handles of this kind currently believed open (successful opens minus
    /// close attempts; a failed close still stops the handle from being tracked
    /// because it is never retried against a possibly recycled value).
    pub live: u64,
}

/// Process-wide WinDivert lifecycle counters, per [`HandleKind`].
///
/// This is the status/test surface for handle instrumention: a test or an
/// evidence harness can take a snapshot and assert deltas without touching the
/// driver (`cargo test` with no WinDivert installed still gets the counters
/// through the recording helpers). The per-kind totals are what proves, for
/// example, that a normal Stop leaves `pin.live == 1` while
/// `intercept.live == inject.live == shadow.live == 0`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct WindivertHandleStats {
    pub pin: HandleCounts,
    pub intercept: HandleCounts,
    pub inject: HandleCounts,
    pub shadow: HandleCounts,
    pub legacy: HandleCounts,
}

impl WindivertHandleStats {
    /// The counters for one kind, as a copy.
    pub fn by_kind(self, kind: HandleKind) -> HandleCounts {
        match kind {
            HandleKind::Pin => self.pin,
            HandleKind::Intercept => self.intercept,
            HandleKind::Inject => self.inject,
            HandleKind::Shadow => self.shadow,
            HandleKind::Legacy => self.legacy,
        }
    }

    /// Every handle currently believed open, across kinds.
    pub fn live_handles(self) -> u64 {
        self.pin.live + self.intercept.live + self.inject.live + self.shadow.live + self.legacy.live
    }
}

/// The atomic form of [`HandleCounts`]. One static per kind.
#[derive(Debug, Default)]
struct AtomicCounts {
    open_attempts: AtomicU64,
    open_successes: AtomicU64,
    open_failures: AtomicU64,
    close_attempts: AtomicU64,
    close_successes: AtomicU64,
    close_failures: AtomicU64,
    live: AtomicI64,
}

impl AtomicCounts {
    const fn new() -> Self {
        Self {
            open_attempts: AtomicU64::new(0),
            open_successes: AtomicU64::new(0),
            open_failures: AtomicU64::new(0),
            close_attempts: AtomicU64::new(0),
            close_successes: AtomicU64::new(0),
            close_failures: AtomicU64::new(0),
            live: AtomicI64::new(0),
        }
    }

    fn snapshot(&self) -> HandleCounts {
        HandleCounts {
            open_attempts: self.open_attempts.load(Ordering::Relaxed),
            open_successes: self.open_successes.load(Ordering::Relaxed),
            open_failures: self.open_failures.load(Ordering::Relaxed),
            close_attempts: self.close_attempts.load(Ordering::Relaxed),
            close_successes: self.close_successes.load(Ordering::Relaxed),
            close_failures: self.close_failures.load(Ordering::Relaxed),
            live: self.live.load(Ordering::Relaxed).max(0) as u64,
        }
    }
}

/// The per-kind counter table, indexed by [`HandleKind::index`].
static COUNTERS: [AtomicCounts; HandleKind::COUNT] = [
    AtomicCounts::new(),
    AtomicCounts::new(),
    AtomicCounts::new(),
    AtomicCounts::new(),
    AtomicCounts::new(),
];

/// Monotonic id source for the instrumentation. Starts at 1 per process.
static NEXT_HANDLE_ID: AtomicU64 = AtomicU64::new(1);

/// Allocate the id for one logical open (a retried open keeps its id).
fn next_handle_id() -> u64 {
    NEXT_HANDLE_ID.fetch_add(1, Ordering::Relaxed)
}

fn counts(kind: HandleKind) -> &'static AtomicCounts {
    &COUNTERS[kind.index()]
}

fn record_open_attempt(kind: HandleKind) {
    counts(kind).open_attempts.fetch_add(1, Ordering::Relaxed);
}

fn record_open_success(kind: HandleKind) {
    counts(kind).open_successes.fetch_add(1, Ordering::Relaxed);
    counts(kind).live.fetch_add(1, Ordering::Relaxed);
}

fn record_open_failure(kind: HandleKind) {
    counts(kind).open_failures.fetch_add(1, Ordering::Relaxed);
}

fn record_close(kind: HandleKind, succeeded_in_driver: bool) {
    let counters = counts(kind);
    counters.close_attempts.fetch_add(1, Ordering::Relaxed);
    if succeeded_in_driver {
        counters.close_successes.fetch_add(1, Ordering::Relaxed);
    } else {
        counters.close_failures.fetch_add(1, Ordering::Relaxed);
    }
    // The `closed` latch means a handle is never tracked live again after the
    // close attempt, even when the driver rejects it (retrying a possibly
    // recycled value is worse than one leaked handle at process exit).
    counters.live.fetch_sub(1, Ordering::Relaxed);
}

/// A process-wide snapshot of the WinDivert handle lifecycle counters.
///
/// This is the small status/test surface for instrumentation evidence: it is
/// cheap, lock-free and safe to call at any time from any thread.
pub fn windivert_handle_stats() -> WindivertHandleStats {
    WindivertHandleStats {
        pin: COUNTERS[HandleKind::Pin.index()].snapshot(),
        intercept: COUNTERS[HandleKind::Intercept.index()].snapshot(),
        inject: COUNTERS[HandleKind::Inject.index()].snapshot(),
        shadow: COUNTERS[HandleKind::Shadow.index()].snapshot(),
        legacy: COUNTERS[HandleKind::Legacy.index()].snapshot(),
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Lifecycle event bridge (P0-1b: `windivert_open` / `windivert_close`)
// ─────────────────────────────────────────────────────────────────────────────
//
// Everything below is a pure logging side effect. The sink (`crate::app::
// lifecycle`) is a no-op until a front end initialises it, so the CLI pays
// nothing and no decision in this module - retry policy, pin semantics,
// teardown order, error text, counters - reads any of it. The helpers that need
// no driver are unit-tested below so the schema and the age/fwp math are pinned
// on a host with no WinDivert installed.

/// The process-wide instant of the last successful handle close.
///
/// `Some` after the first successful close of this process (the pin release
/// included); `None` before it. The next `windivert_open` record carries the gap
/// as `ms_since_last_close`, which is what makes a fast Stop→Optimize
/// attributable from the open record alone instead of from parsed prose.
static LAST_SUCCESSFUL_CLOSE: Mutex<Option<Instant>> = Mutex::new(None);

/// The id of the handle currently held in the pin slot (`0` = no pin held).
///
/// An atomic rather than a read of the slot because a pin *open* runs while
/// [`PinSlot::ensure_open`] holds the slot lock; the open record cannot take that
/// lock again without deadlocking.
static PIN_HANDLE_ID: AtomicU64 = AtomicU64::new(0);

/// The process-wide table of handles believed live, keyed by their monotonic id.
///
/// A `BTreeMap` so the rendered order is deterministic (ascending id) and two
/// runs produce comparable traces. A close *attempt* removes the entry - the
/// same rule the per-kind `live` counter uses for a failed close.
#[derive(Debug, Clone, Default)]
struct LiveHandleTable {
    inner: BTreeMap<u64, HandleKind>,
}

impl LiveHandleTable {
    const fn new() -> Self {
        Self {
            inner: BTreeMap::new(),
        }
    }

    fn insert(&mut self, id: u64, kind: HandleKind) {
        self.inner.insert(id, kind);
    }

    fn forget(&mut self, id: u64) {
        self.inner.remove(&id);
    }

    /// `intercept:2,inject:3` - the `owned_handles` event field.
    fn render(&self) -> String {
        self.inner
            .iter()
            .map(|(id, kind)| format!("{}:{id}", kind.as_str()))
            .collect::<Vec<_>>()
            .join(",")
    }
}

static LIVE_HANDLES: Mutex<LiveHandleTable> = Mutex::new(LiveHandleTable::new());

fn lock_last_close() -> std::sync::MutexGuard<'static, Option<Instant>> {
    LAST_SUCCESSFUL_CLOSE
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn lock_live_handles() -> std::sync::MutexGuard<'static, LiveHandleTable> {
    LIVE_HANDLES
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Milliseconds from `opened` to `now`, saturating at 0.
///
/// The saturation matters because a `close` can be observed after the instant
/// captured at open only in tests/monotonic-clock oddities; the record must
/// never show a negative age as a huge wrap.
fn age_ms(now: Instant, opened: Instant) -> u64 {
    now.saturating_duration_since(opened).as_millis() as u64
}

/// Microseconds from `start` to `now`, saturating at 0.
fn elapsed_us(now: Instant, start: Instant) -> u64 {
    now.saturating_duration_since(start).as_micros() as u64
}

/// The gap since the last successful close, `None` when there has been none.
fn gap_ms(now: Instant, last: Option<Instant>) -> Option<u64> {
    last.map(|last| age_ms(now, last))
}

/// The WinDivert/WFP status as `0x8032000A`, `None` when the error carries no
/// OS code (for example the interior-NUL filter validation, which never reaches
/// the driver).
fn fwp_code(err: &io::Error) -> Option<String> {
    err.raw_os_error()
        .map(|code| format!("0x{:08X}", code as u32))
}

/// The closed-vocabulary name for a handle role in the lifecycle schema.
fn lifecycle_kind(kind: HandleKind) -> lifecycle::HandleKindName {
    match kind {
        HandleKind::Pin => lifecycle::HandleKindName::Pin,
        HandleKind::Intercept => lifecycle::HandleKindName::Intercept,
        HandleKind::Inject => lifecycle::HandleKindName::Inject,
        HandleKind::Shadow => lifecycle::HandleKindName::Shadow,
        HandleKind::Legacy => lifecycle::HandleKindName::Legacy,
    }
}

/// Record a successful close as the process-wide close→open anchor.
fn record_last_close_success(now: Instant) {
    *lock_last_close() = Some(now);
}

/// Milliseconds since the last successful handle close in this process.
///
/// `None` before the first successful close (a cold start at process launch).
/// Exposed crate-wide so the engine record can carry the close→open age without
/// parsing the prose trace.
pub(crate) fn ms_since_last_close() -> Option<u64> {
    gap_ms(Instant::now(), *lock_last_close())
}

/// The id of the pinned driver handle, or `None` while no pin is held.
pub(crate) fn driver_pin_id() -> Option<u64> {
    let raw = PIN_HANDLE_ID.load(Ordering::Acquire);
    if raw == 0 {
        None
    } else {
        Some(raw)
    }
}

fn record_live_handle(id: u64, kind: HandleKind) {
    lock_live_handles().insert(id, kind);
}

fn forget_live_handle(id: u64) {
    lock_live_handles().forget(id);
}

/// Every handle believed open in this process, rendered `intercept:2,inject:3`.
fn current_owned_handles() -> String {
    lock_live_handles().render()
}

/// Every handle believed open in this process, same rendering as the event
/// field. P0-1 accessor for the engine record.
pub(crate) fn live_handle_ids() -> String {
    current_owned_handles()
}

/// One kind's counters as a JSON object (support-bundle payload).
fn counts_json(counts: &HandleCounts) -> Value {
    json!({
        "open_attempts": counts.open_attempts,
        "open_successes": counts.open_successes,
        "open_failures": counts.open_failures,
        "close_attempts": counts.close_attempts,
        "close_successes": counts.close_successes,
        "close_failures": counts.close_failures,
        "live": counts.live,
    })
}

/// The whole counter table as a JSON object (support-bundle payload).
fn handle_stats_json(stats: &WindivertHandleStats) -> Value {
    json!({
        "pin": counts_json(&stats.pin),
        "intercept": counts_json(&stats.intercept),
        "inject": counts_json(&stats.inject),
        "shadow": counts_json(&stats.shadow),
        "legacy": counts_json(&stats.legacy),
        "live_handles": stats.live_handles(),
    })
}

/// The fields one `windivert_open` record is built from.
///
/// A parameter struct keeps the construction testable without a driver and keeps
/// the schema in one place instead of inline in the FFI wrapper.
struct OpenEventParams<'a> {
    id: u64,
    kind: HandleKind,
    attempt: u32,
    layer: WinDivertLayer,
    priority: i16,
    flags_word: u64,
    filter: &'a str,
    intent: lifecycle::Intent,
    pin_active: bool,
    pin_id: Option<u64>,
    elapsed_us: u64,
    ms_since_last_close: Option<u64>,
    fwp_code: Option<String>,
    owned_handles: String,
    result: lifecycle::OpenResult,
    snapshot_id: Option<String>,
}

/// Build the typed `windivert_open` record (no driver, no sink access).
fn build_open_event(params: OpenEventParams<'_>) -> lifecycle::HandleOpen {
    lifecycle::HandleOpen {
        id: params.id,
        kind: lifecycle_kind(params.kind),
        attempt: params.attempt,
        layer: format!("{:?}", params.layer),
        priority: params.priority,
        flags: format!("0x{:04x}", params.flags_word),
        filter: params.filter.to_string(),
        reason: params.intent.reason,
        actor: params.intent.actor,
        command_id: params.intent.command_id,
        pin_active: params.pin_active,
        pin_id: params.pin_id,
        elapsed_us: params.elapsed_us,
        ms_since_last_close: params.ms_since_last_close,
        fwp_code: params.fwp_code,
        owned_handles: params.owned_handles,
        result: params.result,
        snapshot_id: params.snapshot_id,
    }
}

/// Build the typed `windivert_close` record (no driver, no sink access).
fn build_close_event(
    id: u64,
    kind: HandleKind,
    close_age_ms: u64,
    result: lifecycle::CloseResult,
    pin_active: bool,
    owned_handles: String,
) -> lifecycle::HandleClose {
    lifecycle::HandleClose {
        id,
        kind: lifecycle_kind(kind),
        close_age_ms,
        result,
        pin_active,
        owned_handles,
    }
}

/// The context one open-failure support bundle is built from.
struct FailureBundleContext<'a> {
    id: u64,
    kind: HandleKind,
    attempt: u32,
    layer: WinDivertLayer,
    priority: i16,
    flags_word: u64,
    filter: &'a str,
    err: &'a io::Error,
    fwp: Option<&'a str>,
    pin_active: bool,
    pin_id: Option<u64>,
    ms_since_last_close: Option<u64>,
    owned_handles: &'a str,
}

/// Write a rate-limited support bundle for a failed diverting open.
///
/// The pin's own failure is skipped: it is a different episode (non-diverting,
/// opened once per process) and bundling it first would consume the 60 s rate
/// limit and suppress the session handle's bundle - the one that captures the
/// WFP/service state the `FWP_E_IN_USE` investigation needs. Read-only, and the
/// sink's own rate limit applies on top; the return value is the `snapshot_id`
/// the failure record and prose line carry.
fn open_failure_bundle(context: FailureBundleContext<'_>) -> Option<String> {
    if !context.kind.is_diverting() {
        return None;
    }
    lifecycle::support_bundle(
        "windivert_open_failure",
        json!({
            "kind": context.kind.as_str(),
            "id": context.id,
            "attempt": context.attempt,
            "layer": format!("{:?}", context.layer),
            "priority": context.priority,
            "flags": format!("0x{:04x}", context.flags_word),
            "filter": context.filter,
            "fwp": context.fwp,
            "error": context.err.to_string(),
            "pin_active": context.pin_active,
            "pin_id": context.pin_id,
            "ms_since_last_close": context.ms_since_last_close,
            "owned_handles": context.owned_handles,
            "counters": handle_stats_json(&windivert_handle_stats()),
        }),
    )
}

// ─────────────────────────────────────────────────────────────────────────────
//  Open-recovery ladder: R1 bounded retry, R2 one pin-cycle
// ─────────────────────────────────────────────────────────────────────────────

/// Attempts in the R1 bounded-retry rung of one failed open episode, including
/// the first attempt.
///
/// Bounded on purpose: a stale WFP state that survives the budget is not a driver
/// path still winding down, and hiding it behind an endless retry would turn a
/// clear failure into a hang. The roadmap allows 4-6 attempts; 6 is the top of
/// that range and the backoff schedule below stays tiny (1.15 s of sleep total).
const FWP_IN_USE_MAX_ATTEMPTS: u32 = 6;

/// Wall-clock budget for the R1 bounded-retry rung.
///
/// The backoff schedule alone is 1.15 s, so this leaves room for slow
/// `WinDivertOpen` round-trips. It is checked *before* each retry, so a slow call
/// can never schedule a retry that lands past the budget; one in-flight open call
/// may still finish after it, which is why the copy says "about" 5 s.
const FWP_IN_USE_RETRY_BUDGET: Duration = Duration::from_millis(5_000);

/// Backoff before the first R1 retry. Doubles per retry up to the cap.
const FWP_IN_USE_BACKOFF_INITIAL: Duration = Duration::from_millis(50);

/// Cap on the exponential R1 backoff (the roadmap allows 350-500 ms).
const FWP_IN_USE_BACKOFF_CAP: Duration = Duration::from_millis(400);

/// The exponential backoff before R1 retry `retry_index` (0-based), or `None`
/// when the R1 attempt budget is spent.
///
/// The sequence is 50 / 100 / 200 / 400 / 400 ms for the five retries that fit
/// the six-attempt ceiling.
fn fwp_in_use_backoff(retry_index: u32) -> Option<Duration> {
    if retry_index + 1 >= FWP_IN_USE_MAX_ATTEMPTS {
        return None;
    }
    let factor = 1u32.checked_shl(retry_index).unwrap_or(u32::MAX);
    Some(
        FWP_IN_USE_BACKOFF_INITIAL
            .saturating_mul(factor.max(1))
            .min(FWP_IN_USE_BACKOFF_CAP),
    )
}

/// Whether one failed open attempt is worth retrying.
///
/// Only `FWP_E_IN_USE` qualifies; every other failure (driver missing, blocked,
/// not elevated, invalid filter) is permanent and is never retried. The
/// `_pin_active` parameter is accepted and deliberately **not** consulted: a held
/// pin is a device handle, not a WFP state lock, so it must never suppress the
/// bounded retry (roadmap D1). Keeping the parameter makes the pinned path's
/// retry explicit at the call site and in the evidence test, rather than silently
/// unobservable.
fn should_retry_fwp_in_use(err: &io::Error, _pin_active: bool) -> bool {
    is_fwp_in_use(err)
}

/// What the pure recovery policy decided to do after a failed attempt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RecoveryStep {
    /// R1: sleep `backoff`, then try the same open again.
    Retry { backoff: Duration },
    /// R2: release and reopen the driver pin once, then try the open again.
    PinCycle,
    /// Stop retrying and return the last error to the caller.
    GiveUp,
}

/// The pure, unit-testable recovery state for ONE logical open attempt sequence
/// (one "failure episode").
///
/// No driver call lives in this type: it consumes the failed attempt's
/// `io::Error`, the handle role, the pin state and a clock reading, and returns
/// the next step. That is what makes the attempt/wall-clock budget provable in a
/// test without touching WinDivert or WFP (this host's kernel is dirty, so no
/// real-driver run is allowed). Production wiring for [`RecoveryStep::PinCycle`]
/// lives in [`OwnedHandle::open_with_pin_retry`].
#[derive(Debug, Clone, Copy)]
struct OpenRecovery {
    /// Clock reading when the logical open started; the wall-clock budget anchor.
    started: Instant,
    /// Attempts started so far in this episode (the first attempt counts as 1).
    attempts: u32,
    /// Whether the single R2 pin-cycle rung has been used.
    pin_cycled: bool,
}

impl OpenRecovery {
    /// Start a fresh recovery episode anchored at `started` (tests pass a fake
    /// clock; production passes `Instant::now()`).
    fn new(started: Instant) -> Self {
        Self {
            started,
            attempts: 0,
            pin_cycled: false,
        }
    }

    /// Count the attempt that is about to happen and return its 1-based number.
    fn begin_attempt(&mut self) -> u32 {
        self.attempts = self.attempts.saturating_add(1);
        self.attempts
    }

    /// How many attempts this episode has started.
    fn attempts(&self) -> u32 {
        self.attempts
    }

    /// The hard ceiling on attempts for one episode: the R1 retry budget plus the
    /// single R2 retry. It is a constant so a test can prove the loop cannot grow.
    fn max_attempts() -> u32 {
        FWP_IN_USE_MAX_ATTEMPTS + 1
    }

    /// Decide the next step after one attempt failed.
    ///
    /// The order is the recovery ladder:
    /// 1. a non-`FWP_E_IN_USE` failure is permanent (`GiveUp`);
    /// 2. R1 while both the attempt budget and the wall-clock budget allow it
    ///    (pinned or not - the pin never gates the retry);
    /// 3. R2 once per episode, for a diverting handle with the pin held;
    /// 4. otherwise `GiveUp`.
    fn step_after_failure(
        &mut self,
        err: &io::Error,
        kind: HandleKind,
        pin_active: bool,
        now: Instant,
    ) -> RecoveryStep {
        if !should_retry_fwp_in_use(err, pin_active) {
            return RecoveryStep::GiveUp;
        }
        let elapsed = now.saturating_duration_since(self.started);
        if let Some(backoff) = fwp_in_use_backoff(self.attempts.saturating_sub(1)) {
            let fits_budget = elapsed
                .checked_add(backoff)
                .map(|total| total <= FWP_IN_USE_RETRY_BUDGET)
                .unwrap_or(false);
            if fits_budget {
                return RecoveryStep::Retry { backoff };
            }
        }
        // R2: one pin-cycle per episode, diverting handles only. The pin's own
        // open runs while the pin-slot lock is held, so cycling it from here
        // would deadlock and is never eligible (`HandleKind::Pin` is not
        // diverting).
        if !self.pin_cycled && kind.is_diverting() && pin_active {
            self.pin_cycled = true;
            return RecoveryStep::PinCycle;
        }
        RecoveryStep::GiveUp
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Driver pin: one permanent non-diverting handle for the app lifetime
// ─────────────────────────────────────────────────────────────────────────────

/// The filter the pin handle is opened with: it matches nothing, so the handle
/// diverts no traffic. It exists only to keep the driver/service loaded.
const PIN_FILTER: &str = "false";

/// A serialized slot holding at most one handle.
///
/// `active` is a separate atomic so the retry decision and the public
/// [`driver_pin_active`] status can read it without taking the slot lock.
struct PinSlot<H> {
    handle: Mutex<Option<H>>,
    active: AtomicBool,
}

impl<H> PinSlot<H> {
    const fn new() -> Self {
        Self {
            handle: Mutex::new(None),
            active: AtomicBool::new(false),
        }
    }

    /// Whether a handle is currently stored.
    fn active(&self) -> bool {
        self.active.load(Ordering::Acquire)
    }

    /// Store a handle unless one is already stored.
    ///
    /// Returns `Ok(true)` when this call stored a new handle and `Ok(false)`
    /// when one was already present (the opener is then not called, which is
    /// what makes the pin idempotent). The lock is held across `open`, so
    /// concurrent callers cannot race two handles into existence; a failed
    /// open stores nothing, so a later call may retry.
    fn ensure_open(&self, open: impl FnOnce() -> io::Result<H>) -> io::Result<bool> {
        let mut guard = self.lock();
        if guard.is_some() {
            return Ok(false);
        }
        let handle = open()?;
        *guard = Some(handle);
        self.active.store(true, Ordering::Release);
        Ok(true)
    }

    /// Remove and return the stored handle, if any.
    fn take(&self) -> Option<H> {
        let mut guard = self.lock();
        let handle = guard.take();
        if handle.is_some() {
            self.active.store(false, Ordering::Release);
        }
        handle
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Option<H>> {
        // A panic while holding this lock cannot leave a half-written handle:
        // the only mutation is the store/remove of an owned value. Recovering
        // the inner guard keeps the pin usable after an unrelated panic instead
        // of poisoning the process-wide state (same policy as `engine`).
        self.handle
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// The process-wide pinned driver handle. `None` until [`pin_driver`] opens it;
/// the handle is removed (and closed) only by [`release_driver_pin`].
static PIN: PinSlot<OwnedHandle> = PinSlot::new();

/// The id of the handle currently stored in the pin slot, if any.
///
/// Takes the slot lock briefly, so it must only be called when the caller does
/// not already hold it (never from inside [`PinSlot::ensure_open`]'s opener).
fn pin_slot_handle_id() -> Option<u64> {
    PIN.lock().as_ref().map(OwnedHandle::id)
}

/// Open the permanent non-diverting handle that keeps the WinDivert driver
/// loaded.
///
/// Idempotent: when a pin is already held this returns `Ok(())` without opening
/// a second handle, so the engine may call it on every Optimize. The handle is
/// opened with [`PIN_FILTER`] (`"false"`), the network layer, priority 0 and
/// normal (non-sniff, non-send-only) flags, and it is opened before the first
/// diverting handle.
///
/// # Errors
///
/// Returns the actionable remedy as a `String` when the driver cannot be
/// reached (not elevated, driver missing/blocked, or a persistent stale WFP
/// state). A `FWP_E_IN_USE` failure is retried with the bounded R1 budget first
/// (the pin's own open has no R2 pin-cycle rung); a state that survives the
/// budget is reported, never hidden.
pub(crate) fn pin_driver() -> Result<(), String> {
    let attempts_before = windivert_handle_stats().pin.open_attempts;
    let result = PIN.ensure_open(|| {
        OwnedHandle::open_with_pin_retry(
            HandleKind::Pin,
            PIN_FILTER,
            WinDivertLayer::Network,
            0,
            WinDivertFlags::new(),
        )
    });
    match result {
        Ok(true) => {
            // The slot now holds the pin; publish its id for the open records
            // of the diverting handles that follow. Logging only.
            PIN_HANDLE_ID.store(pin_slot_handle_id().unwrap_or(0), Ordering::Release);
            tracing::info!(
                "📌 WinDivert driver pinned (filter={PIN_FILTER:?}, normal flags); the \
                 driver stays loaded until an explicit release"
            );
            Ok(())
        }
        Ok(false) => {
            tracing::debug!("📌 WinDivert driver pin already held; no second handle opened");
            Ok(())
        }
        Err(err) => {
            let attempts = windivert_handle_stats()
                .pin
                .open_attempts
                .saturating_sub(attempts_before);
            let mut message = driver_open_remedy(&err);
            if attempts > 1 {
                message.push_str(&format!(
                    " The pin open was attempted {attempts} times with bounded backoff; \
                     the stale driver state survived the budget."
                ));
            }
            tracing::warn!(
                "❌ WinDivert driver pin open failed after {attempts} attempt(s): {message}"
            );
            Err(message)
        }
    }
}

/// Close the pinned driver handle. Idempotent; a no-op when no pin is held.
///
/// This is the explicit app-shutdown release: call it AFTER the last
/// interceptor session has stopped (the normal Stop path closes the diverting
/// handles and deliberately leaves the pin open). Process exit also releases it.
pub(crate) fn release_driver_pin() {
    let Some(handle) = PIN.take() else {
        tracing::debug!("📌 WinDivert driver pin release ignored: no pin is held");
        return;
    };
    // The slot no longer holds a pin, so the next open record must not name this
    // id as the live pin. Cleared before the close so the release's own close
    // record observes `pin_active=false`, matching the slot.
    PIN_HANDLE_ID.store(0, Ordering::Release);
    let id = handle.id();
    match handle.close() {
        Ok(()) => {
            tracing::info!(
                "📌 WinDivert driver pin released (id={id}); the driver is no longer referenced \
                 by this process (WinDivert 2.2 keeps it loaded until service stop/reboot)"
            )
        }
        Err(err) => tracing::warn!(
            "📌 WinDivert driver pin close failed (id={id}): {err}; the driver is released \
             when the process exits"
        ),
    }
}

/// Whether the permanent pin handle is currently held.
pub(crate) fn driver_pin_active() -> bool {
    PIN.active()
}

// ─────────────────────────────────────────────────────────────────────────────
//  Open-failure remedies
// ─────────────────────────────────────────────────────────────────────────────

/// A user-facing remedy for a WinDivert open failure.
///
/// These are the failures where "try again" is not advice: each one names the
/// actual next action. The raw error is kept at the end so support logs remain
/// searchable.
///
/// The `FWP_E_IN_USE` branch is the restart-resilience ladder (roadmap D5,
/// P0-2): R1/R2 are the automatic rungs this module already runs, R3 is an
/// explicit user action that is deliberately **not** exposed automatically (owner
/// decision OD-1 is open), and R4 - a full shutdown/reboot - is the reasoned last
/// resort, never the headline. There is no "restart Windows" first-line advice.
pub(crate) fn driver_open_remedy(err: &io::Error) -> String {
    if is_fwp_in_use(err) {
        return format!(
            "WinDivert could not open the interception handle: a WFP object from \
             the previous capture session is still in use (FWP_E_IN_USE). This is a \
             Windows kernel-lifetime condition, not proof of a stale LightSpeed \
             copy. LightSpeed's recovery ladder, cheapest first: \
             R1 automatic bounded retry - up to {attempts} attempts with \
             exponential backoff over about {budget_secs} s, with the driver pin \
             held (a held pin never suppresses the retry); \
             R2 automatic pin-cycle - for a capture handle, LightSpeed releases \
             and reopens its driver pin once and retries the open one final time; \
             R3 reset interception driver - stopping the shared WinDivert driver \
             service is an explicit administrator action and LightSpeed does NOT \
             perform it automatically, because it would disrupt every other \
             WinDivert application on this machine and exposing it in the app is \
             still an open owner decision; \
             R4 full shutdown and restart of Windows - the last resort, and only \
             when an external WinDivert user cannot be excluded, because a reboot \
             clears every process's WFP state without identifying what held it. \
             ({err})",
            attempts = FWP_IN_USE_MAX_ATTEMPTS,
            budget_secs = FWP_IN_USE_RETRY_BUDGET.as_secs(),
        );
    }
    match err.raw_os_error() {
        Some(5) => format!(
            "WinDivert requires Administrator privileges. Restart LightSpeed as \
             administrator. ({err})"
        ),
        Some(577) => format!(
            "The WinDivert driver signature is not valid on this machine. Reinstall \
             LightSpeed from the official release or check security software. ({err})"
        ),
        Some(1275) => format!(
            "The WinDivert driver is blocked on this machine (driver blocklist, \
             security software, or virtualization). Allow WinDivert or use a supported \
             host. ({err})"
        ),
        _ => format!("WinDivert driver open failed: {err}"),
    }
}

/// An owned WinDivert handle with deterministic, idempotent teardown.
///
/// # Concurrency
///
/// `recv`, `send` and `shutdown` may be called concurrently from any thread;
/// the WinDivert driver supports that on a single handle. `shutdown` and
/// `close` are not internally serialised against each other: the owner thread
/// calls `shutdown`, waits for every receive thread to acknowledge
/// ([`crate::interceptor::teardown::TeardownAck`]), and only then calls
/// `close` (or drops the last clone). A `close` that races a `shutdown` could
/// run `WinDivertShutdown` against a handle value Windows has already recycled.
pub struct OwnedHandle {
    raw: WinDivertHandle,
    /// Set (once) when a close has been attempted, so `Drop` never closes
    /// twice. It records the attempt, not success: a failed close leaves it
    /// set so we do not retry against a possibly recycled handle value.
    closed: AtomicBool,
    /// Monotonic process-wide id assigned once per logical open. It is the same
    /// id in the open and close log lines, so an evidence run can pair them.
    id: u64,
    /// The role this handle plays; selects the counter bucket and log `kind`.
    kind: HandleKind,
    /// When this handle became live (right after `WinDivertOpen` returned it).
    /// The close record's `close_age_ms` is measured from here; instrumentation
    /// only, nothing branches on it.
    opened: Instant,
}

// SAFETY: `OwnedHandle` holds the WinDivert handle as an opaque value (a raw
// handle, not a Rust reference), and the only interior state is a
// thread-safe `AtomicBool`. The WinDivert FFI explicitly supports concurrent
// `recv`/`send`/`shutdown`/`close` calls on the same handle from different
// threads, so moving it between threads (`Send`) and sharing references across
// them (`Sync`) introduces no data race. [UB taxonomy category 9]
unsafe impl Send for OwnedHandle {}
// SAFETY: see the `Send` impl above. Every method takes `&self`; the only
// shared mutation is inside the driver, which serialises it internally.
unsafe impl Sync for OwnedHandle {}

impl OwnedHandle {
    /// `WinDivertOpen` returns this sentinel on failure, not a NULL handle.
    ///
    /// The API documents `INVALID_HANDLE_VALUE` (`-1`) as its failure return.
    /// Checking only NULL treats a failed open as live, discarding the real
    /// error and turning it into a later, unrelated invalid-handle receive.
    const INVALID_HANDLE_VALUE: isize = -1;

    /// Whether a raw handle from `WinDivertOpen` is a failure sentinel.
    fn is_open_failure(raw: WinDivertHandle) -> bool {
        raw.is_null() || raw as isize == Self::INVALID_HANDLE_VALUE
    }

    /// Open a WinDivert handle (legacy call shape).
    ///
    /// This is the entry point the legacy `capture::windivert_redirect` backend
    /// uses; it keeps its handle counted as [`HandleKind::Legacy`]. The
    /// interception backend opens through [`Self::open_with_pin_retry`] instead.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] when `filter` contains an
    /// interior NUL byte, and the OS error from `GetLastError` when
    /// `WinDivertOpen` fails (for example `FWP_E_IN_USE`; see
    /// [`is_fwp_in_use`]). Both failure sentinels (`NULL` and
    /// `INVALID_HANDLE_VALUE`) are rejected before the handle is stored.
    pub fn open(
        filter: &str,
        layer: WinDivertLayer,
        priority: i16,
        flags: WinDivertFlags,
    ) -> io::Result<Self> {
        Self::open_with_id(
            next_handle_id(),
            HandleKind::Legacy,
            1,
            filter,
            layer,
            priority,
            flags,
        )
    }

    /// Open a handle in a named role, instrumented but without the stale-state
    /// retry. Used by the test/status surface; production opens go through
    /// [`Self::open_with_pin_retry`].
    pub(crate) fn open_as(
        kind: HandleKind,
        filter: &str,
        layer: WinDivertLayer,
        priority: i16,
        flags: WinDivertFlags,
    ) -> io::Result<Self> {
        Self::open_with_id(next_handle_id(), kind, 1, filter, layer, priority, flags)
    }

    /// Open a handle with the R1 bounded retry and the R2 one-shot pin-cycle.
    ///
    /// One logical open gets one id across every attempt. A failed attempt that
    /// is `FWP_E_IN_USE` is recovered by the pure [`OpenRecovery`] policy: at
    /// most [`FWP_IN_USE_MAX_ATTEMPTS`] R1 attempts inside
    /// [`FWP_IN_USE_RETRY_BUDGET`], then - for a diverting handle with the pin
    /// held - exactly one R2 pin-cycle (release the pin, reopen it, retry once).
    /// The retry runs while the pin is held: the pin is a device handle, not a
    /// WFP lock. Every other failure is permanent and is returned immediately so
    /// the caller reports the actionable remedy.
    pub(crate) fn open_with_pin_retry(
        kind: HandleKind,
        filter: &str,
        layer: WinDivertLayer,
        priority: i16,
        flags: WinDivertFlags,
    ) -> io::Result<Self> {
        let id = next_handle_id();
        let mut recovery = OpenRecovery::new(Instant::now());
        loop {
            let attempt = recovery.begin_attempt();
            match Self::open_with_id(id, kind, attempt, filter, layer, priority, flags) {
                Ok(handle) => return Ok(handle),
                Err(err) => {
                    let pin_active = driver_pin_active();
                    match recovery.step_after_failure(&err, kind, pin_active, Instant::now()) {
                        RecoveryStep::Retry { backoff } => {
                            tracing::warn!(
                                "WinDivert handle open retry: id={id} kind={} attempt={}/{} \
                                 after=FWP_E_IN_USE(0x{:08X}) pin_active={pin_active} \
                                 backoff_ms={}",
                                kind.as_str(),
                                recovery.attempts() + 1,
                                FWP_IN_USE_MAX_ATTEMPTS,
                                err.raw_os_error().unwrap_or(0) as u32,
                                backoff.as_millis(),
                            );
                            std::thread::sleep(backoff);
                        }
                        RecoveryStep::PinCycle => {
                            Self::pin_cycle_for_recovery(id, kind, recovery.attempts() + 1, &err);
                        }
                        RecoveryStep::GiveUp => return Err(err),
                    }
                }
            }
        }
    }

    /// R2 production wiring: release the driver pin, reopen it once, and let the
    /// caller retry the diverting open once.
    ///
    /// Uses the existing idempotent [`release_driver_pin`] and [`pin_driver`]
    /// functions; no new pin mechanism and no driver/service stop (R3 stays an
    /// explicit, unimplemented user action). A pin reopen failure is logged but
    /// not substituted for the open's own error: the diverting retry still runs
    /// (the driver may already be loaded), and the open's `FWP_E_IN_USE` remains
    /// the actionable failure. Only ever called for a diverting handle.
    fn pin_cycle_for_recovery(id: u64, kind: HandleKind, next_attempt: u32, err: &io::Error) {
        let released_pin_id = driver_pin_id();
        release_driver_pin();
        let reopened = pin_driver();
        tracing::warn!(
            "WinDivert handle open pin-cycle: id={id} kind={} attempt={next_attempt}/{} \
             after=FWP_E_IN_USE(0x{:08X}) released_pin_id={released_pin_id:?} \
             pin_reopened={} -> retrying the open once",
            kind.as_str(),
            OpenRecovery::max_attempts(),
            err.raw_os_error().unwrap_or(0) as u32,
            reopened.is_ok(),
        );
        if let Err(reason) = reopened {
            tracing::warn!(
                "WinDivert handle open pin-cycle could not reopen the driver pin; \
                 retrying the diverting open anyway: {reason}"
            );
        }
    }

    /// The handle's monotonic process-wide id (for logs and release messages).
    pub(crate) fn id(&self) -> u64 {
        self.id
    }

    /// Open with an already-allocated id: one log line per attempt plus the
    /// per-kind counters. This is the only place `WinDivertOpen` is reached.
    fn open_with_id(
        id: u64,
        kind: HandleKind,
        attempt: u32,
        filter: &str,
        layer: WinDivertLayer,
        priority: i16,
        flags: WinDivertFlags,
    ) -> io::Result<Self> {
        let attempt_started = Instant::now();
        record_open_attempt(kind);
        let flags_word = u64::from(flags);

        // A NUL filter is rejected before the FFI call, but it is still an open
        // attempt and is counted, recorded and logged like any other failure. It
        // gets no support bundle: the driver was provably never reached, so the
        // bundle's WFP/service snapshot would describe state this failure cannot
        // be about.
        let filter_c = match CString::new(filter) {
            Ok(filter) => filter,
            Err(_) => {
                let err = io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "WinDivert filter contains an interior NUL byte",
                );
                record_open_failure(kind);
                let event = build_open_event(OpenEventParams {
                    id,
                    kind,
                    attempt,
                    layer,
                    priority,
                    flags_word,
                    filter,
                    intent: lifecycle::current_intent(),
                    pin_active: driver_pin_active(),
                    pin_id: driver_pin_id(),
                    elapsed_us: elapsed_us(Instant::now(), attempt_started),
                    ms_since_last_close: ms_since_last_close(),
                    fwp_code: None,
                    owned_handles: current_owned_handles(),
                    result: lifecycle::OpenResult::Failed,
                    snapshot_id: None,
                });
                lifecycle::emit("windivert_open", &event);
                tracing::warn!(
                    "WinDivert handle open: id={id} kind={} attempt={attempt} layer={layer:?} \
                     priority={priority} flags=0x{flags_word:04x} filter={filter:?} \
                     -> FAILED ({err})",
                    kind.as_str(),
                );
                return Err(err);
            }
        };

        // SAFETY: [Category 8 - FFI] `filter_c` is a live NUL-terminated C
        // string owned by the local binding, so the pointer is valid for the
        // whole call; the returned handle is checked for both failure sentinels
        // below before it is stored.
        //
        // `WinDivertOpen` returns the raw OS handle as an `isize`, while the
        // remaining calls take a `*mut c_void`; the cast is a pure
        // representation change, not a dereference.
        let raw =
            unsafe { WinDivertOpen(filter_c.as_ptr(), layer, priority, flags) } as WinDivertHandle;
        if Self::is_open_failure(raw) {
            let err = io::Error::last_os_error();
            record_open_failure(kind);
            let now = Instant::now();
            let fwp = fwp_code(&err);
            let pin_active = driver_pin_active();
            let pin_id = driver_pin_id();
            let gap = ms_since_last_close();
            let owned_handles = current_owned_handles();
            // The bundle carries the WFP/service/handle context the failure
            // episode is investigated with; the sink's 60 s rate limit applies,
            // so a retried attempt does not write one per attempt.
            let snapshot_id = open_failure_bundle(FailureBundleContext {
                id,
                kind,
                attempt,
                layer,
                priority,
                flags_word,
                filter,
                err: &err,
                fwp: fwp.as_deref(),
                pin_active,
                pin_id,
                ms_since_last_close: gap,
                owned_handles: &owned_handles,
            });
            let event = build_open_event(OpenEventParams {
                id,
                kind,
                attempt,
                layer,
                priority,
                flags_word,
                filter,
                intent: lifecycle::current_intent(),
                pin_active,
                pin_id,
                elapsed_us: elapsed_us(now, attempt_started),
                ms_since_last_close: gap,
                fwp_code: fwp,
                owned_handles,
                result: lifecycle::OpenResult::Failed,
                snapshot_id: snapshot_id.clone(),
            });
            lifecycle::emit("windivert_open", &event);
            // Append only: the shipped failure line and the remedy text are
            // unchanged, the snapshot id is extra context on the same line.
            let snapshot_suffix = snapshot_id
                .as_deref()
                .map(|snapshot| format!(" snapshot_id={snapshot}"))
                .unwrap_or_default();
            tracing::warn!(
                "WinDivert handle open: id={id} kind={} attempt={attempt} layer={layer:?} \
                 priority={priority} flags=0x{flags_word:04x} filter={filter:?} \
                 -> FAILED ({err}){snapshot_suffix}",
                kind.as_str(),
            );
            return Err(err);
        }

        record_open_success(kind);
        record_live_handle(id, kind);
        let opened = Instant::now();
        let event = build_open_event(OpenEventParams {
            id,
            kind,
            attempt,
            layer,
            priority,
            flags_word,
            filter,
            intent: lifecycle::current_intent(),
            pin_active: driver_pin_active(),
            pin_id: driver_pin_id(),
            elapsed_us: elapsed_us(opened, attempt_started),
            ms_since_last_close: ms_since_last_close(),
            fwp_code: None,
            owned_handles: current_owned_handles(),
            result: lifecycle::OpenResult::Ok,
            snapshot_id: None,
        });
        lifecycle::emit("windivert_open", &event);
        tracing::info!(
            "WinDivert handle open: id={id} kind={} attempt={attempt} layer={layer:?} \
             priority={priority} flags=0x{flags_word:04x} filter={filter:?} -> ok",
            kind.as_str(),
        );
        Ok(Self {
            raw,
            closed: AtomicBool::new(false),
            id,
            kind,
            opened,
        })
    }

    /// Receive one packet into `buf`.
    ///
    /// On success returns the number of bytes written and the packet address.
    /// When the handle has been shut down, the returned error carries the
    /// shutdown sentinel (see [`crate::interceptor::teardown::recv_should_break`]).
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] for an empty buffer or a buffer
    /// larger than `u32::MAX` (the FFI length is `u32`), and the OS error from
    /// `GetLastError` when `WinDivertRecv` fails.
    pub fn recv(&self, buf: &mut [u8]) -> io::Result<(usize, WINDIVERT_ADDRESS)> {
        if buf.is_empty() || buf.len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "WinDivert recv buffer length {} is outside 1..={}",
                    buf.len(),
                    u32::MAX
                ),
            ));
        }

        let mut recv_len: u32 = 0;
        let mut addr = WINDIVERT_ADDRESS::default();

        // SAFETY: [Category 8 - FFI] `buf` is a valid exclusive slice, so its
        // pointer is writable for `buf.len()` bytes (just validated to fit in
        // `u32`); `recv_len` and `addr` are stack locals whose pointers are
        // valid and writable for the call. The driver writes at most
        // `buf.len()` bytes and a fully initialized `WINDIVERT_ADDRESS`.
        let ok = unsafe {
            WinDivertRecv(
                self.raw,
                buf.as_mut_ptr().cast::<c_void>(),
                buf.len() as u32,
                &mut recv_len,
                &mut addr,
            )
        };

        if succeeded(ok) {
            Ok((recv_len as usize, addr))
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Send one raw packet with the given address.
    ///
    /// Returns the number of bytes handed to the driver.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::InvalidInput`] for an empty packet or a packet
    /// larger than `u32::MAX`, and the OS error from `GetLastError` when
    /// `WinDivertSend` fails.
    pub fn send(&self, data: &[u8], addr: &WINDIVERT_ADDRESS) -> io::Result<usize> {
        if data.is_empty() || data.len() > u32::MAX as usize {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!(
                    "WinDivert send packet length {} is outside 1..={}",
                    data.len(),
                    u32::MAX
                ),
            ));
        }

        let mut sent_len: u32 = 0;

        // SAFETY: [Category 8 - FFI] `data` is a valid shared slice whose
        // pointer is readable for `data.len()` bytes (just validated to fit in
        // `u32`); `addr` and `sent_len` are valid references for the call.
        let ok = unsafe {
            WinDivertSend(
                self.raw,
                data.as_ptr().cast::<c_void>(),
                data.len() as u32,
                &mut sent_len,
                addr,
            )
        };

        if succeeded(ok) {
            Ok(sent_len as usize)
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Shut down receive and/or send on the handle, unblocking any thread
    /// currently parked in [`recv`](Self::recv) or `send`.
    ///
    /// This is deliberately `&self`: the owner thread can call it while a
    /// receive thread is blocked and still holds its `Arc<OwnedHandle>`.
    ///
    /// # Errors
    ///
    /// Returns [`io::ErrorKind::Other`] if the handle was already closed
    /// (the FFI is not called on a recycled handle value), and the OS error
    /// from `GetLastError` when `WinDivertShutdown` fails.
    pub fn shutdown(&self, mode: WinDivertShutdownMode) -> io::Result<()> {
        if self.closed.load(Ordering::Acquire) {
            return Err(io::Error::other(
                "WinDivert handle is already closed; refusing to shut it down",
            ));
        }

        // SAFETY: [Category 8 - FFI] `self.raw` is a live handle for the
        // lifetime of `self` (checked above that no close was attempted yet,
        // and the owner contract keeps close after every shutdown), and the
        // mode is a plain `#[repr(u32)]` enum passed by value.
        let ok = unsafe { WinDivertShutdown(self.raw, mode) };

        if succeeded(ok) {
            Ok(())
        } else {
            Err(io::Error::last_os_error())
        }
    }

    /// Close the handle, unregistering its WFP filter and callout.
    ///
    /// Takes `&self` on purpose: the owner thread can close without fighting
    /// the borrow checker against the `Arc` shared with the receive threads.
    /// Repeated calls are guarded by an internal [`AtomicBool`], so this is a
    /// no-op after the first attempt and [`Drop`] never double-closes.
    ///
    /// # Errors
    ///
    /// Returns the OS error from `GetLastError` when `WinDivertClose` fails.
    /// A failed close still marks the handle as closed: retrying against a
    /// possibly recycled handle value is worse than leaking a driver handle
    /// that Windows will release at process exit.
    pub fn close(&self) -> io::Result<()> {
        if self.closed.swap(true, Ordering::AcqRel) {
            return Ok(());
        }

        // SAFETY: [Categories 8 - FFI, 12 - double free] The `swap` above
        // makes exactly one thread reach this call, so the handle is closed at
        // most once; at this point it is still live because `self` owns it and
        // `shutdown`/`close` are serialised by the owner thread.
        let ok = unsafe { WinDivertClose(self.raw) };
        let now = Instant::now();
        let close_age_ms = age_ms(now, self.opened);

        if succeeded(ok) {
            record_close(self.kind, true);
            // Only a successful close becomes the close→open anchor: a failed
            // close leaves the driver still holding the handle, so measuring an
            // open against it would blame the wrong episode.
            record_last_close_success(now);
            forget_live_handle(self.id);
            lifecycle::emit(
                "windivert_close",
                &build_close_event(
                    self.id,
                    self.kind,
                    close_age_ms,
                    lifecycle::CloseResult::Ok,
                    driver_pin_active(),
                    current_owned_handles(),
                ),
            );
            tracing::info!(
                "WinDivert handle close: id={} kind={} -> ok",
                self.id,
                self.kind.as_str()
            );
            Ok(())
        } else {
            let err = io::Error::last_os_error();
            record_close(self.kind, false);
            // The counters already stopped tracking this handle (a failed close
            // is never retried against a possibly recycled value), so the live
            // table follows the same rule for an honest `owned_handles` value.
            forget_live_handle(self.id);
            lifecycle::emit(
                "windivert_close",
                &build_close_event(
                    self.id,
                    self.kind,
                    close_age_ms,
                    lifecycle::CloseResult::Failed,
                    driver_pin_active(),
                    current_owned_handles(),
                ),
            );
            tracing::warn!(
                "WinDivert handle close: id={} kind={} -> FAILED ({err})",
                self.id,
                self.kind.as_str()
            );
            Err(err)
        }
    }
}

impl Drop for OwnedHandle {
    fn drop(&mut self) {
        // Best effort: unblock any straggler parked in recv, then release the
        // filter. Both errors are deliberately ignored here because a `Drop`
        // impl has no channel to report through; callers that need the close
        // result call `close` explicitly.
        let _ = self.shutdown(WinDivertShutdownMode::Both);
        let _ = self.close();
    }
}

/// Returns `true` when `err` is the WFP "handle still in use" failure
/// (`0x8032000A` or `0xC022000A`) that a leaked or still-releasing WinDivert
/// handle state produces.
///
/// It is a status family, not a diagnosis: it covers a management-object
/// reference conflict and a concurrent callout register/unregister race, and the
/// open-recovery ladder (R1 retry, R2 pin-cycle) is the userspace response. The
/// remedy text names the ladder instead of prescribing a reboot.
pub fn is_fwp_in_use(err: &io::Error) -> bool {
    let Some(code) = err.raw_os_error() else {
        return false;
    };
    code == 0x8032_000A_u32 as i32 || code == 0xC022_000A_u32 as i32
}

// ─────────────────────────────────────────────────────────────────────────────
//  Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Serializes the tests that assert *deltas* on the process-wide counters.
    /// The counters are necessarily global (that is the point of the status
    /// surface), so without this a parallel test can increment the same bucket
    /// between another test's before/after snapshots.
    static COUNTER_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn is_fwp_in_use_matches_windows_error_codes() {
        assert!(is_fwp_in_use(&io::Error::from_raw_os_error(
            0x8032_000A_u32 as i32
        )));
        assert!(is_fwp_in_use(&io::Error::from_raw_os_error(
            0xC022_000A_u32 as i32
        )));
    }

    #[test]
    fn is_fwp_in_use_rejects_other_errors() {
        assert!(!is_fwp_in_use(&io::Error::from_raw_os_error(5)));
        assert!(!is_fwp_in_use(&io::Error::new(
            io::ErrorKind::InvalidInput,
            "not a WFP error"
        )));
    }

    #[test]
    fn open_failure_detects_both_sentinels() {
        // `WinDivertOpen` documents INVALID_HANDLE_VALUE, not NULL, as failure.
        assert!(OwnedHandle::is_open_failure(std::ptr::null_mut()));
        assert!(OwnedHandle::is_open_failure(
            OwnedHandle::INVALID_HANDLE_VALUE as WinDivertHandle
        ));
        // A real handle value must never be mistaken for a failure sentinel.
        assert!(!OwnedHandle::is_open_failure(4 as WinDivertHandle));
    }

    // ── Handle roles and instrumentation ──────────────────────────────────

    #[test]
    fn handle_kinds_have_distinct_stable_names() {
        let kinds = [
            HandleKind::Pin,
            HandleKind::Intercept,
            HandleKind::Inject,
            HandleKind::Shadow,
            HandleKind::Legacy,
        ];
        let mut names: Vec<&str> = kinds.iter().map(|k| k.as_str()).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(
            names.len(),
            HandleKind::COUNT,
            "every kind must have its own log/counter name"
        );
        for kind in kinds {
            assert!(
                kind.index() < HandleKind::COUNT,
                "kind {kind:?} must index the counter table"
            );
        }
        // The names the evidence harness greps for.
        assert_eq!(HandleKind::Pin.as_str(), "pin");
        assert_eq!(HandleKind::Intercept.as_str(), "intercept");
        assert_eq!(HandleKind::Inject.as_str(), "inject");
        assert_eq!(HandleKind::Shadow.as_str(), "shadow");
    }

    #[test]
    fn handle_stats_are_counted_per_kind() {
        // The recording helpers are exercised directly: no driver and no FFI
        // are involved, so this runs on every `cargo test` host.
        let _guard = COUNTER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = windivert_handle_stats();
        let kinds = [
            HandleKind::Pin,
            HandleKind::Intercept,
            HandleKind::Inject,
            HandleKind::Shadow,
        ];
        for kind in kinds {
            record_open_attempt(kind);
            record_open_success(kind);
            record_close(kind, true);
        }
        let after = windivert_handle_stats();
        for kind in kinds {
            let b = before.by_kind(kind);
            let a = after.by_kind(kind);
            assert_eq!(
                a.open_attempts - b.open_attempts,
                1,
                "{} open attempts",
                kind.as_str()
            );
            assert_eq!(
                a.open_successes - b.open_successes,
                1,
                "{} open successes",
                kind.as_str()
            );
            assert_eq!(
                a.close_attempts - b.close_attempts,
                1,
                "{} close attempts",
                kind.as_str()
            );
            assert_eq!(
                a.close_successes - b.close_successes,
                1,
                "{} close successes",
                kind.as_str()
            );
            assert_eq!(
                a.open_failures,
                b.open_failures,
                "{} must not gain a failure",
                kind.as_str()
            );
            assert_eq!(
                a.live,
                b.live,
                "{} open followed by close nets zero live handles",
                kind.as_str()
            );
        }
        assert!(
            after.live_handles() >= before.live_handles(),
            "the aggregate live count never goes backwards"
        );
    }

    #[test]
    fn a_failed_open_attempt_is_counted_and_a_close_failure_is_visible() {
        // Distinct kinds from the stats test above keep these assertions
        // independent under the parallel test harness.
        let _guard = COUNTER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = windivert_handle_stats().legacy;
        record_open_attempt(HandleKind::Legacy);
        record_open_failure(HandleKind::Legacy);
        record_open_success(HandleKind::Legacy);
        record_close(HandleKind::Legacy, false);
        let after = windivert_handle_stats().legacy;
        assert_eq!(after.open_attempts - before.open_attempts, 1);
        assert_eq!(after.open_successes - before.open_successes, 1);
        assert_eq!(after.open_failures - before.open_failures, 1);
        assert_eq!(after.close_failures - before.close_failures, 1);
        assert_eq!(after.close_successes, before.close_successes);
    }

    #[test]
    fn handle_ids_are_monotonic() {
        // Other tests may allocate ids concurrently; the property under test is
        // strictly increasing, not consecutive.
        let first = next_handle_id();
        let second = next_handle_id();
        assert!(second > first, "ids must increase ({first} then {second})");
    }

    #[test]
    fn open_rejects_an_interior_nul_before_touching_the_driver() {
        // `open_as` validates the filter before the FFI call, so this runs with
        // no WinDivert driver installed. It must still be counted and logged as
        // a failed attempt, not silently skipped.
        let _guard = COUNTER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let before = windivert_handle_stats().legacy;
        let err = match OwnedHandle::open_as(
            HandleKind::Legacy,
            "udp and outbound\u{0}hijack",
            WinDivertLayer::Network,
            0,
            WinDivertFlags::new(),
        ) {
            Ok(_) => panic!("an interior NUL is not a valid filter"),
            Err(err) => err,
        };
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert_eq!(err.raw_os_error(), None, "no driver call was made");
        let after = windivert_handle_stats().legacy;
        assert_eq!(after.open_attempts - before.open_attempts, 1);
        assert_eq!(after.open_failures - before.open_failures, 1);
        assert_eq!(after.open_successes, before.open_successes);
    }

    // ── Bounded FWP_E_IN_USE retry policy ─────────────────────────────────

    /// FAILING-BEFORE (P0-2): the old 1.12.5 policy returned `false` here
    /// whenever the pin was held, so the pinned restart made exactly one attempt
    /// (`should_retry_fwp_in_use(&stale, true) == false`). A held pin is a device
    /// handle, not a WFP state lock, so it must never suppress the retry
    /// (roadmap D1/D5). This test is the acceptance reproduction for that change.
    #[test]
    fn fwp_in_use_is_retried_while_the_pin_is_held() {
        let stale = io::Error::from_raw_os_error(0x8032_000A_u32 as i32);
        assert!(
            should_retry_fwp_in_use(&stale, true),
            "a held pin must not suppress the bounded FWP_E_IN_USE retry: the \
             pinned path used to make exactly one attempt"
        );
        assert!(
            should_retry_fwp_in_use(&stale, false),
            "the unpinned path keeps its bounded retry"
        );
        // Every non-FWP failure stays permanent, pinned or unpinned: retrying
        // cannot fix permissions, a missing/blocked driver or a bad filter.
        for pin_active in [false, true] {
            assert!(
                !should_retry_fwp_in_use(&io::Error::from_raw_os_error(5), pin_active),
                "permission failures are permanent (pin_active={pin_active})"
            );
            assert!(
                !should_retry_fwp_in_use(&io::Error::from_raw_os_error(577), pin_active),
                "a signature failure is permanent (pin_active={pin_active})"
            );
            assert!(
                !should_retry_fwp_in_use(&io::Error::from_raw_os_error(1275), pin_active),
                "a blocked driver is permanent (pin_active={pin_active})"
            );
            assert!(!should_retry_fwp_in_use(
                &io::Error::new(io::ErrorKind::InvalidInput, "not a WFP error"),
                pin_active
            ));
        }
    }

    // ── Bounded R1 retry budget (pure, no driver) ─────────────────────────

    #[test]
    fn the_open_retry_budget_is_finite_and_within_five_seconds() {
        // The R1 attempt ceiling is in the roadmap's 4-6 range.
        assert!(
            (4..=6).contains(&FWP_IN_USE_MAX_ATTEMPTS),
            "R1 attempts must stay in the documented 4-6 range: {FWP_IN_USE_MAX_ATTEMPTS}"
        );
        // The backoff is exponential from 50 ms, capped in the 350-500 ms band,
        // and it stops when the attempt ceiling is reached.
        let plan: Vec<Duration> = (0..64).filter_map(fwp_in_use_backoff).collect();
        assert_eq!(
            plan.len(),
            (FWP_IN_USE_MAX_ATTEMPTS - 1) as usize,
            "one backoff per retry and never one after the first attempt: {plan:?}"
        );
        assert_eq!(FWP_IN_USE_BACKOFF_INITIAL, Duration::from_millis(50));
        assert!(
            FWP_IN_USE_BACKOFF_CAP >= Duration::from_millis(350)
                && FWP_IN_USE_BACKOFF_CAP <= Duration::from_millis(500),
            "the backoff cap must stay in the roadmap's 350-500 ms band"
        );
        assert_eq!(plan.first().copied(), Some(FWP_IN_USE_BACKOFF_INITIAL));
        assert!(
            plan.iter()
                .all(|backoff| *backoff <= FWP_IN_USE_BACKOFF_CAP),
            "no backoff may exceed the cap: {plan:?}"
        );
        assert!(
            plan.windows(2).all(|pair| pair[1] >= pair[0]),
            "the backoff never shrinks: {plan:?}"
        );
        assert!(
            plan.windows(2).any(|pair| pair[1] > pair[0]),
            "the backoff is exponential before it caps: {plan:?}"
        );
        let scheduled: Duration = plan.iter().sum();
        assert!(
            scheduled <= FWP_IN_USE_RETRY_BUDGET,
            "the scheduled backoff fits the wall-clock budget: {scheduled:?}"
        );
        assert_eq!(
            FWP_IN_USE_RETRY_BUDGET,
            Duration::from_secs(5),
            "the budget is the roadmap's ~5 s"
        );

        // A full failing episode on a fake clock (fast open calls, no pin): the
        // loop must stop after the R1 ceiling and stay inside the budget.
        let stale = io::Error::from_raw_os_error(0x8032_000A_u32 as i32);
        let start = Instant::now();
        let mut now = start;
        let mut recovery = OpenRecovery::new(start);
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            assert!(
                attempts <= OpenRecovery::max_attempts(),
                "the recovery loop must be bounded: attempt {attempts}"
            );
            assert_eq!(recovery.begin_attempt(), attempts);
            now += Duration::from_millis(1); // a fast WinDivertOpen round-trip
            match recovery.step_after_failure(&stale, HandleKind::Intercept, false, now) {
                RecoveryStep::Retry { backoff } => now += backoff,
                RecoveryStep::PinCycle => {
                    panic!("no pin is held, so R2 must not apply (attempt {attempts})")
                }
                RecoveryStep::GiveUp => break,
            }
        }
        assert_eq!(
            attempts, FWP_IN_USE_MAX_ATTEMPTS,
            "the R1 episode ends after exactly the attempt ceiling"
        );
        assert!(
            now.saturating_duration_since(start) <= FWP_IN_USE_RETRY_BUDGET,
            "the whole episode stays inside the wall-clock budget: {:?}",
            now.saturating_duration_since(start)
        );

        // A slow open call cannot stretch the retry ladder: no retry is ever
        // scheduled past the budget; the ladder simply ends earlier.
        let start = Instant::now();
        let mut now = start;
        let mut recovery = OpenRecovery::new(start);
        let mut attempts = 0u32;
        loop {
            attempts += 1;
            assert!(attempts <= OpenRecovery::max_attempts());
            let _ = recovery.begin_attempt();
            now += Duration::from_secs(2); // each open call is slow
            match recovery.step_after_failure(&stale, HandleKind::Intercept, false, now) {
                RecoveryStep::Retry { backoff } => {
                    let retry_at = now.saturating_duration_since(start) + backoff;
                    assert!(
                        retry_at <= FWP_IN_USE_RETRY_BUDGET,
                        "a retry must not be scheduled past the budget: {retry_at:?}"
                    );
                    now += backoff;
                }
                RecoveryStep::PinCycle => panic!("no pin is held, so R2 must not apply"),
                RecoveryStep::GiveUp => break,
            }
        }
        assert!(
            attempts < FWP_IN_USE_MAX_ATTEMPTS,
            "slow open calls end the ladder earlier, not later: {attempts}"
        );
    }

    // ── R2: one pin-cycle per failure episode (pure, no driver) ───────────

    #[test]
    fn one_pin_cycle_follows_the_retry_budget_and_never_repeats() {
        let stale = io::Error::from_raw_os_error(0x8032_000A_u32 as i32);
        let start = Instant::now();
        let mut now = start;
        let mut recovery = OpenRecovery::new(start);

        // R1: six attempts, five backoffs, then the single R2 pin-cycle.
        for attempt in 1..=FWP_IN_USE_MAX_ATTEMPTS {
            assert_eq!(recovery.begin_attempt(), attempt);
            now += Duration::from_millis(1);
            match recovery.step_after_failure(&stale, HandleKind::Intercept, true, now) {
                RecoveryStep::Retry { backoff } => {
                    assert!(
                        attempt < FWP_IN_USE_MAX_ATTEMPTS,
                        "R2 must not pre-empt an R1 attempt"
                    );
                    now += backoff;
                }
                RecoveryStep::PinCycle => assert_eq!(
                    attempt, FWP_IN_USE_MAX_ATTEMPTS,
                    "R2 must wait for the whole R1 retry budget"
                ),
                RecoveryStep::GiveUp => {
                    panic!("attempt {attempt}: a pinned session handle must reach R2")
                }
            }
        }
        // R2's one retry: attempt 7 fails too -> give up, never a second cycle.
        assert_eq!(recovery.begin_attempt(), OpenRecovery::max_attempts());
        now += Duration::from_millis(1);
        assert_eq!(
            recovery.step_after_failure(&stale, HandleKind::Intercept, true, now),
            RecoveryStep::GiveUp,
            "the pin-cycle rung is once per failure episode"
        );
        assert_eq!(
            recovery.step_after_failure(&stale, HandleKind::Intercept, true, now),
            RecoveryStep::GiveUp,
            "the state machine cannot re-arm the pin-cycle rung"
        );

        // No pin held: no R2 rung, the episode ends after R1.
        let mut unpinned = OpenRecovery::new(start);
        for _ in 1..=FWP_IN_USE_MAX_ATTEMPTS {
            let _ = unpinned.begin_attempt();
        }
        assert_eq!(
            unpinned.step_after_failure(&stale, HandleKind::Intercept, false, start),
            RecoveryStep::GiveUp,
            "there is nothing to cycle when no pin is held"
        );

        // The pin's own open must never cycle itself: it runs under the pin-slot
        // lock, so releasing/reopening from inside would deadlock.
        let mut pin_open = OpenRecovery::new(start);
        for _ in 1..=FWP_IN_USE_MAX_ATTEMPTS {
            let _ = pin_open.begin_attempt();
        }
        assert_eq!(
            pin_open.step_after_failure(&stale, HandleKind::Pin, true, start),
            RecoveryStep::GiveUp,
            "the pin's own open must never enter the R2 rung"
        );

        // A permanent failure never reaches R2, even on the last attempt.
        let mut permanent = OpenRecovery::new(start);
        assert_eq!(
            permanent.step_after_failure(
                &io::Error::from_raw_os_error(5),
                HandleKind::Intercept,
                true,
                start
            ),
            RecoveryStep::GiveUp,
            "R2 must not rescue a permanent driver failure"
        );

        // The whole episode is bounded by R1 plus the one R2 attempt.
        assert_eq!(OpenRecovery::max_attempts(), FWP_IN_USE_MAX_ATTEMPTS + 1);
    }

    // ── Failure copy: the R1-R4 recovery ladder ───────────────────────────

    #[test]
    fn fwp_in_use_remedy_is_the_r1_r4_ladder_and_not_reboot_first() {
        let stale = io::Error::from_raw_os_error(0x8032_000A_u32 as i32);
        let copy = driver_open_remedy(&stale);
        let lower = copy.to_ascii_lowercase();

        // The old first-line advice ("... restart Windows to clear it ...") is
        // gone, and the copy does not open by demanding a reboot.
        assert!(
            lower.starts_with("windivert could not open the interception handle"),
            "the remedy names the failed open first: {copy}"
        );
        let first_sentence = lower.split('.').next().unwrap_or_default();
        for banned in ["restart windows", "reboot", "shutdown"] {
            assert!(
                !first_sentence.contains(banned),
                "the first sentence must not prescribe {banned}: {copy}"
            );
        }
        assert!(
            !lower.contains("restart windows"),
            "the hard-coded reboot-first sentence must be gone: {copy}"
        );

        // R1-R4 appear in ascending order: automatic rungs first, explicit R3
        // next, reboot last.
        let mut cursor = 0usize;
        for rung in ["r1", "r2", "r3", "r4"] {
            let found = lower[cursor..]
                .find(rung)
                .unwrap_or_else(|| panic!("{rung} is missing from the ladder: {copy}"));
            cursor += found + rung.len();
        }
        let r1 = lower.find("r1").expect("R1 present");
        let r3 = lower.find("r3").expect("R3 present");
        let r4 = lower.find("r4").expect("R4 present");
        let automatic = &lower[r1..r3];
        assert!(
            automatic.matches("automatic").count() >= 2,
            "R1 and R2 must both be described as automatic: {copy}"
        );
        assert!(
            automatic.contains("pin-cycle"),
            "R2 must name the pin-cycle: {copy}"
        );
        assert!(
            automatic.contains("pin"),
            "R1 must say the retry runs while the pin is held: {copy}"
        );
        let explicit = &lower[r3..r4];
        assert!(
            explicit.contains("explicit"),
            "R3 must be an explicit user action: {copy}"
        );
        assert!(
            explicit.contains("does not") && explicit.contains("automatically"),
            "R3 must state that LightSpeed does not do it automatically: {copy}"
        );
        assert!(
            explicit.contains("administrator"),
            "R3 must carry the elevation warning: {copy}"
        );
        let last = &lower[r4..];
        assert!(
            last.contains("last resort"),
            "R4 must be the last resort: {copy}"
        );
        assert!(
            last.contains("external"),
            "R4 must state the external-holder reason: {copy}"
        );

        // The copy's budget numbers are the code's budget numbers.
        assert!(
            copy.contains(&format!("{FWP_IN_USE_MAX_ATTEMPTS} attempts")),
            "R1 names the real attempt budget: {copy}"
        );
        assert!(
            copy.contains(&format!("about {} s", FWP_IN_USE_RETRY_BUDGET.as_secs())),
            "R1 names the real wall-clock budget: {copy}"
        );

        // The other branches keep their concrete next action.
        assert!(driver_open_remedy(&io::Error::from_raw_os_error(5)).contains("Administrator"));
        assert!(driver_open_remedy(&io::Error::from_raw_os_error(1275)).contains("blocked"));
    }

    // ── Pin slot: idempotence and serialization (no driver needed) ────────

    #[test]
    fn pin_slot_opens_once_and_releases_idempotently() {
        let slot = PinSlot::<u32>::new();
        let calls = AtomicU64::new(0);

        assert!(!slot.active());
        let opened = slot
            .ensure_open(|| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(7)
            })
            .expect("first open succeeds");
        assert!(opened, "the first call must open a handle");
        assert!(slot.active());

        let opened_again = slot
            .ensure_open(|| {
                calls.fetch_add(1, Ordering::SeqCst);
                Ok(8)
            })
            .expect("a held pin is not an error");
        assert!(!opened_again, "an already-held pin must not open again");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "opener called exactly once"
        );

        assert_eq!(slot.take(), Some(7));
        assert!(!slot.active());
        assert_eq!(slot.take(), None, "release is idempotent");
        assert!(!slot.active());
    }

    #[test]
    fn pin_slot_serializes_concurrent_opens() {
        let slot = PinSlot::<u32>::new();
        let calls = AtomicU64::new(0);
        let opened = AtomicU64::new(0);
        let (slot_ref, calls_ref, opened_ref) = (&slot, &calls, &opened);

        std::thread::scope(|scope| {
            let mut workers = Vec::new();
            for value in 0..8u32 {
                workers.push(scope.spawn(move || {
                    let result = slot_ref
                        .ensure_open(|| {
                            calls_ref.fetch_add(1, Ordering::SeqCst);
                            // Hold the slot long enough for the other threads to
                            // pile up behind it.
                            std::thread::sleep(Duration::from_millis(20));
                            Ok(value)
                        })
                        .expect("concurrent ensure_open must not fail");
                    if result {
                        opened_ref.fetch_add(1, Ordering::SeqCst);
                    }
                }));
            }
            for worker in workers {
                worker.join().expect("worker thread does not panic");
            }
        });

        assert_eq!(
            calls.load(Ordering::SeqCst),
            1,
            "exactly one handle may be opened across concurrent pin calls"
        );
        assert_eq!(
            opened.load(Ordering::SeqCst),
            1,
            "exactly one caller reports having opened the pin"
        );
        assert!(slot.active());
    }

    #[test]
    fn pin_slot_failed_open_leaves_no_pin_and_allows_a_retry() {
        let slot = PinSlot::<u32>::new();
        let err = slot
            .ensure_open(|| Err::<u32, _>(io::Error::from_raw_os_error(5)))
            .expect_err("a failed open is reported");
        assert_eq!(err.raw_os_error(), Some(5));
        assert!(!slot.active(), "a failed open must not look pinned");

        let opened = slot
            .ensure_open(|| Ok(9))
            .expect("a later call may retry after a failed open");
        assert!(opened);
        assert!(slot.active());
    }

    // ── Lifecycle event bridge (P0-1b; pure, no driver) ───────────────────

    fn test_intent() -> lifecycle::Intent {
        lifecycle::Intent {
            reason: lifecycle::InterceptReason::ErrorRetry,
            actor: lifecycle::Actor::Retry,
            command_id: "cmd-7".to_string(),
        }
    }

    #[test]
    fn every_handle_kind_maps_to_a_distinct_lifecycle_name() {
        let mappings = [
            (HandleKind::Pin, lifecycle::HandleKindName::Pin),
            (HandleKind::Intercept, lifecycle::HandleKindName::Intercept),
            (HandleKind::Inject, lifecycle::HandleKindName::Inject),
            (HandleKind::Shadow, lifecycle::HandleKindName::Shadow),
            (HandleKind::Legacy, lifecycle::HandleKindName::Legacy),
        ];
        let mut names = Vec::new();
        for (kind, expected) in mappings {
            assert_eq!(lifecycle_kind(kind), expected, "{} mapping", kind.as_str());
            names.push(
                serde_json::to_value(lifecycle_kind(kind)).expect("lifecycle names serialize"),
            );
        }
        let mut sorted = names.clone();
        sorted.sort_by_key(|value| value.to_string());
        sorted.dedup();
        assert_eq!(
            sorted.len(),
            names.len(),
            "names must be distinct: {names:?}"
        );
        // The pin is the one non-diverting handle; a pin failure is not bundled
        // as a session open failure.
        assert!(!HandleKind::Pin.is_diverting());
        for kind in [
            HandleKind::Intercept,
            HandleKind::Inject,
            HandleKind::Shadow,
            HandleKind::Legacy,
        ] {
            assert!(kind.is_diverting(), "{} is a session handle", kind.as_str());
        }
    }

    #[test]
    fn fwp_code_formats_the_os_status_as_eight_hex_digits() {
        assert_eq!(
            fwp_code(&io::Error::from_raw_os_error(0x8032_000A_u32 as i32)).as_deref(),
            Some("0x8032000A"),
            "the field FWP_E_IN_USE keeps its canonical spelling"
        );
        assert_eq!(
            fwp_code(&io::Error::from_raw_os_error(0xC022_000A_u32 as i32)).as_deref(),
            Some("0xC022000A")
        );
        assert_eq!(
            fwp_code(&io::Error::from_raw_os_error(5)).as_deref(),
            Some("0x00000005"),
            "short codes are zero-padded, never bare"
        );
        assert_eq!(
            fwp_code(&io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a driver call"
            )),
            None,
            "no OS error means no fwp_code (the interior-NUL path)"
        );
    }

    #[test]
    fn close_age_and_elapsed_math_saturate_at_zero() {
        let opened = Instant::now();
        assert_eq!(age_ms(opened, opened), 0);
        assert_eq!(age_ms(opened + Duration::from_millis(250), opened), 250);
        assert_eq!(
            age_ms(opened, opened + Duration::from_millis(250)),
            0,
            "a close observed before its open must saturate, not wrap"
        );
        assert_eq!(
            elapsed_us(opened + Duration::from_micros(1_234), opened),
            1_234
        );
        assert_eq!(gap_ms(opened, None), None);
        assert_eq!(
            gap_ms(opened + Duration::from_millis(14), Some(opened)),
            Some(14),
            "the 14 ms Stop→open gap of the field trace is representable"
        );
    }

    #[test]
    fn only_a_successful_close_becomes_the_close_open_anchor() {
        let _guard = COUNTER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        assert_eq!(gap_ms(Instant::now(), None), None);
        record_last_close_success(Instant::now());
        assert!(
            ms_since_last_close().is_some(),
            "after a successful close the process-wide gap is measurable"
        );
    }

    #[test]
    fn live_handle_table_renders_ids_in_ascending_order() {
        let mut table = LiveHandleTable::new();
        assert_eq!(table.render(), "");
        // Inserted out of order on purpose: the rendering must be by id, not by
        // insertion order, so two runs are comparable.
        table.insert(3, HandleKind::Intercept);
        table.insert(2, HandleKind::Inject);
        table.insert(4, HandleKind::Pin);
        assert_eq!(table.render(), "inject:2,intercept:3,pin:4");
        table.forget(3);
        assert_eq!(table.render(), "inject:2,pin:4");
        table.forget(99);
        assert_eq!(
            table.render(),
            "inject:2,pin:4",
            "forgetting a stranger is a no-op"
        );
    }

    #[test]
    fn live_handle_helpers_round_trip_the_global_table() {
        let _guard = COUNTER_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let id = 9_000_001;
        record_live_handle(id, HandleKind::Shadow);
        assert!(
            live_handle_ids()
                .split(',')
                .any(|entry| entry == "shadow:9000001"),
            "a live handle is visible in the rendered table: {}",
            live_handle_ids()
        );
        forget_live_handle(id);
        assert!(
            !live_handle_ids()
                .split(',')
                .any(|entry| entry == "shadow:9000001"),
            "a closed handle disappears from the rendered table"
        );
    }

    #[test]
    fn open_event_carries_the_documented_fields() {
        let event = build_open_event(OpenEventParams {
            id: 42,
            kind: HandleKind::Intercept,
            attempt: 2,
            layer: WinDivertLayer::Network,
            priority: 0,
            flags_word: 0,
            filter: "udp and outbound",
            intent: test_intent(),
            pin_active: true,
            pin_id: Some(1),
            elapsed_us: 1_234,
            ms_since_last_close: Some(500),
            fwp_code: Some("0x8032000A".to_string()),
            owned_handles: "intercept:2,inject:3".to_string(),
            result: lifecycle::OpenResult::Failed,
            snapshot_id: Some("bundle-1".to_string()),
        });
        let value = serde_json::to_value(&event).expect("a handle-open record serializes");
        assert_eq!(value["id"].as_u64(), Some(42), "one id per logical open");
        assert_eq!(value["kind"].as_str(), Some("intercept"));
        assert_eq!(value["attempt"].as_u64(), Some(2));
        assert_eq!(value["layer"].as_str(), Some("Network"));
        assert_eq!(value["priority"].as_i64(), Some(0));
        assert_eq!(value["flags"].as_str(), Some("0x0000"));
        assert_eq!(value["filter"].as_str(), Some("udp and outbound"));
        assert_eq!(value["reason"].as_str(), Some("error_retry"));
        assert_eq!(value["actor"].as_str(), Some("retry"));
        assert_eq!(value["command_id"].as_str(), Some("cmd-7"));
        assert_eq!(value["pin_active"].as_bool(), Some(true));
        assert_eq!(value["pin_id"].as_u64(), Some(1));
        assert_eq!(value["elapsed_us"].as_u64(), Some(1_234));
        assert_eq!(value["ms_since_last_close"].as_u64(), Some(500));
        assert_eq!(value["fwp_code"].as_str(), Some("0x8032000A"));
        assert_eq!(
            value["owned_handles"].as_str(),
            Some("intercept:2,inject:3")
        );
        assert_eq!(value["result"].as_str(), Some("failed"));
        assert_eq!(value["snapshot_id"].as_str(), Some("bundle-1"));

        // A cold start (no successful close yet) and a clean success keep the
        // optional fields absent-or-null rather than inventing a value.
        let cold = build_open_event(OpenEventParams {
            id: 1,
            kind: HandleKind::Pin,
            attempt: 1,
            layer: WinDivertLayer::Network,
            priority: 0,
            flags_word: 0,
            filter: "false",
            intent: lifecycle::Intent::default(),
            pin_active: false,
            pin_id: None,
            elapsed_us: 7,
            ms_since_last_close: None,
            fwp_code: None,
            owned_handles: String::new(),
            result: lifecycle::OpenResult::Ok,
            snapshot_id: None,
        });
        let value = serde_json::to_value(&cold).expect("a cold pin open serializes");
        assert!(value["ms_since_last_close"].is_null());
        assert!(value["fwp_code"].is_null());
        assert!(value["snapshot_id"].is_null());
        assert_eq!(value["kind"].as_str(), Some("pin"));
        assert_eq!(value["result"].as_str(), Some("ok"));
    }

    #[test]
    fn close_event_carries_the_documented_fields() {
        let event = build_close_event(
            3,
            HandleKind::Inject,
            250,
            lifecycle::CloseResult::Ok,
            true,
            "pin:1,intercept:2".to_string(),
        );
        let value = serde_json::to_value(&event).expect("a handle-close record serializes");
        assert_eq!(
            value["id"].as_u64(),
            Some(3),
            "close pairs with the open id"
        );
        assert_eq!(value["kind"].as_str(), Some("inject"));
        assert_eq!(value["close_age_ms"].as_u64(), Some(250));
        assert_eq!(value["result"].as_str(), Some("ok"));
        assert_eq!(value["pin_active"].as_bool(), Some(true));
        assert_eq!(value["owned_handles"].as_str(), Some("pin:1,intercept:2"));

        let failed = build_close_event(
            4,
            HandleKind::Shadow,
            0,
            lifecycle::CloseResult::Failed,
            false,
            String::new(),
        );
        let value = serde_json::to_value(&failed).expect("a failed close serializes");
        assert_eq!(value["result"].as_str(), Some("failed"));
        assert_eq!(value["close_age_ms"].as_u64(), Some(0));
        assert_eq!(value["owned_handles"].as_str(), Some(""));
    }

    #[test]
    fn handle_stats_json_names_every_kind_and_the_live_total() {
        let stats = WindivertHandleStats {
            pin: HandleCounts {
                open_attempts: 1,
                open_successes: 1,
                close_attempts: 1,
                close_successes: 1,
                ..Default::default()
            },
            intercept: HandleCounts {
                open_attempts: 2,
                open_successes: 1,
                open_failures: 1,
                ..Default::default()
            },
            ..Default::default()
        };
        let value = handle_stats_json(&stats);
        assert_eq!(value["pin"]["open_attempts"].as_u64(), Some(1));
        assert_eq!(value["pin"]["open_successes"].as_u64(), Some(1));
        assert_eq!(value["pin"]["live"].as_u64(), Some(0));
        assert_eq!(value["intercept"]["open_failures"].as_u64(), Some(1));
        assert_eq!(value["live_handles"].as_u64(), Some(0));
        for kind in ["pin", "intercept", "inject", "shadow", "legacy"] {
            assert!(value.get(kind).is_some(), "missing {kind} counters");
        }
    }

    // ── Real-driver harness (ignored: needs elevation + WinDivert) ────────

    #[test]
    #[ignore = "requires an elevated Windows shell and the bundled WinDivert driver"]
    fn windivert_pin_lifecycle_keeps_the_driver_loaded() {
        // Run elevated, from a directory containing WinDivert.dll and
        // WinDivert64.sys:
        //   cargo test -p lightspeed-client --features quic,windivert-redirect \
        //     --lib windivert_pin_lifecycle_keeps_the_driver_loaded -- \
        //     --ignored --nocapture
        //
        // This is the real-driver evidence hook for the pinned design: it opens
        // the pin, churns the intercept handle the way normal Stop/start does,
        // and proves the pin (and therefore the loaded driver) survives every
        // cycle. The close/open instrumentation logs are the evidence artifact.
        let before = windivert_handle_stats().pin;
        assert!(!driver_pin_active(), "the harness starts with no pin");

        pin_driver().expect("the driver pin opens");
        assert!(driver_pin_active(), "the pin must be held after pin_driver");
        let first = windivert_handle_stats().pin;
        assert_eq!(first.open_successes - before.open_successes, 1);

        pin_driver().expect("a second pin_driver is an idempotent no-op");
        assert_eq!(
            windivert_handle_stats().pin.open_successes,
            first.open_successes,
            "the second call must not open a second handle"
        );

        for cycle in 0..10 {
            let handle = OwnedHandle::open_with_pin_retry(
                HandleKind::Intercept,
                "udp and outbound and udp.DstPort == 9",
                WinDivertLayer::Network,
                0,
                WinDivertFlags::new(),
            )
            .unwrap_or_else(|e| panic!("intercept open failed on cycle {cycle}: {e}"));
            handle
                .close()
                .unwrap_or_else(|e| panic!("intercept close failed on cycle {cycle}: {e}"));
            assert!(
                driver_pin_active(),
                "the pin must stay open across diverting-handle churn (cycle {cycle})"
            );
        }

        release_driver_pin();
        assert!(!driver_pin_active(), "release must clear the pin");
        release_driver_pin();
        assert!(!driver_pin_active(), "release is idempotent");
        let after = windivert_handle_stats().pin;
        assert_eq!(after.close_successes - before.close_successes, 1);
    }
}
