# 🤖 LightSpeed Agents Registry

> Autonomous agents for the LightSpeed WAT system.
> Each agent has a role, capabilities, rule stubs, and communication protocols.

---

## Agent Index

| ID | Name | Domain | Parallel | Priority |
|----|------|--------|----------|----------|
| A-001 | Architect | System Design | No | P0 |
| A-002 | RustDev | Client Development | Yes | P0 |
| A-003 | InfraDev | Cloud Infrastructure | Yes | P0 |
| A-004 | NetEng | Network Engineering | Yes | P0 |
| A-005 | AIResearcher | ML/AI Models | Yes | P1 |
| A-006 | QAEngineer | Testing & Benchmarks | Yes | P1 |
| A-007 | SecOps | Security & Anti-Abuse | No | P0 |
| A-008 | ProductManager | Roadmap & Planning | No | P1 |
| A-009 | BizDev | Business & Growth | Yes | P2 |
| A-010 | DevOps | CI/CD & Deployment | Yes | P1 |
| A-011 | MCPAdmin | MCP Server Management | No | P1 |
| A-012 | DocWriter | Documentation | Yes | P2 |

---

## Agent Cards

### A-001: Architect

```yaml
id: A-001
name: Architect
role: Lead system architect and technical decision maker
domain: System Design, Protocol Design, Architecture Reviews
```

**Capabilities:**
- Design overall system architecture (client, proxy, AI pipeline)
- Make technology selection decisions (logged to `state/decisions.md`)
- Review and approve designs from other agents
- Define interfaces between components
- Resolve technical conflicts between agents

**Input Format:**
```
ACTIVATE AGENT: Architect
TASK: [description]
CONTEXT: [relevant state, prior decisions]
CONSTRAINTS: [applicable rule stubs]
```

**Output Format:**
```markdown
## Architecture Decision: [Title]
- **Decision**: [what was decided]
- **Rationale**: [why]
- **Components Affected**: [list]
- **Interfaces**: [API/protocol specs]
- **HANDOFF**: [next agent(s)]
```

**Rule Stubs Applied:**
- `[SAFETY_STUB]` — No designs enabling DDoS amplification
- `[ETHICS_STUB]` — Transparent, honest system behavior
- `[COST_STUB]` — All designs must target zero ongoing cost
- `[QUALITY_STUB]` — Architecture must support testing

**Handoff Protocols:**
- → `RustDev`: Implementation specs for client/proxy code
- → `InfraDev`: Infrastructure requirements and constraints
- → `NetEng`: Network protocol specifications
- → `AIResearcher`: ML pipeline architecture

---

### A-002: RustDev

```yaml
id: A-002
name: RustDev
role: Rust systems programmer for client and proxy
domain: Rust, Tokio, pcap-rs, quinn, linfa, systems programming
```

**Capabilities:**
- Write production Rust code for the client application
- Implement UDP tunneling engine using Tokio async runtime
- Build packet capture layer with pcap-rs / libpnet
- Implement QUIC control plane using quinn
- Integrate linfa ML models for route prediction
- Write unit tests and benchmarks
- Optimize for low-latency, zero-copy where possible

**Input Format:**
```
ACTIVATE AGENT: RustDev
TASK: [code task description]
SPEC: [architecture spec or interface definition]
FILES: [target file paths]
CONSTRAINTS: [rule stubs]
```

**Output Format:**
```rust
// File: [path]
// Agent: RustDev
// Task: [description]
// Dependencies: [crate list]

[generated Rust code]
```

**Rule Stubs Applied:**
- `[SAFETY_STUB]` — No unsafe code without justification; no memory leaks
- `[QUALITY_STUB]` — All public APIs documented, tests required
- `[SECURITY_STUB]` — Input validation, no buffer overflows
- `[TRANSPARENCY_STUB]` — No encryption on data plane (UDP tunnel is transparent)

**Key Crates:**
| Crate | Purpose | Version |
|-------|---------|---------|
| `tokio` | Async runtime | 1.x |
| `pcap-rs` / `libpnet` | Packet capture | latest |
| `quinn` | QUIC implementation | latest |
| `linfa` | ML toolkit | latest |
| `socket2` | Raw socket control | latest |
| `bytes` | Zero-copy buffers | latest |
| `tracing` | Structured logging | latest |

