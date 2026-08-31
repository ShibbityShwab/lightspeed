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
    multipath_enabled: bool,
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

        if multipath_enabled {
            let mut paths = vec![best.data_addr];
            paths.extend(route.backups.iter().map(|b| b.data_addr));
            crate::session::set_multipath_paths(paths);
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
