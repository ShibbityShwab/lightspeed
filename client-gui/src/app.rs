//! # LightSpeed GUI — App
//!
//! Main egui application state, UI layout, and pure helper functions.
//! Wraps [`LightSpeedEngine`] in a platform-generic `<P: Platform>` struct
//! so the same GUI code works on Windows (tray-icon) and Linux (stub tray).

use std::net::SocketAddrV4;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{self, GuiConfig, ProxyEntry};
use crate::discovery::{self, DiscoveryOutcome, RelayHealth};
use crate::paths;
use crate::platform::{self, Platform, QuitFlag, TrayAction, TrayHandle};
use crate::update::UpdateStatus;
use eframe::egui;
use egui_plot::{Line, Plot, PlotPoints};

// ── Tray state enum ──────────────────────────────────────────────────────────

/// The four states the tray icon can reflect in its color and tooltip.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Disconnected,
    Connected,
    Optimizing,
    Error,
}
use lightspeed_client::{EngineStatus, LightSpeedEngine};

// ── Proxy nodes ────────────────────────────────────────────────────────────

/// Custom proxies from the environment, as a fallback for users who have not
/// let discovery populate `config.toml` yet.
///
///   LIGHTSPEED_PROXIES="1.2.3.4:4434,5.6.7.8:4434"
///   LIGHTSPEED_PROXY="1.2.3.4:4434"  (legacy single-proxy variable)
pub fn env_proxies() -> Vec<ProxyEntry> {
    let raw = std::env::var("LIGHTSPEED_PROXIES")
        .or_else(|_| std::env::var("LIGHTSPEED_PROXY"))
        .unwrap_or_default();
    raw.split(',')
        .filter_map(|s| {
            let s = s.trim();
            if s.is_empty() {
                return None;
            }
            match s.parse::<SocketAddrV4>() {
                Ok(addr) => Some(ProxyEntry::custom(addr, "Custom")),
                Err(_) => {
                    tracing::warn!("Skipping invalid proxy address: {s}");
                    None
                }
            }
        })
        .collect()
}

// ── Game list ──────────────────────────────────────────────────────────────

/// One entry in the game selector. Derived from the client crate's canonical
/// `GAME_REGISTRY` so the GUI can never drift from the CLI.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameEntry {
    pub key: &'static str,
    pub display: &'static str,
    pub default_port: u16,
}

/// Every supported game, built once from `lightspeed_client::games`.
pub fn games() -> &'static [GameEntry] {
    static GAMES: std::sync::OnceLock<Vec<GameEntry>> = std::sync::OnceLock::new();
    GAMES.get_or_init(|| {
        lightspeed_client::games::all_game_keys()
            .into_iter()
            .map(|(key, display)| GameEntry {
                key,
                display,
                default_port: lightspeed_client::games::detect_game(key)
                    .map(|game| game.redirect_port())
                    .unwrap_or(0),
            })
            .collect()
    })
}

// ── App struct ───────────────────────────────────────────────────────────────

/// The in-progress or completed state of a self-update availability check.
enum UpdateCheckState {
    /// No check has been requested.
    Idle,
    /// A background thread is consulting the releases API.
    Checking,
    /// The check finished; the dialog shows this result.
    Done(Result<UpdateStatus, String>),
}

/// Lifecycle of the background registry discovery thread.
pub enum DiscoveryState {
    /// A background thread is fetching and verifying the registry.
    InFlight(Receiver<DiscoveryOutcome>),
    /// The last attempt finished (successfully or not); the resulting relay
    /// list and any error live in `LightSpeedApp::proxies` /
    /// `LightSpeedApp::discovery_error`.
    Done,
}

/// A relay `/health` probe running on (or finished by) a background thread.
struct HealthProbe {
    addr: SocketAddrV4,
    rx: Receiver<Result<RelayHealth, String>>,
    result: Option<Result<RelayHealth, String>>,
}

/// Platform-generic egui application for the LightSpeed status window.
///
/// Type parameter `P` selects the platform backend (Windows tray or Linux
/// stub).  Most of the UI logic is platform-independent — only the tray
/// interaction, font loading, port detection, and admin checks delegate to
/// `P`.
pub struct LightSpeedApp<P: Platform> {
    engine: Arc<Mutex<LightSpeedEngine>>,
    status: EngineStatus,
    tray: Option<P::Tray>,
    quit: QuitFlag,

