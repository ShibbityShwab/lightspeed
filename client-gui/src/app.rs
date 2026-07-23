use std::net::SocketAddrV4;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::platform::{self, Platform, TrayAction, TrayHandle};
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

// ── Tray state enum ──────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Disconnected,
    Connected,
    Optimizing,
    Error,
}
use lightspeed_client::{EngineStatus, LightSpeedEngine};

// ── Proxy nodes ────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct ProxyEntry {
    pub addr: SocketAddrV4,
    pub label: String,
}

/// Loaded from `LIGHTSPEED_PROXIES` env var (comma-separated `addr:port`)
/// or falls back to localhost placeholders.
///   LIGHTSPEED_PROXIES="1.2.3.4:4434,5.6.7.8:4434"
pub fn load_proxies() -> Vec<ProxyEntry> {
    if let Ok(val) = std::env::var("LIGHTSPEED_PROXIES") {
        let proxies: Vec<_> = val
            .split(',')
            .filter_map(|s| {
                let s = s.trim();
                if s.is_empty() {
                    return None;
                }
                match s.parse::<SocketAddrV4>() {
                    Ok(addr) => Some(ProxyEntry {
                        addr,
                        label: "Custom".into(),
                    }),
                    Err(_) => {
                        tracing::warn!("Skipping invalid proxy address: {s}");
                        None
                    }
                }
            })
            .collect();
        if !proxies.is_empty() {
            return proxies;
        }
        tracing::warn!("LIGHTSPEED_PROXIES set but no valid addresses found — using defaults");
    }
    vec![
        ProxyEntry { addr: "127.0.0.1:4434".parse().expect("valid"), label: "LAX — US West".into() },
        ProxyEntry { addr: "127.0.0.1:4434".parse().expect("valid"), label: "SGP — Singapore".into() },
    ]
}

// ── Game list ──────────────────────────────────────────────────────────────

