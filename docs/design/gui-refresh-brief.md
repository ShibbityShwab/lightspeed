# LightSpeed Desktop 2026 — GUI refresh brief (Lead-enhanced)

**Status:** W1 **ACCEPTED** (QA + owner) with two owner changes on 2026-11-20:
a **mobile-only viewport** (keep the shipped 320 px pinned single-column form)
and a **rebase onto `origin/master` `76aba5d`** before continuing. The W1 delta
wave is reconciling the design set to the new master and the mobile-only
decision; no GUI code changes until the delta spec passes its QA check. This
document is the enhanced version of the owner's request; teammates receive it
as the program contract.
**Program:** WF-081. **Created:** 2026-11-19. **Lead:** root agent.
**Skill for every GUI task:** `.dsh/skills/lightspeed-gui-design/SKILL.md`.
**Historical inputs (not approved):** `ui-audit.md`, `ui-design-critique.md`,
`ui-design-plan.md` — pass 1, verdict **REVISE, 16 required changes**, never
accepted. This brief supersedes them as the working plan.

---

## 1. What the owner asked, enhanced

Verbatim intent: *an expert UI and UX team should collaborate on improving the
GUI; a full refresh is acceptable to support a modern 2026 cross-platform
application with SOTA standards; logic should stay in one place so the CLI and
GUI do not drift or need separate upkeep; the GUI should essentially be a thin
wrapper package.*

Restated as a program:

1. **Refresh the desktop surface** to a 2026 standard: platform-appropriate,
   accessible, responsive, honest, fast, localised — not a reskin of the current
   cards.
2. **Centralise the logic**: one shared core that the CLI and the GUI both
   consume; the GUI keeps only presentation and platform integration.
3. **Prove it**: screenshots of the real window in every reachable state, gate
   counts, and a durable written contract so the next session cannot drift.

The "full refresh" is approved as a **direction**, not as a one-shot big-bang
rewrite. The plan below delivers it in waves, each verifiable, with the owner
gating the two decisions that change the architecture (§13).

## 2. Current state — measured, not remembered

| Fact | Evidence |
|---|---|
| Latest release `v1.11.3`, 39 assets; GUI is the active product surface | `wat/state/current-phase.md` (2026-11-18 entry) |
| GUI is egui/eframe **0.37**, Windows tray app, cross-platform modules exist (macOS **untested**, Linux tray is a stub) | `client-gui/Cargo.toml`, `client-gui/src/platform/mod.rs` |
| Shell is pinned to **320 px** wide, height 420–1400, initial 760 | `client-gui/src/main.rs`, `client-gui/src/app/window.rs` |
| Design tokens exist and are used: `client-gui/src/design.rs`, applied in `app/theme.rs`; shadows already off; fonts Inter + JetBrains Mono + CJK Noto | `client-gui/src/design.rs`, `client-gui/src/app/theme.rs` |
| `app.rs` is 3,888 lines after the Phase A split; 13 `app/` modules; 193 GUI tests at release time | `client-gui/src/app.rs`, `docs/design/app-refactor-plan.md`, state |
| The binding state contract exists: 11 route states + 4 server-leg states | `docs/design/interface.md` |
| The Oct-2026 design sprint stopped at pass 1: hostile critique says REVISE, 16 required changes (elevation path deleted, stale data unguarded, Privacy/About/Disconnect dropped, 320×400 overflow, fabricated relay names) | `docs/design/ui-design-critique.md` |
| `lightspeed-client` already exports the engine, status, registry, destination-aware relay ranking, process enumeration, profiles | `client/src/lib.rs` |
| But the GUI still owns logic: its own `config.toml` schema (`client-gui/src/config.rs`), relay health probing + labels + cached-relay handling (`discovery.rs`), geo lookup (`geo.rs`, `geo_countries.rs`), RTT history (`relay_history.rs`), presence scheduling (`app/presence.rs`), watchdog/race state (`app/watchdog.rs`, `race_watch.rs`) | read of those modules; line counts in the skill |
| The Oct-2026 docs describe egui 0.36.2 and HEAD `4e0ebb7`; the tree is now egui 0.37, HEAD `62c6cf6`, after the app split | `Cargo.lock`, `git log` |

**Implication:** the highest-value work is not a new renderer — it is
(a) making the state contract real, (b) moving logic out of the GUI, and
(c) finally implementing the accepted parts of the design sprint at a modern,
resizable viewport. The renderer choice matters only if accessibility parity or
web-tech becomes a hard requirement (§13 gate 1).

