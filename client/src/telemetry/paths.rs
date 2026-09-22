//! Per-relay leg accumulation and PII-free relay labelling.

use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::{Mutex as StdMutex, OnceLock, PoisonError};

use lightspeed_protocol::telemetry::{PathObservation, MAX_ROUTE_LEGS};

use super::TelemetryCollector;

/// Maximum RTT samples retained per relay leg between flushes.
const LEG_RING_CAPACITY: usize = 256;

/// Hard cap on distinct relay legs tracked between flushes.
const MAX_TRACKED_LEGS: usize = MAX_ROUTE_LEGS;

/// Bounded per-relay accumulation drained on each flush.
#[derive(Default)]
pub(super) struct PathLeg {
    rtt_ms: Vec<f32>,
    lost: u32,
    recovered: u32,
    dedup_saved: u32,
}

/// Insertion-ordered, leg-count-bounded relay accumulator.
#[derive(Default)]
pub(super) struct PathInner {
    legs: Vec<(String, PathLeg)>,
}

impl PathInner {
    fn leg_mut(&mut self, relay: &str) -> Option<&mut PathLeg> {
        if let Some(i) = self.legs.iter().position(|(name, _)| name == relay) {
            return Some(&mut self.legs[i].1);
        }
        if self.legs.len() >= MAX_TRACKED_LEGS {
            return None;
        }
        self.legs.push((relay.to_string(), PathLeg::default()));
        self.legs.last_mut().map(|(_, leg)| leg)
    }

    pub(super) fn drain_observations(&mut self) -> Vec<PathObservation> {
        std::mem::take(&mut self.legs)
            .into_iter()
            .filter_map(|(relay, leg)| {
                if leg.rtt_ms.is_empty() {
                    return None;
                }
                let mut sorted = leg.rtt_ms.clone();
                sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
                let p50 = TelemetryCollector::percentile(&sorted, 50.0);
                let p95 = TelemetryCollector::percentile(&sorted, 95.0);
                let p99 = TelemetryCollector::percentile(&sorted, 99.0);
                Some(PathObservation {
                    relay,
                    rtt_p50_ms: round_one(p50),
                    rtt_p95_ms: round_one(p95),
                    rtt_p99_ms: round_one(p99),
                    jitter_ms: round_one(jitter_ms(&leg.rtt_ms)),
                    samples: sorted.len() as u32,
                    lost: leg.lost,
                    recovered: leg.recovered,
                    dedup_saved: leg.dedup_saved,
                })
            })
            .collect()
    }
}

impl TelemetryCollector {
    /// Record an RTT sample observed on one relay leg (milliseconds).
    pub fn record_path_rtt(&self, relay: &str, rtt_ms: f64) {
        if !rtt_ms.is_finite() || rtt_ms <= 0.0 {
            return;
        }
        let mut paths = self.paths.lock().unwrap_or_else(PoisonError::into_inner);
        let Some(leg) = paths.leg_mut(relay) else {
            return;
        };
        if leg.rtt_ms.len() >= LEG_RING_CAPACITY {
            leg.rtt_ms.remove(0);
        }
        leg.rtt_ms.push(rtt_ms as f32);
    }

    /// Record a lost packet observed on one relay leg.
    pub fn record_path_lost(&self, relay: &str) {
        self.bump_leg(relay, |leg| leg.lost = leg.lost.saturating_add(1));
    }

    /// Record a FEC-recovered packet on one relay leg.
    pub fn record_path_recovered(&self, relay: &str) {
        self.bump_leg(relay, |leg| leg.recovered = leg.recovered.saturating_add(1));
    }

    /// Record a duplicate response suppressed on one relay leg.
    pub fn record_path_dedup(&self, relay: &str) {
        self.bump_leg(relay, |leg| {
            leg.dedup_saved = leg.dedup_saved.saturating_add(1)
        });
    }

    fn bump_leg(&self, relay: &str, bump: impl FnOnce(&mut PathLeg)) {
        let mut paths = self.paths.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(leg) = paths.leg_mut(relay) {
            bump(leg);
        }
    }
}

