//! # LightSpeed GUI - App
//!
//! Main egui application state, UI layout, and pure helper functions.
//! Wraps [`LightSpeedEngine`] in a platform-generic `<P: Platform>` struct
//! so the same GUI code works on Windows (tray-icon) and Linux (stub tray).
//!
//! The window is a **status instrument**: a full-bleed state rail, one fixed
//! action slot, the route card (the one true card), a single scrolling config
//! region, and a pinned footer - the `scroll-body-shell`. Every colour comes
//! from [`crate::design`]; there are no inline colour literals here.

use std::net::SocketAddrV4;
use std::sync::atomic::Ordering;
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{self, GuiConfig, ProxyEntry};
use crate::design::*;
use crate::discovery::{self, DiscoveryOutcome, RelayHealth};
use crate::geo;
use crate::globe;
use crate::i18n;
use crate::paths;
use crate::platform::{Platform, QuitFlag, TrayAction, TrayHandle};
use crate::race_watch::decide_switch;
use crate::update::UpdateStatus;
use eframe::egui;
use lightspeed_client::{EngineStatus, LightSpeedEngine};

// ── Tray state enum ──────────────────────────────────────────────────────────

/// The four states the tray icon can reflect in its color and tooltip.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum TrayState {
    Disconnected,
    Connected,
    Optimizing,
    Error,
}

/// While auto-select is on and the tunnel is up, how often to re-run the relay
/// race so a better - or newly faster - relay can take over.
const RELAY_RECHECK_INTERVAL: Duration = Duration::from_secs(600);

/// Hysteresis for a connected re-race: a winner must beat the relay in use by
/// this many milliseconds before the GUI switches away from it.
const RELAY_SWITCH_MARGIN_MS: u64 = 20;

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
    /// A background thread is downloading, verifying, and handing off the
    /// install; the dialog shows a spinner until the process exits or fails.
    Installing,
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

/// A re-race winner whose latency is being re-measured on a background thread
/// before the GUI decides whether it clears the switch hysteresis.
struct WinnerProbe {
    addr: SocketAddrV4,
    rtt_rx: Receiver<Option<u64>>,
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
    /// When the periodic auto-select re-race last started; cleared while
    /// disconnected or while auto-select is off so the clock restarts cleanly.
    last_race: Option<std::time::Instant>,
    /// A pending timed re-measurement of a connected re-race winner.
    winner_probe: Option<WinnerProbe>,
    show_proxy_manager: bool,
    manager_label_input: String,
    manager_addr_input: String,
    config_error: Option<String>,

    // ── Game routing ──────────────────────────────────────────────────────
    selected_game_idx: usize,
    server_input: String,
    fec_enabled: bool,
    /// Whether the Npcap capture driver is installed, sampled ONCE at startup.
    /// It used to be queried per frame (`sc query npcap` - a blocking process
    /// spawn on the UI thread, and a console-window storm before
    /// `silent_command`), which made opening Settings hang the window.
    capture_available: bool,
    /// Follow the auto-detected game instead of a pinned pick - the Game
    /// field's "Auto (detect)", on by default like the relay's auto-select.
    game_auto: bool,
    /// The Game field's "Manual / Custom…" mode: relay an explicit server.
    game_manual: bool,
    share_latency_stats: bool,
    auto_detected_game: Option<String>,

    // ── System state ──────────────────────────────────────────────────────
    is_admin: bool,
    fonts_setup: bool,
    header_icon: Option<egui::TextureHandle>,

    // ── Sheets and pickers ────────────────────────────────────────────────
    show_settings: bool,

    /// Language pinned in `config.toml`, or `None` to follow the OS locale.
    language: Option<String>,

    // ── Boost diagnostics ─────────────────────────────────────────────────
    boost_start: Option<std::time::Instant>,
    custom_port_input: String,

    // ── Update check ─────────────────────────────────────────────────────
    update_check: UpdateCheckState,
    show_update_dialog: bool,
    update_shared: Arc<Mutex<Option<Result<UpdateStatus, String>>>>,
    install_shared: Arc<Mutex<Option<crate::self_update::InstallOutcome>>>,

    proxies: Vec<ProxyEntry>,
    discovery: DiscoveryState,
    discovery_error: Option<String>,
    health: Option<HealthProbe>,
}

/// Lock `mutex`, recovering a poisoned lock instead of panicking.
///
/// The engine mutex is held across most UI callbacks; one panic while holding
/// it would otherwise make every later frame panic, killing the whole window
/// for a single background failure.
fn lock_or_recover<T>(mutex: &std::sync::Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Time one `/health` round trip to `addr` on a background thread, matching the
/// race's own per-relay measurement, so a connected re-race can compare the
/// winner against the relay in use. Yields `None` when the relay does not
/// answer.
fn spawn_rtt_probe(addr: SocketAddrV4) -> Receiver<Option<u64>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let started = std::time::Instant::now();
        let rtt = discovery::probe_blocking(addr)
            .ok()
            .map(|_| started.elapsed().as_millis() as u64);
        let _ = tx.send(rtt);
    });
    rx
}

