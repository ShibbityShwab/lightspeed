# 🎯 LightSpeed Project Goals

> Detailed technical goals, research findings, architecture decisions, and success metrics.
> This is the "why" and "what" document that drives all workflows and agent tasks.

---

## Mission Statement

**Build a lean, zero-ongoing-cost global network optimizer SaaS alternative to ExitLag** that reduces and stabilizes ping/latency for 2026 multiplayer games via unencrypted, transparent UDP tunneling that preserves user IP.

---

## Core Value Proposition

| For | Who | LightSpeed |
|-----|-----|------------|
| **Gamers** | Experience high latency/ping in multiplayer games | Reduces ping by 15-40% via intelligent route optimization |
| **Competitive Players** | Need lowest possible latency for ranked play | Provides stable, consistent latency with jitter reduction |
| **Budget-Conscious Users** | Can't afford $7-12/month for ExitLag/WTFast | Free forever — zero infrastructure cost architecture |
| **Privacy-Aware Users** | Don't want VPN-like traffic interception | Fully transparent, unencrypted, IP-preserving tunnel |

---

## Success Metrics

### Technical KPIs

| Metric | Target | Measurement |
|--------|--------|-------------|
| **Latency Reduction** | 15-40% improvement over direct path | p50 latency comparison |
| **Jitter Reduction** | ≥ 30% reduction in latency variance | Standard deviation comparison |
| **Tunnel Overhead** | ≤ 5ms additional processing time | Measured at tunnel endpoints |
| **Packet Loss** | ≤ 0.1% additional loss rate | Compared to direct path |
| **Uptime** | ≥ 99.5% proxy availability | Monitoring dashboard |
| **ML Accuracy** | ≥ 80% optimal route selection | A/B test vs oracle |
| **Client Performance** | ≤ 50MB RAM, ≤ 5% CPU idle | Resource monitoring |
| **Infrastructure Cost** | $0.00/month ongoing | Oracle Cloud billing |

### Business KPIs

| Metric | Target | Timeline |
|--------|--------|----------|
| **Beta Users** | 100 | Month 1 after launch |
| **Active Users** | 1,000 | Month 3 |
| **GitHub Stars** | 500 | Month 6 |
| **Supported Games** | 3 (Fortnite, CS2, Dota 2) | Launch |
| **Supported Games** | 10+ | Month 6 |
| **Supported Regions** | 3 proxy regions | Launch |
| **Supported Regions** | 5-7 proxy regions | Month 6 |

---

## Technical Architecture

### System Overview

```
┌──────────────────┐    UDP Tunnel    ┌──────────────────┐    Direct UDP    ┌──────────────────┐
│                  │  (Transparent)   │                  │                  │                  │
│   Game Client    │────────────────▶│   Proxy Node     │────────────────▶│   Game Server    │
│   + LightSpeed   │                  │  (Oracle Cloud   │                  │  (Epic/Valve)    │
│   Client (Rust)  │◀────────────────│   Always Free)   │◀────────────────│                  │
│                  │                  │                  │                  │                  │
└──────────────────┘                  └──────────────────┘                  └──────────────────┘
     User PC                           Free Tier VPS                        Game Infrastructure
     
     Components:                       Components:                        
     - Packet Capture (pcap-rs)        - UDP Relay                        
     - Tunnel Engine (Tokio)           - Auth Handler                     
     - Route Selector (linfa ML)       - Metrics Collector                
     - QUIC Control (quinn)            - QUIC Control Server              
     - Game Config Manager             - Health Check                     
```

### Data Flow (Per Packet)

```
OUTBOUND (Client → Game Server):
1. Game sends UDP packet to game server IP:port
2. pcap-rs captures packet on network interface
3. Tunnel engine wraps packet in LightSpeed header
4. Route selector chooses optimal proxy node
5. Wrapped packet sent to proxy via UDP
6. Proxy strips LightSpeed header
7. Proxy forwards original packet to game server
8. Game server sees original user IP (IP preserved)

INBOUND (Game Server → Client):
1. Game server sends response to user's IP
2. Response arrives at proxy (via network routing)
3. Proxy wraps response in LightSpeed header
4. Proxy sends wrapped packet to client
5. Client strips LightSpeed header
6. Client delivers original packet to game
7. Game receives response as if direct connection

TOTAL ADDED LATENCY: ≤ 5ms (header encode/decode + routing overhead)
```

