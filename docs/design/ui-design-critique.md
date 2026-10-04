# LightSpeed GUI design plan — hostile critique (pass 1 review)

Scope: `docs/design/ui-design-plan.md` reviewed against `docs/design/ui-audit.md`,
`client-gui/DESIGN.md`, the binding `lightspeed-gui-design` skill, the shipped code
(`client-gui/src/app.rs`, `main.rs`, `globe.rs`, `discovery.rs`), and the fonts and widget
APIs actually locked in `Cargo.lock` (egui/eframe 0.36.2, egui_plot 0.37.0), whose sources
were read from `~/.cargo/registry`. Repo `C:/Users/ShibbityShwab/Documents/GitHub/lightspeed`,
branch `master`, HEAD `4e0ebb7`, 2026-10-04. No code was written or changed by this review.

## Verdict

**REVISE — 16 required changes.**

The plan is the first proposal in this repo that reads like a design lead wrote it: it kills the
per-card ALL-CAPS eyebrow, the one-card-for-everything silhouette, the mono-as-costume type scale,
and the eleven-variable hole in the palette, and it grounds the window in the product (a status
instrument, not a page). Those are real, checkable wins and they are why the verdict is REVISE and
not REJECT.

It is not approvable as written. Four things sink it: (1) the wireframes silently delete the
elevation affordance that the *shipped code still gates the boost action on* (`is_admin`,
app.rs:1213) — so the plan as drawn is not implementable and the audit's problem 7 is answered by
deletion rather than by design; (2) the audit's problem 8 (disconnected card shows stale RTT and an
active-relay globe) is implied by a wireframe but never stated as a rule, and the code guard it
needs (`!rtt_history.is_empty()`, app.rs:1357) is untouched; (3) Privacy, Maintenance, About and
Disconnect have no destination anywhere in the new layout — they are dropped, not placed, so audit
problem 9 regresses into "controls that no longer exist"; (4) the layout fixes width-320 and
ignores height-400, where the fixed rail + action bar + route card alone exceed the minimum window
before the footer is drawn.

## Required changes

1. **Keep an elevation path and say where it lives.**
   *Why:* every wireframe shows `BOOST MY GAME` as the one bright control, but the app cannot boost
   unelevated (`let can_act = self.is_admin && … && self.status.connected`, app.rs:1213), so as
   drawn the primary action is dead for a non-admin user and the audit's problem 7 is resolved by
   deleting the only control that worked rather than by fixing its weight.
   *Replace with:* one action slot, three labels, driven by the existing gate — `BOOST MY GAME`
   (enabled, `accent` fill) when `is_admin && connected && a relay is selected`; `RESTART AS
   ADMINISTRATOR TO BOOST` (`accent` fill, same slot, same size) when `!is_admin && !boosting`; `■
   STOP BOOST` (`bg-3` fill, `danger` hairline and ink) when boosting. Delete the caption
   "Administrator is required to redirect game traffic" (app.rs:1255) — the reason is in the button
   label now, not repeated underneath it. The rail stays the loudest object in every state.

2. **State the rule that makes the disconnected card honest.**
   *Why:* audit problem 8 — with the tunnel down the card still paints the previous session's RTT
   curve (guarded only by `!self.status.rtt_history.is_empty()`, app.rs:1357) and still highlights
   LAX as the active node (app.rs:1275-1305). The plan's wireframe shows `--` values and "no arc",
   but nothing in the plan says so as a rule, so pass 2 can implement the wireframe and leave the
   bug.
   *Replace with:* a Layout subsection "Not boosted (no relay)" that states: the route card renders
   only when `status.connected`; when it is not connected the card draws the globe with no arc and
   no emphasised marker, and every ledger value is the literal `--`; the sparkline is gated by
   `connected && !rtt_history.is_empty()`; `disconnect()` clears `rtt_history` and the emphasised
   `active_node` is `None` unless `connected`.

