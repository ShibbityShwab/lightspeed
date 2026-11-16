# LightSpeed — agent instructions

> Canonical entry point for AI agents working in this repository.
> Harness: **DeepSeek Harness (DSH)**. LightSpeed is a zero-cost global network
> optimizer for multiplayer games: a Rust workspace (`client/`, `proxy/`,
> `protocol/`, `client-gui/`) plus `infra/`, `web/`, `dist/`, `packaging/` and
> GitHub Actions.
> Last updated: 2026-11-16

## Start every session here

1. `wat/state/current-phase.md` — live workstream, current state, newest entries.
2. `wat/rules.md` — the policy stubs bind every action. Hard ones:
   **owner spend stays $0.00** (`[COST_STUB]`) and **Windows-native execution,
   no bash/Git Bash/WSL** (`[SHELL_STUB]`).
3. `wat/state/decisions.md` — the decision index; entries live in
   `wat/state/decisions/<YYYY-MM>.md`.

The WAT loop: **read state → adopt a persona → execute → verify → update state.**
"Start the loop", "Continue", and "Continue the loop" all mean: read
`wat/state/current-phase.md` and execute its next action.

## Load the procedure instead of guessing

Procedure skills live in this repo under `.dsh/skills/` and are discovered by
DSH automatically. Load the one that matches the job:

| Skill | Load it when |
|---|---|
| `lightspeed-wat` | starting, continuing or resuming work; the loop and state protocol |
| `lightspeed-team` | multi-agent work, personas, Agent Teams, parallel tasks |
| `lightspeed-windows` | running commands, builds, tests or the GUI on this host |
| `lightspeed-verify` | before claiming anything is verified; evidence rules |

## Team mode and personas

Personas are delegation contracts in `wat/personas/` (roster:
`wat/personas/README.md`). There are eight: `architect`, `rust-dev`,
`infra-dev`, `net-eng`, `qa-engineer`, `sec-ops`, `dev-ops`,
`interface-designer`.

Multi-persona work runs through **Agent Teams** — enable *Use Agent Teams* in the
root conversation's model & reasoning menu (per-Session, off by default). Then:
one shared task per deliverable, one owner per task, subagent name = persona
slug, acceptance criteria and write scopes on the task. Full contract and
delegation envelope: `wat/team-mode.md`.

The Lead (root agent) plans, integrates, re-verifies on the final tree, and owns
the final answer. A teammate's summary is a claim, not evidence.

## Execution rules (Windows host)

- PowerShell 5.1 only. **Never** invoke `bash`, `sh`, Git Bash, WSL or a
  POSIX-only script as a workaround. `git` runs directly.
- Use DSH tools: `read`/`write`/`edit` for files, `grep`/`glob` for discovery,
  `pwsh` for commands. No `cat`/`sed`/`find`/`findstr`.
- Rust toolchain 1.100.0 is installed. `python`/`python3` are Store stubs; `py`
  is missing. Localization and `tools/*.py` need a real CPython — a documented
  prerequisite, not an assumption to fake.
- Linux relay scripts (`infra/scripts/*.sh`) and macOS packaging are
  remote/CI work. Say so; do not port them to the desktop.
- Details, gates and GUI quirks: skill `lightspeed-windows`.

## Verification is mandatory

```powershell
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui
cargo test --workspace --all --exclude lightspeed-gui
# GUI (Windows only; excluded from workspace clippy everywhere):
cargo clippy -p lightspeed-gui --all-targets
cargo test -p lightspeed-gui
```

- Report counts per crate, never "tests pass".
- A bug fix needs a failing-before test and a mutation check (revert → fails →
  restore → passes).
- Evidence goes to `wat/evidence/<YYYY-MM-DD>/` and is named in the report.
- Full definition: skill `lightspeed-verify`.

## State updates end every round

- Append one dated entry at the top of `wat/state/current-phase.md` (what
  changed, why, verification with counts, what is open).
- Log a decision in `wat/state/decisions/<YYYY-MM>.md` when one was made.
- Keep `current-phase.md` small: older entries belong in
  `wat/state/phases/<YYYY-MM>.md`. History is append-only.
- Record in `wat/state/HANDOFF.md` only when handing the project to a new
  session with context the state files do not carry.

## Repository map

| Path | What it is |
|---|---|
| `client/` | engine: capture (pcap/WinDivert), QUIC control plane, FEC tunnel, routing, game profiles, CLI |
| `client-gui/` | the Windows egui tray app (the active product surface) |
| `proxy/` | the relay fleet binary |
| `protocol/` | shared wire types and FEC primitives |
| `infra/` | Terraform, Docker, relay scripts, monitoring collectors |
| `web/` | the GitHub Pages site (static; locales generated) |
| `dist/`, `packaging/` | Scoop / Chocolatey / winget / MSI packaging |
| `.github/workflows/` | CI, release, pages, docker, localization, security, monitors |
| `wat/` | agents, rules, personas, team contract, durable state |
| `docs/` | user docs, design docs, protocol, operations |

## Owner gates — ask, do not assume

One crisp question via `ask_user_question` for: product decisions, spending
anything at all, publishing a release or a public post, credentials/API keys,
and any destructive fleet operation. Bundle related gates into one question.

## Guardrails

- No paid service, no usage-based billing, no trial that converts (`[COST_STUB]`).
- No payload modification, no IP masquerade, no anti-cheat bypass
  (`[TRANSPARENCY_STUB]`, `[ETHICS_STUB]`).
- No secrets in the repo, fixtures, logs or evidence (`[SECURITY_STUB]`).
- No harness drift: no `.opencode/`, no OmO config, no second agent system
  (`[AGENT_STUB]`).
