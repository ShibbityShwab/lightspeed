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
/// Does NOT install any real kernel rules — purely for testing the
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

        // Use std::thread to wait for shutdown — avoids requiring a Tokio runtime
        std::thread::spawn(move || {
            // Block on the oneshot — this is fine in a dedicated OS thread
            let _ = shutdown_rx.blocking_recv();
            active.store(false, Ordering::Relaxed);
            stop_count.fetch_add(1, Ordering::Relaxed);
        });

        Ok(InterceptorHandle::new(shutdown_tx, counters, "mock"))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::interceptor::traits::{InterceptorConfig, TrafficInterceptor};

    fn test_config() -> InterceptorConfig {
        InterceptorConfig {
            game_name: "TestGame".into(),
            pid: Some(12345),
            port_range: (27015, 27017),
            initial_routes: vec![],
            proxy_addr: SocketAddrV4::new(std::net::Ipv4Addr::new(127, 0, 0, 1), 4434),
            fec_enabled: false,
            fec_k: 4,
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
        assert_eq!(stored.pid, Some(12345));
        assert_eq!(stored.port_range, (27015, 27017));
    }

    #[test]
    fn mock_stop_via_handle() {
        let m = MockInterceptor::new();
        let mut handle = m.start(test_config()).unwrap();
        handle.stop();
        // The stop increments asynchronously — give it a moment
        std::thread::sleep(std::time::Duration::from_millis(50));
        assert_eq!(m.stop_count(), 1);
    }

    #[test]
    fn mock_multiple_starts() {
        let m = MockInterceptor::new();
        let _h1 = m.start(test_config()).unwrap();
        let _h2 = m.start(test_config()).unwrap();
        assert_eq!(m.start_count(), 2);
    }
}