### Component Architecture

#### Client (Rust)

```
client/
├── src/
│   ├── main.rs              # Entry point, CLI interface
│   ├── config.rs            # Configuration management
│   ├── tunnel/
│   │   ├── mod.rs           # Tunnel engine orchestrator
│   │   ├── capture.rs       # Packet capture (pcap-rs / WFP on Windows)
│   │   ├── relay.rs         # UDP socket management, send/receive
│   │   ├── header.rs        # LightSpeed header encode/decode
│   │   └── reassembly.rs    # Fragment reassembly (if needed)
│   ├── route/
│   │   ├── mod.rs           # Route management
│   │   ├── selector.rs      # AI-powered route selection
│   │   ├── multipath.rs     # Simultaneous multi-path routing
│   │   └── failover.rs      # Automatic failover logic
│   ├── capture/
│   │   ├── mod.rs           # Platform-specific packet capture
│   │   ├── windows.rs       # WFP (Windows Filtering Platform)
│   │   ├── linux.rs         # AF_PACKET / pcap
│   │   └── macos.rs         # BPF / pcap
│   ├── quic/
│   │   ├── mod.rs           # QUIC control plane client
│   │   ├── discovery.rs     # Proxy discovery protocol
│   │   ├── health.rs        # Proxy health checking
│   │   └── config_sync.rs   # Configuration synchronization
│   ├── ml/
│   │   ├── mod.rs           # ML model management
│   │   ├── predict.rs       # Real-time inference (linfa)
│   │   ├── features.rs      # Feature extraction from network metrics
│   │   └── online.rs        # Online learning / model updates
│   └── games/
│       ├── mod.rs           # Game detection and configuration
│       ├── fortnite.rs      # Fortnite-specific logic
│       ├── cs2.rs           # CS2-specific logic
│       └── dota2.rs         # Dota 2-specific logic
├── Cargo.toml
└── tests/
```

**Key Dependencies:**
```toml
[dependencies]
tokio = { version = "1", features = ["full"] }
pcap = "1"                    # or libpnet
quinn = "0.11"                # QUIC implementation
linfa = "0.7"                 # ML toolkit
linfa-trees = "0.7"           # Random forest
linfa-linear = "0.7"          # Linear regression
socket2 = "0.5"               # Advanced socket control
bytes = "1"                   # Zero-copy byte buffers
tracing = "0.1"               # Structured logging
tracing-subscriber = "0.3"
serde = { version = "1", features = ["derive"] }
toml = "0.8"                  # Configuration parsing
clap = { version = "4", features = ["derive"] }  # CLI
anyhow = "1"                  # Error handling
thiserror = "1"               # Custom errors
```

#### Proxy Server (Rust)

```
proxy/
├── src/
│   ├── main.rs              # Entry point, server startup
│   ├── config.rs            # Server configuration
│   ├── relay.rs             # Core packet relay engine
│   ├── auth.rs              # Client authentication
│   ├── metrics.rs           # Prometheus metrics export
│   ├── health.rs            # Health check endpoint
│   ├── quic_server.rs       # QUIC control plane server
│   ├── rate_limit.rs        # Per-client rate limiting
│   └── abuse.rs             # Abuse detection and blocking
├── Cargo.toml
└── tests/
```

---

## Key Technical Research

### 1. Why UDP Tunneling Beats Direct Path

**Problem:** ISPs route packets based on cost, not latency. The cheapest path often ≠ fastest path.

**Solution:** Route game packets through strategically placed proxy nodes that have better peering arrangements.

**Evidence from Research:**
- BGP routing adds 10-30% unnecessary latency for ~40% of routes
- Cloud providers (Oracle, AWS, Google) have premium backbone networks
- A well-placed proxy can "shortcut" suboptimal BGP paths
- ExitLag, WTFast, Haste all prove this model works commercially

**LightSpeed Advantage:** Same technique, zero cost via Oracle Cloud Always Free.

### 2. Oracle Cloud Always Free — Why It Works

