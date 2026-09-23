//! Regression: the inbound relayed-RTT hook must respect the telemetry enable
//! flag, exactly like the outbound hook. Kept as an integration test so the
//! process-wide tracker it installs cannot leak into the library unit tests.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use lightspeed_client::latency::{self, Clock, IcmpProber, LatencyTracker};

struct NoopProber;

impl IcmpProber for NoopProber {
    fn probe(&self, _target: Ipv4Addr, _seq: u16) -> Option<f32> {
        None
    }
}

struct ManualClock {
    base: Instant,
    offset_ms: AtomicU64,
}

impl ManualClock {
    fn new() -> Self {
        Self {
            base: Instant::now(),
            offset_ms: AtomicU64::new(0),
        }
    }

    fn advance(&self, duration: Duration) {
        self.offset_ms
            .fetch_add(duration.as_millis() as u64, Ordering::Relaxed);
    }
}

impl Clock for ManualClock {
    fn now(&self) -> Instant {
        self.base + Duration::from_millis(self.offset_ms.load(Ordering::Relaxed))
    }
}

#[test]
fn record_inbound_is_gated_on_telemetry_enabled() {
    let server = Ipv4Addr::new(203, 0, 113, 7);
    let clock = Arc::new(ManualClock::new());
    let tracker = Arc::new(LatencyTracker::new(Arc::new(NoopProber), clock.clone()));
    latency::install_global(Arc::clone(&tracker));
    let installed = latency::global().expect("tracker installed");

    lightspeed_client::telemetry::set_enabled(false);
    installed.note_outbound(server);
    clock.advance(Duration::from_millis(20));
    latency::record_inbound(server);
    assert_eq!(
        installed.relayed_p50_ms(),
        None,
        "disabled telemetry must not record a relayed RTT"
    );

    lightspeed_client::telemetry::set_enabled(true);
    installed.note_outbound(server);
    clock.advance(Duration::from_millis(20));
    latency::record_inbound(server);
    assert_eq!(
        installed.relayed_p50_ms(),
        Some(20.0),
        "enabled telemetry records the relayed RTT"
    );
}
