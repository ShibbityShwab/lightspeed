---
name: lightspeed-gui-design
description: Use when designing, redesigning, critiquing, auditing or implementing the LightSpeed desktop GUI (client-gui; egui/eframe) or its cross-platform presentation layer. Covers the 2026 desktop standards floor, the binding state contract, tokens/type/motion, egui constraints, the "logic in one place, GUI is a wrapper" rule, and screenshot-based verification. Load it before touching any GUI surface, wireframe or user-facing copy.
---

# LightSpeed GUI design (2026)

> Paths are relative to the repository root. `impeccable` (user-level) may be
> loaded for critique/audit vocabulary, but its detector and native guidance
> cover web/iOS/Android only; **this skill wins** for LightSpeed.

## 0. Authority and mission

Authority order: owner instruction > `AGENTS.md` + `wat/rules.md` > this skill >
`docs/design/*`. `docs/design/interface.md` is binding for state facts; when this
skill and it disagree about a fact, `interface.md` wins and this skill is
corrected.

The window is a **status instrument**, not a page and not a control panel. A
player alt-tabs mid-match and must answer three questions in about one second:
*is my traffic being boosted, through which relay, and is the link good?*
Starting/stopping the boost, choosing game and relay, and reading the proof
(round trip, packets, sessions) are secondary. Legibility outranks expression;
measurements outrank prose.

## 1. Non-negotiables

1. **State honesty.** Every displayed fact has exactly one driving source.
   Never show stale data, never claim what the engine cannot observe, never
   invent a state the engine cannot distinguish. An unreachable state is
   documented as unverified, not faked.
2. **All states, not the happy path.** Every control ships
   default/hover/focus/active/disabled/loading/error; every data view ships
   loading/error/empty/partial/ideal. A control that cannot show its own state
   is a defect (the old "Boost Server" pill cluster versus a dropdown whose
   selected text is the truth).
3. **Accessibility floor (WCAG 2.2 AA, adapted to native).** 4.5:1 text / 3:1
   UI contrast, measured not asserted; visible keyboard focus; keyboard-complete
   core path; no state carried by colour alone (always a word); minimum 24 px
   pointer target, 44 px preferred for the primary action; reduced motion only
   claimed when a real OS source is wired; layout survives the 9 locales and
   RTL mirroring.
4. **Tokens only.** `client-gui/src/design.rs` is the single token module. No
   new inline `Color32::from_rgb`/`from_rgba` at a use site; no magic spacing,
   radius or duration outside the scale. One radius per role, one surface per
   content role.
5. **No fabricated data or precision.** Relay names, cities, units, pings,
   sample counts and `last {n}s` ages must come from a real field. "Prior ~{ms}"
   is allowed only with its uncertainty token. No wireframe may show a value
   the shipped code cannot produce.
6. **One type scale, one voice.** Only the roles in `design.rs`. Active voice;
   sentence case; one job per sentence; a button label names its outcome and
   keeps the same name through the flow; errors say what happened and how to
   fix it; empty states invite the next action. No promises, dates or
   unmeasured "faster".
7. **Engine behaviour stays out of UI work.** Wire types, engine logic and
   capture belong to `rust-dev`; the GUI consumes the public API.

## 2. 2026 desktop floor

- **Platform-adaptive conventions.** Windows 11 (Fluent/WinUI: caption buttons,
  system accent, snap layouts, tray overflow, per-monitor DPI), macOS (HIG:
  traffic lights, menu bar, ⌘ shortcuts, no tray assumption), Linux (GNOME/KDE:
  differing decoration and tray models). The app may look like one product, but
  window controls, shortcuts and tray behaviour follow the host.
- **Window:** resizable within stated bounds, remembers size/position, restores
  to a visible monitor, respects 200 % scaling, survives live DPI/monitor
  changes, and never clips content. A pinned width is an implementation
  decision to revisit, not a law of the product.
- **Input:** complete keyboard path and tab order; Esc closes sheets; Enter
  confirms; no hover-only affordance; shortcuts are discoverable. Pointer hit
  targets never shrink below the floor on narrow windows.
- **Performance:** the UI thread never blocks (workers + channels, as
  `app/presence.rs` and `discovery.rs` already do); frame time is budgeted for
  integrated GPUs; startup is measured; no idle animation or busy-poll;
  battery-friendly.
- **Integration:** tray/menu actions mirror the window; notifications are
  actionable only; autostart is opt-in; updater shows progress, version and
  failure; logs reachable from the UI; uninstall leaves nothing behind.
- **Cross-platform is a claim with evidence.** Linux/macOS parity is a program
  requirement (owner decision, 2026-11-19) for the GUI and every non-capture
  flow, but it is reported in three separate tiers — build-verified,
  headless-verified, runtime-verified — and user-facing copy never promises a
  tier that has not been reached. Today the CI GUI job is Windows-only, the
  macOS backend is untested and the Linux tray is a stub: treat those as gaps,
  not features.

