//! # LightSpeed GUI - Platform Abstraction
//!
//! Traits and helpers that isolate platform-specific concerns (system tray,
//! font paths, admin detection) behind a generic
//! [`Platform`] + [`TrayHandle`] interface.  The Windows, Linux, and macOS
//! backends live in [`windows`], [`linux`], and [`macos`] respectively and are
//! selected at compile time via `#[cfg]`. The macOS backend is **untested**.

use crate::app::TrayState;
use eframe::egui;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;

mod animations;
mod autostart;
mod elevation;
mod shortcuts;
mod updater;
// The tray menu is Windows-only until the macOS/Linux tray backends stop being
// stubs; the mapping itself is pure, and compiles out cleanly elsewhere.
#[cfg(windows)]
mod tray_menu;

pub use animations::Animations;
pub use autostart::AutostartPlan;
pub use elevation::{ElevationAttempt, ElevationPhase};
pub use shortcuts::{Key, Modifier, Shortcut, ShortcutAction};
pub use updater::InstallChannel;

/// The canonical binding for `action` on this OS (spec W2-4 (d)).
///
/// The one lookup the tray menu and the startup trace share, so a binding can
/// never be re-spelled at a call site.
pub fn shortcut_binding(action: ShortcutAction) -> Option<&'static Shortcut> {
    shortcuts::binding(shortcuts::current_os(), action)
}

/// The updater channel for this process (spec W2-4 (g)).
///
/// `receipt_present` is whether cargo-dist's install receipt was found;
/// `install_asset` whether the Windows zip + checksum pair resolved. A channel
/// that cannot act names the copy that says what to do instead, so the update
/// sheet can never draw a dead install button.
pub fn update_channel(receipt_present: bool, install_asset: bool) -> InstallChannel {
    updater::channel(shortcuts::current_os(), receipt_present, install_asset)
}

/// The result line for an available update on `channel` (no dead button: the
/// install channel returns the plain result, every other channel appends its
/// instruction). Catalogued copy.
pub fn update_available_line(channel: InstallChannel) -> String {
    updater::available_line(channel)
}

/// Shared flag the tray sets when the user picks "Quit"; the frame loop turns
/// it into a clean process exit.
pub type QuitFlag = Arc<AtomicBool>;

