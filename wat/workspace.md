# 🚀 LightSpeed WAT Workspace

> **Workflows, Agents, and Tools** — Autonomous AI-Driven Project Management
> Version: `0.1.0` | Created: 2026-02-21 | Engine: Claude Opus 4.6+

---

## 1. Project Identity

| Field | Value |
|-------|-------|
| **Name** | LightSpeed |
| **Codename** | `lightspeed` |
| **Mission** | Zero-cost global network optimizer SaaS — reduce and stabilize ping/latency for multiplayer games |
| **Target** | ExitLag alternative via free-tier infrastructure, Rust client, AI-driven routing |
| **License** | TBD (see `rules.md` → `[LEGAL_STUB]`) |

---

## 2. Directory Structure

```
lightspeed/
├── wat/                          # ← YOU ARE HERE — WAT System
│   ├── workspace.md              # Root overview (this file)
│   ├── agents.md                 # Agent definitions & roles
│   ├── workflows.md              # Task sequences & chains
│   ├── tools.md                  # Tool stubs & MCP wrappers
│   ├── rules.md                  # LLM rule stubs
│   ├── mcp-integration.md        # MCP server specs
│   ├── project-goals.md          # Goals, research, architecture
│   ├── autonomy-loop.md          # Master autonomous prompt
│   └── state/                    # Runtime state (auto-generated)
│       ├── current-phase.md      # Current workflow phase
│       ├── agent-logs/           # Per-agent execution logs
│       ├── checkpoints/          # Workflow checkpoints
│       └── decisions.md          # Decision log
│
├── client/                       # Rust client (Tokio, pcap-rs, quinn, linfa)
│   ├── Cargo.toml
│   ├── src/
│   │   ├── main.rs
│   │   ├── tunnel/               # UDP tunneling engine
│   │   ├── route/                # AI route optimizer
│   │   ├── capture/              # Packet capture (pcap-rs)
│   │   ├── quic/                 # QUIC control plane (quinn)
│   │   └── ml/                   # ML models (linfa)
│   └── tests/
│
├── infra/                        # Infrastructure as Code
│   ├── terraform/                # Oracle Cloud Free Tier provisioning
│   ├── docker/                   # Container definitions
│   ├── scripts/                  # Deployment scripts
│   └── configs/                  # Proxy/server configurations
│
├── proxy/                        # Proxy server (Rust)
│   ├── Cargo.toml
│   └── src/
│
├── ai/                           # AI/ML models and training
│   ├── models/
│   ├── data/
│   └── training/
│
├── docs/                         # Documentation
│   ├── architecture.md
│   ├── protocol.md
│   └── api.md
│
├── tests/                        # Integration & E2E tests
│   ├── latency-bench/
│   └── game-tests/
│
└── web/                          # Landing page & dashboard
    ├── landing/
    └── dashboard/
```

---

## 3. How to Run the WAT

### Quick Start (Copy-Paste Bootstrap)

1. **Open Claude** (or any compatible LLM)
2. **Paste the contents of `autonomy-loop.md`** as the system/user prompt
3. **The WAT activates**: agents are loaded, workflows begin, tools are called
4. **Interact or let it run**: provide guidance or let autonomous mode execute

### REPL Execution Model

```
┌─────────────────────────────────────────────┐
│              AUTONOMY LOOP                   │
│                                              │
│  ┌─────────┐   ┌──────────┐   ┌──────────┐ │
│  │  READ   │──▶│  PLAN    │──▶│ EXECUTE  │ │
│  │  STATE  │   │  (Agent) │   │  (Tool)  │ │
│  └─────────┘   └──────────┘   └──────────┘ │
│       ▲                            │         │
│       │         ┌──────────┐       │         │
│       └─────────│  VERIFY  │◀──────┘         │
│                 │  & LOG   │                 │
│                 └──────────┘                 │
└─────────────────────────────────────────────┘
```

Each iteration:
1. **READ STATE** → Check `state/current-phase.md` for where we are
2. **PLAN** → Select appropriate agent from `agents.md`, identify next workflow step from `workflows.md`
3. **EXECUTE** → Agent uses tools from `tools.md`, respecting rules from `rules.md`
4. **VERIFY & LOG** → Check outputs, update state, log decisions

### Manual Invocation

To run a specific workflow step:
```
EXECUTE: WF-001, Step 3
AGENT: RustDev
CONTEXT: [paste relevant state]
```

To invoke a specific agent:
```
ACTIVATE AGENT: NetEng
TASK: Design UDP tunnel packet format
CONSTRAINTS: [TRANSPARENCY_STUB], [SECURITY_STUB]
OUTPUT: Protocol specification in Markdown
```

---

## 4. State Management

### State Convention

All runtime state lives in `wat/state/`. Files are Markdown for human readability and LLM parseability.

```markdown
<!-- state/current-phase.md -->
# Current Phase

| Key | Value |
|-----|-------|
| **Active Workflow** | WF-001 (MVP Build) |
| **Current Step** | Step 2: Core Tunnel Engine |
| **Active Agents** | RustDev, NetEng |
| **Blocked On** | None |
| **Last Checkpoint** | 2026-02-21T13:00Z |
| **Next Action** | Implement UDP relay in `client/src/tunnel/` |
```

### Decision Log

Every significant decision is recorded:
```markdown
<!-- state/decisions.md -->
## DEC-001: Use Quinn for Control Plane
- **Date**: 2026-02-21
- **Agent**: Architect
- **Reason**: QUIC provides reliable, multiplexed control channel while keeping data plane as raw UDP
- **Alternatives Considered**: TCP control, gRPC
- **Status**: ACCEPTED
```

---

## 5. Cross-References

| File | Purpose | Key Sections |
|------|---------|-------------|
| [`agents.md`](agents.md) | Who does what | Agent cards, capabilities, stubs |
| [`workflows.md`](workflows.md) | Task sequences | WF-001 through WF-006 |
| [`tools.md`](tools.md) | How things get done | Tool interfaces, MCP wrappers |
| [`rules.md`](rules.md) | Constraints & policies | Safety, ethics, legal stubs |
| [`mcp-integration.md`](mcp-integration.md) | Automation layer | MCP servers, schemas |
| [`project-goals.md`](project-goals.md) | Why & what | Architecture, targets, research |
| [`autonomy-loop.md`](autonomy-loop.md) | Execution engine | Master prompt, state machine |

---

## 6. Version History

| Version | Date | Changes |
|---------|------|---------|
| `0.1.0` | 2026-02-21 | Initial WAT creation — all 8 files |

---

## 7. Conventions

### Naming
- **Agents**: PascalCase (e.g., `RustDev`, `NetEng`)
- **Workflows**: `WF-XXX` (e.g., `WF-001`)
- **Tools**: PascalCase (e.g., `CodeGen`, `PingBenchmark`)
- **Rules/Stubs**: `[UPPERCASE_STUB]` (e.g., `[SAFETY_STUB]`)
- **Decisions**: `DEC-XXX` (e.g., `DEC-001`)

### Output Formats
- **Code**: Fenced code blocks with language tags
- **Structured Data**: Markdown tables or YAML front matter
- **Tool Calls**: XML-style `<tool_call>` blocks (see `tools.md`)
- **Agent Handoffs**: Explicit `HANDOFF:` directives

### Parallel Execution
Agents marked as `parallel: true` in workflows can execute simultaneously. Their outputs are merged at synchronization points (marked `sync: true`).

```
[AgentA] ──┐
            ├──▶ [Sync Point] ──▶ [AgentC]
[AgentB] ──┘
```
