//! # Route Selector Implementations
//!
//! Concrete route selection strategies.

use std::net::SocketAddrV4;

use crate::error::RouteError;
use super::{ProxyHealth, ProxyNode, RouteStrategy, SelectedRoute, RouteSelector};

/// Nearest-proxy selector — picks the proxy with lowest measured latency.
/// This is the default MVP strategy.
pub struct NearestSelector;

impl NearestSelector {
    pub fn new() -> Self {
        Self
    }
}

impl RouteSelector for NearestSelector {
    fn select(
        &self,
        _game_server: SocketAddrV4,
        available_proxies: &[ProxyNode],
    ) -> Result<SelectedRoute, RouteError> {
        // Filter to healthy proxies
        let healthy: Vec<&ProxyNode> = available_proxies
            .iter()
            .filter(|p| p.health == ProxyHealth::Healthy)
            .collect();

        if healthy.is_empty() {
            return Err(RouteError::AllUnhealthy);
        }

        // Sort by latency (lowest first), with unknown latency last
        let mut sorted = healthy.clone();
        sorted.sort_by_key(|p| p.latency_us.unwrap_or(u64::MAX));

        let primary = sorted[0].clone();
        let backups: Vec<ProxyNode> = sorted[1..].iter().map(|p| (*p).clone()).collect();

        Ok(SelectedRoute {
            primary,
            backups,
            confidence: 0.8,
            strategy: RouteStrategy::Nearest,
        })
    }

    fn feedback(&mut self, _proxy_id: &str, _observed_latency_us: u64) {
        // NearestSelector doesn't learn — ML selector does
    }

    fn strategy(&self) -> RouteStrategy {
        RouteStrategy::Nearest
    }
}
