//! # Proxy Metrics
//!
//! Collects and exports proxy performance metrics via Prometheus format.
//! Includes counters, gauges, and histogram-style latency buckets for
//! comprehensive observability.
//!
//! All metrics are designed for free-tier monitoring (no external services needed).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Latency histogram bucket boundaries (microseconds).
const LATENCY_BUCKETS_US: &[u64] = &[
    100,       // 0.1ms
    500,       // 0.5ms
    1_000,     // 1ms
    5_000,     // 5ms
    10_000,    // 10ms
    25_000,    // 25ms
    50_000,    // 50ms
    100_000,   // 100ms
    250_000,   // 250ms
    500_000,   // 500ms
    1_000_000, // 1s
    2_000_000, // 2s
];

/// Number of latency histogram buckets, derived from the bounds so the array
/// length and the rendered series can never drift apart.
const N_BUCKETS: usize = LATENCY_BUCKETS_US.len();

/// Largest proxy-observed upstream lag accepted as a real sample (microseconds).
///
/// A marker older than this is stale (the response never came, or the forward
/// metadata outlived its session) and would poison the average, so it is
/// discarded instead of recorded.
const MAX_UPSTREAM_LAG_US: u64 = 2_000_000;

/// Hard cap on the number of distinct `(game, country)` telemetry cells.
///
/// The client-controlled `game_id` and `client_country` labels decide the key,
/// so an unbounded map would let a client grow proxy memory without limit.
/// Reports that would create a cell past this cap are counted as rejected.
pub const MAX_TELEMETRY_CELLS: usize = 1024;

/// Minimum number of reports a cell must hold before it is emitted.
///
/// This is a k-anonymity floor: emitting a cell backed by one or two reports
/// could correlate a small population back to an individual client, so such
/// cells are withheld until the population is large enough.
pub const MIN_TELEMETRY_CELL_REPORTS: u64 = 3;

/// Aggregated opt-in telemetry for one `(game_id, normalized country)` pair.
///
/// Percentile fields are *sums* of the values individual clients reported
/// (each client's own median/percentile). Dividing a sum by `reports` yields
/// the mean of the clients' reported values, not a population percentile.
#[derive(Debug, Default, Clone)]
pub struct TelemetryCell {
    /// Number of reports aggregated into this cell.
    pub reports: u64,
    /// Sum of the reports' `sample_count` values.
    pub samples: u64,
    /// Sum of the reports' `p50_ms` values.
    pub p50_sum_ms: f64,
    /// Sum of the reports' `p95_ms` values.
    pub p95_sum_ms: f64,
    /// Sum of the reports' `p99_ms` values.
    pub p99_sum_ms: f64,
    /// Sum of the reports' `jitter_ms` values.
    pub jitter_sum_ms: f64,
    /// Sum of the reports' `fec_recoveries` values.
    pub fec_recoveries: u64,
    /// Sum of the reports' `fec_losses` values.
    pub fec_losses: u64,
}

/// Bounded per-`(game, country)` telemetry aggregator.
///
/// Keyed by the raw numeric `game_id` (so unknown ids stay distinct) and the
/// normalized two-letter country. Counters accumulate on ingest; the map is
/// capped at [`TelemetryAggregator::max_cells`], after which new keys are
/// counted in `rejected` instead of inserted.
#[derive(Debug)]
pub struct TelemetryAggregator {
    /// Per-cell aggregates, keyed by `(game_id, normalized country)`.
    pub cells: HashMap<(u8, String), TelemetryCell>,
    /// Reports dropped because the cell cap was reached.
    pub rejected: u64,
    /// Maximum number of distinct cells retained.
    pub max_cells: usize,
}

impl Default for TelemetryAggregator {
    fn default() -> Self {
        Self {
            cells: HashMap::new(),
            rejected: 0,
            max_cells: MAX_TELEMETRY_CELLS,
        }
    }
}

/// Normalize a client-supplied country into a safe, bounded label value.
///
/// Trims surrounding whitespace, keeps only ASCII alphabetic characters,
/// uppercases them, and returns the result only when it is exactly two letters.
/// Anything else (empty, longer, digits, punctuation) collapses to `"XX"` so a
/// client cannot inject arbitrary label text or high-cardinality values.
fn normalize_country(raw: &str) -> String {
    let cleaned: String = raw
        .trim()
        .chars()
        .filter(char::is_ascii_alphabetic)
        .map(|c| c.to_ascii_uppercase())
        .collect();
    if cleaned.len() == 2 {
        cleaned
    } else {
        "XX".to_string()
    }
}

/// Compute a proxy-observed upstream response lag from two monotonic microsecond
/// timestamps.
///
/// `marker_us` is the session-clock time at which a client packet was forwarded
/// to the game server; `now_us` is the session-clock time at which the next
/// response was read on that same socket. Returns `None` when there is no
/// usable sample: no marker was set (`marker_us == 0`), the response is not
/// strictly later than the forward (zero or negative lag), or the gap exceeds
/// [`MAX_UPSTREAM_LAG_US`]. The difference uses `saturating_sub` so it can
/// never underflow.
pub fn upstream_lag(now_us: u64, marker_us: u64) -> Option<u64> {
    if marker_us == 0 || now_us <= marker_us {
        return None;
    }
    let lag = now_us.saturating_sub(marker_us);
    if lag > MAX_UPSTREAM_LAG_US {
        None
    } else {
        Some(lag)
    }
}

