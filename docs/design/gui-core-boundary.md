# GUI core boundary — one logic, GUI as a wrapper

| | |
|---|---|
| Programme | WF-081 "Desktop 2026", W1 architecture task |
| Author | `architect` (task-1; revision r4 = task-10) |
| Date | 2026-11-19 |
| Revision | **r8 (2026-11-19)** — the r4 DTO reconciliation stands; r8 only removes the transient "spec index gap" note now that the spec records A1–A6 in Appendix C (C-13…C-15) and Q3 (see §5.0). Maps: `revision-r4.md`, `revision-r8.md` |
| Revision measured | worktree `dsh-b74d064d-6fe5` at `62c6cf6`, clean tree except the W0/W1 docs |
| Status | **Decision, for Lead integration and owner acceptance of the W1 set.** No code changes. |
| Brief | [gui-refresh-brief.md](gui-refresh-brief.md) — this document executes §7.1's delegated decision, verifies §7.3's duplicate list, and supplies §8's W1 `[architect]` deliverable. It does not restate the brief. |
| Contract it serves | [interface.md](interface.md) (11 route states + server-leg states, one driving fact each) as amended by the spec's Appendix C; the amendments are mirrored in §5.0 and do not edit `interface.md` here (write scope) |
| Specification it serves | [gui-refresh-spec.md](gui-refresh-spec.md) — the W2 slice cut (its §5) depends on this document's §3.2.1 verdict map; the DTO names it cites (`AppStatus.*`, `RelayStatus.connecting`, `WatchdogStatus`, `Capabilities.elevated`, `AppError`) are kept verbatim in §5 |
| Evidence | `wat/evidence/2026-11-19/team/architect/` — `report.md`, `revision-r4.md`, `line-counts.txt`, `structure-counts.txt`, `cargo-metadata-no-deps.json` |

**Line-count convention.** Every count in this document is `total lines (non-blank lines)`, measured by `Get-Content … .Count` and `Get-Content … | Measure-Object -Line` at the revision above. The brief mixes the two conventions from row to row (its §2 table quotes `app.rs` non-blank while §7.3 quotes `discovery.rs` total); the paired form here removes that ambiguity. The reproducible command is in the evidence report.

---

## 1. Decision summary

**D1 — Boundary: an `app` facade module inside `lightspeed-client`, not a new crate.**
`client-gui` and the `lightspeed` CLI consume one shared API at `lightspeed_client::app::{config, commands, status, relay, game, error, capabilities, update, history, watchdog, presence}`. The engine, capture, tunnel and wire modules stay where they are; the facade is the only new public surface, and every DTO in it is either a re-export of an existing `lightspeed-client` type or a wrapper over fields that exist today.

**D2 — What moves out of `client-gui/`: every fact that answers "what is true about the session, route, game or config".** Measured target is §3: 17 logic-bearing modules/components totalling **5,413 total (5,023 non-blank) lines**, plus ~1,367 orchestration/accessor lines inside `app.rs`; the GUI keeps a thin host wrapper (thread/channel plumbing) for some of them. Drawing, tokens, i18n, platform integration and view state stay (§3.3).

**D3 — One config model, two files staged apart.** One `AppConfig` type wraps the existing `client::config::Config` (7 sections / 31 leaf fields, `client/src/config.rs:10-38`) plus three namespaced preference groups. The GUI's duplicate schema fields collapse onto real client fields (`share_latency_stats` → `general.telemetry`; `tunnel_mtu` → `tunnel.mtu`). File-location unification is a later, migration-guarded stage (§6 S5), not part of the type unification.

**D4 — Drift guard: a ratcheting Rust test in the existing workspace gate.** `client/tests/gui_boundary.rs` scans `client-gui/src/**` for (a) engine API paths outside an allowance table, (b) identifiers that were moved to the core, (c) per-file line budgets, and (d) `egui`/`eframe`/`cfg(windows)` in the core. It runs in `cargo test --workspace --exclude lightspeed-gui`, which CI already runs (`.github/workflows/ci.yml:42`), so no workflow change is needed; the allowance table only ever shrinks (§7).

**Why not the new crate (short version, full argument §4):** a new `lightspeed-app-core` crate that depends on `lightspeed-client` **cannot be consumed by the `lightspeed` CLI**, because that binary is a target of the `lightspeed-client` package itself (`client/Cargo.toml:34-40`; confirmed by `cargo metadata`: one package, targets `lightspeed_client:lib` + `lightspeed:bin`). Consuming it would need a package split that renames the release artifacts (`lightspeed-client-*.zip|tar.xz` at `.github/workflows/release.yml:421-427`, plus `dist/scoop`, `dist/chocolatey`, `dist/aur`), i.e. a release/packaging migration — outside W2's stated scope and behind owner gates. The brief's own fallback (§7.1, "if the boundary cannot be cleanly tested without moving capture features, it stays inside `lightspeed-client`") is the only structurally available option, though for a different and stronger reason than the one the brief anticipated (§4.1).

**Falsifier for D1** (what would prove this decision wrong, §4.5): if the facade cannot express a migrated rule without (i) a GUI/platform type in its signature, (ii) a Windows-only `cfg` that breaks the Linux/macOS parity obligation, or (iii) leaving a migrated fact reachable only through raw `lightspeed_client::engine/games/profiles/registry` calls in `client-gui/`, then the boundary is wrong — stop and re-open the crate decision together with the packaging split, do not bend the facade.

---

## 2. Problem statement, in measured terms

The owner's requirement is "logic is in one place" so the CLI and the GUI do not drift. Today the GUI is not a wrapper around the client; it is a **third logic tree** beside the client library and the client binary:

1. The client binary declares its own private copy of the whole module tree — 22 modules at `client/src/main.rs:13-34` versus the library's list at `client/src/lib.rs:52-71` — so "the CLI" and "the library" are the same source compiled twice, and reach `lightspeed_client::` exactly once (`client/src/main.rs:760`).
2. `client-gui/` re-owns 13 logic modules. Several already wrap an existing client API and add a second implementation on top of it (§3.2 findings F-02, F-03, F-07, F-09).
3. The shared crate cannot be reached for the most important one: `config` is `pub(crate)` (`client/src/lib.rs:55`), so the GUI could not use the client config model even if it wanted to, and wrote its own (§3.2 F-01).

Consequences already visible in the tree, all reachable from current code:

- The same "which relay?" question is answered from two different measurements: CLI UDP-keepalive median of 3 (`client/src/modes/proxy_probe.rs:25-75`), GUI TCP-connect median/spread of 5 (`client-gui/src/discovery.rs:240-270,348-354`), both fed into the same shared ranking (`client/src/route/relay_choice.rs:174,243`).
- The same "is a relay switch worth it?" question has two different rules: CLI `(current/5).max(10_000)` µs (`client/src/modes/reroute.rs:21-29`) versus GUI absolute 20 ms (`client-gui/src/app/relay_choice.rs:170`, `client-gui/src/race_watch.rs:15-17`).
- The same "which game was detected?" answer is produced by an engine API returning a display name, then remapped by fuzzy string matching in the GUI (`client-gui/src/app/choice.rs:284-300`); by inspection that maps `Minecraft Java Edition` to the Bedrock key `minecraft` (F-07).
- The engine's own field docs say the no-reply judgement lives in the GUI: `InterceptorHealth` is "Liveness counters the GUI's no-reply watchdog reads" (`client/src/engine.rs:26`, repeated at `:1018`). The contract in `docs/design/interface.md:36` names `relay_reply_fault` + `ReplyFaultStreak` as the only derivation of four of the eleven route states — and both live at `client-gui/src/app/watchdog.rs`.

None of this is a rendering problem, and none of it is fixed by replacing egui. It is fixed by moving the facts to one place and making the GUI consume them — which is exactly the brief's O1.

## 3. Inventory (measured on the current tree)

### 3.1 The three surfaces

| Surface | What it is | Metadata | Why it matters to the boundary |
|---|---|---|---|
| `lightspeed-client` library | engine, capture, tunnel, routing, profiles, registry; **thin** public API | `client/Cargo.toml:38-40`; public modules `client/src/lib.rs:9-11,13-14,20-41,57,60-63,66,69` | This is the only crate both front ends can share. Its public surface is deliberately narrow; most modules are `pub(crate)` (`client/src/lib.rs:52,53,55,56,58,59,64,65,67,68,70,71`). |
| `lightspeed` CLI binary | same package, own module tree | `client/Cargo.toml:34-36`; `client/src/main.rs:13-34`; 1,562 non-blank lines | Already shares *source* with the library. It is not a second implementation — it is the same code compiled as a binary target. So the drift to fix is GUI↔client, not CLI↔client. |
| `lightspeed-gui` | bin-only crate, 33 files, 15,454 total (14,423 non-blank) lines | `client-gui/Cargo.toml:10-12`; measured in evidence `line-counts.txt` | The logic tree to shrink. It depends on `lightspeed-client` already (`client-gui/Cargo.toml:15`), so adopting a facade needs no Cargo change. |

Workspace members at this revision: `protocol`, `client`, `proxy`, `client-gui` (`Cargo.toml:3-8`, confirmed by `cargo metadata --no-deps`: evidence `cargo-metadata-no-deps.json`).

### 3.2 Logic-bearing modules in `client-gui/` — ownership verdict

`egui` coupling = count of `egui::`/`eframe::`/`Ui`/`Painter` mentions in the file; a logic module with zero coupling is pure business code sitting in the GUI crate. Tests are declared `#[test]` attributes in the file (the `app/` split moved many into `app/tests.rs`, 2,491 lines / 78 tests, so module test counts there are 0 while the behaviour is pinned in `app/tests.rs`). **Call-site paths in the penultimate column are relative to `client-gui/src/`** (`app.rs` = `client-gui/src/app.rs`); every other path in this document is repo-root relative.

