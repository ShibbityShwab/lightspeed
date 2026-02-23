//! # Rate Limiting
//!
//! Per-client rate limiting to prevent abuse and ensure fair resource usage.
//! Critical for preventing the proxy from being used as a DDoS amplifier.

use std::collections::HashMap;
use std::net::SocketAddrV4;
use std::time::Instant;

use super::config::RateLimitConfig;

/// Per-client rate limit state.
struct ClientRateState {
    /// Packets in the current window.
    packet_count: u64,
    /// Bytes in the current window.
    byte_count: u64,
    /// When the current window started.
    window_start: Instant,
}

/// Rate limiter for proxy clients.
pub struct RateLimiter {
    /// Per-client state.
    clients: HashMap<SocketAddrV4, ClientRateState>,
    /// Rate limit configuration.
    config: RateLimitConfig,
    /// Window duration for rate counting.
    window_duration: std::time::Duration,
}

impl RateLimiter {
    /// Create a new rate limiter.
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            clients: HashMap::new(),
            config,
            window_duration: std::time::Duration::from_secs(1),
        }
    }

    /// Check if a packet from this client should be allowed.
    #[inline]
    pub fn check(&mut self, client: SocketAddrV4, packet_size: u64) -> RateLimitResult {
        let now = Instant::now();

        let state = self.clients.entry(client).or_insert(ClientRateState {
            packet_count: 0,
            byte_count: 0,
            window_start: now,
        });

        // Reset window if expired
        if now.duration_since(state.window_start) >= self.window_duration {
            state.packet_count = 0;
            state.byte_count = 0;
            state.window_start = now;
        }

        // Check limits
        if state.packet_count >= self.config.max_pps_per_client {
            return RateLimitResult::PacketRateExceeded;
        }
        if state.byte_count + packet_size > self.config.max_bps_per_client {
            return RateLimitResult::BandwidthExceeded;
        }

        // Allow and record
        state.packet_count += 1;
        state.byte_count += packet_size;

        RateLimitResult::Allowed
    }

    /// Clean up state for disconnected clients.
    pub fn cleanup(&mut self) {
        let timeout = std::time::Duration::from_secs(60);
        self.clients
            .retain(|_, state| state.window_start.elapsed() < timeout);
    }
}

/// Result of a rate limit check.
#[derive(Debug, PartialEq)]
pub enum RateLimitResult {
    /// Packet is allowed.
    Allowed,
    /// Packet rate (PPS) exceeded.
    PacketRateExceeded,
    /// Bandwidth (BPS) exceeded.
    BandwidthExceeded,
}
