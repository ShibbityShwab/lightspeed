//! # LightSpeed Proxy Server
//!
//! Receives tunneled UDP packets from clients, strips the LightSpeed header,
//! forwards original packets to game servers, captures responses, re-wraps
//! them, and returns to the client.
//!
//! ## Security
//! - Token-based authentication (QUIC registration → data-plane auth)
//! - Destination IP validation (blocks private/internal IPs)
//! - Abuse detection (amplification + reflection)
//! - Per-client rate limiting
//!
//! Designed to run on any small Linux VPS — current mesh uses Vultr vc2-1c-1gb (~500KB RAM).

use lightspeed_proxy::abuse;
use lightspeed_proxy::auth;
use lightspeed_proxy::config;
use lightspeed_proxy::health;
use lightspeed_proxy::metrics;
use lightspeed_proxy::notify;
use lightspeed_proxy::rate_limit;
use lightspeed_proxy::relay;

#[cfg(feature = "quic")]
use lightspeed_proxy::control;

use std::sync::Arc;

use clap::Parser;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::info;

/// LightSpeed Proxy — UDP relay node
#[derive(Parser, Debug)]
#[command(name = "lightspeed-proxy", version, about)]
struct Cli {
    /// Path to configuration file
    #[arg(short, long, default_value = "proxy.toml")]
    config: String,

    /// UDP data plane bind address (overrides [network] data_port)
    #[arg(long)]
    data_bind: Option<String>,

    /// QUIC control plane bind address (overrides [network] control_port)
    #[arg(long)]
    control_bind: Option<String>,

    /// Health check HTTP bind address (overrides [network] health_port)
    #[arg(long)]
    health_bind: Option<String>,

    /// Enable verbose logging
    #[arg(short, long, default_value_t = false)]
    verbose: bool,

    /// Run configuration and port checks, then exit.
    #[arg(long, default_value_t = false)]
    check: bool,

    /// Like --check, but also bind the data/control/health ports.
    /// Only use while the proxy service is stopped.
    #[arg(long, default_value_t = false)]
    bind_check: bool,

    /// Developer mode: skip destination IP validation.
    #[arg(long, default_value_t = false)]
    dev: bool,
}

/// Maximum time to wait for the control plane to announce shutdown.
#[cfg(feature = "quic")]
const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Wait for SIGINT (Ctrl+C) or, on Unix, SIGTERM.
async fn wait_for_shutdown_signal() -> anyhow::Result<()> {
    #[cfg(unix)]
    {
        let mut terminate =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
        tokio::select! {
            result = tokio::signal::ctrl_c() => result?,
            _ = terminate.recv() => {}
        }
    }
    #[cfg(not(unix))]
    {
        tokio::signal::ctrl_c().await?;
    }
    Ok(())
}

