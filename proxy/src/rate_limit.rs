//! # Rate Limiting
//!
//! Two-tier per-client rate limiting to prevent abuse and ensure fair resource
//! usage. Critical for preventing the proxy from being used as a DDoS amplifier.
//!
//! The per-flow tier keys on `SocketAddrV4` (client IP *and* source port), so a
//! client rotating or spoofing source ports gets a fresh bucket per port. The
//! per-IP aggregate tier keys on `Ipv4Addr` and bounds the total traffic from
//! one IP regardless of source port.

use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::{Duration, Instant};

use super::config::RateLimitConfig;

/// Maximum number of distinct source IPs tracked by the aggregate tier.
///
/// When the table is full, packets from a previously-unseen IP are rejected
/// (fail closed) instead of growing the map without bound.
pub const MAX_TRACKED_IPS: usize = 65_536;

/// Per-entry fixed-window rate limit state.
struct ClientRateState {
    /// Packets in the current window.
    packet_count: u64,
    /// Bytes in the current window.
    byte_count: u64,
    /// When the current window started.
    window_start: Instant,
}

impl ClientRateState {
    fn new(now: Instant) -> Self {
        Self {
            packet_count: 0,
            byte_count: 0,
            window_start: now,
        }
    }
}

/// Reset an entry's counters when its fixed window has elapsed.
fn reset_if_expired(state: &mut ClientRateState, now: Instant, window_duration: Duration) {
    if now.saturating_duration_since(state.window_start) >= window_duration {
        state.packet_count = 0;
        state.byte_count = 0;
        state.window_start = now;
    }
}

/// Rate limiter for proxy clients.
pub struct RateLimiter {
    /// Per-flow state, keyed by client IP and source port.
    clients: HashMap<SocketAddrV4, ClientRateState>,
    /// Per-IP aggregate state, keyed by client IP.
    ips: HashMap<Ipv4Addr, ClientRateState>,
    /// Rate limit configuration.
    config: RateLimitConfig,
    /// Window duration for rate counting.
    window_duration: Duration,
    /// Maximum number of distinct IPs tracked by the aggregate tier.
    max_tracked_ips: usize,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            clients: HashMap::new(),
            ips: HashMap::new(),
            config,
            window_duration: Duration::from_secs(1),
            max_tracked_ips: MAX_TRACKED_IPS,
        }
    }

    /// Check if a packet from this client should be allowed at the current time.
    #[inline]
    pub fn check(&mut self, client: SocketAddrV4, packet_size: u64) -> RateLimitResult {
        self.check_at(client, packet_size, Instant::now())
    }

    /// Check if a packet from this client should be allowed at `now`.
    ///
    /// The per-flow tier is evaluated first, then the per-IP aggregate tier.
    /// Neither window is debited unless both tiers would allow the packet, so a
    /// packet rejected by one tier never consumes the other tier's budget.
    pub fn check_at(
        &mut self,
        client: SocketAddrV4,
        packet_size: u64,
        now: Instant,
    ) -> RateLimitResult {
        // Tier 1: per-flow (client IP + source port).
        let flow = self
            .clients
            .entry(client)
            .or_insert_with(|| ClientRateState::new(now));
        reset_if_expired(flow, now, self.window_duration);

        if flow.packet_count >= self.config.max_pps_per_client {
            return RateLimitResult::PacketRateExceeded;
        }
        if flow.byte_count + packet_size > self.config.max_bps_per_client {
            return RateLimitResult::BandwidthExceeded;
        }

        // Tier 2: per-IP aggregate across all source ports.
        let ip = *client.ip();
        if !self.ips.contains_key(&ip) && self.ips.len() >= self.max_tracked_ips {
            return RateLimitResult::IpTableFull;
        }
        let ip_state = self
            .ips
            .entry(ip)
            .or_insert_with(|| ClientRateState::new(now));
        reset_if_expired(ip_state, now, self.window_duration);

        if ip_state.packet_count >= self.config.max_pps_per_ip {
            return RateLimitResult::IpPacketRateExceeded;
        }
        if ip_state.byte_count + packet_size > self.config.max_bps_per_ip {
            return RateLimitResult::IpBandwidthExceeded;
        }

        // Both tiers allow: debit both in one place so a packet rejected by
        // either tier never consumes the other tier's budget.
        flow.packet_count += 1;
        flow.byte_count += packet_size;
        ip_state.packet_count += 1;
        ip_state.byte_count += packet_size;

        RateLimitResult::Allowed
    }

    /// Clean up state for disconnected clients.
    pub fn cleanup(&mut self) {
        let timeout = Duration::from_secs(60);
        let now = Instant::now();
        self.clients
            .retain(|_, state| now.saturating_duration_since(state.window_start) < timeout);
        self.ips
            .retain(|_, state| now.saturating_duration_since(state.window_start) < timeout);
    }
}

