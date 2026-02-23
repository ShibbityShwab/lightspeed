# Current Phase

| Key | Value |
|-----|-------|
| **Active Workflow** | WF-004 Game Integration + Live Testing |
| **Current Step** | Live integration test, game capture pipeline |
| **Active Agents** | RustDev, NetEng |
| **Blocked On** | Nothing — all dependencies met |
| **Last Checkpoint** | 2026-02-23T22:55:00+07:00 |
| **Next Action** | Bidirectional capture mode, online ML learning |
| **WAT Version** | 0.3.7 |

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
| FEC in UdpRelay | ✅ DONE | FecEncoder in send_to_proxy, FecDecoder in recv_from_proxy |
| FEC in UdpRedirect | ✅ DONE | Full FEC encode/decode in redirect outbound + inbound paths |
| FEC in Proxy Relay | ✅ DONE | Proxy-side FEC decode, recovery, and re-encode for responses |
| FEC Integration Tests | ✅ DONE | 5 tests: data roundtrip, recovery, mixed v1/v2, variable sizes, multi-block |
| WARP Integration | ✅ DONE | client/src/warp.rs — auto-detect, 5-10ms improvement |
| Relay Analysis | ✅ DONE | SGP relay tested, Pacific crossing confirmed as bottleneck |
| Infra Pivot | ✅ DONE | OCI → Vultr-only, native binary (~500KB RAM) |
| Code Cleanup | ✅ DONE | Fixed compiler warnings, cleaned dead code annotations |
| Route Selection | ✅ DONE | RouteSelector integrated into main.rs, auto proxy probing + selection |
| Proxy Health Probing | ✅ DONE | Keepalive-based RTT probing, concurrent multi-proxy health check |
| Live Integration Test | ✅ DONE | `--live-test` mode: 5-phase test (health, route, keepalive, relay, FEC) |
| Game Auto-Detection | ✅ DONE | Process scanning on Windows/Linux/macOS, matches game process names |
| Pcap Capture Backend | ✅ DONE | Full Ethernet→IP→UDP parser, BPF filtering, pcap_backend.rs |
| Capture Mode | ✅ DONE | `--capture` CLI mode: sniff game traffic, forward through tunnel |
| Interface Discovery | ✅ DONE | `--list-interfaces` shows pcap-available network interfaces |

## Recently Completed

| Action | Status | Notes |
|--------|--------|-------|
| Live proxy verification | ✅ DONE | proxy-lax: 204.8ms 10/10 0.3ms jitter, relay-sgp: 34.0ms 10/10 0.3ms jitter |
| Beta release v0.2.0 | ✅ DONE | Version bumped, CHANGELOG updated, live test results recorded |

## In Progress

| Action | Status | Notes |
|--------|--------|-------|
| Bidirectional capture | 🔧 PLANNED | Response injection for full transparent capture (Phase 2) |

## Next Steps

| Action | Owner | Priority | Notes |
|--------|-------|----------|-------|
| **Bidirectional capture** | Agent | **P1** | Raw socket injection for capture mode response path |
| **WF-003 Step 5-6: Online Learning** | Agent | **P1** | Needs live traffic data |
| WF-005: Monitoring Dashboard | Agent | P2 | Prometheus + Grafana |
| Vultr BKK monitoring | Agent | P2 | Check quarterly for Bangkok region availability |
| US-East / EU-West nodes | Agent | P2 | Expand mesh for global coverage |

## CLI Quick Reference

```bash
# Live integration test (health + keepalive echo)
lightspeed --live-test --proxy 149.28.84.139:4434

# Live test with data relay (requires echo_server.py on remote)
lightspeed --live-test --proxy 149.28.84.139:4434 --echo-server 149.28.144.74:9999

# Live test with FEC verification
lightspeed --live-test --proxy 149.28.84.139:4434 --echo-server 149.28.144.74:9999 --fec

# Redirect mode (primary game integration)
lightspeed --game cs2 --game-server 192.168.1.1:27015 --proxy 149.28.84.139:4434

# Capture mode (pcap-based, requires admin + Npcap)
lightspeed --game fortnite --capture --proxy 149.28.84.139:4434

# List network interfaces
lightspeed --list-interfaces

# Probe all configured proxies
lightspeed --probe-proxies
```
