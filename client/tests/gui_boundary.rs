//! Drift guard **G1–G7** for the LightSpeed GUI/core boundary.
//!
//! WF-081 W2-1 stage **S0** (`docs/design/gui-core-boundary.md` §7). This test
//! is the ratchet the staged migration (`§6 S0–S6`) is measured against: it
//! fails when business logic starts moving back into `client-gui/`, when a new
//! module appears without a budget, or when a moved rule is re-implemented.
//!
//! | Rule | Asserts | Ratchet |
//! |---|---|---|
//! | G1 | every `lightspeed_client::` path in `client-gui/src` is under `app`, or in the S0 allowlist | allowlist only shrinks; S6 = `app` + named exceptions |
//! | G2 | a moved identifier is not *defined* in `client-gui/src` | an identifier is added when its module moves; it never comes back |
//! | G3 | every `client-gui/src/**/*.rs` is budgeted at its measured line count | budgets only lower; a new file needs deliberate registration |
//! | G4 | `client/src/app/**` is UI-free (no `egui`/`eframe`/`Color32`/`Painter`/`Platform`) | absolute |
//! | G5 | `client/src/app/**` is portable (no `cfg(windows)`/`cfg(target_os)`) | absolute |
//! | G6 | the location ladder and the route trace feed no ranking (module-wide, and the display-only test travels) | absolute |
//! | G7 | the core owns the provider seam; the GUI installs it before engine creation | absolute after S5a |
//!
//! The tables below are *measurements of this revision*, not aspirations: G1's
//! allowlist is the exact set of direct `lightspeed_client::` paths found in
//! `client-gui/src` at S0, and G3's budgets are the line counts of every GUI
//! source file. Shrinking them is the migration; widening them is drift.
//!
//! Paths resolve from the workspace root through `CARGO_MANIFEST_DIR`, so the
//! guard runs inside the existing workspace job (`.github/workflows/ci.yml`)
//! and needs no workflow change.

use std::fs;
use std::path::{Path, PathBuf};

/// **G1** named adapter exceptions: the complete, **item-level** list of direct
/// `lightspeed_client::` paths the GUI may still name (W2-3b S6b ratchet).
///
/// Everything under `lightspeed_client::app` is the shared facade and always
/// allowed; the table below is only the engine/games/profiles/latency plumbing
/// that is still GUI-side adapter work (the W2-4 parity slice owns moving the
/// rest). **No module-prefix blanket rows are permitted**: each row names one
/// item, and a row also admits `Item::method` (the prefix rule), which is how the
/// GUI uses these types.
///
/// Adapter map, so every exception is named rather than anonymous:
/// * **engine + session snapshot** — `app.rs`, `main.rs`, `app/tests.rs`
/// * **relay status / chosen relay** — `app.rs`, `app/choice.rs`,
///   `app/route_surface.rs`, `app/relay_choice.rs` (server-leg reports, the
///   triangle floor, the current relay, the destination estimator, ranking)
/// * **game catalogue** — `app/games.rs`, `app/editor.rs`, `app/choice.rs`,
///   `app/presence.rs`, `app/tests.rs`
/// * **user profiles** — `app.rs`, `app/games.rs`, `app/tests.rs`
/// * **latency counters** — `app.rs` (whole-loop + shadow-direct counters)
/// * **process list** — `processes.rs` (the one-shot worker input)
/// * **interception config** — `app.rs`, `app/tests.rs` (target/config handed to
///   the boost command)
const ALLOWED_CLIENT_PATHS: &[&str] = &[
    // engine + session snapshot
    "EngineStatus",
    "LightSpeedEngine",
    "PRIOR_UNCERTAINTY_US",
    // relay status / chosen relay
    "DestinationLegSource",
    "RelayCandidate",
    "ScoredRelay",
    "ServerLegAppReport",
    "ServerLegReport",
    "ServerTraceReport",
    "current_relay",
    "ensure_destination_estimator",
    "rank_relays_at_with_direct",
    // game catalogue
    "games::all_game_keys",
    "games::auto_detect",
    "games::process_names_for_name",
    "games::resolve",
    // user profiles
    "profiles::ProfileStore",
    "profiles::UserGameProfile",
    "profiles::UserServer",
    "profiles::global",
    "profiles::schema::PortRange",
    "profiles::schema::is_valid_key",
    "profiles::schema::parse_port_range",
    "profiles::schema::slugify",
    "profiles::schema::validate_for_save",
    // latency counters
    "latency::direct_jitter",
    "latency::direct_sample",
    "latency::like_for_like",
    "latency::probe_direct",
    "latency::relayed_jitter_ms",
    "latency::relayed_p50_ms",
    "latency::relayed_samples",
    "latency::shadow_direct_jitter_ms",
    "latency::shadow_direct_p50_ms",
    "latency::shadow_direct_samples",
    // process list
    "processes::ProcessEntry",
    "processes::list_running_processes",
    // interception config
    "interceptor::ProcessTarget",
    "interceptor::build_config_for_game_with_port_range",
    "interceptor::build_config_for_process_target",
    "interceptor::resolve_process_target",
    "interceptor::traits::ProcessInfo",
];

