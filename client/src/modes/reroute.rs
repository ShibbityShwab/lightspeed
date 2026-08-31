//! # Continuous Re-Routing
//!
//! Periodically re-probes the configured relays and switches the tunnel to a
//! better one when the current path degrades. Builds on the one-shot probe +
//! select logic in [`super::proxy_probe`].

use std::net::SocketAddrV4;
use std::time::Duration;

use tokio::sync::watch;

use crate::route::ProxyHealth;

use super::proxy_probe::select_best_proxy;

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
/// Re-probes `servers` every [`REROUTE_INTERVAL`], re-selects the best relay,
/// and updates the process-global current relay (re-registering for a fresh
/// session token) when a meaningfully better relay is found. The interceptor
/// engine reads the global on every packet send.
pub async fn run_continuous_rerouting(
    servers: Vec<String>,
    data_port: u16,
    control_port: u16,
    game_server: SocketAddrV4,
    strategy: String,
    max_paths: u8,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut current = crate::session::current_proxy();
    let mut current_latency: Option<u64> = None;

    loop {
        tokio::select! {
            _ = shutdown.changed() => break,
            _ = tokio::time::sleep(REROUTE_INTERVAL) => {}
        }

        let route = match select_best_proxy(&servers, data_port, game_server, &strategy).await {
            Ok(r) => r,
            Err(_) => continue,
        };

        let best = route.primary;

        if max_paths >= 2 {
            let mut paths = vec![best.data_addr];
            paths.extend(route.backups.iter().map(|b| b.data_addr));
            paths.truncate(max_paths as usize);
            crate::session::set_multipath_paths(order_by_win_rate(paths));
            log_multipath_stats();
        }

        if best.health != ProxyHealth::Healthy {
            continue;
        }
        let Some(best_latency) = best.latency_us else {
            continue;
        };

        if current == Some(best.data_addr) || !should_switch(current_latency, best_latency) {
            continue;
        }

        match crate::quic::register_session(best.data_addr, control_port).await {
            Some(_) => {
                crate::session::set_current_proxy(best.data_addr);
                tracing::info!(
                    from = ?current,
                    to = %best.data_addr,
                    latency_us = best_latency,
                    "Rerouted to a faster relay"
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
}
