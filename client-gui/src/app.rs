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
use crate::platform::{default_port_range, Platform, QuitFlag, TrayAction, TrayHandle};
use crate::processes::{self, ProcessEntry};
use crate::race_watch::decide_switch;
use crate::update::UpdateStatus;
use eframe::egui;
use lightspeed_client::interceptor::ProcessTarget;
use lightspeed_client::profiles::schema::{is_valid_key, parse_port_range, slugify, PortRange};
use lightspeed_client::profiles::{ProfileStore, UserGameProfile, UserServer};
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
                default_port: lightspeed_client::games::resolve(key)
                    .map(|game| game.redirect_port())
                    .unwrap_or(0),
            })
            .collect()
    })
}

// ── User games (profiles.toml) ─────────────────────────────────────────────

/// The game the app has pinned: a built-in registry entry, or one of the
/// user's own `[[games]]` profiles.
///
/// Returned by value because a user profile owns its strings: `GameEntry`'s
/// `&'static str` cannot name a key that lives in `profiles.toml`.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PickedGame {
    key: String,
    display: String,
    default_port: u16,
}

/// The process-wide profile store the editor reads and writes.
///
/// The same store `games::resolve` consults, so an edit saved here is live for
/// the next Optimize in this process and for the next start from disk.
fn profile_store() -> &'static ProfileStore {
    lightspeed_client::profiles::global()
}

/// The loaded user game profiles, as of the store's last reload.
fn user_profiles() -> Vec<UserGameProfile> {
    profile_store().snapshot().games.clone()
}

/// The loaded saved servers, as of the store's last reload.
fn user_servers() -> Vec<UserServer> {
    profile_store().snapshot().servers.clone()
}

/// The keys of the loaded user game profiles.
fn user_game_keys() -> Vec<String> {
    user_profiles()
        .into_iter()
        .map(|profile| profile.key)
        .collect()
}

/// One row of the Game dropdown, in render order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GamePickRow {
    /// "Auto (detect)".
    Auto,
    /// "Manual / Custom…".
    Manual,
    /// A built-in game, by index into [`games`].
    Builtin(usize),
    /// The "My games" group label, drawn only when profiles exist.
    MyGamesHeader,
    /// A user profile, by index into the snapshot the UI holds.
    User(usize),
    /// "New custom game…", always the last row.
    NewCustom,
}

/// The Game dropdown's rows: the two modes, every built-in in registry order,
/// then the "My games" group, then "New custom game…".
///
/// Additive on purpose: the built-ins keep their order and their indices, so a
/// `selected_game` key saved before profiles existed still means the same game,
/// and the user's own list simply appears below them.
fn game_pick_rows(builtin_count: usize, user_count: usize) -> Vec<GamePickRow> {
    let mut rows = Vec::with_capacity(builtin_count + user_count + 4);
    rows.push(GamePickRow::Auto);
    rows.push(GamePickRow::Manual);
    rows.extend((0..builtin_count).map(GamePickRow::Builtin));
    if user_count > 0 {
        rows.push(GamePickRow::MyGamesHeader);
        rows.extend((0..user_count).map(GamePickRow::User));
    }
    rows.push(GamePickRow::NewCustom);
    rows
}

/// The custom-game editor's fields, plus the profile it edits.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct GameEditorDraft {
    /// The user profile being edited, by index into the store's games; `None`
    /// while composing a new one.
    editing: Option<usize>,
    /// The schema key. Kept from the profile being edited, so renaming does not
    /// silently orphan a key the user already put on a command line; empty for
    /// a new draft, where the key is the slug of the name.
    key: String,
    name: String,
    /// The process-names field, as typed (comma- or space-separated).
    process_names: String,
    /// The UDP-range field, as typed.
    udp_range: String,
    dynamic_server: bool,
    notes: String,
}

impl GameEditorDraft {
    /// A blank draft: "New custom game…".
    fn new_profile() -> Self {
        Self::default()
    }

    /// The draft that edits `profile`.
    fn from_profile(index: usize, profile: &UserGameProfile) -> Self {
        Self {
            editing: Some(index),
            key: profile.key.clone(),
            name: profile.name.clone(),
            process_names: profile.process_names.join(", "),
            udp_range: profile
                .udp_ports
                .map(|range| range.to_string())
                .unwrap_or_default(),
            dynamic_server: profile.dynamic_server,
            notes: profile.notes.clone().unwrap_or_default(),
        }
    }

    /// The schema key this draft describes: the edited profile's own key, else
    /// the slug of the name.
    fn key(&self) -> Option<String> {
        if !self.key.is_empty() {
            return Some(self.key.clone());
        }
        slugify(self.name.trim())
    }

    /// The `[[games]]` entry this draft describes. Every field goes through the
    /// schema's own parser, so what is written is what the fields hold.
    fn to_profile(&self) -> UserGameProfile {
        let notes = self.notes.trim();
        UserGameProfile {
            name: self.name.trim().to_string(),
            key: self.key().unwrap_or_default(),
            process_names: split_process_names(&self.process_names),
            udp_ports: parse_user_port_range(&self.udp_range).ok().flatten(),
            dynamic_server: self.dynamic_server,
            notes: (!notes.is_empty()).then(|| notes.to_string()),
        }
    }
}

/// The editor's UDP-range field, parsed by the schema's own parser so the field
/// and the file can never disagree about what a range is.
fn parse_user_port_range(text: &str) -> Result<Option<PortRange>, String> {
    parse_port_range(text)
}

/// The editor's process-names field: image names separated by commas or spaces,
/// blanks dropped, so a trailing comma is not a typo the user has to hunt down.
fn split_process_names(text: &str) -> Vec<String> {
    text.split(|c: char| c == ',' || c.is_whitespace())
        .filter(|name| !name.is_empty())
        .map(str::to_string)
        .collect()
}

/// The built-in game that already answers to `key`, if any.
///
/// `detect_game` is crate-private, so the probe is `resolve` - the one public
/// resolution path, whose built-in match arms (keys and aliases) answer first,
/// exactly like the schema's own collision check. A key the user's store
/// already defines is a duplicate of their own, not a built-in claim.
fn builtin_claim_of_key(key: &str, user_keys: &[String]) -> Option<String> {
    if user_keys.iter().any(|existing| existing == key) {
        return None;
    }
    lightspeed_client::games::resolve(key)
        .ok()
        .map(|game| game.name().to_string())
}

/// The built-in game that already claims `process`, if any.
///
/// Image names compare case-insensitively, as they do on every platform the
/// scanner runs on; this mirrors the schema's own collision check.
fn builtin_claim_of_process(process: &str) -> Option<String> {
    lightspeed_client::games::all_game_keys()
        .into_iter()
        .find_map(|(_, display)| {
            lightspeed_client::games::process_names_for_name(display)
                .iter()
                .any(|known| known.eq_ignore_ascii_case(process))
                .then(|| display.to_string())
        })
}

/// The editor's live verdict on a draft: the schema's save rules, attributed to
/// the field that carries each one, so the offending input is tinted while the
/// user types instead of only when the save is refused.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DraftVerdict {
    name_ok: bool,
    udp_ok: bool,
    key_ok: bool,
    processes_ok: bool,
    target_ok: bool,
    /// The built-in game that claims the draft's key, when one does.
    key_claimed_by: Option<String>,
    /// The draft's key is already used by another of the user's own games.
    key_duplicate: bool,
}

