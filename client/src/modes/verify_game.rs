//! `--verify-game`: prove that a game profile captures, attributes and relays
//! that game's own traffic.
//!
//! The static checks catch the drift classes (unknown key, empty process list,
//! inverted port range, missing wire id), and the live stage answers the only
//! question a user actually cares about: did bytes the game sent reach the relay
//! and come back? A profile whose filter matches nothing looks identical to a
//! quiet lobby in every counter except `packets_seen`, so both are reported.

use std::net::SocketAddrV4;
use std::path::Path;
use std::time::Duration;

use serde::Serialize;

use crate::interceptor::{build_config_for_game, create_interceptor, process_scanner};
use crate::tunnel::adaptive::AdaptiveConfig;

#[derive(Serialize, Default)]
pub struct VerifyReport {
    pub game: String,
    pub key: String,
    pub wire_id: u8,
    pub process_names: Vec<String>,
    pub ports: (u16, u16),
    pub anti_cheat: String,
    pub dynamic_server: bool,
    pub process_found: bool,
    pub pids: Vec<u32>,
    pub target_pid: Option<u32>,
    pub routes: Vec<String>,
    pub destination_ports: Vec<u16>,
    pub destinations_inside_profile: Option<bool>,
    pub live: Option<LiveReport>,
    pub verdict: Verdict,
    pub problems: Vec<String>,
    pub notes: Vec<String>,
}

#[derive(Serialize, Default)]
pub struct LiveReport {
    pub seconds: u64,
    pub platform: String,
    pub packets_seen: u64,
    pub packets_intercepted: u64,
    pub packets_tunnel_sent: u64,
    pub packets_injected: u64,
    pub packets_from_proxy: u64,
    pub errors: u64,
    pub detected_server: Option<String>,
    pub capture_diagnosis: Option<String>,
    pub tunnelling_enabled: Option<bool>,
}

#[derive(Serialize, Default, PartialEq, Eq, Clone, Copy, Debug)]
#[serde(rename_all = "lowercase")]
pub enum Verdict {
    #[default]
    Inconclusive,
    Pass,
    Fail,
}

impl Verdict {
    pub fn is_fail(self) -> bool {
        self == Self::Fail
    }
}

