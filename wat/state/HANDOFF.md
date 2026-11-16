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

## Verified state at this handoff (2026-11-16, after the WF-067 rollout)

- **1.11.0 is released and live.** `v1.11.0` is the latest GitHub release
  (2026-11-16, 39 cargo-dist assets, body = `CHANGELOG.md`). All eight community
  relays run 1.11.0 with `[network] control_alpn = "required"` and are healthy;
  a 1.11.0 client negotiates `lightspeed-control/1`, a pre-1.11.0 client is
  rejected with alert 120 (owner-accepted traffic impact).
- **The release was published manually.** The Release workflow failed at
  "Create GitHub Release" with `HTTP 403: Resource not accessible by
  integration`; artifacts came from its own green build jobs and packaging
  manifests were bumped by hand. Board task-9 fixes the pipeline.
- **The §7.4 clean window was measured and failed before the flip** (M1 66 -> 520
  in 120 s on the canary, zero accepted handshakes); the fleet flip proceeded on
  the owner's explicit traffic-impact authorization and the deviation is recorded
  in the plan header, the state entry and the decision log.
- Lead gate on the code (unchanged since `2de1665`): fmt/clippy clean (only the
  two pre-existing manifest warnings); workspace **1,557/0/6**; GUI **181/0/1**;
  WinDivert **1,261/0/4**; CI `QUIC Integration Tests` command **1,493/0/2**;
  client quic lib **640/0/1**; proxy quic lib **159/0/0**; localization checks
  green. CI run `43880379231` on `2de1665` is green on every job.
- Agent layer: durable-owner policy is in `wat/team-mode.md` Step 2. Team
  instances are **session-scoped**: a new session starts with the same policy but
  fresh instances (and a fresh, empty board).

## Current workstream: 1.11.0 is live; watch the drain, fix the release pipeline

Read `docs/quic-alpn-rollout.md` (its header now records the rollout, the measured
§7.4 fail and the owner authorization) and the newest entry in
`wat/state/current-phase.md`. The control plane negotiates
`lightspeed-control/1` end to end; the fleet is `required`; the flip tool is
`infra/scripts/relay-control-alpn.sh` with a `Deploy Proxy` dispatch path and a
hermetic self-test in `Infra Script Tests`.

**What is left is monitoring and pipeline repair, not rollout code:**
1. Watch `lightspeed_control_handshakes_total{outcome="rejected_no_application_protocol"}`
   per relay; it is the meter of pre-1.11.0 clients. The real §7.4-style clean
   window is now "M1 stops increasing over a window with >=200 accepted
   handshakes" (board task-10).
2. Fix the Release workflow publish path (403 on `POST /releases` from
   `GITHUB_TOKEN`, plus the skipped choco/winget publish) before the next tag -
   board task-9. Until then a tag needs the manual publish procedure recorded in
   the 2026-11-16 decision entry.
3. Owner decision: the public #140/#147 replies describing the client-first order.
4. Later: the §5 retirement decision (client stops falling back / relay drops
   `/1`) once M1 is flat.

Per-relay rollback stays one command (`--mode off` + restart, no client change).
The POSIX relay smoke script also cannot run on this Windows host - CI verifies
it. Team boards are **session-scoped**: recreate tasks from the plan rather than
expecting the previous ids.

**Do not ask for a Discord webhook.** The owner does not use Discord; monitor
alerts surface as scheduled-workflow failures/logs (decided 2026-11-16).

## What still needs to happen (ranked)

1. **Post-rollout**: watch M1 drain (task-10); fix the Release workflow publish
   path (task-9); owner decision on the public #140/#147 replies.
2. **Owner decisions**: the public #140/#147 replies, and later the §5 retirement
   of the empty-offer fallback.
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
