use super::context::normalize_locale_territory;
use super::*;
use lightspeed_protocol::telemetry::MAX_ROUTE_LEGS;

#[test]
fn test_normalize_locale_territory() {
    assert_eq!(normalize_locale_territory("en-US"), "US");
    assert_eq!(normalize_locale_territory("th_TH"), "TH");
    assert_eq!(normalize_locale_territory("en"), "");
    assert_eq!(normalize_locale_territory(""), "");
    assert_eq!(normalize_locale_territory("en-us-x"), "US");
}

#[tokio::test]
async fn test_telemetry_percentiles() {
    let collector = TelemetryCollector::new();
    // Feed 100 samples: 1..=100 ms
    for i in 1u32..=100 {
        collector.record_rtt(i as f64).await;
    }

    let report = collector.build_report(1, "TH").await.unwrap();
    assert_eq!(report.sample_count, 100);
    // p50 ≈ 50 ms (sorted index 50 of 0..=99)
    assert!((report.p50_ms - 50.0).abs() < 2.0, "p50={}", report.p50_ms);
    // p95 ≈ 95 ms
    assert!((report.p95_ms - 95.0).abs() < 2.0, "p95={}", report.p95_ms);
    // p99 ≈ 99 ms
    assert!((report.p99_ms - 99.0).abs() < 2.0, "p99={}", report.p99_ms);
}

#[tokio::test]
async fn test_telemetry_drains_after_flush() {
    let collector = TelemetryCollector::new();
    collector.record_rtt(30.0).await;
    collector.record_rtt(40.0).await;

    let r1 = collector.build_report(0, "").await;
    assert!(r1.is_some());
    assert_eq!(r1.unwrap().sample_count, 2);

    // Second build should return None — samples were drained.
    let r2 = collector.build_report(0, "").await;
    assert!(r2.is_none());
}

#[tokio::test]
async fn test_fec_counters_reset_after_build() {
    let collector = TelemetryCollector::new();
    collector.record_rtt(20.0).await;
    collector.record_fec_recovery();
    collector.record_fec_recovery();
    collector.record_fec_loss();

    let report = collector.build_report(1, "US").await.unwrap();
    assert_eq!(report.fec_recoveries, 2);
    assert_eq!(report.fec_losses, 1);

    // Counters reset after build.
    assert_eq!(collector.fec_recoveries.load(Ordering::Relaxed), 0);
    assert_eq!(collector.fec_losses.load(Ordering::Relaxed), 0);
}

#[tokio::test]
async fn test_ring_buffer_capped_at_capacity() {
    let collector = TelemetryCollector::new();
    // Overfill the ring buffer by 10 slots.
    for i in 0..(RING_CAPACITY + 10) {
        collector.record_rtt(i as f64).await;
    }
    let inner = collector.inner.lock().await;
    assert_eq!(inner.samples.len(), RING_CAPACITY);
}

/// Two relay legs must produce two `PathObservation` entries whose
/// percentiles are ordered (p50 <= p95 <= p99), and the resulting report
/// must survive the protocol round-trip + no-PII guard.
#[tokio::test]
async fn per_path_stats_reported() {
    let collector = TelemetryCollector::new();
    for i in 1u32..=100 {
        collector.record_rtt(i as f64).await;
    }
    for i in 10u32..=40 {
        collector.record_path_rtt("relay-fra", i as f64);
    }
    for i in 50u32..=90 {
        collector.record_path_rtt("relay-ams", i as f64);
    }
    collector.record_path_lost("relay-fra");
    collector.record_path_lost("relay-fra");
    collector.record_path_dedup("relay-fra");

    let report = collector.build_report(1, "DE").await.unwrap();
    assert_eq!(report.route_legs.len(), 2, "two legs => two observations");

    let fra = report
        .route_legs
        .iter()
        .find(|l| l.relay == "relay-fra")
        .expect("fra leg present");
    let ams = report
        .route_legs
        .iter()
        .find(|l| l.relay == "relay-ams")
        .expect("ams leg present");
    for leg in [fra, ams] {
        assert!(
            leg.rtt_p50_ms <= leg.rtt_p95_ms,
            "p50 <= p95 for {}",
            leg.relay
        );
        assert!(
            leg.rtt_p95_ms <= leg.rtt_p99_ms,
            "p95 <= p99 for {}",
            leg.relay
        );
        assert!(leg.samples > 0, "samples positive for {}", leg.relay);
    }
    assert_eq!(fra.lost, 2, "two recorded losses");
    assert_eq!(fra.dedup_saved, 1, "one recorded dedup");

    let json = serde_json::to_string(&report).unwrap();
    let decoded: TelemetryReport = serde_json::from_str(&json).unwrap();
    assert!(
        decoded.validate().is_ok(),
        "route_legs report must validate: {json}"
    );
    for needle in ["ip", "user", "session", "host"] {
        assert!(
            !json.contains(needle),
            "PII substring {needle:?} leaked into telemetry JSON: {json}"
        );
    }
}

/// More distinct legs than the protocol cap must be bounded to
/// `MAX_ROUTE_LEGS`.
#[tokio::test]
async fn path_count_bounded() {
    let collector = TelemetryCollector::new();
    for _ in 0..10 {
        collector.record_rtt(20.0).await;
    }
    for leg in 0..(MAX_ROUTE_LEGS + 3) {
        let relay = format!("relay-{leg}");
        for i in 0..5u32 {
            collector.record_path_rtt(&relay, 10.0 + i as f64);
        }
    }

    let report = collector.build_report(1, "US").await.unwrap();
    assert!(
        report.route_legs.len() <= MAX_ROUTE_LEGS,
        "legs must be bounded to MAX_ROUTE_LEGS, got {}",
        report.route_legs.len()
    );
    assert!(report.validate().is_ok());

    let second = collector.build_report(1, "US").await;
    assert!(second.is_none(), "legs drain with the global samples");
}
