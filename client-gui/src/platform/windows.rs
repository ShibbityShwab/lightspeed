//! # LightSpeed GUI - Windows Platform
//!
//! Real tray-icon backend using `tray_icon`, admin check via `net session`,
//! capture check via `sc query npcap`, and Rust port detection via
//! `tasklist` + `netstat`.

use std::cell::Cell;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::{TrayState, STEAM_SERVICE_PORTS};
use crate::platform::{self, Platform, QuitFlag, TrayAction, TrayHandle};
use eframe::egui;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};

// ── Tray menu item IDs ──────────────────────────────────────────────────────

const MENU_SHOW: &str = "show";
const MENU_CONNECT: &str = "connect";
const MENU_DISCONNECT: &str = "disconnect";
const MENU_QUIT: &str = "quit";

/// Set once the tray icon exists; read by `has_system_tray`.
static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// SAFETY: `TrayIcon` uses `Rc<RefCell<…>>` internally on Windows, which is
/// SAFETY: `WindowsTray` is only ever created and accessed on the main
/// (egui) thread - the same thread that runs the Windows message loop.
/// It is never sent across threads, so `Rc<RefCell<…>>` internals from
/// the `tray_icon` crate cannot cause data races in practice.
// SAFETY: WindowsTray contains winapi HANDLEs which are Send-safe (HANDLEs
// can be used from any thread in the Windows API model).
unsafe impl Send for WindowsTray {}

/// Windows system-tray icon with a context menu (Show, Connect, Disconnect, Quit).
///
/// Created via [`WindowsPlatform::new_tray`].  Runs on the egui UI thread and
/// communicates back to the app via [`TrayHandle::poll_events`].
pub struct WindowsTray {
    icon: TrayIcon,
    id_show: MenuId,
    id_connect: MenuId,
    id_disconnect: MenuId,
    id_quit: MenuId,
    quit: QuitFlag,
    last_state: Cell<TrayState>,
}

impl WindowsTray {
    pub fn new(quit: QuitFlag) -> Option<Self> {
        let item_show = MenuItem::with_id(MENU_SHOW, "Show window", true, None);
        let item_connect = MenuItem::with_id(MENU_CONNECT, "Connect", true, None);
        let item_disconnect = MenuItem::with_id(MENU_DISCONNECT, "Disconnect", true, None);
        let item_quit = MenuItem::with_id(MENU_QUIT, "Quit", true, None);

        let id_show = item_show.id().clone();
        let id_connect = item_connect.id().clone();
        let id_disconnect = item_disconnect.id().clone();
        let id_quit = item_quit.id().clone();

        let menu = Menu::new();
        let _ = menu.append(&item_show);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&item_connect);
        let _ = menu.append(&item_disconnect);
        let _ = menu.append(&PredefinedMenuItem::separator());
        let _ = menu.append(&item_quit);

        // Start gray (disconnected) - will update on first frame via tray state machine.
        let Some(icon) = lightning_icon(160, 160, 160) else {
            tracing::warn!("Tray icon image unavailable; continuing without a system tray");
            return None;
        };

        let icon = match TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("\u{26a1} LightSpeed \u{2014} disconnected")
            .with_icon(icon)
            .build()
        {
            Ok(icon) => icon,
            Err(e) => {
                tracing::warn!("Failed to create tray icon ({e}); continuing without a tray");
                return None;
            }
        };

        Some(WindowsTray {
            icon,
            id_show,
            id_connect,
            id_disconnect,
            id_quit,
            quit,
            last_state: Cell::new(TrayState::Disconnected),
        })
    }
}

