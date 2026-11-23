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
//! and keeps it open for the app lifetime. Any live WinDivert handle keeps the
//! WinDivert driver/service loaded, so a normal Stop that closes the
//! intercept/inject/shadow handles no longer unloads the driver: the
//! unload/reload race that could leave a WFP filter registered and make the next
//! open fail with `FWP_E_IN_USE` never starts. The pin is closed only by
//! [`release_driver_pin`] (the explicit app-shutdown call, after the diverting
//! handles are closed) or by process exit. It is idempotent and serialized: a
//! second [`pin_driver`] call while a pin is held is a no-op that opens no
//! second handle. This is a *pin*, not a pause gate: the diverting handles are
//! still created and closed by the normal interception lifecycle.
//!
//! When no pin is held and an open fails with `FWP_E_IN_USE`, the open is
//! retried a small, fixed number of times with a bounded backoff (the driver
//! may still be winding down after the previous handle closed). A held pin means
//! the stale state is not a cold start winding down, so that path fails
//! immediately with the actionable remedy instead of retrying forever.
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
//! Windows-only: declared behind
//! `#[cfg(all(target_os = "windows", feature = "windivert-redirect"))]` in
//! `interceptor::mod`, and every dependency it uses is gated with it.

use std::ffi::{c_void, CString};
use std::io;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use windivert_sys::address::WINDIVERT_ADDRESS;
use windivert_sys::{
    WinDivertClose, WinDivertFlags, WinDivertLayer, WinDivertOpen, WinDivertRecv, WinDivertSend,
    WinDivertShutdown, WinDivertShutdownMode,
};

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
//  Bounded FWP_E_IN_USE retry plan
// ─────────────────────────────────────────────────────────────────────────────

/// How many times an open is retried after `FWP_E_IN_USE` when no pin is held.
///
/// Bounded on purpose: a stale WFP state that survives this budget is not a
/// driver path still winding down, and hiding it behind an endless retry would
/// turn a clear failure into a hang.
const FWP_IN_USE_MAX_RETRIES: u32 = 3;

/// Backoff before each retry. Short and strictly bounded: the whole budget is
/// under 600 ms, so a stale state fails fast with the remedy.
const FWP_IN_USE_BACKOFF: [Duration; FWP_IN_USE_MAX_RETRIES as usize] = [
    Duration::from_millis(50),
    Duration::from_millis(150),
    Duration::from_millis(350),
];

/// The backoff before retry number `retry_index` (0-based), or `None` when the
/// retry budget is spent.
fn fwp_in_use_backoff(retry_index: u32) -> Option<Duration> {
    FWP_IN_USE_BACKOFF.get(retry_index as usize).copied()
}

