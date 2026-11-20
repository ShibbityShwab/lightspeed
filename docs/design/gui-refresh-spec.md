# LightSpeed desktop 2026 — implementable refresh spec (**Instrument 2.0**, mobile-only delta r13)

| | |
|---|---|
| Programme | WF-081 "Desktop 2026", **W1 delta** synthesis (docs only) |
| Author | `interface-designer` (task-21); synthesises the four delta outputs |
| Date | 2026-11-20 · worktree `dsh-b74d064d-6fe5`; **baseline `origin/master` `76aba5d`** (16 commits: 1.11.4, warn-only starvation, location ladder, route trace, update disclosure) + the W1 set committed at `0df57ce` |
| Status | **Revision r20 (QA r5 errata).** The owner accepted the W1 set on 2026-11-20 with two changes: a **mobile-only viewport** and the **rebase onto `76aba5d`** (`gui-refresh-brief.md:3-6,318-334`). This r20 pass closes the spec-owned errata from `review-r5.md` (R1-spec: DE 275.1 passes the ≤276 cap by 0.9 px and the HEAD/current overflow sets are separated; R3: A14 gains `interface.md:53-55,:71-74,:232-237`; R8: Q20 names the copy owner, the W2 word-set gate and the ES/PT landing) on top of r18's F1–F9/minor fixes. No code, token, config, workflow, engine or fleet change. `interface.md` is **not** edited here — the contract amendments **A1–A14** are drafted in Appendix C for W2-2 to apply. |
| Supersedes | the W1 spec geometry (360/440/1120), states 6–8 (fallback/re-test/reject) and every copy row tied to them — all retired by the owner decision and by master `76aba5d`. The historical critique mapping is Appendix A; the r10 audit mapping is Appendix B. |
| Inputs | delta `gui-core-boundary.md` r9 (DTO v2, S5a/S5b, A7–A11 proposal); delta `gui-ux-audit-2026.md` r10 (36 findings, warn-only model, contract critique §3.0); delta `design-system-2026.md` r11 (mobile-only budgets, §6.10 notice, §6.11 location/trace, 23 new keys, font identity); delta `cross-platform-2026.md` r4 (320 geometry, `TrayState::RelayStarved`, `client_trace` cfg exception); `gui-refresh-brief.md` owner decisions; `docs/design/interface.md` (stale — §2/Appendix C say how); shipped `client-gui/` sources at `76aba5d`; screenshots and delta evidence under `wat/evidence/2026-11-20/team/`. |
| QA route | this delta feeds the final delta QA (task-22) and then W2. |

**Authority order:** owner decisions and measured evidence > `interface.md` facts **as amended by A1–A14** > program constraints in the brief > prose claims. The shipped code is evidence of what the product does; where it disagrees with the contract, Appendix C names the amendment and which side changes. Items needing the Lead/owner are §7.

---

## 0. What this delta decides, and the facts it inherits

The W2 build contract, rebased: the mobile-only shell (§1); every contract row on screen, from which field, with which components and recovery (§2); the component map (§3); per-OS requirements (§4); five W2 work units (§5); the 320-px screenshot matrix (§6); the decisions left (§7); and the contract amendments A1–A14 + conflict log (Appendix C). It does not re-audit the product: the four delta documents did that.

**Inherited measured requirements — carried forward, not re-derived.**

| # | Requirement | Number | Source |
|---|---|---|---|
| D1 | Contrast is measured; every AA failure fixed | 41 shipped text pairings, **40 AA pass / 1 fail** (selected row **1.35:1**, fix 6.70:1); 20 non-text claims, 8 pass / 12 fail; new rows: state word `NO RETURN PATH` danger on `bg_2` **6.57:1**, notice text **5.61:1** (danger tone) / **9.30:1** (warn tone), location/trace caption `text_3` on card **5.45:1** | `design-system-2026.md` §3 (r11 re-run) |
| D2 | Tokens only; one role, one value | **9 dead tokens** (was 10 — `R_INLINE` is live, the notice frame) + 1 shadowed `grid`; add `ACTION_H_WRAPPED` 56 and the r11 additions; 9 to delete, 7 to re-role; **no token value changed** on the rebase (`design.rs` diff empty) | `design-system-2026.md` §1 (r11), §11.1 |
| D3 | Focus visibility is a Blocker | the custom action is still UIA `Custom` with no name; focus paints nothing (live at 320, `window-connected-320-delta.png`) | `gui-ux-audit-2026.md` B1 |
| D4 | Glyph/type identity | 619 catalog chars, 0 tofu; **Inter subset lacks `y` and `{`**, JetBrains Mono lacks `o q ! " # } ~`; `→` is drawn by Noto Sans JP only, `■ ▲ ▶ ▸ ▾ ◆` by no bundled face; the routed arrow is a painted mark | `design-system-2026.md` §8.2–§8.4 (r11) |
| D5 | Reduced motion: no claim; one polarity rule | the OS value means *animations enabled*: spinner only when explicitly `On`; static text on `Off` **and** `Unknown` (conservative, WCAG 2.2.2); W2-2 (i) seam, W2-4 (i) Windows `SPI_GETCLIENTAREAANIMATION`; macOS/Linux `Unknown` gaps | `design-system-2026.md` §7.4; §5 W2-2 (i)/W2-4 (i) |
| D6 | Type + rail, and the 30 px word fit | one 8-role scale; `HERO_H` 40 (METRIC row measured 40); the 288 px hero row cannot hold a long localized word + the 90 px metric, so the metric gets its own row (§1.2). **At HEAD four words exceeded the 288 px column** — RU `NO RETURN PATH` **342.8**, RU `IDLE` **326.8**, ES `NO RETURN PATH` **309.0**, PT-BR `NO RETURN PATH` **328.4**. **On the current tree RU `Нет возврата` has landed at 222.9 and three remain:** ES (309.0, pending), PT-BR (328.4, pending) and RU `IDLE` (326.8, retired by A2). **DE `OPTIMIZING` 275.1 is inside the ≤276 cap by 0.9 px** (thin — the optional DE `OPTIMIERUNG` 167.2 slack stays available). **F1 remedy (design-system r14): shorten the localized words; acceptance = every word in the W2 word set — the state words, including `state.connecting` and the changed `state.disconnected` value — at `STATE` 30 px measures ≤276 px in its installed chain once its catalogs land** (12 px / 4 % safety under 288, ±2 px tolerance; the type role, the 40 px hero and the chrome budget do not change; Q20). **Landed:** RU `Нет возврата` 222.9. **Proposed pending the copy owner (Q20):** ES `SIN RETORNO` 176.8, PT-BR `SEM RETORNO` 178.7. | `design-system-2026.md` §4.4/§4.5 (r14 re-measure); `delta-copy.md` §5; `delta-r14.md` §1; `review-r5.md` R1/R8 |
| D7 | Action labels at the **264 px** column | `stop_optimizing`/`optimize_route` 1 line in all 9 locales (worst 225.7 / 220.7 px RU); `restart_admin_optimize` 2 lines in EN/DE/ES/FR/PT-BR/KO and **RU is now 2 lines at 356.9 px after the landed copy** (`Перезапустить от имени администратора`); the two-line slot `ACTION_H_WRAPPED` 56 is adopted and the 9 px shrink is deleted; **Q15 resolved: shorten RU copy, no 73 px slot** (§7 Q15; `delta-r16.md` §3) | `design-system-2026.md` §4.4 (r14); `wat/evidence/2026-11-20/team/ux-lead/delta-r16.md` |
| D8 | Mobile-only geometry (owner 2026-11-20) | width **pinned 320 px**, one `narrow` column, height user-resizable **420–1400**, initial **760**; padded content **288**, card inner/action/sheet **264**, notice text column **248**; no 380/619/620 breakpoints, no two-column layout | `gui-refresh-brief.md:323`; `design-system-2026.md` §4.4/§5.4; `main.rs:110-115`, `app/window.rs:35,39` |
| D9 | Fixed chrome and scroll at 320 (recomputed with the metric row) | rail **128** (normal) / **108** (short, sub-line moves into scroll); action bar 64 single-line / 72 wrapped; **disabled-reason line 18 = CAPTION row 14 + S1 4** (a 14-only line gives 235/185; the S1 gap is part of the row); footer 57 / **41 in the compact tier** (`HIT_MIN` 24). **Worst case 320×420: chrome 239 → scroll 181** (≥180 floor kept); best 213/207. 320×760: 249–275 → scroll 485–511. 320×1400: scroll 1125–1151. **W2 uses 239/181**; `design-system-2026.md` §5.4 (r14) now adopts this table (C-17 closed) | §1.3 arithmetic; `design-system-2026.md` §5.4 (r14); `review-r4.md` §2.2 |
| D10 | Warn-only starvation (owner-approved 2026-11-20) | the watchdog **never switches the route** (`WatchdogPlan::switch_to_direct` is always `false`, `app/watchdog.rs:180-206`); a confirmed fault raises a warning after `RELAY_REPLY_STREAK` = 2 consecutive judged windows; **only a fully healthy window clears it** (`return_path_recovered`, `app/watchdog.rs:149-158`); the relay is demoted for future ranking (history), the session keeps relaying; `TrayState::RelayStarved` outranks Optimizing | `app/watchdog.rs:66-95,149-158,180-206`; `app/window.rs:133-159`; `app/tests.rs:322-360`; `gui-ux-audit-2026.md` §4.2 |
| D11 | Honest location ladder, display-only | five exclusive rungs Cloud → Geofeed → Hosting → Country → Unknown (`server_place.rs:171-204`); **only `chooser_point` (Cloud-exact) may feed ranking** (`server_place.rs:222-240`); geofeed/hosting/unknown draw **no dot** (`display_point` `:206-220`); hosting/unknown get the unreliability hint | `server_place.rs`; `infra/geo/geofeeds/README.md:8-22,190-199` |
| D12 | Relay-measured server leg + trace, display-only | relay ICMP `S` (+ `measured, unvalidated` when fail-open), passive app-RTT secondary, relay TTL trace as a placement anchor, `Expected path ~{ms} ms (L + S)` as a **lower bound**; never a ranking input until the E1 gate passes | `client/src/server_leg.rs:95-146,544-559`; `app/choice.rs:183-316`; `docs/design/relay-server-leg-validation.md:48-51` |
| D13 | Client route trace, display-only | the client's own OS traceroute to the game server: hops + last responding hop RTT only, **no city, not the game's ping**; one run per (server, relay session) with a 30-minute server cache, bounded deadline, never private/reserved targets; it is an **undisclosed network probe** until the privacy docs say otherwise (Q13) | `client_trace.rs:1-30,624-643`; `docs/privacy.md:83-99,178-204`; `gui-ux-audit-2026.md` M14 |

**Not decided here:** engine/protocol/capture/relay behaviour; the boundary itself; token values; CI jobs/packaging. DTO names are the architect's (`gui-core-boundary.md` §5, r9); where a fact is missing the spec says so.

**Citation convention:** repo-root relative, except `app.rs`, `app/*.rs`, `platform/*.rs`, `main.rs`, `i18n.rs` (relative to `client-gui/src/`); `client/src/...` is always spelled out.

---

## 1. Shell: the mobile-only viewport

### 1.1 Window form (owner decision 2026-11-20 — Q1 resolved)

The shipped geometry **is** the product: **width pinned at 320 px** (min = max), **one single narrow column**, **height user-resizable 420–1400 px, initial 760** (`main.rs:110-115`, `app/window.rs:35,39`). No 360/440/1120 plan, no 380/619/620 breakpoints, no two-column layout, no wide screenshot set (`gui-refresh-brief.md:323`; `design-system-2026.md` §5.4). `narrow` is always true; the wide branches (`LEDGER_LABEL_W`, side-by-side form rows) are **retired** in W2 with their tokens.