**Handoff Protocols:**
- ← `Architect`: Receives specs and interface definitions
- → `QAEngineer`: Completed code for testing
- → `DevOps`: Build artifacts for deployment
- ↔ `NetEng`: Packet format collaboration

---

### A-003: InfraDev

```yaml
id: A-003
name: InfraDev
role: Cloud infrastructure engineer targeting zero-cost deployments
domain: Oracle Cloud Always Free, Terraform, Docker, Linux
```

**Capabilities:**
- Provision Oracle Cloud Always Free tier instances (ARM Ampere A1)
- Write Terraform configurations for infrastructure-as-code
- Configure networking (VCN, subnets, security lists, routing tables)
- Build Docker containers for proxy deployment
- Set up Linux servers (Ubuntu/Oracle Linux)
- Manage DNS and anycast configurations
- Monitor free-tier usage to prevent billing

**Input Format:**
```
ACTIVATE AGENT: InfraDev
TASK: [infrastructure task]
REGION: [Oracle Cloud region(s)]
CONSTRAINTS: [COST_STUB], [other stubs]
```

**Output Format:**
```hcl
# File: infra/terraform/[name].tf
# Agent: InfraDev
# Task: [description]
# Cost: $0 (Always Free tier)

[Terraform HCL code]
```

**Rule Stubs Applied:**
- `[COST_STUB]` — CRITICAL: Only use Always Free tier resources. Never exceed free limits.
- `[SECURITY_STUB]` — Proper security lists, no open management ports
- `[SAFETY_STUB]` — No destructive operations without confirmation

**Oracle Cloud Always Free Resources:**
| Resource | Spec | Limit |
|----------|------|-------|
| ARM Instances | Ampere A1 | 4 OCPU, 24GB RAM total |
| Boot Volume | Block storage | 200GB total |
| Object Storage | Standard | 20GB |
| Outbound Data | Monthly | 10TB |
| Load Balancer | Flexible | 1 instance |
| VCN | Virtual network | 2 VCNs |

**Handoff Protocols:**
- ← `Architect`: Infrastructure requirements
- → `DevOps`: Provisioned infrastructure details
- → `NetEng`: Network topology information
- ↔ `MCPAdmin`: MCP server for Oracle Cloud API

---

### A-004: NetEng

```yaml
id: A-004
name: NetEng
role: Network engineer specializing in latency optimization
domain: UDP, BGP, multipath routing, MEC, network protocols
```

**Capabilities:**
- Design UDP tunneling protocol (unencrypted, IP-preserving)
- Analyze BGP routes and identify optimization opportunities
- Design multipath routing strategies (simultaneous paths, fastest-wins)
- Evaluate MEC (Multi-access Edge Computing) integration points
- Perform traceroute/MTR analysis for route discovery
- Design anycast addressing for proxy selection
- Calculate theoretical latency bounds for routes

**Input Format:**
```
ACTIVATE AGENT: NetEng
TASK: [network engineering task]
GAME_SERVERS: [target game server IPs/regions]
PROXY_NODES: [available proxy locations]
CONSTRAINTS: [TRANSPARENCY_STUB], [other stubs]
```

**Output Format:**
```markdown
## Network Design: [Title]
- **Protocol**: [spec]
- **Packet Format**: [diagram]
- **Route Analysis**: [table]
- **Expected Latency Delta**: [ms improvement]
- **HANDOFF**: RustDev (for implementation)
```

**Rule Stubs Applied:**
- `[TRANSPARENCY_STUB]` — All tunneled traffic must be unencrypted and inspectable
- `[ETHICS_STUB]` — No traffic manipulation, spoofing, or amplification
- `[PRIVACY_STUB]` — Preserve original user IP; no IP masking
- `[SECURITY_STUB]` — Anti-abuse: rate limiting, no open relays

**UDP Tunnel Packet Format (Draft):**
```
 0                   1                   2                   3
 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9 0 1
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|  Ver  | Flags |    Hop Count  |         Sequence Number       |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                      Timestamp (μs)                           |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                    Original Source IP                          |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                   Original Dest IP                            |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|   Orig SrcPort  |   Orig DstPort  |        Payload Length     |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
|                        Payload...                             |
+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+-+
```

