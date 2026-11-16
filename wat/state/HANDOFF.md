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

## Verified state at this handoff (2026-11-16, after WF-067)

- Branch `master` based on `9276a7e` (WF-066 + the ALPN handoff prep); the
  WF-067 round is the next commit (see the top of `wat/state/current-phase.md`).
- Lead gate on the WF-067 tree: fmt/clippy clean (only the two pre-existing
  manifest warnings); workspace **1,557/0/6**; GUI **181/0/1**; WinDivert
  **1,261/0/4**; the CI `QUIC Integration Tests` command **25 targets,
  1,492/0/2**; client quic lib **640/0/1**; proxy quic lib **158/0/0**;
  localization checks green.
- Product: 1.10.3 released; master carries unreleased work - relay-watchdog fix,
  fallback state contract, validated server-leg signal + honest route surface,
  canonical whole-path ranking key, app-telemetry publication gate, monitor
  instrument fixes, and the versioned-ALPN control plane (stages A/B/F/C/D).
- Live validation on the owner's ROB RUN: quiet-flow v1 falsified for chatty
  games (2 accepted samples / 17 min), shadow sampler 0.49% acceptance, route
  surface honest (prior/bound states), 0 watchdog fallbacks, 0 relay changes.
- Agent layer: durable-owner policy is in `wat/team-mode.md` Step 2 (reuse an idle
  persona instance; spawn only for parallel same-persona work, contaminated
  context, or an unusable instance). The active roster and retired instances are
  in the 2026-11-16 decision entry. Team instances are **session-scoped**: a new
  session starts with the same policy but fresh instances.

## Current workstream: WF-067 landed, owner gates next

Read `docs/quic-alpn-rollout.md` (its header is now current: stages 1/2 are
implemented, unreleased) and the newest entry in
`wat/state/current-phase.md`. The control plane negotiates
`lightspeed-control/1`; the client offers it and falls back once to an empty
offer on alert 120, remembering the answer per relay; the relay has
`[network] control_alpn = "off" | "required"` (default off) with startup,
`/health` and `/metrics` observability and a five-row compatibility matrix.

**Nothing ALPN-related is released and no relay config was touched.** The next
steps are owner decisions, not code: (1) release the accumulated unreleased work
including the stage-1 client build; (2) per-relay `required` flips, each gated on
the §7.4 clean window (>=200 accepted control handshakes, M1 = 0); (3) the public
#140/#147 replies describing the client-first order. The code-free deliverable
that remains is the relay operator procedure (board task-6: `docs/deploy-proxy.md`
+`docs/community-network.md`, no production config edits in that task). The
POSIX relay smoke script cannot run on this Windows host - CI is its verifier.
Team boards are **session-scoped**: a new session starts with an empty board, so
recreate tasks from the plan rather than expecting the previous ids.

**Do not ask for a Discord webhook.** The owner does not use Discord; monitor
alerts surface as scheduled-workflow failures/logs (decided 2026-11-16).

## What still needs to happen (ranked)

1. **ALPN owner gates**: release the accumulated unreleased work including the
   stage-1 client build; each `required` relay flip on its §7.4 clean window; the
   public #140/#147 replies. Then board task-6 (relay operator procedure).
2. **Owner decisions**: those three ALPN gates, plus the release decision for
   everything else that is unreleased.
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
