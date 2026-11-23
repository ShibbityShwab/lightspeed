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
- The pin path suppresses retries (`should_retry` requires `!pin_active`), so
  one transient became a hard failure with “restart Windows” advice.
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
