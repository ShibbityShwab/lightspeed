# ⚡ LightSpeed Workflows

> Task sequences, dependency chains, and execution plans for the LightSpeed WAT system.
> Workflows chain agents via outputs. Each step has clear inputs, outputs, and checkpoints.

---

## Workflow Index

| ID | Name | Phase | Priority | Est. Duration | Status |
|----|------|-------|----------|---------------|--------|
| WF-001 | MVP Build | Development | P0 | 4-6 weeks | NOT_STARTED |
| WF-002 | Proxy Network Setup | Infrastructure | P0 | 2-3 weeks | NOT_STARTED |
| WF-003 | AI Route Optimizer | AI/ML | P1 | 3-4 weeks | NOT_STARTED |
| WF-004 | Game Integration | Integration | P0 | 2-3 weeks | NOT_STARTED |
| WF-005 | Scaling & Monitoring | Operations | P1 | 2 weeks | NOT_STARTED |
| WF-006 | Business Launch | Business | P2 | 3-4 weeks | NOT_STARTED |

### Dependency Graph

```
WF-001 (MVP) ──────────────┐
                            ├──▶ WF-004 (Game Integration) ──▶ WF-005 (Scaling)
WF-002 (Proxy Network) ────┘                                        │
                                                                     ▼
WF-003 (AI Route) ─────────────────────────────────────────▶ WF-005 (Scaling)
                                                                     │
                                                                     ▼
                                                              WF-006 (Launch)
```

**Parallel tracks:** WF-001 + WF-002 + WF-003 can begin simultaneously.
**Sync point:** WF-004 requires WF-001 (client) and WF-002 (proxies) to be complete.

---

## WF-001: MVP Build

> Build the minimum viable product: a Rust client that captures game UDP packets and tunnels them through a single proxy node.

### Overview
| Field | Value |
|-------|-------|
| **Goal** | Working UDP tunnel from client → proxy → game server |
| **Agents** | Architect, RustDev, NetEng, QAEngineer, SecOps |
| **Input** | Project goals (`project-goals.md`) |
| **Output** | Compiled Rust binary, passing basic latency tests |
| **Success Criteria** | Tunneled ping ≤ direct ping + 5ms overhead |

### Steps

#### Step 1: Architecture Design
```yaml
step: 1
name: Architecture Design
agent: Architect
parallel: false
depends_on: []
estimated: 2 days
```

**Task:** Design the complete client architecture including module layout, data flow, and interfaces.

**Input:**
- `project-goals.md` — technical requirements
- `agents.md` → NetEng packet format

**Output:**
```markdown
## Client Architecture Document
- Module dependency graph
- Data flow diagram (packet capture → tunnel → proxy → game server)
- Interface definitions (Rust traits)
- Crate selection rationale
- HANDOFF: RustDev (Step 2), NetEng (Step 2)
```

**Checkpoint:** Architecture document reviewed and saved to `docs/architecture.md`

---

#### Step 2: Core Tunnel Engine (Parallel Track A)
```yaml
step: 2a
name: Core Tunnel Engine
agent: RustDev
parallel: true
depends_on: [step_1]
estimated: 1 week
```

**Task:** Implement the UDP tunnel engine — capture outbound game packets, wrap in tunnel header, send to proxy, receive response, unwrap, deliver to game.

**Input:**
- Architecture document from Step 1
- Packet format from NetEng (agents.md)

**Output:**
```rust
// client/src/tunnel/mod.rs — Tunnel engine
// client/src/tunnel/capture.rs — Packet capture (pcap-rs)
// client/src/tunnel/relay.rs — UDP relay logic
// client/src/tunnel/header.rs — Tunnel header encode/decode
```

**Tool Calls:**
```xml
<tool_call name="CodeGen">
  <param name="language">rust</param>
  <param name="module">tunnel</param>
  <param name="spec">UDP tunnel engine with Tokio async, pcap-rs capture</param>
  <param name="output_path">client/src/tunnel/</param>
</tool_call>
```

**Checkpoint:** `cargo build` succeeds, unit tests pass for header encode/decode

---

#### Step 2b: Protocol Design (Parallel Track B)
```yaml
step: 2b
name: Protocol Design
agent: NetEng
parallel: true
depends_on: [step_1]
estimated: 3 days
```

**Task:** Finalize the UDP tunnel protocol specification: header format, handshake, keepalive, error codes.

