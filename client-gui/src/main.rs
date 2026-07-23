//! LightSpeed GUI — system-tray icon + egui status window.
//!
//! Cross-platform via the `platform` module (Windows tray-icon with
//! `tray_icon`, Linux stub).

mod app;
mod platform;

use eframe::egui;
use std::sync::{Arc, Mutex};

fn main() -> anyhow::Result<()> {

    // Redirect tracing to a file since GUI apps have no console.
    // Use a simple file appender for straightforward single-file logging.
    let path = dirs::data_local_dir()
        .unwrap_or_default()
        .join("Lightspeed")
        .join("gui-trace.log");
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

    // Dedicated multi-thread runtime for the tunnel engine.
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .thread_name("ls-engine")
        .build()?;

    let engine = Arc::new(Mutex::new(lightspeed_client::LightSpeedEngine::new(
        rt.handle().clone(),
    )));

    // Connect to the proxy configured via LIGHTSPEED_PROXY env var.
    let proxy_addr =
        std::env::var("LIGHTSPEED_PROXY").unwrap_or_else(|_| "127.0.0.1:4434".to_string());
    let proxy: std::net::SocketAddrV4 = proxy_addr.parse().unwrap();
    engine.lock().unwrap().connect(proxy);

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([460.0, 420.0])
            .with_min_inner_size([340.0, 280.0])
            .with_title("⚡ LightSpeed"),
        ..Default::default()
    };

    let engine_for_closure = Arc::clone(&engine);
    eframe::run_native(
        "⚡ LightSpeed",
        native_options,
        Box::new(move |_cc: &eframe::CreationContext<'_>| {
            Ok(Box::new(app::LightSpeedApp::<platform::CurrentPlatform>::new(
                Arc::clone(&engine_for_closure),
            )))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    engine.lock().unwrap().disconnect();
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(())
}