    // ── Proxy connection ─────────────────────────────────────────────────
    selected_proxy_idx: usize,
    show_proxy_manager: bool,
    manager_label_input: String,
    manager_addr_input: String,
    config_error: Option<String>,

    // ── Game routing ──────────────────────────────────────────────────────
    selected_game_idx: usize,
    server_input: String,
    fec_enabled: bool,
    auto_detected_game: Option<String>,

    // ── System state ──────────────────────────────────────────────────────
    is_admin: bool,
    fonts_setup: bool,
    header_icon: Option<egui::TextureHandle>,

    // ── Advanced panel toggle ─────────────────────────────────────────────
    show_advanced: bool,

    // ── Boost diagnostics ─────────────────────────────────────────────────
    boost_start: Option<std::time::Instant>,
    custom_port_input: String,

    // ── Update check ─────────────────────────────────────────────────────
    update_check: UpdateCheckState,
    show_update_dialog: bool,
    update_shared: Arc<Mutex<Option<Result<UpdateStatus, String>>>>,

    proxies: Vec<ProxyEntry>,
    discovery: DiscoveryState,
    discovery_error: Option<String>,
    health: Option<HealthProbe>,
}

impl<P: Platform> LightSpeedApp<P> {
    pub fn new(engine: Arc<Mutex<LightSpeedEngine>>, quit: QuitFlag) -> Self {
        let tray = P::new_tray(Arc::clone(&quit));
        tracing::info!("system tray created: {}", tray.is_some());
        let status = engine.lock().unwrap().snapshot();

        let auto_detected_game = try_auto_detect_game();
        let selected_game_idx = auto_detected_game
            .as_deref()
            .and_then(|name| {
                games()
                    .iter()
                    .position(|entry| entry.key.eq_ignore_ascii_case(name))
            })
            .unwrap_or(0);

        let is_admin = P::is_admin();

        let saved = config::load(&paths::config_file());
        let mut proxies = saved.proxies.clone();
        for entry in env_proxies() {
            if !proxies.iter().any(|existing| existing.addr == entry.addr) {
                proxies.push(entry);
            }
        }
        let selected_proxy_idx = saved
            .selected
            .as_deref()
            .and_then(|selected| {
                proxies
                    .iter()
                    .position(|entry| entry.addr.to_string() == selected)
            })
            .unwrap_or(0);

        let mut app = Self {
            engine,
            status,
            tray,
            quit,
            selected_proxy_idx,
            show_proxy_manager: false,
            manager_label_input: String::new(),
            manager_addr_input: String::new(),
            config_error: None,
            selected_game_idx,
            server_input: String::new(),
            fec_enabled: false,
            auto_detected_game,
            is_admin,
            fonts_setup: false,
            header_icon: None,
            show_advanced: false,
            boost_start: None,
            custom_port_input: String::new(),
            update_check: UpdateCheckState::Idle,
            show_update_dialog: false,
            update_shared: Arc::new(Mutex::new(None)),
            proxies,
            discovery: DiscoveryState::InFlight(discovery::spawn_discovery()),
            discovery_error: None,
            health: None,
        };

        // Reconnect to the persisted relay immediately; discovery may replace
        // the list a moment later without dropping the connection.
        app.connect_selected();
        app
    }

    /// Whether hide-to-tray is recoverable.
    ///
    /// `self.tray` is `Some` for the Linux/macOS stubs too, so combine it with
    /// `Platform::has_system_tray()` to avoid hiding with no way back.
    fn tray_available(&self) -> bool {
        self.tray.is_some() && P::has_system_tray()
    }

    fn selected_entry(&self) -> Option<&ProxyEntry> {
        self.proxies.get(self.selected_proxy_idx)
    }

    fn selected_proxy_addr(&self) -> Option<SocketAddrV4> {
        self.selected_entry().map(|entry| entry.addr)
    }