3. **Give Privacy, Maintenance, About and Disconnect a named destination.**
   *Why:* audit problem 9 lists them as buried four levels down; the plan's layout contains only
   Boost server, Game and `▸ Advanced`, so telemetry opt-out, "Check for updates", "Disconnect from
   relay" and the app's identity vanish instead of moving. A plan that removes controls it was told
   to surface is not a fix.
   *Replace with:* the footer becomes `Hide to tray   |   Settings   |   Quit`; `Settings` opens the
   in-window sheet (the same sheet pattern the plan already defines for Proxy Manager) with sections
   `Privacy`, `Maintenance` (Check for updates, Disconnect from relay), `About`. The `▸ Advanced`
   disclosure is deleted; the manual-server fields move into `Settings` under `Advanced`. The main
   column then contains exactly two config rows and nothing hidden.

4. **Retire the dashed-meta copy the plan left standing.**
   *Why:* audit problem 3 cites four offenders; the plan revises the state line and the pipe-joined
   counters but never touches `"Reliability Shield - recover lost packets (+25% data)"`
   (app.rs:811), `"Advanced - set server manually"` (app.rs:838-840) or `"No game running - select
   your game and click Boost"` (app.rs:773-786). None of the three appears in any wireframe, so they
   survive by omission.
   *Replace with:* checkbox label `Reliability Shield` at `label` 12, with the explanation moved
   into a `caption 11` `text-3` sub-line: `Repairs lost packets · +25% upload`; the Advanced
   disclosure label is deleted with requirement 3; the empty game row reads `No game detected` +
   a `Choose game` text button (see requirement 8).

5. **Resolve the ledger-key case contradiction and drop the `ROUTE` eyebrow.**
   *Why:* the Type table says `label` is "field labels and section labels, sentence case" and
   "no wide-tracked all-caps eyebrows", while every wireframe prints `ROUTE`, `RELAY`, `CITY`,
   `GAME SERVER`, `RELAYED` in caps — and `ROUTE` is exactly the eyebrow-above-a-section the audit
   (problem 2) demanded be removed. Two readings of the same document produce two different builds.
   *Replace with:* the only uppercase strings in the window are the rail's state word and a button
   label. Section labels are `label 12` Inter SemiBold sentence case (`Route`, `Settings`); the
   ledger key column is `caption 11` Inter Regular `text-3`, sentence case (`Relay`, `Game server`,
   `Relayed`); `ROUTE` is deleted and the route card is identified by position, not by a heading.

6. **Complete the token scale the plan left open.**
   *Why:* the plan defines one card radius (8) and no radii for controls, fields, tiles or sheets;
   it also leaves the four off-scale radii (6/10/12 and the inline `4.0`, app.rs:857) and the
   flagged magic sizes (46 px button app.rs:1235, 64 px plot app.rs:1355, 104/132 px globe
   app.rs:1315, `add_space(6.0)` app.rs:909) unmapped. Tokens-only is unenforceable while five radii
   and four sizes are unspecified.
   *Replace with:* radii tokens `r-inline 4` (`--r-xs`), `r-control 8` (`--r-sm`), `r-card 8`,
   `r-button 12` (`--r-md`), `r-sheet 16` (`--r-lg`); size tokens `control-h 40`, `action-h 48`,
   `plot-h 64`, `globe-lg 132`, `globe-sm 104`, `globe-xs 72`; the inline `4.0` radius and
   `add_space(6.0)` are deleted in the same change.

7. **Add a height breakpoint, not just a width one.**
   *Why:* the window minimum is 320×400 (main.rs:89). Fixed chrome at 320 is rail (~110 px: 30 px
   word + 19 px sub-line + hairline + margins) + action bar (48 px + margins) + route card (72 px
   globe + four ledger rows + hairline ≈ 210 px) + footer (≈40 px) ≈ 410 px *before the scrolling
   config region gets a pixel* — so at the minimum the scroll region and the footer are squeezed to
   zero, which is audit problem 10 recurring at a different size.
   *Replace with:* a second breakpoint `short = available_height < 560.0`: hide the globe, reduce the
   ledger to `Relay` and `Relayed`, and move the route values into the scrolling region. Budget rule
   stated in the plan: fixed chrome ≤ 220 px, so the scroll region always gets ≥ 180 px at 400 px
   height. At `narrow` (width < 380) keep full-word footer labels and lay them out with
   `ui.horizontal_wrapped` so `Hide to tray` / `Open log` / `Quit` wrap instead of abbreviating to
   `Hide tray` / `Log`.

