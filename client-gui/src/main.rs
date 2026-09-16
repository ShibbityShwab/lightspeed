//! LightSpeed GUI — system-tray icon + egui status window.
//!
//! Cross-platform via the `platform` module (Windows tray-icon with
//! `tray_icon`, Linux stub).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod config;
mod discovery;
mod paths;
mod platform;
mod single_instance;
mod update;

use eframe::egui;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

fn main() -> anyhow::Result<()> {
    // Single-instance guard first: a second launch must not start a second
    // engine, tray, or discovery poller. The guard lives until `main` returns
    // (or the process exits via the tray Quit path).
    let _guard = match single_instance::acquire() {
        single_instance::InstanceOutcome::Acquired(guard) => guard,
        single_instance::InstanceOutcome::AlreadyRunning => {
            single_instance::show_already_running_notice();
            return Ok(());
        }
    };

    // Redirect tracing to a file since GUI apps have no console.
    let path = paths::log_file();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).expect("Failed to create log directory");
    }
    let file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .expect("Failed to open log file");
    let file_appender = Mutex::new(file);
    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
        .with_writer(file_appender)
        .init();

    tracing::info!("LightSpeed GUI starting");

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

    // Dedicated multi-thread runtime for the tunnel engine.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ls-engine")
        .build()?;

    let engine = Arc::new(Mutex::new(lightspeed_client::LightSpeedEngine::new(
        rt.handle().clone(),
    )));

    // The tray sets this on "Quit"; the frame loop tears the engine down and
    // exits the process. The window X still hides to the tray on Windows.
    let quit: platform::QuitFlag = Arc::new(AtomicBool::new(false));

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 420.0])
            .with_min_inner_size([340.0, 280.0])
            .with_title("⚡ LightSpeed"),
        ..Default::default()
    };

    let engine_for_closure = Arc::clone(&engine);
    let quit_for_closure = Arc::clone(&quit);
    eframe::run_native(
        "⚡ LightSpeed",
        native_options,
        Box::new(move |_cc: &eframe::CreationContext<'_>| {
            Ok(Box::new(
                app::LightSpeedApp::<platform::CurrentPlatform>::new(
                    Arc::clone(&engine_for_closure),
                    Arc::clone(&quit_for_closure),
                ),
            ))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    engine.lock().unwrap().disconnect();
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(())
}
