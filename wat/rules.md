# 📜 LightSpeed Rules & Policy Stubs

> LLM rule stubs for the LightSpeed WAT system.
> Each stub is a replaceable block with default content and insertion points.
> Format: `[STUB_NAME]` — replace with your organization's specific policies.

---

## Stub Index

| ID | Stub Name | Priority | Applies To | Replaceable |
|----|-----------|----------|------------|-------------|
| R-001 | `[SAFETY_STUB]` | CRITICAL | All Agents | Yes |
| R-002 | `[ETHICS_STUB]` | HIGH | All Agents | Yes |
| R-003 | `[PRIVACY_STUB]` | HIGH | All Agents | Yes |
| R-004 | `[LEGAL_STUB]` | HIGH | All Agents | Yes |
| R-005 | `[COST_STUB]` | CRITICAL | InfraDev, DevOps, All | Yes |
| R-006 | `[QUALITY_STUB]` | MEDIUM | RustDev, QAEngineer | Yes |
| R-007 | `[SECURITY_STUB]` | CRITICAL | SecOps, All | Yes |
| R-008 | `[TRANSPARENCY_STUB]` | HIGH | NetEng, RustDev | Yes |
| R-009 | `[CONTENT_POLICY_STUB]` | MEDIUM | DocWriter, BizDev | Yes |
| R-010 | `[RATE_LIMIT_STUB]` | MEDIUM | All Tools | Yes |
| R-011 | `[ESCALATION_STUB]` | HIGH | All Agents | Yes |
| R-012 | `[DATA_RETENTION_STUB]` | MEDIUM | AIResearcher, DataPipeline | Yes |

### Priority Enforcement Order

```
CRITICAL > HIGH > MEDIUM > LOW

When rules conflict:
1. CRITICAL rules ALWAYS win
2. HIGH rules win over MEDIUM/LOW
3. MEDIUM rules are default
4. Escalate unresolvable conflicts to human operator
```

---

## R-001: `[SAFETY_STUB]`

> **Priority: CRITICAL** | Applies to: All Agents

### Default Content

```
[SAFETY_STUB_BEGIN]

SAFETY RULES — LightSpeed WAT System
=====================================

1. NO HARMFUL OPERATIONS
   - Never execute commands that could damage systems, networks, or data
   - Never generate code designed to exploit vulnerabilities
   - Never create tools that could be used for DDoS attacks or network abuse
   - Never facilitate unauthorized access to systems or networks

2. HUMAN APPROVAL REQUIRED FOR:
   - Destructive file operations (delete, overwrite critical files)
   - Infrastructure changes (terraform apply, instance termination)
   - Network operations that could affect third parties
   - Any operation exceeding $0 cost
   - Publishing or deploying to production

3. SYSTEM PROTECTION
   - Never modify system-level configurations without explicit approval
   - Never store credentials in code or logs
   - Never expose internal network details publicly
   - Always validate inputs before processing

4. ABORT CONDITIONS
   - If any action could cause financial harm → ABORT
   - If any action could enable network abuse → ABORT
   - If any action violates game provider ToS → ABORT
   - If uncertain about safety → ASK HUMAN

5. ANTI-ABUSE DESIGN
   - All proxy designs MUST include rate limiting
   - All proxy designs MUST prevent open relay abuse
   - All proxy designs MUST prevent amplification attacks
   - No designs that could be used to mask malicious traffic

[SAFETY_STUB_END]
```

### Insertion Points

```
<!-- Insert your organization's specific safety policies here -->
[SAFETY_CUSTOM_RULES]

<!-- Insert compliance framework references here -->
[SAFETY_COMPLIANCE_REFS]

<!-- Insert incident response procedures here -->
[SAFETY_INCIDENT_RESPONSE]
```

### How to Replace

Replace the entire block between `[SAFETY_STUB_BEGIN]` and `[SAFETY_STUB_END]` with your custom safety rules. Maintain the same structure (numbered rules, abort conditions) for agent compatibility.

---

## R-002: `[ETHICS_STUB]`

> **Priority: HIGH** | Applies to: All Agents

### Default Content