impl<P: Platform> LightSpeedApp<P> {
    pub fn new(engine: Arc<Mutex<LightSpeedEngine>>, quit: QuitFlag) -> Self {
        let tray = P::new_tray(Arc::clone(&quit));
        tracing::info!("system tray created: {}", tray.is_some());
        let status = lock_or_recover(&engine).snapshot();

        let auto_detected_game = try_auto_detect_game();

        let is_admin = P::is_admin();

        let saved = config::load(&paths::config_file());

        // The Game field mirrors the relay's: "Auto (detect)" is the default,
        // and a pinned key from the config wins over detection.
        let game_manual = saved.manual_mode;
        let game_auto = saved.game_auto && !game_manual;
        let selected_game_idx = if game_auto {
            auto_detected_game
                .as_deref()
                .and_then(|name| {
                    games()
                        .iter()
                        .position(|entry| entry.key.eq_ignore_ascii_case(name))
                })
                .unwrap_or(0)
        } else {
            saved
                .selected_game
                .as_deref()
                .and_then(|key| games().iter().position(|entry| entry.key == key))
                .unwrap_or(0)
        };
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
        // Pin the language before the first frame; resolving it later would
        // paint one frame of English and then swap the whole window.
        let language = saved.language.clone();
        i18n::set_language(language.as_deref());

        let mut app = Self {
            engine,
            status,
            tray,
            quit,
            selected_proxy_idx,
            auto_select,
            relay_race: None,
            last_race: None,
            winner_probe: None,
            show_proxy_manager: false,
            manager_label_input: String::new(),
            manager_addr_input: String::new(),
            config_error: None,
            selected_game_idx,
            game_auto,
            game_manual,
            server_input: String::new(),
            fec_enabled: false,
            capture_available: P::is_capture_available(),
            share_latency_stats,
            auto_detected_game,
            is_admin,
            fonts_setup: false,
            header_icon: None,
            show_settings: false,
            language,
            boost_start: None,
            custom_port_input: String::new(),
            update_check: UpdateCheckState::Idle,
            show_update_dialog: false,
            update_shared: Arc::new(Mutex::new(None)),
            install_shared: Arc::new(Mutex::new(None)),
            proxies,
            discovery: DiscoveryState::InFlight(discovery::spawn_discovery()),
            discovery_error: None,
            health: None,
        };

        // Reconnect to the persisted relay immediately; discovery may replace
        // the list a moment later without dropping the connection.
        app.engine
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
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
        let mut engine = lock_or_recover(&self.engine);
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
            game_auto: self.game_auto,
            manual_mode: self.game_manual,
            language: self.language.clone(),
            selected_game: if self.game_auto || self.game_manual {
                None
            } else {
                Some(self.selected_game().key.to_string())
            },
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
    ///
    /// A race that finishes while already connected does not switch blindly;
    /// `consider_connected_winner` applies the hysteresis margin instead.
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

        if self.auto_select && self.status.connected {
            self.consider_connected_winner(winner);
            return;
        }

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

    /// A re-race finished while connected: time the winner before letting it
    /// displace the relay in use, so a marginal win cannot flap the tunnel.
    fn consider_connected_winner(&mut self, winner: Option<SocketAddrV4>) {
        let Some(addr) = winner else {
            tracing::warn!("Relay latency race found no reachable relay");
            return;
        };
        if Some(addr) == self.selected_proxy_addr() {
            return; // Already on the fastest relay.
        }
        self.winner_probe = Some(WinnerProbe {
            addr,
            rtt_rx: spawn_rtt_probe(addr),
        });
    }

    /// Collect a finished winner re-measurement and switch only when the winner
    /// clears the hysteresis margin over the current relay's measured RTT.
    fn poll_winner_probe(&mut self) {
        let rtt = match self.winner_probe.as_ref() {
            Some(probe) => match probe.rtt_rx.try_recv() {
                Ok(rtt) => rtt,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => None,
            },
            None => return,
        };
        let Some(WinnerProbe { addr, .. }) = self.winner_probe.take() else {
            return;
        };

        // The user may have pinned a relay or the tunnel may have dropped while
        // the probe ran; either way the race may no longer steer the pick.
        if !self.auto_select || !self.status.connected {
            return;
        }
        let Some(winner_rtt_ms) = rtt else {
            tracing::warn!("Re-race winner {addr} did not answer the follow-up probe");
            return;
        };
        let current_rtt_ms = if self.status.latest_rtt_ms > 0.0 {
            self.status.latest_rtt_ms.round() as u64
        } else {
            0
        };
        let switch = current_rtt_ms == 0
            || decide_switch(current_rtt_ms, winner_rtt_ms, RELAY_SWITCH_MARGIN_MS);
        if !switch {
            tracing::info!(
                "Keeping the current relay: re-race winner {addr} at {winner_rtt_ms} ms does \
                 not beat {current_rtt_ms} ms by {RELAY_SWITCH_MARGIN_MS} ms"
            );
            return;
        }
        if let Some(idx) = self.proxies.iter().position(|entry| entry.addr == addr) {
            tracing::info!("Auto-switched to the faster relay {addr} ({winner_rtt_ms} ms)");
            self.selected_proxy_idx = idx;
            self.connect_selected();
            self.persist_config();
        }
    }

    /// While auto-select is on and the tunnel is up, re-run the relay race
    /// roughly every `RELAY_RECHECK_INTERVAL`, so a better relay can take over.
    fn maybe_rerace(&mut self) {
        if !self.auto_select || !self.status.connected {
            // Restart the clock the next time we are connected under auto-select.
            self.last_race = None;
            return;
        }
        if self.relay_race.is_some() || self.winner_probe.is_some() {
            return;
        }
        match self.last_race {
            Some(at) if at.elapsed() >= RELAY_RECHECK_INTERVAL => {
                self.last_race = Some(std::time::Instant::now());
                self.start_relay_race();
            }
            Some(_) => {}
            None => self.last_race = Some(std::time::Instant::now()),
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
        let mut engine = lock_or_recover(&self.engine);
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
        *lock_or_recover(&self.update_shared) = None;
        self.update_check = UpdateCheckState::Checking;
        self.show_update_dialog = true;
        let shared = Arc::clone(&self.update_shared);
        std::thread::spawn(move || {
            let result = crate::update::check_for_update_blocking();
            *lock_or_recover(&shared) = Some(result);
        });
    }

    /// Start the interceptor boost for the selected game and relay.
    fn start_boost(&mut self) {
        // Warm up port detection for diagnostic logging.
        let _ = parse_custom_port_range(&self.custom_port_input)
            .unwrap_or_else(|| P::detect_game_ports(self.selected_game_idx));

        if let Some(proxy) = self.selected_proxy_addr() {
            let game_key = self.selected_game().key;
            let mut engine = lock_or_recover(&self.engine);
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
        let mut engine = lock_or_recover(&self.engine);
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
            lock_or_recover(&self.engine).disconnect();
        } else if self.selected_entry().is_some() {
            self.connect_selected();
        }
    }

    // ── Rail / route readouts ──────────────────────────────────────────────

    /// The full-bleed state rail: the state word, the live round trip, and the
    /// one-line description of what is happening.
    fn state_rail(&self, ui: &mut egui::Ui, boosting: bool, narrow: bool) {
        let (word, word_color) = if boosting {
            ("OPTIMIZING", signal)
        } else if self.status.connected {
            ("CONNECTED", text_1)
        } else {
            ("NOT OPTIMIZING", text_1)
        };

        let sub_line = if boosting {
            let relay = self
                .selected_entry()
                .and_then(|e| e.node_id.as_deref())
                .map(discovery::friendly_label)
                .unwrap_or_else(|| self.status.proxy_addr.clone());
            format!("{}  \u{2192}  {}", self.selected_game().display, relay)
        } else if self.status.connected {
            match &self.auto_detected_game {
                Some(name) => i18n::t_with("route.relay_ready_named", &[("name", name)]),
                None => i18n::t("route.relay_ready_pick").into_owned(),
            }
        } else {
            i18n::t("route.no_relay").into_owned()
        };

        // The live round trip. On a narrow window it moves to its own row:
        // sharing the headline's row made the value overlap the state word at
        // 320px, and the state word must never be occluded (skill: state
        // legibility first).
        let metric = if self.status.connected {
            let text = if self.status.latest_rtt_ms > 0.0 {
                format!("{:>3.0} ms", self.status.latest_rtt_ms)
            } else {
                "-- ms".to_string()
            };
            let color = if self.status.latest_rtt_ms > 0.0 {
                rtt_colour(self.status.latest_rtt_ms)
            } else {
                text_3
            };
            Some((text, color))
        } else {
            None
        };

        let hero_h = 34.0;
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), hero_h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_height(hero_h);
                ui.label(
                    egui::RichText::new(word)
                        .size(STATE)
                        .family(semibold())
                        .color(word_color),
                );
                if let Some((text, color)) = &metric {
                    if !narrow {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(text.as_str())
                                    .size(METRIC)
                                    .family(egui::FontFamily::Monospace)
                                    .color(*color),
                            );
                        });
                    }
                }
            },
        );
        if let Some((text, color)) = &metric {
            if narrow {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.label(
                        egui::RichText::new(text.as_str())
                            .size(METRIC)
                            .family(egui::FontFamily::Monospace)
                            .color(*color),
                    );
                });
            }
        }
        ui.add_space(S1);
        ui.label(egui::RichText::new(sub_line).size(BODY).color(text_2));
    }

    /// The one true card: the route globe and the label/value ledger. No
    /// heading - it is identified by its position, the only rounded card in
    /// the window.
    fn route_card(&self, ui: &mut egui::Ui, narrow: bool) {
        // The emphasised relay marker is only ever live while connected; with
        // the tunnel down the globe stays at rest and nothing is highlighted.
        let active_node = if self.status.connected {
            self.selected_entry()
                .and_then(|e| e.node_id.clone())
                .or_else(|| self.status.node_id.clone())
        } else {
            None
        };
        let mut markers: Vec<globe::Marker> = Vec::new();
        if self.status.connected {
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
                    colour: if active { accent } else { border_1 },
                    label: entry.label.clone(),
                    emphasis: if active { 1.0 } else { 0.0 },
                });
            }
        }
        let centre = active_node
            .as_deref()
            .and_then(globe::relay_coords)
            .unwrap_or((30.0, 0.0));

        // The game server comes from whichever mode is running; its address is
        // "ip:port", and only an IPv4 literal geolocates.
        let server_ip = [
            self.status.interceptor_server.as_str(),
            self.status.redirect_server.as_str(),
            self.status.windivert_server.as_str(),
        ]
        .into_iter()
        .find_map(|s| s.split(':').next()?.parse::<std::net::Ipv4Addr>().ok());
        let server_at = server_ip.and_then(geo::locate).map(|(code, at)| {
            markers.push(globe::Marker {
                at,
                colour: warn,
                label: code.to_string(),
                emphasis: 0.8,
            });
            at
        });
        let route = server_at.map(|to| (centre, to));

        egui::Frame::new()
            .fill(bg_2)
            .stroke(egui::Stroke::new(1.0, border_1))
            .corner_radius(egui::CornerRadius::same(R_CARD))
            .inner_margin(egui::Margin::same(S3 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    globe::draw(
                        ui,
                        if narrow { GLOBE_SM } else { GLOBE_LG },
                        centre,
                        &markers,
                        route,
                    );
                });
                ui.add_space(S2);
                self.ledger_rows(ui, narrow, false);
                if self.status.connected && !self.status.rtt_history.is_empty() {
                    ui.add_space(S2);
                    sparkline(ui, &self.status.rtt_history);
                }
            });
    }

    /// The label/value ledger. At the short-height breakpoint the `Game server`
    /// row is dropped and the values move into the scrolling region.
    fn ledger_rows(&self, ui: &mut egui::Ui, narrow: bool, short_mode: bool) {
        let relay = self.relay_value();
        self.ledger_row(ui, narrow, "Relay", &relay);
        if !short_mode {
            let game_server = self.game_server_value();
            self.ledger_row(ui, narrow, "Game server", &game_server);
        }
        let routed = self.routed_value();
        self.ledger_row(ui, narrow, "Relayed", &routed);
    }

    fn ledger_row(&self, ui: &mut egui::Ui, narrow: bool, label: &str, value: &LedgerValue) {
        if narrow {
            ui.label(egui::RichText::new(label).size(CAPTION).color(text_3));
            self.ledger_value(ui, value);
        } else {
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), ROW_H),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ledger_label(ui, LEDGER_LABEL_W, label);
                    self.ledger_value(ui, value);
                },
            );
        }
    }

    fn ledger_value(&self, ui: &mut egui::Ui, value: &LedgerValue) {
        match value {
            LedgerValue::Data(text) => {
                ui.monospace(text.as_str());
            }
            LedgerValue::Hint(text) => {
                ui.label(egui::RichText::new(*text).size(BODY).color(text_3));
            }
        }
    }

    /// `Relay` = the human-readable label the app actually has, or the raw
    /// proxy address as a last resort. Never a fabricated city.
    fn relay_value(&self) -> LedgerValue {
        if !self.status.connected {
            return LedgerValue::Hint("--");
        }
        let node_id = self
            .selected_entry()
            .and_then(|e| e.node_id.as_deref())
            .or(self.status.node_id.as_deref());
        if let Some(id) = node_id {
            LedgerValue::Data(discovery::friendly_label(id))
        } else if self.status.proxy_addr.is_empty() {
            LedgerValue::Hint("--")
        } else {
            LedgerValue::Data(self.status.proxy_addr.clone())
        }
    }

    fn game_server_value(&self) -> LedgerValue {
        let server = [
            self.status.interceptor_server.as_str(),
            self.status.redirect_server.as_str(),
            self.status.windivert_server.as_str(),
        ]
        .into_iter()
        .find(|s| !s.is_empty());
        match server {
            Some(s) => LedgerValue::Data(s.to_string()),
            None => LedgerValue::Hint("-- start optimizing to place it"),
        }
    }

    /// Packets THIS session has pushed through the relay. The relay's own
    /// `/health` counters are lifetime totals for the whole fleet node, not our
    /// session, so they must never be shown as if they were ours.
    fn routed_value(&self) -> LedgerValue {
        if self.status.redirect_active {
            LedgerValue::Data(format!(
                "{} pkts  this session",
                group_digits(self.status.redirect_pkts_out)
            ))
        } else {
            LedgerValue::Hint("-- start optimizing to count")
        }
    }

    // ── Config region (the only scrolling part) ────────────────────────────

    /// Configuration section: relay, game, and the optional game picker, in the
    /// one scroll region. At the short-height breakpoint the route values fold
    /// in at the top.
    fn config_body(&mut self, ui: &mut egui::Ui, narrow: bool, short: bool) {
        if short {
            section_label(ui, &i18n::t("route.title"));
            self.ledger_rows(ui, narrow, true);
            ui.add_space(S3);
        }

        // Boost server: which relay carries the game traffic.
        field_row(ui, narrow, "Relay", |ui| {
            ui.horizontal(|ui| {
                let disconnect_w = if self.status.connected { 78.0 } else { 0.0 };
                let manage_w = 64.0;
                let combo_w =
                    (ui.available_width() - manage_w - disconnect_w - S2 * 2.0).max(100.0);
                ui.scope(|ui| {
                    ui.set_max_width(combo_w);
                    self.boost_server_combo(ui);
                });
                if ui
                    .link("Manage")
                    .on_hover_text(i18n::t("proxies.manage_hint"))
                    .clicked()
                {
                    self.show_proxy_manager = true;
                }
                if self.status.connected
                    && ui
                        .link("Disconnect")
                        .on_hover_text(i18n::t("proxies.disconnect_hint"))
                        .clicked()
                {
                    self.toggle_relay_connection();
                }
            });
        });

        if !self.proxies.is_empty() {
            if let Some(err) = self.discovery_error.clone() {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new(i18n::t("relay.refresh_failed"))
                            .size(CAPTION)
                            .color(warn),
                    )
                    .on_hover_text(err);
                    if ui.link(i18n::t("relay.retry")).clicked() {
                        self.start_discovery();
                    }
                });
            }
        }
        ui.add_space(S3);

        // Game: what is being boosted, plus the reliability shield.
        field_row(ui, narrow, "Game", |ui| {
            self.game_combo(ui);
        });
        if let Some(ref detected) = self.auto_detected_game {
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    egui::RichText::new(i18n::t("game.found"))
                        .size(BODY)
                        .color(signal),
                );
                ui.label(
                    egui::RichText::new(detected.clone())
                        .size(BODY)
                        .color(text_1),
                );
            });
        } else if !self.game_manual {
            let caption = if self.game_auto {
                "No game detected - Auto will follow one when you start it"
            } else {
                "No game detected"
            };
            ui.label(egui::RichText::new(caption).size(BODY).color(text_2));
        }
        if self.game_manual {
            self.manual_section(ui);
        }
    }

    /// The Boost Server selector: auto-select or a pinned relay, one dropdown.
    fn boost_server_combo(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        if self.proxies.is_empty() {
            match &self.discovery {
                DiscoveryState::InFlight(_) => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label(egui::RichText::new(i18n::t("relay.discovering")).color(text_2));
                    });
                }
                DiscoveryState::Done => {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(egui::RichText::new(i18n::t("relay.none_discovered")).color(warn))
                            .on_hover_text(self.discovery_error.clone().unwrap_or_else(|| {
                                "The relay registry returned no usable relays.".to_string()
                            }));
                        if ui.link(i18n::t("relay.retry")).clicked() {
                            self.start_discovery();
                        }
                    });
                }
            }
            return;
        }

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
            .width(width)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(auto, i18n::t("relay.auto_fastest"))
                    .clicked()
                {
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

    /// The one-step game selector.
    fn game_combo(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let label = if self.game_manual {
            i18n::t("game.manual_server").into_owned()
        } else if self.game_auto {
            match &self.auto_detected_game {
                Some(name) => i18n::t_with("game.auto_named", &[("name", name)]),
                None => i18n::t("game.auto_detect").into_owned(),
            }
        } else {
            self.selected_game().display.to_string()
        };
        let mut changed = false;
        egui::ComboBox::from_id_salt("game_select")
            .selected_text(label)
            .width(width)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(self.game_auto, i18n::t("game.auto_detect"))
                    .clicked()
                {
                    self.game_auto = true;
                    self.game_manual = false;
                    changed = true;
                }
                ui.separator();
                if ui
                    .selectable_label(self.game_manual, i18n::t("game.manual_custom"))
                    .clicked()
                {
                    self.game_manual = true;
                    self.game_auto = false;
                    changed = true;
                }
                ui.separator();
                for (i, entry) in games().iter().enumerate() {
                    if ui
                        .selectable_label(
                            !self.game_auto && !self.game_manual && i == self.selected_game_idx,
                            entry.display,
                        )
                        .clicked()
                    {
                        self.selected_game_idx = i;
                        self.game_auto = false;
                        self.game_manual = false;
                        changed = true;
                    }
                }
            });
        if changed {
            self.persist_config();
        }
    }

    /// The "Manual / Custom…" flow: relay an explicit server straight from the
    /// Game field, for titles the auto-detect does not know.
    fn manual_section(&mut self, ui: &mut egui::Ui) {
        ui.add_space(S2);
        ui.label(
            egui::RichText::new(
                "Enter your game server's IP and port to start optimizing without \
                 waiting for auto-detect.",
            )
            .size(BODY)
            .color(text_2),
        );
        ui.add_space(S2);
        ui.horizontal(|ui| {
            ui.label(i18n::t("route.server_label"));
            let default_port = self.selected_game().default_port;
            ui.add(
                egui::TextEdit::singleline(&mut self.server_input)
                    .hint_text(format!("e.g. 123.45.67.89:{default_port}"))
                    .desired_width(220.0),
            );
        });
        ui.add_space(S2);
        ui.horizontal(|ui| {
            ui.label(i18n::t("route.custom_port_range")).on_hover_ui(|ui| {
                ui.label(
                    "Override the default port scan range for auto-detect. \
                     Use this if Packets Sent stays at 0 after 15 s.\n\
                     Format: lo-hi  (e.g. 28015-28999)  or a single port.",
                );
                ui.hyperlink_to(
                    "Port not detected - fix guide",
                    "https://github.com/ShibbityShwab/lightspeed/wiki/Troubleshooting#port-not-detected",
                );
            });
            let port_valid = self.custom_port_input.is_empty()
                || parse_custom_port_range(&self.custom_port_input).is_some();
            let valid_color = if port_valid { text_1 } else { danger };
            ui.add(
                egui::TextEdit::singleline(&mut self.custom_port_input)
                    .hint_text(i18n::t("route.port_hint"))
                    .desired_width(200.0)
                    .text_color(valid_color),
            );
            if !port_valid {
                ui.label(egui::RichText::new(i18n::t("proxies.invalid")).color(danger));
            }
        });
        ui.add_space(S2);

        let server_valid = parse_server_addr(&self.server_input).is_some();
        let manual_button = egui::Button::new("Start optimizing (manual)").fill(if server_valid {
            signal_soft
        } else {
            bg_3
        });
        if ui.add_enabled(server_valid, manual_button).clicked() {
            if let Some(server_addr) = parse_server_addr(&self.server_input) {
                let entry = self.selected_game();
                let local_port = server_addr.port().max(entry.default_port);
                if let Some(proxy) = self.selected_proxy_addr() {
                    lock_or_recover(&self.engine).start_redirect(
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
            ui.label(egui::RichText::new(i18n::t("proxies.addr_invalid_hint")).color(warn));
        }

        ui.add_space(S2);
        let instruction = connect_instruction(
            self.selected_game(),
            self.server_input
                .parse::<SocketAddrV4>()
                .map(|a| a.port())
                .unwrap_or(self.selected_game().default_port),
        );
        ui.label(egui::RichText::new(instruction).size(BODY).color(text_2));
        ui.add_space(S2);
    }

    // ── In-window sheets ────────────────────────────────────────────────────

    /// `Settings`: Privacy, Maintenance, Advanced, and About, in the same sheet
    /// pattern as the Proxy Manager.
    fn settings_sheet(&mut self, ctx: &egui::Context) {
        let (max_w, max_h) = sheet_bounds(ctx);
        let mut close = false;
        let response = egui::Modal::new(egui::Id::new("settings_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_max_width(max_w);
                sheet_title(ui, &i18n::t("settings.title"));
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .show(ui, |ui| {
                        self.settings_body(ui);
                    });
                ui.add_space(S2);
                if ui.button("Close").clicked() {
                    close = true;
                }
            });
        if response.should_close() || close {
            self.show_settings = false;
        }
    }

    fn settings_body(&mut self, ui: &mut egui::Ui) {
        // Language first: it changes the labels of everything below it.
        section_label(ui, &i18n::t("settings.language"));
        let selected = i18n::current();
        let label: String = i18n::LOCALES
            .iter()
            .find(|locale| locale.tag == selected)
            .map(|locale| locale.native_name.to_string())
            .unwrap_or_else(|| i18n::t("settings.follow_system").into_owned());
        egui::ComboBox::from_id_salt("language_select")
            .selected_text(egui::RichText::new(label).size(LABEL))
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(self.language.is_none(), i18n::t("settings.follow_system"))
                    .clicked()
                {
                    self.language = None;
                    i18n::set_language(None);
                    install_fonts(ui.ctx(), Some(&i18n::current()));
                    self.persist_config();
                }
                for locale in i18n::LOCALES {
                    let picked = self.language.as_deref() == Some(locale.tag);
                    if ui.selectable_label(picked, locale.native_name).clicked() {
                        self.language = Some(locale.tag.to_string());
                        i18n::set_language(Some(locale.tag));
                        install_fonts(ui.ctx(), Some(&i18n::current()));
                        self.persist_config();
                    }
                }
            });
        ui.add_space(S3);

        // Privacy: anonymous latency telemetry.
        section_label(ui, &i18n::t("settings.privacy"));
        let changed = ui
            .checkbox(
                &mut self.share_latency_stats,
                egui::RichText::new(i18n::t("settings.share_stats"))
                    .size(LABEL)
                    .family(semibold()),
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
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .set_telemetry_enabled(self.share_latency_stats);
        }
        ui.add_space(S3);

        // Maintenance: self-update, the trace log, and the relay link.
        hairline(ui);
        ui.add_space(S2);
        section_label(ui, &i18n::t("settings.maintenance"));
        if ui
            .button("Check for updates")
            .on_hover_text(i18n::t("settings.check_updates_hint"))
            .clicked()
        {
            self.start_update_check();
        }
        ui.add_space(S1);
        if ui.button(i18n::t("settings.open_log")).clicked() {
            self.reveal_log_file();
        }
        ui.add_space(S1);
        ui.label(
            egui::RichText::new(format!(
                "Capture backend (pcap mode): {}",
                if self.capture_available {
                    "available"
                } else {
                    "not detected"
                }
            ))
            .size(CAPTION)
            .color(text_3),
        );
        ui.add_space(S3);

        ui.add_space(S3);

        // About: the brand mark and the project links.
        hairline(ui);
        ui.add_space(S2);
        section_label(ui, &i18n::t("settings.about"));
        ui.horizontal(|ui| {
            if let Some(mark) = self.header_icon.as_ref() {
                ui.add(egui::Image::from_texture(mark).fit_to_exact_size(egui::vec2(28.0, 28.0)));
            }
            ui.vertical(|ui| {
                ui.label(
                    egui::RichText::new("LightSpeed")
                        .size(EMPHASIS)
                        .family(semibold()),
                );
                ui.label(
                    egui::RichText::new(format!("Version {}", env!("CARGO_PKG_VERSION")))
                        .size(CAPTION)
                        .color(text_3),
                );
            });
        });
        ui.add_space(S2);
        ui.label(
            egui::RichText::new(
                "Free and open source, with no accounts and no telemetry you cannot turn off.",
            )
            .size(LABEL)
            .color(text_2),
        );
        ui.add_space(S2);
        ui.horizontal_wrapped(|ui| {
            ui.hyperlink_to("GitHub", "https://github.com/ShibbityShwab/lightspeed")
                .on_hover_text(i18n::t("about.source_hint"));
            ui.hyperlink_to("Website", "https://shibbityshwab.github.io/lightspeed/")
                .on_hover_text(i18n::t("about.website_hint"));
            ui.hyperlink_to(
                "Releases",
                "https://github.com/ShibbityShwab/lightspeed/releases",
            );
        });
        ui.add_space(S2);
        ui.horizontal_wrapped(|ui| {
            ui.label(
                egui::RichText::new(i18n::t("about.enjoying"))
                    .size(LABEL)
                    .color(text_2),
            );
            ui.hyperlink_to(
                egui::RichText::new(i18n::t("about.star_github")).family(semibold()),
                "https://github.com/ShibbityShwab/lightspeed/stargazers",
            )
            .on_hover_text(i18n::t("about.star_hint"));
        });
    }

    /// `Proxy Manager`: the discovered and custom relay list, plus add/remove.
    fn proxy_manager_sheet(&mut self, ctx: &egui::Context) {
        let (max_w, max_h) = sheet_bounds(ctx);
        let mut remove_idx: Option<usize> = None;
        let mut add_addr: Option<SocketAddrV4> = None;
        let mut reset = false;

        let response = egui::Modal::new(egui::Id::new("proxy_manager_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_max_width(max_w);
                sheet_title(ui, &i18n::t("proxies.title"));
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .show(ui, |ui| {
                        for (i, entry) in self.proxies.iter().enumerate() {
                            ui.horizontal_wrapped(|ui| {
                                ui.label(egui::RichText::new(&entry.label).color(text_1));
                                ui.monospace(entry.addr.to_string());
                                ui.label(
                                    egui::RichText::new(
                                        entry.node_id.as_deref().unwrap_or("custom"),
                                    )
                                    .size(CAPTION)
                                    .color(text_3),
                                );
                                if ui.button("×").clicked() {
                                    remove_idx = Some(i);
                                }
                            });
                        }
                        if self.proxies.is_empty() {
                            ui.label(
                                egui::RichText::new(
                                    "No relays yet. Discovery runs automatically; you can \
                                     also add a custom proxy below.",
                                )
                                .size(BODY)
                                .color(text_2),
                            );
                        }

                        ui.add_space(S2);
                        ui.horizontal(|ui| {
                            ui.label(i18n::t("proxies.label_field"));
                            ui.text_edit_singleline(&mut self.manager_label_input);
                        });
                        ui.horizontal(|ui| {
                            ui.label(i18n::t("proxies.addr_field"));
                            ui.text_edit_singleline(&mut self.manager_addr_input);
                        });

                        ui.horizontal_wrapped(|ui| {
                            if ui.button(i18n::t("proxies.add")).clicked()
                                && self.manager_addr_input.parse::<SocketAddrV4>().is_ok()
                            {
                                add_addr = Some(self.manager_addr_input.parse().unwrap());
                            }
                            if ui.button(i18n::t("proxies.refresh")).clicked() {
                                self.start_discovery();
                            }
                        });

                        ui.add_space(S2);
                        ui.horizontal_wrapped(|ui| {
                            if ui.button(i18n::t("proxies.open_config")).clicked() {
                                paths::open_in_os(&paths::config_dir());
                            }
                            if ui.button(i18n::t("proxies.reset_defaults")).clicked() {
                                reset = true;
                            }
                        });
                        if let Some(err) = &self.config_error {
                            ui.label(egui::RichText::new(err).color(danger));
                        }
                    });
                ui.add_space(S2);
                if ui.button("Close").clicked() {
                    self.show_proxy_manager = false;
                }
            });

        if response.should_close() {
            self.show_proxy_manager = false;
        }

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

    /// The self-update check result, install handoff, and any failure.
    fn update_sheet(&mut self, ctx: &egui::Context) {
        let (max_w, max_h) = sheet_bounds(ctx);
        let response = egui::Modal::new(egui::Id::new("update_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_max_width(max_w);
                sheet_title(ui, &i18n::t("update.sheet_title"));
                egui::ScrollArea::vertical()
                    .max_height(max_h)
                    .show(ui, |ui| {
                        match &self.update_check {
                            UpdateCheckState::Checking => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(i18n::t("update.checking"));
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
                                ui.label(i18n::t_with("update.current", &[("version", current.as_str())]));
                                ui.label(i18n::t_with("update.latest", &[("version", latest.as_str())]));
                                ui.add_space(S1);
                                ui.label(crate::update::update_status_line(result));

                                let installable = matches!(result, Ok(status) if status.update_available && status.installer.is_some());
                                if installable {
                                    ui.add_space(S1);
                                    if ui
                                        .button("Install update")
                                        .on_hover_text(
                                            "Download, verify, and install the latest version",
                                        )
                                        .clicked()
                                    {
                                        if let Ok(status) = result {
                                            if let Some(asset) = status.installer.clone() {
                                                *lock_or_recover(&self.install_shared) = None;
                                                self.update_check = UpdateCheckState::Installing;
                                                let shared = Arc::clone(&self.install_shared);
                                                std::thread::spawn(move || {
                                                    let outcome = crate::self_update::install_blocking(
                                                        asset,
                                                    );
                                                    *lock_or_recover(&shared) = Some(outcome);
                                                });
                                            }
                                        }
                                    }
                                }
                            }
                            UpdateCheckState::Installing => {
                                ui.horizontal(|ui| {
                                    ui.spinner();
                                    ui.label(i18n::t("update.installing"));
                                });
                                ui.label(
                                    "Approve the UAC prompt if one appears; LightSpeed will restart.",
                                );
                            }
                            UpdateCheckState::Idle => {}
                        }
                    });
                ui.add_space(S2);
                if ui.button("Close").clicked() {
                    self.show_update_dialog = false;
                    self.update_check = UpdateCheckState::Idle;
                }
            });

        if response.should_close() {
            self.show_update_dialog = false;
            self.update_check = UpdateCheckState::Idle;
        }
    }
}

