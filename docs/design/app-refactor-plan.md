# `client-gui/src/app.rs` refactor plan (audit F-13)

| | |
|---|---|
| Author | `interface-designer` (task-6, delegated by `lead`) |
| Date | 2026-11-18 |
| Revision measured | worktree at `bc88573`, clean tree; `client-gui/src/app.rs` 9,316 lines |
| Status | **due diligence + plan only. No code changed this round; `app.rs` untouched.** |
| Write scope | this file. Evidence: `wat/evidence/2026-11-18/team/interface-designer/` |
| Cargo | not run by this persona (build lock held by `infra-dev`; a split would be executed by `rust-dev` under the gate) |

## Recommendation (read this first)

**Do the split after the next release, and do only Phase A — a mechanical move of
the file's self-contained, test-pinned half into sibling modules. Do not do
Phase B (moving the 72 `impl LightSpeedApp` methods) or Phase C (regrouping the
77 fields) without a feature that forces it.**

- Cost of Phase A: 11 steps (A0-A10); ~5,300 lines relocated; ~80 `pub(super)`
  item annotations and ~40-60 `pub(super)` struct-field annotations, all
  compiler-driven; no logic edits. Estimate **0.5-1 day** for a `rust-dev` who
  has this document and the gate.
- Benefit of Phase A: `app.rs` 9,316 → **~4,050 lines (-57%)**; the test module
  (2,504 lines, 78 tests) and ten self-contained modules become their own files
  (each with its own tests, where it has any); merge-conflict surface shrinks to
  the surface actually being edited.
- Benefit **not** claimed: no correctness, latency or behaviour improvement.
  100% of the moved code is already covered by the 78 unit tests; **none** of
  the `impl LightSpeedApp` methods that stay behind has a unit test (verified:
  no test constructs `LightSpeedApp`). A mechanical move therefore buys
  navigation and conflict surface, not safety.
- Falsifier (split first, before the next release): **if the next two features
  each change >200 lines in `app.rs`, or one feature touches ≥4 of the Phase-A
  clusters, run Phase A first.** Measured churn calibrates the threshold:
  139 commits touched `app.rs` in the last 90 days, 17,703 changed lines,
  median 46 lines/commit; 16 of the 139 exceeded 200 lines and 6 exceeded 700.
  Two consecutive >200-line features is therefore an unusual, detectable
  signal (roughly one in nine commits each), not normal variance.
- Why not now: the queued workstream is a release that must prove the F-01
  MTU fix and the hardened publish path *without* unrelated diff noise, and
  nothing queued is currently blocked by the file size. The tree is clean and
  there are no long-lived branches, so the split loses nothing by waiting one
  release — it is a scheduling decision, not a technical blocker.

## 1. Inventory (measured, not read-every-line)

Method: `grep`/`read` on the file plus a reproducible script,
`wat/evidence/2026-11-18/team/interface-designer/inventory.ps1`, output
`inventory.txt` (cluster line counts, `self.<field>` reference counts, field
inventory, per-test pins, cross-cluster calls). Re-run from the repo root with
`powershell -NoProfile -ExecutionPolicy Bypass -File wat/evidence/2026-11-18/team/interface-designer/inventory.ps1`.

### 1.1 File facts

| Fact | Value |
|---|---|
| Total lines | **9,316** (`ReadAllLines`); 8,807 non-blank; 6,995 non-blank non-`//` |
| `fn` items | **254**: 91 top-level fns, 72 `impl LightSpeedApp` methods, 78 `#[test]` fns, 13 other (small `impl` blocks + test-local helpers) |
| `impl` blocks | 2 on `LightSpeedApp` (`1711`, `5043`) + 5 small ones |
| `LightSpeedApp` fields | **77**, declared `1493-1682` (190 lines, all private) |
| `self.<field>` references inside the 72 methods | **497** |
| `new()` | `1711-1896` (187 lines: config resolution, side-effect ordering, an 80-line struct literal, then `connect_selected()`) |
| `#[cfg(test)] mod tests` | `6813-9316` = **2,504 lines (26.9% of the file)**, 78 tests |
| Tests that construct `LightSpeedApp` | **0** (grep `LightSpeedApp` finds only the declaration, the two `impl`s, and two doc links) |
| egui-facing lines | 49 lines mention `egui::Ui`/`Context`/`Frame`; drawing happens in 7 `&mut self` methods + 25 free widget fns |
| Churn, last 90 days | 139 commits, 17,703 changed lines, median 46 lines/commit; 16 commits >200 lines, 6 >700 |

Correction to audit F-13: its "8,735 lines" is the **non-blank** count at
`343530b` (`Measure-Object -Line` counts non-empty lines); the file is 9,243
total lines there and **9,316** at HEAD (+72 non-blank, +74 net lines from the
F-01 MTU commit `bc159d3`). Use 9,316 as the total when sizing the split.

### 1.2 Cluster map (line ranges at HEAD; ranges are inclusive)

