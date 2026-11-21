# Direction C — Broadcast console · implementation token & component sheet

**Task:** `task-102` (follows `task-97`/`task-100`) · **Owner:** design-system-2
**Consumer:** `interface-designer-2` (implementation in `.agents/worktrees/redesign-c`)
**Status:** owner-selected direction, implementation-ready spec. **Design/evidence only — this
document does not change product code.** · **Date:** 2026-11-21
**Sources of truth:** mockups `wat/evidence/2026-11-21/redesign-visual/c-broadcast-console/`,
fixtures `…/mockups/dir_c.py`, contrast audit `…/contrast-checks.md`, direction rationale
`docs/design/gui-redesign-directions.md` §5. Where this sheet and a mockup differ, **this sheet
wins** — the differences are listed in §9 as intentional token snaps.

Token naming: `--x` values are the fixture custom properties; `c.x` is the proposed
`design.rs` token name. All colours are **dark-only**; Direction C does not implement light mode.

---

## 1. Palette (dark only)

Canvas is near-black; planes separate by value, never by shadow. Rules are 1 px at two strengths.
`dimmer` is **decorative only** (2.87:1 — never text). Ratios are WCAG 2.2 against the plane named.

| token | hex | role | measured contrast |
|---|---|---|---|
| `c.bg` | `#050607` | window canvas | — |
| `c.panel` | `#0A0C0E` | data rail / sheet panel | — |
| `c.panel_2` | `#0D1113` | inset, hover plane, disabled surface | — |
| `c.plate` | `#111518` | pressed plane / raised inset (rare) | — |
| `c.map_bg` | `#080A0C` | telemetry map panel | — |
| `c.rule` | `#1D2329` | hairline rule (row separators, panel border) | 1.10 vs panel (decorative) |
| `c.rule_strong` | `#2B333B` | section-header rule, panel border on hover | 1.48 vs panel (decorative) |
| `c.control_line` | `#58656E` | control boundary (close button, brackets, select boxes) | **3.27** vs panel, 3.16 vs panel_2 (AA 1.4.11) |
| `c.grid` | `#12171B` | telemetry graticule | decorative |
| `c.grid_land` | `#2A343A` | telemetry land dots | decorative |
| `c.grid_fleet` | `#5E7C86` | telemetry fleet crosses | **3.01** vs map_bg (UI mark) |
| `c.ink` | `#F0F5F8` | primary text, state word | **18.46** bg / **17.84** panel |
| `c.ink_2` | `#A9B4BD` | secondary text, row values, prose | **9.61** bg / **9.28** panel |
| `c.dim` | `#79838E` | muted text, micro-captions (min 11 px), labels | **5.26** bg / **5.08** panel |
| `c.ice` | `#9FE8FF` | data highlight, LIVE indicator, links, focus ring | **14.93** bg / **14.42** panel |
| `c.ice_dim` | `#5F93A2` | section index, secondary `ice`, caret | **5.97** bg / **5.77** panel |
| `c.danger` | `#FF5C5C` | fault, danger label, stop action | **6.70** bg / **6.47** panel |
| `c.danger_tint` | `#160C0D` (= `rgba(255,92,92,.07)` over bg) | banner/error wash | danger text **6.35**; body `#FFD8D8` **14.71** |
| `c.warn` | `#FFB13D` | transient warning tag | **11.21** bg |
| `c.focus` | `#9FE8FF` (= `c.ice`) | 2 px focus ring | **14.42** vs panel (AA 2.4.13) |
| `c.disabled_ink` | `#6B7580` | disabled label (inactive, exempt; kept ≥3:1 UI) | 4.05 panel_2 / 4.33 bg |
| `c.disabled_plane` | `#0D1113` (= panel_2) | disabled fill | — |

**Accent — selected default (lime).** One neon surface per screen, reserved for the primary action.

| token | hex | use | contrast |
|---|---|---|---|
| `c.accent` | `#CDFF45` | primary CTA fill | fill vs bg 17.37; fill vs panel 16.78 |
| `c.on_accent` | `#0A0E02` | CTA label | **16.71** on accent |
| `c.accent_hover` | `#D9FF7A` | CTA hover fill | label contrast 18.9 |
| `c.accent_pressed` | `#B9EA2F` | CTA pressed fill | label contrast 14.9 |
| `c.accent_ink` | `#CDFF45` | accent text only if a lime text is ever needed | 17.37 bg / 16.78 panel |

