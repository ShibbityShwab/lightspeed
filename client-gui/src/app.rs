//! egui application + Windows tray icon for LightSpeed GUI.
//!
//! This module is only compiled on Windows (`#[cfg(windows)]` in main.rs).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use lightspeed_client::{EngineStatus, LightSpeedEngine};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};

// ── Tray menu item IDs ──────────────────────────────────────────────────────

const MENU_SHOW: &str = "show";
const MENU_CONNECT: &str = "connect";
const MENU_DISCONNECT: &str = "disconnect";
const MENU_QUIT: &str = "quit";

// ── App struct ───────────────────────────────────────────────────────────────

pub struct LightSpeedApp {
    engine: Arc<Mutex<LightSpeedEngine>>,
    /// Cached status snapshot updated each frame.
    status: EngineStatus,
    /// Tray icon handle — must be kept alive for the duration of the app.
    _tray: TrayIcon,
    /// Proxy address input field.
    proxy_input: String,
    /// Whether to show the connect dialog.
    show_connect_dialog: bool,
    /// Tray menu item IDs for event matching.
    id_show: tray_icon::menu::MenuId,
    id_connect: tray_icon::menu::MenuId,
    id_disconnect: tray_icon::menu::MenuId,
    id_quit: tray_icon::menu::MenuId,
}

impl LightSpeedApp {
    pub fn new(engine: Arc<Mutex<LightSpeedEngine>>) -> Self {
        let (tray, id_show, id_connect, id_disconnect, id_quit) = build_tray();
        let status = engine.lock().unwrap().snapshot();
        let proxy_input = status.proxy_addr.clone();
        Self {
            engine,
            status,
            _tray: tray,
            proxy_input,
            show_connect_dialog: false,
            id_show,
            id_connect,
            id_disconnect,
            id_quit,
        }
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

    let icon = solid_icon(255, 210, 0); // yellow dot

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("⚡ LightSpeed")
        .with_icon(icon)
        .build()
        .expect("Failed to create tray icon");

    (tray, id_show, id_connect, id_disconnect, id_quit)
}

/// Creates a 16×16 solid-colour RGBA tray icon.
fn solid_icon(r: u8, g: u8, b: u8) -> tray_icon::Icon {
    const SIZE: usize = 16;
    let mut rgba = vec![0u8; SIZE * SIZE * 4];
    for i in 0..SIZE * SIZE {
        rgba[i * 4] = r;
        rgba[i * 4 + 1] = g;
        rgba[i * 4 + 2] = b;
        rgba[i * 4 + 3] = 255;
    }
    tray_icon::Icon::from_rgba(rgba, SIZE as u32, SIZE as u32)
        .expect("Failed to build tray icon from RGBA data")
}

// ── eframe::App impl ─────────────────────────────────────────────────────────

impl eframe::App for LightSpeedApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // ── Poll tray icon events ──────────────────────────────────────────
        if let Ok(event) = TrayIconEvent::receiver().try_recv() {
            if matches!(event, TrayIconEvent::DoubleClick { .. }) {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
        }

        // ── Poll tray menu events ─────────────────────────────────────────
        if let Ok(event) = MenuEvent::receiver().try_recv() {
            let id = &event.id;
            if id == &self.id_show {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            } else if id == &self.id_connect {
                self.show_connect_dialog = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            } else if id == &self.id_disconnect {
                self.engine.lock().unwrap().disconnect();
            } else if id == &self.id_quit {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }

        // ── Intercept close → hide to tray ───────────────────────────────
        if ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            return;
        }

        // ── Refresh engine snapshot ───────────────────────────────────────
        self.status = self.engine.lock().unwrap().snapshot();

        // ── Main panel ────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ctx, |ui| {
            // Header
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

            // Proxy info
            ui.horizontal(|ui| {
                ui.label("Proxy:");
                if self.status.proxy_addr.is_empty() {
                    ui.weak("(none)");
                } else {
                    ui.monospace(&self.status.proxy_addr);
                }
            });

            // RTT metric
            ui.horizontal(|ui| {
                ui.label("Latest RTT:");
                if self.status.connected && self.status.latest_rtt_ms > 0.0 {
                    let rtt = self.status.latest_rtt_ms;
                    let colour = rtt_colour(rtt);
                    ui.colored_label(colour, format!("{:.1} ms", rtt));
                } else {
                    ui.weak("—");
                }
            });

            // Packet counters
            ui.horizontal(|ui| {
                ui.label("Packets sent / received:");
                ui.monospace(format!(
                    "{} / {}",
                    self.status.packets_sent, self.status.packets_received
                ));
            });

            ui.add_space(8.0);

            // RTT sparkline
            if !self.status.rtt_history.is_empty() {
                ui.label("RTT history (last 120 keepalives):");
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
                    .height(160.0)
                    .allow_drag(false)
                    .allow_zoom(false)
                    .allow_scroll(false)
                    .show_axes([false, true])
                    .show(ui, |plot_ui| plot_ui.line(line));
            } else {
                ui.weak("Waiting for first keepalive echo…");
                ui.add_space(160.0);
            }

            ui.add_space(8.0);
            ui.separator();

            // Controls
            ui.horizontal(|ui| {
                if self.status.connected {
                    if ui.button("Disconnect").clicked() {
                        self.engine.lock().unwrap().disconnect();
                    }
                } else {
                    if ui.button("Connect…").clicked() {
                        self.show_connect_dialog = true;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Hide to tray").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                });
            });
        });

        // ── Connect dialog ────────────────────────────────────────────────
        if self.show_connect_dialog {
            egui::Window::new("Connect to proxy")
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.label("Proxy address (ip:port):");
                    ui.text_edit_singleline(&mut self.proxy_input);

                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Connect").clicked() {
                            if let Ok(addr) = self.proxy_input.parse::<std::net::SocketAddrV4>() {
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

        // Repaint at ~1 Hz to refresh status without busy-looping.
        ctx.request_repaint_after(Duration::from_secs(1));
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Returns a traffic-light colour for the given RTT value.
fn rtt_colour(rtt_ms: f64) -> egui::Color32 {
    if rtt_ms < 60.0 {
        egui::Color32::from_rgb(80, 200, 120) // green
    } else if rtt_ms < 120.0 {
        egui::Color32::from_rgb(255, 210, 0) // yellow
    } else {
        egui::Color32::from_rgb(220, 80, 80) // red
    }
}
