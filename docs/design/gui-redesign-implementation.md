# GUI redesign — implementation feasibility and cost per direction

**Status:** feasibility study for the owner's redesign round (release hold: **1.12.0 stays untagged**
until a direction is accepted). **Owner of this doc:** `interface-designer-2`, task-99.
**Worktree:** `.agents/worktrees/redesign-feas`, branch `redesign/feasibility`, base
`master da56ff7`. **Docs/evidence only — no product code, no commit/push.**
**Not the design spec:** the direction visuals are owned by `design-system-2`
(task-97, `docs/design/gui-redesign-directions.md` + mockups under
`wat/evidence/2026-11-21/redesign-visual/`), the UX criteria/rubric by `ux-lead-2`
(task-98). This document answers: *can we build each direction in this egui shell,
what exactly has to change, what will it cost, what breaks, and in what order?*

Evidence: `wat/evidence/2026-11-21/redesign-feas/` (`report.md`,
`feasibility-measurements.md` + `.json` + `feas_measure.py`, `code-anchors.txt`,
`mockup-inventory.txt`). Numbers below marked **[measured]** come from that
script against the shipped catalogs and bundled faces; numbers marked
**[estimate]** are CSS-arithmetic or file-delta estimates and are named as such.

---

## 0. Recommendation in one table (details in §4)

| | **A — Instrument, refined** | **B — Quiet native** | **C — Broadcast console** |
|---|---|---|---|
| Verdict | **FEASIBLE — lowest cost/risk** | **FEASIBLE WITH CONDITIONS** (5 owner decisions + biggest build) | **CONDITIONAL — blocked on two spec conflicts** (fit + row model) |
| New owner decisions | type role: does METRIC grow 30→40? decorative copy keys? | light-mode reopen (§9), shadows/elevation (§5.3), brand/web divergence, follow-OS vs fixed light, state word 30→20 | D6 one-line fit/type-role amendment, brand accent (purple→lime), mono-body + localized uppercase, micro-caption role |
| TYPE state word fit **[measured]** | safe at 30 px (0 words >276; max 270 CJK) | safe at 20 px (max 180) | **fails**: 35 px → 10 words >288 (max 315); 44 px → 31 >288 (max 396) |
| Contrast fixes **[measured]** | 0 text / 2 control-border pairs | 6 light + 2 dark text / 2+2 borders | 3 text pairs |
| 420 chrome **[estimate]** | ~217 → scroll ~202 (floor 180 ✓) | ~191 → scroll ~229 (best) | ~216 → scroll ~203 (but hero fit fails) |
| Budget churn | ~+900–1,400 GUI lines | ~+1,700–2,600 | ~+1,100–1,900 (+split app.rs) |
| Effort (implementation) | **4.5–7 dev-days** | **9–14 dev-days** | **5.5–9 dev-days + 1–2 d spec rework** |
| QA effort | 2–3 d | 3.5–5 d | 2.5–3.5 d |
| Release hold cost | ~1.5–2 weeks to accepted+merged | ~3–4 weeks | ~2–3 weeks |
| Risk | low build / **medium owner** ("same family" was already rejected once) | **high build + decision + brand** | medium build / **spec+identity conflicts** |
| Best for | lowest-risk visible refresh, keeps the accepted instrument canon | the strongest "genuinely different, calm, 2026 cross-platform" bet | the strongest "distinctive game-network console" bet |

**Recommended order: mockups → owner pick → one direction branch (tokens/shell →
surfaces → globe/motion) → QA.** Do not pre-build all three or a runtime
three-way theme switch (§5). If the owner wants a real-window preview before
committing, A is the cheapest preview (2–3 dev-days); B needs its light-mode
decision first; C needs its fit decision first.

---

## 1. What already exists to reuse (current tree `da56ff7`)

The redesign is a **presentation-only** change: no `lightspeed-client` core,
workflow, protocol or engine edit is needed for any of A/B/C. That keeps the
GUI-as-wrapper rule (G1/G4/G5) untouched and is the main reason all three are
architecturally feasible.