**Accent — retuned cyan alternative** (same single-accent system; choose one, not both). This is
the "ice as the action" option: cooler, less acid, still distinct from the data `ice`.

| token | hex | use | contrast |
|---|---|---|---|
| `c.accent_alt` | `#7FE3FF` | primary CTA fill | fill vs bg 13.84; vs panel 13.37 |
| `c.on_accent_alt` | `#04121A` | CTA label | **12.95** on alt fill |
| `c.accent_alt_hover` | `#A8F4FF` | hover fill | label contrast 15.18 |
| `c.accent_alt_pressed` | `#62D2F0` | pressed fill | label contrast ~11.4 |

**Danger action.** Never a filled danger: 1 px `c.danger` outline, `c.danger` label, painted 8×8
`c.danger` square; hover plane `panel_2`; focus 2 px `c.focus`; pressed `plate`.

**Rule:** no colour outside this table; no opacity tints on text; tints exist only for
banners/error washes (`danger_tint`). Disabled states use `disabled_ink`/`disabled_plane` and
never the accent.

---

## 2. Type

**Voice:** monospace for every UI string; Inter Regular only for prose sentences (banner body,
settings notes, error detail). One weight (Regular) — no faux bold.

**Families and fallback chain (no new fonts; bundled assets only):**

| family token | chain (first face that has the glyph wins) |
|---|---|
| `font.mono` (UI voice) | `JetBrainsMono-Regular` → locale-ordered CJK Regular: ko `NotoSansKR`, zh-Hans `NotoSansSC`, else `NotoSansJP` → `NotoSansJP` → `NotoSansKR` → `NotoSansSC` → `Ubuntu-Light` (epaint built-in) |
| `font.prose` | `Inter-Regular` → the same CJK chain → `Ubuntu-Light` |

- **JBMono is Latin-only** (229 codepoints; no Cyrillic, no CJK). RU and CJK glyphs fall through
  to the faces above: CJK renders in the Noto face at the same px size (proportional metrics —
  short state words and labels only); **RU Cyrillic renders in Ubuntu-Light, which is proportional**.
  Consequence: RU mono-grid alignment inside long Cyrillic strings is not exact; digits/addresses
  are ASCII and therefore always JBMono, so **tabular numerals and data alignment hold in every
  locale**. The state word and data values are short, so the visual cost is limited.
- **No tofu:** every codepoint of the nine catalogs is drawable by this chain (the shipped
  `every_catalog_glyph_is_drawable_in_every_family` gate already proves the equivalent chain; extend
  its family list to `font.mono` when implementing).
- **Recommended dependency (decision D-C4):** re-cut the JBMono subset with Cyrillic through the
  pinned Q19 pipeline (`scripts/fonts/subset_gui_fonts.py`, +~40 Cyrillic codepoints), so RU UI
  text keeps mono metrics. If not approved, ship the fallback above and record the proportional
  RU fallback in the direction's known limits.

**Sizes / tracking / case** (all values are logical px; line height = `row px` below):

| role | family | px | tracking | case | notes |
|---|---|---|---|---|---|
| state word (`STATE`) | mono | **30** (hard cap) | −0.02em | uppercase (render-time) | one line, `nowrap`; never shrink, never ellipsis; must measure ≤276 px in `font.mono` |
| hero metric | mono | 28 | −0.02em | — | `ice`; unit 11 px `ice_dim`; caption 11 px `dim` |
| section label | mono | 11 | 0.24em | uppercase | `dim`; index `ice_dim` |
| micro-caption / label | mono | **11 (minimum)** | 0.19em | uppercase | `dim`; **no text below 11 px anywhere** |
| data value | mono | 12.5 | 0.01em | as-is | `ink`; highlights `ice` |
| data label | mono | 11 | 0.19em | uppercase | `dim` |
| prose (banner/notes) | Inter | 12 | 0 | sentence case | `ink_2`; banner ink `#FFD8D8` on `danger_tint` |
| error title | mono | 12.5 | 0.02em | as-is | `ink` |
| brand | mono | 11 | 0.34em | uppercase | `ink_2` |
| top meta / footer | mono | 11 | 0.16em | uppercase | `dim` |

