//! # Abuse Detection
//!
//! Detects and blocks abusive clients to prevent the proxy from being
//! used as attack infrastructure (DDoS amplification, reflection, etc.).
//!
//! ## Detection Methods
//! - **Amplification detection**: outbound >> inbound for a client
//! - **Reflection detection**: client sends to many unique destinations rapidly
//! - **Connection flooding**: too many new sessions per source IP
//! - **Banned IP tracking**: temporary bans with auto-expiry
//!
//! ## Destination Validation
//! - Blocks forwarding to private/internal IP ranges (RFC 1918, loopback, etc.)
//! - Prevents the proxy from being used to attack internal infrastructure

use std::collections::{HashMap, HashSet};
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Instant;

/// Abuse detector configuration.
#[derive(Debug, Clone)]
pub struct AbuseConfig {
    /// Maximum amplification ratio (outbound / inbound).
    pub max_amplification_ratio: f64,
    /// Maximum unique destinations per window per client.
    pub max_destinations_per_window: usize,
    /// Ban duration in seconds.
    pub ban_duration_secs: u64,
    /// Tracking window duration in seconds.
    pub window_secs: u64,
}

impl Default for AbuseConfig {
    fn default() -> Self {
        Self {
            max_amplification_ratio: 2.0,
            max_destinations_per_window: 10,
            ban_duration_secs: 3600, // 1 hour
            window_secs: 10,
        }
    }
}

/// Per-client abuse tracking state.
struct ClientTracker {
    /// Bytes received from client (inbound to proxy).
    inbound_bytes: u64,
    /// Bytes sent on behalf of client (outbound from proxy).
    outbound_bytes: u64,
    /// Unique destinations contacted this window.
    destinations: HashSet<SocketAddrV4>,
    /// Window start time.
    window_start: Instant,
}

impl ClientTracker {
    fn new() -> Self {
        Self {
            inbound_bytes: 0,
            outbound_bytes: 0,
            destinations: HashSet::new(),
            window_start: Instant::now(),
        }
    }

    /// Reset the window if it has expired.
    fn maybe_reset_window(&mut self, window_secs: u64) {
        if self.window_start.elapsed().as_secs() >= window_secs {
            self.inbound_bytes = 0;
            self.outbound_bytes = 0;
            self.destinations.clear();
            self.window_start = Instant::now();
        }
    }

    /// Calculate the amplification ratio.
    fn amplification_ratio(&self) -> f64 {
        if self.inbound_bytes == 0 {
            return 0.0;
        }
        self.outbound_bytes as f64 / self.inbound_bytes as f64
    }
}

/// Result of an abuse check.
#[derive(Debug, PartialEq)]
pub enum AbuseCheckResult {
    /// Traffic is allowed.
    Allowed,
    /// Client is banned.
    Banned,
    /// Amplification ratio exceeded — possible DDoS amplification.
    AmplificationDetected,
    /// Too many unique destinations — possible reflection attack.
    ReflectionDetected,
    /// Destination is a private/internal IP — blocked.
    PrivateDestination,
}

/// Abuse detector.
pub struct AbuseDetector {
    /// Per-client tracking.
    tracking: HashMap<Ipv4Addr, ClientTracker>,
    /// Banned clients with ban start time.
    banned: HashMap<Ipv4Addr, Instant>,
    /// Configuration.
    config: AbuseConfig,
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
    pub fn is_banned(&self, ip: &Ipv4Addr) -> bool {
        self.banned.get(ip).is_some_and(|banned_at| {
            banned_at.elapsed().as_secs() < self.config.ban_duration_secs
        })
    }