**Resources Available (Per Account):**
| Resource | Allocation | LightSpeed Usage |
|----------|-----------|-----------------|
| ARM Compute | 4 OCPU, 24GB RAM | 2-4 proxy nodes (1 OCPU, 6GB each) |
| Block Storage | 200GB | 50GB per node OS + logs |
| Outbound Data | 10TB/month | ~3TB for 1000 active users |
| Load Balancer | 1 flexible | Anycast proxy entry point |

**Why Oracle's Network is Good:**
- Oracle has extensive peering with gaming networks
- Low-latency backbone between Oracle regions
- Oracle's network routes are often better than consumer ISPs
- ARM instances have excellent network performance

**Risk Mitigation:**
- Multiple Oracle accounts across regions (within ToS)
- Monitoring at 80% of all limits
- Graceful degradation under load

### 3. Unencrypted Transparent Tunneling

**Why NOT Encrypt (deliberately):**

| Concern | Encrypted (VPN) | Unencrypted (LightSpeed) |
|---------|-----------------|--------------------------|
| Latency overhead | 1-5ms encryption | 0ms (no crypto) |
| Anti-cheat compatibility | ❌ Often flagged | ✅ Transparent |
| User IP preservation | ❌ Masked by VPN | ✅ Preserved |
| Game server trust | ❌ VPN IP blocked | ✅ Real user IP |
| ISP inspection | ❌ Encrypted blob | ✅ Visible game traffic |
| Legal compliance | ⚠️ Varies | ✅ Clear |
| CPU usage | Higher | Minimal |

**Key Insight:** For game latency optimization, encryption is counter-productive. The goal is route optimization, not privacy/security. Transparency builds trust with game providers and anti-cheat systems.

### 4. AI/ML Route Optimization

**Problem:** Static proxy selection (nearest node) isn't optimal. Network conditions vary by time-of-day, congestion, BGP changes, and ISP behavior.

**Solution:** ML model that predicts optimal route in real-time.

**Approach with linfa (Rust ML):**

```
Input Features:
├── current_latency_ms      (real-time probe)
├── historical_p50_latency  (rolling window)
├── jitter_ms              (latency variance)
├── packet_loss_pct        (recent loss rate)
├── hop_count              (traceroute hops)
├── time_of_day            (0-23)
├── day_of_week            (0-6)
├── geographic_distance    (km to proxy)
├── proxy_load             (% capacity)
└── bgp_as_path_length     (AS hops)

Output: Route Score per proxy node (lower = better)
Model: Random Forest (linfa-trees) — fast inference, good accuracy
```

**Online Learning:**
- Model starts with pre-trained weights from benchmark data
- Adapts in real-time based on actual measured latency
- Exponential moving average for recent performance
- Retraining triggered when accuracy drops below threshold

### 5. Multipath Routing

**Strategy: Send packets on multiple paths, use first arrival.**

```
Client → Proxy A (US-East) → Game Server
Client → Proxy B (EU-West) → Game Server  ← First to arrive wins
Client → Direct            → Game Server

Fastest packet is used, others discarded (via sequence numbers).
```

**Benefits:**
- Redundancy: Survives single path failure
- Latency: Always gets best available path
- Jitter reduction: Multiple paths smooth variance

**Cost:** 2-3x bandwidth usage (mitigated by proxy-side dedup)

### 6. MEC (Multi-access Edge Computing) Integration

**Concept:** Edge compute nodes at ISP/carrier facilities for ultra-low-latency routing decisions.

**Current Status:** Research phase. MEC is emerging technology.

**Potential Integration:**
- Route selection at the edge (< 1ms decision time)
- Carrier-grade peering for better routes
- Integration with 5G MEC for mobile gaming

**Timeline:** Post-MVP (v2.0+)

### 7. BGP Intelligence

**Approach:** Use public BGP data to inform route selection.

**Data Sources:**
- RIPE RIS (Routing Information Service)
- RouteViews Project
- Hurricane Electric Looking Glass
- PeeringDB (peering relationships)

**Usage:**
1. Map game server IP → AS number → peering relationships
2. Map Oracle proxy IP → AS number → peering relationships
3. Identify which proxy has shortest AS path to game server
4. Factor BGP community strings for route preference
5. Detect BGP route changes that affect latency

---

## Target Games — Detailed Analysis

### Fortnite (Epic Games)

