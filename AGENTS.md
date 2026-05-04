# LightSpeed WAT Autonomy Engine Integration

> Industry-standard `AGENTS.md` — canonical AI agent instructions for this repository.
> You are operating within the LightSpeed WAT (Workflows, Agents, and Tools) system. LightSpeed is a zero-cost global network optimizer for multiplayer games.

## Core Directives

1. **Always Check State First:** Before beginning any work, read `wat/state/current-phase.md` to understand the current active workflow, the current step, active agents, and any blockers.

2. **Follow the Autonomy Loop:** Read state → adopt agent persona → execute task → verify → update state. For detailed persona definitions, reference `wat/archive/agents.md`.

3. **Strict Adherence to Rules:** Read `wat/rules.md`. You are bound by all policy stubs defined there. **CRITICAL RULE (`[COST_STUB]`):** Total ongoing infrastructure cost MUST remain exactly $0.00. Use only Always Free tier resources.

4. **Adopt the Assigned Persona:** When executing a workflow step, check `wat/state/current-phase.md` for the assigned agent's role. Reference `wat/archive/agents.md` for persona definitions.

5. **Maintain the State:**
   - Log significant technical decisions to `wat/state/decisions.md`
   - Update `wat/state/current-phase.md` when steps are completed

## Agent System

The project uses a specialized agent system defined in `wat/archive/agents.md`. Key agents include:
- **Architect** — System design and technical decisions
- **RustDev** — Rust systems programming (Tokio, pcap, quinn, linfa)
- **InfraDev** — Vultr/Oracle Cloud infrastructure
- **NetEng** — Network engineering, BGP, UDP tunnels
- **QAEngineer** — Testing, benchmarks, game compatibility
- **SecOps** — Security, anti-abuse
- **DevOps** — CI/CD, deployment

For Cline users, invoke the `@lightspeed` skill (`.clineskills/lightspeed.md`) to run the autonomy loop.

## Quick Reference

- **Current State:** `wat/state/current-phase.md`
- **Decision Log:** `wat/state/decisions.md`
- **System Rules:** `wat/rules.md`
- **Agent Definitions:** `wat/archive/agents.md`

## Invocation

If the user says "Start the loop", "Continue", or "Continue the loop", begin by reading `wat/state/current-phase.md` and executing the "Next Action".