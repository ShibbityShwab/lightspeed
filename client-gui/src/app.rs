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
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Receiver;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::config::{self, AppConfig, ConfigRecovery, ProxyEntry};
use crate::design::*;
use crate::discovery::{self, DiscoveryOutcome, RelayHealth};
use crate::globe;
use crate::i18n;
use crate::paths;
use crate::platform::{
    default_port_range, Animations, ElevationAttempt, ElevationPhase, Platform, QuitFlag,
    TrayAction, TrayHandle,
};
use crate::processes::{self, ProcessEntry};
use crate::update::UpdateStatus;
use eframe::egui;
use lightspeed_client::app::commands::AppCore;
use lightspeed_client::app::dto as app_dto;
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
/// The pure `AppStatus` → word/rows adapter; W2-2's shell is its consumer.
pub(crate) mod route_surface;
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
use route_surface::{Line, RouteSurface, RouteWord, Row, RowValue, Tone};
use theme::*;
use watchdog::*;
use widgets::*;
use window::*;

// `main.rs` reaches `install_fonts` and the window-height bounds through
// `crate::app::…`, so the root has to re-export them instead of only importing
// them privately.
pub(crate) use theme::install_fonts;
pub(crate) use window::{MAX_WINDOW_HEIGHT, MIN_WINDOW_HEIGHT};

// ── Tray state ───────────────────────────────────────────────────────────────

/// The five states the tray icon can reflect in its color and tooltip.
///
/// The type and its ordering rule moved to the shared crate at W2-1 S3a
/// (`lightspeed_client::app::dto::TrayState` / `status::tray_state_for`) so the
/// CLI can project the same honesty order. The platform backends still address
/// it through this re-export, so `platform/*` is unchanged.
pub use lightspeed_client::app::dto::TrayState;

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

// `MAX_RELAYS_TRIED` / `SERVER_PING_FRESH_SECS` moved to
// `lightspeed_client::app::status` at W2-1 S3a; `choice::*` re-exports them so
// this module and its tests keep their names.

/// How old a direct-ping sample may be before the window asks for a fresh one.
const PING_REFRESH_AFTER: std::time::Duration = std::time::Duration::from_secs(30);

// ── App struct ───────────────────────────────────────────────────────────────

/// One relay connect attempt this window started, until the engine's own facts
/// end it (`connected`, `control_registered` or `registration_error`).
///
/// The DTO's `RelayStatus.connecting` fact is derived from this plus the engine
/// snapshot; the snapshot is refreshed every frame, and the attempt is resolved
/// in `poll_health` (an orchestration poll, not the drawing code).
type ConnectAttempt = SocketAddrV4;

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
    /// The shared watchdog's publication: the warning, the live judged-window
    /// streak and the one-shot demotion finding (W2-3a / R16).
    watchdog_publication: Arc<Mutex<WatchdogSnapshot>>,
    /// The last publication the frame poll read, so the DTO and the tray show
    /// one coherent snapshot (the verdict is consumed into the history).
    watchdog: WatchdogSnapshot,

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
    /// The watchdog's active warning as last published by the shared thread.
    /// Shown in the tray; it never causes a route change.
    starvation_warning: Option<StarvationWarning>,
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
    /// The tolerant loader's diagnostic when an existing `config.toml` could
    /// not be used (the window is running on defaults). `None` on a clean load
    /// and on a first run. Feeds the DTO's `AppStatus.error` (`error.config`).
    config_recovery: Option<ConfigRecovery>,
    /// The relay connect attempt the window started, until the engine's own
    /// facts end it. Feeds `RelayStatus.connecting` (the §2.1 DTO gap).
    connect_attempt: Option<ConnectAttempt>,
    /// The shared app core: every engine mutation is a named command here, and
    /// its in-flight/last-error state is the DTO's action-loading and Error
    /// source (S3b).
    commands: AppCore,
    /// The no-position fallback ranking the chooser produced for this route,
    /// when it used one. Feeds `RelayStatus.fallback`.
    fallback_ranking: Option<lightspeed_client::app::dto::FallbackRanking>,

    // ── Game routing ──────────────────────────────────────────────────────
    selected_game_idx: usize,
    server_input: String,
    fec_enabled: bool,
    /// Whether the shipped WinDivert binaries are present, sampled ONCE at
    /// startup. It used to probe Npcap via `sc query npcap` - a blocked
    /// dependency and a blocking process spawn on the UI thread.
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
    /// The OS reduced-motion signal, sampled once at startup (spec W2-4 (i)).
    /// Only an explicit `On` animates; `Off`/`Unknown` render static text.
    animations: Animations,
    /// An in-flight elevation handshake, if any (spec §2.2 row 7 / C8). The
    /// frame loop polls it; the process exits only once it is `Confirmed`.
    elevation: Option<ElevationAttempt>,
    /// The catalogued notice a resolved elevation handshake leaves on screen
    /// (spec §2.2 row 7: a declined prompt returns to ready with
    /// `notice.elevation_declined`). Cleared when the user retries.
    elevation_notice: Option<&'static str>,
    fonts_setup: bool,
    header_icon: Option<egui::TextureHandle>,

    // ── Instrument 2.0 shell state (W2-2) ────────────────────
    /// Whether the route card's `Details` disclosure is open. Presentation
    /// state only; never persisted.
    details_open: bool,
    /// Whether the remembered window height has been applied this launch.
    height_restored: bool,
    /// The height last written to the GUI window-state file.
    persisted_height: f32,

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

    /// Whether the opt-in login entry is recorded in the config (WF-081b,
    /// task-74). The Settings toggle reconciles it with the platform entry once
    /// per sheet open; the platform entry is the observable truth.
    autostart: bool,
    /// Once-per-Settings-open guard for the platform reconcile above, so the
    /// registry/file probe is not re-run on every frame the sheet is open.
    autostart_synced: bool,
    /// A failed enable/disable write, shown under the toggle instead of
    /// pretending the entry exists.
    autostart_error: Option<String>,

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

/// The capture-only forced route state (`LIGHTSPEED_CAPTURE_STATE=connected|optimizing|starved|error`).
///
/// Direction C's review captures need the CONNECTED / OPTIMIZING / STARVED /
/// ERROR screens; a real judged starvation needs a live tunnel plus the watchdog
/// window, so the debug build can substitute those documented facts for a
/// screenshot. Release builds compile this to `None` (`cfg!(debug_assertions)`
/// is false), so the shipped app can never show a state the engine did not
/// produce.
/// A capture-only environment switch. Debug builds read the variable; release
/// builds always return `false`, so a shipped install can never be force-opened
/// into an editor/picker/details sheet by its environment (QA hardening).
fn capture_flag(name: &str) -> bool {
    cfg!(debug_assertions) && std::env::var_os(name).is_some()
}

fn capture_forced_state() -> Option<&'static str> {
    if !cfg!(debug_assertions) {
        return None;
    }
    match std::env::var("LIGHTSPEED_CAPTURE_STATE").ok().as_deref() {
        Some("connected") => Some("connected"),
        Some("optimizing") => Some("optimizing"),
        Some("starved") => Some("starved"),
        Some("error") => Some("error"),
        Some("disconnected") => Some("disconnected"),
        Some("connecting") => Some("connecting"),
        Some("holding") => Some("holding"),
        Some("elevation_required") => Some("elevation_required"),
        _ => None,
    }
}

/// The capture-only declined notice (`LIGHTSPEED_SHOW_ELEVATION_DECLINED`).
///
/// **Debug/evidence builds only**: the switch renders the declined state
/// deterministically for the 320x420 captures on a host where the interactive
/// UAC prompt cannot be answered. A release build returns `None`, so a shipped
/// install's environment can never force a notice the handshake did not produce.
fn capture_elevation_notice() -> Option<&'static str> {
    if !cfg!(debug_assertions) {
        return None;
    }
    std::env::var_os("LIGHTSPEED_SHOW_ELEVATION_DECLINED").map(|_| "notice.elevation_declined")
}

/// The catalogued notice a resolved elevation handshake leaves on screen.
///
/// Only a **declined** prompt leaves copy (`notice.elevation_declined`, spec
/// §2.2 row 7): the other resolved phases keep the window, log, and leave the
/// route surface's own notice in place. The state machine names the key, so the
/// sentence cannot drift from the phase.
fn elevation_declined_notice(phase: ElevationPhase) -> Option<&'static str> {
    match phase {
        ElevationPhase::Declined => phase.notice_key(),
        _ => None,
    }
}

