# LightSpeed GUI — three visual directions (redesign round)

**Task:** `task-97` · **Owner:** design-system-2 · **Date:** 2026-11-21 · **Worktree:** `.agents/worktrees/redesign-visual` (branch `redesign/visual`, master `da56ff7`)
**Status:** rendered option set for an owner decision — **design/evidence only, no product code, no commit/push.** · **Cost:** $0.00 (Chrome/Edge render, bundled OFL fonts, no new deps)

The owner rejected the accepted Instrument 2.0 visual outcome as *"looks exactly the same as before"* and asked for a real visual-redesign round; 1.12.0 stays untagged until a direction is accepted. This document presents **three genuinely distinct directions** at the binding 320 px mobile-only form, each rendered as static client-area mockups of the same screens. The owner is choosing **look-and-feel**, not features: every direction carries the same information architecture, the same catalog keys and the same state contract in §2.

---

## 1. How to view the mockups

Evidence root: `wat/evidence/2026-11-21/redesign-visual/`
Contact sheets (start here):

- `overview.png` — the three directions side by side (`CONNECTED` + `NO RETURN PATH`)
- `a-instrument-refined/sheet.png` — A, all states, 760 and 420
- `b-quiet-native/sheet.png` — B light, all states, 760 and 420
- `b-quiet-native/sheet-dark.png` — B dark mode, all states (760)
- `c-broadcast-console/sheet.png` — C, all states, 760 and 420

Per-direction captures (`<state>-<height>.png`, 320×760 / 320×420 logical, rendered at 2× = 640×1520 / 640×840 PNG; client area only, **no OS window chrome**):

| direction | directory | states rendered |
|---|---|---|
| A. Instrument, refined | `a-instrument-refined/` | connected, optimizing, starved, error, settings × {760, 420} |
| B. Quiet native | `b-quiet-native/` | the same ten in light **plus** `dark-<state>-760.png` for all five states |
| C. Broadcast console | `c-broadcast-console/` | the same ten |

**Viewport / right-edge proof (task-100).** The UX review flagged B as clipped on
the right. The B fixtures were re-rendered with a hardened viewport — an inline
`html,body{margin:0;padding:0;width:320px;overflow:hidden}` reset ahead of the
stylesheet, `border-box` on every box, and a 1 px outer window gutter so the
full-bleed status strip/tab bar never paint the device edge — and every capture
was verified from its pixels: **all 35 captures are 320 logical px (640 @2x),
the viewport is flush (B's full-bleed chrome begins within 2 px of both sides; a
stray 8 px body margin would inset it by ≥16), and no panel, control, glyph or
value reaches the outer 8 px.** B edge-ink is **0.000** (the review measured
0.10–0.14 on the old captures). Proof: `edge-clip-check.md` and 15
`b-quiet-native/right-edge/*.png` crops.

Fixtures and the renderer live in `wat/evidence/2026-11-21/redesign-visual/mockups/` (`dir_a.py`, `dir_b.py`, `dir_c.py`, `build_common.py`, `build.py`; HTML/CSS emitted per screen). Re-render with headless Chrome while a local server serves the worktree (`py -3 -m http.server 8765 --bind 127.0.0.1`, then `py -3 …/mockups/build.py`). Chrome 157, `--headless=new`, `--force-device-scale-factor=2`.

All copy is the real English catalog (`client-gui/locales/en.toml`); metrics/counters/addresses are visible sample values (e.g. `218 ms`, `203.0.113.42:27015` — a documentation address). **No map point is fabricated for the game server**: the shipped app only knows the relay city, so every map shows the relay fleet and the active relay, never an invented server location.

---

## 2. The shared information architecture (identical in all three)

