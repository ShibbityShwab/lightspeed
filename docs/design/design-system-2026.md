# LightSpeed desktop 2026 — the visual and component system

**Status:** W1 design set **ACCEPTED** (QA + owner), revised by delta **r11**
(2026-11-20): the owner chose the **mobile-only viewport** — the shipped
`client-gui/src/main.rs` geometry of a 320 px pinned width, one `narrow` column
and a user-resizable 420–1400 px height (initial 760) — and the set was
re-baselined onto master `76aba5d` (warn-only relay starvation, honest location
ladder, route trace, update disclosure). The W1 360×420 / 440×880 / 1120×1400
plan and every two-column layout are **superseded**. The W1 spec's §1.1 geometry
(360/440/1120) is superseded by the same owner decision; its owner reconciles the
spec in the delta wave — this document states the 320 px system and is the
authority for the tokens, type and component contracts. Docs only: no `client-gui`
code, token value or engine/config/fleet change ships with this document.
**Program:** WF-081 (Desktop 2026), wave W1 delta. **Owner:** `design-system` persona.
**Canon inputs:** `docs/design/gui-refresh-brief.md` §6 (direction),
`docs/design/interface.md` (state contract), `.dsh/skills/lightspeed-gui-design/SKILL.md`.
**Historical input, not approved:** `docs/design/ui-design-critique.md` (pass 1,
REVISE, 16 required changes) — used here as a defect checklist (§13).
**Evidence:** every number in this document is produced by a script in
`wat/evidence/2026-11-19/team/design-system/`:
`contrast.py` → `contrast-results.md`, `fonts.py` → `fonts-and-glyphs.md`,
`token_audit.py` → `token-usage.md`. The scripts state their method in their
docstrings and are re-runnable with `py -3`; no number here is an estimate.
The mobile-only delta adds `wat/evidence/2026-11-20/team/design-system/`:
`delta-copy.py` → `delta-copy.md` (320 px copy budgets, wrap-line counts and the
fallback-face audit), plus `delta-r11.md` (finding → change map) and snapshots
of the re-run outputs (`contrast-results.md`, `fonts-and-glyphs.md`,
`token-usage.md`). **Citation baseline:** every repo `file:line` below is verified
against worktree HEAD `0df57ce` (master `76aba5d` + the W1 docs); the delta
re-derived all 54 `app.rs`/`main.rs` citations after the rebase (`cite-check.md`).

**The system in one paragraph.** One dark palette of 53 tokens in
`client-gui/src/design.rs`, one role-based type scale (8 roles, two bundled
weights, Inter + JetBrains Mono + locale-ordered Noto CJK), one 4-px spacing
scale, one radius per surface role, no shadows (elevation is a plane, not a
drop shadow), one density, a seven-state contract for every control, no motion
except what egui animates itself, and no accessibility claim that is not backed
by a measured ratio or a named mechanism. The window is a **mobile-only, 320 px
status instrument**: one column, the state word and the measurement are the
loudest objects, and the single solid accent fill is spent on the one action that
changes the session.

---

## 0. How to read the measurements

| Field | Meaning |
|---|---|
| ratio | WCAG 2.2 contrast ratio, computed from the authored sRGB channels with straight-alpha compositing (`contrast.py` docstring). |
| AA / AAA | WCAG 1.4.3 thresholds: 4.5:1 / 7:1 normal text; 3:1 / 4.5:1 large text. Only the 30 px STATE/METRIC roles claim "large" (§4.1). |
| 1.4.11 | WCAG non-text contrast, 3:1 for visual information required to identify a component or state. |
| measured row height | epaint 0.37's `ascent − descent + line_gap`, each term `round_ui()`-rounded, from the shipped faces' metrics (`fonts.py` docstring; `epaint-0.37.0/src/text/font_face.rs:596-624`). Computed, not rasterised. |
| uses | code lines that reference a token as a value (comments, strings, `info!`/`warn!` macros and same-named locals excluded), `token_audit.py`. |
| shipped | what `client-gui` at this worktree's HEAD actually does, cited by `file:line`. |
| lines | greedy word-wrap count at the render role and column width, measured by `delta-copy.py` from the installed font chain. Type is never shrunk to fit; a string that needs more lines gets them. |

The locked framework is **egui/eframe 0.37.0** (`Cargo.lock`: `egui`, `eframe`,
`epaint`, `egui-wgpu`, `egui_glow` all `0.37.0`; `accesskit 0.25.2`). Every
API-dependent rule in §10 was checked against that version's source in
`%USERPROFILE%\.cargo\registry\src\index.crates.io-*/egui-0.37.0/`.

---

## 1. Token audit

`client-gui/src/design.rs` is the single token module (53 tokens; its own tests
pin the values). The site palette it was adopted from is not loaded at runtime;
there is no second palette *module*. There is a second palette *in practice*:
five inline colour literals in `globe.rs` (§1.3).

**Delta re-audit on master `76aba5d`:** **no token value drift** —
`git diff 62c6cf6..HEAD -- client-gui/src/design.rs` is empty (the token module
was last changed before the W1 snapshot, commit `71fad63`), and the pin tests are
unchanged. **Usage drift exists and is recorded in the tables below:** `R_INLINE`
(cold, 0 uses at the W1 snapshot) is now used for the starvation banner's 4 px
frame; `danger_soft` took a second call site (the banner fill); `text_3`, `warn`,
`danger`, `S1`, `S2`, `CAPTION`, `BODY` gained call sites from the new copy
surfaces.

### 1.1 Colour tokens

| # | token | value | role (what it is for) | uses | status |
|---|---|---|---|---|---|
| 1 | `bg_0` | `#0B0B0D` | deepest plane: sunken text fields, plots, backdrop behind sheets | 3 | used |
| 2 | `bg_1` | `#0F0F11` | window fill / default background | 5 | used |
| 3 | `bg_2` | `#151518` | raised surface: state rail, cards, sheets | 8 | used |
| 4 | `bg_3` | `#1B1B1F` | inactive control fill, quiet filled rows | 7 | used |
| 5 | `bg_4` | `#222227` | hovered control fill (highest plane) | 2 | theme-only |
| 6 | `border_1` | `#25252A` | every hairline: card edges, dividers, rest control boundary | 10 | used |
| 7 | `border_2` | `#333339` | hover hairline | 1 | theme-only |
| 8 | `text_1` | `#F2F2FA` | primary ink: labels, values, the state word when neutral | 17 | used |
| 9 | `text_2` | `#B4B4CC` | prose: help text, sub-lines | 13 | used |
| 10 | `text_3` | `#8A8AA8` | tertiary ink: captions, ledger keys, units, hints, disabled ink, location/trace captions | 30 | used |
| 11 | `accent` | `#6C5CE7` | action hover fill, active map-marker ring, active control stroke; the route arc duplicates this value inline | 3 | used |
| 12 | `accent_strong` | `#7D6FF0` | *claimed* action hover fill | 0 | **DEAD** |
| 13 | `accent_deep` | `#5A4BD4` | action fill at rest/ pressed | 3 | used |
| 14 | `accent_text` | `#A29BFE` | accent as ink: links, focus ring (proposed) | 1 | theme-only |
| 15 | `accent_soft` | `rgba(108,92,231,.1216)` | selection fill, connected rail-edge tint | 2 | used |
| 16 | `accent_line` | `rgba(108,92,231,.2784)` | selection outline / focused-field stroke (shipped) | 1 | theme-only |
| 17 | `signal` | `#00D68F` | good: BOOSTING word, good RTT, saved confirmation | 6 | used |
| 18 | `signal_soft` | `rgba(0,214,143,.1216)` | *claimed* BOOSTING chip tint | 0 | **DEAD** |
| 19 | `info` | `#00CEC9` | *claimed* neutral in-flight fact | 0 | **DEAD** |
| 20 | `info_soft` | `rgba(0,206,201,.1216)` | *claimed* info tint | 0 | **DEAD** |
| 21 | `warn` | `#FDCB6E` | attention: HOLDING word, degraded RTT, recoverable failure, warn-tone notice (contract) | 9 | used |
| 22 | `warn_soft` | `rgba(253,203,110,.1216)` | *claimed* warn tint | 0 | **DEAD** |
| 23 | `danger` | `#FF6B6B` | bad: error text, invalid field ink, bad RTT, STOP label, `NO RETURN PATH` word, starvation-banner ink | 16 | used |
| 24 | `danger_soft` | `rgba(255,107,107,.1216)` | Danger action hover fill; the starvation-banner fill (danger-soft over the card) | 2 | used |
| 25 | `on_accent` | `#FFFFFF` | ink/fill placed on accent: action labels, map marker core | 3 | used |
| 26 | `scrim` | `rgba(11,11,13,.7216)` | modal backdrop | 4 | used |
| 27 | `grid` | `rgba(242,242,250,.0588)` | *claimed* globe graticule | 0 net | **SHADOWED** |

`grid`'s two apparent references are to a local `let grid` in `globe.rs:72` that
shadows the token name; the map paints an inline `#222230` line instead. The
token itself is unused.

### 1.2 Geometry and type tokens

| token | value | role | uses | status |
|---|---|---|---|---|
| `S1` | 4.0 | icon→label gap; label→field gap in narrow rows; banner/spacing after a notice | 13 | used |
| `S2` | 8.0 | default gap between related controls; theme `item_spacing`, `menu_margin`; notice frame padding | 48 | used |
| `S3` | 12.0 | card/sheet inner gutter; button horizontal padding | 10 | used |
| `S4` | 16.0 | panel margin; sheet inner margin; region gutter | 13 | used |
| `S5` | 24.0 | *claimed* separation between major regions | 0 | **DEAD** (kept, see §1.4) |
| `R_INLINE` | 4 | inline-mark radius: the starvation notice's 4 px frame | 1 | used |
| `R_CONTROL` | 8 | field, combo, tile radius | 7 | theme-only |
| `R_CARD` | 8 | the one card radius | 2 | used |
| `R_BUTTON` | 12 | the action slot radius | 1 | used |
| `R_SHEET` | 16 | in-window sheet radius | 1 | used |
| `CONTROL_H` | 40.0 | config control height (combo, field) and egui `interact_size.y` | 3 | used |
| `ACTION_H` | 48.0 | the one action slot height | 1 | used |
| `PLOT_H` | 64.0 | RTT sparkline height | 1 | used |
| `GLOBE_LG` | 92.0 | *claimed* globe size | 0 | **DEAD** |
| `GLOBE_SM` | 76.0 | *claimed* narrow globe size | 0 | **DEAD** |
| `GLOBE_XS` | 72.0 | *claimed* short-height globe size | 0 | **DEAD** |
| `ROW_H` | 24.0 | ledger row allocation | 2 | used |
| `LEDGER_LABEL_W` | 96.0 | wide-breakpoint ledger key column | 2 | used, unreachable in the mobile-only viewport; retired in W2 (§5.4) |
| `CAPTION` | 11.0 | §4 | 35 | used |
| `LABEL` | 12.0 | §4 | 10 | used |
| `BODY` | 13.0 | §4 | 12 | used |
| `EMPHASIS` | 15.0 | §4 | 3 | used |
| `VALUE` | 17.0 | §4 | 1 | theme-only |
| `TITLE` | 20.0 | §4 | 2 | used |
| `STATE` | 30.0 | §4 | 1 | used (but not at 30 px, §4.5) |
| `METRIC` | 30.0 | §4 | 1 | used |

### 1.3 Findings

1. **Nine dead tokens and one shadowed token** (10 of 53 = 19 %). Each carries a
   role in a docstring and none is used: `accent_strong`, `signal_soft`, `info`,
   `info_soft`, `warn_soft`, `S5`, `GLOBE_LG`, `GLOBE_SM`, `GLOBE_XS`, plus
   `grid` (shadowed). `R_INLINE` was on this list at the W1 snapshot and is now
   used by the starvation notice (§1.2) — the delta moved one token from dead to
   live without changing a value. A token that exists, is documented and is never
   used is drift waiting to be copied.
2. **One role has two values.** `design.rs:56` says `accent_strong` is "hover
   fill of the action"; the shipped action hover uses `accent` (`ledger.rs:100`).
   Two tokens claim the hover role.
3. **Two roles are documented but do not match their values.** `border_2` is
   "hover/focus hairline and the scrollbar thumb": focus is `accent_line`
   (`theme.rs:150`) and the scrollbar uses `widget_fg_stroke`
   (`scroll_area.rs:1637`), never `border_2`. `grid` is documented as the
   "globe's graticule" but the map paints a literal.
4. **A second palette in `globe.rs`.** Five inline literals (unchanged by the
   delta — the re-run audit reports the same five sites): map surface
   `#111118` (`:71`), graticule `#222230` (`:72`), land `#4A4A72` (`:73`), route
   `#6C5CE7` (a byte-identical duplicate of `accent`, `:125`), marker core
   `Color32::WHITE` (`:133`), plus a corner radius of `6` (`:74`, off-scale).
   The historical critique counted eight literals in the old `app.rs`; the count
   is now five, all in one file. Definition used across the W1 set (R16): **5
   colour literals = 4 `Color32::from_rgb` + 1 `Color32::WHITE`**; the radius `6`
   is a geometry literal counted separately in §5.6.
5. **Size drift.** The dead `GLOBE_*` tokens describe 92/76/72 px, while the
   shipped map is 132 px tall and at most 430 px wide (`app.rs:2281-2282`) with
   an inline radius of 6. The token module and the renderer disagree.
6. **The shipped state word is not its token size.** `STATE * 0.82` = 24.6 px is
   always taken because the viewport is pinned to 320 px and `narrow` is
   `width < 380` (§4.5).
7. **`VALUE`, `accent_text`, `accent_line`, `bg_4`, `border_2`, `R_CONTROL` are
   theme-only**: they exist through the global `Visuals`, not through a surface
   choosing them. That is acceptable for `bg_4`/`border_2` (stock-widget states)
   but not for the named type role `VALUE`, which is used by no label at all.

### 1.4 Decisions — one role, one value

**Add (each has a role and a measured value; W2 edits `design.rs` and its pin
tests in the same change):**

| new token | value | role | why measured |
|---|---|---|---|
| `border_control` | `#65656A` | the boundary that identifies an interactive control (field, checkbox box, combo) at rest | rest boundary today is 1.19:1; `#65656A` is the first step of the hairline hue `(v,v,v+5)` reaching 3.14:1 on `bg_2`, 3.30:1 on `bg_1`, 3.39:1 on `bg_0` (search in `contrast.py`) |
| `map_land` | `#4A4A72` | land silhouette on the route map | the only map colour that cannot be reused from an existing role; 2.25:1 on `bg_0` is decorative (not a 1.4.11 claim) |
| `HERO_H` | `40.0` | the state rail's hero band height | the shipped fonts lay out a 40 px row (JetBrains Mono 30); the inline 34 px cannot contain it (§4.2) |
| `RAIL_EDGE` | `3.0` | width of the 3 px painted state edge on the rail | shipped at `app.rs:4140-4144`, inline |
| `MAP_H` | `132.0` | route map height | shipped inline at `app.rs:2282`; replaces the dead `GLOBE_*` trio |
| `MAP_W_MAX` | `430.0` | route map maximum width | shipped inline at `app.rs:2281` |
| `HIT_MIN` | `24.0` | minimum pointer target for any control (WCAG 2.5.8) | the shipped 16 px proxy rows cannot host a 24 px target (§5.5) |
| `FOCUS_RING_W` | `2.0` | width of the keyboard-focus ring (painted outside the control) | §3.4: the shipped focus indication measures 1.22:1; a 2 px `accent_text` ring measures 7.89:1 on `bg_1` |
| `ACTION_H_WRAPPED` | `56.0` | the action slot's height when its label wraps to two `EMPHASIS` lines | derivation: two 19 px line rows = 38 + 2 × S2 button padding = 54, rounded up to the next 8-px density step = 56; the two-line action is the adopted spec decision (D7/§1.3) and the silent 9 px shrink is deleted |

