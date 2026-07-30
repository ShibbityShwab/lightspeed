//! `--watch` mode: wait for game, then auto-start interceptor.

use std::net::SocketAddrV4;
use std::time::Duration;
use tracing::info;

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

    // State: Polling → Intercepting → Polling
    enum State { Polling, Intercepting }
    let mut _state = State::Polling;

    loop {
        if let State::Polling = _state {
            match crate::interceptor::process_scanner::find_game_process(&process_refs) {
                Some(p) => {
                    info!("🎮 {} detected! PID {} with {} routes", game_name, p.pid, p.routes.len());

                    let mut config = crate::interceptor::build_config_for_game(
                        game.as_ref(), proxy_addr, fec, fec_k,
                    ).unwrap_or(crate::interceptor::InterceptorConfig {
                        game_name: game_name.clone(), pid: Some(p.pid),
                        port_range: (lo, hi), initial_routes: vec![],
                        proxy_addr, fec_enabled: fec, fec_k,
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
                        Ok(mut handle) => {
                            info!("✅ Interceptor active — optimizing {}\n", game_name);
                            _state = State::Intercepting;

                            // Monitor until game exits
                            loop {
                                tokio::time::sleep(Duration::from_secs(3)).await;
                                if crate::interceptor::process_scanner::find_game_process(&process_refs).is_none() {
                                    info!("👋 {} exited — stopping interceptor\n", game_name);
                                    handle.stop();
                                    _state = State::Polling;
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            info!("❌ Interceptor start failed: {}\n", e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
                None => {
                    tokio::time::sleep(Duration::from_secs(2)).await;
                }
            }
        }
    }
}