8. **Keep game choice one step; make the tile grid a deliberate panel, not launch chrome.**
   *Why:* the registry has **22** entries (`GAME_REGISTRY`, client/src/games/mod.rs:280-303), not
   the six the wireframe shows, and the plan renders the grid as a full-height state whenever
   detection finds nothing — i.e. on most launches, in front of the player's one-second glance. That
   trades audit problem 9's dead ends for a new wall.
   *Replace with:* the `Game` ledger row keeps its themed combo as the one-step selector; a
   `Choose game` text button in that row opens the tile grid inside the *scrolling* region, tiles
   two-up at ≥380 px and one-up below, capped at the 22 registry entries with the current selection
   scrolled into view on open; `Esc` and a `Done` button close it back to the rail.

9. **Take the relay identity from data the app actually has.**
   *Why:* the plan's `RELAY fra1.de` and `CITY Frankfurt` are invented. The app carries a registry
   `node_id` (e.g. `relay-fra-1`) and a `SocketAddrV4`; the only human-readable name available is
   `discovery::friendly_label(node_id)` → `FRA - Frankfurt` (discovery.rs:138-159), and there is no
   standalone city field. A wireframe that requires a value the code cannot produce will be faked in
   pass 2.
   *Replace with:* `Relay` = `friendly_label(node_id)` when `node_id.is_some()`, otherwise
   `status.proxy_addr` verbatim in mono; the `City` row is deleted (it duplicated the label's
   second half); `Game server` = the `ip:port` string from `status.redirect_server` /
   `status.windivert_server` verbatim in mono; `Relayed` = `health.packets_relayed` and
   `health.sessions_created`.

10. **Make the rail and sparkline obey one colour rule.**
    *Why:* the plan says the rail edge, the state word and the sparkline "all carry the same green"
    (boosting wireframe) while principle 4 and anti-slop #5 say the rail's green is the *good*
    signal, and `rtt_colour` already ramps `OK → WARN → BAD` at 60/120 ms (app.rs:1818-1826). A
    green sparkline under an amber `247 ms` metric is the plan contradicting itself.
    *Replace with:* the sparkline stroke is `text-3`; only its last point (and the rail metric) take
    `rtt_colour(latest_rtt_ms)`. State is carried by the rail's edge and word (`signal` only when
    actually boosting), latency by the metric, and the two never share a colour channel.

11. **Commit to migrating all eight literals, and fix the `914` mapping.**
    *Why:* audit problem 6 counts eight inline literals as debt; "migrate when the code around them
    is touched" (Tokens section) can leave app.rs:749 (`(255,190,60)`), app.rs:936
    (`(220,130,50)`) or app.rs:1470 (`(220,80,80)`) untouched, and the plan's own Done checklist
    already says "each legacy literal met during the change" — a weaker promise than the audit
    requires. Separately, the mapping for app.rs:914 gives `(40,90,55) → signal-soft over bg-0`,
    which is not a token but a hand-mixed blend.
    *Replace with:* Done checklist item "all eight sites at app.rs 749, 856, 898, 903, 914, 916,
    936, 1470 are migrated in this change", plus the gate
    `grep -c 'Color32::from_rgb' client-gui/src/app.rs` must equal 10 (the `mod theme` definitions
    only); app.rs:914 becomes `.fill(signal_soft)` where `signal-soft` is a real
    `Color32::from_rgba_unmultiplied` token and egui composites it over the `bg-1` backdrop.

