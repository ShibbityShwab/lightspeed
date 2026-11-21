//! Continuous re-routing for the adopted live modes (post-W2 task-57).
//!
//! Ported from `client/src/modes/reroute.rs`: the probe/select rules, the
//! destination-aware override and the win-rate ordering are unchanged. The one
//! difference is where a switch lands — through the shared engine's
//! [`crate::LightSpeedEngine::switch_relay`] command, so the library data plane
//! and the session state move together (the binary copy could only move the
//! binary statics; see `crate::app::live`).
//!
//! **Parity note, recorded not changed.** The shipped caller
//! (`client/src/main.rs::start_continuous_rerouting`) creates a shutdown watch
//! channel, spawns the loop and drops the sender when the function returns. A
//! dropped `watch::Sender` makes `receiver.changed()` return `Err`, so the
//! shipped loop exits on its first `select!` tick — the CLI's continuous
//! rerouting never actually runs today. [`spawn`] reproduces that construction
//! exactly, so the adopted `--watch` mode behaves identically. Fixing the dead
//! path is a separate product decision, not part of a no-behaviour-change
//! migration.

use std::net::{Ipv4Addr, SocketAddrV4};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::sync::watch;

use crate::engine::LightSpeedEngine;
use crate::modes::proxy_probe::{select_best_proxy, ProbeCandidate};
use crate::route::{ProxyHealth, RouteStrategy};

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
/// fewer than 10 observed responses are treated as unknown and kept first.
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

fn sync_supervisors(
    supervised: &mut Vec<SocketAddrV4>,
    active: &[SocketAddrV4],
    control_port: u16,
    path_mtu_cap: usize,
) {
    for addr in active {
        if !supervised.contains(addr) {
            crate::quic::ensure_registration(*addr, control_port, path_mtu_cap);
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

/// Spawn the continuous re-routing loop for an adopted live mode.
///
/// Same construction and same first-tick behaviour as the shipped caller: the
/// shutdown sender is dropped before this function returns, so the loop stops
/// immediately (parity, see the module note).
pub fn spawn(
    engine: Arc<Mutex<LightSpeedEngine>>,
    candidates: Vec<ProbeCandidate>,
    data_port: u16,
    control_port: u16,
    game_server: SocketAddrV4,
    strategy: String,
    max_paths: u8,
    path_mtu_cap: usize,
) -> tokio::task::JoinHandle<()> {
    let (shutdown_tx, shutdown) = watch::channel(false);
    drop(shutdown_tx);
    tokio::spawn(run(
        engine,
        candidates,
        data_port,
        control_port,
        game_server,
        strategy,
        max_paths,
        path_mtu_cap,
        shutdown,
    ))
}

#[allow(clippy::too_many_arguments)]
async fn run(
    engine: Arc<Mutex<LightSpeedEngine>>,
    candidates: Vec<ProbeCandidate>,
    data_port: u16,
    control_port: u16,
    game_server: SocketAddrV4,
    strategy: String,
    max_paths: u8,
    path_mtu_cap: usize,
    mut shutdown: watch::Receiver<bool>,
) {
    let mut current = crate::session::current_proxy();
    let mut current_latency: Option<u64> = None;
    let mut supervised_paths: Vec<SocketAddrV4> = Vec::new();
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
        // use, so the selection can rank the whole path.
        if known_destination && reported_destination != Some(destination) {
            if let Some(relay) = current {
                let switch = {
                    let mut engine = engine
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    engine.switch_relay(relay, Some(*destination.ip()))
                };
                switch.registered().await;
                reported_destination = Some(destination);
            }
        }

        let route =
            match select_best_proxy(&candidates, data_port, control_port, destination, &strategy)
                .await
            {
                Ok(route) => route,
                Err(_) => continue,
            };

        let best = route.primary;

        if max_paths >= 2 {
            let mut paths = vec![best.data_addr];
            paths.extend(route.backups.iter().map(|b| b.data_addr));
            paths.truncate(max_paths as usize);
            let paths = order_by_win_rate(paths);
            sync_supervisors(&mut supervised_paths, &paths, control_port, path_mtu_cap);
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

        let switch = {
            let mut engine = engine
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            engine.switch_relay(best.data_addr, known_destination.then(|| *destination.ip()))
        };
        match switch.registered().await {
            Some(_) => {
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
    fn switch_thresholds_are_the_shipped_ones() {
        assert!(should_switch(None, 100_000));
        assert!(should_switch(Some(300_000), 200_000));
        assert!(!should_switch(Some(300_000), 290_000));
        assert!(!should_switch(Some(100_000), 150_000));
    }

    #[test]
    fn destination_aware_switches_to_the_relay_the_server_is_near() {
        let sgp = SocketAddrV4::new(Ipv4Addr::new(45, 77, 32, 236), 4434);
        let lax = SocketAddrV4::new(Ipv4Addr::new(207, 246, 106, 36), 4434);
        assert!(should_switch_for(
            true,
            Some(sgp),
            lax,
            Some(65_000),
            214_000
        ));
        assert!(!should_switch_for(
            false,
            Some(sgp),
            lax,
            Some(65_000),
            214_000
        ));
        assert!(!should_switch_for(
            true,
            Some(lax),
            lax,
            Some(214_000),
            214_000
        ));
    }
}