/// **G2** identifiers that may not be *defined* in `client-gui/src`. An
/// identifier is added at the stage that moves its module; an allowance here
/// never comes back. `is_holding` (the deleted fallback-hold variant) is
/// already gone and pins the detector; `GuiConfig` is the pre-boundary config
/// model, moved to `lightspeed_client::app::config` as `AppConfig` at S1; the
/// discovery rows moved to `lightspeed_client::app::discovery` at S2; the
/// status/tray/action/leg rules and their types moved to
/// `lightspeed_client::app::{dto,status}` at S3a (W2-1b).
const FORBIDDEN_DEFINITIONS: &[&str] = &[
    "is_holding",
    "GuiConfig",
    // WF-081b (task-74): the shared config model gained the opt-in `autostart`
    // field, so the schema is core-owned more than ever; a GUI-side `struct
    // AppConfig` (a second schema) must never come back. The GUI keeps only the
    // `crate::config` re-export.
    "AppConfig",
    // S2 — discovery/probe/identity.
    "discover_blocking",
    "parse_nodes",
    "friendly_label",
    "measure_all_blocking",
    "measure_spreads_blocking",
    "race_blocking",
    "fastest_addr",
    "probe_blocking",
    "split_body",
    "summarise",
    "connect_latency",
    "http_get",
    "parse_health",
    // S3a — tray, action slot, counters, leg readers, starvation filter.
    "TrayState",
    "tray_state_for",
    "action_slot",
    "mode_counter",
    "routed_count",
    "returned_count",
    "session_delta",
    "session_pair",
    "history_delta",
    "pair_relay_legs",
    "proxy_index_for",
    "same_relay_endpoint",
    "fresh_direct_us",
    "ping_for_server",
    "direct_us_for",
    "refreshed_server_ping",
    "compare_to_direct",
    "early_triangle_floor",
    "relay_cannot_beat_direct",
    "measured_server_leg",
    "relay_measured_server_leg",
    "relay_measured_app_server_leg",
    "relay_measured_trace",
    "scored_for_relay",
    "server_leg_state",
    "without_starved",
    "prefer_proven",
    "rank_without_destination",
    "ServerPing",
    "ServerLegState",
    "MeasuredServerLeg",
    "RelayLegs",
    "FallbackRanking",
    "SERVER_PING_FRESH_SECS",
    "MAX_RELAYS_TRIED",
    // W2-3a (S3b/S4) â€” commands + the warn-only watchdog/history runtime.
    "AppCore",
    "RelayHistory",
    "CarryVerdict",
    "RETURN_SHARE_DENOMINATOR",
    "MIN_FORWARDS",
    "MAX_ENTRIES",
    "VERDICT_TTL",
    "now_secs",
    "WatchdogPlan",
    "WatchdogSnapshot",
    "WatchdogStatus",
    "WindowEvent",
    "ReplyFaultStreak",
    "RelayReplyFault",
    "ReturnPathFault",
    "RelayStarvationWarning",
    "StarvationWarning",
    "StarvationVerdict",
    "RelayReplyWatch",
    "starvation_verdict",
    "plan_return_path_window",
    "apply_plan",
    "apply_starvation_verdict",
    "return_path_recovered",
    "warning_is_about",
    "warning_message_key",
    "relay_reply_fault",
    "relay_reply_fault_message",
    "relay_reply_window_message",
    "ShadowWindowCounters",
    "spawn_relay_reply_watchdog",
    "RelayHealthBase",
    "RelayHealthVerdict",
    "relay_health_verdict",
    "switch_is_safe",
    "decide_switch",
    "RELAY_REPLY_TIMEOUT",
    "RELAY_REPLY_MIN_INTERCEPTED",
    "RELAY_REPLY_MIN_RETURNED",
    "RELAY_REPLY_SHARE_DENOMINATOR",
    "RELAY_REPLY_STREAK",
    "RELAY_HEALTH_SETTLE",
    "RELAY_HEALTH_WINDOW",
    "RELAY_HEALTH_MAX_UNSENT_SHARE",
    "RELAY_SWITCH_MARGIN_MS",
    // W2-3b (S5a) — the location ladder, its offline indexes and the
    // hosting-network classifier. The GUI keeps only a re-export shell for the
    // one pure formatting helper the frozen route surface asks it for.
    "Place",
    "CloudRegion",
    "HostingProvider",
    "CLOUD_REGIONS",
    "HOSTING_PROVIDERS",
    "GeofeedPlace",
    "GeofeedSource",
    "SERVER_GEOFEED_PLACES",
    "SERVER_GEOFEED_SOURCES",
    "record_at",
    "geofeed_at",
    "place",
    "display_point",
    "chooser_point",
    "geofeed_place_text",
    "country_rung",
    "CLOUD",
    "HOSTING",
    "GEOFEED",
    "RECORD",
    "provider_class",
    "install_provider_classifier",
    "class_of",
    "name_hash",
    "fnv1a64",
    "CLOUD_CLASS_TAG",
    "GEOFED_CLASS_TAG",
    "HASH_MASK",
    "COUNTRIES",
    "locate",
    "ranges",
    "start",
    "code_index",
    "geo_countries",
    // W2-3b (S5b) — the client-trace runtime. The GUI keeps the rendered trace
    // row (caption/nearest/provider, now built in `route_surface`); the
    // scheduler, parser, refusal guard, bounded runner and the `CLIENT_TRACE` log
    // contract live in the core.
    "ClientRoute",
    "TraceHop",
    "TraceFailure",
    "TraceResult",
    "AcceptedTrace",
    "TraceTick",
    "GuiTrace",
    "TraceRunner",
    "OsTraceRunner",
    "TraceCache",
    "InFlight",
    "Attempted",
    "TraceChild",
    "ProcessChild",
    "ChildOutcome",
    "trace_hops",
    "probe_allowed",
    "summarize",
    "parse_hops",
    "rtt_value",
    "rtt_ms",
    "run_bounded",
    "hops_from_outcome",
    "platform_trace_command",
    "info_line",
    "parse_info_line",
    "now_unix_ms",
    "MAX_HOPS",
    "TRACE_DEADLINE",
    "CACHE_TTL",
    "POLL_INTERVAL",
    // W2-4a (task-53, R10) — the trace caption has exactly one implementation
    // (`route_surface::RouteTraceSurface`), and the provider suffix is a
    // location-index fact. The retired client_trace helpers must not return.
    "caption",
    "network_label",
    // W2-4b (task-63) — the last GUI-side copy builders are retired. The rendered
    // rows come from `route_surface` (the one caption implementation), so the
    // duplicate copy helpers must not come back: their copy/one-source rules now
    // live in the route-surface fixtures that own the rendered rows.
    "relay_measured_leg_text",
    "relay_measured_app_leg_text",
    "relay_measured_trace_text",
    "expected_path_text",
    "primary_action",
    "relayed_row_text",
    "group_digits",
    // W2-3b (S6) — the pure update rules. Network, download verification and
    // elevation stay in the GUI adapter; the comparison/asset/checksum logic is
    // shared core so the CLI and the GUI cannot drift.
    "ReleaseAsset",
    "compare_versions",
    "strip_v_prefix",
    "numeric_segments",
    "leading_number",
    "parse_sha256_file",
    // W2-4 parity DTO: one capability fact set; a front end fills it, it does not
    // define its own shape.
    "CapabilityFacts",
    // W2-3b S6b — the shared engine-backed composition root. A front end builds
    // it (seams → engine → AppCore); it must not re-implement the sequence.
    "SessionCore",
    "install_core_seams",
    "is_clean_start",
    // W2-3b S6c/S6d — the canonical/legacy config reader and the explicit
    // defaults marker (Q10 reset semantics). The GUI names the pair and the entry
    // points; it must not re-implement the merge, the marker or the writer.
    "ConfigPaths",
    "load_unified",
    "load_unified_in_dir",
    "save_canonical",
    "reset_to_defaults",
    "write_config",
    "legacy_import_disabled",
    "CANONICAL_FILE_NAME",
    "LEGACY_FILE_NAME",
    "META_TABLE",
    "LEGACY_IMPORT_META_KEY",
];

