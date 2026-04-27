//! Proxy probing and route selection helpers.
//!
//! Provides async functions to measure latency to one or more proxy nodes and
//! select the best one via the configured `RouteSelector` strategy.

use std::net::SocketAddrV4;
use std::time::Duration;

use tracing::warn;

use crate::cli::parse_proxy_addr;
use crate::route::selector::NearestSelector;
use crate::route::{ProxyHealth, ProxyNode, RouteSelector, SelectedRoute};

/// Probe a single proxy by sending keepalive packets and measuring RTT.
///
/// Sends `num_pings` keepalive packets and returns the **median** RTT in
/// microseconds.  Returns `None` if the proxy does not respond within the
/// timeout.
pub async fn probe_single_proxy(
    addr: SocketAddrV4,
    num_pings: usize,
    timeout_ms: u64,
) -> Option<u64> {
    use lightspeed_protocol::TunnelHeader;
    use tokio::net::UdpSocket;

    let socket = UdpSocket::bind("0.0.0.0:0").await.ok()?;
    let mut rtts = Vec::with_capacity(num_pings);

    for seq in 0..num_pings as u16 {
        let send_time = std::time::Instant::now();
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_micros() as u32;

        let header = TunnelHeader::keepalive(seq, ts);
        let packet = header.encode_to_array();

        if socket.send_to(&packet, addr).await.is_err() {
            continue;
        }

        let mut buf = vec![0u8; 128];
        if let Ok(Ok((len, _))) = tokio::time::timeout(
            Duration::from_millis(timeout_ms),
            socket.recv_from(&mut buf),
        )
        .await
        {
            if TunnelHeader::decode(&buf[..len]).is_ok() {
                let rtt = send_time.elapsed().as_micros() as u64;
                rtts.push(rtt);
            }
        }

        // Small delay between pings to avoid burst flooding
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    if rtts.is_empty() {
        return None;
    }

    // Return median RTT
    rtts.sort();
    Some(rtts[rtts.len() / 2])
}

/// Probe all configured proxy servers concurrently and return a `ProxyNode`
/// list with measured latencies.
pub async fn probe_all_proxies(servers: &[String], data_port: u16) -> Vec<ProxyNode> {
    let mut handles = Vec::new();

    for (i, server_str) in servers.iter().enumerate() {
        let addr = match parse_proxy_addr(server_str) {
            Ok(a) => {
                // If the server string has no port component, use the config data_port
                if !server_str.contains(':') {
                    SocketAddrV4::new(*a.ip(), data_port)
                } else {
                    a
                }
            }
            Err(e) => {
                warn!("Cannot resolve proxy {}: {}", server_str, e);
                continue;
            }
        };

        let id = format!("proxy-{}", i);
        let server_str_clone = server_str.clone();
        handles.push(tokio::spawn(async move {
            let latency = probe_single_proxy(addr, 3, 2000).await;
            (id, addr, server_str_clone, latency)
        }));
    }

    let mut nodes = Vec::new();
    for handle in handles {
        if let Ok((id, addr, _server_str, latency)) = handle.await {
            let health = match latency {
                Some(us) if us < 500_000 => ProxyHealth::Healthy, // < 500 ms
                Some(_) => ProxyHealth::Degraded,                  // ≥ 500 ms
                None => ProxyHealth::Unhealthy,                    // No response
            };

            nodes.push(ProxyNode {
                id,
                data_addr: addr,
                control_addr: SocketAddrV4::new(*addr.ip(), 4433),
                region: "unknown".into(),
                health,
                latency_us: latency,
                load: 0.0,
            });
        }
    }

    nodes
}

/// Select the best proxy from the configured server list using the specified
/// strategy.
///
/// Probes all proxies, builds a `ProxyNode` list, and runs the
/// `RouteSelector`.
pub async fn select_best_proxy(
    servers: &[String],
    data_port: u16,
    game_server: SocketAddrV4,
    strategy: &str,
) -> anyhow::Result<SelectedRoute> {
    let nodes = probe_all_proxies(servers, data_port).await;

    if nodes.is_empty() {
        anyhow::bail!("No proxy servers could be resolved");
    }

    let healthy_count = nodes
        .iter()
        .filter(|n| n.health == ProxyHealth::Healthy)
        .count();
    if healthy_count == 0 {
        warn!("⚠️  No healthy proxies found, trying degraded nodes...");
    }

    let selector: Box<dyn RouteSelector> = match strategy {
        "ml" => match crate::route::selector::MlSelector::with_synthetic_training(100) {
            Ok(ml) => {
                tracing::info!("   Using ML route selector");
                Box::new(ml)
            }
            Err(e) => {
                warn!("   ML selector failed ({}), falling back to nearest", e);
                Box::new(NearestSelector::new())
            }
        },
        _ => Box::new(NearestSelector::new()),
    };

    selector
        .select(game_server, &nodes)
        .map_err(|e| anyhow::anyhow!("Route selection failed: {}", e))
}
