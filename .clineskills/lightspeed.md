# @lightspeed — LightSpeed WAT Autonomy Skill

> Activates the LightSpeed WAT (Workflows, Agents, Tools) autonomy loop for the LightSpeed project.
> Invoke with: "@lightspeed start" or "Continue the loop"

## Behavior

When invoked, execute this sequence:

### 1. Read State
Read `wat/state/current-phase.md` to determine:
- Active agent persona to adopt
- Current workflow step and next action
- Any blockers

### 2. Read Task Definition (Optional)
If `wat/TASK.md` exists, read it for:
- Structured success criteria
- Test commands to run
- Expected output
- Files involved

### 3. Read Decisions Log
Read `wat/state/decisions.md` (last 5-10 entries) for recent context.

### 4. Adopt Agent Persona
From `wat/state/current-phase.md`, identify the active agent. Reference `wat/archive/agents.md` for the full persona definition if needed:
- **Architect** — system design, architecture decisions
- **RustDev** — Rust systems programming (Tokio, pcap, quinn, linfa)
- **InfraDev** — Vultr/Oracle Cloud infrastructure
- **NetEng** — network engineering, BGP, UDP tunnels
- **QAEngineer** — testing, benchmarks, game compatibility
- **SecOps** — security, anti-abuse
- **DevOps** — CI/CD, deployment
- **DocWriter** — documentation
- **etc.** — see `wat/archive/agents.md` for full list

### 5. Execute Task
Perform the "Next Action" from `wat/state/current-phase.md`. Verify against any success criteria in `wat/TASK.md`.

### 6. Verify & Update
After completing the step:
- Run `cargo test --workspace --all --exclude lightspeed-gui` and confirm all pass
- Run `cargo clippy --workspace --all-targets --all-features --exclude lightspeed-gui` and confirm clean
- Update `wat/state/current-phase.md` — mark step complete, set next action
- Log to `wat/state/decisions.md` — add any significant decisions made
- Report progress