/// **G3** per-file line budgets for `client-gui/src/**/*.rs`, measured at W2-1
/// S0. A new source file must be added deliberately, with a measured budget.
// Direction C "Broadcast console" (task-101): the console palette, mono voice,
// section/ruler/telemetry components and the review-capture fixture grow the
// presentation files; the new values are the measured line counts on the final
// tree and the numbers above are the shipped Instrument 2.0 baselines.
const MODULE_BUDGET: &[(&str, usize)] = &[
    // W2-4b (task-63): the four dead relay-measured/app/RTT copy helpers and the
    // `expected_path_text` builder were deleted (the rendered rows come from
    // `route_surface`), so the two text modules shrink to their label survivors.
    ("client-gui/src/app/choice.rs", 73),
    ("client-gui/src/app/editor.rs", 243),
    ("client-gui/src/app/games.rs", 105),
    // W2-4b: `primary_action` and `relayed_row_text` were deleted (their rules
    // are pinned by the route-surface fixtures that render the slot and row).
    // 2026-11-23 accessibility slice: the action button carries its AccessKit
    // role/name/automation id, and the painted label cells expose their text.
    ("client-gui/src/app/a11y.rs", 631),
    ("client-gui/src/app/a11y_dump.rs", 420),
    ("client-gui/src/app/ledger.rs", 220),
    ("client-gui/src/app/picker.rs", 98),
    // P0-1 lifecycle instrumentation (2026-11-23, WinDivert stability reset):
    // the GUI is the only place that can observe the render-side presence/card
    // facts, so the behaviour-neutral event emission deliberately grew this
    // worker. 482 -> 740. Pure rules stay in the core (`client/src/app/`); a
    // reviewer who rejects the growth can move the scan-result vocabulary there
    // without touching the worker's channels.
    // 2026-11-23 accessibility slice: `GamePresence::idle()` for the offline
    // renderer (same constructor fields, no worker thread).
    // Merge (2026-11-23): both changes kept; the measured count is 758.
    ("client-gui/src/app/presence.rs", 758),
    ("client-gui/src/app/relay_choice.rs", 60),
    // W2-1b (S3a): the pure presentation seam + its contract fixtures; grown by
    // the W2-1c QA fixes (R1-R15) inside the same slice.
    // W2-4b: the retired copy helpers' pinned rules MOVED here (per the task),
    // so the deliberate growth is the fixture move, not new logic: 1915 -> 2268.
    // `route_surface` stays unfrozen for this cleanup only: no `ui()`/drawing
    // change, no new rendered state, no second caption implementation.
    // P0-1 (2026-11-23): the route surface gained the pure `WordTracker` /
    // `word_change_reason` seam and the error-key mapping the render-side
    // `ui.word`/`ui.error` records need. 2268 -> 2482; the rendered rows and
    // copy are unchanged.
    // P0-3 (2026-11-23, roadmap D4 / invariant I7): the same seam gained the
    // no-masking decision — `error_word_for` keeps the live word while a
    // non-fatal route/registration failure is rendered as the existing warn
    // banner, and only a latched capture fault may claim ERROR. The rendered
    // copy keys are unchanged (the banner reuses the taxonomy sentence), so the
    // growth is the rule plus its T13 fixture: 2482 -> 2570.
    ("client-gui/src/app/route_surface.rs", 2570),
    // W2-4b: the eight retired helper tests left (six moved into the route-surface
    // fixtures, one dead formatter and one duplicate targeting test retired), and
    // one rendered-copy fixture arrived with them. 2853 -> 2618.
    // WF-081b Q19 (font debt): the CJK-subset exemption was deleted and a
    // rasterised no-tofu gate for the picker's own names arrived. 2618 -> 2694.
    // WF-081b (task-74): four fixtures arrive - declined-notice mapping/copy,
    // declined-notice 320 px render, A11 disclosure 320 px render, autostart
    // copy + config round-trip. Combined integration count 2694 -> 2957.
    // WF-081b (task-86): four shell fixtures (tier selection incl. the 420 minimum,
    // compact footer row arithmetic, canonical-table dispatch, tray-gated effects);
    // 2957 -> 3137. WF-081b (Q5): the declined-notice assertion became
    // translation-aware; integrated count 3153.
    // 2026-11-23 accessibility slice: 13 regression tests for the named tree,
    // automation ids, live regions, sheets, localization and determinism.
    ("client-gui/src/app/tests.rs", 4019),
    ("client-gui/src/app/theme.rs", 257),
    ("client-gui/src/app/watchdog.rs", 31),
    // W2-4b: `group_digits` (only ever called by the dead `relayed_row_text`
    // helper) is deleted; the rendered counters come from the one `RelayedPair`.
    // 2026-11-23 accessibility slice: painted helpers (rows, sections, footer,
    // toggle, notices, editorial) report their text/role; the picker/editor
    // fields carry names + ids; `sheet_title` returns its response.
    ("client-gui/src/app/widgets.rs", 1147),
    // WF-081b (task-86): the one tier decision (ShellTier/shell_tier), the compact
    // footer padding arithmetic, and the canonical-table dispatch
    // (	ake_app_shortcut/shortcut_effect). 213 -> 341.
    ("client-gui/src/app/window.rs", 342),
    // W2-1b: +315 lines of DTO assembly/getters. It is the one file scheduled to
    // shrink next: W2-2 replaces the legacy readouts with `route_surface` rows.
    // W2-4b: the stale `app_status` dead_code allowance is gone (the frame's
    // one-snapshot seam calls it); one line net removed. 4424 -> 4423.
    // WF-081b (task-74): the declined-notice wiring (`elevation_declined_notice`
    // + `shell_notice`), the Settings → Startup autostart toggle with its
    // once-per-open platform reconcile, and the A11 `update_disclosure` render
    // (now a free fn so its 320 px fit is testable). No width/layout change
    // beyond the one Settings row and the notices.
    // QA R2 (task-74 conditions): the capture-only declined-notice switch is a
    // debug/evidence-gated helper (`capture_elevation_notice`), so a release
    // build can never be forced into the notice. 4423 -> 4573 -> 4581.
    // WF-081b (task-86): the shell consumes one tier decision, the rail drops the
    // theme item_spacing between its rows (116 -> 108), the compact footer gets a
    // real HIT_MIN row, and the app-scope shortcuts are dispatched before any
    // widget sees them. 4581 -> 4628.
    // P0-1 lifecycle instrumentation (2026-11-23): `app_start`/exit intents,
    // the render-side card/word observation seam and the presence transition
    // records are the behaviour-neutral additions this patch required. 4734 ->
    // 5235. No rendered state, command or engine path changed; the pure helpers
    // live with the seam and the guard's `ui.word`/`ui_error` rules are pinned
    // by the new tests in the same file.
    // P0-4 (2026-11-23, roadmap §3.1 P0-4 / T15): the capture-crossing call
    // sites now pass their `source` (button/auto/tray/system) and the
    // start-of-run hold request, and the update-install click records its one
    // `gui.action`. The pure records and the source vocabularies live in the
    // core (`client/src/app/lifecycle.rs`, `commands.rs`); this file grew only
    // by the call-site arguments and their comments. 5235 -> 5269.
    // 2026-11-23 accessibility slice: pre_frame/render_ui split for the offline
    // renderer, the shell/row/map/notice labels and ids, and the dump switches.
    // Merge (2026-11-23): both changes kept; the measured count is 5566.
    ("client-gui/src/app.rs", 5566),
    // W2-3b S5b: the client-trace runtime moved to
    // `lightspeed_client::app::trace`; the GUI keeps the rendered trace row
    // (`route_surface`) and this re-export shell. 1328 -> 145.
    // W2-4a (R10): the duplicate caption helper is retired; the module is the
    // shared-runtime re-export adapter only. 145 -> 22.
    ("client-gui/src/client_trace.rs", 22),
    // W2-1 S1: the config model + I/O + its tests moved to
    // `lightspeed_client::app::config`; the GUI keeps this re-export shell and
    // `paths::config_file`. The budget dropped 528 -> 18 with the move.
    // W2-3b S6c: the shell re-exports the canonical/legacy reader too. S6d adds
    // `reset_to_defaults` (same width). 18 -> 24.
    ("client-gui/src/config.rs", 24),
    ("client-gui/src/crash.rs", 210),
    ("client-gui/src/design.rs", 456),
    // W2-1 S2: the discovery/probe/measurement rules and 20 of their tests
    // moved to `lightspeed_client::app::discovery`; only the one-shot `mpsc`
    // worker wrappers and the wrapper-answers test stay. 816 -> 78.
    ("client-gui/src/discovery.rs", 78),
    // 2026-11-23 accessibility slice: the telemetry map takes a localized
    // description and reports itself as one Image node with a stable id.
    ("client-gui/src/globe.rs", 361),
    // WF-081b (task-74): the capture/fixture locale override
    // (`LIGHTSPEED_LOCALE`, the same family as the app's `LIGHTSPEED_OPEN_*`
    // switches) plus its blank-tolerance test, so the 320x420 evidence captures
    // can be taken in EN/DE/RU on a host whose config pins a language.
    // QA R2 (task-74 conditions): the override is consulted only behind
    // `cfg!(debug_assertions)` and is documented as debug/evidence-only, so a
    // shipped build cannot be language-forced by its environment. 506 -> 544.
    ("client-gui/src/i18n.rs", 544),
    // WF-081b task-81: the smoke hook now logs the `APP_TITLE` constant and holds
    // a bounded visible window, with a unit test pinning title + viewport; 377 -> 432.
    // 2026-11-23 accessibility slice: the `--a11y-dump <path>` branch and its
    // argv parser/tests (windowless JSON render for CI and agent drivers).
    ("client-gui/src/main.rs", 518),
    // W2-3b S6c: `config_file` becomes the canonical/legacy path pair. 81 -> 91.
    ("client-gui/src/paths.rs", 105),
    ("client-gui/src/platform/linux.rs", 106),
    ("client-gui/src/platform/macos.rs", 98),
    // W2-4b: the `TrayAction` platform exemption is documented in place, so the
    // module grows by the explanation, not by code. 220 -> 227.
    // WF-081b (task-74): the `Platform` trait gains the two calls the Settings
    // toggle makes (`autostart_installed`/`set_autostart`), with the observable
    // truth and surfaced-error contracts documented. 227 -> 246.
    ("client-gui/src/platform/mod.rs", 246),
    ("client-gui/src/platform/windows.rs", 484),
    ("client-gui/src/processes.rs", 282),
    ("client-gui/src/relay_history.rs", 15),
    // W2-3b S6: the default-features update check moved to
    // `lightspeed_client::app::update`; this file keeps the download,
    // `sha2` verification and the elevated swap script. 213 -> 183.
    // W2-4 CI fix (2026-11-21): the Windows-only imports and swap script are
    // `#[cfg(windows)]`-gated and a non-Windows install-honesty test was added,
    // so the platform compiler is clean under `-D warnings`; 183 -> 230.
    ("client-gui/src/self_update.rs", 247),
    // W2-3b S5a: the location ladder, its three indexes and its classifier moved
    // to `lightspeed_client::app::location`; the GUI keeps this re-export shell
    // for the one helper the frozen route surface renders. 595 -> 11.
    ("client-gui/src/server_place.rs", 11),
    // WF-081b task-91: the lock/release poll helper and its three tests (macOS
    // flake hardening); 331 -> 430 (plus the `not(windows)` gate on the deadline).
    ("client-gui/src/single_instance.rs", 430),
    // W2-3b S6: `compare_versions` + `ReleaseAsset` moved; the GUI keeps the
    // network check, the `UpdateStatus` type and the asset resolution. 322 -> 224.
    ("client-gui/src/update.rs", 269),
    // W2-4 (task-61) platform parity: the reduced-motion signal, the opt-in
    // autostart writers, the elevation handshake, the canonical shortcut
    // keymap, the Windows tray presentation and the updater channel decision.
    // Added deliberately with measured budgets; the modules are platform
    // integration (GUI-owned by the skill), not the shared core.
    // W2-4 CI fix (2026-11-21): the Windows-only `Off` variant and
    // `from_windows_flag` carry documented `cfg_attr(not(windows))` exemptions
    // (local to each item); 104 -> 121.
    ("client-gui/src/platform/animations.rs", 121),
    // WF-081b (task-74): the Settings toggle path lands here - `installed_current`
    // (registry query / file existence), `apply_current` + `apply_entry` (write,
    // remove, or `reg.exe` through an injectable runner so tests never write the
    // real HKCU value), and the enable/disable round-trip fixtures - and the
    // module-wide `#![cfg_attr(not(test), allow(dead_code))]` is removed. 433 -> 590.
    ("client-gui/src/platform/autostart.rs", 590),
    ("client-gui/src/platform/elevation.rs", 421),
    ("client-gui/src/platform/shortcuts.rs", 413),
    ("client-gui/src/platform/tray_menu.rs", 280),
    ("client-gui/src/platform/updater.rs", 226),
];