## 3. Objectives and measurable success criteria

| # | Objective | Success criterion (verifiable) |
|---|---|---|
| O1 | One logic surface | `client-gui/` business logic inventory reaches **zero** outside the approved adapter; a drift guard (test or `tools/` grep) fails on new engine/business rules in the GUI; CLI and GUI both call the same core for config, session start/stop, status, relay choice, game detection |
| O2 | State honesty | Every displayed fact in the refreshed spec is traced to one driving source in `interface.md` or the core DTO; no stale-data state survives an audit; unreachable states are documented as unverified |
| O3 | 2026 standards floor | Measured contrast table (AA), keyboard-complete core path, visible focus, no colour-only state, real reduced-motion source or no claim, 9-locale-safe layout, 200 % scaling, platform-adaptive window/tray/shortcut conventions |
| O4 | Linux + macOS parity (owner decision) | New Linux and macOS GUI CI jobs (build, clippy, test); every non-capture flow implemented and platform-adapted on all three OSes (tray/menu, admin semantics, autostart, updater UX, window conventions); parity evidence reported in three tiers — **build-verified**, **headless-verified**, **runtime-verified** — and never collapsed into "parity" |
| O5 | Full refresh implemented, not just planned | New shell implemented in `client-gui/`, with screenshots at the shipped widths and every reachable state; old shell deleted, not left behind a flag |
| O6 | No regressions | Workspace + GUI gates green with counts; CLI behaviour unchanged or better covered; `v1.11.x` install/update path intact |
| O7 | Durable process | `lightspeed-gui-design` skill, `interface.md`, decisions and evidence updated; no rule lives only in a session |

## 4. Non-goals

- Engine, protocol, capture, relay-fleet or routing behaviour (except the
  status DTO / command surface the GUI needs; those go through `rust-dev`).
- New **capture backends** on any OS. Parity covers the GUI and every
  non-capture flow; capture-dependent boost parity is reported per OS from what
  the engine already supports, and is not assumed or promised.
- Website redesign, docs translation bulk work, release/publish (owner gates).
- Marketing claims, "protected/VPN" positioning, anti-cheat framing.
- Paid tooling, services, fonts or assets (`[COST_STUB]`: $0.00).

## 5. Constraints and policy

- Windows host, PowerShell 5.1, no bash/Git Bash/WSL; `pwsh` and DSH tools only.
- `$0.00` owner spend; no paid services, runners, fonts or licences. Bundled
  fonts are OFL and already shipped.
- DSH is the only harness; procedures live in `.dsh/skills/`; no `.opencode/`.
- No secrets, no PII, no telemetry without opt-in; relay addresses already shown
  are fine.
- No agent co-authors in commits; commits name only human authors.
- Owner gates: UI stack, first-delivery scope, cross-platform expectation;
  any release/publish/news post.
- All work lands on the current worktree branch unless the owner says
  otherwise; the Lead owns integration and the final gate.

## 6. Design direction (the part that should not be relitigated)

The window is a **status instrument**: one glance answers *boosted or not, on
what route, is the link good*. The direction below is the canon every wave
must respect; it is implemented in detail by `design-system` and
`interface-designer`.

- **One bright control**: solid accent is spent on exactly one affirmative
  action; every other boundary is a hairline on a near-background surface.
- **The path is the product**: show the relay → game-server route the client
  actually measures, never a "protected" badge or an invented map.
- **Numbers are proof**: values are named, tabular, aligned; prose never
  outranks a measurement; no fabricated precision.
- **Silence unless the state changes**: no entrance animation, no idle motion,
  no hover motion on non-controls. The globe is the only accepted flourish and
  only while boosting and focused (with a real reduced-motion gate).
- **One system**: tokens, type scale and component contracts from
  `design.rs` + the refreshed design-system doc; no local overrides, no second
  palette, no stock egui chrome on custom surfaces.
- **Platform-adaptive**: one product identity, native conventions per OS
  (caption buttons, shortcuts, tray/menu, decorations, DPI behaviour).
- **Honest cross-platform**: Windows-first copy and evidence; macOS/Linux
  backends ship as gaps until smoke-tested, never as claimed parity.

The generic 2026 gaming-app default (neon purple/acid green, HUD chrome, 3D
hyperrealism, Russo One/Chakra Petch) is explicitly rejected. The one-shot
direction pass that returned it is recorded as a negative reference:
`wat/evidence/2026-11-19/uiuxpm-direction.txt`.

