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

    /// Developer mode: skip destination IP validation.
    #[arg(long, default_value_t = false)]
    dev: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // ── --check mode ────────────────────────────────────────────
    if cli.check {
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
        let health_bind = cli
            .health_bind
            .clone()
            .unwrap_or_else(|| format!("0.0.0.0:{}", cfg.network.health_port));

        // 2. Bind check
        print!("   UDP data port ({})... ", data_bind);
        match std::net::UdpSocket::bind(&data_bind) {
            Ok(_) => println!("✅ bindable"),
            Err(e) => {
                println!("❌ {}", e);
                std::process::exit(1);
            }
        }

        print!("   HTTP health port ({})... ", health_bind);
        match std::net::TcpListener::bind(&health_bind) {
            Ok(_) => println!("✅ bindable"),
            Err(e) => {
                println!("❌ {}", e);
                std::process::exit(1);
            }
        }

        println!();
        println!("✅ All checks passed — proxy is ready to start");
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

    // Initialize shared state
    let authenticator = Arc::new(RwLock::new(auth::Authenticator::new(
        config.security.require_auth,
    )));

    let abuse_config = abuse::AbuseConfig {
        dev_mode: cli.dev,
        max_amplification_ratio: config.security.max_amplification_ratio,
        max_destinations_per_window: config.security.max_destinations_per_window,
        ban_duration_secs: config.security.ban_duration_secs,
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

    // Bind data plane UDP socket
    let data_socket = Arc::new(UdpSocket::bind(&data_bind).await?);
    info!("Data plane socket bound to {}", data_socket.local_addr()?);

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
        let data_socket = Arc::clone(&data_socket);
        let abuse_detector = Arc::clone(&abuse_detector);
        let metrics = Arc::clone(&metrics);
        tokio::spawn(async move {
            relay::run_session_manager(engine, data_socket, abuse_detector, metrics).await;
        })
    };

    // Spawn QUIC control plane server (if compiled with --features quic)
    #[cfg(feature = "quic")]
    let control_handle = {
        let control_addr: std::net::SocketAddr = control_bind.parse()?;
        let control_state = Arc::new(control::ControlState::new(
            config.clone(),
            Arc::clone(&authenticator),
        ));
        tokio::spawn(async move {
            if let Err(e) = control::run_control_server(control_addr, control_state).await {
                tracing::error!("QUIC control plane failed: {}", e);
            }
        })
    };

    #[cfg(not(feature = "quic"))]
    info!("QUIC control plane disabled (compile with --features quic)");

    // Spawn health check HTTP server
    let health_handle = {
        let metrics = Arc::clone(&metrics);
        let engine = Arc::clone(&engine);
        let region = config.server.region.clone();
        let node_id = config.server.node_id.clone();
        let health_bind = health_bind.clone();
        let start_time = std::time::Instant::now();
        tokio::spawn(async move {
            if let Err(e) =
                health::run_health_server(health_bind, metrics, engine, region, node_id, start_time)
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

    info!("⚡ LightSpeed Proxy running — press Ctrl+C to stop");

    // Wait for shutdown signal
    tokio::signal::ctrl_c().await?;
    info!("⚡ Shutdown signal received");

    // Abort background tasks
    relay_handle.abort();
    manager_handle.abort();
    health_handle.abort();
    stats_handle.abort();
    #[cfg(feature = "quic")]
    control_handle.abort();

    info!("⚡ LightSpeed Proxy shut down cleanly");
    Ok(())
}
