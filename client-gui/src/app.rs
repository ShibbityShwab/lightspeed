//! egui application + Windows tray icon for LightSpeed GUI.
//!
//! This module is only compiled on Windows (`#[cfg(windows)]` in main.rs).

use std::net::SocketAddrV4;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};
use lightspeed_client::{EngineStatus, LightSpeedEngine};
use tray_icon::{
    menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem},
    TrayIcon, TrayIconBuilder, TrayIconEvent,
};

// ── Proxy nodes ────────────────────────────────────────────────────────────

const PROXIES: &[(&str, &str)] = &[
    ("149.28.84.139:4434", "LAX — US West (206ms)"),
    ("149.28.144.74:4434", "SGP — Singapore (31ms)"),
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
    _tray: TrayIcon,

    // ── Proxy connection ─────────────────────────────────────────
    selected_proxy_idx: usize,
    show_connect_dialog: bool,
    custom_proxy_input: String,

    // ── Game routing ─────────────────────────────────────────────
    selected_game_idx: usize,
    server_input: String,
    fec_enabled: bool,
    auto_detected_game: Option<String>,

    // ── Tray menu IDs ────────────────────────────────────────────
    id_show: tray_icon::menu::MenuId,
    id_connect: tray_icon::menu::MenuId,
    id_disconnect: tray_icon::menu::MenuId,
    id_quit: tray_icon::menu::MenuId,
}

