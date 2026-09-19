//! # Health Check & Metrics Endpoint
//!
//! Lightweight HTTP server using raw Tokio TCP.
//! No heavy web frameworks — just enough HTTP to serve:
//!
//! - `GET /health`   → JSON health status
//! - `GET /metrics`  → Prometheus exposition format
//! - `POST /telemetry` → Ingest anonymous client latency report (opt-in)
//!
//! Both endpoints are used by the monitoring stack (Prometheus + Grafana).

use crate::handoff::HandoffHealth;
use crate::metrics::ProxyMetrics;
use crate::relay::RelayEngine;
use crate::update_state::UpdateState;
use lightspeed_protocol::TelemetryReport;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tracing::{debug, warn};

/// Health check response.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub version: &'static str,
    pub active_connections: u64,
    pub uptime_secs: u64,
    pub region: String,
    pub node_id: String,
    pub packets_relayed: u64,
    pub packets_dropped: u64,
    pub drops_malformed: u64,
    pub drops_auth_rejected: u64,
    pub drops_abuse_blocked: u64,
    pub drops_rate_limited: u64,
    pub drops_fec_malformed: u64,
    pub drops_session_setup: u64,
    pub drops_relay_send_errors: u64,
    pub bytes_relayed: u64,
    pub fec_recoveries: u64,
    pub sessions_created: u64,
    /// Last self-update snapshot; `null` when the state file is unavailable.
    pub update: Option<UpdateState>,
    /// In-place handoff support and the last attempt, if any.
    pub handoff: HandoffHealth,
}

/// Find the byte offset at which the HTTP body begins (after the blank line).
///
/// Returns `None` if no blank line is found (incomplete request).
fn find_body_start(raw: &[u8]) -> Option<usize> {
    // Look for \r\n\r\n first, fall back to \n\n.
    if let Some(pos) = raw.windows(4).position(|w| w == b"\r\n\r\n") {
        return Some(pos + 4);
    }
    if let Some(pos) = raw.windows(2).position(|w| w == b"\n\n") {
        return Some(pos + 2);
    }
    None
}

/// Parse the request path from a raw HTTP request.
fn parse_request_path(raw: &[u8]) -> &str {
    // HTTP request line: "GET /path HTTP/1.1\r\n..."
    let request_str = std::str::from_utf8(raw).unwrap_or("");
    let first_line = request_str.lines().next().unwrap_or("");
    let parts: Vec<&str> = first_line.split_whitespace().collect();
    if parts.len() >= 2 {
        parts[1]
    } else {
        "/"
    }
}

/// Parse, validate, and aggregate one opt-in telemetry POST body.
///
/// The body must be a JSON [`TelemetryReport`]. Parse and validation failures
/// are logged at `warn` with their specific cause and returned as a static
/// message the caller maps to HTTP 400. On success the report is folded into
/// the bounded aggregator and `Ok(())` is returned.
pub fn ingest_telemetry(metrics: &ProxyMetrics, body: &[u8]) -> Result<(), &'static str> {
    let report: TelemetryReport = match serde_json::from_slice(body) {
        Ok(report) => report,
        Err(e) => {
            warn!("Telemetry JSON parse error: {}", e);
            return Err("invalid telemetry JSON");
        }
    };
    if let Err(e) = report.validate() {
        warn!("Telemetry report invalid: {}", e);
        return Err(e);
    }
    debug!(
        game_id = report.game_id,
        country = %report.client_country,
        p50 = report.p50_ms,
        p99 = report.p99_ms,
        samples = report.sample_count,
        route_legs = report.route_legs.len(),
        "Telemetry report ingested"
    );
    metrics.record_telemetry_report(&report);
    Ok(())
}