What W2 changes about the window (not the pinned width):

1. **Persist the height** (and the position where the platform allows) in `UiPrefs`; restore to a visible monitor; clamp to the work area (`cross-platform-2026.md` G2.2). One height per launch — the user controls it.
2. **Remove the content-driven auto-resize** (`app.rs:4283-4287`): the window currently grows/shrinks as state changes by >8 px, so the action bar and footer move under an aiming pointer and the window has no remembered height (`gui-ux-audit-2026.md` M4).
3. **Keep the 420 minimum**: the short/compact tiers below are real at that height, not an edge case.

### 1.2 Regions and the rail's new structure

The rail's measured collision (D6) forces one structural change: **the metric gets its own row below the state word** (`design-system-2026.md` §4.4 item 4, §4.5 row 1, §6.9 gap 11). It is not a size change and not an optional variant.

| # | Region | Contents | Measurement at 320 px | Behaviour |
|---|---|---|---|---|
| R1 | **State rail** (fixed, full-bleed) | row 1: state word (`STATE` 30, dropped `0.82` multiplier); row 2: metric (`METRIC` 30) + its `relay leg` caption (`CAPTION` 11); row 3: sub-line (`BODY` 13) | **2×S3 padding 24 + `HERO_H` 40 + S1 4 + metric row 40 + S1 4 + BODY row 16 = 128 px**; **short mode = 108 px** (sub-line moves into the scrolling region). The word row is **one line by contract after the F1 copy fix** (every state word ≤276 px at `STATE` 30, D6); the 24.6 px fit number is retired | never scrolls. The word is the state; the metric row is always *labelled* (A13) and shows `-- ms` + `relay leg` when unmeasured. Edge colour is redundant, never the only cue |
| R2 | **Notice line** (in the scrolling region, not a fixed band) | one sentence, danger or warn tone, up to the measured 4 lines (`design-system-2026.md` §6.10) | frame `danger_soft`/`warn_soft` + 1 px stroke + `R_INLINE` 4 + S2 margin; text column **248 px**; BODY SemiBold 13 | one at a time; no dismissal, no motion, no control; text is a `ui.label` (in the AccessKit tree). Live-session facts (the starvation warning) sit at the top of the route card; command errors/preconditions sit at the top of the scrolling region. **No conditional fixed band** — a fixed band would change the scroll boundary per state (this supersedes the W1 fixed R2; Appendix C C-18) |
| R3 | **Route card** (fixed at normal height; folds into the scroll region at short height) | warning line (when active) · map (`MAP_H` 132, `MAP_W_MAX` 430→288 at this width) · location ladder block (1–3 caption lines) · ledger rows (≤6 by default + one `Details` disclosure) · cannot-beat caption | `bg_2`, `border_1` 1 px, `R_CARD` 8, S3 padding; card inner **264 px** | content the player reads first; the ledger density rule is M5 |
| R4 | **Action bar** (fixed, full width) | the one action + disabled-reason line (`CAPTION` 14 + S1 4 = 18) | `ACTION_H` 48 (1 line) / `ACTION_H_WRAPPED` 56 (2 lines at 15 px); **no three-line slot** — Q15 is resolved by shortening the RU copy (D7); S2 padding | always above the footer; the label never shrinks below 15 px |
| R5 | **Config region** (the only scrolling region) | relay row · game row · target · advanced/manual fields | `bg_1` plane, one card style | `ScrollArea` with an always-visible scrollbar |
| R6 | **Footer** (fixed) | Hide to tray (only when a real tray exists) · Settings · Quit | hairline 1 + S2 8 + button row 40 + S2 8 = 57 px; **compact tier (<480 height): `HIT_MIN` 24 targets → 41 px** | never moves; no "Hide to tray" when there is no tray |
| R7 | **Sheets** (modal) | Settings · Update · About | `scrim`, `bg_0` body, `border_1`, `R_SHEET` 16, `TITLE` 20, body `ScrollArea`; width **264 px** | per-sheet close policy; focus confined; Esc closes |

### 1.3 Fixed chrome and the scrolling region (320 px, recomputed)

| band | normal (≥560 height) | short (<560) | compact (<480) |
|---|---|---|---|
| rail | 128 | 108 (sub-line into scroll) | 108 |
| action bar | 64 (1 line) / 72 (2 lines) | same | same |
| disabled-reason line | 18 when disabled | same | same |
| footer | 57 | 57 | 41 |
| **worst-case chrome** | **275** (wrapped + reason) | 255 | **239** |
| **scroll region** at **420** | — | — (420 is compact) | **181 worst / 207 best** |
| **scroll region** at **760** | 485 (worst) / 511 | — | — |
| **scroll region** at **1400** | 1125 / 1151 | — | — |

The **≥180 px scroll floor is kept at the 420 minimum (181 px worst case)**: the metric row (+44) and the disabled-reason line (+18) are paid for by the compact footer (−16) and by moving the sub-line into the scrolling region at short height (−20). Best case at 420 is 207. **W2 must use 239/181** — `design-system-2026.md` §5.4 (r14) now adopts this table, and the 18 px reason row is derived as CAPTION 14 + S1 4 (a 14-only row would give 235/185); C-17 is closed by that adoption, not by a drift. The former DS figure 213/207/547/1187 is pre-fix (it predates the metric row and the reason line).

**One line of state word is a hard requirement, not a preference** (F1). The copy remedy caps every uppercased state word at 276 px at `STATE` 30 in its installed chain (D6). The alternatives are priced and rejected: a **two-line word** grows the rail by 36 px (compact worst chrome 275 → **scroll 145 < 180**); a **24/25 px effective word size** (274.2 / 285.7 px at the worst case) breaks the mandated 30 px type role; a **taller action slot** (the former Q15 73 px option) makes the band 73 + 2×S2 = 89 → chrome 108+89+18+41 = **256 → scroll 164 < 180**. W2-2 verifies the table above in a real window and the delta QA re-checks it (Appendix C C-17/C-26).

### 1.4 Height tiers, persistence and 200 % scaling

Breakpoints are **logical points**. Tiers: **normal ≥560** (card fixed, config scrolls); **short <560** (card and sub-line fold into the single scrolling region — the map is **omitted**, there is no smaller honest map size; rail = word+metric only); **compact <480** (footer at the `HIT_MIN` 24 tier; everything except rail/action/footer scrolls). The work area wins over the 420 minimum if the screen is smaller (clamp; no clipping).

**200 % scaling test stays mandatory.** At 200 % the logical work area halves: a 1080p display offers ≈520 pt of height, so a 200 % launch starts in **short** mode; a 768p display offers ≈340 pt and starts **compact**. Captures: **320×420**, **320×760** and **320 pt @ 200 %** (the same window rasterised at 640 physical px), in **EN + DE + RU** (F6: RU is the binding long-word/long-label case — `Нет возврата` and the 2-line restart label; ES/PT-BR join once their shortened state words land — Q20). Row heights are computed at 1.0 ppp; at 125/150/200 % each `round_ui` term can shift a row ±1 px (`design-system-2026.md` §12).

**RTL mirroring is N/A** for the nine shipped locales (`i18n.rs:135-172`: en, de, es, fr, ja, ko, pt-BR, ru, zh-Hans — none right-to-left); recorded, not silent (QA R17/m9).

---

## 2. State-by-state screen spec

### 2.1 Rail-word precedence (one word, one driving fact)

The shipped/owner-approved warn-only model removes every automatic route change, so the word set is smaller and strictly ordered:

1. **Error** — a live command/engine failure (`SessionCommandState.last_error`, engine `*_error`, `registration_error` with `control_registered == false`). Never hidden (A3).
2. **`NO RETURN PATH`** (danger) — the starvation warning is active **and** boosting (`app.rs:2079-2090`; pair filter `app/watchdog.rs:496-506`). The route keeps relaying; the word says the return path is not working (A7). The warning exists only while a tunnel is running.
3. **`HOLDING`** (warn) — the engine's tunnel gate is off while a session runs (`EngineStatus.tunnelling_enabled == Some(false)` + a session; A1); the Choosing sub-line is its sub-state (A4).
4. **`OPTIMIZING`** (signal) — a tunnel backend carries traffic and there is **no active warning** (`run_boosting()` + one counter pair; A5).
5. **`CONNECTING`** — a connect attempt is in flight (DTO gap, §2.1 table).
6. **`CONNECTED`** / **`NOT CONNECTED`** — Idle, split by `engine.connected` (A2).

**Retired by A7:** `DIRECT` / Direct fallback, Re-test pending, Server rejects relay — the state rows, their driving facts (`WatchdogStatus.fault/retry_in/rejection`), their copy keys (`state.direct`, `route.direct_fallback`, `route.retry_in`, `route.server_rejects_relay` — already deleted from the catalogs on master) and the fallback story paragraph. **No aliases**: a stale field path is amended, not shimmed (`gui-core-boundary.md` §5.3).

**Overlays and trace-only rows:** Needs elevation is an overlay (`Capabilities.elevated`, A6), never a word. The warning is an overlay on the route word (it *is* the word while active). Relay recovered is trace-only (A7 reword: the warning clears and the word returns to OPTIMIZING; no banner is invented).

**Presentation seam.** One pure adapter, `client-gui/src/app/route_surface.rs` (new in W2-1), maps `AppStatus` (including the warn-only `WatchdogStatus` and the new location/server-leg/trace outputs) to the §2 word/rows model. Presentation mapping only — no thresholds, verdicts, joins or counters. The shell renders it and derives no state.

