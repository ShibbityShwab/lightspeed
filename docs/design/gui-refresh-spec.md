# LightSpeed desktop 2026 — implementable refresh spec (**Instrument 2.0**)

| | |
|---|---|
| Programme | WF-081 "Desktop 2026", wave **W1** synthesis (docs only) |
| Author | `interface-designer` (task-5); synthesis of the four W1 docs + `interface.md` |
| Date | 2026-11-19 · worktree `dsh-b74d064d-6fe5`, HEAD `62c6cf6` |
| Status | **Revision r1 — applies the QA review's spec-owned required changes:** R1, R2, R3 (A4–A6 + Q3; DTO coordinated with the architect's revision), R4, R5 (spec side), R7, R8, R9, R10, R12, R14, R16, R17. R6/R11/R15 belong to the other docs' revisions; D1 here already carries R6's corrected 1.35:1. For Lead integration and owner acceptance. No code, token, config, workflow, engine or fleet change. W2 does not start until the owner accepts this spec (`gui-refresh-brief.md` §13 gate 2). |
| QA review | `wat/evidence/2026-11-19/team/qa-design/review.md` — verdict **REVISE** (2 Blockers, 5 Criticals, 8 Majors, 9 Minors). The finding → changed-line map for this revision is `wat/evidence/2026-11-19/team/interface-designer/revision-r1.md`. |
| Supersedes | `ui-design-plan.md` (Oct 2026, pass 1, never accepted). The defect checklist in `ui-design-critique.md` is mapped in Appendix A; the audit findings in Appendix B. |
| Binding state facts | `docs/design/interface.md` (11 route states + server-leg states). Where this spec changes a contract fact it is named as an **amendment A1–A6** in Appendix C, with a one-line rationale; the architect's DTO revision reconciles A4–A6 (QA R3/R13). |
| Inputs | `gui-refresh-brief.md` (program contract); `gui-ux-audit-2026.md` (flows, findings, copy); `design-system-2026.md` (tokens, contrast, components, motion); `cross-platform-2026.md` (parity plan, tiers); `gui-core-boundary.md` (DTO and extraction stages); `lightspeed-gui-design` skill; current `client-gui/` sources and the real-window screenshots in `wat/evidence/2026-11-19/team/ux-lead/`. |

**Authority order used to resolve every disagreement:** owner decisions and `interface.md` facts > measured
numbers with evidence > program constraints in the brief > prose claims. Conflicts resolved here are
listed in Appendix C; items needing the Lead/owner are §7.

---

## 0. What this spec decides, and the facts it inherits

The build contract for W2, not a fifth audit: shell and window geometry (§1); every contract state on
screen, from which field, with which components and actions (§2); the component usage map (§3);
per-OS requirements per screen (§4); the W2 slice order that keeps the core extraction and the shell
rewrite out of each other's way (§5); the screenshots QA must produce (§6); the open decisions (§7);
and the critique/audit/conflict mappings (Appendices A–C).

**Inherited measured requirements — carried forward, not re-derived.** These are hard numbers from
W1; W2 acceptance references them by source instead of re-measuring:

| # | Requirement carried into W2 | Number | Source |
|---|---|---|---|
| D1 | Contrast floors are measured, not asserted; the AA failures must be fixed | selected list-row ink **1.35:1** (fail; ink composited over the **selection fill**, the real paint plane — QA R6 corrected the earlier 1.20:1 figure, which composited over the card; both fail, the fix value is unchanged); control boundary **1.08–1.29:1** (fail); focused field **1.22–1.32:1** (fail); focused stock button fill **2.77:1** (fail); fixed values: `border_control #65656A` = 3.14:1 on `bg_2`, focus ring `accent_text` outer 2 px = 7.51–8.11:1, selected row `accent_text` = 6.70:1 | `design-system-2026.md` §3.2–§3.4 (as corrected by QA R6) |
| D2 | Dead tokens are drift; one role, one value | **11 of 53** tokens dead/shadowed; 9 tokens to add (including `ACTION_H_WRAPPED`), 9 to delete, 7 to re-role (all named) | `design-system-2026.md` §1.3–§1.5 |
| D3 | Focus visibility is a Blocker, not polish | the custom action is exposed as UIA `Custom` with an empty name; focus paints nothing | `gui-ux-audit-2026.md` B1; `design-system-2026.md` §6.1, §6.9 |
| D4 | Glyph tofu: never type a mark the bundled chains cannot draw | `→` is drawn by Noto Sans JP only; `■ ▲ ▶ ▸ ▾ ◆ ✗ ← ↻ ⏵` are in **no** bundled chain; ko `한국어`/zh-Hans `简体中文` native names have no bundled face | `design-system-2026.md` §8.2–§8.4 |
| D5 | Reduced motion: **no claim**, and the OS signal is **assigned with one polarity rule** | egui 0.37 has 0 hits for any reduced-motion API. **Rule:** the OS value means *client-area animations enabled*; `progress_indicator()` shows a spinner **only when the value is explicitly `On`**, and static text (no spinner) when it is **`Off`** (reduced motion requested) **or `Unknown`** (conservative; the static text is the pause/stop alternative). W2-2 (i) wires the seam and receives `Unknown` (no OS read) so it shows static text from the start; W2-4 (i) supplies the real Windows value; macOS/Linux are named gaps returning `Unknown` → static text | `design-system-2026.md` §7.4 as revised (QA R5/N1), whose §7.3/§7.4 are aligned to this rule in the same W1 pass (task-14); §4 row, §5 W2-2 (i)/W2-4 (i) |
| D6 | Type: one 8-role scale, measured row heights | `HERO_H = 40` (the 34 px rail cannot contain METRIC 30/40 px row); `STATE 30` + `METRIC 30` + 8 px = 246 of 288 px at narrow | `design-system-2026.md` §4.1–§4.2, §4.5 |
| D7 | The shipped 320 px width cannot hold the widest action label | DE restart label measures **383.0 px** against a 296 px budget; 4 of 15 locale/label pairs overflow → the refresh adopts the §4.4 **two-line action at 15 px** and deletes the 9 px shrink | `design-system-2026.md` §4.4 |
| D8 | Content-driven auto-resize moves the action under the pointer | measured heights for one state family: **866 / 803 / 988 / 914** px (EN connected / EN disconnected / EN boosting / DE connected) | `gui-ux-audit-2026.md` M4 |

**Not decided here:** engine, protocol, capture, relay or routing behaviour; the core boundary
(`gui-core-boundary.md` decides it); token values (`design-system-2026.md` owns them) — this spec
adopts them; CI jobs/packaging (`cross-platform-2026.md` §6, `dev-ops`). No new engine fact is
invented: where the state contract needs a fact the DTO does not carry yet, §2 says **"DTO gap"** and
names the W2-1 work item. Every proposed DTO name is the architect's, marked proposed in
`gui-core-boundary.md` §5.

**Citation convention:** every path is repo-root relative except `app.rs`, `app/*.rs`,
`platform/*.rs`, `main.rs` and `i18n.rs`, which are relative to `client-gui/src/` (the same
convention as `interface.md` and the W1 docs); `client/src/...` is always spelled out.

---

## 1. Shell, window geometry and breakpoints

### 1.1 Window sizes (replaces the pinned 320 px viewport)

`main.rs:94-100` pins min = max = 320 px wide with a content-driven height. The refresh ships a
**resizable, remembered** window (2026 desktop floor: `lightspeed-gui-design` skill §2):

| | Size (logical points) | Rationale |
|---|---|---|
| **min** | **360 × 420** | `interface.md:164` already requires raising the old 320×400 to 360×420: content column = 360 − 2×S4 = 328 ≥ the 320 px content floor (`interface.md:166`); at 360×420 the compact-tier fixed chrome is ≤ 225 px including the wrapped action and the disabled-reason line, leaving ≥ 180 px of scroll region (§1.3 arithmetic). |
| **default** | **440 × 880** | Width 440 → content 408 and action-label budget 384 ≥ the largest measured label 383.0 px **by 1 px — inside the documented kerning tolerance (`design-system-2026.md` §4.4), so the DE/FR/ES/PT restart labels may still wrap; the wrap rule (§1.3) is the same at every width and replaces the 9 px shrink**. Height 880 shows the tallest measured idle-family state (EN connected 866) without scrolling (D8). |
| **max** | **1120 × 1400** | At 1120 the two-column layout reaches both content caps: status column 454 (map `MAP_W_MAX` 430 + 2×S3) + S5 24 + config column 560 (`design-system-2026.md` §5.1) + margins; a wider window only adds empty plane. Height 1400 keeps the shipped cap (`app/window.rs:39`). |
| **clamped** | to the monitor **work area** on first show, restore and monitor change | G2.2 (`cross-platform-2026.md` §5): restore to a visible monitor; a work area smaller than the minimum (200 % scaling on a short display) wins over the minimum — the window never exceeds the screen. Geometry persists in UI prefs (`UiPrefs`, `gui-core-boundary.md` §5.1). |

**Remove content-driven auto-resize** (`app.rs:4046-4053`). It resized the window each time the
content changed by >8 px, so the action bar and footer moved vertically under an aiming pointer
(D8, audit M4). The refresh fixes the height and scrolls the one region that may scroll.

### 1.2 Width breakpoints

