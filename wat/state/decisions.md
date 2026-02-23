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
