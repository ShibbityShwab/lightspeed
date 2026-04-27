//! # Opt-in Latency Telemetry
//!
//! Collects anonymised per-session RTT samples and periodically flushes them
//! to the proxy's `/telemetry` endpoint as a [`TelemetryReport`].
//!
//! ## Privacy guarantees
//!
//! - **Disabled by default** — only enabled via `general.telemetry = true` in
//!   `lightspeed.toml` or the `--telemetry` CLI flag.
//! - **No PII** — the report never includes IP address, user ID, session token,
//!   hostname, or any individual packet timing.
//! - **Aggregated only** — raw RTT samples are reduced to percentiles
//!   (p50/p95/p99) and jitter before being sent; the raw ring buffer is
//!   discarded after each flush.
//! - **First-run disclosure** — a clear banner is printed on every startup when
//!   telemetry is enabled so users always know.
//!
//! ## Endpoint
//!
//! `POST http://<proxy>:8080/telemetry` with a JSON body matching
//! [`TelemetryReport`].  Uses a hand-rolled HTTP/1.0 request so the client
//! does not need an HTTP library dependency.

use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use lightspeed_protocol::TelemetryReport;

/// Maximum RTT samples retained in the rolling window before a flush.
const RING_CAPACITY: usize = 1024;

/// How often to flush telemetry while the tunnel is running.
const FLUSH_INTERVAL: Duration = Duration::from_secs(60 * 15); // 15 minutes

/// Shared RTT sample ring buffer + FEC counters.
///
/// Call [`TelemetryCollector::record_rtt`] from the keepalive receive loop and
/// [`TelemetryCollector::record_fec_recovery`] / [`TelemetryCollector::record_fec_loss`]
/// as FEC events occur.  The collector is cheap to clone (Arc-backed).
#[derive(Clone)]
pub struct TelemetryCollector {
    inner: Arc<Mutex<Inner>>,
    fec_recoveries: Arc<AtomicU32>,
    fec_losses: Arc<AtomicU32>,
}

struct Inner {
    /// Rolling window of round-trip latency samples in milliseconds.
    samples: Vec<f32>,
}

