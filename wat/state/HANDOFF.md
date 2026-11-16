# Handoff: LightSpeed, 2026-11-16

Brief for the incoming session. Start at `wat/state/current-phase.md` (live
state), then `wat/rules.md` (the `$0` cost rule and the Windows shell rule bind
hardest), then `wat/README.md` (the map).

Repo: `ShibbityShwab/lightspeed`, branch `master`.

## What this project is

A zero-cost global network optimizer for multiplayer games, run through a
Workflows/Agents/Tools autonomy system (`wat/`) on **DeepSeek Harness**. Crates:

- `client/` — core engine: pcap/WinDivert capture, QUIC control plane, FEC
  tunnel, latency probing, per-game process detection.
- `client-gui/` — egui Windows tray app (the active product surface).
- `proxy/` — the relay fleet binary.
- `protocol/` — shared wire types.
- `infra/` — Terraform, Docker, relay scripts, monitoring collectors.
- `web/` — GitHub Pages site, locales generated from the English source.
- `dist/`, `packaging/` — Scoop / Chocolatey / winget / MSI.
- `.github/workflows/` — CI, Docker, security audit, deploy, health monitors.

Cost rule (`wat/rules.md`): the **owner's** ongoing spend stays exactly `$0.00`;
the eight community relays are sponsor-funded (decision 2026-09-13).

## Agent setup (2026-11-16 migration)

- Harness is DeepSeek Harness only. opencode/OmO artefacts are gone; do not
  reintroduce harness-specific state directories.
- Procedures are **project skills** in `.dsh/skills/`: `lightspeed-wat`,
  `lightspeed-team`, `lightspeed-windows`, `lightspeed-verify`. They are
  discovered automatically; load one instead of re-deriving a procedure.
- Personas are **delegation contracts** in `wat/personas/` (eight of them). Team
  work uses Agent Teams; the contract is `wat/team-mode.md`.
- State is split: `current-phase.md` is the small hot file; history lives in
  `phases/<YYYY-MM>.md`; decisions live in `decisions/<YYYY-MM>.md`.
- The old `wat/archive/agents.md` was deleted because on Windows it matched the
  `AGENTS.md` instruction loader and was injected into every session. Never name
  a descriptive document `agents.md`.

## Windows host facts (hard-won, do not re-learn)

1. **Shell is Windows PowerShell 5.1.** `pwsh` is not installed. No Git Bash, no
   WSL, no `bash script.sh`. `git` runs directly.
2. **CPython 3.12 is installed** at `%LOCALAPPDATA%\Programs\Python\Python312`
   (per-user, PATH updated). A shell started before 2026-11-16 does not see it;
   restart the shell or pass `-Python` the full path. The Microsoft Store
   aliases `python`/`python3` are still stubs (exit 9009).
3. **Rust is installed** (1.100.0, matching `rust-toolchain.toml`); `cargo fmt`,
   clippy and tests run locally, and the localization gates now run too
   (`scripts/Localization.ps1 -Check` passed: 403 catalog keys, 8 locale pages,
   48 localized docs).
4. **Elevation does not propagate into the agent shell.** A medium-integrity
   shell cannot stop an elevated GUI, and a running GUI locks its own exe. Use
   `scripts/dev-gui.ps1 -Elevate`, stop from the tray, verify installs by size
   (`scripts/install-gui.ps1`).
5. **Single-instance mutex is shared across builds.** A stray `target/debug`
   instance silently wins. Check `Get-Process lightspeed-gui` first.
6. **egui popups are invisible to accessibility tooling.** Verify collapsed
   controls by selected text and a screenshot.
7. **PS 5.1 gotchas:** a single `PSCustomObject` has no reliable `.Count` — wrap
   in `@()`; `Select-Object -First N` terminates a long pipeline early; no `??`,
   no ternary, no `ProcessStartInfo.ArgumentList`.
8. **Line endings.** `.gitattributes` pins LF for text and CRLF for `.cmd`/`.bat`.
   Existing worktrees may still be CRLF until re-checkout; single-line edit
   anchors and the DSH edit tool work either way.

## Verified state at this handoff (2026-11-16, WF-066)