12. **Fix the accent/ink contrast the plan inherits.**
    *Why:* the accessibility floor is a hard constraint. Measured: `#6c5ce7` (accent) as ink on
    `#0a0a1a` is **4.03:1**, below the 4.5:1 AA floor, and `on-accent #ffffff` on accent is
    **4.86:1** — AA-passing, but the thinnest ratio in the set for a 15 px label. The plan adopts
    both but does not say they are fill-only.
    *Replace with:* a stated rule "`accent` is a fill, never ink; accent-as-ink is always
    `accent-text #a29bfe` (8.08:1 on `bg-1`)". Primary-button ink stays `on-accent #ffffff` on an
    `accent-deep #5a4bd4` base (white on it is 5.6:1, verified) with `accent` reserved for the
    hover/rest distinction; the raw `Color32::WHITE` in `primary_button` (app.rs:1804) — currently
    untracked debt — is replaced by the `on-accent` token.

13. **Do not claim a reduced-motion mode the app does not have.**
    *Why:* egui 0.36 has no reduced-motion signal, and the plan's checklist asserts "reduced-motion
    honored by stopping the globe" with no mechanism. Shipping an accessibility checkbox item that
    cannot be verified is worse than not claiming it.
    *Replace with:* either wire the real source (`SystemParametersInfoW(SPI_GETCLIENTAREAANIMATION)`
    polled once into a `reduce_motion: bool`) and gate the globe's rotation, or replace the claim
    with the rule that already exists: the globe rotates only while `boosting` **and**
    `ctx.input(|i| i.focused)`, with no other motion in the window.

14. **Turn off egui's default shadows.**
    *Why:* principle 4 says "no shadows", but `theme::apply` (app.rs:1656-1712) never sets
    `window_shadow`/`popup_shadow`, so both the current `egui::Window`s and any popup keep egui's
    default soft shadow — the exact "one soft shadow everywhere" tell the plan claims to have
    removed.
    *Replace with:* in `apply`: `v.window_shadow = egui::Shadow::NONE; v.popup_shadow =
    egui::Shadow::NONE;` — sheets and combos then read as hairlines on surfaces, as the plan says
    they do.

15. **Clamp and scroll every sheet.**
    *Why:* the plan moves Proxy Manager and Update into in-window sheets. `egui::Modal` sizes to its
    content and does not scroll (modal.rs:16-46); the Proxy Manager list is unbounded (one row per
    relay, app.rs:1421-1428), so at 320 px a pinned sheet overflows the window.
    *Replace with:* every sheet body is wrapped in
    `egui::ScrollArea::vertical().max_height(available_height - 120.0)`, and the sheet frame is
    `ui.set_max_width((available_width - 24.0).min(420.0))`; the Proxy Manager row becomes
    label-over-address at `narrow`, matching the ledger rule.

16. **Specify the ledger mechanics in pixels, not characters.**
    *Why:* "the ledger's label column is fixed at 14 characters" (connected wireframe) is not
    expressible in egui — the column is drawn in proportional Inter, so a character count gives a
    different width per label. Likewise "the state word and the metric sit on the same baseline"
    is not free: egui aligns widget *rects*, and Inter 30 and JetBrains Mono 30 have different
    ascents/descents.
    *Replace with:* `ledger-label-w` is a px token (96 at 440 px, full width at 320 px); each ledger
    row is a fixed `row-h 24` allocation with the value right-aligned into the same x via
    `ui.allocate_ui_with_layout([value_w, row_h], Layout::right_to_left(Align::Center), …)`; the
    rail's hero row allocates `[available_width, 34]` and places the word left and the metric right
    at `Align::Center`, accepting ≤1 px baseline drift as the documented floor (or, if exact, one
    `Painter::text` pass at a shared baseline y).

## Egui feasibility

Every non-standard element the plan asks for, against egui/eframe 0.36.2 and egui_plot 0.37.0 as
locked in `Cargo.lock` (sources read in `~/.cargo/registry/src/index.crates.io-1949cf8c6b5b557f/`).