| # | Module | Lines (total/nb) | Tests | egui | Business behaviour it owns today | Call sites (rel. `client-gui/src/`) | Verdict |
|---|---|---|---|---|---|---|---|
| F-01 | `config.rs` | 524 / 487 | 15 | 5 | second `config.toml` schema: 12 fields (`:54-90`), tolerant load/parse/render/save/reset (`:112-320`), discovered-proxy merge (`:316-320`), MTU clamp (`:176-180`) | `app.rs:19,422,752,3654`; `main.rs:122`; `discovery.rs:16`; `app/relay_choice.rs:190`; `app/tests.rs:650` | **MOVE** model+IO; GUI keeps its file path |
| F-02 | `discovery.rs` | 816 / 752 | 21 | 4 | registry fetch wrapper (`:69-133`), friendly label catalogue (`:137-157`), `/health` HTTP+parse (`:175-178,369-418`), 5-probe client-leg measurement (`:232-295`), race (`:299-364`) | `app.rs:400,580,696,769,775,824,929,934,1386,1394,2002,2400` | **MOVE** blocking/pure parts; keep spawn wrappers (§3.3) |
| F-05 | `app/watchdog.rs` | 712 / 688 | 0 (14 in `tests.rs`) | 5 | reply-fault thresholds (`:3-39`), window judgement (`:229-299`), streak (`:155-188`), retest chain/backoff (`:117-136`), verdict trace lines (`:301-450`), background owner thread (`:464+`) | `app.rs:47,59,495,961,967,1275,1277,1581-1638,1749-1825` | **MOVE** decision logic+thread |
| F-06 | `relay_history.rs` | 528 / 487 | 15 | 0 | per-(relay,server) measurement memory, carries/starved predicates (`:77-102`), TTL/trim (`:49-59,203-219`), JSON persistence (`:220-277`) | `app.rs:259,528-529,842-868,1617-1631,1639-1705,1761`; `app/watchdog.rs:29-30` | **MOVE** |
| F-04/05 | `app/relay_choice.rs` | 198 / 182 | 0 | 1 | relay-health verdict constants + judge (`:3-16,143-166`), switch safety (`:138-141`), 20 ms switch margin (`:168-170`), env proxy parsing (`:174-198`), `RouteRanking`/`ServerLegState`/`ServerPing` types (`:20-90`) | `app.rs:57` glob; used in `poll_relay_health`, `maybe_rerace`, route surface | **MOVE** rules+DTO types |
| — | `app/choice.rs` | 300 / 284 | 0 | 0 | pure derived state for the route contract: leg pairing (`:22-56`), ping freshness/server keying (`:58-108`), direct comparison (`:110-136`), ranking labels (`:138-160`), server-leg state precedence (`:183-230`), triangle floor (`:243-251`), cannot-beat-direct (`:266-282`), game auto-detect remap (`:284-300`) | `app.rs:51` glob; route surface + choice | **MOVE** (all but the label strings) |
| F-08 | `app/ledger.rs` (mixed) | 290 / 273 | 0 (12 in `tests.rs`) | 47 | state math: `primary_action` (`:39-70`), `routed_count` (`:155-174`), `history_delta` (`:176-180`), `returned_count` (`:182-196`), `mode_counter` (`:198-213`), `is_holding` (`:215-224`), `game_caption` (`:226-246`), session deltas (`:247-268`) **and** widget helpers (`:268-290`) | `app.rs:54` glob; every ledger row + `ui()` | **SPLIT**: rules MOVE, widgets STAY |
| F-10 | `app/presence.rs` | 482 / 444 | 7 | 11 | 5 s presence schedule (`:26-96`), Auto-only/process-target gate (`:32-45`), worker thread (`:100-234`) | `app.rs:42,56,283,534,1843-1945`, shutdown `1403` | **SPLIT**: policy+schedule MOVE; worker stays (or moves in S5) |
| F-04 | `race_watch.rs` | 51 / 45 | 4 | 2 | relay-switch hysteresis `decide_switch` (`:15-17`) | `app.rs:28,1299` | **MOVE** |
| F-07 | `app/editor.rs` | 243 / 228 | 0 (4 in `tests.rs`) | 0 | editor draft ↔ `profiles.toml` mapping (`:22-81`), per-field save-rule mirror (`:143-169`), built-in collision probes (`:89-111`), localized reasons (`:171-202`) | `app.rs:52` glob; editor sheet at `3357` | **MOVE** rules (violation enum, not i18n strings) |
| — | `app/games.rs` | 105 / 94 | 0 (3 in `tests.rs`) | 2 | game menu construction from `GAME_REGISTRY`, `PickedGame`, profile-store accessors (`:15-69`), pick rows (`:71-105`) | `app.rs:53` glob; combos | **SPLIT**: registry accessor MOVE, row construction STAY |
| — | `app/picker.rs` | 98 / 91 | 0 (4 in `tests.rs`) | 1 | picker status derivation, target summary (`:1-91`) | `app.rs:55` glob | **STAY** (presentation state derivation) |
| F-11 | `geo.rs` + `geo_countries.rs` | 108/94 + 243/242 | 5 | 0 | offline IPv4→(country,lat,lon) lookup (`geo.rs:33-60`) over a generated 237-country table (`geo_countries.rs:1-6`) and a 1.8 MB blob (`geo.rs:16`) | `app.rs:22,800,2131` | **MOVE** behind a feature (§6 S5) |
| F-12 | `update.rs` + `self_update.rs` | 322/285 + 213/191 | 10 + 3 | 5 + 13 | semver compare (`update.rs:46-61`), receipt/GitHub fallback (`:92-120`), checksum parsing (`self_update.rs:111-119`), elevated install (`:27-89`) | `app.rs:29,373,1412-1424,3667-3749` | **SPLIT**: pure rules MOVE; network/elevation STAY |
| F-09 | `processes.rs` | 282 / 260 | 6 | 6 | worker wrapper around client enumeration (`:36-51`) + filter/path-tail presentation (`:53-70`) | `app.rs:27,705,714` | **STAY** (the correct adapter shape, already delegating to `client/src/processes.rs:48`) |
| — | `paths.rs` | 81 / 69 | 3 | 9 | second config/data dir resolver (`:12-43`) | `main.rs:122`, `app.rs:422,529,752,1631,1693,1947,3625,3654` | **MOVE** config dir resolution; keep log/crash/open helpers |
| — | `globe.rs` | 197 / 178 | 4 | 27 | relay-node coordinate table (`:20-36`) | `app.rs:2106,2119` | **DELETE** table, use core `relay_position` (§4.2) |

Support modules that stay whole, for completeness: `i18n.rs` 506/469 (presentation), `design.rs` 264/237 (tokens), `crash.rs` 210/187, `single_instance.rs` 331/301, `platform/*` 138/121 + 315/286 + 99/85 + 90/77 (platform integration). `app.rs` itself is inventoried in §3.4.

Two measured corrections to the brief's §7.3: the table is accurate in kind but not in count — `discovery.rs` is 816 total/752 non-blank, `app.rs` 4,087 total/3,888 non-blank (the brief's 3,888 is the non-blank count), and `watchdog.rs` is a separate 712-line module plus 14 tests living in `app/tests.rs`, not only "state machines".

### 3.2.1 Verdict → executing stage → W2 slice (the cut key for the spec's §5 scopes)

The spec's §5 slices are cut as W2-1 = S0+S1+S2+S3a (+ the two DTO gaps), W2-2 = the shell, W2-3 = S3b+S4+S5+S6, W2-4 = parity. This table is the mapping those scopes must be re-cut from (QA review B1/R1): every module the slices touch, its verdict (§3.2), the stage that executes it, and the slice that owns the work. "STAY" = GUI-owned per §3.3; any edit is the shell/parity slice's. **Paths in this table are relative to `client-gui/src/` unless they start with `client/`.**

| Module | Verdict | Stage | W2 slice | Scope note |
|---|---|---|---|---|
| `config.rs` | MOVE model + IO | S1 | W2-1 | GUI keeps only its file path; call sites in `app.rs:422,752,3654`, `main.rs:122`, `app/relay_choice.rs:190` change here |
| `discovery.rs` | MOVE blocking/pure; spawn wrappers STAY | S2 | W2-1 | the one-shot `mpsc` wrappers stay in `client-gui` (§3.3) |
| `app/choice.rs` | MOVE all but label strings | S3a | W2-1 | the route-surface derivations become `AppStatus` reads |
| `app/relay_choice.rs` | MOVE (types in S3a; health verdict + 20 ms margin in S4) | S3a + S4 | W2-1 (types/call sites) + W2-3 (rules) | no GUI rule left after S4; `env_proxies` call site changes in S1 |
| `app/ledger.rs` — state rules | SPLIT: MOVE (`primary_action`, counter/session deltas) | S3a | W2-1 | `is_holding` moves but is **demoted** by A1: the Holding contract fact is the engine gate, `is_holding` is only the fallback-hold detail (§5.0) |
| `app/ledger.rs` — widgets | STAY | — | W2-2 | rewritten by the shell slice |
| `app/games.rs` | SPLIT: registry accessor MOVE | S5 | W2-3 | row construction STAY (W2-2 may edit) |
| `app/editor.rs` | MOVE rules | S5 | W2-3 | violation enum; i18n mapping stays GUI |
| `app/picker.rs` | STAY | — | W2-2 | presentation state derivation |
| `app/presence.rs` | SPLIT: policy + schedule MOVE | S5 | W2-3 | worker thread STAY (option to move in S5 only if the CLI adopts it, Q4) |
| `app/watchdog.rs` | MOVE | S4 | W2-3 | thread moves; GUI keeps channel wiring and trace pass-through |
| `relay_history.rs` | MOVE | S4 | W2-3 | file format unchanged, no data migration |
| `race_watch.rs` | MOVE | S4 | W2-3 | no GUI hysteresis rule left |
| `geo.rs` + `geo_countries.rs` | MOVE behind feature `geo-ip` | S5 | W2-3 | asset + generator path move with it (Q2) |
| `update.rs` + `self_update.rs` | SPLIT: pure rules MOVE | **S5** (QA R10: now scheduled, §6) | W2-3 | `compare_versions`/`ReleaseAsset`/`parse_sha256_file` move; network/elevation STAY |
| `paths.rs` | SPLIT: config-dir resolution MOVE | S5 | W2-3 | `client-gui/src/paths.rs` must be in W2-3's scope; log/crash/open helpers STAY |
| `globe.rs` | SPLIT: coordinate table DELETE (use core `relay_position`), drawing STAY | S2 exports `relay_position` | W2-1 export + W2-2 edit | QA R1 puts `globe.rs` in W2-2's scope for the literal/token pass |
| `processes.rs` | STAY | — | — | the sanctioned adapter shape (§3.3) |
| `app/tests.rs` | tests travel with their modules | S1–S5 | W2-1 / W2-2 / W2-3 | required in each slice's write scope (QA R1) |
| `main.rs` | call sites only | S1 | W2-1 | `client-gui/src/main.rs:122` config load |
| `app.rs` | getters/orchestration moved over S1–S4; `ui()` untouched | S1–S4 | W2-1 getters, W2-2 `ui()`, W2-3 orchestration | never two writers (R7 / spec C-10) |
| `app/route_surface.rs` (**new**, presentation-only, spec §2.1) | STAY (new file) | created in W2-1 | W2-1 | one pure adapter `AppStatus`+`WatchdogFacts` → word/sub-line/caption/ledger model; add it to G3's budget table at S0 (it did not exist when the budgets were measured) |
| `design.rs`, `i18n.rs`, `crash.rs`, `single_instance.rs`, `platform/*`, `app/{theme,widgets,window}.rs` | STAY | — | W2-2 / W2-4 | the adapter allowlist (§3.3) |

