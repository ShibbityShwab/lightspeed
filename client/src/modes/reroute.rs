//! # Continuous Re-Routing
//!
//! Periodically re-probes the configured relays and switches the tunnel to a
//! better one when the current path degrades. Builds on the one-shot probe +
//! select logic in [`super::proxy_probe`].

use std::net::{Ipv4Addr, SocketAddrV4};
use std::time::Duration;

use tokio::sync::watch;

use crate::route::{ProxyHealth, RouteStrategy};

use super::proxy_probe::{select_best_proxy, ProbeCandidate};

/// How often to re-probe and consider switching relays.
const REROUTE_INTERVAL: Duration = Duration::from_secs(30);

/// Whether a candidate relay is worth switching to. Requires a meaningful
/// improvement to avoid flapping between near-equal relays.
fn should_switch(current_latency: Option<u64>, candidate_latency: u64) -> bool {
    match current_latency {
        None => true,
        Some(cur) => {
            let threshold = (cur / 5).max(10_000);
            candidate_latency < cur.saturating_sub(threshold)
        }
    }
}

/// Decide between a client-leg re-race and a destination-aware ranking.
///
/// The latency threshold applies to the client leg, which is what a `Nearest`
/// race measures. A `DestinationAware` result has already priced the relay's
/// leg to the game server, so its winner can legitimately have a *worse* client
/// leg than the relay currently carrying the match - the case that kept a
/// player on a nearby relay while the server sat on another continent.
fn should_switch_for(
    destination_aware: bool,
    current: Option<SocketAddrV4>,
    best: SocketAddrV4,
    current_latency: Option<u64>,
    best_latency: u64,
) -> bool {
    if current == Some(best) {
        return false;
    }
    if destination_aware {
        return true;
    }
    should_switch(current_latency, best_latency)
}

/// Order multipath paths by accumulated win rate (highest first). Paths with
/// fewer than 10 observed responses are treated as unknown and kept first so
/// new relays are not demoted before they have a track record.
fn order_by_win_rate(paths: Vec<SocketAddrV4>) -> Vec<SocketAddrV4> {
    let stats = crate::session::multipath_stats();
    let win_rate = |a: &SocketAddrV4| -> f64 {
        stats
            .iter()
            .find(|(addr, _)| addr == a)
            .map(|(_, s)| {
                if s.total >= 10 {
                    s.wins as f64 / s.total as f64
                } else {
                    1.0
                }
            })
            .unwrap_or(1.0)
    };
    let mut ranked: Vec<(SocketAddrV4, f64)> =
        paths.into_iter().map(|p| (p, win_rate(&p))).collect();
    ranked.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    ranked.into_iter().map(|(p, _)| p).collect()
}

/// Register a supervisor for every newly active relay and stop supervisors for
/// relays that dropped out of the active set. Registration is idempotent per
/// relay and each supervisor carries a generation, so a stale task can never
/// overwrite a newer one.
fn sync_supervisors(
    supervised: &mut Vec<SocketAddrV4>,
    active: &[SocketAddrV4],
    control_port: u16,
) {
    for addr in active {
        if !supervised.contains(addr) {
            crate::quic::ensure_registration(*addr, control_port);
        }
    }
    for addr in supervised.iter() {
        if !active.contains(addr) {
            crate::quic::stop_supervisor(*addr);
        }
    }
    supervised.clear();
    supervised.extend_from_slice(active);
}

fn log_multipath_stats() {
    let stats = crate::session::multipath_stats();
    if stats.is_empty() {
        return;
    }
    let mut lines: Vec<String> = stats
        .iter()
        .map(|(a, s)| {
            let rate = if s.total > 0 {
                s.wins as f64 / s.total as f64 * 100.0
            } else {
                0.0
            };
            format!("{} {:.0}% ({}w/{}t)", a, rate, s.wins, s.total)
        })
        .collect();
    lines.sort();
    tracing::info!(stats = %lines.join(", "), "multipath path win rates");
}

