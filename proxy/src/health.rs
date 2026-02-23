//! # Health Check & Metrics Endpoint
//!
//! Lightweight HTTP server using raw Tokio TCP.
//! No heavy web frameworks — just enough HTTP to serve:
//!
//! - `GET /health`  → JSON health status
//! - `GET /metrics` → Prometheus exposition format
//!
//! Both endpoints are used by the monitoring stack (Prometheus + Grafana).

use crate::metrics::ProxyMetrics;
use crate::relay::RelayEngine;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

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
    pub bytes_relayed: u64,
    pub fec_recoveries: u64,
    pub sessions_created: u64,
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

/// Run the HTTP health check + metrics server.
pub async fn run_health_server(
    bind_addr: String,
    metrics: Arc<ProxyMetrics>,
    engine: Arc<RelayEngine>,
    region: String,
    node_id: String,
    start_time: Instant,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!("Health/metrics HTTP server listening on {}", bind_addr);

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
            // Read the request
            let mut buf = [0u8; 1024];
            let n = match tokio::time::timeout(
                std::time::Duration::from_secs(5),
                stream.read(&mut buf),
            )
            .await
            {
                Ok(Ok(n)) => n,
                _ => 0,
            };

            let path = parse_request_path(&buf[..n]);

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
                        bytes_relayed: metrics.bytes_relayed.load(Ordering::Relaxed),
                        fec_recoveries: metrics.fec_recoveries.load(Ordering::Relaxed),
                        sessions_created: metrics.sessions_created.load(Ordering::Relaxed),
                    };
                    (
                        "application/json",
                        serde_json::to_string(&response).unwrap_or_default(),
                    )
                }
                _ => {
                    let not_found = "HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n";
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

    #[test]
    fn test_parse_request_path() {
        assert_eq!(
            parse_request_path(b"GET /health HTTP/1.1\r\nHost: localhost\r\n"),
            "/health"
        );
        assert_eq!(
            parse_request_path(b"GET /metrics HTTP/1.1\r\n"),
            "/metrics"
        );
        assert_eq!(parse_request_path(b"GET / HTTP/1.1\r\n"), "/");
        assert_eq!(parse_request_path(b""), "/");
        assert_eq!(parse_request_path(b"garbage"), "/");
    }
}