| # | Range | Lines | What it is | `self.field` refs / distinct fields | Tests that pin it |
|---|---|---|---|---|---|
| 1 | 1-36 | 36 | module doc + `use` imports | - | - |
| 2 | 37-97 | 61 | `TrayState` enum + 7 timing/limit consts (`RELAY_RECHECK_INTERVAL`, `GAME_PRESENCE_INTERVAL`, `HOLD_FALLBACK`, `LOCK_SETTLE`, `MAX_RELAYS_TRIED`, `SERVER_PING_FRESH_SECS`, `PING_REFRESH_AFTER`) | - | - |
| 3 | 98-133, 141-826, 215 | **723** | **reply watchdog**: `RelayReplyWatch`, `ReplyFallback`, `RelayRejection`, `RetestTarget`, `RelayReplyFault`, `ReplyFaultStreak`(+impl `observe`), `ShadowWindowCounters`(+impl `read`), `retry_seconds_left`, `retry_delay`, `retest_is_about`, `relay_switch_allowed`, `relay_reply_fault`, `zero_return_window`, `relay_reply_fault_message`, `relay_reply_window_message`, `spawn_relay_reply_watchdog`; 6 consts (`RELAY_RETRY_BACKOFF`, `RELAY_REPLY_*`) | 0 (thread + pure fns) | **14**: `no_reply_watchdog_…`, `the_live_24_percent_window_…`, `a_repeated_24_percent_share_…`, `the_live_run_windows_replay_…`, `a_zero_return_window_…`, `a_broken_injector_…`, `different_faults_do_not_share_a_streak`, `a_re_test_stays_…`, `a_background_rank_never_moves_…`, `a_healthy_server_share_…`, `a_silent_relay_and_a_broken_injector_…`, `a_local_send_failure_is_named_…`, `every_relay_reply_fault_carries_…`, `a_judged_window_is_traced_…` |
| 4 | 803-1001 | 199 | **relay choice / health verdicts**: `type RelayLegs` (817), 4 consts (`RELAY_HEALTH_*` 803-815, `RELAY_SWITCH_MARGIN_MS` 967), `RouteRanking`, `MeasuredServerLeg`, `ServerPing`, `ServerLegState`, `RelayHealthBase`, `RelayHealthVerdict`(+`sentence`), `switch_is_safe`, `relay_health_verdict`, `env_proxies` | 0 | 3: `a_gate_off_is_the_only_safe_moment_…`, `relay_health_reads_movement_not_totals`, `relay_health_tolerates_loss_and_a_still_flow` (+ referenced by most ranking tests) |
| 5 | 1002-1102 | 101 | **game registry**: `GameEntry`, `games()`, `PickedGame`, `GamePickRow`, `profile_store`, `user_profiles`, `user_servers`, `user_game_keys`, `game_pick_rows` | 0 | 3: `game_table_matches_the_client_registry`, `the_my_games_group_sits_…`, `a_saved_server_round_trips_…` |
| 6 | 1103-1341 | 239 | **game editor + validation**: `GameEditorDraft`(+4 methods), `parse_user_port_range`, `split_process_names`, `builtin_claim_of_*`, `DraftVerdict`(+`ok`), `validate_draft`, `draft_reasons`, `server_from_fields`, `save_user_profiles` | 0 | 4: `the_editor_verdict_mirrors_the_schema_save_rules`, `a_malformed_range_blocks_saving_…`, `a_saved_server_round_trips_…`, `the_game_editor_sheet_draws_…` |
| 7 | 1342-1382, 1474-1492 | 60 | state enums that are direct field types: `UpdateCheckState`, `DiscoveryState`, `ProcessListState`, `HealthProbe`, `WinnerProbe` | 0 | indirectly (`primary_action_*`, discovery tests) |
| 8 | 1383-1473 | 91 | **process picker**: `PickerStatus`, `ProcessPicker<'a>`, `pick_game_entry`, `picker_status`, `target_for_pick`, `target_summary`, `apply_target_choice` | 0 | 4: `the_picker_reports_…`, `a_picked_process_becomes_…`, `clearing_the_target_returns_…`, `the_target_status_line_…` |
| 9 | 1493-1682 | 190 | `pub struct LightSpeedApp<P: Platform>` — 77 private fields in 8 commented sections (engine/status, relay reply, proxy connection, game routing, system state, sheets/pickers, boost, update) | - | - |
| 10 | 1684-1710 | 27 | `lock_or_recover`, `spawn_rtt_probe` | - | 1: `a_poisoned_mutex_is_recovered_not_panicked` |
| 11 | 1711-1896 | 187 | **`new()`**: tray, engine snapshot, `try_auto_detect_game`, config load, `set_path_mtu_cap`, language pin, Arcs, watchdog spawn, then the 77-field literal, then env-var sheets and `connect_selected()` | 0 | **none** |
| 12 | 1898-2031 | 134 | connect/discovery accessors: `tray_available`, `selected_entry`, `selected_proxy_addr`, `selected_game`, `connect_selected`, `restart_interceptor_on_selected_relay`, `start_discovery`, `start_process_list`, `poll_process_list` | 21 / 15 (`engine, proxies, selected_proxy_idx, discovery, discovery_error, process_list, process_rows, target_process, tray, …`) | **none** |
| 13 | 2032-2318 | 287 | persist/choose/measure: `persist_config`, `start_relay_race`, `choose_relay_for_destination`, `start_relay_measurement`, `poll_relay_measurement` | 32 / 19 (`relay_legs, relay_race, relay_measurement, route_ranking, destination_winner, server_ping, …`) | **none** |
| 14 | 2319-2463 | 145 | follow/hold: `follow_game_server`, `release_stale_hold`, `game_server_addr` | 29 / 12 (`chosen_for_server, locked_addr, locked_since, held_since, server_chosen, …`) | **none** |
| 15 | 2464-2707 | 244 | race/winner/discovery polls: `poll_relay_race`, `consider_connected_winner`, `poll_winner_probe`, `maybe_rerace`, `poll_discovery`, `apply_discovery`, `poll_health`, `shutdown_engine` | 51 / 12 (`relay_race, winner_probe, health, last_race, …`) | **none** |
| 16 | 2708-2876 | 169 | boost + latency trace: `start_update_check` (misfiled here, see §1.4), `start_boost`, `stop_boost`, `log_latency_comparison` | 24 / 15 (`boost_start, relayed_base, health_base, relay_health, …`) | **none** |
| 17 | 2877-3044 | 168 | history/verdict recording: `record_watchdog_verdict`, `record_relay_history`, `refresh_server_ping`, `server_ping_for`, `session_key` | 23 / 7 | **none** |
| 18 | 3045-3259 | 215 | health/presence/admin: `poll_relay_health`, `restart_elevated`, `poll_game_presence`, `reveal_log_file`, `toggle_relay_connection` | 52 / 16 | **none** |
| 19 | 3260-3902 | **643** | **drawing A — rail/route/ledger**: `state_rail`, `route_card`, `ledger_rows`, `server_leg_value`, `cannot_beat_direct`, `current_client_leg_us`, `run_boosting`, `ledger_row/value`, `relay_value`, `game_server_value`, `direct_fallback_text`, `relay_rejected_now`, `relay_rejection_now`, `watchdog_chain`, `session_sent`, `routed_value` (17 methods) | 66 / 12 | **none** |
| 20 | 3903-4354 | 452 | **drawing B — config/combos/manual form**: `config_body`, `ranked_client_leg_us`, `leg_ms`, `boost_server_combo`, `game_combo`, `target_control`, `manual_section` | 75 / **30** (widest field reach in the file) | **none** |
| 21 | 4355-5042 | **688** | **drawing C — sheets**: `settings_sheet`, `settings_body`, `game_editor_sheet`, `target_picker_sheet`, `start_manual_redirect`, `proxy_manager_body`, `update_sheet` | 78 / 29 | **none** |
| 22 | 5043-5376 | 334 | `impl eframe::App::ui()` — the only frame orchestrator: fonts, close decision, tray poll, 11 polls, tray state machine, panel order (rail → route card → footer → action bar → central scroll), sheet dispatch, repaint cadence | 46 / 19 | **none** |
| 23 | 5378-6812 | **1,436** | **pure helpers + widgets**: theme/fonts (`font_definitions`, `cjk_order`, `install_fonts`, `semibold`, `apply_theme`), text (`rtt_colour`, `parse_server_addr`, `connect_instruction`), leg/ranking helpers (`pair_relay_legs` … `relay_cannot_beat_direct`, `try_auto_detect_game`), window (`brand_mark_texture`, `CloseDecision`, `MIN/MAX_WINDOW_HEIGHT`, `monitor_geometry`, `window_height_limit`, `should_race`, `should_apply_race`, `close_decision`, `apply_discovery_result`), ledger/action (`parse_custom_port_range` … `field_label`), shared widget builders (`game_editor_body`, `process_picker`, `process_row`, `target_front_door`, `sheet_*`, `sparkline`, `group_digits`) | 0 | **36 tests** reference a helper in it directly (largest single block; a test may also pin another cluster's helper) |
| 24 | 6813-9316 | **2,504** | `#[cfg(test)] mod tests` — 78 `#[test]` fns + 5 local helpers (`addr`, `sample`, `scored_relay`, `ping_for`, `shadow_window`) | - | - |

Cross-checks that shape the plan:

- **All 78 tests live in clusters 3-8, 10 and 23** — i.e. in exactly the code
  Phase A moves. Clusters 11-22 (the 72 `impl` methods + `new()`, 3,666 lines)
  have **zero** direct unit tests.
- Tests are **function-level**: they call free fns and small structs, and the
  UI tests drive the free body builders through `egui::Context::run_ui`
  (e.g. `the_game_editor_sheet_draws_every_field_and_the_key` at 8878 calls
  `game_editor_body`, not `LightSpeedApp::game_editor_sheet`). So the tests
  protect a module move of clusters 3-8/23 directly; they protect nothing about
  moving the `impl` methods.
- **Shared fields**: the state that a split cannot privatise per surface —
  `status` (9 impl clusters), `proxies` (8), `engine` (8), `target_process` (6),
  `selected_proxy_idx` (6), `auto_select` (5), `game_auto`/`game_manual` (5
  each), then a long tail of 3-4-cluster fields. This is why Phase C (splitting
  the struct into sub-structs) is a separate, much larger change, not a
  side effect of moving files.
- **Cross-cluster helper API** (what a module split must export): computed and
  listed in `inventory.txt` under `CROSS;…`. Roughly 80 free fns/consts and
  **53 methods** are called from outside their own 200-700-line neighbourhood;
  the highest-fanout helpers are `lock_or_recover` (11 impl clusters + the
  watchdog + the tests), `games()`, `relay_health_verdict`,
  `relay_reply_fault_message`, `should_race`, `parse_server_addr`, and the
  ledger counters.

### 1.3 What each cluster touches (fields pulled from the actual references)

The two drawing clusters 20-21 and `ui()` reach the most state: `config_body`
touches 30 distinct fields, `settings_body`/sheets 29, `ui()` 19. By contrast
the watchdog (cluster 3), registries (4-8) and helpers (23) touch **no**
`LightSpeedApp` state at all — they are free functions over arguments. That is
the property that makes Phase A mechanical and Phase B not.

### 1.4 Defects the map already shows (fix opportunistically, do not gate the split on them)

- `start_update_check` (2708) sits between `poll_health` and `start_boost`: an
  update-check method inside the boost cluster. Move it with the update/state
  group, not with boost.
- `RelayLegs` is a type alias at 817, i.e. the relay-choice cluster owns it; a
  cluster scan starting at 827 (the first struct) misses it.
- Field doc comments in the struct are out of sync with their fields:
  `1498-1508` document `relay_rejected`/`relay_retry_at`/`relay_retest`, but
  `1549-1552` attach "estimated whole-loop RTT" / "how long ago the probe behind
  `path_rtt_ms` was taken" to `direct_fallback` and `relayed_base` — the two
  fields those sentences describe (`path_rtt_ms`, probe age) no longer exist.
  Do not propagate that comment block into a module as if it were current.
- Several design docs already cite stale `app.rs` line numbers (e.g.
  `docs/design/custom-games-and-process-targeting.md:27` cites `app.rs:94` for
  `all_game_keys()`; line 94 is a blank line and the call is
  `lightspeed_client::games::all_game_keys()` inside `games()` at 1009-1012).
  The split invalidates nothing that is currently correct; the rule is: cite
  item names, never line numbers, in docs.

## 2. Target layout

`client-gui/src/app.rs` **stays the module root** (`mod app;` in `main.rs` is
unchanged, and `crate::app::…` paths used by `main.rs`/`platform/*.rs` keep
resolving). Rust 2018 lets `app.rs` declare `mod watchdog;` whose file is
`client-gui/src/app/watchdog.rs` — no `lib.rs`, no directory-module conversion,
no path changes elsewhere.

### 2.1 Phase A — move these eleven units (mechanical, test-pinned code)

| New file | Moves from `app.rs` (HEAD lines) | Line count | Items |
|---|---|---|---|
| `app/tests.rs` | 6813-9316 | 2,504 | the whole `#[cfg(test)] mod tests` |
| `app/watchdog.rs` | 98-133, 215, 141-826 | 723 | 7 types/enums, 2 small impls, 9 fns, 6 consts |
| `app/relay_choice.rs` | 803-1001 | 199 | `RelayLegs` alias, 4 consts, 6 types, `switch_is_safe`, `relay_health_verdict`, `env_proxies` |
| `app/games.rs` | 1002-1102 | 101 | `GameEntry`, `games()`, `PickedGame`, `GamePickRow`, 5 registry/store fns |
| `app/editor.rs` | 1103-1341 | 239 | `GameEditorDraft`(+4 methods), `DraftVerdict`(+`ok`), validation/draft/server fns |
| `app/picker.rs` | 1383-1473 | 91 | `PickerStatus`, `ProcessPicker<'a>`, 5 picker fns |
| `app/theme.rs` | 5378-5628 | 251 | 8 theme/text fns (`font_definitions`, `cjk_order`, `install_fonts`, `semibold`, `apply_theme`, `rtt_colour`, `parse_server_addr`, `connect_instruction`) |
| `app/choice.rs` | 5629-5903 | 275 | 16 leg/ranking/trace helpers |
| `app/window.rs` | 5904-6067 | 164 | `brand_mark_texture`, `CloseDecision`, `MIN/MAX_WINDOW_HEIGHT`, 4 window consts (2 `pub(crate)`), `monitor_geometry`, `monitor_local_top`, `window_height_limit`, `should_race`, `should_apply_race`, `close_decision`, `apply_discovery_result` |
| `app/ledger.rs` | 6068-6361 | 294 | `parse_custom_port_range`, `primary_action`, `ActionKind`, `action_button`, `LedgerValue`, 4 counter fns, 4 row fns, 2 label fns |
| `app/widgets.rs` | 6362-6812 | 451 | `EditorActions`, `game_editor_body`, `process_picker`, `TargetRowAction`, `target_front_door`, `process_row`, `invalid_text_field`, `field_row`, `hairline`, `section_label`, `sheet_title`, `sheet_frame`, `sheet_bounds`, `sparkline`, `group_digits` |

Total moved: **5,292 lines**. What stays in `app.rs` (~4,050 lines):
module doc + imports; the timing consts at 37-97; the three field-type enums at
1342-1382 (`UpdateCheckState`, `DiscoveryState`, `ProcessListState`) and the two
probe structs at 1474-1492 (`HealthProbe`, `WinnerProbe`); the 190-line struct
(1493-1682); `lock_or_recover`/`spawn_rtt_probe` (1684-1710); `new()`
(1711-1896); the 71 remaining `impl` methods (1898-5376); plus a `mod` /
re-export block.

### 2.2 Root re-export block (the trick that keeps the diff small)

At the top of `app.rs`, after the imports:

```rust
mod choice;
mod editor;
mod games;
mod ledger;
mod picker;
mod relay_choice;
mod theme;
mod watchdog;
mod widgets;
mod window;
#[cfg(test)]
mod tests;

// Everything the root impl and the tests still name, kept resolvable so no call
// site changes. `rustfmt` will keep one item per line; that formatting churn is
// expected and is the only non-move edit in `app.rs`.
use choice::*;
use editor::*;
use games::*;
use ledger::*;
use picker::*;
use relay_choice::*;
use watchdog::*;
use widgets::*;

// `theme` and `window` are imported explicitly: two of their items must widen to
// `pub(crate)` for `main.rs`, and an explicit import keeps that visible. A name
// that gets a `pub(crate) use` below must NOT also appear in a private `use`
// (two explicit imports of one name is `E0252`).
use theme::{
    apply_theme, cjk_order, connect_instruction, font_definitions, parse_server_addr,
    rtt_colour, semibold,
};
use window::{
    apply_discovery_result, brand_mark_texture, close_decision, monitor_geometry,
    monitor_local_top, should_apply_race, should_race, window_height_limit, CloseDecision,
};
// Items `main.rs` / `platform/*.rs` reach through `crate::app::…`; the plain
// `use` above is not visible outside `app`, so widen these explicitly.
pub(crate) use theme::install_fonts;
pub(crate) use window::{MAX_WINDOW_HEIGHT, MIN_WINDOW_HEIGHT};
```

(Order of the `use` lines is not significant; `cargo fmt` owns the final layout.
If a name is imported both by a glob and explicitly, Rust lets the explicit one
shadow the glob — no `E0252` — so the block above is stable.)

Consequences of this shape:

- Every existing call site (`self.` methods, `ui()`, the tests' `use super::{…}`)
  keeps compiling unchanged; only the moved items gain `pub(super)`.
- Moved items need `pub(super)` (visible in `app` and descendants — the tests are
  a descendant, so `pub(super)` is enough for them too). Items staying in
  `app.rs` need no change at all; children see the parent's privates.
- Glob `use` re-exports don't trigger `unused_imports`, so clippy stays quiet
  while the split is in flight. Convert to explicit imports later only if the
  team wants it; it is optional cleanup, not part of the split.
- If a name the tests reach through `use super::*`/`use super::{…}` does not
  resolve through the root imports, add it to `app/tests.rs` from its real
  module (`use super::watchdog::relay_reply_fault;`). That is the expected
  one-line fix per step; do not "solve" it by keeping items in `app.rs`.
- Field visibility: structs that move need `pub(super)` on the fields that
  `app.rs` still reads. Measured: 66 field declarations exist in the moving
  types; expect **40-60 `pub(super)` annotations**. Known required:
  `RelayRejection` (4), `RetestTarget` (2), `ReplyFallback`/`RelayReplyWatch`
  (as read by `app.rs`), `ServerPing` (3), `RouteRanking` (4),
  `MeasuredServerLeg`, `RelayHealthBase` (2), `GameEditorDraft` (7, read at
  4704-4735), `DraftVerdict`, `PickedGame` (3), `ProcessPicker` (4),
  `HealthProbe` (2), `WinnerProbe` (`app.rs` destructures
  `WinnerProbe { addr, .. }` at 2546), `EditorActions` (4), `TargetRowAction`
  (2). Enum variants (`RelayReplyFault`, `RelayHealthVerdict`, `ServerLegState`,
  `GamePickRow`, `ActionKind`, `LedgerValue`, `CloseDecision`, `PickerStatus`)
  travel with the enum's visibility; no per-variant work.

## 3. Ordered execution (Phase A)

Prerequisites: clean worktree at the measured revision; no concurrent editor of
`app.rs`; the build lock agreed with `infra-dev`. One step per commit, so a
failure reverts one file.

**Baseline before step A0 (record it, do not inherit it):**
`cargo fmt --all --check` clean; `cargo clippy -p lightspeed-gui --all-targets`
exit 0, 0 warnings; `cargo test -p lightspeed-gui` — GUI gate as reported in
`wat/state/current-phase.md` is **186 / 0 / 1** (186 passed, 0 failed, 1
ignored); `app.rs`'s own module is 78 of those. Re-measure and record the real
numbers first; every step must keep them identical.

### A0 — `app/tests.rs` (2,504 lines, 78 tests) — lowest risk, biggest single cut

- Cut `6813-9316` into `client-gui/src/app/tests.rs`; in `app.rs` replace the
  inline module with `#[cfg(test)] mod tests;`.
- Keep `use super::{…}` as-is first (the root still has every item), then let
  rustc point at anything that resolves differently.
- The two `super::install_fonts` / `super::apply_theme` calls inside
  `the_game_editor_sheet_…` (8923-8924) resolve unchanged because A0 does not
  move the theme fns yet; they will resolve through the root re-export block
  added in A6.
- Risk: low. Compile-only. If `cargo test` count changes from 186, the move cut
  a test — restore before proceeding.
- Verify: `cargo test -p lightspeed-gui` (same counts), `cargo clippy
  -p lightspeed-gui --all-targets`, `cargo fmt --check`; `git diff
  --color-moved=zebra` review.

### A1 — `app/watchdog.rs` (723 lines, 14 tests pin it)

- Move the block at 98-133 + 215 + 141-826. This cluster is a background thread
  plus pure judgement fns over counters — no egui, no `&mut self`.
- `pub(super)` the 9 fns, both small impls' methods (`observe`, `read`), the 6
  consts and the 7 types; `pub(super)` the fields of `RelayRejection` and
  `RetestTarget` (read by `relay_rejection_now`/`watchdog_chain` in `app.rs`).
  The `Mutex<Option<…>>` Arcs stay `Arc<Mutex<…>>`; the thread body is unchanged.
- Risk: low-medium. The only real hazard is missing a field annotation so
  `app.rs` cannot read the struct — rustc names each one (`E0616` private
  field); fix and move on. Do **not** "fix" it by making the whole struct
  `pub(crate)` or moving the type back — that defeats the module.
- Verify: same gate; grep the diff to confirm `spawn` body is byte-identical.

### A2 — `app/relay_choice.rs` (199 lines, 3 direct tests + most ranking tests)

- Move 803-1001. Watch the three `RELAY_HEALTH_*` consts (used by
  `poll_relay_health` in `app.rs`) and `RELAY_SWITCH_MARGIN_MS`.
- The `#[cfg_attr(not(test), allow(dead_code))]` attribute on `switch_is_safe`
  (935) must travel with the fn or the allow lands on the wrong item.
- `pub(super)` the enums/structs' read fields (`ServerPing`, `RelayHealthBase`,
  `RelayHealthVerdict`, `RouteRanking`, `MeasuredServerLeg`).
- Risk: low. Verify as A1.

### A3 — `app/games.rs` (101 lines)

- Move 1002-1102. `GameEntry`/`games()` are already `pub` and their fields are
  `pub`; leave visibility alone. `PickedGame` needs `pub(super)` fields (3).
- Registry fns read the profile store from disk; nothing to change.
- Risk: low. Verify as A1.

### A4 — `app/editor.rs` (239 lines, 4 tests)

- Move 1103-1341. `GameEditorDraft`'s 7 fields are read by
  `game_editor_sheet` in `app.rs` (4704-4735) → `pub(super)`; `DraftVerdict`'s
  fields are read by `sheet` code too.
- `save_user_profiles` writes the store — pure file I/O, no state.
- Risk: low. Verify as A1.

### A5 — `app/picker.rs` (91 lines, 4 tests)

- Move only 1383-1473. Leave `UpdateCheckState`/`DiscoveryState`/
  `ProcessListState` (1342-1382) and `HealthProbe`/`WinnerProbe` (1474-1492) in
  `app.rs`: they are the types of `LightSpeedApp` fields and are pattern-matched
  by root methods; keeping them in place avoids five enum-visibility decisions
  for 60 lines of gain.
- Risk: low. Verify as A1.

### A6 — `app/theme.rs` (251 lines, 3 tests)

- Move 5378-5628. `font_definitions` and `install_fonts` are `pub(crate)`; add
  the explicit `pub(crate) use theme::install_fonts;` in the root block.
- `install_fonts` is called from `main.rs` before the first frame and from
  `ui()`/sheet tests; the re-export keeps all three paths working.
- Risk: low. Verify as A1, plus one GUI launch (see §5).

### A7 — `app/choice.rs` (275 lines, ~20 tests)

- Move 5629-5903. These fns take `ServerPing`/`ServerLegState`/
  `MeasuredServerLeg`/`RelayLegs` by argument — import them from
  `super::relay_choice` (or rely on the root re-exports).
- Risk: low. Verify as A1.

### A8 — `app/window.rs` (164 lines, 11 tests)

- Move 5904-6067. `MIN_WINDOW_HEIGHT`/`MAX_WINDOW_HEIGHT` are `pub(crate)` and
  used by `main.rs:97-98` — add the explicit `pub(crate) use window::{…};`
  re-export in the root block, or `main.rs` breaks.
- Risk: low. Verify as A1.

### A9 — `app/ledger.rs` (294 lines, ~12 tests)

- Move 6068-6361. `ActionKind`/`LedgerValue` are matched in `app.rs` — enums
  travel with `pub(super)`; `action_button` is a widget that lives beside the
  ledger rows, fine.
- Risk: low. Verify as A1.

### A10 — `app/widgets.rs` (451 lines, ~6 tests)

- Move 6362-6812. `EditorActions`/`TargetRowAction` fields are read by sheet
  code in `app.rs` → `pub(super)` (6 fields).
- Last because it is the biggest helper block and the most likely to be edited
  concurrently (any UI work touches these widgets).
- Risk: low-medium: this is the block whose tests build real egui frames, so a
  mis-import surfaces as a compile error, not a silent change. Verify as A1 and
  add the GUI launch check (§5).

**End state after A10:** `app.rs` ≈ 4,050 lines; eleven new files; zero
behaviour change; 78 tests still in the crate, still passing; the moved items
`pub(super)` and re-exported once.

### Phase B — moving the 72 `impl` methods (NOT recommended in this round)

If it is ever done, this is the shape (line ranges at HEAD; not executed):

| New file | Moves | Lines |
|---|---|---|
| `app/init.rs` | `new()` 1711-1896 | 187 |
| `app/session.rs` | 1898-2463 (connect, accessors, choose/measure, follow/hold) | 566 |
| `app/polling.rs` | 2464-2876 (race/winner/discovery polls, boost, latency trace, update-check start) | 413 |
| `app/record.rs` | 2877-3044 (history/verdict recording) | 168 |
| `app/health.rs` | 3045-3259 (relay health, presence, elevation) | 215 |
| `app/view/rail.rs` | 3260-3902 | 643 |
| `app/view/config.rs` | 3903-4354 | 452 |
| `app/view/sheets.rs` | 4355-5042 | 688 |
| `app/view/frame.rs` | 5043-5376 (`impl eframe::App`) | 334 |

What makes B different from A: it relocates 3,666 lines that have **no unit
tests**, **53 of the 72 methods are called from outside their own module**
(measured in `inventory.txt`; each needs `pub(super)` and `ui()` must stay able
to call all of them), and every moved method keeps 497 `self.<field>` reads
working — automatic (child modules read the parent's private fields), but it
means the compiler, not the tests, is the only safety net. B's cost is higher
and its benefit smaller than A's (it does not shrink `app.rs` much once A is
done: ~4,050 → ~500, but that 3,500 lines are exactly the untested part).
**Never do B as a big-bang**: one surface per PR, starting with `init.rs` and
the view modules, and only when a feature needs to edit one surface heavily.

### Phase C — regroup the 77 fields into sub-structs (never, unless forced)

This is the only part of the audit's "split by surface" that is *not*
mechanical: 77 fields, **497 `self.<field>` reads** across the 72 methods, a
190-line struct literal, an 80-line `new()` whose initialisation order is
load-bearing (below), and zero unit tests on `LightSpeedApp`. Expected diff is
>1,000 lines with no test able to catch a mis-wired field; the payoff is
narrower borrow scopes, not fewer defects. Do it only if a feature cannot be
expressed without it.

## 4. Blockers and risks (by the audit's own checklist)

| # | Blocker | Evidence | Handling |
|---|---|---|---|
| 1 | **Private fields and impl blocks** | 77 private fields at 1493-1682; 2 `impl LightSpeedApp` blocks | Child modules of `app` can read the parent's private fields/methods, so impl bodies move freely. What needs `pub(super)`: moved items used from `app.rs` (~80 fns/consts/types) and moved struct fields read from `app.rs` (~40-60). Compiler names every miss (`E0616`/`E0603`/`E0432`). |
| 2 | **`#[cfg(test)] mod tests`** | 2,504 lines, 78 tests, `use super::{…}` | Move it first (A0) and keep root re-exports; tests are a descendant of `app`, so `pub(super)` items are visible. All UI tests drive free builders through `run_ui`, so nothing about the harness changes. |
| 3 | **egui lifetimes / immediate-mode ordering** | `ui()` at 5043-5376: `let viewport = ui.available_rect_before_wrap()` before any panel; panel order rail → route card → footer → action bar → central; sheets drawn after the central panel; `ctx.send_viewport_cmd` calls in order | Phase A does not touch `ui()` at all — this is the main reason to keep B out of scope. Any future edit must preserve: rect read first, panel order, sheet dispatch after the scroll area, repaint cadence last. Do not split `ui()` into multiple fns that reorder the `Panel::top/bottom` calls. |
| 4 | **State initialisation order** | `new()` 1711-1896: `set_path_mtu_cap(saved.tunnel_mtu)` **before** any boost; `i18n::set_language` before the first frame; the three watchdog Arcs created and `spawn_relay_reply_watchdog` called **before** the struct literal; `connect_selected()` last; `fonts_setup`/`install_fonts` in the first `ui()` frame | Moving `new()` (Phase B only) must be a byte-identical move. Phase C would break the struct-literal ordering and is out of scope. |
| 5 | **Single `eframe::App` trait impl** | `impl eframe::App` at 5043 | One impl block, one file. Never split the trait impl itself; only sibling `impl LightSpeedApp` blocks can move. |
| 6 | **Single-instance surface** | not in `app.rs`: `main.rs` acquires `single_instance::acquire()` before the eframe run; `--force`/`LIGHTSPEED_GUI_FORCE=1` bypasses it | No blocker. Do **not** try to run a second GUI for smoke testing; close the installed app or use `--force` (see §5). |
| 7 | **Tray surface** | `TrayState` (41-46) is imported by all four `platform/*.rs`; `self.tray`, `P::new_tray` in `new()`, tray poll + state machine in `ui()` (5095-5163), `tray_available()` (1902) | Keep `TrayState` in `app.rs` (it is in the 37-97 block, which stays). If it ever moves, `platform/*.rs` needs a path change in files outside the split's write scope — avoid. |
| 8 | **`#[cfg_attr(not(test), allow(dead_code))]`** | on `switch_is_safe` (935), the one item with a test-only use | Attribute travels with the item; if it is left behind the fn either warns (release) or the allow lands on the next item. Check `cargo clippy -p lightspeed-gui --all-targets` *and* a release-mode check if in doubt. |
| 9 | **`pub(crate)` items used outside `app`** | `main.rs:97-98` (`MIN/MAX_WINDOW_HEIGHT`), `main.rs:124` (`install_fonts`), `platform/*.rs` (`TrayState`) | Explicit `pub(crate) use` re-exports in the root block (§2.2). A plain private `use` would make `crate::app::install_fonts` fail. |
| 10 | **Docs citing line numbers** | `docs/design/custom-games-and-process-targeting.md` (`app.rs:94`, `:651`, `:1230`), `docs/design/ui-audit.md` (`app.rs:1157-1232`), `docs/project-audit-2026-11-18.md` (`app.rs:389-418`, `:879-943`) | Already stale before the split. Do not add new line-number citations; when touching those docs, replace with item names. |
| 11 | **Windows build lock / gate** | `AGENTS.md` gate; `infra-dev` holds the build lock; GUI crate is excluded from workspace clippy | Run `cargo fmt --all --check`, `cargo clippy -p lightspeed-gui --all-targets`, `cargo test -p lightspeed-gui`. Do not run the workspace gate concurrently with `infra-dev`; the GUI is Windows-only here. |
| 12 | **Rebase/conflict cost** | 139 commits touched the file in 90 days | Split on a clean master during a quiet window (after the release); one step per commit keeps a conflict confined to one cluster. No long-lived branches exist today (only `master` + the `stats` data branch), so there is nothing to rebase against. |

## 5. Verification

Per step (must all hold before the next step starts):

```powershell
cargo fmt --all --check
cargo clippy -p lightspeed-gui --all-targets
cargo test -p lightspeed-gui
```

- `cargo test -p lightspeed-gui`: **186 / 0 / 1** before and after every step
  (78 of them live in `app.rs`; the count must not move except for `A0`, where
  it must stay exactly 186 — a change means a test was dropped).
- clippy: exit 0, 0 warnings. `cargo fmt`: clean.
- Diff review: `git diff --color-moved=zebra -- client-gui/src` and
  `git diff --numstat` — additions must equal removals per step plus the
  `mod`/`use`/`pub(super)` lines. No changed logic lines may appear.
- Grep the moved fns out of `app.rs` (`Select-String -Path
  client-gui/src/app.rs -Pattern 'fn <name>'`) and into the new file; nothing
  may exist twice.

GUI check (A6-A10, and any step after a `ui()`-adjacent move): a compile does
not prove the window still draws. The single-instance guard exits with code 2
if the installed LightSpeed GUI is running, so either quit the tray app first
or run with `LIGHTSPEED_GUI_FORCE=1`; `LIGHTSPEED_KEEP_VISIBLE=1` prevents the
close-request from hiding the window. Per this persona's rule, a layout claim
needs a real-window screenshot: capture the window at ~320 px and wide, idle and
boosting, before and after the step, under `wat/evidence/<date>/`, and compare
the shell order (rail, route card, footer, action bar, scroll region). A move
must not change a single pixel; if it does, the move was not mechanical.

Not verified by this plan: the 186 count is the state file's reported GUI gate
from the 2026-11-18 remediation round; this persona did not run cargo (build
lock held by `infra-dev`), so the baseline must be re-measured by whoever
executes step A0.

## 6. Cost / benefit and the decision

| | Phase A | Phase B | Phase C |
|---|---|---|---|
| Lines relocated | 5,292 (56% of file) | 3,666 (100% of the impl layer) | 77 fields / 497 refs |
| Logic changes | none | none | field paths + init order |
| Unit tests on moved code | **78 tests / all of them** | **0** | **0** |
| Safety net | tests + compile | compile only | compile only |
| Effort (one `rust-dev`) | 0.5-1 day incl. gate | 1-2 days | 2-4 days, high regression risk |
| `app.rs` after | ~4,050 lines | ~500 lines | ~500 lines |
| Benefit | navigation, parallel scopes, conflict surface | same, mostly already captured by A | narrower borrows |
| Recommendation | **yes, after the next release** | **only if a feature forces one surface** | **never unless forced** |

What the move does *not* buy, stated plainly so the decision is not oversold:

- It does not make the GUI more testable. The untested part (the 72
  `LightSpeedApp` methods, including `new()` and `ui()`) stays untested; the
  tested part is already tested. Extracting
  logic out of `&mut self` methods into testable free functions (the pattern
  that already produced `close_decision`, `should_race`,
  `apply_discovery_result`, `relay_health_verdict`) is the change that would
  raise confidence, and it is a separate refactor.
- It does not reduce compile times materially (same crate, same code).
- It does not fix any defect. The defects the map found (§1.4) are comment
  drift and doc staleness; fix those in place.

**Decision: after the next release, Phase A.** Concretely: when the 1.11.x
release that proves the MTU fix and the hardened publish path is out, run A0-A5
in one PR (A0 tests + A1-A5 free sub-systems, ~3,857 lines moved) and A6-A10 in a
second PR (~1,435 lines moved), one step per commit, gated as in §5.

**Falsifier — split before the next release if either is true:**

1. The next two features each change **>200 lines** in `app.rs` (calibrated:
   of the 139 `app.rs` commits in the last 90 days, 16 changed >200 lines and 6
   >700; the median is 46). If both of the next two do, the file is costing more
   than one release's worth of rebase noise.
2. One feature touches **≥4 of the Phase-A clusters** (watchdog, relay choice,
   games, editor, picker, theme, choice, window, ledger, widgets). A feature
   that wide in a monolithic file is exactly the diff a split makes reviewable.

Secondary trigger for considering Phase B: a feature that needs to add a field
to `LightSpeedApp` and consume it in ≥3 of the drawing clusters — that is the
signal that the struct has outgrown one file, and it is a Phase C decision, not
Phase A.

## 7. Evidence

- `wat/evidence/2026-11-18/team/interface-designer/inventory.ps1` — the
  reproducible inventory script (PowerShell 5.1, read-only).
- `wat/evidence/2026-11-18/team/interface-designer/inventory.txt` — its output:
  file/cluster/field/function/test counts, `self.<field>` refs per cluster, the
  77-field inventory, per-test pins, cross-cluster call map.
- This document.
- Baseline gate numbers are inherited from `wat/state/current-phase.md`
  (2026-11-18 remediation entry: GUI 186/0/1, GUI clippy clean) and were **not**
  re-run by this persona — cargo is owned by `infra-dev` this round. Anything a
  `rust-dev` inherits from this plan must be re-measured at step A0.
