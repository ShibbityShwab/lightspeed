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
use lightspeed_proxy::budget;
use lightspeed_proxy::config;
use lightspeed_proxy::geo;
use lightspeed_proxy::handoff;
use lightspeed_proxy::health;
use lightspeed_proxy::metrics;
use lightspeed_proxy::notify;
use lightspeed_proxy::rate_limit;
use lightspeed_proxy::relay;

#[cfg(feature = "quic")]
use lightspeed_proxy::control;

use std::net::Ipv4Addr;
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;
#[cfg(target_os = "linux")]
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use clap::Parser;
use tokio::net::UdpSocket;
use tokio::sync::RwLock;
use tracing::{info, warn};

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

    /// Print the handoff manifest schema version and exit.
    #[arg(long, default_value_t = false)]
    handoff_schema: bool,

    /// Validate a handoff manifest (schema, self version/sha, fds) and exit.
    #[arg(long, value_name = "MANIFEST_PATH")]
    handoff_validate: Option<String>,
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

/// Validate a handoff manifest without binding or serving: schema, that the
/// manifest targets this exact binary (version + sha256), and every fd.
fn run_handoff_validate(manifest_path: &str) -> anyhow::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let manifest = handoff::read_manifest(std::path::Path::new(manifest_path))?;
        let self_sha = handoff::sha256_file(std::path::Path::new("/proc/self/exe"))?;
        handoff::check_adoption_identity(&manifest, env!("CARGO_PKG_VERSION"), &self_sha)
            .map_err(|e| anyhow::anyhow!("adoption identity check failed: {e}"))?;
        handoff::validate_manifest_fds(&manifest)
            .map_err(|e| anyhow::anyhow!("fd validation failed: {e}"))?;
        println!("handoff manifest {manifest_path} is valid");
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = manifest_path;
        anyhow::bail!("handoff validation is only supported on Linux")
    }
}

/// Build the geo resolver from configuration.
///
/// Returns `None` (geo off) when disabled or when the database is missing or
/// invalid; a bad database never prevents startup.
fn build_geo_resolver(config: &config::ProxyConfig) -> Option<Arc<dyn geo::GeoResolver>> {
    if !config.geo.enabled {
        warn!("Geo aggregation disabled by configuration");
        return None;
    }
    match geo::load_resolver(std::path::Path::new(&config.geo.mmdb_path)) {
        Some(resolver) => {
            info!("Geo aggregation enabled (mmdb: {})", config.geo.mmdb_path);
            Some(resolver)
        }
        None => {
            warn!(
                "Geo aggregation enabled but MMDB at {} is unavailable; geo disabled",
                config.geo.mmdb_path
            );
            None
        }
    }
}

/// Parse the port from a `host:port` bind string, falling back to `default`.
fn bind_port(bind: &str, default: u16) -> u16 {
    bind.rsplit_once(':')
        .and_then(|(_, port)| port.parse().ok())
        .unwrap_or(default)
}

