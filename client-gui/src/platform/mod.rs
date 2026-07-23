//! # LightSpeed GUI — Platform Abstraction
//!
//! Traits and helpers that isolate platform-specific concerns (system tray,
//! font paths, admin detection, game port discovery) behind a generic
//! [`Platform`] + [`TrayHandle`] interface.  The Windows and Linux backends
//! live in [`windows`] and [`linux`] and are selected at compile time
//! via `#[cfg]`.

use crate::app::TrayState;
use eframe::egui;

/// Actions returned by [`TrayHandle::poll_events`] that the app must handle
/// because they need the Engine (owned by the app).
///
/// Show-window and Quit actions are handled inside the tray (they only need
/// the `egui::Context`) and never appear here.
#[allow(dead_code)] // Connect/Disconnect only constructed on Windows (real tray)
#[derive(Debug, PartialEq, Eq)]
pub enum TrayAction {
    Connect,
    Disconnect,
}

pub trait TrayHandle: Send {
    fn poll_events(&self, ctx: &egui::Context) -> Vec<TrayAction>;

    /// Update the tray icon colour and tooltip to reflect `state`.
    ///
    /// The implementation SHOULD skip redundant updates (same state as last
    /// call) to avoid unnecessary WM_SETICON traffic on Windows.
    fn set_state(&self, state: TrayState, rtt_ms: f64);
}

/// Shared port-range look-up used by both Windows and Linux port-detection
/// paths when live detection (netstat / ss) fails or is unavailable.
pub fn default_port_range(game_idx: usize) -> (u16, u16) {
    let (key, _, default_port) = crate::app::GAMES[game_idx];
    match key {
        "rust" => (28015, 30000),
        "cs2" => (27015, 27100),
        "dota2" => (27015, 27100),
        "valorant" => (7000, 7500),
        "apex" => (37000, 37050),
        "lol" => (5000, 5500),
        "pubg" => (7777, 7843),
        _ => (default_port, default_port),
    }
}

/// Helper: convert a list of candidate ports into a (lo, hi) range.
///
/// The hi end is extended by at least 2 to give the WinDivert/interceptor
/// filter a small buffer.
pub(crate) fn ports_to_range(ports: &[u16]) -> Option<(u16, u16)> {
    if ports.is_empty() {
        return None;
    }
    let lo = *ports.iter().min().unwrap();
    let hi = *ports.iter().max().unwrap();
    Some((lo, hi.max(lo + 2)))
}

pub trait Platform {
    type Tray: TrayHandle;

    fn new_tray() -> Self::Tray;
    fn is_admin() -> bool;
    #[allow(dead_code)]
    fn is_capture_available() -> bool;
    fn setup_fonts(ctx: &egui::Context);

    /// Detect active game-server ports for a game.
    ///
    /// The default implementation tries `detect_rust_ports()` for "rust" and
    /// falls back to `default_port_range()`.  Override `detect_rust_ports()`
    /// (or this method entirely) to add platform-specific detection.
    fn detect_game_ports(game_idx: usize) -> (u16, u16) {
        let (key, _, _) = crate::app::GAMES[game_idx];
        if key == "rust" {
            if let Some(range) = Self::detect_rust_ports() {
                return range;
            }
        }
        default_port_range(game_idx)
    }

    /// Platform-specific Rust port detection (tasklist+netstat on Windows,
    /// pgrep+ss on Linux).  Returns None when the process is not running.
    fn detect_rust_ports() -> Option<(u16, u16)> {
        None
    }

    fn relaunch_as_admin() -> !;
}

// ── Platform selection ──────────────────────────────────────────────────
// Both modules exist on disk so the IDE can check them, but only one is
// compiled depending on the target OS.

#[cfg(windows)]
mod windows;
#[cfg(target_os = "linux")]
mod linux;

#[cfg(windows)]
mod platform_impl {
    pub type CurrentPlatform = super::windows::WindowsPlatform;
}

#[cfg(target_os = "linux")]
mod platform_impl {
    pub type CurrentPlatform = super::linux::LinuxPlatform;
}

pub use platform_impl::*;