// ── eframe::App impl ─────────────────────────────────────────────────────────

impl<P: Platform> eframe::App for LightSpeedApp<P> {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The full window rect, read before any panel claims space, drives the
        // narrow (width) and short (height) breakpoints.
        let viewport = ui.available_rect_before_wrap();
        let narrow = viewport.width() < 380.0;
        let short = viewport.height() < 560.0;

        // One-time first-frame setup: platform-specific fonts and the
        // branded header mark texture.
        if !self.fonts_setup {
            self.fonts_setup = true;
            apply_theme(&ctx);
            P::setup_fonts(&ctx);
            // `main.rs` binds the families before the first frame, when the
            // language is not resolved yet; re-bind now that it is, so the CJK
            // faces are ordered for the active locale rather than defaulted.
            install_fonts(&ctx, Some(&i18n::current()));
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
                    lock_or_recover(&self.engine).disconnect();
                }
            }
        }

        // Refresh engine snapshot and background relay state.
        self.status = lock_or_recover(&self.engine).snapshot();
        self.poll_discovery();
        self.poll_relay_race();
        self.poll_winner_probe();
        self.maybe_rerace();
        self.poll_health();

        // Collect a finished update check from the background thread.
        if matches!(self.update_check, UpdateCheckState::Checking) {
            if let Some(result) = lock_or_recover(&self.update_shared).take() {
                self.update_check = UpdateCheckState::Done(result);
                self.show_update_dialog = true;
            }
        }

        // Collect a finished install attempt. Success means the elevated
        // installer has taken over and this process is about to be killed;
        // failure returns to the dialog so the user can retry.
        if matches!(self.update_check, UpdateCheckState::Installing) {
            if let Some(Err(err)) = lock_or_recover(&self.install_shared).take() {
                self.update_check = UpdateCheckState::Done(Err(format!("install failed: {err}")));
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

        // ── Status window shell ──────────────────────────────────────────
        // The "scroll-body-shell": rail, action bar, and (at normal height)
        // route card are fixed; the config region is the only scrolling part;
        // the footer stays pinned. At a short height the route card folds into
        // the scroll region so the fixed chrome never crowds out the footer.
        let boosting = self.status.interceptor_active
            || self.status.windivert_active
            || self.status.capture_active
            || self.status.redirect_active;

        let rail_edge = if boosting {
            signal
        } else if self.status.connected {
            accent_soft
        } else {
            text_3
        };

        // Rail: full-bleed, no radius, no side margin.
        let rail = egui::Panel::top("state_rail")
            .frame(egui::Frame::new().fill(bg_2).inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: S3 as i8,
                bottom: S3 as i8,
            }))
            .show(ui, |ui| {
                self.state_rail(ui, boosting, narrow);
            });
        // The 3 px left edge is painted after the frame, so it is never
        // rounded or inset; `signal` only when actually boosting.
        ui.painter().vline(
            rail.response.rect.left() + 1.5,
            rail.response.rect.y_range(),
            egui::Stroke::new(3.0, rail_edge),
        );

        // Action bar: one slot, three gate-driven labels.
        let has_relay = self.selected_entry().is_some();
        let (action_label, action_ready) =
            primary_action(self.is_admin, boosting, self.status.connected, has_relay);
        let action_kind = if boosting {
            ActionKind::Danger
        } else {
            ActionKind::Accent
        };
        let action = egui::Panel::top("action_bar")
            .frame(egui::Frame::new().fill(bg_1).inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: S2 as i8,
                bottom: S2 as i8,
            }))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                action_button(ui, action_label, action_kind, action_ready)
            });
        if action.inner.clicked() && action_ready {
            if boosting {
                self.stop_boost();
            } else if !self.is_admin {
                self.restart_elevated();
            } else {
                self.start_boost();
            }
        }

        // Route card: fixed at normal height.
        if !short {
            egui::Panel::top("route_card")
                .frame(egui::Frame::new().fill(bg_1).inner_margin(egui::Margin {
                    left: S4 as i8,
                    right: S4 as i8,
                    top: S2 as i8,
                    bottom: S2 as i8,
                }))
                .show(ui, |ui| {
                    self.route_card(ui, narrow);
                });
        }

        // Footer: pinned, with a hairline above it.
        egui::Panel::bottom("footer")
            .frame(egui::Frame::new().fill(bg_1).inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: 0,
                bottom: S2 as i8,
            }))
            .show(ui, |ui| {
                let xr = ui.max_rect().x_range();
                ui.painter().rect_filled(
                    egui::Rect::from_min_max(
                        egui::pos2(xr.min, ui.max_rect().top()),
                        egui::pos2(xr.max, ui.max_rect().top() + 1.0),
                    ),
                    0.0,
                    border_1,
                );
                ui.add_space(S2);
                ui.horizontal_wrapped(|ui| {
                    if self.tray_available() && ui.button(i18n::t("window.hide_to_tray")).clicked()
                    {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                    if ui.button(i18n::t("settings.title")).clicked() {
                        self.show_settings = true;
                    }
                    if ui.button(i18n::t("action.quit")).clicked() {
                        self.quit.store(true, Ordering::SeqCst);
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                });
            });

        // Config: the only scrolling region.
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg_1).inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: S3 as i8,
                bottom: S3 as i8,
            }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        self.config_body(ui, narrow, short);
                    });
            });

        // ── In-window sheets ─────────────────────────────────────────────
        if self.show_settings {
            self.settings_sheet(&ctx);
        }
        if self.show_proxy_manager {
            self.proxy_manager_sheet(&ctx);
        }
        if self.show_update_dialog {
            self.update_sheet(&ctx);
        }

        // ── Repaint schedule ─────────────────────────────────────────────
        let repaint_interval = if self.status.redirect_active
            || self.status.capture_active
            || self.status.windivert_active
            || self.status.interceptor_active
        {
            Duration::from_millis(500) // 2 Hz for live counters
        } else if matches!(self.discovery, DiscoveryState::InFlight(_))
            || self.relay_race.is_some()
            || self.winner_probe.is_some()
        {
            Duration::from_millis(200) // keep the spinner and latency race moving
        } else {
            Duration::from_secs(1)
        };
        ctx.request_repaint_after(repaint_interval);
    }
}