    fn selected_game(&self) -> &'static GameEntry {
        let games = games();
        let idx = self.selected_game_idx.min(games.len().saturating_sub(1));
        &games[idx]
    }

    fn selected_game_ports(&self) -> (u16, u16) {
        let entry = self.selected_game();
        parse_custom_port_range(&self.custom_port_input)
            .unwrap_or_else(|| platform::default_port_range(entry.key, entry.default_port))
    }

    fn connect_selected(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        let addr = entry.addr;
        let node_id = entry.node_id.clone();
        let mut engine = self.engine.lock().unwrap();
        engine.connect(addr);
        engine.set_node_id(node_id);
    }

    fn start_discovery(&mut self) {
        if matches!(self.discovery, DiscoveryState::InFlight(_)) {
            return;
        }
        self.discovery_error = None;
        self.discovery = DiscoveryState::InFlight(discovery::spawn_discovery());
    }

    fn persist_config(&self) {
        let config = GuiConfig {
            selected: self.selected_entry().map(|entry| entry.addr.to_string()),
            proxies: self.proxies.clone(),
        };
        if let Err(e) = config::save(&paths::config_file(), &config) {
            tracing::warn!("Could not persist GUI config: {e}");
        }
    }

    fn poll_discovery(&mut self) {
        let outcome = match &self.discovery {
            DiscoveryState::InFlight(rx) => match rx.try_recv() {
                Ok(outcome) => Some(outcome),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(DiscoveryOutcome::Failed(
                    "discovery thread stopped".to_string(),
                )),
            },
            DiscoveryState::Done => None,
        };
        if let Some(outcome) = outcome {
            self.apply_discovery(outcome);
        }
    }

    fn apply_discovery(&mut self, outcome: DiscoveryOutcome) {
        let previous = self.selected_entry().map(|entry| entry.addr);
        let (proxies, error) = apply_discovery_result(&self.proxies, &outcome);
        self.proxies = proxies;
        self.discovery_error = error;
        if let Some(addr) = previous {
            if let Some(idx) = self.proxies.iter().position(|entry| entry.addr == addr) {
                self.selected_proxy_idx = idx;
            }
        }
        self.selected_proxy_idx = self
            .selected_proxy_idx
            .min(self.proxies.len().saturating_sub(1));

        match &outcome {
            DiscoveryOutcome::Found(relays) => {
                tracing::info!("Relay discovery found {} relays", relays.len());
                self.persist_config();
                if !self.status.connected && !self.proxies.is_empty() {
                    self.connect_selected();
                }
            }
            DiscoveryOutcome::Failed(reason) => {
                tracing::warn!("Relay discovery failed: {reason}");
            }
        }
        self.discovery = DiscoveryState::Done;
    }

    fn poll_health(&mut self) {
        if let Some(probe) = &mut self.health {
            if probe.result.is_none() {
                if let Ok(result) = probe.rx.try_recv() {
                    probe.result = Some(result);
                }
            }
        }
        if !self.status.connected {
            self.health = None;
            return;
        }
        let Some(entry) = self.selected_entry() else {
            self.health = None;
            return;
        };
        if entry.node_id.is_none() {
            self.health = None;
            return;
        }
        let addr = SocketAddrV4::new(*entry.addr.ip(), discovery::HEALTH_PORT);
        let needs_probe = match &self.health {
            Some(probe) => probe.addr != addr,
            None => true,
        };
        if needs_probe {
            self.health = Some(HealthProbe {
                addr,
                rx: discovery::spawn_health_probe(addr),
                result: None,
            });
        }
    }

    fn shutdown_engine(&mut self) {
        let mut engine = self.engine.lock().unwrap();
        engine.stop_interceptor();
        engine.stop_windivert();
        engine.stop_capture();
        engine.stop_redirect();
        engine.disconnect();
    }

    fn start_update_check(&mut self) {
        if matches!(self.update_check, UpdateCheckState::Checking) {
            return;
        }
        *self.update_shared.lock().unwrap() = None;
        self.update_check = UpdateCheckState::Checking;
        self.show_update_dialog = true;
        let shared = Arc::clone(&self.update_shared);
        std::thread::spawn(move || {
            let result = crate::update::check_for_update_blocking();
            *shared.lock().unwrap() = Some(result);
        });
    }
}