/// Why the relay dropped a packet.
///
/// Every drop increments [`ProxyMetrics::packets_dropped`] (the honest sum of
/// all reasons) exactly once and increments exactly one category counter, so
/// the categories always partition the total.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropReason {
    /// Rejected by the per-client rate limiter (packet or byte rate).
    RateLimit,
    /// The tunnel header could not be decoded (bad version, length, or address).
    Malformed,
    /// Failed IP/session-token authentication.
    Auth,
    /// Blocked by the abuse detector: private destination, allowlist miss,
    /// ban, reflection, or amplification.
    Abuse,
    /// FEC framing was invalid (payload too short or FEC header decode failed).
    FecMalformed,
    /// A relay session could not be created (for example the session cap).
    SessionSetup,
    /// Forwarding the payload to the game server failed at the socket.
    RelaySendError,
}

/// Proxy metrics collector.
pub struct ProxyMetrics {
    /// Total packets relayed.
    pub packets_relayed: AtomicU64,
    /// Total bytes relayed.
    pub bytes_relayed: AtomicU64,
    /// Active client connections.
    pub active_connections: AtomicU64,
    /// Packets dropped (rate limit, auth failure, etc.).
    pub packets_dropped: AtomicU64,
    /// Total relay latency samples (microseconds, for averaging).
    pub relay_latency_sum_us: AtomicU64,
    /// Number of latency samples.
    pub relay_latency_count: AtomicU64,
    /// Latency samples rejected as unusable (no marker, zero lag, or > 2s).
    pub latency_discarded: AtomicU64,

    // ── FEC metrics ─────────────────────────────────────────────
    /// Total FEC parity packets received.
    pub fec_parity_received: AtomicU64,
    /// Total FEC recovery successes (lost packets recovered).
    pub fec_recoveries: AtomicU64,
    /// Total FEC data packets processed.
    pub fec_data_packets: AtomicU64,

    // ── Security metrics ────────────────────────────────────────
    /// Packets rejected by auth.
    pub auth_rejections: AtomicU64,
    /// Packets blocked by abuse detector.
    pub abuse_blocks: AtomicU64,
    /// Rate limit hits.
    pub rate_limit_hits: AtomicU64,
    /// Rate limit hits attributed to the per-IP aggregate tier. A subset of
    /// `rate_limit_hits`.
    pub rate_limit_ip_hits: AtomicU64,
    /// Packets rejected because the per-IP tracking table was full (fail closed).
    pub rate_limit_overflow: AtomicU64,
    /// Packets dropped because the tunnel header was malformed.
    pub drops_malformed: AtomicU64,
    /// Packets dropped because FEC framing was malformed.
    pub drops_fec_malformed: AtomicU64,
    /// Packets dropped because a relay session could not be created.
    pub drops_session_setup: AtomicU64,
    /// Packets dropped because forwarding to the game server failed.
    pub drops_relay_send_errors: AtomicU64,

    // ── Session metrics ─────────────────────────────────────────
    /// Total sessions created (lifetime).
    pub sessions_created: AtomicU64,
    /// Total sessions expired (lifetime).
    pub sessions_expired: AtomicU64,

    // ── Inbound batch metrics (recvmmsg effectiveness) ───────────
    /// Number of inbound receive syscalls (batches).  On Linux with recvmmsg
    /// this is the number of `recvmmsg` calls; on other platforms it equals
    /// `inbound_packets_received` (one call per packet).
    pub inbound_batches_total: AtomicU64,
    /// Total packets received at the inbound socket before any filtering.
    /// `inbound_packets_received / inbound_batches_total` = avg batch size.
    pub inbound_packets_received: AtomicU64,

    // ── Telemetry ────────────────────────────────────────────────
    /// Bounded aggregation of opt-in client telemetry, keyed by
    /// `(game_id, normalized country)`.
    pub telemetry: std::sync::Mutex<TelemetryAggregator>,

    // ── Latency histogram buckets ───────────────────────────────
    /// Per-bucket (non-cumulative) relay latency counts.
    latency_buckets: [AtomicU64; N_BUCKETS],

    // ── Process start time ──────────────────────────────────────
    /// When the proxy started (for uptime gauge).
    pub start_time: Instant,
}

impl Default for ProxyMetrics {
    fn default() -> Self {
        Self::new()
    }
}