impl TelemetryCollector {
    /// Create a new collector.
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(Inner {
                samples: Vec::with_capacity(RING_CAPACITY),
            })),
            fec_recoveries: Arc::new(AtomicU32::new(0)),
            fec_losses: Arc::new(AtomicU32::new(0)),
        }
    }

    /// Record a round-trip latency sample (milliseconds).
    pub async fn record_rtt(&self, rtt_ms: f64) {
        let mut inner = self.inner.lock().await;
        if inner.samples.len() >= RING_CAPACITY {
            // Drop oldest sample (ring-buffer eviction: remove front).
            inner.samples.remove(0);
        }
        inner.samples.push(rtt_ms as f32);
    }

    /// Record a successful FEC packet recovery.
    pub fn record_fec_recovery(&self) {
        self.fec_recoveries.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an unrecoverable FEC block loss.
    pub fn record_fec_loss(&self) {
        self.fec_losses.fetch_add(1, Ordering::Relaxed);
    }

    /// Compute percentiles from a sorted slice.
    fn percentile(sorted: &[f32], pct: f32) -> f32 {
        if sorted.is_empty() {
            return 0.0;
        }
        let idx = ((pct / 100.0) * (sorted.len() - 1) as f32).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// Build a [`TelemetryReport`] from the current samples.
    /// Drains all accumulated samples and FEC counters.
    async fn build_report(&self, game_id: u8, country: &str) -> Option<TelemetryReport> {
        let mut inner = self.inner.lock().await;
        if inner.samples.is_empty() {
            return None;
        }

        let mut sorted = inner.samples.clone();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let p50 = Self::percentile(&sorted, 50.0);
        let p95 = Self::percentile(&sorted, 95.0);
        let p99 = Self::percentile(&sorted, 99.0);

        // Jitter = mean of consecutive absolute deltas.
        let jitter = if sorted.len() >= 2 {
            let deltas: f32 = sorted.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
            deltas / (sorted.len() - 1) as f32
        } else {
            0.0
        };

        let count = inner.samples.len() as u32;
        let recoveries = self.fec_recoveries.swap(0, Ordering::Relaxed);
        let losses = self.fec_losses.swap(0, Ordering::Relaxed);

        // Drain samples after reading.
        inner.samples.clear();

        Some(TelemetryReport {
            game_id,
            client_country: country.to_string(),
            p50_ms: (p50 * 10.0).round() / 10.0,
            p95_ms: (p95 * 10.0).round() / 10.0,
            p99_ms: (p99 * 10.0).round() / 10.0,
            jitter_ms: (jitter * 10.0).round() / 10.0,
            sample_count: count,
            fec_recoveries: recoveries,
            fec_losses: losses,
            client_version: env!("CARGO_PKG_VERSION").to_string(),
        })
    }

    /// Send the report to `http://<proxy_host>:8080/telemetry`.
    ///
    /// Uses a raw Tokio TCP connection + hand-rolled HTTP/1.0 POST so the
    /// client does not need an HTTP client library.  Any network error is
    /// silently swallowed — telemetry is best-effort.
    pub async fn flush(&self, proxy_host: &str, game_id: u8, country: &str) {
        let report = match self.build_report(game_id, country).await {
            Some(r) => r,
            None => {
                debug!("Telemetry flush: no samples to send");
                return;
            }
        };

        if let Err(e) = report.validate() {
            warn!("Telemetry report validation failed (bug): {}", e);
            return;
        }

        let body = match serde_json::to_string(&report) {
            Ok(b) => b,
            Err(e) => {
                warn!("Telemetry serialisation failed: {}", e);
                return;
            }
        };

        let addr = format!("{}:8080", proxy_host);
        let request = format!(
            "POST /telemetry HTTP/1.0\r\nHost: {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            proxy_host,
            body.len(),
            body
        );

        match tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&addr)).await {
            Ok(Ok(mut stream)) => {
                if let Err(e) = stream.write_all(request.as_bytes()).await {
                    debug!("Telemetry send error: {}", e);
                    return;
                }
                let _ = stream.shutdown().await;
                debug!(
                    samples = report.sample_count,
                    p50 = report.p50_ms,
                    p99 = report.p99_ms,
                    "📊 Telemetry flushed"
                );
            }
            Ok(Err(e)) => {
                debug!("Telemetry connect failed ({}): {}", addr, e);
            }
            Err(_) => {
                debug!("Telemetry connect timed out ({})", addr);
            }
        }
    }
}

impl Default for TelemetryCollector {
    fn default() -> Self {
        Self::new()
    }
}

/// Spawn a background task that flushes telemetry every [`FLUSH_INTERVAL`].
///
/// The returned handle's `abort()` method stops flushing on shutdown;
/// call `collector.flush(...)` once more after aborting for the final flush.
pub fn spawn_periodic_flush(
    collector: TelemetryCollector,
    proxy_host: String,
    game_id: u8,
    country: String,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(FLUSH_INTERVAL);
        interval.tick().await; // skip the first immediate tick
        loop {
            interval.tick().await;
            collector.flush(&proxy_host, game_id, &country).await;
        }
    })
}

/// Print the one-time telemetry disclosure banner to the console.
///
/// Should be called **once** per startup when `telemetry == true`.
pub fn print_disclosure() {
    println!();
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│  📊  Anonymous telemetry ENABLED                               │");
    println!("│                                                                 │");
    println!("│  LightSpeed will send anonymised latency stats (p50/p95/p99,   │");
    println!("│  jitter, FEC recoveries) to the proxy every 15 minutes.        │");
    println!("│                                                                 │");
    println!("│  NO IP address, user ID, or packet content is ever sent.       │");
    println!("│  See docs/privacy.md for the full field list.                  │");
    println!("│                                                                 │");
    println!("│  Disable:  --no-telemetry  or  telemetry = false in config     │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