**Re-role / wire (no value change):**

| token | new/cleared role | W2 action |
|---|---|---|
| `bg_4` | hovered stock-control fill (drop the "graticule band" claim) | fix the module comment |
| `border_2` | hover hairline of stock controls (transient, non-identifying; hover is never required to operate, so it makes no 1.4.11 claim) | fix the comment; do not use it as a control boundary |
| `border_1` | decorative hairline only (card edges, dividers) | controls switch to `border_control` |
| `accent_text` | accent-as-ink (links **and** focus ring **and** selected-row ink) | wire via `selection.stroke` (§3.4) and the focus helper |
| `grid` | plot graticule | paint the map with `bg_0` surface + `grid` graticule + `accent` route + `on_accent` marker core + `map_land`; delete the five literals |
| `S5` | separation between unrelated content groups inside a sheet | use it as the section break in the settings sheet (the shipped sheet uses ad-hoc `hairline + S2 + S1`) |
| `on_accent` | the only pure-white fill/ink: action ink and map marker core | reuse in the map |
| `R_INLINE` | inline-mark radius: the starvation notice's 4 px frame (master `76aba5d` uses it) | keep — it is live now; do not delete |
| `danger_soft` / `danger` | danger-tone notice fill/ink (starvation banner) in addition to the Danger action | keep; the banner ink measures 5.61:1 on its own fill (§3.2) |
| `warn` | warn-tone notice ink (contract) in addition to the HOLDING word and RTT ramp | keep; warn on the warn-tint measures 9.30:1 (§3.2) |

**Delete (they duplicate a role or describe a surface that no longer exists):**
`accent_strong`, `signal_soft`, `info`, `info_soft`, `warn_soft`,
`GLOBE_LG`, `GLOBE_SM`, `GLOBE_XS`. The deletion is internal to the GUI crate
(no public API); log it in `wat/state/decisions/2026-11.md` with this document
as the reference. If a semantic tint or a neutral info colour is needed later,
it enters with a role *and* a call site in the same change.

**Kept unchanged (values are already right):** `bg_0`–`bg_3`, `text_1/2/3`,
`accent`, `accent_deep`, `accent_soft`, `signal`, `warn`, `danger`,
`danger_soft`, `scrim`, `S1`–`S4`, `R_CONTROL`, `R_CARD`, `R_BUTTON`,
`R_SHEET`, `CONTROL_H`, `ACTION_H`, `PLOT_H`, `ROW_H`, and the eight type roles.

### 1.5 Enforcement

No new token without a role, no token without either a call site or an explicit
"reserved" decision. W2 adds two tests next to the existing pin tests in
`design.rs`:

1. a **role-uniqueness test**: every token is referenced by at least one code
   line outside `design.rs` (the same count `token_audit.py` performs, expressible
   as a source scan in a `#[test]`);
2. a **measured-contrast test**: a small luminance helper and assertions for the
   §3 floor pairings, so "AA" cannot regress by a value edit (the existing
   `opaque_tokens_match_the_site_hexes` test only pins drift to the website).

---

## 2. Colour application rules

1. **`accent` is a fill, never ink.** Measured: `accent` as 13 px ink on `bg_1`
   is **3.94:1 — AA fail**. Accent-as-ink is `accent_text` (**8.11:1** on
   `bg_0`, 7.89 on `bg_1`, 7.51 on `bg_2`). This rule is not stylistic; it is the
   reason accent text is never used.
2. **One bright fill.** `accent_deep`/`accent` fills appear only on the one
   affirmative action (rest/hover) and on the pressed/focused *state of that
   action*. Stock controls never take an accent fill: the current
   `widgets.active` mapping does, and that is a defect (§3.3, §6.9).
3. **Semantics are fixed and never decorative.** `signal` = good, `warn` =
   attention (degraded but working, recoverable failure), `danger` = bad
   (failure, destructive, invalid), `text_1/2/3` = neutral ink. `info` is deleted:
   a neutral in-flight fact is carried by the word and `text_2`, not by a colour.
4. **No state is carried by colour alone.** The state word is always a word; the
   rail edge and the metric colour are redundant; controls carry a label, a
   mark or a shape; the disabled state changes the ink *and* blocks the action.
   `rtt_colour` (`theme.rs:208-216`) is a redundant channel on a number that is
   also printed.
5. **One role, one value.** If two surfaces need the same appearance, they share
   the token; if one surface needs a new appearance, it gets a role and a
   measured value, not an inline literal.
6. **Tints are alpha on the plane they sit on.** `accent_soft` over `bg_2` is
   `#201E31`; `danger_soft` over `bg_1` is `#2C1A1C`; the scrim over `bg_1` is
   `#0C0C0E` (`contrast-results.md` §1). Never pre-mix a tint into a hex.
7. **Dark-first is the only plane family.** Every ratio in §3 is a dark-plane
   ratio; none transfers to a light plane (§9).

---

## 3. Measured contrast

### 3.1 Method

sRGB relative luminance (WCAG formula), straight-alpha compositing, authored
8-bit channels from `design.rs`. Full script and output:
`wat/evidence/2026-11-19/team/design-system/contrast.py` and
`contrast-results.md`. `on_accent` on `accent` measures **4.86:1**, matching the
historical critique's 4.86:1 on the same pair (`ui-design-critique.md` req. 12) —
the only independent in-repo validation of the method.

**Paint-order rule.** A ratio composites each colour over the plane it is
actually painted on, in the order egui paints it. The selected-row label is
`selection.stroke` painted over the row's own `selection.bg_fill`
(`Style::interact_selectable`, `egui-0.37.0/src/style.rs:360-369`), so its ink is
`#352F64` on `#201E31` = **1.35:1**; compositing that ink over the card behind
the row gives 1.20:1, which was the first-pass measurement error (QA review r1).
A frame stroke is painted inside the frame (`Frame` uses `StrokeKind::Inside`,
`containers/frame.rs:475`), so a focused field's stroke composites over the field
fill. `contrast.py` encodes this rule; the regenerated `contrast-results.md`
records 1.35:1.

### 3.2 Text pairings (41 shipped pairings)

| role | ink on surface | px / face | ratio | AA | AAA |
|---|---|---|---|---|---|
| primary ink / window plane | `#F2F2FA` on `#0F0F11` | 13 Inter Reg | **17.19:1** | PASS | PASS |
| primary ink / rail band (state word CONNECTED/IDLE) | `#F2F2FA` on `#151518` | 30 Inter SemiBold (large) | **16.36:1** | PASS | PASS |
| primary ink / card + section label | `#F2F2FA` on `#151518` | 12 Inter SemiBold | **16.36:1** | PASS | PASS |
| primary ink / sheet body | `#F2F2FA` on `#0B0B0D` | 13 Inter Reg | **17.66:1** | PASS | PASS |
| sheet title | `#F2F2FA` on `#0B0B0D` | 20 Inter SemiBold | **17.66:1** | PASS | PASS |
| prose / rail sub-line | `#B4B4CC` on `#151518` | 13 Inter Reg | **8.97:1** | PASS | PASS |
| prose / sheet | `#B4B4CC` on `#0B0B0D` | 13 Inter Reg | **9.68:1** | PASS | PASS |
| prose / card | `#B4B4CC` on `#151518` | 13 Inter Reg | **8.97:1** | PASS | PASS |
| ledger key on card | `#8A8AA8` on `#151518` | 11 Inter Reg | **5.45:1** | PASS | FAIL |
| sheet caption | `#8A8AA8` on `#0B0B0D` | 11 Inter Reg | **5.88:1** | PASS | FAIL |
| field placeholder / hint | `#8A8AA8` on `#0B0B0D` | 13 Inter Reg | **5.88:1** | PASS | FAIL |
| button ink rest | `#F2F2FA` on `#1B1B1F` | 12 Inter SemiBold | **15.42:1** | PASS | PASS |
| button ink hover | `#F2F2FA` on `#222227` | 12 Inter SemiBold | **14.22:1** | PASS | PASS |
| button ink pressed **or keyboard-focused** | `#FFFFFF` on `#5A4BD4` | 12 Inter SemiBold | **6.19:1** | PASS | FAIL |
| checkbox checked mark | `#F2F2FA` on `#1B1B1F` | 14 painted mark | **15.42:1** | PASS | PASS |
| the one action, rest | `#FFFFFF` on `#5A4BD4` | 15 Inter SemiBold | **6.19:1** | PASS | FAIL |
| the one action, hover | `#FFFFFF` on `#6C5CE7` | 15 Inter SemiBold | **4.86:1** | PASS (thin) | FAIL |
| the one action, disabled | `#8A8AA8` on `#1B1B1F` | 15 Inter SemiBold | **5.14:1** | PASS | FAIL |
| Danger action, rest | `#FF6B6B` on `#1B1B1F` | 15 Inter SemiBold | **6.19:1** | PASS | FAIL |
| Danger action, hover | `#FF6B6B` on `#2C1A1C` | 15 Inter SemiBold | **5.95:1** | PASS | FAIL |
| **selected list-row label (shipped)** | `#352F64` on `#201E31` (ink over the selection fill) | 13 Inter Reg | **1.35:1** | **FAIL** | FAIL |
| link / card | `#A29BFE` on `#151518` | 13 Inter Reg | **7.51:1** | PASS | PASS |
| link / sheet | `#A29BFE` on `#0B0B0D` | 13 Inter Reg | **8.11:1** | PASS | PASS |
| link / window plane | `#A29BFE` on `#0F0F11` | 13 Inter Reg | **7.89:1** | PASS | PASS |
| state word BOOSTING | `#00D68F` on `#151518` | 30 Inter SemiBold (large) | **9.56:1** | PASS | PASS |
| state word HOLDING (warn) | `#FDCB6E` on `#151518` | 30 Inter SemiBold (large) | **12.10:1** | PASS | PASS |
| state word NO RETURN PATH (danger; shipped value `No return path`) | `#FF6B6B` on `#151518` | 30 Inter SemiBold (large; shipped 24.6 px) | **6.57:1** | PASS | PASS |
| metric good | `#00D68F` on `#151518` | 30 JBMono (large) | **9.56:1** | PASS | PASS |
| metric attention | `#FDCB6E` on `#151518` | 30 JBMono (large) | **12.10:1** | PASS | PASS |
| metric bad | `#FF6B6B` on `#151518` | 30 JBMono (large) | **6.57:1** | PASS | PASS |
| metric absent (`-- ms`) | `#8A8AA8` on `#151518` | 30 JBMono (large) | **5.45:1** | PASS | PASS |
| invalid field ink | `#FF6B6B` on `#0B0B0D` | 13 Inter Reg | **7.09:1** | PASS | PASS |
| validation message | `#FF6B6B` on `#0B0B0D` | 11 Inter Reg | **7.09:1** | PASS | PASS |
| save confirmation | `#00D68F` on `#0B0B0D` | 11 Inter Reg | **10.31:1** | PASS | PASS |
| quality warning | `#FDCB6E` on `#151518` | 11 Inter Reg | **12.10:1** | PASS | PASS |
| invalid address hint | `#FDCB6E` on `#151518` | 13 Inter Reg | **12.10:1** | PASS | PASS |
| config error | `#FF6B6B` on `#0B0B0D` | 11 Inter Reg | **7.09:1** | PASS | PASS |
| disabled stock button ink | `#828288` on `#131316` (50 % opacity composite) | 12 Inter SemiBold | **4.85:1** | PASS | FAIL |
| starvation-banner text (danger tone) | `#FF6B6B` on `#311F22` (danger-soft over the card) | 13 Inter SemiBold | **5.61:1** | PASS | FAIL |
| notice warning line (warn tone, contract) | `#FDCB6E` on `#312B22` (warn-soft over the card) | 13 Inter SemiBold | **9.30:1** | PASS | PASS |
| location / route-trace caption on card | `#8A8AA8` on `#151518` | 11 Inter Reg | **5.45:1** | PASS | FAIL |

