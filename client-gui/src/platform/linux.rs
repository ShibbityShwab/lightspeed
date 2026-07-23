use std::sync::Arc;

use crate::app::{TrayState, GAMES, STEAM_SERVICE_PORTS};
use crate::platform::{self, Platform, TrayHandle};
use eframe::egui;

pub struct LinuxTray;

impl TrayHandle for LinuxTray {
    fn poll_events(&self, _ctx: &egui::Context) -> Vec<crate::platform::TrayAction> {
        Vec::new()
    }

    fn set_state(&self, _state: TrayState, _rtt_ms: f64) {}
}

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

    fn detect_game_ports(game_idx: usize) -> (u16, u16) {
        let (key, _, _) = GAMES[game_idx];
        if key == "rust" {
            if let Some(range) = detect_rust_ports_ss() {
                tracing::info!("Linux ss-detected Rust port range: {}-{}", range.0, range.1);
                return range;
            }
            tracing::debug!("ss detection failed — using wide fallback 28015-28999");
        }
        platform::default_port_range(game_idx)
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

    let ss = Command::new("ss").args(["-uapn"]).output().ok()?;
    let mut ports: Vec<u16> = Vec::new();

    for line in String::from_utf8_lossy(&ss.stdout).lines() {
        if !line.contains(&pid_str) {
            continue;
        }
        // ss -uapn output lines look like:
        // UNCONN 0 0    10.0.0.1:56962       0.0.0.0:*    users:(("RustClient",pid=1234,fd=5))
        // We look for the foreign address column (4th whitespace-delimited field)
        // which is the game server the socket is connected to.
        let parts: Vec<&str> = line.split_whitespace().collect();
        for part in &parts {
            if let Some(port_str) = part.rsplit(':').next() {
                if let Ok(port) = port_str.parse::<u16>() {
                    if port >= 1024
                        && !STEAM_SERVICE_PORTS.contains(&port)
                        && !ports.contains(&port)
                    {
                        ports.push(port);
                    }
                }
            }
            // Also check local address column for listening sockets
            if let Some(port_str) = part.split(':').last() {
                if let Ok(port) = port_str.trim_end_matches('*').trim().parse::<u16>() {
                    if port >= 28015 && port <= 30000 && !ports.contains(&port) {
                        ports.push(port);
                    }
                }
            }
        }
    }

    tracing::debug!("RustClient candidate UDP ports: {:?}", ports);

    if ports.is_empty() {
        return None;
    }

    let lo = *ports.iter().min().unwrap();
    let hi = *ports.iter().max().unwrap();
    Some((lo, hi.max(lo + 2)))
}