/// Whether an open failure should be retried.
///
/// Only `FWP_E_IN_USE` with NO pin held is worth retrying: with the pin held the
/// driver is already loaded, so the same code is a persistent stale filter from
/// another process and retrying cannot clear it - the caller must fail with the
/// actionable remedy. Every other failure (driver missing, blocked, not
/// elevated) is permanent and is never retried.
fn should_retry_fwp_in_use(err: &io::Error, pin_active: bool) -> bool {
    !pin_active && is_fwp_in_use(err)
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
/// state). `FWP_E_IN_USE` with no pin held is retried a small bounded number of
/// times first; a state that survives the budget is reported, never hidden.
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
    let id = handle.id();
    match handle.close() {
        Ok(()) => {
            tracing::info!("📌 WinDivert driver pin released (id={id}); the driver may now unload")
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
pub(crate) fn driver_open_remedy(err: &io::Error) -> String {
    if is_fwp_in_use(err) {
        return format!(
            "WinDivert has a stale WFP filter from a previous run. Close any stale \
             LightSpeed process and restart Windows to clear it before retrying. ({err})"
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

    /// Open a handle with the bounded `FWP_E_IN_USE` retry.
    ///
    /// One logical open gets one id across its attempts. When no driver pin is
    /// held and the open fails with `FWP_E_IN_USE`, it is retried up to
    /// [`FWP_IN_USE_MAX_RETRIES`] times with [`FWP_IN_USE_BACKOFF`]; the final
    /// failure is returned as-is so the caller reports the actionable remedy.
    /// With a pin held there is no retry: a loaded driver means the stale state
    /// is external and persistent, not a cold start still winding down.
    pub(crate) fn open_with_pin_retry(
        kind: HandleKind,
        filter: &str,
        layer: WinDivertLayer,
        priority: i16,
        flags: WinDivertFlags,
    ) -> io::Result<Self> {
        let id = next_handle_id();
        let mut attempt: u32 = 1;
        loop {
            match Self::open_with_id(id, kind, attempt, filter, layer, priority, flags) {
                Ok(handle) => return Ok(handle),
                Err(err) => {
                    let pin_active = driver_pin_active();
                    match fwp_in_use_backoff(attempt - 1) {
                        Some(backoff) if should_retry_fwp_in_use(&err, pin_active) => {
                            tracing::warn!(
                                "WinDivert handle open retry: id={id} kind={} attempt={}/{} \
                                 after=FWP_E_IN_USE(0x{:08X}) pin_active={pin_active} \
                                 backoff_ms={}",
                                kind.as_str(),
                                attempt + 1,
                                FWP_IN_USE_MAX_RETRIES + 1,
                                err.raw_os_error().unwrap_or(0) as u32,
                                backoff.as_millis(),
                            );
                            std::thread::sleep(backoff);
                            attempt += 1;
                        }
                        _ => return Err(err),
                    }
                }
            }
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
        record_open_attempt(kind);
        let flags_word = u64::from(flags);

        // A NUL filter is rejected before the FFI call, but it is still an open
        // attempt and is counted and logged like any other failure.
        let filter_c = match CString::new(filter) {
            Ok(filter) => filter,
            Err(_) => {
                let err = io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "WinDivert filter contains an interior NUL byte",
                );
                record_open_failure(kind);
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
            tracing::warn!(
                "WinDivert handle open: id={id} kind={} attempt={attempt} layer={layer:?} \
                 priority={priority} flags=0x{flags_word:04x} filter={filter:?} \
                 -> FAILED ({err})",
                kind.as_str(),
            );
            return Err(err);
        }

        record_open_success(kind);
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

        if succeeded(ok) {
            record_close(self.kind, true);
            tracing::info!(
                "WinDivert handle close: id={} kind={} -> ok",
                self.id,
                self.kind.as_str()
            );
            Ok(())
        } else {
            let err = io::Error::last_os_error();
            record_close(self.kind, false);
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
/// (`0x8032000A` or `0xC022000A`) that a leaked WinDivert handle produces.
///
/// Callers use this to print an actionable message (reboot or close the stale
/// process) instead of a raw error code.
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

    #[test]
    fn fwp_in_use_retry_is_bounded_and_off_while_the_pin_holds() {
        let stale = io::Error::from_raw_os_error(0x8032_000A_u32 as i32);
        assert!(
            should_retry_fwp_in_use(&stale, false),
            "no pin: a winding-down driver deserves a bounded retry"
        );
        assert!(
            !should_retry_fwp_in_use(&stale, true),
            "pin held: FWP_E_IN_USE is an external stale filter and must fail \
             with the remedy instead of retrying"
        );
        assert!(
            !should_retry_fwp_in_use(&io::Error::from_raw_os_error(5), false),
            "permission failures are permanent"
        );
        assert!(
            !should_retry_fwp_in_use(&io::Error::from_raw_os_error(1275), false),
            "a blocked driver is permanent"
        );
        assert!(!should_retry_fwp_in_use(
            &io::Error::new(io::ErrorKind::InvalidInput, "not a WFP error"),
            false
        ));

        // The plan is finite: 3 retries, never a fourth, with a non-shrinking
        // backoff and a total budget well under a second.
        let plan: Vec<Duration> = (0..16).filter_map(fwp_in_use_backoff).collect();
        assert_eq!(plan.len(), FWP_IN_USE_MAX_RETRIES as usize);
        assert!(
            fwp_in_use_backoff(FWP_IN_USE_MAX_RETRIES).is_none(),
            "the retry budget must be exhausted, not extended"
        );
        assert!(
            plan.windows(2).all(|pair| pair[1] >= pair[0]),
            "backoff must not shrink: {plan:?}"
        );
        assert!(
            plan.iter().sum::<Duration>() <= Duration::from_millis(600),
            "the whole retry budget stays small: {plan:?}"
        );
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
