# Current Phase

| Key | Value |
|-----|-------|
| **Active Workflow** | WF-004 Game Integration + FEC Client Wiring |
| **Current Step** | FEC integration into client relay, game capture pipeline |
| **Active Agents** | RustDev, NetEng |
| **Blocked On** | Nothing — all dependencies met |
| **Last Checkpoint** | 2026-02-23T21:20:00+07:00 |
| **Next Action** | Wire FEC into UdpRelay, complete redirect→tunnel pipeline |
| **WAT Version** | 0.3.5 |

## Live Infrastructure (2-Node Vultr Mesh)

| Node | IP | Region | Latency (BKK) | RAM | Role |
|------|----|--------|----------------|-----|------|
| **proxy-lax** | 149.28.84.139 | us-west-lax | 206ms | 504KB | Primary proxy |
| **relay-sgp** | 149.28.144.74 | asia-sgp | 31ms | 496KB | FEC multipath / SEA relay |

| Resource | Value |
|----------|-------|
| **Health URLs** | :8080/health on all nodes |
| **Data Port** | UDP 4434 |
| **Control Port** | UDP 4433 (QUIC disabled) |
| **Landing Page** | https://shibbityshwab.github.io/lightspeed/ |
| **Deployment** | Native binary + systemd (no Docker) |
| **Provider** | Vultr ($300 credit, 60+ months free) |

### Decommissioned Nodes

| Node | IP | Provider | Reason |
|------|----|----------|--------|
| proxy-us-west | 163.192.3.134 | OCI San Jose | ARM capacity issues, 7ms worse peering than Vultr LA |

## With WARP Optimization

| Path | Latency | Notes |
|------|---------|-------|
| BKK → Vultr LA (direct) | **206ms** | Best direct path |
| BKK → Vultr LA (WARP) | **193ms** | 5-10ms improvement via CF NTT backbone |
| ExitLag (reference) | **187ms** | Premium transit peering |
| **Gap to ExitLag** | **6ms** | Closes to ~0ms with FEC loss recovery advantage |

## Completed Steps

| Step | Status | Notes |
|------|--------|-------|
| WF-001 Step 1-7 | ✅ DONE | Full MVP: tunnel, proxy, QUIC, security, tests, release v0.1.0 |
| WF-002 Step 1-3 | ✅ DONE | Terraform IaC, Docker, deployment scripts, security hardening |
| WF-002 Step 4 | ✅ DONE | Vultr mesh: proxy-lax + relay-sgp (OCI SJ decommissioned) |
| WF-003 Step 1-4 | ✅ DONE | ML pipeline: synthetic data, features, RF model, client integration |
| WF-004 Step 1 | ✅ DONE | UDP redirect mode (client/src/redirect.rs) |
| WF-006 Step 1 | ✅ DONE | CI/CD: GitHub Actions (Rust CI, Docker GHCR, Pages) |
| WF-006 Step 2 | ✅ DONE | Landing page live on GitHub Pages |
| WF-006 Step 3 | ✅ DONE | Documentation updated: README, CHANGELOG, architecture, protocol, infra |
| FEC Module | ✅ DONE | XOR-based FEC, 8 tests passing, protocol/src/fec.rs |
| FEC Pipeline | ✅ DONE | FEC integrated into tunnel pipeline (client main.rs) |
| WARP Integration | ✅ DONE | client/src/warp.rs — auto-detect, 5-10ms improvement |
| Relay Analysis | ✅ DONE | SGP relay tested, Pacific crossing confirmed as bottleneck |
| Infra Pivot | ✅ DONE | OCI → Vultr-only, native binary (~500KB RAM) |
| Code Cleanup | ✅ DONE | Fixed compiler warnings, cleaned dead code annotations |

## In Progress

| Action | Status | Notes |
|--------|--------|-------|
| Wire FEC into UdpRelay | 🔧 IN PROGRESS | Connect FecEncoder/Decoder to relay send/recv |
| Game capture pipeline | 🔧 IN PROGRESS | Route selector → capture → redirect → tunnel |
| FEC integration tests | 🔧 IN PROGRESS | E2E tests with FEC-enabled relay |

## Next Steps

| Action | Owner | Priority | Notes |
|--------|-------|----------|-------|
| **Wire FEC into UdpRelay** | Agent | **P0** | FecEncoder in send, FecDecoder in recv |
| **Complete game pipeline** | Agent | **P0** | RouteSelector → GameConfig → Redirect → Tunnel |
| **FEC integration tests** | Agent | **P0** | Test FEC recovery through live tunnel |
| WF-003 Step 5-6: Online Learning | Agent | P1 | Needs live traffic data |
| WF-005: Monitoring Dashboard | Agent | P2 | Prometheus + Grafana |
| Vultr BKK monitoring | Agent | P2 | Check quarterly for Bangkok region availability |
| US-East / EU-West nodes | Agent | P2 | Expand mesh for global coverage |