impl TrayHandle for WindowsTray {
    fn poll_events(&self, ctx: &egui::Context) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            tracing::debug!("Tray menu event: {:?}", event.id);
            if event.id == self.id_show {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if event.id == self.id_connect {
                actions.push(TrayAction::Connect);
            } else if event.id == self.id_disconnect {
                actions.push(TrayAction::Disconnect);
            } else if event.id == self.id_quit {
                // Tray Quit must terminate the process, not hide the window.
                // The frame loop sees the flag, tears the engine down, and
                // exits; the repaint wakes it immediately.
                self.quit.store(true, Ordering::Relaxed);
                ctx.request_repaint();
            }
        }
        while let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }
        actions
    }

    fn set_state(&self, state: TrayState, rtt_ms: f64) {
        if self.last_state.get() == state {
            return;
        }
        self.last_state.set(state);

        let (r, g, b): (u8, u8, u8) = match state {
            TrayState::Disconnected => (160, 160, 160),
            TrayState::Connected => (255, 200, 60),
            TrayState::Optimizing => (80, 210, 120),
            TrayState::Error => (220, 80, 80),
        };
        let tooltip: String = match state {
            TrayState::Disconnected => "\u{26a1} LightSpeed \u{2014} disconnected".into(),
            TrayState::Connected => {
                format!(
                    "\u{26a1} LightSpeed \u{2014} connected \u{00b7} RTT {:.0}ms",
                    rtt_ms
                )
            }
            TrayState::Optimizing => "\u{26a1} LightSpeed \u{2014} optimizing".into(),
            TrayState::Error => "\u{26a1} LightSpeed \u{2014} error".into(),
        };

        if let Some(icon) = lightning_icon(r, g, b) {
            let _ = self.icon.set_icon(Some(icon));
        }
        let _ = self.icon.set_tooltip(Some(&tooltip));
    }
}