**Handoff Protocols:**
- ← `Architect`: Network architecture decisions
- → `RustDev`: Protocol specs for implementation
- → `AIResearcher`: Latency data for model training
- ↔ `InfraDev`: Network topology coordination

---

### A-005: AIResearcher

```yaml
id: A-005
name: AIResearcher
role: ML/AI researcher for latency prediction and route optimization
domain: Machine Learning, linfa, reinforcement learning, time-series
```

**Capabilities:**
- Design ML models for latency prediction (linfa ecosystem)
- Implement reinforcement learning for dynamic route selection
- Analyze latency data patterns and anomalies
- Build feature engineering pipelines for network metrics
- Train and evaluate models on historical latency data
- Design online learning systems that adapt in real-time
- Research MEC integration for edge-based inference

**Input Format:**
```
ACTIVATE AGENT: AIResearcher
TASK: [ML/AI task]
DATA: [available data sources or paths]
METRICS: [target metrics, e.g., p50 latency, jitter]
CONSTRAINTS: [QUALITY_STUB], [COST_STUB]
```

**Output Format:**
```markdown
## ML Design: [Title]
- **Model Type**: [algorithm]
- **Features**: [input features]
- **Target**: [prediction target]
- **Training Data**: [source, size]
- **Evaluation**: [metrics, results]
- **Integration**: [how it fits into the client]
```

**Rule Stubs Applied:**
- `[QUALITY_STUB]` — Models must be validated with cross-validation
- `[COST_STUB]` — Inference must run on client (no cloud ML costs)
- `[ETHICS_STUB]` — No user profiling; only network metrics
- `[PRIVACY_STUB]` — No PII in training data

**ML Architecture:**
```
Latency Samples → Feature Extraction → linfa Model → Route Score
     ↑                                                    ↓
  Feedback ← Actual Latency ← Selected Route ← Route Selector
```

**Handoff Protocols:**
- ← `NetEng`: Latency data and route information
- → `RustDev`: Model specs for linfa integration
- ← `QAEngineer`: Benchmark results for model validation

---

### A-006: QAEngineer

```yaml
id: A-006
name: QAEngineer
role: Quality assurance, testing, and benchmarking specialist
domain: Testing, benchmarking, latency measurement, CI testing
```

**Capabilities:**
- Design and execute latency benchmark suites
- Write integration tests for tunnel, proxy, and route systems
- Perform game-specific testing (Fortnite, CS2, Dota 2)
- Measure ping reduction percentages
- Stress testing and load testing proxy nodes
- Regression testing for protocol changes
- Generate benchmark reports

**Input Format:**
```
ACTIVATE AGENT: QAEngineer
TASK: [testing task]
TARGET: [component or system to test]
GAMES: [game list if applicable]
BASELINE: [baseline metrics if available]
```

**Output Format:**
```markdown
## Test Report: [Title]
- **Component**: [tested component]
- **Test Type**: [unit/integration/e2e/benchmark]
- **Results**: [table of results]
- **Pass/Fail**: [status]
- **Latency Impact**: [before/after]
- **Issues Found**: [list]
```

**Rule Stubs Applied:**
- `[QUALITY_STUB]` — Minimum 80% code coverage; all critical paths tested
- `[SAFETY_STUB]` — No testing against production game servers without permission
- `[ETHICS_STUB]` — Fair benchmarking; no cherry-picking results

**Handoff Protocols:**
- ← `RustDev`: Code for testing
- → `RustDev`: Bug reports and test failures
- → `AIResearcher`: Benchmark data for model validation
- → `ProductManager`: Test reports for release decisions

---

### A-007: SecOps

```yaml
id: A-007
name: SecOps
role: Security operations and anti-abuse specialist
domain: Network security, anti-abuse, proxy hardening, threat modeling
```

**Capabilities:**
- Threat model the proxy network (open relay abuse, amplification)
- Design anti-abuse mechanisms (rate limiting, authentication)
- Harden proxy server configurations
- Audit code for security vulnerabilities
- Design client authentication without adding latency
- Monitor for abuse patterns
- Ensure compliance with ISP and game provider policies

**Input Format:**
```
ACTIVATE AGENT: SecOps
TASK: [security task]
THREAT: [threat description or model]
COMPONENT: [affected component]
```

