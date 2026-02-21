//! # Proxy Discovery
//!
//! Discovers available proxy nodes via:
//! - Static configuration (config file)
//! - DNS-based discovery (SRV records)
//! - QUIC-based peer exchange (proxy shares known peers)

use std::net::SocketAddr;

use crate::error::QuicError;
use crate::route::ProxyNode;

/// Proxy discovery mechanisms.
pub enum DiscoveryMethod {
    /// Read proxy list from configuration file.
    Static,
    /// Discover via DNS SRV records.
    Dns,
    /// Ask a known proxy for its peers.
    PeerExchange,
}

/// Discovers proxy nodes using configured methods.
pub struct ProxyDiscovery {
    /// Discovery methods to try, in order.
    methods: Vec<DiscoveryMethod>,
    /// Statically configured proxy addresses.
    static_proxies: Vec<SocketAddr>,
}

impl ProxyDiscovery {
    /// Create a new discovery instance with static proxies.
    pub fn new(static_proxies: Vec<SocketAddr>) -> Self {
        Self {
            methods: vec![DiscoveryMethod::Static],
            static_proxies,
        }
    }

    /// Discover all available proxy nodes.
    pub async fn discover(&self) -> Result<Vec<ProxyNode>, QuicError> {
        // TODO (WF-001 Step 4): Implement discovery
        // For MVP, just return static proxies from config

        if self.static_proxies.is_empty() {
            return Err(QuicError::DiscoveryFailed(
                "No proxy servers configured".into(),
            ));
        }

        tracing::info!(
            "Discovered {} proxy nodes (static config)",
            self.static_proxies.len()
        );

        Ok(vec![]) // TODO: Convert static_proxies to ProxyNode
    }
}