/// Tokens that mean UI/framework coupling and may never appear in the shared
/// core module (`**G4**`).
const UI_TOKENS: &[&str] = &[
    "egui",
    "eframe",
    "Color32",
    "Painter",
    "Platform",
    "lightspeed_gui",
];

/// Tokens that mean platform-conditional behaviour and may never appear in the
/// shared core module (`**G5**`).
const PLATFORM_CFG_TOKENS: &[&str] = &["cfg(windows)", "cfg(target_os", "cfg(not(windows))"];

/// Ranking call prefixes. A line that ranks may not also name a location
/// ladder input other than the cloud-exact `chooser_point` (`**G6**`).
const RANKING_CALLS: &[&str] = &["rank_relays_", "rank_without_destination("];

/// The non-ranking location identifiers: passing any of these into a ranking
/// call would let display-only data steer the chooser.
const DISPLAY_ONLY_LOCATION: &[&str] = &["display_point", "place(", "Geofeed", "geofeed"];

/// The display-only proof that travels with the location data (W2-3b S5a).
const DISPLAY_ONLY_TEST: &str = "fn a_geofeed_place_never_steers_the_ranking";

/// The moved display-only modules: the location ladder
/// (`client/src/app/location*`) and the client-vantage route trace
/// (`client/src/app/trace*`). Neither may feed the relay ranking: a ranking call
/// appearing in one is exactly the drift G6 exists to catch.
fn is_display_only_module(rel: &str) -> bool {
    rel.starts_with("client/src/app/location") || rel.starts_with("client/src/app/trace")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("client/ has a workspace parent")
        .to_path_buf()
}

