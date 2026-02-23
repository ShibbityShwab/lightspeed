//! # Route Management
//!
//! Manages proxy selection, health monitoring, and failover.
//! Routes can be selected via:
//! - **Nearest**: Simple geographic/latency-based selection
//! - **ML**: AI-powered route prediction using linfa
//! - **Multipath**: Send on multiple paths, use fastest arrival

pub mod failover;
pub mod multipath;
pub mod selector;

use std::net::SocketAddrV4;

use crate::error::RouteError;

/// Information about a proxy node.
#[derive(Debug, Clone)]
pub struct ProxyNode {
    /// Unique identifier for this node.
    pub id: String,
    /// Address of the proxy's data plane (UDP).
    pub data_addr: SocketAddrV4,
    /// Address of the proxy's control plane (QUIC).
    pub control_addr: SocketAddrV4,
    /// Geographic region (e.g., "us-east", "eu-west", "sea").
    pub region: String,
    /// Current health status.
    pub health: ProxyHealth,
    /// Latest measured latency in microseconds.
    pub latency_us: Option<u64>,
    /// Current load (0.0 - 1.0).
    pub load: f64,
}

/// Health status of a proxy node.
#[derive(Debug, Clone, PartialEq)]
pub enum ProxyHealth {
    /// Node is healthy and accepting connections.
    Healthy,
    /// Node is degraded (high latency or partial failures).
    Degraded,
    /// Node is unreachable or down.
    Unhealthy,
    /// Health status unknown (not yet checked).
    Unknown,
}

/// A selected route — the result of route selection.
#[derive(Debug, Clone)]
pub struct SelectedRoute {
    /// Primary proxy to use.
    pub primary: ProxyNode,
    /// Backup proxies for failover (ordered by preference).
    pub backups: Vec<ProxyNode>,
    /// Confidence score (0.0 - 1.0) — how confident the selector is.
    pub confidence: f64,
    /// Which strategy selected this route.
    pub strategy: RouteStrategy,
}

/// Route selection strategy.
#[derive(Debug, Clone, PartialEq)]
pub enum RouteStrategy {
    /// Nearest healthy proxy by latency.
    Nearest,
    /// ML-predicted optimal route.
    MlPredicted,
    /// Multipath — use multiple proxies simultaneously.
    Multipath,
    /// Failover — using backup after primary failed.
    Failover,
    /// Direct — no proxy (bypass mode).
    Direct,
}

/// Trait for route selection — choose the best proxy for a given target.
pub trait RouteSelector: Send + Sync {
    /// Select the best route to the given game server.
    fn select(
        &self,
        game_server: SocketAddrV4,
        available_proxies: &[ProxyNode],
    ) -> Result<SelectedRoute, RouteError>;

    /// Update the selector with observed latency feedback.
    fn feedback(&mut self, proxy_id: &str, observed_latency_us: u64);

    /// Get the current strategy name.
    fn strategy(&self) -> RouteStrategy;
}
