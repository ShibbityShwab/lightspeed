//! Zero-config relay discovery and relay health probing.
//!
//! Discovery consults the signed community registry through the public
//! `lightspeed_client::registry` API on a background thread, so the egui
//! `update()` loop never blocks. Health probes hit each relay's plain-HTTP
//! `/health` endpoint (port 8080) for live packet/session counters.

use std::net::SocketAddrV4;
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use lightspeed_client::registry::{
    discover_nodes, DEFAULT_OPERATOR_PUBKEY_B64, DEFAULT_REGISTRY_URL,
};

use crate::config::ProxyEntry;

/// Control-plane QUIC port of the community relays. Also passed to the
/// registry so discovered certificate fingerprints are pre-pinned for it.
pub const CONTROL_PORT: u16 = 4433;

/// Plain-HTTP health endpoint port on every relay.
pub const HEALTH_PORT: u16 = 8080;

/// Bound for one discovery attempt (the registry fetch itself has a 10 s
/// HTTP timeout; this is the outer guard).
const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(15);

/// Connect/read timeout for a relay health probe.
const HEALTH_TIMEOUT: Duration = Duration::from_secs(3);

/// A relay discovered through the registry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RelayInfo {
    pub node_id: String,
    pub addr: SocketAddrV4,
}

impl RelayInfo {
    /// Convert into the GUI's proxy-list entry, with a friendly region label
    /// when the node id is one of the known community relays.
    pub fn into_entry(self) -> ProxyEntry {
        ProxyEntry::discovered(
            self.node_id.clone(),
            self.addr,
            friendly_label(&self.node_id),
        )
    }
}

/// Result of one background discovery attempt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DiscoveryOutcome {
    /// At least one relay was discovered.
    Found(Vec<RelayInfo>),
    /// Discovery failed or returned nothing; the string is user-facing.
    Failed(String),
}

/// Live counters from a relay's `GET /health`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelayHealth {
    pub packets_relayed: u64,
    pub sessions_created: u64,
}

/// Start a discovery attempt on a background thread; the receiver yields
/// exactly one outcome.
pub fn spawn_discovery() -> Receiver<DiscoveryOutcome> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(discover_blocking());
    });
    rx
}

/// Blocking discovery on its own current-thread Tokio runtime, mirroring the
/// update-checker pattern so callers never need a runtime of their own.
pub fn discover_blocking() -> DiscoveryOutcome {
    let runtime = match tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(e) => return DiscoveryOutcome::Failed(format!("discovery runtime: {e}")),
    };

    let result = runtime.block_on(async {
        tokio::time::timeout(DISCOVERY_TIMEOUT, async {
            discover_nodes(
                DEFAULT_REGISTRY_URL,
                DEFAULT_OPERATOR_PUBKEY_B64,
                CONTROL_PORT,
            )
        })
        .await
    });

    match result {
        Ok(Ok(nodes)) => {
            let relays = parse_nodes(nodes);
            if relays.is_empty() {
                tracing::warn!("Relay registry returned no usable relays");
                DiscoveryOutcome::Failed("registry returned no relays".to_string())
            } else {
                tracing::info!("Discovered {} community relays", relays.len());
                DiscoveryOutcome::Found(relays)
            }
        }
        Ok(Err(e)) => {
            tracing::warn!("Relay discovery failed: {e}");
            DiscoveryOutcome::Failed(e.to_string())
        }
        Err(_) => {
            tracing::warn!("Relay discovery timed out");
            DiscoveryOutcome::Failed("relay discovery timed out".to_string())
        }
    }
}

/// Convert raw `(node_id, "ip:port")` pairs, dropping malformed addresses.
fn parse_nodes(nodes: Vec<(String, String)>) -> Vec<RelayInfo> {
    nodes
        .into_iter()
        .filter_map(|(node_id, addr)| match addr.parse::<SocketAddrV4>() {
            Ok(addr) => Some(RelayInfo { node_id, addr }),
            Err(_) => {
                tracing::warn!("Skipping relay {node_id} with invalid address {addr}");
                None
            }
        })
        .collect()
}

