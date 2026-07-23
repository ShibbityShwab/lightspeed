//! # LightSpeed GUI — Windows Platform
//!
//! Real tray-icon backend using `tray_icon`, Windows-specific font paths,
//! admin check via `net session`, capture check via `sc query npcap`, and
//! Rust port detection via `tasklist` + `netstat`.

use std::cell::Cell;
use std::sync::Arc;

use crate::app::{TrayState, STEAM_SERVICE_PORTS};
use crate::platform::{self, Platform, TrayAction, TrayHandle};
use eframe::egui;
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem, MenuId},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};

// ── Tray menu item IDs ──────────────────────────────────────────────────────

const MENU_SHOW: &str = "show";
const MENU_CONNECT: &str = "connect";
const MENU_DISCONNECT: &str = "disconnect";
const MENU_QUIT: &str = "quit";

/// SAFETY: `TrayIcon` uses `Rc<RefCell<…>>` internally on Windows, which is
/// not `Send` by default.  However, `WindowsTray` is only ever created and
/// accessed on the main (egui) thread — the same thread that runs the Windows
/// message loop — so moving it across threads never happens in practice.
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
    last_state: Cell<TrayState>,
}

impl WindowsTray {
    pub fn new() -> Self {
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

        // Start gray (disconnected) — will update on first frame via tray state machine.
        let icon = lightning_icon(160, 160, 160);

        let icon = TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_tooltip("\u{26a1} LightSpeed \u{2014} disconnected")
            .with_icon(icon)
            .build()
            .expect("Failed to create tray icon");

        WindowsTray {
            icon,
            id_show,
            id_connect,
            id_disconnect,
            id_quit,
            last_state: Cell::new(TrayState::Disconnected),
        }
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
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
                format!("\u{26a1} LightSpeed \u{2014} connected \u{00b7} RTT {:.0}ms", rtt_ms)
            }
            TrayState::Optimizing => "\u{26a1} LightSpeed \u{2014} optimizing".into(),
            TrayState::Error => "\u{26a1} LightSpeed \u{2014} error".into(),
        };

        let _ = self.icon.set_icon(Some(lightning_icon(r, g, b)));
        let _ = self.icon.set_tooltip(Some(&tooltip));
    }
}

/// Windows [`Platform`] backend.
///
/// Uses `tray_icon` for the system tray, Win32 font paths, admin check via
/// `net session`, and Npcap detection via `sc query`.
pub struct WindowsPlatform;

impl Platform for WindowsPlatform {
    type Tray = WindowsTray;

    fn new_tray() -> Self::Tray {
        WindowsTray::new()
    }

    fn is_admin() -> bool {
        use std::process::Command;
        Command::new("net")
            .args(["session"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn is_capture_available() -> bool {
        use std::process::Command;
        Command::new("sc")
            .args(["query", "npcap"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }

    fn setup_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguiemj.ttf") {
            fonts.font_data.insert(
                "seguiemj".to_owned(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts.families.entry(family).or_default().push("seguiemj".to_owned());
            }
        }

        if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
            fonts.font_data.insert(
                "seguisym".to_owned(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            fonts
                .families
                .entry(egui::FontFamily::Proportional)
                .or_default()
                .push("seguisym".to_owned());
        }

        ctx.set_fonts(fonts);
    }

    fn detect_rust_ports() -> Option<(u16, u16)> {
        detect_rust_ports_netstat()
    }

    fn relaunch_as_admin() -> ! {
        let exe = std::env::current_exe()
            .unwrap_or_default()
            .display()
            .to_string();
        let script = format!("Start-Process '{}' -Verb RunAs", exe.replace('\'', "''"));
        let _ = std::process::Command::new("powershell")
            .args(["-WindowStyle", "Hidden", "-Command", &script])
            .spawn();
        std::process::exit(0);
    }
}

// ── Icon generation ──────────────────────────────────────────────────────────

fn lightning_icon(r: u8, g: u8, b: u8) -> tray_icon::Icon {
    const SIZE: usize = 32;
    let poly: [(f32, f32); 6] = [
        (0.55, 0.02),
        (0.18, 0.48),
        (0.50, 0.48),
        (0.10, 0.98),
        (0.82, 0.52),
        (0.50, 0.52),
    ];

    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for y in 0..SIZE {
        for x in 0..SIZE {
            let px = (x as f32 + 0.5) / SIZE as f32;
            let py = (y as f32 + 0.5) / SIZE as f32;
            if point_in_poly(px, py, &poly) {
                let idx = (y * SIZE + x) * 4;
                rgba[idx] = r;
                rgba[idx + 1] = g;
                rgba[idx + 2] = b;
                rgba[idx + 3] = 255;
            }
        }
    }
    tray_icon::Icon::from_rgba(rgba, SIZE as u32, SIZE as u32)
        .expect("Failed to build tray icon from RGBA data")
}

fn point_in_poly(px: f32, py: f32, poly: &[(f32, f32)]) -> bool {
    let n = poly.len();
    let mut inside = false;
    let mut j = n - 1;
    for i in 0..n {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if ((yi > py) != (yj > py)) && (px < (xj - xi) * (py - yi) / (yj - yi) + xi) {
            inside = !inside;
        }
        j = i;
    }
    inside
}

// ── Port detection ───────────────────────────────────────────────────────────

fn detect_rust_ports_netstat() -> Option<(u16, u16)> {
    use std::process::Command;

    let tl = Command::new("tasklist")
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

    let ns = Command::new("netstat")
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
                if !ports.contains(&port) && port >= 28015 && port <= 30000 {
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
