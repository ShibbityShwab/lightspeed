//! LightSpeed GUI — Windows tray icon + egui status window.
//!
//! On non-Windows platforms this binary immediately prints a message and exits
//! with code 1, so it can be compiled cross-platform but should only be
//! installed on Windows.

#[cfg(windows)]
mod app;

fn main() -> anyhow::Result<()> {
    windows_main()
}

#[cfg(windows)]
fn windows_main() -> anyhow::Result<()> {
    use eframe::egui;
    use std::sync::{Arc, Mutex};

    tracing_subscriber::fmt()
        .with_target(false)
        .compact()
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

    // Auto-connect to the default proxy (LAX).
    let proxy: std::net::SocketAddrV4 = "149.28.84.139:4434".parse().unwrap();
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
        Box::new(move |_cc| {
            Ok(Box::new(app::LightSpeedApp::new(Arc::clone(
                &engine_for_closure,
            ))))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe error: {}", e))?;

    engine.lock().unwrap().disconnect();
    rt.shutdown_timeout(std::time::Duration::from_secs(2));
    Ok(())
}

#[cfg(not(windows))]
fn windows_main() -> anyhow::Result<()> {
    eprintln!("lightspeed-gui is Windows-only. Use the `lightspeed` CLI on this platform.");
    std::process::exit(1);
}