- **Tabular numerals:** JBMono is monospaced; keep the numeric `{reading:>3}` padding for the
  metric. ASCII digits never fall back, so numbers stay tabular in all 9 locales.
- **Uppercase rules:** `state.*` words, section labels, micro-captions, CTA label and footer
  labels are uppercased **at render time** (`str::to_uppercase`) because egui has no CSS
  `text-transform`. The catalog copy stays sentence case — no locale edits. CJK uppercase is a
  no-op; DE `ß`→`SS` is the only orthographic change to check in copy review.
- **Line/row heights:** mono 11 px → 16 px row; 12.5 px → 18 px; state 30 px → 32 px line;
  Inter 12 px → 16 px. Use these as `row_height` values (no `round_ui` surprises if the
  implementing code uses `FontId` + `ui.add_space`; measure in the real window).

---

## 3. Spacing, radius, rules, telemetry texture

**Spacing** — Direction C stays on the canonical scale (no off-scale magic numbers):

| token | px | C usage |
|---|---|---|
| `S1` | 4 | label→value gap, tick gap |
| `S2` | 8 | row vertical padding, map inner pad, bracket pad |
| `S3` | 12 | section gap, sheet group gap, banner padding |
| `S4` | 16 | page gutter (left/right), sheet inset, panel padding |
| `S5` | 24 | sheet bottom pad / large separation only |

**Geometry constants:** content width = `320 − 2×S4 = 288`; top bar 30; section rule row 20;
data row min 30 (stacked sentence row: label 16 + gap 4 + prose 16 = 36); panel border 1;
map heights **146 normal / 96 compact**; CTA 48 normal / 46 compact; footer 38 normal / 34 compact.

**Radius:** `R_TAG 2` (brackets/tags), `R_PANEL 3` (panels, CTA, sheet). Nothing rounder; no
pill shapes.

**Rules:** `c.rule` 1 px row separators and panel borders; `c.rule_strong` 1 px section-header
fill rule; `c.control_line` 1 px where a control boundary must be perceivable (AA 1.4.11).
**No filled card chrome:** panels are planes + border, never shadow.

**Telemetry texture (map panel):**
- panel: `c.map_bg`, 1 px `c.rule`, radius 3; content 288×146 (normal) / 288×96 (compact).
- land: **precomputed dot cloud** from `client-gui/data/world-land.bin` (Natural Earth 110m):
  grid pitch **5.6 px** normal (5.0 compact), dot radius **0.85 px**, colour `c.grid_land`.
  Build the dot list once (≈700–900 dots at 288×146 after the land mask; cache it, do **not**
  re-test polygons per frame) and paint as a single `Mesh` of circles or one `PaintCallback`-free
  batch — a per-frame `Shape::circle_filled` loop over the full grid is not acceptable.
- graticule: vertical 1 px `c.grid` at every `w/6`, horizontal 1 px at `h/3`.
- fleet: crosses at the eight real `globe.rs` coordinates, 1 px `c.grid_fleet`, arms 2.6 px.
- active relay: halo circle r 8 `rgba(159,232,255,.09)` + stroke `rgba(159,232,255,.45)` 1 px;
  core r 2.2 `c.ice`; code label 11 px `c.ice` (uppercase, tracking 0.14em). **No game-server
  dot** — the client does not know the server location; never fabricate one.
- tick ruler under the hero: 7 px tall, 1 px `c.rule_strong` top border, vertical ticks every
  8 px (1 px `c.rule_strong` at 75 % alpha), full content width.

**Elevation:** strictly flat. Plane separation only (`bg` → `panel` → `panel_2`/`plate`).
**egui caveats:** use `Frame::NONE` + `stroke` + `fill`; keep `Visuals::window_shadow` and
`Frame::shadow` off; do **not** enable `Visuals::dark_mode` shadows; the only depth cue allowed is
the 1 px rule. Modal: `egui::Modal` with `backdrop_color = Color32::from_black_alpha(174)`
(rgba(2,3,4,.68)); sheet frame = `panel` fill + 1 px `rule_strong` + radius 3 — no shadow.