/// Friendly label for a node id: `relay-lax-1` → `LAX — Los Angeles`.
/// Unknown node ids are shown verbatim.
pub fn friendly_label(node_id: &str) -> String {
    let code = node_id
        .strip_prefix("relay-")
        .unwrap_or(node_id)
        .trim_end_matches(|c: char| c.is_ascii_digit() || c == '-');
    let city = match code {
        "lax" => Some("Los Angeles"),
        "ewr" => Some("New Jersey"),
        "sgp" => Some("Singapore"),
        "fra" => Some("Frankfurt"),
        "nrt" => Some("Tokyo"),
        "bom" => Some("Mumbai"),
        "mad" => Some("Madrid"),
        "syd" => Some("Sydney"),
        _ => None,
    };
    match city {
        Some(city) => format!("{} — {}", code.to_ascii_uppercase(), city),
        None => node_id.to_string(),
    }
}

/// The `/health` address for a relay: the registry advertises the UDP data
/// port, so health probes always use [`HEALTH_PORT`] on the same host.
pub fn health_endpoint(relay: SocketAddrV4) -> SocketAddrV4 {
    SocketAddrV4::new(*relay.ip(), HEALTH_PORT)
}

/// Start a `/health` probe for `addr` on a background thread.
pub fn spawn_health_probe(addr: SocketAddrV4) -> Receiver<Result<RelayHealth, String>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(probe_blocking(addr));
    });
    rx
}

/// Fetch and parse `addr`'s `/health` endpoint.
pub fn probe_blocking(addr: SocketAddrV4) -> Result<RelayHealth, String> {
    let body = http_get(addr, "/health")?;
    parse_health(&body)
}

/// Race every address in `addrs` for the lowest `/health` round trip on a
/// background thread. The receiver yields the fastest relay that answered, or
/// `None` when none did.
pub fn spawn_relay_race(addrs: Vec<SocketAddrV4>) -> Receiver<Option<SocketAddrV4>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(race_blocking(&addrs));
    });
    rx
}

/// Probe every address concurrently and return the one with the lowest
/// round-trip to `/health`. Addresses that fail to answer are ignored.
pub fn race_blocking(addrs: &[SocketAddrV4]) -> Option<SocketAddrV4> {
    if addrs.is_empty() {
        return None;
    }
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        for (index, addr) in addrs.iter().enumerate() {
            let tx = tx.clone();
            let addr = *addr;
            scope.spawn(move || {
                let _ = tx.send((index, addr, probe_latency(addr)));
            });
        }
    });
    drop(tx);

    // Rebuild in input order so equal latencies keep the discovery order.
    let mut samples: Vec<(usize, SocketAddrV4, Option<Duration>)> = rx.iter().collect();
    samples.sort_by_key(|(index, _, _)| *index);
    fastest_addr(
        samples
            .into_iter()
            .map(|(_, addr, latency)| (addr, latency))
            .collect(),
    )
}

/// Time one `/health` round trip, or `None` when the relay does not answer.
fn probe_latency(addr: SocketAddrV4) -> Option<Duration> {
    let started = std::time::Instant::now();
    probe_blocking(addr).ok().map(|_| started.elapsed())
}

/// The address with the smallest successful latency; ties keep input order.
/// Addresses that timed out or failed contribute `None` and are ignored.
pub fn fastest_addr(samples: Vec<(SocketAddrV4, Option<Duration>)>) -> Option<SocketAddrV4> {
    samples
        .into_iter()
        .filter_map(|(addr, latency)| latency.map(|latency| (addr, latency)))
        .reduce(|best, next| if next.1 < best.1 { next } else { best })
        .map(|(addr, _)| addr)
}