| Plan element (plan section) | egui primitive it maps to | Verdict |
|---|---|---|
| Full-bleed state rail, zero radius, no side margin (`Layout`) | `Panel::show(ui, …)` with `Frame::new().fill(bg-2)` and `Margin::ZERO`; the root `Ui` has no margin (eframe 0.36 `App::ui` docs, epi.rs:173) | Feasible |
| 3 px left edge in `signal`/`accent` (`state rail`) | No per-side stroke exists — `Frame::stroke` draws all four sides. Paint it: `ui.painter().vline(x, y_range, Stroke::new(3.0, signal))` after the frame response | Feasible, must not be attempted with `Frame::stroke` |
| Fixed rail / action bar / route card + only the middle scrolling + fixed footer (`Layout`) | `TopBottomPanel::top/bottom(...).show(ui, …)` + `CentralPanel::…show(ui, …)` + `ScrollArea::vertical().auto_shrink([false,false])`; in 0.36 panels take `&mut Ui` (panel.rs:422, 1212), so the shell composes inside `App::ui` | Feasible — and this is the exact fix for audit problem 10 |
| In-window sheets with a `scrim` backdrop and `bg-0` body (`anti-slop 8`) | `egui::Modal::new(Id).backdrop_color(scrim).frame(Frame::new().fill(bg-0).stroke(border-1).corner_radius(r-sheet))` (modal.rs:16-31) | Feasible, native |
| Custom `▸`/`▾` disclosure glyph (`anti-slop 8`) | Text glyph or painted triangle | **Flagged.** Cmap test: `▸` U+25B8 and `▾` U+25BE are absent from Inter Regular/SemiBold, JetBrains Mono, Ubuntu-Light and NotoEmoji; only `emoji-icon-font.ttf` has them. Today's shipped `▶` U+25B6 *is* covered by NotoEmoji and emoji-icon-font, and `■` only by the icon font. Cheapest faithful alternative: paint the marker — `ui.painter().add(Shape::convex_polygon([..3 points..], text-3, Stroke::NONE))`, rotated 90° when open. That is font-independent and matches the 1 px hairline language; do not route the new marks through the emoji fallback (DESIGN.md rule 1). |
| Themed combo box "our box and hairline rather than default chrome" (`anti-slop 8`) | Already done: `theme::apply` sets `widgets.inactive/open` fills, strokes and `menu_corner_radius`, and `window_fill`/`window_stroke` for the popup (app.rs:1656-1712) | **No-op claim.** `ComboBox` exposes no per-instance frame (only `width`, `selected_text`, `icon`), so the themed look *is* the global `Visuals` path. The plan should delete this bullet or say what is actually wrong with the popup today. |
| RTT sparkline, 64 px, no axes, `grid` colour (`boosted wireframe`) | `egui_plot::Plot::new(id).height(64.0).show_axes([false,false]).show_grid([false,false]).grid_color(grid).show_background(false)` — `grid_color` (plot.rs:639), `show_grid` (:628), `show_axes` (:619), `show_background` (:610) all exist in 0.37 | Feasible; cheapest faithful alternative is a hand-painted `ui.painter().add(Shape::line(points, Stroke::new(1.5, text-3)))`, which needs no plot interactions and keeps the token colour exact |
| Globe markers as `◆` / `▲` / `•` glyphs (wireframe legends) | `globe::draw` already paints markers with `painter.circle_filled` (globe.rs:213) and land/grid lines with `Shape::line` (:115-197) | Feasible **only as painter shapes**. If implemented as text, `◆` U+25C6 and `▲` U+25B2 exist only in `emoji-icon-font` — a stroke-weight and metrics mismatch against Inter. Keep shapes; a triangle marker is `Shape::convex_polygon`. |
| 30 px Inter SemiBold state word and 30 px JetBrains Mono metric "on the same baseline" (`Type`, wireframes) | `ui.horizontal` / `Layout::right_to_left(Align::Center)` in a fixed-height row | Feasible to ±1 px; exact baselines require one manual `Painter::text` pass computed from both fonts' ascents. The plan should state the tolerance (see requirement 16). |
| RTT "padded to three characters so 9 ms and 124 ms do not shift the rail" (`Type`) | `format!("{:.0}", ms)` padded to width 3 in mono | Feasible; use `{:>3.0}` (a leading space inside a wrapped `Label` is not a reliable width device) and right-align into the allocated metric rect instead. |
| `accent-soft` / `signal-soft` / `grid` / `scrim` alpha tokens (`Tokens`) | `Color32::from_rgba_unmultiplied(...)`; `Modal::backdrop_color`; `Painter` strokes | Feasible, and matches the plan's own rule that alpha is never baked into a hex. |
| Scrollbar thumb on `border-2` (`Tokens`) | `Visuals.widgets.inactive/hovered` fills (scrollbar has no dedicated knob) | Feasible via the global visuals; note it shares the token with hover hairlines. |
| No shadows anywhere (`Design principle 4`) | `Visuals::window_shadow = Shadow::NONE`, `popup_shadow = Shadow::NONE` | Feasible but **currently unset** — see requirement 14. |
| Globe motion "only while boosting and focused", reduced-motion honoured (`Design principle 5`, Done checklist) | `ctx.input(|i| i.focused)` gate exists; there is no reduced-motion flag in egui 0.36 | Partial — see requirement 13. |
| Game tile grid, 2 columns / 1 at 320 (`game pick`) | `egui::Grid` or two `ui.columns`; 22 tiles need the scroll region | Feasible; see requirement 8 for placement. |