// ── Pure helpers (no platform dependency) ─────────────────────────────────────

/// Build the font families the UI draws with, for `locale`.
///
/// The bundled Inter is a 230-codepoint subset - no Cyrillic, no Greek, no CJK -
/// so every family needs real fallbacks. `semibold` used to chain only
/// `["inter_semibold", "inter"]`, which made every emphasised Russian label draw
/// as tofu while the regular text beside it rendered fine through epaint's
/// built-in Ubuntu-Light.
///
/// `locale` chooses only the ORDER of the three CJK faces. Noto's Japanese,
/// Korean and Simplified-Chinese subsets each carry the shared Han repertoire in
/// their own regional forms - 127 of the Chinese catalog's 172 Han characters
/// also exist in the Japanese face - so a fixed order would draw Chinese text
/// with Japanese glyphs.
pub(crate) fn font_definitions(locale: Option<&str>) -> egui::FontDefinitions {
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
        (
            "cjk_jp",
            include_bytes!("../assets/fonts/NotoSansJP-Regular.otf"),
        ),
        (
            "cjk_kr",
            include_bytes!("../assets/fonts/NotoSansKR-Regular.otf"),
        ),
        (
            "cjk_sc",
            include_bytes!("../assets/fonts/NotoSansSC-Regular.otf"),
        ),
        (
            "cjk_jp_bold",
            include_bytes!("../assets/fonts/NotoSansJP-Bold.otf"),
        ),
        (
            "cjk_kr_bold",
            include_bytes!("../assets/fonts/NotoSansKR-Bold.otf"),
        ),
        (
            "cjk_sc_bold",
            include_bytes!("../assets/fonts/NotoSansSC-Bold.otf"),
        ),
    ] {
        fonts.font_data.insert(
            name.to_owned(),
            Arc::new(egui::FontData::from_static(bytes)),
        );
    }

    let (cjk_regular, cjk_bold) = cjk_order(locale);
    let builtin = fonts
        .families
        .get(&egui::FontFamily::Proportional)
        .cloned()
        .unwrap_or_default();

    let mut proportional = vec!["inter".to_owned()];
    proportional.extend(cjk_regular.iter().map(|name| (*name).to_owned()));
    proportional.extend(builtin.iter().cloned());

    let mut monospace = vec!["jbmono".to_owned()];
    monospace.extend(cjk_regular.iter().map(|name| (*name).to_owned()));
    monospace.extend(builtin.iter().cloned());

    let mut emphasised = vec!["inter_semibold".to_owned(), "inter".to_owned()];
    emphasised.extend(cjk_bold.iter().map(|name| (*name).to_owned()));
    emphasised.extend(cjk_regular.iter().map(|name| (*name).to_owned()));
    emphasised.extend(builtin);

    fonts
        .families
        .insert(egui::FontFamily::Proportional, proportional);
    fonts
        .families
        .insert(egui::FontFamily::Monospace, monospace);
    fonts.families.insert(semibold(), emphasised);
    fonts
}