**Input:**
- Architecture document from Step 1
- Draft packet format from `agents.md` → NetEng

**Output:**
```markdown
# LightSpeed Tunnel Protocol v1
## docs/protocol.md
- Header format (final)
- Handshake sequence
- Keepalive mechanism
- Error codes
- MTU considerations
- Fragmentation handling
```

**Checkpoint:** Protocol document saved and reviewed by Architect

---

#### Step 3: Proxy Server
```yaml
step: 3
name: Proxy Server Implementation
agent: RustDev
parallel: false
depends_on: [step_2a, step_2b]
estimated: 1 week
```

**Task:** Build the proxy server that receives tunneled packets, strips the header, forwards to game server, captures response, re-wraps, and returns to client.

**Input:**
- Protocol spec from Step 2b
- Architecture document

**Output:**
```rust
// proxy/src/main.rs — Proxy server entry point
// proxy/src/relay.rs — Packet relay logic
// proxy/src/auth.rs — Client authentication (lightweight)
// proxy/src/metrics.rs — Latency/throughput metrics
```

**Tool Calls:**
```xml
<tool_call name="CodeGen">
  <param name="language">rust</param>
  <param name="module">proxy</param>
  <param name="spec">UDP proxy server: receive tunnel packets, forward to game, return response</param>
  <param name="output_path">proxy/src/</param>
</tool_call>
```

**Checkpoint:** Proxy compiles, handles echo test packets correctly

---

#### Step 4: QUIC Control Plane
```yaml
step: 4
name: QUIC Control Plane
agent: RustDev
parallel: false
depends_on: [step_3]
estimated: 4 days
```

**Task:** Implement the QUIC-based control plane using quinn for: proxy discovery, route negotiation, health checks, configuration updates.

**Input:**
- Architecture document
- Protocol spec

**Output:**
```rust
// client/src/quic/mod.rs — QUIC control client
// client/src/quic/discovery.rs — Proxy discovery
// client/src/quic/health.rs — Health check protocol
// proxy/src/quic_server.rs — QUIC control server
```

**Checkpoint:** Client can discover and connect to proxy via QUIC, health checks pass

---

#### Step 5: Security Review
```yaml
step: 5
name: Security Review
agent: SecOps
parallel: false
depends_on: [step_4]
estimated: 2 days
```

**Task:** Security audit of the MVP: threat model, code review, anti-abuse verification.

**Input:**
- All code from Steps 2-4
- Protocol spec

**Output:**
```markdown
## Security Audit: MVP
- Threat model results
- Code review findings
- Anti-abuse verification
- Required fixes (if any)
- HANDOFF: RustDev (fixes), then QAEngineer (Step 6)
```

**Checkpoint:** No Critical/High findings, or all fixed before proceeding

---

#### Step 6: Integration Testing
```yaml
step: 6
name: Integration Testing
agent: QAEngineer
parallel: false
depends_on: [step_5]
estimated: 3 days
```

**Task:** End-to-end testing of client ↔ proxy tunnel with simulated game traffic.

**Input:**
- Compiled client and proxy binaries
- Test infrastructure (local or single Oracle Cloud instance)

**Output:**
```markdown
## Test Report: MVP Integration
- Tunnel establishment: [pass/fail]
- Packet relay correctness: [pass/fail]
- Latency overhead: [measured ms]
- Throughput: [measured Mbps]
- Stability (1hr soak): [pass/fail]
- HANDOFF: ProductManager (release decision)
```

**Tool Calls:**
```xml
<tool_call name="PingBenchmark">
  <param name="mode">tunnel_vs_direct</param>
  <param name="target">game_server_ip</param>
  <param name="duration">3600</param>
  <param name="output">tests/results/mvp-integration.json</param>
</tool_call>
```

**Checkpoint:** All tests pass, latency overhead ≤ 5ms

---

#### Step 7: MVP Release
```yaml
step: 7
name: MVP Release
agent: DevOps
parallel: false
depends_on: [step_6]
estimated: 1 day
```

**Task:** Build release binaries, create GitHub release, update documentation.

**Output:**
- Release binaries (Windows x64, Linux x64, Linux ARM64)
- GitHub Release with changelog
- Updated README.md

**Checkpoint:** Release tagged, binaries downloadable

---

## WF-002: Proxy Network Setup

> Deploy a mesh of proxy nodes on Oracle Cloud Always Free tier across multiple regions.

