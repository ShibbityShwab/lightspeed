//! Regression: local latency measurement is independent of telemetry reporting.
//!
//! Measurement (direct ICMP, relayed RTT, shadow direct) feeds local routing
//! and any future do-no-harm bypass decision, so it must run whenever the
//! interceptor/tunnel runs, even with `--no-telemetry`. Reporting to the relay
//! stays strictly opt-in: with telemetry disabled no report is built or sent.
//!
//! Kept as integration tests so the process-wide tracker and the telemetry gate
//! they install cannot leak into the library unit tests.

use std::net::Ipv4Addr;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::time::{Duration, Instant};

use lightspeed_client::latency::{self, Clock, IcmpProber, LatencyTracker, DIRECT_PROBE_COUNT};
use lightspeed_client::telemetry::{self, FlushOutcome, TelemetryCollector};

struct ScriptedProber {
    replies: StdMutex<Vec<Option<f32>>>,
    calls: AtomicUsize,
}

impl ScriptedProber {
    fn new(replies: Vec<Option<f32>>) -> Self {
        Self {
            replies: StdMutex::new(replies),
            calls: AtomicUsize::new(0),
        }
    }
}

impl IcmpProber for ScriptedProber {
    fn probe(&self, _target: Ipv4Addr, _seq: u16) -> Option<f32> {
        self.calls.fetch_add(1, Ordering::Relaxed);
        let mut replies = self.replies.lock().unwrap();
        if replies.is_empty() {
            None
        } else {
            replies.remove(0)
        }
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

const SERVER: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 7);
/// Direct bursts are rate-limited per server, so the ICMP assertion uses its
/// own server to avoid the interval consumed by the relayed leg.
const DIRECT_SERVER: Ipv4Addr = Ipv4Addr::new(203, 0, 113, 8);

/// Given: telemetry reporting is disabled. When: the interceptor/tunnel hooks
/// fire. Then: local measurement is still recorded, because it feeds local
/// decisions and is not sent anywhere without telemetry.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn measurement_runs_with_telemetry_disabled() {
    let clock = Arc::new(ManualClock::new());
    let prober = Arc::new(ScriptedProber::new(vec![Some(50.0); DIRECT_PROBE_COUNT]));
    let tracker = Arc::new(LatencyTracker::new(prober, clock.clone()));
    latency::install_global(Arc::clone(&tracker));
    telemetry::set_enabled(false);
    assert!(!telemetry::is_enabled(), "reporting must be off");

    // Direct ICMP burst: the outbound hook must still kick it off. The burst
    // runs on a blocking thread, so poll rather than assume a fixed delay.
    latency::record_outbound(DIRECT_SERVER);
    let mut direct = None;
    for _ in 0..200 {
        if let Some(median) = tracker.direct_p50_ms() {
            direct = Some(median);
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert_eq!(
        direct,
        Some(50.0),
        "direct ICMP measurement must run without telemetry"
    );

    // Relayed RTT: the inbound hook must still record.
    tracker.note_outbound(SERVER);
    clock.advance(Duration::from_millis(20));
    latency::record_inbound(SERVER);
    assert_eq!(
        tracker.relayed_p50_ms(),
        Some(20.0),
        "relayed measurement must run without telemetry"
    );

    // Shadow direct: the sampler must still claim a slot and record.
    assert!(
        latency::record_shadow_outbound(SERVER),
        "shadow sampling must be due without telemetry"
    );
    clock.advance(Duration::from_millis(15));
    latency::record_shadow_inbound(SERVER);
    assert_eq!(
        tracker.shadow_direct_p50_ms(),
        Some(15.0),
        "shadow direct measurement must run without telemetry"
    );

    assert!(
        !telemetry::is_enabled(),
        "local measurement must never turn reporting on"
    );
}

/// Given: telemetry reporting is disabled. When: a flush is attempted with a
/// sample buffered. Then: flush refuses before building a report or opening a
/// connection, so nothing is sent.
#[tokio::test]
async fn no_report_is_sent_with_telemetry_disabled() {
    let collector = TelemetryCollector::new();
    collector.record_rtt(42.0).await;

    telemetry::set_enabled(false);
    let outcome = collector.flush("127.0.0.1", 0, "").await;

    assert_eq!(
        outcome,
        FlushOutcome::Disabled,
        "a disabled client must not build or send a telemetry report"
    );
}
