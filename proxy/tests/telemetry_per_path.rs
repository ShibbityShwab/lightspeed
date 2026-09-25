//! # Per-Path Telemetry End-to-End Integration Test
//!
//! Proves the whole per-path telemetry chain on the real HTTP surface:
//!
//! 1. A client-shaped [`TelemetryReport`] carrying per-relay
//!    [`PathObservation`] legs is serialised to JSON and POSTed to the proxy's
//!    `/telemetry` endpoint served by [`run_health_server`].
//! 2. The proxy parses and validates the body at the ingest boundary
//!    (`health::ingest_telemetry`) and folds the legs into the bounded
//!    `(relay, game, country)` aggregator.
//! 3. `GET /metrics` renders the k-anonymized Prometheus series, so the test
//!    asserts on the exact scraped text instead of internal structs.
//!
//! A second test locks the protocol privacy contract: the serialised report
//! round-trips through `TelemetryReport::validate` and contains none of the
//! PII-adjacent field names the protocol promises to omit.

use std::time::{Duration, Instant};

use lightspeed_protocol::telemetry::PathObservation;
use lightspeed_protocol::TelemetryReport;
use lightspeed_proxy::health::run_health_server;
use lightspeed_proxy::metrics::ProxyMetrics;
use lightspeed_proxy::relay::RelayEngine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

// ── Fixture constants ───────────────────────────────────────────────

const REGION: &str = "us-west";
const NODE_ID: &str = "proxy-per-path-test";
/// Wire game id 2 maps to the canonical `cs2` key.
const GAME_ID: u8 = lightspeed_protocol::game_id::CS2;
const GAME_KEY: &str = "cs2";
/// Lowercase on the wire; the aggregator normalizes it to `US`.
const COUNTRY_RAW: &str = "us";
const COUNTRY_NORMALIZED: &str = "US";
/// Relay observed on all three reports, so it clears the k >= 3 floor.
const RELAY_ABOVE_K: &str = "relay-alpha";
/// Relay observed on one report only, so the k floor suppresses it.
const RELAY_BELOW_K: &str = "relay-beta";

/// The four latency fields plus the four counters of one route leg.
#[derive(Clone, Copy)]
struct LegValues {
    p50: f32,
    p95: f32,
    p99: f32,
    jitter: f32,
    samples: u32,
    lost: u32,
    recovered: u32,
    dedup_saved: u32,
}

const ABOVE_K_LEGS: [LegValues; 3] = [
    LegValues {
        p50: 10.0,
        p95: 20.0,
        p99: 30.0,
        jitter: 1.0,
        samples: 100,
        lost: 1,
        recovered: 1,
        dedup_saved: 2,
    },
    LegValues {
        p50: 12.0,
        p95: 22.0,
        p99: 32.0,
        jitter: 2.0,
        samples: 110,
        lost: 2,
        recovered: 2,
        dedup_saved: 3,
    },
    LegValues {
        p50: 14.0,
        p95: 24.0,
        p99: 34.0,
        jitter: 3.0,
        samples: 120,
        lost: 3,
        recovered: 3,
        dedup_saved: 4,
    },
];

const BELOW_K_LEG: LegValues = LegValues {
    p50: 40.0,
    p95: 50.0,
    p99: 60.0,
    jitter: 5.0,
    samples: 10,
    lost: 7,
    recovered: 8,
    dedup_saved: 9,
};

fn leg(relay: &str, values: LegValues) -> PathObservation {
    PathObservation {
        relay: relay.to_string(),
        rtt_p50_ms: values.p50,
        rtt_p95_ms: values.p95,
        rtt_p99_ms: values.p99,
        jitter_ms: values.jitter,
        samples: values.samples,
        lost: values.lost,
        recovered: values.recovered,
        dedup_saved: values.dedup_saved,
    }
}