/// (key, display name, default port)
pub const GAMES: &[(&str, &str, u16)] = &[
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

// ── App struct ───────────────────────────────────────────────────────────────

pub struct LightSpeedApp<P: Platform> {
    engine: Arc<Mutex<LightSpeedEngine>>,
    status: EngineStatus,
    tray: P::Tray,

    // ── Proxy connection ─────────────────────────────────────────────────
    selected_proxy_idx: usize,
    show_connect_dialog: bool,
    custom_proxy_input: String,
    show_proxy_manager: bool,
    manager_label_input: String,
    manager_addr_input: String,

    // ── Game routing ──────────────────────────────────────────────────────
    selected_game_idx: usize,
    server_input: String,
    fec_enabled: bool,
    auto_detected_game: Option<String>,

    // ── System state ──────────────────────────────────────────────────────
    is_admin: bool,
    fonts_setup: bool,

    // ── Advanced panel toggle ─────────────────────────────────────────────
    show_advanced: bool,

    // ── Boost diagnostics ─────────────────────────────────────────────────
    boost_start: Option<std::time::Instant>,
    custom_port_input: String,

    proxies: Vec<ProxyEntry>,
}

impl<P: Platform> LightSpeedApp<P> {
    pub fn new(engine: Arc<Mutex<LightSpeedEngine>>) -> Self {
        let tray = P::new_tray();
        let status = engine.lock().unwrap().snapshot();

        let auto_detected_game = try_auto_detect_game();
        let selected_game_idx = auto_detected_game
            .as_deref()
            .and_then(|name| {
                GAMES
                    .iter()
                    .position(|(key, _, _)| key.eq_ignore_ascii_case(name))
            })
            .unwrap_or(0);

        let is_admin = P::is_admin();

        let proxies = load_proxies();

        Self {
            engine,
            status,
            tray,
            selected_proxy_idx: 0,
            show_connect_dialog: false,
            custom_proxy_input: String::new(),
            show_proxy_manager: false,
            manager_label_input: String::new(),
            manager_addr_input: String::new(),
            selected_game_idx,
            server_input: String::new(),
            fec_enabled: false,
            auto_detected_game,
            is_admin,
            fonts_setup: false,
            show_advanced: false,
            boost_start: None,
            custom_port_input: String::new(),
            proxies,
        }
    }

    fn selected_proxy_addr(&self) -> SocketAddrV4 {
        self.proxies[self.selected_proxy_idx].addr
    }
}

// ── eframe::App impl ─────────────────────────────────────────────────────────

impl<P: Platform> eframe::App for LightSpeedApp<P> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // One-time first-frame setup: platform-specific fonts.
        if !self.fonts_setup {
            self.fonts_setup = true;
            P::setup_fonts(&ctx);
        }

        // ── Poll tray events ─────────────────────────────────────────────
        for action in self.tray.poll_events(&ctx) {
            match action {
                TrayAction::Connect => {
                    let proxy = self.selected_proxy_addr();
                    self.engine.lock().unwrap().connect(proxy);
                }
                TrayAction::Disconnect => {
                    self.engine.lock().unwrap().disconnect();
                }
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

            self.tray
                .set_state(new_tray_state, self.status.latest_rtt_ms);
        }

        // ── Main panel ────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ui, |ui| {
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
                for (i, entry) in self.proxies.iter().enumerate() {
                    let btn = ui.selectable_value(&mut self.selected_proxy_idx, i, &entry.label);
                    btn.on_hover_text(format!("{}", entry.addr));
                }
                if self.selected_proxy_idx != prev {
                    let proxy = self.selected_proxy_addr();
                    self.engine.lock().unwrap().connect(proxy);
                }
                if ui.button("✎ Manage").clicked() {
                    self.show_proxy_manager = true;
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
                let line = Line::new("RTT (ms)", points)
                    .color(egui::Color32::from_rgb(100, 180, 255));
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
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(20, 30, 45))
                            .corner_radius(4.0)
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
                            .unwrap_or_else(|| platform::default_port_range(self.selected_game_idx));
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(55, 40, 8))
                            .corner_radius(4.0)
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
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(25, 40, 15))
                        .corner_radius(4.0)
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
                        ui.label("Packet that couldn't be delivered back to your game.\n\
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
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(20, 30, 45))
                            .corner_radius(4.0)
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
                            .unwrap_or_else(|| platform::default_port_range(self.selected_game_idx));
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgb(55, 40, 8))
                            .corner_radius(4.0)
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
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(25, 40, 15))
                        .corner_radius(4.0)
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
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(55, 44, 8))
                        .corner_radius(4.0)
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
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(20, 45, 30))
                    .corner_radius(4.0)
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
                egui::Frame::new()
                    .fill(egui::Color32::from_rgb(30, 50, 30))
                    .corner_radius(4.0)
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
                if self.is_admin {
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(20, 35, 50))
                        .corner_radius(4.0)
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
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(55, 40, 10))
                        .corner_radius(4.0)
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
                                P::relaunch_as_admin();
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
                    // Warm up port detection for diagnostic logging.
                    let _ = parse_custom_port_range(&self.custom_port_input)
                        .unwrap_or_else(|| P::detect_game_ports(self.selected_game_idx));

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
                    egui::Frame::new()
                        .fill(egui::Color32::from_rgb(25, 25, 35))
                        .corner_radius(4.0)
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
                .show(&ctx, |ui| {
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

        // ── Proxy manager window ─────────────────────────────────────────
        if self.show_proxy_manager {
            let mut remove_idx: Option<usize> = None;
            let mut add_addr: Option<SocketAddrV4> = None;

            egui::Window::new("Proxy Manager")
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(&ctx, |ui| {
                    for (i, entry) in self.proxies.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.label(format!("{}.", i + 1));
                            ui.label(&entry.label);
                            ui.label(entry.addr.to_string());
                            if ui.button("✕").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    }

                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.label("Label:");
                        ui.text_edit_singleline(&mut self.manager_label_input);
                    });
                    ui.horizontal(|ui| {
                        ui.label("Addr:");
                        ui.text_edit_singleline(&mut self.manager_addr_input);
                    });

                    ui.horizontal(|ui| {
                        if ui.button("Add Proxy").clicked() {
                            if self.manager_addr_input.parse::<SocketAddrV4>().is_ok() {
                                add_addr = Some(self.manager_addr_input.parse().unwrap());
                            }
                        }
                        if ui.button("Close").clicked() {
                            self.show_proxy_manager = false;
                        }
                    });
                });

            if let Some(idx) = remove_idx {
                self.proxies.remove(idx);
                if !self.proxies.is_empty() {
                    self.selected_proxy_idx = self.selected_proxy_idx.min(self.proxies.len() - 1);
                }
            }
            if let Some(addr) = add_addr {
                let label = if self.manager_label_input.is_empty() {
                    addr.to_string()
                } else {
                    self.manager_label_input.clone()
                };
                self.proxies.push(ProxyEntry { addr, label });
                self.manager_label_input.clear();
                self.manager_addr_input.clear();
            }
        }

        // ── Repaint schedule ─────────────────────────────────────────────
        let repaint_interval = if self.status.redirect_active
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

// ── Pure helpers (no platform dependency) ─────────────────────────────────────

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
        "rust" => format!(
            "In Rust  F1 console:  client.connect 127.0.0.1:{}",
            local_port
        ),
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

/// Known Steam-service UDP ports that RustClient.exe keeps open for
/// Steam NAT punch / relay etc. — we skip these so the WinDivert filter
/// doesn't intercept Steam traffic instead of game traffic.
pub const STEAM_SERVICE_PORTS: &[u16] = &[
    3478, 4379, 4380,  // Steam NAT punch / relay
    27005, // Steam client source
    27015, // Steam SRCDS / query
    27020, // Steam TV
    27036, 27037, // Steam Remote Play
];

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
        if lo <= hi {
            Some((lo, hi))
        } else {
            None
        }
    } else {
        let p = s.parse::<u16>().ok()?;
        Some((p, p))
    }
}