pub async fn run_verify_game(
    game_key: &str,
    proxy: Option<SocketAddrV4>,
    seconds: u64,
    report_path: Option<&Path>,
) -> anyhow::Result<Verdict> {
    let game = crate::games::resolve(game_key)?;
    let mut report = VerifyReport {
        game: game.name().to_string(),
        key: game_key.to_string(),
        wire_id: lightspeed_protocol::game_id::id_for_key(game_key),
        process_names: game.process_names().iter().map(|s| s.to_string()).collect(),
        ports: game.ports(),
        anti_cheat: game.anti_cheat().to_string(),
        dynamic_server: game.dynamic_server(),
        ..Default::default()
    };

    println!("verifying profile '{}' ({})", report.game, game_key);
    println!("  ports: {:?}", report.ports);
    println!("  processes: {:?}", report.process_names);

    if report.process_names.is_empty() {
        report
            .problems
            .push("profile declares no process names".into());
    }
    if report.ports.0 > report.ports.1 {
        report
            .problems
            .push(format!("inverted port range {:?}", report.ports));
    }
    if report.wire_id == lightspeed_protocol::game_id::UNKNOWN {
        report
            .problems
            .push("no protocol wire id for this key".into());
    }

    let running = crate::processes::list_running_processes();
    for process in &running {
        if report
            .process_names
            .iter()
            .any(|known| crate::games::process_name_matches(&process.name, known))
        {
            report.pids.push(process.pid);
        }
    }
    report.process_found = !report.pids.is_empty();
    if report.process_found {
        println!("  running: PIDs {:?}", report.pids);
    } else {
        report.notes.push(format!(
            "none of {:?} is running; the live stage cannot prove capture",
            report.process_names
        ));
    }

    let names: Vec<&str> = report.process_names.iter().map(String::as_str).collect();
    let found = process_scanner::scan_for_games(&names);
    for process in &found {
        for route in &process.routes {
            report.routes.push(route.to_string());
            if route.proto == crate::interceptor::TransportProtocol::Udp
                && !report.destination_ports.contains(&route.remote.port())
            {
                report.destination_ports.push(route.remote.port());
            }
        }
    }
    if report
        .routes
        .iter()
        .any(|route| route.starts_with("Tcp") || route.starts_with("TCP"))
    {
        report
            .notes
            .push("TCP routes are listed but not verified: the tunnel carries UDP only".into());
    }

    let mut owners = crate::interceptor::socket_owners::OwnerCache::default();
    let table = owners.table(std::time::Instant::now());
    report.target_pid = report
        .pids
        .iter()
        .copied()
        .max_by_key(|pid| table.ports_for_pid(*pid).len());
    if let Some(pid) = report.target_pid {
        let bound = table.ports_for_pid(pid).len();
        println!("  target PID {pid} ({bound} local UDP ports)");
    }
    report.destination_ports.sort_unstable();
    if !report.destination_ports.is_empty() {
        let (lo, hi) = report.ports;
        let outside: Vec<u16> = report
            .destination_ports
            .iter()
            .copied()
            .filter(|port| *port < lo || *port > hi)
            .collect();
        report.destinations_inside_profile = Some(outside.is_empty());
        println!("  seat ports seen: {:?}", report.destination_ports);
        if !outside.is_empty() {
            report.problems.push(format!(
                "live traffic goes to ports {:?}, outside the profile range {:?}",
                outside, report.ports
            ));
        }
    } else {
        report
            .notes
            .push("no remote destinations yet; a menu or lobby has none".into());
    }

    if let Some(proxy) = proxy {
        if seconds > 0 {
            match live_stage(game_key, proxy, seconds, report.target_pid, &mut report).await {
                Ok(()) => {}
                Err(error) => {
                    report
                        .problems
                        .push(format!("live stage did not run: {error}"));
                }
            }
        }
    } else {
        report
            .notes
            .push("no proxy resolved, so the live relay stage was skipped".into());
    }

    let verdict = verdict_from(&report.problems, report.live.is_some());
    report.verdict = verdict;
    println!("  verdict: {verdict:?}");
    for problem in &report.problems {
        println!("  problem: {problem}");
    }
    for note in &report.notes {
        println!("  note: {note}");
    }
    if let Some(path) = report_path {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(path, serde_json::to_string_pretty(&report)?)?;
        println!("  report: {}", path.display());
    }
    Ok(report.verdict)
}

fn verdict_from(problems: &[String], has_live: bool) -> Verdict {
    if !problems.is_empty() {
        Verdict::Fail
    } else if has_live {
        Verdict::Pass
    } else {
        Verdict::Inconclusive
    }
}