| Attribute | Details |
|-----------|---------|
| **Developer** | Epic Games |
| **Anti-Cheat** | Easy Anti-Cheat (EAC) |
| **Protocol** | UDP (custom) |
| **Server Ports** | Dynamic (ephemeral range) |
| **Server Regions** | NA-East, NA-West, EU, Asia, Brazil, OCE, ME |
| **Server IPs** | AWS-based, dynamic |
| **Traffic Pattern** | 20-60 packets/sec, 100-500 bytes/packet |
| **Considerations** | EAC must not flag tunnel; dynamic server IP discovery |

### Counter-Strike 2 (Valve)

| Attribute | Details |
|-----------|---------|
| **Developer** | Valve |
| **Anti-Cheat** | VAC (Valve Anti-Cheat) |
| **Protocol** | UDP (Steam Datagram Relay when available) |
| **Server Ports** | 27015-27050 (typical) |
| **Server Regions** | Global (Valve data centers) |
| **Server IPs** | Valve-owned, relatively static |
| **Traffic Pattern** | 64-128 tick, 30-60 packets/sec |
| **Considerations** | Steam Datagram Relay may already optimize; test improvement margin |

### Dota 2 (Valve)

| Attribute | Details |
|-----------|---------|
| **Developer** | Valve |
| **Anti-Cheat** | VAC |
| **Protocol** | UDP (Steam networking) |
| **Server Ports** | 27015-27050 |
| **Server Regions** | US, EU, SEA, SA, India, China, AUS, Japan |
| **Server IPs** | Valve-owned |
| **Traffic Pattern** | 30 tick, lower packet rate than CS2 |
| **Considerations** | Similar to CS2; good test case for different tick rates |

---

## Competitive Landscape

### ExitLag

| Aspect | ExitLag | LightSpeed |
|--------|---------|------------|
| **Price** | $6.50/month | Free |
| **Technology** | Multi-path TCP tunneling | Multi-path UDP tunneling |
| **Encryption** | Yes (proprietary) | No (transparent) |
| **IP Preservation** | No (exit IP changes) | Yes (original IP kept) |
| **Server Network** | 1000+ proprietary servers | Oracle Free Tier nodes |
| **Game Support** | 400+ games | 3 (launch), 10+ (6 months) |
| **AI Routing** | Proprietary algorithm | Open-source ML (linfa) |
| **Open Source** | No | Yes |

### WTFast

| Aspect | WTFast | LightSpeed |
|--------|--------|------------|
| **Price** | $9.99/month | Free |
| **Technology** | GPN (Gamers Private Network) | UDP tunnel mesh |
| **Focus** | Broad game support | Competitive multiplayer |
| **Transparency** | Closed | Open-source |

### Mudfish

| Aspect | Mudfish | LightSpeed |
|--------|---------|------------|
| **Price** | Pay-per-traffic (~$3/month) | Free |
| **Technology** | WFP-based tunnel | pcap-based capture |
| **Unique** | Traffic-based pricing | Zero-cost architecture |

### Our Differentiation

1. **Free forever** — zero infrastructure cost via Oracle Free Tier
2. **Open source** — full transparency, community contributions
3. **Unencrypted** — anti-cheat friendly, no overhead
4. **IP preserving** — game servers see real user IP
5. **AI-powered** — linfa ML for intelligent routing
6. **Community-driven** — build what gamers need

---

## Regional Targeting

### High-Impact Regions (Priority)

These regions have the worst default routing and most to gain:

| Region | Typical Ping Issues | Expected Improvement | Priority |
|--------|-------------------|---------------------|----------|
| **Southeast Asia** | SEA → US servers: 150-250ms | 20-40% | P0 |
| **South America** | Brazil → US servers: 100-200ms | 15-30% | P0 |
| **Eastern Europe** | EU-East → EU-West servers: 50-100ms | 15-25% | P1 |
| **Africa** | Africa → EU servers: 100-300ms | 20-40% | P1 |
| **India** | India → SEA servers: 50-150ms | 15-30% | P1 |
| **Middle East** | ME → EU servers: 80-150ms | 15-25% | P2 |
| **Oceania** | OCE → US-West servers: 150-200ms | 10-20% | P2 |

### Oracle Cloud Regions for Proxy Nodes

