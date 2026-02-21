//! # Multipath Routing
//!
//! Send packets simultaneously via multiple proxy nodes.
//! The fastest response wins; duplicates are dropped via sequence numbers.
//!
//! **Strategy**: Client sends same packet via 2-3 proxies. The proxy that
//! delivers the response first "wins" for that packet. Over time, the
//! system learns which paths are consistently fastest.
//!
//! **Bandwidth cost**: 2-3x upstream, but only 1x downstream (proxy dedup).

use super::ProxyNode;

/// Multipath configuration.
#[derive(Debug, Clone)]
pub struct MultipathConfig {
    /// Maximum number of simultaneous paths.
    pub max_paths: usize,
    /// Whether to use all paths or just the top N.
    pub use_all: bool,
    /// Minimum confidence before falling back to multipath.
    pub ml_confidence_threshold: f64,
}

impl Default for MultipathConfig {
    fn default() -> Self {
        Self {
            max_paths: 2,
            use_all: false,
            ml_confidence_threshold: 0.7,
        }
    }
}

/// Tracks per-path performance for multipath routing.
#[derive(Debug, Clone)]
pub struct PathStats {
    /// Proxy node this path goes through.
    pub proxy: ProxyNode,
    /// Number of times this path was the fastest.
    pub wins: u64,
    /// Total packets sent on this path.
    pub total: u64,
    /// Exponential moving average of latency.
    pub ema_latency_us: f64,
}

// TODO (WF-003): Implement multipath routing engine
// - PacketDuplicator: send same packet to multiple proxies
// - ResponseDeduplicator: accept first response, drop dupes (via sequence number)
// - PathScorer: track which paths consistently win
// - AdaptivePathSelector: dynamically add/remove paths based on performance