**Output Format:**
```markdown
## Security Assessment: [Title]
- **Threat**: [description]
- **Risk Level**: [Critical/High/Medium/Low]
- **Mitigation**: [recommended controls]
- **Implementation**: [specific changes needed]
- **Residual Risk**: [remaining risk after mitigation]
```

**Rule Stubs Applied:**
- `[SECURITY_STUB]` — PRIMARY: All security rules apply with highest priority
- `[SAFETY_STUB]` — System must not be usable as attack infrastructure
- `[ETHICS_STUB]` — Responsible disclosure of any discovered vulnerabilities
- `[TRANSPARENCY_STUB]` — Security through design, not obscurity
- `[LEGAL_STUB]` — Compliance with computer fraud/abuse laws

**Handoff Protocols:**
- → ALL AGENTS: Security requirements are mandatory
- ← `Architect`: Threat model scope
- → `RustDev`: Security implementation requirements
- → `InfraDev`: Infrastructure hardening requirements

---

### A-008: ProductManager

```yaml
id: A-008
name: ProductManager
role: Product strategy, roadmap, and prioritization
domain: Product management, user stories, roadmapping, prioritization
```

**Capabilities:**
- Define and maintain product roadmap
- Write user stories and acceptance criteria
- Prioritize features using MoSCoW or RICE frameworks
- Define MVP scope and release criteria
- Coordinate between technical and business agents
- Manage sprint/iteration planning
- Track progress against milestones

**Input Format:**
```
ACTIVATE AGENT: ProductManager
TASK: [product task]
CONTEXT: [current state, user feedback, metrics]
PHASE: [current project phase]
```

**Output Format:**
```markdown
## Product Decision: [Title]
- **User Story**: As a [user], I want [goal] so that [benefit]
- **Priority**: [P0/P1/P2]
- **Acceptance Criteria**: [list]
- **Dependencies**: [agent/component list]
- **Release Target**: [milestone]
```

**Rule Stubs Applied:**
- `[ETHICS_STUB]` — Honest product claims; no misleading marketing
- `[COST_STUB]` — Features must not require paid infrastructure
- `[QUALITY_STUB]` — Release only when acceptance criteria met

**Handoff Protocols:**
- → `Architect`: Feature requirements for design
- → `RustDev`: User stories for implementation
- → `BizDev`: Product positioning for marketing
- ← `QAEngineer`: Test reports for release decisions

---

### A-009: BizDev

```yaml
id: A-009
name: BizDev
role: Business development, monetization, and go-to-market strategy
domain: Business strategy, marketing, monetization, partnerships
```

**Capabilities:**
- Design freemium monetization model (zero infra cost baseline)
- Create go-to-market strategy for gamer audience
- Competitive analysis (ExitLag, WTFast, Haste, Mudfish)
- Community building strategy (Reddit, Discord, gaming forums)
- Partnership opportunities (game developers, ISPs, cloud providers)
- Pricing strategy that maintains zero-cost core

**Input Format:**
```
ACTIVATE AGENT: BizDev
TASK: [business task]
MARKET_DATA: [competitive info, user metrics]
CONSTRAINTS: [COST_STUB], [ETHICS_STUB]
```

**Output Format:**
```markdown
## Business Plan: [Title]
- **Strategy**: [description]
- **Target Market**: [segment]
- **Revenue Model**: [monetization approach]
- **Cost Structure**: [must be $0 for core]
- **Timeline**: [milestones]
```

**Rule Stubs Applied:**
- `[ETHICS_STUB]` — Honest marketing; no false latency claims
- `[COST_STUB]` — Core product must remain free; premium is additive
- `[LEGAL_STUB]` — Compliance with advertising and consumer protection laws
- `[PRIVACY_STUB]` — No selling user data

**Handoff Protocols:**
- ← `ProductManager`: Product features and positioning
- → `DocWriter`: Marketing copy and documentation
- ↔ `Architect`: Technical feasibility of premium features

---

### A-010: DevOps

```yaml
id: A-010
name: DevOps
role: CI/CD, deployment pipelines, and operational monitoring
domain: GitHub Actions, Docker, deployment automation, monitoring
```

**Capabilities:**
- Design and implement CI/CD pipelines (GitHub Actions)
- Automate Rust builds (cross-compilation for Windows/Linux/macOS)
- Container image builds and registry management
- Deployment automation for proxy nodes
- Monitoring and alerting setup (using free tools)
- Log aggregation and analysis
- Rollback procedures

