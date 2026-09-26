//! # Proxy Metrics
//!
//! Collects and exports proxy performance metrics via Prometheus format.
//! Includes counters, gauges, and histogram-style latency buckets for
//! comprehensive observability.
//!
//! All metrics are designed for free-tier monitoring (no external services needed).

use std::collections::{HashMap, HashSet};
use std::net::IpAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use crate::geo::GeoSide;

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

/// Minimum number of report observations a per-path or geo cell must hold
/// before it is emitted.
///
/// This is a k-anonymity floor for the aggregators that have no source address
/// to key on (`RouteTelemetryAggregator`, `GeoAggregator`): emitting a cell
/// backed by one or two observations could correlate a small population back
/// to an individual client, so such cells are withheld until the population is
/// large enough. The flat client telemetry cell instead floors on distinct
/// source IPs, see [`MIN_TELEMETRY_CELL_SOURCE_IPS`].
pub const MIN_TELEMETRY_CELL_REPORTS: u64 = 3;

/// Minimum number of distinct source IPs a client telemetry cell must have
/// seen before it is emitted.
///
/// Distinct sources are the real k-anonymity floor for client-submitted
/// reports: a single client can inflate [`MIN_TELEMETRY_CELL_REPORTS`] by
/// flushing repeatedly, but it cannot manufacture distinct source addresses.
/// Matches the report floor value so the published k = 3 promise is unchanged.
pub const MIN_TELEMETRY_CELL_SOURCE_IPS: usize = 3;

/// Hard cap on the distinct source IPs retained per client telemetry cell.
///
/// The export decision is final once the cell has seen
/// [`MIN_TELEMETRY_CELL_SOURCE_IPS`] distinct sources, so the set stops
/// growing exactly there. This is the per-cell analogue of
/// [`MAX_TELEMETRY_CELLS`]: it bounds memory even when a client rotates source
/// addresses, and the addresses are never exported.
const MAX_TELEMETRY_CELL_SOURCE_IPS: usize = MIN_TELEMETRY_CELL_SOURCE_IPS;

/// Bounded set of distinct source IP addresses observed in one telemetry cell.
///
/// Addresses are retained only to enforce [`MIN_TELEMETRY_CELL_SOURCE_IPS`] and
/// are never exported. The manual `Debug` impl reports only the count, so an
/// address cannot reach a log line through a derived debug dump.
#[derive(Clone, Default)]
struct SourceIpSet {
    ips: HashSet<IpAddr>,
}

impl SourceIpSet {
    /// Record `ip`, ignored once the set is full or already contains it.
    fn insert(&mut self, ip: IpAddr) -> bool {
        self.ips.len() < MAX_TELEMETRY_CELL_SOURCE_IPS && self.ips.insert(ip)
    }

    /// Number of distinct source IPs observed, capped at
    /// [`MAX_TELEMETRY_CELL_SOURCE_IPS`].
    fn len(&self) -> usize {
        self.ips.len()
    }
}

impl std::fmt::Debug for SourceIpSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SourceIpSet")
            .field("distinct", &self.ips.len())
            .finish()
    }
}

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
    /// Sum of the reports' `direct_p50_ms` values, over reports where present.
    pub direct_sum_ms: f64,
    /// Number of reports that carried `direct_p50_ms`.
    pub direct_count: u64,
    /// Sum of the reports' `relayed_p50_ms` values, over reports where present.
    pub relayed_sum_ms: f64,
    /// Number of reports that carried `relayed_p50_ms`.
    pub relayed_count: u64,
    /// Sum of the reports' `direct_app_p50_ms` values, over reports where
    /// present.
    pub direct_app_sum_ms: f64,
    /// Number of reports that carried `direct_app_p50_ms`.
    pub direct_app_count: u64,
    /// Sum of `direct_p50_ms - relayed_p50_ms` over reports where both are
    /// present.
    pub saved_sum_ms: f64,
    /// Number of reports where both latency values were present.
    pub saved_count: u64,
    /// Sum of `direct_app_p50_ms - relayed_p50_ms` over reports where both are
    /// present, the client's paired app-to-app comparison. Signed: negative
    /// means the relayed path was slower than direct.
    pub saved_app_sum_ms: f64,
    /// Number of reports where both `direct_app_p50_ms` and `relayed_p50_ms`
    /// were present.
    pub saved_app_count: u64,
    /// Number of those paired reports whose saving was negative.
    pub saved_app_negative_count: u64,
    /// Distinct source IPs observed, used only to enforce the k-anonymity
    /// floor. Never exported; bounded by [`MAX_TELEMETRY_CELL_SOURCE_IPS`].
    source_ips: SourceIpSet,
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

/// Aggregated opt-in telemetry for one `(relay, game_id, country)` path.
///
/// Every field is a sum or count over the individual clients' per-leg
/// observations; no raw or per-client sample is retained. As with
/// [`TelemetryCell`], `rtt_*_sum_ms / reports` is the mean of the values the
/// clients reported for that leg, not a population percentile.
#[derive(Debug, Default, Clone)]
pub struct RouteTelemetryCell {
    /// Number of leg observations folded into this cell.
    pub reports: u64,
    /// Sum of the legs' `samples` counts.
    pub samples: u64,
    /// Sum of the legs' `rtt_p50_ms` values.
    pub rtt_p50_sum_ms: f64,
    /// Sum of the legs' `rtt_p95_ms` values.
    pub rtt_p95_sum_ms: f64,
    /// Sum of the legs' `rtt_p99_ms` values.
    pub rtt_p99_sum_ms: f64,
    /// Sum of the legs' `jitter_ms` values.
    pub jitter_sum_ms: f64,
    /// Sum of the legs' `lost` counts.
    pub lost: u64,
    /// Sum of the legs' `recovered` counts.
    pub recovered: u64,
    /// Sum of the legs' `dedup_saved` counts.
    pub dedup_saved: u64,
    /// Distinct sources, used only to enforce the k floor at render time.
    source_ips: SourceIpSet,
}

/// Bounded per-`(relay, game, country)` telemetry aggregator.
///
/// Keyed by the normalized relay id, the raw numeric `game_id`, and the
/// normalized two-letter country. Counters accumulate on ingest; the map is
/// capped at [`RouteTelemetryAggregator::max_cells`], after which new keys are
/// counted in `rejected` instead of inserted.
#[derive(Debug)]
pub struct RouteTelemetryAggregator {
    /// Per-cell aggregates, keyed by `(normalized relay, game_id, country)`.
    pub cells: HashMap<(String, u8, String), RouteTelemetryCell>,
    /// Leg observations dropped because the cell cap was reached.
    pub rejected: u64,
    /// Maximum number of distinct cells retained.
    pub max_cells: usize,
}

impl Default for RouteTelemetryAggregator {
    fn default() -> Self {
        Self {
            cells: HashMap::new(),
            rejected: 0,
            max_cells: MAX_TELEMETRY_CELLS,
        }
    }
}