| Surface | Where | What the redesign reuses | What it costs to restyle |
|---|---|---|---|
| Design tokens | `client-gui/src/design.rs` (362 lines) | one palette (20 opaque + 5 tints), spacing S1–S5, radii, `CONTROL_H`/`ACTION_H`/`HERO_H`/`ROW_H`/`MAP_H`, type scale `CAPTION 11 … STATE 30, METRIC 30`; **pinned by 6 tests** (web-hex identity, tint provenance, geometry, type scale, and the measured AA floors from `design-system` §3, `design.rs:182-362`) | palette values, new roles (chip/micro-caption/border-control), gradients/shadows are all *values* — one file, but pins + contrast table must be re-derived per direction |
| Theme / fonts | `client-gui/src/app/theme.rs` (247 lines) | `font_definitions` (:17) installs Inter Regular/SemiBold, JetBrains Mono, Noto JP/KR/SC ordered by locale; `install_fonts` (:120) before first frame; `apply_theme` (:126-247) sets `Visuals::dark`, planes, widget visuals, `item_spacing`/`button_padding`, `interact_size`, text styles, `Theme::Dark` only | A/C: swap palette + type styles (mirror change to `Visuals`). B: **second palette + light visuals + runtime palette indirection** (the hard part, §4.B) |
| Shell | `client-gui/src/app.rs` (4,628 lines, G3 budget) | `ui()` (:4266) owns the four regions (top rail :2195, conditional route card :2301, bottom action :2649, bottom footer, scroll); `ShellTier`/`shell_tier` (:23-61 window.rs) drives normal/short/compact at 560/480; action/notice/focus helpers | A: rail composition + metric row + chips; B: status-strip relayout + grouped list; C: state block + top bar + ruler + table |
| Custom widgets | `client-gui/src/app/widgets.rs` (521) | `action_button` (custom-painted, three labels), notice/warning, `progress_indicator` (reduced-motion gated), section label, hairline, focus ring, disclosure row, process picker | new chip/badge, table/row grid, bracket toggle, tick ruler, gradient fill helper; all are painter/Frame code, no new deps |
| Route model | `client-gui/src/app/route_surface.rs` (2,268) | the DTO→words adapter: **834 lines of model/builders + 1,434 lines of fixtures (63% tests)**; rows carry `label_key`, values, danger flags, action gate | visually untouched unless a direction changes the *row shape* (C's fixed grid, B's grouped list) — then the model grows and many fixtures move |
| Map | `client-gui/src/globe.rs` (198) | flat equirectangular `draw_map` from the committed Natural Earth 110m blob (`client-gui/data/world-land.bin`), 8 relay city coords, route arc, tokens only | A: layered-stroke + glow treatment (painter shapes); B: 40–56 px minimap (same data); C: **dot-matrix** — precompute land dots once (`build_common.py` shows ~2–3k points; must cache, per-frame dot loop is not free) |
| Sheets | `app.rs` `settings_sheet` :3511 / `settings_body` :3542 / `game_editor_sheet` :3850 / `target_picker_sheet` :3964 / `update_sheet` :4163 | one `egui::Modal` + scroll body pattern | A/C: restyle only; B: bottom-sheet grouping (larger structural change) |
| Copy / locale | `client-gui/src/i18n.rs`, `locales/*.toml` (9 locales) | every string through `i18n::t*`; state/action words uppercased at render (`app.rs:2197,2651`); state words already measured ≤276 px at STATE 30 (spec D6) | A/B/C mockups add decorative copy (wordmark, chips, section labels, policy text) → new keys ×9 locales or documented English fallback; uppercase/width re-measure |
| Guard | `client/tests/gui_boundary.rs` | G1 import surface, G2 forbidden identifiers (~190 entries), **G3 39 per-file budgets**, G4/G5 core rules, 8 tests; runs in the workspace gate | every touched file re-ratchets its budget exactly; one G2/G3 mutation round per slice; app.rs may need a split before it grows past ~5,100 lines |
| Gate baseline | current master (`da56ff7`) | per `wat/state/current-phase.md` final-integration gate: fmt, workspace clippy 0, workspace **2001/0/7**, GUI clippy `-D warnings` 0, GUI **207/0/0**, `cli_compat 14/14`, guard **8/8**, localization + font `--check` | the redesign only changes GUI files/tests + docs; counts grow in GUI only |

**No new runtime dependency is required by any direction.** Everything the
mockups draw is reachable from egui 0.37/epaint: flat/2-stop gradients via
`Mesh`, soft depth via `egui::Shadow` (currently `Shadow::NONE` by canon) or a
flat plane, dotted maps via `Painter::circle_filled`, tick rulers via line
segments, brackets/chips hairlines via `Painter::rect_stroke`. The three mockup
CSS-isms that **do not** map: `backdrop-filter: blur` (A's sheet, unavailable —
use a flat scrim), `box-shadow` blur on light surfaces (egui's Shadow is a simple
feathered rect, close enough on a 320 px window but not a CSS blur), and
per-side borders (paint a line, as the current rail edge does).

