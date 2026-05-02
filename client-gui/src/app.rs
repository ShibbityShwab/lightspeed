//! egui application + Windows tray icon for LightSpeed GUI.
//!
//! This module is only compiled on Windows (`#[cfg(windows)]` in main.rs).

use std::net::SocketAddrV4;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

// ── Tray state enum ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum TrayState {
    Disconnected,
    Connected,
    Optimizing,
    Error,
}
use lightspeed_client::{EngineStatus, LightSpeedEngine};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};

// ── Proxy nodes ────────────────────────────────────────────────────────────

/// (addr, label, recommended-for hint)
const PROXIES: &[(&str, &str, &str)] = &[
    (
        "149.28.84.139:4434",
        "LAX — US West",
        "Best for NA/EU servers (you are 206ms away)",
    ),
    (
        "149.28.144.74:4434",
        "SGP — Singapore",
        "Best for SEA/AU servers (you are 31ms away)",
    ),
];

// ── Game list ──────────────────────────────────────────────────────────────

/// (key, display name, default port)
const GAMES: &[(&str, &str, u16)] = &[
    ("rust", "Rust (Facepunch)", 28015),
    ("fortnite", "Fortnite", 7777),
    ("cs2", "Counter-Strike 2", 27015),
    ("dota2", "Dota 2", 27015),
    ("valorant", "Valorant", 7000),
    ("apex", "Apex Legends", 37015),
    ("ow2", "Overwatch 2", 3724),
    ("lol", "League of Legends", 5000),
    ("pubg", "PUBG: Battlegrounds", 7777),
];

// ── Tray menu item IDs ──────────────────────────────────────────────────────

const MENU_SHOW: &str = "show";
const MENU_CONNECT: &str = "connect";
const MENU_DISCONNECT: &str = "disconnect";
const MENU_QUIT: &str = "quit";

// ── App struct ───────────────────────────────────────────────────────────────

pub struct LightSpeedApp {
    engine: Arc<Mutex<LightSpeedEngine>>,
    status: EngineStatus,
    tray: TrayIcon,
    last_tray_state: TrayState,
    fonts_setup: bool,

    // ── Tray menu IDs ─────────────────────────────────────────────────────
    id_show: tray_icon::menu::MenuId,
    id_connect: tray_icon::menu::MenuId,
    id_disconnect: tray_icon::menu::MenuId,
    id_quit: tray_icon::menu::MenuId,

    // ── Proxy connection ─────────────────────────────────────────────────
    selected_proxy_idx: usize,
    show_connect_dialog: bool,
    custom_proxy_input: String,

    // ── Game routing ──────────────────────────────────────────────────────
    selected_game_idx: usize,
    server_input: String,
    fec_enabled: bool,
    auto_detected_game: Option<String>,

    // ── System state ──────────────────────────────────────────────────────
    #[allow(dead_code)]
    npcap_installed: bool,
    is_admin: bool,

    // ── Advanced panel toggle ─────────────────────────────────────────────
    /// True = "Advanced" expander is open (shows manual server IP input).
    show_advanced: bool,

    // ── WinDivert diagnostics ─────────────────────────────────────────────
    /// Wall-clock time when the current WinDivert boost session started.
    /// Used to show a "port mismatch?" warning if no packets are seen after 15 s.
    boost_start: Option<std::time::Instant>,
    /// Optional user-supplied port range override (e.g. "28015-28999").
    /// When non-empty and parseable it overrides `windivert_port_range()`.
    custom_port_input: String,
}

impl LightSpeedApp {
    pub fn new(engine: Arc<Mutex<LightSpeedEngine>>) -> Self {
        let (tray, id_show, id_connect, id_disconnect, id_quit) = build_tray();
        let status = engine.lock().unwrap().snapshot();

        let auto_detected_game = try_auto_detect_game();
        let selected_game_idx = auto_detected_game
            .as_deref()
            .and_then(|name| GAMES.iter().position(|(key, _, _)| key.eq_ignore_ascii_case(name)))
            .unwrap_or(0);

        let npcap_installed = check_npcap();
        let is_admin = check_is_admin();

        Self {
            engine,
            status,
            tray,
            last_tray_state: TrayState::Disconnected,
            fonts_setup: false,
            id_show,
            id_connect,
            id_disconnect,
            id_quit,
            selected_proxy_idx: 0,
            show_connect_dialog: false,
            custom_proxy_input: String::new(),
            selected_game_idx,
            server_input: String::new(),
            fec_enabled: false,
            auto_detected_game,
            npcap_installed,
            is_admin,
            show_advanced: false,
            boost_start: None,
            custom_port_input: String::new(),
        }
    }

    fn selected_proxy_addr(&self) -> SocketAddrV4 {
        PROXIES[self.selected_proxy_idx]
            .0
            .parse()
            .expect("proxy addr is always valid")
    }

}

// ── Tray builder ─────────────────────────────────────────────────────────────

fn build_tray() -> (
    TrayIcon,
    tray_icon::menu::MenuId,
    tray_icon::menu::MenuId,
    tray_icon::menu::MenuId,
    tray_icon::menu::MenuId,
) {
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

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("\u{26a1} LightSpeed \u{2014} disconnected")
        .with_icon(icon)
        .build()
        .expect("Failed to create tray icon");

    (tray, id_show, id_connect, id_disconnect, id_quit)
}