impl DraftVerdict {
    /// Whether the draft can be saved: every field passes its own rule.
    fn ok(&self) -> bool {
        self.name_ok && self.udp_ok && self.key_ok && self.processes_ok && self.target_ok
    }
}

/// Mirror the schema's save rules per field.
///
/// The authority stays the client's schema - `ProfileStore::save` re-validates
/// and refuses - but a rule the user cannot see is a rule they cannot satisfy,
/// so everything that can block a save is surfaced on the field that carries it.
/// `user_keys` is the store's current keys; `editing` is the key of the profile
/// being edited, which may keep its own.
fn validate_draft(
    draft: &GameEditorDraft,
    user_keys: &[String],
    editing: Option<&str>,
) -> DraftVerdict {
    let processes = split_process_names(&draft.process_names);
    let range = parse_user_port_range(&draft.udp_range);
    let key = draft.key();
    let claimed_by = key
        .as_deref()
        .and_then(|k| builtin_claim_of_key(k, user_keys));
    let duplicate = key
        .as_deref()
        .is_some_and(|k| Some(k) != editing && user_keys.iter().any(|existing| existing == k));
    DraftVerdict {
        name_ok: !draft.name.trim().is_empty(),
        udp_ok: range.is_ok(),
        key_ok: key.as_deref().is_some_and(is_valid_key) && claimed_by.is_none() && !duplicate,
        processes_ok: processes
            .iter()
            .all(|process| builtin_claim_of_process(process).is_none()),
        // At least one of the two targeting fields, as the schema requires.
        target_ok: !processes.is_empty() || matches!(range, Ok(Some(_))),
        key_claimed_by: claimed_by,
        key_duplicate: duplicate,
    }
}

/// The localized reasons a draft cannot be saved, in field order.
///
/// The verdict's flags say *which* field is wrong; the wording is the user's
/// language, so a translated window never shows English rule prose.
fn draft_reasons(verdict: &DraftVerdict, key: Option<&str>) -> Vec<String> {
    let mut reasons = Vec::new();
    if !verdict.name_ok {
        reasons.push(i18n::t("game.editor_name_required").into_owned());
    }
    if !verdict.udp_ok {
        reasons.push(i18n::t("game.editor_udp_invalid").into_owned());
    }
    if !verdict.processes_ok {
        reasons.push(i18n::t("game.editor_process_reserved").into_owned());
    }
    if !verdict.key_ok {
        if let Some(builtin) = &verdict.key_claimed_by {
            reasons.push(i18n::t_with(
                "game.editor_key_reserved",
                &[("key", key.unwrap_or_default()), ("name", builtin.as_str())],
            ));
        } else if verdict.key_duplicate {
            reasons.push(i18n::t("game.editor_key_duplicate").into_owned());
        } else {
            reasons.push(i18n::t("game.editor_key_underivable").into_owned());
        }
    }
    if !verdict.target_ok {
        reasons.push(i18n::t("game.editor_target_required").into_owned());
    }
    reasons
}

/// The `[[servers]]` entry the Manual flow's "Save as…" builds from the fields
/// on screen, or the reason it cannot: what is saved is what was typed.
fn server_from_fields(name: &str, address: &str, port_range: &str) -> Result<UserServer, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(i18n::t("servers.invalid_name").into_owned());
    }
    let address = address.trim();
    if address.parse::<SocketAddrV4>().is_err() {
        return Err(i18n::t("servers.invalid_address").into_owned());
    }
    Ok(UserServer {
        name: name.to_string(),
        address: address.to_string(),
        udp_ports: parse_user_port_range(port_range)?,
    })
}