/// Bounded proxy-observed session aggregation keyed by
/// `(src_country, dst_country)`.
///
/// Both labels are resolved by the proxy itself, so they are never
/// client-controlled. Counters accumulate when a new session is created; the
/// map is capped at [`GeoAggregator::max_cells`], after which a novel key is
/// counted in `rejected` instead of inserted, mirroring the telemetry
/// aggregators.
#[derive(Debug)]
pub struct GeoAggregator {
    /// Per-cell session counts, keyed by `(src_country, dst_country)`.
    pub cells: HashMap<(String, String), u64>,
    /// Sessions dropped because the cell cap was reached.
    pub rejected: u64,
    /// Maximum number of distinct cells retained.
    pub max_cells: usize,
    /// Sessions whose source address could not be resolved to a country.
    pub misses_src: u64,
    /// Sessions whose destination address could not be resolved to a country.
    pub misses_dst: u64,
    /// New sessions skipped because the destination is the relay itself.
    pub skipped_self_tunnel: u64,
}

impl Default for GeoAggregator {
    fn default() -> Self {
        Self {
            cells: HashMap::new(),
            rejected: 0,
            max_cells: MAX_TELEMETRY_CELLS,
            misses_src: 0,
            misses_dst: 0,
            skipped_self_tunnel: 0,
        }
    }
}

impl GeoAggregator {
    /// Fold one new session's country pair into the bounded map.
    pub fn record_session(&mut self, src: &str, dst: &str) {
        let key = (src.to_string(), dst.to_string());
        if !self.cells.contains_key(&key) && self.cells.len() >= self.max_cells {
            self.rejected += 1;
        } else {
            *self.cells.entry(key).or_insert(0) += 1;
        }
    }

    /// Count a country lookup miss on one side of the tunnel.
    pub fn record_miss(&mut self, side: GeoSide) {
        match side {
            GeoSide::Src => self.misses_src += 1,
            GeoSide::Dst => self.misses_dst += 1,
        }
    }

    /// Count a new session whose destination is the relay itself.
    pub fn record_self_tunnel_skip(&mut self) {
        self.skipped_self_tunnel += 1;
    }
}

/// Longest relay identifier accepted from a client.
///
/// Mirrors the protocol's `PathObservation` validation bound.
const MAX_RELAY_ID_LEN: usize = 64;

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