## Audit cross-check

One row per problem in the audit's Top 10. "Plan section" cites where the plan answers (or fails to
answer) it.

| Audit # | Problem (from `ui-audit.md`) | Fixed by the plan? | Plan section | Gap |
|---|---|---|---|---|
| 1 | Every content type is the same card (`theme::card`) | **Fixed** | Layout ("card radius is 8 and there is exactly one card style; the rail has no radius…"), anti-slop 1 | None material. The window now has rail / card / row / tile / sheet; the plan should add one sentence naming which container is correct for which role, or pass 2 will re-monoculture. |
| 2 | ALL-CAPS eyebrow above every section | **Partially** | anti-slop 3 (card titles gone, sentence-case labels), Type ("no wide-tracked all-caps eyebrows") | The wireframes still print `ROUTE` and uppercase ledger keys, contradicting the Type table — audit problem 2 survives inside the plan's own ASCII. See requirement 5. |
| 3 | Templated "WORD - fragment" and pipe-joined meta strings | **Partially** | anti-slop 3, Layout (ledger replaces piped counters) | Fixes app.rs:1164-1171 and :1342-1346; leaves app.rs:811, :773-786 and :838-840 untouched and absent from every wireframe. See requirement 4. |
| 4 | Monospace as costume on a flat type scale; `HEADING 20` unused | **Fixed** | Type (mono confined to `value` 17 / `metric` 30; `title` 20 gets a role), anti-slop 7 | The 17 px value column widens the ledger; at 320 the label-above-value rule covers it. Otherwise clean. |
| 5 | Default egui chrome survives wherever custom chrome exists | **Partially** | anti-slop 8 (both `egui::Window`s → sheets, custom disclosure, combo styling) | The combo bullet is a no-op (already themed by `theme::apply`); the OS default title bar — named in the audit — is kept and undecided; checkbox, scrollbar and plot chrome are unmentioned. |
| 6 | Eight inline literals; eleven missing palette variables | **Partially** | Tokens (full target set + migration table), anti-slop/Done checklist | The token set is complete and correct; the literal policy ("migrate when touched") is weaker than the audit and the mapping for app.rs:914 is a hand-blend, not a token. See requirement 11. |
| 7 | The loudest control is identical in every state and is not the product's action | **Not fixed** | — | The elevation row is deleted from every wireframe and `is_admin` (app.rs:1213) is never mentioned, so the boost action is unreachable unelevated as drawn. See requirement 1. |
| 8 | The disconnected screen claims live data (stale RTT curve, LAX highlighted) | **Not fixed** (implied only) | not-boosted wireframe shows `--` and "no arc" | No rule clears/gates `rtt_history` (app.rs:1357) or the emphasised marker (app.rs:1275-1305); nothing in the plan text addresses audit #8's code guards. See requirement 2. |
| 9 | Half the controls buried in one disclosure, with dead space below | **Partially** | anti-slop 8, Layout (one `▸ Advanced`) | Privacy, Maintenance (incl. Disconnect) and About are removed from every wireframe with no destination. See requirement 3. |
| 10 | Footer never renders; 320 px clips text | **Partially** | Layout ("scroll-body-shell… the footer is never pushed off-screen"), 320 wireframe | Fixes the footer mechanism and states the 320 wrap rule; breaks at 320×400 because fixed chrome exceeds the minimum height, and the wrap is asserted without an egui mechanism (`ui.horizontal` does not wrap). See requirements 7, 15 and 16. |