| region | content (catalog keys) |
|---|---|
| **Top/status rail** | state word (`state.*`), relay-leg metric (`route.relay_rtt` / `route.leg_relay`), one-line status sub-line (`route.relay_ready_named`, `route.relaying_route`, warnings) |
| **Route visual** | relay-fleet map with active relay (Natural Earth 110m land from `client-gui/data/world-land.bin`; relay coordinates from `globe.rs`) |
| **Route data** | Relay (`route.relay`), Relay RTT (`route.relay_rtt`), Game server (`route.game_server` / `route.place_hint`), Relayed (`route.sent_returned` / `route.count_hint`), and while boosting Path trace (`route.route_trace_hops`) + Expected path (`route.expected_path`); Location (`route.location_*`) |
| **Controls** | Relay + Game selectors (`route.relay` / `route.game`, `relay.auto_fastest`, `game.auto_named`), game-found line (`game.found`) |
| **Primary action** | one slot, three labels (`action.optimize_route` / `action.stop_optimizing` / `action.restart_admin_optimize`), warning ink while boosting |
| **Footer** | Hide to tray, Settings, Quit (`window.hide_to_tray`, `settings.title`, `action.quit`) |
| **Warn-only starvation** | `state.not_returning` + `route.relay_not_returning` banner, counters show `sent > 0 · returned 0`, no automatic route claim |
| **Error** | `state.error`, `error.tunnel` + Details / `settings.open_log` |
| **Settings sheet** | Language (`settings.language`), Routing (`settings.remeasure`, starvation policy), Privacy (`settings.share_stats` + the **Q13 disclosure**, `update.disclosure`), Maintenance (`settings.open_log`, capture status), About (`about.version`, `about.blurb`) |

**Settings IA parity (task-100).** The Settings captures restyle — never drop — the
shipped shell's sheet (`client-gui/src/app.rs:3542-3800`): **Language** (System
default), **Startup** (autostart, off by default + hint), **Privacy** (share
stats toggle + the Q13 update-check disclosure), **Routing** (the warn-only
starvation disclosure in full), **Relays** (saved count, re-measure, refresh),
**Maintenance** (check for updates, open log), **Components** (the shell's
literal diagnostic rows: Version, Elevation, Capture, Npcap, Relays), and
**About** (version + blurb). The footer shows the normal tray-mode trio —
**Hide to tray / Settings / Quit** — in every direction. All eight sections are
visible in the 760 Settings captures in all three directions; the 420 captures
show the same IA in the sheet's scroll state (the sheet scrolls by design).

Screens rendered: **CONNECTED**, **OPTIMIZING (boosting)**, **STARVED `NO RETURN PATH`**, **ERROR**, **Settings**, each at 320×760 and the 420 compact height (plus CONNECTED compact, which is the 420 row). Reduced motion: every direction describes a static fallback; no direction requires animation to be legible.

---

## 3. Direction A — Instrument, refined

**Intent.** Keep the product's dark instrument identity and finish it: the same rail/metrology idea, but with a calm accent, one dominant state word, a true hierarchy (state → metric → route data), crafted surfaces (hairline + inner-light layering), and a map drawn from the shipped Natural Earth data with a composed glow and a single active-relay focus. "Same family, finally finished."

**Mockups:** `a-instrument-refined/` (`connected|optimizing|starved|error|settings` × 760/420).

**Palette (dark only).** Near-black blue-charcoal planes, one violet accent reserved for the action, one signal green, quiet danger.

| token | hex | role |
|---|---|---|
| bg0 / bg1 / bg2 / bg3 | `#08090D` / `#0D0F15` / `#12151D` / `#181C26` | canvas / panel / control well / raised |
| line / line2 | `#242938` / `#333B4E` | decorative hairline / control boundary (3.2:1+, AA 1.4.11) |
| ink / ink2 / ink3 | `#F1F3F9` / `#A9B1C5` / `#7C879E` | primary / secondary / tertiary ink |
| signal | `#5BE3A6` | connected / optimizing state |
| warn / danger | `#F0B45F` / `#FF8585` | transient warning / fault + stop action |
| accent / accent2 / on-accent | `#8B7CF6` / `#6C5CE0` / `#0A0912` | the one affirmative action |
| control-line | `#5A6478` | select/close boundaries |

**Type.** Inter Regular/SemiBold/Bold (bundled 4.001) + JetBrains Mono Regular (bundled 2.211). State word **Inter 700 30 px** uppercase, tracking −0.014em, one line (D6-safe; the metric moved to its own readout row instead of sharing the line — the §4.2 F1 fix). Metric: **JBMono 38 px** + 12 px unit + 9 px uppercase caption. Body 12.5/13 px; labels 9–10 px uppercase tracking 0.16em; values 13–15 px; mono for numbers/addresses only.