## 7. Architecture: one logic, GUI as a wrapper

### 7.1 Decided boundary: an `app` facade inside `lightspeed-client`

`architect` decided this in W1 (`docs/design/gui-core-boundary.md`, options
A–D with a falsifier table). A new `lightspeed-app-core` crate is **not**
available: `lightspeed-client` is one package with two targets
(`lightspeed_client:lib` + `lightspeed:bin`), so a crate that owned session
commands would either depend on the client and be unconsumable by its own
binary, or force a package split that renames the release artifacts the publish
gate hard-lists. The facade is the structurally correct boundary, not a
consolation prize.

```
protocol ──> client (engine, capture, tunnel, routing, profiles)
             │
             │  pub mod app   ← the one logic surface:
             │    config, commands, status, relay, game, error,
             │    capabilities, update, history, watchdog, presence
             │
             ├─> lightspeed bin (CLI) ── consumes lightspeed_client::app
             └─> client-gui ─────────── consumes lightspeed_client::app;
                                         owns rendering, platform integration,
                                         i18n and view state only
```

The W2 slices must sequence extraction (S0–S6 in the boundary doc) and the
shell rewrite so they do not collide in `client-gui/src/app.rs` (boundary doc
risk R1); the spec's slice plan owns that ordering.

### 7.2 What may live in `client-gui/`

Presentation state (open sheet, scroll, hover, animation phase), rendering,
platform integration (tray, window, fonts, updater UX), i18n lookup, and
presentation-only formatting. Nothing that answers *what is true about the
session, route, game or config*.

### 7.3 Known duplicates to resolve (W1 inventory, W2 extraction)

| GUI module | Duplicated/owned logic | Target |
|---|---|---|
| `client-gui/src/config.rs` | second `config.toml` schema (selected relay, game, language, manual server, MTU) | one shared config model; UI prefs namespaced |
| `discovery.rs` (816 lines) | relay health probing, friendly labels, cached-relay fallback | shared probing/labels API; GUI keeps the worker wrapper |
| `geo.rs`, `geo_countries.rs` | country/coordinate lookup for the globe | core data table + GUI rendering |
| `relay_history.rs` | RTT history persistence | core history service or core-owned store |
| `app/presence.rs` | 5 s game-presence schedule, Auto-only gate | core presence policy; GUI polls a channel |
| `app/watchdog.rs`, `race_watch.rs` | fallback/race state machines | core status DTO publishes the verdict |
| `self_update.rs`, `update.rs` | update check/apply + UI | core update check; GUI owns progress UX |

### 7.4 Migration discipline

Staged, mechanical, test-pinned: inventory → config model → commands/status
DTO → delete duplicate modules → point the CLI at the same API. No behaviour
change bundled with a move; each stage keeps the full gate green and the CLI
byte-compatible. Drift guard added in the same wave as the first extraction.

## 8. Team and workstreams

The roster currently has eight personas (`wat/personas/README.md`). This
program adds three UI/UX specialist personas at W1 start (registered with
files + `personas.json` + decision): **`ux-lead`**, **`design-system`**,
**`desktop-platform`**. Existing `interface-designer` remains the surface
owner; `architect` owns the boundary decision; `qa-engineer` verifies
independently (never the author); `rust-dev` does the extraction;
`dev-ops` verifies CI/packaging if the workspace changes.

### W0 — setup (this round, lead)

Done: state read; `lightspeed-gui-design` skill authored and live; Impeccable
user-level (no hook/engine); UI UX Pro Max one-shot evidence; this brief.
Owner gates asked (§13). No GUI code.

### W1 — design and architecture (docs only, parallel)

