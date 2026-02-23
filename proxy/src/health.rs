//! # Health Check Endpoint
//!
//! Lightweight HTTP health check server using raw Tokio TCP.
//! No heavy web frameworks — just enough HTTP to serve health/metrics.

use crate::metrics::ProxyMetrics;
use crate::relay::RelayEngine;
use serde::Serialize;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Instant;
use tokio::io::AsyncWriteExt;
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
}

/// Run the HTTP health check server.
pub async fn run_health_server(
    bind_addr: String,
    metrics: Arc<ProxyMetrics>,
    engine: Arc<RelayEngine>,
    region: String,
    node_id: String,
    start_time: Instant,
) -> anyhow::Result<()> {
    let listener = TcpListener::bind(&bind_addr).await?;
    tracing::info!("Health check HTTP server listening on {}", bind_addr);

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
            // Read the request (we don't really parse it — just drain it)
            let mut buf = [0u8; 1024];
            let _ = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                tokio::io::AsyncReadExt::read(&mut stream, &mut buf),
            )
            .await;

            let sessions = engine.active_sessions().await;
            let response = HealthResponse {
                status: "healthy",
                version: env!("CARGO_PKG_VERSION"),
                active_connections: sessions as u64,
                uptime_secs: start_time.elapsed().as_secs(),
                region,
                node_id,
                packets_relayed: metrics.packets_relayed.load(Ordering::Relaxed),
                packets_dropped: metrics.packets_dropped.load(Ordering::Relaxed),
                bytes_relayed: metrics.bytes_relayed.load(Ordering::Relaxed),
            };

            let body = serde_json::to_string(&response).unwrap_or_default();
            let http_response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );

            let _ = stream.write_all(http_response.as_bytes()).await;
            let _ = stream.shutdown().await;
        });
    }
}