**Result: 40 of 41 shipped text pairings pass AA. The one failure is the
selected list-row label at 1.35:1 (the 28 %-alpha accent ink painted over the
row's own selection fill, §3.1), and it is a single token decision, not a
design mystery** (§3.4). AAA is met by 28 of 41; the AAA misses are all
small-size secondary ink and coloured ink, which the canon does not claim
(AAA is `text_1` on every plane: 15.4–17.7:1). The delta's new copy is fully
covered by these rows: the danger-tone notice and the warn-tone notice both
pass AA (5.61:1 / 9.30:1), and the new location/trace captions are the same
`text_3`-on-card pairing as the ledger keys (5.45:1).

Non-shipped rule-justification row: `accent` as 13 px ink on `bg_1` =
**3.94:1** (AA fail), which is why §2 rule 1 forbids it.

### 3.3 Non-text pairings (WCAG 1.4.11, 3:1)

| element | colours | ratio | verdict |
|---|---|---|---|
| card fill vs window plane | `#151518` / `#0F0F11` | 1.05:1 | decorative (no claim) |
| card hairline | `#25252A` / `#0F0F11` | 1.26:1 | decorative |
| hover hairline `border_2` vs card | `#333339` / `#151518` | 1.45:1 | decorative |
| **input fill vs card** | `#0B0B0D` / `#151518` | **1.08:1** | **FAIL** (identify) |
| **input rest border vs card** | `#25252A` / `#151518` | **1.19:1** | **FAIL** (identify) |
| **input rest border vs own fill** | `#25252A` / `#0B0B0D` | **1.29:1** | **FAIL** (identify) |
| **input focused border (`accent_line`) vs card** | `#26224A` / `#151518` | **1.22:1** | **FAIL** (focus) |
| **input focused border vs own fill** | `#26224A` / `#0B0B0D` | **1.32:1** | **FAIL** (focus) |
| **button rest fill vs sheet** | `#1B1B1F` / `#0B0B0D` | **1.15:1** | **FAIL** (identify) |
| **button focused fill vs rest fill** | `#5A4BD4` / `#1B1B1F` | **2.77:1** | **FAIL** (focus) |
| button focused stroke | `#6C5CE7` / `#0B0B0D` | 4.05:1 | PASS |
| **action disabled fill vs ready fill** | `#1B1B1F` / `#5A4BD4` | **2.77:1** | **FAIL** (state) |
| **selected row fill vs popup** | `#201E31` / `#151518` | **1.12:1** | **FAIL** (state) |
| scrollbar handle (hover) vs sheet | `#96969B` / `#0B0B0D` | 6.68:1 | PASS |
| **rail edge CONNECTED (`accent_soft`) vs rail** | `#201E31` / `#151518` | **1.12:1** | **FAIL** (state, redundant) |
| rail edge BOOSTING (`signal`) vs rail | `#00D68F` / `#151518` | 9.56:1 | PASS |
| **checkbox box fill vs sheet** | `#1B1B1F` / `#0B0B0D` | **1.15:1** | **FAIL** (identify) |
| **checkbox box border vs sheet** | `#25252A` / `#0B0B0D` | **1.29:1** | **FAIL** (identify) |
| checkbox check mark | `#F2F2FA` / `#1B1B1F` | 15.42:1 | PASS |
| Danger action hairline vs fill | `#FF6B6B` / `#1B1B1F` | 6.19:1 | PASS |
| starvation-banner fill vs card | `#311F22` / `#151518` | 1.17:1 | decorative (the frame tint is not the identifying cue) |
| starvation-banner stroke vs its fill | `#FF6B6B` / `#311F22` | 5.61:1 | PASS (state; the word carries it, the hairline reinforces) |
| notice warn-tone stroke vs its fill (contract) | `#FDCB6E` / `#312B22` | PASS-scale (measured in `contrast-results.md`) | PASS (contract variant) |
| route arc vs map surface | `#6C5CE7` / `#111118` | 3.87:1 | PASS |
| map land vs map surface | `#4A4A72` / `#111118` | 2.25:1 | decorative |
| map graticule vs map surface | `#222230` / `#111118` | 1.20:1 | decorative |
| hover fill delta | `#222227` / `#1B1B1F` | 1.08:1 | advisory (hover is never required to operate) |
| hover stroke delta | `#333339` / `#25252A` | 1.22:1 | advisory |

**Result: 8 of 20 claimed non-text pairings pass 3:1; 12 fail.** The failures
are one systemic defect — *the dark-on-dark control boundary is invisible* — plus
the focus indication that inherits from it. The delta's notice frame passes: its
danger stroke measures 5.61:1 against its own fill, and the tinted fill itself
(1.17:1 vs the card) is not claimed as an identifying cue.

### 3.4 Fixes, each re-measured

| defect | fix | measured after |
|---|---|---|
| control boundary invisible (1.08–1.29:1) | new `border_control #65656A` on every control at rest; fields keep the `bg_0` fill as a second (weak) cue | 3.14:1 on `bg_2`, 3.30 on `bg_1`, 3.39 on `bg_0` |
| focused text field 1.32:1 | `selection.stroke` becomes opaque `accent_text` (egui paints a focused `TextEdit`'s frame with `selection.stroke`, `theme/default_style.rs:133-140`); no 28 % alpha as ink | 8.11:1 on `bg_0` |
| selected list row 1.35:1 | the same `selection.stroke` change (egui uses it as the selected label's ink over the row fill, `style.rs:360-369`) — one token fixes both | `accent_text` on the selection fill = **6.70:1 PASS**; `text_1` = 14.60:1 if a brighter ink is preferred |
| keyboard focus on stock controls | focus maps to egui's `Active` visuals (`widget_style/mod.rs:214-221`), i.e. a bright accent fill identical to pressed; W2 changes `widgets.active` to the dark palette and adds a 2 px `accent_text` ring painted outside `response.rect` for every focused control | ring 7.89:1 on `bg_1`, 7.51 on `bg_2`, 8.11 on `bg_0`; inset ring on `accent_deep` is 2.55:1, so the ring must be outside |
| disabled action indistinguishable from ready (2.77:1) | disabled = `bg_3` fill + `text_3` ink (shipped, 5.14:1 ink) **plus** the disabled interaction block; do not rely on fill contrast alone | ink 5.14:1 PASS; the state is also carried by the label and the block |
| selected row fill 1.12:1 | selection is carried by ink (6.70:1) + a 1 px `accent_text` left edge; the fill tint is redundant | — |
| rail edge CONNECTED 1.12:1 | the edge is redundant (the word says CONNECTED); either remove the edge in that state or use `border_2`; do not claim it as a state indicator | — |
| checkbox box invisible (1.15/1.29:1) | the box stroke becomes `border_control` (3.14:1 on the sheet plane); the check mark already passes at 15.42:1 | 3.14:1 |
| the one action's hover ink AAA | accepted: 4.86:1 passes AA at 15 px; the rest fill (6.19:1) is the primary state and hover is transient | AA PASS |
| notice frame (master `76aba5d`): danger-soft fill, danger hairline, danger ink, `R_INLINE` 4 px | keep the shipped treatment; it measures 5.61:1 ink and 5.61:1 stroke vs its fill, and the state is carried by the sentence, not the tint | AA PASS; do not add a fourth ink colour |

The `#65656A` value is a searched minimum with a 0.10 margin over 3.0 so integer
rounding cannot drop it below the threshold; the search is in `contrast.py`
(`find_border_control`). It is a control *boundary*, not an ink: it never
carries text.

### 3.5 What is not claimed

- AAA is not claimed for any small secondary or coloured ink. AAA holds for
  `text_1` on every plane (15.42–17.66:1) and for the map/checkmark/big-word
  pairings marked PASS.
- "AA" is claimed only for pairings in §3.2, which are the pairings that exist
  in the shipped code (41 rows after the delta; the new notice and caption rows
  are re-measured from the master tree). A new colour pairing needs its own
  measurement before it ships.
- Hover, pressed and selected *fill steps* do not make 1.4.11 claims; the
  identifying information is the boundary, the ink and the word. Focus does make
  a claim and gets the ring.
- No claim is made about contrast at 200 % scaling: ratios are scale-invariant,
  but rasterisation/hinting is not measured here (§12).

---

## 4. Type system

### 4.1 Roles

Two weights exist and only two are used: **Inter Regular (400)** and **Inter
SemiBold (600)**; data is **JetBrains Mono Regular (400)**. `Inter-Bold.ttf` is
bundled but never registered (`theme.rs:17-93`) — the "weight is binary" rule is
a decision, not a bundle limit; adding a third weight needs a decision and a
role. All tokens are px at 1.0 pixels-per-point; egui lays out in points.

| role | token | px | face | documented line height | **measured row height** | delta | use |
|---|---|---|---|---|---|---|---|
| caption | `CAPTION` | 11 | Inter Reg | 16 | **14.00** | −2.00 | ledger keys, units, footnotes, hints, disabled ink, sheet captions |
| label | `LABEL` | 12 | Inter SemiBold | 16 | **15.00** | −1.00 | field/section labels, button labels, checkbox labels, sheet section heads |
| body | `BODY` | 13 | Inter Reg | 19 | **16.00** | −3.00 | prose, help text, rail sub-line, ledger prose values |
| emphasis | `EMPHASIS` | 15 | Inter SemiBold | 19 | **19.00** | 0.00 | the one action's label (uppercase) |
| value | `VALUE` | 17 | JetBrains Mono | 24 | **22.00** | −2.00 | addresses, ports, counters, node ids (mono data only) |
| title | `TITLE` | 20 | Inter SemiBold | 26 | **24.00** | −2.00 | sheet titles |
| state | `STATE` | 30 | Inter SemiBold | 34 | **36.00** | +2.00 | the uppercase state word in the rail |
| metric | `METRIC` | 30 | JetBrains Mono | 34 | **40.00** | +6.00 | the live round-trip readout in the rail |

Rules:

1. **No ad-hoc sizes.** Every text uses one of these eight roles. The only
   shipped exceptions are measured defects: the `STATE * 0.82` multiplier and
   the action label's `9 px` shrink floor (§4.5). Both are removed in W2, not
   codified.
2. **Mono is for data, never costume.** `VALUE`/`METRIC` carry addresses, ports,
   counters and the RTT number. Prose, labels and buttons are Inter even when
   they contain digits (`app.rs:2674` prose uses BODY, not mono).
3. **Sentence case everywhere except two strings**: the uppercase state word
   (canon, the instrument's readout) and the action label (canon: the button).
   Ledger keys are caption sentence case (`Relay`, `Game server`), never
   all-caps eyebrows.
4. **Weight is binary**; emphasis is carried by size, colour and alignment.
   Do not fake a weight with faux-bold.
5. **The documented line height is not what egui uses.** The token comments
   state a size/line-height pair, but egui derives the row from face metrics. The
   measured column is the real row height; layout budgets must use it, not the
   comment. The comment is corrected in W2 to the measured value (see the delta
   column), or the pairing is restated as `size [row]`.
6. **Uppercase and height.** `EMPHASIS` at 15 px gives a 19 px row; two lines are
   38 px. `STATE` at 30 px gives a 36 px row and `METRIC` 30 px a 40 px row.

### 4.2 The rail's measured geometry

- The rail's hero row is allocated `hero_h = 34.0` (`app.rs:2153`) but the
  shipped faces lay out a 40 px row for the metric (`METRIC` 30 JetBrains Mono)
  and 36 px for the state word. **The 34 px band cannot contain its contents**;
  the row grows to 40 px. W2 sets `HERO_H = 40.0`.
- The `interface.md` sentence "the state rail is a fixed 34 px band" is
  therefore 6 px short of what the shipped fonts lay out and must be corrected by
  `interface-designer` (this document owns the number, not that file).
- Baseline drift: if both roles render at their token size, the measured drift is
  **0.0 px** at 100 % DPI — the row-height difference (+4 px) is exactly twice
  the ascent difference (+2 px), so `Align::Center` cancels it. This *repairs*
  the historical "≤1 px drift" concern at the token sizes.
- The shipped narrow multiplier breaks it: `STATE * 0.82` = 24.6 px gives a
  30 px Inter row and a measured **2.0 px** baseline drift against the 30 px
  metric (§4.5). At the mandated `STATE` 30 px the 288 px rail column overflows
  for ES/PT-BR `NO RETURN PATH` (309.0 / 328.4 px) and RU `IDLE` (326.8 px) in
  the current tree; RU `NO RETURN PATH` is already shortened to 222.9 px by the
  coordinated copy fix (`delta-copy.md` §5). The metric needs its own row
  regardless; the remedy is decided in §4.4 item 4 / §6.9 gap 11.

### 4.3 Numerals and units

- JetBrains Mono is monospaced, so digits are tabular by construction; no
  `VALUE` number shifts the column when it changes. Counters are grouped with
  `group_digits` (`widgets.rs:473-484`) and the metric is padded to three digits
  (`{reading:>3.0} ms`, `app.rs:2139`) so the rail does not jump between 9 and
  124 ms.
- A unit is part of the value string (`ms`, `s`), never a separate styled
  element, and never a fabricated precision: seconds ages are integer, RTT is
  integer ms, sample counts are exact `n=`.
- `—`/`--`: an absent metric prints the literal `--` (`app.rs:2141`), never a
  zero and never a blank. A stale measurement prints its age.

### 4.4 Localisation budgets (measured, not estimated)

Nine locales, **mobile-only geometry**: the viewport is pinned to 320 px wide,
one `narrow` column, height user-resizable 420–1400 px (initial 760)
(`main.rs:109-115`; owner decision 2026-11-20, brief §13). At that width the padded
content is **288 px** (320 − 2×S4), the card inner width **264 px** (288 − 2×S3),
the notice text column **248 px** (264 − 2×S2), the sheet body **264 px**, and
the action label column **264 px** — `action_button` reserves 24 px of the 288 px
content (`inner_w = available_width − 24`, `ledger.rs:81`). The earlier W1 draft's
296 px action budget was arithmetically wrong (it forgot the S4 panel margins);
264 px is the corrected number and the one the delta's measurements use.

**Rule: long copy wraps; it is never shrunk.** The only shipped shrink path is
the action label's 9 px floor (`ledger.rs:81-88`), deleted in W2. The adopted
two-line action is `ACTION_H_WRAPPED` 56 (two 19 px `EMPHASIS` rows = 38 + 2×S2 =
54, rounded to the next 8-px step).

Measured action labels at 264 px (greedy word wrap, `EMPHASIS` 15 Inter SemiBold,
all nine locales):

| label | locales at 1 line | locales at 2 lines | locales at 3 lines | worst width |
|---|---|---|---|---|
| `action.stop_optimizing` | all 9 | — | — | 225.7 px (ru) |
| `action.optimize_route` | all 9 | — | — | 220.7 px (ru) |
| `action.restart_admin_optimize` | ja, zh-Hans | en, de, es, fr, ko, pt-BR, ru | none | 383.0 px (de); RU was 518.6 px / 3 lines at HEAD, now 356.9 px / 2 lines (landed copy) |

Every label now fits the two-line slot: the largest is DE 383.0 px (2 lines), and
the previously-broken RU string was 518.6 px / 3 lines at HEAD and is 356.9 px /
2 lines with `ux-lead`'s landed `ru.toml` copy (`Перезапустить от имени
администратора`, dropping the "to optimize" purpose — flagged to `ux-lead`/spec
to carry elsewhere). `ACTION_H_WRAPPED` 56 is therefore sufficient; the 73 px
three-line option remains priced out (compact worst 256 chrome → 164 scroll,
below the 180 floor). The type is never reduced, and the 9 px shrink floor is
deleted either way.

**New copy from master `76aba5d` measured at the mobile-only widths**
(`delta-copy.py`; lines = greedy wrap at the render role; worst of nine locales):

| copy | site / role | column | EN lines | worst lines |
|---|---|---|---|---|
| `route.relay_not_returning` (banner) | card banner / BODY SemiBold 13 | 248 px | 3 | **4** (de) |
| `route.injection_failed_warning` (banner) | card banner / BODY SemiBold 13 | 248 px | 3 | **4** (de) |
| `settings.starvation_policy` | settings sheet / CAPTION 11 | 264 px | 5 | **6** (de) |
| `route.location_hosting_hint` | card location ladder / CAPTION 11 | 264 px | 2 | **3** (fr) |
| `route.location_*` captions and details | card location ladder / CAPTION 11 | 264 px | 1–2 | 2 (hosting hint 3) |
| `route.location_geofeed_note` | card / CAPTION 11 | 264 px | 2 | 2 |
| `route.trace_route` + `route.trace_nearest` | card trace caption / CAPTION 11 | 264 px | 1 | 1 |
| `route.route_trace_hops` (with sample values) | ledger value / JBMono 17 | 264 px | 2 | **3** (ru) |
| `route.server_leg_relay_measured` | ledger value / JBMono 17 | 264 px | 2 | 2 (ru) |
| `route.server_leg_relay_measured_unvalidated` | ledger value / JBMono 17 | 264 px | 2 | **3** (ru) |
| `route.expected_path` | ledger value / JBMono 17 | 264 px | 1 | 2 (ru) |
| `route.server_leg_app_measured` | ledger value / JBMono 17 | 264 px | 1 | 1 |
| update-sheet hard-coded UAC line | sheet / BODY 13 | 264 px | 2 | 2 |

Consequences, stated as rules rather than tweaks:

1. **The notice text is a 3–4 line banner at 320 px, not a one-or-two-line line.**
   The component contract (§6.10) allows a measured maximum of 4 text lines
   (80 px frame + S2 spacing, inside the scrolling card) and never shrinks the
   type. `ux-lead` may shorten the copy toward the 2-line target (≤496 px of text
   at 13 px semibold), which would bring the frame back to 48 px.
2. **The setting policy paragraph is a 5–6 line caption**, which is fine for a
   settings sheet: the sheet body scrolls; no control is displaced.
3. **The ledger/value rows wrap to 2–3 lines in RU** because the mono value is
   17 px in a 264 px column. The row grows in height; the value is never shrunk
   and no truncation-without-hover is allowed for a value the player needs.
4. **The state word at the mandated 30 px overflows the 288 px rail in three
   current-tree cases; the metric must move to its own row.** Dropping the
   shipped `0.82` multiplier renders the word at `STATE` 30 px. Measured in the
   current tree (`delta-copy.md` §5): **PT-BR `NO RETURN PATH` 328.4 px**
   (+40.4 over the 288 px column), **RU `IDLE` 326.8 px** (+38.8), **ES
   `NO RETURN PATH` 309.0 px** (+21.0); DE `OPTIMIZING` 275.1 px fits 288 with
   12.9 px but is over the proposed 276 px acceptance cap. RU `NO RETURN PATH`
   was 342.8 px at HEAD and is **already shortened to 222.9 px** by `ux-lead`'s
   landed `ru.toml` copy, so that case is fixed in the worktree. The metric
   cannot share the row either: word + S2 8 + metric 90 ≥ 366 px even at the
   shipped 24.6 px. **Acceptance proposed to the spec owner: every locale's
   uppercased state word at `STATE` 30 px measures ≤276 px in its installed
   chain (12 px / 4 % safety under 288, ±2 px measurement tolerance).**
   **Recommended remedy: shorten the remaining over-budget words** — ES
   `SIN RETORNO` 176.8 px and PT-BR `SEM RETORNO` 178.7 px (proposed strings,
   pending the copy owner), RU `IDLE` contingency `НЕ АКТИВНО` 193.9 px (only if
   A2 keeps `idle`; `НЕ ПОДКЛЮЧЕНО` 268.0 px for the A2 `not_connected` row), and
   optionally DE `OPTIMIERUNG` 167.2 px for slack — keeping the 30 px type role,
   the 40 px hero band and the spec's 239/181 compact budget. Fallbacks: a
   measured 24/25 px effective word size (the type role would no longer be
   30 px), or a two-line hero, which prices the compact worst at 275 chrome →
   145 scroll, below the 180 floor (it needs an owner-approved floor change).
   Nothing else: the word must never be occluded. The RU width is a
   Cyrillic-fallback symptom (Ubuntu-Light, not Inter SemiBold; ES/PT accents
   also fall back) — re-subsetting the bundled fonts (Q19) is the long-term fix,
   copy shortening is the v1 fix.
5. **CJK/Korean labels are short** and never bind the budget; the RU (Ubuntu
   fallback) and Latin translations are the binding cases.
6. **The update-check disclosure is documentation-only.** Master `bbc96cc` added
   the user-initiated GitHub release-check disclosure to `docs/privacy.md`
   (§Update check) and `docs/data-dictionary.md`; it ships **no new in-window
   copy**. The update sheet's existing strings are measured in `delta-copy.md`
   §3 and wrap under the same rule (the hard-coded UAC line is 2 lines at
   264 px). If W2 adds a disclosure sentence to the sheet, it follows §6.10's
   measured-max wrap rule and is never shrunk.

### 4.5 Shipped type-scale discrepancies (defects, not variants)

| # | shipped | measured effect | W2 |
|---|---|---|---|
| 1 | `STATE * 0.82` (`app.rs:2161`) always taken (`narrow` is always true at 320 px) | state word renders at 24.6 px, not 30; 2.0 px baseline drift vs the metric; at the mandated `STATE` 30 px the current tree overflows for ES/PT-BR `NO RETURN PATH` (**309.0 / 328.4 px**) and RU `IDLE` (**326.8 px**; RU `NO RETURN PATH` already shortened 342.8→222.9 px), and the metric must move to its own row | drop the multiplier; put the metric on its own row; **shorten the remaining over-budget words to ≤276 px at 30 px** (recommended, §4.4 item 4) — fallback 24/25 px effective size or a two-line hero priced against the floor |
| 2 | action label shrink to a 9 px floor (`ledger.rs:81-88`) | 4/15 locale/label pairs render at 11.6–12.6 px | §4.4 |
| 3 | `hero_h = 34.0` inline (`app.rs:2153`) | 40 px content row in a 34 px allocation | `HERO_H = 40.0` |
| 4 | `VALUE` (17 px) has no call site (`theme.rs:199` only) | the mono role in use is whatever `TextStyle::Monospace` yields plus the 30 px metric; 17 px mono is dead as a named role | either wire it for ledger data values or collapse `VALUE` into a named use; do not leave a role with no call site |

---

## 5. Spacing, radius, elevation, density

### 5.1 Spacing

One scale: **S1 4 / S2 8 / S3 12 / S4 16 / S5 24**. There is no 2, 6, 10, 20 or
32 px spacing token, and no inline `add_space(n)` is allowed (the shipped code
already uses only tokens for spacing: `token-usage.md`).

| token | allowed uses |
|---|---|
| S1 4 | icon→label; label→field in the narrow stacked row; checkmark→label |
| S2 8 | related controls; list row gap; rail word→sub-line; default `item_spacing` |
| S3 12 | card/sheet inner gutter; button horizontal padding |
| S4 16 | window/panel margin; sheet inner margin; region gutter; control→label in form rows |
| S5 24 | separation between unrelated groups inside a sheet (settings: Privacy / Routing / Relays / Maintenance / About) |

Content column: the config region caps its content at `min(available, 560)` and
centers it (`app.rs:4254-4256`); at the mobile-only 320 px viewport the available
width is 288, so the cap never binds and the single column is the whole content
band. Cards use a single left gutter — the shipped left edges are not all aligned
(the rail is full-bleed, the card starts at S4, the config card starts at
S4 + S3), and the W2 shell must declare one gutter per band (layout owner:
`interface-designer`).

### 5.2 Radius

| token | value | role | rule |
|---|---|---|---|
| `R_CONTROL` | 8 | field, combo, tile | one input radius; `R_CARD` equals it *by decision*: a card and a field share the surface radius, and hierarchy is carried by plane + hairline, not by radius |
| `R_CARD` | 8 | the one card style | exactly one card style in the window |
| `R_BUTTON` | 12 | the action slot | the one bright action is the only 12 px surface |
| `R_SHEET` | 16 | in-window sheets | the only 16 px surface |
| `R_INLINE` | 4 | inline marks: the starvation-notice frame (master `76aba5d`) | keep — it is live now; the notice frame is the only 4 px surface. If a disclosure marker returns it is painter-drawn (§8.5), not a rounded chip |

Rules: **one radius per surface role**; no radius literal at a call site (the
shipped map uses a literal `6`, `globe.rs:74` — W2 uses `R_CONTROL`). Radius is
not a hierarchy device: surface plane, hairline and ink weight are.

### 5.3 Elevation: planes, not shadows

Shadows are off and stay off: `window_shadow = Shadow::NONE`,
`popup_shadow = Shadow::NONE` (`theme.rs:141-142`; critique req. 14 is fixed in
the shipped tree). Elevation is a *plane*:

| level | token | who lives here |
|---|---|---|
| sunken | `bg_0` | text-field wells, the map, the sheet body (a modal is a well over the scrim), plots |
| base | `bg_1` | the window plane, the action bar, the footer, the scroll region |
| raised | `bg_2` | the state rail, the one card, the config card, popups (egui `window_fill`) |
| transient | `bg_3` / `bg_4` | rest control fill / hover fill — never a container for other content |
| well + scrim | `bg_0` over `scrim` | modal sheets |

Rules:

1. Adjacent surfaces differ by exactly one plane step, except the modal well
   (sheet `bg_0` over the scrim) — modals are allowed to be two steps because the
   scrim separates them.
2. A raised surface never sits inside a transient plane.
3. No new depth mechanism. No shadow, no gradient, no border-radius gradient.
4. The one card is the only `bg_2` *content* surface inside the scroll region;
   the rail is also `bg_2` but it is chrome, separated by a full-bleed edge.

### 5.4 Density and component metrics

One density: a compact **mobile-only** instrument (one column, 320 px wide). No
"comfortable/compact" toggle.

| element | value | source / rule |
|---|---|---|
| control height | `CONTROL_H` 40 | fields, combos, checkbox row minimum (egui `interact_size.y`) |
| action height | `ACTION_H` 48; `ACTION_H_WRAPPED` 56 with the two-line label | the one action (≥44 preferred target); the wrapped height is derived in §1.4 |
| ledger row | `ROW_H` 24 | one label/value row |
| list row (popup option) | 40 | egui control height; do not shrink |
| dense row (proxy list) | **24 minimum, 40 recommended** | shipped allocation is 16 px (`app.rs:3808`) while a frameless egui button has a 40 px minimum (theme `interact_size.y = 40`), so the shipped row cannot contain its own delete control; W2 rows are 40 px (label + address stacked) with a 24×24 delete target |
| rail hero | `HERO_H` 40 | measured, §4.2 |
| sparkline | `PLOT_H` 64 | hand-painted, no axes |
| route map | `MAP_H` 132, `MAP_W_MAX` 430 | shipped inline values; W2 tokens |
| sheet max width | `(content_w − 24 − 2·S4).min(420 − 2·S4)` | `widgets.rs:424-432`; at the mobile-only 320 px viewport that is **264 px** (the 420 px cap never binds; there is no 360/440 variant in this design) |
| sheet body max height | `max_h − 190` floored at 160 | `app.rs:3327`; the 190 px reserve is a magic number — W2 computes it from title + close + margins instead of a constant |
| window | **mobile-only (owner decision 2026-11-20):** width **pinned at 320 px**, one `narrow` column; height user-resizable **420–1400**, initial **760** | `main.rs:109-115`, `window.rs:35-39`; brief §13. The W1 360×420 / 440×880 / 1120×1400 plan and every two-column layout are superseded |

**Breakpoints: exactly one width layout.** `narrow = width < 380` and
`short = height < 560` (`app.rs:3998-3999`). With `main.rs` pinning the width to
320 px, `narrow` is always true: there is **one column and one width layout**.
The `LEDGER_LABEL_W` column and `field_row`'s side-by-side form are unreachable
and are retired in W2 with their token; `interface.md`'s 380–619 px and ≥620 px
two-column plans are superseded by the owner's mobile-only decision. The `short`
height branch remains real and useful: below 560 px the route card folds into the
scroll region.

**Fixed chrome and the scroll region at 320 px — W2 uses the spec's §1.3 / C-17
table** (measured rows, §4.1 tokens; `delta-copy.py` §6). The rail has the metric
on its own row (the state word and the metric cannot share it, §4.4 item 4):

| band | compact / short mode | normal |
|---|---|---|
| rail | **108** = 2×S3 24 + `HERO_H` 40 + S1 4 + metric row 40 + S1 4 | **128** = + S1 4 + BODY 16 (sub-line) |
| action bar, wrapped label | 72 = `ACTION_H_WRAPPED` 56 + 2×S2 16 | 72 |
| action bar, single-line label | 64 = `ACTION_H` 48 + 2×S2 16 | 64 |
| disabled-reason line (when shown) | 18 = CAPTION row 14 + S1 4 | 18 |
| footer | **41** = hairline 1 + S2 8 + `HIT_MIN` 24 + S2 8 | **57** = 1 + 8 + 40 + 8 |

| case | chrome | scroll region at 420 / 760 / 1400 |
|---|---|---|
| **320×420 compact worst** (wrapped action + reason line) | **239** | **181** |
| 320×420 compact best (single-line action, no reason line) | 213 | 207 |
| **320×760 / 1400 normal worst** | **275** | **485 / 1125** |
| 320×760 / 1400 normal best | 249 | 511 / 1151 |

The compact worst clears the ≥180 px scrolling-region floor by **1 px — not 27**.
The earlier table in this document (213/207/547/1187) omitted the metric row, the
disabled-reason line and the compact footer; it is **superseded by the spec's
239/181 worst case (C-17)** and must not be used as W2's budget. There is no slack
to grow a band without re-pricing (the open 73 px action-label option prices at
256 chrome → 164 scroll, below the floor). The starvation banner and the
location/trace lines live inside the scrolling card, so they add height there and
never push the action or footer off-screen; at 420 px the `short` branch has
already folded the card into the scroll region.

### 5.5 Pointer targets

- Minimum target: `HIT_MIN` 24×24 (WCAG 2.5.8 AA); preferred 44×44 for the
  primary action (`ACTION_H` 48 exceeds it).
- Shipped exceptions: the proxy-row delete `×` is inside a 16 px row; the row
  cannot contain the button's own 40 px minimum, so the target is effectively
  constrained by the row and overlaps the neighbouring address line. Fix with
  §5.4's 40 px row and a 24×24 target.
- The map marker hit rect is 14×14 (`globe.rs:137`) — below the 24 px floor; it
  is hover-only (tooltip), so it identifies but cannot act; still raise to 24×24
  in W2.

### 5.6 Scale-drift register (inline values that must become a token or a rule)

| location | inline value | disposition |
|---|---|---|
| `app.rs:2153` | `hero_h = 34.0` | → `HERO_H` 40 (measured) |
| `app.rs:4140-4144` | rail edge 3.0 px | → `RAIL_EDGE` |
| `app.rs:2281-2282` | map `min(available, 430)`, `132.0` | → `MAP_W_MAX`, `MAP_H` |
| `globe.rs:74` | map corner `6` | → `R_CONTROL` |
| `globe.rs:71-73,125,133` | five colour literals | → `bg_0`, `grid`, `map_land`, `accent`, `on_accent` |
| `app.rs:3808-3815` | proxy row 16 px, name width `-26.0`, min `80.0` | → 40 px row, `HIT_MIN` delete target |
| `app.rs:3803` | `item_spacing.y = 4.0` | → S1 |
| `ledger.rs:81-88` | label inset 24, min width 80, font floor 9 | rule §4.4; express the 24 px inset as `S5` and delete the 9 px floor |
| `ledger.rs:127-131` | stop mark 10×10 at 22 px from left | documented local constants (single-use painter detail, not tokens) |
| `widgets.rs:426-430` | sheet bounds 24 / 420 / 160 | `SHEET_GUTTER` 24? keep as rule constants documented here; `420` stays a max-width constant |
| `app.rs:3327,3914` | sheet body reserve `190.0` | → computed from title + close + margins |
| `app.rs:4254` | content column max 560 | documented rule; never binds at 320 px (available 288) |
| `widgets.rs:451-469` | sparkline pad 4/1, stroke 1.5, dot 2.5 | local painter constants, documented |
| `main.rs:109-115` | 320 / 760 / 420 / 1400 | **decided**: keep the shipped mobile-only geometry (owner, 2026-11-20) — no width change, no wide layouts |

A token is justified only where a second call site must match the value or the
value encodes a system rule (target sizes, hero band, focus ring). A one-off
painter detail stays a documented local constant.

---

## 6. Component state contract

### 6.1 Vocabulary and universal rules

Seven states, defined for every control class:

| state | meaning | universal mechanism |
|---|---|---|
| **default** | at rest, enabled | rest tokens |
| **hover** | pointer over the control (never required to operate) | fill one plane step; hairline one step; no 1.4.11 claim |
| **focus-visible** | reached by keyboard navigation | 2 px `accent_text` ring painted **outside** the control rect (`Painter::rect_stroke` + `StrokeKind::Outside`), fill unchanged by focus alone |
| **active** | pointer/keyboard press in progress | one distinct cue (fill or stroke) that differs from hover; same cue may not be used for focus |
| **disabled** | cannot act | interaction blocked, ink text_3, `bg_3` fill or 50 % group opacity; the label still names the outcome |
| **loading** | action in flight | static text that names the work + a determinate or non-animated indicator; the control cannot be re-triggered |
| **error** | the control's value/action failed | danger ink on the field/row + a word that says what happened; the failing control stays operable except where the failure makes it unsafe |

Rules that hold across every class:

1. **Focus is never a fill swap.** egui's default mapping is exactly that
   (`has_focus()` → `WidgetState::Active` → `widgets.active`, `widget_style/mod.rs:214-221`),
   so a focused button currently repaints as the bright action — both an
   accessibility defect (focus indistinguishable from pressed) and a breach of
   the one-bright-control canon. W2 neutralizes `widgets.active` for stock
   controls and paints the ring.
2. **Focus ring geometry:** 2 px `accent_text`, radius = the control's radius +
   2 px, offset 2 px outside the control rect. Measured 7.51–8.11:1 on every
   plane it can sit on; it must be painted outside (`accent_deep` inset is
   2.55:1).
3. **Every custom-painted interactive control calls
   `response.widget_info(...)`** so AccessKit gets a role/name. Raw
   `allocate_exact_size` + `Sense::click()` registers nothing (standard widgets
   do it themselves: `button.rs:382-392`, `label.rs:281`; the custom action does
   not — `ledger.rs:89-137`).
4. **No colour-only state.** Every coloured state also changes a word, a mark or
   a shape (§2 rule 4).
5. **Hover is optional; focus and disabled are mandatory.** A control that
   cannot show focus or disabled is not ready to ship.
6. **Loading text is static.** No spinner-only loading; motion is delegated to
   §7.

### 6.2 Primary action (custom-painted; `ledger.rs:71-137`, used at `app.rs:4211-4236`)

One slot, three labels, gate-driven: `OPTIMIZE MY ROUTE` /
`RESTART AS ADMINISTRATOR TO OPTIMIZE` / `STOP OPTIMIZING`
(`primary_action`, `ledger.rs:39-57`). Geometry: full content width ×
`ACTION_H` 48, `R_BUTTON` 12, `EMPHASIS` 15 SemiBold, uppercase. The slot has
**two variants** selected by state — `ActionKind::Accent` for the affirmative
action and `ActionKind::Danger` while boosting (`app.rs:4206-4210`) — and they
share geometry, focus ring, measurement method and loading/error rules; only the
fill/ink/hairline differ. **Accent variant:**

| state | appearance (tokens) | mechanism | shipped |
|---|---|---|---|
| default (ready) | fill `accent_deep`, 1 px stroke `accent_deep`, ink `on_accent` — 6.19:1 | `Painter::rect` + painted text (`ledger.rs:116-135`) | yes |
| hover | fill+stroke `accent`, ink `on_accent` — 4.86:1 (AA, thin) | `response.hovered()` | yes |
| focus-visible | fill unchanged; **2 px `accent_text` ring at +2 px outside** | `response.has_focus()` + ring helper | **no — invisible focus today** |
| active (pressed) | fill `accent_deep` (returns from hover), ink `on_accent`; pairwise distinct from the state it leaves (hover) | pointer-down on the same response | no distinct pressed cue (same as hover today) |
| disabled | fill `bg_3`, 1 px `border_1`, ink `text_3` — 5.14:1; the label still names the outcome | `ready=false` (`ledger.rs:97-98`); click handler ignores | yes |
| loading | disabled + static label that names the work — `STARTING…` / `RESTARTING AS ADMINISTRATOR…` from the spec copy deck (`action.starting`, `notice.restarting`) | `SessionCommandState.in_flight` (W2-1, spec section 2.1) is the fact; W2-2 renders the state and blocks re-trigger | **no in the shipped app; the fact is assigned to W2-1 and the state to W2-2** |
| error | the action returns to ready/disabled and the failure appears in the error line below (danger ink, §3.2) + the state word describes the route, not the intent | existing `status.*_error` lines | partial (error text exists; the action itself has no error state) |

**Danger variant** (`ActionKind::Danger`, the same slot while boosting; shipped at
`ledger.rs:105-113` and the stop square at `:124-132`). It is **quiet by
design**: the destructive affordance is the word `STOP OPTIMIZING`, a 1 px
`danger` hairline, danger ink and a painted 10×10 stop square — not a saturated
red fill. The anti-slop list rejects "a big red stop button beside gameplay" as
a mis-click waiting to happen; the shipped quiet treatment is kept and is the
contract. Measured: rest ink **6.19:1**, `danger_soft` hover **5.95:1** (both AA
at 15 px; AAA is not claimed).

| state | appearance (tokens) | mechanism | shipped |
|---|---|---|---|
| default (ready) | fill `bg_3`, 1 px stroke `danger`, ink `danger`, painted 10×10 stop square 22 px from the left edge — **6.19:1** | `Painter::rect` + painted square + text (`ledger.rs:105-113,124-132`) | yes |
| hover | fill `danger_soft` over the action-bar `bg_1` = `#2C1A1C`, 1 px stroke `danger`, ink `danger` — **5.95:1** | `response.hovered()` | yes |
| focus-visible | fill unchanged; **2 px `accent_text` ring at +2 px outside** (7.89:1 on `bg_1`) | `response.has_focus()` + ring helper | **no — invisible focus today, the same defect as the Accent path** |
| active (pressed) | fill returns to `bg_3` from `danger_soft`; ink/hairline stay `danger`; pairwise distinct from the state it leaves (hover) | pointer-down on the same response | no distinct pressed cue today |
| disabled | fill `bg_3`, 1 px `border_1`, ink `text_3` — 5.14:1; **no stop square** (it is painted only while `ready`) | `ready=false` (`ledger.rs:106-107,124`) | yes |
| loading | **deferred**: the spec copy deck defines loading labels only for start/elevation (`action.starting`, `notice.restarting`), not for stop, so no `STOPPING…` copy is invented; the danger slot keeps the shipped no-loading behaviour until a stop-loading label and fact mapping exist in the spec | — | **no — deferred with the missing copy named** |
| error | stop failure returns the slot to ready and the failure appears in the error line below (danger ink, §3.2) | error band | partial (no error state on the slot itself) |

Variant rules: only one variant is shown at a time (`boosting` selects Danger,
otherwise Accent); the focus ring is the system focus token `accent_text` for both
variants because focus is a system state, not the action's semantics; the stop
square is painted (no bundled face draws `■`, §8.3) and appears only while
`ready`; the label uses the adopted §4.4 two-line decision — it must fit the
264 px label column, and the slot uses `ACTION_H_WRAPPED` 56 when the label wraps
(two `EMPHASIS` rows = 38 px + 2 × S2 = 54, rounded to 56). One measured
exception is now resolved by the landed RU copy: the RU restart label was
3 lines at HEAD and is 2 lines (356.9 px) with `ux-lead`'s landed `ru.toml`
string, so the two-line slot covers every locale; the 73 px option is priced out
(§4.4) — the type is never shrunk.

### 6.3 Combo (`egui::ComboBox`; three sites: relay `app.rs:2921`, game
`app.rs:3007`, language `app.rs:3354`)

| state | appearance | mechanism | shipped |
|---|---|---|---|
| default | fill `bg_3`, 1 px `border_1`, ink `text_1`, `R_CONTROL` 8, `CONTROL_H` 40, selected text `LABEL` | `ComboBox::from_id_salt().selected_text().width().show_ui()` | yes (boundary 1.19:1 — fails §3.3; W2 `border_control`) |
| hover | fill `bg_4`, hairline `border_2` (1.08 / 1.22 advisory) | `widgets.hovered` | yes |
| focus-visible | **ring**; fill unchanged. Shipped: focus → `widgets.active` = `accent_deep` fill + `on_accent` ink + `accent` hairline (6.19:1 ink) — identical to pressed and breaks one-bright-control | `widget_state.has_focus()`; W2 `widgets.active` dark + ring helper | **no — wrong visual** |
| active/open | open popup: fill `bg_3` (`widgets.open`), popup `bg_2` + `border_1` + `R_CONTROL`, `menu_margin` S2 | `combo_box.rs:372-381` + `Frame::popup` | yes |
| disabled | not used in the shipped UI; rule = 50 % opacity, no pointer, label preserved | `ui.add_enabled(false, …)` → `Ui::disable()` multiplies opacity 0.5 (`ui.rs:523-527`, `style.rs:1670`) | n/a shipped, contract defined |
| loading | in place of the combo: `text_2` line `Discovering relays…` (`app.rs:2891-2894`, 8.97:1) through the `progress_indicator()` helper — the spinner only while the reduce-motion input is an explicit `On`; `Off`/`Unknown` show the static text (§7.3–7.4) | `DiscoveryState::InFlight` | yes (motion issue only) |
| error | in place of the combo: `warn` ink message + `retry` link (`app.rs:2896-2907`; warn = recoverable). Terminal discovery failure uses danger in the error line | `DiscoveryState::Done` + `discovery_error` | yes (recoverable case) |

Option rows inside a combo use §6.7 (list row). Popups are invisible to OS
accessibility tooling — a screenshot is the only verification (skill §4).

### 6.4 Text field (`egui::TextEdit`; `widgets.rs:333-351`, `app.rs:3140`, editor fields)

| state | appearance | mechanism | shipped |
|---|---|---|---|
| default | fill `bg_0` (well), 1 px `border_1`, ink `text_1`, hint `text_3`, `R_CONTROL`, `CONTROL_H` 40 | `Visuals.text_edit_bg_color`, `widgets.inactive.bg_stroke`, `hint_text_color` | yes — boundary 1.19:1 fails; W2 `border_control` |
| hover | same fill/boundary (pointer is an I-beam; no visual change required) | — | yes |
| focus-visible | frame stroke `selection.stroke`; **shipped `accent_line` 1 px = 1.32:1 FAIL**; W2 opaque `accent_text` (8.11:1), optional 2 px ring for consistency | `theme/default_style.rs:133-140` | **no — fails 1.4.11** |
| active | typing: focus state + caret (`selection.stroke` is also the caret colour) | same | yes |
| disabled | not used; rule = 50 % opacity, read-only variant paints without fill (`TextEdit::CLASS_READ_ONLY`) | `default_style.rs:168-176` | contract defined |
| loading | n/a | | |
| error | ink `danger` on the whole field (`invalid_text_field`, `widgets.rs:340-345`, 7.09:1) + the word `invalid` printed beside it (`widgets.rs:348`) + the specific reason in CAPTION danger | `text_color(ink)` | yes (not colour-only: the marker text exists) |

### 6.5 Checkbox / toggle (`egui::Checkbox`; settings Privacy's share-latency
stats, editor dynamic-server; `app.rs:3384`, `widgets.rs:107-110`)

| state | appearance | mechanism | shipped |
|---|---|---|---|
| default (unchecked) | box 14×14, fill `bg_3`, 1 px `border_1`, radius 4 (egui caps at ⌊0.3×14⌋), label `LABEL` SemiBold `text_1`; row target ≥ 40 px tall | `CheckboxStyle` (`default_style.rs` checkbox provider) | yes — box boundary 1.29:1 fails; W2 `border_control` |
| hover | box fill `bg_4`, hairline `border_2` (advisory) | `widgets.hovered` | yes |
| focus-visible | **ring around the whole row**; shipped focus → `widgets.active` = `accent_deep` box + `on_accent` tick = a bright box (breaks one-bright-control) | W2 `widgets.active` dark + ring | **no — wrong visual** |
| active (pressed) | box fill `bg_4` + 1 px `accent_text` hairline (W2) | `widgets.active.bg_stroke` | no distinct cue today |
| checked | shape: `text_1` tick on `bg_3` — 15.42:1 (state is the tick, not a fill change) | `CheckboxStyle.check_stroke` | yes |
| disabled | 50 % opacity, label preserved | `ui.add_enabled(false, …)` | contract defined (no shipped disabled checkbox) |
| loading | n/a | | |
| error | n/a at the control; a failed persist prints the error line below in danger ink (`app.rs:3869-3871`) | `config_error` | yes |

The re-test checkbox (`retry_after_fallback`) was removed on master `76aba5d` with
the warn-only starvation model; the remaining settings checkbox is
share-latency stats, so the control class is unchanged but narrower.

Toggle semantics: a checkbox is the only two-state control in the app; there is
no switch. If a switch is introduced it is a new component contract, not a
checkbox variant.

### 6.6 Sheet (`egui::Modal`; settings `app.rs:3317`, game editor `:3608`, target
picker `:3717`, update `:3907`)

| state | appearance | mechanism | shipped |
|---|---|---|---|
| default | scrim (`scrim` over the window), body `bg_0`, 1 px `border_1`, `R_SHEET` 16, S4 padding, `TITLE` 20 title, body in `ScrollArea` with max height, close button `LABEL` | `Modal::new(Id).backdrop_color(scrim).frame(sheet_frame())`; `sheet_frame` `widgets.rs:414-420`; `sheet_bounds` `:424-432` | yes |
| hover | only the sheet's own controls react (§6.2–6.5); the sheet surface is inert | — | yes |
| focus-visible | buttons/fields inside follow their own class; the close button gets the ring; **keyboard focus must enter the sheet and not tab into the window behind it** | egui modal layer blocks pointer input; focus traversal is `Memory`'s (`memory/mod.rs:595-596`); focus confinement not verified | **unverified — W2 must prove Tab stays inside and Esc closes** |
| active | drag on backdrop does not move the sheet (in-window modal) | `Modal::default_area` centered | yes |
| disabled | n/a | | |
| loading | update sheet: `Checking…`/`Installing…` text (`app.rs:3917-3922`, `3964-3972`) through the `progress_indicator()` helper — spinner only on an explicit `On`, otherwise static (§7.3); rule for W2 | `UpdateCheckState::Checking/Installing` | yes (motion issue) |
| error | update sheet shows the result line; **shipped result line is plain `text_1`, not danger** — W2: failure in danger ink + the error text names the cause; the sheet stays open | `update::update_status_line` | partial |
| dismissal | backdrop click / Esc / Close all close every sheet (`ModalResponse::should_close`) — **data-loss risk for the game editor and the update installer** | `modal.rs:151` | **defect: W2 sets a per-sheet close policy; the editor guards unsaved drafts** |

Sheet geometry rule: `max_w = (content_w − 24 − 2·S4).min(420 − 2·S4)`
(= **264 px** at the mobile-only 320 px viewport), `max_h = content_h − 24`
(floored 160); body `ScrollArea` height = `max_h − title − close − margins`
computed, not the shipped 190 px constant. The update check is user-initiated;
its privacy disclosure is documentation (`docs/privacy.md` §Update check), and
any in-window disclosure sentence follows §6.10's wrap rule at 264 px.

### 6.7 List row (three variants)

| state | appearance | mechanism | shipped |
|---|---|---|---|
| default | one row: leading label `BODY`/`LABEL` `text_1`, trailing caption/mono `text_3`; row height 40 (popup option) or 24 (ledger); `ROW_H` for ledger rows | `selectable_label`, `LayoutJob` (`widgets.rs:286-325`) | yes |
| hover | fill `bg_4`, ink `text_1` — 14.22:1 | `widgets.hovered` | yes |
| focus-visible | ring (W2); shipped focus → `widgets.active` bright fill | §6.1 | **no** |
| selected | shipped: fill `accent_soft` + ink `selection.stroke` (`accent_line`, 28 % alpha) painted over that fill = **1.35:1 AA fail**; W2: ink `accent_text` on the same fill = **6.70:1**, plus a 1 px `accent_text` left edge | `interact_selectable` (`style.rs:360-369`) | **no — must fix** |
| active | pressed = same as hover + `accent_text` hairline (W2) | `widgets.active` | partial |
| disabled | 50 % opacity | `ui.add_enabled` | contract defined |
| loading | caption `text_3` (`Loading processes…`, `widgets.rs:199-205`) | `PickerStatus::Loading` | yes |
| empty | caption `text_3` (empty table / no match, `widgets.rs:208-223`) | `PickerStatus::Ready` + rows empty | yes |
| error | no dedicated error row; an enumeration failure surfaces through the app error line (W2: row-level danger caption) | — | **missing** |

Dense list (proxy manager, `app.rs:3798-3904`): the shipped 16 px allocation
cannot host its 40 px-minimum delete button; the row becomes 40 px (label +
address stacked) with a 24×24 delete target; the address is `CAPTION` mono
`text_2` (5.88:1 on `bg_0`); an empty list prints a `CAPTION text_2` sentence.

### 6.8 Tile (not shipped — contract for the W2 game picker)

There is **no tile component at HEAD**: game selection is a combo, and the
historical plan's tile grid was never implemented (critique req. 8). The
contract is defined now so the W2 picker cannot invent a fourth row style.

| state | appearance |
|---|---|
| default | a button-like surface: `bg_3` fill, `border_control` hairline, `R_CONTROL`, label `LABEL` `text_1`, secondary line `CAPTION` `text_3`; min height 56, full width at the narrow layout (one-up), two-up only at ≥380 px |
| hover | `bg_4` + `border_2` (advisory), same as list-row hover |
| focus-visible | 2 px `accent_text` ring |
| active | `bg_4` + 1 px `accent_text` hairline |
| selected | `accent_soft` fill + 1 px `accent_text` edge + `text_1` label — the same selected treatment as a list row (6.70:1 inked) |
| disabled | 50 % opacity; a game that cannot act (e.g. capture unavailable) is disabled with the reason in a `CAPTION` `text_3` line, never a silent dead tile |
| loading | the tile grid opens only after the registry is loaded; while loading, the combo's loading state is what shows — no skeleton tiles |
| error | if the registry fails, the grid does not open; the failure shows as the combo's error state (§6.3) |

A tile may not carry a value the registry cannot produce (critique req. 9):
label from `profile.name`/`games()[i].display`, key from the profile key, no
fabricated city or latency.

### 6.9 Shipped state-contract gaps, ranked

| # | severity | gap | evidence |
|---|---|---|---|
| 1 | Blocker | the primary action's keyboard focus is **invisible** (painted from `response.hovered()` only; `Sense::click()` is focusable) — WCAG 2.4.7 | `ledger.rs:93`; egui `sense.rs:55-62` |
| 2 | Blocker | focus maps to the *pressed* visual on every stock control, so a focused button looks like the pressed/primary action and pressed vs focus is indistinguishable | `widget_style/mod.rs:214-221`, `theme.rs:171-174` |
| 3 | Blocker | selected list-row ink is 28 %-alpha accent painted over the 12 %-alpha accent fill → **1.35:1** | `style.rs:360-369`, `theme.rs:149-150` |
| 4 | Critical | control boundaries and the focused field are 1.08–1.32:1 (1.4.11) | §3.3 |
| 5 | Critical | the custom action has no `widget_info` → no accessible role/name (AccessKit), and its ring is absent | `ledger.rs:89-137` vs `button.rs:382-392` |
| 6 | Major | no loading state on the action in the shipped app: repeated clicks re-enter the start path; the fact is now assigned (`SessionCommandState.in_flight`, W2-1) and the state renders in W2-2; the danger/stop loading is deferred because the spec defines no stop label | `app.rs:4222-4236`; spec sections 2.1 and 3 |
| 7 | Major | sheets close on backdrop click while holding typed data (editor) or an install in progress (update) | `modal.rs:151`, `app.rs:3340`/`:3983` |
| 8 | Major | disabled action vs ready action fill contrast 2.77:1 (state is carried by ink + block, so not colour-only, but the fill cue is below 3:1) | §3.3 |
| 9 | Major | dense proxy row cannot contain its own control (16 px allocation, 40 px control minimum) | `app.rs:3808`, `theme.rs:189` |
| 10 | Minor | update failure line is plain text_1, not danger | `app.rs:3938` (line moved on master `76aba5d`) |
| 11 | Critical | at the mandated `STATE` 30 px the current tree overflows the 288 px rail for PT-BR `NO RETURN PATH` (328.4 px), RU `IDLE` (326.8 px) and ES `NO RETURN PATH` (309.0 px); DE `OPTIMIZING` 275.1 px fits 288 but is over the proposed 276 px acceptance cap; RU `NO RETURN PATH` is already shortened (342.8→222.9 px). The metric must move to its own row too (word + S2 + metric ≥366 px even at 24.6 px). Recommended remedy: shorten the remaining over-budget words to ≤276 px at 30 px; fallbacks priced in §4.4 item 4 | `app.rs:2153-2181`; `delta-copy.md` §5; fix under §4.4 item 4 (W2 shell owner) |
| 12 | Major | the starvation banner copy is 3 lines EN / 4 lines worst locale in its 248 px column, not the 1–2 line target; the shipped frame grows to 64–80 px in the card | `locales/en.toml:184-185`; `app.rs:2261-2277`; contract §6.10 |

### 6.10 Notice / warning line (new component contract; shipped by master `76aba5d`)

A display-only line (or paragraph) inside the scrolling card that tells the player
a live session fact needs attention. The shipped instance is the **relay
starvation warning** (`danger` tone, `app.rs:2261-2277`); the contract also
sanctions a `warn` tone for recoverable attention facts. It is not a control and
never carries an action of its own.

| state | appearance | mechanism | shipped |
|---|---|---|---|
| absent (default) | nothing is drawn and nothing is reserved | the driving fact is `None` | yes |
| danger tone | `danger_soft` fill (over `bg_2` card = `#311F22`), 1 px `danger` stroke, `R_INLINE` 4 radius, S2 inner margin, BODY SemiBold 13 in `danger` — **5.61:1 AA**; frame 2×S2 + 16 px per text line; S2 space after | `starvation_warning` from the watchdog; copy key by fault (`watchdog.rs:514-516`) | yes |
| warn tone (contract) | `warn_soft` fill (over card = `#312B22`), 1 px `warn` stroke, `R_INLINE` 4, BODY SemiBold 13 in `warn` — **9.30:1 AA** | for recoverable attention facts | not shipped; contract only |
| loading | n/a — the line is not a control | | |
| error | the notice *is* the error-class display for a live session fault (the error band remains for command failures) | | yes |

Rules:

1. **One or two lines is the target, and the measured maximum is four.** The
   two shipped strings are 3 lines EN and 4 lines (German) in the 248 px text
   column; the contract allows up to **4 text lines** (frame 64–80 px + S2) in
   the scrolling card and **never shrinks the type**. If a future string exceeds
   four lines, the copy is shortened (`ux-lead`) or the frame grows inside the
   scroll region — the type size does not move.
2. **The word carries the state, never the tint.** The sentence names the fault
   and the remedy; the red/warn tint is redundant (`§2 rule 4`). The frame tint
   itself makes no 1.4.11 claim; the ink and the sentence do.
3. **No fabricated remedy and no route claim.** Warn-only mode never switches the
   session direct on its own; the copy may only say what the client can do
   (warn) and what the player can do (stop optimizing / pick another relay).
4. **One notice at a time**, highest severity wins; it is placed above the map in
   the route card and clears when its driving fact clears (no dismissal control,
   no entrance animation, no motion — §7).
5. **Accessibility:** the text is a plain `ui.label` so it is in the AccessKit
   tree; the frame is decoration. The line is not focusable and has no keyboard
   interaction.

### 6.11 Location ladder and route-trace rows (new display rows; shipped by master `76aba5d`)

Two data surfaces answer "where is the game server" without inventing a location.
Both are display-only: they never feed the relay ranking.

**Location ladder** — five mutually exclusive captions in the route card
(`route_location_lines`, `app.rs:435-490`; rendered `app.rs:2289-2338`), CAPTION
11 `text_3`:

| state | copy (en) | source / rule |
|---|---|---|
| cloud region | `Server location: cloud provider region` + `{PROVIDER} · {region}` | the cloud feed's own statement; a provider region is a fact, a city is not |
| operator geofeed | `Server location: operator geofeed — {place}` + `({provider}, as-of {date})` + `Shown as-is from the operator's published geofeed.` + hover `Source: {source}` | the operator's own published feed; shown as-is, never interpolated |
| hosting network | `Server location: hosting network — unknown` + `AS{asn} · {name}` + hint `Free IP databases are unreliable for hosting ranges.` | no dot is drawn (the free database cannot place a hosting range reliably) |
| country estimate | `Server location: country-level estimate from IP` | the weakest rung; says it is an estimate |
| unknown | `Server location: unknown` + the same hint | explicit absence, never a blank or a fake dot |

Rules: one source per row (`server_place::place`); captions, not tooltips, for the
hint (the absence of a location is the fact the reader needs); the geofeed's
source is hover-only provenance, not a displayed claim; measured wrapping at
264 px is 1–3 lines per caption (worst: hosting hint 3 lines FR), so the card
grows and never truncates a caption. No city is claimed for a hop or a hosting
range the product cannot license.

**Route-trace rows** — the client's own TTL trace to the server and the relay's
trace, both display-only:

| element | copy (en) | role / place |
|---|---|---|
| card trace caption | `Route to server: {hops} hops · nearest responding hop {ms} ms` (+ provider name when known) | CAPTION 11 `text_3`, under the map; measured 1 line at 264 px |
| ledger label | `Route` | CAPTION 11 `text_3` |
| ledger value | `{hops} hops · last router RTT {ms} ms` / `{hops} hops · no router answered` | JBMono 17; measured 2 lines EN / 3 worst (RU) at 264 px |
| related values | `Relay→server {ms} ms · measured n={n} · {secs}s ago`, `Expected path ~{ms} ms (L + S)`, `Relay→server (app)` | JBMono 17 values; 1–3 lines worst (RU) |

Rules: the trace is a placement anchor, **not** the game's ping and **not** a
latency verdict; the copy never claims a city, a geolocation of a hop, or a
measurement the client did not take; `{hops}`/`{ms}` are exact integers with
units; the row appears only while a trace for this server exists; values wrap to
a measured maximum of 3 lines and the row grows — no truncation without hover and
no font-size reduction. Accessibility: labels are `ui.label`s and values are
`ui.monospace` labels, so both are announced; the trace is never colour-only.

---

## 7. Motion and reduced motion

### 7.1 Canon

**Silence unless the state changes.** No entrance animation, no idle motion, no
hover animation on non-controls, no parallax, no animated sheet. State changes
paint immediately. The only continuous motion the system accepts is a progress
indicator for work whose duration the user cannot see, and it must have a
non-moving alternative.

### 7.2 What actually moves today (measured in the locked framework)

The app contains **no animation code**: no `animate_*` call and no repaint loop;
the only `request_repaint()` in the crate is a one-off wake-up when the tray Quit
is pressed (`platform/windows.rs:114-118`), not animation. The GUI schedules
frames only with `request_repaint_after` at 2 Hz / 5 Hz / 1 Hz (`app.rs:4307-4321`).
The only continuous motion is what egui itself drives:

| # | motion | purpose | duration | easing | mechanism | reduced-motion |
|---|---|---|---|---|---|---|
| 1 | indeterminate spinner (relay discovery 15 s timeout, update check/install) | show that work is in flight | 1 revolution / s, continuous | linear (`start_angle = time·TAU`, arc breathes with `sin(time)`) | `egui::Spinner`, 40 px by default (`interact_size.y`); requests repaint (`spinner.rs:40-52`) | none |
| 2 | scrollbar show/hide fade | reveal that the region scrolls | 0.2 s | cubic-out | `ctx.animate_bool_responsive` (`context.rs:3719-3720`; `scroll_area.rs:826-832`) | none |
| 3 | scrollbar handle hover fade | show the handle on hover | 0.2 s | cubic-out | `animate_bool_responsive`, dormant opacity 0 → 0.6/1.0 (`scroll_area.rs:1607-1616`; default `ScrollStyle::floating()` with `dormant_handle_opacity: 0.0`) | none |
| 4 | smooth scroll-to | align a control into view | 0.1–0.3 s, 1000 px/s | framework | `Response::scroll_to_me` (`response.rs:925`, `style.rs:941-957`) — **not called anywhere in the app**, so not exercised | none (but if W2 uses it, it must be instant under reduce motion) |

`Style::animation_time` is 0.2 s by default and `theme.rs` does not change it
(`style.rs:1550`); no `ScrollStyle` override exists either. The spinner keeps
repainting the window while it is visible — on a laptop that is a battery cost
for a waiting state, another reason W2 should prefer static text plus a
determinate line for waits it can measure.

### 7.3 Motion table for W2 (what may be added)

| purpose | allowed motion | duration / easing | constraint |
|---|---|---|---|
| progress that has no measurable extent | static text or a spinner, through the single `progress_indicator()` helper; the spinner is shown **only when the helper's reduce-motion input is explicitly `On` (OS animations enabled)**; `Off` (reduced motion requested) and `Unknown` both render static text (spec W2-2 acceptance (i) + W2-4 acceptance (i)) | — | never across more than one region; never on the rail |
| state change highlight | **none** — the state word/colour changes instantly | — | "silence unless the state changes" |
| focus | **none** — the ring appears instantly | — | — |
| scroll | instant or the framework's 0.1–0.3 s, never a custom curve | framework | reduce-motion: instant |
| route visualisation | **none** — the map is static (the globe that could rotate no longer exists: `globe::draw_map`) | — | no ambient motion on the status instrument |

### 7.4 Reduced motion: mechanism, or no claim

**Claim today: none.** egui 0.37 has no reduced-motion signal — a search for
`reduce_motion`, `prefers_reduced`, `reduce-motion`, `animation_preference` and
`disable_animations` across `egui-0.37.0/src` returns **0 hits**. The app reads
no OS accessibility setting (no such call in `client-gui/src`). Therefore this
document makes **no reduced-motion claim**, and the shipped app must not
advertise one (the historical plan's claim was one of the critique's 16
required changes, req. 13 — it is fixed by not claiming it).

**W2 disposition after QA R5: assigned to two spec acceptance lines, not
deleted.** The revised spec (W2 revision, `gui-refresh-spec.md` §5) assigns the
mechanism in two closable places: **W2-2 acceptance (i)** — every wait renders
through one `progress_indicator()` helper that has a static-text alternative and
takes a reduce-motion input (no copy claim, no OS read in the GUI crate); **W2-4
acceptance (i)** — Windows reads
`SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)` through a hand-declared
`user32` `extern "system"` (no new dependency; the `single_instance.rs:157-179`
pattern) and parses it to `On / Off / Unknown`. **The SPI value means client-area
animations are enabled**, so `On` is the only state that may show the spinner;
`Off` (the user's reduced-motion setting) renders static text, and `Unknown` also
renders static text (conservative default, the same rule as macOS/Linux). The
parser is unit-tested against fixture values and the read is checked at runtime on
Windows; macOS/Linux return `Unknown` and are recorded as gaps. The app
still makes **no reduced-motion setting claim** — there is no toggle and no copy
promise; the only observable difference is the spinner becoming static text where
the OS signal is readable. §7.3's gate is therefore satisfiable and
conservative: only an explicit `On` shows a spinner. Consequence for the slice
order: W2-2 wires the seam with the input defaulting to `Unknown` → static text,
so W2-2 removes the spinner risk by default and W2-4 can only *enable* it where
Windows proves an explicit `On`. If the landed numbering
of those acceptance items differs, cite the acceptance text — one
`progress_indicator()` helper with a static-text alternative, and the Windows
`SystemParametersInfoW` read with its `On / Off / Unknown` parser — rather than
the letter.

**Interim exposure, named.** The **currently shipped** app (pre-W2-2) shows the
two continuous spinners — discovery, up to the 15 s timeout, and update
check/install — un-gated; that is the recorded **WCAG 2.2.2 (Pause, Stop, Hide,
Level A) exposure**, and it stays in the parity report as a named residual until
W2-2 lands. It is not a W2-4 residual: W2-2's `Unknown` default converts both
spinners to static text, and W2-4 only re-enables the spinner where Windows
reports an explicit `On`; macOS/Linux stay on static text because they return
`Unknown`.

The mechanism, per OS (a real signal, not a preference stored by the app):

| OS | source | read via | cost |
|---|---|---|---|
| Windows (shipped host) | `SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION = 0x1042)` — "client area animation" is the OS animation toggle | hand-declared `#[link(name = "user32")] extern "system"` in the repo's existing style (`single_instance.rs:157-179` declares `MessageBoxW` exactly this way) — **no new dependency** | one call at startup + on window focus/settings change |
| macOS | `NSWorkspace.shared.accessibilityDisplayShouldReduceMotion` (10.12+) | needs an Objective-C binding crate or a tiny `objc` extern — a dependency decision for the owner/architect | new dependency or hand-declared runtime binding |
| Linux | GNOME: `org.gnome.desktop.interface enable-animations`; KDE: `kreadconfig6 --group KDE --key AnimationDurationFactor`; GTK: `gtk-enable-animations` | read the setting (gsettings/portal) or the GTK setting file | a subprocess or a desktop-portal call |

Effect when the signal says "reduce": spinner replaced by static text (e.g.
`Discovering relays…` — already the adjacent label), scrollbars use
`ScrollStyle::solid()` (no fade; also fixes the hover-only affordance), any
scroll-to is instant. There is nothing else to gate, because §7.1 forbids it.

Status: **assigned (spec W2-2 acceptance (i) + W2-4 acceptance (i)); no
user-facing setting or copy claim.** Rule: `On` = animations enabled and the only
state that may show the spinner; `Off` and `Unknown` render static text. The
honest residual is the **currently shipped** un-gated spinner pair until W2-2
lands; macOS/Linux are gaps because they return `Unknown`.

### 7.5 Forbidden

Entrance animations, hover animation on anything that is not a control, layout
animation, easing on state colour transitions, animated sheet open/close,
parallax, ambient globe rotation, animated skeleton rows, "pulse" on errors,
hover wiggles. A new motion needs a purpose, a duration/easing and its
reduced-motion behaviour in the same change.

---

## 8. Bundled fonts and symbol policy

### 8.1 The chains that are actually installed

`theme.rs:17-93` starts from egui's default definitions and rebuilds three
families. The installed chains (measured, `fonts-and-glyphs.md` §1):

| family | chain |
|---|---|
| proportional | Inter-Regular → Noto Sans JP → KR → SC (locale-ordered) → Ubuntu-Light → egui-icons |
| monospace | JetBrains Mono Regular → the same CJK faces → Ubuntu-Light → egui-icons |
| semibold (`FontFamily::Name("semibold")`) | Inter-SemiBold → Inter-Regular → CJK Bold ×3 → CJK Regular ×3 → Ubuntu-Light → egui-icons |

Not in any chain: **Hack**, **NotoEmoji**, **emoji-icon-font** — eframe 0.37's
default features do not enable epaint's `monochrome_emoji_fonts`, and the theme
clones only the *proportional* builtin list (so the default monospace head
`Hack` is dropped). They are installed as font data but never referenced by a
family. eframe also installs a **system-font discovery provider**
(`system_fonts`), which can render a missing glyph from an OS font — device
dependent, so it is never a design dependency.

Face sizes: Inter 212 mapped codepoints (the docstring's "230-codepoint subset"
is approximate, −18), JetBrains Mono 210, CJK subsets 388–411, Ubuntu-Light
1194, egui-icons 5.

### 8.2 Catalog coverage (measured)

619 unique non-ASCII characters across the nine catalogs (the delta's new copy
added two); the chains cover **all 619 with zero tofu**. But Inter is the
*primary* face for only 23 of them. The rest come, as expected, from the Noto
CJK faces (Japanese/Korean/Chinese) and **Ubuntu-Light for Russian** (Inter has no
Cyrillic) — and, unexpectedly, from a few Latin characters: **de `ß`; es `í`;
fr `É ê î`; pt-BR `ê í` are not in the bundled Inter subset** and render in a
fallback face. Measured per-locale gaps are in `fonts-and-glyphs.md` §4.

**A measured asset defect found by the delta's chain measurement:** the bundled
**Inter subset has no lowercase `y` and no `{`**, so every English word containing
a `y` (`your`, `relay`, `play`, `copy`) draws that glyph from a fallback face
(Noto Sans JP / Ubuntu-Light) inside otherwise-Inter words — including the new
starvation banner. **JetBrains Mono lacks `o`, `q`, `!`, `"`, `#`, `}` and `~`**,
so mono values such as `~{prior} ms` mix faces too. This is a type-consistency
defect in the bundled assets, not a rendering failure, and the fix is a
re-subset (font asset change, OFL, W2/W3 decision in §11.5). The delta's width
measurements already account for the substituted faces (`delta-copy.md` §8).

**The language picker's own names are outside the catalog gate.** The nine
`native_name` strings (`i18n.rs:135-172`) are drawn in the picker
(`app.rs:3368`) but never pass through a catalog, so the repo's
`every_catalog_glyph_is_drawable_in_every_family` test cannot see them. Measured:
**ko `한국어` (국), zh-Hans `简体中文` (简 体) have no bundled face at all** —
they render only through system-font discovery; pt-BR `ê` falls back to a CJK
face. W2 fixes the gate by extending that test to the `native_name` strings (and
the subset itself is a font-asset change requiring a decision).

### 8.3 Symbol policy

Add a mark only if all three families can draw it from a bundled face; prefer a
painter-drawn shape whenever the bundled face is not Inter or the mark would
change weight/optics. Measured for the candidate marks
(`fonts-and-glyphs.md` §3):

| symbol | where | bundled chain | decision |
|---|---|---|---|
| `·` U+00B7 | age separator, `app.rs:2413` | Inter | type |
| `×` U+00D7 | proxy remove, `app.rs:3817` | Inter | type |
| `—` U+2014 | copy | Inter | type |
| `…` U+2026 | ellipsis | Inter | type |
| `•` U+2022 | (proposed) | Inter | type |
| `↑` `↓` U+2191/2193 | (proposed trend) | Inter | type |
| `→` U+2192 | rail route line, `app.rs:2103` | **Noto Sans JP only** | **paint** or add to the Inter subset — today the arrow is drawn by the CJK face beside Inter text |
| `✓` U+2713 | (proposed check) | Noto Sans JP only (the checkbox uses a painted tick) | paint |
| `←` U+2190, `↻` U+21BB, `⏵` U+23F5, `■` U+25A0, `▲` U+25B2, `▶` U+25B6, `▸` U+25B8, `▾` U+25BE, `◆` U+25C6, `✗` U+2717 | legacy/proposed marks | **absent from every chain** | paint |
| egui-icons PUA (`U+E618`…) | egui's five special emoji | egui-icons only (not text-weight) | paint or avoid in text |
| `⚡` U+26A1 | `single_instance.rs:79` message-box string | absent from the egui chain; drawn by the Win32 system font | out of scope (not egui), but do not reuse in the window |

The delta's new copy adds no new typed symbol, but it does type `→` in the new
`Relay→server ...` ledger values (`locales/en.toml:195-196`) — the same
Noto-Sans-JP fallback the rail already uses. W2 should paint the arrow once and
use it in both places (the rail sub-line and the ledger values) rather than type
it. The shipped stop mark already follows this rule: `■` is not typed, it is a
painted 10×10 square (`ledger.rs:124-132`) because the glyph is in no bundled
face.

### 8.4 Painter-drawn marks (spec)

| mark | geometry | stroke/fill | usage |
|---|---|---|---|
| route arrow | 10×5 px chevron/triangle pair or a 6 px line + 4 px head, at baseline centre | 1 px `text_3` | replaces `→` in the rail sub-line |
| disclosure marker (if a disclosure is reintroduced) | 5×5 px right-pointing triangle | filled `text_3`, rotated 90° when open | `▸`/`▾` are absent from every chain |
| stop mark | 10×10 px square, no radius | filled `danger` | the Danger action |
| check mark (if a checkbox needs a custom one) | 8×5 px polyline, 2 px stroke | `text_1` on the box | checkbox |
| map markers | 3.5 px core circle + 5.5 px ring, 1.5 px stroke | core `on_accent`, ring `accent` (active) / `border_1` (inactive); server marker `warn` | `globe.rs:129-144` (keep the shape; tokenize the colours) |
| status dot (if needed) | 6 px circle | state colour | never without a word beside it |

All painted marks are font-independent, so they cannot regress when a face is
swapped, subsetted or a locale picks a different CJK order.

### 8.5 Enforcement

- Extend `every_catalog_glyph_is_drawable_in_every_family` (`app.rs/tests.rs:743-791`)
  to include the `native_name` strings and a fixed symbol allowlist (the type-it
  rows above); the gate then fails on any new typed symbol that no chain draws.
- The gate must test the **installed** chains (it already asserts against the
  real `FontDefinitions`); keep it that way and do not add a scene-font exclusion.
- No new font family without a licence, a role and the cmap gate (owner
  decision). The bundled faces are Inter (OFL), JetBrains Mono (OFL) and Noto CJK
  (OFL) — zero-cost and redistributable.

---

## 9. Light mode decision

**Dark-first is canon. Light mode is out of scope for WF-081.**

- The shipped app pins dark: `Visuals::dark()` (`theme.rs:132`),
  `ctx.set_style_of(Theme::Dark, style)` (`:202`) and `ctx.set_theme(Theme::Dark)`
  (`:203`). egui 0.37 has `Theme { Dark, Light }` and
  `ThemePreference { Dark, Light, System }` (`memory/theme.rs:6-95`), and
  `eframe` can follow the system preference — the app deliberately does not.
- There are **no light tokens**. Every one of the 37 measured text pairings and
  18 non-text claims is a dark-plane pairing. Inverting the palette is
  arithmetically wrong, not merely risky: `accent_text #A29BFE` is 8.11:1 on
  `bg_0` but on a white plane it is 2.43:1; `text_1 #F2F2FA` would become a
  background. A light theme needs new planes, a re-derived ink ramp, new tints,
  re-measured focus/selection/border tokens, and a decision about the Windows 11
  dark/light title-bar. That is a redesign, not a token swap.
- Until a light theme exists and is measured, the app must not expose a
  theme/follow-system control and must not claim OS theme support. The About or
  Settings copy must not promise it.
- **Trigger to reopen:** an owner decision or a hard accessibility requirement.
  If it reopens, it is a separate accepted design task with its own contrast
  table; this document's values remain the dark canon.

---

## 10. egui 0.37 implementation mapping

Every rule above maps to a primitive that exists in the locked version. Citations
are `egui-0.37.0/src/...` in the cargo registry for this worktree; repo citations
are `file:line`.

| rule | egui 0.37 primitive | notes / constraint |
|---|---|---|
| one global theme | `Visuals` fields + `Context::set_style_of(Theme::Dark, style)` / `set_theme` (`context.rs:2594,2674`) | already `theme.rs:131-204` |
| no shadows | `Visuals::window_shadow`/`popup_shadow = Shadow::NONE` (`theme.rs:141-142`) | shipped |
| widget states | `WidgetVisuals` for `noninteractive/inactive/hovered/active/open`; selection via `Style::interact_selectable` (`style.rs:356-369`) | focus maps to `Active` (`widget_style/mod.rs:214-221`) — must be neutralized |
| stock-control pressed cue | `widgets.active.bg_stroke = accent_text` (W2) | stock fill must stay dark (§2 rule 2) |
| focus ring | `Response::has_focus()` (`response.rs:405`) + `Painter::rect_stroke(rect.expand(2.0), radius, Stroke::new(2.0, accent_text), StrokeKind::Outside)` (`painter.rs:406`, `stroke.rs:102`) | painter-API only; no patch kit |
| focused text field | `selection.stroke` is used for the focused `TextEdit` frame (`theme/default_style.rs:133-140`) | set it to opaque `accent_text` to fix 1.32:1 |
| custom action button | `Ui::allocate_exact_size` + `Sense::click`, painted with `Painter::rect`/`galley` (`painter.rs:397,503-536`) | add `response.widget_info(...)` for AccessKit |
| accesskit semantics | `Response::widget_info` (`response.rs:951`) + `WidgetInfo::labeled(Role::Button, enabled, label)` (`data/output.rs:694`) | accessibility is on by default in eframe (`accesskit` feature) |
| sheets | `egui::Modal::new(Id).backdrop_color(scrim).frame(Frame…)` (`containers/modal.rs:16-95`), `ModalResponse::should_close` (`:151`) | no built-in animation; per-sheet close policy is app code |
| sheet body scroll | `ScrollArea::vertical().max_height(…)` + `ScrollBarVisibility::AlwaysVisible` (`containers/scroll_area.rs:180-210`) | scrollbar is floating by default; see motion |
| shell bands | `Panel::top` / `Panel::bottom` / `CentralPanel` with `.frame(...)`, all taking `&mut Ui` (`containers/panel.rs:207,268,425`) | shipped shell `app.rs:4128-4321` |
| per-side rule | *no per-side stroke*: `Frame::stroke` draws all four sides; paint an edge with `Painter::vline` (`painter.rs:337`) | rail edge already painted, `app.rs:4140-4144` |
| popup chrome | `Frame::popup` uses `window_fill`, `window_stroke`, `menu_corner_radius`, `popup_shadow` (`containers/frame.rs:214-224`); combo popup styling is global (`combo_box.rs:372-381`) | a per-widget "custom combo" claim is a no-op |
| disabled | `Ui::add_enabled(false, …)` disables interaction and multiplies painter opacity by `disabled_alpha` (0.5; `ui.rs:523-527`, `style.rs:1670`) | measure the composite, not the token (§3.2) |
| checkbox | `CheckboxStyle { checkbox_size = icon_width, check_stroke, checkbox_frame }` from `theme/default_style.rs`; `icon_width` 14, corner capped at ⌊0.3·14⌋ | box is 14 px; the row target is ≥ `interact_size.y` 40 |
| text field colours | `Visuals.text_edit_bg_color` falls back to `extreme_bg_color` (`style.rs:1260-1263`); `hint_text_color = weak_text_color` | shipped values `bg_0`, `text_3` |
| type styles | `Style::text_styles` map (`theme.rs:191-201`): Heading 20, Body 13, Button 12 semibold family, Small 11, Monospace 17 | roles without a `TextStyle` claim must pass an explicit `FontId` |
| motion | `Style::animation_time` (default 0.2 s, `style.rs:318,1550`), `Context::animate_bool*` (`context.rs:3709-3760`), `emath::easing::cubic_out` | framework-driven only; no reduced-motion API (0 hits) |
| spinner | `egui::Spinner` sizes from `interact_size` (40 px) and requests repaint (`widgets/spinner.rs:40-52`) | continuous; see §7 |
| scrollbars | `ScrollStyle::floating()` default (`style.rs:589-600`, `dormant_handle_opacity: 0.0`), fades via `animate_bool_responsive` (`scroll_area.rs:826-832,1607-1616`); `ScrollStyle::solid()` removes the fade | W2: solid style, also fixes hover-only affordance |
| notice / warning line | `Frame::new().fill(danger_soft).stroke(Stroke::new(1.0, danger)).corner_radius(CornerRadius::same(R_INLINE)).inner_margin(Margin::same(S2 as i8))` around a plain `ui.label` (`container_frame` APIs; shipped `app.rs:2261-2277`) | the frame is paint-only; the word carries the state (§6.10) |
| wrapping long copy | `egui::Label` wraps inside the available width by default; no `layout_no_wrap` for multi-line copy (`Painter::layout_no_wrap` is used only where a single line is intentional, `ledger.rs:81-88`) | the W2 action slot adds an explicit two-line layout; everything else wraps. Type size is never reduced to fit (§4.4) |
| theme switching (light mode) | `Context::style_of`/`set_style_of`, `ThemePreference` (`context.rs:2645,2674`; `memory/theme.rs:67`) | exists but unused; no light tokens |
| reduced motion | **not in egui** | OS signal per §7.4, assigned to spec W2-2 acceptance (i) + W2-4 acceptance (i); win32 `SPI_GETCLIENTAREAANIMATION` via hand-declared `extern "system"` (`single_instance.rs:157-179` pattern); `On` = animations enabled = spinner allowed, `Off`/`Unknown` = static text |
| text metrics | `FontId` + `Painter::layout_no_wrap` (`painter.rs:503`) for measuring before painting (the action's shrink path does this; W2 replaces it with the type budget) | row heights come from face metrics (§4.1) |
| DPI | everything above is in points; `pixels_per_point` rounds glyph raster and `round_ui` row terms | no layout in physical px anywhere in the system |

---

## 11. W2 adoption checklist and open decisions

### 11.1 Token module (`design.rs`) — one change, with the pin tests

- [ ] Add `border_control #65656A`, `map_land #4A4A72`, `HERO_H 40`,
      `RAIL_EDGE 3`, `MAP_H 132`, `MAP_W_MAX 430`, `HIT_MIN 24`,
      `FOCUS_RING_W 2`, `ACTION_H_WRAPPED 56` (the wrapped action height, §1.4).
- [ ] Delete `accent_strong`, `signal_soft`, `info`, `info_soft`, `warn_soft`,
      `GLOBE_LG`, `GLOBE_SM`, `GLOBE_XS` — **nine** tokens. `R_INLINE` is **kept**:
      master `76aba5d` uses it for the starvation-notice frame (§1.2).
- [ ] No token **value** change is required by the master rebase:
      `design.rs` is byte-identical to the W1 snapshot (`git diff
      62c6cf6..HEAD -- client-gui/src/design.rs` is empty). Only usage and the
      role comments move (§1).
- [ ] Re-role `bg_4`, `border_1`, `border_2`, `accent_text`, `grid`, `S5`,
      `on_accent` and correct the module docstring (it currently documents the
      website adoption, not the roles).
- [ ] Update `opaque_tokens_match_the_site_hexes`/`geometry_scale_is_stable`/
      `type_scale_is_stable` for the change, and add the two new tests (§1.5):
      role-uniqueness source scan and measured-contrast floors.
- [ ] Correct the documented line heights to the measured row heights (§4.1).

### 11.2 Theme (`app/theme.rs`)

- [ ] `widgets.active` becomes the dark palette (`bg_4` fill, `accent_text`
      1 px stroke, `text_1` ink) so pressed no longer steals the accent fill;
      `accent_deep`/`accent` remain the custom action's only fills.
- [ ] `selection.stroke = Stroke::new(1.0, accent_text)` (fixes selected-row ink
      6.70:1 and focused-field stroke 8.11:1).
- [ ] Control boundaries: fields/combos/checkboxes use `border_control`.
- [ ] `visuals.selection.bg_fill` stays `accent_soft`; selected rows add the
      1 px `accent_text` left edge (painted, not `Frame::stroke`).
- [ ] `spacing.scroll = ScrollStyle::solid()` (no fade, permanent affordance).
- [ ] `animation_time`: leave the framework default until reduced motion exists;
      nothing in the app calls `animate_*`.
- [ ] Fonts unchanged; `install_fonts` still before the first frame.

### 11.3 Components

- [ ] Shared `focus_ring(ui, response)` helper; applied to every interactive
      control (action, combos, fields, checkboxes, list rows, sheet buttons).
- [ ] `action_button`: ring on focus; a distinct pressed cue; `widget_info`;
      loading state; **both variants (Accent and Danger) per §6.2**; label fits
      §4.4 (copy or two lines); painted stop mark stays.
- [ ] Rail: `HERO_H` 40; remove the `0.82` multiplier; **move the metric to its
      own row below the state word** or shorten the long localized words — the
      measured RU/DE collision is `delta-copy.md` §5 — and paint the route arrow
      rather than typing `→`; rail edge colours §3.4.
- [ ] Notice/warning line per §6.10: frame, 4-line measured max, no shrink, one
      notice at a time; the location ladder and route-trace rows per §6.11.
- [ ] New copy review: `ux-lead` shortens the starvation banner toward 2 lines
      and the RU restart label to 2 lines (§4.4); no type size changes.
- [ ] Bundled-font decision (§8.2, §11.5): re-subset Inter to add `y`/`{` (and
      JetBrains Mono its missing ASCII) or record the fallback mixing as accepted.
- [ ] Text fields: `border_control`; `selection.stroke` fix; invalid treatment
      unchanged (already passes).
- [ ] Sheets: per-sheet close policy (editor/update guard), computed body height,
      danger result line, focus confinement + Esc (test it; mark unverified until
      then).
- [ ] List rows: selected fix, dense proxy row 40 px with 24×24 delete target,
      region/loading/empty/error captions per §6.7.
- [ ] Map: tokens for the five literals; marker hit rect 24×24.
- [ ] Game picker tile: only if the W2 spec moves away from the one-step combo;
      contract in §6.8.

### 11.4 Verification this design requires

- [ ] Real-window screenshots at the **mobile-only widths** — **320×420** (min,
      short mode), **320×760** (initial) and **320@200 %** (the same 320 pt window
      rasterised at 640 px), plus the DE capture for the long-copy rows — for
      every reachable state (the skill's evidence rule); UIA cannot see egui
      popups, so read the image. Include the warning banner, the location ladder
      rungs and a route-trace row, plus the **RU capture for the state-word
      (`IDLE`, `NO RETURN PATH`) and action-label binding cases**, with the
      shortened RU/ES/PT strings, and the DE long-copy capture; verify no
      state-word/metric occlusion, that each long string wraps rather than
      shrinking, and that every locale's state word measures ≤276 px at 30 px
      (§4.4 item 4).
- [ ] A contrast test in the crate (not just this document) so the floors cannot
      regress.
- [ ] A **state-word fit gate** (crate test): every locale's uppercased state
      word at `STATE` 30 px measures ≤276 px in the installed chain (§4.4 item 4);
      the measurement reuses the fonts module from this evidence set.
- [ ] `widgets.rs`/theme: remove the `STATE * 0.82` multiplier and render the
      metric on its own row with `HERO_H` 40 (§4.2, §11.3).
- [ ] A focus/keyboard pass: Tab reaches every control, the ring is visible,
      Esc closes sheets, the primary action is announced by AccessKit.
- [ ] Reduced motion (QA R5): W2-2's `progress_indicator()` has a static-text
      path and takes the reduce-motion input; W2-4's Windows parser is unit-tested
      and the runtime read verified — `On` (animations enabled) is the only state
      that shows the spinner, `Off`/`Unknown` are static; the currently shipped
      un-gated spinners are the 2.2.2 exposure carried in the parity report until
      W2-2 lands.
- [ ] The font gate extended to `native_name` + symbol allowlist.
- [ ] 200 % scaling capture: row heights are computed here at 1.0 ppp; the
      `round_ui` terms and glyph raster change at 125/150/200 %.

### 11.5 Decisions the Lead/owner must take (not the design system's to take)

1. **Delete 9 dead tokens** (internal; needs a decision-log entry and reviewer).
2. **Action label: two-line adopted** — the spec (D7/§1.3) takes the two-line
   action at 15 px and deletes the 9 px shrink; the residual is copy/i18n polish
   (`ux-lead`) and the new `ACTION_H_WRAPPED` 56 token (this doc §1.4). It is no
   longer an open layout decision.
3. **Reduced motion (QA R5)** — assigned by the revised spec to W2-2 (i) (one
   `progress_indicator()` helper with a static-text alternative and a
   reduce-motion input) and W2-4 (i) (Windows `SPI_GETCLIENTAREAANIMATION` read,
   parser unit test, runtime check; macOS/Linux `Unknown` = gaps). No user-facing
   setting claim. Polarity: `On` = animations enabled = the only state that shows
   the spinner; `Off` and `Unknown` are static text. Residual: the currently
   shipped un-gated spinners until W2-2 lands; the 2.2.2 exposure stays in the
   parity report.
4. **Light mode: out of scope** (confirm; §9).
5. **Viewport** — **decided by the owner on 2026-11-20: mobile-only.** Keep the
   shipped geometry (320 px pinned width, one `narrow` column, height resizable
   420–1400, initial 760); the W1 360×420 / 440×880 / 1120×1400 plan and every
   two-column layout are superseded. No width change is in scope.
6. **Inter subset gap (`y`, `{`; JBMono `o`, `q`, `!`, ...)** — re-subset the
   bundled fonts or accept the measured fallback mixing (§8.2). A font-asset
   change is cheap and OFL-clean; the alternative is documented type-mixing in
   every English string containing `y`.
7. **RU restart action copy — resolved by landed copy.** `ux-lead`'s
   `ru.toml` string (`Перезапустить от имени администратора`) measures 356.9 px
   = 2 lines at 264 px (HEAD was 518.6 px = 3 lines), so the action band stays
   72 px and the 239/181 compact worst is unchanged; the 73 px three-line slot is
   priced out. The dropped "to optimize" purpose must be carried elsewhere by the
   spec/copy owner (flagged); the type is never shrunk.
8. **Rail collision + 30 px state word (F1)** — the metric moves to its own row
   (mandatory), and the **recommended remedy is shortening the remaining
   over-budget state words to the proposed acceptance cap ≤276 px at `STATE`
   30 px** (`ux-lead`; ES `SIN RETORNO`, PT-BR `SEM RETORNO`, RU `IDLE`
   contingency, optional DE `OPTIMIERUNG`; RU `NO RETURN PATH` already landed at
   222.9 px). It keeps the 30 px role, the hero band and the 239/181 compact
   budget. Fallbacks: a 24/25 px effective word size (type role departs from 30)
   or a two-line hero priced at 275 chrome → 145 scroll, below the 180 floor —
   needs an owner-approved floor change. §4.4 item 4, §6.9 gap 11.

---

## 12. Unverified measurements and limits

Everything in §3 and §8 is computed from authored values and font tables, not
observed in a running window. The honest list:

- **No screenshot was taken in W1** (docs-only round). Contrast is computed, not
  sampled from a rendered frame; compositing order in egui is assumed to match
  the straight-alpha "over" model (it does for opaque planes; a future screenshot
  sample should confirm the two tinted cases).
- **Row heights are computed** from face metrics with epaint's formula at 1.0
  pixels-per-point, not measured from a laid-out galley. At 125/150/200 % each
  `round_ui` term rounds to a different step, so a row can move ±1 px.
- **Label widths and wrap-line counts are raw `hmtx` advances** through the
  installed font chain (`delta-copy.py`); epaint shapes with `harfrust`
  (GSUB/GPOS), so kerning can move a line by a couple px and, at a word boundary,
  change a line count by one. The budget margins are stated with that tolerance.
- **The banner/location/trace line counts** are computed for the nine catalogs
  with sample placeholder values (3-digit ms, 5-digit seconds, a long provider
  and place name); they are not sampled from a rendered window and no live
  starvation session was observed in the delta round.
- **The face-identity finding** (`y`, `{`, JBMono's missing ASCII) is measured
  from the bundled cmaps, not observed in a window; the byte-exact rendered mix
  should be confirmed with a screenshot at W2.
- **Dynamic states were not observed:** boosting, fallback, the watchdog
  re-tests, sheet focus confinement, AccessKit output, reduced motion (there is
  none), 200 % scaling, and the macOS/Linux OS signals. They are specified, not
  proven.
- **The shipped 16 px proxy row** is code-measured (allocation vs egui's 40 px
  control minimum); the overlap it causes was not screenshot-verified this round.
- **The native-name tofu** is font-table-measured; on Windows the system-font
  discovery provider is likely to render those glyphs, so the defect is
  device-dependent. It is still a defect because the bundled fonts are the only
  guarantee.
- **AAA counts** are reported for completeness; the canon claims AAA only for
  primary ink.

---

## 13. Historical critique (pass 1) — mapping to this document

The 16 required changes in `docs/design/ui-design-critique.md` are historical;
this table records which are answered by the design system, which by another W1
owner, and which are already fixed in the shipped tree.

| # | required change | status |
|---|---|---|
| 1 | keep an elevation path (one action slot, three labels) | shipped (`ledger.rs:39-57`); this doc fixes its focus/loading/pressed states (§6.2) |
| 2 | disconnected card honesty | `ux-lead`/`interface-designer` (state contract); the shipped `route_card` gates the markers and the emphasised relay on `connected`; the map itself stays drawn |
| 3 | Privacy/Maintenance/About/Disconnect destination | shipped into the settings sheet (`app.rs` `settings_body`); copy/IA is `ux-lead` |
| 4 | retire dashed-meta copy | `ux-lead` (copy); type roles enforce sentence case (§4.1) |
| 5 | ledger-key case; delete `ROUTE` eyebrow | this doc: §4.1 rule 3 (only the state word and action label are uppercase) |
| 6 | complete the token scale (radii, sizes) | this doc §1.2, §5.2, §5.4; dead/missing values resolved in §1.4 |
| 7 | height breakpoint | **resolved by the mobile-only decision** (2026-11-20): one 320 px column, `short` height branch only; measured chrome/scroll arithmetic in §5.4 |
| 8 | game choice one step / tile panel | shipped as a combo; tile contract defined in §6.8 for the W2 spec |
| 9 | relay identity from real data | shipped (`app.rs` `relay_value`, `discovery.rs` `friendly_label`) |
| 10 | rail + sparkline one colour rule | shipped: sparkline stroke `text_3`, only the last point uses `rtt_colour` (`widgets.rs:437-470`) |
| 11 | migrate the inline literals | 5 remain, all in `globe.rs`; §1.3/§1.4/§5.6 |
| 12 | accent/ink contrast rule | §2 rule 1, §3.4 (rule measured: 3.94:1 accent-as-ink) |
| 13 | do not claim reduced motion | §7.4: no user-facing claim; the mechanism is assigned to spec W2-2 acceptance (i) + W2-4 acceptance (i), with the un-gated interim and its WCAG 2.2.2 exposure recorded |
| 14 | turn off default shadows | shipped (`theme.rs:141-142`) |
| 15 | clamp and scroll every sheet | shipped (`sheet_bounds`, `ScrollArea`); computed-height fix in §6.6 |
| 16 | ledger mechanics in px, not characters | measured baseline analysis §4.2; `LEDGER_LABEL_W` is retired with the wide branch in W2 |

---

## Appendix A — evidence index

All under `wat/evidence/2026-11-19/team/design-system/` unless stated; the
2026-11-20 delta snapshots are in `wat/evidence/2026-11-20/team/design-system/`.

| file | content |
|---|---|
| `contrast.py` / `contrast-results.md` | contrast method, token values and composited tints, 41 text pairings + 20 non-text claims with ratios/verdicts (re-run on the master delta adds the notice/trace rows), proposed values (`border_control` search, focus ring, selection fix), composite states, method cross-check |
| `fonts.py` / `fonts-and-glyphs.md` | installed font chains, face coverage, candidate-symbol coverage, 619 catalog chars (re-run on the delta), native-name gap, missing-glyph audit (`y`, `{`), measured row heights, rail row fit, localisation budgets |
| `token_audit.py` / `token-usage.md` | per-token use counts and sites (semantic filter documented), dead/theme-only/shadowed tokens, the five inline literals |
| `token-usage-raw.txt` | raw first-pass scan (includes the comment/macro noise the filtered audit removes) — kept so the filter's effect is auditable |
| `revision-r2.md` | QA review r1 finding → change map (R6, R11, R17, R5), with before/after numbers and the re-run record |
| `delta-copy.py` / `delta-copy.md` (`2026-11-20/…`) | mobile-only 320 px copy measurements: added/changed/banner/location/trace/update strings, per-locale wrap-line counts, action-label and state-word fit, chrome/scroll arithmetic, notice-frame budget, face-identity audit |
| `delta-r11.md` (`2026-11-20/…`) | delta r11 finding → change map, with the re-run record and the machine-checkable summary |
| `cite-check.md` (`2026-11-20/…`) | every `app.rs`/`main.rs` citation in this document re-derived against HEAD `0df57ce`: old ref → new ref → current-line content, plus the assertion that no pre-rebase citation remains |
| `delta-r14.md` (`2026-11-20/…`) | delta-fix r14 finding → change map (F1, F2, F4, F20, Q15), the state-word measurement at 30 px, and the recommended remedy for the spec owner |

The scripts are the method; the Markdown is their output. Re-run:
`py -3 wat/evidence/2026-11-19/team/design-system/{contrast,fonts,token_audit}.py`.