**Additional findings of my own (not in the audit's Top 10).**

- **Fabricated relay data.** `RELAY fra1.de` / `CITY Frankfurt` cannot be produced by the app: it
  holds a registry `node_id` (`relay-fra-1`) and a `SocketAddrV4`, and the only name helper is
  `friendly_label` → `FRA - Frankfurt` (discovery.rs:138-159). Requirement 9.
- **22 games, not six.** `GAME_REGISTRY` has 22 entries (client/src/games/mod.rs:280-303); the
  `game pick` wireframe shows six tiles and an ellipsis. As a full-height state it would confront a
  player with 22 tiles on a launch where nothing is detected. Requirement 8.
- **The "binary weight" claim is self-imposed, not a bundle limit.** `client-gui/assets/fonts/`
  ships `Inter-Bold.ttf`, which the plan's Type section never mentions and `theme::install_fonts`
  never registers (app.rs:1701-1745). "No weight emphasis is possible" is a choice the plan presents
  as a constraint: one line in `install_fonts` would expose a bold family.
- **The plan misdiagnoses the combo chrome.** Its anti-slop #8 says combos "get our box and hairline
  rather than default chrome"; `theme::apply` has themed every widget's fill, stroke and radius
  since the last revision (app.rs:1656-1712). The real remaining defaults are the OS title bar, the
  collapsing-header arrow and the `egui_plot` frame/legend — and the plan addresses only one of the
  three.
- **The status vocabulary drifts from the shipped one.** The tray and engine speak
  Connected/Disconnected (app.rs:1120-1128), the rail renames `OFFLINE` to `NOT BOOSTED`, and the
  audit's own screenshots read `CONNECTED`. Renaming the human-visible state without renaming the
  tray tooltip text leaves two vocabularies in one product.
- **`text-3` at 11 px passes but is the floor.** Measured: `#8a8aa8` on `bg-1` = 5.86:1, on `bg-2` =
  5.60:1, on `bg-0` = 6.00:1 — all AA-clean, so the accessibility floor holds, but the plan should
  name the measured value rather than the word "checked".
- **The route card is the only true card but carries the most chrome.** Rail (edge + word + metric) +
  globe + four ledger rows + sparkline is four visual systems inside one hairline; the plan's own
  "spend boldness in one place" restraint suggests moving the sparkline into the Settings sheet or
  cutting it at `short` height.

## Notes

Concerns below cite no success criterion; they are craft judgments, recorded so pass 2 can weigh
them.

- The plan's best idea is the label/value ledger and its biggest risk is the same ledger: four rows
  of near-identical label/value pairs is one substitution away from the generic dashboard the brief
  rejects. The plan already flags this as residual risk; I would go further and cap it at three rows
  in the default state (drop `Game server` until a boost is live).
- The globe is doing two jobs at once — identity (LightSpeed's route) and data (where the traffic
  went). At 104 px and under, the graticule is decoration; the plan keeps it "at rest, no arc",
  which is the calmest and best version of the element in the document.
- "Silence unless the state changes" is a stronger and more useful principle than anything in the
  token tables. If the plan had to keep one paragraph, that is the one.
- The narrow wireframe's `BOOSTING / 24 ms / VALORANT → fra1.de` stack is the clearest expression of
  the instrument idea in the whole document; the wide variants are busier.
- Naming the target module `design.rs` while the plan also keeps `mod theme` as re-exports invites a
  half-finished extraction. One module, one name, one commit.
- The type scale's `value 17` mono is a real improvement, but 17 px mono for `RELAYED` counters at
  440 px will visually outweigh the 13 px body around it; `value 15` mono with tabular padding may
  read more like an instrument and less like a terminal.