async fn live_stage(
    game_key: &str,
    proxy: SocketAddrV4,
    seconds: u64,
    target_pid: Option<u32>,
    report: &mut VerifyReport,
) -> anyhow::Result<()> {
    let game = crate::games::resolve(game_key)?;
    let config = match target_pid {
        Some(pid) => {
            let names: Vec<&str> = report.process_names.iter().map(String::as_str).collect();
            let routes = process_scanner::scan_for_games(&names)
                .into_iter()
                .find(|process| process.pid == pid)
                .map(|process| process.routes)
                .unwrap_or_default();
            let target = crate::interceptor::ProcessTarget::by_pid(pid);
            println!("  live: targeting {}", target.describe());
            crate::interceptor::build_config_for_process_target(
                game.as_ref(),
                target,
                routes,
                proxy,
                false,
                4,
                AdaptiveConfig::default(),
            )
        }
        None => {
            report
                .notes
                .push("no process target could be resolved; verifying the port range path".into());
            build_config_for_game(game.as_ref(), proxy, false, 4, AdaptiveConfig::default())
                .ok_or_else(|| {
                    anyhow::anyhow!("could not build an interceptor config for this game")
                })?
        }
    };
    let interceptor = create_interceptor();
    interceptor
        .check_availability()
        .map_err(|error| anyhow::anyhow!(error))?;
    let platform = interceptor.platform_name().to_string();

    crate::quic::register_session(proxy, 4433).await;
    crate::session::set_current_proxy(proxy);

    let mut handle = interceptor.start(config)?;
    println!("  live: {platform} running for {seconds}s against {proxy}");
    let mut live = LiveReport {
        seconds,
        platform,
        ..Default::default()
    };
    let mut last_server = None;
    for _ in 0..seconds {
        tokio::time::sleep(Duration::from_secs(1)).await;
        let stats = handle.snapshot();
        live.packets_seen = stats.packets_seen;
        live.packets_intercepted = stats.packets_intercepted;
        live.packets_tunnel_sent = stats.packets_tunnel_sent;
        live.packets_injected = stats.packets_injected;
        live.packets_from_proxy = stats.packets_from_proxy;
        live.errors = stats.errors;
        live.capture_diagnosis = stats.capture_diagnosis.clone();
        live.tunnelling_enabled = stats.tunnelling_enabled;
        if let Some(server) = stats.detected_server {
            let text = server.to_string();
            if last_server.as_deref() != Some(text.as_str()) {
                println!("  live: locked game server {text}");
                last_server = Some(text);
            }
            live.detected_server = last_server.clone();
        }
        if live.packets_tunnel_sent > 0 && live.packets_injected > 0 {
            break;
        }
    }
    if !handle.stop_and_wait(Duration::from_secs(3)) {
        report
            .notes
            .push("interceptor teardown did not acknowledge within 3s".into());
    }

    println!(
        "  live: seen {} | relayed {} | sent {} | injected {} | errors {}",
        live.packets_seen,
        live.packets_intercepted,
        live.packets_tunnel_sent,
        live.packets_injected,
        live.errors
    );

    if live.packets_seen == 0 {
        report
            .problems
            .push("capture saw no packets at all; the filter matches nothing".into());
    } else if live.packets_intercepted == 0 {
        report.problems.push(
            "packets were captured but none was attributed to this game; either it is idle or the profile is wrong"
                .into(),
        );
    } else if live.packets_tunnel_sent == 0 {
        report
            .problems
            .push("traffic was selected for relay but no datagram left for the relay".into());
    }
    if live.detected_server.is_none() {
        report
            .notes
            .push("no game server was locked during the run".into());
    } else if let Some(server) = live
        .detected_server
        .as_deref()
        .and_then(|text| text.parse::<SocketAddrV4>().ok())
    {
        let (low, high) = report.ports;
        if server.port() < low || server.port() > high {
            report.problems.push(format!(
                "the locked game server {server} is outside the profile range {:?}",
                report.ports
            ));
        }
    }
    if live.packets_injected == 0 && live.packets_tunnel_sent > 0 {
        report
            .notes
            .push("no reply was injected yet; the game may not have answered".into());
    }
    if let Some(diagnosis) = &live.capture_diagnosis {
        report
            .problems
            .push(format!("capture reported: {diagnosis}"));
    }
    report.live = Some(live);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{verdict_from, Verdict};

    #[test]
    fn a_static_clean_run_is_inconclusive_not_a_pass() {
        assert_eq!(verdict_from(&[], false), Verdict::Inconclusive);
    }

    #[test]
    fn a_clean_live_run_is_the_only_pass() {
        assert_eq!(verdict_from(&[], true), Verdict::Pass);
    }

    #[test]
    fn any_problem_fails_even_with_live_counters() {
        assert_eq!(
            verdict_from(&["capture saw no packets".into()], true),
            Verdict::Fail
        );
    }
}