### Overview
| Field | Value |
|-------|-------|
| **Goal** | 3+ proxy nodes in different regions, all on free tier |
| **Agents** | InfraDev, DevOps, NetEng, SecOps |
| **Input** | Oracle Cloud account, proxy binary from WF-001 |
| **Output** | Running proxy mesh with health monitoring |
| **Success Criteria** | All nodes healthy, <1% packet loss, zero cost |

### Steps

#### Step 1: Oracle Cloud Account Setup
```yaml
step: 1
name: Oracle Cloud Account Setup
agent: InfraDev
parallel: false
depends_on: []
estimated: 1 day
```

**Task:** Create/verify Oracle Cloud account(s), enable Always Free tier, identify available regions.

**Output:**
- Account credentials (stored securely)
- Available regions list with latency to game servers
- Free tier resource inventory

**Tool Calls:**
```xml
<tool_call name="OracleCloudAPI">
  <param name="action">list_regions</param>
  <param name="filter">always_free_eligible</param>
</tool_call>
```

---

#### Step 2: Region Selection
```yaml
step: 2
name: Region Selection
agent: NetEng
parallel: false
depends_on: [step_1]
estimated: 1 day
```

**Task:** Analyze which Oracle Cloud regions provide the best latency reduction paths for target games.

**Target Game Server Regions:**
| Game | Server Regions |
|------|---------------|
| Fortnite | US-East, US-West, EU-West, Asia-SE, Brazil, OCE |
| CS2 | US-East, US-West, EU-West, EU-North, Asia-SE, SA |
| Dota 2 | US-East, US-West, EU-West, EU-East, Asia-SE, SA, India |

**Output:**
- Optimal 3-5 Oracle Cloud regions
- Expected latency improvement per region-game pair
- Network topology diagram

---

#### Step 3: Terraform Infrastructure
```yaml
step: 3
name: Terraform Infrastructure
agent: InfraDev
parallel: false
depends_on: [step_2]
estimated: 3 days
```

**Task:** Write and apply Terraform for all proxy instances, networking, and security.

**Output:**
```hcl
// infra/terraform/main.tf
// infra/terraform/variables.tf
// infra/terraform/networking.tf
// infra/terraform/instances.tf
// infra/terraform/security.tf
// infra/terraform/outputs.tf
```

**Tool Calls:**
```xml
<tool_call name="TerraformPlan">
  <param name="action">plan</param>
  <param name="directory">infra/terraform/</param>
  <param name="var_file">infra/terraform/free-tier.tfvars</param>
</tool_call>
```

**Checkpoint:** `terraform plan` shows only free-tier resources

---

#### Step 4: Proxy Deployment
```yaml
step: 4
name: Proxy Deployment
agent: DevOps
parallel: false
depends_on: [step_3]
estimated: 2 days
```

**Task:** Deploy proxy binary to all provisioned instances using Docker or direct binary deployment.

**Output:**
- Running proxy on each node
- Health check endpoints responding
- Deployment playbook documented

**Tool Calls:**
```xml
<tool_call name="DockerDeploy">
  <param name="image">lightspeed-proxy:latest</param>
  <param name="targets">["node-us-east", "node-eu-west", "node-asia-se"]</param>
  <param name="health_check">/health</param>
</tool_call>
```

---

#### Step 5: Security Hardening
```yaml
step: 5
name: Security Hardening
agent: SecOps
parallel: false
depends_on: [step_4]
estimated: 2 days
```

**Task:** Harden all proxy nodes: firewall rules, fail2ban, rate limiting, monitoring for abuse.

**Output:**
- Hardened configurations applied
- Security audit checklist completed
- Monitoring alerts configured

---

#### Step 6: Mesh Verification
```yaml
step: 6
name: Mesh Verification
agent: QAEngineer
parallel: false
depends_on: [step_5]
estimated: 1 day
```

**Task:** Verify all proxy nodes are operational, test inter-node connectivity, measure baseline latencies.

**Output:**
```markdown
## Proxy Mesh Status Report
| Node | Region | Status | Latency to Game Servers | Free Tier Usage |
|------|--------|--------|------------------------|-----------------|
| ... | ... | ... | ... | ...% |
```

---

## WF-003: AI Route Optimizer

> Build and train the AI/ML system for intelligent route selection and latency prediction.

### Overview
| Field | Value |
|-------|-------|
| **Goal** | ML model that predicts optimal route with >80% accuracy |
| **Agents** | AIResearcher, RustDev, NetEng, QAEngineer |
| **Input** | Latency data from proxy mesh, network topology |
| **Output** | Trained linfa model integrated into client |
| **Success Criteria** | Route selection improves p50 latency by ≥15% |