/// The CJK faces for a locale, own-regional-form first, as (regular, bold).
fn cjk_order(locale: Option<&str>) -> ([&'static str; 3], [&'static str; 3]) {
    match locale {
        Some("ko") => (
            ["cjk_kr", "cjk_jp", "cjk_sc"],
            ["cjk_kr_bold", "cjk_jp_bold", "cjk_sc_bold"],
        ),
        Some("zh-Hans") => (
            ["cjk_sc", "cjk_jp", "cjk_kr"],
            ["cjk_sc_bold", "cjk_jp_bold", "cjk_kr_bold"],
        ),
        _ => (
            ["cjk_jp", "cjk_kr", "cjk_sc"],
            ["cjk_jp_bold", "cjk_kr_bold", "cjk_sc_bold"],
        ),
    }
}

/// Install the bundled fonts.
///
/// Must run BEFORE the first frame, which is why `main.rs` calls it from the
/// eframe creation context rather than from `apply_theme`: `set_fonts` only
/// takes effect on the next frame, and the UI reaches for the named family
/// immediately, so binding them late panics with "FontFamily::Name(..) is not
/// bound to any fonts".
pub(crate) fn install_fonts(ctx: &egui::Context, locale: Option<&str>) {
    ctx.set_fonts(font_definitions(locale));
}

