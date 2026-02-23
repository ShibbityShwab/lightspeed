//! # Proxy Discovery
//!
//! Discovers available proxy nodes via:
//! - Static configuration (config file)
//! - DNS-based discovery (SRV records) — future
//! - QUIC-based peer exchange (proxy shares known peers) — future
//!
//! For MVP, only static discovery is implemented.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use tracing::{info, warn};

use crate::config::ProxyConfig;
use crate::error::QuicError;
use crate::quic::ControlClient;
use crate::route::ProxyNode;

/// Proxy discovery mechanisms.
#[derive(Debug, Clone, PartialEq)]
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
    /// Statically configured proxy addresses (host:port strings).
    static_proxies: Vec<String>,
    /// Default QUIC control port.
    quic_port: u16,
    /// Default data plane port.
    data_port: u16,
}

impl ProxyDiscovery {
    /// Create a new discovery instance from proxy config.
    pub fn from_config(config: &ProxyConfig) -> Self {
        Self {
            methods: vec![DiscoveryMethod::Static],
            static_proxies: config.servers.clone(),
            quic_port: config.quic_port,
            data_port: config.data_port,
        }
    }

    /// Create a new discovery instance with raw address list.
    pub fn new(static_proxies: Vec<String>, quic_port: u16, data_port: u16) -> Self {
        Self {
            methods: vec![DiscoveryMethod::Static],
            static_proxies,
            quic_port,
            data_port,
        }
    }

    /// Discover all available proxy nodes.
    ///
    /// Returns a list of `ProxyNode` structs with their control and data addresses.
    pub async fn discover(&self) -> Result<Vec<ProxyNode>, QuicError> {
        let mut nodes = Vec::new();

        for method in &self.methods {
            match method {
                DiscoveryMethod::Static => {
                    nodes.extend(self.discover_static()?);
                }
                DiscoveryMethod::Dns => {
                    // TODO: DNS SRV record discovery
                    warn!("DNS discovery not yet implemented");
                }
                DiscoveryMethod::PeerExchange => {
                    // TODO: Ask a connected proxy for peers
                    warn!("Peer exchange discovery not yet implemented");
                }
            }
        }

        if nodes.is_empty() {
            return Err(QuicError::DiscoveryFailed(
                "No proxy servers configured or discovered".into(),
            ));
        }

        info!("Discovered {} proxy node(s)", nodes.len());
        Ok(nodes)
    }

    /// Parse static proxy addresses from config into ProxyNode structs.
    fn discover_static(&self) -> Result<Vec<ProxyNode>, QuicError> {
        let mut nodes = Vec::new();

        for (i, addr_str) in self.static_proxies.iter().enumerate() {
            // Parse "host:port" or just "host" (use defaults)
            let (host, port) = if let Some(idx) = addr_str.rfind(':') {
                let h = &addr_str[..idx];
                let p: u16 = addr_str[idx + 1..].parse().unwrap_or(self.data_port);
                (h, p)
            } else {
                (addr_str.as_str(), self.data_port)
            };

            // Parse as IPv4 for now (DNS resolution would go here)
            let ip: Ipv4Addr = match host.parse() {
                Ok(ip) => ip,
                Err(_) => {
                    warn!("Cannot parse proxy address '{}' — skipping", addr_str);
                    continue;
                }
            };

            let data_addr = SocketAddrV4::new(ip, port);
            let control_addr = SocketAddrV4::new(ip, self.quic_port);

            nodes.push(ProxyNode {
                id: format!("proxy-{}", i + 1),
                data_addr,
                control_addr,
                region: "unknown".into(),
                health: crate::route::ProxyHealth::Unknown,
                latency_us: None,
                load: 0.0,
            });
        }

        Ok(nodes)
    }

    /// Probe a discovered proxy via QUIC to get its real identity.
    ///
    /// Updates the node's `id`, `region`, and `latency_us` fields.
    pub async fn probe_node(&self, node: &mut ProxyNode, game: u8) -> Result<(), QuicError> {
        let control_addr: SocketAddr = SocketAddr::V4(node.control_addr);

        let mut client = ControlClient::new()?;
        client.connect(control_addr, game).await?;

        // Update node info from registration response
        if let Some(nid) = client.node_id() {
            node.id = nid.to_string();
        }
        if let Some(region) = client.region() {
            node.region = region.to_string();
        }

        // Measure latency
        match client.ping().await {
            Ok(rtt_us) => {
                node.latency_us = Some(rtt_us);
                node.health = crate::route::ProxyHealth::Healthy;
            }
            Err(e) => {
                warn!("Probe ping failed for {}: {}", node.id, e);
                node.health = crate::route::ProxyHealth::Degraded;
            }
        }

        client.disconnect().await?;
        Ok(())
    }
}