---

## 2. Cheap vs structural levers

| Lever | Cost | Notes |
|---|---|---|
| Colour values / planes / tints | **cheap** (design.rs + pins) | per direction measured: A 0 text fails, B 6+2, C 3 → the token table is not a swap, it is a re-measure |
| Type sizes within existing roles, weights | cheap-ish | assets already bundle Inter-Bold but theme loads only Regular/SemiBold; loading a bold family is ~10 lines. **Changing the role values is a decision, not a tweak** (the type scale is pinned and the 30 px state word is the accepted D6 role) |
| Spacing / padding / radius / row height | cheap | but every chrome-band change re-baselines spec §1.3/D9 (239/181 worst) and the 420 tier tests |
| Elevation (planes vs shadows) | cheap mechanism, **decision** | `Shadow::NONE` is canon (§5.3); B's "soft elevation" wants shadows back → owner decision |
| Chip / badge / eyebrow / section label / tick ruler | cheap–medium | pure painter/`Frame` helpers in `widgets.rs`; add tests for 320 fit + contrast |
| Rail/hero composition, metric size | medium | `shell_rail` is 106 lines; A's wordmark row + 40 px metric and C's top bar + giant word are contained there, but change the rail band → chrome re-baseline |
| Map treatment | medium | restyle is `globe.rs`-local; dot-matrix needs precomputation + caching |
| Route list/table restructure | medium–**structural** | C's 92 px label grid and B's grouped list do not match the current row model (`route_surface.rs` rows are label+value with flexible width; some "labels" are full sentences, e.g. `route.cannot_beat_direct`) → model/presentation change + 1,434 fixture lines at risk |
| **Runtime light/dark theme** | **structural** | 441 direct token references + 20 inline `Color32` sites; tokens are compile-time `const`s. A real light mode needs a palette indirection layer (accessor/`Palette` struct) or a full codemod, plus two contrast matrices. This is why B is priced in days, not hours |
| OS theme detection (B follow-system) | structural + platform | egui supports `ThemePreference::System` but the app deliberately pins dark; following the OS needs a platform read (Windows registry, macOS `NSAppearance`, Linux portal) and a documented claim tier per OS |
| New fonts / native faces | **structural/decision** | canon: bundled faces fixed; B's "system-feeling" is Inter at system-like sizes, not Segoe UI/SF (licensing + glyph gate + new asset). C's mono body + uppercase localized strings exposes the RU fallback (Inter/JBMono are Latin-only; ru resolves to epaint Ubuntu-Light — **a console identity in JBMono cannot survive Russian/CJK**) |
| Motion | medium, policy-bound | canon allows only the spinner today and forbids ambient motion; none of A/B/C's static mockups requires motion (good — keeps reduced-motion risk low). Adding it needs `Animations` gating + tests |

---

## 3. What the mockups actually ask for (extracted from the direction builders)

Read from the `redesign-visual` worktree at analysis time (files owned by
`design-system-2`; mockups were still being rendered — the extracted palette,
type and layout values are in `mockup-inventory.txt` and may be refined by
`docs/design/gui-redesign-directions.md` when it lands; re-check at handoff).

**A — Instrument, refined.** Same dark family, retuned palette (`bg0 #08090D …
accent #8B7CF6`, on-accent is dark ink `#0A0912`, new signal `#5BE3A6`), a
wordmark row, `STATE 30` unchanged, **`METRIC` 30→40 mono**, 12.5 px sub-line,
layered map card with badge/footer strip, chips (READY/BOOSTING/FAULT/STOPPED),
uppercase tracked micro-labels, action gradient + glow, segmented 44 px footer.
Compact hides the sub-line and drops the map to 88 px. It is a
**same-identity polish pass** — the smallest delta to ship.

**B — Quiet native.** Light-first neutral palette with a real dark variant, a
status strip instead of the rail (state word **20 px**, metric 22), soft elevation
(shadows), route as a grouped 44 px-row list with minimap, grouped bottom-sheet
Settings. It is the **biggest visual/direction change and the biggest build**.

**C — Broadcast console.** Dark technical/editorial, JBMono body, **state word
44 px (35 compact)**, lime primary action, section-numbered headers, fixed 92 px
label column, dot-matrix telegraph map, bracket toggles. It is the most
**distinctive** and the most **spec-conflicting**.