| Oracle Region | Code | Strategic Value |
|--------------|------|----------------|
| US East (Ashburn) | us-ashburn-1 | NA-East game servers |
| US West (Phoenix) | us-phoenix-1 | NA-West game servers |
| Germany (Frankfurt) | eu-frankfurt-1 | EU game servers |
| UK (London) | uk-london-1 | EU-West game servers |
| Japan (Tokyo) | ap-tokyo-1 | Asia game servers |
| South Korea (Chuncheon) | ap-chuncheon-1 | Asia competitive gaming |
| Brazil (Sao Paulo) | sa-saopaulo-1 | SA game servers |
| India (Mumbai) | ap-mumbai-1 | India/SEA game servers |
| Singapore | ap-singapore-1 | SEA game servers |

**MVP Launch:** 3 regions (US-East, EU-Frankfurt, SEA-Singapore)
**Phase 2:** 5 regions (+ US-West, SA-Brazil)
**Phase 3:** 7+ regions (+ Tokyo, Mumbai)

---

## Roadmap

### Phase 0: Foundation (Weeks 1-2)
- [x] WAT system creation (this document set)
- [ ] GitHub repository setup
- [ ] Rust project scaffolding (client + proxy)
- [ ] Development environment setup
- [ ] Architecture document finalized

### Phase 1: MVP (Weeks 3-8)
- [ ] UDP tunnel engine (client + proxy)
- [ ] Basic route selection (nearest proxy)
- [ ] Single proxy deployment (Oracle Free)
- [ ] Fortnite support
- [ ] Basic CLI interface
- [ ] Integration testing

### Phase 2: Intelligence (Weeks 9-14)
- [ ] ML route selection (linfa)
- [ ] Multi-proxy mesh (3 regions)
- [ ] CS2 + Dota 2 support
- [ ] QUIC control plane
- [ ] Monitoring dashboard
- [ ] Online learning

### Phase 3: Launch (Weeks 15-18)
- [ ] Landing page + docs
- [ ] Beta release
- [ ] Community setup
- [ ] Performance benchmarks published
- [ ] User feedback loop

### Phase 4: Growth (Weeks 19+)
- [ ] 5+ proxy regions
- [ ] 10+ game support
- [ ] Multipath routing
- [ ] MEC research integration
- [ ] Premium features (optional monetization)
- [ ] Mobile support research

---

## Open Research Questions

| # | Question | Priority | Agent |
|---|----------|----------|-------|
| 1 | What's the optimal Oracle Cloud region set for maximum game coverage? | P0 | NetEng |
| 2 | How does pcap-rs perform vs WFP for Windows packet capture? | P0 | RustDev |
| 3 | Can we achieve < 5ms tunnel overhead on ARM Ampere instances? | P0 | QAEngineer |
| 4 | Does the tunnel trigger EAC/VAC anti-cheat? | P0 | SecOps |
| 5 | What linfa model gives best accuracy/latency tradeoff? | P1 | AIResearcher |
| 6 | Can multipath routing reduce jitter by > 30%? | P1 | NetEng |
| 7 | What's the max concurrent users per free-tier proxy node? | P1 | QAEngineer |
| 8 | Is anycast feasible with Oracle Cloud Free Tier? | P2 | NetEng |
| 9 | Can online learning converge with < 100 samples per route? | P2 | AIResearcher |
| 10 | What BGP data sources provide the most actionable route intelligence? | P2 | NetEng |

---

## Risk Registry

| Risk | Likelihood | Impact | Mitigation |
|------|-----------|--------|------------|
| Oracle removes/limits Always Free | Low | Critical | Multi-cloud backup plan, community fundraising |
| Anti-cheat blocks tunnel | Medium | High | Transparent design, contact game devs, fallback to direct |
| Insufficient latency improvement | Medium | High | Thorough benchmarking, set honest expectations |
| Free tier bandwidth exceeded | Medium | Medium | Monitoring, user limits, load shedding |
| Game provider blocks proxy IPs | Low | High | IP rotation, contact game devs, IP preservation helps |
| Proxy abused as relay | Medium | High | Auth, rate limiting, abuse detection (SecOps priority) |
| Legal challenges | Low | Medium | Transparent design, legal review, ToS compliance |
| Competitor copies approach | Medium | Low | Open source moat, community, first mover in free tier |
