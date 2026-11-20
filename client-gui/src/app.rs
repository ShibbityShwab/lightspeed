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

use std::borrow::Cow;
use std::net::SocketAddrV4;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{self, GuiConfig, ProxyEntry};
use crate::design::*;
use crate::discovery::{self, DiscoveryOutcome, RelayHealth};
use crate::globe;
use crate::i18n;
use crate::paths;
use crate::platform::{default_port_range, Platform, QuitFlag, TrayAction, TrayHandle};
use crate::processes::{self, ProcessEntry};
use crate::race_watch::decide_switch;
use crate::update::UpdateStatus;
use eframe::egui;
use lightspeed_client::interceptor::ProcessTarget;
use lightspeed_client::profiles::schema::{is_valid_key, parse_port_range, slugify, PortRange};
use lightspeed_client::profiles::{ProfileStore, UserGameProfile, UserServer};
use lightspeed_client::{EngineStatus, LightSpeedEngine, RelayCandidate, PRIOR_UNCERTAINTY_US};
use std::net::Ipv4Addr;

mod choice;
mod editor;
mod games;
mod ledger;
mod picker;
mod presence;
mod relay_choice;
#[cfg(test)]
mod tests;
mod theme;
mod watchdog;
mod widgets;
mod window;

use choice::*;
use editor::*;
use games::*;
use ledger::*;
use picker::*;
use presence::*;
use relay_choice::*;
use theme::*;
use watchdog::*;
use widgets::*;
use window::*;

// `main.rs` reaches `install_fonts` and the window-height bounds through
// `crate::app::…`, so the root has to re-export them instead of only importing
// them privately.
pub(crate) use theme::install_fonts;
pub(crate) use window::{MAX_WINDOW_HEIGHT, MIN_WINDOW_HEIGHT};

// ── Tray state enum ──────────────────────────────────────────────────────────

/// The five states the tray icon can reflect in its color and tooltip.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayState {
    Disconnected,
    Connected,
    Optimizing,
    /// A relay is carrying the session but has been judged not to be returning
    /// the game's traffic. It outranks [`Self::Optimizing`]: the tray must never
    /// claim optimization while the return path is failing.
    RelayStarved,
    Error,
}

/// While auto-select is on and the tunnel is up, how often to re-run the relay
/// race so a better - or newly faster - relay can take over.
const RELAY_RECHECK_INTERVAL: Duration = Duration::from_secs(600);

/// How long traffic may be held direct before the app gives up identifying the
/// game server and relays through the nearest relay anyway.
///
/// A hold exists so a player is never routed through a relay chosen before
/// anyone knew where their server was. But a hold that never releases is worse
/// than the thing it prevents: a game outside the profile list, or one whose
/// ports are not matched, would mean no relaying at all and no explanation. Two
/// minutes covers menus, matchmaking and loading; after that the fallback is the
/// old behaviour, and the relay is still re-chosen for the server the moment one
/// does lock.
const HOLD_FALLBACK: Duration = Duration::from_secs(120);

/// How long a locked game server must persist before the relay is chosen for
/// it. A lobby backend locks first and is replaced by the match server a few
/// seconds later; committing on the first address chooses the relay for the
/// lobby and stages the match.
const LOCK_SETTLE: Duration = Duration::from_secs(3);

/// How many distinct relays may be on record as starving one server before the
/// chooser stops choosing automatically.
///
/// Three is the point where the records agree with each other and another
/// automatic pick is just motion; the player can still stop optimizing or pick a
/// relay by hand. The no-reply detection itself lives in `app::watchdog`.
const MAX_RELAYS_TRIED: usize = 3;

/// How old a direct-ping sample may be before the window stops showing it and
/// the ranking stops using it as a triangle bound. One rule for both, because a
/// stale ping displayed as current and a stale ping shaping the choice are the
/// same defect.
const SERVER_PING_FRESH_SECS: u64 = 60;

/// How old a direct-ping sample may be before the window asks for a fresh one.
const PING_REFRESH_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

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

/// Lifecycle of the background process enumeration behind the editor's picker.
///
/// The same shape as [`DiscoveryState`], for the same reason: enumerating the
/// host's processes (`CreateToolhelp32Snapshot`, a `/proc` walk, a `ps` fork)
/// is not something the egui thread may do, so the work happens on a worker
/// thread and the UI only ever polls the receiver.
pub enum ProcessListState {
    /// Nothing has been asked for yet.
    Idle,
    /// A background thread is enumerating the host's processes.
    InFlight(Receiver<Vec<ProcessEntry>>),
    /// The last enumeration finished (with rows, or with nothing at all); the
    /// rows live in `LightSpeedApp::process_rows`.
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
    /// The server the no-reply watchdog found starving. The GUI writes it into
    /// the relay history, which demotes the relay for the next ranking; the
    /// session keeps relaying and the warning is the only visible action.
    relay_starved: Arc<Mutex<Option<StarvationVerdict>>>,
    /// The active starvation warning, published by the watchdog thread for the
    /// window and the tray. `None` while the relay returns the session's traffic
    /// (or nothing is being relayed).
    relay_warning: Arc<Mutex<Option<RelayStarvationWarning>>>,

    // ── Proxy connection ─────────────────────────────────────────────────
    selected_proxy_idx: usize,
    auto_select: bool,
    relay_race: Option<Receiver<Option<SocketAddrV4>>>,

    /// Measured client legs for every discovered relay, kept so the
    /// destination-aware choice can rank the whole path rather than the winner
    /// of a client-leg race.
    relay_legs: Option<RelayLegs>,

    /// The relay the destination-aware chooser picked for the current game
    /// server, shown in the picker so a manual choice can see what it overrides.
    destination_winner: Option<SocketAddrV4>,

    /// The last destination-aware ranking for the game server, kept so the route
    /// surface can say how the server leg was obtained (measured, triangle bound,
    /// or region prior) instead of presenting a relayed path as fully measured.
    route_ranking: Option<RouteRanking>,

    /// The client-side server-location trace: one background trace at a time, a
    /// server-keyed cache, and at most one attempt per (server, relay session).
    /// It runs from the client's own vantage, so it locates the server; it never
    /// contributes to relay ranking.
    client_trace: crate::client_trace::GuiTrace,
    /// The route the card captions, refreshed each frame by
    /// [`Self::poll_client_trace`]; `None` while there is no fresh route for the
    /// locked server.
    client_route: Option<crate::client_trace::ClientRoute>,

    /// The game server the current relay was chosen for, so a new raid's server
    /// triggers a fresh choice and a repeated tick does not.
    chosen_for_server: Option<Ipv4Addr>,
    /// The address last locked and when, for the settle window: the relay is not
    /// chosen until one address survives [`LOCK_SETTLE`].
    locked_addr: Option<Ipv4Addr>,
    locked_since: Option<std::time::Instant>,
    /// Whether a relay has been chosen for the server this run locked, shared
    /// with the engine's hold watcher: the relay being *connected* is not the
    /// same condition as a relay having been chosen *for that server*, and
    /// releasing on the former is what kept a startup relay in place.
    server_chosen: Arc<AtomicBool>,

    /// When tunnelling was last withheld, so a hold that never finds a server
    /// cannot strand the run silently.
    held_since: Option<std::time::Instant>,

    /// True while the engine is deliberately holding game traffic on the direct
    /// path until a relay is chosen for the locked server. This is the normal
    /// start-of-match wait, not a failed route: the watchdog no longer takes
    /// sessions direct, so this is the only hold the window can show.
    holding: bool,
    /// The watchdog's active warning, polled each frame from the thread that
    /// owns the detection. Shown in the rail, the route card and the tray; it
    /// never causes a route change.
    starvation_warning: Option<RelayStarvationWarning>,
    /// Estimated whole-loop RTT to the game server through the relay, in ms;
    /// 0.0 until the sampler has enough paired round trips for this pair.
    /// How long ago the probe behind `path_rtt_ms` was taken.
    /// Cumulative `(up, down)` counters as they stood when this session started,
    /// so the Relayed row describes the current session rather than the process.
    relayed_base: Option<(String, u64, u64)>,
    /// Direct (ICMP) ping to a game server in ms, with its age in seconds **and
    /// the server it was measured for**. The one figure here that is a real
    /// correlated round trip; a record for another server is not evidence about
    /// this one (see [`ServerPing`]).
    server_ping: Option<ServerPing>,
    /// Set while a worker thread runs a direct burst, which blocks for seconds.
    ping_in_flight: std::sync::Arc<std::sync::atomic::AtomicBool>,
    /// When the latency comparison was last logged, so the trace carries a
    /// periodic record of what each path is doing without flooding it.
    compared_at: Option<std::time::Instant>,
    /// What each relay has done for each game server, across sessions.
    relay_history: crate::relay_history::RelayHistory,
    /// Counters as of the last poll, so only this interval's deltas are recorded.
    history_counters: Option<(std::net::SocketAddrV4, u64, u64)>,
    /// When the memory was last written, so a chatty session does not rewrite it
    /// every second.
    history_saved_at: Option<std::time::Instant>,

    /// Baseline for the relay verdict: the two tunnel counters as of the last
    /// sample, so each window is judged on its own movement rather than on a
    /// cumulative ratio that a healthy start would mask forever.
    health_base: Option<RelayHealthBase>,

    /// The last verdict stated, so a stable verdict is logged once rather than
    /// every second.
    relay_health: Option<RelayHealthVerdict>,

    /// The single background worker that scans the host for a running game.
    ///
    /// Detection spawns `tasklist` plus a PowerShell process query (0.4 s here,
    /// seconds on the EDR-heavy machine in issue #178), so it must never run on
    /// the egui thread. The worker owns the 5 s cadence; this handle is only the
    /// polled result channel plus the stop/join token.
    presence: GamePresence,

    /// Whether a game was present at the last scan, so a start/stop happens on a
    /// transition only and not on every tick.
    auto_game_present: bool,

    /// Latched once the auto-start has launched for the current game presence, so
    /// the watcher does not restart the tunnel on every tick.
    auto_started_for_presence: bool,

    /// A client-leg measurement in flight for every relay.
    relay_measurement: Option<Receiver<RelayLegs>>,
    /// When the periodic auto-select re-race last started; cleared while
    /// disconnected or while auto-select is off so the clock restarts cleanly.
    last_race: Option<std::time::Instant>,
    /// A pending timed re-measurement of a connected re-race winner.
    winner_probe: Option<WinnerProbe>,
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
    /// The custom-game editor sheet.
    show_game_editor: bool,
    /// The editor's fields, and the profile they edit.
    game_editor: GameEditorDraft,
    /// The last saved profile's key, so `--game <key>` stays on screen.
    editor_saved: Option<String>,
    /// The editor's last refusal, verbatim from the profile store.
    editor_error: Option<String>,
    /// The editor's process picker: the background enumeration's lifecycle, the
    /// rows it last produced, and the filter field.
    process_list: ProcessListState,
    process_rows: Vec<ProcessEntry>,
    process_search: String,
    /// A pinned user profile key, when the user picked one from "My games".
    selected_custom: Option<String>,
    /// The process the front door picked: relay every packet of ITS traffic.
    /// `None` is the built-in Game path, where the profile's ports decide.
    target_process: Option<ProcessTarget>,
    /// Whether the "Target a process" picker sheet is open.
    show_target_picker: bool,

    /// The Manual flow's "Save as…" name field, and its outcome line.
    save_server_name: String,
    server_saved: Option<String>,
    server_error: Option<String>,

    /// Language pinned in `config.toml`, or `None` to follow the OS locale.
    language: Option<String>,

