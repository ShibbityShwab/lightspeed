//! # Opt-in Latency Telemetry
//!
//! Collects anonymised per-session RTT samples and periodically flushes them
//! to the proxy's `/telemetry` endpoint as a [`TelemetryReport`].
//!
//! ## Privacy guarantees
//!
//! - **On by default (opt-out)**: disable with `--no-telemetry` or
//!   `general.telemetry = false` in `lightspeed.toml`.
//! - **No PII**: the report never includes IP address, user ID, session token,
//!   hostname, or any individual packet timing.
//! - **Aggregated only**: raw RTT samples are reduced to percentiles
//!   (p50/p95/p99) and jitter before being sent; the raw ring buffer is
//!   discarded after each flush. The relay suppresses any cell with fewer than
//!   three reports (k-anonymity floor).
//! - **Startup notice**: a one-line notice is printed once on startup when
//!   telemetry runs, so users always know how to opt out.
//!
//! ## Transport
//!
//! A report is sent as a `Telemetry` control message on the same authenticated
//! QUIC connection the client keeps alive for registration. When no live
//! control connection exists (older relays, `quic` feature disabled, or a
//! reconnect in progress) the flush falls back to
//! `POST http://<proxy>:8080/telemetry` with the same JSON body, so older
//! clients and relays keep working. Both paths feed the same proxy aggregator.

use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Arc, Mutex as StdMutex, PoisonError};
use std::time::Duration;

use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;
use tokio::sync::Mutex;
use tracing::{debug, warn};

use lightspeed_protocol::TelemetryReport;

pub mod context;
pub(crate) mod paths;

use context::TelemetryContext;

/// Maximum RTT samples retained in the rolling window before a flush.
const RING_CAPACITY: usize = 1024;

/// How often to flush telemetry while the tunnel is running.
const FLUSH_INTERVAL: Duration = Duration::from_secs(60 * 15); // 15 minutes

/// Process-wide enable gate for telemetry recording and flushing.
///
/// Defaults to `false`. [`install`] sets it to `true`; [`set_enabled`] lets a
/// caller (for example the GUI toggle) pause and resume without tearing the
/// installed collector down. Read by the global recording hooks and by
/// [`spawn_periodic_flush`].
static ENABLED: AtomicBool = AtomicBool::new(false);

/// Whether telemetry reporting is currently active.
///
/// This gates **reporting only**. Local latency measurement is independent and
/// runs whenever the interceptor/tunnel is active; see `crate::latency`.
pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Outcome of one telemetry flush attempt.
///
/// Reporting is strictly opt-in, so [`FlushOutcome::Disabled`] is returned
/// before any report is built or any connection is opened.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FlushOutcome {
    /// Reporting is disabled; nothing was built or sent.
    Disabled,
    /// Reporting is enabled but the collector had nothing to report.
    NothingToSend,
    /// A report was built and sent, and its samples were committed.
    Sent,
    /// A report was built but the send failed; samples were retained.
    SendFailed,
}

/// Enable or disable telemetry for the rest of the process.
///
/// This never uninstalls the collector: while disabled, the global recording
/// hooks stop accumulating samples and the periodic flush stops sending, but
/// the collector's `OnceLock` stays set. Call [`install`] first so a handle
/// exists.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

/// Install the process-wide telemetry collector and enable reporting.
///
/// **Idempotent.** The first call installs the collector and returns a handle
/// that shares state with the installed collector. Later calls are no-ops that
/// return a handle to the already-installed collector, so installing twice
/// neither panics nor double-installs.
///
/// This also ensures local measurement is installed (`latency::install_measurement`),
/// because a report needs measured values, but measurement is not otherwise
/// tied to telemetry: tunnel/interceptor paths install it independently.
///
/// Spawn [`spawn_periodic_flush`] once a relay address is known to actually
/// send reports.
#[must_use]
pub fn install() -> TelemetryCollector {
    let collector = TelemetryCollector::new();
    paths::install_global_collector(&collector);
    crate::latency::install_measurement();
    set_enabled(true);
    paths::global_collector().cloned().unwrap_or(collector)
}

