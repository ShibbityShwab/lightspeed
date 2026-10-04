# LightSpeed GUI audit - sprint A (design phase)

Scope: the human-facing desktop surface, `client-gui/` (eframe/egui 0.36, Windows tray +
status window). Repo `C:/Users/ShibbityShwab/Documents/GitHub/lightspeed`, branch `master`,
HEAD `4e0ebb7`. Source of truth read for this audit: `client-gui/src/app.rs` (2188 lines),
`client-gui/src/main.rs`, `client-gui/DESIGN.md`, `web/styles.css`, and the binding
`lightspeed-gui-design` skill.

Method: `cargo build -p lightspeed-gui` (0 warnings, 11.8 s) then the debug binary launched
from the repo root; the live window was driven through the desktop tool (window list ->
screenshot -> foreground input -> Win32 `MoveWindow` for the 320 px case). Screenshots in
`## Screenshots` marked **fresh** were captured in this session from that binary; everything
else is a prior-session capture and is labelled as such.

## Current screens

**Disconnected (OFFLINE).** The banner card at the top of the window reads `OFFLINE` in the
dim grey token (`theme::TEXT_DIM`, app.rs:1157) with the sub-line "No boost server"
(app.rs:1172). Directly under it sits a full-width, bright accent-purple button reading
`RESTART AS ADMINISTRATOR` (app.rs:1219-1232) and the caption "Administrator is required to
redirect game traffic" (app.rs:1255) - so the first thing a player sees in the disconnected
state is an elevation prompt, not their connection state. The Connection card still renders
the globe with the LAX relay marked active and still draws the previous session's RTT
sparkline even though `Relay —` is blank (app.rs:1324-1326), so the card claims live data
that is not live. Below that: the Boost Server card (auto-select combo + Manage), the Game
card, and a collapsed `More`. There is no visible way to quit or hide from the window itself
(see problem 10). Evidence: `.omo/evidence/gui-ux/sprint-a-disconnected.png`.

**Connected (CONNECTED).** The banner reads `CONNECTED` in accent purple - not in the
success green the site reserves for a healthy signal (`theme::ACCENT`, app.rs:1155) -
with the sub-line "Not boosting - <n> ms" whose colour is the RTT quality ramp
(`rtt_colour`, app.rs:1817-1825). The same `RESTART AS ADMINISTRATOR` button and caption as
in the offline state sit below it, byte-identical, so the two states are distinguishable only
by the banner. The Connection card then shows the globe with the active relay highlighted,
`Relay  207.246.106.36:4434` in monospace, `Relayed  <packets> packets | <n> sessions` (both
monospace, app.rs:1342-1345) and a 64 px RTT plot (app.rs:1349-1372). A player reads: purple
word, orange millisecond count, purple button, globe. Evidence:
`.omo/evidence/gui-ux/sprint-a-connected.png`.

**Boosting / reachable.** Not captured: reaching `BOOSTING` requires elevation. The binary
launched from this shell runs unelevated, the app therefore never offers the boost action, and
`Capture backend (pcap mode): not detected` is reported in the Advanced card; a manual boost
attempt from the Advanced form did not start (the form is shown in
`.omo/evidence/gui-ux/sprint-a-advanced-panel.png`). From the source, the state is designed as:
headline `BOOSTING` in success green (app.rs:1153), sub-line `<Game> - <rtt> ms` (app.rs:1164-1167),
and the primary button swaps to `■  STOP BOOST` filled with the danger token (app.rs:1216,
1784-1815 `primary_button`). The reachable/unboosted variant of this axis is the captured
`CONNECTED` screen above: the relay is up (`Relayed` counters advancing, RTT plot live) while
the banner deliberately says "Not boosting". Because the boost path is the product's core
action and it is the one state this audit could not photograph, the design sprint should treat
"what does BOOSTING look like" as an open item, not as verified.

**Game section.** Scroll one notch below Boost Server and the Game card is the only place the
player can act on *what* is being boosted: a "Game found: <name>" row when auto-detect hits
(app.rs:766-771) or the dim "No game running - select your game and click Boost" + `Rescan`
pair (app.rs:773-786), a `Game:` combo (default `Fortnite`), and the `Reliability Shield -
recover lost packets (+25% data)` checkbox (app.rs:810-828) whose entire explanation lives in a
hover tooltip. Nothing here is state-coloured: the card is visually identical whether a game is
detected, is not, or is irrelevant. Evidence:
`.omo/evidence/gui-ux/sprint-a-game-section.png` (also visible in the tall settings capture).