/// Procedurally rasterise a ⚡ lightning-bolt silhouette into a 32×32 RGBA icon.
///
/// Uses a 6-vertex non-convex polygon (ray-casting fill) so no image crate is needed.
/// `(r, g, b)` sets the bolt colour; background is transparent.
fn lightning_icon(r: u8, g: u8, b: u8) -> tray_icon::Icon {
    const SIZE: usize = 32;
    // Bolt polygon: 6 vertices, clockwise, y-increasing downwards, normalised [0,1].
    // Vertex sequence traces the ⚡ outline: top → mid-left → inner-jog → bottom → mid-right → inner-jog.
    let poly: [(f32, f32); 6] = [
        (0.55, 0.02), // top
        (0.18, 0.48), // mid-left
        (0.50, 0.48), // inner concave corner (step right)
        (0.10, 0.98), // bottom-left tip
        (0.82, 0.52), // mid-right
        (0.50, 0.52), // inner concave corner (step left)
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

/// Ray-casting point-in-polygon test (works for non-convex simple polygons).
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

/// Load Segoe UI Emoji (and Symbol) from the Windows Fonts directory as egui fallback fonts.
///
/// Called once on first frame. Silently skipped if the files are absent (older OS / CI).
/// Text still renders correctly using egui's bundled Ubuntu-Light; the system emoji font
/// adds coverage for glyphs like 🎮 🟢 🪄 🔑 ✅ that are absent from the bundled subset.
fn setup_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();

    // Segoe UI Emoji — full Unicode 14+ colour/monochrome emoji (Windows 10/11).
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguiemj.ttf") {
        fonts
            .font_data
            .insert("seguiemj".to_owned(), egui::FontData::from_owned(bytes));
        for family in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts
                .families
                .entry(family)
                .or_default()
                .push("seguiemj".to_owned());
        }
    }

    // Segoe UI Symbol — BMP symbol coverage (⚡ ⚠ ▶ ■ ● ℹ ↗ etc.).
    if let Ok(bytes) = std::fs::read(r"C:\Windows\Fonts\seguisym.ttf") {
        fonts
            .font_data
            .insert("seguisym".to_owned(), egui::FontData::from_owned(bytes));
        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .push("seguisym".to_owned());
    }

    ctx.set_fonts(fonts);
}

// ── eframe::App impl ─────────────────────────────────────────────────────────