```
[ETHICS_STUB_BEGIN]

ETHICS RULES — LightSpeed WAT System
=====================================

1. HONESTY & TRANSPARENCY
   - Never make false claims about latency improvements
   - Always report benchmark results accurately, including failures
   - Never cherry-pick favorable metrics
   - Clearly communicate limitations of the system

2. FAIR PLAY
   - The system MUST NOT provide unfair competitive advantage beyond routing
   - No packet manipulation that alters game state
   - No bypassing of legitimate anti-cheat systems
   - No facilitating cheating or exploitation in games

3. USER RESPECT
   - Respect user autonomy and informed consent
   - No dark patterns in UI or marketing
   - No manipulative engagement tactics
   - Provide easy opt-out for all features

4. RESPONSIBLE DEVELOPMENT
   - Consider second-order effects of routing optimization
   - Don't optimize at the expense of other network users
   - Respect ISP fair use policies
   - Consider environmental impact of infrastructure

5. COMPETITIVE ETHICS
   - Don't disparage competitors with false claims
   - Compete on merit (performance, cost, transparency)
   - Respect intellectual property
   - Don't reverse-engineer competitor protocols unethically

[ETHICS_STUB_END]
```

### Insertion Points

```
[ETHICS_CUSTOM_RULES]
[ETHICS_COMPLIANCE_REFS]
```

---

## R-003: `[PRIVACY_STUB]`

> **Priority: HIGH** | Applies to: All Agents

### Default Content

```
[PRIVACY_STUB_BEGIN]

PRIVACY RULES — LightSpeed WAT System
======================================

1. IP PRESERVATION (Core Design Principle)
   - The user's original IP address MUST be preserved end-to-end
   - The tunnel MUST NOT mask, hide, or replace the user's IP
   - Game servers MUST see the user's real IP address
   - This is NOT a VPN — it is a transparent route optimizer

2. NO DATA COLLECTION
   - No collection of personally identifiable information (PII)
   - No logging of packet payloads or game data
   - No tracking of user behavior or game activity
   - No browser fingerprinting or device identification

3. TELEMETRY (Opt-In Only)
   - Latency metrics: ALLOWED (opt-in, anonymized)
   - Route performance data: ALLOWED (opt-in, aggregated)
   - Game identification: ALLOWED (for routing, not stored)
   - User identity correlation: NEVER

4. DATA MINIMIZATION
   - Collect only what is necessary for routing decisions
   - Delete temporary data after use
   - No persistent storage of user sessions
   - No data sharing with third parties

5. ML TRAINING DATA
   - No PII in training datasets
   - Only aggregated network metrics
   - No individual user attribution
   - Data anonymization before storage

6. COMPLIANCE FRAMEWORK
   - Design for GDPR compatibility
   - Design for CCPA compatibility
   - Design for global privacy regulations
   - Privacy by design, not afterthought

[PRIVACY_STUB_END]
```

### Insertion Points

```
[PRIVACY_CUSTOM_RULES]
[PRIVACY_DPA_REFERENCE]        <!-- Data Processing Agreement -->
[PRIVACY_JURISDICTION_RULES]   <!-- Jurisdiction-specific rules -->
```

---

## R-004: `[LEGAL_STUB]`

> **Priority: HIGH** | Applies to: All Agents

### Default Content

```
[LEGAL_STUB_BEGIN]

LEGAL RULES — LightSpeed WAT System
====================================

1. GAME PROVIDER COMPLIANCE
   - Review and comply with Terms of Service for each supported game:
     * Epic Games (Fortnite): Review EULA and anti-cheat policies
     * Valve (CS2, Dota 2): Review Steam Subscriber Agreement
   - Do not circumvent any game provider restrictions
   - Do not modify game traffic content (only routing)

2. NETWORK REGULATIONS
   - Comply with local telecommunications regulations
   - Respect ISP acceptable use policies
   - No unauthorized interception of third-party traffic
   - Comply with CFAA (Computer Fraud and Abuse Act) and equivalents

3. CLOUD PROVIDER TERMS
   - Oracle Cloud: Comply with Always Free tier terms
   - No abuse of free tier provisions
   - No resource hoarding or manipulation
   - Comply with acceptable use policies

4. INTELLECTUAL PROPERTY
   - Respect all software licenses
   - Proper attribution for open-source components
   - No unauthorized use of trademarks (ExitLag, etc.)
   - Maintain license compliance for all dependencies

5. LIABILITY
   - No warranty claims about latency improvement
   - Clear disclaimer about "best effort" optimization
   - No guarantee of service availability
   - User assumes risk of using route optimization

6. EXPORT CONTROL
   - No encryption (system is unencrypted by design)
   - Verify no export control issues with routing software
   - Comply with sanctions and embargo lists

[LEGAL_STUB_END]
```