---

## 4. Per-direction impact, cost and verdict

### A — Instrument, refined — **FEASIBLE, lowest cost, medium owner risk**

**egui approach.** Palette swap + a few new value-roles in `design.rs`; `apply_theme`
gains the gradient/glow conventions (no blur); `shell_rail` grows a wordmark row
and a 40 px metric row; `shell_route_card` grows a header strip (eyebrow + chip)
and a map-footer strip; `globe::draw_map` gains the layered-coastline + glow
rings (all painter shapes, same data); `widgets.rs` gains `chip`, `eyebrow`,
`tick_rule`, `gradient_fill`; footer becomes segmented but keeps `HIT_MIN`/compact
arithmetic. Files: `design.rs`, `app/theme.rs`, `app.rs` (shell `:2182-2725`),
`app/widgets.rs`, `globe.rs`, `app/tests.rs`, guard budget, docs.

**Measured blockers/notes.** **[measured]** state word at 30 px: 0 words over
276 px (max 270), so D6 stays intact. **[measured]** contrast: 0 text-role pairs
below 4.5 — but its `--line2` used as a control boundary is only **1.5–1.6:1**,
so A must add a `border_control`-style token (the current tree already has the
pattern: `border_control #65656A`); decorative hairlines stay low by canon.
**[estimate]** chrome: 760 rail ≈130 (+2), action ≈68 (+4), footer ≈46 (−10) →
chrome ≈244 vs 249 today; 420 compact rail ≈108 (neutral), action ≈63 (−1),
footer ≈46 (+6 vs the compact 40) → chrome ≈217 vs 212, **scroll ≈202 ≥ 180 ✓**.
**Decisions needed:** (i) is a 40 px `METRIC` role acceptable (type-scale pin
change; it does not touch the D6 word mandate); (ii) decorative copy keys
(`ROUTE INSTRUMENT`, `READY/BOOSTING/FAULT/STOPPED`, section eyebrows, "8 relays")
— either add to all 9 catalogs (with DE/RU width checks) or reuse existing keys;
(iii) Inter-Bold as a loaded family or accept SemiBold 600 for the mockup's 700.

**Budget/test churn [estimate].** `design.rs` +50–90, `theme.rs` +30–60,
`app.rs` +250–400, `widgets.rs` +140–220, `globe.rs` +70–130, `tests.rs`
+250–400 (re-pin 5 design tests, ~6–10 render/fit tests, new chip/ruler tests),
guard re-ratchet on those files. GUI tests 207 → ~225–240. Chrome/D9 re-baseline
+ compact-tier fixtures updated.

**QA/verification.** 2–3 days: capture matrix, chrome re-measure, contrast
re-pin, glyph/font check, DE/RU label widths, guard mutation, full gate.
Cross-platform risk low (no deps, no new faces, shadows optional/flat).
Reduced motion: no new motion.

**Verdict: FEASIBLE; 4.5–7 dev-days + 2–3 QA days, ~1.5–2 weeks to accepted+merged.**
Main risk is not engineering: "same family, finally finished" is the direction
closest to the result the owner already rejected as unchanged — the palette/type
delta must be genuinely perceptible (the mockup is), and that is an
owner-acceptance risk, not a build risk.

### B — Quiet native — **FEASIBLE WITH CONDITIONS, biggest build, biggest decision load**

**egui approach.** The hard architectural piece: **runtime palette indirection**.
Today 20 palette constants are referenced directly at **441 sites** plus 20 inline
`Color32::from_*`; a light theme means either (a) a `Palette` struct in
`ctx.data`/`OnceLock` with accessor functions (mechanical codemod of 441 sites +
tests), or (b) compile-time feature/const swap (no runtime mode). Then `apply_theme`
builds `Visuals` for both themes, `Theme::Dark`/`Light` via
`ctx.set_style_of`/`set_theme`, and `ThemePreference` only if the owner wants
follow-system (needs platform reads and a per-OS claim). Shell: status strip
replaces the rail (`shell_rail` rewrite), route card becomes grouped list rows
(match 44 px rows), minimap reuses `draw_map`; Settings becomes a full-width
grouped sheet (scroll + focus trap already exist via Modal/ScrollArea but layout
changes). Soft elevation needs `Shadow` values + a canon decision.

