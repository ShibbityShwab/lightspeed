//! Core OOP trait + supporting types for the TrafficInterceptor framework.
//!
//! Every platform (Windows WinDivert, Linux nftables, macOS pf) implements
//! `TrafficInterceptor` so the engine can drive them through a single interface.
//!
//! ## Design goals
//!
//! - **Zero ongoing cost**: All implementations use OS-native free tools.
//! - **Automatic**: Optionally target by PID so no manual server-IP entry is needed.
//! - **Secure**: Kernel-level interception on all platforms.
//! - **IP-transparent**: Game server always sees the user's real IP.
//! - **Recoverable**: Stale routes reset automatically.

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

use super::bypass::BypassConfig;
use super::teardown::TeardownAck;
use crate::tunnel::adaptive::AdaptiveConfig;
use tokio::sync::oneshot;

// ─────────────────────────────────────────────────────────────────────────────
//  Route & Process types
// ─────────────────────────────────────────────────────────────────────────────

/// Transport protocol of an active socket.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TransportProtocol {
    Udp,
    Tcp,
}

impl std::fmt::Display for TransportProtocol {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Udp => write!(f, "UDP"),
            Self::Tcp => write!(f, "TCP"),
        }
    }
}

/// A single active network connection observed from a game process.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct Route {
    /// Local socket address (game client side).
    pub local: SocketAddrV4,
    /// Remote socket address (game server).
    pub remote: SocketAddrV4,
    /// Transport protocol.
    pub proto: TransportProtocol,
}

impl std::fmt::Display for Route {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {} → {}", self.proto, self.local, self.remote)
    }
}

/// A running game process together with its observed network routes.
#[derive(Clone, Debug)]
pub struct ProcessInfo {
    /// OS process ID.
    pub pid: u32,
    /// Process image name (e.g. `"RustClient.exe"`).
    pub name: String,
    /// Active UDP (and optionally TCP) connections belonging to this PID.
    pub routes: Vec<Route>,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Interceptor config
// ─────────────────────────────────────────────────────────────────────────────

/// Full configuration passed to `TrafficInterceptor::start()`.
#[derive(Clone, Debug)]
pub struct InterceptorConfig {
    /// Human-readable game name (for logging).
    pub game_name: String,

    /// OS process ID to target precisely, if already known.
    ///
    /// On Windows, WinDivert can filter by `processId == N` — zero false
    /// positives even on crowded game-server ports.  Leave `None` to use the
    /// broader port-range filter.
    pub pid: Option<u32>,

    /// UDP port range to intercept when no specific server IP is known.
    ///
    /// Used as the WinDivert filter seed before the server IP is auto-detected
    /// from traffic.  Format: `(low_port, high_port)`.
    pub port_range: (u16, u16),

    /// Pre-discovered server routes from the [`ProcessScanner`].
    ///
    /// If non-empty the interceptor can immediately lock onto the right server
    /// rather than waiting for the debounce detector to accumulate N packets.
    pub initial_routes: Vec<Route>,

    /// LightSpeed proxy address to relay intercepted traffic through.
    pub proxy_addr: SocketAddrV4,

    /// Forward Error Correction enabled?
    pub fec_enabled: bool,

    /// FEC block size (K data packets per parity packet).
    pub fec_k: u8,

    /// Adaptive FEC/duplication policy for this path. Disabled by default,
    /// which keeps the fixed `fec_k` block with parity on every block.
    pub adaptive_fec: AdaptiveConfig,

