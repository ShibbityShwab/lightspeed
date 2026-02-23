# Current Phase

| Key | Value |
|-----|-------|
| **Active Workflow** | WF-002 3-Node Mesh ✅ + Relay Strategy Analysis ✅ |
| **Current Step** | 3 proxies live, relay analysis complete, pivoting to WF-004 |
| **Active Agents** | InfraDev, NetOps |
| **Blocked On** | Nothing — ready for game integration |
| **Last Checkpoint** | 2026-02-23T18:20:00+07:00 |
| **Next Action** | WF-004 Game Integration |
| **WAT Version** | 0.3.4 |

## Live Infrastructure (3-Node Mesh)

| Node | IP | Region | Latency (BKK) | RAM | Role |
|------|----|--------|----------------|-----|------|
| **proxy-us-west** | 163.192.3.134 | us-west (San Jose) | 213ms | 568KB | Origin proxy |
| **proxy-lax** | 149.28.84.139 | us-west-lax | 206ms | 504KB | Primary proxy |
| **relay-sgp** | 149.28.144.74 | asia-sgp | 31ms | 496KB | Asia relay |

| Resource | Value |
|----------|-------|
| **Health URLs** | :8080/health on all nodes |
| **Data Port** | UDP 4434 |
| **Control Port** | UDP 4433 (QUIC disabled) |
| **Landing Page** | https://shibbityshwab.github.io/lightspeed/ |

## Relay Strategy Analysis (2026-02-23)

### Complete Latency Matrix (ICMP from Bangkok)

| Path | Latency | Notes |
|------|---------|-------|
| BKK → Vultr LA (direct) | **206ms** | Best direct path to California |
| BKK → OCI San Jose (direct) | **213ms** | Backup path |
| BKK → Vultr SGP | **31ms** | Asia relay hop |
| SGP → Vultr LA | **178ms** | Pacific crossing from SGP |
| SGP → OCI San Jose | **179ms** | Pacific crossing from SGP |
| **BKK → SGP → LA (relay)** | **~209ms** | **3ms SLOWER than direct** |
| ExitLag (reference) | **187ms** | Premium transit peering |

### Key Findings

1. **Pacific crossing is the bottleneck**: ~172ms from Singapore, ~170ms from Bangkok. Both use the same submarine cables.
2. **Singapore relay does NOT reduce latency**: 31ms (BKK→SGP) + 178ms (SGP→LA) = 209ms > 206ms (direct)
3. **ExitLag's 19ms advantage** comes from premium BGP transit (PCCW/NTT direct peering at Equinix LA), not geographic routing
4. **SGP node IS valuable for**: FEC multipath (redundant recovery paths), mesh redundancy, SEA regional coverage

### Traceroute Analysis (SGP → LA)
```
Hop 1-6: Vultr SGP internal (1ms)
Hop 7:   63.217.104.122 (173ms) ← PACIFIC CROSSING
Hop 10:  45.77.84.104 (165ms)   ← Vultr LA backbone
```

### Strategy Pivot
Instead of relay routing, focus on:
- **FEC multipath**: Data on primary path, parity on secondary — recovers packet loss without retransmission
- **Client-side optimization**: Eliminate retransmit latency (saves 200ms+ per lost packet)
- **Game integration**: The core product functionality

## Completed Steps

| Step | Status | Notes |
|------|--------|-------|
| WF-001 Step 1-7 | ✅ DONE | Full MVP: tunnel, proxy, QUIC, security, tests, release v0.1.0 |
| WF-002 Step 1-3 | ✅ DONE | Terraform IaC, Docker, deployment scripts, security hardening |
| WF-002 Step 4 | ✅ DONE | 3-node mesh: OCI SJ + Vultr LA + Vultr SGP |
| WF-003 Step 1-4 | ✅ DONE | ML pipeline: synthetic data, features, RF model, client integration |
| WF-006 Step 1 | ✅ DONE | CI/CD: GitHub Actions (Rust CI, Docker GHCR, Pages) |
| WF-006 Step 2 | ✅ DONE | Landing page live on GitHub Pages |
| FEC Module | ✅ DONE | XOR-based FEC, 8 tests passing, protocol/src/fec.rs |
| Relay Analysis | ✅ DONE | SGP relay tested, Pacific crossing confirmed as bottleneck |

## Next Steps

| Action | Owner | Priority | Notes |
|--------|-------|----------|-------|
| **WF-004: Game Integration** | Agent | **P0** | Wire client to capture + route game traffic |
| FEC + Multipath Integration | Agent | P0 | Connect FEC to tunnel relay |
| WF-006 Step 3: Documentation | Agent | P1 | — |
| WF-003 Step 5-6: Online Learning | Agent | P1 | Needs traffic data |
| WF-005: Scaling & Monitoring | Agent | P2 | 3-node mesh ready |
