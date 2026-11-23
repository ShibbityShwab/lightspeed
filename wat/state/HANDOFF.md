# Handoff — WinDivert stability reset (2026-11-23)

**Read this first. This file is the compact context for a new session; do not
re-derive the incident from scratch.**

## Status

- Product: LightSpeed Windows GUI, installed **1.12.5**. Stability claim is
  **withdrawn** pending the P0 gate below.
- The owner stopped manual testing. Do not start/stop the app or touch
  WinDivert/WFP unless a task explicitly requires a clean-boot real-driver run.
- Expert audit, roadmap, personas and training are in **PR #225** (branch
  `audit/windivert-stability`, worktree
  `.agents/worktrees/windivert-stability-audit`). Merge first if the new
  session starts from `master`.
- No production code change is in flight. The last plausible cold-start
  experiments were discarded; the released 1.12.5 artifact remains installed.

## P0 progress (2026-11-23)

- **P0-1 LANDED** as a behaviour-neutral instrumentation patch in worktree
  `.agents/worktrees/dsh-97926fd1-6add` (the P0 work is based on `origin/master`
  `9e8029b`; upstream has since moved to **`f2e7bb0`** — PR #226 a11y — see the
  upstream-check entry in `current-phase.md`; changes are uncommitted).
  `client/src/app/lifecycle.rs` emits the typed
  `lifecycle.jsonl` record set plus the matching `event=…` trace line, the
  build/commit/driver identity, the `prev_shutdown` marker and a 60 s
  rate-limited read-only support bundle (WFP enumeration deferred). The engine,
  AppCore, watchdog, interceptor and GUI emit `interceptor_state`,
  `windivert_open`/`windivert_close`, `control_registration`, `presence`,
  `watchdog`, `ui_error`/`ui.word`/`ui_state` and one `shutdown`.
- **P0-2 LANDED.** A held pin no longer suppresses the `FWP_E_IN_USE` retry:
  R1 is bounded (6 attempts / ~5 s / 50→400 ms), R2 is one pin-cycle per
  episode, and the remedy is the honest R1–R4 ladder (R3 explicit only, OD-1;
  reboot last with a reason). No "restart Windows" first line.
- **P0-3 LANDED.** Registration pushes are generation-scoped (a retired
  generation is `Superseded`/`Discarded`, never a failure), a bounded
  registration ledger and typed `{source, epoch, auto_clear, latched}` error
  records clear on recovery, `FAULT_LATCHED` carries close-age, and
  `error_word_for` stops a non-fatal error masking the live word.
- **P0-4 LANDED.** A start is no longer a Stop: a healthy live run is refused
  with `ALREADY_INTERCEPTING`, only a dead/latched-fault run is replaced, and
  `restart_interceptor_request` is the explicit restart. `hold_on_start` reaches
  `InterceptorConfig.start_held`, so the Windows gate is created closed before
  the first captured packet can tunnel (Mock gated for the T12 test). Every
  capture-crossing GUI action records exactly one `gui.action` with its source.
- **P0-5 LANDED (owner OD-7 approved).** The pin/WFP guarantee, the
  unload/reload churn claim, the "callout/filter per open handle" claim and the
  reboot-first remedy are gone from `docs/troubleshooting.md`,
  `docs/release-notes-1.12.5.md`, `CHANGELOG.md`, the runtime `log_open_failure`
  copy and the interceptor module comments. The 84-row matrix is scoped to 84
  assertions on `84e15c3` (not the released artifact). The 8 localized
  troubleshooting pages carry the corrected section and
  `scripts/localization-state.json` was advanced (public translation endpoints
  were unreachable, so the section was translated here; the pages already
  declare unreviewed machine assistance). No tag or published body rewritten.
- **P0-6a HARNESS READY; P0-6 GATE NOT PASSED.** `scripts/windivert-stability/`
  provides the exact-artifact identity gate, a read-only clean-boot preflight,
  a field-based `lifecycle.jsonl` consumer (A2 zero-teardown assertion,
  handle/registration ledgers, 10-check self-test) and A1/A2/A4/A5/A6/A8/A11/A12
  row scaffolding. On this dirty host the Lead re-ran the self-check: exit 2,
  `BLOCKED: dirty kernel`, identity match, rows 0 PASS / 0 FAIL / 1 U / 7
  BLOCKED, host unchanged. **Run the real rows only on an owner-arranged clean
  boot (OD-6); a reboot must never turn a row green.** Evidence
  `wat/evidence/2026-11-23/p0-6/`.
- **Exact artifact verified (read-only):** the installed GUI is the published
  `v1.12.5` artifact byte-for-byte (release zip hash = `sha256.sum`; tag
  `v1.12.5` = `6e30a65` = release target). The 84/84 matrix binary was a
  different binary (`F8ECE582…`, built from `84e15c3`). Evidence
  `wat/evidence/2026-11-23/p0-1/artifact-verify.log`.
