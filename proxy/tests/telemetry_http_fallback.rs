//! Regression: the plaintext HTTP `/telemetry` endpoint still accepts a report
//! for older clients that predate the QUIC control-plane telemetry path.

use std::sync::Arc;
use std::time::{Duration, Instant};

use lightspeed_proxy::health;
use lightspeed_proxy::metrics::ProxyMetrics;
use lightspeed_proxy::relay::RelayEngine;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

/// A well-formed report body matching the HTTP `/telemetry` schema.
const VALID_TELEMETRY_JSON: &str = r#"{"game_id":2,"client_country":"US","p50_ms":30.0,"p95_ms":50.0,"p99_ms":80.0,"jitter_ms":2.0,"sample_count":100,"fec_recoveries":1,"fec_losses":0,"client_version":"1.6.9"}"#;

/// Given: the HTTP health/metrics server. When: a legacy client POSTs a valid
/// telemetry report to `/telemetry`. Then: the endpoint answers 200 and folds
/// the report into the same aggregator the QUIC path feeds.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn http_telemetry_endpoint_still_accepts_a_report() -> anyhow::Result<()> {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await?;
    let addr = listener.local_addr()?;
    let metrics = Arc::new(ProxyMetrics::new());
    let engine = Arc::new(RelayEngine::new(10));
    tokio::spawn(health::run_health_server(
        listener,
        Arc::clone(&metrics),
        engine,
        "test-region".to_string(),
        "test-node".to_string(),
        Instant::now(),
    ));

    let request = format!(
        "POST /telemetry HTTP/1.0\r\nHost: localhost\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        VALID_TELEMETRY_JSON.len(),
        VALID_TELEMETRY_JSON
    );

    let mut stream = TcpStream::connect(addr).await?;
    stream.write_all(request.as_bytes()).await?;
    let mut response = String::new();
    tokio::time::timeout(Duration::from_secs(5), stream.read_to_string(&mut response)).await??;
    assert!(
        response.starts_with("HTTP/1.1 200 OK"),
        "unexpected response: {response}"
    );

    let agg = metrics.telemetry.lock().unwrap();
    assert_eq!(agg.cells.len(), 1, "one telemetry cell");
    assert_eq!(
        agg.cells.values().next().unwrap().reports,
        1,
        "the HTTP report must be aggregated"
    );
    Ok(())
}