/// Normalize a client-supplied relay id into a safe, bounded label value.
///
/// Returns `None` for anything that does not satisfy the protocol's relay-id
/// charset (1..=64 bytes of ASCII alphanumerics or `.`, `_`, `-`). Accepted ids
/// are lowercased so case variants cannot inflate the cell cardinality. This is
/// a defense-in-depth re-check: the ingest boundary already runs
/// [`lightspeed_protocol::TelemetryReport::validate`].
fn normalize_relay(raw: &str) -> Option<String> {
    if raw.is_empty() || raw.len() > MAX_RELAY_ID_LEN {
        return None;
    }
    if !raw
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return None;
    }
    Some(raw.to_ascii_lowercase())
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
    /// A new session was refused because the configured egress budget hard
    /// threshold was reached. Existing sessions are never interrupted.
    EgressBudget,
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
    /// New sessions refused because the egress budget hard threshold was hit.
    pub drops_egress_budget: AtomicU64,

    // ── Egress budget guard ─────────────────────────────────────
    /// Binding egress budget limit in bytes (0 when no budget is enforced).
    pub egress_budget_limit_bytes: AtomicU64,
    /// Egress counted against the binding budget limit.
    pub egress_budget_used_bytes: AtomicU64,
    /// 1 once the soft threshold has been reached.
    pub egress_budget_soft_exceeded: AtomicU64,
    /// 1 once the hard threshold has been reached.
    pub egress_budget_hard_exceeded: AtomicU64,
    /// Number of soft-threshold transitions observed.
    pub egress_budget_soft_events: AtomicU64,

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

    // ── Socket hygiene (buffers + Linux busy-poll) ───────────────
    /// Socket options the kernel accepted. One increment per successful
    /// `setsockopt` (receive buffer, send buffer, or busy-poll).
    pub socket_opts_applied: AtomicU64,
    /// Socket options the kernel refused. Each refusal is skipped and never
    /// fatal; a non-zero value means that socket is running on a default the
    /// kernel would not let us raise.
    pub socket_opts_failed: AtomicU64,
    /// Receive sockets that enabled Linux `SO_BUSY_POLL`.
    pub busy_poll_sockets: AtomicU64,

    // ── Telemetry ────────────────────────────────────────────────
    /// Bounded aggregation of opt-in client telemetry, keyed by
    /// `(game_id, normalized country)`.
    pub telemetry: std::sync::Mutex<TelemetryAggregator>,
    /// Bounded aggregation of per-route-leg client telemetry, keyed by
    /// `(normalized relay, game_id, country)`.
    pub route_telemetry: std::sync::Mutex<RouteTelemetryAggregator>,

    // ── Proxy-observed session geo ──────────────────────────────
    /// Bounded aggregation of new-session country pairs observed by the proxy.
    pub geo: std::sync::Mutex<GeoAggregator>,

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
            drops_egress_budget: AtomicU64::new(0),
            egress_budget_limit_bytes: AtomicU64::new(0),
            egress_budget_used_bytes: AtomicU64::new(0),
            egress_budget_soft_exceeded: AtomicU64::new(0),
            egress_budget_hard_exceeded: AtomicU64::new(0),
            egress_budget_soft_events: AtomicU64::new(0),
            sessions_created: AtomicU64::new(0),
            sessions_expired: AtomicU64::new(0),
            inbound_batches_total: AtomicU64::new(0),
            inbound_packets_received: AtomicU64::new(0),
            socket_opts_applied: AtomicU64::new(0),
            socket_opts_failed: AtomicU64::new(0),
            busy_poll_sockets: AtomicU64::new(0),
            telemetry: std::sync::Mutex::new(TelemetryAggregator::default()),
            route_telemetry: std::sync::Mutex::new(RouteTelemetryAggregator::default()),
            geo: std::sync::Mutex::new(GeoAggregator::default()),
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
            DropReason::EgressBudget => {
                self.drops_egress_budget.fetch_add(1, Ordering::Relaxed);
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
    /// `peer_ip` is used only to count distinct source addresses for the
    /// k-anonymity floor; it is never stored beyond that set and never
    /// exported. When the cell map is already at
    /// [`TelemetryAggregator::max_cells`] and this report would create a new
    /// key, it is counted in `rejected` instead.
    pub fn record_telemetry_report(
        &self,
        report: &lightspeed_protocol::TelemetryReport,
        peer_ip: IpAddr,
    ) {
        {
            let country = normalize_country(&report.client_country);
            let key = (report.game_id, country);
            let mut agg = self
                .telemetry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            if !agg.cells.contains_key(&key) && agg.cells.len() >= agg.max_cells {
                agg.rejected += 1;
            } else {
                let cell = agg.cells.entry(key).or_default();
                cell.source_ips.insert(peer_ip);
                cell.reports += 1;
                cell.samples += u64::from(report.sample_count);
                cell.p50_sum_ms += f64::from(report.p50_ms);
                cell.p95_sum_ms += f64::from(report.p95_ms);
                cell.p99_sum_ms += f64::from(report.p99_ms);
                cell.jitter_sum_ms += f64::from(report.jitter_ms);
                cell.fec_recoveries += u64::from(report.fec_recoveries);
                cell.fec_losses += u64::from(report.fec_losses);
                if let Some(direct) = report.direct_p50_ms {
                    cell.direct_sum_ms += f64::from(direct);
                    cell.direct_count += 1;
                }
                if let Some(relayed) = report.relayed_p50_ms {
                    cell.relayed_sum_ms += f64::from(relayed);
                    cell.relayed_count += 1;
                }
                if let Some(direct_app) = report.direct_app_p50_ms {
                    cell.direct_app_sum_ms += f64::from(direct_app);
                    cell.direct_app_count += 1;
                }
                if let (Some(direct), Some(relayed)) = (report.direct_p50_ms, report.relayed_p50_ms)
                {
                    cell.saved_sum_ms += f64::from(direct) - f64::from(relayed);
                    cell.saved_count += 1;
                }
                // `direct_app_p50_ms` is measured to pair with this relayed leg,
                // so their joint presence is the client's paired comparison. Its
                // medians summarise `saved_app_pairs` local observations, so the
                // sample counter, signed sum, and negative counter are all
                // weighted by that count; a legacy report without the field
                // defaults to one pair and contributes exactly as before.
                if let (Some(direct_app), Some(relayed)) =
                    (report.direct_app_p50_ms, report.relayed_p50_ms)
                {
                    let saved_app = f64::from(direct_app) - f64::from(relayed);
                    let pairs = u64::from(report.saved_app_pairs);
                    cell.saved_app_sum_ms += saved_app * pairs as f64;
                    cell.saved_app_count += pairs;
                    if saved_app < 0.0 {
                        cell.saved_app_negative_count += pairs;
                    }
                }
            }
        }
        self.record_route_legs(
            report.game_id,
            &report.client_country,
            &report.route_legs,
            peer_ip,
        );
    }

    /// Fold a report's per-relay observations into the bounded per-path
    /// aggregator, keyed by `(normalized relay, game_id, country)`.
    ///
    /// Legs whose relay id does not satisfy the protocol charset are skipped,
    /// and a novel key past the cell cap is counted in `rejected` rather than
    /// inserted.
    pub fn record_route_legs(
        &self,
        game_id: u8,
        raw_country: &str,
        legs: &[lightspeed_protocol::telemetry::PathObservation],
        peer_ip: IpAddr,
    ) {
        if legs.is_empty() {
            return;
        }
        let country = normalize_country(raw_country);
        let mut agg = self
            .route_telemetry
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        for leg in legs {
            let Some(relay) = normalize_relay(&leg.relay) else {
                continue;
            };
            let key = (relay, game_id, country.clone());
            if !agg.cells.contains_key(&key) && agg.cells.len() >= agg.max_cells {
                agg.rejected += 1;
                continue;
            }
            let cell = agg.cells.entry(key).or_default();
            cell.source_ips.insert(peer_ip);
            cell.reports += 1;
            cell.samples += u64::from(leg.samples);
            cell.rtt_p50_sum_ms += f64::from(leg.rtt_p50_ms);
            cell.rtt_p95_sum_ms += f64::from(leg.rtt_p95_ms);
            cell.rtt_p99_sum_ms += f64::from(leg.rtt_p99_ms);
            cell.jitter_sum_ms += f64::from(leg.jitter_ms);
            cell.lost += u64::from(leg.lost);
            cell.recovered += u64::from(leg.recovered);
            cell.dedup_saved += u64::from(leg.dedup_saved);
        }
    }

    /// Record one new session's `(src_country, dst_country)` pair.
    pub fn record_geo_session(&self, src: &str, dst: &str) {
        self.geo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_session(src, dst);
    }

    /// Record a country lookup miss on one side of a new session.
    pub fn record_geo_miss(&self, side: GeoSide) {
        self.geo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_miss(side);
    }

    /// Record a new session skipped because its destination is the relay.
    pub fn record_geo_self_tunnel_skip(&self) {
        self.geo
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .record_self_tunnel_skip();
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

    /// Build a collector whose per-path telemetry map is capped at `max_cells`.
    ///
    /// Test-only: production always uses [`MAX_TELEMETRY_CELLS`].
    #[cfg(test)]
    pub fn with_max_route_telemetry_cells(max_cells: usize) -> Self {
        let metrics = Self::new();
        metrics.route_telemetry.lock().unwrap().max_cells = max_cells;
        metrics
    }

    /// Build a collector whose geo session map is capped at `max_cells`.
    ///
    /// Test-only: production always uses [`MAX_TELEMETRY_CELLS`].
    #[cfg(test)]
    pub fn with_max_geo_cells(max_cells: usize) -> Self {
        let metrics = Self::new();
        metrics.geo.lock().unwrap().max_cells = max_cells;
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

    /// Record the outcome of one attempted socket option.
    pub fn record_socket_option(&self, applied: bool) {
        if applied {
            self.socket_opts_applied.fetch_add(1, Ordering::Relaxed);
        } else {
            self.socket_opts_failed.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Record a receive socket that enabled Linux `SO_BUSY_POLL`.
    pub fn record_busy_poll_enabled(&self) {
        self.busy_poll_sockets.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a new session created.
    pub fn record_session_created(&self) {
        self.sessions_created.fetch_add(1, Ordering::Relaxed);
    }

    /// Record a session expired/cleaned.
    pub fn record_session_expired(&self, count: u64) {
        self.sessions_expired.fetch_add(count, Ordering::Relaxed);
    }

    /// Publish the current egress budget state.
    ///
    /// `soft_first` is true only on the evaluation that first crosses the soft
    /// threshold, so the transition counter increments once per crossing.
    pub fn record_egress_budget(
        &self,
        limit_bytes: u64,
        used_bytes: u64,
        soft_exceeded: bool,
        hard_exceeded: bool,
        soft_first: bool,
    ) {
        self.egress_budget_limit_bytes
            .store(limit_bytes, Ordering::Relaxed);
        self.egress_budget_used_bytes
            .store(used_bytes, Ordering::Relaxed);
        self.egress_budget_soft_exceeded
            .store(u64::from(soft_exceeded), Ordering::Relaxed);
        self.egress_budget_hard_exceeded
            .store(u64::from(hard_exceeded), Ordering::Relaxed);
        if soft_first {
            self.egress_budget_soft_events
                .fetch_add(1, Ordering::Relaxed);
        }
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

        // Explicit egress alias of `lightspeed_bytes_relayed_total`: the relay's
        // cumulative outbound bytes in both data-plane directions (forwarded to
        // game servers and returned to clients). Monotonic for the process
        // lifetime. The egress budget guard is defined against this series.
        out.push_str(
            "# HELP lightspeed_egress_bytes_total Cumulative relay egress in bytes: client traffic forwarded to game servers plus game-server responses sent back to clients (both data-plane directions). Monotonic per process.\n",
        );
        out.push_str("# TYPE lightspeed_egress_bytes_total counter\n");
        out.push_str(&format!(
            "lightspeed_egress_bytes_total{{{}}} {}\n",
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

        out.push_str(
            "# HELP lightspeed_drops_egress_budget_total Packets dropped because a new session was refused at the egress budget hard threshold. Existing sessions are never interrupted.\n",
        );
        out.push_str("# TYPE lightspeed_drops_egress_budget_total counter\n");
        out.push_str(&format!(
            "lightspeed_drops_egress_budget_total{{{}}} {}\n",
            labels,
            self.drops_egress_budget.load(Ordering::Relaxed)
        ));

        // ── Egress budget guard ─────────────────────────────────
        out.push_str(
            "# HELP lightspeed_egress_budget_bytes Binding egress budget limit in bytes (0 when no budget is enforced)\n",
        );
        out.push_str("# TYPE lightspeed_egress_budget_bytes gauge\n");
        out.push_str(&format!(
            "lightspeed_egress_budget_bytes{{{}}} {}\n",
            labels,
            self.egress_budget_limit_bytes.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_egress_budget_used_bytes Egress counted against the binding budget limit\n",
        );
        out.push_str("# TYPE lightspeed_egress_budget_used_bytes gauge\n");
        out.push_str(&format!(
            "lightspeed_egress_budget_used_bytes{{{}}} {}\n",
            labels,
            self.egress_budget_used_bytes.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_egress_budget_soft_exceeded 1 when the egress budget soft threshold is reached\n",
        );
        out.push_str("# TYPE lightspeed_egress_budget_soft_exceeded gauge\n");
        out.push_str(&format!(
            "lightspeed_egress_budget_soft_exceeded{{{}}} {}\n",
            labels,
            self.egress_budget_soft_exceeded.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_egress_budget_hard_exceeded 1 when the egress budget hard threshold is reached (new sessions refused)\n",
        );
        out.push_str("# TYPE lightspeed_egress_budget_hard_exceeded gauge\n");
        out.push_str(&format!(
            "lightspeed_egress_budget_hard_exceeded{{{}}} {}\n",
            labels,
            self.egress_budget_hard_exceeded.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_egress_budget_soft_events_total Number of egress budget soft-threshold transitions observed\n",
        );
        out.push_str("# TYPE lightspeed_egress_budget_soft_events_total counter\n");
        out.push_str(&format!(
            "lightspeed_egress_budget_soft_events_total{{{}}} {}\n",
            labels,
            self.egress_budget_soft_events.load(Ordering::Relaxed)
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

        // ── Socket tuning ───────────────────────────────────────
        out.push_str(
            "# HELP lightspeed_socket_opts_applied_total Socket buffer/busy-poll options the kernel accepted\n",
        );
        out.push_str("# TYPE lightspeed_socket_opts_applied_total counter\n");
        out.push_str(&format!(
            "lightspeed_socket_opts_applied_total{{{}}} {}\n",
            labels,
            self.socket_opts_applied.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_socket_opts_failed_total Socket buffer/busy-poll options the kernel refused (skipped, non-fatal)\n",
        );
        out.push_str("# TYPE lightspeed_socket_opts_failed_total counter\n");
        out.push_str(&format!(
            "lightspeed_socket_opts_failed_total{{{}}} {}\n",
            labels,
            self.socket_opts_failed.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_busy_poll_sockets Receive sockets with Linux SO_BUSY_POLL enabled\n",
        );
        out.push_str("# TYPE lightspeed_busy_poll_sockets gauge\n");
        out.push_str(&format!(
            "lightspeed_busy_poll_sockets{{{}}} {}\n",
            labels,
            self.busy_poll_sockets.load(Ordering::Relaxed)
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
            "# HELP lightspeed_telemetry_direct_ms_sum Sum of client-reported direct (un-relayed) game-server ICMP RTT medians (ms); _sum/_count is the mean of client direct medians, not a population p50\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_direct_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_direct_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_relayed_ms_sum Sum of client-reported relayed tunnelled round-trip medians (ms); _sum/_count is the mean of client relayed medians, not a population p50\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_relayed_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_relayed_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_saved_ms_sum Sum of client-reported (direct - relayed) latency savings (ms); positive means the relay was faster\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_saved_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_saved_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_direct_app_ms_sum Sum of client-reported direct application RTT medians (ms), measured by timing a sample of the game's own packets on the un-relayed path; _sum/_count is the mean of client direct-app medians, not a population p50\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_direct_app_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_direct_app_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_saved_app_ms_sum Sum of client-reported (direct app - relayed) app-to-app latency savings (ms), each report weighted by the paired observations behind its medians; positive means the relay was faster\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_saved_app_ms_sum counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_saved_app_ms_count Paired direct-app/relayed observations behind the reported app-to-app savings, summed across reports\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_saved_app_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_saved_app_negative_count Paired direct-app/relayed observations where the relayed app RTT was worse than the direct app RTT (negative saving)\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_saved_app_negative_count counter\n");
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
                if cell.source_ips.len() < MIN_TELEMETRY_CELL_SOURCE_IPS {
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
                out.push_str(&format!(
                    "lightspeed_telemetry_direct_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.direct_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_direct_ms_count{{{}}} {}\n",
                    cell_labels, cell.direct_count
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_relayed_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.relayed_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_relayed_ms_count{{{}}} {}\n",
                    cell_labels, cell.relayed_count
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_saved_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.saved_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_saved_ms_count{{{}}} {}\n",
                    cell_labels, cell.saved_count
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_direct_app_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.direct_app_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_direct_app_ms_count{{{}}} {}\n",
                    cell_labels, cell.direct_app_count
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_saved_app_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.saved_app_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_saved_app_ms_count{{{}}} {}\n",
                    cell_labels, cell.saved_app_count
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_saved_app_negative_count{{{}}} {}\n",
                    cell_labels, cell.saved_app_negative_count
                ));
            }
        }

        // ── Per-route-leg client telemetry (aggregated, k-anonymized) ──
        // HELP/TYPE headers are emitted unconditionally so the family is
        // discoverable before the k-anonymity floor is reached.
        out.push_str(
            "# HELP lightspeed_telemetry_route_reports_total Anonymous opt-in client per-route-leg observations, aggregated by relay, game, and country\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_reports_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_samples_total Sum of client-reported RTT sample counts per route-leg cell\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_samples_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_rtt_p50_ms_sum Sum of client-reported per-leg p50 RTT values (ms); _sum/_count is the mean of client-reported leg medians, not a population p50\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_rtt_p50_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_route_rtt_p50_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_rtt_p95_ms_sum Sum of client-reported per-leg p95 RTT values (ms); _sum/_count is the mean of client-reported leg p95s, not a population p95\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_rtt_p95_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_route_rtt_p95_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_rtt_p99_ms_sum Sum of client-reported per-leg p99 RTT values (ms); _sum/_count is the mean of client-reported leg p99s, not a population p99\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_rtt_p99_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_route_rtt_p99_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_jitter_ms_sum Sum of client-reported per-leg jitter values (ms); _sum/_count is the mean of client-reported leg jitter values, not a population statistic\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_jitter_ms_sum counter\n");
        out.push_str("# TYPE lightspeed_telemetry_route_jitter_ms_count counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_lost_total Sum of client-reported packets lost per route leg\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_lost_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_recovered_total Sum of client-reported packets recovered by FEC per route leg\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_recovered_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_dedup_saved_total Sum of client-reported duplicates suppressed per route leg\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_dedup_saved_total counter\n");
        out.push_str(
            "# HELP lightspeed_telemetry_route_rejected_total Route-leg observations dropped because the per-(relay,game,country) cell cap was reached\n",
        );
        out.push_str("# TYPE lightspeed_telemetry_route_rejected_total counter\n");

        {
            let agg = self
                .route_telemetry
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            out.push_str(&format!(
                "lightspeed_telemetry_route_rejected_total{{{}}} {}\n",
                labels, agg.rejected
            ));
            for ((relay, game_id, country), cell) in &agg.cells {
                if cell.source_ips.len() < MIN_TELEMETRY_CELL_SOURCE_IPS {
                    continue;
                }
                let game = lightspeed_protocol::game_id::key_for_id(*game_id).unwrap_or("unknown");
                let cell_labels = format!(
                    "{},game=\"{}\",country=\"{}\",relay=\"{}\"",
                    labels, game, country, relay
                );
                out.push_str(&format!(
                    "lightspeed_telemetry_route_reports_total{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_samples_total{{{}}} {}\n",
                    cell_labels, cell.samples
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_rtt_p50_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.rtt_p50_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_rtt_p50_ms_count{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_rtt_p95_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.rtt_p95_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_rtt_p95_ms_count{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_rtt_p99_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.rtt_p99_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_rtt_p99_ms_count{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_jitter_ms_sum{{{}}} {:.1}\n",
                    cell_labels, cell.jitter_sum_ms
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_jitter_ms_count{{{}}} {}\n",
                    cell_labels, cell.reports
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_lost_total{{{}}} {}\n",
                    cell_labels, cell.lost
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_recovered_total{{{}}} {}\n",
                    cell_labels, cell.recovered
                ));
                out.push_str(&format!(
                    "lightspeed_telemetry_route_dedup_saved_total{{{}}} {}\n",
                    cell_labels, cell.dedup_saved
                ));
            }
        }

        // ── Proxy-observed session geo (aggregated, k-anonymized) ──
        // HELP/TYPE headers are emitted unconditionally so the family is
        // discoverable before the k-anonymity floor is reached.
        out.push_str(
            "# HELP lightspeed_geo_sessions_total New relay sessions aggregated by proxy-resolved source and destination country\n",
        );
        out.push_str("# TYPE lightspeed_geo_sessions_total counter\n");
        out.push_str(
            "# HELP lightspeed_geo_lookup_misses_total New relay sessions whose source or destination country could not be resolved\n",
        );
        out.push_str("# TYPE lightspeed_geo_lookup_misses_total counter\n");
        out.push_str(
            "# HELP lightspeed_geo_sessions_skipped_total New relay sessions excluded from geo aggregation by reason\n",
        );
        out.push_str("# TYPE lightspeed_geo_sessions_skipped_total counter\n");
        out.push_str(
            "# HELP lightspeed_geo_sessions_rejected_total Geo session pairs dropped because the per-(src,dst) cell cap was reached\n",
        );
        out.push_str("# TYPE lightspeed_geo_sessions_rejected_total counter\n");

        {
            let agg = self
                .geo
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            out.push_str(&format!(
                "lightspeed_geo_sessions_rejected_total{{{}}} {}\n",
                labels, agg.rejected
            ));
            out.push_str(&format!(
                "lightspeed_geo_lookup_misses_total{{{},side=\"src\"}} {}\n",
                labels, agg.misses_src
            ));
            out.push_str(&format!(
                "lightspeed_geo_lookup_misses_total{{{},side=\"dst\"}} {}\n",
                labels, agg.misses_dst
            ));
            out.push_str(&format!(
                "lightspeed_geo_sessions_skipped_total{{{},reason=\"self_tunnel\"}} {}\n",
                labels, agg.skipped_self_tunnel
            ));
            for ((src, dst), count) in &agg.cells {
                if *count < MIN_TELEMETRY_CELL_REPORTS {
                    continue;
                }
                out.push_str(&format!(
                    "lightspeed_geo_sessions_total{{{},src=\"{}\",dst=\"{}\"}} {}\n",
                    labels, src, dst, count
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
    use std::net::Ipv4Addr;

    /// A distinct source IP per `n`, so multi-report fixtures clear the
    /// distinct-source k-anonymity floor.
    fn ip(n: u8) -> IpAddr {
        IpAddr::V4(Ipv4Addr::new(10, 0, 0, n))
    }

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
        // 0.05ms - fits in 0.1ms bucket
        m.record_latency(50);
        // 2ms - fits in 5ms bucket
        m.record_latency(2000);
        // 75ms - fits in 100ms bucket
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
            DropReason::EgressBudget,
        ] {
            m.record_drop(reason);
        }

        let categories = m.drops_malformed.load(Ordering::Relaxed)
            + m.auth_rejections.load(Ordering::Relaxed)
            + m.abuse_blocks.load(Ordering::Relaxed)
            + m.rate_limit_hits.load(Ordering::Relaxed)
            + m.drops_fec_malformed.load(Ordering::Relaxed)
            + m.drops_session_setup.load(Ordering::Relaxed)
            + m.drops_relay_send_errors.load(Ordering::Relaxed)
            + m.drops_egress_budget.load(Ordering::Relaxed);

        assert_eq!(categories, m.packets_dropped.load(Ordering::Relaxed));
    }

    #[test]
    fn test_egress_series_is_explicit_and_monotonic() {
        let m = ProxyMetrics::new();
        m.record_relay(100);
        let first = m.to_prometheus("test", "test-node");
        assert!(first
            .contains("lightspeed_egress_bytes_total{region=\"test\",node_id=\"test-node\"} 100"));

        m.record_relay(250);
        let second = m.to_prometheus("test", "test-node");
        assert!(second
            .contains("lightspeed_egress_bytes_total{region=\"test\",node_id=\"test-node\"} 350"));
        assert!(second.contains("# TYPE lightspeed_egress_bytes_total counter"));
    }

    #[test]
    fn test_egress_budget_series_emitted() {
        let m = ProxyMetrics::new();
        m.record_egress_budget(1000, 800, true, false, true);
        m.record_drop(DropReason::EgressBudget);
        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains("lightspeed_egress_budget_bytes{") && output.contains("} 1000"));
        assert!(
            output.contains("lightspeed_egress_budget_used_bytes{") && output.contains("} 800")
        );
        assert!(output.contains("lightspeed_egress_budget_soft_exceeded{"));
        assert!(output.contains("lightspeed_egress_budget_soft_events_total{"));
        assert!(output.contains("lightspeed_drops_egress_budget_total{"));
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
            direct_p50_ms: None,
            direct_app_p50_ms: None,
            relayed_p50_ms: None,
            saved_app_pairs: 1,
            client_version: "1.4.4".to_string(),
            route_legs: vec![],
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

        m.record_telemetry_report(&report(2, "us "), ip(1));
        m.record_telemetry_report(&second, ip(2));
        m.record_telemetry_report(&third, ip(3));

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

    /// Given: three reports, one with both latencies, one direct-only, one
    /// relayed-only. When: they are folded into the telemetry cell. Then: the
    /// direct and relayed sums/counts each cover only the reports that carried
    /// that value, and saved sums only the reports where both were present.
    #[test]
    fn telemetry_direct_relayed_and_saved_aggregated() {
        let m = ProxyMetrics::new();

        let mut both = report(2, "US");
        both.direct_p50_ms = Some(50.0);
        both.relayed_p50_ms = Some(30.0);
        let mut direct_only = report(2, "US");
        direct_only.direct_p50_ms = Some(60.0);
        let mut relayed_only = report(2, "US");
        relayed_only.relayed_p50_ms = Some(50.0);

        m.record_telemetry_report(&both, ip(1));
        m.record_telemetry_report(&direct_only, ip(2));
        m.record_telemetry_report(&relayed_only, ip(3));

        let output = m.to_prometheus("test", "test-node");
        let labels = "region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"";

        assert!(output.contains(&format!(
            "lightspeed_telemetry_direct_ms_sum{{{labels}}} 110.0"
        )));
        assert!(output.contains(&format!(
            "lightspeed_telemetry_direct_ms_count{{{labels}}} 2"
        )));
        assert!(output.contains(&format!(
            "lightspeed_telemetry_relayed_ms_sum{{{labels}}} 80.0"
        )));
        assert!(output.contains(&format!(
            "lightspeed_telemetry_relayed_ms_count{{{labels}}} 2"
        )));
        assert!(output.contains(&format!(
            "lightspeed_telemetry_saved_ms_sum{{{labels}}} 20.0"
        )));
        assert!(output.contains(&format!(
            "lightspeed_telemetry_saved_ms_count{{{labels}}} 1"
        )));
    }

    /// Given: three reports, two carrying a direct application RTT. When: they
    /// are folded into the telemetry cell. Then: the direct-app sum/count cover
    /// only the reports that carried it, and the family is emitted.
    #[test]
    fn telemetry_direct_app_aggregated_and_emitted() {
        let m = ProxyMetrics::new();

        let mut with_app = report(2, "US");
        with_app.direct_app_p50_ms = Some(48.0);
        let mut second = report(2, "US");
        second.direct_app_p50_ms = Some(52.0);
        let without = report(2, "US");

        m.record_telemetry_report(&with_app, ip(1));
        m.record_telemetry_report(&second, ip(2));
        m.record_telemetry_report(&without, ip(3));

        {
            let agg = m.telemetry.lock().unwrap();
            let cell = agg.cells.values().next().unwrap();
            assert_eq!(cell.direct_app_sum_ms, 100.0);
            assert_eq!(cell.direct_app_count, 2);
        }

        let output = m.to_prometheus("test", "test-node");
        let labels = "region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"";
        assert!(output.contains(&format!(
            "lightspeed_telemetry_direct_app_ms_sum{{{labels}}} 100.0"
        )));
        assert!(output.contains(&format!(
            "lightspeed_telemetry_direct_app_ms_count{{{labels}}} 2"
        )));
    }

    /// Given: a cell below the k-anonymity floor. When: metrics are rendered.
    /// Then: the direct-app family is still declared, but no per-cell series
    /// leaks.
    #[test]
    fn telemetry_direct_app_family_declared_below_k() {
        let m = ProxyMetrics::new();
        m.record_telemetry_report(&report(2, "US"), ip(1));

        let output = m.to_prometheus("test", "test-node");
        for family in [
            "lightspeed_telemetry_direct_app_ms_sum",
            "lightspeed_telemetry_direct_app_ms_count",
        ] {
            assert!(
                output.contains(&format!("# TYPE {family} counter")),
                "missing TYPE for {family}"
            );
        }
        assert!(
            !output.contains("lightspeed_telemetry_direct_app_ms_sum{"),
            "a cell below the k-anonymity floor must not emit direct-app series"
        );
    }

    /// Given: no cell has reached the k-anonymity floor. When: metrics are
    /// rendered. Then: all five new families are still declared, but no
    /// per-cell series leak.
    #[test]
    fn telemetry_latency_families_declared_below_k() {
        let m = ProxyMetrics::new();
        m.record_telemetry_report(&report(2, "US"), ip(1));

        let output = m.to_prometheus("test", "test-node");
        for family in [
            "lightspeed_telemetry_direct_ms_sum",
            "lightspeed_telemetry_relayed_ms_sum",
            "lightspeed_telemetry_saved_ms_sum",
        ] {
            assert!(
                output.contains(&format!("# HELP {family}")),
                "missing HELP for {family}"
            );
            assert!(
                output.contains(&format!("# TYPE {family} counter")),
                "missing TYPE for {family}"
            );
        }
        for family in [
            "lightspeed_telemetry_direct_ms_count",
            "lightspeed_telemetry_relayed_ms_count",
            "lightspeed_telemetry_saved_ms_count",
        ] {
            assert!(
                output.contains(&format!("# TYPE {family} counter")),
                "missing TYPE for {family}"
            );
        }
        assert!(
            !output.contains("lightspeed_telemetry_direct_ms_sum{"),
            "a cell below the k-anonymity floor must not emit per-cell latency series"
        );
    }

    #[test]
    fn test_telemetry_normalization() {
        let m = ProxyMetrics::new();

        for i in 0..3u8 {
            m.record_telemetry_report(&report(2, " usa "), ip(i + 1));
        }
        for i in 0..3u8 {
            m.record_telemetry_report(&report(2, "th"), ip(i + 1));
        }
        for i in 0..3u8 {
            m.record_telemetry_report(&report(2, ""), ip(i + 1));
        }
        for i in 0..3u8 {
            m.record_telemetry_report(&report(250, "th"), ip(i + 1));
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

        m.record_telemetry_report(&report(1, "US"), ip(1));
        m.record_telemetry_report(&report(2, "US"), ip(2));
        // Third distinct cell is over the cap and must be rejected, not stored.
        m.record_telemetry_report(&report(3, "US"), ip(3));

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
        m.record_telemetry_report(&report(2, "US"), ip(1));

        let output = m.to_prometheus("test", "test-node");

        assert!(
            !output.contains("game=\"cs2\",country=\"US\""),
            "a cell below the k-anonymity floor must not be emitted"
        );
    }

    /// Given: three reports for one (game, country) cell, all from the same
    /// source IP. When: metrics are rendered. Then: the cell stays suppressed
    /// even though it holds three reports, because it has one distinct source.
    #[test]
    fn telemetry_same_source_three_reports_stays_suppressed() {
        let m = ProxyMetrics::new();
        for _ in 0..3 {
            m.record_telemetry_report(&report(2, "US"), ip(1));
        }

        let output = m.to_prometheus("test", "test-node");
        assert!(
            !output.contains("game=\"cs2\",country=\"US\""),
            "a cell backed by one distinct IP must stay suppressed"
        );

        let agg = m.telemetry.lock().unwrap();
        let cell = agg.cells.values().next().unwrap();
        assert_eq!(cell.reports, 3, "the reports must still be aggregated");
    }

    /// Given: three reports for one (game, country) cell from three distinct
    /// source IPs. When: metrics are rendered. Then: the cell clears the floor
    /// and is emitted with its report count.
    #[test]
    fn telemetry_three_distinct_sources_export() {
        let m = ProxyMetrics::new();
        for i in 0..3u8 {
            m.record_telemetry_report(&report(2, "US"), ip(i + 1));
        }

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains(
            "lightspeed_telemetry_reports_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"} 3"
        ));
    }

    #[test]
    fn route_cell_floors_on_distinct_sources() {
        let one = ProxyMetrics::new();
        for _ in 0..3 {
            one.record_route_legs(2, "US", &[fra_leg()], ip(1));
        }
        let output = one.to_prometheus("test", "test-node");
        assert!(
            !output.contains("lightspeed_telemetry_route_reports_total{region=\"test\""),
            "a route cell backed by one source must stay suppressed:\n{output}"
        );

        let three = ProxyMetrics::new();
        for i in 0..3u8 {
            three.record_route_legs(2, "US", &[fra_leg()], ip(i + 1));
        }
        let output = three.to_prometheus("test", "test-node");
        let relay = fra_leg().relay;
        assert!(
            output.contains(&format!(
                "lightspeed_telemetry_route_reports_total{{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\",relay=\"{relay}\"}} 3"
            )),
            "a route cell with three distinct sources must be exported:\n{output}"
        );
    }

    /// Given: reports where the paired direct-app RTT is better than, worse
    /// than, and equal to the relayed RTT, plus one unpaired report. When: the
    /// cell is folded and rendered. Then: the signed sum and pair count cover
    /// only the paired reports, and the negative count counts only the report
    /// where the relay was worse.
    #[test]
    fn telemetry_saved_app_signed_and_negative_count() {
        let m = ProxyMetrics::new();

        let mut better = report(2, "US");
        better.direct_app_p50_ms = Some(50.0);
        better.relayed_p50_ms = Some(30.0);

        let mut worse = report(2, "US");
        worse.direct_app_p50_ms = Some(20.0);
        worse.relayed_p50_ms = Some(55.0);

        let mut same = report(2, "US");
        same.direct_app_p50_ms = Some(40.0);
        same.relayed_p50_ms = Some(40.0);

        let mut direct_only = report(2, "US");
        direct_only.direct_app_p50_ms = Some(40.0);

        m.record_telemetry_report(&better, ip(1));
        m.record_telemetry_report(&worse, ip(2));
        m.record_telemetry_report(&same, ip(3));
        m.record_telemetry_report(&direct_only, ip(4));

        {
            let agg = m.telemetry.lock().unwrap();
            let cell = agg.cells.values().next().unwrap();
            assert_eq!(cell.saved_app_count, 3);
            assert_eq!(cell.saved_app_negative_count, 1);
            assert_eq!(cell.saved_app_sum_ms, -15.0);
        }

        let output = m.to_prometheus("test", "test-node");
        let labels = "region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"US\"";
        assert!(output.contains(&format!(
            "lightspeed_telemetry_saved_app_ms_sum{{{labels}}} -15.0"
        )));
        assert!(output.contains(&format!(
            "lightspeed_telemetry_saved_app_ms_count{{{labels}}} 3"
        )));
        assert!(output.contains(&format!(
            "lightspeed_telemetry_saved_app_negative_count{{{labels}}} 1"
        )));
    }

    /// Given: one report whose medians summarise eight paired observations and
    /// another whose medians summarise three. When: the cell is folded. Then:
    /// the saved-app sample counter advances by the reported pair count rather
    /// than once per report, and the signed sum and negative count are weighted
    /// to match.
    #[test]
    fn telemetry_saved_app_counter_advances_by_pairs() {
        let m = ProxyMetrics::new();

        let mut better = report(2, "US");
        better.direct_app_p50_ms = Some(50.0);
        better.relayed_p50_ms = Some(30.0);
        better.saved_app_pairs = 8;

        let mut worse = report(2, "US");
        worse.direct_app_p50_ms = Some(20.0);
        worse.relayed_p50_ms = Some(55.0);
        worse.saved_app_pairs = 3;

        m.record_telemetry_report(&better, ip(1));
        m.record_telemetry_report(&worse, ip(2));

        let agg = m.telemetry.lock().unwrap();
        let cell = agg.cells.values().next().unwrap();
        assert_eq!(
            cell.saved_app_count, 11,
            "a report with N pairs must advance the sample counter by N, not 1"
        );
        assert_eq!(
            cell.saved_app_sum_ms,
            8.0 * 20.0 + 3.0 * -35.0,
            "the signed saving must be weighted by the pair count"
        );
        assert_eq!(
            cell.saved_app_negative_count, 3,
            "the negative count must be weighted by the pair count"
        );
    }

    /// Given: a cell below the k-anonymity floor. When: metrics are rendered.
    /// Then: the saved-app families are still declared, but no per-cell series
    /// leaks.
    #[test]
    fn telemetry_saved_app_families_declared_below_k() {
        let m = ProxyMetrics::new();
        m.record_telemetry_report(&report(2, "US"), ip(1));

        let output = m.to_prometheus("test", "test-node");
        for family in [
            "lightspeed_telemetry_saved_app_ms_sum",
            "lightspeed_telemetry_saved_app_ms_count",
            "lightspeed_telemetry_saved_app_negative_count",
        ] {
            assert!(
                output.contains(&format!("# TYPE {family} counter")),
                "missing TYPE for {family}"
            );
        }
        assert!(
            !output.contains("lightspeed_telemetry_saved_app_ms_sum{"),
            "a cell below the k-anonymity floor must not emit saved-app series"
        );
    }

    /// Given: one cell that sees far more distinct source IPs than the floor.
    /// When: they are folded in. Then: the retained distinct-IP set stays
    /// capped, so client-controlled addresses cannot grow proxy memory.
    #[test]
    fn telemetry_source_ip_set_is_bounded() {
        let m = ProxyMetrics::new();
        for last in 0..=255u8 {
            m.record_telemetry_report(&report(2, "US"), IpAddr::V4(Ipv4Addr::new(10, 0, 1, last)));
        }

        let agg = m.telemetry.lock().unwrap();
        let cell = agg.cells.values().next().unwrap();
        assert_eq!(cell.reports, 256);
        assert_eq!(cell.source_ips.len(), MAX_TELEMETRY_CELL_SOURCE_IPS);
    }

    fn fra_leg() -> lightspeed_protocol::telemetry::PathObservation {
        lightspeed_protocol::telemetry::PathObservation {
            relay: "relay-fra".to_string(),
            rtt_p50_ms: 18.0,
            rtt_p95_ms: 25.0,
            rtt_p99_ms: 30.0,
            jitter_ms: 1.2,
            samples: 60,
            lost: 3,
            recovered: 2,
            dedup_saved: 1,
        }
    }

    fn ams_leg() -> lightspeed_protocol::telemetry::PathObservation {
        lightspeed_protocol::telemetry::PathObservation {
            relay: "relay-ams".to_string(),
            rtt_p50_ms: 22.0,
            rtt_p95_ms: 33.0,
            rtt_p99_ms: 41.0,
            jitter_ms: 2.0,
            samples: 60,
            lost: 5,
            recovered: 4,
            dedup_saved: 0,
        }
    }

    /// Given: three reports, each carrying the same two route legs.
    /// When: the reports are folded into the per-path aggregator.
    /// Then: each relay becomes one cell whose sums are 3x the per-leg values,
    /// and a novel relay past the cell cap is rejected instead of inserted.
    #[test]
    fn per_path_telemetry_aggregated_and_bounded() {
        let m = ProxyMetrics::new();

        for i in 0..3u8 {
            let mut r = report(2, "DE");
            r.route_legs = vec![fra_leg(), ams_leg()];
            m.record_telemetry_report(&r, ip(i + 1));
        }

        let output = m.to_prometheus("test", "test-node");

        // relay-fra: 3 observations, sums are 3x per-leg values.
        assert!(output.contains(
            "lightspeed_telemetry_route_reports_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 3"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_samples_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 180"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_rtt_p50_ms_sum{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 54.0"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_rtt_p50_ms_count{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 3"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_rtt_p95_ms_sum{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 75.0"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_rtt_p99_ms_sum{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 90.0"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_jitter_ms_sum{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 3.6"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_lost_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 9"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_recovered_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 6"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_dedup_saved_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-fra\"} 3"
        ));

        // relay-ams is a distinct cell, not folded into relay-fra.
        assert!(output.contains(
            "lightspeed_telemetry_route_reports_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-ams\"} 3"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_rtt_p99_ms_sum{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-ams\"} 123.0"
        ));
        assert!(output.contains(
            "lightspeed_telemetry_route_lost_total{region=\"test\",node_id=\"test-node\",game=\"cs2\",country=\"DE\",relay=\"relay-ams\"} 15"
        ));

        // Six leg observations over two distinct relay ids collapse to two cells.
        assert_eq!(m.route_telemetry.lock().unwrap().cells.len(), 2);

        // Bounded: past the cell cap, a novel relay is rejected, not inserted.
        let capped = ProxyMetrics::with_max_route_telemetry_cells(1);
        let mut first = report(2, "DE");
        first.route_legs = vec![fra_leg()];
        capped.record_telemetry_report(&first, ip(1));
        let mut second = report(2, "DE");
        second.route_legs = vec![ams_leg()];
        capped.record_telemetry_report(&second, ip(2));
        assert_eq!(capped.route_telemetry.lock().unwrap().cells.len(), 1);
        assert_eq!(capped.route_telemetry.lock().unwrap().rejected, 1);
    }

    /// Given: a single report carrying one route leg.
    /// When: the report is folded in.
    /// Then: the per-relay cell stays under the k-anonymity floor and is withheld,
    /// even though the route metric family itself is declared.
    #[test]
    fn per_path_cell_below_k_suppressed() {
        let m = ProxyMetrics::new();
        let mut r = report(2, "DE");
        r.route_legs = vec![fra_leg()];
        m.record_telemetry_report(&r, ip(1));

        let output = m.to_prometheus("test", "test-node");

        assert!(
            output.contains("lightspeed_telemetry_route_reports_total"),
            "the route metric family must be declared"
        );
        assert!(
            !output.contains("relay=\"relay-fra\""),
            "a per-relay cell below the k-anonymity floor must not be emitted"
        );
    }

    // ── Proxy-observed geo aggregation ──────────────────────────

    #[test]
    fn geo_cell_below_k_suppressed() {
        let m = ProxyMetrics::new();
        m.record_geo_session("US", "DE");

        let output = m.to_prometheus("test", "test-node");

        assert!(
            output.contains("# HELP lightspeed_geo_sessions_total"),
            "the geo metric family must be declared even before the floor"
        );
        assert!(
            !output.contains("lightspeed_geo_sessions_total{"),
            "a geo cell below the k-anonymity floor must not be emitted"
        );
    }

    #[test]
    fn geo_cell_cap_overflows_and_counts_rejected() {
        let m = ProxyMetrics::with_max_geo_cells(2);

        m.record_geo_session("US", "DE");
        m.record_geo_session("FR", "DE");
        // Third distinct pair is over the cap and must be rejected, not stored.
        m.record_geo_session("JP", "DE");

        assert_eq!(m.geo.lock().unwrap().cells.len(), 2);
        assert_eq!(m.geo.lock().unwrap().rejected, 1);

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains(
            "lightspeed_geo_sessions_rejected_total{region=\"test\",node_id=\"test-node\"} 1"
        ));
        assert!(!output.contains("src=\"JP\""));
    }

    #[test]
    fn geo_lookup_misses_by_side() {
        let m = ProxyMetrics::new();
        m.record_geo_miss(crate::geo::GeoSide::Src);
        m.record_geo_miss(crate::geo::GeoSide::Src);
        m.record_geo_miss(crate::geo::GeoSide::Dst);

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains(
            "lightspeed_geo_lookup_misses_total{region=\"test\",node_id=\"test-node\",side=\"src\"} 2"
        ));
        assert!(output.contains(
            "lightspeed_geo_lookup_misses_total{region=\"test\",node_id=\"test-node\",side=\"dst\"} 1"
        ));
    }

    #[test]
    fn geo_self_tunnel_skip_emitted() {
        let m = ProxyMetrics::new();
        m.record_geo_self_tunnel_skip();

        let output = m.to_prometheus("test", "test-node");
        assert!(output.contains(
            "lightspeed_geo_sessions_skipped_total{region=\"test\",node_id=\"test-node\",reason=\"self_tunnel\"} 1"
        ));
    }

    #[test]
    fn geo_help_type_always_emitted() {
        let m = ProxyMetrics::new();
        let output = m.to_prometheus("test", "test-node");

        for family in [
            "lightspeed_geo_sessions_total",
            "lightspeed_geo_lookup_misses_total",
            "lightspeed_geo_sessions_skipped_total",
            "lightspeed_geo_sessions_rejected_total",
        ] {
            assert!(
                output.contains(&format!("# HELP {family}")),
                "missing HELP for {family}"
            );
            assert!(
                output.contains(&format!("# TYPE {family} counter")),
                "missing TYPE for {family}"
            );
        }
    }
}