    /// Record inbound traffic from a client and check for abuse.
    pub fn record_inbound(
        &mut self,
        client_ip: Ipv4Addr,
        dest: SocketAddrV4,
        bytes: u64,
    ) -> AbuseCheckResult {
        // Check ban first
        if self.is_banned(&client_ip) {
            return AbuseCheckResult::Banned;
        }

        // Check destination is not a private IP
        if !is_public_ipv4(dest.ip()) {
            tracing::warn!(
                client = %client_ip,
                dest = %dest,
                "Blocked relay to private/internal IP"
            );
            return AbuseCheckResult::PrivateDestination;
        }

        // Get or create tracker
        let tracker = self
            .tracking
            .entry(client_ip)
            .or_insert_with(ClientTracker::new);

        tracker.maybe_reset_window(self.config.window_secs);
        tracker.inbound_bytes += bytes;
        tracker.destinations.insert(dest);

        // Check destination diversity (reflection detection)
        if tracker.destinations.len() > self.config.max_destinations_per_window {
            tracing::warn!(
                client = %client_ip,
                destinations = tracker.destinations.len(),
                "Reflection attack detected — too many unique destinations"
            );
            self.ban(client_ip);
            return AbuseCheckResult::ReflectionDetected;
        }

        AbuseCheckResult::Allowed
    }

    /// Record outbound traffic sent on behalf of a client.
    pub fn record_outbound(&mut self, client_ip: Ipv4Addr, bytes: u64) {
        if let Some(tracker) = self.tracking.get_mut(&client_ip) {
            tracker.outbound_bytes += bytes;

            // Check amplification ratio
            if tracker.amplification_ratio() > self.config.max_amplification_ratio
                && tracker.outbound_bytes > 10_000
            // Only trigger after meaningful traffic
            {
                tracing::warn!(
                    client = %client_ip,
                    ratio = tracker.amplification_ratio(),
                    inbound = tracker.inbound_bytes,
                    outbound = tracker.outbound_bytes,
                    "Amplification attack detected"
                );
                self.ban(client_ip);
            }
        }
    }

    /// Ban a client IP.
    fn ban(&mut self, ip: Ipv4Addr) {
        tracing::warn!(ip = %ip, duration_secs = self.config.ban_duration_secs, "Client banned");
        self.banned.insert(ip, Instant::now());
        // Remove tracking state
        self.tracking.remove(&ip);
    }

    /// Clean up expired bans and stale tracking data.
    pub fn cleanup(&mut self) {
        let ban_duration = self.config.ban_duration_secs;
        self.banned.retain(|_, banned_at| {
            banned_at.elapsed().as_secs() < ban_duration
        });

        let window_secs = self.config.window_secs * 3; // Keep trackers for 3 windows
        self.tracking.retain(|_, tracker| {
            tracker.window_start.elapsed().as_secs() < window_secs
        });
    }

    /// Number of currently banned IPs.
    pub fn banned_count(&self) -> usize {
        self.banned.len()
    }

    /// Number of tracked clients.
    pub fn tracked_count(&self) -> usize {
        self.tracking.len()
    }
}

// ── Destination Validation ──────────────────────────────────────────