**Delta against the spec's current §5 write scopes** (the actionable re-cut, QA B1/R1): the spec's W2-1 scope lists `client-gui/src/{config,discovery}.rs` + `app.rs` getters — this map requires W2-1 to **also** own `client-gui/src/main.rs:122`, `app/relay_choice.rs` (types/call sites), `app/choice.rs`, `app/ledger.rs` state rules, `app/tests.rs` (moved tests) and the new `app/route_surface.rs`; W2-2 must **add** `globe.rs` and `app/tests.rs` (plus its existing shell scope); W2-3 must **add** `app/{editor,games}.rs`, `client-gui/src/paths.rs`, and the now-scheduled update-rule extraction (`update.rs`/`self_update.rs`) to its write scope. Everything else the spec lists is consistent with this table.

### 3.3 What may stay in `client-gui/` (the adapter allowlist)

Per the brief §7.2 and the skill's §5, the GUI keeps: rendering and the whole `app.rs` draw half; tokens (`design.rs`); i18n lookup (`i18n.rs`); platform integration (`platform/*`, `single_instance.rs`, `crash.rs`, `paths::{log_file,crash_log,open_in_os}`); view state (sheets, scroll, pickers, selection indices); presentation-only formatting (labels, sentences, tabular formatting, the `games()` display table); and **thread/channel plumbing** — the `spawn_*` one-shot `mpsc` wrappers that move a blocking core call off the egui thread. That last item is the sanctioned pattern already used by `client-gui/src/processes.rs:36-51`; it is an adapter shape, not logic, because the work it launches is a core function.

### 3.4 `app.rs` line map (what the extraction actually reaches)

4,087 total lines at this revision: 1–407 header/consts/`TrayState`/state enums/`HealthProbe`/`WinnerProbe`/the 223-line struct; `impl` begins at 408, `new()` at 409; accessors + orchestration run 603–1,969 (**~1,367 lines**); drawing runs 1,971–4,087 (**~2,117 lines**). 71 methods total: **34 orchestration/accessor methods** (e.g. `connect_selected` `:640`, `start_relay_race` `:765`, `choose_relay_for_destination` `:788`, `follow_game_server` `:1020`, `start_boost` `:1432`, watchdog verdict recording `:1581`, `record_relay_history` `:1639`, `poll_relay_health` `:1749`, `poll_game_presence` `:1843`, `shutdown_engine` `:1400`), **4 accessors**, and **33 drawing/derived-value methods** (the pure derivations among them — `server_leg_value` `:2259`, `cannot_beat_direct` `:2328`, `current_client_leg_us` `:2340`, `run_boosting` `:2352`, `session_sent` `:2476`, `ranked_client_leg_us` `:2614`, `leg_ms` `:2626` — become reads of `AppStatus`).

### 3.5 `lightspeed-client` — what the GUI can already use, and the four gaps

Already public and sufficient (re-export list from `client/src/lib.rs`):

| Need | Existing client surface | Evidence |
|---|---|---|
| Engine lifecycle + status | `LightSpeedEngine::{connect, disconnect, snapshot, status_watch, interceptor_health, hold_tunnelling, release_tunnelling, start_interceptor_with_target, stop_interceptor, start_windivert_for_game, stop_windivert, start_capture, stop_capture, start_redirect, stop_redirect, set_path_mtu_cap, path_mtu_cap, set_node_id, set_telemetry_enabled, set_telemetry_game, set_control_port, control_port}`; `EngineStatus` (67 fields), `InterceptorHealth` (10 fields) | `client/src/engine.rs:27-56,60-226,299-1491`; `client/src/lib.rs:13` |
| Destination-aware ranking | `ScoredRelay`, `RelayCandidate`, `DestLegSource`; `rank`, `rank_at`, `rank_with_direct`, `rank_at_with_direct`, `best`, `best_at`, `PRIOR_UNCERTAINTY_US` | `client/src/route/relay_choice.rs:69-130,174-265`; `client/src/lib.rs:20-33` |
| Server-leg estimator | `destination_estimator()`, `ensure_destination_estimator()`, `DestinationEstimator::estimate/measurement_age`, `PathProbe` | `client/src/route/destination.rs:64-114,425-459`; `client/src/lib.rs:38-41` |
| Registry discovery | `discover_relays` → `RelayInfo { node_id, addr, region }`, `discover_nodes`, `discover_data_addrs`, `DEFAULT_*` | `client/src/registry.rs:20-23,78-89,155-230` |
| Games / profiles | `all_game_keys`, `all_games`, `resolve`, `auto_detect`, `process_names_for_name`, `GAME_REGISTRY`; `ProfileStore::global/save/games/servers`, `validate_for_save`, `parse_port_range`, `is_valid_key`, `slugify`, `PortRange` | `client/src/games/mod.rs:120-205,331-352,396-445,480-486,517-556`; `client/src/profiles/mod.rs:18-33,86-255`; `client/src/profiles/schema.rs:79-115,137-148,397-437` |
| Process enumeration | `list_running_processes`, `ProcessEntry` | `client/src/processes.rs:32,48`; `client/src/lib.rs:62` |
| Direct ping / latency counters | `latency::{direct_sample, probe_direct, direct_jitter, shadow_counters, probe_counters, relayed_*}` (67 tests) | `client/src/latency.rs:104-118+`; `client/src/lib.rs:57` |
| Paths | `paths::{config_dir, profiles_file, Platform}` | `client/src/paths.rs:22-86`; `client/src/lib.rs:60` |
| Operational facts | `telemetry::{is_enabled,set_enabled,install,print_notice}`, `current_relay()` | `client/src/telemetry.rs:63-121`; `client/src/lib.rs:27-50,69` |

Four gaps block a clean facade, and all four are `pub(crate)` visibility rather than missing code:

1. **`config` is private** — `client/src/lib.rs:55`. This is the root cause of the GUI's second schema (F-01). Fix: re-export the type through `app`.
2. **`error` is private** — `client/src/lib.rs:56`. There is a complete taxonomy (`LightSpeedError` with `Tunnel/Capture/Route/Quic/Ml/Game/Io` categories, `client/src/error.rs:9-33`) that neither front end can share today; the GUI invents ad-hoc `String` errors (e.g. `client-gui/src/config.rs:297`, `client-gui/src/self_update.rs:23`).
3. **`session` is private** — `client/src/lib.rs:68` — but the one fact the GUI needs is already exposed as `current_relay()` (`client/src/lib.rs:48-50`), so this is not a migration blocker.
4. **No status/command facade at all.** There is no `Status` type in the tree (grep for `struct Status`/`enum Status` in `client/src` returns nothing). The task's "Status" is the CLI command `--status`, which is 134 lines of ad-hoc printing at `client/src/main.rs:731-864`. The status facts exist as the 67-field `EngineStatus` plus the 10-field `InterceptorHealth`; the missing piece is the composition and the derived route state.

The re-export precedent already exists: `lib.rs:20-33` publicly re-exports items from the `pub(crate) mod route`, including renaming `PRIOR_UNDERTAINTY_US` to `PRIOR_UNCERTAINTY_US`. The facade follows the same pattern and needs no visibility change to any existing module except adding `pub mod app;` to `client/src/lib.rs`.

### 3.6 Duplicate-logic findings (each is one source of drift)

| ID | Drift | Side A | Side B | Effect today |
|---|---|---|---|---|
| F-01 | Two config schemas + two files | `client/src/config.rs:10-38` (7 sections, 31 leaf fields, `Config::load/save` `:425-464`) at `lightspeed.toml` (`client/src/cli.rs:11-13`) | `client-gui/src/config.rs:54-90` (12 fields) at `%APPDATA%\light-speed\config.toml` (`client-gui/src/paths.rs:36-43`); cannot reuse A (`client/src/lib.rs:55`) | A CLI-tuned `lightspeed.toml` never reaches the GUI; GUI rewrites its own file on change; two MTU clamps (`client-gui/src/config.rs:176-180` vs `client/src/config.rs:282-287`) |
| F-02 | Two relay-probe implementations, one ranking | UDP keepalive, median of 3 (`client/src/modes/proxy_probe.rs:25-75,127`); fed to `DestinationAwareSelector` (`client/src/route/selector.rs:83-149`) | TCP connect, 5 probes, median+spread (`client-gui/src/discovery.rs:232-295,348-354`); fed to `rank_relays_at_with_direct` (`client-gui/src/app.rs:878-883`) | The ranking is shared (`relay_choice.rs:174,243`) but its inputs have different semantics — a "ping" means two different things across surfaces |
| F-03 | Relay identity fragmented | Registry carries `region` (`client/src/registry.rs:86-88`) and `discover_relays` is documented "for embedders (e.g. the GUI)" (`:196-209`) | GUI defines `RelayInfo{node_id,addr}` without region (`client-gui/src/discovery.rs:34-37`), fetches via `discover_nodes` (`:12-14`), substitutes the display label into `RelayCandidate.region` (`client-gui/src/app.rs:822-830`) — a field documented as a region key (`client/src/route/relay_choice.rs:72-73`) that `regions::resolve_region` cannot parse (`client/src/route/regions.rs:155-171`); separate city table at `client-gui/src/discovery.rs:137-157` and globe table at `client-gui/src/globe.rs:20-36` duplicating `RELAY_POSITIONS` (`client/src/route/relay_choice.rs:478-504`, not re-exported) | Masked for the current 8 relays by the `node_id` path (`relay_choice.rs:366-370,417-426`); a new registry relay that is not in `RELAY_POSITIONS` loses its region prior and scores `Unknown`; the label→region type error is a live seam, not a display detail |
| F-04 | Two switch-hysteresis rules | `(current/5).max(10_000)` µs, 30 s cadence, destination-aware bypass (`client/src/modes/reroute.rs:16-52`) | absolute `RELAY_SWITCH_MARGIN_MS = 20` (`client-gui/src/app/relay_choice.rs:168-170`) applied by `decide_switch` (`client-gui/src/race_watch.rs:15-17`) | CLI and GUI can disagree about when a relay is "clearly better" |
| F-05 | Reply-fault judgement in the GUI only | — | `client-gui/src/app/watchdog.rs` (all of the fault/streak/retest logic) | The engine designs its counters around this consumer (`client/src/engine.rs:26,1018`) but the CLI has no equivalent: `grep watchdog|relay_history` in `client/src` returns only comments. A CLI session has no fallback chain, no re-test, no rejection memory |
| F-06 | Measurement memory GUI-only | — | `client-gui/src/relay_history.rs` persisted to `relay-history.json` (`client-gui/src/paths.rs:26-28`) | Two front ends learn nothing from each other; memory is per-GUI-install |
| F-07 | Game-detection contract gap | `games::auto_detect() -> Box<dyn GameConfig>` returning display name (`client/src/games/mod.rs:513-517,120-122`); canonical keys at `:396-425` | GUI remaps display name → key by substring search (`client-gui/src/app/choice.rs:284-300`) | **Latent defect (by inspection, not executed):** detected `Minecraft Java Edition` matches the earlier `minecraft` key because `"minecraft java edition".contains("minecraft")` → the GUI applies the Bedrock profile to Java (different transport/ports). The CLI keeps the trait object (`client/src/main.rs:1147-1167`) and is unaffected |
| F-08 | No shared status; GUI derives the contract | CLI `--status` ad-hoc 134 lines (`client/src/main.rs:731-864`); no `Status` type | GUI derives route state from raw counters: `poll_relay_health` (`client-gui/src/app.rs:1749-1825`), `direct_fallback` field (`:241`), `is_holding` (`client-gui/src/app/ledger.rs:215-224`), server-leg precedence (`client-gui/src/app/choice.rs:183-230`), `primary_action` (`client-gui/src/app/ledger.rs:39-70`) | interface.md's "exactly one driving fact" is enforced by convention inside one file tree only; nothing prevents a second derivation, and the CLI cannot show the same facts |
| F-09 | CLI binary duplicates the module tree | 22 modules at `client/src/main.rs:13-34` | Library list at `client/src/lib.rs:52-71`; the bin reaches the lib once (`client/src/main.rs:760`) | Structural reason a new crate cannot serve both (D1) |
| F-10 | Two presence policies | GUI: 5 s fixed cadence, Auto-only gate, never auto-starts from a detection (`client-gui/src/app/presence.rs:26-45,202-234`) | CLI `--watch`: 2 s poll when idle, 3 s while intercepting, **auto-starts** the interceptor (`client/src/modes/watch_mode.rs:133-158,60-131`) | Same feature, different behaviour; parity work will have to decide which is canon |
| F-11 | Geo/position split | GUI owns the IPv4→country blob + lookup (`client-gui/src/geo.rs:16,33-60`; `client-gui/src/geo_countries.rs:1-6`; generator writes into the GUI tree, `infra/scripts/build-geo-ipv4.py:38`) | Engine owns relay coordinates (`client/src/route/relay_choice.rs:478-504`) and the region prior (`client/src/route/regions.rs:36-207`) | The GUI's geo lookup is **an input to the ranking** (`client-gui/src/app.rs:799-800`), not decoration; the CLI's selector never passes a server position (`client/src/route/selector.rs:98`), so the two surfaces rank the same candidates with different information |
| F-12 | Update check app-level with GUI-only deps | — | `update.rs`/`self_update.rs`; deps `axoupdater`, `reqwest`, `sha2` are GUI-only (`client-gui/Cargo.toml:20,38,40`) | Fine for network/elevation; the pure version/checksum rules should not need those deps |
| F-13 | Second config-dir implementation + doc drift | `client/src/paths.rs:48-86` (injected-env, tested) | `client-gui/src/paths.rs:12-43` (`dirs` crate) | `client/src/paths.rs:3-6` claims "the GUI's config path both resolve here", which is false at this revision |