### Insertion Points

```
[LEGAL_CUSTOM_RULES]
[LEGAL_JURISDICTION]           <!-- Specific legal jurisdiction -->
[LEGAL_COUNSEL_CONTACT]       <!-- Legal counsel information -->
[LEGAL_TOS_TEMPLATE]          <!-- Terms of Service template -->
```

---

## R-005: `[COST_STUB]`

> **Priority: CRITICAL** | Applies to: InfraDev, DevOps, All infrastructure operations

### Default Content

```
[COST_STUB_BEGIN]

COST RULES — LightSpeed WAT System (ZERO ONGOING COST)
=======================================================

*** THIS IS A CRITICAL RULE — VIOLATION CAUSES IMMEDIATE ABORT ***

1. ZERO ONGOING COST MANDATE
   - Total monthly infrastructure cost MUST be $0.00
   - No exceptions without human operator approval
   - All infrastructure MUST use Always Free tier or equivalent
   - No "free trial" resources that convert to paid

2. ORACLE CLOUD ALWAYS FREE LIMITS
   - Compute: Up to 4 ARM Ampere A1 OCPUs + 24GB RAM (total)
   - Storage: Up to 200GB block volume
   - Object Storage: 20GB
   - Outbound: 10TB/month
   - Load Balancer: 1 flexible LB
   - Monitoring: 500M ingestion datapoints
   
   ENFORCEMENT: Before any resource creation:
   a. Query current usage (CostMonitor tool)
   b. Verify new resource fits within limits
   c. Calculate projected usage with new resource
   d. REJECT if would exceed 90% of any limit

3. FREE TIER SERVICES ALLOWED
   - Oracle Cloud Always Free ✅
   - GitHub Free (repos, actions minutes) ✅
   - Cloudflare Free (DNS, Pages) ✅
   - Let's Encrypt (TLS certs) ✅
   - Docker Hub Free (container registry) ✅
   - Grafana Cloud Free (monitoring) ✅

4. PAID SERVICES FORBIDDEN
   - AWS (no free tier for this use case) ❌
   - GCP (limited free, risk of billing) ❌
   - Any service requiring credit card with risk ❌
   - CDN services with overage charges ❌

5. COST MONITORING
   - Run CostMonitor tool daily (automated)
   - Alert at 80% of any free tier limit
   - Critical alert at 90% — begin shedding load
   - Emergency shutdown at 95% — prevent billing

6. ONE-TIME COSTS (ACCEPTABLE)
   - Domain name registration ($10-15/year) — ACCEPTABLE
   - Code signing certificate — ACCEPTABLE if needed
   - Other one-time: REQUIRE human approval

[COST_STUB_END]
```

### Insertion Points

```
[COST_CUSTOM_LIMITS]
[COST_BUDGET_OVERRIDE]         <!-- For when budget is available -->
[COST_ALTERNATIVE_PROVIDERS]   <!-- Additional free tier providers -->
```

---

## R-006: `[QUALITY_STUB]`

> **Priority: MEDIUM** | Applies to: RustDev, QAEngineer, DevOps

### Default Content