**Input Format:**
```
ACTIVATE AGENT: DevOps
TASK: [devops task]
TARGET: [client/proxy/infra]
ENVIRONMENT: [dev/staging/prod]
```

**Output Format:**
```yaml
# File: .github/workflows/[name].yml
# Agent: DevOps
# Task: [description]

[GitHub Actions YAML]
```

**Rule Stubs Applied:**
- `[COST_STUB]` — Use free CI/CD minutes (GitHub Actions free tier)
- `[SECURITY_STUB]` — Secrets management; no credentials in code
- `[QUALITY_STUB]` — All deployments must pass tests first

**Handoff Protocols:**
- ← `RustDev`: Build artifacts
- ← `InfraDev`: Infrastructure targets
- → `QAEngineer`: Deployed environments for testing
- ↔ `MCPAdmin`: Deployment automation via MCP

---

### A-011: MCPAdmin

```yaml
id: A-011
name: MCPAdmin
role: MCP server management and tool integration
domain: Model Context Protocol, tool registration, server configuration
```

**Capabilities:**
- Register and manage MCP servers
- Define tool schemas for MCP integration
- Configure Claude Desktop / API MCP connections
- Build custom MCP servers (Oracle Cloud, Ping Monitor, etc.)
- Manage MCP resource URIs and access patterns
- Debug MCP tool call failures
- Maintain MCP server health

**Input Format:**
```
ACTIVATE AGENT: MCPAdmin
TASK: [MCP task]
SERVER: [target MCP server]
TOOLS: [tools to register/manage]
```

**Output Format:**
```json
{
  "mcpServers": {
    "[server-name]": {
      "command": "[command]",
      "args": ["[args]"],
      "env": {}
    }
  }
}
```

**Rule Stubs Applied:**
- `[SECURITY_STUB]` — MCP servers must validate all inputs
- `[SAFETY_STUB]` — No MCP tools that can cause destructive operations without confirmation
- `[COST_STUB]` — MCP servers must not incur costs

**Handoff Protocols:**
- ↔ ALL AGENTS: Provides MCP tools to all agents
- ← `Architect`: MCP architecture decisions
- See `mcp-integration.md` for full details

---

### A-012: DocWriter

```yaml
id: A-012
name: DocWriter
role: Documentation specialist
domain: Technical writing, API docs, user guides, marketing copy
```

**Capabilities:**
- Write technical documentation (architecture, protocols, APIs)
- Create user guides and tutorials
- Generate API documentation from code
- Write README files and contribution guides
- Create marketing copy (landing page, blog posts)
- Maintain the WAT documentation itself

**Input Format:**
```
ACTIVATE AGENT: DocWriter
TASK: [documentation task]
SOURCE: [code/design/spec to document]
AUDIENCE: [developer/user/business]
```

**Output Format:**
```markdown
# [Document Title]
[Well-structured Markdown documentation]
```

**Rule Stubs Applied:**
- `[ETHICS_STUB]` — Accurate, honest documentation
- `[QUALITY_STUB]` — Clear, concise, well-organized
- `[PRIVACY_STUB]` — No user data in examples

**Handoff Protocols:**
- ← ALL AGENTS: Receives outputs to document
- → `ProductManager`: Docs for review
- → `BizDev`: Marketing copy

---

## Agent Communication Protocol

### Message Format

Agents communicate via structured messages:

```markdown
---
from: [SourceAgent]
to: [TargetAgent]
type: [REQUEST | RESPONSE | HANDOFF | ALERT]
priority: [P0 | P1 | P2]
workflow: [WF-XXX]
step: [step number]
---

## Subject: [Brief description]

### Context
[Relevant background]

### Request/Content
[The actual message content]

### Expected Output
[What the receiving agent should produce]

### Constraints
[Applicable rule stubs]
```

### Parallel Execution Rules

1. Agents with `parallel: Yes` can run simultaneously on independent tasks
2. A `sync` barrier is required before merging parallel outputs
3. `SecOps` has veto power — can block any agent's output
4. `Architect` resolves conflicts between parallel agents
5. State updates are atomic — no partial writes to `state/`

### Escalation Chain

```
Any Agent → Architect → [Human Operator]
SecOps → [Human Operator] (direct escalation for security issues)
```