| Task | Owner | Deliverable | Acceptance / evidence |
|---|---|---|---|
| `[architect]` one-logic boundary | `architect` | `docs/design/gui-core-boundary.md` | Module inventory with line counts; proposed crate/module boundary; command/status DTO sketch; migration stages; drift guard design; falsifier; no code |
| `[ux-lead]` UX audit + IA + flows | `ux-lead` | `docs/design/gui-ux-audit-2026.md` | JTBD, current-flow friction with concrete evidence, state-inventory critique vs `interface.md`, flows for first run/reconnect/fallback/elevation/error/updater, microcopy inventory, usability test plan, severity-ranked findings |
| `[design-system]` 2026 system | `design-system` | `docs/design/design-system-2026.md` | Token audit vs `design.rs`; measured contrast table; type/space/radius/motion contracts; component state matrix; real reduced-motion mechanism; glyph-coverage constraint; light-mode decision; egui implementation mapping |
| `[desktop-platform]` cross-platform parity plan | `desktop-platform` | `docs/design/cross-platform-2026.md` | Windows/macOS/Linux matrix for window, tray, menus, shortcuts, DPI, notifications, updater, autostart, admin/elevation semantics; capability vs `platform/`; **parity gap-closure plan**; the exact CI jobs and the hardware/headless-smoke ask this host cannot satisfy; honest three-tier evidence statement |
| `[interface-designer]` refreshed spec | `interface-designer` | `docs/design/gui-refresh-spec.md` | Synthesis of the four docs + `interface.md`: shell regions, breakpoints, state-by-state screens with real data sources, copy table, component usage, implementation order, required screenshots. `blocked_by` T1–T4 |
| `[qa-engineer]` design review | `qa-engineer` | review memo + coverage matrix | Independent: every state has a source and a testable acceptance line; a11y checklist, no self-grading. `blocked_by` §8 synthesis |

Write scopes are disjoint (one docs file per owner; `qa` writes evidence only).
Handoff: the Lead integrates, checks consistency across the five docs, and asks
the owner to accept the spec before W2.

### W2 — build (after owner accepts the spec)

| Task | Owner | Scope |
|---|---|---|
| Shared core extraction, stage 1 (config model + commands/status DTO) | `rust-dev` | `client/`, `protocol/`; tests; CLI follows |
| Core extraction, stage 2 (discovery/probing/history/presence) | `rust-dev` | `client/` + delete GUI duplicates |
| New shell implementation | `interface-designer` | `client-gui/src/app*.rs`, `app/`; screenshots |
| Platform parity implementation (tray/menu, admin semantics, autostart, updater UX, window conventions) | `desktop-platform` + `interface-designer` | `platform/`, window/tray/shortcuts |
| Linux + macOS GUI CI jobs and headless smoke path; packaging check | `dev-ops` | `.github/workflows/`, `Cargo.toml`, `dist/` |
| Independent verification | `qa-engineer` | gates + screenshots + state matrix + parity three-tier report |

### W3 — close

Lead re-runs the gate on the integrated tree, records evidence, updates
`interface.md`, the skill and the decision log, and reports counts. Release is
a separate owner gate.

## 9. Deliverables and acceptance (program level)

1. Five W1 docs + one QA review memo, consistent with each other.
2. A W2 implementation whose shell is the spec (old shell deleted), with
   screenshots at the shipped widths and every reachable state.
3. A core boundary where CLI and GUI share config, session commands and status;
   drift guard in CI or `tools/`.
4. Updated `docs/design/interface.md` (state contract) and
   `.dsh/skills/lightspeed-gui-design/SKILL.md` (only if a rule changed).
5. Evidence under `wat/evidence/2026-11-*/`, named in the reports.
6. State + decision updates, with unverified items named.

## 10. Verification protocol

- Docs waves: links resolve, every claim cites a file/line, no invented data;
  the Lead cross-checks the set.
- Code waves: the full gate
  (`cargo fmt --all --check`;
  `cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui`;
  `cargo test --workspace --all --exclude lightspeed-gui`;
  `cargo clippy -p lightspeed-gui --all-targets`;
  `cargo test -p lightspeed-gui`) with counts per crate.
- GUI: real-window screenshots in the mobile-only form (320 px pinned width;
  320×420, 320×760 and 320 pt @ 200 %), in EN + DE + RU (RU is the binding
  long-word/long-label case), across disconnected/idle, connected, boosting,
  the `NO RETURN PATH` warning, game pick/empty, location/trace rows and
  error when reachable. UIA cannot see egui popups — read
  the screenshot.
- A design claim without a screenshot is a plan, not evidence. A state that
  cannot be reached is named unverified. Independent verification is `qa`,
  never the author.
- Bug fixes still need failing-before + mutation check where a defect is
  claimed fixed.

## 11. Risks and mitigations