/// Shared RTT sample ring buffer + FEC counters.
///
/// Call [`TelemetryCollector::record_rtt`] from the keepalive receive loop and
/// [`TelemetryCollector::record_fec_recovery`] / [`TelemetryCollector::record_fec_loss`]
/// as FEC events occur.  The collector is cheap to clone (Arc-backed).
#[derive(Clone)]
pub struct TelemetryCollector {
    inner: Arc<Mutex<Inner>>,
    paths: Arc<StdMutex<paths::PathInner>>,
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
            paths: Arc::new(StdMutex::new(paths::PathInner::default())),
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
    pub(crate) fn percentile(sorted: &[f32], pct: f32) -> f32 {
        if sorted.is_empty() {
            return 0.0;
        }
        let idx = ((pct / 100.0) * (sorted.len() - 1) as f32).round() as usize;
        sorted[idx.min(sorted.len() - 1)]
    }

    /// Build a [`TelemetryReport`] from the current samples without consuming
    /// them. A report is only committed by [`Self::commit_report`] once it has
    /// been sent, so a failed flush can retry with the same data.
    async fn build_report(&self, game_id: u8, country: &str) -> Option<TelemetryReport> {
        let (direct_p50_ms, relayed_p50_ms) = crate::latency::peek_report_values();
        let direct_app_p50_ms = crate::latency::shadow_direct_p50_ms();
        self.build_report_with_latency(
            game_id,
            country,
            direct_p50_ms,
            relayed_p50_ms,
            direct_app_p50_ms,
        )
        .await
    }

