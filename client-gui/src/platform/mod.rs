use crate::app::TrayState;
use eframe::egui;

/// Actions returned by [`TrayHandle::poll_events`] that the app must handle
/// because they need the Engine (owned by the app).
///
/// Show-window and Quit actions are handled inside the tray (they only need
/// the `egui::Context`) and never appear here.
#[derive(Debug, PartialEq, Eq)]
pub enum TrayAction {
    Connect,
    Disconnect,
}

pub trait TrayHandle: Send {
    /// Poll platform tray/menu events.
    ///
    /// The tray handles window-show/hide and quit internally (they only need
    /// `ctx`).  Connect/Disconnect are returned because they need the Engine,
    /// which only the app owns.
    fn poll_events(&self, ctx: &egui::Context) -> Vec<TrayAction>;

    /// Update the tray icon colour and tooltip to reflect `state`.
    ///
    /// The implementation SHOULD skip redundant updates (same state as last
    /// call) to avoid unnecessary WM_SETICON traffic on Windows.
    fn set_state(&self, state: TrayState, rtt_ms: f64);
}

/// Fallback port range for a given game index.
///
/// Used by both Windows and Linux port-detection paths when live
/// detection (netstat / ss) fails or is unavailable.
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

pub trait Platform {
    type Tray: TrayHandle;

    fn new_tray() -> Self::Tray;
    fn is_admin() -> bool;
    fn is_capture_available() -> bool;
    fn setup_fonts(ctx: &egui::Context);
    fn detect_game_ports(game_idx: usize) -> (u16, u16);
    fn relaunch_as_admin() -> !;
}

// ── Platform selection ──────────────────────────────────────────────────
// Both modules exist on disk so the IDE can check them, but only one is
// compiled depending on the target OS.
//
// NB: the #[cfg] attributes are currently commented out during development
// to let the Linux-hosted IDE validate all code paths.  Uncomment before
// shipping.
// #[cfg(windows)]
mod windows;
// #[cfg(target_os = "linux")]
mod linux;

// #[cfg(windows)]
mod platform_impl {
    pub use super::windows::*;
    pub type CurrentPlatform = super::windows::WindowsPlatform;
}

// #[cfg(target_os = "linux")]
// mod platform_impl {
//     pub use super::linux::*;
//     pub type CurrentPlatform = super::linux::LinuxPlatform;
// }

pub use platform_impl::*;
