use super::*;

/// Decode the embedded brand tile into a texture for the About sheet.
///
/// A decode failure is logged and treated as absent so the sheet falls back
/// to plain text.
pub(super) fn brand_mark_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    const MARK_PNG: &[u8] = include_bytes!("../../../web/assets/brand/icon-256.png");
    let rgba = match image::load_from_memory(MARK_PNG) {
        Ok(decoded) => decoded.to_rgba8(),
        Err(e) => {
            tracing::warn!("could not decode header brand mark: {e}");
            return None;
        }
    };
    let size = [rgba.width() as usize, rgba.height() as usize];
    let image = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
    Some(ctx.load_texture("lightspeed-brand-mark", image, egui::TextureOptions::LINEAR))
}

/// What the frame loop should do about a window close or a tray Quit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CloseDecision {
    /// Neither a close request nor a quit flag: keep running normally.
    Ignore,
    /// The window's X was clicked and a real tray exists: hide, keep running.
    Hide,
    /// Quit was requested, or the window closed with no tray to recover from:
    /// tear the engine down and exit.
    Exit,
}

/// The shortest the status window gets. `main.rs` builds the window with it as
/// the minimum, and the auto-size never goes below it.
pub(crate) const MIN_WINDOW_HEIGHT: f32 = 420.0;

/// The tallest the status window gets, even on a screen that could hold more.
/// `main.rs` builds the window with it as the maximum.
pub(crate) const MAX_WINDOW_HEIGHT: f32 = 1400.0;

/// Points kept free below the window for a taskbar or dock. The monitor size is
/// the whole screen, not the work area, so this is an allowance, not a reading.
pub(super) const SCREEN_BOTTOM_RESERVE: f32 = 72.0;

/// Points assumed above the client area when the window position is unknown
/// (Wayland, headless) or not on the reported monitor: a title bar on a window
/// at the top.
pub(super) const TITLE_BAR_ALLOWANCE: f32 = 40.0;

/// The current monitor's height and the client area's top edge measured from
/// that monitor's top, both in points.
///
/// Read from winit, not from egui's `ViewportInfo`: its `inner_rect` is a
/// desktop-wide coordinate divided by this window's scale factor, while
/// `monitor_size` is local to one monitor, so on stacked or mixed-DPI monitors
/// the two do not compare. winit reports the monitor's origin in the same
/// physical pixels as the window, so the offset is taken in pixels and only
/// then scaled (see [`monitor_local_top`]). With no winit window (headless) the
/// egui monitor size is used; where winit cannot place the window (Wayland) the
/// top is `None`. Either way [`window_height_limit`] then assumes the top.
pub(super) fn monitor_geometry(
    frame: &eframe::Frame,
    ctx: &egui::Context,
) -> (Option<f32>, Option<f32>) {
    let pixels_per_point = ctx.pixels_per_point();
    let Some((window, monitor)) = frame
        .winit_window()
        .and_then(|window| Some((window, window.current_monitor()?)))
    else {
        return (
            ctx.input(|i| i.viewport().monitor_size.map(|size| size.y)),
            None,
        );
    };
    let height = monitor.size().height as f32 / pixels_per_point;
    let top = window
        .inner_position()
        .ok()
        .map(|inner| monitor_local_top(inner.y, monitor.position().y, pixels_per_point));
    (Some(height), top)
}

/// The client area's top edge on its own monitor, in points. Both inputs are
/// physical-pixel desktop coordinates, so they are subtracted before scaling:
/// scaling a desktop coordinate first would fold other monitors, at their own
/// scale, into this one's.
pub(super) fn monitor_local_top(
    window_top_px: i32,
    monitor_top_px: i32,
    pixels_per_point: f32,
) -> f32 {
    (window_top_px - monitor_top_px) as f32 / pixels_per_point
}

