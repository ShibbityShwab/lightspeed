# Decisions Log

## D-001: Single-Region Deployment Strategy
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | Deploy single proxy node in OCI us-sanjose-1 (San Jose) |
| **Context** | OCI Always Free ARM instances restricted to home region. ARM A1.Flex returned "Out of host capacity" error. |
| **Outcome** | Deploy VM.Standard.E2.1.Micro (AMD, 1 OCPU, 1GB) in us-sanjose-1 for pipeline validation |
| **Trade-offs** | 1GB RAM limits concurrency; single region means no multi-path routing yet |
| **Future Plan** | Add Singapore node via Hetzner/Vultr (~$3/mo) for Thailand proximity; retry ARM when capacity opens; consider Fly.io for global free nodes |

## D-002: OCI ARM Capacity Workaround
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | Fall back to E2.1.Micro (AMD) after ARM capacity error |
| **Context** | ARM A1.Flex (4 OCPU, 24GB) failed with 500-InternalError "Out of host capacity" in us-sanjose-1 |
| **Outcome** | Successfully deployed E2.1.Micro. IP: 163.192.3.134 |
| **Retry Strategy** | Periodically retry ARM via `terraform apply` with A1.Flex shape; off-peak hours (early morning UTC) have better success |

## D-003: Multi-Region Strategy
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | Plan multi-provider approach instead of single-provider multi-region |
| **Options Evaluated** | (1) Multiple OCI accounts, (2) Fly.io free tier, (3) Hetzner/Vultr cheap VPS, (4) AWS/GCP/Azure free tiers |
| **Recommendation** | OCI San Jose (free) + Hetzner Singapore ($3.29/mo) + Fly.io (free, 3 machines) |
| **Rationale** | User in Thailand needs Asia-SE node; Hetzner Singapore is cheapest; Fly.io adds US-East + EU coverage for free |

## D-004: Native Binary Deployment (No Docker)
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | Deploy proxy as native binary with systemd instead of Docker container |
| **Context** | E2.1.Micro has only 503MB RAM. Docker CE failed to install via cloud-init (yum repo issue on Oracle Linux 9). Docker daemon alone uses ~100-200MB overhead. |
| **Outcome** | Proxy running natively: **568KB RAM**, 3 tasks, health endpoint verified from internet |
| **Method** | Extract linux/amd64 binary from GHCR Docker image → SCP to host → systemd with DynamicUser + SELinux bin_t context |
| **Benefits** | 350x less RAM than Docker approach; no container runtime dependency; faster startup; better systemd integration |
| **Trade-offs** | No container isolation (mitigated by systemd sandboxing: DynamicUser, ProtectSystem, NoNewPrivileges, MemoryDenyWriteExecute); manual binary updates instead of `docker pull` |
| **Future** | Updated deploy.sh supports both modes; CI can publish release binaries alongside Docker images |

## D-005: Vultr LA Primary Proxy
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | Deploy Vultr vc2-1c-1gb in LAX as primary proxy |
| **Context** | $300 Vultr credit = 60 months free. Vultr LA (197ms ping) has best Asia peering among affordable providers, closest to ExitLag (187ms). |
| **Outcome** | proxy-lax at 149.28.84.139, 504KB RAM, 206ms ICMP / 206ms UDP from BKK |
| **Instance** | ID: a31ee716-b807-4dac-9361-4e3e5b466f4f |

## D-006: Singapore Relay Node
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | Deploy Vultr vc2-1c-1gb in Singapore as Asia relay |
| **Context** | Testing Bangkok relay strategy to potentially close 19ms gap with ExitLag (206ms vs 187ms) |
| **Outcome** | relay-sgp at 149.28.144.74, 496KB RAM, 31ms from BKK |
| **Instance** | ID: 310f1dde-03f2-40a3-9e9b-03eb85644d7b |