**Motion intent:** console-instant. State changes paint immediately (no cross-fade, no slide).
The only motion is the **1 Hz blink of the LIVE dot** while the state is connected/optimizing.
Reduced motion (OS source): the dot is steady. Never animate a value; never fade a panel.
**Accepted alternative (review, task-102):** shipping the dot **static in all cases** is an
acceptable, reduced-motion-safe fallback; do not add motion just to match this sheet.
egui: do not call `animate_*`; when blinking is allowed, `ctx.request_repaint_after(Duration::from_millis(1000))`,
otherwise no repaint request; focus ring paints instantly.

---

## 4. Component recipes

All sizes below are exact. Text uses the §2 roles; colours the §1 tokens.

**4.1 Top brand/status line** — 30 px height (28 compact), padding `0 S4`, bottom border 1 px
`c.rule`. Left: `LIGHTSPEED` (brand role, `ink_2`). Right: 5 px LED (`ice` when
connected/optimizing, `danger` when starved/error) + 7 px gap + status segment + ` / ` separator
(`dimmer`, decorative) + version segment 11 px `dim` (`1.12.0`).
*Copy resolution (decision D-C1):* the mockup's `LINK ACTIVE`/`LINK FAULT` and the `STATE` kicker
are **not catalog keys**. Implement without new copy: the status segment shows the localized
`state.*` word already used by the hero (uppercase) and the kicker label is omitted (the rule and
index carry the structure). If the owner approves two new catalog keys on the normal i18n path
(all 9 locales), use `status.link.active` / `status.link.fault`.

**4.2 State kicker + hero** — kicker row: 11 px `dim` `STATE` (or omitted per D-C1) with the
LIVE/FAULT segment: 4 px dot + 11 px `ice`/`danger` uppercase label, 0.24em tracking; gap S2.
Hero: state word 30 px mono uppercase `ink`, tracking −0.02em, single line, top gap S3; measured
against 288 px content — **≤276 px cap** (the old D6 one-line rule). If a locale exceeds 276 at
30 px, the fix is copy (as with DE `OPTIMIERUNG LÄUFT` → `OPTIMIERUNG`), never a smaller word.
Metric readout row under the word, margin-top S2: left = sub-line (mono 11 px? no — sub-line is
prose: Inter 12 px `ink_2`, max 2 lines), right = metric: 28 px `ice` numeral + 11 px `ice_dim`
unit (`MS`) + 11 px `dim` caption on the next line (`RELAY LEG`, from `route.leg_relay`).
Readout uses `Layout::right_to_left(Align::Center)`; metric is its own row (F1-safe).

**4.3 Route data rail** (variable / stacked rows) — panel `c.panel`, 1 px `c.rule`, radius 3,
content pad `S2` vertical / `S4` horizontal.
- normal row: `Grid`/`horizontal` with **max-content label column** (no fixed px grid): label
  11 px `dim` uppercase, value 12.5 px `ink` right-aligned in the remaining width; min height 30;
  1 px `c.rule` bottom separator (last row none).
- **stacked sentence row** (for long values/sentences, e.g. `route.cannot_beat_direct`,
  350–502 px): label 11 px `dim` on its own line, value below as Inter 12 px `ink_2` left-aligned,
  full width, free wrap; min height 36; same separator. Choose stacked when the value's measured
  width at 12.5 px mono exceeds the remaining width for **any** of the nine locales (measure once,
  cache by key).
- rows order: Relay (`route.relay`), Relay RTT (`route.relay_rtt`), Game server
  (`route.game_server` / `route.place_hint`), Relayed (`route.sent_returned` / `route.count_hint`),
  then while boosting: Path trace (`route.route_trace_hops`), Expected (`route.expected_path`),
  Location (`route.location_*`). No user-scrollable table, no zebra, no column rules.
- values: numbers/addresses mono (they stay JBMono); CJK/RU fallback per §2.
- **game-server value:** the shipped DTO carries an `Ipv4Addr` only — render the address without a
  port (`203.0.113.42`); never fabricate `:port` (accepted deviation, task-102).

**4.4 Map / telemetry grid** — recipe in §3. Interaction: hover shows the relay city tooltip
(`relay.*`); the eight markers are the existing `globe.rs` draw; keep the same hit-test semantics
as the shipped map. No labels unless the marker is hovered/active (active shows its code at 11 px).