**Narrow ~320 px.** The window's hard minimum is 320x400 (`main.rs`, `with_min_inner_size`), and
the layout narrows by swapping the globe to 104 px and tightening margins (`narrow` flag on
`available_width() < 380.0`, app.rs:1046-1048). Everything else is left to egui's flow, and it
breaks: the GAME card's "No game running - select your game and click Boost" is clipped
mid-word at the right edge instead of wrapping, "Reliability Shield - recover lost packets
(+25% data" loses its closing text, and the Boost Server row wraps `Manage` onto a second line
under the combo. The state banner and the primary button survive the narrow case intact.
Evidence: `.omo/evidence/gui-ux/sprint-a-narrow-320.png`.

## Screenshots

Fresh - captured in this session (2026-10-04) from the locally built debug binary on `master`
@ `4e0ebb7`, saved under `.omo/evidence/gui-ux/`:

| Path | What it shows |
|---|---|
| `.omo/evidence/gui-ux/sprint-a-connected.png` | Default width (~425 px client). `CONNECTED` banner, `Not boosting - 289 ms`, purple `RESTART AS ADMINISTRATOR`, Connection card with globe, relay address, relayed counters, RTT plot, Boost Server combo, Game card. |
| `.omo/evidence/gui-ux/sprint-a-disconnected.png` | Default width, after "Disconnect from relay". `OFFLINE` / "No boost server", same purple CTA, Connection card showing `Relay —` while still drawing the stale RTT curve and the LAX-active globe marker. |
| `.omo/evidence/gui-ux/sprint-a-game-section.png` | Default width, config column scrolled onto the GAME card: "No game running - select your game and click Boost" + Rescan, `Game: Fortnite`, Reliability Shield checkbox, collapsed `More`. |
| `.omo/evidence/gui-ux/sprint-a-settings-bottom.png` | Default width, `More` expanded: ADVANCED, PRIVACY cards revealed inside the "More" disclosure. |
| `.omo/evidence/gui-ux/sprint-a-advanced-panel.png` | Window widened to ~599 px, Advanced form open: the off-token dark inset frame, hint-only Server field, clipped "Custom Port Range" text, disabled `▶ Start Boost (manual)`, `Capture backend (pcap mode): not detected`. |
| `.omo/evidence/gui-ux/sprint-a-narrow-320.png` | ~320 px client width, CONNECTED: globe shrunk to 104 px, `Manage` wrapped under the combo, GAME/Reliability Shield text clipped at the right edge. |

Prior-session captures (Oct 2-3, 2026; **not** refreshed by this audit - listed because they are
the only evidence for those moments, and they show an older palette in several cases):

| Path | What it shows |
|---|---|
| `.omo/evidence/gui-ux/01-titlebar-runtime-icon-OK.png` | Title-bar runtime icon after the icon fix. |
| `.omo/evidence/gui-ux/02-taskbar-GENERIC-icon-BUG.png` | The generic taskbar icon bug. |
| `.omo/evidence/gui-ux/03-exe-icon-after-fix.png`, `04-exe-icon-final.png` | Exe-level icon verification. |
| `.omo/evidence/gui-ux/05-after-layout-refresh.png` | Window after the layout refresh. |
| `.omo/evidence/gui-ux/11-split-status-window.png` | Older split status/settings window (settings as a separate window). |
| `.omo/evidence/gui-ux/12-installed-taskbar-icon.png`, `13-tray-icon-area.png`, `14-taskbar-full.png`, `14-tray-icon-fixed.png` | Installed-build taskbar/tray icon checks. |
| `.omo/evidence/gui-ux/20-themed-single-window.png` | First single-window themed build. |
| `.omo/evidence/gui-ux/21-themed-primary-fixed.png` | Connected state with the older amber "Relayed" values. |
| `.omo/evidence/gui-ux/22-globe-with-geo.png` | Connected state with the geolocated globe. |
| `.omo/evidence/gui-ux/30-reordered-default.png` | Reordered default window (a stray Bulk CMD window overlaps the capture). |
| `.omo/evidence/gui-ux/31-narrow-320.png`, `41-narrow-320.png` | Earlier 320 px narrow captures. |
| `.omo/evidence/gui-ux/32-installed-reordered.png`, `50-installed-final.png` | Installed build, reordered/final layout. |
| `.omo/evidence/gui-ux/40-globe-land-merged.png` | Globe land-merge iteration. |
| `.omo/evidence/gui-ux/baseline-titlebar.png`, `baseline-taskbar.png`, `baseline-taskbar-buttons.png`, `baseline-taskbar-zoom.png` | Pre-fix baselines. |

## Inline color literals

`grep -n 'Color32::from_rgb' client-gui/src/app.rs` returns **18** occurrences: **10** are the
token definitions inside `mod theme` (app.rs:1624) and **8** are inline literals at their use
sites. The skill's budget is "roughly 20 inline literals"; the real inline count is 8, and all
8 are debt - none of them resolves to a name in the token module.

| # | Line | Literal | Where / role | Verdict |
|---|---|---|---|---|
| 1 | 749 | `egui::Color32::from_rgb(255, 190, 60)` | "Relay refresh failed - using the saved list" warning label | Off-token amber; nearest token `WARN #fdcb6e` - replace |
| 2 | 856 | `egui::Color32::from_rgb(25, 25, 35)` | Fill of the Advanced manual-boost inset frame | Sole literal darker than `BG`; needs a `SURFACE_SUNKEN` token |
| 3 | 898 | `egui::Color32::from_rgb(220, 90, 90)` | `text_color` of the Custom Port Range field when invalid | Duplicate of #4; nearest token `BAD #ff6b6b` - replace |
| 4 | 903 | `egui::Color32::from_rgb(220, 90, 90)` | "invalid" label next to the same field | Duplicate of #3 - one input error, two literals |
| 5 | 914 | `egui::Color32::from_rgb(40, 90, 55)` | Fill of the enabled `▶ Start Boost (manual)` button | A second, darker green that is not `OK #00d68f` |
| 6 | 916 | `egui::Color32::from_rgb(60, 60, 60)` | Fill of the disabled `▶ Start Boost (manual)` button | Neutral grey, not `SURFACE_RAISED`/`BORDER` |
| 7 | 936 | `egui::Color32::from_rgb(220, 130, 50)` | "Enter a valid IP:port" error label | A second amber, different from #1 and from `WARN` |
| 8 | 1470 | `egui::Color32::from_rgb(220, 80, 80)` | Proxy Manager `config_error` label | A third red, different from #3/#4 and from `BAD` |
| 9 | 1629 | `Color32::from_rgb(0x0A, 0x0A, 0x1A)` | `theme::BG` | Token definition (= site `--bg-1`) |
| 10 | 1630 | `Color32::from_rgb(0x10, 0x10, 0x24)` | `theme::SURFACE` | Token definition (= `--bg-2`) |
| 11 | 1631 | `Color32::from_rgb(0x16, 0x16, 0x2E)` | `theme::SURFACE_RAISED` | Token definition (= `--bg-3`) |
| 12 | 1632 | `Color32::from_rgb(0x23, 0x23, 0x3F)` | `theme::BORDER` | Token definition (= `--border-1`) |
| 13 | 1634 | `Color32::from_rgb(0xF2, 0xF2, 0xFA)` | `theme::TEXT` | Token definition (= `--text-1`) |
| 14 | 1635 | `Color32::from_rgb(0x8A, 0x8A, 0xA8)` | `theme::TEXT_DIM` | Token definition (= `--text-3`) |
| 15 | 1637 | `Color32::from_rgb(0x6C, 0x5C, 0xE7)` | `theme::ACCENT` | Token definition (= `--accent`) |
| 16 | 1638 | `Color32::from_rgb(0x00, 0xD6, 0x8F)` | `theme::OK` | Token definition (= `--signal`) |
| 17 | 1639 | `Color32::from_rgb(0xFD, 0xCB, 0x6E)` | `theme::WARN` | Token definition (= `--warn`) |
| 18 | 1640 | `Color32::from_rgb(0xFF, 0x6B, 0x6B)` | `theme::BAD` | Token definition (= `--danger`) |

The 8 inline literals carry only **four** distinct colours (two reds that are nearly identical,
two ambers, one green, one grey), which is the tell that they were written ad hoc rather than
drawn from a scale.

## Token inventory

`mod theme` (app.rs:1624-1815, one module, no separate `design.rs` yet) against the site's
canonical scale (`web/styles.css` `:root`).

Colour:

| App token | App value | Site variable | Site value | Status |
|---|---|---|---|---|
| `BG` | `#0a0a1a` | `--bg-1` | `#0a0a1a` | matches |
| - | - | `--bg-0` | `#07070f` | missing in app |
| `SURFACE` | `#101024` | `--bg-2` | `#101024` | matches |
| `SURFACE_RAISED` | `#16162e` | `--bg-3` | `#16162e` | matches |
| - | - | `--bg-4` | `#1d1d3a` | missing in app |
| `BORDER` | `#23233f` | `--border-1` | `#23233f` | matches |
| - | - | `--border-2` | `#2f2f52` | missing in app |
| `TEXT` | `#f2f2fa` | `--text-1` | `#f2f2fa` | matches |
| - | - | `--text-2` | `#b4b4cc` | missing in app (three text levels collapse to two, DESIGN.md rule 4) |
| `TEXT_DIM` | `#8a8aa8` | `--text-3` | `#8a8aa8` | matches |
| `ACCENT` | `#6c5ce7` | `--accent` | `#6c5ce7` | matches |
| - | - | `--accent-strong` / `--accent-deep` / `--accent-text` | `#7d6ff0` / `#5a4bd4` / `#a29bfe` | missing in app (no hover/pressed/text-on-accent ramp) |
| - | - | `--accent-soft` / `--accent-line` | `rgba(108,92,231,.12)` / `.28` | missing in app |
| `OK` | `#00d68f` | `--signal` | `#00d68f` | matches |
| `WARN` | `#fdcb6e` | `--warn` | `#fdcb6e` | matches |
| `BAD` | `#ff6b6b` | `--danger` | `#ff6b6b` | matches |
| - | - | `--info` | `#00cec9` | missing in app (no neutral-fact colour) |
| - | - | `--signal-soft` / `--info-soft` / `--warn-soft` / `--danger-soft` | 12% tints | missing in app (no semantic surface tints; state tint must be faked with `gamma_multiply`) |

Type (app `CAPTION 11 / LABEL 12 / BODY 13 / EMPHASIS 15 / HEADING 20 / DISPLAY 34`):

| App | px | Site | rem / px | Status |
|---|---|---|---|---|
| `CAPTION` | 11 | `--fs-2xs` | .6875 / 11 | matches |
| `LABEL` | 12 | `--fs-xs` | .75 / 12 | matches |
| `BODY` | 13 | `--fs-sm` | .8125 / 13 | matches |
| `EMPHASIS` | 15 | `--fs-base` | .9375 / 15 | matches |
| - | - | `--fs-md` | 1.0625 / 17 | missing in app |
| `HEADING` | 20 | `--fs-lg` | 1.25 / 20 | value matches - but `HEADING` is never referenced anywhere in app.rs |
| - | - | `--fs-xl` / `--fs-2xl` | 1.5 / 24, 1.875 / 30 | missing in app |
| `DISPLAY` | 34 | `--fs-3xl` / `--fs-display` | 2.375 / 38, clamp(2.4rem,5.4vw,3.75rem) | mismatch: 34 is not on the site scale |
| - | - | `--lh-tight` / `--lh-snug` / `--lh-body` | 1.08 / 1.3 / 1.65 | missing in app (no line-height control at all) |
| - | - | `--ls-tight` / `--ls-wide` | -.02em / .08em | missing in app (letterspacing; the ALL-CAPS eyebrows could use the wide one) |

Spacing (app `S1..S5 = 4 8 12 16 24`): site runs `--sp-1..--sp-24` = 4 8 12 16 **20** 24 **32 40 48
64 80 96**. The app carries the first four and 24 but skips 20 and 32, and stops at 24 - note
`client-gui/DESIGN.md` claims "six steps: `4 8 12 16 24 32`", which the code does not implement
(five constants, no 32). Ad-hoc sizes also leak in: `ui.add_space(4.0)`/`6.0` instead of tokens
(app.rs:826, 854, 909), a 46 px button height (app.rs:1235), 64 px plot (app.rs:1355), 104/132 px
globe (app.rs:1315).

Radii: app `R_INLINE 6`, `R_CARD 10`, `R_BUTTON 12`; site `--r-xs 4`, `--r-sm 8`, `--r-md 12`,
`--r-lg 16`, `--r-xl 24`, `--r-pill 999`. Only `R_BUTTON 12` is on the site scale - 6 and 10 are
off-scale, and a fourth value `4.0` appears inline on the Advanced inset frame (app.rs:857).

Elevation and motion: the site defines `--shadow-1/2/3`, `--glow-accent`, `--dur-fast/base/slow`
and `--ease-out`; the app defines **none** of these - hierarchy cannot be expressed with depth,
and every transition uses egui's default timing.

## Top 10 design problems

1. **Every content type is the same card.** `theme::card` (app.rs:1784-1802) is the single
   container for Boost Server, Game, Advanced, Privacy, Maintenance, About *and* the Connection
   panel - same `SURFACE` fill, same `BORDER` hairline, same `R_CARD` 10, same `S3` margin. A
   load-bearing status panel and a footnote sit at identical weight.
   Evidence: `.omo/evidence/gui-ux/sprint-a-settings-bottom.png` (six cards, one silhouette);
   app.rs:1784-1802.
   Anti-slop trait: *identical rounded cards for every content type, one border-radius for all
   hierarchy.*

2. **ALL-CAPS eyebrow above every section.** `title.to_uppercase()` in `theme::card`
   (app.rs:1793) stamps `CONNECTION`, `BOOST SERVER`, `GAME`, `ADVANCED`, `PRIVACY`,
   `MAINTENANCE`, `ABOUT` in the same 11 px dim semibold.
   Evidence: `.omo/evidence/gui-ux/sprint-a-connected.png`; app.rs:1792-1797.
   Anti-slop trait: *ALL-CAPS eyebrow labels above every section.*

3. **Templated string formulas.** Readouts are built as "WORD - fragment" with a spaced dash
   (`format!("{} - {:.0} ms", …)` app.rs:1164-1167, `"Not boosting - {:.0} ms"` app.rs:1169,
   `"Reliability Shield - recover lost packets (+25% data)"` app.rs:811,
   `"Advanced - set server manually"` app.rs:838-840), and machine-ish facts are joined by a
   separator rule (`Relayed <n> packets | <n> sessions`, app.rs:1342-1346).
   Evidence: `.omo/evidence/gui-ux/sprint-a-connected.png`; app.rs:811, 838-840, 1164-1171, 1342-1346.
   Anti-slop trait: *titles built as "WORD - fragment" with a spaced dash; meta strings joined
   with middle dots (here, a pipe/rule).*

4. **Monospace as costume, on top of a flat type scale.** Every value is monospace at 12 px -
   the relay address (legitimately an ID, app.rs:1327) but also packet/session counters
   (app.rs:1342-1344) which are plain numbers - while `HEADING 20` is defined and used zero
   times and `EMPHASIS 15`/`CAPTION 11` appear once each. The product's proof numbers therefore
   have exactly the weight of their labels.
   Evidence: `.omo/evidence/gui-ux/sprint-a-connected.png`; app.rs:1342-1345, 1642-1647, and the
   `HEADING` reference sweep (0 hits).
   Anti-slop trait: *monospace used for small labels as a costume rather than for data.*

5. **Default egui chrome survives where custom chrome exists.** The `More` disclosure renders
   egui's stock collapsing-header triangle and default header treatment (app.rs:832, no custom
   header); both combos render default dropdown chrome (app.rs:735, 800); the RTT `Plot` keeps
   egui_plot's default grid/frame (app.rs:1349-1372); the window itself is the OS default title
   bar. Next to the themed cards this reads as "default egui with colours changed".
   Evidence: `.omo/evidence/gui-ux/sprint-a-connected.png`, `.omo/evidence/gui-ux/sprint-a-narrow-320.png`.
   Anti-slop trait: *egui-specific tell: leaving default widget chrome wherever custom chrome
   already exists.*

