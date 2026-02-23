//! # Proxy Metrics
//!
//! Collects and exports proxy performance metrics via Prometheus format.
//! All metrics are designed for free-tier monitoring (no external services needed).

use std::sync::atomic::{AtomicU64, Ordering};

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

    /// Record relay latency sample.
    pub fn record_latency(&self, latency_us: u64) {
        self.relay_latency_sum_us
            .fetch_add(latency_us, Ordering::Relaxed);
        self.relay_latency_count.fetch_add(1, Ordering::Relaxed);
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
    pub fn to_prometheus(&self) -> String {
        format!(
            "# HELP lightspeed_packets_relayed Total packets relayed\n\
             # TYPE lightspeed_packets_relayed counter\n\
             lightspeed_packets_relayed {}\n\
             # HELP lightspeed_bytes_relayed Total bytes relayed\n\
             # TYPE lightspeed_bytes_relayed counter\n\
             lightspeed_bytes_relayed {}\n\
             # HELP lightspeed_active_connections Current active connections\n\
             # TYPE lightspeed_active_connections gauge\n\
             lightspeed_active_connections {}\n\
             # HELP lightspeed_packets_dropped Total packets dropped\n\
             # TYPE lightspeed_packets_dropped counter\n\
             lightspeed_packets_dropped {}\n\
             # HELP lightspeed_relay_latency_avg_us Average relay latency (microseconds)\n\
             # TYPE lightspeed_relay_latency_avg_us gauge\n\
             lightspeed_relay_latency_avg_us {:.1}\n",
            self.packets_relayed.load(Ordering::Relaxed),
            self.bytes_relayed.load(Ordering::Relaxed),
            self.active_connections.load(Ordering::Relaxed),
            self.packets_dropped.load(Ordering::Relaxed),
            self.avg_latency_us(),
        )
    }
}