**4.5 CTA** — one per screen, width 288 (content), height 48 normal / 46 compact, radius 3,
fill `c.accent` (or `accent_alt`), label 12.5 px `c.on_accent` uppercase 0.2em tracking with a
painted `▶` glyph (painter triangle 5×6 px, `c.on_accent` 55 % alpha) at S2 gap; margin `S3` above.
States: default/hover (`accent_hover`)/pressed (`accent_pressed`)/focus (2 px `c.focus` ring,
1 px outside, radius 4)/disabled (`c.disabled_plane` fill, `c.disabled_ink` label, no glow)/loading
(static label + existing `progress_indicator`, reduced-motion gated). Danger variant:
transparent fill, 1 px `c.danger` border, `c.danger` label, 8×8 painted square, hover `panel_2`,
pressed `plate`. **Never** two neon surfaces at once; no gradient.

**4.6 Footer** — 38 px normal / 34 compact, top border 1 px `c.rule`, padding `0 S4`, three
targets: `window.hide_to_tray`, `settings.title`, `action.quit` (11 px `dim` uppercase 0.16em,
minimum 24 px pointer target, focus ring 2 px `c.focus`). Separators are ` / ` in `dimmer`
(decorative, never text). Tray mode only: on no-tray platforms the "hide" item is omitted and the
two remaining items keep the trio's spacing (existing shell behaviour, restyled).
**Wrapped-footer allowance (accepted in review, task-102):** when the localized trio cannot fit one
288 px row (DE `IN DEN INFOBEREICH AUSBLENDEN`), the footer wraps to two rows — 38 px normal,
2×30 px compact — keeping the ≥24 px targets and the 420 scroll floor; do not invent a shorter
catalogue label.

**4.7 Settings sheet** — modal per §3 (scrim + `panel` sheet + rule, radius 3, inset S4 =
16 px left/right, top 14, bottom 14). Header: `/// SETTINGS` 12 px mono 0.24em `ink`, 22×22
close bracket (`X`, 1 px `c.control_line`). Eight sections, numbered `01..08`, each: section row
(11 px index `ice_dim` + name 11 px `dim` 0.24em + 1 px `rule` fill rule + optional meta) then
rows. Sections in shipped-shell order: **Language** (row, `settings.language` /
`settings.follow_system`, value `ice` + caret), **Startup** (`settings.autostart` + bracket
`OFF`, hint prose 12 px `dim`), **Privacy** (`settings.share_stats` + bracket `ON`, Q13
disclosure prose), **Routing** (warn-only `settings.starvation_policy` prose in the editorial
block: 2 px `ice_dim` left rule, prose 12 px `ink_2`), **Relays** (`settings.relays`,
`settings.remeasure`, `proxies.refresh`), **Maintenance** (`update.sheet_title`,
`settings.open_log`), **Components** (literal diagnostic rows: Version, Elevation, Capture,
Npcap, Relays — 11 px label `dim`, 11 px value `ink_2`, 20 px rows, no separators), **About**
(`about.version`, `about.blurb` prose). Row height 26; bracket = 11 px `ice` label with 1 px
`rgba(159,232,255,.3)` border, `R_TAG` 2, padding 2×6. Sheet body scrolls (egui `ScrollArea`,
always-visible bar) — the mockup's clipped 420 state is that scroll, not missing IA.

**4.8 Banners** — warning strip: `c.danger_tint` fill, 1 px `c.danger` border-left 3 px, radius 2,
padding `S3`; tag 11 px `danger` 0.24em (`WARNING / <STATE>`); body Inter 12 px `#FFD8D8`, wrap,
max 4 lines. Only one banner at a time; never modal; warn-only starvation (no automatic route
change, no "reconnecting" claim). The banner is the only surface allowed on top of the rail.

**4.9 Error / details** — panel `c.panel`, 1 px `c.danger` border-left 3 px, radius 2, padding
S3; tag `ERROR / TUNNEL` 11 px `danger`; title 12.5 px mono `ink`; body Inter 12 px `ink_2`;
link row: `DETAILS ▸` / `settings.open_log` at 11 px `ice` with 24 px targets. Details disclosure
reuses the shipped pattern (the panel grows; no modal).