**Measured blockers/notes.** **[measured]** fit at 20 px is safe (max 180) — but
**the 30 px state-word role shrinks to 20**, which is a D6/type-role decision
(the canon word size is mandated); **[measured]** contrast in the light palette:
**6 text pairs fail AA** (`ink3` 3.10–3.60 on the three light planes, `ok` 3.99,
`accent` 4.41) and both palettes' control borders fail 3:1 → two re-derived
contrast tables, not one. **[estimate]** chrome: 760 rail/strip ≈106 (−22),
action ≈70, footer ≈46 → chrome ≈222; 420 strip ≈75 (−33) → chrome ≈191,
**scroll ≈229** (best of the three). **Mockup gap to fix before implementation:**
B's 420 variant omits the relay/game control card — the shipped compact scroll
keeps them; hiding them would change the information architecture the owner is
being asked to compare. **Decisions needed:** reopen light mode
(`design-system-2026.md` §9 — it explicitly says a light theme is "a redesign,
not a token swap", out of scope of WF-081, and must be its own accepted design
task with its own contrast table); reopen shadows/elevation (§5.3); brand/web
divergence (site is dark; `design.rs` pins tokens to `web/styles.css`); state
word 30→20; follow-OS vs fixed light; no native faces.

**Budget/test churn [estimate].** `design.rs` +150–260 (two palettes + accessor
layer), `theme.rs` +120–220 (two `Visuals` + selection), `app.rs` +400–650,
`widgets.rs` +180–300, `route_surface.rs` +50–150 (grouped list rows, if row
shape changes) and its fixtures, `globe.rs` +40–80, `tests.rs` +400–600 (dual
contrast matrices, both themes, list/sheet fixtures), optional platform theme
read +60–120, guard re-ratchet (many files; app.rs likely needs the split).
GUI tests 207 → ~250–280. Captures double (light+dark).

**QA/verification.** 3.5–5 days (two themes × capture/contrast/state matrices,
plus theme-switch and OS-preference edge cases). Cross-platform risk medium:
"native" is appearance only (egui draws its own controls; no Windows caption
integration change), follow-OS is three platform reads with separate claim tiers,
and Linux/macOS runtime remains unverified. Reduced motion unaffected.

**Verdict: FEASIBLE WITH CONDITIONS; 9–14 dev-days + 3.5–5 QA days, ~3–4 weeks
to accepted+merged, plus three owner decisions that reopen recorded canons.**
If the owner wants the largest perceptual change and a plausible
"2026 cross-platform utility" story, this is it — at the price of the longest
hold on 1.12.0 and the biggest QA surface.

### C — Broadcast console — **CONDITIONAL; blocked on two spec conflicts, then medium cost**

**egui approach.** Custom state block + 30 px top bar + tick ruler; JBMono body
type styles; section-numbered headers; table rows as a 2-column grid; bracket
toggle; dot-matrix map (precompute land dots from `world-land.bin` and cache;
`build_common.py`'s `land_dots` is the reference algorithm — do it once at
startup, not per frame); lime CTA. Mostly `app.rs` shell + `widgets.rs` + `globe.rs`
painter work; **no motion** in the static mockups (lowest reduced-motion risk).

**Spec conflicts (why it is conditional).**
1. **One-line state word is a hard accepted requirement** (spec F1/D6: every
   uppercased state word ≤276 px at `STATE` 30, one line; two-line grows the rail
   36 px → compact worst scroll 145 < 180; a 24/25 px word breaks the mandated
   role). **[measured]** C's 44 px hero overflows for **31** locale words
   (worst 396), and even its compact 35 px overflows for **10** (EN
   `NO RETURN PATH` 312, DE `NICHT OPTIMIERT` 313, worst CJK 315 > 288 rail).
   So C as drawn requires an explicit **D6/type-role amendment** (per-state size
   clamp, e.g. 44→~30 for long words — which erodes the direction's defining
   oversized readout — or a copy-contract change to shorter state words, which is
   an `interface.md` amendment), *plus* a re-derived chrome table. This is the
   single clearest "would break the accepted contract" flag in this study.
2. **The fixed 92 px label column does not match the real row model.**
   **[measured]** 35 of 108 localized labels exceed 92 px at C's 8 px/.19em, and
   some "labels" are full sentences (`route.cannot_beat_direct`: 350 px EN,
   502 px FR); DE/RU short labels also exceed it (RU "ИГРОВОЙ СЕРВЕР" 126 px).
   C needs per-row variants (grid for short labels, stacked for sentence rows) —
   a design change the mockups do not show, and possibly a
   `route_surface` row-shape change (fixtures at risk).
