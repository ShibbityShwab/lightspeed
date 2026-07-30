//! `--watch` mode: wait for game, then auto-start interceptor.
//!
//! Polls ProcessScanner until the target game is detected with an active
//! server connection, then starts the interceptor. If the game exits,
//! stops the interceptor and resumes watching. Ctrl+C to quit.

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
    info!("   Press Ctrl+C to stop");
    info!("");

    let mut was_running = false;

    loop {
        // Check for game process
        let found = crate::interceptor::process_scanner::find_game_process(&process_refs);

        match found {
            Some(p) if !was_running => {
                info!("🎮 {} detected! PID {} with {} routes", game_name, p.pid, p.routes.len());
                was_running = true;

                // Build config with discovered routes (or server override)
                let mut config = crate::interceptor::build_config_for_game(
                    game.as_ref(), proxy_addr, fec, fec_k,
                ).unwrap_or(crate::interceptor::InterceptorConfig {
                    game_name: game_name.clone(),
                    pid: Some(p.pid),
                    port_range: (lo, hi),
                    initial_routes: vec![],
                    proxy_addr, fec_enabled: fec, fec_k,
                });

                // Apply server override if provided
                if let Some(addr) = server_addr {
                    config.initial_routes.push(crate::interceptor::Route {
                        local: SocketAddrV4::new(std::net::Ipv4Addr::UNSPECIFIED, 0),
                        remote: addr,
                        proto: crate::interceptor::TransportProtocol::Udp,
                    });
                }

                // Start interceptor
                let interceptor = crate::interceptor::create_interceptor();
                match interceptor.check_availability() {
                    Ok(()) => {
                        match interceptor.start(config) {
                            Ok(handle) => {
                                info!("✅ Interceptor active — optimizing {}", game_name);
                                // Keep handle alive until game exits
                                let mut interceptor_handle = Some(handle);

                                // Monitor — check if game is still running
                                loop {
                                    tokio::time::sleep(Duration::from_secs(3)).await;
                                    let still_running = crate::interceptor::process_scanner
                                        ::find_game_process(&process_refs).is_some();

                                    if !still_running {
                                        info!("👋 {} exited — stopping interceptor", game_name);
                                        if let Some(mut h) = interceptor_handle.take() {
                                            h.stop();
                                        }
                                        was_running = false;
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                info!("❌ Interceptor start failed: {}", e);
                                was_running = false;
                            }
                        }
                    }
                    Err(e) => {
                        info!("❌ Interceptor unavailable: {}", e);
                        return Err(anyhow::anyhow!("{}", e));
                    }
                }
            }
            Some(_) => {
                // Game still running — just wait
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
            None => {
                if was_running {
                was_running = false;
                    was_running = false;
                }
                tokio::time::sleep(Duration::from_secs(2)).await;
            }
        }
    }
}