**4.10 Compact 420** — bands: top bar 28, hero block (kicker 16 + state 32 + readout 28) and map
96, route rail shows the first rows and scrolls; CTA 46 + footer 34 fixed. The mockup's bottom
gradient fade is **not** egui-feasible (no shader-free gradient overlay) → replace with the 1 px
`c.rule` under the scroll region, or omit; this is an accepted deviation (D-C5). Scrolling region
at 420 must remain ≥180 px (existing floor).

---

## 5. Interaction states (required for every control)

| control | default | hover | pressed | focus-visible | disabled | loading |
|---|---|---|---|---|---|---|
| CTA | `accent` fill | `accent_hover` | `accent_pressed` | 2 px `c.focus` ring +1 px offset | `disabled_plane` + `disabled_ink` | label + static/`progress_indicator` |
| stop/danger CTA | 1 px `danger` outline | `panel_2` fill | `plate` | `c.focus` ring | disabled plane/ink | — |
| Settings rows / links | `ink_2` value, `ice` link | plane `panel_2` behind row | `plate` | `c.focus` ring | disabled ink | — |
| map markers | `grid_fleet` cross | city tooltip + `ice` ring | — | `c.focus` ring on marker | — | — |
| close bracket | `control_line` border | `rule_strong` border | `plate` | `c.focus` ring | — | — |
| toggles (Startup/Privacy) | bracket `ON` ice / `OFF` dim | plane step | — | `c.focus` ring | disabled ink | — |

No control may rely on colour alone for a state word or value; every state label is text.

---

## 6. Acceptance checklist (for the real build)

Each item is checkable in the real window/CI; failure blocks the direction.

**States & IA**
- [ ] CONNECTED, OPTIMIZING, STARVED (`NO RETURN PATH`), ERROR, Settings implemented; 320×760 and
  320×420 (compact) captured.
- [ ] Settings shows all **eight** shipped sections in order (Language, Startup, Privacy, Routing,
  Relays, Maintenance, Components, About); footer shows **Hide to tray / Settings / Quit** in tray
  mode; no section dropped.
- [ ] Warn-only starvation: banner present, relay still shown, stop action available, no automatic
  route/reconnect claim, `sent>0 · returned 0` counters.
- [ ] Q13 update-check disclosure present in Privacy, verbatim catalog copy.

**Type & fit**
- [ ] State word at **30 px** in `font.mono` measures ≤276 px in every one of the nine locales
  (DE/RU/JA/KO/ZH especially); no shrink, no ellipsis, no wrap; add the measurement to the font
  gate's test data.
- [ ] **No rendered text below 11 px**; micro-captions at 11 px; tracking values from §2.
- [ ] 30 px cap enforced by a test/measure, not by eyeballing; DE `OPTIMIERUNG` (the Q20 copy) used
  if `de.state.optimizing` has not landed yet.
- [ ] Long localized labels (108-label audit / sentence rows such as `route.cannot_beat_direct`,
  350–502 px) render via the **variable/stacked row model**, not a fixed label grid; no clipped
  value in any locale.
- [ ] Tabular numerals: metric/values align across 9→124 ms and 1→10,000 counts.
- [ ] No tofu: the catalog coverage gate passes for `font.mono` and `font.prose`; RU fallback is
  recorded (or the Cyrillic re-cut landed, D-C4).

**Colour & accessibility**
- [ ] Every palette pair in §1 passes 4.5 (text) / 3.0 (large text, boundaries, focus); the
  `contrast-checks.md` pair set is re-run against the implemented tokens (it is the C set: 26
  pairs, 0 failures).
- [ ] Focus ring 2 px `c.focus`, visible on every interactive control, keyboard path complete.
- [ ] Disabled states use disabled tokens, are not colour-only, and are exempt but still ≥3:1 UI.
- [ ] Hover/pressed states exist for CTA, danger CTA, rows, markers, close, toggles.
- [ ] No colour outside §1; no opacity text tints; one accent surface per screen.

**Layout & chrome**
- [ ] Chrome bands: top bar 30/28, CTA 48/46, footer 38/34; content gutter 16; scroll region ≥180
  px at 320×420; no content clipped at 320×760 for CONNECTED and Settings.