fn gui_src() -> PathBuf {
    repo_root().join("client-gui").join("src")
}

fn core_app_src() -> PathBuf {
    repo_root().join("client").join("src").join("app")
}

fn rust_files(root: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().and_then(|e| e.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }
    out.sort();
    out
}

fn repo_relative(path: &Path) -> String {
    path.strip_prefix(repo_root())
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn read(path: &Path) -> String {
    fs::read_to_string(path).unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()))
}

/// The `lightspeed_client::` path an import or expression names, normalized to
/// `module::Type` / `item`. Grouped imports are expanded, `as` aliases drop,
/// and a trailing method (`Type::method`) is kept so the allowlist stays exact.
fn client_paths(text: &str) -> Vec<String> {
    let mut found = Vec::new();
    let needle = "lightspeed_client::";
    let bytes = text.as_bytes();
    let mut from = 0;
    while let Some(rel) = text[from..].find(needle) {
        let mut i = from + rel + needle.len();
        let mut segments: Vec<String> = Vec::new();
        if text[i..].starts_with('{') {
            // A root-level grouped import: `use lightspeed_client::{A, B};`.
            let (items, end) = expand_group(text, i + 1);
            for item in expand_items(items, "") {
                found.push(item.trim_start_matches("::").to_string());
            }
            i = end;
            from = i.max(from + rel + needle.len());
            continue;
        }
        while let Some((seg, next)) = ident_at(text, i) {
            i = next;
            if text[i..].starts_with("::{") {
                let (items, end) = expand_group(text, i + 3);
                let prefix = if segments.is_empty() {
                    seg
                } else {
                    format!("{}::{seg}", segments.join("::"))
                };
                for item in expand_items(items, &prefix) {
                    found.push(item);
                }
                i = end;
                break;
            }
            segments.push(seg);
            if text[i..].starts_with("::") {
                i += 2;
                continue;
            }
            found.push(segments.join("::"));
            break;
        }
        from = i.max(from + rel + needle.len());
        if from >= bytes.len() {
            break;
        }
    }
    found
}