- Branch `master` at `bc6177d` (WF-066 `76fb04b` + the cross-server ping-keying
  fix). Working tree clean; CI green on both commits (14/14 jobs, including
  `QUIC Integration Tests` and `Infra Script Tests`).
- Lead gate: fmt/clippy clean; workspace **1,508 passed / 0 failed / 6 ignored**
  across 29 suites; GUI **181/0/1**; WinDivert feature **1,261/0/4**; QUIC lib
  **857/0/1**.
- Product: 1.10.3 released; master carries unreleased work - relay-watchdog fix,
  fallback state contract, validated server-leg signal + honest route surface,
  canonical whole-path ranking key, app-telemetry publication gate, monitor
  instrument fixes.
- Live validation on the owner's ROB RUN: quiet-flow v1 falsified for chatty
  games (2 accepted samples / 17 min), shadow sampler 0.49% acceptance, route
  surface honest (prior/bound states), 0 watchdog fallbacks, 0 relay changes.
- Agent layer: durable-owner policy is in `wat/team-mode.md` Step 2 (reuse an idle
  persona instance; spawn only for parallel same-persona work, contaminated
  context, or an unusable instance). The active roster and retired instances are
  in the 2026-11-16 decision entry. Team instances are **session-scoped**: a new
  session starts with the same policy but fresh instances.

## Next session: ALPN client-first (WF-067)

Read `docs/quic-alpn-rollout.md` first (why it is a defect §1, the versioned
identifier §2, the compatibility matrix §3, stages §4, exact changes §5, tests
§6, observability §7, owner gates §11, tasks §13). The control plane currently
sets no ALPN on either side; the plan is `lightspeed-control/1`, client offers it
and retries once with an empty list on alert 120, relay gains
`[network] control_alpn = "off" | "required"` (default off), then canary/fleet
flips.

Board: task-26 (protocol + client offer/fallback/observability) and task-27
(relay mode + matrix tests) are **ready** and can run in parallel; task-30 (user
docs) is blocked on task-26; task-28 (interop test in CI) and task-29 (manual
two-relay evidence) are blocked on 26+27; task-4 (operator procedure) is blocked
on task-27. **Writing the code/tests/docs needs no owner approval** (§11) - only
the stage-1 client release and each per-relay `required` flip are owner gates.
Suggested shape: two `rust-dev` instances for 26/27, then `infra-dev` for 28,
`qa-engineer` for 29, `interface-designer` for 30; Lead integrates, re-gates,
pushes, records state. The POSIX relay smoke script cannot run on this Windows
host - CI is the verifier.

**Do not ask for a Discord webhook.** The owner does not use Discord; monitor
alerts surface as scheduled-workflow failures/logs (decided 2026-11-16).

## What still needs to happen (ranked)

1. **ALPN stages A/B/F** (board 26/27/30), then 28/29, then task-4 procedure.
   Owner gates remain the stage-1 release and per-relay flips.
2. **Owner decisions**: stage-1 release; each `required` relay flip; public
   #140/#147 replies describing the client-first order; the release decision for
   the accumulated unreleased work.
3. **Small cleanups**: translation debt 7x8, `docs/architecture.md` selector
   one-liner, `pooled_estimate` test-only, and the recorded ICMP-blocked-server
   watchdog behaviour if it recurs.
4. **v2 relay-vantage probe**: NO-GO until its fixed gates G1-G5 are measured
   with fleet access (`docs/relay-server-leg-probe.md`).
5. **Keep the state protocol** - one dated entry per round, decisions in
   `decisions/<YYYY-MM>.md`, history moved to `phases/` rather than deleted, and
   the durable-owner team policy in `wat/team-mode.md` Step 2.

## Pain points

- **Verify artefacts, not proxies.** Exit codes and summaries lied here before:
  an install "succeeded" while file sizes differed; a suite read "ok, 0 passed".
  Read pass counts, compare bytes, screenshot the UI, mutation-test fixes.
- **Remote-write policy.** Routine commits/pushes to master are authorized
  (standing grant), Conventional Commits, each push gated by the checks above.
  Do not commit or push without the gate passing.
- **Owner gates** are the only things that should block; everything else is a
  task, not a question.