/// The tallest the status window may auto-size to, in points.
///
/// `monitor_height` is the current monitor's full height and `window_top` the
/// top of the window's client area measured from that monitor's top edge (see
/// [`monitor_geometry`]). The window must end above the taskbar or dock, or the
/// pinned action bar below the scroll region is drawn off-screen. A top outside
/// `0..monitor_height` - a window straddling two monitors, its top on the other
/// one - is ignored and the window is assumed to sit at the top of this one.
/// Never below the window's minimum height; with no monitor reading, the fixed
/// maximum.
pub(super) fn window_height_limit(monitor_height: Option<f32>, window_top: Option<f32>) -> f32 {
    let Some(monitor_height) = monitor_height.filter(|h| *h > 0.0) else {
        return MAX_WINDOW_HEIGHT;
    };
    let top = window_top
        .filter(|top| (0.0..monitor_height).contains(top))
        .unwrap_or(TITLE_BAR_ALLOWANCE);
    (monitor_height - SCREEN_BOTTOM_RESERVE - top).clamp(MIN_WINDOW_HEIGHT, MAX_WINDOW_HEIGHT)
}

/// The GUI-owned window geometry state (presentation state, not settings).
///
/// The height belongs to the player, not to the status snapshot: W2-2 removed
/// the content-driven auto-resize and remembers the last height across
/// launches. It is kept beside — not inside — the shared `config.toml` because
/// the core schema has no UI-geometry fields and W2-2's write scope excludes
/// the core crate.
const WINDOW_STATE_FILE: &str = "ui-window-state.toml";

/// The remembered window height, clamped to the launched window's bounds.
pub(super) fn load_window_height(monitor_height: Option<f32>, window_top: Option<f32>) -> f32 {
    let limit = window_height_limit(monitor_height, window_top);
    let saved = std::fs::read_to_string(crate::paths::config_dir().join(WINDOW_STATE_FILE))
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.trim().strip_prefix("height =").map(str::trim))
                .and_then(|value| value.parse::<f32>().ok())
        });
    saved
        .filter(|height| height.is_finite())
        .unwrap_or(MIN_WINDOW_HEIGHT)
        .clamp(MIN_WINDOW_HEIGHT, limit)
}

/// Persist the window height for the next launch. Failures are ignored: a
/// read-only config directory must not break the window.
pub(super) fn save_window_height(height: f32) {
    if !height.is_finite() {
        return;
    }
    let dir = crate::paths::config_dir();
    let _ = std::fs::create_dir_all(&dir);
    let _ = std::fs::write(
        dir.join(WINDOW_STATE_FILE),
        format!("height = {height:.0}\n"),
    );
}

/// Whether discovery should start a latency race instead of connecting to the
/// first relay: only when auto-select is on, disconnected, and relays exist.
pub fn should_race(auto_select: bool, connected: bool, proxy_count: usize) -> bool {
    auto_select && !connected && proxy_count > 0
}

/// Whether a finished latency race may still steer the selection. A relay the
/// user pinned or a connection they started meanwhile wins.
/// Whether a finished client-leg race may choose the relay.
///
/// The race ranks the relay closest to the player, which is the wrong question
/// once the game server is known: [`Self::choose_relay_for_destination`] ranks
/// the whole path by the server-side leg. Defer to it whenever a server has been
/// locked, so a client-leg winner cannot become the data-plane relay.
pub fn should_apply_race(auto_select: bool, connected: bool, server_known: bool) -> bool {
    auto_select && !connected && !server_known
}

/// The tray state for the current frame. The rule moved to the shared crate at
/// W2-1 S3a; this is the re-export the window and its tests address.
pub use lightspeed_client::app::status::tray_state_for;

pub fn close_decision(
    has_system_tray: bool,
    quit_requested: bool,
    close_requested: bool,
) -> CloseDecision {
    if quit_requested {
        return CloseDecision::Exit;
    }
    if close_requested {
        if has_system_tray {
            CloseDecision::Hide
        } else {
            CloseDecision::Exit
        }
    } else {
        CloseDecision::Ignore
    }
}

/// Fold a discovery outcome into the current relay list. Discovered relays are
/// refreshed; custom (user-added) entries are always kept. A failed attempt
/// leaves the list untouched and returns a user-facing reason.
pub fn apply_discovery_result(
    existing: &[ProxyEntry],
    outcome: &DiscoveryOutcome,
) -> (Vec<ProxyEntry>, Option<String>) {
    match outcome {
        DiscoveryOutcome::Found(relays) => {
            let discovered = relays
                .iter()
                .cloned()
                .map(discovery::RelayInfo::into_entry)
                .collect();
            (config::merge_discovered(existing, discovered), None)
        }
        DiscoveryOutcome::Failed(reason) => (existing.to_vec(), Some(reason.clone())),
    }
}