fn ident_at(text: &str, start: usize) -> Option<(String, usize)> {
    let bytes = text.as_bytes();
    let mut i = start;
    if i >= bytes.len() || !(bytes[i].is_ascii_alphabetic() || bytes[i] == b'_') {
        return None;
    }
    let begin = i;
    i += 1;
    while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
        i += 1;
    }
    Some((text[begin..i].to_string(), i))
}

/// Expand `{a, b::{c, d}}` at `pos` (the first byte of the group body, just
/// after `{`), returning the top-level items and the byte index after `}`.
fn expand_group(text: &str, pos: usize) -> (Vec<String>, usize) {
    let bytes = text.as_bytes();
    let mut items = Vec::new();
    let mut buf = String::new();
    let mut depth = 1usize;
    let mut i = pos;
    while i < bytes.len() {
        match bytes[i] {
            b'{' => {
                depth += 1;
                buf.push('{');
            }
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    if !buf.trim().is_empty() {
                        items.push(buf.trim().to_string());
                    }
                    return (items, i + 1);
                }
                buf.push('}');
            }
            b',' if depth == 1 => {
                items.push(buf.trim().to_string());
                buf.clear();
            }
            _ => buf.push(bytes[i] as char),
        }
        i += 1;
    }
    panic!("unbalanced import group in gui_boundary guard");
}

fn expand_items(items: Vec<String>, prefix: &str) -> Vec<String> {
    let mut out = Vec::new();
    for raw in items {
        let item = raw.split(" as ").next().unwrap_or(&raw).trim().to_string();
        if item.is_empty() {
            continue;
        }
        if let Some(open) = item.find('{') {
            let head = item[..open].trim_end_matches(':').trim();
            let (inner, _) = expand_group(&item, open + 1);
            let base = if head.is_empty() {
                prefix.to_string()
            } else {
                format!("{prefix}::{head}")
            };
            out.extend(expand_items(inner, &base));
        } else {
            out.push(format!("{prefix}::{item}"));
        }
    }
    out
}