/// Check if an IPv4 address is a public (routable) address.
///
/// Returns `false` for:
/// - RFC 1918 private ranges (10.0.0.0/8, 172.16.0.0/12, 192.168.0.0/16)
/// - Loopback (127.0.0.0/8)
/// - Link-local (169.254.0.0/16)
/// - Multicast (224.0.0.0/4)
/// - Broadcast (255.255.255.255)
/// - Unspecified (0.0.0.0)
/// - Documentation (192.0.2.0/24, 198.51.100.0/24, 203.0.113.0/24)
/// - Shared address space (100.64.0.0/10)
pub fn is_public_ipv4(ip: &Ipv4Addr) -> bool {
    let octets = ip.octets();

    // Unspecified
    if ip.is_unspecified() {
        return false;
    }

    // Loopback (127.0.0.0/8)
    if ip.is_loopback() {
        return false;
    }

    // Private: 10.0.0.0/8
    if octets[0] == 10 {
        return false;
    }

    // Private: 172.16.0.0/12
    if octets[0] == 172 && (octets[1] >= 16 && octets[1] <= 31) {
        return false;
    }

    // Private: 192.168.0.0/16
    if octets[0] == 192 && octets[1] == 168 {
        return false;
    }

    // Link-local: 169.254.0.0/16
    if octets[0] == 169 && octets[1] == 254 {
        return false;
    }

    // Shared address space: 100.64.0.0/10
    if octets[0] == 100 && (octets[1] >= 64 && octets[1] <= 127) {
        return false;
    }

    // Documentation: 192.0.2.0/24 (TEST-NET-1)
    if octets[0] == 192 && octets[1] == 0 && octets[2] == 2 {
        return false;
    }

    // Documentation: 198.51.100.0/24 (TEST-NET-2)
    if octets[0] == 198 && octets[1] == 51 && octets[2] == 100 {
        return false;
    }

    // Documentation: 203.0.113.0/24 (TEST-NET-3)
    if octets[0] == 203 && octets[1] == 0 && octets[2] == 113 {
        return false;
    }

    // Multicast (224.0.0.0/4)
    if ip.is_multicast() {
        return false;
    }

    // Broadcast
    if ip.is_broadcast() {
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_is_public_ipv4() {
        // Public IPs
        assert!(is_public_ipv4(&Ipv4Addr::new(1, 1, 1, 1)));
        assert!(is_public_ipv4(&Ipv4Addr::new(8, 8, 8, 8)));
        assert!(is_public_ipv4(&Ipv4Addr::new(104, 26, 1, 50)));

        // Private IPs
        assert!(!is_public_ipv4(&Ipv4Addr::new(10, 0, 0, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(172, 16, 0, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(172, 31, 255, 255)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(192, 168, 1, 1)));

        // Special IPs
        assert!(!is_public_ipv4(&Ipv4Addr::new(127, 0, 0, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(0, 0, 0, 0)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(169, 254, 1, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(255, 255, 255, 255)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(224, 0, 0, 1)));

        // Shared address space
        assert!(!is_public_ipv4(&Ipv4Addr::new(100, 64, 0, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(100, 127, 255, 255)));

        // Documentation ranges
        assert!(!is_public_ipv4(&Ipv4Addr::new(192, 0, 2, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(198, 51, 100, 1)));
        assert!(!is_public_ipv4(&Ipv4Addr::new(203, 0, 113, 1)));

        // Edge: 172.15.x is NOT private, 172.32.x is NOT private
        assert!(is_public_ipv4(&Ipv4Addr::new(172, 15, 0, 1)));
        assert!(is_public_ipv4(&Ipv4Addr::new(172, 32, 0, 1)));
    }

    #[test]
    fn test_abuse_detection_allowed() {
        let mut detector = AbuseDetector::new(AbuseConfig::default());
        let ip = Ipv4Addr::new(1, 2, 3, 4);
        let dest = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 50), 7777);

        let result = detector.record_inbound(ip, dest, 100);
        assert_eq!(result, AbuseCheckResult::Allowed);
    }

    #[test]
    fn test_abuse_detection_private_destination() {
        let mut detector = AbuseDetector::new(AbuseConfig::default());
        let ip = Ipv4Addr::new(1, 2, 3, 4);
        let private_dest = SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 1), 22);

        let result = detector.record_inbound(ip, private_dest, 100);
        assert_eq!(result, AbuseCheckResult::PrivateDestination);
    }

    #[test]
    fn test_abuse_detection_reflection() {
        let config = AbuseConfig {
            max_destinations_per_window: 3,
            ..Default::default()
        };
        let mut detector = AbuseDetector::new(config);
        let ip = Ipv4Addr::new(1, 2, 3, 4);

        // First 3 destinations are fine
        for i in 1..=3 {
            let dest = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, i), 7777);
            assert_eq!(
                detector.record_inbound(ip, dest, 100),
                AbuseCheckResult::Allowed
            );
        }

        // 4th triggers reflection detection
        let dest4 = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 100), 7777);
        assert_eq!(
            detector.record_inbound(ip, dest4, 100),
            AbuseCheckResult::ReflectionDetected
        );

        // Subsequent packets should show banned
        let dest5 = SocketAddrV4::new(Ipv4Addr::new(104, 26, 1, 200), 7777);
        assert_eq!(
            detector.record_inbound(ip, dest5, 100),
            AbuseCheckResult::Banned
        );
    }

    #[test]
    fn test_abuse_ban_and_cleanup() {
        let config = AbuseConfig {
            ban_duration_secs: 0, // Instant expiry for test
            ..Default::default()
        };
        let mut detector = AbuseDetector::new(config);
        let ip = Ipv4Addr::new(1, 2, 3, 4);

        // Force a ban
        detector.banned.insert(ip, Instant::now());
        // Ban should already be expired (0 sec duration)
        assert!(!detector.is_banned(&ip));

        detector.cleanup();
        assert_eq!(detector.banned_count(), 0);
    }
}