- [ ] Map is the dot-matrix recipe (cached dots, one batch), 146/96 high, no fabricated
  game-server point; fleet markers use the eight real coordinates.
- [ ] Motion: no fades/slides; LIVE blink at 1 Hz only when allowed; steady with reduced motion
  (verify the OS source the shell already reads).
- [ ] No new runtime dependency; `design.rs` tokens added with pinned tests updated; the
  wrapper rule (no engine logic in client-gui) untouched.

**Visual delta**
- [ ] Real-window captures at 320×760/420 reproduced against the C mockups with only the §9
  token snaps; deviations listed by severity in the review report.

---

## 7. What the reviewer (me) will compare

When the implementation captures arrive, I will compare real-window captures against
`wat/evidence/2026-11-21/redesign-visual/c-broadcast-console/*.png` and report deviations by
severity:

- **S1 blocking** — tofu/missing glyph, clipped text/control, contrast failure, state/IA missing,
  state word >30 px or >276 px, fake data, motion ignoring reduced motion, new dependency.
- **S2 major** — wrong token value that changes hierarchy (e.g. accent used twice, plane wrong,
  micro-caption <11 px, mono voice replaced by Inter), row-model wrong (fixed grid), band heights
  wrong at 420, focus ring missing.
- **S3 minor** — tracking/letter-spacing drift, rule strengths swapped, telemetry grid pitch/dot
  size off, tick ruler missing.
- **S4 nit** — 1–2 px spacing drift within tolerance, caret shape, separator glyph.

Required captures: 320×760 + 320×420 for CONNECTED, OPTIMIZING, STARVED, ERROR, Settings, plus
one real Settings scroll state and one hover/focus state; raw PNGs + a short command log under
`wat/evidence/2026-11-21/redesign-c/`.

---

## 8. Open decisions (for the Lead/owner before/while implementing)

- **D-C1 — micro-label copy.** The C mockups type several labels that are not catalog keys
  (`STATE`, `LIVE`, `FAULT`, `LINK ACTIVE/FAULT`, `ROUTE`, `CONTROL`, `RELAY LEG`). Recommendation:
  reuse existing keys (`state.*`, `route.leg_relay`) and omit non-key micro-labels; if the owner
  wants them, add keys through the normal i18n pipeline (all 9 locales). Do not hardcode English.
- **D-C2 — accent.** Lime `#CDFF45` (selected default) or the retuned cyan `#7FE3FF` (§1). One
  accent; lime is the owner's pick — cyan is documented for comparison.
- **D-C3 — spacing canon.** C's mockup rhythm used 6/10/14 px; this sheet snaps to the canonical
  S1–S5 (4/8/12/16) — a small visual delta, no new scale.
- **D-C4 — RU mono coverage.** Re-cut the JBMono subset with Cyrillic (Q19 pipeline) so RU keeps
  mono metrics; otherwise the documented proportional fallback applies.
- **D-C5 — scroll fade.** The mockups' bottom gradient is illustrative; replace with a 1 px rule
  (no shader-free gradient in egui).
- **D-C6 — brand accent change.** C moves the product accent purple → lime (owner-selected look);
  the website/marketing palette stays purple unless the owner says otherwise — a brand decision,
  not a token detail.
- **D-C7 — route-row model.** Variable/stacked rows replace the current label/value fixed rows;
  `route_surface.rs` fixtures will churn (feasibility: C is medium build cost).

---

## 9. Intentional deviations from the mockups (implement against this sheet)

| mockup | sheet | reason |
|---|---|---|
| micro text 7.5–10.5 px | **11 px floor** everywhere | accessibility floor (task mandate) |
| spacing 6/10/14 px | S2/S3/S4 = 8/12/16 | canonical scale (D-C3) |
| CTA 50 px | 48 px (46 compact) | shipped `ACTION_H`/canon |
| sheet inset 7 px, top 14 | S4 16 px, top/bottom 14 | canonical gutter |
| map label 7.5 px | 11 px | floor |
| status `LINK ACTIVE/FAULT`, kicker `STATE` | omitted or new keys (D-C1) | no invented copy |
| 44/35 px state word (task-97 mockup) | **30 px cap** | D6 one-line fit (task-99 amendment) |
| dotted land at 5.6 px pitch | same, but cached as one batch | egui performance |
| 1 Hz blink | same, gated by reduced motion | accessibility |