### Steps

#### Step 1: Data Collection Pipeline
```yaml
step: 1
name: Data Collection Pipeline
agent: AIResearcher + NetEng
parallel: true
depends_on: []
estimated: 1 week
```

**Task:** Design and implement latency data collection: ping measurements, traceroutes, BGP data, time-of-day patterns.

**Output:**
- Data collection scripts running on proxy nodes
- Data schema definition
- Initial dataset (1 week of measurements)

---

#### Step 2: Feature Engineering
```yaml
step: 2
name: Feature Engineering
agent: AIResearcher
parallel: false
depends_on: [step_1]
estimated: 4 days
```

**Task:** Design feature set for route prediction model.

**Features (Draft):**
| Feature | Type | Source |
|---------|------|--------|
| `current_latency_ms` | float | Active probe |
| `historical_p50_ms` | float | Historical data |
| `historical_p95_ms` | float | Historical data |
| `jitter_ms` | float | Calculated |
| `hop_count` | int | Traceroute |
| `time_of_day` | categorical | Clock |
| `day_of_week` | categorical | Calendar |
| `bgp_as_path_len` | int | BGP data |
| `packet_loss_pct` | float | Active probe |
| `proxy_load` | float | Proxy metrics |
| `geographic_distance_km` | float | GeoIP |

---

#### Step 3: Model Training
```yaml
step: 3
name: Model Training
agent: AIResearcher
parallel: false
depends_on: [step_2]
estimated: 1 week
```

**Task:** Train and evaluate multiple linfa models for route selection.

**Models to Evaluate:**
1. **Random Forest** — baseline, interpretable
2. **Gradient Boosting** — higher accuracy
3. **Linear Regression** — fast inference
4. **K-Nearest Neighbors** — non-parametric baseline

**Evaluation Metrics:**
- MAE (Mean Absolute Error) on latency prediction
- Route selection accuracy (% of times optimal route chosen)
- Inference latency (must be < 1ms on client)

---

#### Step 4: Client Integration
```yaml
step: 4
name: Client Integration
agent: RustDev
parallel: false
depends_on: [step_3]
estimated: 4 days
```

**Task:** Integrate the trained linfa model into the Rust client for real-time route selection.

**Output:**
```rust
// client/src/ml/mod.rs — ML model loader
// client/src/ml/predict.rs — Inference engine
// client/src/ml/features.rs — Feature extraction
// client/src/route/selector.rs — Route selection using ML
```

---

#### Step 5: Online Learning
```yaml
step: 5
name: Online Learning System
agent: AIResearcher + RustDev
parallel: false
depends_on: [step_4]
estimated: 1 week
```

**Task:** Implement online learning that adapts the model based on real latency feedback.

**Design:**
```
Predicted Route → Actual Latency → Feedback → Model Update (incremental)
```

---

#### Step 6: A/B Validation
```yaml
step: 6
name: A/B Validation
agent: QAEngineer
parallel: false
depends_on: [step_5]
estimated: 3 days
```

**Task:** Compare AI-selected routes vs. naive (closest proxy) selection across all target games.

**Output:**
```markdown
## A/B Test: AI vs Naive Route Selection
| Game | Region | Naive p50 | AI p50 | Improvement |
|------|--------|-----------|--------|-------------|
| ... | ... | ... ms | ... ms | ...% |
```

---

## WF-004: Game Integration

> Ensure LightSpeed works correctly with target games: Fortnite, CS2, Dota 2.

### Overview
| Field | Value |
|-------|-------|
| **Goal** | Verified compatibility with 3 target games |
| **Agents** | RustDev, NetEng, QAEngineer |
| **Input** | Working tunnel (WF-001), proxy mesh (WF-002) |
| **Output** | Game-specific configs, verified packet handling |
| **Success Criteria** | All 3 games playable through tunnel, no disconnects |

### Steps

#### Step 1: Game Traffic Analysis
```yaml
step: 1
name: Game Traffic Analysis
agent: NetEng
parallel: true (per game)
depends_on: [WF-001, WF-002]
estimated: 3 days
```

**Task:** Capture and analyze UDP traffic patterns for each target game.