- **Gates:** P0-1 baseline fmt/clippy clean, workspace **2,036 / 0 / 7**, GUI
  **222 / 0 / 0**, WinDivert **1,687 / 0 / 9**. P0-2+P0-3: **2,056 / 223 /
  1,713**. P0-2+P0-3+P0-4: **2,067 / 223 / 1,724**. Final integrated tree
  (`wat/evidence/2026-11-23/final-p0/lead-gate.log`): workspace
  **2,067 / 0 / 7**, GUI **223 / 0 / 0**, WinDivert **1,724 / 0 / 9**, clippy
  0 real warnings, `localized-docs` + `localization-sync` green. Guard note:
  the GUI G3 budgets were deliberately re-measured (`app.rs`
  4734→5235→5269, `presence.rs` 482→740, `route_surface.rs` 2268→2570),
  justified in the decision log; named follow-up is moving the trackers and the
  action vocabulary into the core. G5 was fixed by moving platform FFI to
  `client/src/host.rs`.
- **Upstream check (2026-11-23, not merged).** `origin/master` moved to
  `f2e7bb0` (PR #226, accessibility tree). Applying the whole P0 WIP to it gives
  **4 mechanical conflicts** only: the `LightSpeedApp::new` → `new`/
  `new_offline`/`build` split (keep `lifecycle::init` in the real `new`, never in
  `build`, or offline/a11y rendering would write lifecycle state), the G3
  budgets (merged counts: `app.rs` 5,565, `presence.rs` 758, `route_surface.rs`
  2,570, a11y 631/420), and the two append-only WAT files. The resolved trial
  tree is green: workspace **2,067 / 0 / 7**, GUI **254 / 0 / 1**, WinDivert
  **1,724 / 0 / 9**, localization 4/4. Evidence
  `wat/evidence/2026-11-23/upstream-merge-trial/`. The real worktree was not
  merged (still 39 uncommitted entries on `9e8029b`).
- **Merge + PR (2026-11-23).** The P0 work is committed (4 commits) on
  `feat/windivert-stability-p0`, merged with `origin/master` `f2e7bb0` (PR #226
  a11y) and opened as **PR #228**; the merged tree's clean-build gate is green
  (workspace **2,067 / 0 / 7**, GUI **254 / 0 / 1**, WinDivert
  **1,724 / 0 / 9**, localization 4/4). Conflict resolutions: lifecycle init
  stays in the real `new` (never `build`); merged GUI budgets `app.rs` 5566,
  `presence.rs` 758, `route_surface.rs` 2570; both WAT sides kept.
- **Next:** **P0-6 real rows need an owner-arranged clean boot (OD-6)**. Until
  they pass, no row is green, **no stability claim**, and 1.12.5 remains a
  candidate. Before landing, commit the P0 WIP, merge/rebase `f2e7bb0`,
  resolve the four files above, re-run the gate. No real-driver run, no GUI
  launch, no commit/push/tag/publish.
  Owner decisions 2026-11-23: OD-7 approved, OD-1 declined (R3 explicit
  administrator-only), OD-6 harness-now/later-clean-boot.

## What was observed (facts)

- 1.12.5 cold-started on a clean boot and four manual Stop→Optimize cycles
  succeeded.
- In a real 363 s session, after two intra-session server re-detects and four
  no-reply windows, a **Stop** occurred at 06:49:38.417Z with pin `id=1` still
  active. Diverting handles `id=2/3/4` closed `ok`.
- Fourteen milliseconds after the next start invocation, intercept `id=5`
  failed with `FWP_E_IN_USE` (`0x8032000A`); a second invocation `id=6` failed
  too. The control plane registered successfully immediately after each failed
  capture start, so route plane was alive while capture was dead.
- The pin path suppressed retries (`should_retry` required `!pin_active`), so
  one transient became a hard failure with “restart Windows” advice. **Fixed by
  P0-2** (retry now runs while pinned; the remedy is the R1–R4 ladder).
- The server re-detects themselves closed **zero** capture handles. The Stop
  actor is not logged; a stale ERROR card / manual Stop / implicit pre-stop is
  the working hypothesis, not a proven fact.
- A stale “Registration failed / control-plane registration failed” card can
  survive recovery: a superseded registration writes failure status, the
  replacement’s success does not clear it, and `Error` outranks `Optimizing`.
- The QA 84/84 matrix ran on `84e15c3`, not release HEAD `01438e0`; its cycles
  were 11.8–12.2 s, used one blackholed server, had no return traffic, and
  never exercised a server transition. It cannot validate 1.12.5.

## What is not proven

- The kernel call/object that returns `0x8032000A`; the leading hypotheses are a
  long-session WFP callout unregister race, a leak, or an external holder.
- The actor of the 06:49 Stop and why two start invocations followed.
- Whether the released 1.12.5 reproduces the same failure; trace provenance is
  mixed.
- Whether the pin is worth keeping long-term. Short-term it is a handle, not a
  guarantee.

## P0 — the only patch to start now

All six land before any stability/release claim.

| ID | Work | Owner | Write scope |
|---|---|---|---|
| P0-1 | Lifecycle instrumentation v1: build SHA; interceptor state/reason/actor; open/close ids + close-age + pin/FWP; registration ledger; presence; UI error/word; `lifecycle.jsonl`; rate-limited support bundle | `windows-engineer` + `rust-engineer` | `client/src/interceptor/`, `client/src/engine.rs`, `client/src/app/`, `client-gui/src/app.rs` |
| P0-2 | Restart resilience: bounded retry while pinned; one pin-cycle; typed `FAULT_LATCHED`; close-age context; honest recovery copy; no immediate reboot advice | `windows-engineer` | interceptor + error copy |
| P0-3 | Registration/error state machine: generation guard, ledger, source+epoch clear on recovery, no masking live capture, command-error clear | `rust-engineer` | engine/app/GUI DTO |
| P0-4 | Start/Stop semantics: no implicit pre-stop; explicit restart; action+source logging; hold installed before first tunnelled packet | `rust-engineer` | engine/GUI |
| P0-5 | Truth pass: remove pin/driver-unload guarantees and false callout claims from docs/UI; do not rewrite tags | `release-scribe` | docs, CHANGELOG |
| P0-6 | Evidence gate: exact artifact hash; A2 two-endpoint traffic; A4/A5 fast restart; A6 long session; A11 dirty-kernel recovery; two independent runs | `stability-verifier` | tests/evidence only |

**Suggested first action:** P0-1 only, as a behavior-neutral instrumentation PR.
Do not combine it with recovery/state-machine edits. Then P0-2/P0-3 together
against the same trace, P0-4, P0-5, P0-6.

## Hard acceptance gate

A release may not be called stable until, on the exact candidate artifact:

- Two independent clean-boot runs.
- ≥10 minute traffic-bearing session with a working return path.
- Settlement/POI and region transitions with **no capture teardown**.
- 20 rapid Stop→Optimize cycles; zero `FWP_E_IN_USE`, zero stale cards.
- Registration race: superseded generation must not produce a failure card;
  newer success clears it.
- Quit/relaunch and dirty-kernel recovery are explicit rows; a reboot is never
  used to turn a row green.
- Every row records build hash, timestamps, handle ids, pin state, WFP counts.

Any of these not measured = not a pass.

## P1 — after P0 green

- In-place relay switching from the GUI (core `switch_relay` exists; no GUI
  caller today — this is the primary transition path).
- Auth/menu → region-specific match handled as a normal transition, not a game
  exit.
- Presence debounce; scan failure ≠ absent.
- Opt-in bounded starvation failover (default off).

## P2 — later

- UI action audit log + diagnostics surface.
- Continuous long-session stability suite.
- No direct WFP/NDIS/patched-driver work: EV cert + attestation + GPLv2
  obligations break the $0 rule. Stay on official WinDivert 2.2.2.

## Non-negotiable rules

- Owner spend stays **$0.00**; Windows-native PowerShell 5.1; no bash/WSL.
- No release/publish without the owner gate.
- Do not run WinDivert tests on a dirty kernel and call it a pass.
- Do not claim a fix from a build that differs from the release artifact.
- A control-plane success does not mean the capture plane works, and vice
  versa; keep the two planes separate.
- Never restart capture for a server/region change, relay choice/demotion,
  starvation warning, registration reconnect, one failed presence scan, or a
  stale UI card.

## Owner decisions

1. Add an explicit **Reset interception driver** action (service stop, warning
   if another WinDivert client exists)?
2. Keep the pin short-term, stripped of any guarantee, until P0-6 data decides?
3. Leave automatic starvation failover off by default?

## Bootstrap prompt for the new session

> Read `wat/state/HANDOFF.md`. We are starting the WinDivert stability reset
> from P0-1 only. Create the P0 task board with the task graph in this handoff,
> assign disjoint write scopes, and implement behavior-neutral lifecycle
> instrumentation first. Do not modify release claims, do not publish, and do
> not run real-driver tests on a dirty kernel. Verify the exact artifact and
> report findings; no stability claim until the P0 gate passes.

## References

- Audit PR: #225 (`docs/audit/`, `docs/training/`, `docs/design/windivert-stability-roadmap-2026-11-23.md`, three new `wat/personas/`).
- Full expert audits: `docs/audit/{windivert-lifecycle,game-transition-state-machine,stability-verification}-2026-11-23.md`.
- Curriculum: `docs/training/in-house-stability-curriculum.md`.
- User trace: `wat/evidence/2026-11-23/stability-audit/user-server-transition-trace.log`.
- Existing release/audit worktree: `.agents/worktrees/pinned-driver-1.12.5`.