impl eframe::App for LightSpeedApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // One-time first-frame setup: emoji font fallback.
        if !self.fonts_setup {
            self.fonts_setup = true;
            setup_fonts(ctx);
        }

        // ── Poll tray/menu events each frame ─────────────────────────────
        // Using receiver() instead of set_event_handler() so events are
        // delivered correctly even when running as Administrator (UIPI
        // blocks callback-based messages from Explorer at medium IL).
        while let Ok(event) = MenuEvent::receiver().try_recv() {
            tracing::debug!("Tray menu event: {:?}", event.id);
            if event.id == self.id_show {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if event.id == self.id_connect {
                let proxy = self.selected_proxy_addr();
                self.engine.lock().unwrap().connect(proxy);
            } else if event.id == self.id_disconnect {
                self.engine.lock().unwrap().disconnect();
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

        // Intercept close → hide to tray.
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }

        // Refresh engine snapshot.
        self.status = self.engine.lock().unwrap().snapshot();

        // ── Tray icon state machine ───────────────────────────────────────
        {
            let has_error = self.status.windivert_error.is_some()
                || self.status.capture_error.is_some()
                || self.status.redirect_error.is_some()
                || self.status.interceptor_error.is_some();
            let new_tray_state = if has_error {
                TrayState::Error
            } else if self.status.windivert_active
                || self.status.capture_active
                || self.status.redirect_active
                || self.status.interceptor_active
            {
                TrayState::Optimizing
            } else if self.status.connected {
                TrayState::Connected
            } else {
                TrayState::Disconnected
            };

            if new_tray_state != self.last_tray_state {
                self.last_tray_state = new_tray_state;
                let (r, g, b): (u8, u8, u8) = match new_tray_state {
                    TrayState::Disconnected => (160, 160, 160), // gray
                    TrayState::Connected    => (255, 200,  60), // amber
                    TrayState::Optimizing   => ( 80, 210, 120), // green
                    TrayState::Error        => (220,  80,  80), // red
                };
                let tooltip: String = match new_tray_state {
                    TrayState::Disconnected => "\u{26a1} LightSpeed \u{2014} disconnected".into(),
                    TrayState::Connected    => format!(
                        "\u{26a1} LightSpeed \u{2014} connected \u{00b7} RTT {:.0}ms",
                        self.status.latest_rtt_ms
                    ),
                    TrayState::Optimizing   => "\u{26a1} LightSpeed \u{2014} optimizing".into(),
                    TrayState::Error        => "\u{26a1} LightSpeed \u{2014} error".into(),
                };
                let _ = self.tray.set_icon(Some(lightning_icon(r, g, b)));
                let _ = self.tray.set_tooltip(Some(&tooltip));
            }
        }

        // ── Main panel ────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            // ── Header ───────────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.heading("⚡ LightSpeed");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (label, colour) = if self.status.connected {
                        ("● Connected", egui::Color32::from_rgb(80, 200, 120))
                    } else {
                        ("● Disconnected", egui::Color32::from_rgb(220, 80, 80))
                    };
                    ui.colored_label(colour, label);
                });
            });

            ui.separator();

            // ── Boost Server selector ─────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Boost Server:")
                    .on_hover_ui(|ui| {
                        ui.label("Choose the relay server closest to your game server.\n\
                                  Closer to the game server = lower ping, even if it's\n\
                                  farther from your physical location.");
                        ui.hyperlink_to("📖 Which server should I pick?",
                            "https://github.com/ShibbityShwab/lightspeed/wiki/Choosing-a-Boost-Server");
                    });
                let prev = self.selected_proxy_idx;
                for (i, (_, label, hint)) in PROXIES.iter().enumerate() {
                    let btn = ui.selectable_value(&mut self.selected_proxy_idx, i, *label);
                    btn.on_hover_text(*hint);
                }
                if self.selected_proxy_idx != prev {
                    let proxy = self.selected_proxy_addr();
                    self.engine.lock().unwrap().connect(proxy);
                }
            });

            ui.horizontal(|ui| {
                ui.label("Boost Ping:")
                    .on_hover_ui(|ui| {
                        ui.label("Round-trip time from your PC to the Boost Server.\n\
                                  🟢 < 60ms  |  🟡 60–120ms  |  🔴 > 120ms\n\
                                  This becomes your in-game ping when Boost is engaged.");
                        ui.hyperlink_to("📖 Understanding ping",
                            "https://github.com/ShibbityShwab/lightspeed/wiki/Understanding-Ping");
                    });
                if self.status.connected && self.status.latest_rtt_ms > 0.0 {
                    let rtt = self.status.latest_rtt_ms;
                    ui.colored_label(rtt_colour(rtt), format!("{:.1} ms", rtt));
                } else if self.status.connected {
                    ui.weak("measuring…");
                } else {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), "offline");
                }
                ui.separator();
                ui.label(format!(
                    "Heartbeat: {} out / {} in",
                    self.status.packets_sent, self.status.packets_received
                ))
                .on_hover_text(
                    "Small 'are you still there?' messages sent every 5 seconds \
                     to keep the connection alive and measure latency."
                );
            });

            // RTT sparkline
            if !self.status.rtt_history.is_empty() {
                let points: PlotPoints = self
                    .status
                    .rtt_history
                    .iter()
                    .enumerate()
                    .map(|(i, &v)| [i as f64, v])
                    .collect();
                let line = Line::new(points)
                    .color(egui::Color32::from_rgb(100, 180, 255))
                    .name("RTT (ms)");
                Plot::new("rtt_plot")
                    .height(80.0)
                    .allow_drag(false)
                    .allow_zoom(false)
                    .allow_scroll(false)
                    .show_axes([false, true])
                    .show(ui, |plot_ui| plot_ui.line(line));
            } else {
                ui.add_space(80.0);
            }

            ui.separator();

            // ── Game Routing section ──────────────────────────────────────
            ui.heading("🎮 Game Routing");
            ui.add_space(4.0);

            let any_active =
                self.status.redirect_active
                    || self.status.capture_active
                    || self.status.windivert_active
                    || self.status.interceptor_active;

            if self.status.interceptor_active {
                // ── BOOST ENGAGED (OOP Interceptor) state ──────────────────────
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 200, 60),
                        "⚡ BOOST ENGAGED",
                    );
                    if !self.status.interceptor_server.is_empty() {
                        ui.label(format!(" — {}", self.status.interceptor_server))
                            .on_hover_text("The game server your packets are being routed through the Boost Server to reach.");
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Packets Sent:")
                        .on_hover_ui(|ui| {
                            ui.label("Game packets captured and forwarded to the Boost Server.");
                            ui.hyperlink_to("📖 What the numbers mean",
                                "https://github.com/ShibbityShwab/lightspeed/wiki/What-The-Numbers-Mean");
                        });
                    ui.monospace(format!("{:>8}", self.status.interceptor_intercepted));
                    ui.separator();
                    ui.label("Returned:")
                        .on_hover_text("Responses received from the Boost Server (relayed from game server).");
                    ui.monospace(format!("{:>8}", self.status.interceptor_from_proxy));
                    ui.separator();
                    ui.label("Delivered:")
                        .on_hover_text("Responses injected back into your game — your game sees these as coming directly from the game server.");
                    ui.monospace(format!("{:>8}", self.status.interceptor_injected));
                });
                if self.status.interceptor_errors > 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        format!("⚠ Drops: {}", self.status.interceptor_errors),
                    )
                    .on_hover_ui(|ui| {
                        ui.label("Packets that couldn't be delivered back to your game.\n\
                                  Usually a firewall issue — see Troubleshooting.");
                        ui.hyperlink_to("📖 Fix Drops",
                            "https://github.com/ShibbityShwab/lightspeed/wiki/Troubleshooting#packets-sent-climbing-packets-delivered-0");
                    });
                }

                ui.add_space(4.0);
                if self.status.interceptor_intercepted == 0 {
                    // No packets yet — waiting for game traffic.
                    let elapsed = self.boost_start
                        .map(|t| t.elapsed().as_secs())
                        .unwrap_or(0);

                    if elapsed < 15 {
                        // First 15 s: friendly "finding server" indicator.
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(20, 30, 45))
                            .rounding(4.0)
                            .inner_margin(8.0)
                            .show(ui, |ui: &mut egui::Ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(120, 180, 255),
                                    "🎯 Finding your game server…",
                                );
                                ui.weak(
                                    "Launch your game and connect to a server.\n\
                                     Your connection is passing through normally until we lock on.",
                                );
                            });
                    } else {
                        // 15 s+ with no packets → likely port mismatch — amber warning.
                        let (lo, hi) = parse_custom_port_range(&self.custom_port_input)
                            .unwrap_or_else(|| windivert_port_range(self.selected_game_idx));
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(55, 40, 8))
                            .rounding(4.0)
                            .inner_margin(8.0)
                            .show(ui, |ui: &mut egui::Ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 190, 60),
                                    "⚠ No game traffic seen — possible port mismatch",
                                );
                                ui.weak(format!(
                                    "Watching ports {lo}–{hi}. Your server may be on a \
                                     different port.\n\
                                     Stop Boost, open ▶ Advanced, set a Custom Port Range, \
                                     then click BOOST MY GAME again.",
                                ));
                                ui.hyperlink_to(
                                    "📖 Fix: port not detected",
                                    "https://github.com/ShibbityShwab/lightspeed/wiki/Troubleshooting#port-not-detected",
                                );
                            });
                    }
                } else {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(25, 40, 15))
                        .rounding(4.0)
                        .inner_margin(8.0)
                        .show(ui, |ui: &mut egui::Ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(150, 255, 150),
                                "✅ Boost active — play normally, your game is fully optimised.",
                            );
                            ui.weak(
                                "Your in-game ping now reflects the Boost Server route. \
                                 If you switch servers, LightSpeed will re-detect automatically.",
                            );
                        });
                }

                ui.add_space(6.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("■ Stop Boost")
                            .fill(egui::Color32::from_rgb(160, 45, 45)),
                    )
                    .on_hover_text("Stop routing game traffic through the Boost Server and return to your normal connection.")
                    .clicked()
                {
                    self.engine.lock().unwrap().stop_interceptor();
                    self.boost_start = None;
                }

                if let Some(ref err) = self.status.interceptor_error {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        format!("⚠ Error: {}", err),
                    );
                }
            } else if self.status.windivert_active {
                // ── BOOST ENGAGED (WinDivert) state ──────────────────────
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(255, 200, 60),
                        "⚡ BOOST ENGAGED",
                    );
                    if !self.status.windivert_server.is_empty() {
                        ui.label(format!(" — {}", self.status.windivert_server))
                            .on_hover_text("The game server your packets are being routed through the Boost Server to reach.");
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("Packets Sent:")
                        .on_hover_ui(|ui| {
                            ui.label("Game packets captured and forwarded to the Boost Server.");
                            ui.hyperlink_to("📖 What the numbers mean",
                                "https://github.com/ShibbityShwab/lightspeed/wiki/What-The-Numbers-Mean");
                        });
                    ui.monospace(format!("{:>8}", self.status.windivert_intercepted));
                    ui.separator();
                    ui.label("Returned:")
                        .on_hover_text("Responses received from the Boost Server (relayed from game server).");
                    ui.monospace(format!("{:>8}", self.status.windivert_from_proxy));
                    ui.separator();
                    ui.label("Delivered:")
                        .on_hover_text("Responses injected back into your game — your game sees these as coming directly from the game server.");
                    ui.monospace(format!("{:>8}", self.status.windivert_injected));
                });
                if self.status.windivert_errors > 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        format!("⚠ Drops: {}", self.status.windivert_errors),
                    )
                    .on_hover_ui(|ui| {
                        ui.label("Packets that couldn't be delivered back to your game.\n\
                                  Usually a firewall issue — see Troubleshooting.");
                        ui.hyperlink_to("📖 Fix Drops",
                            "https://github.com/ShibbityShwab/lightspeed/wiki/Troubleshooting#packets-sent-climbing-packets-delivered-0");
                    });
                }

                ui.add_space(4.0);
                if self.status.windivert_intercepted == 0 {
                    // No packets yet — waiting for game traffic.
                    let elapsed = self.boost_start
                        .map(|t| t.elapsed().as_secs())
                        .unwrap_or(0);

                    if elapsed < 15 {
                        // First 15 s: friendly "finding server" indicator.
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(20, 30, 45))
                            .rounding(4.0)
                            .inner_margin(8.0)
                            .show(ui, |ui: &mut egui::Ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(120, 180, 255),
                                    "🎯 Finding your game server…",
                                );
                                ui.weak(
                                    "Launch your game and connect to a server.\n\
                                     Your connection is passing through normally until we lock on.",
                                );
                            });
                    } else {
                        // 15 s+ with no packets → likely port mismatch — amber warning.
                        let (lo, hi) = parse_custom_port_range(&self.custom_port_input)
                            .unwrap_or_else(|| windivert_port_range(self.selected_game_idx));
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgb(55, 40, 8))
                            .rounding(4.0)
                            .inner_margin(8.0)
                            .show(ui, |ui: &mut egui::Ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 190, 60),
                                    "⚠ No game traffic seen — possible port mismatch",
                                );
                                ui.weak(format!(
                                    "Watching ports {lo}–{hi}. Your server may be on a \
                                     different port.\n\
                                     Stop Boost, open ▶ Advanced, set a Custom Port Range, \
                                     then click BOOST MY GAME again.",
                                ));
                                ui.hyperlink_to(
                                    "📖 Fix: port not detected",
                                    "https://github.com/ShibbityShwab/lightspeed/wiki/Troubleshooting#port-not-detected",
                                );
                            });
                    }
                } else {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(25, 40, 15))
                        .rounding(4.0)
                        .inner_margin(8.0)
                        .show(ui, |ui: &mut egui::Ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(150, 255, 150),
                                "✅ Boost active — play normally, your game is fully optimised.",
                            );
                            ui.weak(
                                "Your in-game ping now reflects the Boost Server route. \
                                 If you switch servers, LightSpeed will re-detect automatically.",
                            );
                        });
                }

                ui.add_space(6.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("■ Stop Boost")
                            .fill(egui::Color32::from_rgb(160, 45, 45)),
                    )
                    .on_hover_text("Stop routing game traffic through the Boost Server and return to your normal connection.")
                    .clicked()
                {
                    self.engine.lock().unwrap().stop_windivert();
                    self.boost_start = None;
                }

                if let Some(ref err) = self.status.windivert_error {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        format!("⚠ Error: {}", err),
                    );
                }
            } else if self.status.capture_active {
                // ── BOOST ACTIVE (capture/pcap mode) ─────────────────────
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(80, 200, 120),
                        "⚡ BOOST ENGAGED",
                    );
                    ui.label(format!(
                        " — {} ({})",
                        self.status.capture_game, self.status.capture_interface,
                    ));
                });

                // Live packet stats
                ui.horizontal(|ui| {
                    ui.label("Packets Boosted:")
                        .on_hover_text("Game packets captured and forwarded to the Boost Server.");
                    ui.monospace(format!("{:>8}", self.status.capture_pkts_out));
                    ui.separator();
                    ui.label("Returned:")
                        .on_hover_text("Responses received from the Boost Server.");
                    ui.monospace(format!("{:>8}", self.status.capture_pkts_in));
                });
                if self.status.capture_errors > 0 {
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        format!("⚠ Drops: {}", self.status.capture_errors),
                    )
                    .on_hover_text("Packets that couldn't be delivered — check your firewall settings.");
                }
                if self.status.capture_fec && self.status.capture_fec_recovered > 0 {
                    ui.label(format!(
                        "🛡 Lost packets recovered: {}",
                        self.status.capture_fec_recovered
                    ))
                    .on_hover_text("Reliability Shield recovered these dropped packets before your game noticed.");
                }

                // Diagnostic: proxy working but no game packets seen yet.
                if self.status.capture_pkts_in > 5 && self.status.capture_pkts_out == 0 {
                    ui.add_space(2.0);
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(55, 44, 8))
                        .rounding(4.0)
                        .inner_margin(8.0)
                        .show(ui, |ui: &mut egui::Ui| {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 210, 60),
                                "⚠ No game traffic detected yet.",
                            );
                            ui.weak("• Make sure your game is connected to a server (not just the menu).");
                            ui.weak("• If using a non-standard port, use Advanced — set server manually.");
                        });
                }

                ui.add_space(4.0);
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(20, 45, 30))
                    .rounding(4.0)
                    .inner_margin(8.0)
                    .show(ui, |ui: &mut egui::Ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(150, 255, 150),
                            "✅ Boost active — just play normally.",
                        );
                        ui.weak("LightSpeed is silently rerouting your game traffic.");
                    });

                ui.add_space(6.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("■  Stop Boost")
                            .fill(egui::Color32::from_rgb(160, 45, 45)),
                    )
                    .on_hover_text("Stop the boost and return to your normal connection.")
                    .clicked()
                {
                    self.engine.lock().unwrap().stop_capture();
                }

                if let Some(ref err) = self.status.capture_error {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        format!("⚠ Error: {}", err),
                    );
                }
            } else if self.status.redirect_active {
                // ── MANUAL BOOST ACTIVE ───────────────────────────────────
                ui.horizontal(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(80, 200, 120), "⚡ BOOST ENGAGED (manual)");
                    ui.label(format!(
                        " — {} → port {}",
                        self.status.redirect_game, self.status.redirect_local_port,
                    ));
                });
                ui.label(format!("Game server:  {}", self.status.redirect_server))
                    .on_hover_text("The real game server your traffic is being routed to.");

                ui.horizontal(|ui| {
                    ui.label("Packets Sent:")
                        .on_hover_text("Game packets forwarded to the Boost Server.");
                    ui.monospace(format!("{:>8}", self.status.redirect_pkts_out));
                    ui.separator();
                    ui.label("Returned:")
                        .on_hover_text("Responses from the Boost Server.");
                    ui.monospace(format!("{:>8}", self.status.redirect_pkts_in));
                    ui.separator();
                    let err_colour = if self.status.redirect_errors > 0 {
                        egui::Color32::from_rgb(220, 80, 80)
                    } else {
                        egui::Color32::GRAY
                    };
                    ui.colored_label(err_colour, format!("Drops: {}", self.status.redirect_errors))
                        .on_hover_text("Packets dropped in transit.");
                });

                if self.status.redirect_fec {
                    ui.label(format!(
                        "🛡 Reliability Shield — parity: {}  recovered: {}",
                        self.status.redirect_fec_parity, self.status.redirect_fec_recovered,
                    ))
                    .on_hover_ui(|ui| {
                        ui.label("Reliability Shield (FEC) is active. Extra data is sent so dropped \
                                  packets can be reconstructed by the Boost Server.");
                        ui.hyperlink_to("📖 About Reliability Shield",
                            "https://github.com/ShibbityShwab/lightspeed/wiki/Reliability-Shield");
                    });
                }

                ui.add_space(4.0);
                let instruction =
                    connect_instruction(self.selected_game_idx, self.status.redirect_local_port);
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(30, 50, 30))
                    .rounding(4.0)
                    .inner_margin(8.0)
                    .show(ui, |ui: &mut egui::Ui| {
                        ui.colored_label(egui::Color32::from_rgb(150, 255, 150), &instruction);
                    });

                ui.add_space(6.0);
                if ui
                    .add_sized(
                        [ui.available_width(), 32.0],
                        egui::Button::new("■  Stop Boost")
                            .fill(egui::Color32::from_rgb(160, 45, 45)),
                    )
                    .on_hover_text("Stop boost and return to your normal connection.")
                    .clicked()
                {
                    self.engine.lock().unwrap().stop_redirect();
                }

                if let Some(ref err) = self.status.redirect_error {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        format!("⚠ Error: {}", err),
                    );
                }
            } else {
                // ── IDLE: single Optimize button ──────────────────────────
                let _ = any_active;

                // ── Game auto-detect banner ───────────────────────────────
                if let Some(ref detected) = self.auto_detected_game {
                    ui.horizontal(|ui| {
                        ui.colored_label(egui::Color32::from_rgb(80, 200, 120), "🎮 Game found:")
                            .on_hover_text("LightSpeed automatically detected a running game.");
                        ui.label(detected);
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.weak("No game running — select your game and click Boost")
                            .on_hover_text(
                                "Start your game and connect to a server, then click \
                                 BOOST MY GAME. Or select your game manually below.",
                            );
                        if ui.small_button("🔄 Rescan").clicked() {
                            self.auto_detected_game = try_auto_detect_game();
                            if let Some(ref name) = self.auto_detected_game {
                                if let Some(idx) = GAMES
                                    .iter()
                                    .position(|(k, _, _)| k.eq_ignore_ascii_case(name))
                                {
                                    self.selected_game_idx = idx;
                                }
                            }
                        }
                    });
                }

                // ── Game dropdown ─────────────────────────────────────────
                ui.horizontal(|ui| {
                    ui.label("Game:  ")
                        .on_hover_text("Select the game you want to boost. LightSpeed will \
                                        automatically route its traffic for lower ping.");
                    egui::ComboBox::from_id_salt("game_select")
                        .selected_text(GAMES[self.selected_game_idx].1)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (i, (_, display, _)) in GAMES.iter().enumerate() {
                                ui.selectable_value(&mut self.selected_game_idx, i, *display);
                            }
                        });
                });

                ui.add_space(4.0);

                // ── Reliability Shield (FEC) toggle ───────────────────────
                ui.horizontal(|ui| {
                    ui.checkbox(
                        &mut self.fec_enabled,
                        "🛡 Reliability Shield — recover lost packets (+25% data)",
                    )
                    .on_hover_ui(|ui| {
                        ui.label(
                            "Reliability Shield sends extra repair data so the Boost Server \
                             can reconstruct any packets your connection drops — no more \
                             rubber-banding from packet loss. Uses ~25% extra upload bandwidth.",
                        );
                        ui.hyperlink_to(
                            "📖 Learn more about Reliability Shield",
                            "https://github.com/ShibbityShwab/lightspeed/wiki/Reliability-Shield",
                        );
                    });
                });

                ui.add_space(8.0);

                // ── Method info strip ─────────────────────────────────────
                // Show what backend will be used (no tabs — just informational).
                if self.is_admin {
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(20, 35, 50))
                        .rounding(4.0)
                        .inner_margin(8.0)
                        .show(ui, |ui: &mut egui::Ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(100, 180, 255),
                                    "⚡ Mode: Deep Boost (OS-level interception)",
                                )
                                .on_hover_ui(|ui| {
                                    ui.label(
                                        "Deep Boost intercepts game traffic at the OS level, \
                                         giving the lowest possible ping improvement. Your game \
                                         will show the Boost Server ping as its connection ping — \
                                         this is normal.",
                                    );
                                    ui.hyperlink_to(
                                        "📖 How Deep Boost works",
                                        "https://github.com/ShibbityShwab/lightspeed/wiki/How-It-Works",
                                    );
                                });
                            });
                            ui.weak(
                                "All game traffic is routed through the Boost Server. \
                                 Your in-game ping = your ping to the Boost Server.",
                            );
                        });
                } else {
                    // Not admin — show restart nudge inline
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(55, 40, 10))
                        .rounding(4.0)
                        .inner_margin(8.0)
                        .show(ui, |ui: &mut egui::Ui| {
                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    egui::Color32::from_rgb(255, 180, 50),
                                    "⚠ Needs to run as Administrator to boost your game.",
                                )
                                .on_hover_ui(|ui| {
                                    ui.label(
                                        "Deep Boost needs Administrator access to intercept \
                                         game traffic at the OS level. Click the button below \
                                         to relaunch with the required permissions.",
                                    );
                                    ui.hyperlink_to(
                                        "📖 Why Administrator?",
                                        "https://github.com/ShibbityShwab/lightspeed/wiki/FAQ#why-admin",
                                    );
                                });
                            });
                            ui.add_space(4.0);
                            if ui
                                .button("🔑 Restart as Administrator")
                                .on_hover_text(
                                    "Relaunches LightSpeed with elevated privileges (UAC prompt).",
                                )
                                .clicked()
                            {
                                relaunch_as_admin();
                            }
                        });
                }

                ui.add_space(10.0);

                // ── THE OPTIMIZE BUTTON ───────────────────────────────────
                let btn_color = if self.is_admin {
                    egui::Color32::from_rgb(80, 50, 5)
                } else {
                    egui::Color32::from_rgb(55, 55, 55)
                };
                let btn_label = if self.is_admin {
                    "⚡  BOOST MY GAME"
                } else {
                    "⚡  BOOST MY GAME  (requires Administrator)"
                };
                if ui
                    .add_sized(
                        [ui.available_width(), 40.0],
                        egui::Button::new(
                            egui::RichText::new(btn_label)
                                .size(16.0)
                                .color(if self.is_admin {
                                    egui::Color32::from_rgb(255, 210, 100)
                                } else {
                                    egui::Color32::from_rgb(140, 140, 140)
                                }),
                        )
                        .fill(btn_color),
                    )
                    .on_hover_text(if self.is_admin {
                        "Click Boost, then launch your game and join any server.\n\
                         LightSpeed automatically finds your game server and routes \
                         traffic through the Boost Server for lower ping."
                    } else {
                        "Run LightSpeed as Administrator to boost your game."
                    })
                    .clicked()
                    && self.is_admin
                {
                    // Auto-pick best backend: WinDivert (kernel) is always preferred
                    // when running as admin — no fallback needed.
                    let _ = parse_custom_port_range(&self.custom_port_input)
                            .unwrap_or_else(|| detect_windivert_port_range(self.selected_game_idx));
                    
                    let game_key = GAMES[self.selected_game_idx].0;
                    let result = self.engine.lock().unwrap().start_interceptor(
                        game_key,
                        self.selected_proxy_addr(),
                        self.fec_enabled,
                        4, // default FEC K
                    );
                    if let Err(e) = result {
                        tracing::error!("start_interceptor failed: {}", e);
                    } else {
                        self.boost_start = Some(std::time::Instant::now());
                        self.last_tray_state = TrayState::Connected; // force tray update
                    }
                }

                ui.add_space(6.0);

                // ── Advanced expander (manual server IP fallback) ─────────
                let adv_label = if self.show_advanced {
                    "▼ Advanced — set server manually"
                } else {
                    "▶ Advanced — set server manually"
                };
                if ui
                    .small_button(adv_label)
                    .on_hover_text(
                        "If auto-detect doesn't find your server, enter the game \
                         server IP:port here to start boosting manually.",
                    )
                    .clicked()
                {
                    self.show_advanced = !self.show_advanced;
                }

                if self.show_advanced {
                    ui.add_space(4.0);
                    egui::Frame::none()
                        .fill(egui::Color32::from_rgb(25, 25, 35))
                        .rounding(4.0)
                        .inner_margin(8.0)
                        .show(ui, |ui: &mut egui::Ui| {
                            ui.weak(
                                "Enter your game server's IP and port to start boosting \
                                 without waiting for auto-detect. Find the IP in your \
                                 game's server browser.",
                            );
                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label("Server:");
                                let default_port = GAMES[self.selected_game_idx].2;
                                ui.add(
                                    egui::TextEdit::singleline(&mut self.server_input)
                                        .hint_text(format!("e.g. 123.45.67.89:{}", default_port))
                                        .desired_width(220.0),
                                );
                            });

                            ui.add_space(4.0);
                            ui.horizontal(|ui| {
                                ui.label("Custom Port Range:")
                                    .on_hover_ui(|ui| {
                                        ui.label(
                                            "Override the default port scan range for auto-detect. \
                                             Use this if Packets Sent stays at 0 after 15 s.\n\
                                             Format: lo-hi  (e.g. 28015-28999)  or a single port."
                                        );
                                        ui.hyperlink_to(
                                            "📖 Port not detected — fix guide",
                                            "https://github.com/ShibbityShwab/lightspeed/wiki/Troubleshooting#port-not-detected",
                                        );
                                    });
                                let port_valid = self.custom_port_input.is_empty()
                                    || parse_custom_port_range(&self.custom_port_input).is_some();
                                let te = egui::TextEdit::singleline(&mut self.custom_port_input)
                                    .hint_text("e.g. 28015-28999 (leave blank for auto)")
                                    .desired_width(200.0)
                                    .text_color(if port_valid {
                                        ui.visuals().text_color()
                                    } else {
                                        egui::Color32::from_rgb(220, 90, 90)
                                    });
                                ui.add(te);
                                if !port_valid {
                                    ui.colored_label(
                                        egui::Color32::from_rgb(220, 90, 90),
                                        "⚠ invalid",
                                    );
                                }
                            });

                            ui.add_space(6.0);

                            let server_valid = parse_server_addr(&self.server_input).is_some();
                            let mbtn = egui::Button::new("▶  Start Boost (manual)")
                                .fill(if server_valid {
                                    egui::Color32::from_rgb(40, 90, 55)
                                } else {
                                    egui::Color32::from_rgb(60, 60, 60)
                                });
                            if ui.add_enabled(server_valid, mbtn).clicked() {
                                if let Some(server_addr) = parse_server_addr(&self.server_input) {
                                    let (game_key, game_display, default_port) =
                                        GAMES[self.selected_game_idx];
                                    let local_port = server_addr.port().max(default_port);
                                    let proxy = self.selected_proxy_addr();
                                    self.engine.lock().unwrap().start_redirect(
                                        server_addr,
                                        local_port,
                                        self.fec_enabled,
                                        4,
                                        game_display.to_string(),
                                        proxy,
                                    );
                                    let _ = game_key;
                                }
                            }
                            if !server_valid && !self.server_input.is_empty() {
                                ui.colored_label(
                                    egui::Color32::from_rgb(220, 130, 50),
                                    "⚠ Enter a valid IP:port (e.g. 1.2.3.4:28015)",
                                );
                            }

                            ui.add_space(4.0);
                            let instruction = connect_instruction(
                                self.selected_game_idx,
                                self.server_input
                                    .parse::<SocketAddrV4>()
                                    .map(|a| a.port())
                                    .unwrap_or(GAMES[self.selected_game_idx].2),
                            );
                            ui.weak(instruction);
                        });
                }
            }

            ui.add_space(8.0);
            ui.separator();

            // ── Footer controls ───────────────────────────────────────────
            ui.horizontal(|ui| {
                if self.status.connected {
                    if ui
                        .small_button("Disconnect Boost Server")
                        .on_hover_text("Disconnect from the Boost Server. Your game will use its normal connection.")
                        .clicked()
                    {
                        self.engine.lock().unwrap().disconnect();
                    }
                } else if ui
                    .small_button("Reconnect Boost Server")
                    .on_hover_text("Reconnect to the Boost Server.")
                    .clicked()
                {
                    let proxy = self.selected_proxy_addr();
                    self.engine.lock().unwrap().connect(proxy);
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Hide to tray").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                });
            });
        });

        // ── Custom proxy connect dialog ───────────────────────────────────
        if self.show_connect_dialog {
            egui::Window::new("Connect to custom proxy")
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Proxy address (ip:port):");
                    ui.text_edit_singleline(&mut self.custom_proxy_input);
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Connect").clicked() {
                            if let Ok(addr) = self.custom_proxy_input.parse::<SocketAddrV4>() {
                                self.engine.lock().unwrap().connect(addr);
                                self.show_connect_dialog = false;
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.show_connect_dialog = false;
                        }
                    });
                });
        }

        // ── Repaint schedule ─────────────────────────────────────────────
        let repaint_interval =
            if self.status.redirect_active
                || self.status.capture_active
                || self.status.windivert_active
                || self.status.interceptor_active
            {
                Duration::from_millis(500) // 2 Hz for live counters
            } else {
                Duration::from_secs(1)
            };
        ctx.request_repaint_after(repaint_interval);
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