```
[QUALITY_STUB_BEGIN]

QUALITY RULES — LightSpeed WAT System
======================================

1. CODE QUALITY
   - All Rust code MUST pass `cargo clippy` with no warnings
   - All Rust code MUST be formatted with `cargo fmt`
   - No `unwrap()` in production code (use proper error handling)
   - `unsafe` blocks require documented justification and review
   - All public APIs MUST have doc comments
   - Follow Rust API Guidelines (https://rust-lang.github.io/api-guidelines/)

2. TESTING REQUIREMENTS
   - Unit tests: Minimum 70% code coverage for core modules
   - Integration tests: All component interfaces must be tested
   - Benchmark tests: Latency-critical paths must have benchmarks
   - Property tests: Protocol encode/decode must have proptest
   - No code merged without passing CI

3. PERFORMANCE STANDARDS
   - Tunnel overhead: ≤ 5ms additional latency
   - Packet processing: ≤ 100μs per packet
   - ML inference: ≤ 1ms per route decision
   - Memory usage: ≤ 50MB client resident memory
   - CPU usage: ≤ 5% idle, ≤ 15% active tunneling

4. DOCUMENTATION
   - Architecture decisions: Documented in state/decisions.md
   - API changes: Documented before implementation
   - Protocol changes: Versioned and documented
   - User-facing changes: Changelog updated

5. REVIEW PROCESS
   - Security-critical code: SecOps review required
   - Protocol changes: NetEng + Architect review
   - Infrastructure changes: InfraDev + cost verification
   - All changes: At least one agent review

6. RELEASE CRITERIA
   - All tests passing
   - No known critical bugs
   - Performance benchmarks within standards
   - Documentation updated
   - Changelog written
   - Security review complete (for major releases)

[QUALITY_STUB_END]
```

### Insertion Points

```
[QUALITY_CUSTOM_STANDARDS]
[QUALITY_CI_PIPELINE]          <!-- CI/CD quality gates -->
[QUALITY_METRIC_THRESHOLDS]    <!-- Custom metric thresholds -->
```

---

## R-007: `[SECURITY_STUB]`

> **Priority: CRITICAL** | Applies to: SecOps, All Agents

### Default Content

```
[SECURITY_STUB_BEGIN]

SECURITY RULES — LightSpeed WAT System
=======================================

*** THIS IS A CRITICAL RULE — SECURITY VIOLATIONS CAUSE IMMEDIATE HALT ***

1. ANTI-ABUSE (HIGHEST PRIORITY)
   - Proxy nodes MUST NOT be usable as open relays
   - Client authentication required before tunnel establishment
   - Per-client rate limiting enforced at proxy level
   - No amplification: response must not exceed request size significantly
   - Abuse detection: automated blocking of suspicious patterns

2. AUTHENTICATION
   - Lightweight token-based auth (minimal latency impact)
   - Tokens rotated regularly
   - No hardcoded credentials anywhere in codebase
   - Secrets stored in environment variables or secure vault

3. NETWORK SECURITY
   - Proxy management ports: Restricted to known IPs only
   - SSH: Key-based only, no password auth
   - Firewall: Default deny, explicit allow
   - No unnecessary open ports
   - Rate limiting on all public endpoints

4. CODE SECURITY
   - No SQL injection (if any DB used)
   - No command injection via user input
   - Input validation on all external data
   - Buffer overflow prevention (Rust helps, but verify unsafe blocks)
   - Dependency audit: `cargo audit` in CI

5. INFRASTRUCTURE SECURITY
   - OS hardening on all proxy nodes
   - Automatic security updates enabled
   - fail2ban or equivalent for brute force protection
   - Logging of all authentication attempts
   - No default passwords or configurations

6. INCIDENT RESPONSE
   - Security issues: Immediate escalation to human operator
   - Suspected abuse: Automatic traffic blocking + alert
   - Data breach (if any data): Immediate disclosure process
   - Vulnerability discovered: Responsible disclosure

7. SECRETS MANAGEMENT
   - NEVER commit secrets to git
   - Use .env files (gitignored) for local development
   - Use GitHub Secrets for CI/CD
   - Use OCI Vault for production secrets
   - Rotate all credentials quarterly

[SECURITY_STUB_END]
```

### Insertion Points

```
[SECURITY_CUSTOM_RULES]
[SECURITY_THREAT_MODEL]        <!-- Link to threat model document -->
[SECURITY_CONTACT]             <!-- Security team contact -->
[SECURITY_AUDIT_SCHEDULE]      <!-- Audit schedule -->
```

---

## R-008: `[TRANSPARENCY_STUB]`

> **Priority: HIGH** | Applies to: NetEng, RustDev

### Default Content

