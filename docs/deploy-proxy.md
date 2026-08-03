# Deploy a LightSpeed Proxy

> Deploy your own zero-cost proxy relay on any Linux VPS with a free tier.

## Quickstart

See the full deployment guide in **[`infra/README.md`](../infra/README.md)** — it covers:

- **Option A:** Native binary + systemd (recommended, ~500KB RAM)
- **Option B:** Docker (pull from GHCR or build locally)
- **Option C:** Automated deploy script

## Multi-Node Mesh

For best results, deploy 2-3 nodes in different regions:

| Region | Typical RTT to Bangkok |
|--------|----------------------|
| Singapore | ~31 ms |
| Tokyo | ~85 ms |
| Los Angeles | ~206 ms |
| Frankfurt | ~170 ms |
| Sydney | ~115 ms |

Add all proxies to your config:

```toml
[proxy]
servers = [
    "YOUR_SGP_IP:4434",
    "YOUR_LAX_IP:4434",
]
```

The client probes all proxies on startup and auto-selects the fastest route.

## Monitoring

```bash
# Health check
curl http://YOUR_PROXY_IP:8080/health

# Prometheus metrics
curl http://YOUR_PROXY_IP:8080/metrics
```