    /// Build a report from explicit latency values. Split out so field
    /// propagation is testable without installing the process-wide tracker.
    async fn build_report_with_latency(
        &self,
        game_id: u8,
        country: &str,
        direct_p50_ms: Option<f32>,
        relayed_p50_ms: Option<f32>,
        direct_app_p50_ms: Option<f32>,
    ) -> Option<TelemetryReport> {
        let inner = self.inner.lock().await;
        if inner.samples.is_empty()
            && direct_p50_ms.is_none()
            && relayed_p50_ms.is_none()
            && direct_app_p50_ms.is_none()
        {
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
        drop(inner);

        let recoveries = self.fec_recoveries.load(Ordering::Relaxed);
        let losses = self.fec_losses.load(Ordering::Relaxed);

        let route_legs = self
            .paths
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .snapshot_observations();

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
            direct_p50_ms,
            relayed_p50_ms,
            direct_app_p50_ms,
            client_version: env!("CARGO_PKG_VERSION").to_string(),
            route_legs,
        })
    }

    /// Drop the samples, FEC counters, and path legs covered by a report that
    /// was sent successfully. Called only after the POST succeeds.
    async fn commit_report(&self, report: &TelemetryReport) {
        {
            let mut inner = self.inner.lock().await;
            let reported = (report.sample_count as usize).min(inner.samples.len());
            inner.samples.drain(0..reported);
        }
        self.fec_recoveries
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(report.fec_recoveries))
            })
            .ok();
        self.fec_losses
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                Some(current.saturating_sub(report.fec_losses))
            })
            .ok();
        crate::latency::commit_report_values();
        self.paths
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .drain_observations();
    }

    /// Send the report, preferring the authenticated QUIC control plane and
    /// falling back to `http://<proxy_host>:8080/telemetry`.
    ///
    /// Reporting is strictly opt-in: when [`is_enabled`] is false this returns
    /// [`FlushOutcome::Disabled`] without building a report or opening a
    /// connection. Local measurement is unaffected; see `crate::latency`.
    pub async fn flush(&self, proxy_host: &str, game_id: u8, country: &str) -> FlushOutcome {
        self.flush_preferring_control(proxy_host, None, game_id, country)
            .await
    }

    /// Send the report over `control_addr`'s live QUIC control connection when
    /// one exists, otherwise over the plaintext HTTP endpoint.
    ///
    /// A report is attempted at most once per transport per flush: a failed
    /// control-plane send falls through to HTTP rather than retrying, so
    /// telemetry can never spin or delay gameplay or the control plane.
    pub async fn flush_preferring_control(
        &self,
        proxy_host: &str,
        control_addr: Option<SocketAddrV4>,
        game_id: u8,
        country: &str,
    ) -> FlushOutcome {
        if !is_enabled() {
            debug!("Telemetry flush skipped: reporting disabled");
            return FlushOutcome::Disabled;
        }

        let report = match self.build_report(game_id, country).await {
            Some(r) => r,
            None => {
                debug!("Telemetry flush: no samples to send");
                return FlushOutcome::NothingToSend;
            }
        };

        if let Err(e) = report.validate() {
            warn!("Telemetry report validation failed (bug): {}", e);
            return FlushOutcome::SendFailed;
        }

        let body = match serde_json::to_string(&report) {
            Ok(b) => b,
            Err(e) => {
                warn!("Telemetry serialisation failed: {}", e);
                return FlushOutcome::SendFailed;
            }
        };

        if let Some(addr) = control_addr {
            match crate::quic::send_telemetry(addr, body.as_bytes()).await {
                Ok(()) => {
                    self.commit_report(&report).await;
                    debug!(
                        samples = report.sample_count,
                        p50 = report.p50_ms,
                        p99 = report.p99_ms,
                        "📊 Telemetry flushed over control plane"
                    );
                    return FlushOutcome::Sent;
                }
                Err(e) => debug!(
                    "Control-plane telemetry unavailable ({}); falling back to HTTP",
                    e
                ),
            }
        }

        if self.post_http(proxy_host, &body).await {
            self.commit_report(&report).await;
            debug!(
                samples = report.sample_count,
                p50 = report.p50_ms,
                p99 = report.p99_ms,
                "📊 Telemetry flushed"
            );
            FlushOutcome::Sent
        } else {
            FlushOutcome::SendFailed
        }
    }

    /// POST `body` to `http://<proxy_host>:8080/telemetry` with a hand-rolled
    /// HTTP/1.0 request so the client needs no HTTP library. Returns whether
    /// the write completed; network errors are swallowed because telemetry is
    /// best-effort.
    async fn post_http(&self, proxy_host: &str, body: &str) -> bool {
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
                    return false;
                }
                let _ = stream.shutdown().await;
                true
            }
            Ok(Err(e)) => {
                debug!("Telemetry connect failed ({}): {}", addr, e);
                false
            }
            Err(_) => {
                debug!("Telemetry connect timed out ({})", addr);
                false
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
/// `control_addr` is the relay's supervised control-plane path; when a live
/// QUIC control connection exists for it the report travels encrypted there,
/// otherwise the flush falls back to `proxy_host`'s HTTP endpoint.
///
/// The returned handle's `abort()` method stops flushing on shutdown;
/// call `collector.flush(...)` once more after aborting for the final flush.
pub fn spawn_periodic_flush(
    collector: TelemetryCollector,
    proxy_host: String,
    control_addr: Option<SocketAddrV4>,
    ctx: TelemetryContext,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(FLUSH_INTERVAL);
        interval.tick().await; // skip the first immediate tick
        loop {
            interval.tick().await;
            if !is_enabled() {
                continue;
            }
            let _ = collector
                .flush_preferring_control(&proxy_host, control_addr, ctx.game_id, &ctx.country)
                .await;
        }
    })
}

/// Print the one-line startup notice that telemetry is active.
///
/// Printed once per startup when telemetry runs, so users always see what is
/// sent and how to opt out.
pub fn print_notice() {
    println!(
        "📊 Anonymous telemetry on: aggregate latency stats only, no IPs. \
         Disable with --no-telemetry or telemetry = false in lightspeed.toml. \
         Policy: https://github.com/ShibbityShwab/lightspeed/blob/master/docs/privacy.md"
    );
}

#[cfg(test)]
mod tests;