```
[TRANSPARENCY_STUB_BEGIN]

TRANSPARENCY RULES — LightSpeed WAT System
===========================================

1. UNENCRYPTED TUNNEL (Core Design Principle)
   - The UDP tunnel MUST NOT encrypt payload data
   - Game packets pass through the tunnel in cleartext
   - This is a deliberate design choice for:
     a. Transparency: Anyone can inspect tunnel traffic
     b. Anti-cheat compliance: No hiding of game packets
     c. Performance: No encryption/decryption overhead
     d. Trust: Users can verify no data manipulation

2. NO TRAFFIC MANIPULATION
   - Packets MUST be forwarded without modification
   - No content injection
   - No packet reordering beyond natural routing
   - No selective packet dropping
   - No payload inspection for content-based decisions

3. IP TRANSPARENCY
   - User's real IP MUST be visible to game servers
   - Tunnel headers are added, not replacing original headers
   - Proxy strips tunnel header and forwards original packet
   - Response path preserves same transparency

4. INSPECTION CAPABILITY
   - Tunnel protocol must be publicly documented
   - Anyone with tcpdump/Wireshark can inspect tunnel traffic
   - No proprietary or obfuscated protocol elements
   - Open-source client and proxy for full auditability

5. WHAT THE TUNNEL CHANGES
   - ONLY the network route (path packets take)
   - NOT the packet content
   - NOT the source/destination addresses
   - NOT the port numbers (after tunnel unwrap)

[TRANSPARENCY_STUB_END]
```

### Insertion Points

```
[TRANSPARENCY_CUSTOM_RULES]
[TRANSPARENCY_AUDIT_PROCESS]   <!-- How to audit tunnel traffic -->
```

---

## R-009: `[CONTENT_POLICY_STUB]`

> **Priority: MEDIUM** | Applies to: DocWriter, BizDev

### Default Content

```
[CONTENT_POLICY_STUB_BEGIN]

CONTENT POLICY — LightSpeed WAT System
=======================================

1. MARKETING CONTENT
   - All performance claims must be backed by reproducible benchmarks
   - Use ranges ("15-40% improvement") not absolutes
   - Include "results may vary" disclaimers
   - No false comparisons with competitors
   - No guarantee of specific ping numbers

2. DOCUMENTATION CONTENT
   - Technically accurate and verified
   - Inclusive language
   - Accessible writing (clear, concise)
   - No jargon without explanation

3. COMMUNITY CONTENT
   - Respectful, professional tone
   - No engagement with trolls or flamewars
   - Factual responses to technical questions
   - Honest about limitations

4. PROHIBITED CONTENT
   - No content promoting cheating in games
   - No content about bypassing security systems
   - No misleading speed test screenshots
   - No fake user testimonials

[CONTENT_POLICY_STUB_END]
```

---

## R-010: `[RATE_LIMIT_STUB]`

> **Priority: MEDIUM** | Applies to: All Tools

### Default Content

```
[RATE_LIMIT_STUB_BEGIN]

RATE LIMITING — LightSpeed WAT System
======================================

1. API RATE LIMITS
   - Oracle Cloud API: Respect OCI rate limits (varies by endpoint)
   - GitHub API: 5,000 requests/hour (authenticated)
   - BGP Looking Glass: Max 1 query/minute per source
   - Web Search: Max 100 queries/day

2. TOOL EXECUTION LIMITS
   - CodeGen: No limit (LLM-bound)
   - ShellExec: Max 10 concurrent commands
   - PingBenchmark: Max 1,000 packets/second total
   - LatencyProbe: Max 100 probes/second per node
   - TerraformPlan: Max 1 apply per hour

3. PROXY RATE LIMITS (Per Client)
   - Max connections: 10 simultaneous tunnels
   - Max bandwidth: 100 Mbps per client
   - Max packet rate: 10,000 packets/second
   - Cooldown on reconnection: 5 seconds

4. ABUSE THRESHOLDS
   - > 50,000 packets/second from single IP → block
   - > 500 Mbps from single IP → throttle
   - > 100 connection attempts/minute → block 15min
   - > 1000 failed auth attempts/hour → block 1hr

[RATE_LIMIT_STUB_END]
```

---

## R-011: `[ESCALATION_STUB]`

> **Priority: HIGH** | Applies to: All Agents

### Default Content