| Risk | Mitigation |
|---|---|
| egui accessibility ceiling (custom-painted controls, limited semantics) | Scope AA floors, AccessKit/keyboard test pass; if screen-reader parity becomes hard, escalate the stack decision to the owner before W2 (§13 gate 1) |
| Scope explosion ("full refresh") | Waves, owner-accepted spec, one deliverable per task, disjoint scopes |
| Stale Oct-2026 design docs misread as approved | This brief declares them historical; spec supersedes; skill says so |
| Parity is required but this host cannot smoke-test Linux/macOS | W1 returns the exact CI jobs, headless-smoke path and hardware/runner ask; parity reported in three tiers; owner supplies access at the W2 gate or runtime parity stays explicitly unverified |
| Elevation/WinDivert limits mean some states are unreachable for screenshots | Document; use logged state + forced fixtures only where honest |
| Narrow 320 px width constrains long copy | Retired by the owner's 2026-11-20 mobile-only decision; long copy wraps (never shrinks), the state-word fit gate is ≤276 px, and the RU/ES/PT strings are shortened |
| Translation debt across 9 locales | New copy lands with catalog keys; English fallback only with the documented decision; localization gate on push |
| Core extraction regresses CLI | Staged moves with tests, CLI compatibility check, no behaviour change bundled |
| New third-party skill/tool drift | Repo-native skill is authoritative; Impeccable user-level only (no hook/engine); version recorded |
| Owner availability for gates | Bundle gates; keep the program unblocked on docs while waiting |

## 12. Definition of done (program)

- [ ] The owner accepted the W1 spec (§13 gates answered).
- [ ] The refreshed shell is implemented and the old shell deleted.
- [ ] CLI and GUI consume one core for config, commands and status; drift guard
      passes; duplicates removed.
- [ ] Linux and macOS GUI jobs are green and every non-capture flow has a
      platform implementation; parity evidence is labelled build/headless/runtime
      with the runtime gaps named.
- [ ] All reachable states have screenshots; unreachable states named.
- [ ] Contrast table measured; keyboard path demonstrated; i18n check passes;
      200 % scaling checked.
- [ ] Gates green with per-crate counts; evidence paths recorded.
- [ ] `interface.md`, skill, decisions and `current-phase.md` updated.
- [ ] No paid service, no secrets, no harness drift, no co-author trailers.

## 13. Owner decisions (answered 2026-11-19)

1. **UI stack: Instrument 2.0** — in-place egui refresh plus the shared core.
   Tauri 2 and a web frontend are **out of scope**. A future hard
   screen-reader-parity requirement reopens this decision; it is the documented
   falsifier for the egui choice.
2. **Delivery: design set → owner review → build.** W1 is docs only. W2 starts
   after the owner accepts `docs/design/gui-refresh-spec.md`.
3. **Cross-platform: Linux and macOS parity is a hard requirement** for the GUI
   and every non-capture flow (presentation, config, discovery, relay choice,
   status, updater UX, tray/menu/window conventions, autostart). Capture-
   dependent boost parity follows whatever the engine supports per OS and is
   reported separately, never assumed.
   **Implication, accepted deliberately:** this Windows host cannot run
   Linux/macOS GUI smoke tests. In **PR CI** the GUI is built/linted/tested only
   by the `windows-gui` job (`.github/workflows/ci.yml`; every Linux job and
   `macos-smoke` exclude `lightspeed-gui`), but the **tag-time release** pipeline
   does build Linux x86_64 and both macOS GUI artifacts (verified against the
   v1.11.3 assets). Parity therefore needs **external verification**: new Linux
   and macOS GUI PR jobs plus either owner hardware or a headless smoke runner.
   W1's `desktop-platform` deliverable returns the exact ask and the
   gap-closure plan (`docs/design/cross-platform-2026.md` §6); until those run,
   parity status is "planned, unverified" — never claimed.

### Owner decisions 2026-11-20 (after the W1 QA ACCEPT)

| Decision | Answer |
|---|---|
| W1 set | Accepted (owner read the spec on 2026-11-20); W2 authorised after the delta wave passes QA |
| Window geometry (Q1) | **Mobile-only viewport**: width pinned at 320 px, single column, height user-resizable 420–1400 (initial 760), no 440/1120 two-column layouts — supersedes the 360/440/1120 plan below |
| Base | Rebased onto `origin/master` `76aba5d` (16 commits: 1.11.4, warn-only relay starvation, honest location ladder, route trace, update disclosure); the W1 docs are being re-baselined by the delta wave |
| Locale completeness (Q5) | Translate the English-identical ja/ko/zh keys (45 as audited on master `76aba5d`; re-verify at W2) + new copy through the in-repo pipeline |
| Tray / packaging (Q6) | macOS menu bar + Linux StatusNotifierItem where the DE supports it; honest GNOME no-tray; bare tarball/shell installer, no paid notarization, no `.app`/`.dmg` in v1 |
| Notifications (Q7) | Explicit no-claim |
| Config precedence (Q10) | Canonical config wins; the legacy GUI file is preserved; one release-note line records it |
| Parity verification | CI + headless jobs now on free runners; **runtime tier stays unverified** until hardware exists |

