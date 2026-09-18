//! `--watch` mode: wait for game, then auto-start interceptor.

use std::net::SocketAddrV4;
use std::time::Duration;
use tracing::{info, warn};

use crate::interceptor::InterceptorHandle;

/// Stop the active interceptor, if any, and wait for its platform owner
/// threads to release their handles.
///
/// On Windows that means both WinDivert owner threads ack (fail-closed on
/// timeout); on Linux/macOS/mock the wait returns immediately. The handle is
/// always taken, so no return path can leave it running.
fn stop_active_interceptor(active: &mut Option<InterceptorHandle>) {
    if let Some(mut handle) = active.take() {
        if !handle.stop_and_wait(Duration::from_secs(3)) {
            warn!(
                "⚠️  Interceptor teardown did not complete within 3s; \
                 platform filter/handles may still be closing"
            );
        }
    }
}

pub async fn run_watch_mode(
    game_key: &str,
    proxy_addr: SocketAddrV4,
    fec: bool,
    fec_k: u8,
    server_addr: Option<SocketAddrV4>,
) -> anyhow::Result<()> {
    let game = crate::games::detect_game(game_key)?;
    let game_name = game.name().to_string();
    let process_names: Vec<String> = game.process_names().iter().map(|s| s.to_string()).collect();
    let (lo, hi) = game.ports();
    let process_refs: Vec<&str> = process_names.iter().map(|s| s.as_str()).collect();

    info!("👀 Watching for {}...", game_name);
    info!("   Processes: {}", process_names.join(", "));
    info!("   Ports:     {}-{}", lo, hi);
    info!("   Press Ctrl+C to stop\n");

    // Registered once so every wait below can race the same signal future;
    // whichever loop is active still tears the interceptor down cleanly.
    let mut ctrl_c = Box::pin(tokio::signal::ctrl_c());

    // State: Polling → Intercepting → Polling
    enum State {
        Polling,
        Intercepting,
    }
    let mut state = State::Polling;
    // The one live handle, kept outside the match arm so every return path and
    // the game-exit path can reach it for teardown.
    let mut active: Option<InterceptorHandle> = None;

    loop {
        match state {
            State::Polling => {
                match crate::interceptor::process_scanner::find_game_process(&process_refs) {
                    Some(p) => {
                        info!(
                            "🎮 {} detected! PID {} with {} routes",
                            game_name,
                            p.pid,
                            p.routes.len()
                        );

                        let mut config = crate::interceptor::build_config_for_game(
                            game.as_ref(),
                            proxy_addr,
                            fec,
                            fec_k,
                        )
                        .unwrap_or(crate::interceptor::InterceptorConfig {
                            game_name: game_name.clone(),
                            pid: Some(p.pid),
                            port_range: (lo, hi),
                            initial_routes: vec![],
                            proxy_addr,
                            fec_enabled: fec,
                            fec_k,
                        });

                        if let Some(addr) = server_addr {
                            config.initial_routes.push(crate::interceptor::Route {
                                local: SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 0),
                                remote: addr,
                                proto: crate::interceptor::TransportProtocol::Udp,
                            });
                        }

                        let interceptor = crate::interceptor::create_interceptor();
                        if let Err(e) = interceptor.check_availability() {
                            info!("❌ Interceptor unavailable: {}", e);
                            return Err(anyhow::anyhow!("{}", e));
                        }

                        match interceptor.start(config) {
                            Ok(handle) => {
                                info!("✅ Interceptor active, optimizing {}\n", game_name);
                                active = Some(handle);
                                state = State::Intercepting;
                            }
                            Err(e) => {
                                info!("❌ Interceptor start failed: {}\n", e);
                                tokio::select! {
                                    _ = &mut ctrl_c => {
                                        info!("🛑 Shutdown requested, stopping");
                                        return Ok(());
                                    }
                                    _ = tokio::time::sleep(Duration::from_secs(5)) => {}
                                }
                            }
                        }
                    }
                    None => {
                        tokio::select! {
                            _ = &mut ctrl_c => {
                                info!("🛑 Shutdown requested, stopping");
                                return Ok(());
                            }
                            _ = tokio::time::sleep(Duration::from_secs(2)) => {}
                        }
                    }
                }
            }
            State::Intercepting => {
                tokio::select! {
                    _ = &mut ctrl_c => {
                        info!("🛑 Shutdown requested, stopping interceptor...");
                        stop_active_interceptor(&mut active);
                        return Ok(());
                    }
                    _ = tokio::time::sleep(Duration::from_secs(3)) => {}
                }

                if crate::interceptor::process_scanner::find_game_process(&process_refs).is_none() {
                    info!("👋 {} exited, stopping interceptor\n", game_name);
                    stop_active_interceptor(&mut active);
                    state = State::Polling;
                }
            }
        }
    }
}
