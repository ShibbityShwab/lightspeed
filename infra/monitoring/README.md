# ⚡ LightSpeed Monitoring Stack

Prometheus + Grafana monitoring for the LightSpeed proxy mesh.

## Quick Start

```bash
cd infra/monitoring
docker compose up -d
```

- **Grafana**: http://localhost:3000 (login: `admin` / `lightspeed`)
- **Prometheus**: http://localhost:9090

The dashboard auto-loads on first visit — no manual import needed.

## Architecture

```
┌─────────────────────┐     scrape :8080/metrics
│  Prometheus (9090)   │──────────────────────────▶  proxy-lax  (149.28.84.139)
│  15s scrape, 30d ret │──────────────────────────▶  relay-sgp  (149.28.144.74)
└──────────┬──────────┘
           │ query
┌──────────▼──────────┐
│   Grafana (3000)    │  ⚡ LightSpeed Proxy Mesh dashboard
│   admin/lightspeed  │  20 panels, 6 sections, auto-refresh 10s
└─────────────────────┘
```

## Dashboard Sections

| Section | Panels | What It Shows |
|---------|--------|---------------|
| ⚡ Mesh Overview | Node Status, Uptime, Connections, Version | At-a-glance health |
| 📊 Traffic | Packets/sec, Bytes/sec, Drops, Connections | Throughput and load |
| ⏱️ Latency | Avg latency, Histogram distribution | Relay performance |
| 🔧 FEC | Parity packets, Recoveries, Totals | Error correction effectiveness |
| 🛡️ Security | Auth rejections, Abuse blocks, Rate limits | Threat detection |
| 📈 Sessions | Session create rate, Lifetime totals | Client activity |

## Metrics Exported

Each proxy node exports these at `GET :8080/metrics`:

| Metric | Type | Description |
|--------|------|-------------|
| `lightspeed_packets_relayed_total` | counter | Total packets relayed |
| `lightspeed_bytes_relayed_total` | counter | Total bytes relayed |
| `lightspeed_packets_dropped_total` | counter | Total packets dropped |
| `lightspeed_active_connections` | gauge | Current active connections |
| `lightspeed_uptime_seconds` | gauge | Proxy uptime |
| `lightspeed_relay_latency_avg_us` | gauge | Average relay latency (μs) |
| `lightspeed_relay_latency_us` | histogram | Latency distribution (11 buckets) |
| `lightspeed_fec_parity_received_total` | counter | FEC parity packets received |
| `lightspeed_fec_recoveries_total` | counter | Packets recovered via FEC |
| `lightspeed_fec_data_packets_total` | counter | FEC data packets processed |
| `lightspeed_auth_rejections_total` | counter | Auth rejections |
| `lightspeed_abuse_blocks_total` | counter | Abuse detector blocks |
| `lightspeed_rate_limit_hits_total` | counter | Rate limit hits |
| `lightspeed_sessions_created_total` | counter | Total sessions created |
| `lightspeed_sessions_expired_total` | counter | Total sessions expired |
| `lightspeed_build_info` | gauge | Build version info |

All metrics include `region` and `node_id` labels.

## Alert Rules

10 rules across 5 groups:

| Alert | Severity | Condition |
|-------|----------|-----------|
| ProxyNodeDown | critical | Node unreachable for 30s |
| ProxyNodeRestarted | warning | Uptime < 5min |
| HighRelayLatency | warning | Avg latency > 100ms for 2min |
| CriticalRelayLatency | critical | Avg latency > 500ms for 1min |
| HighConnectionCount | warning | > 80 connections for 5min |
| NoTraffic | info | Zero packets for 15min |
| HighPacketDropRate | warning | > 5% drop rate for 5min |
| HighAuthRejections | warning | > 10/s rejections for 2min |
| AbuseDetected | critical | > 5/s abuse blocks for 1min |
| HighFECRecoveryRate | info | FEC recovering > 1/s for 5min |

## Adding a New Node

1. Add the target to `prometheus/prometheus.yml`:
   ```yaml
   - targets: ["NEW_IP:8080"]
     labels:
       node_id: "proxy-new"
       region: "new-region"
       provider: "vultr"
   ```

2. Reload Prometheus:
   ```bash
   curl -X POST http://localhost:9090/-/reload
   ```

The dashboard automatically picks up new nodes via label queries.

## Operations

```bash
# Start
docker compose up -d

# View logs
docker compose logs -f prometheus
docker compose logs -f grafana

# Stop (keep data)
docker compose down

# Stop + wipe all data
docker compose down -v

# Reload Prometheus config (no restart needed)
curl -X POST http://localhost:9090/-/reload
```

## Resource Usage

The monitoring stack is lightweight:

| Component | RAM | Disk (30d) |
|-----------|-----|------------|
| Prometheus | ~50MB | ~200MB |
| Grafana | ~40MB | ~10MB |

Can run on any machine with Docker — your local machine, a $3/mo VPS, or even a Raspberry Pi.