impl LightSpeedApp {
    pub fn new(engine: Arc<Mutex<LightSpeedEngine>>) -> Self {
        let (tray, id_show, id_connect, id_disconnect, id_quit) = build_tray();
        let status = engine.lock().unwrap().snapshot();

        // Try to auto-detect a running game at startup.
        let auto_detected_game = try_auto_detect_game();
        // Select matching game in the list if detected.
        let selected_game_idx = auto_detected_game
            .as_deref()
            .and_then(|name| {
                GAMES.iter().position(|(key, _, _)| {
                    key.eq_ignore_ascii_case(name)
                })
            })
            .unwrap_or(0); // default to Rust

        Self {
            engine,
            status,
            _tray: tray,
            selected_proxy_idx: 0,
            show_connect_dialog: false,
            custom_proxy_input: String::new(),
            selected_game_idx,
            server_input: String::new(),
            fec_enabled: false,
            auto_detected_game,
            id_show,
            id_connect,
            id_disconnect,
            id_quit,
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

    let icon = solid_icon(255, 210, 0);

    let tray = TrayIconBuilder::new()
        .with_menu(Box::new(menu))
        .with_tooltip("⚡ LightSpeed")
        .with_icon(icon)
        .build()
        .expect("Failed to create tray icon");

    (tray, id_show, id_connect, id_disconnect, id_quit)
}

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
        // ── Poll tray icon events ─────────────────────────────────────────
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
                let proxy = self.selected_proxy_addr();
                self.engine.lock().unwrap().connect(proxy);
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

            // ── Proxy selector ────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Proxy:");
                let prev = self.selected_proxy_idx;
                for (i, (_, label)) in PROXIES.iter().enumerate() {
                    ui.selectable_value(&mut self.selected_proxy_idx, i, *label);
                }
                if self.selected_proxy_idx != prev {
                    // Reconnect keepalive to new proxy
                    let proxy = self.selected_proxy_addr();
                    self.engine.lock().unwrap().connect(proxy);
                }
            });

            // ── RTT + stats ───────────────────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Proxy RTT:");
                if self.status.connected && self.status.latest_rtt_ms > 0.0 {
                    let rtt = self.status.latest_rtt_ms;
                    ui.colored_label(rtt_colour(rtt), format!("{:.1} ms", rtt));
                } else {
                    ui.weak("measuring…");
                }
                ui.separator();
                ui.label(format!(
                    "KA packets: {} / {}",
                    self.status.packets_sent, self.status.packets_received
                ));
            });

            // RTT sparkline (compact)
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
                    .height(100.0)
                    .allow_drag(false)
                    .allow_zoom(false)
                    .allow_scroll(false)
                    .show_axes([false, true])
                    .show(ui, |plot_ui| plot_ui.line(line));
            } else {
                ui.weak("Waiting for first keepalive echo…");
                ui.add_space(100.0);
            }

            ui.separator();

            // ── Game Routing section ──────────────────────────────────────
            ui.heading("🎮 Game Routing");
            ui.add_space(4.0);

            if self.status.redirect_active {
                // ── ACTIVE state ──────────────────────────────────────────
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(80, 200, 120),
                        "● ACTIVE",
                    );
                    ui.label(format!(
                        " — {} → 127.0.0.1:{}",
                        self.status.redirect_game,
                        self.status.redirect_local_port,
                    ));
                });
                ui.label(format!("Server:  {}", self.status.redirect_server));

                // Live packet stats
                ui.horizontal(|ui| {
                    ui.label("Out:");
                    ui.monospace(format!("{}", self.status.redirect_pkts_out));
                    ui.separator();
                    ui.label("In:");
                    ui.monospace(format!("{}", self.status.redirect_pkts_in));
                    ui.separator();
                    ui.label("Errors:");
                    let err_colour = if self.status.redirect_errors > 0 {
                        egui::Color32::from_rgb(220, 80, 80)
                    } else {
                        egui::Color32::GRAY
                    };
                    ui.colored_label(err_colour, format!("{}", self.status.redirect_errors));
                });

                if self.status.redirect_fec {
                    ui.label(format!(
                        "FEC — parity sent: {}  recovered: {}",
                        self.status.redirect_fec_parity,
                        self.status.redirect_fec_recovered,
                    ));
                }

                // Show the connect instruction for the game
                ui.add_space(4.0);
                let instruction = connect_instruction(
                    self.selected_game_idx,
                    self.status.redirect_local_port,
                );
                egui::Frame::none()
                    .fill(egui::Color32::from_rgb(30, 50, 30))
                    .rounding(4.0)
                    .inner_margin(8.0)
                    .show(ui, |ui: &mut egui::Ui| {
                        ui.colored_label(egui::Color32::from_rgb(150, 255, 150), &instruction);
                    });

                ui.add_space(6.0);
                if ui
                    .add(egui::Button::new("■  Stop optimizing").fill(
                        egui::Color32::from_rgb(180, 50, 50),
                    ))
                    .clicked()
                {
                    self.engine.lock().unwrap().stop_redirect();
                }

                // Show error if any
                if let Some(ref err) = self.status.redirect_error {
                    ui.add_space(4.0);
                    ui.colored_label(
                        egui::Color32::from_rgb(220, 80, 80),
                        format!("⚠ Error: {}", err),
                    );
                }
            } else {
                // ── IDLE state: config form ────────────────────────────────

                // Auto-detect banner
                if let Some(ref detected) = self.auto_detected_game {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(80, 200, 120),
                            "🟢 Auto-detected:",
                        );
                        ui.label(detected);
                    });
                } else {
                    ui.horizontal(|ui| {
                        ui.weak("No game detected");
                        if ui.small_button("🔄 Rescan").clicked() {
                            self.auto_detected_game = try_auto_detect_game();
                            if let Some(ref name) = self.auto_detected_game {
                                if let Some(idx) = GAMES.iter().position(|(k, _, _)| {
                                    k.eq_ignore_ascii_case(name)
                                }) {
                                    self.selected_game_idx = idx;
                                }
                            }
                        }
                    });
                }

                // Game dropdown
                ui.horizontal(|ui| {
                    ui.label("Game:  ");
                    egui::ComboBox::from_id_salt("game_select")
                        .selected_text(GAMES[self.selected_game_idx].1)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (i, (_, display, _)) in GAMES.iter().enumerate() {
                                ui.selectable_value(
                                    &mut self.selected_game_idx,
                                    i,
                                    *display,
                                );
                            }
                        });
                });

                // Server IP input
                ui.horizontal(|ui| {
                    ui.label("Server:");
                    let default_port = GAMES[self.selected_game_idx].2;
                    ui.add(
                        egui::TextEdit::singleline(&mut self.server_input)
                            .hint_text(format!("e.g. 123.45.67.89:{}", default_port))
                            .desired_width(220.0),
                    );
                });

                // FEC toggle
                ui.horizontal(|ui| {
                    ui.checkbox(&mut self.fec_enabled, "FEC (packet loss recovery, +25% bandwidth)");
                });

                ui.add_space(8.0);

                // Start button
                let server_valid = parse_server_addr(&self.server_input).is_some();
                let btn = egui::Button::new("▶  Start optimizing")
                    .fill(if server_valid {
                        egui::Color32::from_rgb(40, 120, 60)
                    } else {
                        egui::Color32::from_rgb(60, 60, 60)
                    });

                if ui.add_enabled(server_valid, btn).clicked() {
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

                if self.server_input.is_empty() {
                    ui.weak("Enter the game server IP:port to enable");
                }
            }

            ui.add_space(8.0);
            ui.separator();

            // ── Footer controls ───────────────────────────────────────────
            ui.horizontal(|ui| {
                if self.status.connected {
                    if ui.small_button("Disconnect proxy").clicked() {
                        self.engine.lock().unwrap().disconnect();
                    }
                } else if ui.small_button("Reconnect proxy").clicked() {
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
                            if let Ok(addr) =
                                self.custom_proxy_input.parse::<SocketAddrV4>()
                            {
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

        // Repaint at ~2 Hz while redirect is active (for live counters), 1 Hz otherwise.
        let repaint_interval = if self.status.redirect_active {
            Duration::from_millis(500)
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

/// Parse a "host:port" or "ip:port" string into a SocketAddrV4.
fn parse_server_addr(s: &str) -> Option<SocketAddrV4> {
    if s.is_empty() {
        return None;
    }
    // Try direct parse first (ip:port).
    if let Ok(addr) = s.parse::<SocketAddrV4>() {
        return Some(addr);
    }
    // If missing port, try appending the default.
    None
}

/// Returns the in-game connect instruction for the active game.
fn connect_instruction(game_idx: usize, local_port: u16) -> String {
    let (key, _, _) = GAMES[game_idx];
    match key {
        "rust" => format!(
            "In Rust  F1 console:  client.connect 127.0.0.1:{}",
            local_port
        ),
        "cs2" => format!(
            "In CS2 console:  connect 127.0.0.1:{}",
            local_port
        ),
        "dota2" => format!(
            "In Dota 2 console:  connect 127.0.0.1:{}",
            local_port
        ),
        _ => format!(
            "Direct your game to connect to:  127.0.0.1:{}",
            local_port
        ),
    }
}

/// Try to auto-detect a running supported game.
/// Returns the game key (e.g. "rust") or None.
fn try_auto_detect_game() -> Option<String> {
    // Use the same detection logic as the CLI.
    match lightspeed_client::games::auto_detect() {
        Ok(game) => {
            // Map display name back to key for our GAMES list.
            let name_lower = game.name().to_lowercase();
            let key = GAMES.iter().find_map(|(k, display, _)| {
                if display.to_lowercase().contains(&name_lower)
                    || name_lower.contains(k)
                {
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