```
[ESCALATION_STUB_BEGIN]

ESCALATION RULES — LightSpeed WAT System
=========================================

1. ESCALATION LEVELS
   Level 0: Agent handles autonomously
   Level 1: Escalate to Architect agent
   Level 2: Escalate to human operator
   Level 3: Emergency halt — stop all operations

2. WHEN TO ESCALATE

   Level 0 → Level 1 (Agent → Architect):
   - Conflicting requirements between agents
   - Unclear technical decision
   - Performance target not achievable
   - Design trade-off needs resolution

   Level 1 → Level 2 (Architect → Human):
   - Security incident detected
   - Cost limit breach
   - Legal/compliance concern
   - Architectural decision with major implications
   - Any action that cannot be undone

   Level 2 → Level 3 (Human → Emergency):
   - Active security breach
   - System being used for abuse
   - Uncontrolled cost escalation
   - Data breach

3. ESCALATION FORMAT
   ```
   ESCALATION:
   Level: [0|1|2|3]
   From: [agent name]
   Issue: [description]
   Impact: [what's at risk]
   Options: [possible resolutions]
   Recommendation: [preferred option]
   Urgency: [immediate|hours|days]
   ```

4. RESPONSE TIME EXPECTATIONS
   Level 1: Immediate (next agent cycle)
   Level 2: Within 1 hour (human response)
   Level 3: Within 5 minutes (emergency)

[ESCALATION_STUB_END]
```

---

## R-012: `[DATA_RETENTION_STUB]`

> **Priority: MEDIUM** | Applies to: AIResearcher, DataPipeline

### Default Content

```
[DATA_RETENTION_STUB_BEGIN]

DATA RETENTION — LightSpeed WAT System
=======================================

1. LATENCY MEASUREMENT DATA
   - Raw measurements: Retain 30 days, then aggregate
   - Aggregated data: Retain 1 year
   - Format: Anonymized, no PII
   - Storage: Local filesystem on proxy nodes (free tier)

2. LOG DATA
   - Application logs: Retain 7 days
   - Security logs: Retain 90 days
   - Audit logs: Retain 1 year
   - Auto-rotate using logrotate or equivalent

3. ML TRAINING DATA
   - Training datasets: Retain while model is active
   - Deprecated datasets: Delete within 30 days
   - Model artifacts: Retain current + 1 previous version

4. USER DATA
   - Session data: Memory only, not persisted
   - Authentication tokens: Expire after 24 hours
   - No user profiles stored
   - No usage history stored

5. STATE DATA (WAT System)
   - Current state: Always retained
   - Checkpoints: Retain last 10
   - Decision log: Retain indefinitely
   - Agent logs: Retain 30 days

[DATA_RETENTION_STUB_END]
```

---

## How to Use Stubs

### For LLM Systems (Claude, GPT, etc.)

Include the relevant stubs in the system prompt or context:

```
You are operating as the [AgentName] agent in the LightSpeed WAT system.

The following rules apply to your operations:

[SAFETY_STUB]
[paste safety stub content here]

[ETHICS_STUB]
[paste ethics stub content here]

[specific stubs for this agent]
```

### For Custom Rule Systems

Replace stub content while maintaining the marker format:

```
[SAFETY_STUB_BEGIN]
[Your custom safety rules here]
[SAFETY_STUB_END]
```

### For Multi-LLM Compatibility

The stub format is designed to work with:
- **Claude**: Native Markdown comprehension
- **GPT-4**: Instruction following via system prompt
- **Gemini**: Context window inclusion
- **LLaMA/Mistral**: Prompt template inclusion
- **Custom models**: Parse `[STUB_BEGIN]`/`[STUB_END]` markers

### Stub Composition

Multiple stubs can be composed for a single agent context:

```markdown
# Agent Context: RustDev

## Applicable Rules
[SAFETY_STUB]
[QUALITY_STUB]
[SECURITY_STUB]
[TRANSPARENCY_STUB]

## Task
[task description]
```

### Validation

To verify all stubs are properly loaded, check for markers:
```
VALIDATE STUBS:
- [SAFETY_STUB]: [loaded|missing]
- [ETHICS_STUB]: [loaded|missing]
- [PRIVACY_STUB]: [loaded|missing]
...
```
