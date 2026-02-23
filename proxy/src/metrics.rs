//! # Proxy Metrics
//!
//! Collects and exports proxy performance metrics via Prometheus format.
//! Includes counters, gauges, and histogram-style latency buckets for
//! comprehensive observability.
//!
//! All metrics are designed for free-tier monitoring (no external services needed).

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

/// Latency histogram bucket boundaries (microseconds).
const LATENCY_BUCKETS_US: &[u64] = &[
    100,     // 0.1ms
    500,     // 0.5ms
    1_000,   // 1ms
    5_000,   // 5ms
    10_000,  // 10ms
    25_000,  // 25ms
    50_000,  // 50ms
    100_000, // 100ms
    250_000, // 250ms
    500_000, // 500ms
    1_000_000, // 1s
];

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

    // ── Session metrics ─────────────────────────────────────────
    /// Total sessions created (lifetime).
    pub sessions_created: AtomicU64,
    /// Total sessions expired (lifetime).
    pub sessions_expired: AtomicU64,

    // ── Latency histogram buckets ───────────────────────────────
    /// Counts per bucket for relay latency (cumulative).
    latency_buckets: [AtomicU64; 11],

    // ── Process start time ──────────────────────────────────────
    /// When the proxy started (for uptime gauge).
    pub start_time: Instant,
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
            fec_parity_received: AtomicU64::new(0),
            fec_recoveries: AtomicU64::new(0),
            fec_data_packets: AtomicU64::new(0),
            auth_rejections: AtomicU64::new(0),
            abuse_blocks: AtomicU64::new(0),
            rate_limit_hits: AtomicU64::new(0),
            sessions_created: AtomicU64::new(0),
            sessions_expired: AtomicU64::new(0),
            latency_buckets: Default::default(),
            start_time: Instant::now(),
        }
    }

    /// Record a relayed packet.
    pub fn record_relay(&self, bytes: u64) {
        self.packets_relayed.fetch_add(1, Ordering::Relaxed);
        self.bytes_relayed.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record a dropped packet.
    pub fn record_drop(&self) {
        self.packets_dropped.fetch_add(1, Ordering::Relaxed);
    }

    /// Record relay latency sample (with histogram).
    pub fn record_latency(&self, latency_us: u64) {
        self.relay_latency_sum_us
            .fetch_add(latency_us, Ordering::Relaxed);
        self.relay_latency_count.fetch_add(1, Ordering::Relaxed);

        // Update histogram buckets (cumulative)
        for (i, &bound) in LATENCY_BUCKETS_US.iter().enumerate() {
            if latency_us <= bound {
                self.latency_buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
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
        out.push_str(
            "# HELP lightspeed_active_connections Current active client connections\n",
        );
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
            "# HELP lightspeed_relay_latency_avg_us Average relay latency in microseconds\n",
        );
        out.push_str("# TYPE lightspeed_relay_latency_avg_us gauge\n");
        out.push_str(&format!(
            "lightspeed_relay_latency_avg_us{{{}}} {:.1}\n",
            labels,
            self.avg_latency_us()
        ));

        // Latency histogram
        out.push_str(
            "# HELP lightspeed_relay_latency_us Relay latency histogram in microseconds\n",
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

        // ── FEC metrics ─────────────────────────────────────────
        out.push_str("# HELP lightspeed_fec_parity_received_total FEC parity packets received\n");
        out.push_str("# TYPE lightspeed_fec_parity_received_total counter\n");
        out.push_str(&format!(
            "lightspeed_fec_parity_received_total{{{}}} {}\n",
            labels,
            self.fec_parity_received.load(Ordering::Relaxed)
        ));

        out.push_str(
            "# HELP lightspeed_fec_recoveries_total Packets recovered via FEC\n",
        );
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
        m.record_drop();
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

        let output = m.to_prometheus("us-west-lax", "proxy-lax");

        assert!(output.contains("lightspeed_packets_relayed_total"));
        assert!(output.contains("lightspeed_bytes_relayed_total"));
        assert!(output.contains("lightspeed_active_connections"));
        assert!(output.contains("lightspeed_uptime_seconds"));
        assert!(output.contains("lightspeed_fec_recoveries_total"));
        assert!(output.contains("lightspeed_auth_rejections_total"));
        assert!(output.contains("lightspeed_build_info"));
        assert!(output.contains("lightspeed_relay_latency_us_bucket"));
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
}