## D-007: Relay Strategy Analysis — Singapore Does NOT Reduce Latency
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | Abandon relay-for-latency strategy; SGP node repurposed for FEC multipath + redundancy |
| **Context** | Hypothesis: routing BKK→SGP→LA would skip ISP's suboptimal routing and reduce total latency |
| **Testing** | BKK→SGP: 31ms, SGP→LA: 178ms, total relay: 209ms vs direct BKK→LA: 206ms |
| **Root Cause** | Pacific crossing dominates at ~172ms from either origin. Submarine cable physics, not routing, is the bottleneck. ExitLag's 187ms advantage comes from premium BGP transit (PCCW/NTT peering at Equinix LA), not geographic routing. |
| **Traceroute Evidence** | SGP→LA: hop 6 (1ms, SGP local) → hop 7 (173ms, Pacific crossing). No intermediate detour — just raw transoceanic distance. |
| **Revised Strategy** | (1) FEC multipath: send data on direct path, parity via SGP — recover packet loss without retransmission; (2) Client-side FEC eliminates 200ms+ retransmit penalty; (3) Focus on game integration as core product value |
| **SGP Node Value** | FEC redundant path, mesh failover, SEA regional coverage |

## D-008: Cloudflare WARP Local Route Optimization — 5-10ms Improvement Confirmed
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | Recommend WARP as free local route optimizer; build WARP-detection into client |
| **Context** | Testing whether Cloudflare's Bangkok PoP (4ms ICMP) could provide better transit to LA |
| **Testing Results** | Direct: 202-204ms avg; WARP: 193-197ms avg (**5-10ms improvement**); WARP to OCI: 203ms (was 213ms = **10ms improvement**) |
| **Traceroute Analysis** | WARP path: You→CF BKK (33ms)→CF backbone→NTT (36ms)→Pacific crossing (166ms)→Vultr LA. Direct path Pacific crossing: 172ms. **CF's NTT peering saves 6ms on Pacific crossing.** |
| **WARP Mode** | Full "Warp" mode with MASQUE protocol — tunnels ALL traffic including UDP. Game traffic WILL route through WARP. |
| **Limitation** | WARP adds ~33ms tunnel overhead (encapsulation), partially offsetting the 6ms peering advantage. Net: ~5ms improvement. |
| **With WARP Minimum** | 193ms — **only 6ms from ExitLag's 187ms!** |
| **Client Strategy** | (1) Detect WARP availability, recommend enabling for free 5ms savings; (2) Build route probing: test direct, WARP, and relay paths, select fastest; (3) Continuously monitor and auto-switch on degradation |
| **Combined Strategy** | WARP (193ms base) + FEC (recover lost packets in 3ms vs 400ms retransmit) = potentially **better effective gaming latency than ExitLag** despite 6ms higher base RTT |

## D-009: Bangkok VPS Relay — Only Works with Premium Transit
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Decision** | A random Thai VPS does NOT help — transit quality matters more than location |
| **Testing** | HostAtom BKK (27.254.145.144): 7ms from user, BUT LA→HostAtom = 205ms. Uses ChinaMobile transit (223.120.x.x), same Pacific crossing as ISP. Relay total: ~212ms = WORSE. |
| **Key Insight** | BKK relay only helps if provider has **premium NTT/PCCW transit**. Cloudflare has this (166ms Pacific via NTT). HostAtom doesn't (205ms via ChinaMobile). Most Thai VPS providers use cheap transit. |
| **Conclusion** | Cloudflare WARP IS the best Bangkok relay we can get for free. Only ExitLag-level BGP transit agreements ($$$) would do better. |
| **Theoretical Best** | BKK VPS + NTT transit: ~177ms (beats ExitLag). But no affordable Thai provider offers this. |
| **Final Strategy** | Use WARP (free, 193ms) + FEC (3ms loss recovery) + multipath (redundancy via SGP) = best achievable setup |

## D-010: ISP Path Analysis — HGC Singapore Detour Identified as Root Cause
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Discovery** | Traffic goes BKK → Singapore (NOT Hong Kong as previously assumed!) |
| **Path** | True ISP (2ms) → SBN/AWN SGP (33ms) → HGC Global SGP (36ms) → HGC internal detour (65ms, +29ms!) → HGC Pacific (215ms, 150ms cable) → Vultr LA (203ms) |
| **ASN Evidence** | Hop 9: AS45430 SBN/AWN = Singapore; Hop 10: AS9304 HGC = Singapore; Hop 14: AS9304 HGC = Los Angeles |
| **Bottleneck** | Hop 12 (10.165.171.1): HGC internal router adds 29ms BEFORE Pacific crossing. Without it: 36ms + 150ms = 186ms = ExitLag speed! |
| **Vultr BKK** | Does NOT exist. Nearest Vultr Asia: SGP (31ms), Tokyo (107ms), Seoul, Osaka. No HK either. |
| **If Vultr had BKK** | ~5ms access + 172ms Vultr backbone = **177ms — beats ExitLag by 10ms!** |
| **Why SGP relay loses** | Bypasses HGC detour (good) but adds 31ms BKK→SGP hop (bad). Net: 31+178=209ms > 203ms direct |
| **Why WARP wins** | Bypasses HGC entirely via NTT backbone. MASQUE overhead (33ms) < HGC detour (29ms) + ISP routing overhead. Net: 193-197ms |
| **UDP routing** | Confirmed same path as ICMP (UDP 206ms vs ICMP 202ms). BGP routes by destination IP, not protocol. |