---

## 10. Files

- Sheet: `docs/design/gui-c-token-sheet.md` (this file, uncommitted).
- Mockups and fixtures: `wat/evidence/2026-11-21/redesign-visual/c-broadcast-console/`,
  `…/mockups/dir_c.py`.
- Contrast audit: `…/redesign-visual/contrast-checks.md` (C: 26 pairs, 0 failures).
- Review evidence target: `wat/evidence/2026-11-21/redesign-c/` (implementation's captures).

---

## 11. Implementation review deltas (task-102)

Reviewed the `redesign-c` real-window captures (23-state matrix, EN+DE+RU+JA) against this sheet
and the mockups; full report with evidence:
`wat/evidence/2026-11-21/redesign-visual/c-implementation-review/review.md` (automated pixel
measurements in `review_c.json`).

**Accepted implementation deltas (spec updated above):** DE wrapped footer (§4.6); static LIVE dot
as the reduced-motion fallback (§3); game-server address without port (§4.3); crown copy D-C1
(brand + tone dot + version, no invented `LINK/STATE/LIVE` literals); Settings sheet scrolls
internally (all eight sections present across the 760 captures); RU `Ubuntu-Light` proportional
fallback (D-C4, digits/addresses stay JBMono); telemetry dots cached as one batch; 11 px floor and
the 30 px state cap hold in the real chain (measured 151–238 px word widths); lime accent exactly
once per screen; chrome bands and the 420 scroll floor confirmed (EN 209, DE 183).

**Status: all fixes below were verified in round 2 (2026-11-21) — see the review's §7; the round-2 capture set passes the automated harness 0/28. Kept here as the fix record.**

| id | severity | finding | fix direction |
|---|---|---|---|
| M1 | Major | `console_mini_row` paints `NPCAP` label and long value with zero gap (reads as one word) — `row_fits` measured pair fits but the painted value is wider than its measurement, so the inline branch runs the value into the label | measure label and value with the same font/tracking used to paint; require `label_w + S4 + value_w <= ui.available_width()`; otherwise use the existing stacked branch; unit test with `("Npcap", "not installed (pcap fallback only)")` |
| M2 | Major | hero state word is tone-coloured (Signal→ice, Danger→danger) instead of `c.ink`; the approved mockups/sheet keep tone in dot/tag/banner | paint the hero word `c.ink` for every tone (keep `status_ink` for the crown dot); if the owner wants the tone-coloured word it is a sheet amendment, not a silent change |
| m1 | Minor | Settings sheet leaves the footer band bright below the modal (mockup has sheet+scrim over the whole client) | extend sheet/backdrop to the client bottom or dim the footer while Settings is open |
| m2 | Minor | DE 760 readout: subline and `RELAISSTRECKE` caption share a baseline with ~10 px gap and read together | reserve caption width in the subline wrap (or own line for long captions) |
| m3 | Minor | focus ring not visible in the focus capture and `focus_ring()` is wired only on CTA/bracket/footer | wire `c.focus` ring on settings/close/select/rows too; re-capture a clearly focused control |
| m4 | Minor | implementer report/JSON mismatch (760 footer 38/scroll 511 vs JSON 54/493) | reconcile report numbers with `chrome-measurement-en-760.json` |
| m5 | Minor | forced-state fixture leaves dangling subline "Relaying via … for" in `optimizing-760-en.png` | supply the game placeholder or label the forced-state captures illustrative |
| m6 | Minor | banner/error tags omit the sheet's `WARNING /`·`ERROR /` prefix form | accept or align using existing catalog keys (no invented English) |

No Blocker was found; contrast, IA parity, palette, state cap, text floor, map and bands pass.

**Round-2 verification (2026-11-21):** M1 (Components row stacks; exact unit test), M2 (hero word
`c.ink` in all tones), m1 (modal scrim dims footer), m2 (DE readout reserves the metric block), m3
(focus rings wired + captured on the close bracket), m4 (report follows the JSON), m5 (illustrative
game named in the fixture), m6 (state-word-only tags recorded as the copy decision) — all verified
against the regenerated 30-capture set; automated harness 0/28 failures. The direction is **ready
for owner presentation review**.