// ── eframe::App impl ─────────────────────────────────────────────────────────

impl<P: Platform> eframe::App for LightSpeedApp<P> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // One-time first-frame setup: platform-specific fonts and the
        // branded header mark texture.
        if !self.fonts_setup {
            self.fonts_setup = true;
            P::setup_fonts(&ctx);
            self.header_icon = brand_mark_texture(&ctx);
        }

        // Tray Quit and the window X are deliberately different: Quit tears
        // the process down, X hides to the tray where a real tray exists.
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        match close_decision(
            self.tray_available(),
            self.quit.load(Ordering::Relaxed),
            close_requested,
        ) {
            CloseDecision::Exit => {
                tracing::info!("Quit requested — stopping engine");
                self.shutdown_engine();
                std::process::exit(0);
            }
            CloseDecision::Hide => {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                return;
            }
            CloseDecision::Ignore => {}
        }

        // ── Poll tray events ─────────────────────────────────────────────
        let tray_actions = match &self.tray {
            Some(tray) => tray.poll_events(&ctx),
            None => Vec::new(),
        };
        for action in tray_actions {
            match action {
                TrayAction::Connect => self.connect_selected(),
                TrayAction::Disconnect => {
                    self.engine.lock().unwrap().disconnect();
                }
            }
        }

        // Refresh engine snapshot and background relay state.
        self.status = self.engine.lock().unwrap().snapshot();
        self.poll_discovery();
        self.poll_health();

        // Collect a finished update check from the background thread.
        if matches!(self.update_check, UpdateCheckState::Checking) {
            if let Some(result) = self.update_shared.lock().unwrap().take() {
                self.update_check = UpdateCheckState::Done(result);
                self.show_update_dialog = true;
            }
        }

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

            if let Some(tray) = &self.tray {
                tray.set_state(new_tray_state, self.status.latest_rtt_ms);
            }
        }

        // ── Main panel ────────────────────────────────────────────────────
        egui::CentralPanel::default().show(ui, |ui| {
            // Keep the footer action bar pinned while the status content
            // scrolls, so the Boost button stays reachable at small sizes.
            egui::ScrollArea::vertical()
                .max_height((ui.available_height() - 40.0).max(120.0))
                .show(ui, |ui| {
            // ── Header ───────────────────────────────────────────────────
            ui.horizontal(|ui| {
                if let Some(mark) = &self.header_icon {
                    ui.add(
                        egui::Image::from_texture(mark)
                            .fit_to_exact_size(egui::vec2(22.0, 22.0)),
                    );
                }
                ui.heading("LightSpeed");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let (label, colour) = if self.status.connected {
                        ("• Connected", egui::Color32::from_rgb(80, 200, 120))
                    } else {
                        ("• Disconnected", egui::Color32::from_rgb(220, 80, 80))
                    };
                    let response = ui.colored_label(colour, label);
                    if self.status.connected {
                        response.on_hover_text(format!("Boost Server: {}", self.status.proxy_addr));
                    }
                });
            });

            ui.separator();

            // ── Boost Server selector ─────────────────────────────────────
            ui.horizontal_wrapped(|ui| {
                ui.label("Boost Server:")
                    .on_hover_ui(|ui| {
                        ui.label("Choose the relay server closest to your game server.\n\
                                  Closer to the game server = lower ping, even if it's\n\
                                  farther from your physical location.");
                        ui.hyperlink_to("📖 Which server should I pick?",
                            "https://github.com/ShibbityShwab/lightspeed/wiki/Choosing-a-Boost-Server");
                    });
                if self.proxies.is_empty() {
                    match &self.discovery {
                        DiscoveryState::InFlight(_) => {
                            ui.spinner();
                            ui.weak("Discovering relays…");
                        }
                        DiscoveryState::Done => {
                            ui.colored_label(
                                egui::Color32::from_rgb(255, 190, 60),
                                "⚠ No relays discovered",
                            )
                            .on_hover_text(
                                self.discovery_error.clone().unwrap_or_else(|| {
                                    "The relay registry returned no usable relays.".to_string()
                                }),
                            );
                            if ui.small_button("Retry").clicked() {
                                self.start_discovery();
                            }
                        }
                    }
                } else {
                    let prev = self.selected_proxy_idx;
                    for (i, entry) in self.proxies.iter().enumerate() {
                        let btn = ui.selectable_value(&mut self.selected_proxy_idx, i, &entry.label);
                        btn.on_hover_text(format!("{}", entry.addr));
                    }
                    if self.selected_proxy_idx != prev {
                        self.connect_selected();
                        self.persist_config();
                    }
                }
                if ui.button("⚙ Manage").clicked() {
                    self.show_proxy_manager = true;
                }
            });

            if !self.proxies.is_empty() {
                if let Some(err) = self.discovery_error.clone() {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 190, 60),
                            "⚠ Relay refresh failed — using the saved list",
                        )
                        .on_hover_text(err);
                        if ui.small_button("Retry").clicked() {
                            self.start_discovery();
                        }
                    });
                }
            }

            ui.horizontal(|ui| {
                ui.label("Boost Ping:")
                    .on_hover_ui(|ui| {
                        ui.label("Round-trip time from your PC to the Boost Server.");
                        ui.horizontal(|ui| {
                            ui.colored_label(egui::Color32::from_rgb(80, 200, 120), "• < 60ms");
                            ui.label("  |  ");
                            ui.colored_label(egui::Color32::from_rgb(255, 210, 0), "• 60-120ms");
                            ui.label("  |  ");
                            ui.colored_label(egui::Color32::from_rgb(220, 80, 80), "• > 120ms");
                        });
                        ui.label("This becomes your in-game ping when Boost is engaged.");
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
                    "Keepalive: {} sent / {} echoed",
                    self.status.packets_sent, self.status.packets_received
                ))
                .on_hover_text(
                    "Keepalive pings sent to the Boost Server, and echo replies \
                     received. '0 echoed' is normal until the relay answers and \
                     does not affect game traffic.",
                );
            });

            // ── Control-plane registration ────────────────────────────────
            ui.horizontal(|ui| {
                ui.label("Control:")
                    .on_hover_text("The relay's QUIC control plane registers this session and issues a token.");
                if self.status.control_registered {
                    let node = self.status.node_id.as_deref().unwrap_or("relay");
                    ui.colored_label(
                        egui::Color32::from_rgb(80, 200, 120),
                        format!("✓ registered with {node}"),
                    );
                    if let Some(token) = self.status.session_token {
                        ui.weak(format!("token 0x{token:08x}"));
                    }
                } else if let Some(ref err) = self.status.registration_error {
                    ui.colored_label(egui::Color32::from_rgb(220, 80, 80), format!("⚠ {err}"));
                } else {
                    ui.weak("registering…");
                }
                if self.status.keepalive_generation > 0 {
                    ui.separator();
                    ui.weak(format!("session #{}", self.status.keepalive_generation));
                }
            });

            // ── Relay health (optional, never blocks the frame) ───────────
            if self.status.connected {
                if let Some(probe) = &self.health {
                    ui.horizontal(|ui| {
                        ui.label("Relay health:").on_hover_text(
                            "Live counters from the selected relay's HTTP /health endpoint.",
                        );
                        match &probe.result {
                            Some(Ok(health)) => {
                                ui.monospace(format!("{} packets relayed", health.packets_relayed));
                                ui.separator();
                                ui.monospace(format!("{} sessions", health.sessions_created));
                            }
                            Some(Err(_)) => {
                                ui.weak("unavailable");
                            }
                            None => {
                                ui.weak("checking…");
                            }
                        }
                    });
                }
            }

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
                    )
                    .on_hover_text(format!(
                        "Interceptor backend: {}",
                        self.status.interceptor_platform
                    ));
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
                        let (lo, hi) = self.selected_game_ports();
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
                        let (lo, hi) = self.selected_game_ports();
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
                    ui.separator();
                    ui.label("Injected:")
                        .on_hover_text("Responses injected back into your game.");
                    ui.monospace(format!("{:>8}", self.status.capture_injected));
                });
                if !self.status.capture_bpf.is_empty() {
                    ui.weak(format!("Filter: {}", self.status.capture_bpf))
                        .on_hover_text("BPF capture filter in use for this session.");
                }
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
                        " — {} -> port {}",
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
                    connect_instruction(self.selected_game(), self.status.redirect_local_port);
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
                                if let Some(idx) = games()
                                    .iter()
                                    .position(|entry| entry.key.eq_ignore_ascii_case(name))
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
                        .selected_text(self.selected_game().display)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            for (i, entry) in games().iter().enumerate() {
                                ui.selectable_value(&mut self.selected_game_idx, i, entry.display);
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
                                    "Relaunches LightSpeed with elevated privileges (system permission prompt).",
                                )
                                .clicked()
                            {
                                P::relaunch_as_admin();
                            }
                        });
                }

                ui.add_space(10.0);

                // ── THE OPTIMIZE BUTTON ───────────────────────────────────
                let has_relay = self.selected_entry().is_some();
                let can_boost = self.is_admin && has_relay;
                let btn_color = if can_boost {
                    egui::Color32::from_rgb(80, 50, 5)
                } else {
                    egui::Color32::from_rgb(55, 55, 55)
                };
                let btn_label = if !has_relay {
                    "⚡  BOOST MY GAME  (no relay available)"
                } else if self.is_admin {
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
                                .color(if can_boost {
                                    egui::Color32::from_rgb(255, 210, 100)
                                } else {
                                    egui::Color32::from_rgb(140, 140, 140)
                                }),
                        )
                        .fill(btn_color),
                    )
                    .on_hover_text(if !has_relay {
                        "No relay is available. Check your internet connection, or \
                         add a custom proxy in Manage."
                    } else if self.is_admin {
                        "Click Boost, then launch your game and join any server.\n\
                         LightSpeed automatically finds your game server and routes \
                         traffic through the Boost Server for lower ping."
                    } else {
                        "Run LightSpeed as Administrator to boost your game."
                    })
                    .clicked()
                    && can_boost
                {
                    // Warm up port detection for diagnostic logging.
                    let _ = parse_custom_port_range(&self.custom_port_input)
                        .unwrap_or_else(|| P::detect_game_ports(self.selected_game_idx));

                    if let Some(proxy) = self.selected_proxy_addr() {
                        let game_key = self.selected_game().key;
                        let result = self.engine.lock().unwrap().start_interceptor(
                            game_key,
                            proxy,
                            self.fec_enabled,
                            4, // default FEC K
                        );
                        if let Err(e) = result {
                            tracing::error!("start_interceptor failed: {}", e);
                        } else {
                            self.boost_start = Some(std::time::Instant::now());
                        }
                    }
                }

                ui.add_space(6.0);

                // ── Advanced expander (manual server IP fallback) ─────────
                let adv_label = if self.show_advanced {
                    "v Advanced — set server manually"
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
                                let default_port = self.selected_game().default_port;
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
                                    let entry = self.selected_game();
                                    let local_port = server_addr.port().max(entry.default_port);
                                    if let Some(proxy) = self.selected_proxy_addr() {
                                        self.engine.lock().unwrap().start_redirect(
                                            server_addr,
                                            local_port,
                                            self.fec_enabled,
                                            4,
                                            entry.display.to_string(),
                                            proxy,
                                        );
                                    }
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
                                self.selected_game(),
                                self.server_input
                                    .parse::<SocketAddrV4>()
                                    .map(|a| a.port())
                                    .unwrap_or(self.selected_game().default_port),
                            );
                            ui.weak(instruction);

                            ui.add_space(4.0);
                            ui.weak(format!(
                                "Capture backend (pcap mode): {}",
                                if P::is_capture_available() {
                                    "available"
                                } else {
                                    "not detected"
                                }
                            ));
                        });
                }
            }
                });

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
                    .add_enabled(
                        self.selected_entry().is_some(),
                        egui::Button::new("Reconnect Boost Server").small(),
                    )
                    .on_hover_text("Reconnect to the Boost Server.")
                    .clicked()
                {
                    self.connect_selected();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .small_button("Check for updates")
                        .on_hover_text("Check whether a newer version of LightSpeed is available.")
                        .clicked()
                    {
                        self.start_update_check();
                    }
                    if ui
                        .small_button("Open log file")
                        .on_hover_text("Open gui-trace.log in your file manager.")
                        .clicked()
                    {
                        paths::open_in_os(&paths::log_file());
                    }
                    if self.tray_available() && ui.small_button("Hide to tray").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                });
            });
        });

        // ── Proxy manager window ─────────────────────────────────────────
        if self.show_proxy_manager {
            let mut remove_idx: Option<usize> = None;
            let mut add_addr: Option<SocketAddrV4> = None;
            let mut reset = false;

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
                            ui.weak(entry.node_id.as_deref().unwrap_or("custom"));
                            if ui.button("×").clicked() {
                                remove_idx = Some(i);
                            }
                        });
                    }
                    if self.proxies.is_empty() {
                        ui.weak(
                            "No relays yet. Discovery runs automatically; you can \
                             also add a custom proxy below.",
                        );
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
                        if ui.button("Add Proxy").clicked()
                            && self.manager_addr_input.parse::<SocketAddrV4>().is_ok()
                        {
                            add_addr = Some(self.manager_addr_input.parse().unwrap());
                        }
                        if ui.button("Refresh relays").clicked() {
                            self.start_discovery();
                        }
                    });

                    ui.separator();
                    ui.horizontal(|ui| {
                        if ui.button("Open config folder").clicked() {
                            paths::open_in_os(&paths::config_dir());
                        }
                        if ui.button("Reset to defaults").clicked() {
                            reset = true;
                        }
                        if ui.button("Close").clicked() {
                            self.show_proxy_manager = false;
                        }
                    });
                    if let Some(err) = &self.config_error {
                        ui.colored_label(egui::Color32::from_rgb(220, 80, 80), err);
                    }
                });

            if let Some(idx) = remove_idx {
                self.proxies.remove(idx);
                self.selected_proxy_idx = self
                    .selected_proxy_idx
                    .min(self.proxies.len().saturating_sub(1));
                self.persist_config();
            }
            if let Some(addr) = add_addr {
                let label = if self.manager_label_input.is_empty() {
                    addr.to_string()
                } else {
                    self.manager_label_input.clone()
                };
                self.proxies.push(ProxyEntry::custom(addr, label));
                self.manager_label_input.clear();
                self.manager_addr_input.clear();
                self.persist_config();
            }
            if reset {
                match config::reset(&paths::config_file()) {
                    Ok(()) => {
                        self.proxies = env_proxies();
                        self.selected_proxy_idx = 0;
                        self.config_error = None;
                        self.start_discovery();
                        self.connect_selected();
                    }
                    Err(e) => self.config_error = Some(e),
                }
            }
        }

        // ── Update check dialog ──────────────────────────────────────────
        if self.show_update_dialog {
            egui::Window::new("Check for updates")
                .resizable(false)
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(&ctx, |ui| {
                    match &self.update_check {
                        UpdateCheckState::Checking => {
                            ui.horizontal(|ui| {
                                ui.spinner();
                                ui.label("Checking for updates…");
                            });
                        }
                        UpdateCheckState::Done(result) => {
                            let current = match result {
                                Ok(status) => status.current.clone(),
                                Err(_) => env!("CARGO_PKG_VERSION").to_string(),
                            };
                            let latest = match result {
                                Ok(status) => status
                                    .latest
                                    .clone()
                                    .unwrap_or_else(|| "unknown".to_string()),
                                Err(_) => "unknown".to_string(),
                            };
                            ui.label(format!("Current version: {current}"));
                            ui.label(format!("Latest version: {latest}"));
                            ui.add_space(4.0);
                            ui.label(crate::update::update_status_line(result));
                        }
                        UpdateCheckState::Idle => {}
                    }
                    ui.add_space(4.0);
                    ui.horizontal(|ui| {
                        if ui.button("Close").clicked() {
                            self.show_update_dialog = false;
                            self.update_check = UpdateCheckState::Idle;
                        }
                    });
                });
        }

        // ── Repaint schedule ─────────────────────────────────────────────
        let repaint_interval = if self.status.redirect_active
            || self.status.capture_active
            || self.status.windivert_active
            || self.status.interceptor_active
        {
            Duration::from_millis(500) // 2 Hz for live counters
        } else if matches!(self.discovery, DiscoveryState::InFlight(_)) {
            Duration::from_millis(200) // keep the discovery spinner moving
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

fn connect_instruction(game: &GameEntry, local_port: u16) -> String {
    match game.key {
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
            games().iter().find_map(|entry| {
                if entry.display.to_lowercase().contains(&name_lower)
                    || name_lower.contains(entry.key)
                {
                    Some(entry.key.to_string())
                } else {
                    None
                }
            })
        }
        Err(_) => None,
    }
}

