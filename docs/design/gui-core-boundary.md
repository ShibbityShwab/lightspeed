# GUI core boundary — one logic, GUI as a wrapper

| | |
|---|---|
| Programme | WF-081 "Desktop 2026", W1 architecture + delta task-17 |
| Author | `architect` (task-1; r4 = task-10; r8 = task-15; r9 = task-17) |
| Date | 2026-11-20 |
| Revision | **r15 (2026-11-20)** — r9 re-baseline plus the task-24 repair: the r9 working-tree copy was double-encoded UTF-8 (origin: the task-17 in-place PowerShell 5.1 rewrite passes; repaired at byte level — see `wat/evidence/2026-11-20/team/architect/delta-r15.md`), with the F9 generator/feature/owner naming and the F15/F16/F23 wording corrections. Revision history: **r9 (2026-11-20)** — re-baselined after the W1 commit was rebased onto `origin/master` `76aba5d`: 16 commits, **+5,553/−789** across `client-gui/` and `client/src/route` (plus new `client/src/server_leg.rs`, protocol control messages, proxy relay/trace). New modules inventoried, verdicts/stages re-mapped, DTO extended for the new facts, drift guard re-budgeted. Map: `wat/evidence/2026-11-20/team/architect/delta-r9.md` |
| Revision measured | worktree `dsh-b74d064d-6fe5` at **`0df57ce`** (the W1 commit `3648645` rebased onto `76aba5d`), clean tree |
| Status | **Decision, for Lead integration and owner acceptance of the W1 set.** No code changes. |
| **Contract status (read first)** | The rebase did **not** change `interface.md` (261 lines; `git diff 62c6cf6..HEAD` empty). The shipped code now differs from it: automatic direct fallback and its re-test are **removed** (warn-only starvation), and the location ladder, route trace, relay-measured server-leg and update-check disclosure surfaces are new. The W2 spec delta carries the contract amendments (A7–A11 are proposed here); W2-2 applies them. Until that delta lands, **W2-1 acceptance (b) "one field path per `interface.md` row" must be graded against the amended rows, not the stale file** — see §5.0 and R17 |
| Brief | [gui-refresh-brief.md](gui-refresh-brief.md) — this document executes §7.1's delegated decision, verifies §7.3's duplicate list, and supplies §8's W1 `[architect]` deliverable. It does not restate the brief |
| Contract it serves | [interface.md](interface.md) (11 route states + server-leg states) as amended by the spec's Appendix C; amendments A1–A6 are recorded there, A7–A11 are pending in the spec delta (§5.0) |
| Specification it serves | [gui-refresh-spec.md](gui-refresh-spec.md) — the W2 slice cut (its §5) is re-cut from §3.2.1; the DTO names it cites (`AppStatus.*`, `RelayStatus.connecting`, `SessionCommandState.*`, `WatchdogStatus`, `Capabilities.elevated`, `AppError`) are kept in §5 |
| Evidence | `wat/evidence/2026-11-20/team/architect/delta-r15.md` (this fix: encoding repair + F9/F15/F16/F23) and `delta-r9.md` (r9 re-baseline; its encoding claim corrected in place) + `wat/evidence/2026-11-19/team/architect/{report.md,revision-r4.md,revision-r8.md,line-counts.txt,structure-counts.txt,cargo-metadata-no-deps.json}` |

**Line-count convention.** Every count is `total lines (non-blank lines)`, measured at the revision above by `Get-Content … .Count` and `Get-Content … | Measure-Object -Line`, as r8 established. **The r8 numbers are superseded**: `client-gui/src` was 33 files / 15,454 total (14,423 non-blank) / 196 `#[test]` declarations at `62c6cf6`; it is now **38 files / 19,416 total (18,151 non-blank) / 264 `#[test]` declarations** (2 of them `#[cfg(not(windows))]` in `client-gui/src/single_instance.rs:280,300`, so 262 compile on Windows; 1 ignored). `client/src` is 122 files / 58,078 total (52,658 non-blank). The reproducible command is in the evidence report.

---

## 1. Decision summary

**D1 — Boundary: an `app` facade module inside `lightspeed-client`, not a new crate.** Unchanged from r8 and still valid at `0df57ce`: the client binary is a target of the same package (`client/Cargo.toml:34-40`), so a crate that depends on `lightspeed-client` cannot be consumed by it without a package split that renames the release artifacts (`.github/workflows/release.yml:421-427`). `client-gui` and the `lightspeed` CLI consume one API at `lightspeed_client::app::{config, commands, status, relay, location, trace, game, error, capabilities, update, history, watchdog, presence}`.

**D2 — What moves out of `client-gui/`: every fact that answers "what is true about the session, route, game, server or config".** Measured target is §3: **23 logic-bearing modules totalling 8,688 total (8,093 non-blank) lines** (r8: 5,413/5,023), plus **~1,393 orchestration/accessor lines** inside `client-gui/src/app.rs` (which is 4,323 lines, up 236). The GUI keeps a thin host wrapper (thread/channel plumbing) for the moved services.

**D3 — One config model; automatic fallback removed.** One `AppConfig` type wraps the existing `client::config::Config` (7 sections / 31 leaf fields, `client/src/config.rs:10-38`) plus `RoutePrefs`/`GamePrefs`/`UiPrefs`. New at this revision: the GUI's `retry_after_fallback` field is **retired** (the warn-only mode has no retry budget); an old `config.toml` still loads (the key is ignored) and the key is not written back (`client-gui/src/config.rs:396-413`). File-location unification stays a later, migration-guarded stage (§6 S5).

**D4 — Drift guard: a ratcheting Rust test in the existing workspace gate**, now with **G6 (location feeds no ranking)** and **G7 (provider-seam ownership)** because the new master introduced two cross-boundary traps (§7).

**D5 — Four new invariants the move must carry** (each is enforced by a test or the guard):

1. **Location is display-only where it is not exact.** The honest ladder (`Cloud → Geofeed → Hosting → Country → Unknown`) may label and caption the card, but only a **cloud-provider region** may enter the relay chooser: `chooser_point` returns `Some` for `Place::Cloud { at: Some(..) }` and `None` for everything else (`client-gui/src/server_place.rs:232-240`), and a test pins that a geofeed entry never steers ranking (`client-gui/src/server_place.rs:456-466`). Operator geofeed labels are attribution/as-of display, never coordinates (`infra/geo/geofeeds/README.md:8-22`; owner decision 2026-11-20).
2. **The provider seam has one owner.** Flow-aware locking lives in the client (`client/src/interceptor/order.rs:131-185`), but the classifier is installed by the GUI's `main` (`client-gui/src/main.rs:81-86`). After extraction, **`AppCore` installs it before the engine starts**, so the CLI gets the same lock quality; the GUI must not be the only front end that does.
3. **Starvation is warn-only and must stay so.** The GUI state machine warns, publishes a history demotion, and never switches the route (`client-gui/src/app/watchdog.rs:180-206,232-274`); a test asserts a confirmed starvation window never switches to direct (`client-gui/src/app/tests.rs:322-360`). The move may not reintroduce automatic fallback.
4. **Relay-measured data does not rank until the experiment passes.** The relay's ICMP `S`, passive app-RTT and TTL trace are validated and pair-bound in the client (`client/src/server_leg.rs:95-146,544-559`) and displayed only; steering requires the pre-registered E1 gate (`docs/design/relay-server-leg-validation.md:26,48-51`).