/// Serve the HTTP health check + metrics endpoints on an already-bound
/// listener. Binding is the caller's responsibility so a bind failure is
/// observable before startup is reported ready.
pub async fn run_health_server(
    listener: TcpListener,
    metrics: Arc<ProxyMetrics>,
    engine: Arc<RelayEngine>,
    region: String,
    node_id: String,
    start_time: Instant,
) -> anyhow::Result<()> {
    tracing::info!(
        "Health/metrics HTTP server listening on {}",
        listener
            .local_addr()
            .map(|addr| addr.to_string())
            .unwrap_or_else(|_| "<unknown>".to_string())
    );

    loop {
        let (mut stream, _addr) = match listener.accept().await {
            Ok(conn) => conn,
            Err(e) => {
                tracing::warn!("Health accept error: {}", e);
                continue;
            }
        };

        let metrics = Arc::clone(&metrics);
        let engine = Arc::clone(&engine);
        let region = region.clone();
        let node_id = node_id.clone();

        tokio::spawn(async move {
            // Read up to 4 KiB — large enough for any well-formed request
            // (GET headers + POST telemetry body ≤ ~500 bytes).
            let mut buf = [0u8; 4096];
            let n = match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                stream.read(&mut buf),
            )
            .await
            {
                Ok(Ok(n)) => n,
                _ => 0,
            };

            let raw = &buf[..n];
            let request_str = std::str::from_utf8(raw).unwrap_or("");
            let path = parse_request_path(raw);

            // Detect POST /telemetry before the general match so we can handle
            // the body extraction separately.
            let first_line = request_str.lines().next().unwrap_or("");
            let is_post_telemetry =
                first_line.starts_with("POST ") && path.starts_with("/telemetry");

            if is_post_telemetry {
                // The POST body follows the blank line (\r\n\r\n or \n\n).
                let body_slice = if let Some(pos) = find_body_start(raw) {
                    &raw[pos..]
                } else {
                    b""
                };

                // Guard: reject oversized bodies before parsing.
                if body_slice.len() > 2048 {
                    let resp = "HTTP/1.1 413 Payload Too Large\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(resp.as_bytes()).await;
                    let _ = stream.shutdown().await;
                    return;
                }

                let resp = match ingest_telemetry(&metrics, body_slice) {
                    Ok(()) => "HTTP/1.1 200 OK\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    Err(_) => {
                        "HTTP/1.1 400 Bad Request\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                    }
                };
                let _ = stream.write_all(resp.as_bytes()).await;
                let _ = stream.shutdown().await;
                return;
            }

            let (content_type, body) = match path {
                "/metrics" => {
                    // Prometheus exposition format
                    let sessions = engine.active_sessions().await;
                    metrics
                        .active_connections
                        .store(sessions as u64, Ordering::Relaxed);
                    (
                        "text/plain; version=0.0.4; charset=utf-8",
                        metrics.to_prometheus(&region, &node_id),
                    )
                }
                "/health" | "/" => {
                    // JSON health check
                    let sessions = engine.active_sessions().await;
                    let response = HealthResponse {
                        status: "healthy",
                        version: env!("CARGO_PKG_VERSION"),
                        active_connections: sessions as u64,
                        uptime_secs: start_time.elapsed().as_secs(),
                        region: region.clone(),
                        node_id: node_id.clone(),
                        packets_relayed: metrics.packets_relayed.load(Ordering::Relaxed),
                        packets_dropped: metrics.packets_dropped.load(Ordering::Relaxed),
                        drops_malformed: metrics.drops_malformed.load(Ordering::Relaxed),
                        drops_auth_rejected: metrics.auth_rejections.load(Ordering::Relaxed),
                        drops_abuse_blocked: metrics.abuse_blocks.load(Ordering::Relaxed),
                        drops_rate_limited: metrics.rate_limit_hits.load(Ordering::Relaxed),
                        drops_fec_malformed: metrics.drops_fec_malformed.load(Ordering::Relaxed),
                        drops_session_setup: metrics.drops_session_setup.load(Ordering::Relaxed),
                        drops_relay_send_errors: metrics
                            .drops_relay_send_errors
                            .load(Ordering::Relaxed),
                        bytes_relayed: metrics.bytes_relayed.load(Ordering::Relaxed),
                        fec_recoveries: metrics.fec_recoveries.load(Ordering::Relaxed),
                        sessions_created: metrics.sessions_created.load(Ordering::Relaxed),
                        update: crate::update_state::current_update_state(),
                        handoff: crate::handoff::handoff_health(),
                    };
                    (
                        "application/json",
                        serde_json::to_string(&response).unwrap_or_default(),
                    )
                }
                _ => {
                    let not_found =
                        "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
                    let _ = stream.write_all(not_found.as_bytes()).await;
                    let _ = stream.shutdown().await;
                    return;
                }
            };

            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                content_type,
                body.len(),
                body
            );

            let _ = stream.write_all(http_response.as_bytes()).await;
            let _ = stream.shutdown().await;
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    /// Serializes env-var mutation across parallel tests.
    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn env_guard() -> std::sync::MutexGuard<'static, ()> {
        ENV_TEST_LOCK
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    fn temp_state_dir(tag: &str) -> std::path::PathBuf {
        use std::sync::atomic::{AtomicU64, Ordering};
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos());
        let dir = std::env::temp_dir().join(format!(
            "ls-update-{}-{}-{}-{}",
            tag,
            std::process::id(),
            nanos,
            n
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn sample_response() -> HealthResponse {
        HealthResponse {
            status: "healthy",
            version: env!("CARGO_PKG_VERSION"),
            active_connections: 0,
            uptime_secs: 0,
            region: "test-region".to_string(),
            node_id: "test-node".to_string(),
            packets_relayed: 0,
            packets_dropped: 0,
            drops_malformed: 0,
            drops_auth_rejected: 0,
            drops_abuse_blocked: 0,
            drops_rate_limited: 0,
            drops_fec_malformed: 0,
            drops_session_setup: 0,
            drops_relay_send_errors: 0,
            bytes_relayed: 0,
            fec_recoveries: 0,
            sessions_created: 0,
            update: crate::update_state::current_update_state(),
            handoff: HandoffHealth {
                supported: crate::handoff::SUPPORTED,
                last: None,
            },
        }
    }

    #[test]
    fn test_parse_request_path() {
        assert_eq!(
            parse_request_path(b"GET /health HTTP/1.1\r\nHost: localhost\r\n"),
            "/health"
        );
        assert_eq!(parse_request_path(b"GET /metrics HTTP/1.1\r\n"), "/metrics");
        assert_eq!(parse_request_path(b"GET / HTTP/1.1\r\n"), "/");
        assert_eq!(parse_request_path(b""), "/");
        assert_eq!(parse_request_path(b"garbage"), "/");
    }

    #[test]
    fn test_find_body_start_crlf() {
        let req = b"POST /telemetry HTTP/1.0\r\nContent-Length: 5\r\n\r\nhello";
        let pos = find_body_start(req).unwrap();
        assert_eq!(&req[pos..], b"hello");
    }

    #[test]
    fn test_find_body_start_lf() {
        let req = b"POST /telemetry HTTP/1.0\nContent-Length: 5\n\nhello";
        let pos = find_body_start(req).unwrap();
        assert_eq!(&req[pos..], b"hello");
    }

    #[test]
    fn test_find_body_start_no_separator() {
        assert!(find_body_start(b"POST /telemetry HTTP/1.0\r\n").is_none());
    }

    #[test]
    fn test_telemetry_report_roundtrip_via_json() {
        let report = TelemetryReport {
            game_id: 2,
            client_country: "TH".to_string(),
            p50_ms: 31.0,
            p95_ms: 45.0,
            p99_ms: 60.0,
            jitter_ms: 2.0,
            sample_count: 120,
            fec_recoveries: 1,
            fec_losses: 0,
            client_version: "0.4.0-dev".to_string(),
            route_legs: vec![],
        };
        assert!(report.validate().is_ok());

        let json = serde_json::to_string(&report).unwrap();
        let decoded: TelemetryReport = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.game_id, 2);
        assert_eq!(decoded.client_country, "TH");
        assert_eq!(decoded.sample_count, 120);
    }

    #[test]
    fn test_health_has_flat_drop_fields() {
        let response = HealthResponse {
            status: "healthy",
            version: env!("CARGO_PKG_VERSION"),
            active_connections: 0,
            uptime_secs: 0,
            region: "us-west".to_string(),
            node_id: "local-test".to_string(),
            packets_relayed: 0,
            packets_dropped: 28,
            drops_malformed: 1,
            drops_auth_rejected: 2,
            drops_abuse_blocked: 3,
            drops_rate_limited: 4,
            drops_fec_malformed: 5,
            drops_session_setup: 6,
            drops_relay_send_errors: 7,
            bytes_relayed: 0,
            fec_recoveries: 0,
            sessions_created: 0,
            update: None,
            handoff: HandoffHealth {
                supported: crate::handoff::SUPPORTED,
                last: None,
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        for key in [
            "drops_malformed",
            "drops_auth_rejected",
            "drops_abuse_blocked",
            "drops_rate_limited",
            "drops_fec_malformed",
            "drops_session_setup",
            "drops_relay_send_errors",
        ] {
            assert!(
                json.contains(&format!("\"{key}\"")),
                "missing {key}: {json}"
            );
        }
    }

    #[test]
    fn health_exposes_handoff_support_and_last_status() {
        let mut response = HealthResponse {
            status: "healthy",
            version: env!("CARGO_PKG_VERSION"),
            active_connections: 0,
            uptime_secs: 0,
            region: "test".to_string(),
            node_id: "test".to_string(),
            packets_relayed: 0,
            packets_dropped: 0,
            drops_malformed: 0,
            drops_auth_rejected: 0,
            drops_abuse_blocked: 0,
            drops_rate_limited: 0,
            drops_fec_malformed: 0,
            drops_session_setup: 0,
            drops_relay_send_errors: 0,
            bytes_relayed: 0,
            fec_recoveries: 0,
            sessions_created: 0,
            update: None,
            handoff: HandoffHealth {
                supported: true,
                last: None,
            },
        };
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"handoff\":{"), "{json}");
        assert!(json.contains("\"supported\":true"), "{json}");
        assert!(json.contains("\"last\":null"), "{json}");

        response.handoff.last = Some(crate::handoff::HandoffStatus::ok(
            "id",
            "1.0.0",
            "1.0.1",
            2,
            1_700_000_000_000,
        ));
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"sessions_transferred\":2"), "{json}");
        assert!(json.contains("\"result\":\"ok\""), "{json}");
    }

    #[test]
    fn test_ingest_telemetry_valid_invalid() {
        let m = ProxyMetrics::new();
        let valid = br#"{"game_id":2,"client_country":"us","p50_ms":30.0,"p95_ms":50.0,"p99_ms":80.0,"jitter_ms":2.0,"sample_count":100,"fec_recoveries":1,"fec_losses":0,"client_version":"1.4.4"}"#;

        assert!(ingest_telemetry(&m, valid).is_ok());
        {
            let agg = m.telemetry.lock().unwrap();
            assert_eq!(agg.cells.len(), 1);
            let cell = agg.cells.values().next().unwrap();
            assert_eq!(cell.reports, 1);
        }

        // p95 < p50 is rejected by TelemetryReport::validate.
        let bad_percentile = br#"{"game_id":2,"client_country":"us","p50_ms":100.0,"p95_ms":50.0,"p99_ms":80.0,"jitter_ms":2.0,"sample_count":100,"fec_recoveries":1,"fec_losses":0,"client_version":"1.4.4"}"#;
        assert!(ingest_telemetry(&m, bad_percentile).is_err());

        assert!(ingest_telemetry(&m, b"not json at all").is_err());
        // Failed ingests must not create cells.
        assert_eq!(m.telemetry.lock().unwrap().cells.len(), 1);
    }

    #[test]
    fn update_state_exposed_when_present() {
        let _guard = env_guard();
        crate::update_state::reset_cache_for_test();
        let dir = temp_state_dir("present");
        let path = dir.join("update-state.json");
        std::fs::write(
            &path,
            r#"{"schema_version":1,"current_version":"1.4.4","active_release":"/opt/lightspeed/releases/1.4.4","available_version":"1.4.5","rollback_version":null,"last_check_at":1700000000,"last_apply_at":1700000005,"last_result":"updated","last_error":null}"#,
        )
        .unwrap();
        std::env::set_var("LIGHTSPEED_UPDATE_STATE", &path);

        let response = sample_response();
        let update = response.update.clone().expect("update should be present");
        assert_eq!(update.current_version, "1.4.4");
        assert_eq!(update.active_release, "/opt/lightspeed/releases/1.4.4");
        assert_eq!(update.available_version.as_deref(), Some("1.4.5"));
        assert_eq!(update.rollback_version, None);
        assert_eq!(update.last_check_at, 1_700_000_000);
        assert_eq!(update.last_apply_at, Some(1_700_000_005));
        assert_eq!(update.last_result, "updated");
        assert_eq!(update.last_error, None);

        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"update\":{"), "update not nested: {json}");
        assert!(json.contains("/opt/lightspeed/releases/1.4.4"), "{json}");
        assert!(!json.contains("schema_version"), "{json}");

        std::env::remove_var("LIGHTSPEED_UPDATE_STATE");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_null_when_absent() {
        let _guard = env_guard();
        crate::update_state::reset_cache_for_test();
        let dir = temp_state_dir("absent");
        let path = dir.join("does-not-exist.json");
        std::env::set_var("LIGHTSPEED_UPDATE_STATE", &path);

        let response = sample_response();
        assert!(response.update.is_none());
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"update\":null"), "{json}");

        std::env::remove_var("LIGHTSPEED_UPDATE_STATE");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn update_null_when_malformed() {
        let _guard = env_guard();
        crate::update_state::reset_cache_for_test();
        let dir = temp_state_dir("malformed");
        let path = dir.join("update-state.json");
        std::fs::write(&path, b"{ this is not json").unwrap();
        std::env::set_var("LIGHTSPEED_UPDATE_STATE", &path);

        let response = sample_response();
        assert!(response.update.is_none());
        let json = serde_json::to_string(&response).unwrap();
        assert!(json.contains("\"update\":null"), "{json}");

        std::env::remove_var("LIGHTSPEED_UPDATE_STATE");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn oversized_state_is_ignored() {
        let _guard = env_guard();
        crate::update_state::reset_cache_for_test();
        let dir = temp_state_dir("oversized");
        let path = dir.join("update-state.json");
        let mut contents = String::from(
            r#"{"schema_version":1,"current_version":"1.4.4","active_release":"/opt/lightspeed/releases/1.4.4","available_version":null,"rollback_version":null,"last_check_at":1700000000,"last_apply_at":null,"last_result":"ok","last_error":null}"#,
        );
        contents.push_str(&" ".repeat(70 * 1024));
        assert!(contents.len() > 64 * 1024);
        std::fs::write(&path, contents).unwrap();
        std::env::set_var("LIGHTSPEED_UPDATE_STATE", &path);

        let response = sample_response();
        assert!(response.update.is_none());

        std::env::remove_var("LIGHTSPEED_UPDATE_STATE");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
