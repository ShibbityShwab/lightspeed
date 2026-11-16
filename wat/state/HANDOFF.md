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
2. **Python is absent.** `python`/`python3` are Microsoft Store stubs (exit 9009),
   `py` is missing. Localization scripts and `tools/*.py` need a real CPython —
   a documented prerequisite, not a silent assumption.
3. **Rust is installed** (1.100.0, matching `rust-toolchain.toml`); `cargo fmt`,
   clippy and tests run locally. The localization workflow cannot.
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

## Verified state at this handoff

This session changed agent infrastructure, not product code. `tools/Check-LightSpeed.ps1
-SkipGui -SkipBuild` passed: fmt clean; workspace clippy (`--all-targets
--all-features --exclude lightspeed-gui`) clean; `cargo test --workspace --all
--exclude lightspeed-gui` **1,444 passed / 0 failed / 6 ignored across 29
suites**. `cargo audit` and `cargo deny` are not installed here and were reported
as skipped (CI owns them); the GUI gates were skipped because no GUI source
changed; the localization gates need CPython. Product state remains what the
newest phase entries describe: 1.8.4 shipped, the ARC Raiders transport split
implemented, the live join through it still the open proof. Packaging: Chocolatey
pushed and awaiting moderation, winget pending.

## What still needs to happen (ranked)

1. **Re-run the full gate after any code change** — the migration touched docs,
   state and scripts, not product code.
2. **Localization port** — the `.sh` wrappers are now `.py` entry points plus a
   PowerShell runner; proving them on this host needs CPython installed (owner
   decision) or CI.
3. **Owner-gated channels** — Chocolatey moderation, winget PR, product calls
   (#137 Minecraft, #59 Fortnite). Surface as one crisp question.
4. **Keep the state protocol** — one dated entry per round, decisions in
   `decisions/<YYYY-MM>.md`, history moved to `phases/` rather than deleted.

## Pain points

- **Verify artefacts, not proxies.** Exit codes and summaries lied here before:
  an install "succeeded" while file sizes differed; a suite read "ok, 0 passed".
  Read pass counts, compare bytes, screenshot the UI, mutation-test fixes.
- **Remote-write policy.** Routine commits/pushes to master are authorized
  (standing grant), Conventional Commits, each push gated by the checks above.
  Do not commit or push without the gate passing.
- **Owner gates** are the only things that should block; everything else is a
  task, not a question.
