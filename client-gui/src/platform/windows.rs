//! # LightSpeed GUI - Windows Platform
//!
//! Real tray-icon backend using `tray_icon`, admin check via the process token,
//! the reduced-motion probe from `SPI_GETCLIENTAREAANIMATION`, and the
//! elevation handshake that keeps this process alive until the elevated
//! instance confirms startup (spec §2.2 row 7 / C8).
//!
//! The tray's colour/tooltip/menu mapping is pure data in [`super::tray_menu`];
//! this file applies it. The capture check (`sc query npcap`) is the known
//! wrong-dependency stub (cross-platform plan S9) and is not this wave's work.

use std::cell::{Cell, RefCell};
use std::sync::atomic::{AtomicBool, Ordering};

use crate::app::TrayState;
use crate::i18n;
use eframe::egui;
use tray_icon::menu::accelerator::{Accelerator, Code, Modifiers};
use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

use super::shortcuts::{Key, KeyChord, Modifier, ShortcutAction};
use super::tray_menu;
use super::{
    Animations, ElevationAttempt, ElevationPhase, Platform, QuitFlag, TrayAction, TrayHandle,
};

// ── Tray menu item IDs ──────────────────────────────────────────────────────

const MENU_SHOW: &str = "show";
const MENU_CONNECT: &str = "connect";
const MENU_DISCONNECT: &str = "disconnect";
const MENU_QUIT: &str = "quit";

/// Set once the tray icon exists; read by `has_system_tray`.
static TRAY_AVAILABLE: AtomicBool = AtomicBool::new(false);

/// SAFETY: `TrayIcon` uses `Rc<RefCell<…>>` internally on Windows, which is
/// not `Send`; `WindowsTray` is only ever created and accessed on the main
/// (egui) thread - the same thread that runs the Windows message loop. It is
/// never sent across threads, so `Rc<RefCell<…>>` internals from the
/// `tray_icon` crate cannot cause data races in practice.
// SAFETY: WindowsTray contains winapi HANDLEs which are Send-safe (HANDLEs
// can be used from any thread in the Windows API model).
unsafe impl Send for WindowsTray {}

/// Windows system-tray icon with a context menu (Show, Connect, Disconnect, Quit).
///
/// Created via [`WindowsPlatform::new_tray`]. Runs on the egui UI thread and
/// communicates back to the app via [`TrayHandle::poll_events`]. The menu is
/// built from [`tray_menu::presentation`], so its enabled rows, icon colour and
/// tooltip all come from the same state mapping the fifth (`RelayStarved`)
/// state lives in.
pub struct WindowsTray {
    icon: TrayIcon,
    item_show: MenuItem,
    item_connect: MenuItem,
    item_disconnect: MenuItem,
    item_quit: MenuItem,
    quit: QuitFlag,
    last_state: Cell<TrayState>,
    /// The language the current labels were built in; a Settings language
    /// change rebuilds the catalogued copy on the next frame.
    last_language: RefCell<String>,
}

impl WindowsTray {
    pub fn new(quit: QuitFlag) -> Option<Self> {
        // The menu opens on the icon's first state (disconnected) and is
        // re-localized/re-enabled by `set_state` on the first frame.
        let menu = tray_menu::presentation(TrayState::Disconnected);
        let enabled = |action: tray_menu::TrayMenuAction| {
            menu.items
                .iter()
                .find(|item| item.action == action)
                .map(|item| item.enabled)
                .unwrap_or(true)
        };
        let item_show = MenuItem::with_id(
            MENU_SHOW,
            tray_menu::TrayMenuAction::Show.label(),
            enabled(tray_menu::TrayMenuAction::Show),
            None,
        );
        let item_connect = MenuItem::with_id(
            MENU_CONNECT,
            tray_menu::TrayMenuAction::Connect.label(),
            enabled(tray_menu::TrayMenuAction::Connect),
            None,
        );
        let item_disconnect = MenuItem::with_id(
            MENU_DISCONNECT,
            tray_menu::TrayMenuAction::Disconnect.label(),
            enabled(tray_menu::TrayMenuAction::Disconnect),
            None,
        );
        let item_quit = MenuItem::with_id(
            MENU_QUIT,
            tray_menu::TrayMenuAction::Quit.label(),
            enabled(tray_menu::TrayMenuAction::Quit),
            quit_accelerator(),
        );

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
            .with_tooltip(format!(
                "\u{26a1} LightSpeed \u{2014} {}",
                i18n::t("state.disconnected")
            ))
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
            item_show,
            item_connect,
            item_disconnect,
            item_quit,
            quit,
            last_state: Cell::new(TrayState::Disconnected),
            last_language: RefCell::new(String::new()),
        })
    }
}