fn allowed_client_path(path: &str) -> bool {
    if path == "app" || path.starts_with("app::") {
        return true;
    }
    ALLOWED_CLIENT_PATHS.iter().any(|allowed| {
        path == *allowed
            || path
                .strip_prefix(allowed)
                .is_some_and(|rest| rest.starts_with("::"))
    })
}

/// **G1** — every direct `lightspeed_client::` path is under `app` or in the
/// S0 allowlist.
#[test]
fn g1_gui_uses_only_the_app_facade_or_allowlisted_items() {
    let mut violations = Vec::new();
    for file in rust_files(&gui_src()) {
        let text = read(&file);
        for path in client_paths(&text) {
            if !allowed_client_path(&path) {
                violations.push(format!(
                    "{}: lightspeed_client::{path}",
                    repo_relative(&file)
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "G1: the GUI may only name `lightspeed_client::app` or allowlisted items; \
         move the call through the facade instead:\n{}",
        violations.join("\n")
    );
}

/// **G2** — a moved identifier is not defined in `client-gui/src`.
#[test]
fn g2_moved_identifiers_are_not_reimplemented_in_the_gui() {
    let mut violations = Vec::new();
    for file in rust_files(&gui_src()) {
        let text = read(&file);
        for ident in FORBIDDEN_DEFINITIONS {
            let keywords = [
                "fn", "struct", "enum", "trait", "type", "const", "static", "mod",
            ];
            for keyword in keywords {
                let pattern = format!("{keyword} {ident}");
                let mut search_from = 0;
                while let Some(found) = text[search_from..].find(&pattern) {
                    let end = search_from + found + pattern.len();
                    search_from = end;
                    // Whole-identifier boundary: `relay_measured_trace` is not
                    // implemented by `relay_measured_trace_text`.
                    let next = text[end..].chars().next();
                    let continues_identifier = next
                        .map(|c| c.is_ascii_alphanumeric() || c == '_' || c == ':')
                        .unwrap_or(false);
                    if continues_identifier {
                        continue;
                    }
                    violations.push(format!("{}: defines `{ident}`", repo_relative(&file)));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "G2: these rules have moved to the shared crate; do not re-implement them \
         in the GUI:\n{}",
        violations.join("\n")
    );
}

/// **G3** — every GUI source file is registered with a budget, and none grew.
#[test]
fn g3_gui_source_files_are_budgeted_and_do_not_grow() {
    let mut measured = Vec::new();
    let mut violations = Vec::new();
    for file in rust_files(&gui_src()) {
        let rel = repo_relative(&file);
        let lines = read(&file).lines().count();
        measured.push(rel.clone());
        match MODULE_BUDGET.iter().find(|(path, _)| *path == rel) {
            // The budget is the measured line count: growth fails, and stale
            // slack fails too, so the table cannot drift away from the tree
            // (QA W2-1 R13).
            Some((_, budget)) if lines > *budget => violations.push(format!(
                "{rel}: {lines} lines, budget {budget} (reduce the module or move logic to the core)"
            )),
            Some((_, budget)) if lines < *budget => violations.push(format!(
                "{rel}: {lines} lines, budget {budget} (tighten MODULE_BUDGET to the measured count)"
            )),
            Some(_) => {}
            None => violations.push(format!(
                "{rel}: not registered in MODULE_BUDGET (add a deliberate budget)"
            )),
        }
    }
    let registered: Vec<&str> = MODULE_BUDGET.iter().map(|(path, _)| *path).collect();
    for path in registered {
        if !measured.iter().any(|m| m == path) {
            violations.push(format!(
                "{path}: budgeted but missing from the tree (update MODULE_BUDGET when a module moves)"
            ));
        }
    }
    assert!(
        violations.is_empty(),
        "G3: client-gui/ is a thin adapter and must not grow:\n{}",
        violations.join("\n")
    );
}

fn core_app_files() -> Vec<PathBuf> {
    rust_files(&core_app_src())
        .into_iter()
        .filter(|file| {
            // The guard itself lives in client/tests; this is documentation of
            // the invariant, not an exclusion.
            !repo_relative(file).contains("/tests/")
        })
        .collect()
}

/// **G4** — the shared core module is UI-free.
#[test]
fn g4_core_app_has_no_ui_coupling() {
    let mut violations = Vec::new();
    for file in core_app_files() {
        let text = read(&file);
        for token in UI_TOKENS {
            if text.contains(token) {
                violations.push(format!("{}: names `{token}`", repo_relative(&file)));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "G4: client/src/app/ is logic shared with the CLI; it may not depend on the UI:\n{}",
        violations.join("\n")
    );
}

/// **G5** — the shared core module is portable: no platform-conditional
/// behaviour. Windows-only facts stay behind `client-gui/src/platform/`.
#[test]
fn g5_core_app_has_no_platform_conditional_behaviour() {
    let mut violations = Vec::new();
    for file in core_app_files() {
        let text = read(&file);
        for token in PLATFORM_CFG_TOKENS {
            if text.contains(token) {
                violations.push(format!("{}: contains `{token}`", repo_relative(&file)));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "G5: client/src/app/ must build the same on every target; cfg(windows) belongs in platform/:\n{}",
        violations.join("\n")
    );
}

/// **G6** — the location ladder and the route trace feed no ranking. Only the
/// cloud-exact `chooser_point` may reach the chooser; geofeed/hosting/country/
/// unknown points are display-only (spec A8, owner decision 2026-11-20).
///
/// W2-3b S5a moved the ladder, its data and its tests into
/// `client/src/app/location*`, so the rule also asserts that (a) no ranking call
/// appears anywhere in a display-only module — neither the data nor the trace
/// can be wired into the chooser from its new home — and (b) the display-only
/// proof (`a_geofeed_place_never_steers_the_ranking`, `server_place.rs:456-466`
/// before the move) travelled with the data instead of being left behind.
/// S5b adds the client-vantage trace to the no-ranking module set: it is a
/// placement anchor for display, never a measured leg.
#[test]
fn g6_location_never_steers_the_ranking() {
    let mut violations = Vec::new();
    for file in core_app_files() {
        let rel = repo_relative(&file);
        let text = read(&file);
        for (index, line) in text.lines().enumerate() {
            if !RANKING_CALLS.iter().any(|call| line.contains(call)) {
                continue;
            }
            if is_display_only_module(&rel) {
                violations.push(format!(
                    "{rel}:{}: a display-only module may not rank relays: {}",
                    index + 1,
                    line.trim()
                ));
                continue;
            }
            for token in DISPLAY_ONLY_LOCATION {
                if line.contains(token) {
                    violations.push(format!(
                        "{rel}:{}: a ranking call also names display-only location data `{token}`: {}",
                        index + 1,
                        line.trim()
                    ));
                }
            }
        }
    }
    assert!(
        violations.is_empty(),
        "G6: only the cloud-exact chooser_point may be a ranking input:\n{}",
        violations.join("\n")
    );

    let travelled = core_app_files()
        .into_iter()
        .any(|file| read(&file).contains(DISPLAY_ONLY_TEST));
    assert!(
        travelled,
        "G6: the display-only proof `{}` must travel with the location data \
         (it lived in client-gui/src/server_place.rs:456-466 before S5a)",
        DISPLAY_ONLY_TEST.trim()
    );
}

/// **G7** — the provider-classifier seam is owned by the core.
///
/// W2-3b S5a moved the hosting-network classifier and the location indexes it
/// reads into `client/src/app/location`, so the raw installer call exists
/// exactly once there and **zero** times in `client-gui/src` (the GUI only asks
/// the core to install it, before the engine is created). A front end owning the
/// seam again, or the install being dropped from the creation path, fails here.
#[test]
fn g7_provider_seam_is_core_owned_and_installed_before_engine_creation() {
    let mut seam_sites = Vec::new();
    let mut installer_definitions = Vec::new();
    for file in core_app_files() {
        let rel = repo_relative(&file);
        let text = read(&file);
        for (index, line) in text.lines().enumerate() {
            if line.contains("install_server_provider_lookup(")
                && !line.trim_start().starts_with("//")
            {
                seam_sites.push(format!("{rel}:{}", index + 1));
            }
            if line.contains("pub fn install_provider_classifier(") {
                installer_definitions.push(rel.clone());
            }
        }
    }
    assert_eq!(
        seam_sites.len(),
        1,
        "G7: exactly one component installs the classifier seam (found {seam_sites:?})"
    );
    assert_eq!(
        seam_sites[0].split(':').next().unwrap_or_default(),
        "client/src/app/location/provider.rs",
        "G7: after S5a the core owns the classifier seam, not a front end"
    );
    assert_eq!(
        installer_definitions,
        vec!["client/src/app/location/provider.rs".to_string()],
        "G7: the core exposes exactly one classifier installer"
    );

    // No front end may install the seam directly again.
    let mut gui_sites = Vec::new();
    for file in rust_files(&gui_src()) {
        for (index, line) in read(&file).lines().enumerate() {
            if line.contains("install_server_provider_lookup(") {
                gui_sites.push(format!("{}:{}", repo_relative(&file), index + 1));
            }
        }
    }
    assert!(
        gui_sites.is_empty(),
        "G7: the GUI must not install the provider seam directly; call the core \
         installer instead (found {gui_sites:?})"
    );

    // ... and the GUI's composition root installs it before creating the engine.
    let main = read(&gui_src().join("main.rs"));
    assert_eq!(
        main.matches("install_provider_classifier();").count(),
        1,
        "G7: the GUI installs the classifier exactly once (lost install and duplicate both fail)"
    );
    let install_at = main.find("install_provider_classifier();").expect(
        "G7: the GUI composition root must install the classifier (the seam must not be lost)",
    );
    let engine_at = main.find("LightSpeedEngine::new(").expect(
        "G7: the GUI composition root is expected to create the engine after installing the seam",
    );
    assert!(
        install_at < engine_at,
        "G7: the classifier must be installed before engine creation"
    );
}

/// The guard's tables are measurements, not guesses: a measurement helper used
/// only by the mutation proof in the evidence report. Kept in the test file so
/// a reviewer can re-derive a budget without a second tool.
#[test]
fn g3_measurement_helper_matches_the_table() {
    let measured = rust_files(&gui_src()).len();
    assert_eq!(
        measured,
        MODULE_BUDGET.len(),
        "MODULE_BUDGET must cover every GUI source file"
    );
}