fn rtt_colour(rtt_ms: f64) -> egui::Color32 {
    if rtt_ms < 60.0 {
        egui::Color32::from_rgb(80, 200, 120)
    } else if rtt_ms < 120.0 {
        egui::Color32::from_rgb(255, 210, 0)
    } else {
        egui::Color32::from_rgb(220, 80, 80)
    }
}

fn parse_server_addr(s: &str) -> Option<SocketAddrV4> {
    if s.is_empty() {
        return None;
    }
    s.parse::<SocketAddrV4>().ok()
}

fn connect_instruction(game_idx: usize, local_port: u16) -> String {
    let (key, _, _) = GAMES[game_idx];
    match key {
        "rust" => format!("In Rust  F1 console:  client.connect 127.0.0.1:{}", local_port),
        "cs2" => format!("In CS2 console:  connect 127.0.0.1:{}", local_port),
        "dota2" => format!("In Dota 2 console:  connect 127.0.0.1:{}", local_port),
        _ => format!("Connect your game to:  127.0.0.1:{}", local_port),
    }
}

fn try_auto_detect_game() -> Option<String> {
    match lightspeed_client::games::auto_detect() {
        Ok(game) => {
            let name_lower = game.name().to_lowercase();
            let key = GAMES.iter().find_map(|(k, display, _)| {
                if display.to_lowercase().contains(&name_lower) || name_lower.contains(k) {
                    Some(*k)
                } else {
                    None
                }
            });
            key.map(|k| k.to_string())
        }
        Err(_) => None,
    }
}