/// Run the continuous re-routing loop until `shutdown` fires.
///
/// Re-probes `candidates` every [`REROUTE_INTERVAL`], re-selects the best
/// relay, and updates the process-global current relay (re-registering for a
/// fresh session token) when a meaningfully better relay is found. The
/// interceptor engine reads the global on every packet send.
pub async fn run_continuous_rerouting(
    candidates: Vec<ProbeCandidate>,
    data_port: u16,
    control_port: u16,
    game_server: SocketAddrV4,
    strategy: String,
    max_paths: u8,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut current = crate::session::current_proxy();
    let mut current_latency: Option<u64> = None;
    let mut supervised_paths: Vec<SocketAddrV4> = Vec::new();
    // The destination already reported to a relay. The registration ack is the
    // only source of a destination's region, and the estimator needs that region
    // to price the relay-to-server leg, so the first iteration after a server is
    // locked spends one registration learning it.
    let mut reported_destination: Option<SocketAddrV4> = None;
    const UNSPECIFIED: SocketAddrV4 = SocketAddrV4::new(Ipv4Addr::UNSPECIFIED, 0);

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(REROUTE_INTERVAL) => {}
        }

        let destination = crate::session::locked_destination().unwrap_or(game_server);
        let known_destination = destination != UNSPECIFIED;

        // Seed the region for a newly locked server through the relay already in
        // use, so the selection that follows can rank the whole path instead of
        // falling back to client-nearest.
        if known_destination && reported_destination != Some(destination) {
            if let Some(relay) = current {
                crate::quic::register_session_with_destination(
                    relay,
                    control_port,
                    Some(*destination.ip()),
                )
                .await;
                reported_destination = Some(destination);
            }
        }

        let route =
            match select_best_proxy(&candidates, data_port, control_port, destination, &strategy)
                .await
            {
                Ok(r) => r,
                Err(_) => continue,
            };

        let best = route.primary;

        if max_paths >= 2 {
            let mut paths = vec![best.data_addr];
            paths.extend(route.backups.iter().map(|b| b.data_addr));
            paths.truncate(max_paths as usize);
            let paths = order_by_win_rate(paths);
            sync_supervisors(&mut supervised_paths, &paths, control_port);
            crate::session::set_multipath_paths(paths);
            log_multipath_stats();
        }

        if best.health != ProxyHealth::Healthy {
            continue;
        }
        let Some(best_latency) = best.latency_us else {
            continue;
        };

        let destination_aware = matches!(&route.strategy, RouteStrategy::DestinationAware);
        if !should_switch_for(
            destination_aware,
            current,
            best.data_addr,
            current_latency,
            best_latency,
        ) {
            continue;
        }

        let registered = if known_destination {
            crate::quic::register_session_with_destination(
                best.data_addr,
                control_port,
                Some(*destination.ip()),
            )
            .await
        } else {
            crate::quic::register_session(best.data_addr, control_port).await
        };

        match registered {
            Some(_) => {
                crate::session::set_current_proxy(best.data_addr);
                tracing::info!(
                    from = ?current,
                    to = %best.data_addr,
                    destination = %destination,
                    destination_aware,
                    latency_us = best_latency,
                    "Rerouted to a better relay"
                );
                current = Some(best.data_addr);
                current_latency = Some(best_latency);
            }
            None => {
                tracing::warn!(to = %best.data_addr, "Re-routing skipped: could not register");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_should_switch_first_time() {
        assert!(should_switch(None, 100_000));
    }

    #[test]
    fn test_should_switch_meaningful_improvement() {
        assert!(should_switch(Some(300_000), 200_000));
        assert!(should_switch(Some(50_000), 30_000));
    }

    #[test]
    fn test_no_switch_on_small_difference() {
        assert!(!should_switch(Some(300_000), 290_000));
        assert!(!should_switch(Some(50_000), 49_000));
    }

    #[test]
    fn test_no_switch_on_worse_candidate() {
        assert!(!should_switch(Some(100_000), 150_000));
    }

    #[test]
    fn destination_aware_switches_to_the_relay_the_server_is_near() {
        // Live case: a player in Asia on a US East server. Singapore wins the
        // client leg (65 ms) but loses the whole path (65 + ~230); Los Angeles
        // has the worse client leg (214 ms) and the better path (214 + ~65).
        let sgp = SocketAddrV4::new(Ipv4Addr::new(45, 77, 32, 236), 4434);
        let lax = SocketAddrV4::new(Ipv4Addr::new(207, 246, 106, 36), 4434);
        assert!(should_switch_for(
            true,
            Some(sgp),
            lax,
            Some(65_000),
            214_000
        ));
        // The same swap must not happen on a client-leg race alone.
        assert!(!should_switch_for(
            false,
            Some(sgp),
            lax,
            Some(65_000),
            214_000
        ));
    }

    #[test]
    fn no_switch_when_already_on_the_best_relay() {
        let lax = SocketAddrV4::new(Ipv4Addr::new(207, 246, 106, 36), 4434);
        assert!(!should_switch_for(
            true,
            Some(lax),
            lax,
            Some(214_000),
            214_000
        ));
        assert!(!should_switch_for(
            false,
            Some(lax),
            lax,
            Some(214_000),
            214_000
        ));
    }
}
