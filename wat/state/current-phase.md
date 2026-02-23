# Current Phase

| Key | Value |
|-----|-------|
| **Active Workflow** | WF-005 Scaling & Monitoring |
| **Current Step** | Steps 1-3 complete. Load test tool ready. Deploy updated binary → run load tests → mesh expansion. |
| **Active Agents** | DevOps, InfraDev |
| **Blocked On** | Deploy updated binary to live nodes (needs SSH key for Vultr) |
| **Last Checkpoint** | 2026-02-24T01:59:00+07:00 |
| **Next Action** | 1) `deploy-vultr.sh` to push new binary, 2) `docker compose up` monitoring, 3) `load_test.py --all-nodes`, 4) mesh expansion |
| **WAT Version** | 0.3.9 |

## WF-005 Progress

| Step | Name | Status |
|------|------|--------|
| Step 1 | Monitoring Stack (Prometheus + Grafana) | ✅ Complete |
| Step 2 | Alerting Rules | ✅ Complete |
| Step 3 | Auto-Recovery (systemd verified) | ✅ Complete |
| Step 4 | Load Testing | 🔲 Not Started |

### What Was Built (WF-005 Steps 1-3)

1. **Enhanced Prometheus metrics** (`proxy/src/metrics.rs`)
   - 20+ metrics: relay counters, latency histogram (11 buckets), FEC stats, security counters, session lifecycle, build info, uptime
   - All metrics labeled with `region` + `node_id`

2. **Route-aware HTTP server** (`proxy/src/health.rs`)
   - `GET /health` → JSON health response
   - `GET /metrics` → Prometheus exposition format
   - 404 for unknown paths

3. **Metrics wired into relay** (`proxy/src/relay.rs`)
   - Auth rejections, abuse blocks, rate limit hits tracked
   - FEC parity/recovery/data packet counters
   - Session created/expired lifecycle tracking

4. **Prometheus configuration** (`infra/monitoring/prometheus/`)
   - Scrape targets for proxy-lax (149.28.84.139) and relay-sgp (149.28.144.74)
   - 10s scrape interval, 30d retention, 1GB max storage

5. **Alerting rules** (`infra/monitoring/prometheus/alerts.yml`)
   - 10 rules across 5 groups: node health, latency, capacity, security, FEC

6. **Grafana dashboard** (`infra/monitoring/grafana/dashboards/lightspeed-overview.json`)
   - 6 sections, 20 panels: Overview, Traffic, Latency, FEC, Security, Sessions
   - Auto-provisioned datasource + dashboard

7. **Docker Compose** (`infra/monitoring/docker-compose.yml`)
   - One-command deploy: `docker compose up -d`
   - Prometheus + Grafana with persistent volumes and health checks

8. **Enhanced mesh-health.sh** (`infra/scripts/mesh-health.sh`)
   - Built-in Vultr node list (no Terraform required)
   - `--metrics` flag for Prometheus scraping
   - `--json` flag for machine-readable output

## Live Infrastructure (2-Node Vultr Mesh)

| Node | IP | Region | Latency (BKK) | RAM | Role |
|------|----|--------|----------------|-----|------|
| **proxy-lax** | 149.28.84.139 | us-west-lax | 206ms | 504KB | Primary proxy |
| **relay-sgp** | 149.28.144.74 | asia-sgp | 31ms | 496KB | FEC multipath / SEA relay |

| Resource | Value |
|----------|-------|
| **Health URLs** | :8080/health on all nodes |
| **Metrics URLs** | :8080/metrics on all nodes (Prometheus format) |
| **Data Port** | UDP 4434 |
| **Control Port** | UDP 4433 (QUIC disabled) |
| **Landing Page** | https://shibbityshwab.github.io/lightspeed/ |
| **Monitoring** | Prometheus + Grafana (docker compose) |
| **Deployment** | Native binary + systemd (no Docker) |
| **Provider** | Vultr ($300 credit, 60+ months free) |

### Decommissioned Nodes

| Node | IP | Provider | Reason |
|------|----|----------|--------|
| proxy-sanjose | 163.192.3.134 | OCI | E2.1.Micro too limited, moved to Vultr |