**Facts the contract needs and the DTO does not carry yet — closed in W2-1** (names indicative; the architect's DTO revision reconciles — `gui-core-boundary.md` §5.3):

| Fact | Field (W2-1) | Drives |
|---|---|---|
| a connect attempt is in flight | `RelayStatus.connecting` | state 2 |
| a session command is in flight | `SessionCommandState.in_flight` | the action loading state (`STARTING…`, §3) |
| the last session command failed | `SessionCommandState.last_error: Option<AppError>` | Error |
| the config file was unreadable, defaults used | `LoadedAppConfig.recovery` (`ConfigRecovery`) | `error.config` (§2.8) |

### 2.2 The amended route rows

**1. Idle** — `interface.md:31`; **A2** (two words). Shipped words are `NOT OPTIMIZING` (`state.idle`) and `CONNECTED`; `NOT CONNECTED` replaces the disconnected word and **wires the existing unused `state.disconnected` key** (`client-gui/locales/en.toml:149`) — no duplicate key.
- Fact/source: `AppStatus.engine.connected` (`client/src/engine.rs:62`).
- Rail: word; metric row `-- ms` + `relay leg`; sub-line by game mode: `Relay ready · Auto waits for your game` / `Relay ready · {name} follows` / `Relay ready · {name} pinned`; disconnected: `No relay yet` (shipped `route.no_relay`) — W2 keeps the short form and the visible next action instead of a new sentence.
- Ledger (default): `Relay` (`--` or label/address), `Relay RTT` when measured, `Game server` `-- start optimizing to place it`, `Relayed` `-- start optimizing to count`; `Details` disclosure absent until a session exists.
- Actions: the one action per §3 gate; the Relay row carries `Connect`/`Disconnect` (`action.connect` / `action.disconnect` — existing unused keys wired, B2); disabled-reason line when not ready (§2.9).
- Recovery: `Connect` in the Relay row; discovery `Try again` in the relay row's error state.

**2. Connecting** — `interface.md:32`. **DTO gap**, W2-1 adds `RelayStatus.connecting`; `control_registered`/`registration_error` end the attempt.
- Rail: `CONNECTING` (`state.connecting`); sub-line `Reaching {relay}…` (`route.connecting`); metric `-- ms` + `relay leg`.
- Actions: Relay combo disabled during the attempt; action disabled with reason `Waiting for the relay` (`action_reason.waiting_relay`).
- Recovery: success → CONNECTED; failure → **Error** with the registration sentence; `Connect` retries.

**3. Holding** — `interface.md:33`; **A1** (fact = the engine gate; `is_holding` is deleted on master, not a variant).
- Fact/source: `AppStatus.engine.tunnelling_enabled == Some(false)` with a session/interceptor present (`client/src/engine.rs:195`; `app.rs:1845-1850`). The old `direct_fallback` proxy no longer exists.
- Rail: `HOLDING` (warn); sub-line in order: server not locked → `Finding the game server…` (`route.holding_scan`, replacing the engine's untranslated scan sentence in a value slot, C6); chooser in flight → `Choosing a relay for this server` (`route.choosing`); otherwise `Traffic stays direct until a relay is chosen for the game server` (`route.holding`).
- Ledger (default): `Relay`, `Path` = `Holding`, `Game server` (last known/`--`), `Relayed` (`No traffic relayed yet` until the first send).
- Actions: `STOP OPTIMIZING`; Relay/Game combos enabled (choosing is the resolution).
- Recovery: gate opens → OPTIMIZING; the engine lifts the hold after `HOLD_FALLBACK` 120 s (`app.rs:97,1226-1238`) and the rail follows the engine (no countdown copy exists, so none is invented).

**4. Choosing** — `interface.md:34`; **A4** (sub-state of HOLDING, not a word). The merge is authorised now (it was called undocumented at `gui-ux-audit-2026.md:375`).
- Fact/source: the destination-aware chooser is in flight for the locked server (`AppStatus.relay.choosing`; `app.rs:942-971`).
- Rail: `HOLDING` + `Choosing a relay for this server`; everything else identical to Holding.
- Recovery: same; if no candidate can be chosen the session stays on the relay path it has (warn-only makes no route change) — no invented "no relays" word.

**5. Relaying** — `interface.md:35`; **A5** (fact = the `run_boosting()` gate + **one** counter-source pair, never "interceptor counters" generically — the mix let `Relayed` contradict the warning, audit C3).
- Fact/source: `run_boosting()` (`app.rs:2645-2650`); the pair is the interceptor's per-server flow when present, else the redirect pair — one source, W2-1 derives it once (`RelayedPair`).
- Rail: `OPTIMIZING`; sub-line `{game display}` + the **painted route arrow** (`design-system-2026.md` §8.4) + `{relay label}`; metric row = relay leg (`latest_rtt_ms`) + `relay leg` caption.
- Card: warning line if the starvation warning is active (the word becomes `NO RETURN PATH`); map; location block; ledger (default ≤6 rows, §3); cannot-beat caption when provable.
- Actions: `STOP OPTIMIZING`; Relay/Game combos enabled (changing either restarts interception — stated in the warning remedy copy, M16). While `SessionCommandState.in_flight == Some(StartBoost)` the action shows `STARTING…` (`action.starting`) and cannot re-trigger.
- Recovery: a fatal engine error → Error; a warning → `NO RETURN PATH` while the session keeps relaying; game ends → tunnel stops → Idle; the `Relayed` pair resets with the session (`app.rs` session-key reset).

**6. No return path** — **new row; A7 replaces the three retried states.** This is the flagship honesty surface of the rebase.
- Fact/source: `AppStatus.watchdog.warning: Option<RelayStarvationWarning>` = `{server, relay: Option<address>, fault}` (`app/watchdog.rs:66-84`), raised only after `RELAY_REPLY_STREAK` = 2 consecutive judged windows carry the same fault (`ReplyFaultStreak::observe`, `app/watchdog.rs:126-147`); `None` clears only on a window with **no fault at all** (`return_path_recovered`, `app/watchdog.rs:149-158`). The warning is per (server, relay) pair; a new server or a changed relay clears it, an unknown relay keeps it (`warning_is_about`, `app/watchdog.rs:496-506`). It is published to the relay history once per episode so the pair is demoted for future ranking (`StarvationVerdict`/`CarryVerdict`).
- Rail: word `NO RETURN PATH` (`state.not_returning`, danger); sub-line keeps naming the current route (`{game} → {relay}`) — the player is still relayed through it; the route card carries the warning line and the remedy.
- **Warning line (R2 component, §6.10), danger tone, text 248 px, 3–4 lines measured:** copy by fault (`warning_message_key`, `app/watchdog.rs:512-519`):
  - `NoReplyFromRelay` / `TooFewReplies` → `route.relay_not_returning`, **W2 value (C1 + M16):** `No game traffic is coming back from this relay yet. Stop optimizing or pick another relay; the game's connection restarts.` — states the measurement, not the relay's intent (a silent game server produces the same signal); the earlier "Relay is not returning your game traffic" is retired.
  - `InjectionFailed` → `route.injection_failed_warning`, **W2 value (C6 + M16):** `The relay's replies are not reaching your game. Stop optimizing, then open the log in Settings → Maintenance.` — the local fault is not blamed on the relay.
- Actions the surface may offer: `STOP OPTIMIZING`; pick another relay (the game connection restarts — the cost is stated, M16); for the injection fault, the log path is named. **No automatic route change is claimed or performed** (warn-only; owner-approved 2026-11-20).
- Tray: `TrayState::RelayStarved` (amber-orange, distinct from error red), never `Optimizing`, while boosting + warning (`app/window.rs:133-159`; `platform/windows.rs:137-158`).
- Recovery: a fully healthy judged window clears the warning and the word returns to `OPTIMIZING`; **no "recovered" banner** (trace-only; A7). Settings → Routing states the policy (`settings.starvation_policy`).
- Honesty limit: the relay-side starvation counters added on master (`proxy`/control messages) are not shown until they are measured on the fleet; the sentence therefore claims the client's observation only (audit C1).

**7. Needs elevation** — `interface.md:40`; **A6** (overlay driven by `Capabilities.elevated`, never a word; the architect's DTO revision removes `RouteState::NeedsElevation`).
- Fact/source: `Capabilities.elevated` supplied by the front end (`platform/mod.rs:94`).
- Surface: rail keeps its route word; action label `RESTART AS ADMINISTRATOR TO OPTIMIZE` (macOS/Linux variant `Restart to grant network access`, `action.restart_network_access`; **RU is the shortened two-line copy**, Q15); notice line (warn) `LightSpeed needs administrator rights to relay game traffic.` (`notice.needs_admin`). The elevation invite appears only when a relay is selected and the boost can otherwise start (M1).
- Actions: the handshake: `RESTARTING AS ADMINISTRATOR…` loading label, the current process stays alive until the elevated instance confirms startup (C8 fix); declined prompt returns to ready with `notice.elevation_declined`.
- Verification: the handshake state machine is unit-tested and closes W2-4 (e); the **interactive UAC-decline path is U** on this host (elevated harness) with the fixture named.

**8. Error** — `interface.md:41`; **A3**.
- Fact/source: `SessionCommandState.last_error: Option<AppError>` + the config recovery diagnostic + the engine `*_error`/`registration_error` fields; one `AppError.Kind`, one sentence (§2.8); no `String` as copy.
- Surface: word `ERROR` (danger); sub-line = the kind's sentence; the notice line at the scroll top carries `Details` (verbatim engine message, CAPTION mono `text_2`) and `Open log file`. Counters freeze, never clear.
- Actions: the action gate labels retry a start; a registration failure disables the action and is retried by the Relay row's `Connect`.
- Recovery: retry; `Open log file`; Settings → Components.

**9. Relay recovered** — `interface.md:39`; **A7 reword.** The warning clears visibly (the danger line and the word disappear when a fully healthy window proves the return path); there is **no "recovered" banner**. Acceptance is the absence: a test asserts no recovered state renders (`RouteState` has no variant; trace-only).

### 2.3 Server-leg and path rows (display-only)

Shown only while a tunnel carries traffic and the route is the relay. **One precedence**: relay-measured rows lead when the relay has a report for this exact (relay, server) pair; the v1 estimator rows are the fallback when it has none. Both are display-only; nothing here feeds the ranking (D12; the E1 gate is `docs/design/relay-server-leg-validation.md:48-51`).

| Row | Copy (shipped keys, en) | Source / rule |
|---|---|---|
| `Server leg` (relay-measured) | `Relay→server {ms} ms · measured n={n} · {secs}s ago` / `… measured, unvalidated n={n} · {secs}s ago` | `ServerLegReport` for this pair (`client/src/server_leg.rs:95-146`); `unvalidated` = the triangle floor could not be checked (fail-open) — the qualifier is required |
| `Expected path` (label/value split, M15) | label `Expected path` (`route.expected_path_label`, new); value `~{ms} ms (relay leg + server leg)` | `expected_path_text` (`app/choice.rs:295-302`): `L + S` is a **lower bound**, `~` marks it; only when a fresh client leg exists; the label must not render the template (M15 defect) |
| `Relay→server (app)` | label `Relay→server (app)`; value `{ms} ms · n={n}` | passive app-RTT report (`app/choice.rs:225-247`); secondary, display-only |
| `Path trace` (client trace, label renamed from `Route`, m3) | `{hops} hops · last router RTT {ms} ms` / `{hops} hops · no router answered` | client trace for this server (`client_trace.rs`; `app/choice.rs:272-284`); placement anchor, **not the game's ping** |
| `Server leg` (estimator fallback) | `measured {ms} ms · n={n}` / `at least {ms} ms · triangle bound` / `not measured · prior ~{prior} ms ±{unc} ms` / `not measured · no region prior` | the four v1 states (`interface.md:63-66`, A9 keeps them as the fallback path) |
| `Relay cannot beat direct` caption | `This relay cannot be faster than direct for this server` + `Details` arithmetic | the one provable whole-path claim (`interface.md:67`); `L1 >= L3d` from the keepalive leg + a fresh same-server direct ping (`SERVER_PING_FRESH_SECS` 60 s) |

Rules: no city, no invented age, no "faster" claim; the relay's own trace (`ServerTraceReport`) is the A10 counterpart to the client trace; the row set above lives behind the card's `Details` disclosure except the single best `Server leg` row (§3).

### 2.4 Location ladder and client route trace (display-only)

**Location ladder** — five mutually exclusive captions under the map, `CAPTION` 11 `text_3`, left-aligned on the one gutter (M9; the shipped centring is a defect):

| Rung | Copy (shipped keys) | Rule |
|---|---|---|
| Cloud region (exact) | `Server location: cloud provider region` + `{PROVIDER} · {region}` | provider's own published ranges; the **only** rung that may feed the chooser (`chooser_point`) |
| Operator geofeed (display-only) | `Server location: operator geofeed — {place}` + `({provider}, as-of {date})` + `Shown as-is from the operator's published geofeed.` + hover `Source: {source}` | RFC 8805 feed shown as-is; **no dot** (the feed has no coordinate); never ranking |
| Hosting network (display-only) | `Server location: hosting network — unknown` + `AS{asn} · {name}` + `Free IP databases are unreliable for hosting ranges.` | **no dot**; the hint is a caption, not a tooltip |
| Country estimate | `Server location: country-level estimate from IP` | country centroid only; says it is an estimate |
| Unknown | `Server location: unknown` + the same hint | explicit absence; never a blank or a fake dot |

Rules: one source per rung (`server_place::place`); captions are never hover-only; no city a source cannot produce; the geofeed source URL is the only hover fact; the block wraps (1–3 lines measured) and is never truncated (`design-system-2026.md` §6.11).

**Client route trace** — one caption under the map while a trace exists: `Route to server: {hops} hops` + `· nearest responding hop {ms} ms` + the provider/hosting network name when the shipped indexes know the last hop (`client_trace.rs:624-643`). Hover explanation must be a catalogued string (C6/M8 — shipped is hardcoded English). It claims no city and is not the game's RTT. **Privacy gate:** the traceroute is an undisclosed network probe until `docs/privacy.md`/`docs/data-dictionary.md` list it (M14; Q13) — S5b does not ship before that.

### 2.5 Update-check disclosure (A11)

The update sheet is the surface that acts, so it states the disclosure in one sentence: the check is **user-initiated only**, contacts `api.github.com` for the latest release, sends nothing but the request (User-Agent `lightspeed-gui-updater/<version>`), and no startup/background check exists (`docs/privacy.md:184-204`; `update.rs:92-201`). New key `update.disclosure`; the sheet keeps the version line, result line and one action; no install button when no installer asset exists (non-Windows). No copy may claim a background check or a passive "update notice".

### 2.6 Sheets (updated)

| Sheet | Entry | Content | Close policy |
|---|---|---|---|
| **Settings** | footer | Language · Privacy (telemetry) · **Routing** — the warn-only policy (`settings.starvation_policy`; the retest checkbox and its 20/45/90 s tooltip are **gone** with the machine) · Relays (40 px rows, 24×24 delete) · Maintenance (check updates, open log, refresh relays, re-measure) · **Components** (diagnostic; the one documented English-only exception, `interface.md:231`) · About | backdrop/Esc/Close; no unsaved data |
| **Update** | Settings → Maintenance | current/latest (one spelling, no leading `v`), result line, the **A11 disclosure sentence**, one action (Install on Windows; channel instruction elsewhere) | not backdrop-closable while installing |
| **Game editor** | Game row → New custom game… | name/process names/UDP range/dynamic server/notes; per-field violations | guards an unsaved draft |
| **Target picker** | Game row → Target a process | search; loading/empty/no-match rows; Clear target | backdrop/Esc closes |
| **About** | Settings → About | brand mark, version, links | backdrop/Esc/Close |

### 2.7 Copy deck

**A. Shipped delta keys (23 new, already in the catalogs — keep, render per §2).** `state.not_returning` (`No return path`); `route.relay_not_returning`; `route.injection_failed_warning`; `settings.starvation_policy`; `route.server_leg_relay_measured`; `route.server_leg_relay_measured_unvalidated`; `route.server_leg_app`; `route.server_leg_app_measured`; `route.expected_path`; `route.route_trace`; `route.route_trace_hops`; `route.route_trace_hops_only`; `route.location_estimate`; `route.location_provider`; `route.location_hosting`; `route.location_unknown`; `route.location_hosting_hint`; `route.location_geofeed`; `route.location_geofeed_detail`; `route.location_geofeed_note`; `route.location_geofeed_source`; `route.trace_route`; `route.trace_nearest` (`client-gui/locales/en.toml:153,184-211,223-224`). Deleted on master and not to be re-added: `state.direct`, `route.direct_fallback`, `route.retry_in`, `route.server_rejects_relay` (all nine catalogs).

**B. W2 copy changes required by the r10 findings** (one row per key; English source; 9 locales through the pipeline — Q5):

| Key | Shipped / current | W2 value | Why |
|---|---|---|---|
| `route.relay_not_returning` | `Relay is not returning your game traffic - stop optimizing or pick another relay` | `No game traffic is coming back from this relay yet. Stop optimizing or pick another relay; the game's connection restarts.` | C1 (measurement, not verdict) + M16 (remedy cost) |
| `route.injection_failed_warning` | `The relay's replies are not reaching your game - stop optimizing and check the log` | `The relay's replies are not reaching your game. Stop optimizing, then open the log in Settings → Maintenance.` | M16 (in-app log path) + C6 |
| `route.expected_path` | `Expected path ~{ms} ms (L + S)` used as label **and** value | value: `~{ms} ms (relay leg + server leg)`; new label key `route.expected_path_label` = `Expected path` | M15 (placeholder leak, duplicate row) |
| `route.route_trace` | `Route` | `Path trace` | m3 ("Route" has three meanings) |
| `route.location_*` prefix | `Server location:` repeated on 7 keys | label/value block: `Location` label + the rung as the value; hosting hint as a caption under it | M9/m4 (one gutter, 320 px readability) |
| `settings.starvation_policy` | 191 ch (229 de), 5 lines | shortened toward two sentences: warn + the two manual options; the banner carries the short form | §4.1 #9 (one job per sentence) |
| `settings.hold_policy` | `Waits up to 3s…` (wrong: `HOLD_FALLBACK` = 120 s) | `Keeps traffic direct until it identifies the game server, for up to 2 minutes.` generated from `HOLD_FALLBACK` | C5 (40× wrong number) |
| trace hover explanation | hardcoded English (`app.rs:2350-2354`) | catalog key, or a focusable disclosure caption | C6/M8 |
| tray strings | hardcoded English tooltips/menu | catalog keys: `tray.show`, `tray.connect`, `tray.disconnect`, `tray.quit`, `tray.starved` (short form of the warning) | M12 (9 locales) |
| `state.disconnected` | `Disconnected` (unused) | `Not connected` — **wired** | A2, ends dead copy |
| `action.connect` / `action.disconnect` | unused | wired into the Relay row | B2 (one-way disconnect) |
| `action.starting` | — | `Starting…` (action loading) | real `SessionCommandState.in_flight` fact |
| locale honesty | ja/ko/zh-Hans carry **45** English-identical keys = **26 pre-delta + 19 added by the delta** (incl. all new honesty strings) | translate the state/route/location/trace vocabulary through the pipeline (owner: `gui-refresh-brief.md:325`) or re-record the fallback with the real 45-key list; no "9 locales" claim until then | C7/F19 |
| hardcoded English list | `Routing`, Components labels/values, telemetry/port/relay hovers, UAC line, second-instance notice | catalog keys; Components stays the documented exception | §4.1 #18/C7 |

The remaining W1 keys designed but not yet shipped stay as specified: `state.connecting`, `route.relay_ready_auto/follows/pinned`, `route.leg_relay` (metric leg caption), `route.connecting`, `route.holding`, `route.holding_scan`, `relay.try_again`, `notice.needs_admin/restarting/elevation_declined`, `action.restart_network_access`, `action_reason.*`, `error.*` (§2.8), `tray.*`. Every number in copy comes from the constant that owns it; labels never shrink.

### 2.8 Error taxonomy → copy (unchanged from the W1 set)

One `AppError.Kind` → one sentence; the verbatim engine message is behind `Details`/the log:
`Capture`/`Tunnel` → `error.tunnel` (`The tunnel could not start.`); `Route`/`Quic` → `error.registration` (`The relay refused this session.`); `Config` → `error.config` (`Your settings could not be read. LightSpeed is using safe defaults.`, sourced by the W2-1 `ConfigRecovery`); `Game`/`Validation` → `error.game`; `Io` → `error.io`; `Unsupported` → `error.unsupported`. A `Kind` cannot ship without its sentence.

### 2.9 Notice/warning component contract (design-system §6.10) and disabled-reason line

- **One component for both live-session warnings and command errors:** a display-only line/paragraph inside the scrolling region; danger or warn tone; measured max **4 text lines**; one at a time; no dismissal, no motion, no action of its own; the word carries the state and the tint is redundant; the sentence never claims a route change (warn-only).
- **Disabled-reason line:** one `CAPTION` `text_3` row directly under the action whenever it is disabled — first unmet precondition in gate order (relay → connecting → registration → manual address). A control that cannot act says why (M6).

---

## 3. Component usage table

| Component (design-system §) | Where | States that must exist |
|---|---|---|
| **State rail** (shell band) | R1 | word row per §2.1; **metric row** (METRIC + `relay leg` caption), `-- ms` when unmeasured; edge colour redundant; **state-word fit gate: every word in the W2 word set ≤276 px at `STATE` 30 in its installed chain, measured when the catalogs land** (D6/F1/Q20; the 281.1-of-288 measurement was @24.6 px and is retired) |
| **Notice/warning line** (§6.10) | card top (live warning) and scroll top (command error/precondition) | absent · danger tone (starvation) · warn tone (precondition) · error detail with `Details` + `Open log file`; 4-line measured max |
| **Primary action** (§6.2) | R4, every state | default · hover · focus-visible (ring) · active · **danger variant** (stop; §6.2 r11 states) · disabled (+reason line) · loading (`STARTING…` from `in_flight`, `RESTARTING AS ADMINISTRATOR…`) · error. Gate: ready = elevated ∧ connected ∧ `control_registered` ∧ a relay is selected ∧ not boosting; unelevated shows the restart label only when the boost can otherwise start (M1); labels wrap at 15 px (`ACTION_H_WRAPPED` 56), never shrink |
| **Combo** (§6.3) | Relay row, Game row, Settings→Language | default · hover · focus · open · disabled · loading (`Discovering relays…`) · error (visible cause + `Try again`) |
| **Text field** (§6.4) | manual server, proxy add, editor fields, search | default · hover · focus-visible (`selection.stroke` fix) · active · disabled · error (danger ink + word + reason) |
| **Checkbox** (§6.5) | telemetry; editor dynamic server | default · hover · focus · checked · disabled · persist-error line |
| **Sheet** (§6.6) | Settings, Update, Game editor, Target picker | default · loading (update check/install) · error (danger result line) · focus confined + Esc · width 264 |
| **List row** (§6.7) | ledger rows; combo options; process rows; dense relay rows | default · hover · focus · selected (6.70:1) · active · disabled · loading · empty · error |
| **Location ladder** (§6.11) | route card under the map | five rungs; caption rows; display-only; no dot for geofeed/hosting/unknown; hint caption |
| **Route-trace rows** (§6.11) | card caption + `Details` rows | caption when a trace exists; `Path trace` value variants (with/without router RTT); display-only; ≤3 wrapped lines |
| **Map** (painter; tokens) | route card, normal height | tokens replace the **5 literal colour sites** (4 × `from_rgb` + `1 × Color32::WHITE`); marker hit rect 24×24; no motion |
| **Footer buttons** (stock themed) | R6 | default · hover · focus ring · active · disabled; 40 px row normal / `HIT_MIN` 24 compact |
| **Details disclosure** (painted marker, `R_INLINE`-free) | route card, holds the secondary rows | closed/open · focusable · keyboard-operable; the only place hover-only facts move to (M7/M8) |
| **Tile** (§6.8) | not used — one-step Game combo stays | — (deferred) |
| **Sparkline** | **removed in W2 v1** (owner/Lead decision; a min-max shape is not a measurement) | — |
| **Focus ring helper** (§6.1) | every interactive control | 2 px `accent_text` outside the rect; fill unchanged by focus; stock `widgets.active` no longer steals the accent fill |
| **Tray state** (§4 platform) | platform tray | five states incl. `RelayStarved`; menu items enabled from the state; tooltip localized (M12) |

**Ledger row visibility** (default rows; the rest live in the `Details` disclosure — M5 density: **≤6 default rows**)

| Row | Idle | Holding | Relaying | Relaying + warning | Error |
|---|---|---|---|---|---|
| `Relay` | `--` / label | label | label | label (still the route) | frozen |
| `Relay RTT` | when measured | when measured | when measured | when measured | frozen |
| `Server leg` (best source) | hidden | hidden | relay-measured or estimator state | same | frozen |
| `Game server` | `-- start optimizing to place it` | last known / `--` | verbatim `ip:port` | same | frozen |
| `Relayed` | `-- start optimizing to count` | `No traffic relayed yet` | `{sent} up · {returned} down` (warn ink if sent > 0 and returned 0) | same pair — must agree with the warning (C3) | frozen |
| `Path` | hidden | `Holding` | hidden | hidden | frozen |
| `Details` disclosure | absent | hidden | `Server ping`, `Expected path`, `Relay→server (app)`, `Path trace`, estimator fallback row when a relay report leads, cannot-beat arithmetic | same | frozen |

---

## 4. Per-screen platform requirements

Tiers: **B** build, **H** headless, **R** runtime — never collapsed into "parity". This host produces **B** + Windows **R** only; Linux/macOS **R** needs owner hardware or a self-hosted runner (`cross-platform-2026.md` §6.3, §7).

| Screen / surface | Windows | macOS | Linux | Minimum tier | Gap / owner |
|---|---|---|---|---|---|
| **Shell, window chrome, all states** | mobile-only form (320 pinned, height 420–1400/760); remembered height/position; clamp to work area; 200 % proof | same form; traffic lights/app menu; `.app` identity decision | same; DE decorations; `narrow` always true | **R** each | G2.2 r4 (320); packaging G2.4 |
| **Rail/notice/action semantics** | identical copy; UAC wording | `Restart to grant network access`; no "administrator" | same; privilege = `CAP_NET_ADMIN`, not root | **R** each | G1.2, G1.3 |
| **Tray / menu bar** | real `tray-icon`; **five states incl. `RelayStarved`** (`platform/windows.rs:137-158`); localized menu/tooltips (M12) | NSStatusItem; same state vocabulary; template icon | StatusNotifierItem where supported; honest GNOME no-tray | **H** creation + **R** interaction | G1.4/G1.5; Q14 |
| **Shortcuts** | Esc closes sheet, Enter confirms, Ctrl+Q/Ctrl+, | ⌘Q/⌘,/⌘H (tray only) | Ctrl equivalents | **R** each | G2.1 |
| **Elevation flow** | token check; handshake before exit; declined-prompt copy | scoped authorization, config paths forwarded | `CAP_NET_ADMIN` probe, `pkexec` forwarding `HOME`/`XDG_*` | **H** decision + **R** flow (**U** here) | G1.2/G1.3 |
| **Autostart** | opt-in `HKCU\…\Run` | LaunchAgent | XDG autostart | **H** writer + **R** login | G1.7 |
| **Updater sheet + disclosure** | in-place install; A11 sentence; version normalized | channel truth; disclosure; no dead install button | same; AUR instruction | **H** + **R** | G1.6; Q18 |
| **Location ladder** | tables ship in-app; display-only | same; no OS call | same | **H** + Windows **R** | G1.1-adjacent (S5a); Q11 |
| **Route trace (client)** | `tracert` via `silent_command` (`client_trace.rs:349-375`) | `traceroute -n -q 1` — needs `traceroute` in PATH | same | **H** + Windows **R**; macOS/Linux runtime **U** | parity §3.4; Q13 privacy |
| **Reduced motion (WCAG 2.2.2)** | W2-4 (i): `SPI_GETCLIENTAREAANIMATION`, `On` shows spinner only, `Off`/`Unknown` static | `NSWorkspace…` **gap** → `Unknown` = static | gsettings/KDE **gap** → `Unknown` = static | **H** + Windows runtime check | Q4/D5; pre-W2-2 spinners are the exposed hazard |
| **Single instance / paths / crash** | unchanged | visible second-launch notice; `.app` paths | visible notice; `xdg-open` | **H** + **R** | G1.8/G1.9 |
| **Notifications** | **no-claim** (`gui-refresh-brief.md:327`) | same | same | — (documented) | G2.3 |

Copy rule: the app may not promise a tier an OS has not reached; `docs/user-guide.md:31` is retiered in the same change (G0.4).

---

## 5. W2 work units (5; one `app.rs` writer at a time)

The rebase makes W2-3 much larger; the boundary proposes splitting it (Q12). This spec **adopts the split** and defines five units. The **one-writer rule stands**: W2-1 edits `app.rs` call sites/getters only (never `impl eframe::App::ui`); W2-2 owns `ui()` and the drawing; W2-3a/W2-3b edit orchestration only after W2-2, rebased on the new shell; W2-4 touches `platform/*`/workflows and no `app.rs` except tray-action wiring coordinated with W2-3a.

### W2-1 — "Spine": DTO v2 + presentation adapter (owner `rust-dev`)

Scope: core-boundary **S0** (guard G1–G7) + **S1** (config; retired `retry_after_fallback` tolerated on read, never written) + **S2** (discovery/probe/identity) + **S3a** (status DTO v2: `AppStatus` incl. `location`, `server_leg`, `route_trace`, `tray`, `watchdog.warning`, `relay.carry`, `fallback`; moved `tray_state_for`, starvation-filter ranking, measured-leg readers, `relayed_row_text` starving flag) + the four §2.1 fact paths + the `route_surface` adapter. No visual change; shell untouched.
Write scope: `client/src/app/**` (new modules incl. `dto`, `status`, `config`, `discovery`, `relay`, `error`), **`client/src/lib.rs`** (`pub mod app;` + re-exports — `gui-core-boundary.md:157,197`), `client/src/server_leg.rs` re-export surface only (the file itself is S5a's), `client/tests/gui_boundary.rs` (new), `client-gui/src/config.rs`, `discovery.rs`, `main.rs` (config/font call site; the provider install stays until S5a), `client-gui/src/app.rs` (config/discovery call sites + getter reads only), `client-gui/src/app/{choice,ledger,relay_choice,window}.rs` (rule halves), `app/route_surface.rs` (new), `app/tests.rs` (moved clusters + fixtures), evidence dir.
Acceptance: (a) guard G1–G7 green with mutation proof; (b) one field path per **amended** contract row — §2 is the **contract of record** for this slice (A1–A13; A14 is prose-only and does not change rows), graded before Q3's approval is needed; Idle, Connecting, Holding, Choosing, Relaying, No return path (warning + carry + streak), Needs elevation overlay, Error, 4 estimator server-leg rows, the relay-measured rows, location status, route trace, tray — with both-directions tests; (c) the four fact paths exist; (d) `route_surface` pure adapter, a fixture per rendered row (Relay recovered absence assertion included) and the warning/clear transitions (`WatchdogPlan` fixtures); (e) `Relayed` one source pair; (f) legacy key preservation, counts move; (g) **no visual change**: the pre-slice baseline is captured as `wat/evidence/<date>/w2-1/baseline-320-{connected,disconnected}.png`, the live-metric mask rects recorded in `baseline-mask.json`, pass = ≤0.1 % changed pixels outside the mask and an identical `uia.json`; (h) baseline re-measured at S0: the r8 counts are stale (`gui-core-boundary.md` §6 — 264 GUI tests at the rebase).
Gate: repo gate with per-crate counts; guard in the existing workspace job.

### W2-2 — "Instrument 2.0 shell, mobile-only" (owner `interface-designer`)

Scope: §1 shell (mobile-only, rail metric row, tiers, persistence, 200 %), §2 rows/copy/components (warning line, location ladder, trace caption, ledger density + `Details`, update disclosure), §3 components, **`interface.md` edits A1–A14** (A14 = the non-state stale prose, Appendix C), token adoption (`design.rs`, `ACTION_H_WRAPPED`, 9 deletions/7 re-roles), theme fixes (focus ring, `selection.stroke`, `border_control`, active/pressed, danger variant), a11y semantics; renders `route_surface`, derives no state.
Write scope: `client-gui/src/app.rs` (`ui()` + drawing, **not** call sites/orchestration), `app/{theme,ledger,widgets,window}.rs`, `globe.rs`, `app/tests.rs`, `design.rs`, `locales/en.toml` (+ 8 catalogs), `i18n.rs` keys, `docs/design/interface.md`, evidence dir. It consumes `route_surface.rs` and does not edit it.
Acceptance: (a) mobile-only form (320 pinned, height persisted/clamped, no auto-resize); (b) **320×420, 320×760, 320 pt @ 200 %** captures in **EN + DE + RU** (RU is the binding long-word/long-label case; F6), no clipping, the §1.3 chrome/scroll table verified (worst **239/181** at 420), the **state-word fit gate** (**every word in the W2 word set** — incl. `state.connecting` and the new `state.disconnected` value — ≤276 px at `STATE` 30 in its installed chain, measured when the catalogs land; D6/Q20), no state-word/metric occlusion; (c) every §2 row rendered from `route_surface`, shell derives nothing; (d) component states incl. the **notice/warning line**, location rungs, trace rows, `Details` disclosure, UIA-read action name, selected-row 6.70:1, `border_control`, opaque focus `selection.stroke`, danger action variant, `TrayState::RelayStarved` mapping passed to the platform; (e) error panel `Details` + `Open log file`, disabled-reason line, Relay row Connect/Disconnect, `STARTING…` loading/error states, sheet close policy; (f) in-crate contrast test: every D1 pairing meets its **post-fix floor**, incl. selected-row **6.70:1** — the measured 1.35:1 is the before-state and must fail the test under mutation — plus the notice rows; (g) keyboard: Tab reaches every control, ring visible, **Tab confined to sheets, Esc closes and returns focus, Space/Enter activate the action**; (h) sparkline removed, five map literal sites tokenized, glyph gate extended to `native_name` + painted marks; (i) reduced-motion seam (no OS read; `Unknown` → static text); (j) §6 reachable rows; (k) C7 honesty record: the keys still shipping English (**45 = 26 pre-delta + 19 delta keys**) and their translation state recorded in `wat/evidence/<date>/w2-2/locale-fallback.md` (**no new file inside `client-gui/locales/`**; the write scope is unchanged); no "9 locales" claim until Q5 lands.
Gate: repo gate + shell tests. Verification: `qa` runs the §6 matrix, keyboard/UIA transcript, recorded contrast output.

### W2-3a — "Warn-only runtime" (owner `rust-dev`)

Scope: **S3b** commands (`SessionCommand`/`AppCore`) + **S4** warn-only watchdog/history (`WatchdogPlan`, `RelayStarvationWarning`, `ReplyFaultStreak`, history demotion + `CarryVerdict`), behaviour-frozen.
Write scope: `client/src/app/{commands,watchdog,history}.rs`, `client-gui/src/app/{watchdog,relay_choice}.rs` (moved), `relay_history.rs` (moved), `app/race_watch.rs` removed, `app.rs` orchestration (**not** `ui()`, **not** `route_surface.rs`), `client-gui/src/app/tests.rs` (moved warning/never-switch tests), guard allowances, evidence dir.
Acceptance: (a) the warn-only tests move **before** the thread: `app/tests.rs:155-185,188-216,265-291,322-360,362-380,383-425` stay green and the "never switches" assertion is unchanged (`WatchdogPlan::switch_to_direct` always false); (b) the warning's raise/clear rules are byte-identical to master (behaviour freeze rule 8, `gui-core-boundary.md` §6); (c) `route_surface` fixtures identical before/after the `WatchdogFacts` swap; (d) tray state ordering preserved (`RelayStarved` never Optimizing); (e) commands return `AppError` (Error row source); (f) fallback-warning surfaces unchanged where reachable, named unverified where not.
Gate: repo gate + Linux CI workspace tests; per-crate counts.

### W2-3b — "Location + trace + CLI" (owner `rust-dev`; `infra-dev`/`dev-ops` coordinate)

Scope: **S5a** location ladder + provider seam + data/feature/generator + `paths.rs`; **S5b** client trace; **S6** CLI adoption + guard ratchet to zero; the update-rule extraction and the capability DTO for W2-4.
Write scope (**S5a landed 2026-11-20**: data at `client/data/`, location modules at `client/src/app/location*`, GUI `server_place.rs` is an 11-line shell): `client/src/app/{location,server_leg,trace,presence,game,geo,update,capabilities}.rs`; `client-gui/src/client_trace.rs` moved/thinned; `client/data/server-*.bin` + `server-*.sources.json`; `client-gui/src/{update,self_update}.rs` removed/thinned; `client-gui/src/paths.rs` (SPLIT: config-dir moves; log/crash/open helpers stay); `client-gui/src/main.rs` (provider install deleted after `AppCore` installs it); `client-gui/src/app/{editor,games}.rs` moved; **`app/choice.rs`'s text-builder residue STAYS as presentation (owned by W2-2) — only its rule halves moved in W2-1, so nothing is "moved" here (F3.3)**; `infra/geo/geofeeds/**`; **generators: `infra/scripts/build-server-location.py` (cloud/hosting blobs, `:1-14`), `infra/scripts/build-server-city.py` (geofeed table/manifests, outputs `:54-69`), `infra/scripts/build-geo-ipv4.py` (country blob, `:38-39`)** — the generators are **infra-owned**: `infra-dev` regenerates and commits the artifacts, `dev-ops` only wires CI, `rust-dev` moves the modules; **features: `server-location`** = the three server indexes + tables/manifests + the moved `app::location`; **`geo-ip`** = only the country blob/table behind `geo.rs`, the ladder's last rung (off ⇒ ladder ends at `Place::Unknown`); both enabled by `client-gui`, covered by `full`, shipped CLI defaults off (`gui-core-boundary.md` §3.2.2 r15); `client/src/main.rs`/`modes/*` call sites; `client/tests/cli_compat.rs` + goldens; guard allowances; evidence dir.
Acceptance: (a) **normalized golden-file CLI comparison** written before S6 (`--status`/`--probe-proxies` masked live values; `--list-games`/malformed `--config` exact; reviewed golden updates — QA R4); (b) guard G1–G7 allowance empty at S6 except named adapter exceptions + **S6 mutation proof**; (c) **display-only proofs**: `chooser_point` returns `None` for Geofeed/Hosting/Country and no ranking path takes `place()`/`display_point` (G6), the moved `server_place` test `a_geofeed_place_never_steers_the_ranking` (`server_place.rs:456-466`) stays; (d) **seam proof**: `install_server_provider_lookup` exists in `client/src/app/**` and nowhere in `client-gui` (G7), classifier installed before engine creation; (e) feature-gated data: the two features are measured separately — `server-location` (the three server indexes) and `geo-ip` (the country blob only; off ⇒ the ladder ends at `Unknown`); the artifact delta of each is recorded with the generator-run source-manifest hashes requested from `infra-dev`, fallback F-F if unacceptable; (f) client trace: scheduler/parser/guard/`info_line` move with the **20 runtime tests** (the 4 caption tests stay in the GUI presentation shell; QA R3 errata), one log-line golden, no ranking input; (g) S5 config-file fixtures old/new/both, Q10 precedence, never deleted, release-note line; (h) update pure rules moved (`compare_versions`/`parse_sha256_file`/`ReleaseAsset`), no new client deps; (i) capability DTO for W2-4; (j) probe-semantics/presence decisions only if approved.
Gate: repo gate + Linux CI for the moved tests; per-crate counts.
**Blocked until Q13 (traceroute disclosure) is answered for S5b.**

### W2-4 — "Parity" (owner `desktop-platform`; `dev-ops` for workflows)

Scope: `cross-platform-2026.md` r4 G0.1, G0.2, G1.1–G1.9, G2.1 + reduced-motion Windows signal + the `RelayStarved` tray on real platforms + the `client_trace` runtime dependency note. Named outside W2 v1: G2.2 per-OS DPI proof (Windows proof is W2-2 (b); macOS/Linux need hardware), G2.3 no-claim (owner), G2.4 packaging (owner/`dev-ops`).
Write scope: `client-gui/src/platform/**`, i18n platform variants, `Cargo.toml` dependency decisions (after `sec-ops`), `.github/workflows/ci.yml` (`dev-ops`), platform copy/docs retiering, evidence dir.
Acceptance: (a) `linux-gui` + `macos-gui` PR jobs green with per-OS counts (both non-Windows single-instance tests appear); (b) headless smoke exit 0 with title/log probe, advisory until proven; (c) tray/menu incl. the fifth state; (d) shortcut keymaps; (e) elevation handshake state machine unit-tested; **UAC-decline interactive = U**, named fixture; (f) autostart temp-dir writers; (g) updater channel decision (no dead install button); (h) three-tier statement per surface; (i) reduced motion Windows `SPI_GETCLIENTAREAANIMATION` (spinner only on `On`; `Off`/`Unknown` static; macOS/Linux `Unknown` gaps); (j) `client_trace` runtime dependency (`traceroute` on Linux/macOS) recorded in the parity report.
Gate: repo gate (Rust), `localization-sync --check` (`.github/workflows/ci.yml:216-217`), new jobs.

---

## 6. Required screenshot matrix (320 px only)

Widths are logical points: **320×420** (min, short+compact), **320×760** (initial), **320 pt @ 200 %** (the same window rasterised at 640 physical px). **R** = reachable here with the named fixture; **U** = unreachable → report unverified with the fixture named; **—** = no surface. No row may be **R** without its fixture. All captures: **EN + DE + RU** (RU is the binding long-word/long-label case — the ≤276 px state word and the 2-line restart label; F6; ES/PT-BR join once their shortened state words land (Q20); 45 English keys in ja/ko/zh remain per C7).

| State / surface | 320×420 | 320×760 | 320@200 % | Fixture / note |
|---|---|---|---|---|
| Idle, disconnected | R | R | R | live delta capture `window-connected-320-delta.png` is the connected counterpart |
| Idle, connected (Auto, no game) | R | R | R | live (delta capture) |
| Idle + detected game | R | R | R | synthetic detectable process; display name, never the key |
| Connecting | R | R | R | unroutable relay address (brief window); error row follows |
| Holding — no server locked | R | R | R | synthetic process + WinDivert |
| Choosing (HOLDING sub-line) | **U** | **U** | **U** | needs a locked game server; adapter fixture covers it |
| Relaying — real game traffic | **U** | **U** | **U** | live game session through a relay; synthetic process relays nothing |
| **`NO RETURN PATH` + warning line (both fault copies)** | **U** | **U** | **U** | needs a live starving relay (forwards, returns nothing) — same limitation as W1; banner/layout computed in `delta-copy.md`; code + tests cite the states |
| Relay recovered (no banner) | **U** | **U** | **U** | cleared-warning state needs the same fixture; assertion is the absence |
| Location ladder — cloud region | **U** | **U** | **U** | needs a server in a cloud range with a live session |
| Location ladder — operator geofeed | **U** | **U** | **U** | geofeed label was live-verified earlier (`wat/state/current-phase.md:113`); the rendered block is not captured |
| Location ladder — hosting network + hint | **U** | **U** | **U** | needs a hosting-range server (the G-Core class) |
| Location ladder — country estimate | **U** | **U** | **U** | needs a non-cloud, non-hosting server |
| Location ladder — unknown | **U** | **U** | **U** | needs a server with no country match |
| Client route trace caption | **U** | **U** | **U** | needs a locked server with traffic; `CLIENT_TRACE` log sample exists (`wat/state/current-phase.md:143`) |
| Relay-measured `Server leg` / `Expected path` / app leg / relay `Path trace` | **U** | **U** | **U** | need live relay reports for the pair; rows unit-tested |
| Needs elevation | **U** | **U** | **U** | agent harness is elevated; fixture = non-elevated interactive desktop (owner/QA) |
| Error: start refusal | R | R | R | exe folder without `WinDivert.dll` |
| Error: registration refused | **U** | **U** | **U** | needs a refusing relay |
| Settings — Routing (warn-only policy) | R | R | R | live delta captures `window-settings-routing-320-delta.png` (+DE) |
| Update sheet — up to date + disclosure | R | R | R | up-to-date branch live; disclosure sentence is new copy (W2) |
| Game editor (validation error) | R | R | R | keyboard-only pass too |
| Target picker (loading/empty/no-match) | R | R | R | — |
| Language picker (nine names) | R | R | R | glyph check (D4) |
| Tray menu/tooltip (four reachable states) | R (screen capture) | — | — | idle/connected/optimizing/error; menu items enabled from the state |
| Tray `RelayStarved` icon/tooltip | **U** | **U** | **U** | needs the starving fixture; the icon colour/tooltip is a unit-level mapping only (Q14) |
| Second-instance notice | R | — | — | — |
| Focus ring + action AccessKit name | R | R | R | name **read from the UIA tree** |
| Shell at 200 % with no clipping | — | — | R | also one compact capture if the work area forces it |
| **Rail metric row + state-word fit at 320** | R | R | R | the delta's fixed collision and the F1 copy constraint; **captures in EN, DE and RU** (RU `Нет возврата`; the ≤276 px fit gate checked in the image review) |

Rules: read the image, not the code; a case that cannot be set up is **unverified** with its fixture named — never fabricated. Save under `wat/evidence/<date>/qa/` with state + width in the filename; the UIA dump beside the focus rows. The **warning/location/trace rows are the delta's honesty surfaces and are all U here** — that is the honest position, not a defect of the matrix.

---

## 7. Decisions

| # | Decision | State |
|---|---|---|
| Q1 | Window geometry | **RESOLVED (owner 2026-11-20): mobile-only** — 320 pinned, single column, height 420–1400 initial 760; §1.1; Appendix C A12 |
| Q2 | Rail metric | **RESOLVED (Lead-adopted + A13):** the metric is the relay leg, visibly labelled `relay leg`; the direct ping stays in the `Server ping` detail row with its age; the metric gets its own rail row (§1.2) |
| Q3 | Contract amendments | **A1–A6 authorised; A7–A14 drafted in Appendix C and must be approved** (A14 = the non-state stale prose) so W2-2 can apply them to `interface.md`. W2-1 (b) grades against **§2 as the contract of record**, so it does not wait for the edit (F23, boundary r15) |
| Q4 | Sparkline | **RESOLVED (Lead-adopted): removed in W2 v1** |
| Q5 | Locale completeness | **RESOLVED (owner): translate through the in-repo pipeline** (`gui-refresh-brief.md:325`). The real set is **45** English-identical keys in ja/ko/zh = **26 pre-delta + 19 added by the delta** — the owner's "26" is the pre-delta count, not a contradiction (F19). W2-2 (k) records the remainder in its evidence dir and no "9 locales" claim ships until zero |
| Q6 | Tray/packaging | **RESOLVED (owner):** macOS menu bar + Linux SNI where supported, honest GNOME no-tray, bare tarball/shell installer, no notarization, no `.app`/`.dmg` in v1 |
| Q7 | Notifications | **RESOLVED: explicit no-claim** |
| Q8 | CLI runtime adoption / probe semantics | **RESOLVED (Lead-adopted): deferred** |
| Q9 | Presence policy | **RESOLVED: GUI policy is the product canon** |
| Q10 | Config precedence | **RESOLVED (owner):** canonical config wins, legacy GUI file preserved, one release-note line |
| Q11 | Location-data packaging (boundary Q2/Q11 + F9) | **two feature gates with one meaning each:** `server-location` = the three server indexes + tables/manifests + the moved `app::location`; `geo-ip` = **only** the country blob/table behind `geo.rs` (off ⇒ the ladder ends at `Place::Unknown`); both enabled by `client-gui`, covered by `full`, shipped CLI defaults off. **Three generators:** `build-server-location.py` (cloud/hosting), `build-server-city.py` (geofeed), `build-geo-ipv4.py` (country). **Owner split:** `rust-dev` moves modules+gates; `infra-dev` owns the generators/committed artifacts; `dev-ops` wires CI. W2-3b decides the final asset home with the measured artifact delta — Lead + `infra-dev` |
| Q12 | W2-3 split | **adopted here as W2-3a + W2-3b** (§5); the Lead may merge them back, but the behaviour-frozen S4 must land as its own unit |
| Q13 | **Traceroute disclosure** (blocker for S5b) | the client runs an OS traceroute automatically; `docs/privacy.md` lists ICMP/shadow measurement but **not** this (`docs/privacy.md:83-99,178-184`). Owner/comms decision: disclose it in the privacy policy + data dictionary before S5b ships, or name it unverified and keep the caption out. **`[PRIVACY_STUB]` applies** |
| Q14 | Tray `RelayStarved` shell/platform scope | W2-2 maps the fifth state and localizes it; macOS/Linux tray backends are stubs → the state cannot be observed there (Windows **R**, macOS/Linux **U**); the parity report says so (no new gap row; boundary Q14) |
| Q15 | RU restart action label | **RESOLVED (ux-lead/owner): shorten the RU copy to two lines** — `Перезапустить от имени администратора` (356.9 px @ 15) is **landed** in `ru.toml` and covered by `ACTION_H_WRAPPED` 56 with **no chrome change**. The widened 73 px slot is **rejected**: its band is 73 + 2×S2 = 89, so the 420 compact worst becomes chrome **256 → scroll 164 < the 180 floor** (§1.3, F8). The dropped "to optimize" purpose belongs in the elevation overlay's reason line, not a third action row. ES/PT-BR state words are **Q20** (approved, land in W2-2) |
| Q16 | Metric row vs design-system §5.4 chrome | **RESOLVED:** the spec's table is authoritative — **239/181 worst at 320×420** (235/185 only if the reason row omitted its S1 gap); `design-system-2026.md` §5.4 r14 now adopts it and deletes the "27 px to spare" claim (C-17 closed). W2-2 measures in a real window |
| Q17 | Starvation banner / policy copy shortening | C1 + M16 + `settings.starvation_policy` length: the W2 values in §2.7 B are the spec's decision; `ux-lead` may shorten toward the 2-line target without changing the claim. The relay-side counters must not be cited until measured |
| Q18 | Update-sheet disclosure sentence | A11 requires the sentence; `docs/privacy.md:184-204` already documents the behaviour — W2-2 adds the in-app key and must not promise cancellation (M8 nuance: closing stops the UI, not the parked request) |
| Q19 | Bundled-font identity (`y`, `{`, JBMono ASCII) | re-subset the OFL fonts or accept the measured fallback mixing (`design-system-2026.md` §8.2/§11.5); **owner**, a font-asset change |
| Q20 | **State-word copy owner + W2 word-set gate (R8)** | The ES/PT shortened words and the W2 word-set translations (`state.connecting`; the changed `state.disconnected` = `Not connected`) are owned by **`ux-lead`** under the Q5 pipeline decision. ES `SIN RETORNO` 176.8 / PT-BR `SEM RETORNO` 178.7 are **approved and land in W2-2** with the catalogs (they do not block the shell starting), and **join the §6 captures once landed**. The ≤276 px gate is measured on **every word in the W2 word set when the catalogs land** — not only on the current catalogs. Until then the current-tree ES/PT words (309.0 / 328.4) are recorded **pending**, never passed; RU `IDLE` retires with A2 and DE `OPTIMIZING` 275.1 already passes by 0.9 px. Lead records the decision; W2-2 verifies |

---

## Appendix A — historical critique (pass 1): the 16 required changes

| # | Required change | Status | Where satisfied / why |
|---|---|---|---|
| 1 | Elevation path, one slot three labels | S | §2.2 row 7; §3 primary action |
| 2 | Disconnected card honest | S | §2.2 row 1 + ledger matrix |
| 3 | Privacy/Maintenance/About/Disconnect destination | S | §2.6 Settings + Relay row Connect/Disconnect (B2) |
| 4 | Retire dashed-meta copy | S | §2.7 copy deck (no `WORD - fragment`); the remaining `-` strings are on the C1/M16 fix list |
| 5 | Ledger-key case, delete `ROUTE` eyebrow | S | sentence-case labels; §3 |
| 6 | Complete the token scale | S | D2; mobile-only token set; `ACTION_H_WRAPPED` |
| 7 | Height breakpoint | S | §1.4 short+compact at mobile-only; **the retired width breakpoints are gone** (owner decision, A12) |
| 8 | Game choice one step; deliberate picker | S | §3 one-step combo; tile deferred |
| 9 | Relay identity from real data | S | ledger `Relay` label/address; no city |
| 10 | Rail + sparkline one colour rule | S | sparkline removed (owner); rail edge redundant (§1.2) |
| 11 | Migrate all literals | S | five literal colour sites (4 × `from_rgb` + `Color32::WHITE`), W2-2 (h) |
| 12 | Accent/ink contrast rule | S | D1; `accent` fill-only, `accent_text` 7.51–8.11:1 |
| 13 | Do not claim reduced motion | S | D5; the signal is assigned (W2-2/W2-4) and the claim is absent |
| 14 | Shadows off | S | shipped; kept |
| 15 | Clamp and scroll every sheet | S | §2.6/§3 sheets; width 264 |
| 16 | Ledger mechanics in px | S | `ROW_H` 24; one label-above-value column at 320 (the retired `LEDGER_LABEL_W` is gone) |

## Appendix B — r10 audit findings (4 Blockers, 8 Criticals, 16 Majors, 8 Minors)

| ID | Finding | Status | Where |
|---|---|---|---|
| B1 | Action unnamed, no visible focus | S | §3 primary action; W2-2 (d)(g); live UIA read required |
| B2 | Disconnect one-way | S | §2.2 row 1 Relay row; `action.connect` wired |
| B3 | Failed boost silent | S | Error row + §2.8 + `last_error` fact |
| B4 | Registration invisible | S | Connecting/Error (control_registered/registration_error) |
| C1 | **Starvation banner blames the relay for an unestablished fault** | S | §2.2 row 6: copy states the observation; relay-side counters not cited until measured |
| C2 | `OPTIMIZING` while held | S | A1 + §2.1 order (HOLDING before OPTIMIZING) |
| C3 | `Relayed` impossible pair contradicts the warning | S | A5 one source pair; §3 row must agree with the warning |
| C4 | Headline metric unlabelled/changes meaning | S | A13 + §1.2 metric row labelled `relay leg` |
| C5 | Hold duration wrong 40× (retest copy fixed on master) | S | §2.7 B `settings.hold_policy` from `HOLD_FALLBACK` |
| C6 | Internal strings as copy | S | §2.8 taxonomy + `Details`/log; trace hover catalogued; scan sentence relocated |
| C7 | 9-locale claim untrue (now 45 keys) | D | §2.7 B + W2-2 (k): record the remainder; owner decided to translate (Q5). Reason: translation is pipeline work |
| C8 | Elevation exits before UAC resolves | S/U | §2.2 row 7 handshake; interactive decline **U** (fixture named) |
| M1–M4 | elevation CTA, pick-your-game contradiction, raw game key, width/auto-height | S (M4 re-scoped) | M1/M2/M3 in §2/copy; M4 = §1.1 height persistence + no auto-resize |
| M5 | **Route-card density at 320** | S | §3 ledger rule: ≤6 default rows + one `Details`; location compact; trace in `Details` |
| M6 | Disabled action never explains | S | §2.9 disabled-reason line |
| M7/M8 | Hover-only facts; new geofeed/trace hovers | S | §2.4/§3: captions + `Details`; geofeed source is the only allowed hover |
| M9 | Centred prose breaks the one gutter | S | §2.4 location block left-aligned; §1.2 one gutter |
| M10/M11 | <24 px targets; destructive actions unconfirmed | S | §3 list rows/delete target; §2.6 confirm/undo |
| M12 | Tray English-only, lying menu, fact-free error | S | §2.7 tray keys + §4 tray row (`RelayStarved` honest) |
| M13 | Update flow dead-ends / raw internals | S | §2.5 + §2.8 + §2.7 version normalization |
| M14 | Traceroute not in the privacy network list | **open decision** | Q13 — blocker for S5b; the caption does not ship before it |
| M15 | `Expected path` label/value collision leaks `{ms}` | S | §2.3 label/value split |
| M16 | Starvation remedy omits cost/log path | S | §2.2 row 6 copy |
| m1–m4 | version spelling, separators, "Route" meanings, repeated `Server location:` | S | §2.7 B / §2.3 / §2.4 |
| m5–m8 | sparkline, ghost hint, 5 literals, disclosure sentence stronger than code | S | removed / port-range hover deleted / W2-2 (h) / Q18 |

## Appendix C — `interface.md` amendments A1–A14 and conflicts resolved

Authority: owner decisions + measured evidence > the contract as amended. **`interface.md` is not edited here** (write scope); **W2-2 applies A1–A14** in the same change as the shell (§7 Q3). The shipped code is the evidence for what the contract must say. Amendment numbering: A1–A6 were recorded by the W1 set; **A7–A11 are the architect's r9 proposal**; **A12–A13 are the UX/r10 slots re-cut here**; **A14 is the non-state stale-prose set (QA F5)**; the r10 audit's internal A7–A13 letters are folded as mapped below.

### A1–A6 (recorded; still authorised)

| # | Contract location | Change | Rationale |
|---|---|---|---|
| A1 | Holding (`interface.md:33`) | driving fact = `EngineStatus.tunnelling_enabled == Some(false)` while a session runs; drop `direct_fallback + sent == 0`; `is_holding` is deleted on master | the fallback proxy no longer exists |
| A2 | Idle (`:31`) | split into `NOT CONNECTED` / `CONNECTED`; `NOT CONNECTED` wires the unused `state.disconnected`; fact `engine.connected`; note the shipped word is `NOT OPTIMIZING` until the shell lands | two shipped words for one row |
| A3 | Error (`:41`) | a localized taxonomy sentence leads; the engine's verbatim message is one `Details` disclosure/log away | honesty + one voice |
| A4 | Choosing (`:34`) | a sub-state of the HOLDING word (`relay.choosing`), not a distinct word | the merge was undocumented |
| A5 | Relaying (`:35`) | driving fact = the `run_boosting()` gate + **one** counter-source pair, not "interceptor counters" | C3 contradiction |
| A6 | Needs elevation (`:40`) | an overlay driven by `Capabilities.elevated`; `RouteState::NeedsElevation` removed, the word is never `NeedsElevation` | one fact path |

### A7 — Warn-only starvation replaces the auto-fallback rows (architect A7 + r10 A7–A9/A11)

- **Retire** `interface.md:36` Direct fallback, `:37` Re-test pending, `:38` Server rejects relay, and the story paragraph `:43-46` ("goes direct → countdown → recovered → rejects"). Their keys and fields are gone from master.
- **Add** a `NO RETURN PATH` warning row: word = `No return path` (danger) **only while boosting**; driving fact `RelayStarvationWarning {server, relay, fault}` raised after `RELAY_REPLY_STREAK` = 2 consecutive same-fault judged windows; cleared only by a fully healthy window; per (server, relay); the session **never changes route** (warn-only, owner 2026-11-20); the relay is demoted for future ranking; tray `RelayStarved`; copy states the observation, never the relay's fault beyond it.
- **Reword** `:39` Relay recovered: the warning clears and the word returns to OPTIMIZING; no banner is invented (the old "trace-only, falls back to Relaying" sentence is replaced by "the warning clears when a healthy window proves the return path; no recovered banner").
- **Update** the honesty rules: keep `:138-141` ("claim only what the client measures") and add that starvation is a client-side observation, relay-side counters only when measured.
- **Mapper note:** r10's A7 (Holding fact) is A1; r10's A8 (delete rows) + A11 (add the warning row) are A7 here; r10's A9 (Relay-recovered reword) is A7 here.

### A8 — Honest location ladder is display-only (architect A8 + r10 A10 part)

- **Add** the location surface to the route card: five mutually exclusive rungs Cloud / Geofeed / Hosting / Country / Unknown, each naming its provenance; hosting/unknown carry the unreliable-database hint; geofeed is shown as-is with source/as-of.
- **Rules that are contract facts:** only `chooser_point` (Cloud-exact) may feed the ranking; geofeed/hosting/unknown draw **no dot**; no city a source cannot produce; the ladder is display-only.

### A9 — Relay-measured server leg is display-only until the E1 gate (architect A9 + r10 A10 part)

- **Add** to the server-leg section: relay ICMP `S` (`measured` / `measured, unvalidated`), passive app-RTT, relay TTL trace, `Expected path ~{ms} ms (L + S)` as a **lower bound**; the four v1 estimator rows remain the fallback when no relay report exists.
- **Rules:** display-only; not a ranking input until `docs/design/relay-server-leg-validation.md:48-51` passes; `unvalidated` is a required qualifier (fail-open); never a whole-path verdict.

### A10 — Route trace / placement anchor (architect A10 + r10 part)

- **Add** the client route-trace surface (caption + `Path trace` row): hops and last responding hop RTT only; not the game's ping; no city; display-only; the trace exists only while a trace for this server exists; the disclosure question (Q13) precedes its shipping.

### A11 — Update-check disclosure is a product contract (architect A11)

- **Add** the update-check disclosure: the check is user-initiated only (no startup/background check), contacts the GitHub release API, sends nothing but the request; the sheet states it; `UpdateStatus` is produced only by a user action.

### A12 — Mobile-only geometry replaces the layout section (r10 A12; range corrected per F22)

- **Replace the layout rules** in `interface.md` §Layout (`:145-174`, whose stale parts are the 380/619/620 breakpoints at `:150-157` and the 360×420 minimum at `:164-170`) with the owner's form: width pinned **320 px**, one single column, height user-resizable **420–1400, initial 760**; one width layout; captures **320×420, 320×760, 320@200 %**; the 180-px scroll floor restated with the spec's chrome table; no wide screenshots.
- **Range correction:** the **435/1270** capture strings are **not** in `:145-174` — they are at `interface.md:18` and `:249-250`; `:172` is the `435x720 / 720x610` proof. A12 covers the layout rules only; **A14** edits those strings. (Same mis-citation corrected in `gui-ux-audit-2026.md:119` by its owner; F22.)

### A13 — Headline metric (r10 A13)

- **Decide:** the rail metric is the **relay leg**, visibly labelled (`relay leg` caption); it is **not** the game's ping; the direct ICMP ping lives in the `Server ping` detail row with its age and server key. The metric occupies its **own row** below the state word (layout, not contract). The shipped "direct ping when fresh" headline reverts to the contract in W2-2.

### A14 — Non-state stale prose (F5; apply in the same W2-2 change as A1–A13)

A1–A13 fix every route/server-leg row, but the binding contract's **method and verification prose** still instructs the pre-delta product (`review-r4.md` §1.6's seven items plus the three passages `review-r5.md` R3 finds in the amended sections). W2-2 must apply these too, or it leaves the exact staleness the delta exists to remove.

| `interface.md` | Stale text | Required edit |
|---|---|---|
| `:18` | Method step 4 requires captures in "`narrow (~435 px) and wide (~1270 px)`" | replace with the **320-only** capture set: 320×420, 320×760, 320@200 % (A12) |
| `:24` | "**Eleven route states**, plus the four server-leg states" | **nine** route rows — A7 retires three and adds the `NO RETURN PATH` warning row — and the server-leg section as amended by A9 |
| `:142-143` | "`Relay recovered` is trace-only … the window falls back to the `Relaying` sentence" | align with A7: the **warning clearing** is the observable effect; no recovered banner and no rail word |
| `:238-240` | "The **seven** new `route.server_leg*` / `route.cannot_beat_direct` catalog strings ship as English in all nine locales" | the measured set is **45** English-identical ja/ko/zh keys (**26 pre-delta + 19 added by the delta**), with the Q5 translation decision recorded |
| `:247-251` | "the 435/1270 px captures in the layout spec need a resizable build and are not reachable in this one. State that in the report…" | retired: those widths are permanently withdrawn (A12); the honest set is the 320 captures |
| `:254` | "start a boost, **let the watchdog fall back** and capture it" | warn-only: there is no fall-back state. Force the **warning** only with a starving fixture; otherwise mark the capture **U** (no invented state) |
| `:257-261` | server-leg captures: "relayed **with** one only if the v1 estimator has a production feed in the build being captured" | pre-delta: the primary measured branch is now the **relay's own report** (A9); the estimator is the fallback; keep the "never fake a measurement" rule |
| `:53-55` | server-leg intro: "There is exactly one driving fact: the ranking row for **the relay in use** … plus the v1 estimator's published measurement when one exists." | after A9 the **relay's own report** is the primary measured source; the v1 estimator is the fallback (`review-r5.md` R3) |
| `:71-74` | rule: "**On a direct fallback** the `Path` row already says the relay is not in use…" | the direct-fallback state is retired by A7; say **"when the session is not relayed"** |
| `:232-237` | Known issues: "The `Server leg` row's **measured branch** … is wired to the v1 estimator's `estimate()` and `measurement_age()` … depends on the estimator's production feed" | pre-delta framing; A9 makes the relay report the primary measured branch and the estimator the fallback (the `:238-240` English-keys bullet right below is already row 4) |

### Conflicts resolved

| # | Conflict | Resolution | Pointer |
|---|---|---|---|
| C-1 | `interface.md:157` old short-mode "globe to 76 px" vs the flat map | map omitted at short height; no smaller honest size | §1.4 |
| C-2 | `interface.md:159` 34 px rail vs `HERO_H` 40 | `HERO_H` 40 + metric row + sub-line; the 34 px band is corrected | §1.2 |
| C-3 | Old ≥620 split vs the mobile-only owner decision | **retired**; A12 | §1.1 |
| C-4 | W1 360/440/1120 plan vs owner mobile-only | superseded; 320 is the product form; `interface.md:164`'s 360×420 raise is withdrawn by A12 | §1.1, A12 |
| C-5 | `interface.md:36` Holding fact vs the gate | A1 | §2.2 row 3 |
| C-6 | `interface.md:31` one Idle row vs shipped words + dead key | A2 | §2.2 row 1 |
| C-7 | `interface.md:41` Error verbatim vs no raw internals | A3 | §2.2 row 8 |
| C-8 | `interface.md:134-136` headline vs shipped direct-ping headline | A13; contract kept, metric labelled and moved to its own row | §1.2/§2.2 row 5 |
| C-9 | `design-system-2026.md` §6.9 selected-row severity | 1.35:1 recorded (Critical per the skill ladder); fixed in W2-2 | D1 |
| C-10 | Boundary R7 vs its own `app.rs` scopes | sequencing + region split; one `app.rs` writer at a time | §5 |
| C-11 | DTO gaps (connecting, in-flight, command error, config recovery) | W2-1 adds the four paths | §2.1 |
| C-12 | Sparkline left open by the audit/design system | removed in W2 v1 (owner/Lead) | §3 |
| C-13 | `interface.md:34` Choosing as a state | A4 | §2.2 row 4 |
| C-14 | `interface.md:35` Relaying "interceptor counters" | A5 | §2.2 row 5 |
| C-15 | `interface.md:40` Needs elevation as a state / boundary variant | A6 | §2.2 row 7 |
| C-16 | critique req. 7's ≤220 chrome budget vs the adopted wrapped action | budget recomputed for 320: **239 chrome / 181 scroll worst at 420**; the old 220/180 figures are retired with the old geometry | §1.3 |
| C-17 | **`design-system-2026.md` §5.4 chrome 213 / scroll 207/547/1187 vs its own §4.4 item 4 metric-row fix** | **CLOSED:** DS r14 re-cuts §5.4 to the spec table — rail 108/128, action 72/64, reason 18 (CAPTION 14 + S1 4), footer 41/57, **worst 239 → scroll 181 @ 320×420** — and deletes the "27 px to spare" claim. W2 uses **239/181**; W2-2 measures in a real window (F2/Q16) | §1.2/§1.3, Q16 |
| C-18 | W1 fixed notice band vs `design-system-2026.md` §6.10 in-card notice | one notice component inside the scrolling region; no fixed conditional band (a fixed band would move the scroll boundary per state) | §2.9 |
| C-19 | `interface.md` fallback rows 36–38 vs shipped warn-only model | A7 retires them; no aliases (stale field paths are amended, not shimmed — boundary R17) | A7 |
| C-20 | **Amendment numbering:** architect A7–A11 vs r10's internal A7–A13 | reconciled: A7 warn-only (r10 A7/A8/A9/A11 folded), A8 location, A9 measured leg, A10 trace, A11 update disclosure, A12 geometry, A13 metric, **A14 non-state prose (F5, not an r10 slot)** | this appendix |
| C-21 | Location data would feed the ranking vs display-only | `chooser_point` Cloud-only; G6 guard + `a_geofeed_place_never_steers_the_ranking` test | A8, W2-3b (c) |
| C-22 | Relay-measured leg vs the E1 gate | display-only until the validation gate passes | A9 |
| C-23 | Client traceroute vs `[PRIVACY_STUB]` | disclosed before S5b (Q13); the caption does not ship first | Q13 |
| C-24 | RU restart label 3 lines vs `ACTION_H_WRAPPED` 56 | **RESOLVED:** RU copy shortened to two lines (`Перезапустить от имени администратора`, 356.9 px @ 15, landed); the 73 px slot is rejected because its band prices the 420 compact worst at 256 → scroll 164 < 180 (Q15/F8); the "to optimize" purpose moves to the elevation overlay's reason line | §1.2/§7 |
| C-25 | Shipped rail word+metric row vs measured collision | metric on its own row (D6); every 320 capture checks no occlusion | §1.2, §6 |
| C-26 | **F1: the state-word fit was measured at the shipped 24.6 px while W2 renders `STATE` 30 px** — at 30 px, **HEAD had four over 288**: RU `NO RETURN PATH` 342.8, RU `IDLE` 326.8, ES 309.0, PT-BR 328.4. **Current tree:** RU landed 222.9; the remaining three are ES/PT (pending, Q20) and RU `IDLE` (retired by A2); **DE `OPTIMIZING` 275.1 passes the ≤276 cap by 0.9 px** (thin; the optional DE `OPTIMIERUNG` 167.2 slack stays). | **Remedy = shorten the localized words.** Acceptance: **every word in the W2 word set** (the state words, incl. `state.connecting` and the changed `state.disconnected` value) **≤276 px at `STATE` 30 in its installed chain, measured when its catalogs land** (12 px / 4 % safety, ±2 px tolerance). ES/PT land in W2-2 with the catalogs and then join the §6 captures (Q20). One-line word enforced: a two-line word → scroll 145; a 24/25 px cap breaks the type role; the old 73 px action option → 164 — all rejected (F1/F8) | D6, §1.2/§1.3, W2-2 (b), Q15, Q20 |
| C-27 | **F9: location-data generators/features/owners** — boundary r9 named only `build-server-location.py`; the geofeed table/manifests come from `build-server-city.py` and the country blob from `build-geo-ipv4.py`, and the two feature gates were conflated | Q11 + W2-3b now name **all three generators**, **two features with one meaning each** (`server-location` = three server indexes; `geo-ip` = country blob only, off ⇒ ladder ends at `Unknown`) and the **owner split** (`rust-dev` moves modules+gates; `infra-dev` owns generators/committed artifacts; `dev-ops` wires CI) — boundary r15 §3.2.2 | Q11, W2-3b scope/(e) |