**Falsifier for D1** (unchanged): if the facade cannot express a migrated rule without (i) a GUI/platform type in its signature, (ii) a Windows-only `cfg` that breaks the Linux/macOS parity obligation, or (iii) leaving a migrated fact reachable only through raw `lightspeed_client::…` calls in `client-gui/`, the boundary is wrong — stop and re-open the crate decision together with the packaging split. New instance of (i): if the location data can only be moved by bundling it into a CLI-hostile dependency, fall back to a front-end-supplied `chooser_point` seam instead of moving the data (same shape as G7's provider seam).

---

## 2. Problem statement, in measured terms

The owner's requirement is unchanged: logic in one place, so the CLI and the GUI cannot drift. At `0df57ce` the rebase **added 5,553 lines of new product logic**, most of it in `client-gui/`, and created three new cross-boundary traps that this document must decide:

1. **The rebase grew the GUI again, not shrank it.** `client-gui/src` gained five modules totalling 2,571 lines before tests (`server_place.rs` 595, `client_trace.rs` 1,326, `server_geofeed_tables.rs` 327, `detection_provider.rs` 184, `server_tables.rs` 139), plus 240 lines in `app/choice.rs`, 300 in `relay_history.rs` and a rewritten `app/watchdog.rs`. The GUI remains a **third logic tree** beside the client library and the client binary (the bin still declares its own 23-module tree, `client/src/main.rs:13-34` plus `mod server_leg` at `:31`).
2. **A core behaviour now depends on a front end installing a classifier.** Flow-aware server locking is core (`client/src/interceptor/order.rs:131-139,171-185`), but the provider classifier is installed only by the GUI (`client-gui/src/main.rs:81-86`), and `client/src/interceptor/mod.rs:81` marks the export `allow(unused_imports)` because "only the GUI (a separate crate) calls it". The CLI installs nothing, so the same engine behaves differently by front end — the exact class of drift O1 exists to stop.
3. **The location ladder is a ranking input with a display-only constraint.** `choose_relay_for_destination` feeds `chooser_point` into the ranking (`client-gui/src/app.rs:880`), and the ladder exists because free geoIP is wrong for hosting ranges (a G-Core US-Virginia server resolved to JP/Tokyo; `client-gui/src/server_place.rs:1-31`). The operator-geofeed part must never reach the chooser (`client-gui/src/server_place.rs:222-240`, test `:456`), while its label/attribution is user-visible. One fact, two consumers with opposite rules — it needs one home with the constraint tested beside it.
4. **Two new measurement surfaces live in the GUI.** `client_trace.rs` runs the OS traceroute against the locked server from the client's vantage (`:1-29`), and the relay-measured server-leg rows read validated reports from the client crate but decide visibility/labels in `client-gui/src/app/choice.rs:191-293`. The former is product logic (a network probe schedule, parse rules and a 30-minute cache), the latter is presentation over an existing core API.
5. **The shipped watchdog is now warn-only, and the contract is stale.** `direct_fallback`, `retry_in` and `server_rejects_relay` copy keys are gone from the locales (`client-gui/locales/en.toml` diff), `retry_after_fallback` is retired (`client-gui/src/config.rs:54-86,398-411`), and the Starvation warning replaced the auto-fallback (`client-gui/src/app/watchdog.rs:66-95,180-274`). `interface.md` still defines Direct fallback / Re-test pending / Server rejects relay (`docs/design/interface.md:36-38`); the spec delta must amend them, and W2-1 cannot be graded against the stale rows.

None of this is a rendering problem. It is fixed by moving the facts to one place, giving the core the ownership of the seams, and making the GUI consume them.

## 3. Inventory (measured at `0df57ce`)

### 3.1 The three surfaces

| Surface | What it is | Metadata | What changed in the rebase |
|---|---|---|---|
| `lightspeed-client` library | engine, capture, tunnel, routing, profiles, registry; public API | `client/Cargo.toml:38-40`; public modules `client/src/lib.rs:9-11,13-14,20-41,48-50,60-63,81`, private `:65-85` | New public **`server_leg`** module (`lib.rs:60-63,81`; `client/src/server_leg.rs`, 1,780 total/1,677 non-blank, 29 tests) with three validated, pair-bound report types and accessors; new `route::destination::tunnelled_samples` (`client/src/route/destination.rs:188,473`); `ScoredRelay` gained `dest_leg_prior_us`/`client_leg_us` and a new sort order (`client/src/route/relay_choice.rs:99-136,212-221`); flow-aware locking in `interceptor/order.rs` (1,410/1,289, 27 tests) with the provider seam re-exported at `client/src/interceptor/mod.rs:81` |
| `lightspeed` CLI binary | same package, own module tree | `client/Cargo.toml:34-36`; `client/src/main.rs:13-34` (now 23 modules incl. `server_leg` `:31`); 1,562 non-blank lines | Compiles the new `server_leg` module and its control-plane ingestion (`client/src/quic/mod.rs:1601-1689`); it still **does not install** the provider classifier (§2.2) |
| `lightspeed-gui` | bin-only crate | `client-gui/Cargo.toml:10-12`; **38 files, 19,416 total (18,151 non-blank)**, 264 `#[test]` declarations (262 on Windows, 1 ignored) | +5 modules, +3,962 total lines; new data blobs `server-cloud.bin` 63,820 B, `server-hosting.bin` 169,320 B, `server-geofeed.bin` 119,480 B (+ sources manifests), 5,553/−789 overall |

The import direction is unchanged and fits D1: `client-gui` depends on `lightspeed-client` (`client-gui/Cargo.toml:15`), so the facade is a library addition, not a dependency rewrite.

### 3.2 Logic-bearing modules in `client-gui/` — ownership verdict at `0df57ce`

`egui` = count of `egui::`/`eframe::`/`Ui`/`Painter` mentions; a logic module with near-zero coupling is pure business code in the GUI crate. Tests are `#[test]` attributes in the file (the `app/` split keeps many in `app/tests.rs`, 3,090 lines / 88 tests). **All `path:line` citations in this document are repo-root relative**; the Module column names the `client-gui/src/` file, and shorthand `:line` refs inside a row are lines of that module.

| # | Module | Lines (total/nb) | Tests | egui | Business behaviour it owns today | Call sites | Verdict |
|---|---|---|---|---|---|---|---|
| F-01 | `config.rs` | 528 / 490 | 16 | 1 | second `config.toml` schema (now **11** fields; `retry_after_fallback` retired), tolerant load/parse/render/save/reset (`:107-302`), discovered-proxy merge (`:302-306`), MTU clamp | `client-gui/src/app.rs:19,505,545,827,3892,3894`; `client-gui/src/main.rs:137`; `client-gui/src/discovery.rs:16` | **MOVE** model+IO; GUI keeps its file path |
| F-05 | `app/watchdog.rs` | 718 / 691 | 0 (14 in `app/tests.rs`) | 2 | **rewritten warn-only starvation state machine**: thresholds (`:3-39`), `RelayStarvationWarning` (`:66-84`), `StarvationVerdict` (`:86-95`), `ReplyFaultStreak` (`:114-147`), `WindowEvent`/`WatchdogPlan` with `switch_to_direct: bool` always false (`:160-206`), `starvation_verdict` (`:208-223`), `plan_return_path_window` (`:225-260`), `apply_starvation_verdict` (`:262-274`), fault judgement (`:276-330`), trace contract (`:332-387`), owner thread (`:534`) | `client-gui/src/app.rs:191,195,1840-1870,1706-1729` | **MOVE** (behaviour frozen); GUI keeps channel wiring |
| F-06 | `relay_history.rs` | 790 / 735 | 24 | 0 | +300 lines: `Record` (`:71-85`), **`starvation_signature`** (`:94-113`), `CarryVerdict {Proven,Starved,Unproven}` (`:136-155`), `record_starvation` (`:218-240`), `carry_verdict` (`:287-311`), legacy `failed` verdict read (`:209-216`), JSON persistence (`:342-388`) | `client-gui/src/app.rs:608,925-928,1721,1777,1784`; `client-gui/src/app/watchdog.rs:29-30` | **MOVE** |
| F-14 | `server_place.rs` | 595 / 566 | 17 | 0 | **honest location ladder**: `Place {Cloud,Geofeed,Hosting,Country,Unknown}` (`:47-94`), blobs included (`:40-43`), `place()` (`:171-204`), `display_point` (`:206-220`), **`chooser_point` display-only for everything but Cloud** (`:222-240`), geofeed text (`:154-169`); the owner/licence/takedown constraints are in its header (`:9-31`) | `client-gui/src/app.rs:880,2225-2226`; `client-gui/src/detection_provider.rs:43`; `client-gui/src/client_trace.rs:609` | **SPLIT**: data+lookup+`chooser_point` MOVE (S5a); place→copy stays GUI |
| — | `server_tables.rs` | 139 / 135 | 0 | 0 | generated cloud regions + hosting providers (`// @generated …` header, `:1-6`) | via `client-gui/src/server_place.rs:38` | **MOVE** with location data (S5a) |
| — | `server_geofeed_tables.rs` | 327 / 323 | 0 | 0 | generated operator-geofeed places/sources; header records the owner decision (no licensed database; display-only; attribution/takedown list) (`:1-13`) | via `client-gui/src/server_place.rs:35-37` | **MOVE** with location data (S5a) |
| F-16 | `client_trace.rs` | 1,326 / 1,219 | 24 | 1 | **client-vantage traceroute**: `TraceHop`/`ClientRoute`/`TraceFailure` (`:56-93`), target sanity (`:98-129`), OS-tool parser+summariser (`:131-249`), worker (`:383-421`), one-run scheduler with 30-min server cache and per-session retry (`:442-599`), display caption (`:624-645`), stable `info_line` log contract (`:649-661`) | `client-gui/src/app.rs:220,1275-1299,4070` | **SPLIT**: scheduler/parser/probe-guard/`info_line` MOVE (S5b); `caption` copy STAY |
| F-15 | `detection_provider.rs` | 184 / 165 | 7 | 0 | the **provider-class bridge** installed into the client: `provider_class` (`:42-44`), namespace-tagged class encoding (`:28-57`), "country/unknown → no opinion" (`:53-56`) | `client-gui/src/main.rs:85` | **MOVE** to core (S5a); installation moves to `AppCore` (G7) |
| — | `app/choice.rs` | 540 / 513 | 0 (tests in `app/tests.rs`) | 0 | +240: measured-leg readers (`:191-293`), expected path (`:295-316`), server-leg state (`:318-362`), cannot-beat-direct (`:380-403`), **starvation filter** `without_starved` (`:405-439`), proven-carry preference (`:441-466`), `FallbackRanking`/`rank_without_destination` (`:468-522`), auto-detect remap (`:524-540`) | `client-gui/src/app.rs:50` glob; `choose_relay_for_destination`, route surface | **MOVE** (rules); text builders STAY |
| F-08 | `app/ledger.rs` (mixed) | 284 / 268 | 0 (12 in `tests.rs`) | 46 | state math: `primary_action` (`:39-57`), `routed_count` (`:155-174`), `history_delta` (`:176-180`), `returned_count` (`:182-196`), `mode_counter` (`:198-213`), `session_delta` (`:232-249`), **`relayed_row_text` starving flag** (`:251-260`) **and** widget helpers (`:262-284`); `is_holding` is **deleted** | `client-gui/src/app.rs:53` glob | **SPLIT**: rules MOVE, widgets STAY |
| F-10 | `app/relay_choice.rs` | 198 / 182 | 0 | 0 | relay-health verdict constants + judge (`:3-16,143-166`), switch safety (`:138-141`), 20 ms switch margin (`:168-170`), env proxies (`:174-198`), `RouteRanking`/`ServerLegState`/`ServerPing` types (`:20-90`) — unchanged at this revision | `client-gui/src/app.rs:56` glob | **MOVE** rules+DTO types |
| — | `app/window.rs` | 198 / 184 | 0 (11 in `tests.rs`) | 6 | +28: **`tray_state_for`** honesty order — error, RelayStarved (only while boosting), Optimizing, Connected, Disconnected (`:130-154`); window geometry/close/race rules (`:7-129,161-197`) | `client-gui/src/app.rs:4097-4102` | **SPLIT**: pure tray/close/race rules MOVE (S3a); tray icon mapping STAY |
| F-07 | `app/presence.rs` | 482 / 444 | 7 | 6 | 5 s presence schedule, Auto-only gate, worker (`:26-234`) — unchanged | `client-gui/src/app.rs:41,55,283,534,1944-2046` | **SPLIT**: policy+schedule MOVE; worker stays |
| F-04 | `race_watch.rs` | 51 / 45 | 4 | 0 | relay-switch hysteresis `decide_switch` (`:15-17`) — unchanged | `client-gui/src/app.rs:27,1424` | **MOVE** |
| — | `app/editor.rs` | 243 / 228 | 0 (4 in `tests.rs`) | 0 | editor draft↔`profiles.toml` mapping, per-field save-rule mirror, built-in collision probes (`:1-243`) — unchanged | `client-gui/src/app.rs:51` glob; editor sheet `:3595` | **MOVE** rules |
| — | `app/games.rs` | 105 / 94 | 0 (3 in `tests.rs`) | 1 | game menu construction from `GAME_REGISTRY`, `PickedGame`, profile-store accessors (`:15-69`), pick rows (`:71-105`) | `client-gui/src/app.rs:52` glob | **SPLIT**: registry accessor MOVE; rows STAY |
| — | `app/picker.rs` | 98 / 91 | 0 (4 in `tests.rs`) | 0 | picker status derivation (`:1-91`) | `client-gui/src/app.rs:54` glob | **STAY** |
| F-11 | `geo.rs` + `geo_countries.rs` | 108/94 + 243/242 | 5 | 0 | offline IPv4→country lookup over the 1.8 MB DB-IP blob (`client-gui/src/geo.rs:16,33-60`) — now only the **last rung** of the server ladder | `client-gui/src/server_place.rs:200` | **MOVE** behind a feature (§6 S5a) |
| — | `globe.rs` | 197 / 178 | 4 | 26 | relay-coordinate table (`:20-36`) for the map | `client-gui/src/app.rs:2198-2282` | **SPLIT**: table DELETE (use core `relay_position`), drawing STAY |
| F-12 | `update.rs` + `self_update.rs` | 322/285 + 213/191 | 10 + 3 | 5 + 13 | semver compare (`client-gui/src/update.rs:46-61`), receipt/GitHub fallback (`:92-201`), status line (`:234`), checksum parsing (`client-gui/src/self_update.rs:111-119`), elevated install (`:27-89`) | `client-gui/src/app.rs:28,373,1537-1555,3905-3987` | **SPLIT**: pure rules MOVE; network/elevation STAY |
| F-18 | `main.rs` (GUI) | 258 / 237 | 0 | 14 | composition root: module list (`:8-29`), **provider install** (`:81-86`), rustls pin, runtime/engine, eframe, viewport | — | **STAY** as composition root; the provider install call moves into `AppCore` (G7) |
| — | `client-gui/src/app.rs` | 4,323 / 4,120 | 0 (88 in `app/tests.rs`) | 526 | orchestration + drawing; new: `RouteLocationLines`/`route_location_lines` (`:411-489`), `holding` from the engine gate (`:247,1845-1848`), new facts and rows (§3.4) | — | **SPLIT** per §3.4/§3.2.1 |
| — | `app/tests.rs` | 3,090 / 2,903 | 88 | 61 | tests travel with their modules | — | **MOVE** with modules |
| — | `processes.rs` | 282 / 260 | 6 | 1 | worker wrapper + filter/path-tail, already delegating to `client/src/processes.rs:48` | `client-gui/src/app.rs:26,781,790` | **STAY** (the sanctioned adapter shape) |
| — | `paths.rs` | 81 / 69 | 3 | 0 | second config/data-dir resolver (`:12-43`) | `client-gui/src/main.rs:137`, `client-gui/src/app.rs:505,608,827,1721,1784,3892` | **SPLIT**: config-dir part MOVE; log/crash/open helpers STAY |
| — | `platform/windows.rs` | 322 / 293 | 0 | 9 | +7: **`TrayState::RelayStarved` icon/tooltip** (`:137-155`) | — | **STAY** (platform integration) |
| — | `i18n.rs`, `design.rs`, `crash.rs`, `single_instance.rs`, `platform/{mod,linux,macos}.rs`, `app/{theme,widgets}.rs` | measured in §3.1 evidence | — | — | presentation/platform | — | **STAY** |

### 3.2.1 Verdict → executing stage → W2 slice (the cut key for the spec's §5 scopes)

The spec's slices are W2-1 = S0+S1+S2+S3a (+ the DTO gaps), W2-2 = the shell, W2-3 = S3b+S4+S5+S6, W2-4 = parity. This table is what the Lead re-cuts those scopes from (QA B1/R1, re-cut for the delta).

| Module | Verdict | Stage | W2 slice | Scope note |
|---|---|---|---|---|
| `config.rs` | MOVE model+IO | S1 | W2-1 | GUI keeps only its file path; retired `retry_after_fallback` key is tolerated, never re-written |
| `discovery.rs` | MOVE blocking/pure; spawn wrappers STAY | S2 | W2-1 | the one-shot `mpsc` wrappers stay in `client-gui` |
| `app/choice.rs` | MOVE rules | S3a | W2-1 | new: starvation filter, proven-carry preference, fallback ranking, measured-leg readers; text builders STAY |
| `app/ledger.rs` — state rules | SPLIT: MOVE (`primary_action`, counters, session delta, starving flag) | S3a | W2-1 | `is_holding` is deleted; widgets STAY to W2-2 |
| `app/window.rs` — tray state | SPLIT: MOVE `tray_state_for` | S3a | W2-1 | icon/tooltip mapping STAY (W2-2) |
| `app/relay_choice.rs` | MOVE (types S3a; health verdict + margin S4) | S3a+S4 | W2-1 types / W2-3 rules | no GUI rule left after S4 |
| `app/watchdog.rs` | MOVE (warn-only, frozen) | S4 | W2-3 | behaviour freeze: warning only, never a switch (`:189-191,249-253`) |
| `relay_history.rs` | MOVE | S4 | W2-3 | new `CarryVerdict`/starvation signature; file format backward-compatible (`:336-388`) |
| `race_watch.rs` | MOVE | S4 | W2-3 | — |
| `app/presence.rs` | SPLIT: policy+schedule MOVE | S5 | W2-3 | worker STAY (or move only if CLI adopts it, Q4) |
| `app/editor.rs` | MOVE rules | S5 | W2-3 | violation enum; i18n mapping STAY |
| `app/games.rs` | SPLIT: registry accessor MOVE | S5 | W2-3 | row construction STAY |
| `server_place.rs` + `server_tables.rs` + `server_geofeed_tables.rs` | SPLIT: ladder/data/`chooser_point` MOVE; place→copy STAY | S5a (new) | W2-3 | feature-gate the data: `server-location` (three server indexes) and `geo-ip` (country rung) — generators/owner in §3.2.2; display-only constraint test travels (`client-gui/src/server_place.rs:456-466`) |
| `detection_provider.rs` | MOVE classifier to core | S5a | W2-3 | installation moves to `AppCore` (G7); GUI `client-gui/src/main.rs:81-86` call deleted |
| `client_trace.rs` | SPLIT: scheduler/parser/guard/`info_line` MOVE; `caption` STAY | S5b (new) | W2-3 | OS-tool execution is core-side app logic (no GUI type in signatures) |
| `geo.rs` + `geo_countries.rs` | MOVE behind feature `geo-ip` | S5a | W2-3 | now the ladder's last rung; `geo-ip` gates only the DB-IP blob/table — generators/owner in §3.2.2, asset move in Q11 |
| `update.rs` + `self_update.rs` | SPLIT: pure rules MOVE | S5 | W2-3 | network/elevation STAY; QA R10 |
| `paths.rs` | SPLIT: config-dir MOVE | S5 | W2-3 | `paths.rs` must be in W2-3's scope; log/crash/open STAY |
| `globe.rs` | SPLIT: table DELETE (core `relay_position`) | S2 export | W2-2 edit | QA R1 keeps `globe.rs` in W2-2's scope |
| `client-gui/src/app.rs` | getters/orchestration over S1–S5; `ui()` untouched | S1–S5 | W2-1 getters / W2-2 `ui()` / W2-3 orchestration | never two writers (R7 / spec C-10) |
| `main.rs` (GUI) | STAY (composition root); provider install moves | S5a | W2-3 | after S5a the GUI `main` has no detection-provider call |
| `app/tests.rs` | tests travel with their modules | S1–S5 | W2-1 / W2-2 / W2-3 | required in each slice's scope (QA R1) |
| `processes.rs`, `app/picker.rs`, `i18n.rs`, `design.rs`, `crash.rs`, `single_instance.rs`, `platform/*`, `app/{theme,widgets}.rs` | STAY | — | W2-2 / W2-4 | adapter allowlist (§3.3) |

**Delta against the spec's current §5 write scopes** (actionable re-cut): W2-1 must also own `client-gui/src/main.rs:137`, `app/{choice,ledger,relay_choice,window}.rs` (rules), `app/tests.rs` and the new `app/route_surface.rs`; W2-2 must add `globe.rs`, `app/tests.rs` and the new location copy/tray-starved mapping; **W2-3 grows materially** — it must add `client-gui/src/{server_place,server_tables,server_geofeed_tables,detection_provider,client_trace}.rs`, `client-gui/data/server-*.bin` + `server-*.sources.json`, **both** location-data generators (`infra/scripts/build-server-location.py` for cloud/hosting and **`infra/scripts/build-server-city.py` for the geofeed set**, §3.2.2), `infra/geo/geofeeds/**`, `client-gui/src/paths.rs`, and the `server-location`/`geo-ip` feature wiring (Q12 proposes splitting W2-3).

### 3.2.2 Location data — generators, feature gates, owner (QA F9)

The ladder is fed by three committed data sets with different build pipelines. They must be named separately so W2-3b regenerates/moves the right artifacts and each feature gate has one meaning.

| Committed artifact | Generated by | Feature gate | Data owner |
|---|---|---|---|
| `client-gui/data/server-cloud.bin` + `server-hosting.bin` + `client-gui/src/server_tables.rs` | `infra/scripts/build-server-location.py` (header `:1-14`: cloud + hosting are the only artifacts this script builds) | `server-location` | `infra-dev` |
| `client-gui/data/server-geofeed.bin` + `client-gui/src/server_geofeed_tables.rs` + `client-gui/data/server-geofeed.sources.json` | **`infra/scripts/build-server-city.py:54-69`** (OUTPUTS; `--ship` writes the three committed copies at `:65-68`; inputs configured in `infra/geo/geofeeds/**`, `README.md:3-4`) | `server-location` | `infra-dev` |
| `client-gui/data/geo-ipv4.bin` + `client-gui/src/geo_countries.rs` | `infra/scripts/build-geo-ipv4.py:38-39` (DB-IP Lite) | **`geo-ip`** | `infra-dev` |

- **Feature ownership, one meaning each:** `server-location` gates the three server indexes + their Rust tables/manifests and the moved `app::location` module; `geo-ip` gates **only** the country blob/table behind `geo.rs`, the ladder's last rung (with `geo-ip` off the ladder ends at `Place::Unknown`). The GUI enables both; `full` should include both for CI coverage while the shipped CLI defaults off (R4).
- **One owner for the move:** `rust-dev` owns W2-3b (module move + the two feature gates in `client/Cargo.toml`); `infra-dev` owns the generators and committed artifacts above; `dev-ops` only wires CI. A regeneration is requested from `infra-dev` with the run's source manifest hashes; the shipped tree never calls a network service (`infra/scripts/build-server-location.py:1-15`).

### 3.3 What may stay in `client-gui/` (the adapter allowlist)

Per brief §7.2 and the skill §5, the GUI keeps: rendering and the `client-gui/src/app.rs` draw half; tokens (`design.rs`); i18n lookup (`i18n.rs`); platform integration (`platform/*`, `single_instance.rs`, `crash.rs`, `paths::{log_file,crash_log,open_in_os}`, tray icon/tooltip mapping); view state (sheets, scroll, pickers, selection indices); **presentation copy over DTO facts** — including the location block (`RouteLocationLines`/`route_location_lines`, `client-gui/src/app.rs:411-489`), the geofeed attribution line, the server-leg/route-trace row labels and the starvation banner wording; and **thread/channel plumbing** — the one-shot `mpsc` wrappers already used at `client-gui/src/processes.rs:36-51`. None of those decides a route, a location point or a relay order.

### 3.4 `client-gui/src/app.rs` line map at `0df57ce`

4,323 total lines: 1–490 header/consts/`TrayState` (now incl. `RelayStarved` at `:79`)/state enums/`HealthProbe`/`WinnerProbe`/the struct (183)/the new `RouteLocationLines`+`route_location_lines` presentation helpers (`:411-489`); `impl` begins 491, `new()` 492–678; accessors + orchestration run **679–2,071 (~1,393 lines)**; drawing runs **2,072–4,323 (~2,252 lines)**. **72 methods** (r8: 71), including the new `locked_server_ip` `:1257`, `poll_client_trace` `:1275-1299`, `record_starvation_verdict` `:1706-1729`, `relay_measured_server_leg_rows` `:2490-2501`, `relay_measured_app_server_leg_row` `:2512-2520`, `relay_measured_trace_row` `:2531-2539`; the tray state is computed at `:4097-4102` from `tray_state_for(has_error, warning_active, boosting, connected)`. The new fields are `relay_starved` `:191`, `relay_warning` `:195`, `client_trace` `:220`, `client_route` `:224`, `holding` `:247`, `starvation_warning` `:251`.

### 3.5 `lightspeed-client` — what the GUI can already use, and the gaps

| Need | Existing client surface | Evidence |
|---|---|---|
| Engine lifecycle + status | `LightSpeedEngine` (22 public methods) + `EngineStatus` (67 fields) + `InterceptorHealth` (10 fields) | `client/src/engine.rs:27-56,60-226,299-1491`; `client/src/lib.rs:13` |
| Relay-measured server leg (new) | `ServerLegReport {median_us, jitter_us, samples, age, validated}`, `ServerLegAppReport {passive_median_us, samples, age, floor_ok}`, `ServerTraceReport {hops, last_hop, last_hop_rtt_us, complete, age}`; accessors `server_leg_report`, `server_leg_app_report`, `server_leg_trace`; `server_leg_trace_log_line` for the log contract | `client/src/server_leg.rs:95-113,129-146,544-559,247,441,724`; `client/src/lib.rs:60-63,81` |
| Relay-report ingestion | control-plane reads `ServerLeg`/`ServerLegApp`/`ServerTrace` on server-initiated streams after advertising `caps::SERVER_LEG` | `protocol/src/control.rs:303-399`; `client/src/quic/mod.rs:587,1365,1601-1689` |
| Destination-aware ranking | `ScoredRelay {relay, total_leg_us, dest_leg_us, dest_leg_prior_us, client_leg_us, dest_source, jitter_us}`, `RelayCandidate`, `DestLegSource`, `rank/rank_with_direct/rank_at/rank_at_with_direct/best/best_at`, `relay_position` | `client/src/route/relay_choice.rs:78-156,212-221,234,304,554-572`; `client/src/lib.rs:20-33` |
| Estimator + trace sample count | `destination_estimator()`, `estimate`, `measurement_age`, `tunnelled_samples` | `client/src/route/destination.rs:188,425-480`; `client/src/lib.rs:38-41` |
| Flow-aware locking + provider seam (new) | `FlowOwner`, `ProviderClass`, `ProviderLookup`, `install_server_provider_lookup`, `server_provider_class`, `is_lockable_destination` | `client/src/interceptor/order.rs:131-185,197`; re-export `client/src/interceptor/mod.rs:81` |
| Registry discovery | `discover_relays` → `RelayInfo {node_id, addr, region}` | `client/src/registry.rs:78-89,196-229`; `client/src/lib.rs:14` |
| Games / profiles / processes / latency / paths / telemetry | unchanged from r8 (§3.5 there) | `client/src/lib.rs:60-63,70,73-76,79,83` |

The four r8 gaps remain (this revision adds a fifth): **`config` private** (`lib.rs:68`), **`error` private** (`:69`), **`session` private** but `current_relay()` exposed (`:48-50`), **no `Status`/command facade** (no `struct Status` in `client/src`; the CLI `--status` is ad-hoc printing at `client/src/main.rs:732-865` originally — see §5.8), and **new: the provider seam has no owner** — the classifier is `fn` pointer installed by a front end, so "no install" silently degrades locking (`client/src/interceptor/order.rs:178-185`).

### 3.6 Duplicate/cross-boundary findings at this revision

| ID | Drift | Side A | Side B | Effect today |
|---|---|---|---|---|
| F-01 | Two config schemas + two files (unchanged) | `client/src/config.rs:10-38` (31 leaf fields) | `client-gui/src/config.rs:54-86` (11 fields; `retry_after_fallback` retired) at `%APPDATA%\light-speed\config.toml` (`client-gui/src/paths.rs:36-43`) | CLI-tuned `lightspeed.toml` still cannot reach the GUI; two MTU clamps |
| F-02 | Two probe implementations, one ranking (unchanged) | CLI UDP keepalive (`client/src/modes/proxy_probe.rs:25-75`) | GUI TCP connect (`client-gui/src/discovery.rs:232-295,348-354`) | same ranking, different "ping" semantics |
| F-03 | Relay identity: registry `region` dropped, display label put into `RelayCandidate.region` | registry `region` (`client/src/registry.rs:86-88`); `discover_relays` "for embedders (e.g. the GUI)" (`:196-209`) | GUI `RelayInfo{node_id,addr}` (`client-gui/src/discovery.rs:34-37`), label into region (`client-gui/src/app.rs:902-910`) | masked by `node_id`; still a wrong-type seam |
| F-04 | Two switch-hysteresis rules (unchanged) | `(current/5).max(10_000)` µs (`client/src/modes/reroute.rs:21-29`) | 20 ms absolute (`client-gui/src/app/relay_choice.rs:168-170`, `client-gui/src/race_watch.rs:15-17`) | CLI and GUI disagree on "clearly better" |
| F-05 | **Warn-only starvation state machine lives in the GUI** | engine publishes only raw counters (`client/src/engine.rs:26,1018`) | `client-gui/src/app/watchdog.rs` (fault windows, warnings, history verdict) | CLI has no warning/demotion; a GUI-only product behaviour |
| F-06 | **Starvation/carry memory GUI-only (+300 lines)** | — | `client-gui/src/relay_history.rs` persisted to `relay-history.json` | front ends learn nothing from each other |
| F-07 | Game-detection contract gap (unchanged; latent Java→Bedrock remap) | `games::auto_detect` returns a display name (`client/src/games/mod.rs:513-517`) | GUI fuzzy remap (`client-gui/src/app/choice.rs:524-540`) | latent mis-map, by inspection |
| F-08 | No shared status DTO; GUI derives the contract | no `Status` type | `client-gui/src/app.rs` derivations + `app/choice.rs`, `app/ledger.rs`, `app/window.rs` | "one driving fact" enforced only by convention |
| F-09 | CLI binary duplicates the module tree | 23 modules `client/src/main.rs:13-34` | library `client/src/lib.rs:9-14,60-85` | structural reason for D1 |
| F-10 | Two presence policies (unchanged) | GUI 5 s Auto-only (`client-gui/src/app/presence.rs:26-45`) | CLI `--watch` 2 s/3 s auto-start (`client/src/modes/watch_mode.rs:60-158`) | parity decision open (Q7) |
| F-11 | Geo/position split (now a ladder) | GUI owns all three offline indexes + country blob (`client-gui/src/server_place.rs:35-43`) | engine owns relay positions (`relay_choice.rs:554-572`) and region prior (`client/src/route/regions.rs:36-207`) | ladder is a ranking input via `chooser_point`; display-only must be guaranteed |
| F-12 | Update check GUI-only deps (unchanged) | — | `update.rs`/`self_update.rs` + `axoupdater`/`reqwest`/`sha2` (`client-gui/Cargo.toml:20,38,40`); disclosure docs `docs/privacy.md:184-202`, `docs/data-dictionary.md` | pure rules shareable; network/elevation GUI |
| F-13 | Second config-dir resolver + stale doc claim (unchanged) | `client/src/paths.rs:48-86` | `client-gui/src/paths.rs:12-43` | two path implementations |
| **F-14** | **Location ladder is a GUI-owned ranking input under a display-only rule** | display copy/story lives in GUI (`client-gui/src/app.rs:411-489`) | data+lookup+`chooser_point` also GUI (`server_place.rs`), and the chooser consumes it (`client-gui/src/app.rs:880`); only Cloud may return a point, geofeed test `client-gui/src/server_place.rs:456` | one fact with two consumers and opposite rules; no core home today |
| **F-15** | **Provider seam ownership inversion** | core locking consults an installed classifier (`client/src/interceptor/order.rs:131-185`) and documents "only the GUI calls it" (`client/src/interceptor/mod.rs:77-81`) | GUI installs it (`client-gui/src/main.rs:81-86`); CLI installs nothing | same engine, different locking quality by front end |
| **F-16** | **Client traceroute (a network probe) lives in the GUI** | `client_trace.rs` (1,326 lines) + `client-gui/src/app.rs:220,1275-1299,4070` | — | product measurement, not presentation; privacy/docs coverage is thin (§8 R14) |
| **F-17** | **Relay-measured leg display vs core validation** | core validates/pair-binds reports (`server_leg.rs:1-50,247,441,724`) | GUI decides rows/visibility/labels (`client-gui/src/app/choice.rs:191-293`; `client-gui/src/app.rs:2490-2539`) | must stay display-only until E1 (`relay-server-leg-validation.md:26,48-51`) |
| **F-18** | **Contract is stale vs shipped warn-only mode** | `interface.md:36-38` still defines Direct fallback/Re-test pending/Server rejects relay | code removed auto-fallback and its copy; `retry_after_fallback` retired; warning + history demotion replace it (`app/watchdog.rs`, `client-gui/src/relay_history.rs:218-240`) | W2-1 (b) false acceptance until the spec delta amends the rows (R17) |
| **F-19** | **Update-check disclosure is documentation-only truth** | user-initiated check exists (`client-gui/src/update.rs:92-201`) | disclosure lives in `docs/privacy.md:184-202` + data dictionary; GUI shows only the button/hover | the DTO must not add a background-check state; the disclosure is a product contract |
| **F-20** | **app.rs grew, not shrank** | 4,087 → 4,323 (+236) at `62c6cf6..HEAD`, including new presentation helpers (location) and orchestration (trace/starvation polls) | extraction plan (r8) unchanged | reinforces staged extraction; do not add new logic to `client-gui/src/app.rs` |

## 4. Boundary decision (unchanged, with new seams)

### 4.1 Option A — new `lightspeed-app-core` crate (still rejected)

The r8 argument holds verbatim at `0df57ce`: the CLI is a target of the `lightspeed-client` package (`client/Cargo.toml:34-40`; `cargo metadata` at r8 showed one package with `lightspeed_client:lib` + `lightspeed:bin`), so a facade crate that owns session commands cannot be consumed by it without a package split that renames the release artifacts (`.github/workflows/release.yml:421-427`, `dist/scoop`, `dist/chocolatey`, `dist/aur`). The new `server_leg` public module is a further reason a facade *inside* the crate is simpler: it reuses `pub(crate)` modules and needs no new crate-level API commitment.

### 4.2 Option B — `lightspeed_client::app` facade module (chosen, extended)

The r8 module layout stands (`config`, `commands`, `dto`, `discovery`, `relay`, `history`, `watchdog`, `presence`, `game`, `update`, `geo`), with these additions required by the rebase:

```
src/app/
  location.rs   Place + ladder + server tables (moved from server_place/server_tables/
                server_geofeed_tables, feature `server-location`), chooser_point + display_point,
                provider_class (from detection_provider), attribution/as-of provenance
  trace.rs      client-vantage traceroute scheduler/parser (from client_trace), info_line log contract
  server_leg.rs thin accessors over the client's server_leg reports for the DTO (no new storage)
  commands.rs   + installs the provider classifier before engine start (G7)
```

Rules added at this revision (on top of r8's one-direction / UI-free / cross-platform / DTO-cheap / naming rules):

- **Seam ownership:** the provider classifier is installed by `AppCore` (once, before any engine task) from `app::location::provider_class`; a front end never installs it (`client-gui/src/main.rs:81-86` call is deleted in S5a). The core owns the obligation; a missing install becomes a core bug, not a front-end choice.
- **Display-only boundary:** `app::location` exposes two functions with different authority — `place()`/display provenance for copy, and `chooser_point()` for ranking. Only `chooser_point` may be passed into `rank_*` (`client-gui/src/app.rs:880` today); the geofeed rule is tested (`client-gui/src/server_place.rs:456-466`) and guarded (G6).
- **Non-steering measurement:** relay-measured reports are exposed for display only; no ranking call may consume them before the E1 gate (`relay-server-leg-validation.md:48-51`). The facade exposes them as `AppStatus` facts, not as ranking inputs.
- **Warn-only:** the watchdog move is behaviour-frozen; `WatchdogPlan::switch_to_direct` stays `false` (`client-gui/src/app/watchdog.rs:180-206`) and the test that proves it moves with it (`client-gui/src/app/tests.rs:322-360`).

### 4.3 Option C/D — unchanged

Option C (pure core below `client`) remains the deferred fallback; Option D (status quo) remains rejected — O1 is a program requirement, and the rebase added three more duplicates (§3.6 F-14/F-15/F-16).

### 4.4 Falsifier (updated)

The r8 falsifier table (F-A…F-E) stands. Two new triggers:

| # | Falsifier | Check | Response |
|---|---|---|---|
| F-F | Moving the location data into the client crate makes the CLI binary grow past an acceptable budget, or the licence/attribution constraints cannot travel with the data | Measure artifact delta in S5a; `infra/geo/geofeeds/README.md:8-22,190-199` must be followed | Feature-gate the data (preferred) or keep the data GUI-side and move only the classifier/`chooser_point` decision, with the GUI passing its table through the G7 seam (same shape as the provider) |
| F-G | The provider seam cannot be installed by `AppCore` without a GUI type or a startup ordering hazard | Compile + S5a tests; `install_server_provider_lookup` call must exist in `client/src/app/**` and nowhere in `client-gui/src` after S5a (G7) | If ordering is the blocker, install it in `AppCore::new` before `LightSpeedEngine::new` (the GUI does it before the engine today, `client-gui/src/main.rs:81-96`) |

## 5. The shared API (DTO sketch)

**Everything in this section is proposed for W2.** No item exists at `0df57ce`; each row names the real field/type/function it carries, so the implementer is not asked to invent behaviour. The names the spec cites are kept (`AppStatus.*`, `RelayStatus.connecting`, `SessionCommandState.*`, `WatchdogStatus`, `Capabilities.elevated`, `AppError`); the **new** names below are proposed for the spec delta (A7–A11) and marked as such.

### 5.0 Contract amendments — A1–A6 recorded, A7–A11 needed for the delta

A1–A6 are recorded in the spec's Appendix C (C-5…C-7, C-13…C-15) and Q3; the DTO mirror is unchanged from r8 and the code at `0df57ce` still matches it, with one update: **A1's note "`is_holding` remains only as the fallback-hold variant" is superseded by the code** — `is_holding` is deleted and the Holding fact is exactly `EngineStatus.tunnelling_enabled == Some(false)` while a session runs (`client-gui/src/app.rs:247,1845-1848`).

| Amendment | Contract row | Single DTO home | Status at `0df57ce` |
|---|---|---|---|
| **A1** Holding fact = engine tunnel gate | Holding (`interface.md:33`) | `AppStatus.engine.tunnelling_enabled == Some(false)` with `AppStatus.interceptor` present | code matches (`client-gui/src/app.rs:1845-1848`); `is_holding` deleted |
| **A2** Idle splits NOT CONNECTED/CONNECTED | Idle (`:31`) | `AppStatus.engine.connected` selects the word; one `RouteState::Idle` | unchanged |
| **A3** Error taxonomy + verbatim details | Error (`:41`) | `AppStatus.error: Option<AppError>` + `AppError.Kind`; inputs are `SessionCommandState.last_error`, `ConfigRecovery`, engine `*_error`/`registration_error` | unchanged |
| **A4** Choosing is a sub-state of HOLDING | Choosing (`:34`) | `AppStatus.relay.choosing: bool`; rail word `RouteState::Holding` | unchanged |
| **A5** Relaying = boosting gate + one counter pair | Relaying (`:35`) | `AppStatus.boosting` (run_boosting gate) + `AppStatus.relayed: Option<RelayedPair>` | unchanged (`client-gui/src/app.rs:2645-2650`) |
| **A6** Needs elevation = overlay | Needs elevation (`:40`) | `Capabilities.elevated` only; no route variant | unchanged |
| **A7 (new, provisional)** Warn-only starvation replaces the auto-fallback rows | Direct fallback (`:36`), Re-test pending (`:37`), Server rejects relay (`:38`) | `AppStatus.watchdog.warning: Option<RelayStarvationWarning>` + `AppStatus.watchdog.carry: Option<CarryVerdict>`; no direct-switch, no retry clock. The three old rows become one Warning overlay plus a history demotion for the next ranking | shipped in `app/watchdog.rs`, `relay_history.rs`; interface rows not yet amended (R17) |
| **A8 (new, provisional)** Honest location ladder is display-only | new surface (route card location) | `AppStatus.location: LocationStatus { place: Place, display_point: Option<(f32,f32)>, source: Option<GeofeedSource> }`; ranking may only use `chooser_point` (Cloud-exact) | shipped in `server_place.rs`; owner decision `infra/geo/geofeeds/README.md:8-22` |
| **A9 (new, provisional)** Relay-measured server leg is display-only until E1 | server-leg rows | `AppStatus.server_leg: ServerLegStatus { icmp: Option<ServerLegReport>, app: Option<ServerLegAppReport>, trace: Option<ServerTraceReport>, expected_path_ms: Option<f64> }` | shipped in `server_leg.rs` + `client-gui/src/app/choice.rs:191-316`; E1 gate `relay-server-leg-validation.md:48-51` |
| **A10 (new, provisional)** Route trace / placement anchor row | new surface | `AppStatus.route_trace: Option<ClientRoute>`; the relay's own trace is `ServerTraceReport` inside A9. Neither is a city or a ping | shipped in `client_trace.rs` + `client-gui/src/app.rs:220,1275-1299` |
| **A11 (new, provisional)** Update-check disclosure is a product contract | update sheet/settings | `UpdateStatus {current, latest, update_available, installer}` produced **only** by a user-initiated `app::update::check()`; no startup/background check exists | shipped (`client-gui/src/update.rs:92-201`); documented `docs/privacy.md:184-202`, `docs/data-dictionary.md` |

Authority note: A1–A6 are authorised by the spec's Appendix C/Q3; **A7–A11 are this document's proposal for the spec delta** and do not edit `interface.md` (write scope). The spec owner must draft them and W2-2 applies them (Q10).

### 5.1 Config model (load/save)

| Proposed | Carries / replaces | Real source |
|---|---|---|
| `AppConfig { client: Config, route: RoutePrefs, game: GamePrefs, ui: UiPrefs }` | one persisted model | `Config` (`client/src/config.rs:10-38`); GUI `GuiConfig` (`client-gui/src/config.rs:54-86`) |
| `AppConfig::load(path) -> Result<Self, AppError>` / `load_or_default(path) -> LoadedAppConfig { config, recovery }` | CLI hard-fail then default; GUI tolerant load; recovery diagnostic for the Error row | `client/src/main.rs:401-404`; `client-gui/src/config.rs:107-124` |
| `ConfigRecovery { path, reason }` | the tolerant-loader diagnostic the GUI logs and discards | `client-gui/src/config.rs:107-124` |
| `RoutePrefs { selected, proxies, auto_select }` (no `retry_after_fallback`) | `GuiConfig::{selected, proxies, auto_select}` (one field **retired**) | `client-gui/src/config.rs:54-86,398-411` |
| `GamePrefs { game_auto, selected_game, manual_mode, manual_server, manual_port_range }` | the five GUI fields | `client-gui/src/config.rs:54-86` |
| `UiPrefs { language }` | GUI-only | `client-gui/src/config.rs:54-86` |

Collapse rules from r8 stand (`share_latency_stats` → `general.telemetry`; `tunnel_mtu` → `tunnel.mtu`). New: the retired `retry_after_fallback` must be **ignored on load and never written** (the GUI already test-pins this; the facade must keep the legacy reader).

### 5.2 Commands (session start/stop/select)

The r8 table stands (SelectRelay/Disconnect/StartBoost/StopBoost/SetPathMtu/Hold + `SessionCommandState`). New command-adjacent facts:

| Proposed | Replaces / calls | Real source |
|---|---|---|
| `AppCore::new(..)` installs `app::location::provider_class` via `install_server_provider_lookup` before the engine starts | the GUI's `main` install | `client-gui/src/main.rs:81-86`; `client/src/interceptor/order.rs:171-176` |
| `SessionCommandState { in_flight, last_error }` (unchanged) | action loading + start-refusal error | spec §2.1; `client-gui/src/app.rs:1578` (today only a log) |
| `app::update::check()` (user-initiated; not a `SessionCommand`) | the Settings/update-sheet call | `client-gui/src/app.rs:1537-1555,3905-3987`; `client-gui/src/update.rs:92-201` |

### 5.3 Status snapshot — the expanded DTO

| Proposed | Carries | Real source |
|---|---|---|
| `AppStatus { engine, interceptor, relay, route, watchdog, presence, session, boosting, relayed, error, location, server_leg, route_trace, tray }` | one snapshot per frame | `EngineStatus` 67 fields (`client/src/engine.rs:60-226`); `InterceptorHealth` 10 (`:27-56`); `snapshot`/`interceptor_health` (`:1395,1019`); `session`/`boosting`/`relayed`/`error` per r8; **new: `location`, `server_leg`, `route_trace`, `tray`** |
| `RelayStatus { relay, ranked, server, server_ping, server_leg: ServerLegState, connecting, choosing, cannot_beat_direct, starving: Option<RelayStarvationWarning>, carry: Option<CarryVerdict>, fallback: Option<FallbackRanking> }` | route + starvation facts | `ScoredRelay` (`relay_choice.rs:99-136`); `ServerPing`/`ServerLegState` (`client-gui/src/app/relay_choice.rs:40-90`); `RelayStarvationWarning` (`client-gui/src/app/watchdog.rs:66-84`); `CarryVerdict` (`client-gui/src/relay_history.rs:136-155`); `FallbackRanking` (`client-gui/src/app/choice.rs:468-478`) |
| `WatchdogStatus { warning: Option<RelayStarvationWarning>, streak: ReplyFaultStreak, event: WindowEvent }` | the fallback chain states, now **warn-only** (no `retest`, no `rejection`, no `retry_in`) | `client-gui/src/app/watchdog.rs:66-147,160-206` |
| `RelayedPair { sent, returned }` + `tray: TrayState` (projection; `RelayStarved` exists) | one counter pair; honesty order of the tray | `client-gui/src/app/ledger.rs:251-260`; `client-gui/src/app/window.rs:130-159`; `client-gui/src/app.rs:4097-4102` |
| `LocationStatus { place: Place, display_point: Option<(f32,f32)>, geofeed: Option<GeofeedProvenance { provider, country, region, city, source, as_of }>, chooser_point: Option<(f64,f64)> }` | the honest ladder; `display_point` and `chooser_point` are deliberately different (the latter Cloud-only) | `client-gui/src/server_place.rs:47-94,171-240`; copy stays GUI (`client-gui/src/app.rs:411-489`) |
| `ServerLegStatus { icmp: Option<ServerLegReport>, app: Option<ServerLegAppReport>, trace: Option<ServerTraceReport>, expected_path_ms: Option<f64> }` | relay-measured display rows; pair-bound and validated in core | `client/src/server_leg.rs:95-146,544-559`; `client-gui/src/app/choice.rs:191-316`; rows `client-gui/src/app.rs:2490-2539` |
| `route_trace: Option<ClientRoute { hops, last_hop, last_hop_rtt_ms, complete }>` | the client-vantage trace row / placement anchor | `client-gui/src/client_trace.rs:56-78`; `client-gui/src/app.rs:220,1275-1299` |
| `AppStatus.error: Option<AppError>` (A3) | taxonomy + verbatim detail | as r8 |

**Contract-row → single field path** (the table acceptance (b) must use after the delta):

| Contract row (`interface.md`) | Single field path | Amendment / note |
|---|---|---|
| Idle (`:31`) | `AppStatus.engine.connected` | A2 |
| Connecting (`:32`) | `AppStatus.relay.connecting` | DTO gap, W2-1 |
| Holding (`:33`) | `AppStatus.engine.tunnelling_enabled == Some(false)` with `interceptor` present | A1; `is_holding` deleted |
| Choosing (`:34`) | `AppStatus.relay.choosing` | A4 |
| Relaying (`:35`) | `AppStatus.boosting`; display `AppStatus.relayed` | A5 |
| Direct fallback (`:36`) — **redefined by A7** | `AppStatus.watchdog.warning` (fault + relay + server) | no direct switch, no retry clock |
| Re-test pending (`:37`) — **redefined by A7** | `AppStatus.watchdog.warning` + `AppStatus.relay.carry` (next-ranking demotion) | retry/re-test removed with the auto-fallback |
| Server rejects relay (`:38`) — **redefined by A7** | `AppStatus.relay.carry == Starved` / starvation signature | history memory, not a session action |
| Relay recovered (`:39`) | **no word** — the warning clearing is the observable effect (A7), not a route state | the recovery sentence itself remains trace-only; there is no recovered banner or rail word |
| Needs elevation (`:40`) | `Capabilities.elevated` | A6 |
| Error (`:41`) | `AppStatus.error` | A3 |

**New non-route surfaces the window now shows** (each is output of the DTO, not a new route word):

| Surface | DTO output | Real source |
|---|---|---|
| Server-location block (caption/detail/note/hint/source) | `AppStatus.location` | `client-gui/src/server_place.rs:47-94,154-169`; copy `client-gui/src/app.rs:411-489` |
| Relay-measured `Server leg` + `Relay→server (app)` + `Route` rows | `AppStatus.server_leg.{icmp,app,trace}` | `server_leg.rs:95-146,544-559`; rows `client-gui/src/app.rs:2490-2539` |
| `Expected path ~{ms} ms (L + S)` | `AppStatus.server_leg.expected_path_ms` (client leg + relay ICMP `S`) | `client-gui/src/app/choice.rs:295-316`; row label `client-gui/src/app.rs:2498-2499` |
| Client route trace (`Route` row, `{hops} hops · last router …`) | `AppStatus.route_trace` | `client-gui/src/client_trace.rs:56-78,624-661`; `client-gui/src/app.rs:2342,2531-2539` |
| Starvation warning banner + `RelayStarved` tray + rail sub-line | `AppStatus.watchdog.warning`, `AppStatus.tray` | `client-gui/src/app/watchdog.rs:66-84`; `client-gui/src/app.rs:2079,2261,4097-4102`; copy keys `client-gui/locales/en.toml` |
| Update-check disclosure | no field — `UpdateStatus` is produced only by a user action | `client-gui/src/update.rs:92-201`; `docs/privacy.md:184-202` |

Rules: one field path per row; `RouteState` remains the rail-word projection — **7 words from 5 rail-word variants** (Idle contributes `NOT CONNECTED`/`CONNECTED`) — with no variant for trace-only or overlay facts; front-end inputs stay `Capabilities.elevated`/`language`; a fact the client cannot observe gets no field.

**Compatibility with the un-amended spec (A7):** the spec's current §2.2 states 6–8 still cite `WatchdogStatus.fault`, `.retry_in` and `.rejection` (`docs/design/gui-refresh-spec.md` §2.2). The shipped code has removed the retry clock and the rejection chain, so the delta maps those three surfaces to `WatchdogStatus.warning` + `RelayStatus.carry`. **No alias is proposed** — a stale field path must be amended (Q10), not shimmed, or W2-1 would implement dead states.

### 5.4 Relay choice

| Proposed | Carries / replaces | Real source |
|---|---|---|
| `ClientLeg { median, jitter }` | `RelaySample` (moved) | `client-gui/src/discovery.rs:216-223` |
| `RelayChoiceInputs { server, candidates, client_legs, direct_ping, history }` | GUI inputs assembled at `choose_relay_for_destination` | `client-gui/src/app.rs:870-1030`; `RelayHistory` |
| `choose_relay(inputs) -> RelayChoice { Chosen { winner, ranking, skipped_starved, proven }, NoServer, NoCandidate, AllCondemned }` | the chooser, including both branches | `client-gui/src/app.rs:942-971`; `rank_relays_at_with_direct` (`relay_choice.rs:304`); `rank_without_destination` (`client-gui/src/app/choice.rs:501-522`) |
| `chooser_point(ip) -> Option<(f64,f64)>` **Cloud-exact only** | the only location input the ranking may take | `client-gui/src/server_place.rs:222-240`; test `:456-466` |
| `without_starved` / `prefer_proven` / `rank_without_destination` | starvation demotion + proven-carry preference when no exact position | `client-gui/src/app/choice.rs:405-522`; `client-gui/src/relay_history.rs:287-311` |
| `ScoredRelay` new ordering | whole-path total → prefixed prior (`dest_leg_prior_us`) → measured client leg (`client_leg_us`) → jitter → address | `client/src/route/relay_choice.rs:99-136,212-221` |
| `relay_position(node_id)` / registry `region` carried | relay identity | `relay_choice.rs:554-572`; `client/src/registry.rs:86-88`; F-03 still open (labels into `region`, `client-gui/src/app.rs:902-910`) |

### 5.5 Game detection (unchanged from r8)

`DetectedGame { key, name }` + `detect_running()` over `games::auto_detect`/`GAME_REGISTRY` (`client/src/games/mod.rs:396-425,513-517`); the GUI's fuzzy remap (`client-gui/src/app/choice.rs:524-540`) is deleted. `ProfileViolation`/`validate_draft` unchanged (`app/editor.rs`).

### 5.6 Capabilities, location provenance and update disclosure

| Proposed | Carries | Real source |
|---|---|---|
| `Capabilities { version, os, arch, features, interceptor_platform, interceptor_available, elevated }` | CLI `--status` + GUI Components as one fact set (unchanged) | `client/src/main.rs:732-865,760-775`; `client-gui/src/app.rs:3481-3520`; `client-gui/src/platform/mod.rs:94` |
| **`LocationSources { cloud: bool, geofeed: {operators: &[&str], as_of: &str, display_only: true}, hosting: bool, country: bool }`** | the card's provenance and the display-only claim; sourced from the committed tables/manifests, never a network call | `client-gui/src/server_place.rs:35-46`; `server-geofeed.sources.json` (`display_only: true`); `infra/geo/geofeeds/README.md:8-22` |
| `UpdateStatus { current, latest, update_available, installer }` + **`check` is user-initiated only** | update sheet; disclosure contract (A11) | `client-gui/src/update.rs:15-44,92-201`; `docs/privacy.md:184-202`; `docs/data-dictionary.md` |
| pure update rules (`compare_versions`, `ReleaseAsset`, `parse_sha256_file`) | unchanged | `client-gui/src/update.rs:46-61`; `client-gui/src/self_update.rs:111-119`; network/elevation STAY (`client-gui/Cargo.toml:20,38,40`) |

Display-only constraint made explicit: the location source table is **not** a ranking input. `capabilities` may report that operator-geofeed data is present and display-only; no DTO field issues coordinates from it — only `chooser_point`, and only for cloud regions.

### 5.7 Error taxonomy (unchanged, A3 wiring stands)

`AppError` + `Kind` over `LightSpeedError` (`client/src/error.rs:9-33`), re-exported through `app::error`; `AppStatus.error` is composed from `SessionCommandState.last_error`, the `ConfigRecovery` diagnostic and the engine `*_error`/`registration_error`. No `String` crosses the boundary.

### 5.8 CLI call sites that migrate (S6)

The r8 table stands (`client/src/main.rs:401-404,411-414,93-245,1146-1167,594-614,731-864,1111-1144,673-704,1228`). New at this revision: the CLI now compiles and ingests the relay server-leg reports (`client/src/main.rs:31`; `client/src/quic/mod.rs:1601-1689`), so the facade must also **install the provider classifier** for the CLI (D5.2) even though the CLI does not display the leg; and `--status` should report the same `Capabilities`/`LocationSources` facts.

## 6. Staged migration plan

Every stage keeps the full gate green, changes no behaviour, and is independently revertible. Gate commands are the repo gate (`AGENTS.md:69-76`):

```
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
cargo clippy -p lightspeed-gui --all-targets
cargo test -p lightspeed-gui
```

**Baseline must be re-measured before S0, not inherited.** The r8 baseline (1,624/0/6 workspace, 193/0/1 GUI at `62c6cf6`) is **stale**: the rebase added code and tests, and `wat/state/current-phase.md:25` explicitly says the W1 docs' content was not re-verified against the new master. Measured now: GUI declares **264 `#[test]`** (2 `cfg(not(windows))` → 262 on Windows, 1 ignored); client adds `server_leg` (29) and route tests (relay_choice 20, destination 21, order 27). The first W2-1 action is to run the gate and record per-crate counts.

| Stage | Scope (write) | What moves | Gate expectation | CLI compatibility proof | Rollback |
|---|---|---|---|---|---|
| **S0** guard skeleton | `client/tests/gui_boundary.rs` (new), evidence | ratchet guard G1–G7: allowances/budgets = this revision's measurements; **including the new modules and the location/provider rules** | workspace green with the guard passing; GUI unchanged | CLI untouched | delete the test file |
| **S1** config model | `client/src/app/{mod,config}.rs`, `client-gui/src/config.rs` → thin, `client-gui/src/app.rs:505,827,3892`, `client-gui/src/main.rs:137` | `GuiConfig` → `AppConfig`; legacy serializer keeps every shipped key; **`retry_after_fallback` tolerated on read, never written**; 16 GUI tests move | same counts move between crates; counts recorded | `--config` parse/default behaviour unchanged; malformed-config test | revert |
| **S2** discovery/probe/identity | `client/src/app/{discovery,relay}.rs`, `client-gui/src/discovery.rs` → wrapper | registry discovery (adopt `discover_relays` so `region` survives), `/health`, 5-probe measurement, race, parse, `relay_position` re-export; 21 tests move | +21/−21; Linux CI now runs them | none | revert |
| **S3a** status DTO v2 | `client/src/app/{dto,status}.rs`, `client-gui/src/app.rs` getters, `app/{choice,ledger,window}.rs` rules + `app/tests.rs` | `AppStatus` incl. `location`/`server_leg`/`route_trace`/`tray`, `starvation warning`, `carry`, `fallback`; move `tray_state_for`, `relayed_row_text` starving flag, starvation-filter ranking, measured-leg readers; **plus the spec's W2-1 facts** (`RelayStatus.connecting`, `AppStatus.error`, `SessionCommandState.*`, `LoadedAppConfig.recovery`) | tests move; GUI drawing unchanged; screenshot unchanged | CLI untouched | revert |
| **S3b** commands | `client/src/app/commands.rs`, `client-gui/src/app.rs` orchestration | `SessionCommand` + `AppCore`; connect/boost/hold/relay-restart/shutdown become commands | GUI visual check idle/boosting/holding/warning | CLI untouched | revert |
| **S4** judgement + memory | `client/src/app/{watchdog,history}.rs`, `app/relay_choice.rs`, `race_watch.rs` | **warn-only** watchdog thread + `WatchdogPlan`/`RelayStarvationWarning`/history demotion, `RelayHistory` + `CarryVerdict`; GUI keeps `mpsc` wiring and trace pass-through | 14+24+4 tests move; fallback-warning surfaces unchanged; the "never switches" test moves **and stays green** | CLI untouched (adoption Q4) | revert; `relay-history.json` format unchanged |
| **S5a** location ladder + provider seam | `client/src/app/{location,server_leg}.rs`, `client-gui/src/{server_place,server_tables,server_geofeed_tables,detection_provider}.rs` removed/thinned, `client-gui/data/server-*.bin` + manifests, `infra/geo/geofeeds/**`, **both generators** (`infra/scripts/build-server-location.py` cloud/hosting + `infra/scripts/build-server-city.py` geofeed, §3.2.2), `Cargo.toml` features `server-location` + `geo-ip`; owner: `rust-dev` (move/gates) / `infra-dev` (data) | ladder + three indexes + `chooser_point`/`display_point` + `provider_class`; **`AppCore` installs the classifier**; GUI keeps the copy/labels (`client-gui/src/app.rs:411-489`); display-only constraint test moves | workspace + GUI green; feature-gated so the CLI artifact delta is bounded and measured; `server_place` 17 tests move | CLI flags/output unchanged; CLI gains provider-aware locking (behaviour improvement, no output change) | revert; data move behind features |
| **S5b** client trace | `client/src/app/trace.rs`, `client-gui/src/client_trace.rs` → scheduler/parser moved; `caption` stays, `client-gui/src/app.rs:220,1275-1299` | traceroute scheduler, parser, `probe_allowed` guard, `info_line` log contract; GUI keeps the `Route` row label/drawing | 24 tests move; one new log-line golden test | CLI untouched | revert |
| **S6** CLI adoption + ratchet to zero | `client/src/main.rs` call sites, `client/src/modes/*`, guard allowances → empty | repoint §5.8; delete now-duplicate CLI modules only where unreferenced; guard allowance shrinks to adapter exceptions | full gate + CLI text/exit tests; guard mutation proof recorded | per-row output comparison (`--status`, `--list-games`, malformed `--config`, `--probe-proxies`, normalized goldens per spec QA R4) | revert S6; S1–S5 stay |

Stage rules from r8 stand (one stage per PR series, one commit per moved unit, before/after per-crate counts, named GUI-only tests, independent QA, compatibility tests before S6). New rules:

6. **Display-only proof travels with the location data:** S5a's report must show `chooser_point` returns `None` for Geofeed/Hosting/Country and that no ranking path calls `place()` (G6).
7. **Seam ownership proof:** S5a's report must show the provider install exists in `client/src/app/**` and nowhere in `client-gui/src` (G7), and that the CLI path benefits (a unit test of the installed classifier via the facade).
8. **S4 is behaviour-frozen:** the warn-only test (`client-gui/src/app/tests.rs:322-360`) and the "plan never switches" assertion move before the thread does; a move that changes when a warning appears is not a move.

## 7. Drift guard design

`client/tests/gui_boundary.rs` (r8 G1–G5) is extended for the rebase. It still runs in the existing workspace gate (`.github/workflows/ci.yml:42`), so no workflow change.

| Rule | What it asserts | Fails when | Ratchet |
|---|---|---|---|
| **G1 — import surface** | every `lightspeed_client::` path in `client-gui/src` is under `lightspeed_client::app` or in `ALLOWED_CLIENT_PATHS`. New allowances start with the rebase's direct calls: `server_leg_report`/`server_leg_app_report`/`server_leg_trace` (`client-gui/src/app/choice.rs:196,230,262`), `install_server_provider_lookup` (`client-gui/src/main.rs:85`), the `ServerLeg*`/`ServerTraceReport` types | a new direct engine/games/profiles/registry/server_leg call appears without the table | table starts as the S0 snapshot and only shrinks; S6 goal `app` + named exceptions |
| **G2 — moved identifiers** | identifiers that must not reappear in `client-gui/src`: r8 list plus `starvation_verdict`, `plan_return_path_window`, `apply_starvation_verdict`, `return_path_recovered`, `RelayStarvationWarning`, `WatchdogPlan`, `WindowEvent`, `ReplyFaultStreak`, `record_starvation`, `starvation_signature`, `CarryVerdict`, `carry_verdict`, `without_starved`, `prefer_proven`, `rank_without_destination`, `FallbackRanking`, `tray_state_for`, `is_holding`, `rank_relays_`, `relay_health_verdict`, `chooser_point`, `display_point`, `geofeed_place_text`, `provider_class`, `trace_hops`, `GuiTrace`, `probe_allowed`, `relay_measured_server_leg`, `relay_measured_app_server_leg`, `relay_measured_trace`, `expected_path_text`, `validate_draft`, `compare_versions`, `parse_sha256_file`, `RelayHistory` | a business rule is re-implemented | identifiers are added at the stage that moves them; allowance ends at zero |
| **G3 — size budget** | `MODULE_BUDGET` covers **every** `client-gui/src/**/*.rs`, initial values = §3.2 totals (new: app.rs 4,323; app/choice 540; app/watchdog 718; relay_history 790; client_trace 1,326; server_place 595; server_tables 139; server_geofeed_tables 327; detection_provider 184; app/window 198; app/ledger 284; config 528; main 258; app/tests 3,090; …) | a new `.rs` appears unlisted or grows past budget | budgets only lower; a new module requires a deliberate registration |
| **G4 — core is UI-free** | `client/src/app/**` has no `egui`/`eframe`/`Color32`/`Painter`/`Platform`/`client-gui` | logic creeps into rendering | absolute |
| **G5 — core is portable** | `client/src/app/**` has no `cfg(windows)` on behaviour | Windows-only extraction breaks parity | absolute; Windows-only facts stay in `platform/` |
| **G6 (new) — location feeds no ranking** | in `client/src/app/**`: ranking inputs may only be `chooser_point`; `place()`/`display_point` may not be passed to `rank*`; geofeed data may not appear in the ranking module; the moved `server_place` tests include `a_geofeed_place_never_steers_the_ranking` | a geofeed/country/display point reaches the chooser | absolute (the `Place` tests travel with the data) |
| **G7 (new) — seam ownership** | `install_server_provider_lookup` appears in `client/src/app/**` and **not** in `client-gui/src`; the classifier is installed before engine creation | a front end owns the seam again, or the install is lost in a refactor | absolute after S5a; during S5a the old call is the only allowed occurrence |

Ratchet discipline unchanged: the guard lands green on the dirty tree at S0 (allowances/budgets = measured values), then shrinks. The r8 proof obligation (inject → fail → revert → pass) is repeated at S0 and S6; the injection list is in the delta evidence.

## 8. Risks

| # | Risk | Likelihood / impact | Mitigation |
|---|---|---|---|
| R1 | Moving the warn-only watchdog changes when a warning appears/clears (behaviour regression in a live match) | Medium / high | Move-only S4 with the 14 behavioural tests + the "never switches" test moved first; behaviour freeze rule 8; QA independent; real-window warning state where reachable, named unverified where not |
| R2 | CLI output/exit-code changes when S6 repoints `--status`/probe paths | Medium / medium | CLI compatibility tests before S6; normalized golden comparison for live-measurement output (QA R4) |
| R3 | Config-file unification (S5) loses or splits settings, including the newly retired key | Medium / high | One model, two serializers from S1; retired key read-tolerant/write-never; fixtures old/new/both; never delete; release note (owner gate) |
| R4 | Geo/location blobs grow the CLI binary (1.8 MB country + 353 KB new indexes) | Medium / medium | Feature-gate `geo-ip`/`server-location`; measure artifact delta in S5a; fallback: keep data GUI-side and pass through the G7-style seam |
| R5 | Moved tests now run on Linux CI where they ran only in the Windows GUI job | Medium / low | S2/S4/S5 gate includes Linux CI; Windows-only tests stay in the GUI job |
| R6 | Public types (`Config`, `EngineStatus`, `ServerLegReport`, …) need doc comments / surface commitments | High / low | already documented; `QUALITY_STUB`; crate not published |
| R7 | Facade adoption collides with the W2 shell rewrite in `client-gui/src/app.rs` | High / high | Sequencing per spec §5; one `client-gui/src/app.rs` writer at a time; S3–S5 after the shell or rebased onto it |
| R8 | `#![allow(dead_code)]` (`client/src/lib.rs:7`) hides unused facade items | Low / low | accepted; G1/CLI exercise the facade |
| R9 | DTO exposes a `pub(crate)` type → `private_interfaces` warning | Medium / medium | DTOs defined in `app` or re-export public ones (all new types are `pub`) |
| R10 | Presence/elevation parity decided accidentally | Medium / medium | unchanged (Q7) |
| **R11** | A4–A6 were a transient gap; now resolved (spec C-13…C-15, Q3) | — | r8 closed it; A7–A11 replace it as the open contract delta (R17) |
| **R12** | Location data: licence/takedown/attribution broken by a data move, or a display-only label used for ranking | Medium / high | owner decision and deny list travel with the data (`infra/geo/geofeeds/README.md:8-22,190-199`); G6 + `client-gui/src/server_place.rs:456` test; no licensed database (`client-gui/src/server_geofeed_tables.rs:7`) |
| **R13** | Provider seam: after the move, a front end forgets to install the classifier (silent locking degradation) | Medium / high | G7; `AppCore` owns the install; a facade test asserts `server_provider_class` is live after construction |
| **R14** | Client-side traceroute is an undisclosed network probe (privacy/comms) | Medium / medium | `docs/privacy.md:83-99` covers local measurement (ICMP/shadow) but **not** the traceroute; S5b must either document it (privacy + data dictionary) or name it as unverified; owner/comms gate (Q13) |
| **R15** | Warn-only behaviour regresses to auto-switch during extraction | Medium / high | rule 8 + moved test; `WatchdogPlan::switch_to_direct` stays false (`client-gui/src/app/watchdog.rs:180-206`) |
| **R16** | Relay-measured leg gets used for ranking before E1 passes | Medium / high | D5.4; E1 gate (`relay-server-leg-validation.md:48-51`); DTO exposes it as display data only; G6-style rule for ranking inputs |
| **R17** | **Contract is stale (interface.md unchanged) → W2-1 acceptance (b) false acceptance** | High / high | W2-1 (b) grades against **spec §2 / A1–A11 as the contract of record** (the spec's own Q3 wording): the A7–A11 text must be recorded in the spec before W2-1 starts; W2-2 then applies the recorded set to `interface.md` in its own turn (spec §5 order). This doc's §5.3 table is the proposed contract of record (Q10) |
| **R18** | `client-gui/src/app.rs` grew to 4,323 lines and the rebase added new orchestration there | High / medium | new logic goes to `app` modules only; S3/S4/S5 remove orchestration; no new `client-gui/src/app.rs` logic in W2-2 except `ui()` |

## 9. Open questions (for the Lead, owner or W2 implementer)

1. **Q1 probe semantics** (unchanged): CLI UDP keepalive vs GUI TCP connect; converge or keep both.
2. **Q2 geo asset** (updated): the 1.8 MB country blob stays at `client-gui/data/geo-ipv4.bin` (1,810,610 B; `server_place.rs` makes it the ladder's last rung). Move with the ladder behind `geo-ip` (Q11), or keep GUI-side?
3. **Q3 config file unification** (unchanged). Add: the retired `retry_after_fallback` needs a release-note line (no retry budget any more).
4. **Q4 CLI adoption of watchdog/history**: still defer.
5. **Q5 status DTO granularity**: composition, now with the new groups (`location`, `server_leg`, `route_trace`, `tray`).
6. **Q6 update check**: pure rules move only; the user-initiated disclosure is a product contract, not a field.
7. **Q7 presence policy canon**: unchanged.
8. **Q8 W2 scheduling**: unchanged sequencing.
9. **Q9 amendment authority**: A4–A6 closed; superseded by Q10.
10. **Q10 contract delta A7–A11 (new, blocker for W2-1 (b))**: the spec owner drafts A7 (warn-only starvation replaces rows 36–38), A8 (location ladder display-only), A9 (relay-measured leg display-only until E1), A10 (route trace/placement anchor), A11 (update-check disclosure); W2-2 applies them to `interface.md`. Until then, acceptance (b) must use §5.3's amended mapping and say so.
11. **Q11 location-data packaging (new)**: two feature names with one meaning each — `server-location` (three server indexes: `server-cloud.bin`/`server-hosting.bin` from `infra/scripts/build-server-location.py:1-14`, `server-geofeed.bin`/table/manifest from **`infra/scripts/build-server-city.py:54-69`**) and `geo-ip` (country blob/table from `infra/scripts/build-geo-ipv4.py:38-39`); asset home (client-crate `data/` vs GUI); owners `infra-dev` (generators/data) and `rust-dev` (feature gates in W2-3b, §3.2.2); measured CLI artifact-size budget (R4).
12. **Q12 W2-3 split (new)**: W2-3 now carries S3b+S4+S5a+S5b+S6; propose splitting into **W2-3a** (S3b+S4, runtime/warn-only) and **W2-3b** (S5a+S5b+S6, location/trace/CLI). The Lead owns the cut.
13. **Q13 traceroute disclosure (new)**: the client now runs an OS traceroute automatically for the locked server (`client_trace.rs`); `docs/privacy.md` documents ICMP/shadow probes but not this. Confirm the copy/disclosure (or name it unverified) before S5b ships; `[PRIVACY_STUB]` and the owner's no-hidden-network rule apply.
14. **Q14 tray `RelayStarved` shell work (new)**: W2-2 must render the new tray state/icon/tooltip (`client-gui/src/platform/windows.rs:137-155`) and the location/leg/trace copy; confirm the shell slice's scope grows accordingly (QA R1-style re-cut).

## 10. What this document does not decide

- It does not change the state contract; `docs/design/interface.md` remains binding **until the spec delta lands**. A7–A11 are proposals for that delta; W2-2 applies them and no edit to `interface.md` is made here (write scope).
- It does not pick the probe implementation, the config file path, the presence policy, the location-data packaging or the traceroute disclosure — Q1/Q3/Q7/Q11/Q13.
- It does not plan the shell, tokens, platform adapters, copy or CI parity jobs — those are the other W1/delta documents.
- It makes no code, config, manifest, workflow or fleet change (docs only).