/// The font family for emphasised text, matching the website's font weights.
fn semibold() -> egui::FontFamily {
    egui::FontFamily::Name("semibold".into())
}

/// Install the application theme. Called once, on the first frame.
///
/// Every colour comes from [`crate::design`]; shadows are off, and the two
/// global hairlines use `border-1`/`border-2` rather than accent arithmetic.
fn apply_theme(ctx: &egui::Context) {
    let mut v = egui::Visuals::dark();
    v.panel_fill = bg_1;
    v.window_fill = bg_2;
    v.extreme_bg_color = bg_0;
    v.faint_bg_color = bg_2;
    v.code_bg_color = bg_2;
    v.text_edit_bg_color = Some(bg_0);
    v.window_stroke = egui::Stroke::new(1.0, border_1);
    v.window_corner_radius = egui::CornerRadius::same(R_CONTROL);
    v.window_shadow = egui::Shadow::NONE;
    v.popup_shadow = egui::Shadow::NONE;
    v.menu_corner_radius = egui::CornerRadius::same(R_CONTROL);
    v.override_text_color = Some(text_1);
    v.weak_text_color = Some(text_3);
    v.hyperlink_color = accent_text;
    v.warn_fg_color = warn;
    v.error_fg_color = danger;
    v.selection.bg_fill = accent_soft;
    v.selection.stroke = egui::Stroke::new(1.0, accent_line);

    let line = egui::Stroke::new(1.0, border_1);
    v.widgets.noninteractive.bg_fill = bg_2;
    v.widgets.noninteractive.weak_bg_fill = bg_2;
    v.widgets.noninteractive.bg_stroke = line;
    v.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, text_3);
    v.widgets.noninteractive.corner_radius = egui::CornerRadius::same(R_CONTROL);

    v.widgets.inactive.bg_fill = bg_3;
    v.widgets.inactive.weak_bg_fill = bg_3;
    v.widgets.inactive.bg_stroke = line;
    v.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.inactive.corner_radius = egui::CornerRadius::same(R_CONTROL);

    v.widgets.hovered.bg_fill = bg_4;
    v.widgets.hovered.weak_bg_fill = bg_4;
    v.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, border_2);
    v.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.hovered.corner_radius = egui::CornerRadius::same(R_CONTROL);

    v.widgets.active.bg_fill = accent_deep;
    v.widgets.active.weak_bg_fill = accent_deep;
    v.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent);
    v.widgets.active.fg_stroke = egui::Stroke::new(1.0, on_accent);
    v.widgets.active.corner_radius = egui::CornerRadius::same(R_CONTROL);

    v.widgets.open.bg_fill = bg_3;
    v.widgets.open.weak_bg_fill = bg_3;
    v.widgets.open.bg_stroke = line;
    v.widgets.open.fg_stroke = egui::Stroke::new(1.0, text_1);
    v.widgets.open.corner_radius = egui::CornerRadius::same(R_CONTROL);

    let mut style = (*ctx.style_of(egui::Theme::Dark)).clone();
    style.visuals = v;
    style.spacing.item_spacing = egui::vec2(S2, S2);
    style.spacing.button_padding = egui::vec2(S3, S2);
    style.spacing.window_margin = egui::Margin::same(S4 as i8);
    style.spacing.menu_margin = egui::Margin::same(S2 as i8);
    style.spacing.interact_size.y = CONTROL_H;
    style.spacing.icon_width = 14.0;
    style.text_styles = [
        (egui::TextStyle::Heading, egui::FontId::proportional(TITLE)),
        (egui::TextStyle::Body, egui::FontId::proportional(BODY)),
        (
            egui::TextStyle::Button,
            egui::FontId::new(LABEL, semibold()),
        ),
        (egui::TextStyle::Small, egui::FontId::proportional(CAPTION)),
        (egui::TextStyle::Monospace, egui::FontId::monospace(VALUE)),
    ]
    .into();
    ctx.set_style_of(egui::Theme::Dark, style);
    ctx.set_theme(egui::Theme::Dark);
}

