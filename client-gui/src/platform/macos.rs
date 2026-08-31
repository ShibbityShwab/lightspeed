//! # LightSpeed GUI — macOS Platform
//!
//! **UNTESTED.** This backend compiles but has not been run on real macOS
//! hardware. It is modeled on the Linux backend: stub tray (no menu-bar
//! integration yet), Apple Color Emoji fonts, `id -u` admin check,
//! `tcpdump`/`dumpcap` capture check, and Rust port detection via `pgrep` +
//! `lsof`.

use std::sync::Arc;

use crate::app::{TrayState, STEAM_SERVICE_PORTS};
use crate::platform::{self, Platform, TrayHandle};
use eframe::egui;

/// No-op tray handle for macOS (no menu-bar integration yet).
pub struct MacosTray;

impl TrayHandle for MacosTray {
    fn poll_events(&self, _ctx: &egui::Context) -> Vec<crate::platform::TrayAction> {
        Vec::new()
    }

    fn set_state(&self, _state: TrayState, _rtt_ms: f64) {}
}

/// macOS [`Platform`] backend.
///
/// **UNTESTED**: compiles but has not been validated on macOS hardware. Stub
/// tray, Apple Color Emoji fonts, `id -u` admin check, `tcpdump`/`dumpcap`
/// capture check, and `pgrep` + `lsof` port detection.
pub struct MacosPlatform;

impl Platform for MacosPlatform {
    type Tray = MacosTray;

    fn new_tray() -> Self::Tray {
        MacosTray
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

        // Apple Color Emoji is the correct system emoji font, but it is
        // sbix-only (no glyph outlines), which egui/ab_glyph cannot rasterize,
        // so emoji render as tofu regardless. The load is harmless; real emoji
        // support needs egui_noto_emoji or a monochrome emoji TTF.
        let path = "/System/Library/Fonts/Apple Color Emoji.ttc";
        if let Ok(bytes) = std::fs::read(path) {
            fonts.font_data.insert(
                "apple-color-emoji".to_owned(),
                Arc::new(egui::FontData::from_owned(bytes)),
            );
            for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
                fonts
                    .families
                    .entry(family)
                    .or_default()
                    .push("apple-color-emoji".to_owned());
            }
        }

        ctx.set_fonts(fonts);
    }

    fn detect_rust_ports() -> Option<(u16, u16)> {
        detect_rust_ports_lsof()
    }

    fn relaunch_as_admin() -> ! {
        let exe = std::env::current_exe().unwrap_or_default();
        let exe_str = exe.display().to_string();
        // Wrap the path in AppleScript's `quoted form of` so spaces, quotes and
        // shell metacharacters in the executable path survive into the elevated
        // `do shell script` (Apple TN2065 quoting guidance).
        let escaped = exe_str.replace('\\', "\\\\").replace('"', "\\\"");
        let script = format!(
            "do shell script (quoted form of \"{}\") with administrator privileges",
            escaped
        );
        let _ = std::process::Command::new("osascript")
            .args(["-e", &script])
            .spawn();
        std::process::exit(0);
    }
}

// ── Port detection ───────────────────────────────────────────────────────────

fn detect_rust_ports_lsof() -> Option<(u16, u16)> {
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

    // lsof -nP -a -p <pid> -iUDP lists that process's UDP sockets. The NAME
    // column is "local:port" (listening) or "local:port->remote:port" (connected):
    //   COMMAND     PID   USER   FD   TYPE  DEVICE SIZE/OFF NODE NAME
    //   RustClient 1234  user   12u  IPv4  0x...       0t0  UDP  *:28015
    let lsof = Command::new("lsof")
        .args(["-nP", "-a", "-p", &pid_str, "-iUDP"])
        .output()
        .ok()?;
    let mut ports: Vec<u16> = Vec::new();

    for line in String::from_utf8_lossy(&lsof.stdout).lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 9 {
            continue;
        }
        // The NAME column (index 8) holds the local:port (and remote:port when
        // connected). Capture both so the interceptor covers the full range.
        if let Some(name) = parts.get(8) {
            for side in name.split("->") {
                if let Some(port_str) = side.rsplit(':').next() {
                    if let Ok(port) = port_str.parse::<u16>() {
                        if port >= 1024
                            && !STEAM_SERVICE_PORTS.contains(&port)
                            && !ports.contains(&port)
                        {
                            ports.push(port);
                        }
                    }
                }
            }
        }
    }

    tracing::debug!("RustClient candidate UDP ports: {:?}", ports);

    platform::ports_to_range(&ports)
}