fn report_with_legs(route_legs: Vec<PathObservation>) -> TelemetryReport {
    TelemetryReport {
        game_id: GAME_ID,
        client_country: COUNTRY_RAW.to_string(),
        p50_ms: 30.0,
        p95_ms: 50.0,
        p99_ms: 80.0,
        jitter_ms: 2.0,
        sample_count: 100,
        fec_recoveries: 1,
        fec_losses: 0,
        direct_p50_ms: None,
        direct_app_p50_ms: None,
        relayed_p50_ms: None,
        client_version: "1.4.4".to_string(),
        route_legs,
    }
}

// ── HTTP helpers ────────────────────────────────────────────────────

/// Bind an ephemeral listener and hand it to the real health server.
async fn spawn_health_server() -> (String, tokio::task::JoinHandle<()>) {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();

    let metrics = std::sync::Arc::new(ProxyMetrics::new());
    let engine = std::sync::Arc::new(RelayEngine::new(16));
    let handle = tokio::spawn(async move {
        if let Err(error) = run_health_server(
            listener,
            metrics,
            engine,
            REGION.to_string(),
            NODE_ID.to_string(),
            Instant::now(),
        )
        .await
        {
            eprintln!("health server exited early: {error}");
        }
    });
    (addr, handle)
}

async fn connect_with_retry(addr: &str) -> TcpStream {
    for _ in 0..100 {
        if let Ok(stream) = TcpStream::connect(addr).await {
            return stream;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("health server at {addr} never accepted a connection");
}

/// POST one report and return the raw HTTP status line.
async fn post_telemetry(addr: &str, report: &TelemetryReport) -> String {
    let body = serde_json::to_vec(report).unwrap();
    let head = format!(
        "POST /telemetry HTTP/1.1\r\nHost: {addr}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let mut request = head.into_bytes();
    request.extend_from_slice(&body);

    let exchange = async {
        let mut stream = connect_with_retry(addr).await;
        stream.write_all(&request).await.unwrap();
        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        response
    };
    let response = tokio::time::timeout(Duration::from_secs(5), exchange)
        .await
        .expect("telemetry POST timed out");
    response.lines().next().unwrap_or_default().to_string()
}

/// GET /metrics and return `(status_line, body)`.
async fn scrape_metrics(addr: &str) -> (String, String) {
    let request = format!("GET /metrics HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    let exchange = async {
        let mut stream = connect_with_retry(addr).await;
        stream.write_all(request.as_bytes()).await.unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).await.unwrap();
        String::from_utf8(raw).expect("Prometheus body is UTF-8")
    };
    let response = tokio::time::timeout(Duration::from_secs(5), exchange)
        .await
        .expect("metrics scrape timed out");
    let (head, body) = response
        .split_once("\r\n\r\n")
        .expect("HTTP response has a header/body separator");
    (
        head.lines().next().unwrap_or_default().to_string(),
        body.to_string(),
    )
}

// ── Tests ───────────────────────────────────────────────────────────

#[tokio::test]
async fn per_path_telemetry_end_to_end_over_http() {
    let (addr, server) = spawn_health_server().await;

    // Report 1 carries two legs for the same game/country; reports 2 and 3
    // carry only the shared relay, so it reaches the k >= 3 floor while the
    // second relay stays below it.
    let reports = [
        report_with_legs(vec![
            leg(RELAY_ABOVE_K, ABOVE_K_LEGS[0]),
            leg(RELAY_BELOW_K, BELOW_K_LEG),
        ]),
        report_with_legs(vec![leg(RELAY_ABOVE_K, ABOVE_K_LEGS[1])]),
        report_with_legs(vec![leg(RELAY_ABOVE_K, ABOVE_K_LEGS[2])]),
    ];

    for report in &reports {
        assert!(
            report.validate().is_ok(),
            "fixture must pass the protocol validator"
        );
        let status = post_telemetry(&addr, report).await;
        assert!(
            status.starts_with("HTTP/1.1 200 OK"),
            "ingest must accept a valid report, got: {status}"
        );
    }

    let (status, metrics) = scrape_metrics(&addr).await;
    assert_eq!(status, "HTTP/1.1 200 OK");
    assert!(
        metrics.contains("# TYPE lightspeed_telemetry_route_reports_total counter"),
        "route telemetry family must be advertised"
    );

    // The k >= 3 cell renders every series with the full label set, and the
    // sums are the sums of the three legs' reported values.
    let cell = format!(
        "region=\"{REGION}\",node_id=\"{NODE_ID}\",game=\"{GAME_KEY}\",country=\"{COUNTRY_NORMALIZED}\",relay=\"{RELAY_ABOVE_K}\""
    );
    for (series, value) in [
        ("lightspeed_telemetry_route_reports_total", "3"),
        ("lightspeed_telemetry_route_samples_total", "330"),
        ("lightspeed_telemetry_route_rtt_p50_ms_sum", "36.0"),
        ("lightspeed_telemetry_route_rtt_p50_ms_count", "3"),
        ("lightspeed_telemetry_route_rtt_p95_ms_sum", "66.0"),
        ("lightspeed_telemetry_route_rtt_p95_ms_count", "3"),
        ("lightspeed_telemetry_route_rtt_p99_ms_sum", "96.0"),
        ("lightspeed_telemetry_route_rtt_p99_ms_count", "3"),
        ("lightspeed_telemetry_route_jitter_ms_sum", "6.0"),
        ("lightspeed_telemetry_route_jitter_ms_count", "3"),
        ("lightspeed_telemetry_route_lost_total", "6"),
        ("lightspeed_telemetry_route_recovered_total", "6"),
        ("lightspeed_telemetry_route_dedup_saved_total", "9"),
    ] {
        let expected = format!("{series}{{{cell}}} {value}");
        assert!(
            metrics.contains(&expected),
            "missing route series line: {expected}\n--- scrape ---\n{metrics}"
        );
    }

    // The flat (game, country) cell has three reports but only one distinct
    // source IP (all requests arrive from the test's loopback address), so the
    // distinct-source k-anonymity floor withholds it from the scrape.
    let flat_cell = format!(
        "region=\"{REGION}\",node_id=\"{NODE_ID}\",game=\"{GAME_KEY}\",country=\"{COUNTRY_NORMALIZED}\""
    );
    let flat_line = format!("lightspeed_telemetry_reports_total{{{flat_cell}}} 3");
    assert!(
        !metrics.contains(&flat_line),
        "a flat cell backed by one distinct source must stay suppressed: {flat_line}"
    );

    // No cell was dropped by the cap or the relay-charset filter.
    let node_labels = format!("region=\"{REGION}\",node_id=\"{NODE_ID}\"");
    let rejected = format!("lightspeed_telemetry_route_rejected_total{{{node_labels}}} 0");
    assert!(
        metrics.contains(&rejected),
        "no route cell should be rejected: {rejected}"
    );

    // Below-k privacy: one observation must not leak its relay label anywhere
    // in the scrape.
    assert!(
        !metrics.contains(&format!("relay=\"{RELAY_BELOW_K}\"")),
        "below-k route relay must be suppressed from the scrape"
    );

    server.abort();
}

#[test]
fn per_path_report_satisfies_protocol_privacy_guard() {
    let report = report_with_legs(vec![
        leg(RELAY_ABOVE_K, ABOVE_K_LEGS[0]),
        leg(RELAY_BELOW_K, BELOW_K_LEG),
    ]);

    assert!(
        report.validate().is_ok(),
        "a two-leg report for the same game/country must validate"
    );

    let json = serde_json::to_string(&report).unwrap();
    let decoded: TelemetryReport = serde_json::from_str(&json).unwrap();
    assert_eq!(
        decoded.validate(),
        Ok(()),
        "round-tripped report must still satisfy the protocol guard"
    );
    assert_eq!(
        decoded.route_legs, report.route_legs,
        "per-path observations must survive serialisation"
    );

    for forbidden in ["ip", "user", "session", "host"] {
        assert!(
            !json.contains(forbidden),
            "no-PII guard: telemetry JSON must not contain {forbidden:?}: {json}"
        );
    }
}
