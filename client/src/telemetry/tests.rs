use super::context::normalize_locale_territory;
use super::*;
use lightspeed_protocol::telemetry::MAX_ROUTE_LEGS;

/// Serializes the tests that toggle the process-wide telemetry gate, since the
/// gate is shared by every test in this binary.
static TELEMETRY_GATE_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
async fn test_telemetry_peek_repeats_until_commit() {
    let collector = TelemetryCollector::new();
    collector.record_rtt(30.0).await;
    collector.record_rtt(40.0).await;

    let first = collector.build_report(0, "").await;
    assert_eq!(first.map(|r| r.sample_count), Some(2));

    // Peeking again before commit still sees the same samples.
    let second = collector.build_report(0, "").await;
    assert_eq!(second.as_ref().map(|r| r.sample_count), Some(2));

    collector
        .commit_report(&second.expect("report to commit"))
        .await;

    // Only the commit drains.
    let third = collector.build_report(0, "").await;
    assert!(third.is_none());
}

#[tokio::test]
async fn failed_post_retains_samples() {
    let _gate = TELEMETRY_GATE_LOCK.lock().await;
    let collector = TelemetryCollector::new();
    collector.record_rtt(42.0).await;

    // Reporting is opt-in, so enable it for this send attempt. Nothing listens
    // on 127.0.0.1:8080 in the test environment, so the POST fails; the samples
    // it carried must survive for the next flush.
    set_enabled(true);
    let outcome = collector.flush("127.0.0.1", 0, "").await;
    set_enabled(false);
    assert_eq!(outcome, FlushOutcome::SendFailed);

    let report = collector.build_report(0, "").await;
    assert_eq!(
        report.map(|r| r.sample_count),
        Some(1),
        "a failed POST must not drop the samples it drained"
    );
}

/// Given: a control-plane address with no supervised connection. When: a flush
/// runs preferring QUIC. Then: it falls back to HTTP rather than dropping the
/// report, and a total failure retains the samples for the next flush.
#[tokio::test]
async fn control_plane_unavailable_falls_back_to_http() {
    let _gate = TELEMETRY_GATE_LOCK.lock().await;
    let collector = TelemetryCollector::new();
    collector.record_rtt(12.0).await;

    let addr = std::net::SocketAddrV4::new(std::net::Ipv4Addr::LOCALHOST, 4434);
    set_enabled(true);
    let outcome = collector
        .flush_preferring_control("127.0.0.1", Some(addr), 0, "")
        .await;
    set_enabled(false);

    assert_eq!(
        outcome,
        FlushOutcome::SendFailed,
        "no control connection: the HTTP fallback must be attempted"
    );
    let report = collector.build_report(0, "").await;
    assert_eq!(
        report.map(|r| r.sample_count),
        Some(1),
        "a failed flush must not drop the samples it drained"
    );
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
    // A peek does not consume the counters.
    assert_eq!(collector.fec_recoveries.load(Ordering::Relaxed), 2);
    assert_eq!(collector.fec_losses.load(Ordering::Relaxed), 1);

    collector.commit_report(&report).await;
    assert_eq!(collector.fec_recoveries.load(Ordering::Relaxed), 0);
    assert_eq!(collector.fec_losses.load(Ordering::Relaxed), 0);
}

/// Given: explicit latency values including a direct application RTT. When: a
/// report is built. Then: the direct-app value is carried through and survives
/// validation and the JSON round trip.
#[tokio::test]
async fn build_report_carries_direct_app_p50() {
    let collector = TelemetryCollector::new();
    collector.record_rtt(30.0).await;

    let report = collector
        .build_report_with_latency(1, "TH", None, Some(31.0), Some(47.5))
        .await
        .expect("report");

    assert_eq!(report.direct_app_p50_ms, Some(47.5));
    assert_eq!(report.relayed_p50_ms, Some(31.0));
    assert!(report.validate().is_ok(), "report must validate");

    let json = serde_json::to_string(&report).unwrap();
    let decoded: TelemetryReport = serde_json::from_str(&json).unwrap();
    assert_eq!(decoded.direct_app_p50_ms, Some(47.5));
}

/// Given: no RTT ring samples at all. When: a direct application RTT is
/// present. Then: a report is still produced.
#[tokio::test]
async fn direct_app_alone_produces_a_report() {
    let collector = TelemetryCollector::new();
    let report = collector
        .build_report_with_latency(1, "US", None, None, Some(20.0))
        .await
        .expect("a direct-app sample must produce a report with no RTT ring samples");

    assert_eq!(report.direct_app_p50_ms, Some(20.0));
    assert!(report.validate().is_ok());
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
    assert!(
        second.is_some(),
        "a peek repeats the legs until the report is committed"
    );

    collector
        .commit_report(&second.expect("report to commit"))
        .await;
    let third = collector.build_report(1, "US").await;
    assert!(third.is_none(), "committing drains the legs and samples");
}