    /// Do-no-harm bypass gate configuration.
    pub bypass: BypassConfig,
}

impl InterceptorConfig {
    /// Whether the configured game rotates through ephemeral server addresses.
    ///
    /// Derived from the game profile by display name. Unknown names default to
    /// `false`, preserving the legacy lock-onto-first-route behaviour.
    pub fn dynamic_server(&self) -> bool {
        crate::games::dynamic_server_for_name(&self.game_name)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Counters & stats
// ─────────────────────────────────────────────────────────────────────────────

/// Shared atomic counters owned by the interceptor task.
///
/// The engine polls these (via `InterceptorHandle::snapshot()`) every GUI frame
/// without taking a mutex.
#[derive(Debug)]
pub struct InterceptorCounters {
    /// Outbound game packets intercepted and forwarded to proxy.
    pub packets_intercepted: AtomicU64,
    /// Bytes intercepted (outbound).
    pub bytes_intercepted: AtomicU64,
    /// Spoofed inbound packets injected back to the game.
    pub packets_injected: AtomicU64,
    /// Bytes injected (inbound).
    pub bytes_injected: AtomicU64,
    /// Proxy responses received.
    pub packets_from_proxy: AtomicU64,
    /// Errors (intercept or inject failures).
    pub errors: AtomicU64,
    /// Game payloads forwarded with fragmentation allowed because they
    /// exceeded the conservative tunnel payload budget.
    pub payloads_over_budget: AtomicU64,
    /// Current inline adaptive parity-to-data ratio in basis points
    /// (2500 = 1/4). Zero while adaptive FEC is off or parity is suppressed.
    pub adaptive_parity_ratio_bp: AtomicU64,
    /// Times the bypass gate decided Direct and the action layer honoured it.
    pub bypass_allowed: AtomicU64,
    /// Times a bypass was computed but not applied (dry run, fail-open, or an
    /// unsafe mid-flow teardown), so the relay was kept.
    pub bypass_refused: AtomicU64,
    /// Bypass-gate evaluations that produced a decision.
    pub bypass_decisions: AtomicU64,
    /// Relay<->Direct state transitions recorded by the gate.
    pub bypass_flips: AtomicU64,
    /// Pre-gate evaluations that received a fresh client->relay RTT to
    /// compare against the direct path (auto/dry_run).
    pub bypass_pre_gate_rtt: AtomicU64,
    /// Auto-detected (or pre-configured) server address.
    /// SAFETY: This `std::sync::Mutex` is used from async tasks but the lock
    /// is never held across an await point.  If that changes, migrate to
    /// `tokio::sync::Mutex` and make `snapshot()` async.
    pub detected_server: std::sync::Mutex<Option<SocketAddrV4>>,
    /// Human-readable description of the most recent fatal error, if any.
    pub last_error: std::sync::Mutex<Option<String>>,
}

impl Default for InterceptorCounters {
    fn default() -> Self {
        Self {
            packets_intercepted: AtomicU64::new(0),
            bytes_intercepted: AtomicU64::new(0),
            packets_injected: AtomicU64::new(0),
            bytes_injected: AtomicU64::new(0),
            packets_from_proxy: AtomicU64::new(0),
            errors: AtomicU64::new(0),
            payloads_over_budget: AtomicU64::new(0),
            adaptive_parity_ratio_bp: AtomicU64::new(0),
            bypass_allowed: AtomicU64::new(0),
            bypass_refused: AtomicU64::new(0),
            bypass_decisions: AtomicU64::new(0),
            bypass_flips: AtomicU64::new(0),
            bypass_pre_gate_rtt: AtomicU64::new(0),
            detected_server: std::sync::Mutex::new(None),
            last_error: std::sync::Mutex::new(None),
        }
    }
}

impl InterceptorCounters {
    /// Take a cheap snapshot for GUI display.
    pub fn snapshot(&self, platform: &'static str) -> InterceptorStats {
        InterceptorStats {
            packets_intercepted: self.packets_intercepted.load(Ordering::Relaxed),
            bytes_intercepted: self.bytes_intercepted.load(Ordering::Relaxed),
            packets_injected: self.packets_injected.load(Ordering::Relaxed),
            bytes_injected: self.bytes_injected.load(Ordering::Relaxed),
            packets_from_proxy: self.packets_from_proxy.load(Ordering::Relaxed),
            errors: self.errors.load(Ordering::Relaxed),
            payloads_over_budget: self.payloads_over_budget.load(Ordering::Relaxed),
            adaptive_parity_ratio_bp: self.adaptive_parity_ratio_bp.load(Ordering::Relaxed),
            bypass_allowed: self.bypass_allowed.load(Ordering::Relaxed),
            bypass_refused: self.bypass_refused.load(Ordering::Relaxed),
            bypass_decisions: self.bypass_decisions.load(Ordering::Relaxed),
            bypass_flips: self.bypass_flips.load(Ordering::Relaxed),
            bypass_pre_gate_rtt: self.bypass_pre_gate_rtt.load(Ordering::Relaxed),
            detected_server: self.detected_server.lock().map(|g| *g).unwrap_or(None),
            last_error: self.last_error.lock().ok().and_then(|g| g.clone()),
            platform,
        }
    }
}

/// Cheap snapshot of interceptor state for GUI display.
#[derive(Clone, Debug, Default)]
pub struct InterceptorStats {
    pub packets_intercepted: u64,
    pub bytes_intercepted: u64,
    pub packets_injected: u64,
    pub bytes_injected: u64,
    pub packets_from_proxy: u64,
    pub errors: u64,
    /// Payloads forwarded with fragmentation allowed for exceeding the
    /// conservative tunnel payload budget.
    pub payloads_over_budget: u64,
    /// Current inline adaptive parity-to-data ratio in basis points.
    pub adaptive_parity_ratio_bp: u64,
    /// Times the bypass gate decided Direct and it was honoured.
    pub bypass_allowed: u64,
    /// Times a bypass was computed but not applied, so the relay was kept.
    pub bypass_refused: u64,
    /// Bypass-gate evaluations that produced a decision.
    pub bypass_decisions: u64,
    /// Relay<->Direct state transitions recorded by the gate.
    pub bypass_flips: u64,
    /// Pre-gate evaluations that received a fresh client->relay RTT.
    pub bypass_pre_gate_rtt: u64,
    /// The game server address discovered at runtime (or pre-configured).
    pub detected_server: Option<SocketAddrV4>,
    /// Description of the most recent fatal error, if any.
    pub last_error: Option<String>,
    /// Platform name: `"WinDivert"`, `"nftables"`, `"pf"`, …
    pub platform: &'static str,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Live handle returned by start()
// ─────────────────────────────────────────────────────────────────────────────

/// Platform-specific teardown wiring for a running interceptor.
///
/// A platform-neutral [`InterceptorHandle`] cannot release a WinDivert receive
/// thread parked in a blocking `recv`, and it cannot know when every owner
/// thread has stopped touching the driver handle. A backend that has those
/// needs bundles them here:
///
/// - `unblock` releases the parked thread (for example `WinDivertShutdown`).
///   It runs at most once, from [`InterceptorHandle::stop`].
/// - `ack` completes when every owner thread confirmed it can no longer touch
///   the handle, so the driver state can be released deterministically.
///
/// Backends without blocking receive threads (Linux, macOS, mock) pass `None`
/// to [`InterceptorHandle::new`] and keep the signalling-only behaviour.
pub struct PlatformTeardown {
    unblock: Box<dyn FnOnce() + Send>,
    ack: TeardownAck,
}

impl PlatformTeardown {
    /// Bundle an unblock hook with the acknowledgement of its owner threads.
    pub(super) fn new(unblock: impl FnOnce() + Send + 'static, ack: TeardownAck) -> Self {
        Self {
            unblock: Box::new(unblock),
            ack,
        }
    }
}

/// Handle to an active interceptor session, returned by `TrafficInterceptor::start()`.
///
/// Dropping this handle calls `stop()` (it sends the shutdown signal and
/// releases any thread parked in a blocking platform receive) but does NOT
/// block: background tasks clean up asynchronously. Call
/// [`stop_and_wait`](Self::stop_and_wait) when the caller must know the
/// platform resources were actually released.
pub struct InterceptorHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    pub counters: Arc<InterceptorCounters>,
    platform: &'static str,
    /// Releases a thread parked in a blocking platform receive. Taken on the
    /// first `stop()`, so it never runs twice.
    unblock: Option<Box<dyn FnOnce() + Send>>,
    /// Completes once every platform owner thread acknowledged that it stopped
    /// touching its handle.
    teardown_ack: Option<TeardownAck>,
}

impl InterceptorHandle {
    pub(super) fn new(
        shutdown_tx: oneshot::Sender<()>,
        counters: Arc<InterceptorCounters>,
        platform: &'static str,
        teardown: Option<PlatformTeardown>,
    ) -> Self {
        let (unblock, teardown_ack) = match teardown {
            Some(teardown) => (Some(teardown.unblock), Some(teardown.ack)),
            None => (None, None),
        };
        Self {
            shutdown_tx: Some(shutdown_tx),
            counters,
            platform,
            unblock,
            teardown_ack,
        }
    }

    /// Snapshot the live counters — cheap, no lock held.
    pub fn snapshot(&self) -> InterceptorStats {
        self.counters.snapshot(self.platform)
    }

    /// Send the shutdown signal and release any thread parked in a blocking
    /// platform receive. Background tasks exit asynchronously; use
    /// [`stop_and_wait`](Self::stop_and_wait) to wait for them.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        // Taking the hook here means mixing `stop()` and `stop_and_wait()`
        // still runs it exactly once.
        if let Some(unblock) = self.unblock.take() {
            unblock();
        }
    }

    /// Stop intercepting and wait up to `timeout` for every platform owner
    /// thread to release its handle.
    ///
    /// Returns `true` immediately when the platform needs no teardown
    /// acknowledgement (Linux, macOS, mock), and `true` when every owner
    /// thread acknowledged within `timeout`. Returns `false` on timeout: a
    /// platform thread may still be touching its handle, so the caller must
    /// not assume the kernel filter was released.
    pub fn stop_and_wait(&mut self, timeout: Duration) -> bool {
        self.stop();
        match self.teardown_ack.take() {
            Some(ack) => ack.wait(timeout),
            None => true,
        }
    }

    /// Whether the shutdown signal has NOT been sent yet.
    pub fn is_active(&self) -> bool {
        self.shutdown_tx.is_some()
    }
}

impl Drop for InterceptorHandle {
    fn drop(&mut self) {
        self.stop();
    }
}

// ─────────────────────────────────────────────────────────────────────────────
//  Core trait
// ─────────────────────────────────────────────────────────────────────────────

/// Platform-specific traffic interception.
///
/// Implementations intercept outbound game-UDP packets at the OS/kernel level,
/// tunnel them through the LightSpeed proxy, and inject spoofed server→game
/// responses — all transparently.
///
/// # Platform map
///
/// | Platform | Mechanism                         | Precision        |
/// |----------|-----------------------------------|------------------|
/// | Windows  | WinDivert (NDIS kernel driver)    | Per-process PID  |
/// | Linux    | nftables TPROXY / iptables REDIRECT | cgroup or UID  |
/// | macOS    | pfctl `rdr-to` anchor             | Port-based        |
///
/// All implementations preserve the user's real IP end-to-end (NOT a VPN —
/// the game server always sees the client's original source address).
pub trait TrafficInterceptor: Send + Sync {
    /// Start intercepting traffic according to `config`.
    ///
    /// Spawns async/blocking background tasks and returns immediately.
    /// The returned [`InterceptorHandle`] carries live counters and a shutdown
    /// signal. Dropping the handle stops interception.
    fn start(&self, config: InterceptorConfig) -> anyhow::Result<InterceptorHandle>;