/// Persist the profiles document, and refresh the cached snapshot.
///
/// `ProfileStore::save` validates, writes, and reloads: the reload is what makes
/// an edit take effect on the next `resolve()` - the next Optimize - without a
/// restart, and the file write is what carries it into the next start.
fn save_user_profiles(
    store: &ProfileStore,
    games: &[UserGameProfile],
    servers: &[UserServer],
) -> Result<(), String> {
    store.save(games, servers).map_err(|errors| {
        errors
            .iter()
            .map(|error| error.to_string())
            .collect::<Vec<_>>()
            .join("; ")
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

/// What the picker is showing this frame: rows from the last enumeration, or a
/// load still in flight.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PickerStatus {
    Loading,
    Ready,
}

/// The picker's live inputs for one frame, borrowed from the app.
struct ProcessPicker<'a> {
    /// The rows the last enumeration produced.
    rows: &'a [ProcessEntry],
    status: PickerStatus,
    /// The filter field, editable in place.
    search: &'a mut String,
}

/// What the picker knows about the last enumeration ({status} of
/// [`ProcessListState`]).
fn picker_status(state: &ProcessListState) -> PickerStatus {
    match state {
        ProcessListState::Done => PickerStatus::Ready,
        // A load is either running or about to be started by the sheet's own
        // open handler, so the honest thing on screen is the spinner's line.
        ProcessListState::InFlight(_) | ProcessListState::Idle => PickerStatus::Loading,
    }
}

/// The process target a picker row stands for: the image name - the identity
/// that survives a relaunch - plus the PID the row was running under. The start
/// path re-resolves the name against the live process table, so the PID is what
/// the status surface shows, never what interception binds to.
fn target_for_pick(entry: &ProcessEntry) -> ProcessTarget {
    ProcessTarget {
        name: Some(entry.name.clone()),
        pid: Some(entry.pid),
    }
}

/// The status-surface line for a picked process: `Target: <name> (PID <n>)`,
/// naming the process exactly as the start path describes it.
fn target_summary(target: &ProcessTarget) -> String {
    let described = target.describe();
    i18n::t_with("game.target_set", &[("target", described.as_str())])
}

/// Apply one frame of the target picker to the app's target.
///
/// A chosen row wins; an explicit Clear drops the target so the built-in Game
/// path decides again; a frame with neither leaves the target alone.
fn apply_target_choice(
    current: Option<ProcessTarget>,
    picked: Option<ProcessTarget>,
    clear: bool,
) -> Option<ProcessTarget> {
    if clear {
        None
    } else {
        picked.or(current)
    }
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
            server_input: saved.manual_server.clone(),
            fec_enabled: false,
            capture_available: P::is_capture_available(),
            share_latency_stats,
            auto_detected_game,
            is_admin,
            fonts_setup: false,
            header_icon: None,
            show_settings: false,
            show_game_editor: false,
            game_editor: GameEditorDraft::new_profile(),
            editor_saved: None,
            editor_error: None,
            process_list: ProcessListState::Idle,
            process_rows: Vec::new(),
            process_search: String::new(),
            selected_custom,
            target_process: None,
            show_target_picker: false,
            save_server_name: String::new(),
            server_saved: None,
            server_error: None,
            language,
            boost_start: None,
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
        let games = games();
        let idx = self.selected_game_idx.min(games.len().saturating_sub(1));
        let entry = &games[idx];
        PickedGame {
            key: entry.key.to_string(),
            display: entry.display.to_string(),
            default_port: entry.default_port,
        }
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
            (i18n::t("state.optimizing").to_uppercase(), signal)
        } else if self.status.connected {
            (i18n::t("state.connected").to_uppercase(), text_1)
        } else {
            (i18n::t("state.idle").to_uppercase(), text_1)
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
    /// row is dropped and the values move into the scrolling region. The
    /// `Target` row is drawn only while a process is targeted, so the ledger is
    /// unchanged on the built-in path.
    fn ledger_rows(&self, ui: &mut egui::Ui, narrow: bool, short_mode: bool) {
        let relay = self.relay_value();
        self.ledger_row(ui, narrow, &i18n::t("route.relay"), &relay);
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
        if let Some(id) = node_id {
            LedgerValue::Data(discovery::friendly_label(id))
        } else if self.status.proxy_addr.is_empty() {
            LedgerValue::Hint("--".into())
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
            None => LedgerValue::Hint(i18n::t("route.place_hint")),
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
            LedgerValue::Hint(i18n::t("route.count_hint"))
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
                if ui
                    .link(i18n::t("action.manage").as_ref())
                    .on_hover_text(i18n::t("proxies.manage_hint"))
                    .clicked()
                {
                    self.show_proxy_manager = true;
                }
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
        } else if !self.game_manual {
            let caption = if self.game_auto {
                i18n::t("game.none_detected_hint")
            } else {
                i18n::t("game.none_detected")
            };
            ui.label(egui::RichText::new(caption).size(BODY).color(text_2));
        }
        // The front door for process-scoped interception, on the Game row it
        // takes over whenever a process is picked.
        self.target_control(ui);
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
            let edit = ui.add(
                egui::TextEdit::singleline(&mut self.server_input)
                    .hint_text(format!("e.g. 123.45.67.89:{default_port}"))
                    .desired_width(220.0),
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
                    .width(200.0)
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
            ui.label(i18n::t("route.custom_port_range")).on_hover_ui(|ui| {
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
                200.0,
                !port_valid,
            )
            .changed()
            {
                self.persist_config();
            }
        });
        ui.add_space(S2);

        // Save as: append the two fields above as a `[[servers]]` entry.
        ui.horizontal(|ui| {
            ui.label(i18n::t("servers.save_as"));
            ui.add(
                egui::TextEdit::singleline(&mut self.save_server_name)
                    .hint_text(i18n::t("servers.save_name_hint"))
                    .desired_width(160.0),
            );
            if ui.button(i18n::t("servers.save")).clicked() {
                save_server = true;
            }
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
        let manual_button = egui::Button::new(i18n::t("route.start_manual"))
            .fill(if server_valid { signal_soft } else { bg_3 });
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
            .button(i18n::t("update.sheet_title").as_ref())
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
            egui::RichText::new(i18n::t_with(
                "settings.capture_status",
                &[(
                    "state",
                    if self.capture_available {
                        i18n::t("settings.capture_available")
                    } else {
                        i18n::t("settings.capture_missing")
                    }
                    .as_ref(),
                )],
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

        let response = egui::Modal::new(egui::Id::new("game_editor_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_max_width(max_w);
                sheet_title(ui, &i18n::t("game.editor_title"));
                egui::ScrollArea::vertical()
                    .max_height(max_h)
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

        let response = egui::Modal::new(egui::Id::new("target_process_sheet"))
            .backdrop_color(scrim)
            .frame(sheet_frame())
            .show(ctx, |ui| {
                ui.set_max_width(max_w);
                sheet_title(ui, &i18n::t("game.target_process"));
                ui.label(
                    egui::RichText::new(i18n::t("game.target_process_hint"))
                        .size(BODY)
                        .color(text_2),
                );
                egui::ScrollArea::vertical()
                    .max_height(max_h)
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
                if ui.button(i18n::t("action.close").as_ref()).clicked() {
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
        self.poll_process_list();
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
                action_button(ui, &action_label, action_kind, action_ready)
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

fn connect_instruction(game_key: &str, local_port: u16) -> String {
    let port = local_port.to_string();
    match game_key {
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
/// - `Stop optimizing` when boosting.
/// - `Restart as Administrator to optimize` when unelevated (and not boosting).
/// - `Optimize my route` when elevated, connected, and a relay is selected.
/// - `Optimize my route`, disabled, otherwise (elevated but not ready).
///
/// Returned upper-case because the action slot draws in caps; `to_uppercase`
/// leaves a locale without letter case (ja, ko, zh-Hans) untouched.
fn primary_action(
    is_admin: bool,
    boosting: bool,
    connected: bool,
    has_relay: bool,
) -> (String, bool) {
    if boosting {
        (i18n::t("action.stop_optimizing").to_uppercase(), true)
    } else if !is_admin {
        (
            i18n::t("action.restart_admin_optimize").to_uppercase(),
            true,
        )
    } else if connected && has_relay {
        (i18n::t("action.optimize_route").to_uppercase(), true)
    } else {
        (i18n::t("action.optimize_route").to_uppercase(), false)
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
    Hint(Cow<'static, str>),
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

/// What the custom-game editor asked for this frame.
///
/// The sheet cannot mutate the app while the body holds a `&mut` on the draft,
/// so the body records intent and the sheet applies it after the frame. That
/// record is also what the headless render test reads.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct EditorActions {
    /// "Edit" on one of the user's profiles, by index into the snapshot.
    edit: Option<usize>,
    save: bool,
    delete: bool,
    /// "Refresh" on the process picker: re-read the host's process table.
    refresh_processes: bool,
}

/// The custom-game editor's body: the user's profiles, the five fields, the live
/// verdict, the key, and the actions.
///
/// A free function rather than a method so the sheet can be rendered in a test
/// without an engine, a tray, or a config file.
fn game_editor_body(
    ui: &mut egui::Ui,
    draft: &mut GameEditorDraft,
    verdict: &DraftVerdict,
    editor_saved: Option<&str>,
    editor_error: Option<&str>,
    user_games: &[UserGameProfile],
    mut picker: ProcessPicker<'_>,
    actions: &mut EditorActions,
) {
    // The user's profiles, each with the way back into it.
    section_label(ui, &i18n::t("game.editor_list"));
    if user_games.is_empty() {
        ui.label(
            egui::RichText::new(i18n::t("game.editor_no_games"))
                .size(BODY)
                .color(text_2),
        );
    }
    for (i, profile) in user_games.iter().enumerate() {
        ui.horizontal_wrapped(|ui| {
            ui.label(egui::RichText::new(&profile.name).color(text_1));
            ui.label(
                egui::RichText::new(&profile.key)
                    .size(CAPTION)
                    .color(text_3),
            );
            if ui.button(i18n::t("game.editor_edit")).clicked() {
                actions.edit = Some(i);
            }
        });
    }
    ui.add_space(S2);
    hairline(ui);
    ui.add_space(S2);

    ui.horizontal(|ui| {
        ui.label(i18n::t("game.editor_name"));
        invalid_text_field(
            ui,
            &mut draft.name,
            i18n::t("game.editor_name_hint"),
            220.0,
            !verdict.name_ok,
        );
    });
    ui.horizontal(|ui| {
        ui.label(i18n::t("game.editor_process_names"));
        invalid_text_field(
            ui,
            &mut draft.process_names,
            i18n::t("game.editor_process_hint"),
            260.0,
            !verdict.processes_ok,
        );
    });
    // The picker sits directly under the field it fills: choosing a row appends
    // its image name there, and the field stays a plain typed field.
    let already = split_process_names(&draft.process_names);
    let chosen = process_picker(
        ui,
        &mut picker,
        |entry| {
            already
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&entry.name))
        },
        &mut actions.refresh_processes,
    );
    if let Some(entry) = chosen {
        draft.process_names = processes::append_name(&draft.process_names, &entry.name);
    }
    ui.horizontal(|ui| {
        ui.label(i18n::t("game.editor_udp_range"));
        invalid_text_field(
            ui,
            &mut draft.udp_range,
            i18n::t("game.editor_udp_hint"),
            200.0,
            !verdict.udp_ok,
        );
    });
    // No TCP field: the interception path is UDP-only, so the schema refuses a
    // `tcp_ports` entry and the editor must not offer what a save would refuse.
    ui.checkbox(
        &mut draft.dynamic_server,
        egui::RichText::new(i18n::t("game.editor_dynamic_server")).size(LABEL),
    );
    ui.horizontal(|ui| {
        ui.label(i18n::t("game.editor_notes"));
        ui.add(
            egui::TextEdit::singleline(&mut draft.notes)
                .hint_text(i18n::t("game.editor_notes_hint"))
                .desired_width(260.0),
        );
    });

    // Live validation, named on the field that carries it.
    for reason in draft_reasons(verdict, draft.key().as_deref()) {
        ui.label(egui::RichText::new(reason).size(CAPTION).color(danger));
    }

    // The key, while it is being composed and after it is saved: this is what
    // `--game <key>` takes.
    if let Some(key) = draft.key() {
        ui.label(
            egui::RichText::new(i18n::t_with("game.editor_key", &[("key", key.as_str())]))
                .size(CAPTION)
                .color(if verdict.key_ok { text_2 } else { danger }),
        );
    }
    if let Some(saved) = editor_saved {
        ui.label(egui::RichText::new(saved).size(CAPTION).color(signal));
    }
    if let Some(error) = editor_error {
        ui.label(egui::RichText::new(error).size(CAPTION).color(danger));
    }

    ui.add_space(S2);
    ui.label(
        egui::RichText::new(i18n::t("game.editor_new_hint"))
            .size(CAPTION)
            .color(text_3),
    );
    ui.add_space(S2);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(verdict.ok(), egui::Button::new(i18n::t("game.editor_save")))
            .clicked()
        {
            actions.save = true;
        }
        if draft.editing.is_some() && ui.button(i18n::t("game.editor_delete")).clicked() {
            actions.delete = true;
        }
    });
}

/// The live process picker: the machine's process table, filtered by a search
/// field, where choosing a row is reported back to the caller.
///
/// The rows are whatever the last background enumeration returned - nothing in
/// here enumerates or blocks, which is what makes it safe to draw from the egui
/// thread. Degrading honestly is the point of the status line: an enumeration
/// that came back empty (or a platform that will not enumerate at all) says so,
/// and the caller's own field (or process target) keeps working without it.
///
/// The same picker serves the editor's process-names field and the app's
/// process target: `highlighted` decides which rows draw as already chosen, and
/// a click on a row is handed back so the caller decides what choosing means.
/// `refresh` is set when the Refresh button is pressed.
fn process_picker<'a>(
    ui: &mut egui::Ui,
    picker: &mut ProcessPicker<'a>,
    mut highlighted: impl FnMut(&ProcessEntry) -> bool,
    refresh: &mut bool,
) -> Option<&'a ProcessEntry> {
    ui.add_space(S2);
    ui.horizontal(|ui| {
        section_label(ui, &i18n::t("game.editor_process_picker"));
        if ui.button(i18n::t("game.editor_process_refresh")).clicked() {
            *refresh = true;
        }
    });

    let width = ui.available_width();
    ui.add(
        egui::TextEdit::singleline(&mut *picker.search)
            .hint_text(i18n::t("game.editor_process_search_hint"))
            .desired_width(width),
    );

    // Copied out of the borrow of `picker` so the chosen row can be handed back
    // with the picker's own lifetime.
    let rows = picker.rows;
    match picker.status {
        PickerStatus::Loading => {
            ui.label(
                egui::RichText::new(i18n::t("game.editor_process_loading"))
                    .size(CAPTION)
                    .color(text_3),
            );
            None
        }
        PickerStatus::Ready => {
            if rows.is_empty() {
                ui.label(
                    egui::RichText::new(i18n::t("game.editor_process_empty"))
                        .size(CAPTION)
                        .color(text_3),
                );
                None
            } else {
                let shown = processes::matching(rows, picker.search);
                if shown.is_empty() {
                    ui.label(
                        egui::RichText::new(i18n::t("game.editor_process_no_match"))
                            .size(CAPTION)
                            .color(text_3),
                    );
                }
                let mut chosen = None;
                for entry in shown {
                    if process_row(ui, entry, highlighted(entry)).clicked() {
                        chosen = Some(entry);
                    }
                }
                chosen
            }
        }
    }
}

/// What one frame of the target front door asks the app to do.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct TargetRowAction {
    /// "Target a process" was pressed: open the picker.
    open_picker: bool,
    /// "Clear target" was pressed: back to the built-in Game path.
    clear: bool,
}

/// The front door, as a bare widget: the picked process and the way back to the
/// built-in Game path, or the way in to the picker.
///
/// A frame with a target names it - `Target: <name> (PID <n>)` - and offers
/// Clear; a frame without one offers the picker and says what it does. It
/// records intent instead of mutating the app, so the caller applies it after
/// the frame and the widget renders headlessly in a test.
fn target_front_door(ui: &mut egui::Ui, target: Option<&ProcessTarget>) -> TargetRowAction {
    let mut action = TargetRowAction::default();
    ui.horizontal_wrapped(|ui| match target {
        Some(target) => {
            ui.label(
                egui::RichText::new(target_summary(target))
                    .size(LABEL)
                    .color(text_1),
            );
            if ui.link(i18n::t("game.target_clear")).clicked() {
                action.clear = true;
            }
        }
        None => {
            if ui.button(i18n::t("game.target_process")).clicked() {
                action.open_picker = true;
            }
            ui.label(
                egui::RichText::new(i18n::t("game.target_none"))
                    .size(CAPTION)
                    .color(text_3),
            );
        }
    });
    action
}

/// One picker row: the image name (emphasised), the PID, and the path's tail.
///
/// The whole row is the button - not a small control on it - and because it is
/// a `selectable_label`, hover and the "already in the field" state come from
/// the theme's own widget visuals instead of a hand-rolled fill. Choosing a row
/// whose name the field already carries repaints the same row rather than
/// changing anything: the append dedupes case-insensitively.
fn process_row(ui: &mut egui::Ui, entry: &ProcessEntry, in_field: bool) -> egui::Response {
    let mut job = egui::text::LayoutJob::default();
    job.append(
        &entry.name,
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(BODY, semibold()),
            color: if in_field { text_3 } else { text_1 },
            ..Default::default()
        },
    );
    job.append(
        &format!("  {}", entry.pid),
        0.0,
        egui::TextFormat {
            font_id: egui::FontId::new(CAPTION, egui::FontFamily::Monospace),
            color: text_3,
            ..Default::default()
        },
    );
    if let Some(path) = &entry.path {
        job.append(
            &format!(
                "  {}",
                processes::tail_path(path, processes::PATH_TAIL_CHARS)
            ),
            0.0,
            egui::TextFormat {
                font_id: egui::FontId::proportional(CAPTION),
                color: text_3,
                ..Default::default()
            },
        );
    }
    ui.selectable_label(in_field, job)
}

/// A single-line field whose ink turns `danger` while its content is invalid,
/// with the existing `proxies.invalid` marker beside it.
///
/// The one implementation of the invalid-input treatment: the Manual flow's
/// port range and every field of the profile editor share it, so "invalid"
/// never looks like two different things.
fn invalid_text_field(
    ui: &mut egui::Ui,
    text: &mut String,
    hint: impl Into<egui::WidgetText>,
    width: f32,
    invalid: bool,
) -> egui::Response {
    let ink = if invalid { danger } else { text_1 };
    let response = ui.add(
        egui::TextEdit::singleline(text)
            .hint_text(hint)
            .desired_width(width)
            .text_color(ink),
    );
    if invalid {
        ui.label(egui::RichText::new(i18n::t("proxies.invalid")).color(danger));
    }
    response
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
        apply_discovery_result, apply_target_choice, close_decision, draft_reasons,
        game_editor_body, game_pick_rows, games, picker_status, primary_action, process_picker,
        save_user_profiles, server_from_fields, should_apply_race, should_race, target_for_pick,
        target_front_door, target_summary, validate_draft, CloseDecision, EditorActions,
        GameEditorDraft, GamePickRow, PickerStatus, ProcessListState, ProcessPicker, ProcessTarget,
        TargetRowAction,
    };
    use crate::config::ProxyEntry;
    use crate::discovery::{DiscoveryOutcome, RelayInfo};
    use crate::processes::ProcessEntry;
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
        let _guard = crate::i18n::test_lock();
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
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(None);
        // Boosting: the only destructive control.
        assert_eq!(
            primary_action(true, true, true, true),
            ("STOP OPTIMIZING".to_string(), true)
        );
        assert_eq!(
            primary_action(false, true, false, false),
            ("STOP OPTIMIZING".to_string(), true)
        );
        // Unelevated: the action is elevation.
        assert_eq!(
            primary_action(false, false, true, true),
            ("RESTART AS ADMINISTRATOR TO OPTIMIZE".to_string(), true)
        );
        assert_eq!(
            primary_action(false, false, false, false),
            ("RESTART AS ADMINISTRATOR TO OPTIMIZE".to_string(), true)
        );
        // Elevated and ready: the product's action.
        assert_eq!(
            primary_action(true, false, true, true),
            ("OPTIMIZE MY ROUTE".to_string(), true)
        );
    }

    #[test]
    fn primary_action_disables_the_action_until_connected_and_relayed() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(None);
        // Elevated but disconnected: keep the label, but the slot cannot act.
        assert_eq!(
            primary_action(true, false, false, true),
            ("OPTIMIZE MY ROUTE".to_string(), false)
        );
        // Elevated and connected, but no relay is selected.
        assert_eq!(
            primary_action(true, false, true, false),
            ("OPTIMIZE MY ROUTE".to_string(), false)
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

    /// Discussion #146's first ask, closed end to end: the "Custom Port Range"
    /// the user typed in Manual mode is persisted, and once restored it
    /// actuates - it is parsed and handed to the interceptor as the config's
    /// `port_range`, beating the profile's own range.
    ///
    /// The two steps mirror `start_boost` exactly. The assertion is about
    /// reaching `InterceptorConfig`, so it lives in this file (the one that
    /// wires the stored string into the engine) rather than next to the TOML
    /// round-trips in `config.rs`.
    #[test]
    fn a_stored_manual_port_range_reaches_the_interceptor_config() {
        // As the loader restores it from disk.
        let saved = crate::config::GuiConfig {
            manual_port_range: "27000-27050".to_string(),
            ..Default::default()
        };

        let game = lightspeed_client::games::resolve("rust").expect("rust is a built-in key");
        let proxy = "127.0.0.1:4434".parse().unwrap();

        let cfg = lightspeed_client::interceptor::build_config_for_game_with_port_range(
            game.as_ref(),
            proxy,
            false,
            4,
            Default::default(),
            super::parse_custom_port_range(&saved.manual_port_range),
        )
        .expect("a config is built even when the game process is not running");
        assert_eq!(
            cfg.port_range,
            (27000, 27050),
            "the stored Manual range must reach the interceptor config"
        );

        // A malformed stored value parses to `None`: the profile default
        // stands and nothing panics.
        let cfg = lightspeed_client::interceptor::build_config_for_game_with_port_range(
            game.as_ref(),
            proxy,
            false,
            4,
            Default::default(),
            super::parse_custom_port_range("not-a-range"),
        )
        .expect("a config is built even when the game process is not running");
        assert_eq!(cfg.port_range, game.ports());
    }

    /// The Game dropdown's model, not its pixels: the two modes, every built-in
    /// in registry order, then the user's own group, then the way in.
    #[test]
    fn the_my_games_group_sits_below_the_built_ins_and_ends_with_the_editor_entry() {
        let built_ins = games().len();
        let rows = game_pick_rows(built_ins, 2);
        assert_eq!(rows[0], GamePickRow::Auto);
        assert_eq!(rows[1], GamePickRow::Manual);

        let builtin_at: Vec<usize> = rows
            .iter()
            .enumerate()
            .filter_map(|(at, row)| matches!(row, GamePickRow::Builtin(_)).then_some(at))
            .collect();
        assert_eq!(
            builtin_at,
            (2..built_ins + 2).collect::<Vec<_>>(),
            "every built-in is present, in registry order, right below the modes"
        );
        for (at, row) in rows.iter().enumerate() {
            if let GamePickRow::Builtin(i) = row {
                assert_eq!(*i, at - 2, "built-in indices must not shift");
            }
        }

        let header = rows
            .iter()
            .position(|row| *row == GamePickRow::MyGamesHeader)
            .expect("the group header is drawn when profiles exist");
        assert!(
            builtin_at.iter().all(|at| *at < header),
            "the My games group sits below the built-ins: {rows:?}"
        );
        assert_eq!(
            rows.iter()
                .filter_map(|row| match row {
                    GamePickRow::User(i) => Some(*i),
                    _ => None,
                })
                .collect::<Vec<_>>(),
            vec![0, 1],
            "one row per profile, in store order"
        );
        assert_eq!(rows.last(), Some(&GamePickRow::NewCustom));

        // No profiles, no group - and the editor entry stays reachable.
        let rows = game_pick_rows(built_ins, 0);
        assert!(!rows.contains(&GamePickRow::MyGamesHeader));
        assert_eq!(rows.last(), Some(&GamePickRow::NewCustom));
        assert_eq!(rows.len(), built_ins + 3);
    }

    /// The editor's live verdict against the authority that actually refuses a
    /// save: for every draft, "the Save button is enabled" must equal "the
    /// schema accepts the document the button would write".
    #[test]
    fn the_editor_verdict_mirrors_the_schema_save_rules() {
        use lightspeed_client::profiles::schema::validate_for_save;
        use lightspeed_client::profiles::UserGameProfile;

        fn draft(name: &str, key: &str, processes: &str, udp: &str) -> GameEditorDraft {
            GameEditorDraft {
                key: key.to_string(),
                name: name.to_string(),
                process_names: processes.to_string(),
                udp_range: udp.to_string(),
                ..Default::default()
            }
        }

        fn existing(key: &str) -> UserGameProfile {
            UserGameProfile {
                name: key.to_string(),
                key: key.to_string(),
                process_names: vec!["Mine.exe".to_string()],
                udp_ports: None,
                dynamic_server: false,
                notes: None,
            }
        }

        /// What the sheet's Save button would write.
        fn document(
            existing: &[UserGameProfile],
            draft: &GameEditorDraft,
            editing: Option<usize>,
        ) -> Vec<UserGameProfile> {
            let mut games = existing.to_vec();
            match editing {
                Some(i) if i < games.len() => games[i] = draft.to_profile(),
                _ => games.push(draft.to_profile()),
            }
            games
        }

        let cases: Vec<(&str, GameEditorDraft, Option<usize>, Vec<UserGameProfile>)> = vec![
            (
                "a new profile with process names",
                draft("My Game", "", "MyGame.exe", ""),
                None,
                vec![],
            ),
            (
                "a new profile with only a UDP range",
                draft("Ports Only", "", "", "27015-27050"),
                None,
                vec![],
            ),
            (
                "a blank name",
                draft("", "", "MyGame.exe", ""),
                None,
                vec![],
            ),
            (
                "neither process names nor a range",
                draft("Empty", "", "", ""),
                None,
                vec![],
            ),
            (
                "a key a built-in claims",
                draft("Sneaky", "minecraft", "MyGame.exe", ""),
                None,
                vec![],
            ),
            (
                "a process name a built-in claims",
                draft("Sneaky", "", "RustClient.exe", ""),
                None,
                vec![],
            ),
            (
                "a key another profile already uses",
                draft("Sneaky", "my-game", "MyGame.exe", ""),
                None,
                vec![existing("my-game")],
            ),
            (
                "editing a profile keeps its own key",
                draft("Renamed", "my-game", "MyGame.exe", ""),
                Some(0),
                vec![existing("my-game")],
            ),
            (
                "a range that is not a range",
                draft("Typo", "", "", "27015-abc"),
                None,
                vec![],
            ),
            (
                "a range whose lo is above its hi",
                draft("Backwards", "", "", "27050-27015"),
                None,
                vec![],
            ),
        ];

        for (label, draft, editing, stored) in cases {
            let keys: Vec<String> = stored.iter().map(|game| game.key.clone()).collect();
            let editing_key = editing
                .and_then(|i| stored.get(i))
                .map(|game| game.key.clone());
            let verdict = validate_draft(&draft, &keys, editing_key.as_deref());
            let document = document(&stored, &draft, editing);
            let accepted = validate_for_save(&document, &[]).is_ok();
            assert_eq!(
                verdict.ok(),
                accepted,
                "{label}: the editor and the schema disagree ({verdict:?}, {document:?})"
            );
        }
    }

    /// A range the schema would silently drop still blocks Save: the field has
    /// to actuate, not vanish (the dead-control rule this round exists to fix).
    #[test]
    fn a_malformed_range_blocks_saving_even_though_the_schema_would_drop_it() {
        let draft = GameEditorDraft {
            name: "Typo".to_string(),
            process_names: "MyGame.exe".to_string(),
            udp_range: "27o15-27050".to_string(),
            ..Default::default()
        };
        let verdict = validate_draft(&draft, &[], None);
        assert!(!verdict.udp_ok, "the range field is the field at fault");
        assert!(!verdict.ok(), "an unparseable range must block the save");

        // The schema alone would drop the range and save a profile without it.
        let document = vec![draft.to_profile()];
        assert!(lightspeed_client::profiles::schema::validate_for_save(&document, &[]).is_ok());
        assert_eq!(document[0].udp_ports, None);

        // And the reason shown is the range, in the user's language.
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));
        let reasons = draft_reasons(&verdict, draft.key().as_deref());
        crate::i18n::set_language(None);
        assert!(
            reasons.iter().any(|reason| reason.contains("range")),
            "the range must be named: {reasons:?}"
        );
    }

    /// The Manual flow's "Save as…" writes a `[[servers]]` entry that survives
    /// the round trip into the next start, and a game saved from the editor
    /// lives in the same document without displacing it.
    #[test]
    fn a_saved_server_round_trips_through_the_profile_store() {
        use lightspeed_client::profiles::ProfileStore;

        let dir =
            std::env::temp_dir().join(format!("lightspeed-gui-profiles-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("profiles.toml");
        let store = ProfileStore::load_from(&path);

        let server = server_from_fields("My server", "203.0.113.7:27015", "27015-27050")
            .expect("a named, well-formed server");
        assert_eq!(server.address, "203.0.113.7:27015");
        assert_eq!(
            server.udp_ports.map(|range| range.to_string()).as_deref(),
            Some("27015-27050")
        );

        // Exactly what the Save as button does: read the loaded list, append,
        // write through the store.
        let mut servers = store.servers();
        servers.push(server.clone());
        save_user_profiles(&store, &store.games(), &servers).expect("save the servers");

        // The store reloaded as part of the save, so the next resolve in this
        // process sees it without a restart.
        assert_eq!(store.servers(), vec![server.clone()]);

        // The editor's own write goes through the same document, and must not
        // displace the saved server.
        let game = GameEditorDraft {
            name: "My Game".to_string(),
            process_names: "MyGame.exe".to_string(),
            ..Default::default()
        }
        .to_profile();
        save_user_profiles(&store, std::slice::from_ref(&game), &servers)
            .expect("save both halves");

        // A fresh store is the next start: it reads both entries back.
        let reopened = ProfileStore::load_from(&path);
        assert_eq!(reopened.games(), vec![game]);
        assert_eq!(reopened.servers(), vec![server]);
        let text = std::fs::read_to_string(&path).expect("the document was written");
        assert!(text.contains("[[servers]]"), "{text}");
        assert!(text.contains("27015-27050"), "{text}");

        // An unnamed or unaddressable server is refused before anything is
        // written - the fields on screen are the fields that get saved.
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));
        let unnamed = server_from_fields("  ", "203.0.113.7:27015", "");
        let unaddressed = server_from_fields("Named", "not-an-address", "");
        let bad_range = server_from_fields("Named", "203.0.113.7:27015", "nope");
        crate::i18n::set_language(None);
        assert!(unnamed.is_err() && unaddressed.is_err() && bad_range.is_err());
        assert_eq!(reopened.servers().len(), 1, "a refusal writes nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The picker's one state machine: while a load is in flight - or has not
    /// been started yet - the sheet says so; only a finished enumeration shows
    /// rows.
    #[test]
    fn the_picker_reports_a_load_in_flight_and_a_finished_table() {
        let (tx, rx) = std::sync::mpsc::channel();
        assert_eq!(
            picker_status(&ProcessListState::Idle),
            PickerStatus::Loading
        );
        assert_eq!(
            picker_status(&ProcessListState::InFlight(rx)),
            PickerStatus::Loading
        );
        assert_eq!(picker_status(&ProcessListState::Done), PickerStatus::Ready);
        drop(tx);
    }

    /// The editor sheet, rendered and read back.
    ///
    /// Asserting on the widget calls would pass with the sheet wired to nothing,
    /// so this drives the body the sheet itself draws and collects the text it
    /// actually painted: the five fields, the process picker and its rows, the
    /// key that `--game <key>` takes, and the actions - with no TCP field among
    /// them.
    #[test]
    fn the_game_editor_sheet_draws_every_field_and_the_key() {
        use eframe::egui;

        fn collect(shape: &egui::Shape, out: &mut Vec<(String, f32)>) {
            match shape {
                egui::Shape::Text(text) => out.push((
                    text.galley.text().to_string(),
                    text.pos.x + text.galley.size().x,
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }

        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));

        let mut draft = GameEditorDraft::new_profile();
        draft.name = "My Game".to_string();
        draft.process_names = "MyGame.exe".to_string();
        draft.udp_range = "27015-27050".to_string();
        let verdict = validate_draft(&draft, &[], None);
        assert!(
            verdict.ok(),
            "the fixture must be a valid draft: {verdict:?}"
        );

        // One picker row, with a path far wider than the sheet: the row has to
        // be drawn, trimmed, and still stay inside the sheet (whose cap at a
        // 440 px screen is `sheet_bounds`' 416).
        let rows = vec![ProcessEntry {
            pid: 4242,
            name: "MyGame.exe".to_string(),
            path: Some(
                "C:\\Program Files\\Some Vendor\\A Very Long Game Folder\\bin\\win64\\MyGame.exe"
                    .to_string(),
            ),
            started_at: None,
        }];

        let ctx = egui::Context::default();
        super::install_fonts(&ctx, Some("en"));
        super::apply_theme(&ctx);
        let raw = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::pos2(0.0, 0.0),
                egui::vec2(440.0, 800.0),
            )),
            ..Default::default()
        };
        let mut search = String::new();
        let mut actions = EditorActions::default();
        let mut out = ctx.run_ui(raw, |ui| {
            game_editor_body(
                ui,
                &mut draft,
                &verdict,
                None,
                None,
                &[],
                ProcessPicker {
                    rows: &rows,
                    status: PickerStatus::Ready,
                    search: &mut search,
                },
                &mut actions,
            );
        });
        out.textures_delta.clear();
        let mut drawn = Vec::new();
        for clipped in &out.shapes {
            collect(&clipped.shape, &mut drawn);
        }
        let frame = drawn
            .iter()
            .map(|(text, _)| text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let widest = drawn
            .iter()
            .map(|(_, right)| *right)
            .fold(0.0_f32, f32::max);
        crate::i18n::set_language(None);

        // Drawn, and drawn in order: this is the sheet's vertical layout, top
        // to bottom, read back off the frame.
        let mut last = 0;
        for expected in [
            "My games",
            "Name:",
            "Process names:",
            "Running processes",
            "Refresh",
            "Filter by name or path",
            "UDP range:",
            "Server rotates between matches",
            "Notes:",
            "Key: my-game - start the CLI with --game my-game",
            "Save game",
        ] {
            let at = frame
                .find(expected)
                .unwrap_or_else(|| panic!("the sheet did not draw {expected:?}:\n{frame}"));
            assert!(at >= last, "{expected:?} is drawn out of order:\n{frame}");
            last = at;
        }
        // The picker row itself: the image name, the PID, and a path trimmed to
        // its tail - the tail has to reach the frame, not the full path.
        assert!(
            frame.contains("MyGame.exe  4242  …bin\\win64\\MyGame.exe"),
            "the picker did not draw the row's name, PID and trimmed path:\n{frame}"
        );
        assert!(
            widest <= 416.0,
            "a row was painted past the sheet's width ({widest} px):\n{frame}"
        );
        assert!(
            !frame.contains("TCP"),
            "the editor must not offer a TCP field:\n{frame}"
        );
        assert_eq!(actions, EditorActions::default(), "a render clicks nothing");
    }

    /// The pick the front door makes is the state the primary action starts
    /// interception from: the chosen row becomes the target, the engine binds it
    /// by image name against the live process table, and the config the
    /// interceptor starts from carries that process - not a port range.
    #[test]
    fn a_picked_process_becomes_the_target_interception_starts_from() {
        use lightspeed_client::interceptor::traits::ProcessInfo;
        use lightspeed_client::interceptor::{
            build_config_for_process_target, resolve_process_target,
        };

        let entry = ProcessEntry {
            pid: 4242,
            name: "MyGame.exe".to_string(),
            path: Some("C:\\Games\\MyGame\\MyGame.exe".to_string()),
            started_at: None,
        };
        let target = apply_target_choice(None, Some(target_for_pick(&entry)), false)
            .expect("choosing a row sets the target");

        // The status surface and the start path agree on the identity.
        assert_eq!(target.describe(), "MyGame.exe (PID 4242)");

        // What `start_boost` does with it: bind the name to the live table,
        // then build the config the interceptor starts from.
        let live = ProcessInfo {
            pid: 4242,
            name: "MyGame.exe".to_string(),
            routes: vec![],
        };
        let bound =
            resolve_process_target(&target, Some(&live)).expect("the running process binds");
        let game = lightspeed_client::games::resolve("rust").expect("rust is a built-in key");
        let cfg = build_config_for_process_target(
            game.as_ref(),
            bound.clone(),
            vec![],
            addr("127.0.0.1:4434"),
            false,
            4,
            Default::default(),
        );
        assert_eq!(
            cfg.process_target,
            Some(bound),
            "the config the primary action starts from must carry the picked process"
        );
        assert_eq!(
            cfg.process_names,
            vec!["MyGame.exe".to_string()],
            "the config targets the picked image, not the profile's own processes"
        );
    }

    /// Clearing drops the target, and the built-in Game path is exactly what the
    /// primary action builds next: a config with no process target and the
    /// profile's own port range standing again.
    #[test]
    fn clearing_the_target_returns_to_the_built_in_game_path() {
        let target = ProcessTarget {
            name: Some("MyGame.exe".to_string()),
            pid: Some(4242),
        };
        assert_eq!(
            apply_target_choice(Some(target.clone()), None, true),
            None,
            "Clear drops the target"
        );
        assert_eq!(
            apply_target_choice(Some(target.clone()), None, false),
            Some(target),
            "a frame that neither picks nor clears leaves the target alone"
        );

        let game = lightspeed_client::games::resolve("rust").expect("rust is a built-in key");
        let cfg = lightspeed_client::interceptor::build_config_for_game_with_port_range(
            game.as_ref(),
            addr("127.0.0.1:4434"),
            false,
            4,
            Default::default(),
            None,
        )
        .expect("the built-in path builds a config");
        assert_eq!(
            cfg.process_target, None,
            "the built-in path carries no process target"
        );
        assert_eq!(
            cfg.port_range,
            game.ports(),
            "the profile's own range decides again"
        );
    }

    /// The status surface's line, through i18n: the same identity in English and
    /// German, with the placeholder substituted and never left as a brace.
    #[test]
    fn the_target_status_line_names_the_process_and_its_pid() {
        let _guard = crate::i18n::test_lock();
        let target = ProcessTarget {
            name: Some("MyGame.exe".to_string()),
            pid: Some(4242),
        };
        crate::i18n::set_language(Some("en"));
        assert_eq!(target_summary(&target), "Target: MyGame.exe (PID 4242)");
        crate::i18n::set_language(Some("de"));
        assert_eq!(target_summary(&target), "Ziel: MyGame.exe (PID 4242)");
        crate::i18n::set_language(None);
    }

    /// The front door, rendered and read back: the picked process and the way
    /// back to the built-in path, or the way in to the picker - inside a 320 px
    /// window, with nothing clicked.
    #[test]
    fn the_target_front_door_draws_the_pick_and_the_way_back() {
        use eframe::egui;

        fn collect(shape: &egui::Shape, out: &mut Vec<(String, f32)>) {
            match shape {
                egui::Shape::Text(text) => out.push((
                    text.galley.text().to_string(),
                    text.pos.x + text.galley.size().x,
                )),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }

        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));
        let target = ProcessTarget {
            name: Some("MyGame.exe".to_string()),
            pid: Some(4242),
        };

        let frame = |target: Option<&ProcessTarget>| -> (String, f32) {
            let ctx = egui::Context::default();
            super::install_fonts(&ctx, Some("en"));
            super::apply_theme(&ctx);
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(320.0, 800.0),
                )),
                ..Default::default()
            };
            let mut out = ctx.run_ui(raw, |ui| {
                let action = target_front_door(ui, target);
                assert_eq!(
                    action,
                    TargetRowAction::default(),
                    "a render clicks nothing"
                );
            });
            out.textures_delta.clear();
            let mut drawn = Vec::new();
            for clipped in &out.shapes {
                collect(&clipped.shape, &mut drawn);
            }
            let widest = drawn
                .iter()
                .map(|(_, right)| *right)
                .fold(0.0_f32, f32::max);
            (
                drawn
                    .iter()
                    .map(|(text, _)| text.as_str())
                    .collect::<Vec<_>>()
                    .join("\n"),
                widest,
            )
        };

        let (set, set_wide) = frame(Some(&target));
        assert!(
            set.contains("Target: MyGame.exe (PID 4242)"),
            "the front door did not name the picked process:\n{set}"
        );
        assert!(
            set.contains("Clear target"),
            "the front door did not offer the way back:\n{set}"
        );
        assert!(
            set_wide <= 320.0,
            "the front door drew past a 320 px window ({set_wide} px):\n{set}"
        );

        let (unset, unset_wide) = frame(None);
        assert!(
            unset.contains("Target a process"),
            "the front door did not offer the picker:\n{unset}"
        );
        assert!(
            unset.contains(crate::i18n::t("game.target_none").as_ref()),
            "the front door did not say what it does:\n{unset}"
        );
        assert!(
            unset_wide <= 320.0,
            "the front door drew past a 320 px window ({unset_wide} px):\n{unset}"
        );
        crate::i18n::set_language(None);
    }

    /// The picker both surfaces share: an in-flight load, an empty table, and a
    /// filter nothing matches each say what they are, and a finished table draws
    /// its rows - all without enumerating or blocking.
    #[test]
    fn the_shared_picker_draws_loading_empty_and_no_match_honestly() {
        use eframe::egui;

        fn collect(shape: &egui::Shape, out: &mut Vec<String>) {
            match shape {
                egui::Shape::Text(text) => out.push(text.galley.text().to_string()),
                egui::Shape::Vec(shapes) => {
                    for shape in shapes {
                        collect(shape, out);
                    }
                }
                _ => {}
            }
        }

        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(Some("en"));
        let rows = vec![ProcessEntry {
            pid: 7,
            name: "steam.exe".to_string(),
            path: Some("C:\\Steam\\steam.exe".to_string()),
            started_at: None,
        }];

        let draw = |status: PickerStatus, rows: &[ProcessEntry], query: &str| -> String {
            let ctx = egui::Context::default();
            super::install_fonts(&ctx, Some("en"));
            super::apply_theme(&ctx);
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::pos2(0.0, 0.0),
                    egui::vec2(440.0, 800.0),
                )),
                ..Default::default()
            };
            let mut search = query.to_string();
            let mut refresh = false;
            let mut out = ctx.run_ui(raw, |ui| {
                let mut picker = ProcessPicker {
                    rows,
                    status,
                    search: &mut search,
                };
                let chosen = process_picker(ui, &mut picker, |_| false, &mut refresh);
                assert!(chosen.is_none(), "a render clicks no row");
            });
            out.textures_delta.clear();
            assert!(!refresh, "a render presses no button");
            let mut drawn = Vec::new();
            for clipped in &out.shapes {
                collect(&clipped.shape, &mut drawn);
            }
            drawn.join("\n")
        };

        assert!(
            draw(PickerStatus::Loading, &[], "")
                .contains(crate::i18n::t("game.editor_process_loading").as_ref()),
            "a load in flight must say so"
        );
        assert!(
            draw(PickerStatus::Ready, &[], "")
                .contains(crate::i18n::t("game.editor_process_empty").as_ref()),
            "an empty table must say so"
        );
        assert!(
            draw(PickerStatus::Ready, &rows, "nonesuch")
                .contains(crate::i18n::t("game.editor_process_no_match").as_ref()),
            "a filter nothing matches must say so"
        );
        let ready = draw(PickerStatus::Ready, &rows, "");
        assert!(
            ready.contains("steam.exe"),
            "a finished table draws its rows:\n{ready}"
        );
        assert!(ready.contains('7'), "the row names the PID:\n{ready}");
        crate::i18n::set_language(None);
    }

    /// The elevation gate is unchanged by targeting: an unelevated app asks for
    /// a restart, never a start, whether or not a process is picked. The target
    /// is not an input to the action slot at all, which is exactly why the
    /// requirement stays honest.
    #[test]
    fn an_unelevated_app_still_asks_for_elevation_when_a_process_is_targeted() {
        let _guard = crate::i18n::test_lock();
        crate::i18n::set_language(None);
        // The arguments the frame passes with a target set and nothing boosting:
        // unelevated, and either connected with a relay or not yet ready.
        assert_eq!(
            primary_action(false, false, true, true),
            ("RESTART AS ADMINISTRATOR TO OPTIMIZE".to_string(), true)
        );
        assert_eq!(
            primary_action(false, false, false, true),
            ("RESTART AS ADMINISTRATOR TO OPTIMIZE".to_string(), true)
        );
    }
}