/// Decode the embedded brand tile into a texture for the header.
///
/// A decode failure is logged and treated as absent so the header falls back
/// to plain text.
fn brand_mark_texture(ctx: &egui::Context) -> Option<egui::TextureHandle> {
    const MARK_PNG: &[u8] = include_bytes!("../../web/assets/brand/icon-256.png");
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

#[cfg(test)]
mod tests {
    use super::{apply_discovery_result, close_decision, games, CloseDecision};
    use crate::config::ProxyEntry;
    use crate::discovery::{DiscoveryOutcome, RelayInfo};
    use std::net::SocketAddrV4;

    fn addr(s: &str) -> SocketAddrV4 {
        s.parse().expect("test address")
    }

    #[test]
    fn window_close_hides_when_a_real_tray_exists() {
        assert_eq!(close_decision(true, false, true), CloseDecision::Hide);
    }

    #[test]
    fn window_close_exits_when_there_is_no_tray_to_hide_to() {
        assert_eq!(close_decision(false, false, true), CloseDecision::Exit);
    }

    #[test]
    fn tray_quit_exits_even_with_a_tray_and_an_open_window() {
        assert_eq!(close_decision(true, true, false), CloseDecision::Exit);
        assert_eq!(close_decision(true, true, true), CloseDecision::Exit);
        assert_eq!(close_decision(false, true, true), CloseDecision::Exit);
    }

    #[test]
    fn no_close_and_no_quit_keeps_running() {
        assert_eq!(close_decision(true, false, false), CloseDecision::Ignore);
        assert_eq!(close_decision(false, false, false), CloseDecision::Ignore);
    }

    #[test]
    fn game_table_matches_the_client_registry() {
        let entries = games();
        let registry = lightspeed_client::games::all_game_keys();
        // The client registry owns the entry count; this test only proves the
        // GUI table is derived from it without drift.
        assert_eq!(entries.len(), registry.len());
        for (entry, (key, display)) in entries.iter().zip(registry) {
            assert_eq!(entry.key, key);
            assert_eq!(entry.display, display);
            assert!(
                entry.default_port > 0,
                "{} resolved to no default port",
                entry.key
            );
        }
    }

    #[test]
    fn discovery_failure_keeps_existing_relays_and_reports_the_reason() {
        let existing = vec![
            ProxyEntry::discovered("relay-lax-1", addr("207.246.106.36:4434"), "LAX"),
            ProxyEntry::custom(addr("127.0.0.1:4434"), "Local proxy"),
        ];
        let (proxies, error) = apply_discovery_result(
            &existing,
            &DiscoveryOutcome::Failed("registry unreachable".to_string()),
        );
        assert_eq!(proxies, existing);
        assert_eq!(error.as_deref(), Some("registry unreachable"));
    }

    #[test]
    fn discovery_success_refreshes_relays_and_keeps_custom_entries() {
        let existing = vec![
            ProxyEntry::discovered("relay-old", addr("9.9.9.9:4434"), "Old relay"),
            ProxyEntry::custom(addr("127.0.0.1:4434"), "Local proxy"),
        ];
        let outcome = DiscoveryOutcome::Found(vec![RelayInfo {
            node_id: "relay-lax-1".to_string(),
            addr: addr("207.246.106.36:4434"),
        }]);
        let (proxies, error) = apply_discovery_result(&existing, &outcome);
        assert_eq!(error, None);
        assert_eq!(proxies.len(), 2);
        assert_eq!(proxies[0].node_id.as_deref(), Some("relay-lax-1"));
        assert_eq!(proxies[0].label, "LAX — Los Angeles");
        assert_eq!(proxies[1].label, "Local proxy");
        assert!(!proxies[1].is_discovered());
    }
}