Lead-adopted after the owner's read: Q2 rail metric = relay leg, labelled;
Q3 amendments **A1–A14** (A1–A13 rows; A14 non-state prose applied by W2-2);
Q4 sparkline removed in W2 v1; Q8 CLI runtime adoption deferred; Q9 GUI
presence policy is the product canon; Q11 geo-asset home decided in W2-3 with
`infra-dev`/`dev-ops`; Q15/Q16 as recorded in the spec (shorten RU copy; chrome
239/181).

### Parity baseline (corrected by W1; `docs/design/cross-platform-2026.md` is authoritative)

The first draft of this table was wrong in both directions. The W1
`desktop-platform` deliverable checked 224 citations and corrected it:

| Surface | Windows | Linux | macOS |
|---|---|---|---|
| PR CI GUI build/lint/test | `windows-gui` job (build + clippy + test) | excluded from every PR job | excluded from `macos-smoke` |
| Release artifact build | MSI + zip | x86_64 tar.xz, tag-time only | x86_64 + aarch64 tar.xz + shell installer, tag-time only |
| Tray / menu bar | real `tray-icon` | no-op stub | no-op stub; **untested** |
| Admin / elevation | token check + UAC relaunch; user config survives | `id -u == 0`; `pkexec` relaunch drops `HOME`/`XDG_CONFIG_HOME`, so the elevated GUI reads root's config | `id -u == 0`; `osascript` relaunches the whole GUI as root, same config loss, against macOS norms |
| Capture capability probe | `sc query npcap` — wrong: the shipped GUI uses WinDivert | `which tcpdump/dumpcap` — wrong: the engine uses nftables + `CAP_NET_ADMIN` | same wrong probe: `pfctl` + root is the real backend |
| Autostart | **not implemented** | **not implemented** | **not implemented** |
| Updater UX | check + in-place install | check only, no action | check only, no action |
| Window conventions | native Win11 decorations, pinned 320 px | DE decorations, pinned 320 px | decorations, no `.app`/`.dmg` in CI |
| Shortcuts | none (egui focus only) | same | same |

Corrections C1–C5 and the stub inventory S1–S23 are in
`docs/design/cross-platform-2026.md` §1 and §4. Two follow-ups are named there
and are **not** silently edited here: the false comment at
`client-gui/Cargo.toml:74-75` / `dist-workspace.toml:16-17`, and the unverified
user-facing claim at `docs/user-guide.md:31` ("the GUI works on Linux"), which
must be downgraded in the same localization pass that changes it. `dev-ops`
owns the W2 CI expansion; `desktop-platform` owns the rest of the plan.

## Appendix A — inputs read for this brief

`AGENTS.md`; `wat/rules.md`; `wat/state/current-phase.md`; `wat/team-mode.md`;
`wat/personas/README.md`; `docs/design/interface.md`; `docs/design/ui-audit.md`;
`docs/design/ui-design-critique.md`; `docs/design/ui-design-plan.md`;
`docs/design/app-refactor-plan.md` (state summary);
`client-gui/src/main.rs`, `config.rs`, `design.rs`, `discovery.rs`,
`processes.rs`, `app/theme.rs`, `app/window.rs`, `platform/mod.rs`;
`client/src/lib.rs`, `cli.rs`; `client-gui/Cargo.toml`.

## Appendix B — program tooling created this round

- `.dsh/skills/lightspeed-gui-design/SKILL.md` — repo-native fused skill
  (2026 floor, state/token canon, egui traps, thin-wrapper rule, audit protocol,
  anti-slop, screenshot verification). Discovered live by DSH.
- `%DSH_HOME%\skills\impeccable` — Impeccable v4.10.0, manual copy, user-level
  only; no installer, hook or engine run; not committed.
- `wat/evidence/2026-11-19/uiuxpm-direction.txt` — one-shot UI UX Pro Max
  direction pass, kept as a negative reference.
- `AGENTS.md` skill table + `wat/state/decisions/2026-11.md` entry.