/// Parse `server.public_ip`, warning and ignoring an invalid value.
fn parse_public_ip(raw: Option<&str>) -> Option<Ipv4Addr> {
    let raw = raw?;
    match raw.parse() {
        Ok(ip) => Some(ip),
        Err(error) => {
            warn!("Invalid server.public_ip {raw:?}: {error}");
            None
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    // Handoff-only modes are side-effect free: they print or validate and exit
    // before any logging or binding, so they never serve.
    if cli.handoff_schema {
        println!("{}", handoff::HANDOFF_SCHEMA_VERSION);
        return Ok(());
    }
    if let Some(manifest_path) = cli.handoff_validate.as_deref() {
        return run_handoff_validate(manifest_path);
    }

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
    let mut config = config::ProxyConfig::load(&cli.config).unwrap_or_else(|e| {
        tracing::warn!("Config not found ({}), using defaults", e);
        config::ProxyConfig::default()
    });
    config.apply_env_overrides();
    let geo_resolver = build_geo_resolver(&config);

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
    let budget_guard = if config.budget.enabled {
        let guard = Arc::new(budget::BudgetGuard::new(config.budget.clone()));
        if guard.is_enabled() {
            info!(
                max_bytes = config.budget.max_bytes,
                monthly_bytes = config.budget.monthly_bytes,
                soft_threshold_pct = config.budget.soft_threshold_pct,
                hard_threshold_pct = config.budget.hard_threshold_pct,
                "Egress budget guard enabled"
            );
        } else {
            warn!("[budget] enabled but no max_bytes or monthly_bytes set; guard is inert");
        }
        Some(guard)
    } else {
        info!("Egress budget guard disabled");
        None
    };
    let public_ip = parse_public_ip(config.server.public_ip.as_deref());
    if geo_resolver.is_some() && public_ip.is_none() {
        warn!(
            "Geo aggregation is enabled but server.public_ip is unset; self-tunnel \
             sessions are filtered only by the control port, so geo counts may include \
             relay-to-self artifacts. Set [server] public_ip for exact filtering."
        );
    }
    let geo_state = geo_resolver.map(|resolver| relay::GeoState {
        resolver,
        metrics: Arc::clone(&metrics),
        public_ip,
        control_port: bind_port(&control_bind, config.network.control_port),
        data_port: bind_port(&data_bind, config.network.data_port),
    });
    #[cfg(target_os = "linux")]
    let proxy_started_at_unix_ms = handoff::now_unix_ms();

    // Data plane, auth table, and engine: adopt them from a handoff manifest
    // when the environment requests it, otherwise start fresh and bind the data
    // socket. Adoption is env-only so a plain systemd restart never adopts.
    let (authenticator, engine, data_socket) = match handoff::manifest_path_from_env() {
        Some(path) => {
            info!("Adopting in-place handoff from {}", path.display());
            adopt_handoff(
                &path,
                &config,
                &metrics,
                geo_state.clone(),
                budget_guard.clone(),
            )
            .await
            .map_err(|e| {
                anyhow::anyhow!("handoff adoption from {} failed: {e:#}", path.display())
            })?
        }
        None => {
            let authenticator = Arc::new(RwLock::new(auth::Authenticator::new(
                config.security.require_auth,
            )));
            let mut relay_engine = relay::RelayEngine::new(config.server.max_clients)
                .with_metrics(Arc::clone(&metrics));
            if let Some(geo) = geo_state.clone() {
                relay_engine = relay_engine.with_geo(geo);
            }
            if let Some(guard) = budget_guard.clone() {
                relay_engine = relay_engine.with_budget(guard);
            }
            let engine = Arc::new(relay_engine);
            // Bind every serving plane before spawning any task, so a bind
            // failure aborts startup instead of leaving a half-started service
            // that has already announced readiness to systemd.
            let data_socket =
                Arc::new(UdpSocket::bind(&data_bind).await.map_err(|e| {
                    anyhow::anyhow!("failed to bind data plane on {data_bind}: {e}")
                })?);
            (authenticator, engine, data_socket)
        }
    };
    info!("Data plane socket bound to {}", data_socket.local_addr()?);

    #[cfg(feature = "quic")]
    let control_server = {
        let control_addr: std::net::SocketAddr = control_bind.parse()?;
        let control_state = Arc::new(control::ControlState::new(
            config.clone(),
            Arc::clone(&authenticator),
            Arc::clone(&metrics),
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
    let mut control_handle = Some(tokio::spawn(control_server.run(control_shutdown_rx)));

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

    #[cfg(unix)]
    let mut sigusr2 =
        tokio::signal::unix::signal(tokio::signal::unix::SignalKind::user_defined2())?;

    // Wait for a shutdown signal, or attempt an in-place handoff on SIGUSR2.
    // A handoff that does not exec leaves this process serving and waits again.
    loop {
        #[cfg(unix)]
        {
            tokio::select! {
                result = wait_for_shutdown_signal() => {
                    result?;
                    break;
                }
                _ = sigusr2.recv() => {
                    info!("SIGUSR2 received: attempting in-place handoff");
                    #[cfg(target_os = "linux")]
                    {
                        attempt_handoff(
                            &engine,
                            &authenticator,
                            &data_socket,
                            proxy_started_at_unix_ms,
                            #[cfg(feature = "quic")]
                            &control_shutdown_tx,
                            #[cfg(feature = "quic")]
                            &mut control_handle,
                        )
                        .await;
                    }
                    #[cfg(not(target_os = "linux"))]
                    {
                        attempt_handoff_unsupported().await;
                    }
                    info!("Handoff attempt finished; proxy continues to serve");
                }
            }
        }
        #[cfg(not(unix))]
        {
            wait_for_shutdown_signal().await?;
            break;
        }
    }
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
    // endpoint. A hung client must not be able to hang the whole shutdown. A
    // handoff attempt may already have consumed and stopped it.
    #[cfg(feature = "quic")]
    if let Some(mut handle) = control_handle.take() {
        match tokio::time::timeout(SHUTDOWN_TIMEOUT, &mut handle).await {
            Ok(Ok(())) => info!("Control plane announced shutdown and closed"),
            Ok(Err(e)) => tracing::warn!("Control plane task failed during shutdown: {}", e),
            Err(_) => {
                tracing::warn!(
                    "Control plane shutdown exceeded {:?}; aborting",
                    SHUTDOWN_TIMEOUT
                );
                handle.abort();
            }
        }
    }

    info!("⚡ LightSpeed Proxy shut down cleanly");
    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
//  In-place handoff
// ─────────────────────────────────────────────────────────────────────────────

/// Adopt a manifest from the previous process instead of binding the data fd.
///
/// The manifest is fully validated (schema, that it targets this exact binary,
/// and every fd) before anything is taken over, so a failure returns an error
/// and exits non-zero without partial serving.
#[cfg(target_os = "linux")]
async fn adopt_handoff(
    manifest_path: &std::path::Path,
    config: &config::ProxyConfig,
    metrics: &Arc<metrics::ProxyMetrics>,
    geo: Option<relay::GeoState>,
    budget_guard: Option<Arc<budget::BudgetGuard>>,
) -> anyhow::Result<(
    Arc<RwLock<auth::Authenticator>>,
    Arc<relay::RelayEngine>,
    Arc<UdpSocket>,
)> {
    let manifest = handoff::read_manifest(manifest_path)?;
    let self_sha = handoff::sha256_file(std::path::Path::new("/proc/self/exe"))?;
    handoff::check_adoption_identity(&manifest, env!("CARGO_PKG_VERSION"), &self_sha)
        .map_err(|e| anyhow::anyhow!("adoption identity check failed: {e}"))?;
    handoff::validate_manifest_fds(&manifest)
        .map_err(|e| anyhow::anyhow!("adopted fd validation failed: {e}"))?;

    let data_socket = Arc::new(UdpSocket::from_std(handoff::adopt_std_udp(
        manifest.data_fd,
    )?)?);

    let authenticator = Arc::new(RwLock::new(auth::Authenticator::new(
        config.security.require_auth,
    )));
    authenticator
        .write()
        .await
        .restore(&manifest.auth, std::time::Instant::now());

    let mut relay_engine =
        relay::RelayEngine::new(config.server.max_clients).with_metrics(Arc::clone(metrics));
    if let Some(geo) = geo {
        relay_engine = relay_engine.with_geo(geo);
    }
    if let Some(guard) = budget_guard {
        relay_engine = relay_engine.with_budget(guard);
    }
    let engine = Arc::new(relay_engine);
    let installed = engine
        .install_handoff_sessions(
            &manifest.sessions,
            Arc::clone(&data_socket),
            Arc::clone(metrics),
        )
        .await;

    handoff::set_cloexec(manifest.data_fd)?;
    // Per-session fds are sealed by install_handoff_sessions as each socket is
    // adopted; snapshots that were skipped had their fd closed there, so they
    // must not be touched here (a stale number could have been reused).

    info!(
        sessions = installed,
        auth_tokens = manifest.auth.len(),
        "Adopted handoff state"
    );
    handoff::record_handoff_status(handoff::HandoffStatus::ok(
        &manifest.handoff_id,
        &manifest.from_version,
        &manifest.to_version,
        installed as u64,
        handoff::now_unix_ms(),
    ));

    Ok((authenticator, engine, data_socket))
}

/// Handoff adoption is a Linux-only `execve` capability.
#[cfg(not(target_os = "linux"))]
async fn adopt_handoff(
    manifest_path: &std::path::Path,
    _config: &config::ProxyConfig,
    _metrics: &Arc<metrics::ProxyMetrics>,
    _geo: Option<relay::GeoState>,
    _budget_guard: Option<Arc<budget::BudgetGuard>>,
) -> anyhow::Result<(
    Arc<RwLock<auth::Authenticator>>,
    Arc<relay::RelayEngine>,
    Arc<UdpSocket>,
)> {
    anyhow::bail!(
        "handoff adoption is only supported on Linux (manifest {})",
        manifest_path.display()
    )
}

/// One handoff attempt: never let a failure end the process.
#[cfg(target_os = "linux")]
async fn attempt_handoff(
    engine: &Arc<relay::RelayEngine>,
    authenticator: &Arc<RwLock<auth::Authenticator>>,
    data_socket: &Arc<UdpSocket>,
    proxy_started_at_unix_ms: u64,
    #[cfg(feature = "quic")] control_shutdown_tx: &tokio::sync::watch::Sender<bool>,
    #[cfg(feature = "quic")] control_handle: &mut Option<tokio::task::JoinHandle<()>>,
) {
    static IN_PROGRESS: AtomicBool = AtomicBool::new(false);
    if IN_PROGRESS
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_err()
    {
        tracing::warn!("handoff already in progress; ignoring SIGUSR2");
        return;
    }

    #[cfg(feature = "quic")]
    let outcome = {
        run_handoff_sequence(
            engine,
            authenticator,
            data_socket,
            proxy_started_at_unix_ms,
            control_shutdown_tx,
            control_handle,
        )
        .await
    };
    #[cfg(not(feature = "quic"))]
    let outcome = {
        run_handoff_sequence(engine, authenticator, data_socket, proxy_started_at_unix_ms).await
    };

    if let Err(e) = outcome {
        tracing::warn!(error = %e, "handoff attempt failed; continuing to serve");
    }
    engine.unfreeze_handoff();
    IN_PROGRESS.store(false, Ordering::Release);
}

/// SIGUSR2 handoff is unavailable off Linux; record a rejection, keep serving.
#[cfg(all(unix, not(target_os = "linux")))]
async fn attempt_handoff_unsupported() {
    handoff::record_handoff_status(handoff::HandoffStatus::rejected(
        "in-place handoff is only supported on Linux",
        handoff::now_unix_ms(),
    ));
}

#[cfg(target_os = "linux")]
fn record_rejected(context: &str, detail: &str, req: Option<&handoff::HandoffRequest>) {
    let mut status =
        handoff::HandoffStatus::rejected(format!("{context}: {detail}"), handoff::now_unix_ms());
    if let Some(req) = req {
        status = status.with_request(req);
    }
    handoff::record_handoff_status(status);
}

#[cfg(target_os = "linux")]
fn record_failed(context: &str, detail: &str, manifest: &handoff::HandoffManifest) {
    handoff::record_handoff_status(
        handoff::HandoffStatus::failed(format!("{context}: {detail}"), handoff::now_unix_ms())
            .with_manifest(manifest),
    );
}

/// Stage one attempt: validate the request, snapshot, write the manifest, then
/// pre-validate and exec the new binary. It only returns on failure, having
/// restored every staged fd and recorded the result.
#[cfg(target_os = "linux")]
async fn run_handoff_sequence(
    engine: &Arc<relay::RelayEngine>,
    authenticator: &Arc<RwLock<auth::Authenticator>>,
    data_socket: &Arc<UdpSocket>,
    proxy_started_at_unix_ms: u64,
    #[cfg(feature = "quic")] control_shutdown_tx: &tokio::sync::watch::Sender<bool>,
    #[cfg(feature = "quic")] control_handle: &mut Option<tokio::task::JoinHandle<()>>,
) -> anyhow::Result<()> {
    // (a) Read and validate the root-owned request.
    let req = match handoff::read_request(std::path::Path::new(handoff::DEFAULT_REQUEST_PATH)) {
        Ok(req) => req,
        Err(e) => {
            record_rejected("cannot read handoff request", &format!("{e:#}"), None);
            anyhow::bail!("cannot read handoff request: {e:#}");
        }
    };
    if let Err(e) =
        handoff::validate_request(&req, &handoff::releases_root(), handoff::now_unix_ms())
    {
        record_rejected("handoff request rejected", &e, Some(&req));
        anyhow::bail!("handoff request rejected: {e}");
    }
    if req.version == env!("CARGO_PKG_VERSION") {
        let detail = format!("request version {} equals the running version", req.version);
        record_rejected("handoff request rejected", &detail, Some(&req));
        anyhow::bail!("{detail}");
    }

    // (a2) Bind every check to the exact inode that will be executed. The fd
    // stays open (FD_CLOEXEC cleared below) and both the pre-validator and the
    // final exec address the target through /proc/self/fd/<fd>, so a path swap
    // between hashing and exec cannot change what runs. A failure here is a
    // refusal: never fall back to the raw path.
    let (binary_file, verified_sha) = match handoff::open_verified_binary(&req) {
        Ok(verified) => verified,
        Err(e) => {
            record_rejected("handoff request rejected", &e, Some(&req));
            anyhow::bail!("handoff request rejected: {e}");
        }
    };
    let binary_fd = binary_file.as_raw_fd();
    let exec_path = format!("/proc/self/fd/{binary_fd}");
    if let Err(e) = std::fs::metadata(&exec_path) {
        record_rejected(
            "handoff request rejected",
            &format!("/proc/self/fd is unusable: {e}"),
            Some(&req),
        );
        anyhow::bail!("cannot execute via {exec_path}: {e}");
    }

    // (b) Freeze new-session creation for the millisecond snapshot window.
    engine.freeze_handoff();

    // (c) Snapshot sessions and auth, then build the manifest.
    let snapshotter = Arc::clone(engine);
    let snapshots = tokio::task::spawn_blocking(move || {
        snapshotter.snapshot_handoff(std::time::Instant::now())
    })
    .await
    .map_err(|e| anyhow::anyhow!("handoff snapshot task failed: {e}"))?;
    let auth = authenticator
        .read()
        .await
        .snapshot(std::time::Instant::now());

    let manifest = handoff::HandoffManifest {
        schema_version: handoff::HANDOFF_SCHEMA_VERSION,
        handoff_id: req.handoff_id.clone(),
        from_version: env!("CARGO_PKG_VERSION").to_string(),
        to_version: req.version.clone(),
        to_sha256: verified_sha.clone(),
        created_at_unix_ms: handoff::now_unix_ms(),
        data_fd: data_socket.as_raw_fd(),
        tcp_fd: None,
        proxy_started_at_unix_ms,
        sessions: snapshots,
        auth,
    };

    // (d) Write the manifest, then stage every fd so the child inherits it.
    let manifest_path = handoff::manifest_write_path();
    if let Err(e) = handoff::write_json_atomic(&manifest_path, &manifest) {
        record_failed(
            "cannot write handoff manifest",
            &format!("{e:#}"),
            &manifest,
        );
        anyhow::bail!("cannot write handoff manifest: {e:#}");
    }

    let fds: Vec<std::os::fd::RawFd> = std::iter::once(manifest.data_fd)
        .chain(manifest.sessions.iter().map(|s| s.outbound_fd))
        .chain(std::iter::once(binary_fd))
        .collect();
    if let Err(e) = stage_fds_for_exec(&fds) {
        record_failed("cannot stage fds for exec", &format!("{e:#}"), &manifest);
        anyhow::bail!("cannot stage fds for exec: {e:#}");
    }

    // (e) Pre-validate the target, through the verified fd, in a child that
    // inherits the staged fds.
    let validate_target = exec_path.clone();
    let validate_path = manifest_path.clone();
    let joined = tokio::task::spawn_blocking(move || {
        run_prevalidate(
            &validate_target,
            &validate_path,
            std::time::Duration::from_secs(3),
        )
    })
    .await;
    let validated = match joined {
        Ok(result) => result,
        Err(e) => {
            restore_cloexec(&fds);
            record_failed("handoff validator task failed", &e.to_string(), &manifest);
            anyhow::bail!("handoff validator task failed: {e}");
        }
    };
    if let Err(e) = validated {
        restore_cloexec(&fds);
        record_failed(
            "handoff pre-validation failed",
            &format!("{e:#}"),
            &manifest,
        );
        anyhow::bail!("handoff pre-validation failed: {e:#}");
    }

    // Seal the verified binary fd before the final exec. execve of
    // /proc/self/fd/<fd> resolves the fd while it is still open, and FD_CLOEXEC
    // only closes it in the new image, so this works and stops the adopted
    // process from inheriting (and leaking) a descriptor to its own binary.
    let _ = handoff::set_cloexec(binary_fd);

    // (f) Announce control-plane shutdown, free the ports, then exec in place.
    #[cfg(feature = "quic")]
    {
        let _ = control_shutdown_tx.send(true);
        if let Some(handle) = control_handle.as_mut() {
            let _ = tokio::time::timeout(std::time::Duration::from_secs(2), handle).await;
        }
        *control_handle = None;
    }

    let argv = handoff_argv();
    let mut command = std::process::Command::new(&exec_path);
    if argv.len() > 1 {
        command.args(&argv[1..]);
    }
    command.arg0(&req.binary_path);
    command.env(handoff::MANIFEST_ENV, &manifest_path);
    let exec_error = command.exec();

    restore_cloexec(&fds);
    record_failed(
        "exec of the handoff target failed",
        &exec_error.to_string(),
        &manifest,
    );
    // The control plane has already been shut down, so this process can no
    // longer serve. Exit non-zero (after the status is recorded) so systemd
    // restarts the still-current old release instead of leaving a process with
    // a dead control plane running.
    std::process::exit(1);
}

/// Clear `FD_CLOEXEC` on every fd and verify it took; restore all on failure.
#[cfg(target_os = "linux")]
fn stage_fds_for_exec(fds: &[std::os::fd::RawFd]) -> anyhow::Result<()> {
    let mut cleared = Vec::with_capacity(fds.len());
    for &fd in fds {
        if let Err(e) = handoff::clear_cloexec(fd) {
            restore_cloexec(&cleared);
            return Err(anyhow::anyhow!("cannot clear FD_CLOEXEC on fd {fd}: {e}"));
        }
        cleared.push(fd);
    }
    for &fd in fds {
        match handoff::is_cloexec(fd) {
            Ok(false) => {}
            Ok(true) => {
                restore_cloexec(&cleared);
                return Err(anyhow::anyhow!("fd {fd} is still FD_CLOEXEC after staging"));
            }
            Err(e) => {
                restore_cloexec(&cleared);
                return Err(anyhow::anyhow!("cannot verify FD_CLOEXEC on fd {fd}: {e}"));
            }
        }
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn restore_cloexec(fds: &[std::os::fd::RawFd]) {
    for &fd in fds {
        let _ = handoff::set_cloexec(fd);
    }
}

/// Run `binary --handoff-validate <manifest>` under a hard timeout.
#[cfg(target_os = "linux")]
fn run_prevalidate(
    binary: &str,
    manifest_path: &std::path::Path,
    timeout: std::time::Duration,
) -> anyhow::Result<()> {
    let mut child = std::process::Command::new(binary)
        .arg("--handoff-validate")
        .arg(manifest_path)
        .env_remove(handoff::MANIFEST_ENV)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::inherit())
        .spawn()
        .map_err(|e| anyhow::anyhow!("cannot spawn handoff validator: {e}"))?;

    let started = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => anyhow::bail!("handoff validator exited with {status}"),
            Ok(None) if started.elapsed() >= timeout => {
                let _ = child.kill();
                let _ = child.wait();
                anyhow::bail!("handoff validator exceeded {timeout:?}");
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(10)),
            Err(e) => anyhow::bail!("cannot wait for handoff validator: {e}"),
        }
    }
}

/// The original argv with the handoff-only flags removed.
#[cfg(target_os = "linux")]
fn handoff_argv() -> Vec<std::ffi::OsString> {
    let mut args = std::env::args_os();
    let mut argv = Vec::new();
    if let Some(program) = args.next() {
        argv.push(program);
    }
    let mut skip_value = false;
    for arg in args {
        if skip_value {
            skip_value = false;
            continue;
        }
        let text = arg.to_string_lossy();
        if text == "--handoff-schema" {
            continue;
        }
        if text == "--handoff-validate" {
            skip_value = true;
            continue;
        }
        if text.starts_with("--handoff-validate=") {
            continue;
        }
        argv.push(arg);
    }
    argv
}