    /// Human-readable platform identifier, e.g. `"WinDivert"`.
    fn platform_name(&self) -> &'static str;

    /// Whether this interceptor can run on the current OS + feature set.
    ///
    /// Returns `Err` with a human-readable explanation if unavailable.
    fn check_availability(&self) -> Result<(), String>;
}

// ─────────────────────────────────────────────────────────────────────────────
//  Stub for unsupported platforms
// ─────────────────────────────────────────────────────────────────────────────

/// A `TrafficInterceptor` that always fails — used when no backend is compiled in.
pub struct UnsupportedInterceptor {
    reason: String,
}

impl UnsupportedInterceptor {
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl TrafficInterceptor for UnsupportedInterceptor {
    fn start(&self, _config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        anyhow::bail!("{}", self.reason)
    }

    fn platform_name(&self) -> &'static str {
        "unsupported"
    }

    fn check_availability(&self) -> Result<(), String> {
        Err(self.reason.clone())
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicUsize;

    use super::*;

    fn config_for(game_name: &str) -> InterceptorConfig {
        InterceptorConfig {
            game_name: game_name.to_string(),
            pid: None,
            port_range: (1, 2),
            initial_routes: vec![],
            proxy_addr: "127.0.0.1:4434".parse().unwrap(),
            fec_enabled: false,
            fec_k: 4,
            adaptive_fec: Default::default(),
            bypass: Default::default(),
        }
    }

    fn handle_with(teardown: Option<PlatformTeardown>) -> InterceptorHandle {
        let (shutdown_tx, _shutdown_rx) = oneshot::channel();
        InterceptorHandle::new(
            shutdown_tx,
            Arc::new(InterceptorCounters::default()),
            "test",
            teardown,
        )
    }

    #[test]
    fn dynamic_server_follows_the_game_profile() {
        assert!(config_for("Fortnite").dynamic_server());
        assert!(!config_for("Rust").dynamic_server());
        assert!(!config_for("SmokeTest").dynamic_server());
    }

    #[test]
    fn stop_and_wait_without_teardown_returns_true() {
        let mut handle = handle_with(None);
        assert!(handle.stop_and_wait(Duration::from_millis(50)));
        assert!(!handle.is_active());
    }

    #[test]
    fn stop_and_wait_waits_for_the_owner_thread_to_ack() {
        let (ack_tx, ack) = TeardownAck::new(1);
        let (release_tx, release_rx) = std::sync::mpsc::channel::<()>();
        let owner = std::thread::spawn(move || {
            let _ = release_rx.recv();
            let _ = ack_tx.send(());
        });
        let teardown = PlatformTeardown::new(
            move || {
                let _ = release_tx.send(());
            },
            ack,
        );
        let mut handle = handle_with(Some(teardown));

        assert!(handle.stop_and_wait(Duration::from_millis(500)));

        owner.join().expect("owner thread must finish");
    }

    #[test]
    fn unblock_hook_runs_exactly_once_across_stop_and_stop_and_wait() {
        let calls = Arc::new(AtomicUsize::new(0));
        let calls_hook = Arc::clone(&calls);
        let (ack_tx, ack) = TeardownAck::new(1);
        let teardown = PlatformTeardown::new(
            move || {
                calls_hook.fetch_add(1, Ordering::Relaxed);
                let _ = ack_tx.send(());
            },
            ack,
        );
        let mut handle = handle_with(Some(teardown));

        handle.stop();
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "stop() must run the unblock hook"
        );

        assert!(handle.stop_and_wait(Duration::from_millis(50)));
        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "the unblock hook must never run twice"
        );
    }

    #[test]
    fn stop_and_wait_returns_false_when_the_ack_never_arrives() {
        let (ack_tx, ack) = TeardownAck::new(1);
        let teardown = PlatformTeardown::new(|| {}, ack);
        let mut handle = handle_with(Some(teardown));

        assert!(!handle.stop_and_wait(Duration::from_millis(30)));

        // The sender stayed alive for the whole wait, so the failure is the
        // timeout, not an early channel disconnect.
        drop(ack_tx);
    }
}