3. **Identity/accessibility conflicts to decide, not discover:** lime accent
   replaces the purple brand (and diverges from `web/styles.css`); JBMono as body
   + uppercase on localized strings breaks down for RU (Cyrillic falls back to
   Ubuntu-Light — no mono console look) and CJK (proportional); 8–9 px labels are
   below the current `CAPTION 11` floor and need a new micro-caption role with
   its own contrast proof; "[ON]" bracket toggle and uppercase tracking on
   localized row labels need DE/RU width/legibility review; the new
   `LINK ACTIVE/LINK FAULT` status word risks a second state readout competing
   with the state contract (one word, one driving fact — spec §2.1).

**Measured notes.** **[measured]** contrast: 3 text-role pairs fail (`dimmer`
2.78–2.87 on panel/bg, `ice-dim` 4.42) — small token fixes. **[estimate]**
chrome: 760 rail (30 px top bar + state block) ≈166 (+38), action ≈73 (+9),
footer ≈38 (−18) → chrome ≈277 vs 249 today, so the **normal-tier D9 numbers need
re-baselining** (scroll 760−277−1 ≈ 482); 420 rail ≈120, action ≈62, footer ≈34
→ chrome ≈216, scroll ≈203 ≥180 — so the **420 tier survives C's layout**; its
blocker is the word fit, not the 420 chrome.

**Budget/test churn [estimate].** `design.rs` +60–110 (ice/neon/dim/micro-caption
+ contrast), `theme.rs` +40–70, `app.rs` +300–450 (and a **split of the shell into
`app/shell.rs` is recommended** if app.rs would pass ~5,100 lines), `widgets.rs`
+220–380 (grid/table/tick ruler/bracket), `globe.rs` +120–220 (dot cache),
`tests.rs` +300–450, possibly `route_surface.rs` +100–250 if the grid forces row
variants, guard re-ratchet. GUI tests 207 → ~235–255.

**QA/verification.** 2.5–3.5 days, plus D6/fit re-measurement in EN/DE/RU/JA/RU
and a design/spec re-approval round before implementation starts (1–2 days).

**Verdict: CONDITIONAL; 5.5–9 dev-days + 2.5–3.5 QA days once the fit and row-model
decisions land (~2–3 weeks total).** It is the most distinctive direction and the
quickest to make the app look *unlike* Instrument 2.0 — but it cannot be built
as drawn without amending the accepted one-line fit rule, and its mono/uppercase
identity does not survive all nine locales without extra font decisions.
If the owner wants C, budget the spec amendment first and choose between
"clamped hero" (keeps the rule, less console-y) and "copy shortening" (keeps the
hero, amends the state words).

---

## 5. Recommended implementation order and prototype path

**Phase P0 — decisions (Lead/owner, before any code).**
1. Pick the direction (Lead presents the mockups + the UX rubric from task-98).
2. Log the direction decision; collect the per-direction decisions from §4
   (A: metric role + decorative copy keys; B: light mode, shadows, brand/web,
   state-word size, follow-OS; C: fit rule, accent brand, mono/locale, micro-caption).
3. Re-measure the accepted mockup against the real catalogs/faces if the design
   numbers changed (`feas_measure.py` is re-runnable).

**Phase P1 — tokens + theme scaffolding (serial, one writer).** `design.rs` and
`app/theme.rs` first: palette values, new roles (chip/border-control/micro-caption
as needed), type-scale changes, gradient/shadow conventions, `Visuals` updates.
Update the pinned tests and the measured-contrast table **in the same commit**
(a token edit that silently regresses AA must fail the test — that is the
existing discipline in `design.rs:308`). Re-ratchet the G3 budgets for the
touched files. Output: an app that still renders the old shell with the new
values (visible, shippable, testable).

**Phase P2 — shell (rail/status strip, action, footer, hero).** One writer in
`app.rs`; capture 320×760 + 320×420 in EN+DE **at the end of this phase** and
re-baseline spec §1.3/D9 + the compact-tier tests/fixtures if the bands moved.
This is the phase that proves the owner's first impression.

**Phase P3 — surfaces (route card/list/grid, chips, map, sheets).** Can split
across writers once P1 names freeze: `app.rs` surfaces, `widgets.rs` components,
`globe.rs` map, `route_surface.rs` only if the row shape changed. Sheets last
inside this phase (they reuse the same components).

