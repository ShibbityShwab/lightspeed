//! LightSpeed GUI - system-tray icon + egui status window.
//!
//! Cross-platform via the `platform` module (Windows tray-icon with
//! `tray_icon`, Linux stub).

#![cfg_attr(windows, windows_subsystem = "windows")]

mod app;
mod client_trace;
mod config;
mod crash;
mod design;
mod discovery;
mod globe;
mod i18n;
mod paths;
mod platform;
mod processes;
mod relay_history;
mod self_update;
mod server_place;
mod single_instance;
mod update;

use eframe::egui;
use platform::Platform;
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
    // The platform half of the parity matrix, in the log: which reduced-motion
    // source is wired, the canonical shortcut table, and the autostart writer
    // plan (W2-4 (d)/(f)/(i)). No behaviour; this is what the parity report and
    // a support log cite instead of a guess.
    log_platform_facts();

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

    // G0.2 headless smoke: the real window, no engine/discovery/tray, three
    // frames, a deterministic log marker and a clean exit 0. Used by the
    // Linux/macOS GUI jobs; it must never change shipped behaviour and is
    // reachable only through the env var.
    if smoke_mode() {
        return run_smoke();
    }

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

    // Server detection runs in the client's WinDivert data plane. The core owns
    // the shipped location indexes and the one classifier seam (guard G7); the
    // GUI only asks the core to install it, before the engine - or any engine
    // task - exists, so a candidate endpoint is never classified without it
    // (the 2026-11-20 mis-lock).
    lightspeed_client::app::location::install_provider_classifier();
    tracing::info!("detection provider classifier installed");

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
        viewport: build_viewport(),
        ..Default::default()
    };

    let engine_for_closure = Arc::clone(&engine);
    let quit_for_closure = Arc::clone(&quit);
    tracing::info!("entering eframe::run_native");
    eframe::run_native(
        "LightSpeed",
        native_options,
        Box::new(move |cc: &eframe::CreationContext<'_>| {
            // The language is resolved here, not in the app, so the CJK faces are
            // ordered correctly on the very first painted frame.
            let language = config::load_unified(&paths::config_paths()).config.language;
            i18n::set_language(language.as_deref());
            app::install_fonts(&cc.egui_ctx, Some(&i18n::current()));
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

/// The status window is intentionally small: it shows connection state, the
/// current ping and one action, so a tall window would just be empty. The
/// settings live in their own window (see app.rs `settings_window`).
/// Fixed mobile width. The client is a phone-shaped status window: 320 px is
/// the contract, so the width is pinned rather than clamped, and only the
/// height is free to follow the content.
fn build_viewport() -> egui::ViewportBuilder {
    let viewport = egui::ViewportBuilder::default()
        .with_inner_size([320.0, 760.0])
        // The same bounds the auto-size in `app.rs` clamps to, from one place.
        .with_min_inner_size([320.0, app::MIN_WINDOW_HEIGHT])
        .with_max_inner_size([320.0, app::MAX_WINDOW_HEIGHT])
        .with_resizable(true)
        .with_title("LightSpeed");
    // A failed icon decode must not abort startup.
    match window_icon() {
        Some(icon) => viewport.with_icon(icon),
        None => viewport,
    }
}

/// The one platform-parity log line set (W2-4): reduced-motion source, the
/// canonical shortcut table and the autostart writer plan.
///
/// Support and the parity report cite these lines instead of re-deriving what
/// the platform claims. No behaviour is triggered here.
fn log_platform_facts() {
    let animations = platform::CurrentPlatform::animations();
    tracing::info!(
        "platform={} reduced-motion={:?} source={}",
        std::env::consts::OS,
        animations,
        animations.source()
    );
    for row in platform::CurrentPlatform::shortcuts() {
        tracing::info!(
            "shortcut {} -> {:?} ({:?}, tray={}, modifiers={:?}, key={:?}, menu_label={:?})",
            row.display,
            row.action,
            row.scope,
            row.requires_tray,
            row.chord.modifiers,
            row.chord.key,
            row.action.label_key()
        );
    }
    for action in [
        platform::ShortcutAction::Quit,
        platform::ShortcutAction::Settings,
        platform::ShortcutAction::HideToTray,
    ] {
        match platform::shortcut_binding(action) {
            Some(row) => tracing::info!("menu accelerator {:?} = {}", action, row.display),
            None => tracing::warn!("no canonical binding for {action:?}"),
        }
    }
    match platform::CurrentPlatform::autostart_plan() {
        Some(plan) => tracing::info!("autostart writer {} ({})", plan.kind(), plan.describe()),
        None => tracing::warn!("autostart writer plan unavailable (unknown executable path)"),
    }
}

/// Whether this process is the G0.2 headless smoke run.
fn smoke_mode() -> bool {
    std::env::var("LIGHTSPEED_GUI_SMOKE")
        .map(|value| value == "1")
        .unwrap_or(false)
}

/// G0.2 headless smoke: open the real window, prove it exists, close cleanly.
///
/// Deliberately does no engine, tokio, classifier, discovery, tray or
/// single-instance *engine* work: the probe is the window itself plus the log
/// markers, and the process exits 0. The single-instance guard above is kept so
/// a smoke run on a busy desktop is a real "second instance" exit 2, not a
/// false positive.
fn run_smoke() -> anyhow::Result<()> {
    tracing::info!("GUI_SMOKE mode: engine, discovery and tray are skipped");
    let native_options = eframe::NativeOptions {
        renderer: renderer_from_env(),
        viewport: build_viewport(),
        ..Default::default()
    };
    eframe::run_native(
        "LightSpeed",
        native_options,
        Box::new(|_cc: &eframe::CreationContext<'_>| {
            Ok(Box::new(SmokeApp {
                frames: 0,
                closed: false,
            }))
        }),
    )
    .map_err(|e| anyhow::anyhow!("eframe smoke error: {e}"))?;
    Ok(())
}

/// The smoke run's frame loop: prove the window on frame 1, close on frame 3.
///
/// The markers are the CI contract (`GUI_SMOKE window_created …` then
/// `GUI_SMOKE exit=0 …`); keep them stable when touching this file.
struct SmokeApp {
    frames: u32,
    /// The close was already requested; the marker is emitted once so the CI
    /// probe sees exactly one `GUI_SMOKE exit=0` line.
    closed: bool,
}

impl eframe::App for SmokeApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.frames = self.frames.saturating_add(1);
        if self.frames == 1 {
            let ctx = ui.ctx();
            let title = ctx.input(|input| input.viewport().title.clone());
            let pixels_per_point = ctx.pixels_per_point();
            tracing::info!(
                "GUI_SMOKE window_created title={:?} pixels_per_point={pixels_per_point:.3} backend={} renderer={:?}",
                title.unwrap_or_default(),
                std::env::consts::OS,
                renderer_from_env()
            );
        }
        if self.frames >= 3 && !self.closed {
            self.closed = true;
            tracing::info!("GUI_SMOKE exit=0 frames={}", self.frames);
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Keep painting without busy-blocking: eframe drives the loop.
        ui.ctx().request_repaint();
    }
}

/// Decode the embedded brand PNG into the window and taskbar icon.
///
/// A decode failure is logged and treated as "no icon" so a bad asset can
/// never stop the window from opening.
///
/// This covers only the window the app already owns (`WM_SETICON`). The
/// shell's taskbar button and every Explorer surface are served from the
/// exe-level resource that `build.rs` compiles in, so both paths are needed:
/// removing either one leaves a generic icon somewhere.
fn window_icon() -> Option<egui::IconData> {
    const ICON_PNG: &[u8] = include_bytes!("../../web/assets/brand/icon-256.png");
    let rgba = match image::load_from_memory(ICON_PNG) {
        Ok(decoded) => decoded.to_rgba8(),
        Err(e) => {
            tracing::warn!("could not decode window icon: {e}");
            return None;
        }
    };
    let (width, height) = rgba.dimensions();
    Some(egui::IconData {
        rgba: rgba.into_raw(),
        width,
        height,
    })
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