/// Round to one decimal place, matching the global report's precision.
fn round_one(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}

/// Mean of consecutive absolute RTT deltas in arrival order.
fn jitter_ms(samples: &[f32]) -> f32 {
    if samples.len() < 2 {
        return 0.0;
    }
    let deltas: f32 = samples.windows(2).map(|w| (w[1] - w[0]).abs()).sum::<f32>();
    deltas / (samples.len() - 1) as f32
}

/// Process-wide collector, installed by [`super::install`].
static GLOBAL_COLLECTOR: OnceLock<TelemetryCollector> = OnceLock::new();

/// Install the active collector so transport call sites can feed per-leg
/// telemetry without threading the handle through every call. Idempotent: the
/// first call wins and later calls are ignored. Recording is additionally
/// gated by [`super::is_enabled`], so a disabled collector accumulates nothing.
pub fn install_global_collector(collector: &TelemetryCollector) {
    let _ = GLOBAL_COLLECTOR.set(collector.clone());
}

/// The installed collector, or `None` when telemetry is disabled.
pub fn global_collector() -> Option<&'static TelemetryCollector> {
    GLOBAL_COLLECTOR.get()
}

/// Relay address -> registry node id, populated from signed-registry discovery.
static RELAY_NODES: OnceLock<StdMutex<HashMap<SocketAddr, String>>> = OnceLock::new();

fn relay_nodes() -> &'static StdMutex<HashMap<SocketAddr, String>> {
    RELAY_NODES.get_or_init(|| StdMutex::new(HashMap::new()))
}

/// Associate a configured relay address with its registry node id so reports
/// carry `"relay-fra"` rather than any network location. Invalid ids are
/// ignored and fall back to an opaque digest label.
pub fn register_relay_node(addr: impl Into<SocketAddr>, node_id: &str) {
    if let Some(label) = sanitize_relay_label(node_id) {
        relay_nodes()
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .insert(addr.into(), label);
    }
}

/// A stable, PII-free label for a relay: its registry node id when known,
/// otherwise a digest of the address. The raw address is never returned.
pub fn relay_label(addr: impl Into<SocketAddr>) -> String {
    let addr = addr.into();
    if let Some(label) = relay_nodes()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(&addr)
    {
        return label.clone();
    }
    opaque_relay_label(addr)
}

/// Feed a per-leg response into telemetry when it is enabled. `latency_us == 0`
/// means no RTT was measured, so only the duplicate signal is recorded.
pub fn record_relay_response(addr: impl Into<SocketAddr>, latency_us: u64, duplicate: bool) {
    if !super::is_enabled() {
        return;
    }
    let Some(collector) = global_collector() else {
        return;
    };
    let label = relay_label(addr);
    if duplicate {
        collector.record_path_dedup(&label);
    }
    if latency_us > 0 {
        collector.record_path_rtt(&label, latency_us as f64 / 1000.0);
    }
}

/// Feed a per-leg FEC recovery into telemetry when it is enabled.
pub fn record_relay_recovery(addr: impl Into<SocketAddr>) {
    if !super::is_enabled() {
        return;
    }
    if let Some(collector) = global_collector() {
        collector.record_path_recovered(&relay_label(addr));
    }
}

fn sanitize_relay_label(label: &str) -> Option<String> {
    if label.is_empty() || label.len() > 64 {
        return None;
    }
    if !label
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'.' | b'_' | b'-'))
    {
        return None;
    }
    Some(label.to_string())
}

fn opaque_relay_label(addr: SocketAddr) -> String {
    let mut bytes = Vec::with_capacity(18);
    match addr {
        SocketAddr::V4(v4) => {
            bytes.extend_from_slice(&v4.ip().octets());
            bytes.extend_from_slice(&v4.port().to_be_bytes());
        }
        SocketAddr::V6(v6) => {
            bytes.extend_from_slice(&v6.ip().octets());
            bytes.extend_from_slice(&v6.port().to_be_bytes());
        }
    }
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        hash ^= u64::from(b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("relay-{hash:016x}")
}
