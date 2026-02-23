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