**Per Game Output:**
```markdown
## Traffic Profile: [Game Name]
- **Protocol**: UDP
- **Port Range**: [ports]
- **Packet Size Distribution**: [min/avg/max bytes]
- **Packet Rate**: [packets/sec]
- **Server Discovery**: [how game finds servers]
- **Anti-Cheat Considerations**: [EAC, VAC, etc.]
- **NAT Traversal**: [game's approach]
```

---

#### Step 2: Per-Game Tunnel Configuration
```yaml
step: 2
name: Per-Game Tunnel Config
agent: RustDev
parallel: true (per game)
depends_on: [step_1]
estimated: 4 days
```

**Task:** Create game-specific tunnel configurations (port rules, packet filters, server lists).

**Output:**
```toml
# client/configs/fortnite.toml
# client/configs/cs2.toml
# client/configs/dota2.toml
```

---

#### Step 3: Anti-Cheat Compatibility
```yaml
step: 3
name: Anti-Cheat Compatibility
agent: SecOps + RustDev
parallel: false
depends_on: [step_2]
estimated: 3 days
```

**Task:** Ensure the tunnel doesn't trigger anti-cheat systems (EasyAntiCheat, VAC).

**Key Concerns:**
- Packet capture method (pcap vs raw socket vs WFP on Windows)
- IP preservation (original IP must reach game server)
- No packet modification (only routing change)
- Transparent operation (not hiding/spoofing)

---

#### Step 4: Game-Specific Testing
```yaml
step: 4
name: Game-Specific Testing
agent: QAEngineer
parallel: true (per game)
depends_on: [step_3]
estimated: 4 days
```

**Task:** Play each game through the tunnel, verify functionality and measure improvement.

**Test Matrix:**
| Test | Fortnite | CS2 | Dota 2 |
|------|----------|-----|--------|
| Connect to server | | | |
| Play full match | | | |
| No disconnects (10 games) | | | |
| Latency improvement | | | |
| No anti-cheat triggers | | | |
| Voice chat works | | | |
| Spectator mode works | | | |

---

## WF-005: Scaling & Monitoring

> Set up production monitoring, auto-recovery, and scaling strategies.

### Overview
| Field | Value |
|-------|-------|
| **Goal** | Production-ready monitoring and operational tooling |
| **Agents** | DevOps, InfraDev, QAEngineer |
| **Input** | Running system from WF-001 through WF-004 |
| **Output** | Monitoring dashboards, alerting, auto-recovery |
| **Success Criteria** | 99.5% proxy uptime, auto-recovery < 5min |

### Steps

#### Step 1: Monitoring Stack
```yaml
step: 1
name: Monitoring Stack
agent: DevOps
parallel: false
depends_on: [WF-002]
estimated: 3 days
```

**Task:** Deploy monitoring using free tools (Prometheus + Grafana on free tier, or lightweight alternatives).

**Metrics to Monitor:**
- Proxy node health (CPU, memory, network)
- Tunnel latency (p50, p95, p99)
- Active connections per node
- Packet loss rate
- Free tier usage (% of limits)
- Client error rates

---

#### Step 2: Alerting
```yaml
step: 2
name: Alerting Setup
agent: DevOps
parallel: false
depends_on: [step_1]
estimated: 1 day
```

**Task:** Configure alerts for critical conditions.

**Alert Rules:**
| Alert | Condition | Severity | Action |
|-------|-----------|----------|--------|
| Node Down | Health check fail > 30s | Critical | Auto-restart, notify |
| High Latency | p95 > 100ms over baseline | Warning | Log, investigate |
| Free Tier 80% | Any resource > 80% | Warning | Notify, plan mitigation |
| Free Tier 95% | Any resource > 95% | Critical | Scale down, notify |
| Abuse Detected | Rate limit triggered > 100/min | Warning | Block source, notify |

---

#### Step 3: Auto-Recovery
```yaml
step: 3
name: Auto-Recovery
agent: DevOps + InfraDev
parallel: false
depends_on: [step_2]
estimated: 2 days
```

**Task:** Implement automatic recovery for common failure modes.

**Recovery Procedures:**
1. **Proxy crash** → Auto-restart via systemd/Docker restart policy
2. **Instance reboot** → Auto-start proxy on boot
3. **Network partition** → Client fails over to next-best proxy
4. **Free tier limit approaching** → Graceful connection shedding

---

#### Step 4: Load Testing
```yaml
step: 4
name: Load Testing
agent: QAEngineer
parallel: false
depends_on: [step_3]
estimated: 2 days
```