6. **Token debt: eight inline literals and an 11-variable hole in the palette.** The literals in
   the table above (lines 749, 856, 898, 903, 914, 916, 936, 1470) carry four distinct ad-hoc
   colours, including three near-identical reds and two different ambers that match neither
   `BAD` nor `WARN`; the Advanced inset frame (app.rs:856) is the only surface darker than `BG`.
   Meanwhile the site's `--bg-0`, `--bg-4`, `--border-2`, `--text-2`, `--accent-strong`,
   `--accent-deep`, `--accent-text`, `--accent-soft`, `--accent-line`, `--info` and all four
   `*-soft` tints have no app counterpart.
   Evidence: app.rs:749, 856, 898, 903, 914, 916, 936, 1470; `.omo/evidence/gui-ux/sprint-a-advanced-panel.png`
   (the off-token dark inset).
   Anti-slop trait: no listed tell - violates the skill's tokens-only / site-is-canonical
   policy and the mandate that every colour comes from a token.

7. **The loudest control is identical in every state, and it is not the product's action.**
   `CONNECTED` and `OFFLINE` both render the same full-width bright accent button reading
   `RESTART AS ADMINISTRATOR` (app.rs:1215-1240), with the same caption repeated underneath
   (app.rs:1255) - text that duplicates the button's own hover text. The one glance that should
   answer "am I boosted / am I safe" instead lands on an elevation prompt that never changes,
   and the actual `BOOST MY GAME` label is unreachable unelevated.
   Evidence: `.omo/evidence/gui-ux/sprint-a-connected.png` vs
   `.omo/evidence/gui-ux/sprint-a-disconnected.png` (identical primary row); app.rs:1215-1240, 1255.
   Anti-slop trait: no listed tell - violates the doctrine's "state legibility" priority and
   "spend boldness in one place / exactly one primary action".