**Phase P4 — globe/hero/motion.** For A/C this is the map craft and (optional)
glow; for B the minimap. No direction's static mockup needs motion; if the owner
asks for any later, it goes through `Animations` + the reduced-motion rules and
its own tests.

**Phase P5 — verification/QA.** §6.

**Fast prototype path (recommended):**
- The HTML mockups are the prototype — they are cheaper and more faithful than a
  half-themed egui build for comparing directions.
- If the owner wants a **real-window look check before committing**, build a
  throwaway spike branch for **A** (≈2–3 dev-days: palette + type + rail/card/
  action/footer/map restyle; no guard/docs work) and capture it — A needs the
  least scaffolding and best represents "is this different enough?".
- **Do not build a three-way runtime theme switch.** Because tokens are
  compile-time constants at 441 sites, a real switch costs 1.5–2.5 dev-days of
  indirection that the accepted direction may throw away; the mockup round plus
  a one-direction spike answers the owner's question faster. A debug-only
  `LIGHTSPEED_THEME` palette swap is cheap **only** after P1's indirection
  exists, i.e. it is a B deliverable (B needs runtime light/dark anyway), not a
  pre-decision tool.
- One direction per branch (`redesign/<direction>` off `da56ff7`), one commit
  per phase, Lead gates and PRs; no commit/push by the implementer per repo rules.

**Parallelization notes.** P1 serial (design.rs/theme.rs single writer). After
tokens freeze, `app.rs` shell and `widgets.rs`/`globe.rs` can be parallel; tests
follow each component. `route_surface.rs` changes (if any) must stay with the
same writer as the row consumers. Guard/doc updates belong to the integrator, not
each implementer.

---

## 6. Verification plan (per accepted direction; same bar as WF-081b)

1. **Guard:** G3 re-ratchet every touched file to the exact measured count (the
   guard refuses stale budgets); G2 unchanged (nothing moves to the core) but
   re-run the G2/G3 mutation rounds with SHA-256-identical restores; 8/8.
2. **Full gate:** fmt; workspace clippy 0; workspace tests (unchanged
   `2001/0/7` unless someone legitimately adds core tests — the redesign is
   GUI-only); GUI clippy `-D warnings`; GUI tests (est. 225–280 per direction);
   `cli_compat` 14/14; localization `-Check`; font `--check`.
3. **Design-pin tests:** palette hex identity (or a deliberate decision record
   that the web/styles.css tie is broken — B/C diverge), tint provenance,
   geometry, type scale, and the measured contrast table (A 1 table; C 1 table;
   B **2** tables). New component tests: chip/ruler/toggle focus and contrast.
4. **The D6 fit gate:** re-measure every rendered state word at its shipped size
   in EN/DE/RU/ES/PT (+JA/KO/ZH for C) with the installed chain — this is the
   blocker for C and must be part of its acceptance, not discovered in QA.