/// The quality ramp for a round trip: good under 60 ms, attention under 120 ms,
/// bad beyond that.
fn rtt_colour(rtt_ms: f64) -> egui::Color32 {
    if rtt_ms < 60.0 {
        signal
    } else if rtt_ms < 120.0 {
        warn
    } else {
        danger
    }
}

fn parse_server_addr(s: &str) -> Option<SocketAddrV4> {
    if s.is_empty() {
        return None;
    }
    s.parse::<SocketAddrV4>().ok()
}

fn connect_instruction(game: &GameEntry, local_port: u16) -> String {
    let port = local_port.to_string();
    match game.key {
        "rust" => i18n::t_with("connect.rust", &[("port", port.as_str())]),
        "cs2" => i18n::t_with("connect.cs2", &[("port", port.as_str())]),
        "dota2" => i18n::t_with("connect.dota2", &[("port", port.as_str())]),
        _ => i18n::t_with("connect.generic", &[("port", port.as_str())]),
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

/// Decode the embedded brand tile into a texture for the About sheet.
///
/// A decode failure is logged and treated as absent so the sheet falls back
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

/// The one action slot, as a pure function of the gates.
///
/// The returned flag is "enabled": whether the slot can act right now. The
/// three labels plus the disabled case are the whole state machine:
/// - `STOP BOOST` when boosting.
/// - `RESTART AS ADMINISTRATOR TO BOOST` when unelevated (and not boosting).
/// - `BOOST MY GAME` when elevated, connected, and a relay is selected.
/// - `BOOST MY GAME`, disabled, otherwise (elevated but not ready).
fn primary_action(
    is_admin: bool,
    boosting: bool,
    connected: bool,
    has_relay: bool,
) -> (&'static str, bool) {
    if boosting {
        ("STOP OPTIMIZING", true)
    } else if !is_admin {
        ("RESTART AS ADMINISTRATOR TO OPTIMIZE", true)
    } else if connected && has_relay {
        ("OPTIMIZE MY ROUTE", true)
    } else {
        ("OPTIMIZE MY ROUTE", false)
    }
}

/// The two treatments the action slot takes. Accent is the one bright fill;
/// Danger is the quiet destructive control.
#[derive(Clone, Copy, PartialEq, Eq)]
enum ActionKind {
    Accent,
    Danger,
}

/// The one bright control: a custom-painted button so it never reads as
/// default egui chrome. Accent uses `accent-deep` at rest (white ink on it is
/// AA-safe) and brightens to `accent` on hover; Danger uses `bg-3` with a
/// danger hairline and ink, tinting with `danger-soft` on hover.
fn action_button(ui: &mut egui::Ui, text: &str, kind: ActionKind, ready: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), ACTION_H),
        egui::Sense::click(),
    );
    let hovered = response.hovered() && ready;

    let (fill, stroke, ink) = match kind {
        ActionKind::Accent => {
            if !ready {
                (bg_3, border_1, text_3)
            } else if hovered {
                (accent, accent, on_accent)
            } else {
                (accent_deep, accent_deep, on_accent)
            }
        }
        ActionKind::Danger => {
            if !ready {
                (bg_3, border_1, text_3)
            } else if hovered {
                (danger_soft, danger, danger)
            } else {
                (bg_3, danger, danger)
            }
        }
    };

    let painter = ui.painter();
    painter.rect(
        rect,
        egui::CornerRadius::same(R_BUTTON),
        fill,
        egui::Stroke::new(1.0, stroke),
        egui::StrokeKind::Inside,
    );
    if kind == ActionKind::Danger && ready {
        // A painted stop square: U+25A0 lives only in the icon font, so the
        // mark is drawn rather than typed (DESIGN.md rule 1).
        let square = egui::Rect::from_center_size(
            egui::pos2(rect.left() + 22.0, rect.center().y),
            egui::vec2(10.0, 10.0),
        );
        painter.rect_filled(square, 0.0, danger);
    }
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::new(EMPHASIS, semibold()),
        ink,
    );
    response
}

/// One value in the label/value ledger.
enum LedgerValue {
    /// Genuine data (addresses, ports, counters): JetBrains Mono.
    Data(String),
    /// A dim prose hint where there is no value to show.
    Hint(&'static str),
}

/// The ledger's label cell: caption ink, fixed width, left-aligned.
fn ledger_label(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, ROW_H), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::proportional(CAPTION),
        text_3,
    );
}

/// A config field's label cell: `label` ink, fixed width at the wide breakpoint.
fn field_label(ui: &mut egui::Ui, width: f32, text: &str) {
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, CONTROL_H), egui::Sense::hover());
    ui.painter().text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        text,
        egui::FontId::new(LABEL, semibold()),
        text_1,
    );
}

/// A config row: label left at the wide breakpoint, label above at narrow, so
/// the control always keeps a full-width tap target when the window is snapped
/// beside a game.
fn field_row(ui: &mut egui::Ui, narrow: bool, label: &str, add_field: impl FnOnce(&mut egui::Ui)) {
    if narrow {
        ui.label(
            egui::RichText::new(label)
                .size(LABEL)
                .family(semibold())
                .color(text_1),
        );
        ui.add_space(S1);
        add_field(ui);
    } else {
        ui.allocate_ui_with_layout(
            egui::vec2(ui.available_width(), CONTROL_H),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                field_label(ui, LEDGER_LABEL_W, label);
                add_field(ui);
            },
        );
    }
}

/// A section heading in the config region or a sheet body.
/// A 1px `border-1` hairline: the design's only divider between sections.
fn hairline(ui: &mut egui::Ui) {
    let width = ui.available_width();
    let (rect, _) = ui.allocate_exact_size(egui::vec2(width, 1.0), egui::Sense::hover());
    ui.painter().rect_filled(rect, 0.0, border_1);
}

fn section_label(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(LABEL)
            .family(semibold())
            .color(text_1),
    );
    ui.add_space(S1);
}

/// A sheet title (`title` type).
fn sheet_title(ui: &mut egui::Ui, text: &str) {
    ui.label(
        egui::RichText::new(text)
            .size(TITLE)
            .family(semibold())
            .color(text_1),
    );
    ui.add_space(S2);
}