/// Windows [`Platform`] backend.
///
/// Uses `tray_icon` for the system tray, admin check via `net session`, and
/// Npcap detection via `sc query`.
pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    type Tray = WindowsTray;

    fn new_tray(quit: QuitFlag) -> Option<Self::Tray> {
        let tray = WindowsTray::new(quit);
        TRAY_AVAILABLE.store(tray.is_some(), Ordering::SeqCst);
        if tray.is_none() {
            tracing::warn!("System tray unavailable; closing the window will exit the app");
        }
        tray
    }

    /// Whether this process can raise an interface (needs the driver).
    ///
    /// Reads the process token rather than shelling out to `net session`,
    /// which also fails when the Server service is merely stopped - reporting a
    /// limited user on a machine where the process is in fact elevated.
    fn is_admin() -> bool {
        // Declared locally: pulling in `windows-sys` for one token query would
        // add a dependency with a dozen features for three calls.
        #[link(name = "advapi32")]
        extern "system" {
            fn OpenProcessToken(
                process: *mut core::ffi::c_void,
                desired_access: u32,
                token: *mut *mut core::ffi::c_void,
            ) -> i32;
            fn GetTokenInformation(
                token: *mut core::ffi::c_void,
                class: i32,
                info: *mut core::ffi::c_void,
                len: u32,
                returned: *mut u32,
            ) -> i32;
        }
        #[link(name = "kernel32")]
        extern "system" {
            fn GetCurrentProcess() -> *mut core::ffi::c_void;
            fn CloseHandle(handle: *mut core::ffi::c_void) -> i32;
        }

        const TOKEN_QUERY: u32 = 0x0008;
        const TOKEN_ELEVATION_CLASS: i32 = 20;

        // SAFETY: `token` is only read after a successful `OpenProcessToken`
        // and is closed on every path; the buffer passed to
        // `GetTokenInformation` is exactly the size of the struct it fills.
        unsafe {
            let mut token: *mut core::ffi::c_void = std::ptr::null_mut();
            if OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) == 0 {
                return false;
            }
            let mut elevated: u32 = 0;
            let mut returned: u32 = 0;
            let ok = GetTokenInformation(
                token,
                TOKEN_ELEVATION_CLASS,
                &mut elevated as *mut u32 as *mut core::ffi::c_void,
                std::mem::size_of::<u32>() as u32,
                &mut returned,
            );
            CloseHandle(token);
            ok != 0 && elevated != 0
        }
    }

    fn is_capture_available() -> bool {
        crate::platform::silent_command("sc")
            .args(["query", "npcap"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn setup_fonts(_ctx: &egui::Context) {
        // egui's bundled default fonts already include a monochrome Noto
        // Emoji, which renders the UI emojis consistently on every platform.
        // Color-emoji fonts cannot be rasterized by egui, so nothing else is
        // loaded here.
    }

    fn has_system_tray() -> bool {
        TRAY_AVAILABLE.load(Ordering::SeqCst)
    }

    fn detect_rust_ports() -> Option<(u16, u16)> {
        detect_rust_ports_netstat()
    }

    /// Relaunch elevated, but only when that would actually change anything.
    ///
    /// The previous version always tore the process down and respawned it, so
    /// pressing the button on an already-elevated install restarted the app as
    /// admin at itself - a pointless flash of the window and a dropped session.
    /// When this process is already elevated there is nothing to raise, so it
    /// says so and does nothing.
    fn relaunch_as_admin() {
        if Self::is_admin() {
            tracing::info!("already elevated; not relaunching");
            return;
        }
        let exe = std::env::current_exe()
            .unwrap_or_default()
            .display()
            .to_string();
        let script = format!("Start-Process '{}' -Verb RunAs", exe.replace('\'', "''"));
        let _ = crate::platform::silent_command("powershell")
            .args(["-WindowStyle", "Hidden", "-Command", &script])
            .spawn();
        std::process::exit(0);
    }
}

// ── Icon generation ──────────────────────────────────────────────────────────

fn lightning_icon(r: u8, g: u8, b: u8) -> Option<tray_icon::Icon> {
    const SIZE: usize = 32;
    const LOGO_PNG: &[u8] = include_bytes!("../../../web/assets/brand/icon-256.png");
    let decoded = match image::load_from_memory(LOGO_PNG) {
        Ok(decoded) => decoded
            .resize_exact(
                SIZE as u32,
                SIZE as u32,
                image::imageops::FilterType::Lanczos3,
            )
            .to_rgba8(),
        Err(e) => {
            tracing::warn!("could not decode tray logo: {e}");
            return None;
        }
    };
    let mut rgba = decoded.into_raw();
    // Per-state signal: an 8px corner square in the state colour
    // (grey / amber / green / red). The mark itself is the same artwork the
    // window, taskbar and About surfaces use.
    const DOT: usize = 8;
    for y in (SIZE - DOT)..SIZE {
        for x in (SIZE - DOT)..SIZE {
            let idx = (y * SIZE + x) * 4;
            rgba[idx] = r;
            rgba[idx + 1] = g;
            rgba[idx + 2] = b;
            rgba[idx + 3] = 255;
        }
    }
    match tray_icon::Icon::from_rgba(rgba, SIZE as u32, SIZE as u32) {
        Ok(icon) => Some(icon),
        Err(e) => {
            tracing::warn!("Failed to build tray icon from RGBA data: {e}");
            None
        }
    }
}

// ── Port detection ───────────────────────────────────────────────────────────

fn detect_rust_ports_netstat() -> Option<(u16, u16)> {
    let tl = crate::platform::silent_command("tasklist")
        .args(["/FI", "IMAGENAME eq RustClient.exe", "/FO", "CSV", "/NH"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&tl.stdout);
    if text.trim().to_ascii_lowercase().starts_with("info:") || text.trim().is_empty() {
        tracing::debug!("RustClient.exe not found in tasklist");
        return None;
    }
    let pid: u32 = text
        .lines()
        .filter_map(|line| {
            let mut fields = line.split(',');
            let _name = fields.next()?;
            fields.next()?.trim().trim_matches('"').parse().ok()
        })
        .next()?;

    tracing::debug!("RustClient.exe PID = {}", pid);

    let ns = crate::platform::silent_command("netstat")
        .args(["-ano", "-p", "UDP"])
        .output()
        .ok()?;

    let pid_str = pid.to_string();
    let mut ports: Vec<u16> = Vec::new();

    for line in String::from_utf8_lossy(&ns.stdout).lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 {
            continue;
        }
        if !parts[0].eq_ignore_ascii_case("UDP") {
            continue;
        }

        let line_pid = parts.last().unwrap_or(&"");
        if *line_pid != pid_str {
            continue;
        }

        // Local address: parts[1]
        if let Some(port_str) = parts[1].rsplit(':').next() {
            if let Ok(port) = port_str.parse::<u16>() {
                if port >= 1024 && !STEAM_SERVICE_PORTS.contains(&port) && !ports.contains(&port) {
                    ports.push(port);
                }
            }
        }

        // Foreign address: parts[2]
        if let Some(port_str) = parts[2].rsplit(':').next() {
            if let Ok(port) = port_str.parse::<u16>() {
                if !ports.contains(&port) && (28015..=30000).contains(&port) {
                    ports.push(port);
                }
            }
        }
    }

    tracing::debug!(
        "RustClient.exe (PID {}) candidate UDP ports: {:?}",
        pid,
        ports
    );

    platform::ports_to_range(&ports)
}