/// The Quit accelerator from the canonical keymap (`Ctrl+Q` on Windows).
///
/// The menu is the visible proof the shortcut is discoverable; the table is
/// owned by [`super::shortcuts`], never re-spelled here.
fn quit_accelerator() -> Option<Accelerator> {
    let row = super::shortcut_binding(ShortcutAction::Quit)?;
    Some(Accelerator::new(
        accelerators_modifiers(&row.chord),
        accelerator_code(row.chord.key),
    ))
}

fn accelerators_modifiers(chord: &KeyChord) -> Modifiers {
    let mut modifiers = Modifiers::empty();
    for modifier in chord.modifiers {
        modifiers |= match modifier {
            Modifier::Ctrl => Modifiers::CONTROL,
            // The Windows table never uses ⌘; mapping it keeps the helper total.
            Modifier::Meta => Modifiers::META,
        };
    }
    modifiers
}

fn accelerator_code(key: Key) -> Code {
    match key {
        Key::Q => Code::KeyQ,
        Key::Comma => Code::Comma,
        Key::H => Code::KeyH,
        Key::Escape => Code::Escape,
        Key::Enter => Code::Enter,
    }
}

impl TrayHandle for WindowsTray {
    fn poll_events(&self, ctx: &egui::Context) -> Vec<TrayAction> {
        let mut actions = Vec::new();
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            tracing::debug!("Tray menu event: {:?}", event.id);
            if event.id == self.item_show.id() {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if event.id == self.item_connect.id() {
                actions.push(TrayAction::Connect);
            } else if event.id == self.item_disconnect.id() {
                actions.push(TrayAction::Disconnect);
            } else if event.id == self.item_quit.id() {
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
        // Re-apply when the state OR the language changed; every other frame is
        // a no-op (no WM_SETICON / tooltip traffic on an idle window).
        let language = i18n::current();
        if self.last_state.get() == state && *self.last_language.borrow() == language {
            return;
        }
        self.last_state.set(state);
        *self.last_language.borrow_mut() = language;

        let presentation = tray_menu::presentation(state);
        // One mapping drives the menu: Connect and Disconnect are never both
        // offered, and a starving session can always disconnect.
        for item in &presentation.items {
            match item.action {
                tray_menu::TrayMenuAction::Show => self.item_show.set_enabled(item.enabled),
                tray_menu::TrayMenuAction::Connect => self.item_connect.set_enabled(item.enabled),
                tray_menu::TrayMenuAction::Disconnect => {
                    self.item_disconnect.set_enabled(item.enabled)
                }
                tray_menu::TrayMenuAction::Quit => self.item_quit.set_enabled(item.enabled),
            }
        }
        let tooltip = presentation.tooltip(rtt_ms);
        let (r, g, b) = presentation.icon_rgb;
        if let Some(icon) = lightning_icon(r, g, b) {
            let _ = self.icon.set_icon(Some(icon));
        }
        let _ = self.icon.set_tooltip(Some(&tooltip));
    }
}

/// Windows [`Platform`] backend.
///
/// Uses `tray_icon` for the system tray, the process token for elevation,
/// `SPI_GETCLIENTAREAANIMATION` for reduced motion, and Npcap detection via
/// `sc query` (the known wrong-dependency stub, plan S9).
pub struct WindowsPlatform;

/// `SPI_GETCLIENTAREAANIMATION`: non-zero means client-area animation is ON.
const SPI_GETCLIENTAREAANIMATION: u32 = 0x1042;

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

    /// The real reduced-motion signal (spec W2-4 (i)).
    ///
    /// `SPI_GETCLIENTAREAANIMATION` returns a non-zero `BOOL` when client-area
    /// animation is enabled; that is the polarity the spec's D5 rule uses. A
    /// failed query is `Unknown` (static), never an accidental `On`.
    fn animations() -> Animations {
        #[link(name = "user32")]
        extern "system" {
            fn SystemParametersInfoW(
                action: u32,
                param: u32,
                value: *mut core::ffi::c_void,
                winini: u32,
            ) -> i32;
        }
        let mut enabled: i32 = 0;
        // SAFETY: the pointer is to a live `i32` of the size the API writes,
        // and the call is a read-only query (`uiParam = 0`, `fWinIni = 0`).
        let ok = unsafe {
            SystemParametersInfoW(
                SPI_GETCLIENTAREAANIMATION,
                0,
                &mut enabled as *mut i32 as *mut core::ffi::c_void,
                0,
            )
        };
        let animations = Animations::from_windows_flag(ok != 0, enabled);
        tracing::debug!(
            "reduced-motion signal: {:?} ({})",
            animations,
            animations.source()
        );
        animations
    }

    fn has_system_tray() -> bool {
        TRAY_AVAILABLE.load(Ordering::SeqCst)
    }

    /// Relaunch elevated, but only when that would actually change anything.
    ///
    /// The legacy primitive: it spawns the request and exits immediately. The
    /// app uses [`Self::begin_elevation`] instead, which keeps this process
    /// alive until the elevated instance confirms (C8 fix).
    fn relaunch_as_admin() {
        if Self::is_admin() {
            tracing::info!("already elevated; not relaunching");
            return;
        }
        let Ok(exe) = std::env::current_exe() else {
            tracing::warn!("elevation: cannot resolve the executable path");
            return;
        };
        if let Err(e) = spawn_elevated_helper(&exe) {
            tracing::warn!("elevation helper failed to start: {e}");
        }
        std::process::exit(0);
    }

    /// The C8 fix: ask for elevation, keep this process alive, and let the
    /// frame loop exit only once the elevated instance confirms.
    ///
    /// The helper is a hidden PowerShell that runs `Start-Process -Verb RunAs`
    /// and exits 0 on consent or 1223 (`ERROR_CANCELLED`) on decline. The
    /// current process is still holding the single-instance guard while the
    /// elevated instance starts, so the elevated child is launched with
    /// `--force`: that bypass is the documented, logged escape hatch, and the
    /// old instance exits moments later.
    fn begin_elevation() -> ElevationAttempt {
        if Self::is_admin() {
            tracing::info!("already elevated; no elevation handshake needed");
            return ElevationAttempt::resolved(ElevationPhase::Ready);
        }
        let exe = match std::env::current_exe() {
            Ok(exe) => exe,
            Err(e) => {
                tracing::warn!("elevation: cannot resolve the executable path: {e}");
                return ElevationAttempt::resolved(ElevationPhase::Failed);
            }
        };
        match spawn_elevated_helper(&exe) {
            Ok(child) => {
                tracing::info!(
                    "elevation requested; waiting for the elevated instance to confirm before exiting"
                );
                ElevationAttempt::watch(child)
            }
            Err(e) => {
                tracing::warn!("elevation helper failed to start: {e}");
                ElevationAttempt::resolved(ElevationPhase::Failed)
            }
        }
    }
}

/// Spawn the hidden PowerShell that raises the UAC prompt.
///
/// The exit code contract is the handshake: 0 = the elevated instance started,
/// 1223 = the user declined the prompt, anything else = the launch failed.
fn spawn_elevated_helper(exe: &std::path::Path) -> std::io::Result<std::process::Child> {
    let exe = exe.display().to_string().replace('\'', "''");
    let script = format!(
        "try {{ Start-Process '{exe}' -ArgumentList '--force' -Verb RunAs -ErrorAction Stop | Out-Null; exit 0 }} catch {{ exit 1223 }}"
    );
    crate::platform::silent_command("powershell")
        .args(["-NoProfile", "-WindowStyle", "Hidden", "-Command", &script])
        .spawn()
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