/// Check if Npcap is installed on this Windows system.
fn check_npcap() -> bool {
    use std::process::Command;
    Command::new("sc")
        .args(["query", "npcap"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Check if the current process is running as Administrator.
fn check_is_admin() -> bool {
    use std::process::Command;
    // `net session` requires admin — succeeds if elevated, fails if not.
    Command::new("net")
        .args(["session"])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Determine the WinDivert port range for the selected game using real-time
/// process inspection where available, with a wide static range as fallback.
///
/// For Rust, this calls `detect_rust_ports_netstat()` to find the actual UDP
/// sockets open by `RustClient.exe` — so it works even on community servers
/// that use completely non-standard ports.  For other games it delegates to
/// `windivert_port_range()`.
fn detect_windivert_port_range(game_idx: usize) -> (u16, u16) {
    let (key, _, _) = GAMES[game_idx];
    if key == "rust" {
        if let Some(range) = detect_rust_ports_netstat() {
            tracing::info!(
                "🔍 RustClient.exe netstat-detected port range: {}-{}",
                range.0, range.1
            );
            return range;
        }
        tracing::debug!("RustClient.exe netstat detection failed — using wide fallback 28015-28999");
    }
    windivert_port_range(game_idx)
}

/// Known Steam-service UDP ports that RustClient.exe keeps open for
/// Steam NAT punch / relay etc. — we skip these so the WinDivert filter
/// doesn't intercept Steam traffic instead of game traffic.
const STEAM_SERVICE_PORTS: &[u16] = &[
    3478, 4379, 4380,  // Steam NAT punch / relay
    27005,             // Steam client source
    27015,             // Steam SRCDS / query
    27020,             // Steam TV
    27036, 27037,      // Steam Remote Play
];

/// Detect the actual UDP port(s) that `RustClient.exe` is using by
/// querying `netstat -ano` and cross-referencing with `tasklist`.
///
/// Returns `Some((lo, hi))` — the min/max of detected game ports with a
/// small headroom window — or `None` if the process isn't found or has no
/// relevant sockets.
#[cfg(target_os = "windows")]
fn detect_rust_ports_netstat() -> Option<(u16, u16)> {
    use std::process::Command;

    // ── Step 1: find PID of RustClient.exe ───────────────────────────────
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

    // ── Step 2: enumerate UDP endpoints owned by that PID ─────────────────
    let ns = Command::new("netstat")
        .args(["-ano", "-p", "UDP"])
        .output()
        .ok()?;

    let pid_str = pid.to_string();
    let mut ports: Vec<u16> = Vec::new();

    for line in String::from_utf8_lossy(&ns.stdout).lines() {
        // Windows netstat UDP output format:
        // Column 0: Proto (UDP)
        // Column 1: Local Address (your_ip:your_port)
        // Column 2: Foreign Address (*:* or ip:port)
        // Column 3: State (blank for UDP)
        // Column 4: PID (if -o is used)
        // Note: Sometimes Column 3 is missing in UDP output.
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() < 4 { continue; }
        if !parts[0].eq_ignore_ascii_case("UDP") { continue; }
        
        // Find PID - it's usually the last part
        let line_pid = parts.last().unwrap_or(&"");
        if *line_pid != pid_str { continue; }

        // We want the FOREIGN port (the game server port), not our LOCAL port.
        // For UDP, netstat -ano usually shows "*:*" for the foreign address 
        // until a packet is sent. If it's still "*:*", we have to fallback 
        // to our local port as a hint, or use the wide default.
        let foreign_addr = parts[2];
        if let Some(port_str) = foreign_addr.rsplit(':').next() {
            if let Ok(port) = port_str.parse::<u16>() {
                if port >= 1024 && !STEAM_SERVICE_PORTS.contains(&port) && !ports.contains(&port) {
                    ports.push(port);
                }
            }
        }
        
        // If foreign port is unknown (*), check the local port. 
        // In some games, the local port matches the remote port (source=dest).
        if ports.is_empty() {
            if let Some(port_str) = parts[1].rsplit(':').next() {
                if let Ok(port) = port_str.parse::<u16>() {
                    if port >= 28015 && port <= 30000 && !ports.contains(&port) {
                        ports.push(port);
                    }
                }
            }
        }
    }

    tracing::debug!("RustClient.exe (PID {}) candidate UDP ports: {:?}", pid, ports);

    if ports.is_empty() {
        return None;
    }

    let lo = *ports.iter().min().unwrap();
    let hi = *ports.iter().max().unwrap();
    // Small headroom: ensure at least a 3-port window around the detected range.
    Some((lo, hi.max(lo + 2)))
}

#[cfg(not(target_os = "windows"))]
fn detect_rust_ports_netstat() -> Option<(u16, u16)> {
    None
}

/// Returns the WinDivert port range (lo, hi) for the selected game.
///
/// These match the game-server UDP port ranges used by each title.
/// WinDivert opens a broad filter over the range and auto-detects the
/// actual server from the first intercepted outbound packet.
///
/// Ranges are intentionally wide because community servers regularly use
/// non-default ports.  The stale-server timeout (5 s) prevents locking on
/// to wrong hosts even with a wide filter.
fn windivert_port_range(game_idx: usize) -> (u16, u16) {
    let (key, _, default_port) = GAMES[game_idx];
    match key {
        // Community Rust servers use any port in 28015–30000.
        // Official Facepunch servers default to 28015.
        "rust"     => (28015, 30000),
        // Source-engine games share the 27000 block; matchmaking,
        // community, and dedicated server ports vary widely.
        "cs2"      => (27015, 27100),
        "dota2"    => (27015, 27100),
        "valorant" => (7000,  7500),
        "apex"     => (37000, 37050),
        "lol"      => (5000,  5500),
        "pubg"     => (7777,  7843),
        _ => (default_port, default_port),
    }
}

/// Parse a user-supplied port range string.
///
/// Accepted formats:
///  - `"28015-28999"` → `(28015, 28999)`
///  - `"28015"`       → `(28015, 28015)`
///  - `""`            → `None`  (blank → use default)
fn parse_custom_port_range(s: &str) -> Option<(u16, u16)> {
    let s = s.trim();
    if s.is_empty() {
        return None;
    }
    if let Some((lo_s, hi_s)) = s.split_once('-') {
        let lo = lo_s.trim().parse::<u16>().ok()?;
        let hi = hi_s.trim().parse::<u16>().ok()?;
        if lo <= hi { Some((lo, hi)) } else { None }
    } else {
        let p = s.parse::<u16>().ok()?;
        Some((p, p))
    }
}

/// Relaunch the current exe with elevated privileges via PowerShell UAC.
fn relaunch_as_admin() {
    let exe = std::env::current_exe()
        .unwrap_or_default()
        .display()
        .to_string();
    // Use Start-Process -Verb RunAs to show UAC prompt.
    let script = format!("Start-Process '{}' -Verb RunAs", exe.replace('\'', "''"));
    let _ = std::process::Command::new("powershell")
        .args(["-WindowStyle", "Hidden", "-Command", &script])
        .spawn();
    // Exit current (unelevated) process — the new elevated one will start.
    std::process::exit(0);
}
