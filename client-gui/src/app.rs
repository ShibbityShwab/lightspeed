//! # LightSpeed GUI - App
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
use crate::geo;
use crate::globe;
use crate::paths;
use crate::platform::{Platform, QuitFlag, TrayAction, TrayHandle};
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
/// stub).  Most of the UI logic is platform-independent - only the tray
/// interaction, font loading, port detection, and admin checks delegate to
/// `P`.
pub struct LightSpeedApp<P: Platform> {
    engine: Arc<Mutex<LightSpeedEngine>>,
    status: EngineStatus,
    tray: Option<P::Tray>,
    quit: QuitFlag,

    // ── Proxy connection ─────────────────────────────────────────────────
    selected_proxy_idx: usize,
    auto_select: bool,
    relay_race: Option<Receiver<Option<SocketAddrV4>>>,
    show_proxy_manager: bool,
    manager_label_input: String,
    manager_addr_input: String,
    config_error: Option<String>,

    // ── Game routing ──────────────────────────────────────────────────────
    selected_game_idx: usize,
    server_input: String,
    fec_enabled: bool,
    share_latency_stats: bool,
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
        let auto_select = saved.auto_select;
        let share_latency_stats = saved.share_latency_stats;

        let mut app = Self {
            engine,
            status,
            tray,
            quit,
            selected_proxy_idx,
            auto_select,
            relay_race: None,
            show_proxy_manager: false,
            manager_label_input: String::new(),
            manager_addr_input: String::new(),
            config_error: None,
            selected_game_idx,
            server_input: String::new(),
            fec_enabled: false,
            share_latency_stats,
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
        app.engine
            .lock()
            .unwrap()
            .set_telemetry_enabled(share_latency_stats);
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
            auto_select: self.auto_select,
            share_latency_stats: self.share_latency_stats,
        };
        if let Err(e) = config::save(&paths::config_file(), &config) {
            tracing::warn!("Could not persist GUI config: {e}");
        }
    }

    /// Race the discovered relays and connect to the fastest one.
    fn start_relay_race(&mut self) {
        let addrs: Vec<SocketAddrV4> = self
            .proxies
            .iter()
            .map(|entry| discovery::health_endpoint(entry.addr))
            .collect();
        if addrs.is_empty() {
            return;
        }
        tracing::info!("Racing {} relays for the lowest latency", addrs.len());
        self.relay_race = Some(discovery::spawn_relay_race(addrs));
    }

    /// Apply a finished latency race: connect to the winner when auto-select is
    /// still on and the user has not connected or pinned a relay meanwhile.
    fn poll_relay_race(&mut self) {
        let winner = match &self.relay_race {
            Some(rx) => match rx.try_recv() {
                Ok(winner) => winner,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
            },
            None => return,
        };
        self.relay_race = None;

        if !should_apply_race(self.auto_select, self.status.connected) {
            return;
        }
        let Some(addr) = winner else {
            tracing::warn!("Relay latency race found no reachable relay");
            return;
        };
        if let Some(idx) = self.proxies.iter().position(|entry| entry.addr == addr) {
            if idx != self.selected_proxy_idx {
                tracing::info!("Auto-selected the fastest relay {addr}");
                self.selected_proxy_idx = idx;
            }
            self.connect_selected();
            self.persist_config();
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
                if should_race(self.auto_select, self.status.connected, self.proxies.len()) {
                    self.start_relay_race();
                } else if !self.status.connected && !self.proxies.is_empty() {
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
        let addr = discovery::health_endpoint(entry.addr);
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

    /// Start the interceptor boost for the selected game and relay.
    fn start_boost(&mut self) {
        // Warm up port detection for diagnostic logging.
        let _ = parse_custom_port_range(&self.custom_port_input)
            .unwrap_or_else(|| P::detect_game_ports(self.selected_game_idx));

        if let Some(proxy) = self.selected_proxy_addr() {
            let game_key = self.selected_game().key;
            let mut engine = self.engine.lock().unwrap();
            engine.set_telemetry_game(game_key);
            let result = engine.start_interceptor(
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

    /// Stop every active boost backend and clear the start timestamp.
    fn stop_boost(&mut self) {
        let mut engine = self.engine.lock().unwrap();
        engine.stop_interceptor();
        engine.stop_windivert();
        engine.stop_capture();
        engine.stop_redirect();
        self.boost_start = None;
    }

    /// Relaunch with elevated privileges so the interceptor can start.
    fn restart_elevated(&mut self) {
        P::relaunch_as_admin();
    }

    /// Open the GUI trace log in the OS file manager.
    fn reveal_log_file(&self) {
        paths::open_in_os(&paths::log_file());
    }

    /// Drop or restore the control-plane link to the current relay.
    ///
    /// Stopping an active boost first is deliberate: leaving the interceptor or
    /// its tunnel running against a relay we just disconnected would strand the
    /// engine mid-redirect, which is the state the old Disconnect button
    /// avoided by only appearing while idle.
    fn toggle_relay_connection(&mut self) {
        if self.status.connected {
            if self.status.interceptor_active {
                self.stop_boost();
            }
            self.engine.lock().unwrap().disconnect();
        } else if self.selected_entry().is_some() {
            self.connect_selected();
        }
    }

    /// Configuration section: relay, game, and advanced settings in one scroll.
    fn config_body(&mut self, ui: &mut egui::Ui) {
        // Boost Server: which relay carries the game traffic.
        theme::card(ui, "Boost Server", |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("Boost Server:").on_hover_ui(|ui| {
                    ui.label(
                        "Choose the relay server closest to your game server.\n\
                                  Closer to the game server = lower ping, even if it's\n\
                                  farther from your physical location.",
                    );
                    ui.hyperlink_to(
                        "Which server should I pick?",
                        "https://github.com/ShibbityShwab/lightspeed/wiki/Choosing-a-Boost-Server",
                    );
                });
                if self.proxies.is_empty() {
                    match &self.discovery {
                        DiscoveryState::InFlight(_) => {
                            ui.spinner();
                            ui.weak("Discovering relays…");
                        }
                        DiscoveryState::Done => {
                            ui.colored_label(theme::WARN, "No relays discovered")
                                .on_hover_text(self.discovery_error.clone().unwrap_or_else(|| {
                                    "The relay registry returned no usable relays.".to_string()
                                }));
                            if ui.small_button("Retry").clicked() {
                                self.start_discovery();
                            }
                        }
                    }
                } else {
                    // One dropdown rather than a pill per relay plus a separate
                    // Auto switch: the pills highlighted the current pick even
                    // when Auto had made it, so Auto looked off while it was on.
                    let current = if self.auto_select {
                        "Auto (fastest)".to_string()
                    } else {
                        self.selected_entry()
                            .map(|e| e.label.clone())
                            .unwrap_or_else(|| "Select a relay".to_string())
                    };
                    let mut auto = self.auto_select;
                    let mut chosen: Option<usize> = None;

                    egui::ComboBox::from_id_salt("boost_server_select")
                        .selected_text(current)
                        .width(200.0)
                        .show_ui(ui, |ui| {
                            if ui.selectable_label(auto, "Auto (fastest)").clicked() {
                                auto = true;
                            }
                            ui.separator();
                            for (i, entry) in self.proxies.iter().enumerate() {
                                let picked = !auto && i == self.selected_proxy_idx;
                                if ui.selectable_label(picked, &entry.label).clicked() {
                                    auto = false;
                                    chosen = Some(i);
                                }
                            }
                        })
                        .response
                        .on_hover_text(
                            "Auto uses the lowest-latency relay and reacts as new ones \
                             appear. Picking one by name pins it instead.",
                        );

                    if auto != self.auto_select {
                        self.auto_select = auto;
                        self.persist_config();
                        if auto && !self.status.connected {
                            self.start_relay_race();
                        }
                    }
                    if let Some(i) = chosen {
                        self.selected_proxy_idx = i;
                        self.auto_select = false;
                        self.connect_selected();
                        self.persist_config();
                    }
                }
                if ui.button("Manage").clicked() {
                    self.show_proxy_manager = true;
                }
            });

            if !self.proxies.is_empty() {
                if let Some(err) = self.discovery_error.clone() {
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(255, 190, 60),
                            "Relay refresh failed - using the saved list",
                        )
                        .on_hover_text(err);
                        if ui.small_button("Retry").clicked() {
                            self.start_discovery();
                        }
                    });
                }
            }
        });

        ui.add_space(theme::S3);

        // Game: what is being boosted, plus the reliability shield.
        theme::card(ui, "Game", |ui| {
            if let Some(ref detected) = self.auto_detected_game {
                ui.horizontal(|ui| {
                    ui.colored_label(theme::OK, "Game found:")
                        .on_hover_text("LightSpeed automatically detected a running game.");
                    ui.label(detected);
                });
            } else {
                ui.horizontal(|ui| {
                    ui.weak("No game running - select your game and click Boost")
                        .on_hover_text(
                            "Start your game and connect to a server, then click \
                             BOOST MY GAME. Or select your game manually below.",
                        );
                    if ui.small_button("Rescan").clicked() {
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
            ui.horizontal(|ui| {
                ui.label("Game:  ").on_hover_text(
                    "Select the game you want to boost. LightSpeed will \
                                    automatically route its traffic for lower ping.",
                );
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

            ui.horizontal(|ui| {
                ui.checkbox(
                    &mut self.fec_enabled,
                    "Reliability Shield - recover lost packets (+25% data)",
                )
                .on_hover_ui(|ui| {
                    ui.label(
                        "Reliability Shield sends extra repair data so the Boost Server \
                         can reconstruct any packets your connection drops - no more \
                         rubber-banding from packet loss. Uses ~25% extra upload bandwidth.",
                    );
                    ui.hyperlink_to(
                        "Learn more about Reliability Shield",
                        "https://github.com/ShibbityShwab/lightspeed/wiki/Reliability-Shield",
                    );
                });
            });

            ui.add_space(4.0);
        });

        ui.add_space(theme::S3);

        // Rarely-used options folded behind a single collapsed section.
        egui::CollapsingHeader::new("More")
            .default_open(false)
            .show(ui, |ui| {
                // Advanced: manual server override and custom port range.
                theme::card(ui, "Advanced", |ui| {
            let adv_label = if self.show_advanced {
                "v Advanced - set server manually"
            } else {
                "▶ Advanced - set server manually"
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
                                        "Port not detected - fix guide",
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
                                    "invalid",
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
                                "Enter a valid IP:port (e.g. 1.2.3.4:28015)",
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
        });

        ui.add_space(theme::S3);

        // Privacy: anonymous latency telemetry.
        theme::card(ui, "Privacy", |ui| {
            ui.horizontal(|ui| {
                let changed = ui
                    .checkbox(
                        &mut self.share_latency_stats,
                        "Share anonymous latency stats",
                    )
                    .on_hover_text(
                        "Send anonymous aggregate RTT, jitter, and FEC stats to your \
                         relay so the community can see real latency improvements. No \
                         IP address, identifier, or packet content is ever sent.",
                    )
                    .changed();
                if changed {
                    self.persist_config();
                    self.engine
                        .lock()
                        .unwrap()
                        .set_telemetry_enabled(self.share_latency_stats);
                }
            });
        });

        ui.add_space(theme::S3);

        // Maintenance: self-update check.
        theme::card(ui, "Maintenance", |ui| {
            if ui
                .button("Check for updates")
                .on_hover_text("Check whether a newer version of LightSpeed is available.")
                .clicked()
            {
                self.start_update_check();
            }

            // Manual link control. The status window only starts and stops
            // boosting; dropping the relay connection is a rare, deliberate
            // action, so it lives here rather than competing with Boost.
            ui.add_space(theme::S2);
            ui.horizontal(|ui| {
                let connected = self.status.connected;
                let label = if connected {
                    "Disconnect from relay"
                } else {
                    "Connect to relay"
                };
                let hint = if connected {
                    "Drop the control-plane link to the current relay"
                } else {
                    "Reconnect to the selected relay"
                };
                if ui.button(label).on_hover_text(hint).clicked() {
                    self.toggle_relay_connection();
                }
            });
        });

        ui.add_space(theme::S3);

        // About carries the brand mark that used to sit above the state banner,
        // where the OS title bar was already showing the same logo and name.
        about(ui, self.header_icon.as_ref());

        ui.add_space(theme::S3);

        // Connection details live at the top of the window, beside the state
        // banner, rather than behind this disclosure.
            });
    }
}

// ── eframe::App impl ─────────────────────────────────────────────────────────

impl<P: Platform> eframe::App for LightSpeedApp<P> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // Windows get snapped narrow on a small screen or beside a game, so the
        // layout adapts rather than clipping: tighter margins and a smaller
        // globe below this width.
        let narrow = ui.available_width() < 380.0;

        // One-time first-frame setup: platform-specific fonts and the
        // branded header mark texture.
        if !self.fonts_setup {
            self.fonts_setup = true;
            theme::apply(&ctx);
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
                tracing::info!("Quit requested - stopping engine");
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
        self.poll_relay_race();
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

        // ── Status window ────────────────────────────────────────────────
        egui::CentralPanel::default().show(ui, |ui| {
            // No in-app brand row: the OS title bar already says "LightSpeed"
            // and carries the mark, so repeating it here was duplication. The
            // logo and the project links live in the About card instead.
            // State banner: the single answer to "am I boosted?".
            let boosting = self.status.interceptor_active
                || self.status.windivert_active
                || self.status.capture_active
                || self.status.redirect_active;
            let (headline, colour) = if boosting {
                ("BOOSTING", theme::OK)
            } else if self.status.connected {
                ("CONNECTED", theme::ACCENT)
            } else {
                ("OFFLINE", theme::TEXT_DIM)
            };
            // The headline answers the two questions a player actually has, in
            // one glance: is it on, and how good is it. The relay address is
            // detail, so it lives in the card below rather than here.
            let detail = match (boosting, self.status.connected) {
                (true, _) => format!(
                    "{} - {:.0} ms",
                    self.selected_game().display,
                    self.status.latest_rtt_ms
                ),
                (false, true) if self.status.latest_rtt_ms > 0.0 => {
                    format!("Not boosting - {:.0} ms", self.status.latest_rtt_ms)
                }
                (false, true) => "Not boosting - measuring".to_string(),
                (false, false) => "No boost server".to_string(),
            };
            egui::Frame::new()
                .fill(theme::SURFACE)
                .stroke(egui::Stroke::new(1.0, theme::BORDER))
                .corner_radius(egui::CornerRadius::same(10))
                .inner_margin(egui::Margin::same(theme::S4 as i8))
                .show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    ui.vertical_centered(|ui| {
                        ui.label(
                            egui::RichText::new(headline)
                                .color(colour)
                                .size(theme::DISPLAY)
                                .family(theme::semibold()),
                        );
                        ui.add_space(theme::S1);
                        // The ping carries its own quality colour, so the one
                        // glance that answers "is it on and is it good" does
                        // not need reading.
                        let detail_colour =
                            if self.status.connected && self.status.latest_rtt_ms > 0.0 {
                                rtt_colour(self.status.latest_rtt_ms)
                            } else {
                                theme::TEXT_DIM
                            };
                        ui.label(
                            egui::RichText::new(detail)
                                .size(theme::LABEL)
                                .color(detail_colour),
                        );
                    });
                });

            ui.add_space(theme::S2);

            // One primary action button. When it cannot act, it says why
            // rather than only greying out, so the state is never a dead end.
            let can_act = self.is_admin && self.selected_entry().is_some() && self.status.connected;
            // While unelevated the only action the user can take is elevating,
            // so that is what the primary slot offers. A disabled Boost button
            // here would be the largest control on screen doing nothing.
            let enabled = boosting || can_act || !self.is_admin;
            let (label, _fill, why) = if boosting {
                ("■  STOP BOOST", theme::BAD, "Stop routing game traffic")
            } else if !self.is_admin {
                (
                    "RESTART AS ADMINISTRATOR",
                    theme::ACCENT,
                    "Required to redirect game traffic",
                )
            } else if can_act {
                ("BOOST MY GAME", theme::ACCENT, "Start routing game traffic")
            } else if !self.status.connected {
                ("BOOST MY GAME", theme::SURFACE, "Connecting to a relay…")
            } else {
                (
                    "BOOST MY GAME",
                    theme::SURFACE,
                    "Pick a boost server in Settings first",
                )
            };
            let primary = ui.add_enabled_ui(enabled, |ui| {
                ui.add_sized(
                    [ui.available_width(), 46.0],
                    theme::primary_button(label, enabled),
                )
            });
            primary.response.on_hover_text(why);
            if primary.inner.clicked() {
                if boosting {
                    self.stop_boost();
                } else if !self.is_admin {
                    self.restart_elevated();
                } else {
                    self.start_boost();
                }
            }

            if !self.is_admin && !boosting {
                ui.add_space(theme::S2);
                ui.vertical_centered(|ui| {
                    ui.label(
                        egui::RichText::new("Administrator is required to redirect game traffic")
                            .size(theme::LABEL)
                            .color(theme::TEXT_DIM),
                    );
                });
            }

            ui.add_space(theme::S3);

            // One card for the whole connection: where it egresses, and what it
            // has carried. Two cards made three views of the same thing.
            theme::card(ui, "Connection", |ui| {
                // Route geometry: the node id encodes the city, and a game
                // server's address is geolocated offline, so both ends place.
                let active_node = self
                    .selected_entry()
                    .and_then(|e| e.node_id.clone())
                    .or_else(|| self.status.node_id.clone());
                let mut markers: Vec<globe::Marker> = Vec::new();
                for entry in &self.proxies {
                    let Some(id) = entry.node_id.as_deref() else {
                        continue;
                    };
                    let Some(at) = globe::relay_coords(id) else {
                        continue;
                    };
                    let active = active_node.as_deref() == Some(id);
                    markers.push(globe::Marker {
                        at,
                        colour: if active { theme::ACCENT } else { theme::BORDER },
                        label: entry.label.clone(),
                        emphasis: if active { 1.0 } else { 0.0 },
                    });
                }
                let centre = active_node
                    .as_deref()
                    .and_then(globe::relay_coords)
                    .unwrap_or((30.0, 0.0));

                // The game server comes from whichever mode is running; its
                // address is "ip:port", and only an IPv4 literal geolocates.
                let server_ip = [
                    self.status.redirect_server.as_str(),
                    self.status.windivert_server.as_str(),
                ]
                .iter()
                .find_map(|s| s.split(':').next()?.parse::<std::net::Ipv4Addr>().ok());
                let server_at = server_ip.and_then(geo::locate).map(|(code, at)| {
                    markers.push(globe::Marker {
                        at,
                        colour: theme::WARN,
                        label: code.to_string(),
                        emphasis: 0.8,
                    });
                    at
                });
                let route = server_at.map(|to| (centre, to));

                ui.vertical_centered(|ui| {
                    globe::draw(
                        ui,
                        if narrow { 104.0 } else { 132.0 },
                        centre,
                        &markers,
                        route,
                    );
                });
                ui.add_space(theme::S2);

                ui.horizontal(|ui| {
                    ui.label("Relay");
                    if self.status.connected {
                        ui.monospace(&self.status.proxy_addr);
                    } else {
                        ui.weak("—");
                    }
                });

                if self.status.connected {
                    if let Some(probe) = &self.health {
                        ui.add_space(theme::S2);
                        ui.horizontal(|ui| {
                            ui.label("Relayed").on_hover_text(
                                "Live counters from the selected relay's HTTP /health endpoint.",
                            );
                            match &probe.result {
                                Some(Ok(health)) => {
                                    ui.monospace(format!("{} packets", health.packets_relayed));
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

                if !self.status.rtt_history.is_empty() {
                    ui.add_space(theme::S2);
                    let points: PlotPoints = self
                        .status
                        .rtt_history
                        .iter()
                        .enumerate()
                        .map(|(i, &v)| [i as f64, v])
                        .collect();
                    let line = Line::new("RTT (ms)", points).color(theme::ACCENT);
                    Plot::new("rtt_plot")
                        .height(64.0)
                        .allow_drag(false)
                        .allow_zoom(false)
                        .allow_scroll(false)
                        .show_axes([false, true])
                        .show(ui, |plot_ui| plot_ui.line(line));
                }
            });

            ui.add_space(theme::S3);

            // Configuration scrolls under the status summary so the window
            // can stay small next to a game.
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    self.config_body(ui);
                });

            ui.add_space(theme::S2);

            ui.add_space(theme::S5);
            // Bottom action row.
            ui.horizontal(|ui| {
                if self.tray_available() && ui.small_button("Hide to tray").clicked() {
                    ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                }
                if ui.small_button("Open log file").clicked() {
                    self.reveal_log_file();
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Quit").clicked() {
                        self.quit.store(true, Ordering::SeqCst);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
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
        } else if matches!(self.discovery, DiscoveryState::InFlight(_)) || self.relay_race.is_some()
        {
            Duration::from_millis(200) // keep the spinner and latency race moving
        } else {
            Duration::from_secs(1)
        };
        ctx.request_repaint_after(repaint_interval);
    }
}

// ── Pure helpers (no platform dependency) ─────────────────────────────────────

/// Named palette and shared spacing for the window.
///
/// Before this existed the UI carried 62 colour literals inline at their use
/// sites and re-typed the same `Frame::new().fill(..).corner_radius(4.0)
/// .inner_margin(8.0)` recipe a dozen times, so no two panels were guaranteed
/// to agree on anything. Centralising them is what makes the surfaces below
/// read as one interface instead of a stack of separately-styled rows.
/// Install the bundled fonts.
///
/// Must run BEFORE the first frame, which is why `main.rs` calls it from the
/// eframe creation context rather than from `theme::apply`: `set_fonts` only
/// takes effect on the next frame, and the UI reaches for the named family
/// immediately, so binding them late panics with "FontFamily::Name(..) is not
/// bound to any fonts".
pub(crate) fn install_fonts(ctx: &egui::Context) {
    theme::install_fonts(ctx);
}

/// Design tokens and the single place that styles the application.
///
/// Values mirror the website's `styles.css` custom properties so the app and the
/// site read as one product; rationale is in `client-gui/DESIGN.md`.
mod theme {
    use eframe::egui::{
        self, Color32, CornerRadius, FontId, Frame, Margin, RichText, Stroke, TextStyle, Ui,
    };

    pub const BG: Color32 = Color32::from_rgb(0x0A, 0x0A, 0x1A);
    pub const SURFACE: Color32 = Color32::from_rgb(0x10, 0x10, 0x24);
    pub const SURFACE_RAISED: Color32 = Color32::from_rgb(0x16, 0x16, 0x2E);
    pub const BORDER: Color32 = Color32::from_rgb(0x23, 0x23, 0x3F);

    pub const TEXT: Color32 = Color32::from_rgb(0xF2, 0xF2, 0xFA);
    pub const TEXT_DIM: Color32 = Color32::from_rgb(0x8A, 0x8A, 0xA8);

    pub const ACCENT: Color32 = Color32::from_rgb(0x6C, 0x5C, 0xE7);
    pub const OK: Color32 = Color32::from_rgb(0x00, 0xD6, 0x8F);
    pub const WARN: Color32 = Color32::from_rgb(0xFD, 0xCB, 0x6E);
    pub const BAD: Color32 = Color32::from_rgb(0xFF, 0x6B, 0x6B);

    pub const CAPTION: f32 = 11.0;
    pub const LABEL: f32 = 12.0;
    pub const BODY: f32 = 13.0;
    pub const EMPHASIS: f32 = 15.0;
    pub const HEADING: f32 = 20.0;
    pub const DISPLAY: f32 = 34.0;

    pub const S1: f32 = 4.0;
    pub const S2: f32 = 8.0;
    pub const S3: f32 = 12.0;
    pub const S4: f32 = 16.0;
    pub const S5: f32 = 24.0;

    const R_INLINE: u8 = 6;
    const R_CARD: u8 = 10;
    const R_BUTTON: u8 = 12;

    /// Install the application theme. Called once, on the first frame.
    pub fn apply(ctx: &egui::Context) {
        let mut v = egui::Visuals::dark();
        v.panel_fill = BG;
        v.window_fill = SURFACE;
        v.extreme_bg_color = BG;
        v.faint_bg_color = SURFACE;
        v.code_bg_color = SURFACE;
        v.text_edit_bg_color = Some(BG);
        v.window_stroke = Stroke::new(1.0, BORDER);
        v.window_corner_radius = CornerRadius::same(R_CARD);
        v.menu_corner_radius = CornerRadius::same(R_INLINE);
        v.override_text_color = Some(TEXT);
        v.weak_text_color = Some(TEXT_DIM);
        v.hyperlink_color = ACCENT;
        v.warn_fg_color = WARN;
        v.error_fg_color = BAD;
        v.selection.bg_fill = ACCENT.gamma_multiply(0.35);
        v.selection.stroke = Stroke::new(1.0, TEXT);

        let line = Stroke::new(1.0, BORDER);
        v.widgets.noninteractive.bg_fill = SURFACE;
        v.widgets.noninteractive.weak_bg_fill = SURFACE;
        v.widgets.noninteractive.bg_stroke = line;
        v.widgets.noninteractive.fg_stroke = Stroke::new(1.0, TEXT_DIM);
        v.widgets.noninteractive.corner_radius = CornerRadius::same(R_INLINE);

        v.widgets.inactive.bg_fill = SURFACE_RAISED;
        v.widgets.inactive.weak_bg_fill = SURFACE_RAISED;
        v.widgets.inactive.bg_stroke = line;
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.inactive.corner_radius = CornerRadius::same(R_INLINE);

        v.widgets.hovered.bg_fill = BORDER;
        v.widgets.hovered.weak_bg_fill = BORDER;
        v.widgets.hovered.bg_stroke = Stroke::new(1.0, ACCENT.gamma_multiply(0.6));
        v.widgets.hovered.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.hovered.corner_radius = CornerRadius::same(R_INLINE);

        v.widgets.active.bg_fill = ACCENT;
        v.widgets.active.weak_bg_fill = ACCENT;
        v.widgets.active.bg_stroke = Stroke::new(1.0, ACCENT);
        v.widgets.active.fg_stroke = Stroke::new(1.0, Color32::WHITE);
        v.widgets.active.corner_radius = CornerRadius::same(R_INLINE);

        v.widgets.open.bg_fill = SURFACE_RAISED;
        v.widgets.open.weak_bg_fill = SURFACE_RAISED;
        v.widgets.open.bg_stroke = line;
        v.widgets.open.fg_stroke = Stroke::new(1.0, TEXT);
        v.widgets.open.corner_radius = CornerRadius::same(R_INLINE);

        let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
        style.visuals = v;
        style.spacing.item_spacing = egui::vec2(S2, S2);
        style.spacing.button_padding = egui::vec2(S3, S2);
        style.spacing.window_margin = Margin::same(S4 as i8);
        style.spacing.menu_margin = Margin::same(S2 as i8);
        style.spacing.interact_size.y = 26.0;
        style.spacing.icon_width = 14.0;
        style.text_styles = [
            (TextStyle::Heading, FontId::proportional(HEADING)),
            (TextStyle::Body, FontId::proportional(BODY)),
            (TextStyle::Button, FontId::proportional(BODY)),
            (TextStyle::Small, FontId::proportional(CAPTION)),
            (TextStyle::Monospace, FontId::monospace(LABEL)),
        ]
        .into();
        ctx.set_style_of(egui::Theme::Dark, style);
        ctx.set_theme(egui::Theme::Dark);
    }

    /// The font family for emphasised text, matching the website's font weights.
    pub fn semibold() -> egui::FontFamily {
        egui::FontFamily::Name("semibold".into())
    }

    /// Inter and JetBrains Mono, the same faces the website loads.
    ///
    /// Both are inserted at the FRONT of their family and egui's own fallbacks
    /// are left behind them. The bundled files are latin subsets, and the UI
    /// draws geometric glyphs (the stop square, the disclosure triangle, the
    /// em dash, the ellipsis) that would otherwise come out as tofu.
    pub(super) fn install_fonts(ctx: &egui::Context) {
        use std::sync::Arc;

        let mut fonts = egui::FontDefinitions::default();
        for (name, bytes) in [
            (
                "inter",
                include_bytes!("../assets/fonts/Inter-Regular.ttf") as &[u8],
            ),
            (
                "inter_semibold",
                include_bytes!("../assets/fonts/Inter-SemiBold.ttf"),
            ),
            (
                "jbmono",
                include_bytes!("../assets/fonts/JetBrainsMono-Regular.ttf"),
            ),
        ] {
            fonts.font_data.insert(
                name.to_owned(),
                Arc::new(egui::FontData::from_static(bytes)),
            );
        }

        fonts
            .families
            .entry(egui::FontFamily::Proportional)
            .or_default()
            .insert(0, "inter".to_owned());
        fonts
            .families
            .entry(egui::FontFamily::Monospace)
            .or_default()
            .insert(0, "jbmono".to_owned());
        fonts.families.insert(
            semibold(),
            vec!["inter_semibold".to_owned(), "inter".to_owned()],
        );

        ctx.set_fonts(fonts);
    }

    /// A section: a hairline on a near-background surface, never a bright block.
    pub fn card<R>(ui: &mut Ui, title: &str, body: impl FnOnce(&mut Ui) -> R) -> R {
        Frame::new()
            .fill(SURFACE)
            .stroke(Stroke::new(1.0, BORDER))
            .corner_radius(CornerRadius::same(R_CARD))
            .inner_margin(Margin::same(S3 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    RichText::new(title.to_uppercase())
                        .size(CAPTION)
                        .color(TEXT_DIM)
                        .family(semibold()),
                );
                ui.add_space(S2);
                body(ui)
            })
            .inner
    }

    /// The one bright control on screen.
    pub fn primary_button(text: &str, ready: bool) -> egui::Button<'static> {
        let (fill, ink) = if ready {
            (ACCENT, Color32::WHITE)
        } else {
            (SURFACE_RAISED, TEXT_DIM)
        };
        egui::Button::new(
            RichText::new(text.to_string())
                .size(EMPHASIS)
                .color(ink)
                .family(semibold()),
        )
        .fill(fill)
        .stroke(Stroke::new(1.0, if ready { ACCENT } else { BORDER }))
        .corner_radius(CornerRadius::same(R_BUTTON))
    }
}

fn rtt_colour(rtt_ms: f64) -> egui::Color32 {
    if rtt_ms < 60.0 {
        theme::OK
    } else if rtt_ms < 120.0 {
        theme::WARN
    } else {
        theme::BAD
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

/// Whether discovery should start a latency race instead of connecting to the
/// first relay: only when auto-select is on, disconnected, and relays exist.
pub fn should_race(auto_select: bool, connected: bool, proxy_count: usize) -> bool {
    auto_select && !connected && proxy_count > 0
}

/// Whether a finished latency race may still steer the selection. A relay the
/// user pinned or a connection they started meanwhile wins.
pub fn should_apply_race(auto_select: bool, connected: bool) -> bool {
    auto_select && !connected
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
/// Steam NAT punch / relay etc. - we skip these so the WinDivert filter
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

/// Project identity, links, and the ask to star the repository.
///
/// Lives at the foot of the settings column rather than at the top of the
/// window: the OS title bar already shows the mark and the name, so repeating
/// them above the status banner said nothing the user did not already know.
fn about(ui: &mut egui::Ui, mark: Option<&egui::TextureHandle>) {
    theme::card(ui, "About", |ui| {
        ui.horizontal(|ui| {
            if let Some(mark) = mark {
                ui.add(egui::Image::from_texture(mark).fit_to_exact_size(egui::vec2(28.0, 28.0)));
            }
            ui.vertical(|ui| {
                ui.label(egui::RichText::new("LightSpeed").size(theme::EMPHASIS));
                ui.label(
                    egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .size(theme::CAPTION)
                        .color(theme::TEXT_DIM),
                );
            });
        });

        ui.add_space(theme::S2);
        ui.label(
            egui::RichText::new(
                "Free and open source, with no accounts and no telemetry you cannot turn off.",
            )
            .size(theme::LABEL)
            .color(theme::TEXT_DIM),
        );

        ui.add_space(theme::S3);
        ui.horizontal(|ui| {
            ui.hyperlink_to("GitHub", "https://github.com/ShibbityShwab/lightspeed")
                .on_hover_text("Source code, issues and releases");
            ui.separator();
            ui.hyperlink_to("Website", "https://shibbityshwab.github.io/lightspeed/")
                .on_hover_text("Live relay status, benchmarks and docs");
            ui.separator();
            ui.hyperlink_to(
                "Releases",
                "https://github.com/ShibbityShwab/lightspeed/releases",
            );
        });

        ui.add_space(theme::S3);
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Enjoying it?")
                    .size(theme::LABEL)
                    .color(theme::TEXT_DIM),
            );
            ui.hyperlink_to(
                egui::RichText::new("Star it on GitHub").family(theme::semibold()),
                "https://github.com/ShibbityShwab/lightspeed/stargazers",
            )
            .on_hover_text("Stars help other players find LightSpeed");
        });
    });
}

#[cfg(test)]
mod tests {
    use super::{
        apply_discovery_result, close_decision, games, should_apply_race, should_race,
        CloseDecision,
    };
    use crate::config::ProxyEntry;
    use crate::discovery::{DiscoveryOutcome, RelayInfo};
    use std::net::SocketAddrV4;

    fn addr(s: &str) -> SocketAddrV4 {
        s.parse().expect("test address")
    }

    /// Renders one frame with the app's fonts and theme installed.
    ///
    /// Catches the typography wiring headlessly: an unregistered or misspelled
    /// named family panics inside epaint the moment text uses it, which is
    /// exactly how the bundled Inter crash reached a real user.
    #[test]
    fn the_theme_renders_a_frame_using_the_emphasis_family() {
        let ctx = eframe::egui::Context::default();
        super::install_fonts(&ctx);
        super::theme::apply(&ctx);

        let raw = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(420.0, 800.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.label(eframe::egui::RichText::new("Emphasis").family(super::theme::semibold()));
            ui.label("body");
            ui.monospace("207.246.106.36:4434");
        });
        out.textures_delta.clear();
        assert!(!out.shapes.is_empty(), "a frame with text drew nothing");
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
    fn auto_select_races_only_when_disconnected_with_relays() {
        assert!(should_race(true, false, 2));
        assert!(!should_race(true, true, 2), "already connected");
        assert!(!should_race(true, false, 0), "no relays to race");
        assert!(!should_race(false, false, 2), "user pinned a relay");
    }

    #[test]
    fn a_pinned_relay_or_live_connection_beats_a_late_race_result() {
        assert!(should_apply_race(true, false));
        assert!(!should_apply_race(false, false), "user pinned a relay");
        assert!(!should_apply_race(true, true), "connected meanwhile");
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
        assert_eq!(proxies[0].label, "LAX - Los Angeles");
        assert_eq!(proxies[1].label, "Local proxy");
        assert!(!proxies[1].is_discovered());
    }
}
