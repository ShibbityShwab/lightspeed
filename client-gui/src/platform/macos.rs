//! # LightSpeed GUI - macOS Platform
//!
//! **UNTESTED.** This backend compiles but has not been run on real macOS
//! hardware. It is modeled on the Linux backend: stub tray (no menu-bar
//! integration yet), `id -u` admin check, and `tcpdump`/`dumpcap` capture
//! check.

use crate::app::TrayState;
use crate::platform::{Platform, QuitFlag, TrayHandle};
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
/// tray, `id -u` admin check, and `tcpdump`/`dumpcap` capture check.
pub struct MacosPlatform;

impl Platform for MacosPlatform {
    type Tray = MacosTray;

    fn new_tray(_quit: QuitFlag) -> Option<Self::Tray> {
        Some(MacosTray)
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

    fn setup_fonts(_ctx: &egui::Context) {
        // egui's bundled default fonts already include a monochrome Noto
        // Emoji, which renders the UI emojis consistently on every platform.
        // Color-emoji fonts cannot be rasterized by egui, so nothing else is
        // loaded here.
    }

    fn relaunch_as_admin() {
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
