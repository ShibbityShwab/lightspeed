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
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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
    /// Auto-detected (or pre-configured) server address.
    pub detected_server: std::sync::Mutex<Option<SocketAddrV4>>,
}

impl Default for InterceptorCounters {
    fn default() -> Self {
        Self {
            packets_intercepted: AtomicU64::new(0),
            bytes_intercepted:   AtomicU64::new(0),
            packets_injected:    AtomicU64::new(0),
            bytes_injected:      AtomicU64::new(0),
            packets_from_proxy:  AtomicU64::new(0),
            errors:              AtomicU64::new(0),
            detected_server:     std::sync::Mutex::new(None),
        }
    }
}

impl InterceptorCounters {
    /// Take a cheap snapshot for GUI display.
    pub fn snapshot(&self, platform: &'static str) -> InterceptorStats {
        InterceptorStats {
            packets_intercepted: self.packets_intercepted.load(Ordering::Relaxed),
            bytes_intercepted:   self.bytes_intercepted.load(Ordering::Relaxed),
            packets_injected:    self.packets_injected.load(Ordering::Relaxed),
            bytes_injected:      self.bytes_injected.load(Ordering::Relaxed),
            packets_from_proxy:  self.packets_from_proxy.load(Ordering::Relaxed),
            errors:              self.errors.load(Ordering::Relaxed),
            detected_server: self
                .detected_server
                .lock()
                .map(|g| *g)
                .unwrap_or(None),
            platform,
        }
    }
}

/// Cheap snapshot of interceptor state for GUI display.
#[derive(Clone, Debug, Default)]
pub struct InterceptorStats {
    pub packets_intercepted: u64,
    pub bytes_intercepted:   u64,
    pub packets_injected:    u64,
    pub bytes_injected:      u64,
    pub packets_from_proxy:  u64,
    pub errors:              u64,
    /// The game server address discovered at runtime (or pre-configured).
    pub detected_server: Option<SocketAddrV4>,
    /// Platform name: `"WinDivert"`, `"nftables"`, `"pf"`, …
    pub platform: &'static str,
}

// ─────────────────────────────────────────────────────────────────────────────
//  Live handle returned by start()
// ─────────────────────────────────────────────────────────────────────────────

/// Handle to an active interceptor session, returned by `TrafficInterceptor::start()`.
///
/// Dropping this handle calls `stop()` (sends the shutdown signal) but does
/// NOT block — background tasks clean up asynchronously.
pub struct InterceptorHandle {
    shutdown_tx: Option<oneshot::Sender<()>>,
    pub counters: Arc<InterceptorCounters>,
    platform: &'static str,
}

impl InterceptorHandle {
    pub(super) fn new(
        shutdown_tx: oneshot::Sender<()>,
        counters: Arc<InterceptorCounters>,
        platform: &'static str,
    ) -> Self {
        Self {
            shutdown_tx: Some(shutdown_tx),
            counters,
            platform,
        }
    }

    /// Snapshot the live counters — cheap, no lock held.
    pub fn snapshot(&self) -> InterceptorStats {
        self.counters.snapshot(self.platform)
    }

    /// Send the shutdown signal.  Background tasks will exit gracefully.
    pub fn stop(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
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
        Self { reason: reason.into() }
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