## D-011: Meaningful Improvement Paths — Research Summary
| Field | Value |
|-------|-------|
| **Date** | 2026-02-23 |
| **Goal** | Close the 6ms gap (WARP 193ms vs ExitLag 187ms) or leapfrog ExitLag |

### Option A: Cloudflare Spectrum (Enterprise) — ⭐ Best Product Architecture
| Detail | Value |
|--------|-------|
| What | L4 proxy for TCP/UDP through CF anycast. Game traffic auto-routes via BKK PoP → NTT → LA. |
| Benefit | **No WARP needed on client.** Users connect to CF domain, traffic optimized automatically. + Argo Smart Routing + DDoS protection. |
| Estimated latency | ~193ms (same as WARP, but transparent to users) |
| Cost | Enterprise plan required for UDP ($5000+/mo). Pro ($20/mo) = TCP only. |
| Verdict | **Best for scale** but too expensive for MVP. Revisit at 1000+ users. |

### Option B: Integrate WARP into Client — ⭐ Best Immediate Improvement
| Detail | Value |
|--------|-------|
| What | Build WARP detection/recommendation into LightSpeed client installer. |
| Benefit | Free 5-10ms improvement. Auto-detect WARP → recommend enabling → one-click setup. |
| Estimated latency | 193ms (with WARP) vs 203ms (without) |
| Cost | Free (development effort only) |
| Verdict | **Do this NOW.** Most practical improvement. |

### Option C: Vultr Bangkok Region — 🏆 Would Beat ExitLag
| Detail | Value |
|--------|-------|
| What | If Vultr adds Bangkok, our proxy there would have direct Vultr backbone to LA. |
| Estimated latency | **~177ms** (5ms local + 172ms Vultr backbone = beats ExitLag by 10ms!) |
| Status | Vultr has NO BKK plans announced. Nearest: SGP (31ms). |
| Action | Monitor Vultr regions API. Feature request via Vultr support. |

### Option D: BKNIX Colocation with NTT Transit
| Detail | Value |
|--------|-------|
| What | Colocate a server at NTT Bangkok1/Bangkok2 (BKNIX present) with NTT transit. |
| BKNIX stats | 67 peering networks. NTT BKK2 at Chon Buri (near AAG cable landing!). |
| Benefit | Direct NTT backbone to LA. Estimated **~177ms** from BKK via NTT. |
| Cost | Colocation: $500-2000+/mo minimum. Enterprise-level. |
| Verdict | Only viable at significant scale. |

### Option E: Cloudflare Tunnel + Workers (Creative Hack)
| Detail | Value |
|--------|-------|
| What | Use Cloudflare Tunnel (cloudflared) on Vultr LA to expose proxy. Traffic enters via CF BKK PoP. |
| Limitation | cloudflared only supports TCP/HTTP, NOT raw UDP. Won't work for game traffic. |
| Verdict | Dead end for UDP gaming. |

### Option F: Alternative ISP Transit
| Detail | Value |
|--------|-------|
| What | User's ISP (True Internet) uses HGC transit with 29ms detour. Other ISPs might use NTT directly. |
| Tested | True → SBN/AWN → HGC Singapore (with detour) → Pacific → LA = 203ms |
| WARP bypass | CF BKK → NTT (no detour) → Pacific → LA = 193ms |
| Verdict | ISP-specific. Can't control for all users. WARP recommendation covers this universally. |

### Recommended Priority
1. **Option B: WARP integration** — Free, 5-10ms improvement, do NOW
2. **Option C: Monitor Vultr BKK** — Check quarterly, would be transformative
3. **Option A: Spectrum Enterprise** — At scale (1000+ users, revenue)
4. **Option D: BKNIX colo** — At significant scale