impl ProxyMetrics {
    /// Create a new metrics collector.
    pub fn new() -> Self {
        Self {
            packets_relayed: AtomicU64::new(0),
            bytes_relayed: AtomicU64::new(0),
            active_connections: AtomicU64::new(0),
            packets_dropped: AtomicU64::new(0),
            relay_latency_sum_us: AtomicU64::new(0),
            relay_latency_count: AtomicU64::new(0),
            latency_discarded: AtomicU64::new(0),
            fec_parity_received: AtomicU64::new(0),
            fec_recoveries: AtomicU64::new(0),
            fec_data_packets: AtomicU64::new(0),
            auth_rejections: AtomicU64::new(0),
            abuse_blocks: AtomicU64::new(0),
            rate_limit_hits: AtomicU64::new(0),
            rate_limit_ip_hits: AtomicU64::new(0),
            rate_limit_overflow: AtomicU64::new(0),
            drops_malformed: AtomicU64::new(0),
            drops_fec_malformed: AtomicU64::new(0),
            drops_session_setup: AtomicU64::new(0),
            drops_relay_send_errors: AtomicU64::new(0),
            sessions_created: AtomicU64::new(0),
            sessions_expired: AtomicU64::new(0),
            inbound_batches_total: AtomicU64::new(0),
            inbound_packets_received: AtomicU64::new(0),
            telemetry: std::sync::Mutex::new(TelemetryAggregator::default()),
            latency_buckets: Default::default(),
            start_time: Instant::now(),
        }
    }