/// Result of a rate limit check.
#[derive(Debug, PartialEq)]
pub enum RateLimitResult {
    /// Packet is allowed.
    Allowed,
    /// Per-flow packet rate (PPS) exceeded.
    PacketRateExceeded,
    /// Per-flow bandwidth (BPS) exceeded.
    BandwidthExceeded,
    /// Per-IP aggregate packet rate (PPS) exceeded.
    IpPacketRateExceeded,
    /// Per-IP aggregate bandwidth (BPS) exceeded.
    IpBandwidthExceeded,
    /// Per-IP tracking table is full and the source IP is new (fail closed).
    IpTableFull,
}

#[cfg(test)]
impl RateLimiter {
    /// Test-only: create a limiter whose per-IP table holds `max` entries.
    fn with_max_tracked_ips(config: RateLimitConfig, max: usize) -> Self {
        Self {
            max_tracked_ips: max,
            ..Self::new(config)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn default_config() -> RateLimitConfig {
        RateLimitConfig::default()
    }

    fn client(port: u16) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::LOCALHOST, port)
    }

    fn distinct_ip(n: u8) -> SocketAddrV4 {
        SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, n), 4000)
    }

    #[test]
    fn test_ip_tier_blocks_after_default() {
        let mut limiter = RateLimiter::new(RateLimitConfig {
            max_pps_per_client: 1_000_000,
            max_bps_per_client: 1_000_000_000,
            ..default_config()
        });
        let now = Instant::now();

        let mut allowed = 0u64;
        let mut first_rejection = None;
        // One more packet than the per-IP cap, each from a distinct source port
        // on the same IP. The per-flow tier is high enough that only the
        // aggregate tier can reject.
        for port in 1..=(default_config().max_pps_per_ip as u16 + 1) {
            match limiter.check_at(client(port), 64, now) {
                RateLimitResult::Allowed => allowed += 1,
                other => {
                    first_rejection = Some(other);
                    break;
                }
            }
        }

        assert_eq!(allowed, default_config().max_pps_per_ip);
        assert_eq!(first_rejection, Some(RateLimitResult::IpPacketRateExceeded));
    }

    #[test]
    fn test_both_tiers_must_allow() {
        let mut limiter = RateLimiter::new(RateLimitConfig {
            max_pps_per_client: 1,
            max_bps_per_client: 1_000_000,
            max_pps_per_ip: 10,
            max_bps_per_ip: 1_000_000,
        });
        let now = Instant::now();

        assert_eq!(
            limiter.check_at(client(1), 64, now),
            RateLimitResult::Allowed
        );
        // The second packet from the same flow is rejected by the flow tier.
        assert_eq!(
            limiter.check_at(client(1), 64, now),
            RateLimitResult::PacketRateExceeded
        );

        // The rejected packet must not have debited the per-IP budget: only 1
        // of the IP's 10 slots is used, so exactly 9 more distinct flows pass
        // before the aggregate tier rejects.
        let mut allowed = 0;
        for port in 2..40u16 {
            match limiter.check_at(client(port), 64, now) {
                RateLimitResult::Allowed => allowed += 1,
                _ => break,
            }
        }
        assert_eq!(
            allowed, 9,
            "a flow-tier rejection must not consume per-IP budget"
        );
    }

    #[test]
    fn test_ip_table_fails_closed_when_full() {
        let mut limiter = RateLimiter::with_max_tracked_ips(default_config(), 2);
        let now = Instant::now();

        assert_eq!(
            limiter.check_at(distinct_ip(1), 64, now),
            RateLimitResult::Allowed
        );
        assert_eq!(
            limiter.check_at(distinct_ip(2), 64, now),
            RateLimitResult::Allowed
        );
        assert_eq!(
            limiter.check_at(distinct_ip(3), 64, now),
            RateLimitResult::IpTableFull
        );
        assert_eq!(
            limiter.ips.len(),
            2,
            "a rejected new IP must not be inserted"
        );
    }

    #[test]
    fn test_window_resets_with_check_at() {
        let mut limiter = RateLimiter::new(RateLimitConfig {
            max_pps_per_client: 1,
            max_bps_per_client: 1_000_000,
            max_pps_per_ip: 1,
            max_bps_per_ip: 1_000_000,
        });
        let t0 = Instant::now();

        assert_eq!(
            limiter.check_at(client(1), 64, t0),
            RateLimitResult::Allowed
        );
        assert_eq!(
            limiter.check_at(client(1), 64, t0),
            RateLimitResult::PacketRateExceeded
        );

        let t1 = t0 + Duration::from_millis(1_001);
        assert_eq!(
            limiter.check_at(client(1), 64, t1),
            RateLimitResult::Allowed
        );
    }
}