**Spacing / radius / elevation.** 4-px rhythm (4/5/6/7/9/10/12/14/16 px); radius 10 (controls), 12 (action), 14 (cards/map); elevation = 1 px hairline + 3 % top inner light + a soft ambient drop shadow on the action only; no blurred chrome. Map atmosphere = layered strokes (a wide low-alpha glow stroke under a crisp 1 px coastline) — no Gaussian blur, which egui cannot paint.

**Motion intent.** 160 ms ease-out state-word/ink cross-fade; the metric swaps without a roll; a 1.6 s breathing ring on the active relay only while boosting; reduced-motion → static ring and instant swaps.

**Hero / state rail.** Brand row, state word with a toned state dot (green/red), then a readout row: sub-line left, 38 px relay-leg metric right. The state word never competes with the metric for width, so long localized words fit. Rail uses a faint violet top wash + a 1 px rule.

**Globe / card.** Equirectangular Natural Earth 110m silhouette (the real shipped data), graticule, eight fleet markers at their real coordinates, active relay as a white-core/ring marker with an `LAX` tag; route card = eyebrow + status chip + label/value rows with hairlines; controls card = label + select well; map footer = city + fleet count.

**Button hierarchy.** One filled violet gradient primary (46 px, 12 px radius); the destructive state is quiet — 1 px danger outline, danger ink, painted 10×10 stop square, no red fill. Footer buttons are quiet text.

**What differs perceptually.** The closest to today's product, but visibly denser and more composed: larger metric readout, one accent instead of mixed green/red shout, real map with land glow, aligned data rows, chips and hairlines, full 760 layout without clipping.

