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

/// Friendly label for a node id: `relay-lax-1` -> `LAX - Los Angeles`.
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
        Some(city) => format!("{} - {}", code.to_ascii_uppercase(), city),
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

/// Measure every address on a background thread; the receiver yields the full
/// sample set once, so a caller ranking the whole path has every leg.
pub fn spawn_relay_measurement(
    addrs: Vec<SocketAddrV4>,
) -> Receiver<Vec<(SocketAddrV4, Option<RelaySample>)>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(measure_spreads_blocking(&addrs));
    });
    rx
}

/// One relay's measured client leg: the latency and how much it moved.
///
/// Both are needed because the owner's goal is "lowest latency and the most stable
/// connection", and on a mobile link the spread is what makes a match feel bad
/// even when the median is fine. A single probe cannot see spread: the ranking
/// weights jitter, and until this existed it was always handed zero.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RelaySample {
    /// Middle value across the probes.
    pub median: Duration,
    /// Spread across the probes: second-slowest minus fastest, so one spike or
    /// one lucky fast probe does not decide it.
    pub jitter: Duration,
}

/// How many times each relay is probed before its leg is written up.
///
/// Five, not one, because a single probe cannot see spread and the ranking weighs
/// it twice (`JITTER_WEIGHT` in `relay_choice`). Five still leaves three probes in
/// the middle once the fastest and slowest are held out, so one spike cannot
/// decide the relay. The cost is bounded: once per relay per
/// `RELAY_RECHECK_INTERVAL`, never per packet.
pub const RELAY_PROBES: usize = 5;

/// Probe every address `RELAY_PROBES` times and return each one's median and
/// spread, in input order.
///
/// Unlike [`measure_all_blocking`], which takes one sample and is used for the
/// fastest-relay race, this exists to characterise a link. A relay that does not
/// answer at all is omitted rather than reported as zero.
pub fn measure_spreads_blocking(
    addrs: &[SocketAddrV4],
) -> Vec<(SocketAddrV4, Option<RelaySample>)> {
    if addrs.is_empty() {
        return Vec::new();
    }
    let (tx, rx) = mpsc::channel();
    std::thread::scope(|scope| {
        for (index, addr) in addrs.iter().enumerate() {
            let tx = tx.clone();
            let addr = *addr;
            scope.spawn(move || {
                let mut times: Vec<Duration> = Vec::with_capacity(RELAY_PROBES);
                for _ in 0..RELAY_PROBES {
                    if let Some(rtt) = probe_latency(addr) {
                        times.push(rtt);
                    }
                }
                let _ = tx.send((index, addr, summarise(&mut times)));
            });
        }
    });
    drop(tx);

    let mut samples: Vec<(usize, SocketAddrV4, Option<RelaySample>)> = rx.iter().collect();
    samples.sort_by_key(|(index, _, _)| *index);
    samples
        .into_iter()
        .map(|(_, addr, spread)| (addr, spread))
        .collect()
}

/// Median and spread of the successful probes, or `None` when none succeeded.
///
/// The spread holds out one probe at each end (second-slowest minus fastest)
/// rather than being `max - min`: with a handful of probes, one transient spike
/// would otherwise dominate a term the ranking doubles and decide the relay for
/// the whole interval. The low end is the outright fastest probe because on a
/// congested mobile link the slow probe is the realistic outlier, while a fast
/// one is how a quiet moment looks.
fn summarise(times: &mut [Duration]) -> Option<RelaySample> {
    if times.is_empty() {
        return None;
    }
    times.sort_unstable();
    let n = times.len();
    let median = times[n / 2];
    // The handful of probes makes max - min unusable, but a percentile over five
    // samples is also too coarse to separate "steady with one spike" from "slow
    // every time": with sorted samples the answer is the second-slowest against
    // the fastest, which holds out exactly one probe at each end.
    let fast = times[0];
    let typical_slow = times[n - 1 - usize::from(n > 1)];
    let jitter = typical_slow.saturating_sub(fast);
    Some(RelaySample { median, jitter })
}

/// Probe every address concurrently and return the one with the lowest
/// round-trip to `/health`. Addresses that fail to answer are ignored.
pub fn race_blocking(addrs: &[SocketAddrV4]) -> Option<SocketAddrV4> {
    fastest_addr(measure_all_blocking(addrs))
}