## 3. LightSpeed canon — do not relitigate without a decision

- **Product:** zero-cost network optimizer; transparent tunnel; no payload
  modification, no IP masquerade, no anti-cheat bypass; no "protected"/"VPN"
  badge; no claim that relaying is better than direct without a measurement.
- **State contract:** `docs/design/interface.md` (11 route states + server-leg
  states, each with one driving fact). Update it whenever the contract changes.
- **Tokens/type:** `client-gui/src/design.rs` — bg-0..bg-4, border-1/2,
  text-1/2/3, accent family, signal/info/warn/danger with soft tints, S1-S5,
  radii, and the CAPTION/LABEL/BODY/EMPHASIS/VALUE/TITLE/STATE/METRIC scale.
  Values are pinned by tests; change values only with a logged decision.
- **Fonts are fixed and bundled:** Inter Regular/SemiBold, JetBrains Mono
  Regular, plus CJK Noto faces (JP/KR/SC) ordered by locale. No new family
  without a decision, a licence and the cmap coverage test. Prefer painter
  shapes over Unicode glyphs: `▸`, `◆`, `▲` are not in Inter/JetBrains Mono.
- **Dark-first.** Light mode is a token swap and a redesign, not an inversion —
  and a decision, not a default.
- **The uppercase state word is canon** (the instrument's readout); it is not
  the "all-caps eyebrow" anti-pattern. Everything else is sentence case.
- **Nine locales.** Every string goes through `i18n.rs`; new keys need all
  locales or the documented English-fallback decision.
- **Current shell:** width pinned at 320 px in `main.rs`, height 420–1400,
  initial 760. Changing the viewport is a design decision with screenshots.
- **Elevation is real:** boosting is gated on administrator rights. One action
  slot, three labels — boost / restart as administrator / stop — driven by the
  existing gate; never draw an action that cannot act.
- **Single-instance mutex and elevation quirks:** see `lightspeed-windows`.

## 4. egui implementation constraints

Verify API details against `Cargo.lock` (currently eframe/egui 0.37) before
quoting anything below; these are the traps already measured in this repo.

- Every widget is ours to draw. Extend the global `Visuals` in
  `app/theme.rs`; do not leave stock egui chrome on a surface that has custom
  chrome. Shadows are already off — keep them off unless the system adds depth
  deliberately.
- `Frame::stroke` draws all four sides; there is no per-side stroke. Paint a
  rail edge with `Painter::vline`/`Shape`, not `Frame::stroke`.
- Shell: fixed rail + fixed action bar + one scrolling config region + fixed
  footer (`TopBottomPanel`/`CentralPanel` + `ScrollArea`), so the action can
  never be pushed off-screen.
- `egui::Modal` sizes to content and does not scroll: clamp the sheet and wrap
  its body in a `ScrollArea`.
- Popups/windows are invisible to accessibility tooling: a screenshot is the
  only proof of a collapsed control.
- There is no reduced-motion signal in egui; poll the OS or do not claim the
  feature. Motion is limited to the globe while boosting and focused; no
  entrance animation, no hover motion on non-controls.
- Two font families do not share a baseline in a horizontal layout; align rects
  and document the ≤1 px drift, or paint one manual text pass.
- Fonts must be installed before the first frame (`install_fonts` from the
  creation context); late binding panics.
- Use in-window sheets, one card style, one input style and one gutter; the
  themed combo chrome is global, so a per-widget "custom combo" claim is a
  no-op.

## 5. One logic, GUI is a wrapper (owner requirement)

- Business logic, session orchestration, the config schema, relay discovery and
  probing, game detection, profile resolution, status math and derived state
  live in the shared crates (`lightspeed-client`; add a core/app crate when the
  boundary needs one) — **not** in `client-gui/`. The CLI and the GUI consume
  the same API so a fix lands once and never drifts.
- `client-gui/` may own: view state (open sheet, scroll, hover), platform
  integration (tray, window, fonts, updater UX), rendering, i18n lookup and
  presentation-only formatting. Anything that answers "what is true about the
  session, route, game or config" belongs upstream.
- One config schema: CLI and GUI read/write the same model; UI preferences are
  a namespaced section, not a second file. Today `client-gui/src/config.rs` is
  a separate `config.toml` — a known drift to resolve, not a pattern to extend.
- A fact shown on two surfaces has one source passed down; never derive it
  twice. A number labelled "ping" is a defect; say what it measures and which
  leg.
- Shared behaviour has a test in the shared crate; GUI tests cover pure
  presentation helpers only. No hidden business rule under `#[cfg(windows)]` in
  a view.
- A drift guard (grep/test) fails when engine calls or business rules appear in
  `client-gui/` outside the approved adapter; exceptions are named.
- Migration is staged: inventory → move config model → move commands/status
  DTO → delete duplicate modules → point the CLI at the same API. No big-bang
  rewrite.

## 6. Audit protocol (BUILD and AUDIT modes)

**BUILD:** lock the system before the screens — tokens, type, spacing, radii,
states — then wireframes, then code. Never create a parallel token system when
one exists.

**AUDIT:** scope pass (surfaces, token layer, framework); systemic-smell pass
first (inline colours, magic px, stale facts, duplicate derivation, stock
chrome); then per-domain pass; rank findings by leverage, do not list them.

Severity ladder, adapted for a native desktop instrument:

| Severity | Definition |
|---|---|
| **Blocker** | A false state or data claim, an unreachable core action, a keyboard trap, or a core flow a screen-reader user cannot complete |
| **Critical** | WCAG AA failure with a workaround, missing error/loading/empty on a core flow, layout broken at 320 px or 200 % zoom |
| **Major** | Limited-scope AA failure, incomplete state coverage, colour-only state, systemic token violation, < 24 px target |
| **Minor** | Polish, drift, AAA/best-practice gaps |

Finding format: `[SEVERITY] <surface>: <problem>` / `Where:` / `Rule:` /
`Impact:` / `Fix:` / `Effort:`. End an audit with counts per severity, the top
three leverage fixes, and the checks **not** verified.

## 7. Anti-slop tells (2026)

Reject: identical rounded cards for every content type; one radius for all
hierarchy; decorative gradients or washes to fill space; an eyebrow label above
every section; `WORD — fragment` and `A · B · C` meta strings; monospace as
costume (mono only for data); fake precision or units; numbered markers where
the content is not a sequence; accent sprayed onto one word for no
informational reason; entrance animation on every element; hover motion on
things that are not controls; stock framework chrome anywhere custom chrome
exists; a "Protected/Boosted" badge over unmeasured data; a big red stop button
beside gameplay (a mis-click waiting to happen).

Protect the counter-signals: Inter + JetBrains Mono are deliberate; dark-first
is canon; the uppercase state word is canon; boldness is spent in one place
(the one bright action and the route). The generic gaming-app default —
neon purple/acid green, HUD grids, 3D hyperrealism, Russo One/Chakra Petch — is
the slop this product already rejected. The 2026-11-19 direction pass returned
exactly that default; treat it as a negative reference, not a brief
(`wat/evidence/2026-11-19/uiuxpm-direction.txt`).

## 8. Verify in a real window

- Use `scripts/dev-gui.ps1` (single-instance guard) and
  `scripts/install-gui.ps1`; elevation does not propagate into the agent shell
  (see `lightspeed-windows`).
- Capture the real window: the shipped widths (320 px is real today; wider only
  in a resizable build) × states (disconnected/idle, connected, boosting, game
  pick/empty, error/fallback when reachable). The UIA tree cannot see egui
  popups — read the screenshot.
- Save to `wat/evidence/<YYYY-MM-DD>/` and name the paths in the report.
- Gate: `cargo fmt --all --check`;
  `cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui`;
  `cargo test --workspace --all --exclude lightspeed-gui`;
  `cargo clippy -p lightspeed-gui --all-targets`; `cargo test -p lightspeed-gui`.
  Report counts per crate, never adjectives.
- An unreachable state is unverified: say so. Never inject samples to fake a
  measurement screenshot.

## 9. Change discipline

- Update `docs/design/interface.md` when the state contract changes; update
  this skill only when a rule changes; log the decision in
  `wat/state/decisions/<YYYY-MM>.md`.
- The October 2026 design set (`ui-audit.md`, `ui-design-critique.md`,
  `ui-design-plan.md`) is historical: pass 1 was **REVISE** with 16 required
  changes and was never accepted. Do not treat it as approved; supersede it
  with a dated, accepted spec.
- Prefer deleting a control over hiding it; prefer one obvious action over two
  competing ones; when the state contract and a layout disagree, the contract
  wins; when a measurement and a claim disagree, the measurement wins.
- Do not relitigate a recorded decision in a UI change. Open a decision entry.

## 10. Sources and maintenance

Adapted for LightSpeed from: `anthropics/skills` → `frontend-design`
(Apache-2.0); `martinholovsky/SOTA-skills` → `sota-frontend-design`
(CC BY 4.0, audit protocol and severity ladder); `pbakaus/impeccable`
(Apache-2.0, command vocabulary and review discipline). LightSpeed-specific
rules (state contract, tokens, egui traps, thin-wrapper rule, verification) are
original. `impeccable` is installed user-level for the critique/audit/spec
playbooks; it does not detect egui slop and its native guidance covers
iOS/Android only, so this skill is authoritative here.

Last updated: 2026-11-19 (v1) — created with the WF-081 GUI-refresh program.