**Task:** Stress test the proxy network to find capacity limits within free tier.

**Output:**
```markdown
## Load Test Report
- Max concurrent tunnels per node: [N]
- Max throughput per node: [Mbps]
- Degradation point: [connections]
- Free tier headroom: [%]
```

---

## WF-006: Business Launch

> Prepare and execute the go-to-market strategy.

### Overview
| Field | Value |
|-------|-------|
| **Goal** | Public beta launch with initial user acquisition |
| **Agents** | BizDev, ProductManager, DocWriter, DevOps |
| **Input** | Working, tested product from WF-001 through WF-005 |
| **Output** | Landing page, download page, community channels |
| **Success Criteria** | 100 beta users in first month |

### Steps

#### Step 1: Brand & Positioning
```yaml
step: 1
name: Brand and Positioning
agent: BizDev
parallel: true
depends_on: []
estimated: 3 days
```

**Task:** Define brand identity, positioning against ExitLag/WTFast, key messaging.

**Output:**
- Brand guidelines (name: LightSpeed, tagline, colors, tone)
- Competitive positioning matrix
- Key messages for gamer audience

---

#### Step 2: Landing Page
```yaml
step: 2
name: Landing Page
agent: DocWriter + DevOps
parallel: false
depends_on: [step_1]
estimated: 4 days
```

**Task:** Build and deploy a landing page (static, free hosting via GitHub Pages or Cloudflare Pages).

**Output:**
```
web/landing/
├── index.html
├── styles.css
├── app.js
└── assets/
```

**Sections:**
1. Hero: "Reduce your ping. Free. Forever."
2. How it works (3-step diagram)
3. Supported games
4. Benchmark results (real data)
5. Download button
6. FAQ
7. Community links

---

#### Step 3: Documentation Site
```yaml
step: 3
name: Documentation Site
agent: DocWriter
parallel: true
depends_on: [step_1]
estimated: 3 days
```

**Task:** Create user-facing documentation: installation guide, configuration, FAQ, troubleshooting.

---

#### Step 4: Community Setup
```yaml
step: 4
name: Community Setup
agent: BizDev
parallel: true
depends_on: [step_1]
estimated: 1 day
```

**Task:** Create community channels.

**Channels:**
- GitHub Discussions (primary)
- Discord server
- Reddit presence (r/gaming, r/competitivegaming, game-specific subs)

---

#### Step 5: Beta Release
```yaml
step: 5
name: Beta Release
agent: ProductManager + DevOps
parallel: false
depends_on: [step_2, step_3, step_4, WF-005]
estimated: 2 days
```

**Task:** Execute beta launch: publish release, announce on community channels, monitor.

**Launch Checklist:**
- [ ] Release binaries published
- [ ] Landing page live
- [ ] Documentation complete
- [ ] Community channels active
- [ ] Monitoring dashboards ready
- [ ] Support process defined
- [ ] Rollback plan tested

---

#### Step 6: Feedback Loop
```yaml
step: 6
name: Feedback Loop
agent: ProductManager
parallel: false
depends_on: [step_5]
estimated: ongoing
```

**Task:** Collect and process user feedback, prioritize improvements, feed back into WF-001 (iterate).

**Feedback Sources:**
- GitHub Issues
- Discord feedback channel
- In-app telemetry (opt-in, latency metrics only)
- Community posts

---

## Workflow Execution Protocol

### Starting a Workflow

```
EXECUTE WORKFLOW: WF-[XXX]
STARTING_STEP: [step number, default 1]
CONTEXT: [any overrides or additional context]
```

### Step Transitions

After each step completes:
1. **Verify** output meets checkpoint criteria
2. **Log** to `state/checkpoints/WF-XXX-step-N.md`
3. **Check** if next step dependencies are met
4. **Dispatch** next step to assigned agent
5. **Update** `state/current-phase.md`

### Failure Handling

```
IF step fails:
  1. Log failure to state/agent-logs/
  2. IF retryable → retry (max 3 times)
  3. IF fixable → HANDOFF to appropriate agent with fix instructions
  4. IF blocking → ESCALATE to Architect
  5. IF critical → HALT workflow, notify human operator
```

### Parallel Step Execution

```
PARALLEL_START: [step_2a, step_2b]
  → Execute both simultaneously
  → Each produces independent output
PARALLEL_END: sync
  → Verify both outputs
  → Merge into unified state
  → Proceed to next step
```