| Width | Layout |
|---|---|
| **< 380** | One column. Ledger rows are label-above-value; action and controls full width. |
| **380–619** | One column. Ledger rows are label/value side by side (`LEDGER_LABEL_W` 96 px becomes reachable). |
| **≥ 620** | Two columns (`interface.md:156` kept): status column = `min(58 %, 454)` (the route card's cap), config column = the remainder, capped at 560 and centred. The **rail, action bar and footer stay full-width bands** — the one bright action is never narrowed by the column split. |

`interface.md`'s ≥620 split is kept; only three points are refined and must land in `interface.md` in
the same W2-2 change that ships the shell (`gui-refresh-brief.md` §9 item 4): the status-column cap, the
full-width action band, and the rail height (§1.3). **W2-2 also applies amendments A1–A6** (Appendix C)
to `interface.md`; the edits are listed in the W2-2 write scope and approved via §7 Q3.

### 1.3 Regions (top to bottom, fixed bands vs scrolling)

| # | Region | Contents | Tokens / geometry | Behaviour |
|---|---|---|---|---|
| R1 | **State rail** (fixed, full-bleed) | state word (STATE), metric (METRIC) + its leg caption (CAPTION), one sub-line (BODY) | `bg_2`, 3 px painted left edge `RAIL_EDGE`; `HERO_H` 40 + one CAPTION row (14 measured) + one BODY row (16 measured) + S1/S3 margins ⇒ ≈102 px | never scrolls; edge colour per §2 precedence; edge is redundant with the word, never the only state cue |
| R2 | **Notice band** (fixed, only when a class applies) | one sentence + at most one inline action link (24 px target) | full-bleed `bg_2`, class ink (`danger` error / `warn` precondition), BODY 13, max 2 lines | hidden when no class applies; content and classes in §2.7 |
| R3 | **Route card** (fixed at normal height; scrolls in short/compact) | map + ledger rows | `bg_2`, `border_1` 1 px, `R_CARD`, S3 padding, one card style; map `MAP_H` 132 / `MAP_W_MAX` 430 | ledger row visibility per §3; map shown only in the normal-height fixed card |
| R4 | **Action bar** (fixed, full width) | the one action (`ACTION_H` 48, or `ACTION_H_WRAPPED` 56 when the label wraps to two lines at 15 px — the adopted `design-system-2026.md` §4.4 decision; the silent 9 px shrink is deleted) + a disabled-reason line (CAPTION `text_3`, 14 px row) | `bg_1`, S2 padding | always above the footer, never pushed off-screen; label from the action gate |
| R5 | **Config region** (the only scrolling region) | Relay row · Game row · Target a process · advanced/manual fields | `bg_1` plane; config card `bg_2` per one-card style | `ScrollArea` with `AlwaysVisible` scrollbar; the only region that scrolls at normal height |
| R6 | **Footer** (fixed) | Hide to tray (only when a real tray exists) · Settings · Quit | `bg_1`, hairline above, buttons `HIT_MIN` ≥ 24 | never moves; primary path `Hide to tray` first |
| R7 | **Sheets** (modal over the shell) | Settings, Update, Game editor, Target picker, About | `scrim` backdrop, `bg_0` body, `border_1`, `R_SHEET` 16, `TITLE` 20, body `ScrollArea`, computed max height | per-sheet close policy (§2.4); Tab stays inside, Esc closes (except a sheet holding unsaved data) |

**Fixed chrome budget, computed from D6 and `design-system-2026.md` §5.4 — self-consistent with the
adopted two-line action.** At **360×420** (compact tier, footer buttons at `HIT_MIN` 24):
rail ≈102 + action bar 72 (`ACTION_H_WRAPPED` 56 + 2×S2) + disabled-reason line 18 + footer ≈33 =
**≤225 px**, leaving **≥180 px (≈195)** of scroll region. At the **default 440×880** (footer tier 40):
≤241 px, leaving ≥600 px. This **supersedes critique req. 7's ≤220 px** budget
(`ui-design-critique.md:100-111`), which assumed a single-line action and no reason line: the +20 px is
the two-line `EMPHASIS` row (§4.4 copy decision) plus the disabled-reason row (M5), both required —
recorded as a budget change, not drift. W2-2 proves the numbers with the 360×420 screenshot in **EN and
DE** (the widest measured Latin locale: 383.0 px) and the 440×880 pair; a label never shrinks below
`EMPHASIS` 15 px to fit. The shell uses `Panel::top/bottom` + `CentralPanel` and a single
`ScrollArea`, so the action cannot be pushed off-screen (`lightspeed-gui-design` skill §4).

### 1.4 Height: short, compact, and 200 % scaling

Breakpoints are in **logical points**, not pixels (`lightspeed-gui-design` skill §2: never lay out in
physical px). At 200 % scaling the logical work area halves: a **1080p** display offers ≈520 pt of
height, so a launch at 200 % on 1080p starts in short mode; a **1440p** display offers ≈720 pt and
stays in normal mode; a 768p display offers ≈340 pt and starts in compact mode. That is why the
short/compact tiers are part of the W2 acceptance, not an edge case.

| Height | Mode | Behaviour |
|---|---|---|
| **≥ 560** | normal | R3 fixed as a card; config region scrolls below it. |
| **< 560** | **short** (`interface.md:157`) | R3 folds into the scroll region as its first item; the **map is omitted**, not shrunk (there is no smaller honest map size after the flat-map redesign; this supersedes `interface.md:157`'s "collapse the globe to 76 px" — the globe no longer exists). Rail, action bar and footer stay fixed. |
| **< 480** | **compact** | as short, plus footer controls use the `HIT_MIN` 24 px target tier and R2 is limited to one sentence. Nothing is hidden; everything except R1/R4/R6 scrolls. |
| work area < min | **clamped** | effective minimum = work area; the shell renders compact and scrolls; no control is clipped (the only fixed bands are rail/action/footer). |

*Rules that hold at every size:* the action bar and footer are always visible and hittable; every
control keeps the 24 px pointer floor; no text is clipped (`interface.md:169-170`: a control that does
not fit moves to the next row, it is never clipped). **RTL mirroring is not applicable** to the nine
shipped locales (`i18n.rs:135-172`: en, de, es, fr, ja, ko, pt-BR, ru, zh-Hans — none is
right-to-left); recorded as N/A rather than left silent (QA R17/m9). A future RTL locale needs its own
layout pass and a mirroring acceptance.

---

## 2. State-by-state screen spec

### 2.1 Head-state precedence (one word, one driving fact)

The rail word is derived **once** (core, W2-1) from the first matching row; a lower row may add
ledger rows but may not change the word:

1. **Error** — any live last session-command error (`SessionCommandState.last_error`; the collected
   `AppStatus` error field) or engine `interceptor_error` / `capture_error` / `windivert_error` /
   `redirect_error`, or `control_registered == false` with `registration_error.is_some()`. A failure is
   never hidden behind a healthy-looking word (B3/B4).
2. **Direct fallback** / **Re-test pending** / **Server rejects relay** — while the watchdog's fault
   or chain is active for the locked server (`AppStatus.watchdog`).
3. **Relaying** — a tunnel backend is carrying traffic (`run_boosting()`, `app.rs:2352`) **and** the
   counters come from the one source pair in §2.2 state 5 (amendment **A5**).
4. **Holding** (with the Choosing sub-line) — the engine's tunnel gate is off while a session is
   running (`EngineStatus.tunnelling_enabled == Some(false)`, `client/src/engine.rs:195`; amendment
   **A1**). Choosing is a sub-state of this word, not a second rail word (amendment **A4**).
5. **Connecting** — a connect attempt is in flight (`RelayStatus.connecting`; **DTO gap**, §2.1 table).
6. **Idle** — connected (`CONNECTED`) or not (`NOT CONNECTED`); amendment **A2** splits the contract row.

"Needs elevation" is **not** a rail word (§2.2 state 10; amendment **A6**); "Relay recovered" has **no
surface** by contract (`interface.md:39`, `:142-143`).

**Presentation seam (closes QA B1's cross-slice dependency).** The rail/ledger view model is produced
by one pure adapter, `client-gui/src/app/route_surface.rs` (new in W2-1), with inputs `AppStatus` +
`WatchdogFacts` and output the §2 word/sub-line/caption/ledger model. It is **presentation mapping
only**: it may not compute thresholds, verdicts, joins or counters — every field is a read. The shell
(W2-2) renders it and derives no state. The three watchdog rows take `WatchdogFacts` from the current
GUI watchdog until W2-3 swaps that input for the core `WatchdogStatus`; the adapter fixtures then prove
the output is unchanged.

**Facts the contract needs and the DTO does not carry — closed in W2-1.** `gui-core-boundary.md` §5's
names are indicative; the architect's DTO revision reconciles A4–A6 and the `RouteState` list
(QA R3/R13). This spec's acceptance requires the **field paths**, not the variant spellings:

| Fact | Field (W2-1) | Drives |
|---|---|---|
| a connect attempt is in flight | `RelayStatus.connecting` | §2.2 state 2 |
| a session command is in flight | `SessionCommandState.in_flight` | the action loading state (`STARTING…`, §3) |
| the last session command failed | `SessionCommandState.last_error: Option<AppError>` | §2.2 state 11 (start refusal) |
| the config file was unreadable, defaults used | `AppConfig` recovery diagnostic (from `load_or_default`) | `error.config` (§2.6) |

`RouteState::NeedsElevation` may not select the rail word (amendment A6).

### 2.2 Route states 1–11

Every block names: what the surface must say (contract), the single driving fact and where it comes
from, the exact on-screen copy, the components, the actions with their conditions, and recovery.

**1. Idle** — `interface.md:31`. Two words, one row (amendment A2; the shipped words are `NOT
OPTIMIZING` from `state.idle` and `CONNECTED`, audit §3.1). `NOT CONNECTED` **replaces** the shipped
disconnected word and **wires the existing unused `state.disconnected` key** (`client-gui/locales/en.toml:148`, dead copy
per audit §4.1 #20) with a corrected value — it is not new copy, and a duplicate key is not added.
- Fact/source: `AppStatus.engine.connected` (`EngineStatus.connected`, `client/src/engine.rs:62`).
- Surface: word `CONNECTED` (`state.connected`) when connected, else `NOT CONNECTED`
  (`state.disconnected`, value changed). Sub-line, by game mode: `Relay ready · Auto waits for your
  game` (`route.relay_ready_auto`), `Relay ready · {name} follows` (`route.relay_ready_follows`,
  detected), `Relay ready · {name} pinned` (`route.relay_ready_pinned`, pinned); disconnected: `No
  relay connected` (`route.no_relay`). Metric `-- ms` + caption `relay leg`; ledger: Relay `--` or
  label, `Relay RTT` when measured, `Game server` `Not measured yet` (`route.place_hint` new value),
  `Relayed` `No traffic relayed yet` (`route.count_hint` new value).
- Components: rail; Relay combo (`design-system-2026.md` §6.3 loading/empty/error); primary action; footer.
- Actions: action gate per `design-system-2026.md` §6.2; Relay row carries `Connect` (`action.connect`)
  when a relay is selected and disconnected (`Disconnect` when connected — B2 fix); boost disabled
  reason line when not ready (§2.7).
- Recovery: `Connect` in the Relay row; discovery `Try again` in the relay row's error state (§2.7).

**2. Connecting** — `interface.md:32`. **DTO gap:** `EngineStatus` has no connecting fact
(`connected` becomes true only with the keepalive loop, `client/src/engine.rs:60-62`); W2-1 adds
`RelayStatus.connecting` to the commands/status DTO and the session command sets it for the attempt
(field path per §2.1; the architect's DTO revision reconciles the shape).
- Fact/source: session command in flight → `RelayStatus.connecting`; `EngineStatus.control_registered`
  (`:79`) and `registration_error` (`:87`) end the attempt.
- Surface: word `CONNECTING` (`state.connecting`, new); sub-line `Reaching {relay}…`
  (`route.connecting`, new; `{relay}` = the Relay row's own label); metric `-- ms` + `relay leg`.
- Components: rail; Relay combo (disabled during the attempt); primary action (disabled).
- Actions: none beyond the Relay row (the engine times the attempt out; action disabled with reason
  `Waiting for the relay`, `action_reason.waiting_relay`, new).
- Recovery: success → CONNECTED; failure → **Error** with the registration sentence; the Relay row's
  `Connect` retries.

**3. Holding** — `interface.md:33`; amendment A1 replaces the `is_holding` proxy with the engine gate
(Appendix C C-5).
- Fact/source: `AppStatus.engine.tunnelling_enabled == Some(false)` while a session runs
  (`client/src/engine.rs:195`; `InterceptorHealth.tunnelling_enabled`, `:50`), together with
  `AppStatus.interceptor` and `AppStatus.relay.server`. The old proxy (`direct_fallback && sent == 0`,
  `app/ledger.rs:215-217`) missed the pre-server hold and let the rail read OPTIMIZING (audit C2,
  `window-boosting-hold-320.png`).
- Surface: word `HOLDING` (`state.holding`, warn). Sub-line in order: no server locked →
  `Finding the game server…` (`route.holding_scan`, new; replaces the engine's untranslated
  `Auto-detecting (1024-65535)…` shown in a value slot, audit C6/#6); server locked and the chooser in
  flight (state 4) → `Choosing a relay for this server` (`route.choosing`); server locked, no choice
  yet → `Traffic stays direct until a relay is chosen for the game server` (`route.holding`, new —
  the contract sentence). Ledger `Path` row = `Holding`; `Relayed` reads `No traffic relayed yet`
  until the first send.
- Components: rail; route card ledger (`Path`, list-row warn variant); primary action (danger).
- Actions: `STOP OPTIMIZING` enabled. Relay and Game combos stay enabled — choosing is the resolution.
- Recovery: a relay is chosen → the gate opens and the state becomes RELAYING; the engine lifts the
  hold after `HOLD_FALLBACK` 120 s (`app.rs:94,1140-1151`) and the rail follows the engine (no
  countdown copy exists in the contract, so none is invented).

**4. Choosing** — `interface.md:34`. **Amendment A4:** the HOLDING word is one head-state and Choosing
is its sub-line sub-state, not a distinct rail word; `interface.md:34` is annotated as a sub-state (the
merge was called "undocumented" at `gui-ux-audit-2026.md:375`, audit §3.1 — now authorised). The DTO may
carry a sub-field or a variant; the architect's revision decides, and the spec requires only that the
word not change.
- Fact/source: the destination-aware chooser is in flight for the locked server — `AppStatus.relay`
  ranking state from `app::relay::choose_relay` (`gui-core-boundary.md` §5.4); today
  `choose_relay_for_destination` (`app.rs:788-917`).
- Surface: word `HOLDING`; sub-line `Choosing a relay for this server` (`route.choosing`); no other
  difference from state 3. Components and actions identical to state 3.
- Recovery: same as state 3; if no candidate can be chosen, the contract's direct/fallback family
  takes over — no invented "no relays" rail state.

**5. Relaying** — `interface.md:35`. **Amendment A5:** the driving fact is the `run_boosting()` gate
plus **one** counter-source pair — not "interceptor counters" generically (`interface.md:35`), which
let two modes be mixed (audit C3).
- Fact/source: `run_boosting()` (`app.rs:2352`) = `interceptor_active || windivert_active ||
  capture_active || redirect_active`; counters from the **same** source pair: the interceptor's
  per-server flow (`interceptor_flows`, `client/src/engine.rs:188`) when present, else redirect
  `redirect_pkts_out/in` (`:101-103`) — never a mix (C3 fix; W2-1 derives the pair once).
- Surface: word `OPTIMIZING` (signal). Sub-line `{Game display}` + the **painted route arrow**
  (`design-system-2026.md` §8.4) + `{relay label}` — never the `→` glyph (D4). Metric = relay leg
  (`latest_rtt_ms`, `:66`) + caption `relay leg`.
  Ledger: `Relay`, `Relay RTT`, `Server ping` when a fresh direct ping exists for this server
  (`{ms} ms · {age}s`, `SERVER_PING_FRESH_SECS` 60 s, `app.rs:119`), `Server leg` (§2.3), `Game
  server` (verbatim `ip:port`), `Relayed` `{sent} up · {returned} down` (warn ink while sent > 0 and
  returned == 0).
- Components: rail; route card (map + ledger); action (danger `STOP OPTIMIZING`).
- Actions: `STOP OPTIMIZING`; Relay/Game combos enabled (changing either restarts interception —
  today's behaviour, unchanged). While `SessionCommandState.in_flight == Some(StartBoost)` the action
  shows the loading state (`STARTING…`, `action.starting`) and cannot re-trigger (design-system §6.2,
  closing its §6.9 gap 6 with a real fact).
- Recovery/fallback: watchdog fault → state 6; fatal engine error → state 11; game ends → the ledger
  keeps the last state until the tunnel stops, then state 1 (no stale counters: the row's session
  delta resets with the session key, `app.rs:2497-2508`).

**6. Direct fallback** — `interface.md:36`; must name **why** (audit C1). Its fact is **delivered in
W2-3** (`WatchdogStatus`); until then the adapter receives the current GUI watchdog facts as its
`WatchdogFacts` input (§2.1), so the shell renders this state from W2-2 without inventing it.
- Fact/source: `AppStatus.watchdog.fault` = `RelayReplyFault` (`app/watchdog.rs:144-153`) + the
  `ReplyFaultStreak` verdict (two consecutive judged windows, ≥20 intercepted, return share below
  1/8); exposure via `WatchdogStatus` (`gui-core-boundary.md` §5.3).
- Surface: word `DIRECT` (warn). Sub-line by fault: `NoReplyFromRelay` → `Direct fallback: the relay
  is not returning this server's traffic` (`route.direct_fallback_relay`, new); `InjectionFailed` →
  `Direct fallback: this PC could not inject the replies the relay sent`
  (`route.direct_fallback_inject`, new); `TooFewReplies` → `Direct fallback: too little of this
  server's traffic is coming back through the relay` (`route.direct_fallback_share`, new). Ledger
  `Path` row = `Direct fallback` (warn); the Relay row still names the abandoned relay.
- Components: rail; ledger `Path` (warn); action (danger).
- Actions: `STOP OPTIMIZING`; re-test is automatic while `retry_after_fallback` is on.
- Recovery: a judged window with a working return share closes the chain → RELAYING; the recovery
  itself is trace-only (`interface.md:39`) — no banner, ever.

**7. Re-test pending** — `interface.md:37`.
- Fact/source: `WatchdogStatus.retry_in` from `relay_retry_at` / `ReplyFallback.due`, published only
  while `retry_after_fallback` is on; countdown helper `retry_seconds_left` floors at 1 s
  (`app/watchdog.rs:126-131`).
- Surface: word `DIRECT`; sub-line `Direct fallback · retry in {secs}s` (existing `route.direct_fallback`
  + `route.retry_in` joined with the canon separator `·`); ledger `Path` = `Direct fallback`; Relay
  row names the abandoned relay. No other change from state 6.
- Actions: `STOP OPTIMIZING`. Settings → `Re-test a relay after falling back` (checkbox) turns it off.
- Recovery: at zero the **same** relay is judged again — no re-rank, no interception restart (contract);
  the schedule is `RELAY_RETRY_BACKOFF = [30 s, 120 s]` (`app/watchdog.rs:121-122`) and the Settings
  copy is generated from it (C5).

**8. Server rejects relay** — `interface.md:38`.
- Fact/source: `WatchdogStatus.rejection` — retries spent or retries off, and only when the rejection
  belongs to the **locked** server (`app.rs:2456-2467`); the memory is a chain that returned nothing on
  every window.
- Surface: word `DIRECT`; sub-line `Server rejects the relay` (`route.server_rejects_relay`); ledger
  `Path` = `Server rejects the relay`; Relay row unchanged.
- Actions: `STOP OPTIMIZING`; pick another relay (Relay combo) or another game.
- Recovery: a new server or a new session clears it; nothing is remembered as a failure otherwise.

**9. Relay recovered** — `interface.md:39`. **No surface by contract:** trace-only, exact trace line
`The relay is returning this server's traffic again; the fallback chain is closed`. The window falls
back to the RELAYING sentence. Acceptance is the **absence**: no banner, no flash, no word; a test
asserts the rail never renders a recovered state (there is no `RouteState` variant for it,
`gui-core-boundary.md` §5.3). Not screenshot-able by design; named as such in §6.

**10. Needs elevation** — `interface.md:40`. **Amendment A6:** an overlay driven by
`Capabilities.elevated`, never a rail word; the architect's DTO revision reconciles
`RouteState::NeedsElevation` (removed or marked deprecated) so there is **one** field path (QA
R3/R13).
- Fact/source: `Capabilities.elevated` (`gui-core-boundary.md` §5.6) supplied by the front end
  (`platform/mod.rs:94`, `is_admin`) via `AppStatus::collect(front_end: FrontEndFacts)`; the boost
  requires privilege (`platform/windows.rs:184-231`).
- Surface: rail keeps its route word; action label becomes `RESTART AS ADMINISTRATOR TO OPTIMIZE`
  (existing key; macOS/Linux variant `Restart to grant network access`,
  `action.restart_network_access`, new — those platforms have no "administrator" semantics,
  `cross-platform-2026.md` G1.2); notice band (warn) `LightSpeed needs administrator rights to relay
  game traffic.` (`notice.needs_admin`, new). The elevation invite appears **only when a relay is
  selected and the app can otherwise start** (M1 fix); otherwise the action stays the disabled boost
  label with its reason.
- Actions: pressing the action runs the platform elevation handshake: action label
  `RESTARTING AS ADMINISTRATOR…` (`notice.restarting`, new; loading state, control cannot be
  re-triggered), the **current process stays alive** until the elevated instance confirms startup
  (C8 fix, `platform/windows.rs:259-273` today exits immediately); declined prompt returns to ready
  with notice `The administrator prompt was declined. LightSpeed is still running.`
  (`notice.elevation_declined`, new). **Verification tier:** the handshake state machine is unit-tested
  and closes W2-4 (e); the **interactive UAC-decline path is U on this host** (the agent harness is
  elevated, `gui-ux-audit-2026.md:1067-1070`) — the fixture that would settle it is a non-elevated
  interactive Windows desktop run by the owner/QA, and until one exists W2-4 records it unverified.
- Recovery: elevated instance owns the session; otherwise the window is still there to retry.

**11. Error** — `interface.md:41`; amendment A3.
- Fact/source: `SessionCommandState.last_error: Option<AppError>` (`gui-core-boundary.md` §5.7 shape;
  today a start failure is only `tracing::error!`, `app.rs:1452-1454` — **DTO gap**, W2-1) + the
  config recovery diagnostic for `Config` (below) + `EngineStatus`
  `interceptor_error`/`capture_error`/`windivert_error`/`redirect_error`
  (`client/src/engine.rs:111,133,151,203`) plus `registration_error` (`:87`). One `AppError.Kind`,
  one sentence (§2.6); no `String` crosses the boundary.
- Surface: word `ERROR` (danger). Sub-line = the taxonomy sentence for the `AppError.Kind` (§2.6).
  Notice band (danger) exposes `Details` (a focusable disclosure, painted marker) with the engine's
  **verbatim** message in CAPTION mono `text_2`, and `Open log file`. The route state is never
  OPTIMIZING over a failure; counters are frozen, not cleared.
- Components: rail; notice band; action; ledger (frozen values).
- Actions: the action slot keeps its gate labels — `OPTIMIZE MY ROUTE` retries a **start** failure
  when the gate is ready; a **registration** failure disables the action with its reason and is
  retried by the Relay row's `Connect`. No new fourth label (canon: three labels, skill §3).
- Recovery: retry the action; `Open log file`; Settings → Components (diagnostics); a changed error
  replaces the sentence.

### 2.3 Server-leg states (`interface.md:48-67`)

Rendered only while a tunnel backend carries traffic **and** the route is the relay — never on a
direct fallback (`interface.md:69-76`). One fact: the ranking row for **the relay in use**, plus the
v1 estimator's published measurement. Component: ledger list row (mono value; `Hint` ink for the
not-measured variants).

| Row state (contract) | Exact copy | Driving fact / DTO source |
|---|---|---|
| **Measured** (`interface.md:63`) | `measured {ms} ms · n={n}` and `· last {secs}s` **only** when the estimator reports an age | estimator `estimate()` + `measurement_age()`; `ServerLegState::Measured` (`app.rs:2259-2318`; `gui-core-boundary.md` §5.4) |
| **Bounded** (`:64`) | `at least {ms} ms · triangle bound` | ranking row `dest_source == GeometricFloor`, or the early `L3d − L1` proof; `ServerLegState::Bounded` |
| **Not measured** (`:65`) | `not measured · prior ~{prior} ms ±{unc} ms` | ranking `dest_source == RegionPrior`; `{unc}` = `PRIOR_UNCERTAINTY_US` 25 ms |
| **Unknown** (`:66`) | `not measured · no region prior` | ranking row `Unknown`, or `Measured` with nothing publishable; `ServerLegState::Unknown` |
| **Relay cannot beat direct** (`:67`) | caption `This relay cannot be faster than direct for this server` (CAPTION, warn) + `Details` giving `client leg {x} ms` and `direct ping {y} ms` | `L1 >= L3d` from the keepalive one-RTT leg and a **fresh, same-server** direct ping (`SERVER_PING_FRESH_SECS` 60 s) |

Rules carried: no claim that relaying is faster than direct; a direct ping is evidence about exactly
one server (a foreign record is "not measured", never reused); the age is never invented; the early
bound re-ranks nothing; the row describes the relay **carrying** traffic even when a better one is
staged. These are `interface.md:69-105` verbatim and are acceptance criteria for W2-1's DTO tests.

### 2.4 Sheets (modal, not route states)

| Sheet | Entry | Content groups | Close policy |
|---|---|---|---|
| **Settings** (`settings.sheet_title`) | footer `Settings` | Language · Privacy (telemetry) · Routing (`Re-test a relay after falling back`, generated copy) · Relays (dense list rows, 40 px, 24×24 delete) · Maintenance (check updates, open log, refresh relays, re-measure) · Components (diagnostic, English-only by documented exception) · About | backdrop click / Esc / Close; no unsaved data |
| **Update** (`update.sheet_title`) | Settings → Check for updates | current/latest (one spelling: no leading `v`), result line, one action (Install on Windows; per-channel instruction elsewhere — §4) | **not** backdrop-closable while installing; must not strand the window |
| **Game editor** (`game.editor_title`) | Game row → New custom game… | name / process names / UDP range / dynamic server / notes, per-field violation text | **guards an unsaved draft**: backdrop click does not discard; Esc asks/refuses while dirty |
| **Target picker** (`game.target_process`) | Game row → Target a process | search, process list rows (loading / empty / no-match), Clear target | backdrop / Esc closes |
| **About** | Settings → About | brand mark, version, links, star link | backdrop / Esc / Close |

### 2.5 Copy deck — new and changed strings (English source of truth)

All strings live in `client-gui/locales/en.toml`; W2 adds them to all nine catalogs (`i18n.rs:135-172`)
or records the documented English fallback with the real key list (C7). `{placeholders}` are
substituted, never concatenated. Button/action labels keep the same name through the flow.

| Key | English value | Change |
|---|---|---|
| `state.disconnected` | `Not connected` | **existing unused key** (`client-gui/locales/en.toml:148`) wired; value changed — deletes dead copy instead of adding a duplicate (`state.not_connected` is dropped from this plan; renders `NOT CONNECTED`) |
| `state.connecting` | `Connecting` | new |
| `action.starting` | `Starting…` | new (action loading label from `SessionCommandState.in_flight`; renders uppercase) |
| `route.relay_ready_auto` | `Relay ready · Auto waits for your game` | replaces `relay_ready_pick` |
| `route.relay_ready_follows` | `Relay ready · {name} follows` | replaces `relay_ready_named` |
| `route.relay_ready_pinned` | `Relay ready · {name} pinned` | new |
| `route.leg_relay` | `relay leg` | new (metric caption; C4) |
| `route.connecting` | `Reaching {relay}…` | new |
| `route.holding` | `Traffic stays direct until a relay is chosen for the game server` | new (contract sentence) |
| `route.holding_scan` | `Finding the game server…` | new (replaces the engine sentence in the value slot) |
| `route.no_relay` | `No relay connected` | changed |
| `route.direct_fallback_relay` | `Direct fallback: the relay is not returning this server's traffic` | new (C1) |
| `route.direct_fallback_inject` | `Direct fallback: this PC could not inject the replies the relay sent` | new (C1) |
| `route.direct_fallback_share` | `Direct fallback: too little of this server's traffic is coming back through the relay` | new (C1) |
| `route.place_hint` | `Not measured yet` | changed (#13, names no button) |
| `route.count_hint` | `No traffic relayed yet` | changed (#13) |
| `game.none_detected_hint` | `Auto follows a game when one starts.` | changed (M2; one instruction) |
| `game.found` | `Game found: {name}` | changed to display name (M3) |
| `relay.refresh_failed` | `The relay list did not update. Using the relays saved on this PC.` | changed (M6 jargon) |
| `relay.try_again` | `Try again` | new (visible, not hover-only) |
| `notice.needs_admin` | `LightSpeed needs administrator rights to relay game traffic.` | new |
| `notice.restarting` | `Restarting as administrator…` | new (loading label) |
| `notice.elevation_declined` | `The administrator prompt was declined. LightSpeed is still running.` | new (C8) |
| `action.connect` / `action.disconnect` | `Connect` / `Disconnect` | **wire the existing unused keys** (B2) |
| `action.restart_network_access` | `Restart to grant network access` | new (macOS/Linux elevation variant) |
| `action_reason.*` | `Connect a relay to start` · `Waiting for the relay` · `The relay has not accepted this session yet` · `Enter a valid server address` | new (disabled reason, M5) |
| `settings.hold_policy` | `LightSpeed keeps traffic direct until it identifies the game server, for up to {secs}s.` | changed; `{secs}` = `HOLD_FALLBACK` 120 s (C5) |
| `settings.retry_policy_hint` | `Retests the relay after {first}, then after {second}. After the last retest the session stays direct until the server changes.` | new; values from `RELAY_RETRY_BACKOFF` = 30 s, 2 min (C5) |
| `tray.show` / `tray.connect` / `tray.disconnect` / `tray.quit` | `Show window` / `Connect` / `Disconnect` / `Quit` | new keys (tray is English-only today, M11) |

Everything else (settings labels, update strings, editor strings, About) keeps its key but moves
hardcoded English into a key: `Routing`, the retest hover, the telemetry hover, the port-range hover,
the UAC line, `Add Proxy`→`Add relay`, `Open config folder`→`Open settings folder`,
`Reset to defaults`→`Reset settings`, and the Components block stays the one documented English-only
diagnostic exception (`interface.md:231`; C7).

### 2.6 Error taxonomy → copy (one `AppError` kind, one sentence)

`AppError.Kind` is the core boundary's (`gui-core-boundary.md` §5.7). The GUI maps kind → key and
never shows a `String` as copy; the verbatim message lives behind `Details`/the log (C6, amendment A3).

| Kind | Rail sentence (key) | Recovery shown |
|---|---|---|
| `Capture` / `Tunnel` (start refusal) | `error.tunnel`: `The tunnel could not start.` | action retries; `Open log file`; Settings → Components |
| `Route` / `Quic` (registration) | `error.registration`: `The relay refused this session.` | Relay row `Connect` retries |
| `Config` | `error.config`: `Your settings could not be read. LightSpeed is using safe defaults.` | Settings; `Open settings folder`. **Source:** the W2-1 config recovery diagnostic — `AppConfig::load_or_default` is tolerant but returns what it recovered from, so the GUI can state it (QA review §3.5); the copy is not shipped until that fact exists |
| `Game` / `Validation` | `error.game`: `That game profile could not be saved.` | per-field violations in the editor |
| `Io` | `error.io`: `LightSpeed could not write its settings.` | `Open settings folder` |
| `Unsupported` | `error.unsupported`: `This build cannot capture on this system.` | Settings → Components names the backend |

New keys under `error.*` render with `danger` ink; the raw engine string is never the only copy
(double-boxed in `Details`). The taxonomy is additive: a new `Kind` cannot ship without its sentence.

### 2.7 Notice band and disabled-reason line (new component contract)

- **Notice band (R2)** — appears only for two classes: **error** (danger) and **precondition** (warn).
  One sentence, max two lines at the minimum width, plus at most one inline action link
  (`Try again`, `Open log file`, `Settings`) with a 24 px target. Error class also carries the
  `Details` disclosure (verbatim engine words). Content is in the body, never hover-only (M7); the
  class colour is redundant with the sentence (no colour-only state). Defined with existing tokens —
  no new colour token, no shadow.
- **Disabled-reason line** — one CAPTION `text_3` line directly under the action button whenever the
  action is disabled: the first unmet precondition in gate order
  (relay → connecting → registration → manual address). A control that cannot act always says why
  (M5, `lightspeed-gui-design` skill §1.2).

---

## 3. Component usage table

Component names and states are `design-system-2026.md` §6; the W2 implementer must not invent a
variant. Every interactive component gets that document's §6.1 focus ring and, if custom-painted,
`response.widget_info(...)` (D3).

| Component (design-system §) | Where | States that must exist |
|---|---|---|
| **State rail** (shell band) | R1, every state | one word per §2.1; edge/word/metric colours per `design-system.md` §3.2/§3.4; `-- ms` + `relay leg` caption when unmeasured; metric never unlabelled (C4) |
| **Notice band** (§2.7, new) | R2 | hidden · precondition · error (with `Details`, `Open log file`) |
| **Primary action** (§6.2) | R4, every state | default · hover · focus-visible (ring) · active · **danger variant** (the `Stop optimizing` kind; its states are defined in `design-system-2026.md` §6.2 as revised per QA R11 — the implementer must not infer it) · disabled (+reason line) · loading (`STARTING…` from `SessionCommandState.in_flight`, `RESTARTING AS ADMINISTRATOR…` from the elevation handshake) · error (returns to ready/disabled with the band). Gate: ready = elevated ∧ connected ∧ `control_registered` ∧ a relay is selected ∧ not boosting; unelevated shows the restart label only when the boost can otherwise start (M1) |
| **Combo** (§6.3) | Relay row; Game row; Settings → Language | default · hover · focus-visible · open · disabled · loading (`Discovering relays…`) · error (visible cause + `Try again`) |
| **Text field** (§6.4) | Manual server; proxy add; editor fields; process search | default · hover · focus-visible (`selection.stroke` fix) · active · disabled · error (danger ink + word + reason) |
| **Checkbox** (§6.5) | Privacy telemetry; Routing retest; editor dynamic server | default/unchecked · hover · focus-visible · active · checked · disabled · persist-error line |
| **Sheet** (§6.6) | Settings · Update · Game editor · Target picker | default · loading (update check/install) · error (danger result line) · dismissal per §2.4; focus confined, Esc closes |
| **List row** (§6.7) | Ledger rows; relay combo options; process rows; dense proxy rows | default · hover · focus · selected (6.70:1 fix) · active · disabled · loading · empty · error |
| **Map** (painter; tokens) | Route card, normal height only | markers: active relay `accent` ring / others `border_1` / server `warn`; route arc `accent`; **five literal colour sites replaced by tokens** — definition: 4 × `Color32::from_rgb` (`globe.rs:71-73,125`) + 1 × `Color32::WHITE` (`:133`); the audit's "four" counts only `from_rgb` (QA R16); marker hit rect 24×24; no motion |
| **Footer buttons** (stock themed) | R6 | default · hover · focus ring · active · disabled; `HIT_MIN` 24 target |
| **Tile** (§6.8) | **not used in W2 v1** — the Game row keeps the one-step combo (critique req. 8 satisfied by one step); the contract stays defined for a later picker | — (deferred, no invented surface) |
| **Sparkline** | **removed in W2 v1** — M14 asked for a label + fixed scale or removal; a min-max-normalised shape is not a measurement. If it returns it needs a new component contract (fixed scale, unit, age, reduced-motion rule) | — |
| **Focus ring helper** (§6.1) | every interactive control | 2 px `accent_text` outside the rect; fill unchanged by focus; stock `widgets.active` no longer steals the accent fill |

**Ledger row visibility** (one row per fact; `--` means hint ink, not a fake value)

| Row | Idle (disc.) | Idle (conn.) | Holding | Relaying | Direct fallback / retest / rejects | Error |
|---|---|---|---|---|---|---|
| `Relay` | `--` | label or address | label or address | label or address | abandoned relay named | frozen |
| `Relay RTT` | hidden | when measured | when measured | when measured | hidden (not the route) | frozen |
| `Server ping` | hidden | hidden | hidden | when fresh for this server | hidden | frozen |
| `Server leg` | hidden | hidden | hidden | §2.3 | hidden (direct route) | frozen |
| `Path` | hidden | hidden | `Holding` | hidden (route is the relay) | `Direct fallback` / `Server rejects the relay` | frozen |
| `Target` | only when a process is targeted | same | same | same | same | frozen |
| `Game server` | hidden short mode | `Not measured yet` | scan/`Not measured yet` | verbatim `ip:port` | last known / `Not measured yet` | frozen |
| `Relayed` | `No traffic relayed yet` | `No traffic relayed yet` | `No traffic relayed yet` | `{sent} up · {returned} down` | frozen pair | frozen |

---

## 4. Per-screen platform requirements (from the parity plan)

Tiers are the three-tier vocabulary of `cross-platform-2026.md` §7 — **B** build, **H** headless,
**R** runtime — and are never collapsed into "parity". This Windows host can produce **B** and the
Windows **R** tier only; Linux/macOS **R** needs owner hardware or a self-hosted runner (§6.3 there).

| Screen / surface | Windows | macOS | Linux | Minimum tier | Gap / owner |
|---|---|---|---|---|---|
| **Shell, window chrome, all route states** | native Win11 decorations, snap, dark title bar; remembered geometry; 200 % checked | traffic lights, app menu; `.app` identity decision | DE decorations; fractional scaling | **R** each | G2.2 (`interface-designer` spec + `desktop-platform`); packaging G2.4 |
| **Rail / notice / action semantics** | identical copy; UAC wording | `Restart to grant network access` (no "administrator") | same as macOS; privilege = `CAP_NET_ADMIN`, not root | **R** each | G1.2, G1.3 |
| **Relay row / discovery error** | unchanged | backends shared; headless discovery log proof | same | **H** + **R** | engine flows already cross-platform (`cross-platform-2026.md` §3.3) |
| **Game row / presence** | unchanged | one headless scan with a synthetic process | same | **H** | presence policy Q7 open |
| **Tray / menu bar** | real tray, **localized** and state-correct menu (M11) | NSStatusItem + menu (Show/Connect/Disconnect/Quit), state word in tooltip, template icon | StatusNotifierItem where the DE supports it; **honest no-tray mode** on GNOME: close exits, no "Hide to tray" copy | **H** creation + **R** interaction | G1.4, G1.5; open decisions §7 |
| **Shortcuts** | Esc closes sheet, Enter confirms, Ctrl+Q quit, Ctrl+, Settings | ⌘Q / ⌘, / ⌘H (tray only); menu items carry them | Ctrl equivalents | **R** each | G2.1 |
| **Elevation flow (state 10)** | keep token check; handshake before exit; declined-prompt copy | scoped authorization, config paths forwarded, no root GUI | CAP_NET_ADMIN probe, `pkexec` forwarding `HOME`/`XDG_*`, no root config | **H** decision + **R** flow | G1.2, G1.3 (C8) |
| **Autostart** | opt-in `HKCU\…\Run`; hidden start to tray | LaunchAgent; hidden start | XDG autostart `.desktop` | **H** pure writer + **R** login | G1.7 |
| **Updater sheet** | in-place install unchanged | channel truth: installer receipt / package manager / tarball → instruction; no install button that cannot act | same; AUR instruction | **H** decision + **R** click-through | G1.6 |
| **Capability facts (Components)** | one core probe (WinDivert), OS-correct name, `requires` | `pfctl` + authorization | `nft` + privilege | **H** + **R** | G1.1; "Npcap" row only if `pcap-capture` is compiled |
| **Single instance / crash / paths** | unchanged | visible second-launch notice; `.app` paths in docs | visible notice; `xdg-open` | **H** + **R** | G1.8, G1.9 |
| **Notifications** | **no-claim** (window + tray is the notification) | same | same | — (documented no-claim) | G2.3, owner confirms §7 |
| **Reduced motion (WCAG 2.2.2)** | **assigned with one polarity rule:** W2-4 (i) reads `SPI_GETCLIENTAREAANIMATION` via a hand-declared `user32` extern; the value means *client-area animations enabled*; `progress_indicator()` shows a spinner **only when explicitly `On`**, static text when **`Off` or `Unknown`**; parser unit-tested + runtime check | `NSWorkspace.accessibilityDisplayShouldReduceMotion` — **named gap** (returns `Unknown` → static text; no claim) | gsettings `enable-animations` / KDE `AnimationDurationFactor` — **named gap** (returns `Unknown` → static text) | **H** decision + Windows runtime check; macOS/Linux recorded gaps | W2-4 (i); W2-2 (i) owns the seam and receives `Unknown` (no OS read) so it shows static text from the start; the **pre-W2-2 shipped spinners** are the recorded 2.2.2 exposure, not a W2 claim. `design-system-2026.md` §7.3/§7.4 are aligned to this rule in the same W1 pass (task-14) |

Copy rule everywhere: the app may not promise a tier the OS has not reached; user-facing docs and
`docs/user-guide.md:31` are retiered in the same change (`cross-platform-2026.md` §7.3, G0.4).

---

## 5. W2 implementation order (4 slices, no app.rs collision)

The collision is real: the core boundary's S1–S4 and the shell rewrite all edit
`client-gui/src/app.rs` (`gui-core-boundary.md` §8 R7). This spec resolves it by **sequencing, one
writer of `app.rs` at a time**, by giving each slice a narrow region, and by **cutting each write scope
from the boundary's MOVE/SPLIT verdicts so every acceptance line is deliverable by its own slice**
(QA R1 — the previous cut required facts delivered two slices later):

- **W2-1** edits `app.rs` config/discovery call sites + getter reads, **never** `impl eframe::App::ui`;
  it also introduces the `route_surface` presentation adapter so W2-2 needs no later fact.
- **W2-2** owns `impl eframe::App::ui` + the shell/theme/widget modules + `design.rs` + `globe.rs`;
  it consumes the adapter and computes no state.
- **W2-3** edits orchestration after W2-2 lands, rebased on the new shell — never during it; it swaps
  the adapter's `WatchdogFacts` input to the core and proves the output unchanged.
- **W2-4** touches `platform/*`, tray copy and workflows; no `app.rs` except tray-action wiring
  coordinated with W2-3.

**Deliberate reorder of the brief's W2 table** (`gui-refresh-brief.md:205-211` lists both extraction
stages before the shell): S0–S3a are file-local and land first as the spine; S3b/S4/S5 rewrite the same
orchestration the new shell reads, so they are rebased **after** it instead of merged with it. This is
the R7 sequencing option ("shell first and extraction rebased", `gui-core-boundary.md` §8) — not a
different plan.

### W2-1 — "Spine": honest state facts + presentation adapter (owner `rust-dev`)

Scope: core-boundary **S0** (ratchet guard) + **S1** (config model) + **S2** (discovery/probe/identity) +
**S3a** (status DTO/read path) + the **four fact paths** of §2.1 + the presentation adapter. No visual
change; the shell is untouched.
Write scope (every file the acceptance needs): `client/src/app/**` (new: `mod`, `config`, `dto`,
`status`, `discovery`, `relay`, `error`), **`client/src/lib.rs`** (add `pub mod app;` + the facade
re-exports — the only new public surface, `gui-core-boundary.md:157,197`), `client/tests/gui_boundary.rs`
(new), `client-gui/src/config.rs` and `discovery.rs` (thinned), `client-gui/src/main.rs`
(config-load/font-order call site, `:122`), `client-gui/src/app.rs` (config load/save and discovery
**call sites** + getter reads; **never** `impl eframe::App::ui`),
`client-gui/src/app/{choice,ledger,relay_choice}.rs` (the S3a **rule** halves move out; the rendering /
widget halves stay for W2-2 — sequential region split), `client-gui/src/app/route_surface.rs` (new
adapter), `client-gui/src/app/tests.rs` (moved derivation-test clusters + the adapter fixtures), evidence
dir.
Acceptance: (a) guard G1–G5 green with mutation proof (inject → fails → revert → passes, logs saved).
(b) **Rows closable in this slice** — one field path + a both-directions test for every row whose fact
S3a delivers: Idle, Connecting, Holding, Choosing (sub-state), Relaying, Needs elevation (overlay via
`FrontEndFacts`), Error's command/config facts, and the 4 server-leg rows. **Rows declared W2-3-owned:**
Direct fallback, Re-test pending, Server rejects relay (their fact is `WatchdogStatus`, delivered by S4
in W2-3) — W2-1 asserts the *absence* of a second derivation: `AppStatus` exposes no field for them and
`route_surface` receives `WatchdogFacts` as an input it does not derive.
(c) the four fact paths of §2.1 exist and reach the view model: `RelayStatus.connecting`,
`SessionCommandState.in_flight`, `SessionCommandState.last_error`, and the `AppConfig` recovery
diagnostic (source for `error.config`).
(d) `route_surface` adapter: pure (no egui; no thresholds, verdicts, joins or counters — presentation
mapping only) from `AppStatus` + `WatchdogFacts` to the §2 rail/ledger model; a fixture test for each of
the **10 rendered route states** (Relay recovered has no surface by contract, `interface.md:39`) plus the
**absence assertion** that no recovered state can render, plus the 4 server-leg rows and the
cannot-beat caption (the three W2-3-owned fallback fixtures use the current GUI watchdog facts as the
adapter input until W2-3).
(e) `Relayed` pair from one source (C3) with tests.
(f) legacy `config.toml` serializers preserve every shipped key; test counts move, total constant.
(g) **no visual change, with a named method and a real baseline (QA R14):** before the first commit,
capture the base tree at 320 px in connected + disconnected into
`wat/evidence/<date>/W2-1/baseline/`; after the slice capture the same two states at the same window
size; pass = ≤0.1 % changed pixels outside the live-metric mask (mask coordinates recorded in the
capture script) **and** an identical UIA dump (`uia.json`, role + name for every element). The ux-lead
PNG set is not a baseline artifact and is not used for this gate.
Gate: full repo gate (`AGENTS.md:69-76`) with per-crate counts; guard runs in the existing workspace job.
Verification: `qa` re-runs the guard mutation, the pixel/UIA diff and the adapter fixtures.

### W2-2 — "Instrument 2.0 shell" (owner `interface-designer`)

Scope: §1 shell/geometry, §2 states/copy/components, §3 components, §4 shell-owned parts (shortcut
rules, tray copy hooks), token adoption per `design-system-2026.md` §11.1, theme fixes (focus ring,
`selection.stroke`, `border_control`, active/pressed, danger action), a11y semantics; it renders the
W2-1 `route_surface` view model and computes no state.
Write scope: `client-gui/src/app.rs` (the `ui()` shell and the drawing methods — **not** the
config/discovery call sites or orchestration), `app/{theme,ledger,widgets,window}.rs`,
`client-gui/src/globe.rs` (map literals + 24×24 marker), `app/tests.rs` (shell tests + glyph gate),
`design.rs` + pin tests, `locales/en.toml` (+ 8 catalogs), `i18n.rs` keys, **`docs/design/interface.md`**
(the A1–A6 edits and the three layout corrections of Appendix C — approved via §7 Q3), evidence dir.
It **consumes** `app/route_surface.rs` and does not edit it (a change there is a W2-1-owned revision);
`ledger.rs` is listed here for its **rendering** half only — its pure rules moved in W2-1 and the two
slices are sequential, so the region split is `W2-1 rules / W2-2 rendering`.
Acceptance: (a) window min/default/max per §1.1; geometry persisted and clamped to the work area.
(b) breakpoints reachable — screenshots at 360×420, 440×880, 720×880, 1120×880 with no clipping; the
**360 and 440 captures are EN and DE** (the widest measured Latin locale; ja/ko/zh only after Q5); at
360×420 the compact chrome is ≤225 px including the wrapped action and reason line and the scroll region
≥180 px; the two-line 15 px label replaces the 9 px shrink in DE.
(c) every §2 state's word/sub-line/caption/ledger comes from `route_surface` (the shell derives no
state); the mapping is fixture-tested by W2-1 (d) and the shell's layout helper for each view-model
fixture is tested here, incl. the §2.1 precedence ordering.
(d) §3 component states: focus ring on every control; action `widget_info`; **the action's AccessKit
name is read from the UIA tree and recorded**; selected-row 6.70:1; `border_control` on controls;
opaque focus `selection.stroke`; the **danger action variant** per `design-system-2026.md` §6.2 (as
revised, QA R11).
(e) error band + `Details` + `Open log file`, disabled-reason line, Relay row Connect/Disconnect, action
loading (`STARTING…`) and error states, per-sheet close policy.
(f) in-crate measured-contrast test for the D1 floors (including the corrected 1.35:1 row).
(g) **keyboard/focus (QA R7):** Tab reaches every control and the ring is visible; **Tab is confined to
the open sheet and Esc closes it returning focus to the invoking control**; **Space and Enter activate
the custom action**; the action's AccessKit name is read from the tree; the keyboard/UIA transcript is
saved as evidence.
(h) sparkline removed; the five map literal colour sites tokenized (definition in §3); the glyph gate
extended to `native_name` + the painted-mark allowlist.
(i) **reduced motion:** no copy claim; every wait renders through one `progress_indicator()` helper
whose input is `Animations::On | Off | Unknown` (the OS value means *animations enabled*). The helper
shows a spinner **only on `On`** and static text on **`Off` and `Unknown`**. The GUI crate performs **no
OS read** in W2-2, so its input is `Unknown` and every wait shows static text — the **pre-W2-2 shipped
spinners** are the recorded WCAG 2.2.2 exposure; W2-4 (i) supplies the real OS value.
(j) the §6 reachable rows are produced; unreachable rows are listed with their fixtures.
(k) **locale honesty:** the keys still shipping English (the audit's 26 in ja/ko/zh-Hans plus any new
key not yet translated) are **recorded** in `client-gui/locales/README.md` with the Q5 decision pending;
the app makes no "9 locales" claim until then (QA C7).
Gate: full repo gate + the shell's own tests (the adapter is fixture-tested by W2-1).
Verification: `qa` independent §6 matrix, keyboard/UIA transcript, recorded contrast output.

### W2-3 — "Runtime extraction" (owner `rust-dev`)

Scope: core-boundary **S3b** (commands) + **S4** (watchdog/history/relay-health) + **S5** (presence,
game/`DetectedGame`, geo, one config file) + **S6** (CLI adoption, guard ratchet to zero) + the
**update-rule extraction** the boundary schedules nowhere (F-12; QA R10) + the **capability DTO** for
W2-4 (G1.1).
Write scope: `client/src/app/**` (adds `commands`, `watchdog`, `history`, `presence`, `game`, `geo`,
`update`, `capabilities`), `client/src/main.rs` call sites, `client/src/modes/*`,
`client-gui/src/{relay_history,geo,geo_countries,race_watch}.rs` deleted, `client-gui/src/update.rs` and
`self_update.rs` thinned (pure rules out; network/receipt/elevated install stay),
`client-gui/src/app/{watchdog,relay_choice,editor,games}.rs` (plus any `choice.rs` / `relay_choice.rs`
residue — the route-contract halves are already moved by W2-1/S3a), `client-gui/src/paths.rs`
(**SPLIT: config-dir resolution moves; log/crash/open helpers stay** — `gui-core-boundary.md:87,115`),
**`client-gui/data/geo-ipv4.bin` (current location; Q11(a) would move it to `client/data/geo-ipv4.bin`) + the `geo-ip` feature wiring in the manifests** (`client/Cargo.toml`,
`client-gui/Cargo.toml`: enabled by `client-gui`, added to `full` for CI coverage —
`gui-core-boundary.md:419`), **`infra/scripts/build-geo-ipv4.py:38`** (the generator path — **infra-owned
coordinate**: the same PR needs `infra-dev`/`dev-ops`; `gui-core-boundary.md:419,467`),
`client-gui/src/app.rs` orchestration (**not** `ui()`, **not** `route_surface.rs`),
`client/tests/cli_compat.rs` + its golden files, `client/tests/gui_boundary.rs` allowances, evidence dir.
Acceptance: (a) **normalized golden-file CLI comparison (QA R4), written before S6 starts:**
`--status` and `--probe-proxies` are normalised against committed golden files that mask live values
(latency ms, packet/byte counters, version, addresses — the mask list is in the test source), then
diffed; `--list-games` and a malformed `--config` are compared exactly; updating a golden file requires
a reviewed commit, never a silent rewrite.
(b) guard G1–G5 at S6 with the allowance empty except the named adapter exceptions — no raw
`lightspeed_client::engine/games/profiles/registry` call in `client-gui`; **S6 guard-mutation proof**
(inject one violation → guard fails → revert → passes; logs saved — boundary §7 proof obligation).
(c) the DTO is the only state source: `route_surface` fixtures produce **identical output before and
after** the `WatchdogFacts` source swap for the three fallback rows; a G2 grep test fails if a moved
identifier returns to the GUI.
(d) fallback/retest/rejects surfaces unchanged where reachable; named unverified where not.
(e) **S5 config-file acceptance (QA R9):** fixtures for old-only, new-only and both files; precedence
per §7 Q10 (`--config` > canonical > legacy GUI file, never silently merged); the legacy `config.toml`
is read and **never deleted**; a first-run migration test; one draft release-note line in the slice
evidence (published only with the next release, owner gate).
(f) **update-rule extraction (QA R10):** `compare_versions`, `parse_sha256_file` and `ReleaseAsset`
selection move to `client/src/app/update.rs` with their tests; the GUI keeps network/receipt/elevated
install; `axoupdater`/`reqwest`/`sha2` stay GUI-only (no new client dependency); no CLI behaviour change.
(g) **capability DTO for W2-4:** `app::capabilities` publishes `{backend, available, requires, elevated}`
derived from `TrafficInterceptor::check_availability()` plus front-end facts, unit-tested per cfg; W2-4
consumes it and deletes the GUI shell-outs (G1.1).
(h) probe-semantics Q1 and presence-policy Q7 only if the Lead approves them; otherwise both
implementations move unmerged and the divergence stays documented.
Gate: full repo gate + Linux CI workspace tests for the moved modules; per-crate counts.
Verification: golden normalizer run before/after, guard-mutation log, config-fixture output, per-crate
counts, `qa` re-run.

### W2-4 — "Parity" (owner `desktop-platform`; `dev-ops` for workflows)

Scope: `cross-platform-2026.md` G0.1, G0.2, G1.1–G1.9, G2.1, plus the reduced-motion Windows signal
in acceptance (i). **Dependencies:** the `app::capabilities` DTO from W2-3 (g) for G1.1, and W2-2's shell
copy keys.
**Named outside W2 v1 (QA R8):** G2.2's per-OS window/DPI proof (the Windows geometry proof is W2-2 (a);
macOS/Linux need owner hardware), G2.3 notifications (a §7 Q7 decision; no code if the no-claim stands),
G2.4 packaging (`.app`/`.dmg`, aarch64 Linux GUI — owner/`dev-ops`, separate gate).
Write scope: `client-gui/src/platform/**`, i18n platform variants, `Cargo.toml` dependency decisions
(after `sec-ops` audit), `.github/workflows/ci.yml` (`dev-ops`), platform copy + docs retiering,
evidence dir.
Acceptance: (a) `linux-gui` + `macos-gui` PR jobs green with per-OS counts (the two
`cfg(not(windows))` single-instance tests must appear). (b) headless smoke exit 0 with the
window-title/log probe, advisory until proven. (c) tray/menu per §4, `has_system_tray()` real.
(d) shortcut keymap unit tables + runtime pass where hardware exists. (e) **elevation:** the handshake
state machine (ready → restarting → declined/failed → ready; the process does not exit before the
elevated instance confirms) implemented and unit-tested — that closes the slice; the **interactive
UAC-decline flow is U** on this host (the harness is elevated; QA R4) and the parity report records it
unverified with the fixture named (a non-elevated interactive Windows desktop run by the owner/QA).
(f) autostart writers unit-tested in temp dirs. (g) updater channel→copy/action decision unit-tested, no
install button that cannot act. (h) three-tier parity statement per surface with evidence paths —
"parity" only where every surface reached its minimum tier. (i) **reduced motion:** Windows
`SPI_GETCLIENTAREAANIMATION` (**the value means *client-area animations enabled***) read through a
hand-declared `user32` extern (repo pattern, `single_instance.rs:157-179`; no new dependency) →
`Animations::On | Off | Unknown`; `progress_indicator()` shows a spinner **only when `On`** and static
text when **`Off` or `Unknown`**; the parser is unit-tested against fixture values and checked in a
Windows runtime run; macOS/Linux are named gaps returning `Unknown` (static text; no claim); the app
still makes no reduced-motion-setting claim, and the pre-W2-2/W2-4 exposure remains recorded in the
parity report.
Gate: full repo gate (Rust changes), `localization-sync --check`
(`.github/workflows/ci.yml:216-217`), and the new jobs.
Verification: CI run logs and per-OS counts, headless-smoke logs, keyboard/tray interaction evidence
where available, a three-tier statement per surface.

---

## 6. Required screenshot matrix (QA)

Read the screenshot, not the code (`interface.md:245-256`); egui popups are invisible to UIA, so a
popup state is proven only by an image. Widths are logical points; the 200 % column captures the same
440 pt window at OS scale 200 % (physical 880 px). **R** = required and reachable on this host **with
the named fixture**; **U** = required but unreachable here → report unverified with the fixture named;
**—** = no surface by contract. **No row may be marked R when its fixture is unavailable** (QA B2): the
earlier matrix marked Relaying and Needs elevation R while their own notes said otherwise.

| State / surface | 360×420 | 440×880 | 720×880 | 1120×880 | 440@200 % | Fixture / note |
|---|---|---|---|---|---|---|
| Idle, disconnected | R | R | R | R | R | — |
| Idle, connected (Auto, no game) | R | R | R | R | R | prior 320 px capture for comparison: `window-connected-320.png` |
| Idle, connected + detected game | R | R | R | R | R | synthetic detectable process; display name, never the key (M3) |
| Connecting | R | R | R | R | R | fixture: an unroutable relay address keeps the attempt briefly visible; the end state is the Error row |
| Holding — no server locked | R | R | R | R | R | synthetic process + WinDivert (the ux-lead capture `window-boosting-hold-320.png` shows the old OPTIMIZING bug) |
| Choosing (HOLDING sub-line) | **U** | **U** | **U** | **U** | **U** | needs a locked game server; the transient sub-line is covered by the W2-1 `route_surface` fixture (QA B2) |
| Relaying — tunnel up, real game traffic | **U** | **U** | **U** | **U** | **U** | needs a live game session through a relay; a synthetic process relays nothing (audit §9.3). Split from the hold row per QA B2 |
| Direct fallback (three causes) | **U** | **U** | **U** | **U** | **U** | needs a forwarding-not-returning relay; the copy is unit-tested in the adapter fixtures |
| Re-test pending (countdown) | **U** | **U** | **U** | **U** | **U** | same fixture; countdown unit-tested |
| Server rejects relay | **U** | **U** | **U** | **U** | **U** | same fixture + retries spent |
| Relay recovered | — | — | — | — | — | trace-only by contract (`interface.md:39`) |
| Needs elevation | **U** | **U** | **U** | **U** | **U** | the agent harness is elevated (`gui-ux-audit-2026.md:1067-1070`); the fixture is a **non-elevated interactive Windows desktop run by the owner/QA** — named, not available here (QA B2) |
| Error: start refusal | R | R | R | R | R | exe folder without `WinDivert.dll` (T3 method, audit §6.1) |
| Error: registration refused | **U** | **U** | **U** | **U** | **U** | needs a relay that rejects registration |
| Settings sheet (EN + DE) | R | R | R | R | R | DE required for the untranslated-copy check |
| Update sheet (up to date) | R | R | R | R | R | available/install branches **U** |
| Game editor (incl. validation error) | R | R | R | R | R | keyboard-only pass too |
| Target picker (loading/empty/no-match) | R | R | R | R | R | — |
| Language picker (all nine names) | R | R | R | R | R | `한국어`/`简体中文` glyph check (D4) |
| Tray menu + tooltip | R (screen capture) | — | — | — | — | one capture per tray state; macOS/Linux **U** |
| Second-instance notice | R | — | — | — | — | — |
| Focus ring + action AccessKit name | R | R | R | R | R | the name is **read from the UIA tree** and recorded (QA R7), never asserted from the source |
| `Server leg` measured / bounded / prior / unknown | **U** | **U** | **U** | **U** | **U** | measured needs ≥8 validated samples per feed (`interface.md:232-237`) |
| Relay cannot beat direct caption | **U** | **U** | **U** | **U** | **U** | needs a fresh same-server direct ping + the client leg; unit-tested in the adapter fixture |
| Shell at 200 % with no clipping | — | — | — | — | R | plus one 200 % capture of the compact tier if the work area forces it |

Rules: the **360 and 440 captures are produced in EN and DE** (the widest measured Latin locale,
383.0 px — the wrap case); ja/ko/zh captures only after the C7/Q5 translation decision. A case that
cannot be set up is reported **unverified** with the missing fixture named — never fabricated
(`lightspeed-gui-design` skill §8). Screenshots go under `wat/evidence/<date>/qa/` with the state and
width in the filename; the UIA dump (roles + names) is saved beside each focus-row capture.

---

## 7. Open decisions for the owner / Lead

| # | Decision | Options | Recommendation | Who |
|---|---|---|---|---|
| Q1 | Window geometry (§1.1) | ship 360×420 / 440×880 / 1120×1400 as specified, or keep a pinned narrow default | ship as specified: it is the only way the D7 label overflow and D8 moving-action defects close | owner (W1 acceptance) |
| Q2 | Rail metric policy | (a) relay leg only, labelled (contract, §2) or (b) direct ping when fresh (shipped behaviour today) | (a): `interface.md:134-136` is the fact; the direct ping stays in the `Server ping` row with its age | Lead (reverses a shipped behaviour) |
| Q3 | Contract amendments **A1–A6** (Appendix C) + the `interface.md` layout edits | approve so W2-2 can edit `interface.md` (it is in its write scope, §5) | approve: A1–A3 correct facts where the engine and the contract disagree; A4–A6 (Choosing under HOLDING, Relaying's fact, the Needs-elevation home) are coordinated with the architect's DTO revision and close QA R3/R13 — without them W2-1 (b) would be a false acceptance | Lead |
| Q4 | Sparkline | delete (this spec) vs a new fixed-scale component contract | delete in W2 v1; revisit only with a labelled, fixed-scale chart | Lead |
| Q5 | Locale completeness (C7) | translate the 26 English keys + new keys in ja/ko/zh, or re-record the fallback decision with the real key list | translate the state/route vocabulary (the honesty surface) and keep the English fallback for diagnostics only; the pipeline is already in-repo and free | owner (scope) |
| Q6 | Tray scope: Linux SNI vs no tray; macOS packaging | SNI where available + honest GNOME no-tray (recommended) vs no Linux tray; bare tarball vs ad-hoc `.app` | as `cross-platform-2026.md` §9 recommends | owner |
| Q7 | Notifications | explicit no-claim vs implement | no-claim (§4) | owner |
| Q8 | CLI adoption of the watchdog/history runtime (core-boundary Q4) and probe-semantics convergence (Q1) | defer both vs adopt | defer: both change user-visible CLI output and need a release gate | Lead |
| Q9 | Presence policy canon (core-boundary Q7): GUI Auto-only 5 s vs CLI `--watch` auto-start | pick one product behaviour | GUI policy is the product; document the CLI difference | `ux-lead`/`desktop-platform`, Lead records |
| Q10 | Config-file unification (S5) precedence rule | `--config` > canonical > legacy GUI file, never merge silently | as recommended in `gui-core-boundary.md` §9 Q3; needs a release-note line | owner |
| Q11 | **Geo-asset home** (boundary Q2, `gui-core-boundary.md:467`): move the 1.8 MB blob (currently `client-gui/data/geo-ipv4.bin`, 1,810,610 B; target `client/data/geo-ipv4.bin` under (a)) + generated country table into `lightspeed-client` behind a `geo-ip` feature (enabled by `client-gui`, added to `full` for CI), or keep the blob GUI-side and pass `(lat,lon)` into `choose_relay`? | (a) move + feature-gate, artifact delta measured in W2-3; (b) keep GUI-side, pass coordinates | (a) with the delta measured and recorded in W2-3 (`gui-core-boundary.md:455`); if the CLI binary growth is unacceptable, fall back to (b) — the API already takes a position. The generator `infra/scripts/build-geo-ipv4.py:38` is infra-owned, so the move needs `infra-dev`/`dev-ops` in the same PR | Lead + `infra-dev`/`dev-ops` |

---

## Appendix A — historical critique (`ui-design-critique.md`, pass 1): the 16 required changes

Status: **S** satisfied by this spec + a W1 doc (with the pointer), **D** deferred with a reason
(named W2 slice), **R** rejected (never appropriate). None is silently dropped.

| # | Required change (critique) | Status | Where satisfied / why |
|---|---|---|---|
| 1 | Keep an elevation path, one slot three labels | S | §2.2 state 10, §3 primary action; `design-system.md` §6.2 |
| 2 | Rule making the disconnected card honest | S | §2.2 state 1 + ledger matrix: markers gated on connected, `--` values, no stale counters |
| 3 | Destination for Privacy/Maintenance/About/Disconnect | S | §2.4 Settings sheet + Relay row Connect/Disconnect (B2) |
| 4 | Retire dashed-meta copy | S | §2.5 copy deck (`·` canon separator, no `WORD - fragment`; hints name no button) |
| 5 | Ledger-key case, delete `ROUTE` eyebrow | S | `design-system.md` §4.1 rule 3; §3 component table (sentence-case labels only) |
| 6 | Complete the token scale | S | `design-system.md` §1.2–§1.4 adopted as W2-2 acceptance; D2 |
| 7 | Height breakpoint | S | §1.4 short + compact + 200 % rule; replaces the never-exercised `<560` branch |
| 8 | Game choice one step; deliberate picker panel | S | §3: one-step combo kept; tile contract defined but unused (`design-system.md` §6.8) |
| 9 | Relay identity from real data | S | §2 ledger `Relay` = registry label or address; no city field, no invented names |
| 10 | Rail + sparkline one colour rule | S | `design-system.md` §3.4 (rail edge redundant, signal only when boosting); sparkline deleted (Q4) |
| 11 | Migrate all literals | S | `design-system.md` §1.3/§1.4: **five literal colour sites** (4 × `Color32::from_rgb` + 1 × `Color32::WHITE`; QA R16 definition) all in `globe.rs`; W2-2 acceptance (h) |
| 12 | Accent/ink contrast rule | S | D1; `accent` fill-only (3.94:1 as ink = fail), `accent_text` 7.51–8.11:1 |
| 13 | Do not claim reduced motion | S | D5; the signal is **assigned**: W2-2 (i) static-text seam + W2-4 (i) Windows `SPI_GETCLIENTAREAANIMATION`; no copy claim |
| 14 | Shadows off | S | shipped (`app/theme.rs:141-142`); kept |
| 15 | Clamp and scroll every sheet | S | §2.4 + `design-system.md` §6.6 (computed body height, per-sheet close policy) |
| 16 | Ledger mechanics in px | S | §3 `ROW_H` 24 / `LEDGER_LABEL_W` 96; `design-system.md` §4.2 measured drift 0.0 px at token sizes |

## Appendix B — UX audit (`gui-ux-audit-2026.md`): 4 Blockers + 8 Criticals

| ID | Finding | Status | Where satisfied |
|---|---|---|---|
| B1 | Action has no accessible name and no visible focus | S | §3 primary action (`widget_info` + focus ring); `design-system.md` §6.1/§6.2; W2-2 acceptance (d)(g) |
| B2 | Disconnect is one-way; dead end | S | §2.2 state 1 Relay row `Connect`/`Disconnect` (existing unused keys), disabled reason |
| B3 | Failed boost is silent | S | §2.2 state 11 + §2.6; `SessionCommandState.last_error` fact in W2-1; auto-start retries only after the error changes |
| B4 | Registration failure invisible; CONNECTED can be false | S | §2.2 states 2/11: `control_registered` + `registration_error` drive the word; `connecting` fact W2-1 |
| C1 | Direct fallback does not say why | S | §2.2 state 6: three cause sentences from `RelayReplyFault` |
| C2 | OPTIMIZING while traffic is held direct | S | §2.2 state 3 + amendment A1 (engine gate as the fact) |
| C3 | `Relayed` shows an impossible direction pair | S | §2.2 state 5 + amendment A5: one source pair (per-server flow, else redirect pair); W2-1 acceptance (e) |
| C4 | Headline metric unlabelled and changes meaning | S | §2.2 state 1/5: relay leg + visible `relay leg` caption; contract kept (Q2) |
| C5 | Two settings facts are false | S | §2.5: copy generated from `HOLD_FALLBACK` 120 s and `RELAY_RETRY_BACKOFF` [30 s, 120 s]; test asserts the rendering |
| C6 | Internal strings shown as copy | S | §2.6 taxonomy + `Details`/log; scan sentence moved to the rail sub-line (amendment A3) |
| C7 | The 9-locale claim is untrue on the honesty surface | **D** | §2.5/§2.6 catalogue every hardcoded string and the new keys; W2-2 acceptance (k) **records** the keys that still ship English; the translation pass itself is W2-2/W2-4 and the scope decision is §7 Q5 — until then the app says "9 catalogs, N keys pending", never "9 locales". Reason: translation is copy work with its own pipeline, not a layout change. |
| C8 | Elevation exits before UAC resolves | S (plan, one part U) | §2.2 state 10 handshake + loading label + declined recovery copy; W2-4 (e) closes the handshake state machine with unit tests and **records the interactive UAC-decline path U** on this host (QA R4) |

## Appendix C — conflicts resolved and contract amendments

Authority: owner + `interface.md` facts > measured numbers > brief constraints > prose. Each conflict
below is resolved once, here, so W2 does not relitigate it; **amendments A1–A6 are the authorised
`interface.md` edits** (§7 Q3, W2-2 write scope).

| # | Conflict (both sides) | Resolution | Pointer |
|---|---|---|---|
| C-1 | `interface.md:157` short mode "collapse the globe to 76 px" vs the flat map (`MAP_H` 132) and the deleted `GLOBE_SM` token (D2) | map is **omitted** in short/compact mode; no smaller honest size exists, no new inline value | §1.4 |
| C-2 | `interface.md:159` fixed 34 px rail band vs measured `HERO_H` 40 (D6) | rail band = `HERO_H` 40 + caption + sub-line; the 34 px number is corrected in `interface.md` | §1.3 |
| C-3 | `interface.md` ≥620 split vs full-width single action and the status-column cap | split kept; status column capped at 454, action/footer/rail full width | §1.2 |
| C-4 | brief §2 "320 pinned" / `design-system.md` §5.4 "320 pinned" / `cross-platform-2026.md:180` G2.2 "keep the 320 px width as a minimum" vs this spec + `interface.md:164` | the pinned viewport is **superseded** by §1.1; the product minimum is **360×420** and 320 is not a product law anywhere; `desktop-platform` updates G2.2 in its W1 revision (QA M2/R12) | §1.1 |
| C-5 | `interface.md:36` Holding fact `direct_fallback + sent == 0` vs the engine gate `tunnelling_enabled` and the live hold-without-fallback screenshot (audit C2) | **Amendment A1:** Holding's fact is the engine's tunnel gate while a session runs; `is_holding` remains only the fallback-hold variant | §2.2 state 3 |
| C-6 | `interface.md:31` one Idle row vs the shipped words `NOT OPTIMIZING` (`state.idle`) / `CONNECTED`, plus the dead `state.disconnected` key (audit §3.1, §4.1 #20) | **Amendment A2:** Idle is two words; `NOT CONNECTED` replaces the shipped disconnected word and **wires the existing `state.disconnected` key** (value corrected) instead of adding a duplicate; fact `engine.connected` | §2.2 state 1, §2.5 |
| C-7 | `interface.md:41` Error "in the engine's own words" vs audit C6 (no raw internals as copy) | **Amendment A3:** a localized taxonomy sentence leads; the verbatim engine message is one `Details` disclosure / the log away | §2.2 state 11, §2.6 |
| C-8 | `interface.md:134-136` headline = relay leg vs shipped direct-ping headline (audit §3.4.1) | contract wins (Q2): metric is the relay leg, labelled; direct ping stays in its row with its age | §2.2 states 1/5 |
| C-9 | `design-system.md` §6.9 ranks the selected-row ink as **Blocker**; measured **1.35:1** over the selection fill (the doc's 1.20:1 composited over the card — QA R6 corrected the plane, not the severity); the skill ladder makes AA failures **Critical** (the audit's 4 Blockers exclude it) | severity recorded as **Critical** per the ladder, fixed in W2-2 regardless; no spec claim depends on the label | D1, §3 list row |
| C-10 | `gui-core-boundary.md` §8 R7 says S1/S2 are "files outside `app.rs`" while §6 lists `app.rs` call sites for both | resolved by **sequencing + region split** in §5: one `app.rs` writer at a time; W2-1 never touches `ui()` | §5 |
| C-11 | `gui-core-boundary.md` DTO has no connecting, command-in-flight, command-error or config-recovery fact, but the contract needs those states | W2-1 scope grows by the **four** fact paths in §2.1 (`RelayStatus.connecting`, `SessionCommandState.in_flight`, `SessionCommandState.last_error`, the `AppConfig` recovery diagnostic); the architect's DTO revision reconciles the shapes | §2.1 table, §5 W2-1 |
| C-12 | audit M14 leaves the sparkline to `design-system`, which leaves it open (`design-system.md` §5.6) | this spec decides: remove it in W2 v1 (a min-max shape is not a measurement); Q4 | §3 |
| C-13 | `interface.md:34` defines **Choosing** as a state vs this spec's HOLDING word with a Choosing sub-line (audit §3.1: the merge was undocumented) | **Amendment A4:** one head-state (HOLDING) with a Choosing sub-line; `interface.md:34` is annotated as a sub-state; the DTO may keep a sub-field or a variant (architect's revision) | §2.2 state 4 |
| C-14 | `interface.md:35` names the Relaying driving fact as "interceptor counters" generically vs the `run_boosting()` gate + one counter-source pair (audit C3) | **Amendment A5:** the gate selects the state; one pair supplies the counters | §2.2 state 5 |
| C-15 | `interface.md:40` defines **Needs elevation** as a state, and `gui-core-boundary.md:253` keeps a `RouteState::NeedsElevation` variant, vs the overlay fact this spec uses | **Amendment A6:** Needs elevation is an overlay driven by `Capabilities.elevated`, never a rail word; the architect's revision removes or deprecates the variant so there is one field path (QA R3/R13) | §2.2 state 10 |
| C-16 | `ui-design-critique.md:100-111` req. 7's ≤220 px fixed-chrome budget vs the adopted two-line action (D7) + the disabled-reason line (M5) | budget restated from token arithmetic, full sum: **rail 102 + action bar 72 (wrapped action 56 + 2×S2 16) + reason line 18 + footer 33 (24 px `HIT_MIN` target tier + S2 8 + hairline 1) = 225 px** at 360×420, leaving **195 px** of scroll (≥180); at 440 the footer tier is 40 → 241 px. The old ≤220 figure was unachievable with the adopted decisions (QA R4/N7) | §1.3 |
