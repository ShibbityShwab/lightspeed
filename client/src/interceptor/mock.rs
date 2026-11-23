//! Mock TrafficInterceptor for testing.
//!
//! Implements the full `TrafficInterceptor` trait with an in-memory backend
//! that records calls and allows test assertions on the interceptor pipeline
//! without requiring root, kernel modules, or real game processes.

#[cfg(test)]
use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use super::traits::{
    InterceptorConfig, InterceptorCounters, InterceptorHandle, TrafficInterceptor,
};

/// A mock interceptor that records calls for test assertions.
///
/// Does NOT install any real kernel rules, purely for testing the
/// interceptor pipeline (create → check → start → counter increment → stop).
pub struct MockInterceptor {
    /// Whether `check_availability()` should succeed.
    available: bool,
    /// Number of times `start()` was called.
    start_count: Arc<AtomicU64>,
    /// Number of times `stop()` was called.
    stop_count: Arc<AtomicU64>,
    /// Last config passed to `start()`.
    last_config: Arc<Mutex<Option<InterceptorConfig>>>,
    /// Whether the interceptor is currently "active".
    active: Arc<AtomicBool>,
}

impl MockInterceptor {
    /// Create a mock that reports as available.
    pub fn new() -> Self {
        Self {
            available: true,
            start_count: Arc::new(AtomicU64::new(0)),
            stop_count: Arc::new(AtomicU64::new(0)),
            last_config: Arc::new(Mutex::new(None)),
            active: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Create a mock that reports as unavailable.
    pub fn unavailable() -> Self {
        Self {
            available: false,
            ..Self::new()
        }
    }

    /// How many times `start()` has been called.
    pub fn start_count(&self) -> u64 {
        self.start_count.load(Ordering::Relaxed)
    }

    /// How many times `stop()` was called via the handle.
    pub fn stop_count(&self) -> u64 {
        self.stop_count.load(Ordering::Relaxed)
    }

    /// Clone of the last config passed to `start()`.
    pub fn last_config(&self) -> Option<InterceptorConfig> {
        self.last_config.lock().unwrap().clone()
    }
}

impl Default for MockInterceptor {
    fn default() -> Self {
        Self::new()
    }
}

/// A minimal in-memory config for crate-internal tests.
///
/// Shared by the mock's own tests and the command-layer P0-4/T15 tests, so a
/// test that needs "a live run" never has to open a driver handle.
#[cfg(test)]
pub(crate) fn test_config() -> InterceptorConfig {
    InterceptorConfig {
        game_name: "TestGame".into(),
        dynamic_server: false,
        transport: crate::interceptor::TransportProto::Udp,
        capture_tcp_ports: None,
        process_names: vec!["TestGame.exe".to_string()],
        process_target: None,
        port_range: (27015, 27017),
        path_mtu: crate::tunnel::pmtud::CONSERVATIVE_PATH_MTU,
        initial_routes: vec![],
        proxy_addr: SocketAddrV4::new(std::net::Ipv4Addr::new(127, 0, 0, 1), 4434),
        start_held: false,
        fec_enabled: false,
        fec_k: 4,
        adaptive_fec: Default::default(),
        bypass: Default::default(),
    }
}

/// Test-only observation helpers for the mock's asynchronous shutdown.
///
/// `stop()` only signals the mock's dedicated shutdown thread, which flips
/// `active` and increments `stop_count` after it wakes. Tests used to
/// `sleep(50ms)` and hope; a loaded CI worker could deschedule that thread past
/// the sleep and fail the assertion (WF-081b task-73/76). Waiting on the
/// observable counter with a bounded deadline is deterministic under any thread
/// count and still fails (with the observed value) if the thread never runs.
#[cfg(test)]
impl MockInterceptor {
    /// Upper bound for a shutdown observation in tests: generous against a
    /// loaded CI worker, small enough that a real hang still fails fast.
    pub const STOP_DEADLINE: std::time::Duration = std::time::Duration::from_secs(5);

    /// Poll interval while waiting: short enough to be instant when the
    /// shutdown thread is already done, cheap enough for a bounded wait.
    const STOP_POLL: std::time::Duration = std::time::Duration::from_millis(2);

    /// Block until at least `expected` stops have been observed, or the bounded
    /// deadline elapses; returns the last observed count either way.
    pub fn wait_for_stop_count(&self, expected: u64) -> u64 {
        let started = std::time::Instant::now();
        loop {
            let seen = self.stop_count();
            if seen >= expected || started.elapsed() >= Self::STOP_DEADLINE {
                return seen;
            }
            std::thread::sleep(Self::STOP_POLL);
        }
    }
}

impl TrafficInterceptor for MockInterceptor {
    fn platform_name(&self) -> &'static str {
        "mock"
    }

    fn check_availability(&self) -> Result<(), String> {
        if self.available {
            Ok(())
        } else {
            Err("Mock interceptor is configured as unavailable".into())
        }
    }

    fn start(&self, config: InterceptorConfig) -> anyhow::Result<InterceptorHandle> {
        self.start_count.fetch_add(1, Ordering::Relaxed);
        *self.last_config.lock().unwrap() = Some(config.clone());
        self.active.store(true, Ordering::Relaxed);

        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let active = Arc::clone(&self.active);
        let stop_count = Arc::clone(&self.stop_count);
        let counters = Arc::new(InterceptorCounters::default());

        // Use std::thread to wait for shutdown, avoids requiring a Tokio runtime
        std::thread::spawn(move || {
            // Block on the oneshot, this is fine in a dedicated OS thread
            let _ = shutdown_rx.blocking_recv();
            active.store(false, Ordering::Relaxed);
            stop_count.fetch_add(1, Ordering::Relaxed);
        });

        let mut handle = InterceptorHandle::new(shutdown_tx, counters, "mock", None);
        // P0-4 / T12: mirror the one platform behaviour the engine-level hold
        // test needs without a driver. A caller that asked for the hold gets a
        // gate that is already closed; a caller that did not (every existing
        // test) keeps the backend's "no gate" answer, exactly as Linux/macOS
        // report and as the mock always did.
        if config.start_held {
            handle.attach_tunnelling_switch(Arc::new(AtomicBool::new(false)));
        }
        Ok(handle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interceptor::traits::{InterceptorConfig, ProcessTarget, TrafficInterceptor};

    fn test_config() -> InterceptorConfig {
        InterceptorConfig {
            game_name: "TestGame".into(),
            dynamic_server: false,
            transport: crate::interceptor::TransportProto::Udp,
            capture_tcp_ports: None,
            process_names: vec!["TestGame.exe".to_string()],
            process_target: Some(ProcessTarget::by_pid(12345)),
            port_range: (27015, 27017),
            path_mtu: crate::tunnel::pmtud::CONSERVATIVE_PATH_MTU,
            initial_routes: vec![],
            proxy_addr: SocketAddrV4::new(std::net::Ipv4Addr::new(127, 0, 0, 1), 4434),
            start_held: false,
            fec_enabled: false,
            fec_k: 4,
            adaptive_fec: Default::default(),
            bypass: Default::default(),
        }
    }

    #[test]
    fn mock_platform_name() {
        let m = MockInterceptor::new();
        assert_eq!(m.platform_name(), "mock");
    }

    #[test]
    fn mock_available() {
        let m = MockInterceptor::new();
        assert!(m.check_availability().is_ok());
    }

    #[test]
    fn mock_unavailable() {
        let m = MockInterceptor::unavailable();
        assert!(m.check_availability().is_err());
    }

    #[test]
    fn mock_start_increments_count() {
        let m = MockInterceptor::new();
        assert_eq!(m.start_count(), 0);
        let _h = m.start(test_config()).unwrap();
        assert_eq!(m.start_count(), 1);
    }

    #[test]
    fn mock_start_stores_config() {
        let m = MockInterceptor::new();
        let cfg = test_config();
        let _h = m.start(cfg.clone()).unwrap();
        let stored = m.last_config().unwrap();
        assert_eq!(stored.game_name, "TestGame");
        assert_eq!(stored.process_target, Some(ProcessTarget::by_pid(12345)));
        assert_eq!(stored.port_range, (27015, 27017));
    }

    #[test]
    fn mock_stop_via_handle() {
        let m = MockInterceptor::new();
        let mut handle = m.start(test_config()).unwrap();
        handle.stop();
        // Observe the asynchronous stop with a bounded wait instead of a fixed
        // sleep (task-73/76): the shutdown thread is the observation point.
        let seen = m.wait_for_stop_count(1);
        assert_eq!(seen, 1, "the mock shutdown thread must observe the stop");
        assert_eq!(m.stop_count(), 1);
    }

    #[test]
    fn mock_multiple_starts() {
        let m = MockInterceptor::new();
        let _h1 = m.start(test_config()).unwrap();
        let _h2 = m.start(test_config()).unwrap();
        assert_eq!(m.start_count(), 2);
    }

    #[test]
    fn a_held_start_attaches_a_closed_gate_and_an_unheld_start_stays_gateless() {
        // P0-4 / T12: the hold must exist *before* the start returns, so a
        // packet cannot be classified against an open gate in the window the
        // GUI's old post-start `hold_tunnelling()` call left. A start that did
        // not ask for the hold keeps the backend's "no gate" answer (the
        // Linux/macOS behaviour), which is what the caller must be able to see.
        let m = MockInterceptor::new();
        let mut held_config = test_config();
        held_config.start_held = true;
        let held = m.start(held_config).expect("held mock start");
        assert_eq!(
            held.tunnelling_enabled(),
            Some(false),
            "a held start must return a CLOSED gate, not a gate the caller must close later"
        );
        assert!(held.set_tunnelling_enabled(true));
        assert_eq!(held.tunnelling_enabled(), Some(true));

        let plain = m.start(test_config()).expect("unheld mock start");
        assert_eq!(
            plain.tunnelling_enabled(),
            None,
            "an unheld start must keep the mock gateless, as before"
        );
    }
}