5. **Real-window captures** (the only proof of paint): states
   disconnected/connecting/holding/optimizing/starved/error/settings/details/
   elevation-declined × **320×760** and **320×420** × **EN, DE, RU**
   (+JA/ZH/KO where the direction's typography changes, esp. C) — 20–50 captures,
   no clipping, no occlusion, banner/footer wrap cases (DE footer wrap) included.
   Use the existing harness pattern
   (`wat/evidence/2026-11-21/wf081b-shell-wiring/capture-shell.ps1`:
   client-area CopyFromScreen + same-run UIA geometry) and, for 420,
   `scripts/fixtures/measure-chrome.py` (now on master) to verify the re-baselined
   chrome/scroll floor. §1.3/D9 numbers and the 239/181 worst case must be
   re-derived and recorded if the direction changes the bands.
6. **Interaction/a11y:** UIA read of the action/buttons; Tab order complete,
   focus ring visible on every plane, sheets trap Esc/focus; keyboard shortcuts
   unchanged (Ctrl+Q/,/H, Esc/Enter); selected-row/danger/disabled states;
   reduced-motion On/Off captures for any spinner state (no new motion in
   A/C/B as drawn).
7. **Renderers/DPI:** glow + wgpu spot check (`LIGHTSPEED_GUI_RENDERER`) since
   gradients/shadows can differ; 100 % verified, 200 % remains **U** (single
   display host) exactly as WF-081b recorded.
8. **QA independent pass** (separate agent, own captures) and **owner
   acceptance** on the real window. Only the owner can validate "is it
   perceptibly a new design" and, for B, the brand/light-mode trade; only
   runtime hardware can validate macOS/Linux look, OS dark/light following,
   and 200 % DPI.

**Release hold:** 1.12.0 is prepared on master (`18230dc`) but **untagged**; the
redesign PR merges before any tag. Realistic hold: A ~1.5–2 weeks, C ~2–3
weeks (after a 1–2 day spec decision), B ~3–4 weeks.

---

## 7. Flags (what would break, by contract)

| Contract | A | B | C |
|---|---|---|---|
| Mobile-only 320 pinned / single column / 420–1400 | safe | safe (fix 420 mockup omitting controls — IA must stay) | safe layout; fixed 92 px column vs real row labels needs per-row variants |
| State contract (`interface.md`, one word, one fact) | safe (no new words; chips repeat status redundantly — must be colour-redundant only) | safe if copy reused; "Starvation policy / Warn, keep relaying" is new policy copy → decision | **risk**: `LINK ACTIVE/LINK FAULT` becomes a second status readout; must be deleted or derived from the one word |
| Drift guard | G3 budgets only | G3 heavily + platform theme read (still GUI); app.rs split recommended | G3 + possible row-model change in `route_surface.rs` (its fixtures are 63 % of the file) |
| 420 tiers / D9 (239/181 worst) | re-baseline (+~10 px chrome; floor kept) | **improves** chrome (~−27 px); numbers/test re-baseline | 420 chrome ~216 (floor kept); **normal-tier chrome +~28 needs D9 re-baseline**; hero fit fails the one-line rule |
| Type/AA canon | 0 text fails; add `border_control`; METRIC role decision | 8 text fails to fix + 2 contrast tables + light/shadow/brand decisions | 3 text fails + micro-caption role + mono/uppercase locale decisions + brand accent decision |
| Fonts (bundled, fixed) | optionally load Inter-Bold (asset already bundled) | no change (Inter at new sizes) | mono body breaks for RU/CJK — needs font decision |
| Dependencies | none | none (OS theme read is hand-declared Win32/objc/portal, no crate) | none |
| Motion/reduced motion | none | none | none (static mockups) |

---

## 8. Open questions for the Lead/owner (bundle into one gate)

1. **Direction**: A, B or C? (Lead presents mockups + UX rubric.)
2. **Type roles**: is the accepted `STATE 30`/`METRIC 30`/`CAPTION 11` type scale
   open? A needs METRIC 40; B needs STATE 20; C needs STATE 44/35 + micro-caption
   ~8–9. These are design-system decisions with the one-line fit gate attached.
3. **B only**: reopen light mode (§9) and shadows (§5.3)? follow-OS or fixed
   light? accept web-brand divergence? is "native" appearance-only acceptable?
4. **C only**: amend the fit rule (clamped per-state hero) or shorten the state
   words (`interface.md` amendment)? accept the lime accent brand change? accept
   mono-body with RU/CJK fallbacks?
5. **Copy**: are new decorative/labels keys allowed (A: wordmark/chips/eyebrows;
   B: section labels/policy line; C: brand/status words/section numbers), and may
   they ship in English fallback or must all 9 locales land with the direction?
6. **Release**: confirm 1.12.0 stays untagged until the redesign is accepted, and
   which version the accepted redesign ships as.

---

## Appendix — key current-tree anchors used for the cost model

`client-gui/src/design.rs` tokens :25-115, geometry/type :117-160, pins
:182-362 · `app/theme.rs` fonts :17-93, `install_fonts` :120, `apply_theme`
:126-247 · `app/window.rs` tiers :23-61, height/geometry :33-171, dispatch
:174-341 · `app.rs` shell :2182-2725 (`shell_rail` :2195, `shell_route_card`
:2301, `shell_ledger` :2444, `shell_error_panel` :2605, `shell_action` :2649),
config/settings :2979-3640, sheets :3511-4163, `ui()` :4266 · `app/widgets.rs`
:521 (action/notice/progress/rows/focus) · `globe.rs` map :63-180 · 
`app/route_surface.rs` model :1-834, fixtures :835-2268 · `client/tests/gui_boundary.rs`
G1 :52, G2 :112, G3 :339-479, UI_TOKENS :481 · fonts: `client-gui/assets/fonts`
(README + `subset.json` + `scripts/fonts/subset_gui_fonts.py --check`) ·
mockups: `wat/evidence/2026-11-21/redesign-visual/` (`a-instrument-refined`,
`b-quiet-native`, `c-broadcast-console`, `mockups/manifest.json`) — owned by
`design-system-2`, read-only here.