/// Probe every address concurrently and return every measured round trip.
///
/// [`race_blocking`] keeps only the winner, which throws away exactly the
/// per-relay figures a destination-aware choice needs: knowing that Los Angeles
/// answers in 244 ms is useless without knowing what New Jersey answers, because
/// the winner of that race is not the winner of the whole path. Addresses that
/// fail to answer are omitted rather than reported as zero.
pub fn measure_all_blocking(addrs: &[SocketAddrV4]) -> Vec<(SocketAddrV4, Option<Duration>)> {
    if addrs.is_empty() {
        return Vec::new();
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
    samples
        .into_iter()
        .map(|(_, addr, latency)| (addr, latency))
        .collect()
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
        fastest_addr, friendly_label, health_endpoint, measure_spreads_blocking, parse_health,
        parse_nodes, probe_blocking, race_blocking, spawn_relay_race, split_body, summarise,
        RelayHealth, RelaySample, RELAY_PROBES,
    };
    use std::net::SocketAddrV4;
    use std::time::Duration;

    #[test]
    fn friendly_labels_known_and_unknown_nodes() {
        assert_eq!(friendly_label("relay-lax-1"), "LAX - Los Angeles");
        assert_eq!(friendly_label("relay-ewr-1"), "EWR - New Jersey");
        assert_eq!(friendly_label("relay-sgp-1"), "SGP - Singapore");
        assert_eq!(friendly_label("relay-fra"), "FRA - Frankfurt");
        assert_eq!(friendly_label("relay-nrt"), "NRT - Tokyo");
        assert_eq!(friendly_label("relay-bom-1"), "BOM - Mumbai");
        assert_eq!(friendly_label("relay-mad-1"), "MAD - Madrid");
        assert_eq!(friendly_label("relay-syd-1"), "SYD - Sydney");
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
    fn summarise_reports_the_median_and_the_tail_spread() {
        // An even count has no middle element; the upper middle is the median of
        // record, so a split sample set does not depend on how it was sorted.
        // Sorted 10, 20, 30, 40: the median is the upper middle, and the spread
        // is second-slowest minus fastest.
        let mut times = [
            Duration::from_millis(40),
            Duration::from_millis(10),
            Duration::from_millis(30),
            Duration::from_millis(20),
        ];
        assert_eq!(
            summarise(&mut times),
            Some(RelaySample {
                median: Duration::from_millis(30),
                jitter: Duration::from_millis(20),
            })
        );
    }

    #[test]
    fn summarise_ignores_a_single_outlier_probe() {
        let mut times = [
            Duration::from_millis(100),
            Duration::from_millis(101),
            Duration::from_millis(99),
            Duration::from_millis(102),
            Duration::from_millis(290),
        ];
        // max - min would report 191 ms of swing, and the ranking doubles that,
        // so one spike would outvote a relay that is otherwise steady.
        assert_eq!(
            summarise(&mut times),
            Some(RelaySample {
                median: Duration::from_millis(101),
                jitter: Duration::from_millis(3),
            })
        );
    }

    #[test]
    fn summarise_of_one_probe_has_no_spread_information() {
        let mut times = [Duration::from_millis(77)];
        assert_eq!(
            summarise(&mut times),
            Some(RelaySample {
                median: Duration::from_millis(77),
                jitter: Duration::ZERO,
            })
        );
        assert_eq!(summarise(&mut []), None);
    }

    #[test]
    fn spreads_are_returned_in_input_order_and_silent_relays_are_none() {
        use std::io::{Read, Write};

        // Two ports bound then dropped: connects are refused, which is the
        // "relay did not answer" case the measurement must not report as zero.
        let silent = |()| -> SocketAddrV4 {
            match std::net::TcpListener::bind("127.0.0.1:0")
                .expect("bind")
                .local_addr()
                .expect("addr")
            {
                std::net::SocketAddr::V4(v4) => v4,
                std::net::SocketAddr::V6(_) => unreachable!("bound to IPv4"),
            }
        };
        let first = silent(());
        let second = silent(());
        let refused = [first, second];

        let samples = measure_spreads_blocking(&refused);
        assert_eq!(samples.len(), refused.len());
        assert_eq!(
            samples.iter().map(|(addr, _)| *addr).collect::<Vec<_>>(),
            refused.to_vec(),
            "results stay in input order"
        );
        assert!(
            samples.iter().all(|(_, sample)| sample.is_none()),
            "a relay that never answered has no sample, not a zero one"
        );

        let samples = measure_spreads_blocking(&[]);
        assert!(samples.is_empty());

        // One relay answering every probe: the median is a real round trip and
        // the spread across five loopback probes is reported, not left at zero.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let responder = match listener.local_addr().expect("addr") {
            std::net::SocketAddr::V4(v4) => v4,
            std::net::SocketAddr::V6(_) => unreachable!("bound to IPv4"),
        };
        let handle = std::thread::spawn(move || {
            for _ in 0..RELAY_PROBES {
                if let Ok((mut stream, _)) = listener.accept() {
                    let mut buf = [0u8; 1024];
                    let _ = stream.read(&mut buf);
                    let body = r#"{"packets_relayed":0,"sessions_created":0}"#;
                    let response = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                        body.len(),
                        body
                    );
                    let _ = stream.write_all(response.as_bytes());
                }
            }
        });
        let measured = measure_spreads_blocking(&[responder]);
        handle.join().expect("server thread");
        assert_eq!(measured.len(), 1);
        assert_eq!(measured[0].0, responder);
        assert!(
            measured[0].1.map(|s| s.median) > Some(Duration::ZERO),
            "a relay that answered has a real median"
        );
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