/// Minimal HTTP/1.1 GET for the relays' plain-HTTP health endpoint. The GUI
/// deliberately avoids pulling in an HTTP client for one unauthenticated
/// request on a fixed port.
fn http_get(addr: SocketAddrV4, path: &str) -> Result<String, String> {
    use std::io::{Read, Write};

    let socket = std::net::SocketAddr::V4(addr);
    let mut stream = std::net::TcpStream::connect_timeout(&socket, HEALTH_TIMEOUT)
        .map_err(|e| format!("connect to {addr}: {e}"))?;
    stream
        .set_read_timeout(Some(HEALTH_TIMEOUT))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(HEALTH_TIMEOUT))
        .map_err(|e| e.to_string())?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: {addr}\r\nConnection: close\r\n\r\n");
    stream
        .write_all(request.as_bytes())
        .map_err(|e| e.to_string())?;
    let mut buf = Vec::new();
    stream.read_to_end(&mut buf).map_err(|e| e.to_string())?;
    let text = String::from_utf8_lossy(&buf);
    split_body(&text)
        .map(str::to_string)
        .ok_or_else(|| "malformed HTTP response".to_string())
}

/// Split an HTTP response into its body (everything after the blank line).
fn split_body(response: &str) -> Option<&str> {
    if let Some(pos) = response.find("\r\n\r\n") {
        return Some(&response[pos + 4..]);
    }
    if let Some(pos) = response.find("\n\n") {
        return Some(&response[pos + 2..]);
    }
    None
}

/// Parse the fields the GUI displays from a `/health` JSON body.
fn parse_health(body: &str) -> Result<RelayHealth, String> {
    let value: serde_json::Value =
        serde_json::from_str(body).map_err(|e| format!("bad health JSON: {e}"))?;
    Ok(RelayHealth {
        packets_relayed: value
            .get("packets_relayed")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
        sessions_created: value
            .get("sessions_created")
            .and_then(serde_json::Value::as_u64)
            .unwrap_or(0),
    })
}

#[cfg(test)]
mod tests {
    use super::{
        fastest_addr, friendly_label, health_endpoint, parse_health, parse_nodes, probe_blocking,
        race_blocking, spawn_relay_race, split_body, RelayHealth,
    };
    use std::net::SocketAddrV4;
    use std::time::Duration;

    #[test]
    fn friendly_labels_known_and_unknown_nodes() {
        assert_eq!(friendly_label("relay-lax-1"), "LAX — Los Angeles");
        assert_eq!(friendly_label("relay-ewr-1"), "EWR — New Jersey");
        assert_eq!(friendly_label("relay-sgp-1"), "SGP — Singapore");
        assert_eq!(friendly_label("relay-fra"), "FRA — Frankfurt");
        assert_eq!(friendly_label("relay-nrt"), "NRT — Tokyo");
        assert_eq!(friendly_label("relay-bom-1"), "BOM — Mumbai");
        assert_eq!(friendly_label("relay-mad-1"), "MAD — Madrid");
        assert_eq!(friendly_label("relay-syd-1"), "SYD — Sydney");
        assert_eq!(friendly_label("relay-xyz-9"), "relay-xyz-9");
    }

    #[test]
    fn parse_nodes_drops_invalid_addresses() {
        let nodes = parse_nodes(vec![
            ("relay-lax-1".into(), "207.246.106.36:4434".into()),
            ("broken".into(), "not-an-address".into()),
        ]);
        assert_eq!(nodes.len(), 1);
        assert_eq!(nodes[0].node_id, "relay-lax-1");
        assert_eq!(
            nodes[0].addr,
            "207.246.106.36:4434".parse::<SocketAddrV4>().unwrap()
        );
    }

    #[test]
    fn split_body_handles_crlf_and_lf() {
        assert_eq!(
            split_body("HTTP/1.1 200 OK\r\n\r\n{\"a\":1}"),
            Some("{\"a\":1}")
        );
        assert_eq!(
            split_body("HTTP/1.1 200 OK\n\n{\"a\":1}"),
            Some("{\"a\":1}")
        );
        assert_eq!(split_body("no separator"), None);
    }