/// The shared frame for every in-window sheet: `bg-0`, a `border-1` hairline,
/// and the sheet radius.
fn sheet_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(bg_0)
        .stroke(egui::Stroke::new(1.0, border_1))
        .corner_radius(egui::CornerRadius::same(R_SHEET))
        .inner_margin(egui::Margin::same(S4 as i8))
}

/// The capped width and height a sheet may take, so a pinned sheet never
/// overflows a 320 px window.
fn sheet_bounds(ctx: &egui::Context) -> (f32, f32) {
    let screen = ctx.content_rect();
    let max_w = (screen.width() - 24.0).min(420.0);
    let max_h = (screen.height() - 120.0).max(120.0);
    (max_w, max_h)
}

/// A hand-painted RTT sparkline: the stroke is `text-3` and only the latest
/// point carries the quality ramp, so latency colour never shares a channel
/// with the state colour.
fn sparkline(ui: &mut egui::Ui, history: &[f64]) {
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), PLOT_H),
        egui::Sense::hover(),
    );
    if history.len() < 2 {
        return;
    }
    let painter = ui.painter_at(rect);
    let n = history.len();
    let (min, max) = history
        .iter()
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &v| {
            (lo.min(v), hi.max(v))
        });
    let span = (max - min).max(1.0);
    let pad = rect.shrink2(egui::vec2(1.0, 4.0));
    let points: Vec<egui::Pos2> = history
        .iter()
        .enumerate()
        .map(|(i, &v)| {
            let x = pad.left() + (i as f32 / (n - 1) as f32) * pad.width();
            let y = pad.bottom() - ((v - min) / span) as f32 * pad.height();
            egui::pos2(x, y)
        })
        .collect();
    painter.add(egui::Shape::line(
        points.clone(),
        egui::Stroke::new(1.5, text_3),
    ));
    if let (Some(&last), Some(&point)) = (history.last(), points.last()) {
        painter.circle_filled(point, 2.5, rtt_colour(last));
    }
}

/// Group a counter's thousands with commas, so `12412` reads `12,412`.
fn group_digits(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::lock_or_recover;

    #[test]
    fn a_poisoned_mutex_is_recovered_not_panicked() {
        let mutex = std::sync::Mutex::new(String::from("state"));
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _guard = mutex.lock().unwrap();
            panic!("simulated panic while holding the mutex");
        }));
        assert!(result.is_err(), "the panic must poison the mutex");
        assert_eq!(*lock_or_recover(&mutex), "state");
    }
    use super::{
        apply_discovery_result, close_decision, games, primary_action, should_apply_race,
        should_race, CloseDecision,
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
        super::install_fonts(&ctx, None);
        super::apply_theme(&ctx);

        let raw = eframe::egui::RawInput {
            screen_rect: Some(eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(0.0, 0.0),
                eframe::egui::vec2(420.0, 800.0),
            )),
            ..Default::default()
        };
        let mut out = ctx.run_ui(raw, |ui| {
            ui.label(eframe::egui::RichText::new("Emphasis").family(super::semibold()));
            ui.label("body");
            ui.monospace("207.246.106.36:4434");
        });
        out.textures_delta.clear();
        assert!(!out.shapes.is_empty(), "a frame with text drew nothing");
    }

    /// The localized render, proven end to end rather than by proxy: the same
    /// widget code must draw different text under two languages, and the
    /// German frame must contain the German string and not the English one.
    #[test]
    fn a_localized_frame_draws_the_translated_string() {
        let drawn = |key: &str| {
            let ctx = eframe::egui::Context::default();
            super::install_fonts(&ctx, Some("de"));
            super::apply_theme(&ctx);
            let raw = eframe::egui::RawInput {
                screen_rect: Some(eframe::egui::Rect::from_min_size(
                    eframe::egui::pos2(0.0, 0.0),
                    eframe::egui::vec2(420.0, 800.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                super::section_label(ui, &crate::i18n::t(key));
            });
            out.textures_delta.clear();
            format!("{:?}", out.shapes)
        };

        crate::i18n::set_language(Some("de"));
        let german = format!("{:?}", drawn("settings.privacy"));
        crate::i18n::set_language(None);
        let english = format!("{:?}", drawn("settings.privacy"));

        assert!(
            german.contains("Datenschutz"),
            "the German frame did not draw the German string: {german}"
        );
        assert!(
            english.contains("Privacy"),
            "the English frame did not draw the English string: {english}"
        );
        assert_ne!(german, english, "both languages drew identical frames");
    }

    /// Every character in every catalog must be drawable by the families the app
    /// installs.
    ///
    /// Asserted against the real `FontDefinitions` the app builds and the face
    /// bytes inside them, not a copy of the chain, so dropping a face from a
    /// family or adding a catalog character nothing carries both fail here. The
    /// bundled Inter is a 230-codepoint subset and `semibold` once chained only
    /// Inter - every emphasised Cyrillic label in the shipped build was tofu.
    #[test]
    fn every_catalog_glyph_is_drawable_in_every_family() {
        use ab_glyph::{Font as _, FontArc};

        let catalogs: [(&str, &str); 9] = [
            ("de", include_str!("../locales/de.toml")),
            ("en", include_str!("../locales/en.toml")),
            ("es", include_str!("../locales/es.toml")),
            ("fr", include_str!("../locales/fr.toml")),
            ("ja", include_str!("../locales/ja.toml")),
            ("ko", include_str!("../locales/ko.toml")),
            ("pt-BR", include_str!("../locales/pt-BR.toml")),
            ("ru", include_str!("../locales/ru.toml")),
            ("zh-Hans", include_str!("../locales/zh-Hans.toml")),
        ];

        let defs = super::font_definitions(Some("zh-Hans"));
        let faces: Vec<(String, FontArc)> = defs
            .font_data
            .iter()
            .map(|(name, data)| {
                let face =
                    FontArc::try_from_vec(data.font.to_vec()).expect("a bundled face must parse");
                (name.clone(), face)
            })
            .collect();

        let mut undrawable: Vec<String> = Vec::new();
        for (family, chain) in &defs.families {
            for (tag, catalog) in catalogs {
                for ch in catalog.chars().filter(|ch| !ch.is_ascii()) {
                    let drawable = chain.iter().any(|name| {
                        faces
                            .iter()
                            .find(|(face_name, _)| face_name == name)
                            .is_some_and(|(_, face)| face.glyph_id(ch).0 != 0)
                    });
                    if !drawable {
                        undrawable.push(format!("{family:?} / {tag}: U+{:04X} {ch}", ch as u32));
                    }
                }
            }
        }
        undrawable.sort();
        undrawable.dedup();
        assert!(
            undrawable.is_empty(),
            "catalog characters no installed face can draw: {undrawable:?}"
        );
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

    #[test]
    fn primary_action_names_the_three_states() {
        // Boosting: the only destructive control.
        assert_eq!(
            primary_action(true, true, true, true),
            ("STOP OPTIMIZING", true)
        );
        assert_eq!(
            primary_action(false, true, false, false),
            ("STOP OPTIMIZING", true)
        );
        // Unelevated: the action is elevation.
        assert_eq!(
            primary_action(false, false, true, true),
            ("RESTART AS ADMINISTRATOR TO OPTIMIZE", true)
        );
        assert_eq!(
            primary_action(false, false, false, false),
            ("RESTART AS ADMINISTRATOR TO OPTIMIZE", true)
        );
        // Elevated and ready: the product's action.
        assert_eq!(
            primary_action(true, false, true, true),
            ("OPTIMIZE MY ROUTE", true)
        );
    }

    #[test]
    fn primary_action_disables_boost_until_connected_and_relayed() {
        // Elevated but disconnected: keep the label, but the slot cannot act.
        assert_eq!(
            primary_action(true, false, false, true),
            ("OPTIMIZE MY ROUTE", false)
        );
        // Elevated and connected, but no relay is selected.
        assert_eq!(
            primary_action(true, false, true, false),
            ("OPTIMIZE MY ROUTE", false)
        );
    }

    #[test]
    fn group_digits_thousands_groups_counters() {
        use super::group_digits;
        assert_eq!(group_digits(0), "0");
        assert_eq!(group_digits(41), "41");
        assert_eq!(group_digits(12412), "12,412");
        assert_eq!(group_digits(1_234_567), "1,234,567");
    }
}