8. **The disconnected screen claims live data.** With the tunnel down and `Relay —` blank
   (app.rs:1324-1326), the Connection card still draws the previous session's RTT sparkline
   (guarded only by `!rtt_history.is_empty()`, app.rs:1349) and still highlights LAX as the
   active node on the globe (app.rs:1290-1305), so the card contradicts the `OFFLINE` banner two
   rows above it.
   Evidence: `.omo/evidence/gui-ux/sprint-a-disconnected.png`; app.rs:1290-1305, 1324-1372.
   Anti-slop trait: no listed tell - violates "glanceable truth" (the primary visual job) and
   the accessibility floor of never letting colour/graphics state contradict the text state.

9. **Half the window's controls hide inside one disclosure, with dead space below.** `Privacy`,
   `Maintenance` (including `Disconnect from relay`) and `About` are all nested inside the
   collapsed `More` header (app.rs:832-1035, a brace-region the collapsed screenshot confirms:
   expanding `More` reveals Advanced, then Privacy, then Maintenance, then About), so telemetry
   opt-out, update check, relay disconnect and the app's identity are four levels down the column
   while ~400 px of the window sits empty.
   Evidence: `.omo/evidence/gui-ux/sprint-a-settings-bottom.png`, `.omo/evidence/gui-ux/sprint-a-connected.png`
   (the same window with `More` collapsed); app.rs:832-1035.
   Anti-slop trait: no listed tell - violates the doctrine's hierarchy/restraint ("cut any
   decoration that does not serve glanceable truth") and the "no dead ends" intent of the
   primary-action rule.

10. **The bottom action row never renders, and 320 px clips text.** `ScrollArea::vertical().
    auto_shrink([false, false])` (app.rs:1381-1387) consumes the entire remaining column, so the
    footer built immediately after it - `Hide to tray` / `Open log file` / `Quit`
    (app.rs:1390-1404) - is squeezed to zero height; it is absent at 800 px, 1299 px and 1394 px
    window heights, leaving the tray controls reachable only via the tray icon. At the 320 px
    minimum the GAME card and Reliability Shield lines are clipped mid-sentence instead of
    wrapping (app.rs:773-786, 810-828).
    Evidence: `.omo/evidence/gui-ux/sprint-a-connected.png` and
    `.omo/evidence/gui-ux/sprint-a-narrow-320.png` (no footer anywhere; clipped copy);
    app.rs:1381-1404.
    Anti-slop trait: no listed tell - violates the skill's hard constraints ("must work at
    320 px minimum"; the tray and boost flow are load-bearing).
