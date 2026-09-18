//! LightSpeed GUI — system-tray icon + egui status window.
//!
//! Cross-platform via the `platform` module (Windows tray-icon with
//! `tray_icon`, Linux stub).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod config;
mod crash;
mod discovery;
mod paths;
mod platform;
mod single_instance;
mod update;

use eframe::egui;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

fn main() {
    // Absolutely first: a panic before the hook is installed is invisible in
    // the console-less Windows build. `run` returns errors instead of
    // panicking, and those are persisted here too.
    crash::install_panic_hook();
    if let Err(e) = run() {
        crash::record_fatal(&format!("{e:#}"));
        std::process::exit(1);
    }
}

fn run() -> anyhow::Result<()> {
    // Logging is live before the single-instance guard so a guard rejection
    // is explained in the trace log instead of dying silently.
    let log_target = init_logging();
    tracing::info!("LightSpeed GUI starting");
    tracing::info!("panic hook installed");
    tracing::info!("logging ready ({log_target})");

    // A second launch must not start a second engine, tray, or discovery
    // poller. `--force` / `LIGHTSPEED_GUI_FORCE=1` deliberately bypasses it.
    let _guard = match single_instance::acquire() {
        single_instance::InstanceOutcome::Acquired(guard) => {
            tracing::info!("single-instance guard acquired");
            guard
        }
        single_instance::InstanceOutcome::AlreadyRunning => {
            tracing::warn!("Another LightSpeed instance is already running; exiting with code 2");
            single_instance::show_already_running_notice();
            std::process::exit(2);
        }
    };

    // The GUI links both rustls providers (ring through the client's QUIC
    // stack, aws-lc-rs through reqwest/axoupdater), so rustls cannot pick one
    // automatically and panics on the first QUIC control-plane call. Pin one
    // process-wide before any engine task starts.
    if rustls::crypto::ring::default_provider()
        .install_default()
        .is_err()
    {
        tracing::debug!("rustls CryptoProvider was already installed");
    }
    tracing::info!("rustls provider ready");

    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ls-engine")
        .build()?;
    tracing::info!("tokio runtime ready");

    let engine = Arc::new(Mutex::new(lightspeed_client::LightSpeedEngine::new(
        rt.handle().clone(),
    )));
    tracing::info!("engine ready");

    let quit: platform::QuitFlag = Arc::new(AtomicBool::new(false));

    let native_options = eframe::NativeOptions {
        renderer: renderer_from_env(),
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([520.0, 660.0])
            .with_min_inner_size([340.0, 280.0])
            .with_title("⚡ LightSpeed"),
        ..Default::default()
    };

    let engine_for_closure = Arc::clone(&engine);
    let quit_for_closure = Arc::clone(&quit);
    tracing::info!("entering eframe::run_native");
    eframe::run_native(
        "⚡ LightSpeed",
        native_options,
        Box::new(move |_cc: &eframe::CreationContext<'_>| {
            let app = app::LightSpeedApp::<platform::CurrentPlatform>::new(
                Arc::clone(&engine_for_closure),
                Arc::clone(&quit_for_closure),
            );
            tracing::info!("app created");
            Ok(Box::new(app))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;
    tracing::info!("eframe::run_native returned");

    engine.lock().unwrap().disconnect();
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(())
}

/// Initialize tracing without ever panicking.
///
/// Order of preference: the data-dir file, a temp-dir file, then a sink that
/// discards every record. Returns a human-readable description of the sink.
fn init_logging() -> String {
    if let Some((path, file)) = open_log_file() {
        if try_init_writer(Mutex::new(file)) {
            return format!("file {}", path.display());
        }
        return "already initialized".to_string();
    }
    if let Some((path, file)) = open_temp_log_file() {
        if try_init_writer(Mutex::new(file)) {
            return format!("temp file {}", path.display());
        }
        return "already initialized".to_string();
    }
    try_init_writer(std::io::sink);
    "discard (no writable log location)".to_string()
}

fn try_init_writer<W>(writer: W) -> bool
where
    W: for<'a> tracing_subscriber::fmt::MakeWriter<'a> + Send + Sync + 'static,
{
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .with_writer(writer)
        .try_init()
        .is_ok()
}

fn open_log_file() -> Option<(std::path::PathBuf, std::fs::File)> {
    let path = paths::log_file();
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    Some((path, file))
}

fn open_temp_log_file() -> Option<(std::path::PathBuf, std::fs::File)> {
    let path = std::env::temp_dir().join("lightspeed-gui-trace.log");
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok()?;
    Some((path, file))
}

/// Pick the eframe renderer, honoring `LIGHTSPEED_GUI_RENDERER=glow|wgpu`.
///
/// wgpu is the default; glow is the diagnostic escape hatch for machines
/// where wgpu adapter creation fails.
fn renderer_from_env() -> eframe::Renderer {
    match std::env::var("LIGHTSPEED_GUI_RENDERER").as_deref() {
        Ok("glow") => {
            tracing::info!("renderer override: glow");
            eframe::Renderer::Glow
        }
        Ok("wgpu") => {
            tracing::info!("renderer override: wgpu");
            eframe::Renderer::Wgpu
        }
        Ok(other) => {
            tracing::warn!("ignoring unknown LIGHTSPEED_GUI_RENDERER={other:?}; using default");
            eframe::Renderer::default()
        }
        Err(_) => eframe::Renderer::default(),
    }
}
