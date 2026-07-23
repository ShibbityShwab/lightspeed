//! # LightSpeed GUI — Linux Platform
//!
//! Stub tray (Linux has no system-tray standard), font setup via
//! Noto Color Emoji, admin check via `id -u`, capture check via
//! `tcpdump`/`dumpcap`, and Rust port detection via `pgrep` + `ss`.

use std::sync::Arc;

use crate::app::{TrayState, STEAM_SERVICE_PORTS};
use crate::platform::{self, Platform, TrayHandle};
use eframe::egui;

/// No-op tray handle for Linux (no standard system-tray on modern DEs).
pub struct LinuxTray;

impl TrayHandle for LinuxTray {
    fn poll_events(&self, _ctx: &egui::Context) -> Vec<crate::platform::TrayAction> {
        Vec::new()
    }

    fn set_state(&self, _state: TrayState, _rtt_ms: f64) {}
}

/// Linux [`Platform`] backend.
///
/// Stub tray (no system-tray standard on modern DEs), font setup via
/// Noto Color Emoji, admin check via `id -u`, capture check via
/// `tcpdump`/`dumpcap`.
pub struct LinuxPlatform;

impl Platform for LinuxPlatform {
    type Tray = LinuxTray;

    fn new_tray() -> Self::Tray {
        LinuxTray
    }

    fn is_admin() -> bool {
        use std::process::Command;
        Command::new("id")
            .args(["-u"])
            .output()
            .ok()
            .and_then(|o| {
                let s = String::from_utf8_lossy(&o.stdout).trim().to_string();
                s.parse::<u32>().ok()
            })
            .map(|uid| uid == 0)
            .unwrap_or(false)
    }

    fn is_capture_available() -> bool {
        use std::process::Command;
        let tcpdump = Command::new("which")
            .args(["tcpdump"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        let dumpcap = Command::new("which")
            .args(["dumpcap"])
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);

        tcpdump || dumpcap
    }

    fn setup_fonts(ctx: &egui::Context) {
        let mut fonts = egui::FontDefinitions::default();

        let candidates = [
            "/usr/share/fonts/truetype/noto/NotoColorEmoji.ttf",
            "/usr/share/fonts/noto/NotoColorEmoji.ttf",
            "/usr/share/fonts/google-noto-emoji/NotoColorEmoji.ttf",
            "/usr/share/fonts/noto-emoji/NotoColorEmoji.ttf",
        ];

        for path in &candidates {
            if let Ok(bytes) = std::fs::read(path) {
                fonts.font_data.insert(
                    "noto-emoji".to_owned(),
                    Arc::new(egui::FontData::from_owned(bytes)),
                );
                for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                    fonts
                        .families
                        .entry(family)
                        .or_default()
                        .push("noto-emoji".to_owned());
                }
                break;
            }
        }

        ctx.set_fonts(fonts);
    }

    fn detect_rust_ports() -> Option<(u16, u16)> {
        detect_rust_ports_ss()
    }

    fn relaunch_as_admin() -> ! {
        let exe = std::env::current_exe().unwrap_or_default();
        let exe_str = exe.display().to_string();
        let _ = std::process::Command::new("pkexec")
            .args([&exe_str])
            .spawn();
        std::process::exit(0);
    }
}

// ── Port detection ───────────────────────────────────────────────────────────

fn detect_rust_ports_ss() -> Option<(u16, u16)> {
    use std::process::Command;

    let ps = Command::new("pgrep")
        .args(["-x", "RustClient"])
        .output()
        .ok()?;
    let pid_str = String::from_utf8_lossy(&ps.stdout).trim().to_string();
    if pid_str.is_empty() {
        return None;
    }

    tracing::debug!("RustClient PID = {}", pid_str);

    // ss -uapn output format (columns):
    //   UNCONN  0      0    10.0.0.1:56962    0.0.0.0:*    users:(("RustClient",pid=1234,fd=5))
    //   col:    1      2      3        4           5               6
    // Column 4 = local address:port, column 5 = remote address:port.
    let ss = Command::new("ss").args(["-uapn"]).output().ok()?;
    let mut ports: Vec<u16> = Vec::new();

    for line in String::from_utf8_lossy(&ss.stdout).lines() {
        if !line.contains(&pid_str) {
            continue;
        }
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 5 {
            continue;
        }

        // Local port (col 4) — RustClient's listening socket.
        if let Some(port_str) = parts[3].rsplit(':').next() {
            if let Ok(port) = port_str.parse::<u16>() {
                if port >= 1024 && !STEAM_SERVICE_PORTS.contains(&port) && !ports.contains(&port) {
                    ports.push(port);
                }
            }
        }

        // Remote port (col 5) — the game server it's connected to.
        let remote_field = parts[4].trim_end_matches('*');
        if let Some(port_str) = remote_field.rsplit(':').next() {
            if let Ok(port) = port_str.parse::<u16>() {
                if port >= 1024 && !ports.contains(&port) {
                    ports.push(port);
                }
            }
        }
    }

    tracing::debug!("RustClient candidate UDP ports: {:?}", ports);

    platform::ports_to_range(&ports)
}