/// A [`std::process::Command`] for `program` that cannot flash a console window
/// on Windows.
///
/// This GUI owns no console (`windows_subsystem = "windows"`), so spawning a
/// console binary without `CREATE_NO_WINDOW` makes Windows allocate a NEW
/// console window for the child. On non-Windows targets this is exactly
/// `Command::new`.
pub(crate) fn silent_command(program: &str) -> std::process::Command {
    #[allow(unused_mut)]
    let mut cmd = std::process::Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Actions returned by [`TrayHandle::poll_events`] that the app must handle
/// because they need the Engine (owned by the app).
///
/// Show-window and Quit actions are handled inside the tray (they only need
/// the `egui::Context` and the shared [`QuitFlag`) and never appear here.
///
/// **Platform exemption (documented in W2-4b):** the two variants are
/// constructed only by the Windows tray backend; the Linux and macOS stub trays
/// never return them, so a non-Windows build would warn that the variants are
/// never constructed. The enum itself is used on every target (it is the
/// `poll_events` return type and `app.rs` matches both arms), so this is one
/// named platform exemption, not a blanket allowance.
#[allow(dead_code)] // Windows tray constructs Connect/Disconnect; the stub trays never do
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

/// The built-in UDP scan range for a game key, with `default_port` as the
/// fallback for a key the table does not cover.
///
/// The Manual flow shows it as the port-range field's example, so the hint
/// reads as the game about to be scanned.
pub fn default_port_range(key: &str, default_port: u16) -> (u16, u16) {
    match key {
        "rust" => (28015, 30000),
        "cs2" | "csgo" | "dota2" => (27015, 27100),
        "valorant" => (7000, 7500),
        "apex" => (37000, 37050),
        "lol" => (5000, 5500),
        "pubg" => (7777, 7843),
        "bodycam" => (27000, 27050),
        "zomboid" => (16261, 16262),
        "wardogs" => (7777, 7788),
        "minecraft" => (19132, 19133),
        "hunt" => (20000, 20099),
        "arcraiders" => (7600, 7777),
        "jedioutcast" => (28070, 28070),
        "tibia" => (7171, 7172),
        "robrun" => (32768, 60999),
        "fc27" => (21000, 21050),
        _ => (default_port, default_port),
    }
}

pub trait Platform {
    type Tray: TrayHandle;

    /// Build the platform tray, or `None` when it is unavailable.
    ///
    /// Returning `None` (instead of panicking) lets the app keep running
    /// without a tray and makes closing the window exit instead of hiding it.
    fn new_tray(quit: QuitFlag) -> Option<Self::Tray>;
    fn is_admin() -> bool;
    fn is_capture_available() -> bool;
    fn setup_fonts(ctx: &egui::Context);

    /// Whether this platform has a real, recoverable system tray.
    ///
    /// On Windows this reflects whether the icon was actually created (a
    /// failed tray is not recoverable); Linux and macOS use a stub tray and
    /// always return `false`, so the close-intercept and "Hide to tray"
    /// affordance are skipped there (hiding would strand the window).
    fn has_system_tray() -> bool {
        false
    }

    /// Relaunch elevated, or do nothing when already elevated.
    ///
    /// The low-level primitive: it spawns the platform's elevation request and,
    /// on the legacy backends, exits this process. `begin_elevation` wraps it
    /// with the handshake that fixes the C8 "exits before UAC resolves" defect.
    fn relaunch_as_admin();

    /// What the OS says about motion (WCAG 2.2.2; spec W2-4 (i)).
    ///
    /// Every backend answers explicitly: Windows probes
    /// `SPI_GETCLIENTAREAANIMATION`, macOS and Linux return `Unknown` because
    /// no source is wired (named gaps). Only an explicit `On` may animate.
    fn animations() -> Animations;

    /// The canonical shortcut keymap for this OS (spec W2-4 (d)).
    fn shortcuts() -> &'static [Shortcut] {
        shortcuts::keymap(shortcuts::current_os())
    }

    /// The opt-in autostart writer plan for this install, when the executable
    /// path is knowable (spec W2-4 (f)).
    ///
    /// This returns data; writing it is the Settings toggle's job (WF-081b,
    /// task-74). The startup trace prints it, so a broken plan shows up in the
    /// log instead of only at login.
    fn autostart_plan() -> Option<AutostartPlan> {
        autostart::plan_for_current()
    }

    /// Whether this install's opt-in login entry currently exists.
    ///
    /// The Settings toggle reconciles its checkbox against this observable
    /// truth once per sheet open, rather than trusting a config value that an
    /// outside change (a cleaned registry, a deleted `.desktop`) could make
    /// stale.
    fn autostart_installed() -> bool {
        autostart::installed_current()
    }

    /// Write or remove this install's login entry.
    ///
    /// The error is surfaced verbatim so the Settings toggle can put its
    /// checkbox back and say what failed instead of reporting a success the
    /// platform did not perform. The `--autostart` entry contract is unchanged.
    fn set_autostart(enabled: bool) -> Result<(), String> {
        autostart::apply_current(enabled)
    }

    /// Begin the elevation handshake (spec §2.2 row 7 / C8; W2-4 (e)).
    ///
    /// The returned attempt is polled by the frame loop. The current process
    /// may exit only once the phase is [`ElevationPhase::Confirmed`]: a declined
    /// or failed prompt returns to ready with the window still open. The legacy
    /// backends relaunch and exit inside [`Self::relaunch_as_admin`], so their
    /// attempt is already resolved.
    fn begin_elevation() -> ElevationAttempt {
        if Self::is_admin() {
            return ElevationAttempt::resolved(ElevationPhase::Ready);
        }
        Self::relaunch_as_admin();
        ElevationAttempt::resolved(ElevationPhase::Confirmed)
    }
}

// ── Platform selection ──────────────────────────────────────────────────
// Both modules exist on disk so the IDE can check them, but only one is
// compiled depending on the target OS.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(windows)]
mod windows;

#[cfg(windows)]
mod platform_impl {
    pub type CurrentPlatform = super::windows::WindowsPlatform;
}

#[cfg(target_os = "linux")]
mod platform_impl {
    pub type CurrentPlatform = super::linux::LinuxPlatform;
}

#[cfg(target_os = "macos")]
mod platform_impl {
    pub type CurrentPlatform = super::macos::MacosPlatform;
}

pub use platform_impl::*;