    #[test]
    fn parse_health_reads_counters() {
        let health = parse_health(r#"{"status":"ok","packets_relayed":1234,"sessions_created":7}"#)
            .expect("valid health JSON");
        assert_eq!(
            health,
            RelayHealth {
                packets_relayed: 1234,
                sessions_created: 7
            }
        );
    }

    #[test]
    fn parse_health_defaults_missing_counters_to_zero() {
        let health = parse_health(r#"{"status":"ok"}"#).expect("valid health JSON");
        assert_eq!(
            health,
            RelayHealth {
                packets_relayed: 0,
                sessions_created: 0
            }
        );
    }

    #[test]
    fn parse_health_rejects_non_json() {
        assert!(parse_health("<html>nope</html>").is_err());
    }

    #[test]
    #[ignore = "hits the live registry and production relay fleet"]
    fn live_race_prefers_a_reachable_relay() {
        let relays: Vec<SocketAddrV4> = match super::discover_blocking() {
            super::DiscoveryOutcome::Found(nodes) => nodes
                .into_iter()
                .map(|node| health_endpoint(node.addr))
                .collect(),
            super::DiscoveryOutcome::Failed(reason) => panic!("discovery failed: {reason}"),
        };
        let winner = race_blocking(&relays);
        println!("raced {} relays, winner = {winner:?}", relays.len());
        assert!(winner.is_some(), "at least one relay should answer /health");
    }

    #[test]
    fn health_endpoint_replaces_the_registry_port() {
        // The registry advertises the UDP data port; health is always on 8080.
        let relay = "203.0.113.5:4434".parse::<SocketAddrV4>().expect("addr");
        assert_eq!(
            health_endpoint(relay),
            "203.0.113.5:8080".parse::<SocketAddrV4>().expect("addr")
        );
    }

    #[test]
    fn fastest_addr_picks_the_lowest_latency_and_skips_failures() {
        let a = "10.0.0.1:8080".parse::<SocketAddrV4>().expect("addr");
        let b = "10.0.0.2:8080".parse::<SocketAddrV4>().expect("addr");
        let c = "10.0.0.3:8080".parse::<SocketAddrV4>().expect("addr");
        let samples = vec![
            (a, None),
            (b, Some(Duration::from_millis(80))),
            (c, Some(Duration::from_millis(20))),
        ];
        assert_eq!(fastest_addr(samples), Some(c));
    }

    #[test]
    fn fastest_addr_ties_keep_input_order() {
        let a = "10.0.0.1:8080".parse::<SocketAddrV4>().expect("addr");
        let b = "10.0.0.2:8080".parse::<SocketAddrV4>().expect("addr");
        let samples = vec![
            (a, Some(Duration::from_millis(30))),
            (b, Some(Duration::from_millis(30))),
        ];
        assert_eq!(fastest_addr(samples), Some(a));
    }

    #[test]
    fn fastest_addr_without_successes_is_none() {
        let a = "10.0.0.1:8080".parse::<SocketAddrV4>().expect("addr");
        assert_eq!(fastest_addr(vec![(a, None)]), None);
        assert_eq!(fastest_addr(Vec::new()), None);
    }

    #[test]
    fn race_without_addresses_returns_none() {
        assert_eq!(race_blocking(&[]), None);
    }

    #[test]
    fn spawn_relay_race_delivers_a_result() {
        let rx = spawn_relay_race(Vec::new());
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(2)).ok(),
            Some(None),
            "an empty race resolves to no winner"
        );
    }

    #[test]
    fn race_prefers_the_responding_relay() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let responder = match listener.local_addr().expect("addr") {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => unreachable!("bound to IPv4"),
        };
        // Bind and immediately drop a second port so connects are refused.
        let closed = match std::net::TcpListener::bind("127.0.0.1:0")
            .expect("bind")
            .local_addr()
            .expect("addr")
        {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => unreachable!("bound to IPv4"),
        };
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = r#"{"packets_relayed":1,"sessions_created":1}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let winner = race_blocking(&[closed, responder]);
        handle.join().expect("server thread");
        assert_eq!(winner, Some(responder));
    }

    #[test]
    fn probe_reads_a_local_health_endpoint() {
        use std::io::{Read, Write};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let addr = match listener.local_addr().expect("addr") {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => unreachable!("bound to IPv4"),
        };
        let handle = std::thread::spawn(move || {
            if let Ok((mut stream, _)) = listener.accept() {
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let body = r#"{"packets_relayed":42,"sessions_created":3}"#;
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes());
            }
        });
        let health = probe_blocking(addr).expect("probe succeeds");
        assert_eq!(
            health,
            RelayHealth {
                packets_relayed: 42,
                sessions_created: 3
            }
        );
        handle.join().expect("server thread");
    }
}