/// The one warn notice the config region renders above the inputs it explains.
///
/// A live handshake outcome - a declined elevation prompt - outranks the
/// standing Needs-elevation notice from the route surface while it is present:
/// it says what just happened instead of repeating the requirement. It clears
/// when the user retries ([`LightSpeedApp::restart_elevated`]).
fn shell_notice(
    elevation_notice: Option<&'static str>,
    surface_notice: Option<&Line>,
) -> Option<Line> {
    match (elevation_notice, surface_notice) {
        (Some(key), _) => Some(Line::new(key)),
        (None, Some(line)) => Some(line.clone()),
        (None, None) => None,
    }
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
        let status = lock_or_recover(&engine).snapshot();

        // Detection is deliberately not run here: `new()` executes on the thread
        // that will pump the egui loop, and one scan spawns two processes. The
        // presence worker (see `app::presence`) performs the first scan as soon
        // as the first frame enables it, and `poll_game_presence` applies it.
        let auto_detected_game: Option<String> = None;

        let is_admin = P::is_admin();

        let loaded = config::load_unified(&paths::config_paths());
        // Keep the tolerant loader's diagnostic: the status DTO turns it into
        // the localized settings-error sentence instead of looking like a fresh
        // install. Only an unusable existing file produces one; a first run is
        // not an error.
        let config_recovery = loaded.recovery;
        let saved = loaded.config;
        // Apply the persisted path-MTU cap before any boost can start; the data
        // plane also lowers it on an EMSGSIZE rejection, so this is the user's
        // explicit floor rather than the only protection. It is a command, so a
        // failure would land in the Error row like any other.
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

        // The tray is created **after** the language is pinned: its menu labels
        // and tooltip are catalogued copy (`tray.*`/`state.*`), so building it
        // earlier would leave English labels behind a German config (W2-4 (c)).
        let tray = P::new_tray(Arc::clone(&quit));
        tracing::info!("system tray created: {}", tray.is_some());

        // One OS read for the whole process: the reduced-motion signal gates
        // every spinner (W2-4 (i)). A failed probe is `Unknown` = static.
        let animations = P::animations();
        tracing::info!(
            "reduced-motion signal: {:?} ({})",
            animations,
            animations.source()
        );

        // The shared watchdog publishes one coherent snapshot: warning, live
        // judged-window streak (R16) and the one-shot demotion finding.
        let watchdog_publication = Arc::new(Mutex::new(WatchdogSnapshot::default()));
        let mut commands = AppCore::new(Arc::clone(&engine));
        let _ = commands.set_path_mtu_cap(saved.tunnel_mtu);

        // The sampler that pairs tunnelled round trips drops every sample while
        // no estimator is installed, and the only installer was the CLI's probe
        // path - so in the GUI the whole-loop figure and the measured server legs
        // for relay ranking were both empty. Install it before traffic can arrive.
        lightspeed_client::ensure_destination_estimator();
        spawn_relay_reply_watchdog(
            Arc::clone(&engine),
            Arc::clone(&quit),
            Arc::clone(&watchdog_publication),
        );
        let mut app = Self {
            engine,
            status,
            tray,
            quit,
            watchdog_publication,
            watchdog: WatchdogSnapshot::default(),
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
            config_recovery,
            connect_attempt: None,
            commands,
            fallback_ranking: None,
            selected_game_idx,
            game_auto,
            game_manual,
            server_input: saved.manual_server.clone(),
            fec_enabled: false,
            capture_available: P::is_capture_available(),
            share_latency_stats,
            auto_detected_game,
            is_admin,
            animations,
            elevation: None,
            elevation_notice: capture_elevation_notice(),
            fonts_setup: false,
            header_icon: None,
            details_open: capture_flag("LIGHTSPEED_OPEN_DETAILS"),
            height_restored: false,
            persisted_height: 0.0,
            show_settings: std::env::var_os("LIGHTSPEED_OPEN_SETTINGS").is_some(),
            show_game_editor: capture_flag("LIGHTSPEED_OPEN_EDITOR"),
            game_editor: GameEditorDraft::new_profile(),
            editor_saved: None,
            editor_error: None,
            process_list: ProcessListState::Idle,
            process_rows: Vec::new(),
            process_search: String::new(),
            selected_custom,
            target_process: None,
            show_target_picker: capture_flag("LIGHTSPEED_OPEN_PICKER"),
            save_server_name: String::new(),
            server_saved: None,
            server_error: None,
            language,
            tunnel_mtu,
            autostart: saved.autostart,
            autostart_synced: false,
            autostart_error: None,
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
        // `select_relay` reconnects the control plane and publishes the new
        // relay through `session::current_proxy()`. The WinDivert tunnel task
        // reads that value for every outbound datagram, so the data plane moves
        // with the selection. Restarting interception here would tear down and
        // recreate the WinDivert handles for no reason, and repeated relay
        // switches could then hit the driver's stale WFP objects.
        if let Err(error) = self.commands.select_relay(addr, node_id) {
            tracing::error!("relay connect command failed: {error:?}");
            return;
        }
        // The DTO's `connecting` fact: this window asked for the link, and the
        // engine's own facts end the attempt (see `poll_health`).
        self.connect_attempt = Some(addr);
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
        let config = AppConfig {
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
            autostart: self.autostart,
            selected_game: if self.game_auto || self.game_manual {
                None
            } else {
                Some(self.selected_game().key)
            },
        };
        if let Err(e) = config::save_canonical(&paths::config_paths(), &config) {
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
        let server_location = lightspeed_client::app::location::chooser_point(server);

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
                self.fallback_ranking = None;
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
                let fallback =
                    rank_without_destination(server, &candidates, direct_us, &self.relay_history);
                let FallbackRanking {
                    ranked,
                    skipped_starved,
                    proven,
                } = fallback.clone();
                // The DTO carries the exact ranking the fallback used, so the
                // surface can never re-derive a different provenance.
                self.fallback_ranking = Some(fallback);
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
        // Resolve the window's connect attempt from the engine's own facts: the
        // keepalive coming up, registration completing or a registration error
        // all end the attempt (spec §2.1/§2.2 row 2). This poll runs every frame
        // from the frame loop, so the fact is settled before the DTO is read.
        if self.connect_attempt.is_some()
            && (self.status.connected
                || self.status.control_registered
                || self.status.registration_error.is_some())
        {
            self.connect_attempt = None;
        }
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
        let _ = self.commands.stop_boost();
        let _ = self.commands.disconnect();
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

        let Some(proxy) = self.selected_proxy_addr() else {
            return;
        };
        let game_key = self.selected_game().key;
        // The command owns the in-flight state and the Error row's source (S3b):
        // a failure comes back typed, with the engine's verbatim detail.
        if let Err(error) = self.commands.start_boost(
            &game_key,
            proxy,
            self.fec_enabled,
            4, // default FEC K
            port_range_override,
            process_target,
        ) {
            tracing::error!("start_interceptor failed: {error:?}");
            return;
        }
        self.boost_start = Some(std::time::Instant::now());
        self.relayed_base = None;
        {
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
            let held = self.commands.hold_tunnelling().unwrap_or(false);
            if held {
                self.held_since = Some(std::time::Instant::now());
                tracing::info!("Holding traffic on the direct path until the game server is known");
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
                let status = lock_or_recover(&self.engine).status_watch();
                let chosen = Arc::clone(&self.server_chosen);
                let armed = self
                    .commands
                    .release_hold_when_connected(move || {
                        chosen.load(Ordering::Relaxed)
                            && status.read().map(|s| s.connected).unwrap_or(false)
                    })
                    .unwrap_or(false);
                if !armed {
                    tracing::warn!("No release watcher: this run is not held");
                }
            } else {
                tracing::debug!("This backend has no tunnelling gate; starting unheld");
            }
        }
    }

    /// Pause boost backends; Quit uses the full-stop command instead.
    fn stop_boost(&mut self) {
        if let Err(error) = self.commands.pause_boost() {
            tracing::error!("pause command failed: {error:?}");
        }
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
    fn apply_watchdog_demotion(
        &mut self,
        server: SocketAddrV4,
        published_relay: Option<SocketAddrV4>,
    ) {
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
        // The shared watchdog publishes from its own thread; this poll mirrors the
        // snapshot for the shell and tray and consumes the one-shot demotion
        // finding. The live streak is published for real now (W2-3a / R16).
        {
            let mut published = lock_or_recover(&self.watchdog_publication);
            let verdict = published.verdict.take();
            self.watchdog = WatchdogSnapshot {
                warning: published.warning,
                streak: published.streak,
                verdict: None,
            };
            drop(published);
            self.starvation_warning = self.watchdog.warning;
            if let Some((server, relay)) = verdict {
                self.apply_watchdog_demotion(server, relay);
            }
        }
        if let Some(server) = health.as_ref().and_then(|health| health.detected_server) {
            self.refresh_server_ping(*server.ip());
        }
        self.log_latency_comparison();
        self.record_relay_history();

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
    ///
    /// Starts the handshake instead of exiting: the frame loop polls it and the
    /// process only exits once the elevated instance confirms (spec §2.2 row 7,
    /// C8). A second click while a prompt is open is ignored.
    fn restart_elevated(&mut self) {
        if self.elevation.is_some() {
            return;
        }
        // Retrying clears the previous outcome's notice: the user has acted on
        // it, so the old sentence would describe a prompt that is no longer the
        // latest event.
        self.elevation_notice = None;
        let attempt = P::begin_elevation();
        tracing::info!("elevation handshake: {:?}", attempt.phase());
        self.elevation = Some(attempt);
    }

    /// Poll an in-flight elevation handshake.
    ///
    /// The exit rule is the fix: only `Confirmed` may terminate this
    /// (unelevated) process. `Declined` and `Failed` keep the window alive and
    /// drop the attempt so the user can retry; the declined notice copy is
    /// named by the handshake (`notice.elevation_declined`) and its rendering is
    /// shell work outside this wave.
    fn poll_elevation(&mut self) {
        let Some(attempt) = self.elevation.as_mut() else {
            return;
        };
        let phase = attempt.poll();
        if phase.is_pending() {
            return;
        }
        // The exit rule is the fix: only a confirmed elevated instance may
        // terminate this process (C8). Declined/failed/ready keep the window.
        if phase.may_exit() {
            tracing::info!("elevated instance confirmed; exiting the unelevated process");
            std::process::exit(0);
        }
        if phase != ElevationPhase::Ready {
            let notice = phase.notice_key().unwrap_or("elevation failed");
            tracing::warn!("elevation handshake ended in {phase:?} ({notice}); staying open");
        }
        // The declined copy is rendered by the shell (spec §2.2 row 7): the
        // window stays open and says what happened. Other resolved phases keep
        // logging only, exactly as before.
        self.elevation_notice = elevation_declined_notice(phase);
        attempt.reset();
        self.elevation = None;
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
            let _ = self.commands.disconnect();
            // A user disconnect ends any connect attempt this window started; a
            // disconnected idle window must not report CONNECTING.
            self.connect_attempt = None;
        } else if self.selected_entry().is_some() {
            self.connect_selected();
        }
    }

    // ── Instrument 2.0 shell (W2-2) ───────────────────────────────────────

    /// Render a catalogue line (`i18n` stays a shell concern; the seam returns
    /// keys plus placeholders).
    fn line_text(&self, line: &Line) -> String {
        let args: Vec<(&str, &str)> = line
            .args
            .iter()
            .map(|(key, value)| (*key, value.as_str()))
            .collect();
        i18n::t_with(line.key, &args)
    }

    /// Direction C's hero (`gui-c-token-sheet.md` Â§4.1/Â§4.2): a strict two-slot
    /// crown (brand left; tone dot + version right, no second status text), the
    /// 30 px capped mono state word in `c.ink`, the readout (prose sub-line +
    /// ice relay-leg metric) and the tick ruler.
    fn shell_rail(&self, ui: &mut egui::Ui, surface: &RouteSurface, compact: bool) {
        let word = i18n::t(surface.word.key()).to_uppercase();
        let tone = surface.word.tone();
        // The hero word is `c.ink` in every tone (token sheet Â§1/Â§2): the tone
        // lives on the crown dot, the banner and the readout, never on the word.
        let state_ink = text_1;
        let status_ink = match tone {
            Tone::Signal => ice,
            Tone::Warn => warn,
            Tone::Danger => danger,
            Tone::Muted => text_3,
        };
        let top_h = if compact { TOP_H_COMPACT } else { TOP_H };
        let (bar, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), top_h),
            egui::Sense::hover(),
        );
        {
            let painter = ui.painter();
            painter.rect_filled(
                egui::Rect::from_min_max(egui::pos2(bar.left(), bar.bottom() - 1.0), bar.max),
                0.0,
                border_1,
            );
            let brand_font = egui::FontId::new(BRAND, console());
            paint_tracked(
                painter,
                egui::pos2(bar.left() + S4, bar.center().y),
                egui::Align2::LEFT_CENTER,
                "LIGHTSPEED",
                &brand_font,
                text_2,
                TRACK_BRAND,
            );
            let meta_font = egui::FontId::new(CAPTION, console());
            let version = env!("CARGO_PKG_VERSION");
            let version_w = tracked_width(painter, version, &meta_font, 0.0);
            paint_tracked(
                painter,
                egui::pos2(bar.right() - S4, bar.center().y),
                egui::Align2::RIGHT_CENTER,
                version,
                &meta_font,
                text_3,
                0.0,
            );
            let dot_x = bar.right() - S4 - version_w - S1 - 2.5;
            painter.circle_filled(egui::pos2(dot_x, bar.center().y), 2.5, status_ink);
        }
        let pad_top = if compact { 0.0 } else { S2 };
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: pad_top as i8,
                bottom: 0,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                // Hero: the 30 px capped mono state word, one line, no shrink.
                let (hero, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), HERO_H),
                    egui::Sense::hover(),
                );
                {
                    let font = egui::FontId::new(STATE, console());
                    paint_tracked(
                        ui.painter(),
                        egui::pos2(hero.left(), hero.center().y),
                        egui::Align2::LEFT_CENTER,
                        &word,
                        &font,
                        state_ink,
                        TRACK_STATE,
                    );
                }
                if compact {
                    ui.add_space(S1);
                } else {
                    ui.add_space(S2);
                }
                // Readout: prose sub-line left, ice relay-leg metric right with
                // its unit and leg caption (F1-safe; the metric is its own row).
                let readout_h = if compact { 34.0 } else { 46.0 };
                let (readout, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), readout_h),
                    egui::Sense::hover(),
                );
                {
                    let painter = ui.painter();
                    if !compact {
                        if let Some(line) = &surface.subline {
                            let text = self.line_text(line);
                            let font = egui::FontId::new(PROSE, prose());
                            // Reserve the metric block so the DE sub-line never
                            // reads as one run with "RELAISSTRECKE" (review m2).
                            let metric_reserve = (readout.width() * 0.42).max(76.0);
                            let wrap = (readout.width() - metric_reserve - S3).max(60.0);
                            let galley = painter.layout(text, font, text_2, wrap);
                            let at = egui::pos2(readout.left(), readout.bottom() - galley.size().y);
                            painter.galley(at, galley, text_2);
                        }
                    }
                    let (num, ink_m) = match surface.metric.value_ms {
                        Some(ms) => (format!("{ms:>3}"), ice),
                        None => ("--".to_string(), text_3),
                    };
                    let metric_size = if compact { 23.0 } else { METRIC };
                    let mfont = egui::FontId::new(metric_size, console());
                    let ufont = egui::FontId::new(CAPTION, console());
                    let capfont = egui::FontId::new(CAPTION, console());
                    let caption = i18n::t(surface.metric.caption_key).to_uppercase();
                    let num_w = tracked_width(painter, &num, &mfont, TRACK_VALUE);
                    let unit = "MS";
                    let unit_w = tracked_width(painter, unit, &ufont, 0.0);
                    let num_x = readout.right() - num_w - S1 - unit_w;
                    paint_tracked(
                        painter,
                        egui::pos2(num_x, readout.top()),
                        egui::Align2::LEFT_TOP,
                        &num,
                        &mfont,
                        ink_m,
                        TRACK_VALUE,
                    );
                    paint_tracked(
                        painter,
                        egui::pos2(num_x + num_w + S1, readout.top() + (metric_size - CAPTION)),
                        egui::Align2::LEFT_TOP,
                        unit,
                        &ufont,
                        ice_dim,
                        0.0,
                    );
                    if compact {
                        // §4.10 caption-line rule: reserve the numeral block's
                        // width and draw the caption in the remaining column
                        // (bottom-left), so a long RU/DE caption can never sit
                        // under the `218 MS` numeral.
                        let reserved = num_w + S1 + unit_w + S2;
                        let cap_w = (readout.width() - reserved).max(40.0);
                        let galley = painter.layout(caption, capfont, text_3, cap_w);
                        let at = egui::pos2(readout.left(), readout.bottom() - galley.size().y);
                        painter.galley(at, galley, text_3);
                    } else {
                        paint_tracked(
                            painter,
                            egui::pos2(readout.right(), readout.bottom()),
                            egui::Align2::RIGHT_BOTTOM,
                            &caption,
                            &capfont,
                            text_3,
                            TRACK_META,
                        );
                    }
                }
                if !compact {
                    ui.add_space(S2);
                }
                // Tick ruler: 1 px top rule + 1 px ticks every 8 px (Â§3).
                let (ruler, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), RULER_H),
                    egui::Sense::hover(),
                );
                {
                    let painter = ui.painter();
                    painter.line_segment(
                        [ruler.left_top(), ruler.right_top()],
                        egui::Stroke::new(1.0, border_2),
                    );
                    let mut x = ruler.left() + S2;
                    while x < ruler.right() {
                        painter.line_segment(
                            [
                                egui::pos2(x, ruler.top() + 1.0),
                                egui::pos2(x, ruler.bottom()),
                            ],
                            egui::Stroke::new(1.0, border_2),
                        );
                        x += 8.0;
                    }
                }
            });
    }

    /// The route data rail: the telemetry map plus the variable/stacked rows
    /// (`gui-c-token-sheet.md` Â§4.3/Â§4.4). No game-server point is drawn.
    fn shell_route_card(&mut self, ui: &mut egui::Ui, surface: &RouteSurface, compact: bool) {
        egui::Frame::new()
            .fill(bg_1)
            .stroke(egui::Stroke::new(1.0, border_1))
            .corner_radius(egui::CornerRadius::same(R_PANEL))
            .inner_margin(egui::Margin::same(S1 as i8))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.spacing_mut().item_spacing.y = 0.0;
                let markers = self.map_markers();
                globe::draw_telemetry_map(
                    ui,
                    ui.available_width(),
                    if compact { MAP_H_COMPACT } else { MAP_H },
                    &markers,
                );
                ui.add_space(S1);
                self.shell_ledger(ui, surface);
                if let Some(location) = &surface.location {
                    let label = i18n::t(location.label_key).to_uppercase();
                    let value = self.line_text(&location.rung);
                    let has_hint = location.hint.is_some()
                        || location.detail.is_some()
                        || location.note.is_some()
                        || location.as_is.is_some();
                    console_row(ui, &label, &value, text_2, VALUE, false, has_hint);
                    for extra in [&location.detail, &location.note].into_iter().flatten() {
                        let text = self.row_plain_text(extra);
                        prose_label(ui, &text, PROSE, text_3);
                    }
                    if let Some(hint) = location.hint {
                        prose_label(ui, &i18n::t(hint), PROSE, text_3);
                    }
                    if let Some(as_is) = location.as_is {
                        prose_label(ui, &i18n::t(as_is), PROSE, text_3);
                    }
                }
            });
    }

    /// The plain text of a row value (used for the location ladder's secondary
    /// lines; the styling decision stays with the caller).
    fn row_plain_text(&self, value: &RowValue) -> String {
        match value {
            RowValue::Data(text) => text.clone(),
            RowValue::Copy(line) => self.line_text(line),
            RowValue::Hint(key) => i18n::t(key).into_owned(),
            RowValue::Warn(line) => self.line_text(line),
        }
    }

    /// Build the map markers from the current session: the relay fleet with the
    /// active relay highlighted (ice) and the rest as fleet crosses. The client
    /// knows no game-server location, so none is invented (token sheet Â§4.4).
    fn map_markers(&self) -> Vec<globe::Marker> {
        let active_node = if self.status.connected
            || matches!(
                capture_forced_state(),
                Some("connected") | Some("optimizing") | Some("starved")
            ) {
            self.selected_entry()
                .and_then(|e| e.node_id.clone())
                .or_else(|| self.status.node_id.clone())
        } else {
            None
        };
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
                colour: if active { accent_text } else { map_fleet },
                label: entry.label.clone(),
            });
        }
        markers
    }

    /// The data rail: the default rows plus one `Details` disclosure holding the
    /// secondary rows (token sheet Â§4.3).
    fn shell_ledger(&mut self, ui: &mut egui::Ui, surface: &RouteSurface) {
        let total = surface.ledger.len();
        for (i, row) in surface.ledger.iter().enumerate() {
            let draw_rule = i + 1 < total || !surface.details.is_empty();
            self.shell_row(ui, row, draw_rule);
        }
        if surface.details.is_empty() {
            return;
        }
        let label = i18n::t("route.details").to_uppercase();
        let (rect, response) = ui.allocate_exact_size(
            egui::vec2(ui.available_width(), HIT_MIN),
            egui::Sense::click(),
        );
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::Role::Button, self.details_open, &label)
        });
        if response.hovered() {
            ui.painter()
                .rect_filled(rect, egui::CornerRadius::same(R_TAG), bg_2);
        }
        let painter = ui.painter();
        let font = egui::FontId::new(CAPTION, console());
        paint_tracked(
            painter,
            egui::pos2(rect.left() + S2, rect.center().y),
            egui::Align2::LEFT_CENTER,
            &label,
            &font,
            if response.hovered() { text_1 } else { text_2 },
            TRACK_LABEL,
        );
        let cx = rect.right() - S2;
        let cy = rect.center().y;
        let dir = if self.details_open { 1.0 } else { -1.0 };
        painter.add(egui::Shape::line(
            vec![
                egui::pos2(cx - 3.0, cy - 2.0 * dir),
                egui::pos2(cx + 3.0, cy + 2.0 * dir),
            ],
            egui::Stroke::new(1.5, text_3),
        ));
        focus_ring(ui, &response, R_TAG);
        if activated(ui, &response) {
            self.details_open = !self.details_open;
        }
        if self.details_open {
            let count = surface.details.len();
            for (i, row) in surface.details.iter().enumerate() {
                self.shell_row(ui, row, i + 1 < count);
            }
        } else {
            ui.add_space(S2);
        }
    }

    /// One data row in the console's **variable** language (token sheet Â§4.3):
    /// a max-content label column, the value right-aligned; when the pair does
    /// not fit the 288 px column â€” or the "label" is a full sentence â€” the row
    /// stacks instead of clipping. No fixed label grid.
    fn shell_row(&self, ui: &mut egui::Ui, row: &Row, draw_rule: bool) {
        let label_raw = i18n::t(row.label_key).into_owned();
        let label = label_raw.to_uppercase();
        let (value_text, mono, ink, size) = self.row_value_parts(&row.value);
        let label_font = egui::FontId::new(CAPTION, console());
        let value_font = egui::FontId::new(size, if mono { console() } else { prose() });
        let width = ui.available_width();
        let label_w = {
            let painter = ui.painter();
            tracked_width(painter, &label, &label_font, TRACK_LABEL)
        };
        let value_w = ui
            .painter()
            .layout_no_wrap(value_text.clone(), value_font.clone(), ink)
            .size()
            .x;
        let word_count = label_raw.split_whitespace().count();
        if word_count > 3 && label_w > width * 0.45 {
            let detail = if matches!(row.value, RowValue::Hint(_)) {
                None
            } else {
                Some(value_text.as_str())
            };
            console_sentence_row(ui, &label_raw, detail, draw_rule);
            return;
        }
        if !row_fits(label_w, value_w, width) {
            let value_galley = ui
                .painter()
                .layout(value_text.clone(), value_font, ink, width);
            let height = 16.0 + S1 + value_galley.size().y + S1;
            let (rect, _) =
                ui.allocate_exact_size(egui::vec2(width, height.max(ROW_H)), egui::Sense::hover());
            let painter = ui.painter();
            paint_tracked(
                painter,
                egui::pos2(rect.left(), rect.top() + CAPTION),
                egui::Align2::LEFT_CENTER,
                &label,
                &label_font,
                text_3,
                TRACK_LABEL,
            );
            painter.galley(
                egui::pos2(rect.left(), rect.top() + 16.0 + S1),
                value_galley,
                ink,
            );
            if draw_rule {
                painter.line_segment(
                    [
                        egui::pos2(rect.left(), rect.bottom() - 0.5),
                        egui::pos2(rect.right(), rect.bottom() - 0.5),
                    ],
                    egui::Stroke::new(1.0, border_1),
                );
            }
            return;
        }
        console_row(ui, &label, &value_text, ink, size, mono, draw_rule);
    }

    /// The rendered text, type and ink of a row value (Direction C: data is
    /// 12.5 px mono `ink`; hints are 11 px dim; warnings are 12 px prose warn).
    fn row_value_parts(&self, value: &RowValue) -> (String, bool, egui::Color32, f32) {
        match value {
            RowValue::Data(text) => (text.clone(), true, text_1, VALUE),
            RowValue::Copy(line) => (self.line_text(line), true, text_1, VALUE),
            RowValue::Hint(key) => (i18n::t(key).into_owned(), false, text_3, CAPTION),
            RowValue::Warn(line) => (self.line_text(line), false, warn, PROSE),
        }
    }

    /// The command-error panel (`Â§2.2` / `Â§2.9`): the taxonomy sentence, the
    /// verbatim engine detail behind `Details`, and an `Open log file` link â€”
    /// restyled to Direction C's danger panel (token sheet Â§4.9). The tag is the
    /// localized error word (D-C1: no English `ERROR / TUNNEL` literal).
    fn shell_error_panel(&mut self, ui: &mut egui::Ui, surface: &RouteSurface) {
        if surface.word != RouteWord::Error {
            return;
        }
        let detail = self
            .engine_error()
            .and_then(|error| error.detail.clone())
            .unwrap_or_default();
        let tag = i18n::t("state.error").to_uppercase();
        notice_frame(ui, true, |ui| {
            tracked_label(ui, &tag, CAPTION, danger, TRACK_TAG);
            if let Some(subline) = &surface.subline {
                ui.add_space(S1);
                let text = self.line_text(subline);
                prose_label(ui, &text, PROSE, danger_text);
            }
            let mut open = self.details_open;
            if disclosure_row(ui, i18n::t("route.details").as_ref(), &mut open) {
                self.details_open = !self.details_open;
            }
            if self.details_open && !detail.is_empty() {
                prose_label(ui, &detail, PROSE, text_2);
            }
            ui.add_space(S1);
            let open_log = console_link(ui, i18n::t("settings.open_log").as_ref());
            if activated(ui, &open_log) {
                self.reveal_log_file();
            }
        });
    }

    /// The one action slot + the disabled-reason line under it when the slot
    /// cannot act (`Â§2.9`, M6). Direction C: the single lime CTA (46 px in the
    /// compact tier) or the quiet danger outline, via `action_button`.
    fn shell_action(&mut self, ui: &mut egui::Ui, surface: &RouteSurface, compact: bool) {
        let action = &surface.action;
        let label = i18n::t(action.label_key).to_uppercase();
        let kind = if action.danger {
            ActionKind::Danger
        } else {
            ActionKind::Accent
        };
        let response = action_button(ui, &label, kind, action.enabled, action.in_flight, compact);
        if response.clicked() && action.enabled && !action.in_flight {
            self.apply_action(surface);
        }
        if let Some(reason) = &action.disabled_reason {
            ui.add_space(S1);
            ui.label(
                egui::RichText::new(self.line_text(reason))
                    .size(CAPTION)
                    .color(text_3),
            );
        }
    }
    /// Map the one slot to the session command (the shell changes no route
    /// itself; each branch is the existing command).
    fn apply_action(&mut self, surface: &RouteSurface) {
        if surface.action.danger {
            self.stop_boost();
            return;
        }
        if !self.is_admin {
            self.restart_elevated();
            return;
        }
        if self.run_boosting() {
            self.stop_boost();
        } else if self.game_manual && !self.server_input.trim().is_empty() {
            self.start_manual_redirect();
        } else {
            self.start_boost();
        }
    }

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
    fn session_sent(&self) -> Option<u64> {
        routed_count(
            self.status.redirect_active,
            self.status.redirect_pkts_out,
            self.status.interceptor_active,
            self.status.interceptor_tunnel_sent,
        )
    }

    fn run_boosting(&self) -> bool {
        self.status.interceptor_active
            || self.status.windivert_active
            || self.status.capture_active
            || self.status.redirect_active
    }

    // ── Status DTO v2 assembly (W2-1 S3a) ──────────────────────────────────

    /// The one status snapshot the presentation layer will render.
    ///
    /// Assembled from the engine snapshot, the facts the app already derives for
    /// its own readouts and the watchdog's published warning; nothing here is a
    /// second derivation of a displayed value. The W2-2 shell consumes this via
    /// `route_surface::route_surface` and derives no state itself (W2-4b
    /// confirmed the single call at the frame's one-snapshot seam and removed
    /// the stale W2-1 `dead_code` allowance).
    fn app_status(&self) -> app_dto::AppStatus {
        let boosting = self.run_boosting();
        let server = self.game_server_addr();
        let relay = self.selected_entry().map(|entry| entry.addr);
        let has_error = self.engine_error().is_some();
        let relayed = if boosting {
            let flow = server.and_then(|server| {
                app_dto::RelayedPair::for_flow(server, &self.status.interceptor_flows)
            });
            flow.or_else(|| {
                let base = self
                    .relayed_base
                    .as_ref()
                    .filter(|(key, _, _)| *key == self.session_key())
                    .map(|(_, sent, returned)| (*sent, *returned));
                session_pair(
                    self.status.redirect_active,
                    self.status.redirect_pkts_out,
                    self.status.redirect_pkts_in,
                    self.status.interceptor_active,
                    self.status.interceptor_tunnel_sent,
                    self.status.interceptor_from_proxy,
                    base,
                )
            })
        } else {
            None
        };
        let ranked = self
            .route_ranking
            .as_ref()
            .filter(|ranking| Some(ranking.server) == server)
            .map(|ranking| ranking.scored.clone())
            .unwrap_or_default();
        let server_leg_state = match (server, relay) {
            (Some(server), Some(relay)) if boosting && !self.holding => {
                let scored = self
                    .route_ranking
                    .as_ref()
                    .filter(|ranking| ranking.server == server)
                    .and_then(|ranking| scored_for_relay(&ranking.scored, relay));
                let measured = measured_server_leg(relay, server);
                let early = early_triangle_floor(
                    self.server_ping,
                    server,
                    self.ranked_client_leg_us(relay),
                );
                server_leg_state(scored, measured, early)
            }
            _ => app_dto::ServerLegState::Unknown,
        };
        let connect_attempt = self.connect_attempt;
        let mut status = app_dto::AppStatus {
            engine: self.status.clone(),
            interceptor: lock_or_recover(&self.engine)
                .interceptor_health()
                .map(|health| app_dto::InterceptorFacts {
                    detected_server: health.detected_server,
                    packets_intercepted: health.packets_intercepted,
                    packets_injected: health.packets_injected,
                    packets_reply_data: health.packets_reply_data,
                    errors: health.errors,
                    last_error: health.last_error,
                }),
            relay: app_dto::RelayStatus {
                relay,
                ranked: ranked.clone(),
                server,
                server_ping: server.and_then(|server| self.server_ping_for(server)),
                server_leg: server_leg_state,
                connecting: app_dto::connecting_fact(
                    self.status.connected,
                    self.status.control_registered,
                    self.status.registration_error.as_deref(),
                    connect_attempt.is_some(),
                ),
                choosing: app_dto::choosing_fact(
                    self.holding,
                    self.locked_addr.is_some(),
                    self.server_chosen.load(Ordering::Relaxed),
                ),
                cannot_beat_direct: self.cannot_beat_direct(),
                carry: match (relay, server) {
                    (Some(relay), Some(server)) => Some(self.carry_of(relay, server)),
                    _ => None,
                },
                fallback: self.fallback_ranking.clone(),
            },
            watchdog: app_dto::WatchdogStatus {
                warning: self.watchdog.warning,
                // W2-3a closes the S4 deferral: the shared watchdog publishes the
                // real judged-window streak, and `None` means no live streak.
                streak: self.watchdog.streak,
            },
            location: server
                .map(|server| self.location_status(server))
                .unwrap_or_default(),
            server_leg: self.relay_measured_status(),
            route_trace: self.client_route,
            boosting,
            relayed,
            error: self.engine_error(),
            session: self.commands.session().clone(),
            capabilities: app_dto::Capabilities {
                elevated: self.is_admin,
            },
            tray: tray_state_for(
                has_error,
                self.watchdog.warning.is_some() && boosting,
                boosting,
                self.status.connected,
            ),
        };
        // Capture-only fixture (debug builds only, see `capture_forced_state`).
        if let Some(forced) = capture_forced_state() {
            let fixture_server = Ipv4Addr::new(203, 0, 113, 42);
            let fixture_server_addr = SocketAddrV4::new(fixture_server, 27_015);
            match forced {
                "connected" | "optimizing" | "starved" => {
                    status.engine.connected = true;
                    status.engine.redirect_game = "robun".to_string();
                    status.relay.relay = self.selected_entry().map(|entry| entry.addr);
                    status.relay.server = Some(fixture_server);
                    status.location = self.location_status(fixture_server);
                    match forced {
                        "connected" => {
                            status.engine.latest_rtt_ms = 218.0;
                            status.relayed = None;
                        }
                        "optimizing" => {
                            status.boosting = true;
                            status.engine.tunnelling_enabled = Some(true);
                            status.engine.latest_rtt_ms = 42.0;
                            status.relayed = Some(app_dto::RelayedPair {
                                sent: 1_284,
                                returned: 1_196,
                            });
                        }
                        _ => {
                            status.boosting = true;
                            status.engine.tunnelling_enabled = Some(true);
                            status.engine.latest_rtt_ms = 146.0;
                            status.watchdog.warning = Some(app_dto::StarvationWarning {
                                server: fixture_server_addr,
                                relay: self.selected_entry().map(|entry| entry.addr),
                                fault: app_dto::ReturnPathFault::NoReplyFromRelay,
                            });
                            status.watchdog.streak = Some(4);
                            status.relayed = Some(app_dto::RelayedPair {
                                sent: 1_284,
                                returned: 0,
                            });
                        }
                    }
                }
                "disconnected" => {
                    status.engine.connected = false;
                    status.relay.connecting = false;
                    status.boosting = false;
                    status.relayed = None;
                    status.relay.server = None;
                    status.engine.latest_rtt_ms = 0.0;
                }
                "connecting" => {
                    status.engine.connected = false;
                    status.relay.relay = self.selected_entry().map(|entry| entry.addr);
                    status.relay.connecting = true;
                    status.boosting = false;
                    status.relayed = None;
                }
                "holding" => {
                    status.engine.connected = true;
                    status.boosting = true;
                    status.engine.tunnelling_enabled = Some(false);
                    status.relay.relay = self.selected_entry().map(|entry| entry.addr);
                    status.relay.server = None;
                    status.relayed = None;
                    status.engine.latest_rtt_ms = 0.0;
                }
                "elevation_required" => {
                    status.engine.connected = true;
                    status.relay.relay = self.selected_entry().map(|entry| entry.addr);
                    status.capabilities.elevated = false;
                    status.boosting = false;
                    status.engine.latest_rtt_ms = 0.0;
                }
                "error" => {
                    status.boosting = false;
                    status.error = Some(app_dto::AppError::for_kind(
                        app_dto::AppErrorKind::Tunnel,
                        "capture fixture: the tunnel could not be started",
                    ));
                }
                _ => {}
            }
        }
        status
    }

    /// The route's carry memory as the shared filter sees it.
    fn carry_of(
        &self,
        relay: SocketAddrV4,
        server: Ipv4Addr,
    ) -> lightspeed_client::app::dto::CarryVerdict {
        use lightspeed_client::app::dto::CarryVerdict;
        match crate::relay_history::RelayHistory::carry_verdict(&self.relay_history, relay, server)
        {
            crate::relay_history::CarryVerdict::Proven => CarryVerdict::Proven,
            crate::relay_history::CarryVerdict::Starved => CarryVerdict::Starved,
            crate::relay_history::CarryVerdict::Unproven => CarryVerdict::Unproven,
        }
    }

    /// The current error fact (spec §2.8): the last failed command, else the
    /// tolerant loader's recovery, else a live engine/registration failure.
    fn engine_error(&self) -> Option<app_dto::AppError> {
        if let Some(error) = self.commands.session().last_error.as_ref() {
            return Some(error.clone());
        }
        if let Some(recovery) = &self.config_recovery {
            return Some(app_dto::AppError::config(recovery));
        }
        if let Some(error) = &self.status.registration_error {
            return Some(app_dto::AppError::registration(error.clone()));
        }
        for (kind, detail) in [
            (app_dto::AppErrorKind::Capture, &self.status.windivert_error),
            (app_dto::AppErrorKind::Capture, &self.status.capture_error),
            (app_dto::AppErrorKind::Tunnel, &self.status.redirect_error),
            (
                app_dto::AppErrorKind::Tunnel,
                &self.status.interceptor_error,
            ),
        ] {
            if let Some(detail) = detail {
                return Some(app_dto::AppError::for_kind(kind, detail.clone()));
            }
        }
        None
    }

    /// The honest location ladder for `server` (amendment A8).
    fn location_status(&self, server: Ipv4Addr) -> app_dto::LocationStatus {
        use app_dto::{GeofeedProvenance, LocationRung, LocationStatus};
        use lightspeed_client::app::location::Place;
        let mut status = LocationStatus {
            display_point: lightspeed_client::app::location::display_point(server),
            chooser_point: lightspeed_client::app::location::chooser_point(server),
            ..LocationStatus::default()
        };
        match lightspeed_client::app::location::place(server) {
            Place::Cloud {
                provider, region, ..
            } => {
                status.rung = LocationRung::Cloud;
                status.provider = Some(provider);
                status.region = Some(region);
            }
            Place::Geofeed {
                provider,
                country,
                region,
                city,
                source,
                as_of,
            } => {
                status.rung = LocationRung::Geofeed;
                status.geofeed = Some(GeofeedProvenance {
                    provider,
                    country,
                    region,
                    city,
                    source,
                    as_of,
                });
                // The feed has no coordinate: no dot, ever.
                status.display_point = None;
            }
            Place::Hosting { asn, name } => {
                status.rung = LocationRung::Hosting;
                status.asn = Some(asn);
                status.hosting = Some(name);
                // The country beside a hosting range is exactly what the free
                // databases get wrong; the ladder draws no dot for it.
                status.display_point = None;
            }
            Place::Country { code, .. } => {
                status.rung = LocationRung::Country;
                status.country = Some(code);
            }
            Place::Unknown => {
                status.rung = LocationRung::Unknown;
                status.display_point = None;
            }
        }
        status
    }

    /// The relay-measured leg rows for the pair on screen.
    fn relay_measured_status(&self) -> app_dto::ServerLegStatus {
        if !self.run_boosting() || self.holding {
            return app_dto::ServerLegStatus::default();
        }
        let Some(server) = self.game_server_addr() else {
            return app_dto::ServerLegStatus::default();
        };
        let Some(relay) = self.selected_entry().map(|entry| entry.addr) else {
            return app_dto::ServerLegStatus::default();
        };
        let icmp = relay_measured_server_leg(relay, server);
        app_dto::ServerLegStatus {
            icmp,
            app: relay_measured_app_server_leg(relay, server),
            trace: relay_measured_trace(relay, server),
            expected_path_ms: icmp.and_then(|report| {
                app_dto::expected_path_ms(self.status.latest_rtt_ms, report.median_us)
            }),
        }
    }

    fn config_body(&mut self, ui: &mut egui::Ui, _narrow: bool, _short: bool) {
        ui.spacing_mut().item_spacing.y = 0.0;
        // Relay: one inline console row (label left, picker right, Â§4.7).
        console_field_row(ui, &i18n::t("route.relay"), |ui| {
            self.boost_server_combo(ui)
        });
        if self.status.connected {
            let disconnect = console_link(ui, i18n::t("action.disconnect").as_ref());
            if activated(ui, &disconnect) {
                self.toggle_relay_connection();
            }
        }
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
        ui.add_space(S1);
        console_field_row(ui, &i18n::t("route.game"), |ui| self.game_combo(ui));
        if let Some(ref detected) = self.auto_detected_game {
            ui.horizontal_wrapped(|ui| {
                let (dot, _) = ui.allocate_exact_size(egui::vec2(4.0, 16.0), egui::Sense::hover());
                ui.painter().circle_filled(dot.center(), 2.0, signal);
                ui.label(
                    egui::RichText::new(i18n::t("game.found"))
                        .size(CAPTION)
                        .color(accent_text),
                );
                ui.label(
                    egui::RichText::new(detected.clone())
                        .size(CAPTION)
                        .color(text_1),
                );
            });
        } else if let Some(key) = game_caption(
            self.game_auto,
            self.game_manual,
            self.auto_detected_game.as_deref(),
        ) {
            ui.label(
                egui::RichText::new(i18n::t(key))
                    .size(CAPTION)
                    .color(text_3),
            );
        }
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
                    // One wait indicator; motion only when the platform's OS
                    // reduced-motion read is explicitly `On` (W2-4 (i)).
                    progress_indicator(ui, i18n::t("relay.discovering").as_ref(), self.animations);
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
                let (head, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 24.0),
                    egui::Sense::hover(),
                );
                {
                    let painter = ui.painter();
                    let font = egui::FontId::new(TITLE, console());
                    let slash_w = paint_tracked(
                        painter,
                        egui::pos2(head.left(), head.center().y),
                        egui::Align2::LEFT_CENTER,
                        "///",
                        &font,
                        ice_dim,
                        0.0,
                    );
                    paint_tracked(
                        painter,
                        egui::pos2(head.left() + slash_w + S2, head.center().y),
                        egui::Align2::LEFT_CENTER,
                        &i18n::t("settings.title").to_uppercase(),
                        &font,
                        text_1,
                        TRACK_SECTION,
                    );
                }
                let close_rect = egui::Rect::from_min_size(
                    egui::pos2(head.right() - 22.0, head.top() + 1.0),
                    egui::vec2(22.0, 22.0),
                );
                let close_response = ui.interact(
                    close_rect,
                    ui.scope_id().with("settings_close"),
                    egui::Sense::click(),
                );
                close_response.widget_info(|| {
                    egui::WidgetInfo::labeled(
                        egui::Role::Button,
                        true,
                        i18n::t("action.close").as_ref(),
                    )
                });
                if close_response.hovered() {
                    ui.painter()
                        .rect_filled(close_rect, egui::CornerRadius::same(R_TAG), bg_2);
                }
                ui.painter().rect(
                    close_rect,
                    egui::CornerRadius::same(R_TAG),
                    bg_1,
                    egui::Stroke::new(1.0, border_control),
                    egui::StrokeKind::Inside,
                );
                let xfont = egui::FontId::new(CAPTION, console());
                paint_tracked(
                    ui.painter(),
                    close_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "X",
                    &xfont,
                    text_2,
                    0.0,
                );
                focus_ring(ui, &close_response, R_TAG);
                if activated(ui, &close_response) {
                    close = true;
                }
                let (rule, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rule, 0.0, border_1);
                ui.add_space(S2);
                let body_h = (max_h - 150.0).max(240.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(ui.available_width(), body_h),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        egui::ScrollArea::vertical()
                            .max_height(body_h)
                            .scroll_bar_visibility(
                                egui::scroll_area::ScrollBarVisibility::AlwaysVisible,
                            )
                            .show(ui, |ui| {
                                self.settings_body(ui);
                            });
                    },
                );
            });
        if response.should_close() || close {
            self.show_settings = false;
        }
    }

    fn settings_body(&mut self, ui: &mut egui::Ui) {
        ui.spacing_mut().item_spacing.y = S1;

        // 01 Language.
        console_section(
            ui,
            Some("01"),
            Some(i18n::t("settings.language").as_ref()),
            None,
        );
        let selected = i18n::current();
        let label: String = i18n::LOCALES
            .iter()
            .find(|locale| locale.tag == selected)
            .map(|locale| locale.native_name.to_string())
            .unwrap_or_else(|| i18n::t("settings.follow_system").into_owned());
        console_field_row(ui, &i18n::t("settings.language"), |ui| {
            let combo = egui::ComboBox::from_id_salt("language_select")
                .selected_text(egui::RichText::new(label.clone()).size(LABEL))
                .width(ui.available_width())
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(
                            self.language.is_none(),
                            i18n::t("settings.follow_system"),
                        )
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
            focus_ring(ui, &combo.response, R_TAG);
        });
        ui.add_space(S2);

        // 02 Startup.
        console_section(
            ui,
            Some("02"),
            Some(i18n::t("settings.startup").as_ref()),
            None,
        );
        if !self.autostart_synced {
            self.autostart_synced = true;
            let installed = P::autostart_installed();
            if installed != self.autostart {
                tracing::info!(
                    "autostart entry installed={installed}; recording it over the stored preference"
                );
                self.autostart = installed;
                self.persist_config();
            }
        }
        let toggle = console_toggle(ui, &i18n::t("settings.autostart"), self.autostart);
        if toggle.changed() {
            self.autostart = !self.autostart;
            let enabled = self.autostart;
            match P::set_autostart(enabled) {
                Ok(()) => {
                    self.autostart_error = None;
                    self.persist_config();
                    tracing::info!(
                        "autostart entry {}",
                        if enabled { "enabled" } else { "disabled" }
                    );
                }
                Err(error) => {
                    tracing::warn!("autostart write failed: {error}");
                    self.autostart = !enabled;
                    self.autostart_error = Some(error);
                }
            }
        }
        if let Some(error) = &self.autostart_error {
            prose_label(
                ui,
                &format!("{}: {error}", i18n::t("error.io")),
                PROSE,
                danger,
            );
        }
        prose_label(ui, &i18n::t("settings.autostart_hint"), PROSE, text_3);
        ui.add_space(S2);

        // 03 Privacy + Q13 disclosure.
        console_section(
            ui,
            Some("03"),
            Some(i18n::t("settings.privacy").as_ref()),
            None,
        );
        let toggle = console_toggle(
            ui,
            &i18n::t("settings.share_stats"),
            self.share_latency_stats,
        );
        if toggle.changed() {
            self.share_latency_stats = !self.share_latency_stats;
            self.persist_config();
            self.engine
                .lock()
                .unwrap_or_else(|poisoned| poisoned.into_inner())
                .set_telemetry_enabled(self.share_latency_stats);
        }
        console_editorial(ui, &i18n::t("update.disclosure"));
        ui.add_space(S2);

        // 04 Routing.
        console_section(
            ui,
            Some("04"),
            Some(i18n::t("settings.routing").as_ref()),
            None,
        );
        console_editorial(ui, &i18n::t("settings.starvation_policy"));
        let hold_minutes = (HOLD_FALLBACK.as_secs() / 60).to_string();
        prose_label(
            ui,
            &i18n::t_with(
                "settings.hold_policy",
                &[("minutes", hold_minutes.as_str())],
            ),
            PROSE,
            text_3,
        );
        ui.add_space(S2);

        // 05 Relays.
        console_section(
            ui,
            Some("05"),
            Some(i18n::t("settings.relays").as_ref()),
            None,
        );
        self.proxy_manager_body(ui);
        ui.horizontal_wrapped(|ui| {
            let remeasure = console_link(ui, i18n::t("settings.remeasure").as_ref());
            if activated(ui, &remeasure) {
                self.start_relay_measurement();
            }
            let refresh = console_link(ui, i18n::t("proxies.refresh").as_ref());
            if activated(ui, &refresh) {
                self.start_discovery();
            }
        });
        ui.add_space(S2);

        // 06 Maintenance.
        console_section(
            ui,
            Some("06"),
            Some(i18n::t("settings.maintenance").as_ref()),
            None,
        );
        ui.horizontal_wrapped(|ui| {
            let updates = console_link(ui, i18n::t("update.sheet_title").as_ref());
            if activated(ui, &updates) {
                self.start_update_check();
            }
            let open_log = console_link(ui, i18n::t("settings.open_log").as_ref());
            if activated(ui, &open_log) {
                self.reveal_log_file();
            }
        });
        ui.add_space(S2);

        // 07 Components (literal diagnostic rows; stacked when long).
        console_section(ui, Some("07"), Some("Components"), None);
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
        let driver = if self.capture_available {
            "WinDivert files present"
        } else {
            "WinDivert missing"
        };
        let relays = format!("{} configured", self.proxies.len());
        for (label, value) in [
            ("Version", env!("CARGO_PKG_VERSION")),
            ("Elevation", admin),
            ("Capture", capture),
            ("Driver", driver),
            ("Relays", relays.as_str()),
        ] {
            console_mini_row(ui, label, value);
        }
        ui.add_space(S2);

        // 08 About.
        console_section(
            ui,
            Some("08"),
            Some(i18n::t("settings.about").as_ref()),
            None,
        );
        ui.horizontal(|ui| {
            if let Some(mark) = self.header_icon.as_ref() {
                ui.add(egui::Image::from_texture(mark).fit_to_exact_size(egui::vec2(24.0, 24.0)));
            }
            ui.vertical(|ui| {
                tracked_label(ui, "LIGHTSPEED", BRAND, text_2, TRACK_BRAND);
                let version =
                    i18n::t_with("about.version", &[("version", env!("CARGO_PKG_VERSION"))]);
                tracked_label(ui, &version.to_uppercase(), CAPTION, text_3, TRACK_META);
            });
        });
        prose_label(ui, &i18n::t("about.blurb"), PROSE, text_3);
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
        ui.horizontal_wrapped(|ui| {
            prose_label(ui, &i18n::t("about.enjoying"), PROSE, text_2);
            ui.hyperlink_to(
                egui::RichText::new(i18n::t("about.star_github")).size(CAPTION),
                "https://github.com/ShibbityShwab/lightspeed/stargazers",
            )
            .on_hover_text(i18n::t("about.star_hint"));
        });
        ui.add_space(S3);
    }
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
                prose_label(ui, &i18n::t("game.target_process_hint"), PROSE, text_3);
                let (rule, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rule, 0.0, border_1);
                ui.add_space(S2);
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
                ui.horizontal_wrapped(|ui| {
                    if current.is_some() {
                        let clear_link = console_link(ui, i18n::t("game.target_clear").as_ref());
                        if activated(ui, &clear_link) {
                            clear = true;
                        }
                    }
                    let close_link = console_link(ui, i18n::t("action.close").as_ref());
                    if activated(ui, &close_link) {
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
            let game_name = entry.display.to_string();
            if let Err(error) = self.commands.start_redirect(
                server_addr,
                local_port,
                self.fec_enabled,
                4,
                &game_name,
                proxy,
            ) {
                tracing::error!("start_redirect failed: {error:?}");
            }
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
            match config::reset_to_defaults(&paths::config_paths()) {
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
                                progress_indicator(ui, i18n::t("update.checking").as_ref(), self.animations);
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
                                prose_label(
                                    ui,
                                    &i18n::t_with(
                                        "update.current",
                                        &[("version", current.as_str())],
                                    ),
                                    PROSE,
                                    text_2,
                                );
                                prose_label(
                                    ui,
                                    &i18n::t_with(
                                        "update.latest",
                                        &[("version", latest.as_str())],
                                    ),
                                    PROSE,
                                    text_2,
                                );
                                prose_label(
                                    ui,
                                    &crate::update::update_status_line(result),
                                    PROSE,
                                    text_2,
                                );

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
                                progress_indicator(ui, i18n::t("update.installing").as_ref(), self.animations);
                                prose_label(
                                    ui,
                                    "Approve the UAC prompt if one appears; LightSpeed will restart.",
                                    PROSE,
                                    text_3,
                                );
                            }
                            UpdateCheckState::Idle => {}
                        }
                        // A11 (Q18): the disclosure belongs to the surface that
                        // acts, in every state - not only after a result.
                        if !matches!(self.update_check, UpdateCheckState::Idle) {
                            ui.add_space(S1);
                            update_disclosure(ui);
                        }
                    });
                ui.add_space(S2);
                let close = console_link(ui, i18n::t("action.close").as_ref());
                if activated(ui, &close) {
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

/// The A11 update-check disclosure (spec §2.5 / Q18).
///
/// The update sheet is the surface that acts, so it states the contract: the
/// check is user-initiated only, contacts GitHub's release API for the latest
/// release, sends nothing but the request, and no startup or background check
/// exists. Catalogued copy so the promise is localized with the rest of the
/// sheet (English fallback until Q5 lands in some locales).
fn update_disclosure(ui: &mut egui::Ui) {
    console_editorial(ui, &i18n::t("update.disclosure"));
}

// ── eframe::App impl ─────────────────────────────────────────────────────────

impl<P: Platform> eframe::App for LightSpeedApp<P> {
    fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        // The full window rect, read before any panel claims space, drives the
        // tier breakpoints (spec §1.4: normal ≥560, short <560, compact <480).
        let viewport = ui.available_rect_before_wrap();
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

        // ── App-scope shortcuts (canonical `platform::shortcuts` table) ─────
        // Esc/Enter are sheet semantics and stay with the sheets; the three
        // app-wide chords are consumed here, before any widget sees them.
        // Hide-to-tray is inert without a real tray (it would strand the
        // window) and under automation's `LIGHTSPEED_KEEP_VISIBLE` pin.
        let can_hide =
            self.tray_available() && std::env::var_os("LIGHTSPEED_KEEP_VISIBLE").is_none();
        match shortcut_effect(take_app_shortcut(&ctx), can_hide) {
            ShortcutEffect::Quit => {
                tracing::info!("Ctrl+Q: quitting from the canonical shortcut table");
                save_window_height(viewport.height());
                self.quit.store(true, Ordering::SeqCst);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            ShortcutEffect::OpenSettings => self.show_settings = true,
            ShortcutEffect::HideToTray => {
                tracing::info!("Ctrl+H: hiding to the system tray");
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            ShortcutEffect::IgnoredNoTray => {
                tracing::info!("hide-to-tray shortcut ignored: no real system tray");
            }
            ShortcutEffect::Idle => {}
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
        self.poll_elevation();

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

        // ── One status snapshot for the whole frame (W2-1 seam) ─────────────
        let surface = route_surface::route_surface(&self.app_status());

        // ── Tray icon state machine ─────────────────────────────────────────
        // The tray projects the same snapshot, so `RelayStarved` can never be
        // painted as `Optimizing` (`app/window.rs` ordering).
        if let Some(tray) = &self.tray {
            tray.set_state(surface.tray, self.status.latest_rtt_ms);
        }

        // ── Geometry: remember the player's height, never auto-resize ───────
        // W2-2 removed the content-driven resize. The height belongs to the
        // player: restore the remembered value once, then persist changes.
        if !self.height_restored {
            self.height_restored = true;
            let height = load_window_height(monitor_height, window_top);
            self.persisted_height = height;
            ctx.send_viewport_cmd(egui::ViewportCommand::InnerSize(egui::vec2(320.0, height)));
        }
        if (viewport.height() - self.persisted_height).abs() > 8.0 {
            let clamped = viewport.height().clamp(
                MIN_WINDOW_HEIGHT,
                window_height_limit(monitor_height, window_top),
            );
            save_window_height(clamped);
            self.persisted_height = clamped;
        }
        // ── The one tier decision for this frame (spec §1.4) ────────────────
        // The 420 px minimum is compact: the same height value drives both the
        // rail/card fold (`short`) and the footer target tier (`compact`).
        let tier = shell_tier(viewport.height());
        let short = tier.is_short();
        let compact = tier.is_compact();

        // â”€â”€ Status window shell (Direction C) â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€â”€
        // One 320 px column: the 30 px brand/status bar + hero + tick ruler are
        // fixed; the route data rail, the control rail and every notice live in
        // the one scrolling region; the single CTA and the three-target footer
        // are always pinned (token sheet Â§4.10).
        egui::Panel::top("state_rail")
            .frame(egui::Frame::new().fill(bg_0))
            .show(ui, |ui| {
                self.shell_rail(ui, &surface, compact);
            });

        // Footer: flat console links under a 1 px rule (Â§4.6). When the
        // localized trio cannot share the 288 px row (DE), the footer wraps into
        // intentional rows; the compact wrapped row is 30 px so the 420 tier's
        // â‰¥180 px scroll floor still holds.
        egui::Panel::bottom("footer")
            .frame(egui::Frame::new().fill(bg_0))
            .show(ui, |ui| {
                let (rule, _) = ui.allocate_exact_size(
                    egui::vec2(ui.available_width(), 1.0),
                    egui::Sense::hover(),
                );
                ui.painter().rect_filled(rule, 0.0, border_1);
                egui::Frame::new()
                    .inner_margin(egui::Margin {
                        left: S4 as i8,
                        right: S4 as i8,
                        top: 0,
                        bottom: 0,
                    })
                    .show(ui, |ui| {
                        let can_hide = self.tray_available()
                            && std::env::var_os("LIGHTSPEED_KEEP_VISIBLE").is_none();
                        let mut entries: Vec<(std::borrow::Cow<'static, str>, u8)> = Vec::new();
                        if can_hide {
                            entries.push((i18n::t("window.hide_to_tray"), 0));
                        }
                        entries.push((i18n::t("settings.title"), 1));
                        entries.push((i18n::t("action.quit"), 2));
                        let height = if compact { FOOTER_H_COMPACT } else { FOOTER_H };
                        let font = egui::FontId::new(CAPTION, console());
                        let base_width = |index: usize| {
                            let label = entries[index].0.to_uppercase();
                            tracked_width(ui.painter(), &label, &font, 0.0) + 4.0 * S2
                        };
                        let per_column = ui.available_width() / entries.len().max(1) as f32;
                        let non_ascii = entries.iter().any(|(label, _)| !label.is_ascii());
                        let crowded = (0..entries.len()).any(|index| {
                            let label = entries[index].0.to_uppercase();
                            let width = tracked_width(ui.painter(), &label, &font, 0.0);
                            width > per_column - S2 || (non_ascii && width > per_column - 5.0 * S2)
                        });
                        let mut rows: Vec<Vec<usize>> = if !crowded
                            && (0..entries.len()).map(base_width).sum::<f32>()
                                <= ui.available_width()
                        {
                            vec![(0..entries.len()).collect()]
                        } else {
                            vec![vec![0], (1..entries.len()).collect()]
                        };
                        ui.spacing_mut().item_spacing.y = 0.0;
                        let wrapped = rows.len() > 1;
                        let row_height = if wrapped && compact { 30.0 } else { height };
                        for row in rows.drain(..) {
                            let labels: Vec<&str> =
                                row.iter().map(|index| entries[*index].0.as_ref()).collect();
                            if let Some(local) = console_footer(ui, &labels, row_height) {
                                match entries[row[local]].1 {
                                    0 => {
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(
                                            false,
                                        ));
                                    }
                                    1 => {
                                        self.show_settings = true;
                                    }
                                    _ => {
                                        save_window_height(viewport.height());
                                        self.quit.store(true, Ordering::SeqCst);
                                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                                    }
                                }
                            }
                        }
                    });
            });

        // Action bar: the one CTA + its disabled-reason line, always above the
        // footer and never off-screen.
        egui::Panel::bottom("action_bar")
            .frame(egui::Frame::new().fill(bg_0).inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: (if compact { S1 } else { S2 }) as i8,
                bottom: (if compact { S1 } else { S2 }) as i8,
            }))
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                self.shell_action(ui, &surface, compact);
            });

        // The only scrolling region: error panel, warning banner, notices, the
        // route data rail (01) and the control rail (02).
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(bg_0).inner_margin(egui::Margin {
                left: S4 as i8,
                right: S4 as i8,
                top: S2 as i8,
                bottom: S2 as i8,
            }))
            .show(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .show(ui, |ui| {
                        ui.set_width(ui.available_width());
                        if surface.word == RouteWord::Error {
                            self.shell_error_panel(ui, &surface);
                            ui.add_space(S2);
                        }
                        if let Some(warning) = &surface.warning {
                            let body = self.line_text(warning);
                            let tag = i18n::t(surface.word.key()).to_uppercase();
                            notice_frame(ui, true, |ui| {
                                tracked_label(ui, &tag, CAPTION, danger, TRACK_TAG);
                                ui.add_space(S1);
                                prose_label(ui, &body, PROSE, danger_text);
                            });
                            ui.add_space(S2);
                        }
                        if let Some(notice) =
                            shell_notice(self.elevation_notice, surface.notice.as_ref())
                        {
                            let text = self.line_text(&notice);
                            // UX round 3 F1: the shipped shell renders the
                            // elevation notice without a tag — the hero is the
                            // one state expression; no hardcoded HOLDING label.
                            notice_frame(ui, false, |ui| {
                                prose_label(ui, &text, PROSE, warn);
                            });
                            ui.add_space(S2);
                        }
                        let relay_meta = self
                            .selected_entry()
                            .map(|entry| entry.label.to_uppercase())
                            .or_else(|| {
                                self.status.node_id.clone().map(|node| node.to_uppercase())
                            });
                        console_section(
                            ui,
                            Some("01"),
                            Some(i18n::t("route.title").as_ref()),
                            relay_meta.as_deref(),
                        );
                        ui.add_space(S2);
                        self.shell_route_card(ui, &surface, compact);
                        ui.add_space(S3);
                        let game_meta = self
                            .auto_detected_game
                            .clone()
                            .map(|game| game.to_uppercase());
                        console_section(
                            ui,
                            Some("02"),
                            Some(i18n::t("action.manage").as_ref()),
                            game_meta.as_deref(),
                        );
                        ui.add_space(S2);
                        egui::Frame::new()
                            .fill(bg_1)
                            .stroke(egui::Stroke::new(1.0, border_1))
                            .corner_radius(egui::CornerRadius::same(R_PANEL))
                            .inner_margin(egui::Margin::same(S2 as i8))
                            .show(ui, |ui| {
                                ui.set_width(ui.available_width());
                                self.config_body(ui, true, short);
                            });
                        ui.add_space(S3);
                    });
            });
        // ── In-window sheets ─────────────────────────────────────────────
        // Leaving Settings clears the once-per-open platform reconcile guard, so
        // the next open re-reads the real login entry instead of a stale value.
        if !self.show_settings {
            self.autostart_synced = false;
        }
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