    /// Effective path-MTU cap for the tunnel, persisted from `config.toml`.
    tunnel_mtu: usize,

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

/// The route card's location lines for one server place.
///
/// `caption` names the strongest evidence the offline indexes have, `detail`
/// names the provider region, operator place or hosting network, `note` is the
/// operator-geofeed "shown as-is" line (with `source` as its hover provenance),
/// and `hint` is the one extra line the card shows when no free database could
/// place the server.
struct RouteLocationLines {
    caption: String,
    detail: Option<String>,
    note: Option<String>,
    source: Option<String>,
    hint: Option<String>,
}

/// Build the route card's location block without touching a `ui`.
///
/// Only the two answers that could not place the server carry the hint. A
/// hosting match deliberately has no dot and no city, so the card says why the
/// location is unknown instead of drawing a false one; `Unknown` gets the same
/// line because no database matched at all. An operator geofeed is the
/// operator's own statement: it gets the place plus attribution and the as-of
/// date, no free-database caveat, and still no map dot (the feed has no
/// coordinate).
fn route_location_lines(place: crate::server_place::Place) -> RouteLocationLines {
    use crate::server_place::Place;
    match place {
        Place::Cloud {
            provider, region, ..
        } => RouteLocationLines {
            caption: i18n::t("route.location_provider").to_string(),
            detail: Some(format!("{} · {region}", provider.to_uppercase())),
            note: None,
            source: None,
            hint: None,
        },
        Place::Geofeed {
            provider,
            country,
            region,
            city,
            source,
            as_of,
        } => {
            let place = crate::server_place::geofeed_place_text(country, region, city);
            RouteLocationLines {
                caption: i18n::t_with("route.location_geofeed", &[("place", place.as_str())]),
                detail: Some(i18n::t_with(
                    "route.location_geofeed_detail",
                    &[("provider", provider), ("date", as_of)],
                )),
                note: Some(i18n::t("route.location_geofeed_note").to_string()),
                source: Some(source.to_string()),
                hint: None,
            }
        }
        Place::Hosting { asn, name } => RouteLocationLines {
            caption: i18n::t("route.location_hosting").to_string(),
            detail: Some(format!("AS{asn} · {name}")),
            note: None,
            source: None,
            hint: Some(i18n::t("route.location_hosting_hint").to_string()),
        },
        Place::Country { .. } => RouteLocationLines {
            caption: i18n::t("route.location_estimate").to_string(),
            detail: None,
            note: None,
            source: None,
            hint: None,
        },
        Place::Unknown => RouteLocationLines {
            caption: i18n::t("route.location_unknown").to_string(),
            detail: None,
            note: None,
            source: None,
            hint: Some(i18n::t("route.location_hosting_hint").to_string()),
        },
    }
}

impl<P: Platform> LightSpeedApp<P> {
    pub fn new(engine: Arc<Mutex<LightSpeedEngine>>, quit: QuitFlag) -> Self {
        let tray = P::new_tray(Arc::clone(&quit));
        tracing::info!("system tray created: {}", tray.is_some());
        let status = lock_or_recover(&engine).snapshot();

        // Detection is deliberately not run here: `new()` executes on the thread
        // that will pump the egui loop, and one scan spawns two processes. The
        // presence worker (see `app::presence`) performs the first scan as soon
        // as the first frame enables it, and `poll_game_presence` applies it.
        let auto_detected_game: Option<String> = None;

        let is_admin = P::is_admin();

        let saved = config::load(&paths::config_file());
        // Apply the persisted path-MTU cap before any boost can start; the data
        // plane also lowers it on an EMSGSIZE rejection, so this is the user's
        // explicit floor rather than the only protection.
        lock_or_recover(&engine).set_path_mtu_cap(saved.tunnel_mtu);
        let tunnel_mtu = saved.tunnel_mtu;

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
        // A pinned key that is not a built-in is one of the user's profiles, so
        // it stays pinned even though the built-in lookup above fell back to 0.
        let selected_custom = if game_auto || game_manual {
            None
        } else {
            saved
                .selected_game
                .as_deref()
                .filter(|key| games().iter().all(|entry| entry.key != *key))
                .filter(|key| user_game_keys().iter().any(|user_key| user_key == key))
                .map(str::to_string)
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

        let relay_starved: Arc<Mutex<Option<StarvationVerdict>>> = Arc::new(Mutex::new(None));
        let relay_warning: Arc<Mutex<Option<RelayStarvationWarning>>> = Arc::new(Mutex::new(None));

        // The sampler that pairs tunnelled round trips drops every sample while
        // no estimator is installed, and the only installer was the CLI's probe
        // path - so in the GUI the whole-loop figure and the measured server legs
        // for relay ranking were both empty. Install it before traffic can arrive.
        lightspeed_client::ensure_destination_estimator();
        spawn_relay_reply_watchdog(
            Arc::clone(&engine),
            Arc::clone(&quit),
            Arc::clone(&relay_starved),
            Arc::clone(&relay_warning),
        );
        let mut app = Self {
            engine,
            status,
            tray,
            quit,
            relay_starved,
            relay_warning,
            selected_proxy_idx,
            auto_select,
            relay_race: None,
            relay_legs: None,
            destination_winner: None,
            route_ranking: None,
            client_trace: crate::client_trace::GuiTrace::new(),
            client_route: None,
            relay_measurement: None,
            chosen_for_server: None,
            locked_addr: None,
            locked_since: None,
            server_chosen: Arc::new(AtomicBool::new(false)),
            held_since: None,
            holding: false,
            starvation_warning: None,
            relayed_base: None,
            server_ping: None,
            ping_in_flight: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            compared_at: None,
            relay_history: crate::relay_history::RelayHistory::load(
                &crate::paths::relay_history_file(),
            ),
            history_counters: None,
            history_saved_at: None,
            presence: GamePresence::spawn(),
            auto_game_present: false,
            auto_started_for_presence: false,
            last_race: None,
            winner_probe: None,
            manager_label_input: String::new(),
            manager_addr_input: String::new(),
            config_error: None,
            selected_game_idx,
            game_auto,
            game_manual,
            server_input: saved.manual_server.clone(),
            fec_enabled: false,
            capture_available: P::is_capture_available(),
            share_latency_stats,
            auto_detected_game,
            is_admin,
            fonts_setup: false,
            header_icon: None,
            show_settings: std::env::var_os("LIGHTSPEED_OPEN_SETTINGS").is_some(),
            show_game_editor: false,
            game_editor: GameEditorDraft::new_profile(),
            editor_saved: None,
            editor_error: None,
            process_list: ProcessListState::Idle,
            process_rows: Vec::new(),
            process_search: String::new(),
            selected_custom,
            target_process: None,
            show_target_picker: std::env::var_os("LIGHTSPEED_OPEN_PICKER").is_some(),
            save_server_name: String::new(),
            server_saved: None,
            server_error: None,
            language,
            tunnel_mtu,
            boost_start: None,
            health_base: None,
            relay_health: None,
            custom_port_input: saved.manual_port_range.clone(),
            update_check: UpdateCheckState::Idle,
            show_update_dialog: false,
            update_shared: Arc::new(Mutex::new(None)),
            install_shared: Arc::new(Mutex::new(None)),
            proxies,
            discovery: DiscoveryState::InFlight(discovery::spawn_discovery()),
            discovery_error: None,
            health: None,
        };

        if std::env::var_os("LIGHTSPEED_OPEN_UPDATE").is_some() {
            app.start_update_check();
        }

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

    /// The pinned game: a user profile when one is selected, else the built-in
    /// at [`Self::selected_game_idx`].
    ///
    /// By value: a user profile owns its strings, so the pick cannot borrow from
    /// a `&'static` registry table.
    fn selected_game(&self) -> PickedGame {
        if let Some(key) = &self.selected_custom {
            if let Some(profile) = user_profiles()
                .into_iter()
                .find(|profile| profile.key == *key)
            {
                return PickedGame {
                    key: profile.key,
                    display: profile.name,
                    default_port: profile.udp_ports.map(|range| range.lo).unwrap_or(0),
                };
            }
        }
        pick_game_entry(
            self.game_auto,
            self.auto_detected_game.as_deref(),
            self.selected_game_idx,
        )
    }

    fn connect_selected(&mut self) {
        let Some(entry) = self.selected_entry() else {
            return;
        };
        let addr = entry.addr;
        let node_id = entry.node_id.clone();
        let was_intercepting = {
            let mut engine = lock_or_recover(&self.engine);
            let running = engine.is_intercepting();
            engine.connect(addr);
            engine.set_node_id(node_id);
            running
        };
        // Any relay change moves the data plane with it: a switch that only
        // reconnects the control plane leaves the interceptor sending to the
        // old relay while the token belongs to the new one.
        if was_intercepting {
            self.restart_interceptor_on_selected_relay();
        }
    }

    /// Move a running interceptor onto the relay the follow just selected.
    ///
    /// `connect_selected` reconnects the control plane only. The data plane is
    /// a separate socket created with the proxy address at start time, so
    /// without this restart the game keeps sending to the old relay while the
    /// session token belongs to the new one, and the old relay rejects every
    /// packet. The hold is deliberately not re-armed: the server is locked and
    /// its relay is already chosen, so the new interceptor should relay at once.
    fn restart_interceptor_on_selected_relay(&mut self) {
        let Some(proxy) = self.selected_proxy_addr() else {
            return;
        };
        let game_key = self.selected_game().key;
        let port_range_override = parse_custom_port_range(&self.custom_port_input);
        let process_target = self.target_process.clone();
        let mut engine = lock_or_recover(&self.engine);
        engine.set_telemetry_game(&game_key);
        match engine.start_interceptor_with_target(
            &game_key,
            proxy,
            self.fec_enabled,
            4, // default FEC K
            port_range_override,
            process_target,
        ) {
            Ok(()) => tracing::info!("Restarted interception on the new relay {proxy}"),
            Err(e) => tracing::error!("relay switch could not restart interception: {e}"),
        }
    }

    fn start_discovery(&mut self) {
        if matches!(self.discovery, DiscoveryState::InFlight(_)) {
            return;
        }
        self.discovery_error = None;
        self.discovery = DiscoveryState::InFlight(discovery::spawn_discovery());
    }

    /// Begin (or restart) the background enumeration behind the editor's
    /// process picker.
    ///
    /// Called when the sheet opens and from its Refresh button. A load already
    /// in flight is left alone: it will land in a frame or two, and stacking a
    /// second snapshot of the same machine would only waste work.
    fn start_process_list(&mut self) {
        if matches!(self.process_list, ProcessListState::InFlight(_)) {
            return;
        }
        self.process_list = ProcessListState::InFlight(processes::spawn_process_list());
    }

    /// Collect a finished enumeration. The rows are published only once the
    /// worker returns, so the picker never shows a half-built table.
    fn poll_process_list(&mut self) {
        let rows = match &self.process_list {
            ProcessListState::InFlight(rx) => match rx.try_recv() {
                Ok(rows) => Some(rows),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
                // The worker cannot fail (the enumeration fails open), so a
                // dropped sender means an empty table, not a hang on
                // "Loading" for the rest of the session.
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some(Vec::new()),
            },
            ProcessListState::Idle | ProcessListState::Done => None,
        };
        if let Some(rows) = rows {
            tracing::info!("Process picker: {} processes", rows.len());
            self.process_rows = rows;
            self.process_list = ProcessListState::Done;
        }
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
            manual_server: self.server_input.clone(),
            manual_port_range: self.custom_port_input.clone(),
            tunnel_mtu: self.tunnel_mtu,
            selected_game: if self.game_auto || self.game_manual {
                None
            } else {
                Some(self.selected_game().key)
            },
        };
        if let Err(e) = config::save(&paths::config_file(), &config) {
            tracing::warn!("Could not persist GUI config: {e}");
        }
    }

    /// Race the discovered relays and connect to the fastest one.
    ///
    /// This is the fallback for when nothing is known about the destination. A
    /// race answers "which relay is closest to *me*", which is only half the
    /// path: for a player joining a server in another region it produced a
    /// wrong answer in the field (Los Angeles for an Ashburn server, from
    /// Bangkok). [`Self::start_destination_choice`] is preferred whenever a game
    /// server is known, and this remains for the case where none is.
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

    /// Choose the relay that minimises the whole path to the live game server.
    ///
    /// Preferred over [`Self::start_relay_race`] whenever a destination is known,
    /// because the relay-to-server leg is the half that decides whether relaying
    /// beats going direct. Returns `None` when no server is locked yet, which is
    /// when the client-leg race is still the best information available.
    ///
    /// When the server's position is not known exactly, the region prior is a
    /// guess about the half of the path the client cannot measure; the choice then
    /// falls back to the relays with a proven carry record for this exact server,
    /// then the measured client leg, jitter and (last) the relay address, and it
    /// removes relays whose record shows the starvation signature. See
    /// [`rank_without_destination`].
    ///
    /// The client legs come from [`Self::start_relay_measurement`], which measures
    /// every relay rather than only the winner: the winner of the client-leg race
    /// is exactly the relay this function is usually about to reject.
    fn choose_relay_for_destination(&mut self) -> Option<SocketAddrV4> {
        let server = self.game_server_addr()?;
        let measured = self.relay_legs.as_ref()?;

        // The server's position, when one source is exact enough to rank on.
        // Only a cloud-provider region qualifies: a country centroid is wrong
        // for hosting ranges (a G-Core US-Virginia server resolves to JP in the
        // free databases), so the chooser now gets a point only for the exact
        // case. Without one, `rank_without_destination` ranks on measured
        // evidence and drops the region prior rather than presenting a guess.
        let server_location = crate::server_place::chooser_point(server);

        let candidates: Vec<RelayCandidate> = self
            .proxies
            .iter()
            .filter_map(|entry| {
                // Match on the IP, not the whole address. The client leg is
                // measured against each relay's `/health` endpoint, which lives
                // on `HEALTH_PORT` rather than the advertised data port, so a
                // full-address comparison silently drops every candidate and the
                // chooser returns `None` forever. That is what happened: the
                // destination-aware selection never produced a single result and
                // the app stayed on the client-leg winner.
                let client_leg_us = measured
                    .iter()
                    .find(|(addr, _)| addr.ip() == entry.addr.ip())
                    .and_then(|(_, sample)| sample.map(|s| s.median.as_micros() as u64))?;
                let client_jitter_us = measured
                    .iter()
                    .find(|(addr, _)| addr.ip() == entry.addr.ip())
                    .and_then(|(_, sample)| sample.map(|s| s.jitter.as_micros() as u64))
                    .unwrap_or(0);
                Some(RelayCandidate {
                    relay: entry.addr,
                    region: Some(discovery::friendly_label(
                        entry.node_id.as_deref().unwrap_or_default(),
                    )),
                    node_id: entry.node_id.clone(),
                    client_leg_us: Some(client_leg_us),
                    client_jitter_us,
                })
            })
            .collect();

        if candidates.is_empty() {
            return None;
        }

        // A relay on record as not carrying this server is skipped. The record
        // does not need the watchdog's exhausted-chain verdict any more: a carry
        // record with enough traffic and no reply at all is the starvation
        // signature that kept the live 2026-11-19 session re-selecting
        // relay-sgp-1 (4,522 forwards, zero replies, `failed` never set). If
        // every candidate is on record, they are still the only option, so the
        // unfiltered list stands.
        if self.relay_history.condemned_count(server) >= MAX_RELAYS_TRIED {
            tracing::info!(
                server = %server,
                condemned = self.relay_history.condemned_count(server),
                "Every attempt at this server has failed; not choosing a relay"
            );
            return None;
        }

        // The measured direct ping bounds every relay's destination leg: a
        // relayed path is never shorter than the direct one it replaces. Only a
        // fresh measurement **of this server** may bound the ranking or support a
        // comparison: a stale ping describes a path the client is no longer
        // measuring, and a ping measured against another server describes a
        // different path entirely - flooring this candidate against it would
        // invent a bound (the live cross-server race, task-25).
        let direct_us = direct_us_for(self.server_ping, server);
        let (ranked, skipped, proven) = match server_location {
            Some(loc) => {
                let (candidates, skipped) =
                    without_starved(server, &candidates, &self.relay_history);
                (
                    lightspeed_client::rank_relays_at_with_direct(
                        server,
                        loc,
                        &candidates,
                        direct_us,
                    ),
                    skipped,
                    0,
                )
            }
            // Without an exact server position the region prior is a guess about
            // the half of the path the client cannot measure. That guess is what
            // put a G-Core Virginia server in Japan and then selected relay-sgp-1
            // for it, so the fallback ranks on what is real instead: a proven
            // carry record for this exact server first, then the measured client
            // leg, jitter, and only last the numerical relay address.
            None => {
                let FallbackRanking {
                    ranked,
                    skipped_starved,
                    proven,
                } = rank_without_destination(server, &candidates, direct_us, &self.relay_history);
                (ranked, skipped_starved, proven)
            }
        };
        if skipped > 0 {
            tracing::info!(
                server = %server,
                skipped,
                ranked = ranked.len(),
                "Relays on record as starving this server were skipped"
            );
        }
        if proven > 0 {
            tracing::info!(
                server = %server,
                proven,
                "No exact server position: preferring the relays whose record proves they carry \
                 this server over the untrusted region prior"
            );
        }
        let floored = ranked
            .iter()
            .filter(|scored| {
                matches!(
                    scored.dest_source,
                    lightspeed_client::DestinationLegSource::GeometricFloor
                )
            })
            .count();
        // The engine's order IS the product's order: `rank_relays_*` compares the
        // whole-path total (client leg + destination leg + the prior's
        // uncertainty), with the pre-floor prior, the measured client leg, and
        // jitter as tie-breaks, so there is no second ranking rule here to drift
        // from it. The counterfactuals that used to need an interim re-rank live
        // in the engine suite as
        // `relay_choice::tests::the_whole_path_total_outranks_the_destination_bucket`
        // and
        // `relay_choice::tests::a_floor_tie_with_equal_priors_is_broken_by_the_client_leg_not_the_address`.
        let best = ranked.first().cloned()?;
        let state = ranking_state_label(best.dest_source, best.relay, server);
        let claim = direct_claim_label(direct_us, best.total_leg_us);
        tracing::info!(
            "Destination-aware relay choice for {server}: {} ({state}, total {} ms{claim}) - \
             {floored} of {} candidates raised to the direct-ping floor",
            best.relay,
            best.total_leg_us / 1000,
            ranked.len()
        );
        let chosen = best.relay;
        // One record of the ranking, written here and read only by the route
        // surface: the server leg's provenance must not be derived twice.
        self.route_ranking = Some(RouteRanking {
            server,
            scored: ranked,
        });
        Some(chosen)
    }

    /// Measure every discovered relay's client leg on a background thread.
    ///
    /// The results feed the destination-aware choice, which needs all of them:
    /// ranking the whole path requires knowing both the relay that is closest to
    /// the player and the relay that is closest to the server, and those are
    /// usually different relays.
    fn start_relay_measurement(&mut self) {
        let addrs: Vec<SocketAddrV4> = self
            .proxies
            .iter()
            .map(|entry| discovery::health_endpoint(entry.addr))
            .collect();
        if addrs.is_empty() {
            return;
        }
        self.relay_measurement = Some(discovery::spawn_relay_measurement(addrs));
    }

    /// Collect a finished client-leg measurement, then choose for the destination.
    fn poll_relay_measurement(&mut self) {
        let samples = match &self.relay_measurement {
            Some(rx) => match rx.try_recv() {
                Ok(samples) => samples,
                Err(std::sync::mpsc::TryRecvError::Empty) => return,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Vec::new(),
            },
            None => return,
        };
        self.relay_measurement = None;

        // Translate the samples back to the relays they belong to before storing
        // them, so everything downstream can keep meaning "relay" by its data
        // address.
        self.relay_legs = Some(pair_relay_legs(&self.proxies, &samples));

        if !self.auto_select {
            return;
        }
        if let Some(addr) = self.choose_relay_for_destination() {
            self.destination_winner = Some(addr);
            if let Some(idx) = self.proxies.iter().position(|entry| entry.addr == addr) {
                if idx != self.selected_proxy_idx {
                    // The same rule as `follow_game_server`: a relay change moves
                    // the data plane, so it is only safe while tunnelling is
                    // withheld. A re-measurement during a live match records the
                    // better server-side relay for the next session instead of
                    // restarting the interceptor under the player. The no-reply
                    // watchdog no longer owns the route, so there is no chain to
                    // veto this switch.
                    let gate = lock_or_recover(&self.engine)
                        .interceptor_health()
                        .and_then(|health| health.tunnelling_enabled);
                    if !switch_is_safe(gate) {
                        let current = self
                            .proxies
                            .get(self.selected_proxy_idx)
                            .map(|entry| entry.addr)
                            .unwrap_or(addr);
                        tracing::info!(
                            "Keeping the live session on {current}; {addr} is better for this \
                             server and applies next session"
                        );
                        return;
                    }
                    tracing::info!("Selected {addr} for the current game server");
                    self.selected_proxy_idx = idx;
                    self.connect_selected();
                    self.persist_config();
                }
            }
        }
    }

    /// Re-select the relay whenever the game server changes.
    ///
    /// Relay selection needs a destination, and the destination only exists once
    /// interception has run and locked a game server. Without this, the app
    /// connects by the client-leg race (which chose Los Angeles for an Ashburn
    /// server) and would stay there for the whole match, because the initial
    /// choice happens before any server is known.
    ///
    /// The extraction-shooter case makes this a per-match event: a new raid is a
    /// new server, possibly in a new region, and the relay that was right for the
    /// last match is not necessarily right for this one.
    fn follow_game_server(&mut self) {
        if !self.status.interceptor_active {
            return;
        }
        let server = match self.game_server_addr() {
            Some(server) => server,
            None => return,
        };
        if self.chosen_for_server == Some(server) {
            return;
        }
        if !self.auto_select {
            // A pinned relay has no choice to make, but every new server gets a
            // fresh chance at the relay: a hold left over from the lobby backend
            // must not pin the whole session direct.
            self.chosen_for_server = Some(server);
            self.server_chosen.store(true, Ordering::Relaxed);
            self.held_since = None;
            if lock_or_recover(&self.engine).release_tunnelling() {
                tracing::info!(
                    "Pinned relay: re-arming the relay for the new game server {server}"
                );
            }
            return;
        }

        // The choice needs measured client legs for every relay; ask for them if
        // this is the first server of the session.
        if self.relay_legs.is_none() && self.relay_measurement.is_none() {
            self.start_relay_measurement();
            return;
        }

        let first_server = self.chosen_for_server.is_none();
        let held_now = lock_or_recover(&self.engine)
            .interceptor_health()
            .and_then(|health| health.tunnelling_enabled)
            == Some(false);

        // Nothing is flowing while the hold is up, so this is the only window in
        // which a wrong guess can be corrected for free. Wait for the address to
        // settle instead of committing to whatever locked first.
        if first_server && held_now {
            if self.locked_addr != Some(server) {
                self.locked_addr = Some(server);
                self.locked_since = Some(std::time::Instant::now());
                tracing::info!(
                    "Waiting for the game server to settle before choosing a relay ({server})"
                );
                return;
            }
            if !self
                .locked_since
                .is_some_and(|since| since.elapsed() >= LOCK_SETTLE)
            {
                return;
            }
        }

        self.chosen_for_server = Some(server);
        self.server_chosen.store(true, Ordering::Relaxed);
        self.locked_addr = None;
        self.locked_since = None;
        let chosen = self.choose_relay_for_destination();
        let Some(addr) = chosen else {
            return;
        };
        if let Some(idx) = self.proxies.iter().position(|entry| entry.addr == addr) {
            if idx != self.selected_proxy_idx {
                // The gate, not the GUI's stale hold flag, says whether game
                // traffic is actually flowing: the engine's release watcher can
                // end a hold without this thread seeing a frame.
                let held = lock_or_recover(&self.engine)
                    .interceptor_health()
                    .and_then(|health| health.tunnelling_enabled)
                    == Some(false);
                if first_server && held {
                    // No game traffic has been released yet: switching now is
                    // safe, and the new interceptor will carry the first packet
                    // through the chosen relay.
                    tracing::info!("Following the game server: switching to {addr}");
                    self.selected_proxy_idx = idx;
                    self.connect_selected();
                    self.persist_config();
                } else {
                    // A live session is already on a relay. Restarting the
                    // interceptor to switch drops the game's join for seconds,
                    // which is worse than keeping a usable relay. The choice
                    // applies to the next session instead.
                    let current = self
                        .proxies
                        .get(self.selected_proxy_idx)
                        .map(|entry| entry.addr)
                        .unwrap_or(addr);
                    tracing::info!(
                        "Keeping the live session on {current}; {addr} is better for this server and applies next session"
                    );
                }
            }
        }

        // The relay for this server is settled, whether or not it changed: the
        // hold has served its purpose and traffic may now flow through it.
        self.held_since = None;
        if lock_or_recover(&self.engine).release_tunnelling() {
            tracing::info!("Relay chosen for {server}; releasing the hold and relaying");
        }
    }

    /// Give up on a hold that has not found a game server, rather than idle.
    ///
    /// A hold with no deadline is a silent failure: a game the profiles do not
    /// know, or one whose ports we do not match, would leave the app relaying
    /// nothing and saying nothing. Falling back keeps the app doing what it did
    /// before the hold existed, and a server that locks later is still routed to
    /// the right relay.
    fn release_stale_hold(&mut self) {
        let Some(since) = self.held_since else {
            return;
        };
        if since.elapsed() < HOLD_FALLBACK {
            return;
        }
        self.held_since = None;
        if lock_or_recover(&self.engine).release_tunnelling() {
            tracing::warn!(
                "Could not identify the game server within {}s; relaying through the nearest \
                 relay instead. Traffic stays direct until then.",
                HOLD_FALLBACK.as_secs()
            );
        }
    }

    /// The live game server the interceptor is talking to, if any.
    fn game_server_addr(&self) -> Option<Ipv4Addr> {
        let raw = self.status.interceptor_server.as_str();
        let host = raw.split(':').next()?;
        host.parse::<Ipv4Addr>().ok()
    }

    /// The game server the interceptor has locked, whichever mode is running.
    ///
    /// This mirrors the route card's own address extraction so the client trace
    /// and the card can never describe different servers: the card reads the
    /// interceptor, redirect and WinDivert fields, and `game_server_addr` reads
    /// only the interceptor, which is the wrong subject for a location probe
    /// when the session is carried by another mode.
    fn locked_server_ip(&self) -> Option<Ipv4Addr> {
        [
            self.status.interceptor_server.as_str(),
            self.status.redirect_server.as_str(),
            self.status.windivert_server.as_str(),
        ]
        .into_iter()
        .find_map(|server| server.split(':').next()?.parse::<Ipv4Addr>().ok())
    }

    /// Ask the client-traceroute scheduler for the locked server's route and
    /// log the one line each accepted trace produces.
    ///
    /// Never blocks the UI thread: the OS tool runs on its own worker thread, a
    /// finished route is cached, and the scheduler starts at most one run at a
    /// time. The session argument is the engine's keepalive generation, which is
    /// a new id every time a relay connection is established, so a failed trace
    /// is retried for a new session rather than on every frame.
    fn poll_client_trace(&mut self) {
        let server = self.locked_server_ip();
        let tick = self.client_trace.update(
            server,
            self.status.keepalive_generation,
            std::time::Instant::now(),
        );
        if let Some(accepted) = tick.accepted {
            tracing::info!(
                "{}",
                crate::client_trace::info_line(
                    accepted.server,
                    &accepted.route,
                    crate::client_trace::now_unix_ms(),
                )
            );
        }
        self.client_route = tick.route;
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

        // Once a relay has been chosen for a known game server, that choice
        // governs. The race ranks the client leg alone, which is the half of the
        // path that put a live match on a relay 17,000 km from its server, and it
        // runs again on a timer - so without this guard it would keep dragging the
        // relay back to the client-leg winner. The race still runs so its samples
        // stay fresh; only its winner is declined.
        if self.chosen_for_server.is_some() {
            tracing::debug!(
                "Declining the client-leg race winner; the relay is chosen for the game server"
            );
            return;
        }

        if self.auto_select && self.status.connected {
            self.consider_connected_winner(winner);
            return;
        }

        if !should_apply_race(
            self.auto_select,
            self.status.connected,
            self.game_server_addr().is_some(),
        ) {
            return;
        }
        let Some(addr) = winner else {
            tracing::warn!("Relay latency race found no reachable relay");
            return;
        };
        let Some(idx) = proxy_index_for(&self.proxies, addr) else {
            tracing::warn!("Relay race winner {addr} is not in the proxy list; ignoring it");
            return;
        };
        if idx != self.selected_proxy_idx {
            tracing::info!("Auto-selected the fastest relay {}", self.proxies[idx].addr);
            self.selected_proxy_idx = idx;
        }
        self.connect_selected();
        self.persist_config();
    }

    /// A re-race finished while connected: time the winner before letting it
    /// displace the relay in use, so a marginal win cannot flap the tunnel.
    fn consider_connected_winner(&mut self, winner: Option<SocketAddrV4>) {
        let Some(addr) = winner else {
            tracing::warn!("Relay latency race found no reachable relay");
            return;
        };
        if self
            .selected_proxy_addr()
            .is_some_and(|current| same_relay_endpoint(current, addr))
        {
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
        let Some(idx) = proxy_index_for(&self.proxies, addr) else {
            tracing::warn!("Re-race winner {addr} is not in the proxy list; ignoring it");
            return;
        };
        tracing::info!(
            "Auto-switched to the faster relay {} ({winner_rtt_ms} ms)",
            self.proxies[idx].addr
        );
        self.selected_proxy_idx = idx;
        self.connect_selected();
        self.persist_config();
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
                self.start_relay_measurement();
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
        // Stop the background watcher first and wait for it: a presence scan is
        // two process spawns, and the app must not go away while one runs.
        self.presence.shutdown();
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

    /// Start the interceptor boost for the selected game and relay, for the
    /// picked process when there is one.
    ///
    /// A picked process is the interception target and beats every port range:
    /// the engine binds it by image name against the live process table at start
    /// time. With no target the built-in Game path decides, exactly as before.
    fn start_boost(&mut self) {
        // The persisted "Custom Port Range" actuates here: a well-formed range
        // wins over the profile's own, and a blank or malformed one parses to
        // `None` so the profile default stands. A typo never panics the UI
        // thread and never silently drops a good range.
        let port_range_override = parse_custom_port_range(&self.custom_port_input);
        let process_target = self.target_process.clone();

        if let Some(proxy) = self.selected_proxy_addr() {
            let game_key = self.selected_game().key;
            let mut engine = lock_or_recover(&self.engine);
            engine.set_telemetry_game(&game_key);
            let result = engine.start_interceptor_with_target(
                &game_key,
                proxy,
                self.fec_enabled,
                4, // default FEC K
                port_range_override,
                process_target,
            );
            if let Err(e) = result {
                tracing::error!("start_interceptor failed: {}", e);
            } else {
                self.boost_start = Some(std::time::Instant::now());
                self.relayed_base = None;

                // Hold traffic direct until a relay has been chosen for the game
                // server. This is the consumer-simple promise: never route a
                // player through a relay picked before anyone knew where their
                // server was. In the field that mistake tunnelled a match on a
                // Sao Paulo server through a relay in Los Angeles, and the
                // client's connect timed out.
                //
                // The interceptor still captures and attributes while held, so
                // the destination is discovered and the hold can be released;
                // holding never makes the server unknowable.
                let held = engine.hold_tunnelling();
                if held {
                    self.held_since = Some(std::time::Instant::now());
                    tracing::info!(
                        "Holding traffic on the direct path until the game server is known"
                    );
                    // The relay connected for this game can arrive on a frame the
                    // GUI's own release never sees, so the engine ends the hold
                    // itself the moment that happens. Nothing changes for a run
                    // that does find its server: `follow_game_server` releases
                    // first, and this only stops the watcher.
                    //
                    // The engine guard is dropped before the watcher is armed
                    // again: arming spawns a task on the engine runtime, and
                    // holding this mutex across that spawn is what stopped the
                    // arming call from ever returning in a live run.
                    let status = engine.status_watch();
                    let chosen = Arc::clone(&self.server_chosen);
                    drop(engine);
                    let armed =
                        lock_or_recover(&self.engine).release_hold_when_connected(move || {
                            chosen.load(Ordering::Relaxed)
                                && status.read().map(|s| s.connected).unwrap_or(false)
                        });
                    if !armed {
                        tracing::warn!("No release watcher: this run is not held");
                    }
                } else {
                    tracing::debug!("This backend has no tunnelling gate; starting unheld");
                }
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
        self.relayed_base = None;
        self.health_base = None;
        self.relay_health = None;
    }

    /// Record what each path is doing, every ten seconds.
    ///
    /// The geometry says a relay cannot reduce round trip, so the only thing it
    /// can offer is steadiness. That is a measurable claim: direct ping against
    /// the client-to-relay leg, and each one's spread.
    fn log_latency_comparison(&mut self) {
        if !self.status.connected {
            return;
        }
        let due = self
            .compared_at
            .is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(10));
        if !due {
            return;
        }
        self.compared_at = Some(std::time::Instant::now());
        let relay_spread = {
            let mut history = self.status.rtt_history.clone();
            history.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            if history.len() >= 4 {
                let lo = history[history.len() / 10];
                let hi = history[history.len() * 9 / 10];
                Some(hi - lo)
            } else {
                None
            }
        };
        let server = self
            .status
            .interceptor_server
            .split(':')
            .next()
            .and_then(|ip| ip.parse::<std::net::Ipv4Addr>().ok());
        tracing::info!(
            direct_ms = server
                .and_then(|server| self.server_ping_for(server))
                .map(|ping| ping.ms),
            direct_jitter_ms = server.map(lightspeed_client::latency::direct_jitter),
            shadow_jitter_ms = lightspeed_client::latency::shadow_direct_jitter_ms(),
            shadow_samples = lightspeed_client::latency::shadow_direct_samples(),
            shadow_app_ms = lightspeed_client::latency::shadow_direct_p50_ms(),
            relay_samples = lightspeed_client::latency::relayed_samples(),
            relay_app_ms = lightspeed_client::latency::relayed_p50_ms(),
            relay_ms = self.status.latest_rtt_ms,
            relay_spread_ms = relay_spread,
            relayed_jitter_ms = lightspeed_client::latency::relayed_jitter_ms(),
            like_for_like = ?lightspeed_client::latency::like_for_like().map(|c| (
                c.direct_app_p50_ms,
                c.relay_app_p50_ms,
                c.shadow_samples,
                c.relay_samples,
            )),
            relay = %self.status.proxy_addr,
            "Latency comparison"
        );
    }

    /// Write the watchdog's starvation finding for a relay and server into the
    /// relay history, so the next ranking already excludes that path.
    ///
    /// The watchdog thread cannot touch the history (the GUI owns it), so it
    /// publishes the finding and this poll writes it. The record is the
    /// starvation signature, not an exhausted-chain `failed` verdict: the
    /// warn-only mode has no chain to exhaust, because it never retries and
    /// never switches paths. The session keeps relaying while the finding is
    /// written; only a future ranking changes.
    fn record_starvation_verdict(&mut self) {
        let Some((server, published_relay)) = lock_or_recover(&self.relay_starved).take() else {
            return;
        };
        let Some(relay) = published_relay.or_else(|| {
            lightspeed_client::current_relay().or_else(|| {
                self.status
                    .proxy_addr
                    .parse::<std::net::SocketAddrV4>()
                    .ok()
            })
        }) else {
            return;
        };
        apply_starvation_verdict(&mut self.relay_history, server, relay);
        let _ = self.relay_history.save(&crate::paths::relay_history_file());
        tracing::info!(
            relay = %relay,
            server = %server,
            "The return-path watchdog found no reply for this server; the relay is demoted \
             for the next ranking. The session keeps relaying and the player has been warned"
        );
    }

    fn record_relay_history(&mut self) {
        if !(self.status.interceptor_active || self.status.redirect_active) {
            self.history_counters = None;
            return;
        }
        // The interceptor's relay, not the one the window shows: those are set at
        // different moments, and keying on the wrong one attributed EWR's traffic
        // to LAX - LAX's record for one server jumped from 55 to 592 forwards
        // while EWR was carrying it.
        let Some(relay) = lightspeed_client::current_relay().or_else(|| {
            self.status
                .proxy_addr
                .parse::<std::net::SocketAddrV4>()
                .ok()
        }) else {
            return;
        };
        let Some(server) = self
            .status
            .interceptor_server
            .split(':')
            .next()
            .and_then(|ip| ip.parse::<std::net::Ipv4Addr>().ok())
        else {
            return;
        };
        // Per-destination counters, not the relay-wide ones: those also count
        // keepalives and parity frames and are cumulative across relay switches,
        // which produced a 112% reply share on a live session. A verdict rests on
        // these numbers, so they have to be the server's own.
        let Some((_, forwarded, returned)) = self
            .status
            .interceptor_flows
            .iter()
            .find(|(addr, _, _)| *addr == server)
            .copied()
        else {
            self.history_counters = None;
            return;
        };
        if let Some((last_relay, last_forwarded, last_returned)) = self.history_counters {
            // A relay switch mid-interval would attribute one relay's traffic to
            // the next; a fresh baseline costs one interval and cannot lie.
            if last_relay == relay {
                if let Some((forwards, replies)) =
                    history_delta((last_forwarded, last_returned), (forwarded, returned))
                {
                    self.relay_history.record(relay, server, forwards, replies);
                }
                let due = self
                    .history_saved_at
                    .is_none_or(|at| at.elapsed() >= std::time::Duration::from_secs(30));
                if due {
                    self.history_saved_at = Some(std::time::Instant::now());
                    let _ = self.relay_history.save(&crate::paths::relay_history_file());
                }
            }
        }
        self.history_counters = Some((relay, forwarded, returned));
    }

    /// Read the measured direct ping to the game server, and start a burst when
    /// the figure is missing or old.
    ///
    /// The burst is ICMP echo, which is the only latency the client can measure
    /// with real correlation: the reply carries the request's sequence. It runs on
    /// a worker thread because five probes with timeouts block for seconds.
    fn refresh_server_ping(&mut self, server: std::net::Ipv4Addr) {
        let sample = lightspeed_client::latency::direct_sample(server);
        let stale = sample.is_none_or(|(_, age)| age > PING_REFRESH_AFTER);
        // The record is stamped with the server it was measured for and replaced
        // wholesale, so a poll for a new server can never leave the previous
        // server's value looking current.
        self.server_ping =
            refreshed_server_ping(server, sample.map(|(ms, age)| (ms, age.as_secs())));
        if !stale {
            return;
        }
        let flag = std::sync::Arc::clone(&self.ping_in_flight);
        if flag.swap(true, std::sync::atomic::Ordering::AcqRel) {
            return;
        }
        std::thread::spawn(move || {
            let _ = lightspeed_client::latency::probe_direct(server);
            flag.store(false, std::sync::atomic::Ordering::Release);
        });
    }

    /// The direct-ping record, only while it belongs to `server`.
    ///
    /// Every consumer that describes a specific server's path asks through here:
    /// a sample measured against another server is not evidence about this one.
    /// This is the whole fix for the cross-server floor race observed live
    /// (task-24 §4): `follow_game_server` runs before `poll_relay_health`, so for
    /// one frame the stored record can still describe the previous server, and
    /// the ranking must read that as "direct ping not measured", not as this
    /// server's floor.
    fn server_ping_for(&self, server: Ipv4Addr) -> Option<ServerPing> {
        ping_for_server(self.server_ping, server)
    }

    /// What identifies the run the Relayed counters describe: a new relay or a new
    /// game server is a new session, and the row starts counting again.
    fn session_key(&self) -> String {
        format!(
            "{}|{}|{}",
            self.status.proxy_addr, self.status.interceptor_server, self.status.redirect_server
        )
    }

    fn poll_relay_health(&mut self) {
        let health = lock_or_recover(&self.engine).interceptor_health();
        // The only hold left is the start-of-match wait for a relay choice. The
        // watchdog no longer takes the session direct, so a held tunnel with a
        // locked server is "choosing a relay", never a failed route.
        self.holding = health
            .as_ref()
            .map(|health| {
                health.tunnelling_enabled == Some(false) && health.detected_server.is_some()
            })
            .unwrap_or(false);
        // The watchdog publishes its warning from its own thread; the window is
        // the consumer that shows it. Reading the slot is what keeps the tray and
        // the card in step with the detection that produced it.
        self.starvation_warning = *lock_or_recover(&self.relay_warning);
        if let Some(server) = health.as_ref().and_then(|health| health.detected_server) {
            self.refresh_server_ping(*server.ip());
        }
        self.log_latency_comparison();
        self.record_relay_history();
        // The finding is written into the relay history, which demotes the relay
        // for the next ranking. The session keeps relaying: the warning, not a
        // route change, is how the user hears about it.
        self.record_starvation_verdict();
        if self.status.interceptor_active || self.status.redirect_active {
            let key = self.session_key();
            let stale = self
                .relayed_base
                .as_ref()
                .is_none_or(|(base_key, _, _)| *base_key != key);
            if stale {
                if let (Some(sent), Some(returned)) = (
                    self.session_sent(),
                    returned_count(
                        self.status.redirect_active,
                        self.status.redirect_pkts_in,
                        self.status.interceptor_active,
                        self.status.interceptor_from_proxy,
                    ),
                ) {
                    self.relayed_base = Some((key, sent, returned));
                }
            }
        }
        let Some(started) = self.boost_start else {
            return;
        };
        let base = match self.health_base {
            Some(base) => base,
            None => {
                let base = RelayHealthBase {
                    at: std::time::Instant::now(),
                    intercepted: self.status.interceptor_intercepted,
                    sent: self.status.interceptor_tunnel_sent,
                };
                self.health_base = Some(base);
                return;
            }
        };
        if started.elapsed() < RELAY_HEALTH_SETTLE {
            return;
        }
        if base.at.elapsed() < RELAY_HEALTH_WINDOW {
            return;
        }
        let decision = relay_health_verdict(
            base.intercepted,
            base.sent,
            self.status.interceptor_intercepted,
            self.status.interceptor_tunnel_sent,
        );
        self.health_base = Some(RelayHealthBase {
            at: std::time::Instant::now(),
            intercepted: self.status.interceptor_intercepted,
            sent: self.status.interceptor_tunnel_sent,
        });
        let verdict = match decision {
            RelayHealthVerdict::Idle => return,
            other => other,
        };
        if self.relay_health != Some(verdict) {
            self.relay_health = Some(verdict);
            tracing::info!("Relay verdict: {}", verdict.sentence());
        }
    }

    /// Relaunch with elevated privileges so the interceptor can start.
    fn restart_elevated(&mut self) {
        P::relaunch_as_admin();
    }

    /// Apply the background game-presence watcher's latest result.
    ///
    /// This is what removes the button press the user has to remember: with
    /// `game_auto` on, relaying starts when a known game shows up and stops when
    /// it leaves, so a match is optimized from its first packet rather than from
    /// whichever moment the user noticed.
    ///
    /// The scan itself - two process spawns that block for 0.4-3.4 s - runs on
    /// the watcher thread (see `app::presence`); this method only tells it which
    /// mode the Game field is in and applies whatever finished.
    ///
    /// A manual game pick or a manual process target disables the arm entirely:
    /// the user has expressed an intent that an automatic scan must not override.
    fn poll_game_presence(&mut self) {
        // Auto is the only scanning mode. Manual / Custom hands routing to the
        // typed server, and a pinned game or process target is an explicit
        // choice, so "not allowed" also means "never auto-start from a scan".
        let auto_mode = self.game_auto && !self.game_manual;
        let allowed = presence_scan_allowed(
            self.game_auto,
            self.game_manual,
            self.target_process.is_some(),
        );
        self.presence.set_enabled(allowed);
        if !allowed {
            // Drop a result produced before the switch, so leaving Auto can
            // never be followed by an auto-start on a stale scan.
            self.presence.discard_pending();
            if !auto_mode {
                self.auto_detected_game = None;
                self.auto_game_present = false;
            }
            return;
        }

        // The worker owns the 5 s cadence and keeps one scan in flight, so this
        // never waits; a frame without a result keeps the previous state.
        let Some(detected) = self.presence.latest() else {
            return;
        };
        let present = detected.is_some();
        let changed = detected != self.auto_detected_game;
        self.auto_detected_game = detected;
        self.auto_game_present = present;

        if changed {
            tracing::info!(
                "Game presence changed: {}",
                if present {
                    self.auto_detected_game.as_deref().unwrap_or("unknown")
                } else {
                    "none"
                }
            );
        }

        // Start once per game presence, not on every tick. Verified live: an
        // earlier version acted only on a presence *transition*, which fired
        // before the relay was connected and then never retried; removing the
        // transition gate overshot the other way and restarted the tunnel
        // repeatedly (`Racing 8 relays` every few seconds). The latch below is
        // the middle: it retries until the start succeeds, then holds until the
        // game actually leaves.
        if present {
            if self.auto_started_for_presence {
                return;
            }
            if self.status.interceptor_active {
                self.auto_started_for_presence = true;
                return;
            }
            // Zero clicks means the relay leg needs no press either, so the
            // auto-start path connects when nothing is connected yet. A user
            // who pinned a relay (`auto_select` off) has made the call already,
            // so their pick is connected directly and never re-raced. Both
            // happen at most once per presence: re-connecting each tick would
            // reset the control-plane link while the race is still in flight.
            if !self.auto_started_for_presence
                && should_race(self.auto_select, self.status.connected, self.proxies.len())
            {
                tracing::info!(
                    "Game present and no relay connected; connecting one before interception"
                );
                self.start_relay_race();
                self.auto_started_for_presence = true;
            } else if !self.auto_started_for_presence
                && self.selected_proxy_addr().is_some()
                && !self.status.connected
            {
                tracing::info!("Game present; connecting the pinned relay before interception");
                self.connect_selected();
                self.persist_config();
                self.auto_started_for_presence = true;
            }
            if self.status.connected {
                tracing::info!(
                    "Game present and relay connected; starting interception for {}",
                    self.auto_detected_game.as_deref().unwrap_or("unknown")
                );
                self.start_boost();
                // Latch regardless of the result. A start that failed because the
                // game is mid-launch would otherwise retry forever, and a start
                // that succeeded must not repeat; either way the tunnel is now
                // the engine's to manage until the game leaves.
                self.auto_started_for_presence = true;
            }
        } else if self.status.interceptor_active {
            tracing::info!("Game exited; stopping the relay it was using");
            self.stop_boost();
            self.auto_started_for_presence = false;
        } else {
            self.auto_started_for_presence = false;
        }
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
        // The state word describes the route, not the intent. The live ROB RUN
        // run read "OPTIMIZING" in green while the ledger underneath said the
        // relay had returned nothing; the tray must not repeat that lie. The
        // warning outranks OPTIMIZING while the return path is failing, and a
        // hold is the normal start-of-match wait for a relay choice, never a
        // route that failed.
        let warning = self.starvation_warning.filter(|_| boosting);
        let (word, word_color) = if boosting && warning.is_some() {
            (i18n::t("state.not_returning").to_uppercase(), danger)
        } else if boosting && self.holding {
            (i18n::t("state.holding").to_uppercase(), warn)
        } else if boosting {
            (i18n::t("state.optimizing").to_uppercase(), signal)
        } else if self.status.connected {
            (i18n::t("state.connected").to_uppercase(), text_1)
        } else {
            (i18n::t("state.idle").to_uppercase(), text_1)
        };

        let sub_line = if boosting && self.holding {
            i18n::t("route.choosing").into_owned()
        } else if boosting {
            // The route stays named while the warning is up: the player is still
            // relayed through this relay, and the route card carries the warning
            // and the remedy.
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
            // The headline is the player's ping, which is the whole loop through
            // the relay when a quiet-flow probe has measured it; the relay leg is
            // only what is left to show when nothing has been attributed yet.
            // The headline is the game-server ping when a direct measurement is
            // fresh: that is the figure a player compares with the one their game
            // shows, and ICMP is the only latency here with real correlation.
            // Otherwise the relay leg, which is always measured. Freshness is one
            // rule ([`SERVER_PING_FRESH_SECS`]), shared with the ranking's floor
            // and the triangle verdict; the ping is also keyed to the server it
            // measured (task-25), so a stale record from a previous server is
            // never shown as this server's ping.
            let direct_ping = self
                .game_server_addr()
                .and_then(|server| self.server_ping_for(server));
            let (reading, hint) = match direct_ping {
                Some(ping) if fresh_direct_us(Some((ping.ms, ping.age_secs))).is_some() => {
                    (ping.ms as f64, i18n::t("route.server_ping_hint"))
                }
                _ => (self.status.latest_rtt_ms, i18n::t("route.rtt_hint")),
            };
            let text = if reading > 0.0 {
                format!("{reading:>3.0} ms")
            } else {
                "-- ms".to_string()
            };
            let color = if reading > 0.0 {
                rtt_colour(reading)
            } else {
                text_3
            };
            Some((text, color, hint))
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
                        .size(if narrow { STATE * 0.82 } else { STATE })
                        .family(semibold())
                        .color(word_color),
                );
                if let Some((text, color, hint)) = &metric {
                    {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.label(
                                egui::RichText::new(text.as_str())
                                    .size(METRIC)
                                    .family(egui::FontFamily::Monospace)
                                    .color(*color),
                            )
                            .on_hover_text(hint.clone());
                        });
                    }
                }
            },
        );

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
                });
            }
        }
        let centre = active_node
            .as_deref()
            .and_then(globe::relay_coords)
            .unwrap_or((30.0, 0.0));

        // The game server comes from whichever mode is running; its address is
        // "ip:port", and only an IPv4 literal geolocates. The same helper feeds
        // the client trace, so the caption and the dot cannot describe different
        // servers.
        let server_ip = self.locked_server_ip();
        let server_place = server_ip.map(crate::server_place::place);
        let server_at = server_ip.and_then(crate::server_place::display_point);
        if let Some(at) = server_at {
            let label = match server_place {
                Some(crate::server_place::Place::Cloud {
                    provider, region, ..
                }) => format!("{} {region}", provider.to_uppercase()),
                Some(crate::server_place::Place::Country { code, .. }) => code.to_string(),
                _ => String::new(),
            };
            markers.push(globe::Marker {
                at,
                colour: warn,
                label,
            });
        }
        let route = server_at.map(|to| (centre, to));
        // What the card may say about the server's place, and the source that
        // backs it. This is the honest ladder: an exact provider region, a
        // hosting network whose geoIP country is not trustworthy, the country
        // estimate, or nothing. Only the cloud and country cases have a point.
        let location_caption = server_place.map(route_location_lines);

        egui::Frame::new()
            .fill(bg_2)
            .stroke(egui::Stroke::new(1.0, border_1))
            .corner_radius(egui::CornerRadius::same(R_CARD))
            .inner_margin(egui::Margin::same(S3 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                // The unmissable warning: the relay is carrying the session but
                // has been judged not to be returning the game's traffic. It is
                // drawn above the map, in the danger tint, because the old
                // surface showed a green OPTIMIZING rail while the ledger said
                // the relay returned nothing. The banner names the remedy - the
                // player's own decision - and never claims a route change.
                if let Some(warning) = self.starvation_warning.filter(|_| self.run_boosting()) {
                    egui::Frame::new()
                        .fill(danger_soft)
                        .stroke(egui::Stroke::new(1.0, danger))
                        .corner_radius(egui::CornerRadius::same(R_INLINE))
                        .inner_margin(egui::Margin::same(S2 as i8))
                        .show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.label(
                                egui::RichText::new(i18n::t(warning_message_key(warning.fault)))
                                    .size(BODY)
                                    .family(semibold())
                                    .color(danger),
                            );
                        });
                    ui.add_space(S2);
                }
                ui.vertical_centered(|ui| {
                    // A flat map instead of the globe: legible at a glance and it
                    // spends the card's width rather than its height.
                    let map_w = ui.available_width().min(430.0);
                    globe::draw_map(ui, map_w, 132.0, &markers, route);
                    // The dot's position comes from free databases, not from a
                    // measurement: say which source backs it under the map
                    // instead of letting it read as a verified location. Hosting
                    // ranges are the documented weak spot (a G-Core US-Virginia
                    // server publishes as JP in the shipped DB-IP data), so such
                    // a server gets no dot at all - just its network name.
                    if let Some(lines) = &location_caption {
                        ui.add_space(S1);
                        ui.label(
                            egui::RichText::new(lines.caption.as_str())
                                .size(CAPTION)
                                .color(text_3),
                        );
                        if let Some(detail) = &lines.detail {
                            ui.label(
                                egui::RichText::new(detail.as_str())
                                    .size(CAPTION)
                                    .color(text_3),
                            );
                        }
                        // An operator geofeed row is the operator's own
                        // statement, so the card says it is shown as-is and
                        // keeps the feed URL on hover as the provenance.
                        if let Some(note) = &lines.note {
                            let response = ui.label(
                                egui::RichText::new(note.as_str())
                                    .size(CAPTION)
                                    .color(text_3),
                            );
                            if let Some(source) = &lines.source {
                                response.on_hover_text(i18n::t_with(
                                    "route.location_geofeed_source",
                                    &[("source", source)],
                                ));
                            }
                        }
                        // A hosting-range or unknown server has no dot, so the
                        // card says why: free IP databases are the documented
                        // weak spot for exactly these ranges. It is a caption
                        // line, not a tooltip, because the absence of a location
                        // is the fact the reader needs.
                        if let Some(hint) = &lines.hint {
                            ui.label(
                                egui::RichText::new(hint.as_str())
                                    .size(CAPTION)
                                    .color(text_3),
                            );
                        }
                    }
                    // The client's own trace reaches the game server directly,
                    // so it locates the *server*, not the relay: the last hop
                    // that answered is the closest point to it this host can
                    // observe, and its RTT is not the game's ping. The caption
                    // states only that, plus a provider name when the shipped R1
                    // indexes know the prefix. A city would need hop-geolocation
                    // data the product does not have, so none is claimed.
                    if let Some(caption) = self
                        .client_route
                        .as_ref()
                        .and_then(crate::client_trace::caption)
                    {
                        ui.add_space(S1);
                        ui.label(
                            egui::RichText::new(caption.as_str())
                                .size(CAPTION)
                                .color(text_3),
                        )
                        .on_hover_text(
                            "The last hop that answered the client's own traceroute to the \
                             game server: a location hint, not a measurement of the server's \
                             game RTT.",
                        );
                    }
                });
                ui.add_space(S2);
                self.ledger_rows(ui, narrow, false);
                // The one whole-path claim the client can prove from its own
                // measurements: when the one-RTT client leg already reaches the
                // direct ping, adding the relay hop cannot shorten the path. It
                // is stated only while both figures are fresh, and never as a
                // latency verdict on the server leg, which the client cannot see.
                if let Some((client_leg_us, direct_us)) = self.cannot_beat_direct() {
                    ui.add_space(S2);
                    ui.label(
                        egui::RichText::new(i18n::t("route.cannot_beat_direct"))
                            .size(CAPTION)
                            .color(warn),
                    )
                    .on_hover_text(format!(
                        "Client leg {:.0} ms to the relay is at least the direct ping {:.0} ms, \
                         so a path through this relay cannot be shorter than going direct.",
                        client_leg_us as f64 / 1000.0,
                        direct_us as f64 / 1000.0
                    ));
                }
                // A chart of nothing is worse than no chart: while the client is
                // merely connected, the sparkline draws a flat line across an empty
                // band and reads as data. It is drawn only while a session is
                // actually relaying.
                let relaying = self.status.interceptor_active || self.status.redirect_active;
                if relaying && !self.status.rtt_history.is_empty() {
                    ui.add_space(S2);
                    sparkline(ui, &self.status.rtt_history);
                }
            });
    }

    /// The label/value ledger. At the short-height breakpoint the `Game server`
    /// row is dropped and the values move into the scrolling region. The
    /// `Target` row is drawn only while a process is targeted, so the ledger is
    /// unchanged on the built-in path.
    fn ledger_rows(&self, ui: &mut egui::Ui, narrow: bool, short_mode: bool) {
        let relay = self.relay_value();
        self.ledger_row(ui, narrow, &i18n::t("route.relay"), &relay);
        // The hero number is the keepalive round trip to the relay node, not the
        // game's own ping, and only a hover said so. Naming it in the card makes
        // the difference legible without pointing at it.
        // The whole-loop figure, measured only when the flow went quiet enough
        // for a reply to be attributable to one send. It carries its own age,
        // because the relay leg beside it is instantaneous and swings: a stale
        // probe can legitimately read below the current leg without either
        // number being wrong.
        // The direct ping is shown only for the server it was measured against;
        // the age is part of the value so staleness is visible without a second
        // rule. A record for a previous server is not this server's ping
        // (task-25), so it is hidden rather than relabelled.
        if let Some(ping) = self
            .game_server_addr()
            .and_then(|server| self.server_ping_for(server))
        {
            let value = LedgerValue::Data(format!("{:.0} ms \u{00b7} {}s", ping.ms, ping.age_secs));
            self.ledger_row(ui, narrow, &i18n::t("route.server_ping"), &value);
        }
        // The relay-to-server leg is the half of the path LightSpeed cannot see
        // without a measurement, and the surface used to present it as if it were
        // known. The relay's own ICMP measurement is the strongest evidence
        // available, so it leads: the row states the measured leg and, beside it,
        // the only whole-path figure the two measured legs support (`L + S`).
        // Only while this route is the relay actually carrying traffic.
        if let Some((leg, expected)) = self.relay_measured_server_leg_rows() {
            self.ledger_row(ui, narrow, &i18n::t("route.server_leg"), &leg);
            if let Some(expected) = expected {
                self.ledger_row(ui, narrow, &i18n::t("route.expected_path"), &expected);
            }
        } else if let Some(value) = self.server_leg_value() {
            // The row states how the choice obtained the leg - measured with its
            // sample count, a triangle bound, or the region prior with its
            // uncertainty - and appears only while that choice is the route
            // actually carrying traffic.
            self.ledger_row(ui, narrow, &i18n::t("route.server_leg"), &value);
        }
        // The relay's passive app-RTT sample rides beside the ICMP `S` as a
        // secondary, display-only figure. It is available even when ICMP is
        // blocked (ordinary game responses still time it), which is exactly why
        // the F4 revision uses it as the relay-side peer reference; it never
        // feeds the ranking and nothing is drawn when no fresh sample exists.
        if let Some(app) = self.relay_measured_app_server_leg_row() {
            self.ledger_row(ui, narrow, &i18n::t("route.server_leg_app"), &app);
        }
        // The relay's TTL traceroute, when it has one for this pair: a
        // placement anchor for a hosting-network server whose geolocation is
        // unknown. The row names the hop count and the last router's RTT only;
        // it claims no city and feeds no ranking decision.
        if let Some(trace) = self.relay_measured_trace_row() {
            self.ledger_row(ui, narrow, &i18n::t("route.route_trace"), &trace);
        }
        if self.status.connected && self.status.latest_rtt_ms > 0.0 {
            let value = LedgerValue::Data(format!("{:.0} ms", self.status.latest_rtt_ms));
            self.ledger_row(ui, narrow, &i18n::t("route.relay_rtt"), &value);
        }
        // The path row exists only for the start-of-match hold now: the watchdog
        // never takes the session direct, so there is no fallback state to name.
        if self.holding {
            self.ledger_row(
                ui,
                narrow,
                &i18n::t("route.path"),
                &LedgerValue::Warn(i18n::t("state.holding")),
            );
        }
        if let Some(target) = &self.target_process {
            let target = LedgerValue::Data(target.describe());
            self.ledger_row(ui, narrow, &i18n::t("route.target"), &target);
        }
        if !short_mode {
            let game_server = self.game_server_value();
            self.ledger_row(ui, narrow, &i18n::t("route.game_server"), &game_server);
        }
        let routed = self.routed_value();
        self.ledger_row(ui, narrow, &i18n::t("route.relayed"), &routed);
    }

    /// The relay-measured server-leg rows for the route on screen.
    ///
    /// The relay measures the relay→server leg (`S`) with unprivileged ICMP and
    /// reports it over the control plane; the client validates it before this
    /// crate ever sees it and says whether the triangle-floor check actually
    /// ran, so a fail-open report is labelled `measured, unvalidated`. `L` is
    /// the one-RTT client→relay keepalive already displayed as `Relay RTT`, so
    /// `L + S` is the first whole-path figure whose both terms are measured. It
    /// is a floor, not the real path, and the locale string says so with `~`.
    ///
    /// Present only while this client is tunnelling through the relay (a held
    /// session's own row already says the relay is not in use) and only for the
    /// pair the card is describing: the report is keyed by `(relay, server)` in
    /// the client crate, so another relay's or another server's measurement can
    /// never be rendered here.
    fn relay_measured_server_leg_rows(&self) -> Option<(LedgerValue, Option<LedgerValue>)> {
        if !self.run_boosting() || self.holding {
            return None;
        }
        let server = self.game_server_addr()?;
        let relay = self.selected_entry()?.addr;
        let report = relay_measured_server_leg(relay, server)?;
        let leg = LedgerValue::Data(relay_measured_leg_text(&report));
        let expected =
            expected_path_text(self.status.latest_rtt_ms, report.median_us).map(LedgerValue::Data);
        Some((leg, expected))
    }

    /// The relay's passive app-RTT row for the route on screen.
    ///
    /// Display-only and secondary: it is the relay's own
    /// forward→first-response median for the same `(relay, server)` pair, shown
    /// beside the ICMP `S`. The client crate already refused a stale, zero or
    /// absurd report, so `Some` here is always a presentable relay measurement;
    /// it feeds no ranking decision (the F4 revision keeps `S` non-steering
    /// until the experiment passes) and disappears when the passive window goes
    /// quiet rather than showing an old number.
    fn relay_measured_app_server_leg_row(&self) -> Option<LedgerValue> {
        if !self.run_boosting() || self.holding {
            return None;
        }
        let server = self.game_server_addr()?;
        let relay = self.selected_entry()?.addr;
        let report = relay_measured_app_server_leg(relay, server)?;
        Some(LedgerValue::Data(relay_measured_app_leg_text(&report)))
    }

    /// The relay's TTL-traceroute row for the route on screen: the last router
    /// on the relay→server path and its round trip.
    ///
    /// This is the placement anchor for a hosting-network server whose
    /// geolocation is unknown: "the path reaches this router" instead of
    /// nothing. The client crate owns the plausibility and 10-minute freshness
    /// rules and binds the report to this exact `(relay, server)` pair, so
    /// `Some` here is always a fresh, well-formed relay trace. It feeds no
    /// ranking decision and claims no city.
    fn relay_measured_trace_row(&self) -> Option<LedgerValue> {
        if !self.run_boosting() || self.holding {
            return None;
        }
        let server = self.game_server_addr()?;
        let relay = self.selected_entry()?.addr;
        let report = relay_measured_trace(relay, server)?;
        Some(LedgerValue::Data(relay_measured_trace_text(&report)))
    }

    /// The route surface's `Server leg` row: the relay-to-server leg for the
    /// relay in use, and how the relay choice obtained it.
    ///
    /// Present only while this client is tunnelling and the route is the relay (a
    /// held session's own row already says the relay is not in use), and only
    /// for a game server the chooser ranked with a relay it ranked. A row with no
    /// fact behind it would be exactly the invented state this surface exists to
    /// stop.
    ///
    /// Units: microseconds in the model, milliseconds on screen. One metric, one
    /// label (`route.server_leg`); the hover carries no separate number.
    fn server_leg_value(&self) -> Option<LedgerValue> {
        if !self.run_boosting() || self.holding {
            return None;
        }
        let server = self.game_server_addr()?;
        let ranking = self
            .route_ranking
            .as_ref()
            .filter(|ranking| ranking.server == server)?;
        let relay = self.selected_entry()?.addr;
        let scored = scored_for_relay(&ranking.scored, relay);
        let measured = measured_server_leg(relay, server);
        // The ping may have landed after the choice: state the triangle floor it
        // proves now rather than leaving the row on the prior until the 600 s
        // re-rank. Same verdict, same inputs the ranking would use - no re-rank.
        // The ping is keyed to `server`, so the previous server's sample is
        // refused here too (task-25).
        let early_floor_us =
            early_triangle_floor(self.server_ping, server, self.ranked_client_leg_us(relay));
        match server_leg_state(scored, measured, early_floor_us) {
            ServerLegState::Measured(measured) => {
                let ms = format!("{:.0}", measured.dest_leg_us as f64 / 1000.0);
                let samples = measured.samples.to_string();
                let text = match measured.age {
                    // Only the estimator may say how old the newest sample is.
                    // `None` (no age reported) is rendered as no age, never as
                    // zero and never estimated from the choice's own clock.
                    Some(age) => {
                        let secs = age.as_secs().to_string();
                        i18n::t_with(
                            "route.server_leg_measured_age",
                            &[("ms", &ms), ("n", &samples), ("secs", &secs)],
                        )
                    }
                    None => {
                        i18n::t_with("route.server_leg_measured", &[("ms", &ms), ("n", &samples)])
                    }
                };
                Some(LedgerValue::Data(text))
            }
            ServerLegState::Bounded { floor_us } => {
                let ms = format!("{:.0}", floor_us as f64 / 1000.0);
                Some(LedgerValue::Data(i18n::t_with(
                    "route.server_leg_bounded",
                    &[("ms", &ms)],
                )))
            }
            ServerLegState::Prior { prior_us } => {
                let prior = format!("{:.0}", prior_us as f64 / 1000.0);
                let uncertainty = format!("{:.0}", PRIOR_UNCERTAINTY_US as f64 / 1000.0);
                Some(LedgerValue::Hint(
                    i18n::t_with(
                        "route.server_leg_prior",
                        &[("prior", &prior), ("unc", &uncertainty)],
                    )
                    .into(),
                ))
            }
            ServerLegState::Unknown => Some(LedgerValue::Hint(i18n::t("route.server_leg_unknown"))),
        }
    }

    /// The triangle-inequality verdict for the relay in use, when it holds.
    ///
    /// The client leg is the keepalive echo - the one-RTT leg the client measures
    /// continuously - falling back to the ranking's probe median before the first
    /// echo arrives. Both are one-RTT measurements of the same leg, which is what
    /// the triangle inequality needs; the old two-RTT HTTP wall time would have
    /// made this verdict wrong in the other direction.
    fn cannot_beat_direct(&self) -> Option<(u64, u64)> {
        if !self.run_boosting() || self.holding {
            return None;
        }
        let client_leg_us = self.current_client_leg_us()?;
        // Keyed to the locked server: a ping measured against another server is
        // not this server's L3d (task-25).
        let server = self.game_server_addr()?;
        relay_cannot_beat_direct(Some(client_leg_us), self.server_ping, server)
    }

    /// The one-RTT client leg to the relay in use, microseconds.
    fn current_client_leg_us(&self) -> Option<u64> {
        if self.status.latest_rtt_ms > 0.0 {
            return Some((self.status.latest_rtt_ms * 1000.0).round() as u64);
        }
        let relay = self.selected_entry()?.addr;
        self.ranked_client_leg_us(relay)
    }

    /// Whether any tunnel backend is currently carrying the game's traffic.
    ///
    /// One definition, because the rail, the tray and the route surface must not
    /// disagree about whether the client is "on".
    fn run_boosting(&self) -> bool {
        self.status.interceptor_active
            || self.status.windivert_active
            || self.status.capture_active
            || self.status.redirect_active
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
                ui.label(egui::RichText::new(text.as_ref()).size(BODY).color(text_3));
            }
            LedgerValue::Warn(text) => {
                ui.label(egui::RichText::new(text.as_ref()).size(BODY).color(warn));
            }
        }
    }

    /// `Relay` = the human-readable label the app actually has, or the raw
    /// proxy address as a last resort. Never a fabricated city.
    fn relay_value(&self) -> LedgerValue {
        if !self.status.connected {
            return LedgerValue::Hint("--".into());
        }
        let node_id = self
            .selected_entry()
            .and_then(|e| e.node_id.as_deref())
            .or(self.status.node_id.as_deref());
        let base = if let Some(id) = node_id {
            LedgerValue::Data(discovery::friendly_label(id))
        } else if self.status.proxy_addr.is_empty() {
            LedgerValue::Hint("--".into())
        } else {
            LedgerValue::Data(self.status.proxy_addr.clone())
        };
        // Direct fallback used to be glued onto this value, which made a relay
        // name and a routing state read as one string. It has its own row now.
        base
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
            None => LedgerValue::Hint(i18n::t("route.place_hint")),
        }
    }

    /// Packets THIS session has pushed through the relay. The relay's own
    /// `/health` counters are lifetime totals for the whole fleet node, not our
    /// session, so they must never be shown as if they were ours.
    ///
    /// Outbound datagrams this session handed to the relay, in whichever mode
    /// is running.
    fn session_sent(&self) -> Option<u64> {
        routed_count(
            self.status.redirect_active,
            self.status.redirect_pkts_out,
            self.status.interceptor_active,
            self.status.interceptor_tunnel_sent,
        )
    }

    fn routed_value(&self) -> LedgerValue {
        let sent = self.session_sent();
        let returned = returned_count(
            self.status.redirect_active,
            self.status.redirect_pkts_in,
            self.status.interceptor_active,
            self.status.interceptor_from_proxy,
        );
        // The engine's counters are cumulative for the interceptor, so a relay
        // switch or a held session carried the old traffic into the new session's
        // numbers. The row is about this session, so the delta from where it
        // started is what it shows.
        let base = self
            .relayed_base
            .as_ref()
            .filter(|(key, _, _)| *key == self.session_key())
            .map(|(_, sent, returned)| (*sent, *returned));
        let (sent, returned) = match (sent, returned) {
            (Some(sent), Some(returned)) => {
                let (sent, returned) = session_delta(sent, returned, base);
                (Some(sent), Some(returned))
            }
            other => other,
        };
        match (sent, returned) {
            (Some(sent), Some(returned)) => {
                let (text, starving) = relayed_row_text(sent, returned);
                if starving {
                    LedgerValue::Warn(text.into())
                } else {
                    LedgerValue::Data(text)
                }
            }
            _ => LedgerValue::Hint(i18n::t("route.count_hint")),
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
        field_row(ui, narrow, &i18n::t("route.relay"), |ui| {
            ui.horizontal(|ui| {
                let disconnect_w = if self.status.connected { 78.0 } else { 0.0 };
                let manage_w = 64.0;
                let combo_w =
                    (ui.available_width() - manage_w - disconnect_w - S2 * 2.0).max(100.0);
                ui.scope(|ui| {
                    ui.set_max_width(combo_w);
                    self.boost_server_combo(ui);
                });
                if self.status.connected
                    && ui
                        .link(i18n::t("action.disconnect").as_ref())
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
        field_row(ui, narrow, &i18n::t("route.game"), |ui| {
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
        } else if let Some(key) = game_caption(
            self.game_auto,
            self.game_manual,
            self.auto_detected_game.as_deref(),
        ) {
            ui.label(egui::RichText::new(i18n::t(key)).size(BODY).color(text_2));
        }
        // The process front door belongs to Manual mode, or stays while a target
        // is set so it can be cleared. It used to be drawn on every game state,
        // which read as an unexplained extra choice under a game already chosen.
        if self.game_manual || self.target_process.is_some() {
            self.target_control(ui);
        }
        if self.game_manual {
            self.manual_section(ui);
        }
    }

    /// The one-RTT client leg the ranking charges for `addr`, in microseconds,
    /// from the last full measurement pass.
    ///
    /// The probe is a TCP connect (one round trip), not the two-RTT HTTP wall
    /// time it used to be - see [`discovery::RelaySample`] - so this figure is
    /// directly comparable with a one-RTT destination estimate.
    fn ranked_client_leg_us(&self, addr: std::net::SocketAddrV4) -> Option<u64> {
        let sample = self
            .relay_legs
            .as_ref()?
            .iter()
            .find(|(candidate, _)| *candidate == addr)?
            .1
            .as_ref()?;
        Some(u64::try_from(sample.median.as_micros()).unwrap_or(u64::MAX))
    }

    /// The measured client leg for `addr`, in milliseconds (the picker's unit).
    fn leg_ms(&self, addr: std::net::SocketAddrV4) -> Option<u64> {
        self.ranked_client_leg_us(addr).map(|us| us / 1000)
    }

    /// The Boost Server selector: auto-select or a pinned relay, one dropdown.
    fn boost_server_combo(&mut self, ui: &mut egui::Ui) {
        // The picker shows each relay's measured client leg, so it needs the
        // measurement the destination-aware chooser already uses. Opening the
        // picker is the moment the user wants those numbers; measuring then
        // costs one background pass and never blocks the frame.
        if self.relay_legs.is_none() && self.relay_measurement.is_none() {
            self.start_relay_measurement();
        }
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
            i18n::t("relay.auto_fastest").into_owned()
        } else {
            self.selected_entry()
                .map(|e| e.label.clone())
                .unwrap_or_else(|| i18n::t("relay.select_a_relay").into_owned())
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
                    // A name alone asks the user to choose blind. The measured
                    // client leg is already in memory for the destination-aware
                    // chooser, so the picker shows the number it ranks on, and
                    // marks the relay it would pick for this server.
                    let best = self.destination_winner == Some(entry.addr);
                    let text = match self.leg_ms(entry.addr) {
                        Some(ms) if best => format!("{}  {ms:>3} ms  best", entry.label),
                        Some(ms) => format!("{}  {ms:>3} ms", entry.label),
                        None if best => format!("{}  best", entry.label),
                        None => entry.label.clone(),
                    };
                    if ui
                        .selectable_label(picked, text)
                        .on_hover_text(
                            "Median of five probes to this relay: the client leg only, \
                             not the path to your game server and not your in-game ping.",
                        )
                        .clicked()
                    {
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
            if auto {
                // Re-evaluate the server that is already locked: without this,
                // turning Auto on while a match is live keeps whatever relay is
                // connected, because `follow_game_server` returns early once the
                // server has a choice. Measured live: Auto selected, still on
                // Los Angeles for a Sao Paulo server.
                self.chosen_for_server = None;
                self.server_chosen.store(false, Ordering::Relaxed);
                self.locked_addr = None;
                self.locked_since = None;
            }
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

    /// The one-step game selector: the two modes, the built-in registry, the
    /// user's own profiles under "My games", and the way into the editor.
    fn game_combo(&mut self, ui: &mut egui::Ui) {
        let width = ui.available_width();
        let user_games = user_profiles();
        let label = if self.game_manual {
            i18n::t("game.manual_server").into_owned()
        } else if self.game_auto {
            match &self.auto_detected_game {
                Some(name) => i18n::t_with("game.auto_named", &[("name", name)]),
                None => i18n::t("game.auto_detect").into_owned(),
            }
        } else {
            self.selected_game().display
        };
        let mut changed = false;
        let mut open_editor = false;
        egui::ComboBox::from_id_salt("game_select")
            .selected_text(label)
            .width(width)
            .show_ui(ui, |ui| {
                for row in game_pick_rows(games().len(), user_games.len()) {
                    match row {
                        GamePickRow::Auto => {
                            if ui
                                .selectable_label(self.game_auto, i18n::t("game.auto_detect"))
                                .clicked()
                            {
                                self.game_auto = true;
                                self.game_manual = false;
                                self.selected_custom = None;
                                changed = true;
                            }
                        }
                        GamePickRow::Manual => {
                            if ui
                                .selectable_label(self.game_manual, i18n::t("game.manual_custom"))
                                .clicked()
                            {
                                self.game_manual = true;
                                self.game_auto = false;
                                self.selected_custom = None;
                                changed = true;
                            }
                        }
                        GamePickRow::Builtin(i) => {
                            let picked = !self.game_auto
                                && !self.game_manual
                                && self.selected_custom.is_none()
                                && i == self.selected_game_idx;
                            if ui.selectable_label(picked, games()[i].display).clicked() {
                                self.selected_game_idx = i;
                                self.selected_custom = None;
                                self.game_auto = false;
                                self.game_manual = false;
                                changed = true;
                            }
                        }
                        GamePickRow::MyGamesHeader => {
                            ui.separator();
                            ui.label(
                                egui::RichText::new(i18n::t("game.my_games"))
                                    .size(CAPTION)
                                    .color(text_3),
                            );
                        }
                        GamePickRow::User(i) => {
                            let profile = &user_games[i];
                            let picked = !self.game_auto
                                && !self.game_manual
                                && self.selected_custom.as_deref() == Some(profile.key.as_str());
                            if ui
                                .selectable_label(picked, &profile.name)
                                .on_hover_text(&profile.key)
                                .clicked()
                            {
                                self.selected_custom = Some(profile.key.clone());
                                self.game_auto = false;
                                self.game_manual = false;
                                changed = true;
                            }
                        }
                        GamePickRow::NewCustom => {
                            if ui.link(i18n::t("game.new_custom")).clicked() {
                                ui.close();
                                open_editor = true;
                            }
                        }
                    }
                }
            });
        if changed {
            self.persist_config();
        }
        if open_editor {
            self.game_editor = GameEditorDraft::new_profile();
            self.editor_saved = None;
            self.editor_error = None;
            self.process_search.clear();
            // The picker's table is read once per open, on a worker thread; the
            // egui thread only polls for it (see `poll_process_list`).
            self.start_process_list();
            self.show_game_editor = true;
        }
    }

    /// The front door for process-scoped interception, sitting with the Game
    /// row it takes over whenever a process is picked.
    ///
    /// A frame without a target offers the picker; a frame with one names the
    /// process it will relay and offers the way back to the built-in Game path.
    /// Nothing here enumerates: the picker's table is read once per open, on a
    /// worker thread (see `start_process_list`), exactly like the editor's.
    fn target_control(&mut self, ui: &mut egui::Ui) {
        ui.add_space(S2);
        let action = target_front_door(ui, self.target_process.as_ref());
        if action.clear {
            self.target_process = None;
        }
        if action.open_picker {
            self.process_search.clear();
            self.start_process_list();
            self.show_target_picker = true;
        }
    }

    /// The "Manual / Custom…" flow: relay an explicit server straight from the
    /// Game field, for titles the auto-detect does not know.
    fn manual_section(&mut self, ui: &mut egui::Ui) {
        let mut chosen_server: Option<usize> = None;
        let mut save_server = false;
        ui.add_space(S2);
        // One line, not three: the paragraph said the same thing as the fields
        // directly beneath it and cost a wrapped block of height.
        ui.label(
            egui::RichText::new("Enter the server's IP and port.")
                .size(CAPTION)
                .color(text_3),
        );
        ui.add_space(S1);
        ui.horizontal(|ui| {
            let label = ui.label(i18n::t("route.server_label"));
            let default_port = self.selected_game().default_port;
            // The row's own right edge, not `available_width`: inside a horizontal
            // row that value still counts the space the label already took, so a
            // field sized from it pushed the card past the window edge.
            let field_w =
                (ui.max_rect().right() - label.rect.right() - ui.spacing().item_spacing.x - 2.0)
                    .max(80.0);
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.server_input)
                    .hint_text(format!("e.g. 123.45.67.89:{default_port}"))
                    .desired_width(field_w),
            );
            // Written as it is typed, so the address is still here after a
            // restart even if the window is hidden to the tray rather than
            // closed, which is the only way the app is normally left.
            if edit.changed() {
                self.persist_config();
            }
        });

        // Saved servers: a preset fills the address above and the range below.
        let servers = user_servers();
        if !servers.is_empty() {
            ui.add_space(S2);
            ui.horizontal(|ui| {
                ui.label(i18n::t("servers.saved"));
                let current = servers
                    .iter()
                    .position(|server| server.address == self.server_input.trim())
                    .map(|i| servers[i].name.clone())
                    .unwrap_or_else(|| i18n::t("servers.none").into_owned());
                egui::ComboBox::from_id_salt("saved_server_select")
                    .selected_text(current)
                    .width((ui.available_width() - 8.0).max(80.0))
                    .show_ui(ui, |ui| {
                        for (i, server) in servers.iter().enumerate() {
                            let label = format!("{}  {}", server.name, server.address);
                            if ui.selectable_label(false, label).clicked() {
                                chosen_server = Some(i);
                            }
                        }
                    });
            });
        }
        ui.add_space(S2);
        // The example is the pinned game's own built-in scan range, so the
        // field reads as the game about to be scanned rather than one fixed
        // game's ports; a game with no range in the table keeps the format
        // token as its example.
        let game = self.selected_game();
        let (lo, hi) = default_port_range(&game.key, game.default_port);
        let range_example = if lo == 0 {
            "lo-hi".to_string()
        } else {
            format!("{lo}-{hi}")
        };
        ui.horizontal(|ui| {
            let label = ui
                .label(i18n::t("route.custom_port_range"))
                .on_hover_ui(|ui| {
                ui.label(format!(
                    "Override the default port scan range for auto-detect. \
                     Use this if Packets Sent stays at 0 after 15 s.\n\
                     Format: lo-hi  (e.g. {range_example})  or a single port."
                ));
                ui.hyperlink_to(
                    i18n::t("route.port_fix_guide"),
                    "https://github.com/ShibbityShwab/lightspeed/wiki/Troubleshooting#port-not-detected",
                );
            });
            let port_valid = self.custom_port_input.is_empty()
                || parse_custom_port_range(&self.custom_port_input).is_some();
            // The one invalid-input treatment, shared with the profile editor.
            if invalid_text_field(
                ui,
                &mut self.custom_port_input,
                i18n::t_with("route.port_hint", &[("range", range_example.as_str())]),
                // Sized from the row's own right edge: inside a horizontal row
                // `available_width` still counts the label, so a field sized from
                // it made the card wider than the window.
                (ui.max_rect().right()
                    - label.rect.right()
                    - ui.spacing().item_spacing.x
                    - 2.0)
                    .max(80.0),
                !port_valid,
            )
            .changed()
            {
                self.persist_config();
            }
        });
        ui.add_space(S2);

        // Save as: the label on its own line, then a field sized to leave the
        // button room. In one row the field's 160 px plus a label plus the button
        // overran a 320 px card and the button was clipped at the edge.
        ui.label(i18n::t("servers.save_as"));
        ui.horizontal(|ui| {
            // Button first from the right, then the field takes the remainder:
            // a guessed reserve for the button was 18 px short in this locale, so
            // the row pushed the card past the window edge.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui.button(i18n::t("servers.save")).clicked() {
                    save_server = true;
                }
                let field_w = (ui.available_width() - ui.spacing().item_spacing.x).max(80.0);
                ui.add(
                    egui::TextEdit::singleline(&mut self.save_server_name)
                        .hint_text(i18n::t("servers.save_name_hint"))
                        .desired_width(field_w),
                );
            });
        });
        if let Some(saved) = &self.server_saved {
            ui.label(egui::RichText::new(saved).size(CAPTION).color(signal));
        }
        if let Some(error) = &self.server_error {
            ui.label(egui::RichText::new(error).size(CAPTION).color(danger));
        }
        if let Some(i) = chosen_server {
            self.server_input = servers[i].address.clone();
            self.custom_port_input = servers[i]
                .udp_ports
                .map(|range| range.to_string())
                .unwrap_or_default();
            self.persist_config();
        }
        if save_server {
            match server_from_fields(
                &self.save_server_name,
                &self.server_input,
                &self.custom_port_input,
            ) {
                Ok(server) => {
                    let store = profile_store();
                    let mut servers = store.servers();
                    servers.push(server.clone());
                    match save_user_profiles(store, &store.games(), &servers) {
                        Ok(()) => {
                            self.server_saved = Some(i18n::t_with(
                                "servers.saved_notice",
                                &[("name", server.name.as_str())],
                            ));
                            self.server_error = None;
                            self.save_server_name.clear();
                        }
                        Err(error) => {
                            self.server_saved = None;
                            self.server_error = Some(error);
                        }
                    }
                }
                Err(reason) => {
                    self.server_saved = None;
                    self.server_error = Some(reason);
                }
            }
        }
        ui.add_space(S2);

        let server_valid = parse_server_addr(&self.server_input).is_some();
        if !server_valid && !self.server_input.is_empty() {
            ui.label(egui::RichText::new(i18n::t("proxies.addr_invalid_hint")).color(warn));
        }

        ui.add_space(S2);
        let picked = self.selected_game();
        let local_port = self
            .server_input
            .parse::<SocketAddrV4>()
            .map(|addr| addr.port())
            .unwrap_or(picked.default_port);
        let instruction = connect_instruction(&picked.key, local_port);
        ui.label(egui::RichText::new(instruction).size(BODY).color(text_2));
        ui.add_space(S2);
    }

    // ── In-window sheets ────────────────────────────────────────────────────

    /// `Settings`: Privacy, Maintenance, Advanced, and About, in the same sheet
    /// pattern as the Proxy Manager.
    fn settings_sheet(&mut self, ctx: &egui::Context) {
        let (max_w, max_h) = sheet_bounds(ctx);
        let mut close = false;
        let response = egui::Modal::new(egui::Id::unique("settings_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_width(max_w);
                sheet_title(ui, &i18n::t("settings.title"));
                egui::ScrollArea::vertical()
                    // Size the body from the space that actually remains after
                    // the frame padding, the title and Close; a fixed reserve left
                    // the last button clipped behind Close once Settings grew.
                    .max_height((max_h - 190.0).max(160.0))
                    // The sheet scrolls by design; a scrollbar that only appears
                    // on hover left About below the fold with no cue that it was
                    // there.
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        self.settings_body(ui);
                    });
                ui.add_space(S2);
                if ui.button(i18n::t("action.close").as_ref()).clicked() {
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
        ui.add_space(S2);

        // Privacy: anonymous latency telemetry.
        ui.add_space(S2);
        hairline(ui);
        ui.add_space(S1);
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

        // Routing behaviour: what happens when a relay stops returning the
        // game's traffic. There is no control here any more - the automatic
        // direct fallback and its re-test budget were removed on 2026-11-20 in
        // the owner-approved warn-only mode - so this states the behaviour
        // instead of promising a setting. It is a caption rather than a hover
        // because the old checkbox's tooltip promised re-tests "after 20 s, then
        // 45 s, then 90 s" while the code actually did 30 s/120 s/park, and the
        // text was never localized. The decision belongs to the player: STOP
        // OPTIMIZING, or pick another relay.
        ui.add_space(S2);
        hairline(ui);
        ui.add_space(S1);
        section_label(ui, "Routing");
        ui.label(
            egui::RichText::new(i18n::t("settings.starvation_policy"))
                .size(CAPTION)
                .color(text_3),
        );
        ui.add_space(S2);

        // Relays: the list itself, inlined from the old Proxy Manager modal.
        ui.add_space(S2);
        hairline(ui);
        ui.add_space(S1);
        section_label(ui, &i18n::t("settings.relays"));
        self.proxy_manager_body(ui);

        // Maintenance: self-update, the trace log, and the relay link.
        hairline(ui);
        ui.add_space(S2);
        section_label(ui, &i18n::t("settings.maintenance"));
        // One wrapped row: three stacked buttons cost 120 px of a 853 px window
        // and pushed Components and About off the bottom of the sheet.
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(i18n::t("update.sheet_title").as_ref())
                .on_hover_text(i18n::t("settings.check_updates_hint"))
                .clicked()
            {
                self.start_update_check();
            }
            if ui.button(i18n::t("settings.open_log")).clicked() {
                self.reveal_log_file();
            }
            if ui
                .button(i18n::t("proxies.refresh"))
                .on_hover_text("Fetch the latest community relay list from the registry.")
                .clicked()
            {
                self.start_discovery();
            }
            if ui
                .button(i18n::t("settings.remeasure"))
                .on_hover_text(
                    "Probe every relay again and refresh the numbers the relay picker shows.",
                )
                .clicked()
            {
                self.start_relay_measurement();
            }
        });
        ui.add_space(S1);
        // The capture backend and Npcap now live in the Components block below.
        // Leaving a second, differently-worded line here is how "pcap mode: not
        // detected" ended up reading as broken while WinDivert was relaying.
        // The hold is a policy the client applies silently; saying it in the
        // settings gives the wait a reason and a duration.
        ui.label(
            egui::RichText::new(i18n::t_with(
                "settings.hold_policy",
                &[("secs", LOCK_SETTLE.as_secs().to_string().as_str())],
            ))
            .size(CAPTION)
            .color(text_3),
        );
        ui.add_space(S2);

        // Components: what the client depends on and what state each is in, so a
        // bug report can answer "is WinDivert there, am I elevated, which build
        // is this" without reading a log. Labels are literal here rather than in
        // the catalogs: this block is diagnostic text and is worth translating
        // only once its rows have settled.
        hairline(ui);
        ui.add_space(S2);
        ui.label(
            egui::RichText::new("Components")
                .size(CAPTION)
                .color(text_2),
        );
        // A table, not a list of controls: tighten the rows here and put the
        // spacing back before the next section.
        ui.spacing_mut().item_spacing.y = 2.0;
        ui.add_space(S1);
        egui::Grid::new("components")
            .num_columns(2)
            .spacing([10.0, 0.0])
            .min_row_height(14.0)
            .show(ui, |ui| {
                let admin = if self.is_admin {
                    "administrator"
                } else {
                    "standard user"
                };
                let capture = if self.status.interceptor_active {
                    self.status.interceptor_platform
                } else {
                    "idle"
                };
                let npcap = if self.capture_available {
                    "installed"
                } else {
                    "not installed (pcap fallback only)"
                };
                let relays = format!("{} configured", self.proxies.len());
                for (label, value) in [
                    ("Version", env!("CARGO_PKG_VERSION")),
                    ("Elevation", admin),
                    ("Capture", capture),
                    ("Npcap", npcap),
                    ("Relays", relays.as_str()),
                ] {
                    ui.label(egui::RichText::new(label).size(CAPTION).color(text_3));
                    ui.label(egui::RichText::new(value).size(CAPTION).color(text_2));
                    ui.end_row();
                }
            });
        ui.add_space(S2);

        // About: the brand mark and the project links.
        ui.spacing_mut().item_spacing.y = S2;
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
                    egui::RichText::new(i18n::t_with(
                        "about.version",
                        &[("version", env!("CARGO_PKG_VERSION"))],
                    ))
                    .size(CAPTION)
                    .color(text_3),
                );
            });
        });
        ui.add_space(S2);
        ui.label(
            egui::RichText::new(i18n::t("about.blurb"))
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

    /// `Custom game`: the profile editor, in the same sheet pattern as Settings.
    ///
    /// A new profile is composed here; the Game dropdown's "My games" group only
    /// selects one that exists. Saving writes `profiles.toml` and reloads the
    /// store, and the resulting key stays on screen so `--game <key>` is
    /// discoverable without reading the file.
    fn game_editor_sheet(&mut self, ctx: &egui::Context) {
        let (max_w, max_h) = sheet_bounds(ctx);
        let mut close = false;
        let mut actions = EditorActions::default();
        let user_games = user_profiles();
        let user_keys = user_game_keys();
        let editing_key = self
            .game_editor
            .editing
            .and_then(|i| user_games.get(i))
            .map(|profile| profile.key.clone());
        let verdict = validate_draft(&self.game_editor, &user_keys, editing_key.as_deref());

        let response = egui::Modal::new(egui::Id::unique("game_editor_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_width(max_w);
                sheet_title(ui, &i18n::t("game.editor_title"));
                egui::ScrollArea::vertical()
                    .max_height((max_h - 190.0).max(160.0))
                    .show(ui, |ui| {
                        game_editor_body(
                            ui,
                            &mut self.game_editor,
                            &verdict,
                            self.editor_saved.as_deref(),
                            self.editor_error.as_deref(),
                            &user_games,
                            ProcessPicker {
                                rows: &self.process_rows,
                                status: picker_status(&self.process_list),
                                search: &mut self.process_search,
                            },
                            &mut actions,
                        );
                    });
                ui.add_space(S2);
                if ui.button(i18n::t("action.close").as_ref()).clicked() {
                    close = true;
                }
            });
        if response.should_close() || close {
            self.show_game_editor = false;
        }

        if actions.refresh_processes {
            self.start_process_list();
        }

        if let Some(i) = actions.edit {
            if let Some(profile) = user_games.get(i) {
                self.game_editor = GameEditorDraft::from_profile(i, profile);
                self.editor_saved = None;
                self.editor_error = None;
            }
        }
        if actions.save {
            let profile = self.game_editor.to_profile();
            let store = profile_store();
            let mut games = store.games();
            match self.game_editor.editing {
                Some(i) if i < games.len() => games[i] = profile.clone(),
                _ => games.push(profile.clone()),
            }
            match save_user_profiles(store, &games, &store.servers()) {
                Ok(()) => {
                    // Re-read through the store, which just reloaded: what the
                    // sheet shows next is what the next start will read.
                    let games = store.games();
                    if let Some(i) = games.iter().position(|game| game.key == profile.key) {
                        self.game_editor = GameEditorDraft::from_profile(i, &games[i]);
                    }
                    self.editor_saved = Some(i18n::t_with(
                        "game.editor_saved",
                        &[
                            ("name", profile.name.as_str()),
                            ("key", profile.key.as_str()),
                        ],
                    ));
                    self.editor_error = None;
                }
                Err(error) => {
                    self.editor_error = Some(error);
                    self.editor_saved = None;
                }
            }
        }
        if actions.delete {
            if let Some(i) = self.game_editor.editing {
                let store = profile_store();
                let mut games = store.games();
                if i < games.len() {
                    games.remove(i);
                    match save_user_profiles(store, &games, &store.servers()) {
                        Ok(()) => {
                            self.game_editor = GameEditorDraft::new_profile();
                            self.editor_saved = None;
                            self.editor_error = None;
                        }
                        Err(error) => self.editor_error = Some(error),
                    }
                }
            }
        }
    }

    /// `Target a process`: pick a running program and relay all of its
    /// traffic, whatever it is.
    ///
    /// The same picker, filter, off-thread enumeration and honest Loading /
    /// Empty / No-match states as the editor's process field; the difference is
    /// what a chosen row means - it becomes the interception target instead of
    /// filling a text field.
    fn target_picker_sheet(&mut self, ctx: &egui::Context) {
        let (max_w, max_h) = sheet_bounds(ctx);
        let mut close = false;
        let mut clear = false;
        let mut refresh = false;
        let mut chosen: Option<ProcessTarget> = None;
        let current = self.target_process.clone();

        let response = egui::Modal::new(egui::Id::unique("target_process_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_width(max_w);
                sheet_title(ui, &i18n::t("game.target_process"));
                ui.label(
                    egui::RichText::new(i18n::t("game.target_process_hint"))
                        .size(BODY)
                        .color(text_2),
                );
                egui::ScrollArea::vertical()
                    .max_height((max_h - 190.0).max(160.0))
                    .show(ui, |ui| {
                        let mut picker = ProcessPicker {
                            rows: &self.process_rows,
                            status: picker_status(&self.process_list),
                            search: &mut self.process_search,
                        };
                        chosen = process_picker(
                            ui,
                            &mut picker,
                            |entry| {
                                current
                                    .as_ref()
                                    .and_then(ProcessTarget::name)
                                    .is_some_and(|name| name.eq_ignore_ascii_case(&entry.name))
                            },
                            &mut refresh,
                        )
                        .map(target_for_pick);
                    });
                ui.add_space(S2);
                ui.horizontal(|ui| {
                    if current.is_some() && ui.button(i18n::t("game.target_clear")).clicked() {
                        clear = true;
                    }
                    if ui.button(i18n::t("action.close")).clicked() {
                        close = true;
                    }
                });
            });

        // The sheet's frame is gone; apply what it recorded. A chosen row wins
        // and closes the sheet, Clear drops the target, and a frame with neither
        // leaves the target alone.
        if chosen.is_some() {
            self.show_target_picker = false;
        }
        self.target_process = apply_target_choice(self.target_process.take(), chosen, clear);
        if refresh {
            self.start_process_list();
        }
        if response.should_close() || close {
            self.show_target_picker = false;
        }
    }

    /// Start the redirect path for the server typed in Manual mode.
    fn start_manual_redirect(&mut self) {
        let Some(server_addr) = parse_server_addr(&self.server_input) else {
            return;
        };
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

    /// The relay list: discovered and custom relays, add and remove.
    ///
    /// It lives in Settings rather than in its own modal, so configuration has
    /// one surface and the main window keeps only the relay picker.
    fn proxy_manager_body(&mut self, ui: &mut egui::Ui) {
        let mut remove_idx: Option<usize> = None;
        let mut add_addr: Option<SocketAddrV4> = None;
        let mut reset = false;

        ui.spacing_mut().item_spacing.y = 4.0;
        for (i, entry) in self.proxies.iter().enumerate() {
            // Fixed 16 px rows: resolved against the scroll area, each entry used
            // to be ~109 px and no spacing setting could shrink it.
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 16.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    let name_w = (ui.available_width() - 26.0).max(80.0);
                    ui.add_sized(
                        [name_w, 16.0],
                        egui::Label::new(egui::RichText::new(&entry.label).color(text_1))
                            .truncate(),
                    );
                    if ui.add(egui::Button::new("×").frame(false)).clicked() {
                        remove_idx = Some(i);
                    }
                },
            );
            ui.allocate_ui_with_layout(
                egui::vec2(ui.available_width(), 16.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    ui.label(
                        egui::RichText::new(entry.addr.to_string())
                            .size(CAPTION)
                            .monospace()
                            .color(text_2),
                    );
                    ui.label(
                        egui::RichText::new(entry.node_id.as_deref().unwrap_or("custom"))
                            .size(CAPTION)
                            .color(text_3),
                    );
                },
            );
        }
        if self.proxies.is_empty() {
            ui.label(
                egui::RichText::new("No relays yet. Discovery runs automatically.")
                    .size(CAPTION)
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
            if ui.button(i18n::t("proxies.open_config")).clicked() {
                paths::open_in_os(&paths::config_dir());
            }
            if ui.button(i18n::t("proxies.reset_defaults")).clicked() {
                reset = true;
            }
        });
        if let Some(err) = &self.config_error {
            ui.label(egui::RichText::new(err).size(CAPTION).color(danger));
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

    fn update_sheet(&mut self, ctx: &egui::Context) {
        let (max_w, max_h) = sheet_bounds(ctx);
        let response = egui::Modal::new(egui::Id::unique("update_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_width(max_w);
                sheet_title(ui, &i18n::t("update.sheet_title"));
                egui::ScrollArea::vertical()
                    .max_height((max_h - 190.0).max(160.0))
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
                                        .button(i18n::t("update.install").as_ref())
                                        .on_hover_text(i18n::t("update.install_hint"))
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
                if ui.button(i18n::t("action.close").as_ref()).clicked() {
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
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The full window rect, read before any panel claims space, drives the
        // narrow (width) and short (height) breakpoints.
        let viewport = ui.available_rect_before_wrap();
        let narrow = viewport.width() < 380.0;
        let short = viewport.height() < 560.0;
        // Where the window sits on its monitor, for the auto-size cap below.
        let (monitor_height, window_top) = monitor_geometry(frame, &ctx);

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
                // Testing switch: with LIGHTSPEED_KEEP_VISIBLE set, a close
                // request is ignored instead of hiding the window to the tray.
                // The hide is right for players; it costs an automated session
                // its subject every time a synthesized click lands on the X.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                if std::env::var_os("LIGHTSPEED_KEEP_VISIBLE").is_some() {
                    return;
                }
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
        self.poll_process_list();
        self.poll_relay_race();
        self.poll_relay_measurement();
        self.poll_winner_probe();
        self.maybe_rerace();
        self.poll_health();
        self.poll_game_presence();
        self.follow_game_server();
        self.release_stale_hold();
        self.poll_client_trace();
        self.poll_relay_health();

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
            let boosting = self.run_boosting();
            let warning_active = self.starvation_warning.is_some() && boosting;
            let new_tray_state =
                tray_state_for(has_error, warning_active, boosting, self.status.connected);

            if let Some(tray) = &self.tray {
                tray.set_state(new_tray_state, self.status.latest_rtt_ms);
            }
        }

        // ── Status window shell ──────────────────────────────────────────
        // The "scroll-body-shell": rail, action bar, and (at normal height)
        // route card are fixed; the config region is the only scrolling part;
        // the footer stays pinned. At a short height the route card folds into
        // the scroll region so the fixed chrome never crowds out the footer.
        let boosting = self.run_boosting();

        let rail_edge = if boosting {
            // The rail's left edge carries the same honesty as the state word:
            // a failing return path paints it in the danger colour, not green.
            if self.starvation_warning.is_some() {
                danger
            } else {
                signal
            }
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
                    if self.tray_available()
                        && std::env::var_os("LIGHTSPEED_KEEP_VISIBLE").is_none()
                        && ui.button(i18n::t("window.hide_to_tray")).clicked()
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

        // Action bar: one slot, three gate-driven labels. It sits below the
        // inputs it acts on - the relay and the game are choices, this is their
        // outcome - and above the footer, where a thumb reaches it on a phone.
        let has_relay = self.selected_entry().is_some();
        let (action_label, action_ready) =
            primary_action(self.is_admin, boosting, self.status.connected, has_relay);
        // In Manual mode the action applies the server typed into the form, so it
        // is only ready once that address parses.
        let action_ready =
            action_ready && (!self.game_manual || parse_server_addr(&self.server_input).is_some());
        let action_kind = if boosting {
            ActionKind::Danger
        } else {
            ActionKind::Accent
        };
        let action = egui::Panel::bottom("action_bar")
            .frame(egui::Frame::new().fill(bg_1).inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: S2 as i8,
                bottom: S2 as i8,
            }))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                action_button(ui, &action_label, action_kind, action_ready)
            });
        if action.inner.clicked() && action_ready {
            if boosting {
                self.stop_boost();
            } else if !self.is_admin {
                self.restart_elevated();
            } else if self.game_manual {
                // Manual mode's typed server is applied through the redirect path;
                // it used to have its own Start button inside the form, which read
                // as a second, redundant optimize control.
                self.start_manual_redirect();
            } else {
                self.start_boost();
            }
        }

        // Config: the only scrolling region.
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg_1).inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: S3 as i8,
                bottom: S3 as i8,
            }))
            .show(ui, |ui| {
                let out = egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        // Cap and centre the column. `set_max_width` alone left the
                        // child measuring against an unbounded width inside this
                        // scroll area, so the ledger lost its label column; setting
                        // the width explicitly gives the row layout a number to work
                        // from.
                        let target = ui.available_width().min(560.0);
                        ui.vertical_centered(|ui| {
                            ui.set_width(target);
                            // Same surface, radius, border and padding as the map
                            // card above: the pickers were bare fields on the panel
                            // while the status sat on a card, which is the style
                            // split the review called out.
                            egui::Frame::new()
                                .fill(bg_2)
                                .stroke(egui::Stroke::new(1.0, border_1))
                                .corner_radius(egui::CornerRadius::same(R_CARD))
                                .inner_margin(egui::Margin::same(S3 as i8))
                                .show(ui, |ui| {
                                    ui.set_width((target - 2.0 * S3).max(200.0));
                                    self.config_body(ui, narrow, short);
                                });
                        });
                    });

                // Fixed width, content height: the window grows and shrinks with
                // what is drawn, so nothing is hidden and nothing is left empty.
                // The 8 px deadband keeps a one-pixel text re-wrap from becoming a
                // resize loop.
                //
                // The growth stops at the bottom of the screen. Past it, the pinned
                // action bar and footer were drawn off-screen - in Manual mode, the
                // tallest form, that hid the only button that applies the typed
                // server. Capped, this region scrolls instead, as the shell intends.
                let chrome = (viewport.height() - out.inner_rect.height()).max(0.0);
                let limit = window_height_limit(monitor_height, window_top);
                let needed = (chrome + out.content_size.y + 2.0).clamp(MIN_WINDOW_HEIGHT, limit);
                if (needed - viewport.height()).abs() > 8.0 {
                    ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(
                        320.0, needed,
                    )));
                }
            });

        // ── In-window sheets ─────────────────────────────────────────────
        if self.show_settings {
            self.settings_sheet(&ctx);
        }
        if self.show_update_dialog {
            self.update_sheet(&ctx);
        }
        if self.show_target_picker {
            self.target_picker_sheet(&ctx);
        }
        if self.show_game_editor {
            self.game_editor_sheet(&ctx);
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