    /// Record a relayed packet.
    pub fn record_relay(&self, bytes: u64) {
        self.packets_relayed.fetch_add(1, Ordering::Relaxed);
        self.bytes_relayed.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record a dropped packet and its reason.
    ///
    /// Always increments `packets_dropped` (the sum of all reasons) and
    /// exactly one category counter.
    pub fn record_drop(&self, reason: DropReason) {
        self.packets_dropped.fetch_add(1, Ordering::Relaxed);
        match reason {
            DropReason::RateLimit => self.record_rate_limit(),
            DropReason::Malformed => {
                self.drops_malformed.fetch_add(1, Ordering::Relaxed);
            }
            DropReason::Auth => self.record_auth_rejection(),
            DropReason::Abuse => self.record_abuse_block(),
            DropReason::FecMalformed => {
                self.drops_fec_malformed.fetch_add(1, Ordering::Relaxed);
            }
            DropReason::SessionSetup => {
                self.drops_session_setup.fetch_add(1, Ordering::Relaxed);
            }
            DropReason::RelaySendError => {
                self.drops_relay_send_errors.fetch_add(1, Ordering::Relaxed);
            }
        }
    }

    /// Record relay latency sample (with histogram).
    pub fn record_latency(&self, latency_us: u64) {
        self.relay_latency_sum_us
            .fetch_add(latency_us, Ordering::Relaxed);
        self.relay_latency_count.fetch_add(1, Ordering::Relaxed);

        // Store a per-bucket delta: increment ONLY the first bucket whose bound
        // covers this sample. `to_prometheus` converts these deltas into the
        // cumulative series Prometheus expects, so the two must not both
        // accumulate.
        for (i, &bound) in LATENCY_BUCKETS_US.iter().enumerate() {
            if latency_us <= bound {
                self.latency_buckets[i].fetch_add(1, Ordering::Relaxed);
                break;
            }
        }
    }

    /// Record a latency sample that could not be used.
    ///
    /// This happens when a response arrives with no matching forward marker,
    /// when the measured gap is zero, or when the gap exceeds the 2s sanity
    /// bound. The sample is counted here rather than fed to
    /// [`Self::record_latency`] so a stale marker cannot distort the histogram.
    pub fn record_latency_discarded(&self) {
        self.latency_discarded.fetch_add(1, Ordering::Relaxed);
    }

    /// Record FEC parity packet received.
    pub fn record_fec_parity(&self) {
        self.fec_parity_received.fetch_add(1, Ordering::Relaxed);
    }

    /// Record FEC recovery success.
    pub fn record_fec_recovery(&self) {
        self.fec_recoveries.fetch_add(1, Ordering::Relaxed);
    }

    /// Record FEC data packet processed.
    pub fn record_fec_data(&self) {
        self.fec_data_packets.fetch_add(1, Ordering::Relaxed);
    }

    /// Record auth rejection.
    pub fn record_auth_rejection(&self) {
        self.auth_rejections.fetch_add(1, Ordering::Relaxed);
    }

    /// Record abuse block.
    pub fn record_abuse_block(&self) {
        self.abuse_blocks.fetch_add(1, Ordering::Relaxed);
    }

    /// Record rate limit hit.
    pub fn record_rate_limit(&self) {
        self.rate_limit_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a rate limit hit attributed to the per-IP aggregate tier.
    ///
    /// This is a subset counter. The combined `rate_limit_hits` (and
    /// `packets_dropped`) are already incremented exactly once by
    /// [`Self::record_drop`] with [`DropReason::RateLimit`], so incrementing the
    /// combined counter here as well would double count an IP-tier drop and
    /// break the invariant that the category counters sum to `packets_dropped`.
    pub fn record_rate_limit_ip(&self) {
        self.rate_limit_ip_hits.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a packet rejected because the per-IP tracking table was full.
    pub fn record_rate_limit_overflow(&self) {
        self.rate_limit_overflow.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an anonymous opt-in telemetry report received from a client.
    ///
    /// The report is normalized and folded into its `(game_id, country)` cell.
    /// When the cell map is already at [`TelemetryAggregator::max_cells`] and
    /// this report would create a new key, it is counted in `rejected` instead.
    pub fn record_telemetry_report(&self, report: &lightspeed_protocol::TelemetryReport) {
        let country = normalize_country(&report.client_country);
        let key = (report.game_id, country);
        let mut agg = self
            .telemetry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if !agg.cells.contains_key(&key) && agg.cells.len() >= agg.max_cells {
            agg.rejected += 1;
            return;
        }
        let cell = agg.cells.entry(key).or_default();
        cell.reports += 1;
        cell.samples += u64::from(report.sample_count);
        cell.p50_sum_ms += f64::from(report.p50_ms);
        cell.p95_sum_ms += f64::from(report.p95_ms);
        cell.p99_sum_ms += f64::from(report.p99_ms);
        cell.jitter_sum_ms += f64::from(report.jitter_ms);
        cell.fec_recoveries += u64::from(report.fec_recoveries);
        cell.fec_losses += u64::from(report.fec_losses);
    }

    /// Build a collector whose telemetry map is capped at `max_cells`.
    ///
    /// Test-only: production always uses [`MAX_TELEMETRY_CELLS`].
    #[cfg(test)]
    pub fn with_max_telemetry_cells(max_cells: usize) -> Self {
        let metrics = Self::new();
        metrics.telemetry.lock().unwrap().max_cells = max_cells;
        metrics
    }

    /// Record one inbound receive batch containing `n` packets.
    ///
    /// On Linux with `recvmmsg` this is called once per `recvmmsg` syscall.
    /// On other platforms it is called once per `recv_from` call with `n = 1`.
    pub fn record_inbound_batch(&self, n: usize) {
        self.inbound_batches_total.fetch_add(1, Ordering::Relaxed);
        self.inbound_packets_received
            .fetch_add(n as u64, Ordering::Relaxed);
    }

    /// Record a new session created.
    pub fn record_session_created(&self) {
        self.sessions_created.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a session expired/cleaned.
    pub fn record_session_expired(&self, count: u64) {
        self.sessions_expired.fetch_add(count, Ordering::Relaxed);
    }

    /// Get average relay latency in microseconds.
    pub fn avg_latency_us(&self) -> f64 {
        let count = self.relay_latency_count.load(Ordering::Relaxed);
        if count == 0 {
            return 0.0;
        }
        let sum = self.relay_latency_sum_us.load(Ordering::Relaxed);
        sum as f64 / count as f64
    }

    /// Format metrics in Prometheus exposition format.
    pub fn to_prometheus(&self, region: &str, node_id: &str) -> String {
        let labels = format!("region=\"{}\",node_id=\"{}\"", region, node_id);
        let uptime = self.start_time.elapsed().as_secs();

        let mut out = String::with_capacity(4096);

        // ── Relay counters ──────────────────────────────────────
        out.push_str("# HELP lightspeed_packets_relayed_total Total packets relayed\n");
        out.push_str("# TYPE lightspeed_packets_relayed_total counter\n");
        out.push_str(&format!(
            "lightspeed_packets_relayed_total{{{}}} {}\n",
            labels,
            self.packets_relayed.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP lightspeed_bytes_relayed_total Total bytes relayed\n");
        out.push_str("# TYPE lightspeed_bytes_relayed_total counter\n");
        out.push_str(&format!(
            "lightspeed_bytes_relayed_total{{{}}} {}\n",
            labels,
            self.bytes_relayed.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP lightspeed_packets_dropped_total Total packets dropped\n");
        out.push_str("# TYPE lightspeed_packets_dropped_total counter\n");
        out.push_str(&format!(
            "lightspeed_packets_dropped_total{{{}}} {}\n",
            labels,
            self.packets_dropped.load(Ordering::Relaxed)
        ));

        // ── Gauges ──────────────────────────────────────────────
        out.push_str("# HELP lightspeed_active_connections Current active client connections\n");
        out.push_str("# TYPE lightspeed_active_connections gauge\n");
        out.push_str(&format!(
            "lightspeed_active_connections{{{}}} {}\n",
            labels,
            self.active_connections.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP lightspeed_uptime_seconds Proxy uptime in seconds\n");
        out.push_str("# TYPE lightspeed_uptime_seconds gauge\n");
        out.push_str(&format!(
            "lightspeed_uptime_seconds{{{}}} {}\n",
            labels, uptime
        ));

        // ── Latency ─────────────────────────────────────────────
        out.push_str(
            "# HELP lightspeed_relay_latency_avg_us Average proxy-observed upstream response lag in microseconds (time from forwarding a client packet to the game server until the next response is read on that session socket; NOT client RTT)\n",
        );
        out.push_str("# TYPE lightspeed_relay_latency_avg_us gauge\n");
        out.push_str(&format!(
            "lightspeed_relay_latency_avg_us{{{}}} {:.1}\n",
            labels,
            self.avg_latency_us()
        ));

        // Latency histogram
        out.push_str(
            "# HELP lightspeed_relay_latency_us Proxy-observed upstream response lag histogram in microseconds (NOT client RTT)\n",
        );
        out.push_str("# TYPE lightspeed_relay_latency_us histogram\n");
        let total_count = self.relay_latency_count.load(Ordering::Relaxed);
        let total_sum = self.relay_latency_sum_us.load(Ordering::Relaxed);
        let mut cumulative = 0u64;
        for (i, &bound) in LATENCY_BUCKETS_US.iter().enumerate() {
            cumulative += self.latency_buckets[i].load(Ordering::Relaxed);
            out.push_str(&format!(
                "lightspeed_relay_latency_us_bucket{{{},le=\"{}\"}} {}\n",
                labels,
                bound as f64 / 1000.0, // convert to ms for readability
                cumulative
            ));
        }
        out.push_str(&format!(
            "lightspeed_relay_latency_us_bucket{{{},le=\"+Inf\"}} {}\n",
            labels, total_count
        ));
        out.push_str(&format!(
            "lightspeed_relay_latency_us_sum{{{}}} {}\n",
            labels, total_sum
        ));
        out.push_str(&format!(
            "lightspeed_relay_latency_us_count{{{}}} {}\n",
            labels, total_count
        ));

        out.push_str(
            "# HELP lightspeed_relay_latency_discarded_total Paired latency samples discarded as unusable (zero lag or beyond the 2s bound); responses with no pending forward are not sampled and are not counted\n",
        );
        out.push_str("# TYPE lightspeed_relay_latency_discarded_total counter\n");
        out.push_str(&format!(
            "lightspeed_relay_latency_discarded_total{{{}}} {}\n",
            labels,
            self.latency_discarded.load(Ordering::Relaxed)
        ));

        // ── FEC metrics ─────────────────────────────────────────
        out.push_str("# HELP lightspeed_fec_parity_received_total FEC parity packets received\n");
        out.push_str("# TYPE lightspeed_fec_parity_received_total counter\n");
        out.push_str(&format!(
            "lightspeed_fec_parity_received_total{{{}}} {}\n",
            labels,
            self.fec_parity_received.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP lightspeed_fec_recoveries_total Packets recovered via FEC\n");
        out.push_str("# TYPE lightspeed_fec_recoveries_total counter\n");
        out.push_str(&format!(
            "lightspeed_fec_recoveries_total{{{}}} {}\n",
            labels,
            self.fec_recoveries.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP lightspeed_fec_data_packets_total FEC data packets processed\n");
        out.push_str("# TYPE lightspeed_fec_data_packets_total counter\n");
        out.push_str(&format!(
            "lightspeed_fec_data_packets_total{{{}}} {}\n",
            labels,
            self.fec_data_packets.load(Ordering::Relaxed)
        ));

        // ── Security metrics ────────────────────────────────────
        out.push_str("# HELP lightspeed_auth_rejections_total Auth rejections\n");
        out.push_str("# TYPE lightspeed_auth_rejections_total counter\n");
        out.push_str(&format!(
            "lightspeed_auth_rejections_total{{{}}} {}\n",
            labels,
            self.auth_rejections.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP lightspeed_abuse_blocks_total Abuse detector blocks\n");
        out.push_str("# TYPE lightspeed_abuse_blocks_total counter\n");
        out.push_str(&format!(
            "lightspeed_abuse_blocks_total{{{}}} {}\n",
            labels,
            self.abuse_blocks.load(Ordering::Relaxed)
        ));

        out.push_str("# HELP lightspeed_rate_limit_hits_total Rate limit hits\n");
        out.push_str("# TYPE lightspeed_rate_limit_hits_total counter\n");
        out.push_str(&format!(
            "lightspeed_rate_limit_hits_total{{{}}} {}\n",
            labels,
            self.rate_limit_hits.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_rate_limit_ip_hits_total Rate limit hits from the per-IP aggregate tier\n",
        );
        out.push_str("# TYPE lightspeed_rate_limit_ip_hits_total counter\n");
        out.push_str(&format!(
            "lightspeed_rate_limit_ip_hits_total{{{}}} {}\n",
            labels,
            self.rate_limit_ip_hits.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_rate_limit_overflow_total Packets rejected because the per-IP rate limit table was full\n",
        );
        out.push_str("# TYPE lightspeed_rate_limit_overflow_total counter\n");
        out.push_str(&format!(
            "lightspeed_rate_limit_overflow_total{{{}}} {}\n",
            labels,
            self.rate_limit_overflow.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_drops_malformed_total Packets dropped for a malformed tunnel header\n",
        );
        out.push_str("# TYPE lightspeed_drops_malformed_total counter\n");
        out.push_str(&format!(
            "lightspeed_drops_malformed_total{{{}}} {}\n",
            labels,
            self.drops_malformed.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_drops_fec_malformed_total Packets dropped for malformed FEC framing\n",
        );
        out.push_str("# TYPE lightspeed_drops_fec_malformed_total counter\n");
        out.push_str(&format!(
            "lightspeed_drops_fec_malformed_total{{{}}} {}\n",
            labels,
            self.drops_fec_malformed.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_drops_session_setup_total Packets dropped because the relay session could not be created\n",
        );
        out.push_str("# TYPE lightspeed_drops_session_setup_total counter\n");
        out.push_str(&format!(
            "lightspeed_drops_session_setup_total{{{}}} {}\n",
            labels,
            self.drops_session_setup.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_drops_relay_send_errors_total Packets dropped because forwarding to the game server failed\n",
        );
        out.push_str("# TYPE lightspeed_drops_relay_send_errors_total counter\n");
        out.push_str(&format!(
            "lightspeed_drops_relay_send_errors_total{{{}}} {}\n",
            labels,
            self.drops_relay_send_errors.load(Ordering::Relaxed)
        ));

        // ── Session metrics ─────────────────────────────────────
        out.push_str(
            "# HELP lightspeed_sessions_created_total Total sessions created (lifetime)\n",
        );
        out.push_str("# TYPE lightspeed_sessions_created_total counter\n");
        out.push_str(&format!(
            "lightspeed_sessions_created_total{{{}}} {}\n",
            labels,
            self.sessions_created.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_sessions_expired_total Total sessions expired (lifetime)\n",
        );
        out.push_str("# TYPE lightspeed_sessions_expired_total counter\n");
        out.push_str(&format!(
            "lightspeed_sessions_expired_total{{{}}} {}\n",
            labels,
            self.sessions_expired.load(Ordering::Relaxed)
        ));

        // ── Inbound batch metrics ───────────────────────────────
        out.push_str(
            "# HELP lightspeed_inbound_batches_total Inbound receive syscalls (recvmmsg batches)\n",
        );
        out.push_str("# TYPE lightspeed_inbound_batches_total counter\n");
        out.push_str(&format!(
            "lightspeed_inbound_batches_total{{{}}} {}\n",
            labels,
            self.inbound_batches_total.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_inbound_packets_received_total Packets received at inbound socket (pre-filter)\n",
        );
        out.push_str("# TYPE lightspeed_inbound_packets_received_total counter\n");
        out.push_str(&format!(
            "lightspeed_inbound_packets_received_total{{{}}} {}\n",
            labels,
            self.inbound_packets_received.load(Ordering::Relaxed)
        ));

        // ── Client telemetry (aggregated, k-anonymized) ─────────
        // HELP/TYPE headers are emitted unconditionally so the family is
        // discoverable before the k-anonymity floor is reached.
        out.push_str(
            "# HELP lightspeed_telemetry_reports_total Anonymous opt-in client telemetry reports, aggregated by game and country\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_reports_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_samples_total Sum of client-reported RTT sample counts per telemetry cell\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_samples_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_p50_ms_sum Sum of client-reported p50 values (ms); _sum/_count is the mean of client medians, not a population p50\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_p50_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_p50_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_p95_ms_sum Sum of client-reported p95 values (ms); _sum/_count is the mean of client p95s, not a population p95\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_p95_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_p95_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_p99_ms_sum Sum of client-reported p99 values (ms); _sum/_count is the mean of client p99s, not a population p99\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_p99_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_p99_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_jitter_ms_sum Sum of client-reported jitter values (ms); _sum/_count is the mean of client jitter values, not a population statistic\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_jitter_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_jitter_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_fec_recoveries_total Sum of client-reported FEC recoveries\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_fec_recoveries_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_fec_losses_total Sum of client-reported FEC losses\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_fec_losses_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_rejected_total Telemetry reports dropped because the per-(game,country) cell cap was reached\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_rejected_total counter\n");

        {
            // Recover from a poisoned lock rather than panicking: a metrics
            // scrape must never bring down the health server.
            let agg = self
                .telemetry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            out.push_str(&format!(
                "lightspeed_telemetry_rejected_total{{{}}} {}\n",
                labels, agg.rejected
            ));
            for ((game_id, country), cell) in &agg.cells {
                if cell.reports < MIN_TELEMETRY_CELL_REPORTS {
                    continue;
                }
                let game = lightspeed_protocol::game_id::key_for_id(*game_id).unwrap_or("unknown");
                let cell_labels = format!("{},game=\"{}\",country=\"{}\"", labels, game, country);
                out.push_str(&format!(
                    "lightspeed_telemetry_reports_total{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_samples_total{{{}}} {}\n",
                    cell_labels, cell.samples
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_p50_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.p50_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_p50_ms_count{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_p95_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.p95_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_p95_ms_count{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_p99_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.p99_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_p99_ms_count{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_jitter_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.jitter_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_jitter_ms_count{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_fec_recoveries_total{{{}}} {}\n",
                    cell_labels, cell.fec_recoveries
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_fec_losses_total{{{}}} {}\n",
                    cell_labels, cell.fec_losses
                ));
            }
        }

        // ── Build info ──────────────────────────────────────────
        out.push_str("# HELP lightspeed_build_info Build information\n");
        out.push_str("# TYPE lightspeed_build_info gauge\n");
        out.push_str(&format!(
            "lightspeed_build_info{{{},version=\"{}\"}} 1\n",
            labels,
            env!("CARGO_PKG_VERSION")
        ));

        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prometheus_output_format() {
        let m = ProxyMetrics::new();
        m.record_relay(100);
        m.record_relay(200);
        m.record_drop(DropReason::Malformed);
        m.record_latency(5000);
        m.record_latency(15000);
        m.record_fec_parity();
        m.record_fec_recovery();
        m.record_fec_data();
        m.record_auth_rejection();
        m.record_abuse_block();
        m.record_rate_limit();
        m.record_session_created();
        m.record_session_expired(1);

        m.record_inbound_batch(4);

        let output = m.to_prometheus("us-west-lax", "proxy-lax");

        assert!(output.contains("lightspeed_packets_relayed_total"));
        assert!(output.contains("lightspeed_bytes_relayed_total"));
        assert!(output.contains("lightspeed_active_connections"));
        assert!(output.contains("lightspeed_uptime_seconds"));
        assert!(output.contains("lightspeed_fec_recoveries_total"));
        assert!(output.contains("lightspeed_auth_rejections_total"));
        assert!(output.contains("lightspeed_build_info"));
        assert!(output.contains("lightspeed_inbound_batches_total"));
        assert!(output.contains("lightspeed_inbound_packets_received_total"));
        assert!(output.contains("lightspeed_relay_latency_us_bucket"));
        assert!(output.contains("lightspeed_telemetry_reports_total"));
        assert!(output.contains("region=\"us-west-lax\""));
        assert!(output.contains("node_id=\"proxy-lax\""));
    }

    #[test]
    fn test_latency_histogram_buckets() {
        let m = ProxyMetrics::new();
        // 0.05ms — fits in 0.1ms bucket
        m.record_latency(50);
        // 2ms — fits in 5ms bucket
        m.record_latency(2000);
        // 75ms — fits in 100ms bucket
        m.record_latency(75000);

        let output = m.to_prometheus("test", "test-node");
        // All 3 should be in the +Inf bucket
        assert!(output.contains("le=\"+Inf\"} 3"));
    }

    #[test]
    fn test_avg_latency_zero_samples() {
        let m = ProxyMetrics::new();
        assert_eq!(m.avg_latency_us(), 0.0);
    }

    #[test]
    fn test_avg_latency_with_samples() {
        let m = ProxyMetrics::new();
        m.record_latency(1000);
        m.record_latency(3000);
        assert_eq!(m.avg_latency_us(), 2000.0);
    }

    #[test]
    fn test_record_latency_is_non_cumulative() {
        let m = ProxyMetrics::new();
        m.record_latency(2000);

        let output = m.to_prometheus("test", "test-node");

        assert!(output.contains("le=\"5\"} 1"));
        assert!(output.contains("le=\"1\"} 0"));
        assert!(output.contains("le=\"+Inf\"} 1"));
        // The sample belongs to exactly one bucket, so the rendered cumulative
        // series must count it once at every larger bound, not once per bucket.
        assert!(output.contains("le=\"10\"} 1"));
    }

    #[test]
    fn test_drop_sum_identity() {
        let m = ProxyMetrics::new();
        for reason in [
            DropReason::RateLimit,
            DropReason::Malformed,
            DropReason::Auth,
            DropReason::Abuse,
            DropReason::FecMalformed,
            DropReason::SessionSetup,
            DropReason::RelaySendError,
        ] {
            m.record_drop(reason);
        }

        let categories = m.drops_malformed.load(Ordering::Relaxed)
            + m.auth_rejections.load(Ordering::Relaxed)
            + m.abuse_blocks.load(Ordering::Relaxed)
            + m.rate_limit_hits.load(Ordering::Relaxed)
            + m.drops_fec_malformed.load(Ordering::Relaxed)
            + m.drops_session_setup.load(Ordering::Relaxed)
            + m.drops_relay_send_errors.load(Ordering::Relaxed);

        assert_eq!(categories, m.packets_dropped.load(Ordering::Relaxed));
    }

    #[test]
    fn test_fec_data_counter_emitted() {
        let m = ProxyMetrics::new();
        m.record_fec_data();

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains("lightspeed_fec_data_packets_total"));
        assert!(output.contains(
            "lightspeed_fec_data_packets_total{region=\"test\",node_id=\"test-node\"} 1"
        ));
    }

    #[test]
    fn test_upstream_lag_bounds() {
        // A sane positive lag is reported as-is.
        assert_eq!(upstream_lag(100, 50), Some(50));
        // Zero lag (response in the same microsecond) is not a usable sample.
        assert_eq!(upstream_lag(50, 50), None);
        // No marker recorded (0) means nothing was forwarded.
        assert_eq!(upstream_lag(3_000_000, 0), None);
        assert_eq!(upstream_lag(100, 0), None);
        // The 2s bound is inclusive; anything older is discarded.
        assert_eq!(upstream_lag(2_000_001, 1), Some(2_000_000));
        assert_eq!(upstream_lag(2_000_002, 1), None);
    }

    #[test]
    fn test_latency_help_is_upstream_lag() {
        let m = ProxyMetrics::new();
        let output = m.to_prometheus("test", "test-node");

        assert!(
            output.contains("upstream response lag"),
            "latency HELP text must describe proxy-observed upstream response lag"
        );
    }

    #[test]
    fn test_latency_discarded_counter_emitted() {
        let m = ProxyMetrics::new();
        m.record_latency_discarded();

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains(
            "lightspeed_relay_latency_discarded_total{region=\"test\",node_id=\"test-node\"} 1"
        ));
    }

    #[test]
    fn test_rate_limit_ip_hits_and_combined() {
        let m = ProxyMetrics::new();
        // An IP-tier drop is recorded like any other rate-limit drop (combined
        // counter + packets_dropped), plus the per-IP subset marker.
        m.record_drop(DropReason::RateLimit);
        m.record_rate_limit_ip();

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains(
            "lightspeed_rate_limit_ip_hits_total{region=\"test\",node_id=\"test-node\"} 1"
        ));
        // The subset marker must not also bump the combined counter.
        assert!(output
            .contains("lightspeed_rate_limit_hits_total{region=\"test\",node_id=\"test-node\"} 1"));
        assert_eq!(m.packets_dropped.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_ip_tier_drop_preserves_sum_identity() {
        let m = ProxyMetrics::new();
        // Simulate an IP-tier rejection exactly as relay.rs records it. If the
        // per-IP marker also bumped the combined counter, the categories would
        // exceed packets_dropped.
        m.record_drop(DropReason::RateLimit);
        m.record_rate_limit_ip();

        let categories = m.drops_malformed.load(Ordering::Relaxed)
            + m.auth_rejections.load(Ordering::Relaxed)
            + m.abuse_blocks.load(Ordering::Relaxed)
            + m.rate_limit_hits.load(Ordering::Relaxed)
            + m.drops_fec_malformed.load(Ordering::Relaxed)
            + m.drops_session_setup.load(Ordering::Relaxed)
            + m.drops_relay_send_errors.load(Ordering::Relaxed);

        assert_eq!(categories, m.packets_dropped.load(Ordering::Relaxed));
        assert_eq!(m.rate_limit_ip_hits.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_rate_limit_overflow_counter_emitted() {
        let m = ProxyMetrics::new();
        m.record_rate_limit_overflow();

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains(
            "lightspeed_rate_limit_overflow_total{region=\"test\",node_id=\"test-node\"} 1"
        ));
    }

    fn report(game_id: u8, country: &str) -> lightspeed_protocol::TelemetryReport {
        lightspeed_protocol::TelemetryReport {
            game_id,
            client_country: country.to_string(),
            p50_ms: 30.0,
            p95_ms: 50.0,
            p99_ms: 80.0,
            jitter_ms: 2.0,
            sample_count: 100,
            fec_recoveries: 1,
            fec_losses: 0,
            client_version: "1.4.4".to_string(),
        }
    }

    #[test]
    fn test_telemetry_aggregator_sums_and_counts() {
        let m = ProxyMetrics::new();

        let mut second = report(2, "us ");
        second.sample_count = 200;
        second.p50_ms = 40.0;
        second.p95_ms = 60.0;
        second.p99_ms = 90.0;
        second.jitter_ms = 3.0;
        second.fec_recoveries = 2;
        second.fec_losses = 1;

        let mut third = report(2, "us ");
        third.sample_count = 300;
        third.p50_ms = 50.0;
        third.p95_ms = 70.0;
        third.p99_ms = 100.0;
        third.jitter_ms = 4.0;
        third.fec_recoveries = 3;
        third.fec_losses = 2;

        m.record_telemetry_report(&report(2, "us "));
        m.record_telemetry_report(&second);
        m.record_telemetry_report(&third);

        let output = m.to_prometheus("test", "test-node");

        assert!(output.contains(
            "lightspeed_telemetry_reports_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"} 3"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_samples_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"} 600"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_p50_ms_sum{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"} 120.0"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_p50_ms_count{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"} 3"
        ));
        // Distinct jitter and FEC values must sum, not overwrite.
        assert!(output.contains(
            "lightspeed_telemetry_jitter_ms_sum{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"} 9.0"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_fec_recoveries_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"} 6"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_fec_losses_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"} 3"
        ));
    }

    #[test]
    fn test_telemetry_normalization() {
        let m = ProxyMetrics::new();

        for _ in 0..3 {
            m.record_telemetry_report(&report(2, " usa "));
        }
        for _ in 0..3 {
            m.record_telemetry_report(&report(2, "th"));
        }
        for _ in 0..3 {
            m.record_telemetry_report(&report(2, ""));
        }
        for _ in 0..3 {
            m.record_telemetry_report(&report(250, "th"));
        }

        let output = m.to_prometheus("test", "test-node");

        // " usa " and "" both collapse to XX; "th" uppercases to TH; 3-letter
        // codes are rejected outright.
        assert!(output.contains("game=\"cs2\",country=\"XX\""));
        assert!(output.contains("game=\"cs2\",country=\"TH\""));
        assert!(output.contains("game=\"unknown\",country=\"TH\""));
        assert!(!output.contains("country=\"USA\""));
        assert!(!output.contains("country=\"us \""));
    }

    #[test]
    fn test_telemetry_cell_cap_overflows() {
        let m = ProxyMetrics::with_max_telemetry_cells(2);

        m.record_telemetry_report(&report(1, "US"));
        m.record_telemetry_report(&report(2, "US"));
        // Third distinct cell is over the cap and must be rejected, not stored.
        m.record_telemetry_report(&report(3, "US"));

        assert_eq!(m.telemetry.lock().unwrap().cells.len(), 2);
        assert_eq!(m.telemetry.lock().unwrap().rejected, 1);

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains(
            "lightspeed_telemetry_rejected_total{region=\"test\",node_id=\"test-node\"} 1"
        ));
        assert!(!output.contains("game=\"dota2\""));
    }

    #[test]
    fn test_telemetry_cells_below_k_are_suppressed() {
        let m = ProxyMetrics::new();
        m.record_telemetry_report(&report(2, "US"));

        let output = m.to_prometheus("test", "test-node");

        assert!(
            !output.contains("game=\"cs2\",country=\"US\""),
            "a cell below the k-anonymity floor must not be emitted"
        );
    }
}