**Implementation reality (task-99 feasibility).** **Lowest cost/risk of the three and fine as drawn.** It evolves the existing dark token set and the current egui widget language: a `Visuals` refresh, token additions (`control-line`, gradient stops, glow alpha), the state-word/metric layout split already mandated by §4.2/F1, and the map drawn with two `Shape::line` passes over the existing `world-land.bin` (dots/markers already implemented in `globe.rs`). No canon reopen: dark-first stays, shadows stay off (the action's soft shadow is the one exception to price), no new font family, no new runtime dependency. It does **not** reopen light mode, and it keeps the 30 px state role. Residual work is polish-level (spacing/typography pass over the shell and cards), not a re-architecture. **Contrast: full actual-use audit passes (A palette: 29 pairs, 0 failures).** The Settings capture now shows all eight shipped sections. Risk: being "same family" it may still read as too close to Instrument 2.0 if the owner wanted a clear break — the contrast with B/C makes that visible.

---

## 4. Direction B — Quiet native

**Intent.** A calm, trustworthy cross-platform utility: light-first with a proper dark mode, warm-neutral surfaces, system-feeling typography and soft elevation, status as a clear tinted strip, and the route as a clean grouped list. The window should feel like a native system app that happens to be a game-network tool, not a instrument panel.

**Mockups:** light set in `b-quiet-native/sheet.png`; dark set in `b-quiet-native/sheet-dark.png` (all five states rendered in both modes).

**Palette — light.**

| token | hex | role |
|---|---|---|
| canvas / surface / surface2 | `#F5F5F2` / `#FFFFFF` / `#EEEEE9` | window / card / inset |
| line / control-line | `#E3E1DA` / `#86847C` | decorative hairline / control boundary (3.7:1 AA) |
| ink / ink2 / ink3 | `#1B1C1E` / `#5A5D64` / `#6B6F77` | primary / secondary / tertiary |
| accent / accent-soft / on-accent | `#2E6BE6` / `#E9F0FE` / `#FFFFFF` | single restrained accent, primary action, focus tint |
| ok / ok-soft / ok-line | `#1B8A57` / `#E4F3EB` / `#BFE3D1` | connected status strip |
| danger / danger-soft / danger-line | `#B23A2E` / `#FBEAE7` / `#EEC9C3` | fault strip, warning banner, stop |
| warn / warn-soft | `#8A5A00` / `#FAF0DC` | transient warning strip |

**Palette — dark.**

| token | hex | role |
|---|---|---|
| canvas / surface / surface2 | `#141517` / `#1C1D20` / `#232529` | window / card / inset |
| line / control-line | `#2D2F35` / `#757980` | hairline / control boundary |
| ink / ink2 / ink3 | `#F1F2F4` / `#A6A9B1` / `#8A8E98` | primary / secondary / tertiary |
| accent / accent-soft / on-accent | `#7FA6FF` / `#1D2942` / `#0E1A33` | accent, focus tint, primary action |
| ok / ok-soft | `#4ECB92` / `#163224` | connected status |
| danger / danger-soft | `#FF8A80` / `#38201E` | fault, warning, stop |
| warn / warn-soft | `#E2B066` / `#332717` | transient warning |

**Type.** Inter Regular/SemiBold/Bold only for UI (label-role JBMono remains for addresses, per catalog roles). Native-feeling scale: 10/11/12/13/15/17/20 px, sentence-case everywhere except the canon uppercase state word and action label; **state word 20 px SemiBold uppercase** with 0.055em tracking; tabular numerals for the metric (22 px); section labels 10 px uppercase over cards; rows are 44 px with a 13 px label and a right-aligned 13–13.5 px value.

**Spacing / radius / elevation.** 4-px rhythm with roomier gaps (4/8/12/16/20); radius 12 (cards/controls), 18 (sheet top); elevation = two soft shadows (`0 1px 2px`, `0 4px 14px` at ~5 % black) plus 1 px hairline; sheet gets a larger shadow and a scrim. Light and dark use separate shadow strengths.

**Motion intent.** Native, quiet: 120 ms ease-out for strip tint/state change, no bounce, no entrance animation; scroll content uses a soft bottom fade; reduced-motion → no tint transition beyond an instant swap.

**Hero / state rail.** A full-width tinted **status strip** as the hero: small `STATUS` label, state word + dot, one-line sub-line, relay-leg metric right; the strip takes the state tone (green connected, blue optimizing, warm/red fault) with its own hairline. It reads as a system banner, not a panel.

**Globe / card.** A compact 92 px light map inside the route card's top (same Natural Earth data, low-contrast coastlines, graticule, active relay dot) so location stays visual but quiet; below it a grouped list of label→value rows; the relay/game controls are a second grouped list with chevrons; cards float on the canvas with soft shadows.

**Button hierarchy.** One filled blue primary (48 px, 12 px radius, white label); the stop variant is a quiet white card with danger ink and hairline; footer is a native bottom bar of three text buttons; sheet rows use chevrons/links.

**What differs perceptually.** Day-and-night difference from A/C: light canvas, floating white cards, soft shadows, small calm type, a status banner, no dark instrument chrome. The window reads as a polished native utility rather than a game tool; dark mode keeps the same calm with deep-neutral surfaces.

**Implementation reality (task-99 feasibility).** **Not a cheap token swap — the most expensive of the three.** As a theme swap it would touch **441 compile-time token references** and require **two contrast tables** (light + dark) falling out of the current single dark table. It reopens canon decisions the owner must take: **light mode is explicitly out of scope in `design-system-2026.md` §9** (dark-first canon — this direction *is* that decision), **shadows are off in §5.3** (soft elevation re-enables them), the **brand/web divergence** (the site is dark), **follow-OS vs fixed light** (`follow_system` exists as a language concept; the theme would need an explicit `ThemePreference` policy), and the **state-word role would move 30 → 20 px** (a token value change that must be logged and its fit gate re-derived). It is kept in the set because the owner may want exactly this calm/native feel. **Rendering status (task-100):** both light and dark modes are rendered for all five states; the earlier right-edge clip report was fixed with the hardened viewport (inline reset + border-box + 1 px gutter) and is now pixel-verified flush with edge-ink 0.000. **Contrast: full actual-use audit passes (B light and B dark: 24 pairs each, 0 failures after the `ink3` token adjustments).**

---

## 5. Direction C — Broadcast console

**Intent.** A dark, technical/editorial console for a game-network instrument: mono type as the voice, a high-contrast data rail, numbered sections and hairline rules, a dot-matrix telemetry map built from the shipped Natural Earth data, and **exactly one neon accent reserved for the primary action**. Distinctive without anti-cheat/hacker tropes: no glitch, no scan-line gimmick, no skulls, no fake telemetry — only the facts the client can observe.

**Mockups:** `c-broadcast-console/` (`connected|optimizing|starved|error|settings` × 760/420). **The state word is capped at 30 px** per the task-99 feasibility amendment (see below); identity comes from the mono/data-rail system, not an oversized word.

**Palette (dark only).** Near-black with a cool data highlight and a single acid-lime CTA.

| token | hex | role |
|---|---|---|
| bg / panel / panel2 | `#050607` / `#0A0C0E` / `#0D1113` | canvas / panel / inset |
| line / line2 / control-line | `#1D2329` / `#2B333B` / `#58656E` | rules / hairlines / control boundary (3.3:1 AA) |
| ink / ink2 / dim | `#F0F5F8` / `#A9B4BD` / `#79838E` | primary / secondary / tertiary (all AA on panels) |
| ice / ice-dim | `#9FE8FF` / `#4E7F8C` | data highlight, "live" indicator, links |
| warn / danger | `#FFB13D` / `#FF5C5C` | transient warning / fault |
| neon / on-neon | `#CDFF45` / `#0A0E02` | **the only neon: the primary action** |

**Type.** JetBrains Mono Regular is the UI voice (labels/small caps 8–9 px with 0.19–0.24em tracking, values 10.5–12.5 px); Inter Regular is used only for prose sentences (banner body, settings notes). State word **JBMono 30 px** uppercase with tight tracking (compact 30 px); metric readout 28 px mono in `ice`; section indices `01 / 02`; one weight only (no faux bold).

**Spacing / radius / elevation.** Tight technical rhythm (4/6/8/10/14 px); radius 2–3 px (panels/rules) and 3 px on the CTA; **flat** — elevation is the 1 px rule, never a shadow; `bg0`-black canvas with `panel` plates. The telemetry map is precomputed dot positions (Natural Earth land sampled on a grid) drawn as circles; fleet nodes are small crosses, the active relay a ring + label.

**Motion intent.** Console-instant: state swaps paint immediately, no easing; the only motion is a 1 Hz blink on the `LIVE` indicator and a single pulse on the active relay; **reduced-motion → both static**. No ticker, no glitch, no scan line.

**Hero / state rail.** Top bar (brand, `LINK ACTIVE/FAULT`, version), then `STATE ● LIVE|FAULT`, the 30 px mono state word, and a readout row: sub-line left, relay-leg metric right in `ice` (metric on its own row, so long localized words fit); tick ruler under the block.

**Globe / card.** Dot-matrix world (shipped land data, dim land dots `#2A343A`) with graticule and eight fleet crosses; the route data is an editorial table (variable label column, right-aligned values, hairline rules), sections numbered `01 ROUTE` / `02 CONTROL`; no rounded cards, no shadows.

**Button hierarchy.** The single neon lime block (50 px, 3 px radius, dark label) is the **only** neon surface on the screen — the affirmative action; the stop variant is a danger outline with a painted square; footer is mono text links.

**What differs perceptually.** Night-black editorial console: mono everywhere, giant data numerals, rules and indices, a dot-matrix map, and one acid-lime control. It reads as a broadcast/telemetry instrument for game networking, clearly unlike A's layered soft instrument and B's native light utility.

**Implementation reality (task-99 feasibility + amendments applied).** Medium cost, with two amendments already reflected in the mockups:

- **State word capped at 30 px (D6).** As originally drawn (44 px / 35 compact) it was not buildable: EN `NO RETURN PATH` measures **312 px at 35 px and 396 px at 44 px**, **10/31** localized state words exceed the 288 px column at 35 px and **31/31** at 44 px. The mockups now cap it at 30 px and carry the identity through the data rail, mono/small-caps, rules, telemetry grid, contrast and the single accent.
- **No fixed 92 px label grid.** 35/108 localized labels exceed 92 px and sentence rows (`route.cannot_beat_direct`) run 350–502 px, so the row model is **variable**: `grid-template-columns: max-content minmax(0,1fr)` with a `.tr.sentence` stacked variant (label above, full-width value) — the fixtures implement it; only short-label rows appear in these mockups.
- Canon impacts to price: mono-dominant UI type at 8–9 px (readability must pass a rendered-size check), uppercase transform on data values (a style choice that departs from sentence-case copy), and the neon accent as the single bright control. No new runtime dependency; dot map is precomputed drawing. **Contrast: full actual-use audit passes (C palette: 26 pairs, 0 failures after the `ice-dim` token adjustment).** The Settings capture shows all eight shipped sections in the console styling (restyled, not dropped).

---

## 6. What the owner is choosing (perceptual differences at a glance)

| | A. Instrument, refined | B. Quiet native | C. Broadcast console |
|---|---|---|---|
| first impression | dark, layered, precise instrument | calm light-native utility | black editorial console |
| colour | near-black + violet accent, green signal | warm white/day + blue accent, soft tints | near-black + ice data + one lime CTA |
| type voice | Inter hero + mono numbers | Inter throughout, tabular numerals | mono everywhere, Inter for prose only |
| state word | 30 px Inter, toned dot, own row | 20 px strip label, tinted banner | 30 px mono, `LIVE/FAULT` kicker |
| map | crafted silhouette + glow, 84–106 px | quiet light mini-map, 92 px | dot-matrix telemetry, 146 px |
| surfaces | hairlines + inner light, radius 14 | white cards + soft shadows, radius 12–18 | flat plates + 1 px rules, radius 2–3 |
| action | violet gradient fill | blue fill | neon lime block (only neon) |
| price to build | **low** (evolves current dark system) | **high** (light+dark theme, 441 token refs, 2 contrast tables, canon reopen) | **medium** (new mono type roles, row model, dot map; D6-capped) |

---

## 7. Constraints compliance

- **Mobile-only 320 px pinned; 420–1400 height** — all mockups are 320 wide; 420 (compact) and 760 rendered; no desktop layout is proposed.
- **All existing states / copy / catalog keys** — every visible string maps to a catalog key in §2; no invented state, no new copy, no fabricated metric or location.
- **Warn-only starvation** — the STARVED state shows the banner, `sent > 0 · returned 0`, the relay still selected and `STOP OPTIMIZING`; no direction claims an automatic route change.
- **Q13 disclosure** — the "only update check is the one you ask for" disclosure is present in the Privacy section of every Settings screen.
- **AA contrast + reduced motion** — the audit now enumerates **every actually-rendered text and UI pair** (body, muted, micro-caption, state word, banner, control boundary, focus, accent/CTA, danger, composited tints) across A, B light, B dark and C: **103 audited pairs, 0 failures**. The first full pass found **5 failures** — B light/dark micro-labels on the status tints (4.40–4.42:1) and C's section index (4.42:1) — fixed by token changes: B light `ink3 #6B6F77→#63676F`, B dark `ink3 #8A8E98→#9498A2`, C `ice-dim #4E7F8C→#5F93A2`. The fail table and every pair are in `contrast-checks.md`; decorative separators carry no claim. Each direction states its static reduced-motion fallback.
- **egui/eframe implementable, no new runtime deps** — Inter + JetBrains Mono (already bundled), standard egui primitives (rects, strokes, text, painter circles/polylines, `Visuals`, `Shadow`). No blurs/glass/webview/new font family in any direction.
- **No logic change** — the mockups are static fixtures; the state source, warning model and copy are untouched.

---

## 8. Unverified / limits of this option set

- **English mockups only.** Localized long-word risk is stated (and C's state word was re-capped using the feasibility measurements), but DE/RU/JA/KO/ZH screens were not rendered; DE's `OPTIMIERUNG LÄUFT`, RU strings and CJK text must be spot-checked in the chosen direction before a build decision.
- **Static captures**: hover/focus/active/disabled/loading and the collapsed sheet are not pictured; motion is described, not recorded. The Settings sheet's 420 captures show its scroll state (the real sheet scrolls).
- **Verification is pixel-based, not DOM-introspected**: Chrome's Windows launcher does not deliver `--dump-dom` through a subprocess pipe, so the clip proof checks rendered pixels (viewport flush, 320 px width, no edge content) rather than computed layout. The inline viewport reset is belt-and-braces on top of the stylesheet reset.
- **No egui verification** — these are HTML/CSS renders, not a running GUI; the chosen direction needs a theme/shell implementation and rendered-window verification.
- **200 % display scaling** is not captured; every mockup is a 2× raster of the 1× logical layout, so DPI rounding is not exercised.
- **Light mode (B)** is a design-system canon reopening, not an accepted decision; **C's** mono type and uppercase data style, and **A's** soft action shadow, are proposals that need logged decisions if chosen.
- The LightSpeed brand wordmark/site palette is not re-branded by these mockups; A/C stay closest to the current brand, B deliberately diverges toward a system-utility voice.
