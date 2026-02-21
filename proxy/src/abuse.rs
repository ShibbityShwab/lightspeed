//! # Abuse Detection
//!
//! Detects and blocks abusive clients to prevent the proxy from being
//! used as attack infrastructure.
//!
//! ## Detection Methods
//! - Amplification detection: outbound >> inbound for a client
//! - Reflection detection: client sends to many different destinations rapidly
//! - Pattern matching: known attack signatures

use std::collections::HashMap;
use std::net::SocketAddrV4;

/// Abuse detector configuration.
#[derive(Debug, Clone)]
pub struct AbuseConfig {
    /// Maximum amplification ratio (outbound / inbound).
    pub max_amplification_ratio: f64,
    /// Maximum unique destinations per second per client.
    pub max_destinations_per_sec: usize,
    /// Ban duration in seconds.
    pub ban_duration_secs: u64,
}

impl Default for AbuseConfig {
    fn default() -> Self {
        Self {
            max_amplification_ratio: 2.0,
            max_destinations_per_sec: 10,
            ban_duration_secs: 3600, // 1 hour
        }
    }
}

/// Abuse detector.
pub struct AbuseDetector {
    /// Per-client tracking.
    tracking: HashMap<SocketAddrV4, ClientTracker>,
    /// Banned clients.
    banned: HashMap<SocketAddrV4, std::time::Instant>,
    /// Configuration.
    config: AbuseConfig,
}

struct ClientTracker {
    /// Bytes received from client.
    inbound_bytes: u64,
    /// Bytes sent on behalf of client.
    outbound_bytes: u64,
    /// Unique destinations this second.
    destinations: std::collections::HashSet<SocketAddrV4>,
    /// Window start.
    window_start: std::time::Instant,
}

impl AbuseDetector {
    /// Create a new abuse detector.
    pub fn new(config: AbuseConfig) -> Self {
        Self {
            tracking: HashMap::new(),
            banned: HashMap::new(),
            config,
        }
    }

    /// Check if a client is currently banned.
    pub fn is_banned(&self, client: &SocketAddrV4) -> bool {
        self.banned.get(client).is_some_and(|banned_at| {
            banned_at.elapsed().as_secs() < self.config.ban_duration_secs
        })
    }

    /// Clean up expired bans and old tracking data.
    pub fn cleanup(&mut self) {
        let ban_duration = self.config.ban_duration_secs;
        self.banned.retain(|_, banned_at| {
            banned_at.elapsed().as_secs() < ban_duration
        });
    }
}