/// Perform the real data/control/health binds for `--bind-check`.
fn bind_all(data: &str, control: &str, health: &str) -> anyhow::Result<()> {
    print!("   UDP data port ({data})... ");
    std::net::UdpSocket::bind(data)?;
    println!("✅ bindable");
    print!("   UDP control port ({control})... ");
    std::net::UdpSocket::bind(control)?;
    println!("✅ bindable");
    print!("   TCP health port ({health})... ");
    std::net::TcpListener::bind(health)?;
    println!("✅ bindable");
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // ── --check / --bind-check mode ─────────────────────────────
    if cli.check || cli.bind_check {
        println!("🔍 LightSpeed Proxy health check");
        println!();

        // 1. Config
        print!("   Config file ({})... ", cli.config);
        let cfg = match config::ProxyConfig::load(&cli.config) {
            Ok(c) => {
                println!("✅ loaded");
                c
            }
            Err(e) => {
                println!("❌ {}", e);
                std::process::exit(1);
            }
        };
        println!("      Node ID:      {}", cfg.server.node_id);
        println!("      Data port:    {}", cfg.network.data_port);
        println!("      Auth enabled: {}", cfg.security.require_auth);
        println!("      Abuse PPS:    {}", cfg.rate_limit.max_pps_per_client);

        let data_bind = cli
            .data_bind
            .clone()
            .unwrap_or_else(|| format!("0.0.0.0:{}", cfg.network.data_port));
        let control_bind = cli
            .control_bind
            .clone()
            .unwrap_or_else(|| format!("0.0.0.0:{}", cfg.network.control_port));
        let health_bind = cli
            .health_bind
            .clone()
            .unwrap_or_else(|| format!("0.0.0.0:{}", cfg.network.health_port));

        // 2. Validate bind addresses without touching the ports.
        for (label, addr) in [
            ("Data", &data_bind),
            ("Control", &control_bind),
            ("Health", &health_bind),
        ] {
            print!("   {label} bind address ({addr})... ");
            if let Err(e) = addr.parse::<std::net::SocketAddr>() {
                println!("❌ {e}");
                std::process::exit(1);
            }
            println!("✅ valid");
        }

        // 3. TLS assets (QUIC only): readability, or first-boot writability.
        #[cfg(feature = "quic")]
        {
            print!("   TLS assets... ");
            if let Err(e) = control::validate_tls_assets() {
                println!("❌ {e}");
                std::process::exit(1);
            }
            println!("✅ readable");
        }

        // 4. Real binds only for --bind-check. Plain --check never binds, so
        //    it stays a valid gate while the service is already running.
        if cli.bind_check {
            if let Err(e) = bind_all(&data_bind, &control_bind, &health_bind) {
                println!("❌ {}", e);
                std::process::exit(1);
            }
        }

        println!();
        if cli.bind_check {
            println!("✅ All checks passed — proxy is ready to start");
        } else {
            println!("✅ All checks passed — config is valid (ports not bound)");
        }
        return Ok(());
    }

    // Initialize tracing
    let filter = if cli.verbose { "debug" } else { "info" };
    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_target(true)
        .init();

    // Load configuration
    let config = config::ProxyConfig::load(&cli.config).unwrap_or_else(|e| {
        tracing::warn!("Config not found ({}), using defaults", e);
        config::ProxyConfig::default()
    });

    let data_bind = cli
        .data_bind
        .clone()
        .unwrap_or_else(|| format!("0.0.0.0:{}", config.network.data_port));
    let control_bind = cli
        .control_bind
        .clone()
        .unwrap_or_else(|| format!("0.0.0.0:{}", config.network.control_port));
    let health_bind = cli
        .health_bind
        .clone()
        .unwrap_or_else(|| format!("0.0.0.0:{}", config.network.health_port));

    info!(
        "⚡ LightSpeed Proxy v{} starting",
        env!("CARGO_PKG_VERSION")
    );
    info!("Data plane:    {}", data_bind);
    info!("Control plane: {}", control_bind);
    info!("Health check:  {}", health_bind);

    info!("Node ID: {}", config.server.node_id);
    info!("Region:  {}", config.server.region);
    info!("Max clients: {}", config.server.max_clients);
    info!(
        "Auth enforcement: {}",
        if config.security.require_auth {
            "ENABLED"
        } else {
            "disabled (dev mode)"
        }
    );

    if config.security.destination_allowlist.is_empty() {
        tracing::warn!(
            "destination_allowlist is empty: this relay will forward to ANY public \
             destination (open proxy). Set a game-server CIDR allowlist for shared relays."
        );
    }

    // Initialize shared state
    let authenticator = Arc::new(RwLock::new(auth::Authenticator::new(
        config.security.require_auth,
    )));

    let abuse_config = abuse::AbuseConfig {
        dev_mode: cli.dev,
        max_amplification_ratio: config.security.max_amplification_ratio,
        max_destinations_per_window: config.security.max_destinations_per_window,
        ban_duration_secs: config.security.ban_duration_secs,
        destination_allowlist: config
            .security
            .destination_allowlist
            .iter()
            .filter_map(|s| abuse::parse_cidr(s))
            .collect(),
        ..Default::default()
    };
    let abuse_detector = Arc::new(tokio::sync::Mutex::new(abuse::AbuseDetector::new(
        abuse_config,
    )));

    let rate_limiter = Arc::new(tokio::sync::Mutex::new(rate_limit::RateLimiter::new(
        config.rate_limit.clone(),
    )));
    let metrics = Arc::new(metrics::ProxyMetrics::new());
    let engine = Arc::new(relay::RelayEngine::new(config.server.max_clients));

    // Bind every serving plane before spawning any task, so a bind failure
    // aborts startup instead of leaving a half-started service that has
    // already announced readiness to systemd.
    let data_socket = Arc::new(
        UdpSocket::bind(&data_bind)
            .await
            .map_err(|e| anyhow::anyhow!("failed to bind data plane on {data_bind}: {e}"))?,
    );
    info!("Data plane socket bound to {}", data_socket.local_addr()?);

    #[cfg(feature = "quic")]
    let control_server = {
        let control_addr: std::net::SocketAddr = control_bind.parse()?;
        let control_state = Arc::new(control::ControlState::new(
            config.clone(),
            Arc::clone(&authenticator),
        ));
        control::ControlServer::bind(control_addr, control_state).map_err(|e| {
            anyhow::anyhow!("failed to bind QUIC control plane on {control_addr}: {e}")
        })?
    };

    let health_listener = tokio::net::TcpListener::bind(&health_bind)
        .await
        .map_err(|e| anyhow::anyhow!("failed to bind health listener on {health_bind}: {e}"))?;
    info!(
        "Health/metrics HTTP server bound to {}",
        health_listener.local_addr()?
    );

    // Spawn the relay inbound loop (client → game server)
    let relay_handle = {
        let data_socket = Arc::clone(&data_socket);
        let engine = Arc::clone(&engine);
        let rate_limiter = Arc::clone(&rate_limiter);
        let authenticator = Arc::clone(&authenticator);
        let abuse_detector = Arc::clone(&abuse_detector);
        let metrics = Arc::clone(&metrics);
        tokio::spawn(async move {
            if let Err(e) = relay::run_relay_inbound(
                data_socket,
                engine,
                rate_limiter,
                authenticator,
                abuse_detector,
                metrics,
            )
            .await
            {
                tracing::error!("Relay inbound loop failed: {}", e);
            }
        })
    };

    // Spawn the session manager (handles response listeners + cleanup)
    let manager_handle = {
        let engine = Arc::clone(&engine);
        let abuse_detector = Arc::clone(&abuse_detector);
        let metrics = Arc::clone(&metrics);
        let rate_limiter = Arc::clone(&rate_limiter);
        let authenticator = Arc::clone(&authenticator);
        tokio::spawn(async move {
            relay::run_session_manager(
                engine,
                abuse_detector,
                metrics,
                rate_limiter,
                authenticator,
            )
            .await;
        })
    };

    // Spawn the TCP inbound listener (client → proxy over TCP, opt-in)
    let tcp_handle = if config.network.tcp_enabled {
        let tcp_bind: std::net::SocketAddr = data_bind.parse()?;
        let engine = Arc::clone(&engine);
        let rate_limiter = Arc::clone(&rate_limiter);
        let authenticator = Arc::clone(&authenticator);
        let abuse_detector = Arc::clone(&abuse_detector);
        let metrics = Arc::clone(&metrics);
        let max_connections = config.network.tcp_max_connections;
        let read_timeout = std::time::Duration::from_secs(config.network.tcp_read_timeout_secs);
        Some(tokio::spawn(async move {
            if let Err(e) = relay::run_tcp_inbound(
                tcp_bind,
                engine,
                rate_limiter,
                authenticator,
                abuse_detector,
                metrics,
                max_connections,
                read_timeout,
            )
            .await
            {
                tracing::error!("TCP inbound loop failed: {}", e);
            }
        }))
    } else {
        info!("TCP inbound listener disabled");
        None
    };

    // Serve the QUIC control plane on the endpoint bound above.
    #[cfg(feature = "quic")]
    let (control_shutdown_tx, control_shutdown_rx) = tokio::sync::watch::channel(false);
    #[cfg(feature = "quic")]
    let mut control_handle = tokio::spawn(control_server.run(control_shutdown_rx));

    #[cfg(not(feature = "quic"))]
    info!("QUIC control plane disabled (compile with --features quic)");

    // Serve health/metrics on the listener bound above.
    let health_handle = {
        let metrics = Arc::clone(&metrics);
        let engine = Arc::clone(&engine);
        let region = config.server.region.clone();
        let node_id = config.server.node_id.clone();
        let start_time = std::time::Instant::now();
        tokio::spawn(async move {
            if let Err(e) = health::run_health_server(
                health_listener,
                metrics,
                engine,
                region,
                node_id,
                start_time,
            )
            .await
            {
                tracing::error!("Health check server failed: {}", e);
            }
        })
    };

    // Spawn periodic stats logger
    let stats_handle = {
        let metrics = Arc::clone(&metrics);
        let engine = Arc::clone(&engine);
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
            loop {
                interval.tick().await;
                let sessions = engine.active_sessions().await;
                let relayed = metrics
                    .packets_relayed
                    .load(std::sync::atomic::Ordering::Relaxed);
                let dropped = metrics
                    .packets_dropped
                    .load(std::sync::atomic::Ordering::Relaxed);
                let bytes = metrics
                    .bytes_relayed
                    .load(std::sync::atomic::Ordering::Relaxed);
                info!(
                    sessions = sessions,
                    packets_relayed = relayed,
                    packets_dropped = dropped,
                    bytes_relayed = bytes,
                    avg_latency_us = format!("{:.1}", metrics.avg_latency_us()),
                    "📊 Proxy stats"
                );
            }
        })
    };

    // systemd readiness: every plane and the session manager are now spawned,
    // so READY means the proxy is serving, not merely configured. This is
    // additive and never alters an existing startup failure path.
    notify::ready();
    let data_local = data_socket
        .local_addr()
        .map(|addr| addr.to_string())
        .unwrap_or_else(|_| data_bind.clone());
    notify::status(&format!(
        "node={} region={} data={} control={} health={}",
        config.server.node_id, config.server.region, data_local, control_bind, health_bind
    ));

    // Refresh the systemd watchdog at half of WatchdogSec. `tokio::time::interval`
    // fires its first tick immediately, so consume it before the loop to keep the
    // first ping one interval after startup.
    let watchdog_handle = if notify::is_enabled() {
        notify::watchdog_interval().map(|interval| {
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                ticker.tick().await;
                loop {
                    ticker.tick().await;
                    notify::watchdog();
                }
            })
        })
    } else {
        None
    };

    info!("⚡ LightSpeed Proxy running — press Ctrl+C to stop");

    // Wait for shutdown signal
    wait_for_shutdown_signal().await?;
    info!("⚡ Shutdown signal received");

    // Tell systemd we are shutting down before any task is aborted.
    notify::stopping();

    // Tell control-plane clients first: clients reconnect immediately instead
    // of waiting out the QUIC idle timeout.
    #[cfg(feature = "quic")]
    let _ = control_shutdown_tx.send(true);

    // Abort background tasks
    relay_handle.abort();
    manager_handle.abort();
    if let Some(handle) = tcp_handle {
        handle.abort();
    }
    health_handle.abort();
    stats_handle.abort();
    if let Some(handle) = watchdog_handle {
        handle.abort();
    }

    // Bounded wait for the control plane to announce shutdown and close its
    // endpoint. A hung client must not be able to hang the whole shutdown.
    #[cfg(feature = "quic")]
    match tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut control_handle).await {
        Ok(Ok(())) => info!("Control plane announced shutdown and closed"),
        Ok(Err(e)) => tracing::warn!("Control plane task failed during shutdown: {}", e),
        Err(_) => {
            tracing::warn!(
                "Control plane shutdown exceeded {:?}; aborting",
                SHUTDOWN_TIMEOUT
            );
            control_handle.abort();
        }
    }

    info!("⚡ LightSpeed Proxy shut down cleanly");
    Ok(())
}
