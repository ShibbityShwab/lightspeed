# WAT — Workflows, Agents and Tools

The operating system for agents working on LightSpeed. Every file here is
loadable; nothing here duplicates another file.

## Read in this order

| File | What it is | Load when |
|---|---|---|
| [`../AGENTS.md`](../AGENTS.md) | The contract: start here, tool rules, gates, state updates | Every session (loaded automatically) |
| [`rules.md`](rules.md) | Policy stubs: cost, safety, security, transparency, quality, ethics, privacy, shell, agent, state | Every session, before acting |
| [`state/current-phase.md`](state/current-phase.md) | Live workstream + newest dated entries | Every session, first |
| [`state/phases/`](state/phases/) | Older phase entries by month | When the current entry points at history |
| [`state/decisions.md`](state/decisions.md) | Decision log index and format contract | Before a decision, or to find one |
| [`state/decisions/<YYYY-MM>.md`](state/decisions/) | The decision entries themselves | When you need the reasoning |
| [`personas/`](personas/README.md) | Eight delegation contracts | When delegating or adopting a role |
| [`team-mode.md`](team-mode.md) | Agent Teams: task board, envelope, ownership | Multi-agent work |
| [`state/HANDOFF.md`](state/HANDOFF.md) | Context that is not in state files | First session after a long gap |

## Procedures are skills

The step-by-step procedures live in `.dsh/skills/` so DeepSeek Harness can load
them on demand instead of paying for them on every request:

| Skill | Covers |
|---|---|
| `lightspeed-wat` | the loop, state protocol, persona adoption |
| `lightspeed-team` | Team mode, task board, delegation envelope |
| `lightspeed-windows` | PowerShell 5.1, toolchain reality, GUI quirks |
| `lightspeed-verify` | gates, evidence, mutation checks, per-area minimums |

Load one with the `skill` tool. If a procedure is missing from both this map and
the skills, it does not exist yet — write it down before doing it twice.

## Directory map

```text
wat/
  rules.md              policy stubs (binding)
  team-mode.md          Agent Teams contract
  personas/             one file per persona + roster JSON + schema
  state/
    current-phase.md    hot state: live workstream + newest entries
    phases/             monthly phase archives + index
    decisions.md        decision index + format contract
    decisions/          monthly decision entries + template
    HANDOFF.md          newcomer brief
  archive/              historical only, never instructions
```

## Conventions

- **Dates** are `YYYY-MM-DD` and are the only sort key.
- **Entry ids**: workflows are `WF-NNN`; use the next free number for a new
  workflow, and name it in the state entry.
- **Evidence**: `wat/evidence/<YYYY-MM-DD>/` (gitignored).
- **No file named `AGENTS.md`/`agents.md`** outside the project root — on Windows
  it is injected as workspace instructions (see `archive/README.md`).