## 4. Boundary decision

### 4.1 Option A — new `lightspeed-app-core` crate (depends on `lightspeed-client`)

Shape: `protocol → client → app-core → {client bin, client-gui}` per the brief's §7.1 graph.

What it buys: a compiler-enforced seam (the GUI's import surface becomes the crate root), an independent test target, and a natural home for the CLI's orchestration.

Why it is rejected **for W2**:

1. **Package cycle.** `lightspeed` the binary is a target of the `lightspeed-client` package (`client/Cargo.toml:34-40`; `cargo metadata` target list `lightspeed_client:lib` + `lightspeed:bin`). A `lightspeed-app-core` crate that owns session commands must depend on `lightspeed-client` (the engine and status live there). For the CLI to consume it, `lightspeed-client` would have to depend on `lightspeed-app-core` — a package cycle, which Cargo forbids even across different targets.
2. **Breaking the cycle means a package split, i.e. a release change.** The only clean alternative is moving the CLI binary into its own package. That renames the published artifacts the release gate hard-lists (`.github/workflows/release.yml:421-427`) and that the distribution manifests reference (`dist/scoop/lightspeed.json`, `dist/chocolatey/tools/chocolateyInstall.ps1`, `dist/aur/PKGBUILD`), i.e. a release/packaging migration with an owner gate. The brief puts release/publish out of scope (§4) and the W2 write scopes are `client/`, `protocol/`, `client-gui/`, `platform/`, `.github/workflows/` only for parity jobs (§8).
3. **It would force the public API wider than the facade does.** A facade inside the crate may use `pub(crate)` modules (only the DTOs are re-exported, exactly as `lib.rs:20-33` already does). An external crate could only use the library's `pub` surface, so `config`, `cli`, `modes`, `route`, `session`, `quic` would all have to be made public with doc comments (`wat/rules.md:102-103`), a much bigger API commitment than one new module.

### 4.2 Option B — `lightspeed_client::app` facade module (CHOSEN)

```
lightspeed-client (package)
├── protocol                      (unchanged)
├── src/engine.rs, capture/, tunnel/, route/, quic/, session/ ...   (unchanged)
├── src/lib.rs  += pub mod app;   ← the only new public surface
│   src/app/
│     mod.rs          AppCore handle + re-exports (Config, LightSpeedError, ScoredRelay, …)
│     config.rs       AppConfig = client::Config + RoutePrefs + GamePrefs + UiPrefs
│     dto.rs          AppStatus, RouteState, RelayStatus, WatchdogStatus, RelayedPair, Capabilities, DetectedGame, AppError  (contract amendments A1–A6 in §5.0)
│     commands.rs     SessionCommand + apply()
│     discovery.rs    registry discovery, client-leg probe, /health  (from client-gui)
│     relay.rs        choose_relay(): locate → rank_at_with_direct → history filter
│     history.rs      RelayHistory                                   (from client-gui)
│     watchdog.rs     reply-fault judgement + fallback chain          (from client-gui)
│     presence.rs     presence policy + schedule                      (from client-gui)
│     game.rs         DetectedGame + editor validation violations     (from client-gui/app)
│     update.rs       semver compare + release-asset selection (pure) (from client-gui)
│     geo.rs          locate() + countries table, feature-gated       (from client-gui)
└── src/main.rs (CLI)             keeps its module tree; migrates call sites per §6 S6
```

Rules that keep it honest:

- **One direction.** Only front ends depend on `app`; `app` may use any `pub(crate)` client module; nothing in `client/` outside `app` may import `app` (checked by the guard, §7 G4-reverse).
- **UI-free.** `client/src/app/**` must not mention `egui`, `eframe`, or any GUI type; the guard fails if it does (this is what keeps the core reusable by the CLI and by a future non-egui shell).
- **Cross-platform.** No `cfg(windows)` gate on behaviour inside `app`; the only `cfg` the core may use is feature gating (`geo-ip`) — any platform-specific fact (elevation, tray, install) is supplied by the front end, never detected inside the core. The parity obligation (brief §13.3) makes this a hard rule, not a style note.
- **DTOs are cheap and owned.** `AppStatus` carries `EngineStatus` by value (its documented contract: "cheap to clone, safe to share", `client/src/engine.rs:58`).
- **Naming.** In `client-gui`, import it as `use lightspeed_client::app as core;` to avoid confusion with the existing `client-gui/src/app.rs` module.

Why B wins on the evidence:

1. It is the only option that satisfies O1 (CLI and GUI consume the same core) **without** touching the release surface, because the CLI already shares the library's source tree (F-09) and can call `lightspeed_client::app::…` from `client/src/main.rs` exactly as `:760` already does.
2. The GUI already depends on the client crate (`client-gui/Cargo.toml:15`), so adoption is a code move, not a dependency rewrite.
3. The four blocking gaps are visibility/DTO gaps (§3.5), not architecture: one `pub mod app;` line plus re-exports, with an existing precedent at `client/src/lib.rs:20-33`.
4. The measured scope of the move is 17 modules (~5.0k non-blank lines, §3.2), not a rewrite of the client crate; the engine, tunnel and capture modules are not touched.
5. The brief's W2 owner table already assigns the extraction to `rust-dev` with write scope `client/` (§8), which matches B and not A.

### 4.3 Option C — a "pure core" crate *below* `client` (protocol → core → client → front ends)

The cleanest layering in the abstract: `AppConfig`/DTOs/ranking rules/probe/geo live in a crate that depends only on `lightspeed-protocol`; the client implements the session seams and the front ends use the crate's DTOs. It is rejected because it needs the engine's status types to move down first (`EngineStatus`, `InterceptorHealth`), which is a far larger, behaviour-adjacent move than the W2 extraction, and because it still cannot carry session commands without an engine dependency — it solves the same cycle by deferring it. Recorded as the fallback if Option B's falsifier fires *and* the packaging split is accepted.

### 4.4 Option D — status quo plus GUI-only tests

Keep the GUI logic where it is, add tests and an audit script. Rejected: the brief's O1 makes zero-GUI-business-logic a program requirement, and the duplicates are not stylistic — F-03, F-04 and F-07 are behavioural divergences that drift with every feature. Testing the second implementation does not remove the second implementation.

### 4.5 Falsifier (for Option B) and the check that proves it wrong

The design is wrong if any of these becomes true **during W2**; each has a concrete check, and the W2 implementer must record the result in the stage report:

| # | Falsifier | Check that detects it | Response |
|---|---|---|---|
| F-A | A migrated rule cannot be expressed UI-free (needs an egui/eframe or `Platform` type in a signature) | Guard rule G4 (`egui|eframe` in `client/src/app`) plus the compiler: a facade item whose signature names a GUI type cannot compile without moving that type | Stop the stage; split the rule instead (the fact stays in core, the label rendering calls back into the GUI). If more than two rules need this, re-open §4.1 |
| F-B | A migrated rule needs `cfg(windows)`/capture-only API not available on Linux/macOS | Guard rule G5 (`cfg(windows)` scan of `client/src/app`) + `cargo test --workspace` on the Linux CI job (`.github/workflows/ci.yml:42`) | The rule is not portable: keep it in `platform/`, or move only the portable part |
| F-C | After S6, `client-gui/` still needs raw `lightspeed_client::engine/games/profiles/registry` calls for a fact listed in O1 | Guard rule G1 allowance table non-empty at S6 exit | The boundary leaked: do not delete the allowance; list the item and decide whether it is adapter-owned (then name it in the allowlist with a reason) or missing from the facade (then add it) |
| F-D | Packaging/release migration becomes in-scope (owner reopens the UI stack or a second out-of-graph front end appears) | Brief §13.1 decision changes, or a front end outside the workspace is proposed | Re-open §4.1/§4.3; the packaging cost becomes justified then |
| F-E | The drift guard does not catch a business rule that lands in the GUI in the first two waves | Post-wave guard audit: diff the GUI commits since the guard landed and ask "was any of this a business rule?" | The textual guard is insufficient: add a compile-time seam (move the facade behind a second crate, which then forces the packaging split) — this is the designed escalation |

**What would prove the *rejection of Option A* wrong:** a packaging decision that moves the CLI to its own package anyway (e.g. for an unrelated reason), or a second product surface outside the workspace. In that world Option A's cost collapses to "add a crate" and it becomes the better boundary; the facade's module layout is designed to move into a crate unchanged, so this reversal is cheap (the facade already has no inward dependencies beyond the client library).

## 5. The shared API (DTO sketch)

**Everything in this section is proposed for W2.** No item here exists at `62c6cf6`; each row names the existing field/type/function it carries or replaces, so the implementer is not asked to invent behaviour. Names are suggestions; signatures are the decision. The names the spec cites (`AppStatus.engine`, `AppStatus.interceptor`, `AppStatus.relay`, `AppStatus.watchdog`, `AppStatus.error`, `RelayStatus.connecting`, `WatchdogStatus`, `Capabilities.elevated`, `AppError.Kind`) are kept verbatim.

### 5.0 Contract amendments A1–A6 — one DTO home each

Authority: the spec's Appendix C and Q3 are the `interface.md` edit authority, and they record **A1–A6**: C-5…C-7 for A1–A3 and **C-13…C-15 for A4–A6**, with Q3 reading "Contract amendments **A1–A6**" and `interface.md` in W2-2's write scope (spec §7 Q3 at `gui-refresh-spec.md:775`, Appendix C authority at `:831-832`, C-5…C-7 at `:840-842`, C-13…C-15 at `:848-850`; lines as of the 851-line spec revision). The spec defers the DTO shape to this revision (state 4: "The DTO may carry a sub-field or a variant; the architect's revision decides"; state 10: `RouteState::NeedsElevation` "removed or marked deprecated"). So **A1–A6 are authorised and mirrored here**, which is what makes W2-1 acceptance "one field path per `interface.md` row" satisfiable. *Historical note:* r4 recorded a temporary gap while the spec's index still listed A1–A3; the spec's revision added C-13…C-15 and the Q3 A1–A6 wording on 2026-11-19, so that gap is **resolved** — no open action remains for this document. This document still does not edit `interface.md` (write scope); W2-2 applies the amendments.

| Amendment | Contract row | Single DTO home | What changed from r3 |
|---|---|---|---|
| **A1** Holding's fact is the engine's tunnel gate while a session runs (spec C-5) | Holding (`interface.md:33`) | `AppStatus.engine.tunnelling_enabled == Some(false)`, corroborated by `AppStatus.interceptor` present; no other field may drive the word | `is_holding` is **retired as the contract fact**; it survives only as the fallback-hold sub-detail (spec C-5) |
| **A2** Idle splits into `NOT CONNECTED` / `CONNECTED` | Idle (`:31`) | `AppStatus.engine.connected`; one `RouteState::Idle` | no DTO shape change — one variant, two words from one field |
| **A3** Error leads with a taxonomy sentence; verbatim engine text only in `Details` | Error (`:41`) | `AppStatus.error: Option<AppError>` (`AppError.Kind` for the sentence), composed from `SessionCommandState.last_error`, the `AppConfig` recovery diagnostic and the engine `interceptor_error`/`capture_error`/`windivert_error`/`redirect_error` + `registration_error` | fields added (also spec C-11; the spec's §2.1 fact table names both the command state and the config recovery) |
| **A4** Choosing is a **sub-state of the HOLDING word**, not a distinct rail word | Choosing (`:34`) | `AppStatus.relay.choosing: bool`; the rail word is `RouteState::Holding`; **no `RouteState::Choosing` variant** | variant removed; sub-field is the Choosing row's one field path |
| **A5** Relaying's fact is the `run_boosting()` gate plus **one** counter source pair | Relaying (`:35`) | `AppStatus.boosting: bool` (= `interceptor_active \|\| windivert_active \|\| capture_active \|\| redirect_active`, `client-gui/src/app.rs:2352-2357`); display pair `AppStatus.relayed: Option<RelayedPair>` | counters are a derived value, never a state driver; no "interceptor counters" generically |
| **A6** Needs elevation is an **overlay**, not a route word | Needs elevation (`:40`) | `Capabilities.elevated: bool` (front-end supplied); **`RouteState::NeedsElevation` removed** | one home; the variant is gone, the action/notice derive from the overlay |

**A4 why a sub-field, not a variant.** Choosing and Holding render the same word, the same action set and the same components; only the sub-line differs (`route.choosing`). A second `RouteState` variant would give the rail word two writers and force every word-matching render to know that two variants are one word — the ambiguity the amendment exists to close. The sub-field keeps a single word driver and still gives the Choosing contract row its own field path (`AppStatus.relay.choosing`), which is what acceptance (b) checks.

**A6 why `Capabilities.elevated` and not a variant.** The overlay's driver is a platform fact the core cannot observe itself (elevation is real and platform-owned, `client-gui/src/platform/mod.rs:94`), so it must be a front-end input, not a core-observed route state. Marking the variant "deprecated" would keep two homes and re-open QA M3/R13; it is removed, and any render that needs elevation reads `Capabilities.elevated` (the action slot already does: `primary_action(is_admin, …)`, `client-gui/src/app/ledger.rs:39-57`).

### 5.1 Config model (load/save)

| Proposed | Carries / replaces | Real source |
|---|---|---|
| `AppConfig { client: Config, route: RoutePrefs, game: GamePrefs, ui: UiPrefs }` | one persisted model | `Config` (`client/src/config.rs:10-38`); GUI `GuiConfig` fields (`client-gui/src/config.rs:54-90`) |
| `AppConfig::load(path) -> Result<Self, AppError>` / `load_or_default(path) -> LoadedAppConfig { config, recovery: Option<ConfigRecovery> }` | CLI hard-fail then default (`client/src/main.rs:400-403`) and GUI tolerant load (`client-gui/src/config.rs:112-127`) as two entry points over one parser; the recovery diagnostic is the observable fact the spec's `error.config` sentence needs (spec §2.1) | both |
| `ConfigRecovery { path: PathBuf, reason: String }` | the tolerant-loader diagnostic the GUI discards today (`client-gui/src/config.rs:112-127` logs it and returns defaults) | becomes the `AppError::Kind::Config` input to `AppStatus.error` (A3) |
| `AppConfig::save(&self, path)` | `Config::save` (`client/src/config.rs:459-463`) + GUI render/save (`client-gui/src/config.rs:237-300`) | both |
| `RoutePrefs { selected: Option<String>, proxies: Vec<RelayEntry>, auto_select: bool, retry_after_fallback: bool }` | `GuiConfig::{selected, proxies, auto_select, retry_after_fallback}` | `client-gui/src/config.rs:56-67` |
| `RelayEntry { addr: SocketAddrV4, label: String, node_id: Option<String> }` | `ProxyEntry` | `client-gui/src/config.rs:14-21` |
| `GamePrefs { game_auto, selected_game, manual_mode, manual_server, manual_port_range }` | the five GUI fields | `client-gui/src/config.rs:70-83` |
| `UiPrefs { language: Option<String> }` | GUI-only field, namespaced `[ui]` | `client-gui/src/config.rs:77` |

Collapse rules (F-01): `share_latency_stats` (`client-gui/src/config.rs:63`) becomes `client.general.telemetry` (`client/src/config.rs:47-53`); `tunnel_mtu` (`client-gui/src/config.rs:89`) becomes `client.tunnel.mtu` (`client/src/config.rs:82-83`) with the single clamp `TunnelConfig::effective_mtu_cap` (`client/src/config.rs:282-287`) replacing the GUI copy (`client-gui/src/config.rs:176-180`). Until S5, **both on-disk formats stay supported** by explicit serializers (`legacy_gui` / `client`) over the one model — that is what keeps an existing user config working and is why type unification can precede file unification.

### 5.2 Commands (session start/stop/select)

| Proposed | Replaces / calls | Real source |
|---|---|---|
| `SessionCommand::SelectRelay { addr, node_id }` | `connect_selected` (`client-gui/src/app.rs:640-659`) | `LightSpeedEngine::connect` + `set_node_id` (`client/src/engine.rs:360,392`) |
| `SessionCommand::Disconnect` | `shutdown_engine` (`client-gui/src/app.rs:1400-1410`) | `engine.rs:494` |
| `SessionCommand::StartBoost(BoostRequest)` / `StopBoost` | `start_boost`/`stop_boost` (`client-gui/src/app.rs:1432-1513`); CLI `run_intercept_mode` config build (`client/src/modes/intercept_mode.rs:45-103`) maps onto the same `BoostRequest` construction (not the CLI's Ctrl+C loop) | `LightSpeedEngine::start_interceptor_with_target` (`client/src/engine.rs:1097`), `stop_interceptor` (`:1370`), `start_windivert_for_game(_with_transport)` (`:862,883`), `start_capture`/`stop_capture` (`:594,703`), `start_redirect`/`stop_redirect` (`:509,573`) |
| `BoostRequest { game_key: String, relay: SocketAddrV4, fec: bool, fec_k: u8, port_range: Option<PortRange>, target: Option<ProcessTarget>, path_mtu: usize }` | the six arguments threaded today | `ProcessTarget` (`client/src/interceptor/traits.rs:85`, reachable through the public `interceptor` module at `client/src/lib.rs:11`; imported by the GUI at `client-gui/src/app.rs:31`), `PortRange` (`client/src/profiles/schema.rs:79`) |
| `SessionCommand::SetPathMtu(usize)` | `set_path_mtu_cap` at startup (`client-gui/src/app.rs:426`; `LightSpeedEngine::set_path_mtu_cap`, `client/src/engine.rs:347`) | same |
| `SessionCommand::Hold { release_when_connected: bool }` | the hold/release dance in `start_boost` (`client-gui/src/app.rs:1468-1497`) | `hold_tunnelling`/`release_tunnelling`/`release_hold_when_connected` (`client/src/engine.rs:1286,1293,1312`) |
| `AppCore::apply(cmd) -> Result<(), AppError>` | owns `Arc<Mutex<LightSpeedEngine>>`, the mutex discipline today at `client-gui/src/app.rs:386-394` (`lock_or_recover`) | `EngineStatus` is a poisoned-lock-safe `RwLock` (`client/src/engine.rs:230-246`) |
| `SessionCommandState { in_flight: Option<SessionCommand>, last_error: Option<AppError> }` (carried as `AppStatus.session`) | the spec's W2-1 command facts: `in_flight` drives the action loading state (`STARTING…`) and `last_error` the start-refusal error (spec §2.1, DTO gaps) | neither exists today: a start failure is only `tracing::error!` (`client-gui/src/app.rs:1452-1454`); `apply` sets `in_flight` for the command's duration and stores the failure |

### 5.3 Status snapshot

`AppStatus` is the single status surface. It is composed, not flattened: `engine` is the 67-field `EngineStatus` verbatim, and every derived group has exactly one field path per contract row (mapping below). `RouteState` is the **rail word only**, projected once inside `AppStatus::collect` from the rows' facts — it is never a second source that the GUI re-derives.

| Proposed | Carries | Real source |
|---|---|---|
| `AppStatus { engine: EngineStatus, interceptor: Option<InterceptorHealth>, relay: RelayStatus, route: RouteState, watchdog: WatchdogStatus, presence: PresenceStatus, session: SessionCommandState, boosting: bool, relayed: Option<RelayedPair>, error: Option<AppError> }` | one snapshot per frame | `EngineStatus` 67 fields (`client/src/engine.rs:60-226`); `InterceptorHealth` 10 fields (`:27-56`); `snapshot` (`:1395`), `interceptor_health` (`:1019`), `status_watch` (`:1390`); `session` = §5.2 command state; `boosting`/`relayed` = A5; `error` = A3 |
| `RouteState` — **rail words only** (8 after A4/A6): `Idle`, `Connecting`, `Holding`, `Relaying`, `DirectFallback`, `RetestPending { retry_in }`, `ServerRejectsRelay`, `Error { message }` | the rail word; no variant for a row without one | `interface.md:29-46` as amended: "Relay recovered" is trace-only (`:39`), Choosing is a sub-field of HOLDING (A4), Needs elevation is an overlay (A6) — see §5.0 |
| `RelayStatus { relay: Option<SocketAddrV4>, ranked: Vec<ScoredRelay>, server: Option<Ipv4Addr>, server_ping: Option<ServerPing>, server_leg: ServerLegState, connecting: bool, choosing: bool, cannot_beat_direct: Option<(u64, u64)> }` | the route facts | `ScoredRelay` (`client/src/route/relay_choice.rs:90-106`); `ServerPing`/`ServerLegState`/`MeasuredServerLeg` (`client-gui/src/app/relay_choice.rs:40-90`, moved); `connecting` = the session command in flight (spec C-11 DTO gap, W2-1; ended by `engine.control_registered`/`registration_error`); `choosing` = A4 sub-state (client-leg measurement/ranking in flight for the locked server); `cannot_beat_direct` = `client-gui/src/app/choice.rs:266-282` (moved) |
| `RelayedPair { sent: u64, returned: u64 }` (`AppStatus.relayed`) | A5's one display pair | the per-server `EngineStatus.interceptor_flows` row for the locked server when present (`client/src/engine.rs:188`), else `redirect_pkts_out`/`redirect_pkts_in` while redirect is active (`:101-103`) — **never mixed**; derived exactly once |
| `WatchdogStatus { fault: Option<RelayReplyFault>, streak: ReplyFaultStreak, retest: Option<RetestTarget>, rejection: Option<RelayRejection>, retry_in: Option<Duration> }` | the fallback chain states | `client-gui/src/app/watchdog.rs:92-188` (moved) |

**Contract-row → single field path** (this is the table acceptance (b) checks):

| Contract row (`interface.md`) | Single field path | Amendment / note |
|---|---|---|
| Idle (`:31`) | `AppStatus.engine.connected` | A2: selects `NOT CONNECTED` / `CONNECTED`; one `RouteState::Idle` |
| Connecting (`:32`) | `AppStatus.relay.connecting` | DTO gap (spec C-11, W2-1); ends on `engine.control_registered` / `engine.registration_error` |
| Holding (`:33`) | `AppStatus.engine.tunnelling_enabled == Some(false)` with `AppStatus.interceptor` present | A1: the engine gate is the fact; `is_holding` is not |
| Choosing (`:34`) | `AppStatus.relay.choosing` | A4: sub-state of the HOLDING word (`RouteState::Holding`) |
| Relaying (`:35`) | `AppStatus.boosting`; display pair `AppStatus.relayed` | A5: the `run_boosting()` gate drives the word; the pair is derived, never a driver |
| Direct fallback (`:36`) | `AppStatus.watchdog.fault` | |
| Re-test pending (`:37`) | `AppStatus.watchdog.retry_in` | |
| Server rejects relay (`:38`) | `AppStatus.watchdog.rejection` | |
| Relay recovered (`:39`) | **none** — trace-only, no surface | the guard's G2 test asserts there is no `RouteState` variant for it |
| Needs elevation (`:40`) | `Capabilities.elevated` (front-end supplied) | A6: overlay, not a route word; the rail keeps its route word |
| Error (`:41`) | `AppStatus.error` (`AppError`/`AppError.Kind`), composed from `AppStatus.session.last_error`, the `ConfigRecovery` diagnostic and `engine.interceptor_error`/`capture_error`/`windivert_error`/`redirect_error` + `registration_error` | A3 |

**Server-leg rows and the caption** (`interface.md:48-67`, rendered only while `boosting` and the route is the relay):

| Contract row | Single field path | Source |
|---|---|---|
| Server leg measured (`:63`) | `AppStatus.relay.server_leg == ServerLegState::Measured(..)` | estimator `estimate()`/`measurement_age()`; `client-gui/src/app/choice.rs:170-181,197-230` (moved) |
| Server leg bounded (`:64`) | `ServerLegState::Bounded { floor_us }` | ranking `dest_source == GeometricFloor`, or the early `L3d − L1` proof |
| Server leg not measured (`:65`) | `ServerLegState::Prior { prior_us }` | `dest_source == RegionPrior`; `{unc}` = `PRIOR_UNCERTAINTY_US` 25 ms |
| Server leg unknown (`:66`) | `ServerLegState::Unknown` | ranking row `Unknown`, or `Measured` with nothing publishable |
| Relay cannot beat direct (`:67`) | `AppStatus.relay.cannot_beat_direct: Option<(u64, u64)>` | `L1 >= L3d` from the keepalive one-RTT leg and a **fresh, same-server** direct ping (`SERVER_PING_FRESH_SECS` 60 s, `client-gui/src/app.rs:119`); `client-gui/src/app/choice.rs:266-282` (moved) |

Rules that keep this a DTO and not a god-struct: **each contract row has exactly one field path**, and `RouteState` is a projection computed once in `AppStatus::collect` from those paths (so "one path per row" and "one rail word" never disagree). A fact the engine cannot observe does not get a field or variant (`interface.md:25-27,142-143`); the only front-end facts are explicit inputs to `AppStatus::collect(front_end: FrontEndFacts)` — `Capabilities.elevated` and `language` — never guessed.

**Two W2-1 facts the spec adds that are not contract rows** (spec §2.1 "facts the contract needs and the DTO does not carry"): `SessionCommandState.in_flight` (`AppStatus.session.in_flight`) drives the action loading state (`STARTING…`), and `LoadedAppConfig.recovery` (`ConfigRecovery`) feeds the `error.config` sentence. Neither may select a rail word. The spec's W2-1 presentation seam (`client-gui/src/app/route_surface.rs`, new) consumes `AppStatus` + the interim GUI `WatchdogFacts` until S4 swaps that input for `WatchdogStatus`; that adapter is presentation-only (§3.3) and derives no state — so the two watchdog names are the spec's staging seam, not a second DTO.

Derived presentation facts stay derived **once**: `primary_action` is a pure function of (`Capabilities.elevated`, `AppStatus.boosting`, `AppStatus.engine.connected`, relay presence) (`client-gui/src/app/ledger.rs:39-57`); `direct_fallback` becomes a projection of `AppStatus.watchdog` (today the GUI field at `client-gui/src/app.rs:241,1749-1825`); `cannot_beat_direct` lives in `RelayStatus`; `is_holding` (`client-gui/src/app/ledger.rs:215-224`) moves with its tests but is **demoted by A1** to the fallback-hold detail — it may not drive the Holding word.

### 5.4 Relay choice

| Proposed | Carries / replaces | Real source |
|---|---|---|
| `ClientLeg { median: Duration, jitter: Duration }` | `RelaySample` (moved) | `client-gui/src/discovery.rs:216-223` |
| `RelayChoiceInputs { server: Option<Ipv4Addr>, candidates: &[RelayCandidate], client_legs: &[(SocketAddrV4, Option<ClientLeg>)], direct_ping: Option<ServerPing>, history: &RelayHistory }` | the GUI's inputs assembled at `choose_relay_for_destination` | `RelayCandidate` (`client/src/route/relay_choice.rs:69-86`); `ServerPing` (`client-gui/src/app/relay_choice.rs:61-69`); `RelayHistory` (F-06) |
| `choose_relay(inputs) -> RelayChoice { Chosen { winner: ScoredRelay, ranking: Vec<ScoredRelay> }, NoServer, NoCandidate, AllCondemned }` | `client-gui/src/app.rs:788-917` | `rank_relays_at_with_direct` (`client/src/route/relay_choice.rs:243`); geo lookup (moved); history filter (`client-gui/src/app.rs:842-868`) |
| `direct_us_for(ping, server)` / `early_triangle_floor(...)` / `server_leg_state(...)` | freshness + triangle-floor rules | `client-gui/src/app/choice.rs:89-108,197-251` (moved) |
| `relay_position(node_id)` / `RelayEntry.region` fixed | relay identity | `client/src/route/relay_choice.rs:487-504` (re-exported in S2); registry `region` (`client/src/registry.rs:86-88`) |

The chooser is also the source of the A4 sub-state: `AppStatus.relay.choosing` is true while the client-leg measurement/ranking pass for the locked server is in flight (`start_relay_measurement`/`choose_relay_for_destination`, `client-gui/src/app.rs:925-957`), and the `RelayChoice` result becomes `AppStatus.relay.ranked`/`server_leg`. The GUI never re-derives "choosing" from a timer; if a W2-1 slice cannot obtain an in-flight signal, the field is `false` and the sub-line is not shown — an honest fallback, not an invented state (the spec's §6 Choosing capture is a slow-chooser fixture).

### 5.5 Game detection

| Proposed | Carries | Real source |
|---|---|---|
| `DetectedGame { key: String, name: String }` + `detect_running() -> Option<DetectedGame>` | replaces the GUI's display-name fuzzy remap (F-07) | `games::auto_detect` (`client/src/games/mod.rs:513-517`); canonical key table `GAME_REGISTRY` (`:396-425`) |
| `ProfileViolation` enum + `validate_draft(draft, user_keys, editing) -> Vec<(Field, ProfileViolation)>` | the editor's per-field rule mirror | `client-gui/src/app/editor.rs:143-169`; authority `profiles::schema::validate_for_save` (`client/src/profiles/schema.rs:397`) — the GUI maps violations to i18n keys (`client-gui/src/app/editor.rs:175-202`), so no localized text crosses the boundary |

### 5.6 Capabilities and update

| Proposed | Carries | Real source |
|---|---|---|
| `Capabilities { version, os, arch, features, interceptor_platform, interceptor_available, elevated }` | the CLI `--status` block and the GUI Components block as one fact set | CLI print `client/src/main.rs:731-864`; interceptor probe `client/src/main.rs:760-775`; GUI Components `client-gui/src/app.rs:3243-3290`; `elevated` supplied by the front end (`platform/*::is_admin`, `client-gui/src/platform/mod.rs:94`) |
| `compare_versions(current, latest) -> bool`, `ReleaseAsset { tag, zip_url, sha256_url }`, `parse_sha256_file(text)` | pure update rules | `client-gui/src/update.rs:30-87`; `client-gui/src/self_update.rs:111-119` |
| Network/receipt/elevated install stays in the GUI | `axoupdater`, `reqwest`, `sha2` are GUI-only dependencies (`client-gui/Cargo.toml:20,38,40`) | do not add them to the client crate's dependency graph |

**A6 single home:** `Capabilities.elevated` is the **only** field path for the Needs-elevation contract row (`interface.md:40`); it is front-end supplied (`platform/*::is_admin`, `client-gui/src/platform/mod.rs:94`) and passed into `AppStatus::collect` as an explicit input. The rail keeps its route word, and the action/notice derive from the overlay plus readiness (`primary_action(is_admin, …)`, `client-gui/src/app/ledger.rs:39-57`). `RouteState` has no `NeedsElevation` variant (§5.0); a render that needs elevation reads this field, never a route state.

### 5.7 Error taxonomy

One `AppError` with a `Kind` (`Config`, `Tunnel`, `Capture`, `Route`, `Quic`, `Game`, `Validation`, `Io`, `Unsupported`) mapping the existing `LightSpeedError` categories (`client/src/error.rs:9-33`) and wrapping command failures. Re-exported through `app::error` (precedent: `client/src/lib.rs:20-33` re-exports items from a `pub(crate)` module). The CLI prints `Kind` as today's messages; the GUI maps `Kind` → i18n key. No `String` errors cross the boundary — the GUI's current ad-hoc strings (`client-gui/src/config.rs:297`, `client-gui/src/self_update.rs:23`) become `AppError` values.

**A3 wiring:** `AppStatus.error: Option<AppError>` is the Error row's single field path (§5.3). It is composed from `SessionCommandState.last_error` (`AppCore::apply`'s result; today a start failure is only `tracing::error!`, `client-gui/src/app.rs:1452-1454` — spec C-11 DTO gap, W2-1), from the `ConfigRecovery` diagnostic of `AppConfig::load_or_default` (the observable fact behind `error.Config`, spec §2.1), and from the engine's own `interceptor_error`/`capture_error`/`windivert_error`/`redirect_error` (`client/src/engine.rs:111,133,151,203`) plus `registration_error` (`:87`). The localized taxonomy sentence comes from `Kind`; the verbatim engine message is carried for the GUI's `Details` disclosure and is never promoted to the headline word.

### 5.8 CLI call sites that migrate (S6)

| CLI behaviour | Current site | Migrates to |
|---|---|---|
| config load/default | `client/src/main.rs:400-403` | `app::config::{load, load_or_default}` |
| profiles init | `client/src/main.rs:411-414` | unchanged (`profiles::init`, already shared) |
| proxy resolution (explicit → configured → registry → pick) | `client/src/main.rs:93-245` (`resolve_proxy_addr`), probes via `client/src/modes/proxy_probe.rs:168-194` | `app::discovery` + `app::relay::choose_relay` (probe-semantics decision Q1 first) |
| game detection for the default path | `client/src/main.rs:1146-1167` | `app::game::detect_running` (CLI keeps its `Box<dyn GameConfig>` use where it needs the full profile) |
| `--list-games` | `client/src/main.rs:594-614` | unchanged (`games::*` + `profiles::render_list_games` already shared) |
| `--status` | `client/src/main.rs:731-864` | `app::capabilities::Capabilities` + printer; output text preserved |
| `--check` environment diagnostics | `client/src/main.rs:868-…` (interceptor + elevation checks) | `app::capabilities` for the shared parts; platform probes stay as they are |
| session start (`--start-interceptor`) | `client/src/main.rs:1111-1144` → `client/src/modes/intercept_mode.rs:35-127` | `app::commands::BoostRequest` construction; the mode's Ctrl+C loop is CLI-owned and unchanged |
| `--watch` | `client/src/main.rs:673-704` → `client/src/modes/watch_mode.rs` | unchanged in W2 (Q4/Q7 decide whether it adopts the shared presence policy) |
| `--probe-proxies` | `client/src/main.rs:1228` | `app::discovery` (same probe-semantics decision) |

## 6. Staged migration plan

Every stage keeps the full gate green, changes no behaviour, and is independently revertible. Gate commands are the repo gate (`AGENTS.md:69-76`):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
cargo clippy -p lightspeed-gui --all-targets
cargo test -p lightspeed-gui
# plus, if the WinDivert module is touched (it is not planned):
cargo test -p lightspeed-client --features windivert-redirect
```

Baseline to record before S0, not inherit: at `62c6cf6` the gate of record is workspace **1,624 passed / 0 failed / 6 ignored** and GUI **193 / 0 / 1** (`wat/state/current-phase.md:35,47`). Measured on this tree: `client-gui/src` declares 196 `#[test]`; 2 are `#[cfg(not(windows))]` (`client-gui/src/single_instance.rs:280,300`), so a Windows run compiles 194 = 193 passed + 1 ignored (`client-gui/src/discovery.rs:501`), which matches. A stage may move tests between crates; a stage may not reduce the compiled test count or change CLI output.

| Stage | Scope (write) | What moves | Gate expectation | CLI compatibility proof | Rollback |
|---|---|---|---|---|---|
| **S0** guard skeleton | `client/tests/gui_boundary.rs` (new), evidence | the ratchet guard in "current violations allowed" mode: G1 allowance lists every existing `lightspeed_client::` path in the GUI; G2 identifier list is empty for now; G3 budgets = measured §3.2 numbers | workspace green with the guard passing; GUI unchanged | CLI untouched | delete the test file |
| **S1** config model | `client/src/app/{mod,config}.rs`, `client-gui/src/config.rs` → thin, `app.rs` call sites, `client-gui/src/main.rs:122` | `GuiConfig` → `AppConfig` (legacy serializer preserves the exact `config.toml` keys: `client-gui/src/config.rs:237-290`); `share_latency_stats`/`tunnel_mtu` collapse to `general.telemetry`/`tunnel.mtu`; 15 GUI tests move to core; CLI `Config` re-exported unchanged | +15 client tests, −15 GUI tests (same total); counts recorded | `--config` parse failures/default behaviour unchanged (unit tests already pin it, `client/src/config.rs:466-979`); add one `cli_profile`-style test for a malformed `--config` | revert S1 commits; GUI file format was untouched |
| **S2** discovery/probe/identity | `client/src/app/{discovery,relay}.rs`, `client-gui/src/discovery.rs` → ~60-line host wrapper | registry discovery (adopt `discover_relays` so `region` survives), `/health` client, 5-probe measurement, race, health parse, **the fix that passes `region` not the label into `RelayCandidate`**, and the `relay_position` re-export + `RelayEntry.region` plumbing that lets `globe.rs` drop its coordinate table; 21 tests move | +21 client, −21 GUI; workspace tests now include the moved probe tests on Linux CI (they bind loopback only — verify) | CLI probe path untouched in this stage | revert; GUI wrapper still calls old module until the commit is dropped |
| **S3a** status DTO (read path) | `client/src/app/{dto,status}.rs`, `app.rs` derived getters, `client-gui/src/app/{choice,ledger}.rs` (rules), new `client-gui/src/app/route_surface.rs` (presentation seam, spec §2.1) | `AppStatus` assembly from `EngineStatus`+`InterceptorHealth`; the contract-row → single-field-path table (§5.3) incl. amendments A1–A6; move `ServerPing`/`ServerLegState`/`can_beat`/`early_triangle_floor`/`is_holding` (demoted, A1)/`primary_action`/`direct_fallback` derivation; **plus the spec's W2-1 facts**: `RelayStatus.connecting`, `AppStatus.error`, `SessionCommandState.{in_flight,last_error}`, `LoadedAppConfig.recovery` (spec §2.1 table, C-11) | tests move from `app/tests.rs` clusters into `client/src/app/`; GUI drawing unchanged; real-window screenshot unchanged; each contract row's single path tested both directions (spec acceptance b) | CLI untouched | revert |
| **S3b** commands | `client/src/app/commands.rs`, `app.rs` orchestration | `SessionCommand` + `AppCore`; `connect_selected`, start/stop boost, hold/release, relay restart, `shutdown_engine` become command calls; `restart_interceptor_on_selected_relay`'s logic moves whole | GUI visual check: idle/boosting/fallback states render identically (screenshots per skill §8) | CLI untouched | revert |
| **S4** judgement + memory | `client/src/app/{watchdog,history}.rs`, `app/relay_choice.rs`, `race_watch.rs` | watchdog thread + fault logic + retest backoff, `RelayHistory` + persistence, relay-health verdict, switch hysteresis. GUI keeps only `mpsc` wiring and trace pass-through | 14+15+4 tests move; real-window fallback/retest text unchanged | CLI untouched (adoption is Q4) | revert; `relay-history.json` format unchanged so no data migration |
| **S5** presence, game, geo, config file, update rules | `client/src/app/{presence,game,geo,update}.rs`, `client-gui/src/{geo,geo_countries,update,self_update}.rs` (thinned), `client-gui/src/paths.rs` (SPLIT: config-dir part only), `client-gui/data/geo-ipv4.bin` (current location, 1,810,610 B; the blob moves to `client/data/geo-ipv4.bin` only under Q2 option (a)), `infra/scripts/build-geo-ipv4.py:38` (infra scope, coordinate) | presence policy + schedule (worker stays GUI); `DetectedGame` canonical mapping (fixes F-07); geo lookup behind feature `geo-ip` (enabled by `client-gui`, added to `full` for CI coverage); **the update pure rules** `compare_versions`/`ReleaseAsset`/`parse_sha256_file` (F-12; QA R10 — now scheduled here, not left undeclared); **one config file** with legacy import. Acceptance for the file move (QA R9): old/new/both fixtures, Q3 precedence, the legacy file is never deleted, one release-note line | workspace + GUI green; Linux/macOS compile of the GUI crate is not part of CI yet — the feature must be `cfg`-clean by construction (no target cfg) | CLI flags unchanged; `--config` semantics unchanged; GUI first-run migration is GUI behaviour (test fixtures for old/new/both files) | revert; the legacy `config.toml` is only read, never deleted, so rollback loses nothing |
| **S6** CLI adoption + ratchet to zero | `client/src/main.rs` (call sites only), `client/src/modes/*`, `client/tests/gui_boundary.rs` allowances → empty | repoint the §5.8 call sites; delete the CLI's now-duplicate modules **only where nothing references them**; guard allowance table shrinks to the adapter-owned exceptions (expected: spawn wrappers, platform, i18n, design) | full gate + CLI text/exit tests green; GUI 193-count back to its own baseline; guard mutation proof recorded (inject → fail → revert → pass) as required by §7 | per-row output comparison before/after (capture `--status`, `--list-games`, `--config` bad file, `--probe-proxies`); new permanent tests under `client/tests/` following `client/tests/cli_profiles.rs:41-50` | revert S6 commits; S1–S5 stay landed |

Stage rules that make "gate green" checkable rather than asserted:

1. One stage per PR series, one commit per moved unit (the `app-refactor-plan.md` Phase-A discipline, `docs/design/app-refactor-plan.md:275-302`); "no behaviour change bundled with a move".
2. Before/after gate output saved under `wat/evidence/2026-11-*/` with counts **per crate**, never "tests pass" (`wat/rules.md:101`).
3. Any test that must stay in the GUI (platform/Windows semantics, e.g. `single_instance.rs`) is named in the stage report; everything else moves with its module.
4. QA independently re-runs the gate and the guard mutation; the author never verifies their own stage (`AGENTS.md:48-49`).
5. S6 does not start until the CLI compatibility tests exist and pass on the pre-S6 tree.

## 7. Drift guard design

**Location:** `client/tests/gui_boundary.rs` — a workspace test, so it runs in the existing `cargo test --workspace --exclude lightspeed-gui` CI job (`.github/workflows/ci.yml:42`) and in the local gate; no workflow edit, no new tooling dependency. The test reads `client-gui/src/**/*.rs` via `std::fs` from `CARGO_MANIFEST_DIR/../client-gui/src`.

| Rule | What it asserts | Fails when | Ratchet |
|---|---|---|---|
| **G1 — import surface** | every `lightspeed_client::` path in `client-gui/src` is either under `lightspeed_client::app` or in `ALLOWED_CLIENT_PATHS` (file → path prefixes) | a new direct engine/games/profiles/registry call appears, or an allowed one is added without editing the table | the table starts as the S0 snapshot of all current calls and only shrinks; S6 goal is `app` only + the named adapter exceptions |
| **G2 — moved identifiers** | a listed set of identifiers does not appear in `client-gui/src` outside its allowance: `RETURN_SHARE_DENOMINATOR`, `RELAY_REPLY_STREAK`, `RELAY_REPLY_MIN_INTERCEPTED`, `relay_reply_fault`, `decide_switch`, `RELAY_SWITCH_MARGIN_MS`, `friendly_label`, `discover_nodes`, `probe_blocking`, `measure_spreads_blocking`, `rank_relays_`, `relay_health_verdict`, `is_holding`, `primary_action`, `try_auto_detect_game`, `compare_versions`, `parse_sha256_file`, `validate_draft`, `RelayHistory` | a business rule is re-implemented (the identifier comes back) | identifiers are added at the stage that moves them; the allowance for each ends at zero |
| **G3 — size budget** | `MODULE_BUDGET: &[(&str, usize)]` — every `client-gui/src/**/*.rs` file is listed with a maximum total line count (initial values = §3.2 measured totals) | a new `.rs` file appears unlisted, or an existing one grows past its budget | budgets only lower; they are lowered in the same commit that deletes the moved code |
| **G4 — core is UI-free** | scanning `client/src/app/**/*.rs`: no `egui`, `eframe`, `Color32`, `Painter`, `Platform` trait, `client-gui` | logic creeps into a rendering dependency, or the facade starts depending on the GUI | absolute (no allowance) |
| **G5 — core is portable** | scanning `client/src/app/**/*.rs`: no `cfg(windows)` / `cfg(target_os = "windows")` on behaviour | an extraction hides a rule behind Windows-only code, silently breaking the Linux/macOS parity obligation (brief §13.3) | absolute; a legitimate Windows-only fact stays in `platform/` |

Why this shape and not a `tools/` grep or a CI script: it is a Rust test in the workspace gate the team already runs and already reads counts from; it cannot be skipped by forgetting a workflow edit; it has no shell/platform dependency (the host is Windows, CI is Linux, and both run `cargo test`); and it is ratcheted, so the day it lands it is green on the current (dirty) tree instead of requiring the migration to be finished first.

**Proof obligation** (same as any bug fix in this repo, `AGENTS.md:80-81`): at S0 and again at S6, inject one violation (e.g. add `fn sneaky() -> u64 { crate::relay_history::RETURN_SHARE_DENOMINATOR }` with a `use`), show the guard **fails**, revert, show it passes, and save both logs. The exact injection list is in the evidence report.

**Limits, stated honestly:** the guard is textual, so it cannot detect a business rule rewritten without its original identifier (e.g. a new `if unsent * 8 > decided` in the GUI). Mitigations: the ratchet makes any *new* identifier outside the allowlist a review question; G3 makes a new logic-sized file visible; the "shared behaviour is tested in the shared crate" rule means a re-implementation must also duplicate its tests to pass review; and the escalation path is F-E (a compile-time seam). It is weaker than a crate boundary and this is the accepted cost of the packaging decision (§4.1).

## 8. Risks

| # | Risk | Likelihood / impact | Mitigation |
|---|---|---|---|
| R1 | Moving the watchdog/fallback state machine changes when a session falls back (behaviour regression behind a state the owner sees in a live match) | Medium / high | Move-only stages with the 14 behavioural tests moved first; interface.md's driving facts as the acceptance oracle; QA independent; real-window screenshots of fallback states where reachable, named unverified where not (elevation/WinDivert limits) |
| R2 | CLI output/exit-code changes when S6 repoints `--status`/probe paths | Medium / medium | CLI compatibility tests written **before** S6 and run on the pre-S6 tree; text compared line-by-line; probe-semantics decision (Q1) kept out of S6 until approved |
| R3 | Config-file unification (S5) loses or splits a user's settings | Medium / high | One model, two serializers from S1; S5 only *reads* the legacy `config.toml` and writes the canonical file; never deletes; fixtures for old/new/both; release-note line (owner gate, `wat/rules.md:60-80` COMMS) |
| R4 | Geo blob in the core grows the CLI binary | Low / low-medium | Feature-gate `geo-ip` (GUI only initially); measure artifact delta in S5 and record it; if the delta is unacceptable, keep the blob GUI-side and pass `(lat,lon)` into `choose_relay` (the API already takes a position) |
| R5 | Moved tests now run on Linux CI where they previously ran only in the Windows GUI job | Medium / low | S2/S4 stage gate includes the Linux CI result; any Windows-only test stays in the GUI job; the portability is a benefit (cross-platform coverage) but must be verified, not assumed |
| R6 | `Config`/`EngineStatus`/error types become public API and need doc comments | High / low | They already have doc comments; `QUALITY_STUB` requires public docs (`wat/rules.md:102-103`); the crate is not published, and `lib.rs:20-33` established the re-export precedent |
| R7 | Facade adoption collides with the W2 shell rewrite: both edit `client-gui/src/app.rs` | High / high | Sequence, do not parallelise: S1/S2 (files outside `app.rs`) can run beside shell work; **S3a/S3b/S4 must not run concurrently with the shell rewrite** — Lead sequences them (extraction before shell, or shell first and extraction rebased). Write scopes on the tasks must reflect this |
| R8 | `#![allow(dead_code)]` at `client/src/lib.rs:7` hides unused facade items | Low / low | Accepted (pre-existing, documented in the file); the CLI/CLI tests exercise the facade, and the guard's G1 table reveals unused allowances at S6 |
| R9 | Facade DTO exposes a `pub(crate)` type in a public signature → `private_interfaces` warning | Medium / medium | DTOs are defined in `app` or re-export the public ones (`EngineStatus`, `ScoredRelay`, `Config` are all `pub`); the only rename needed is `PRIOR_UNDERTAINTY_US`, already handled at `lib.rs:29-32` |
| R10 | Presence/elevation policy parity (GUI never auto-starts; CLI `--watch` does) is decided accidentally during the move | Medium / medium | Not decided in W2; recorded as Q7 for `ux-lead`/`desktop-platform`; the extraction keeps both behaviours exactly as shipped |
| R11 | ~~A4–A6 not yet in the spec's Appendix C/Q3~~ — **resolved 2026-11-19**: the spec records A1–A6 (Appendix C C-13…C-15 at `gui-refresh-spec.md:848-850`, plus C-5…C-7 at `:840-842`; Q3 "A1–A6" at `:775`; lines as of its 851-line revision), so W2-1 acceptance (b) now rests on an authorised contract. r4 recorded the gap while it was open | Closed | No action for this document. Residual: W2-2 edits `interface.md` under the approved Q3, as planned. |

## 9. Open questions (for the Lead, owner or W2 implementer)

1. **Q1 probe semantics.** Keep two probes or converge? The CLI measures UDP keepalive (median of 3), the GUI TCP connect (median/spread of 5). Recommendation: converge on the TCP one-RTT figure (its rationale is already canonical in `docs/server-leg-signal.md` and interface.md:109-116) — but that changes `--probe-proxies` numbers, which is a user-visible CLI behaviour change and needs a changelog line + owner release gate. Until approved, S2 moves both with distinct names and no surface is repointed.
2. **Q2 geo asset.** Move the 1.8 MB blob (currently `client-gui/data/geo-ipv4.bin`, 1,810,610 B) + generated table into the client crate (feature-gated) or keep it GUI-side and pass coordinates? Depends on a CLI artifact-size budget; the generator path (`infra/scripts/build-geo-ipv4.py:38`) is infra-owned, so the move needs `infra-dev`/`dev-ops` coordination in the same PR.
3. **Q3 config file unification (S5).** Precedence when both files exist: `--config` > canonical > legacy GUI file is the recommended rule; "never silently merge"; migration writes and leaves the old file. Needs a product-facing release note; owner sign-off at the W1/W2 gate.
4. **Q4 does the CLI want the watchdog/history runtime?** If yes, S6 grows from "same API" to "same runtime behaviour" (new CLI capability, new QA surface). Recommendation: defer, document as available-but-unused core.
5. **Q5 status DTO granularity.** Composition (engine verbatim + derived groups, as §5.3) vs one flat struct; recommendation is composition because interface.md requires one path per row; W2 may split groups further without changing the rule.
6. **Q6 update check.** Only the pure rules move (no new client dependencies); confirm no one needs axoupdater in the CLI before S1 closes.
7. **Q7 presence policy canon.** GUI: Auto-only, 5 s, never auto-starts. CLI `--watch`: 2 s/3 s, auto-starts. Which is the product behaviour for the 2026 GUI + parity plan? `ux-lead`/`desktop-platform` decision, not the architect's.
8. **Q8 W2 scheduling.** The shell rewrite and stages S3–S4 both rewrite `app.rs`; the Lead must sequence them (R7).
9. **Q9 amendment authority (A4–A6) — resolved 2026-11-19.** The spec records A1–A6 in Appendix C (C-13…C-15 for A4–A6 at `gui-refresh-spec.md:848-850`, C-5…C-7 for A1–A3 at `:840-842`), and Q3 asks approval for "Contract amendments A1–A6" at `:775` with `interface.md` in W2-2's write scope (lines as of its 851-line revision). The DTO mirror in §5.0 therefore rests on an authorised contract; no open question remains. *Historical:* r4 asked the spec owner to add A4–A6 — the owner did. This document still does not edit `interface.md` (write scope).

## 10. What this document does not decide

- It does not change the state contract; `docs/design/interface.md` remains binding and is the acceptance oracle for stage S3a. Contract amendments A1–A6 are recorded in the spec's Appendix C (C-5…C-7, C-13…C-15) and Q3; §5.0 mirrors them into the DTO only.
- It does not pick the probe implementation, the config file's final path, or the presence policy — those are Q1/Q3/Q7.
- It does not plan the shell, tokens, platform adapters or CI parity jobs — those are the other four W1 documents.
- It makes no code, config, manifest, workflow or fleet change (W1 is docs-only).
