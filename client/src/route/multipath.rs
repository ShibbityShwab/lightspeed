//! # Multipath Routing
//!
//! Send each packet down multiple relay paths simultaneously; the first
//! response wins and later duplicates are dropped. Trades upstream bandwidth
//! (N×) for lower, more stable latency and resilience to a single bad path.

use std::collections::{HashMap, HashSet, VecDeque};
use std::net::SocketAddrV4;

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
#[derive(Debug, Clone, Default)]
pub struct PathStats {
    /// Number of times this path delivered the first (winning) response.
    pub wins: u64,
    /// Total responses received on this path.
    pub total: u64,
    /// Exponential moving average of latency (microseconds).
    pub ema_latency_us: f64,
}

/// The multipath engine: holds the active paths, dedupes responses by a key,
/// and scores which paths win.
#[derive(Debug)]
pub struct MultipathState {
    /// Active relay data-plane addresses, in preference order.
    paths: Vec<SocketAddrV4>,
    /// Dedup window: keys seen recently (responses already received).
    seen: HashSet<u16>,
    seen_order: VecDeque<u16>,
    /// Max dedup window size.
    max_seen: usize,
    /// Per-path win stats.
    stats: HashMap<SocketAddrV4, PathStats>,
}

impl MultipathState {
    pub fn new(max_seen: usize) -> Self {
        Self {
            paths: Vec::new(),
            seen: HashSet::with_capacity(max_seen),
            seen_order: VecDeque::with_capacity(max_seen),
            max_seen: max_seen.max(1),
            stats: HashMap::new(),
        }
    }

    /// Set the active paths (top-N relays). Fewer than 2 disables multipath.
    pub fn set_paths(&mut self, addrs: Vec<SocketAddrV4>) {
        self.paths = addrs;
        self.stats.retain(|a, _| self.paths.contains(a));
    }

    /// The active paths.
    pub fn paths(&self) -> &[SocketAddrV4] {
        &self.paths
    }

    /// Whether multipath is active (two or more paths).
    pub fn is_active(&self) -> bool {
        self.paths.len() >= 2
    }

    /// Record a received response key and report whether it is a duplicate.
    pub fn is_duplicate(&mut self, key: u16) -> bool {
        if self.seen.contains(&key) {
            return true;
        }
        self.seen.insert(key);
        self.seen_order.push_back(key);
        if self.seen_order.len() > self.max_seen {
            if let Some(old) = self.seen_order.pop_front() {
                self.seen.remove(&old);
            }
        }
        false
    }

    /// Record that `addr` delivered the winning response with `latency_us`.
    pub fn record_win(&mut self, addr: SocketAddrV4, latency_us: u64) {
        let s = self.stats.entry(addr).or_default();
        s.wins += 1;
        s.total += 1;
        let l = latency_us as f64;
        s.ema_latency_us = if s.total == 1 {
            l
        } else {
            0.2 * l + 0.8 * s.ema_latency_us
        };
    }

    /// Record that `addr` delivered a non-winning response.
    pub fn record_loss(&mut self, addr: SocketAddrV4) {
        self.stats.entry(addr).or_default().total += 1;
    }

    /// The path with the most wins (None when no responses seen yet).
    pub fn winning_path(&self) -> Option<SocketAddrV4> {
        self.stats
            .iter()
            .filter(|(a, _)| self.paths.contains(a))
            .max_by_key(|(_, s)| s.wins)
            .map(|(a, _)| *a)
    }

    /// Per-path stats snapshot.
    pub fn stats(&self) -> &HashMap<SocketAddrV4, PathStats> {
        &self.stats
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn addr(a: u8, b: u8, c: u8, d: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(a, b, c, d), 4434)
    }

    #[test]
    fn test_dedupe_window() {
        let mut m = MultipathState::new(4);
        assert!(!m.is_duplicate(10));
        assert!(!m.is_duplicate(11));
        assert!(m.is_duplicate(10)); // duplicate
                                     // Eviction: window of 4
        assert!(!m.is_duplicate(12));
        assert!(!m.is_duplicate(13));
        assert!(!m.is_duplicate(14)); // evicts 10
        assert!(!m.is_duplicate(10)); // 10 no longer seen
    }

    #[test]
    fn test_is_active() {
        let mut m = MultipathState::new(16);
        assert!(!m.is_active());
        m.set_paths(vec![addr(1, 1, 1, 1)]);
        assert!(!m.is_active());
        m.set_paths(vec![addr(1, 1, 1, 1), addr(2, 2, 2, 2)]);
        assert!(m.is_active());
    }

    #[test]
    fn test_winning_path() {
        let mut m = MultipathState::new(16);
        let a1 = addr(1, 1, 1, 1);
        let a2 = addr(2, 2, 2, 2);
        m.set_paths(vec![a1, a2]);
        assert_eq!(m.winning_path(), None);
        m.record_win(a1, 100);
        m.record_win(a2, 50);
        m.record_win(a1, 90);
        assert_eq!(m.winning_path(), Some(a1)); // a1 has 2 wins
        assert_eq!(m.stats().get(&a1).unwrap().wins, 2);
        assert_eq!(m.stats().get(&a2).unwrap().total, 1);
    }
}
