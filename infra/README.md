# ⚡ LightSpeed — Self-Hosted Proxy Deployment

> Deploy a lightweight proxy mesh on any Linux VPS — ~500KB RAM per node.

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                     Your VPS (any provider)                  │
│                                                              │
│  ┌──────────────────┐          ┌──────────────────┐         │
│  │  proxy-node-1     │          │  proxy-node-2     │         │
│  │  <your-ip-1>      │          │  <your-ip-2>      │         │
│  │  Region A          │          │  Region B          │         │
│  │                   │          │                   │         │
│  │  UDP :4434 (data)  │          │  UDP :4434 (data)  │         │
│  │  HTTP :8080 (health)│         │  HTTP :8080 (health)│        │
│  │  ~500KB actual RAM │          │  ~500KB actual RAM │         │
│  └──────────────────┘          └──────────────────┘         │
│                                                              │
│  Deployment: Native binary + systemd (or Docker)             │
└──────────────────────────────────────────────────────────────┘
```

> **Self-Hosted Model**: You run your own proxy node(s). There is no shared "LightSpeed network" — you own and control your infrastructure.

## Choosing a Region (Network Position)

A relay only reduces ping if it has a *lower-latency, better-peered* path to the game server than your home ISP. Pick the relay region **closest to the game server**, not closest to you.

| Game server region | OCI Always-Free region | Fallback region |
|---|---|---|
| US-East | `us-ashburn-1` (Ashburn, VA) | `us-chicago-1` |
| US-West | `us-phoenix-1` / `us-sanjose-1` | — |
| EU-Central | `eu-frankfurt-1` | `eu-zurich-1` |
| EU-West | `eu-amsterdam-1` / `uk-london-1` | — |
| APAC (Japan/Korea) | `ap-tokyo-1` / `ap-seoul-1` | `ap-osaka-1` |
| APAC (Southeast Asia) | `ap-singapore-1` | `ap-mumbai-1` |
| South America-East | `sa-saopaulo-1` | `sa-vinhedo-1` |

**Oracle Cloud Always-Free constraints (2026):**
- Always-Free compute must live in your **home region** — one tenancy covers one region. Covering multiple regions means multiple accounts (one per region).
- Per tenancy you get **2 OCPU / 12 GB** of ARM (Ampere A1) *plus* two `VM.Standard.E2.1.Micro` x86 (1/8 OCPU, 1 GB). That's ~4 small relay VMs per region.
- ARM capacity is frequently **"out of capacity"** in popular regions (Ashburn, Frankfurt, Tokyo) — provision with a retry + fall back to `E2.1.Micro`.
- ~**10 TB/month** egress per tenancy (≈ 600+ concurrent 50 Kbps game streams). GCP free tier (1 GB/month, US-only) and AWS (12-month expiry) are *not* viable for relay egress.
- Idle instances may be **reclaimed** (~7 days) — a relay with no traffic can vanish.

> **Measurement beats guesswork.** The client already probes all configured proxies and picks the fastest. Deploy relays in a few candidate regions, then let the client's `--probe-proxies` tell you which region actually wins for your game + location.

## Quick Start

### Prerequisites

- A Linux VPS (any provider: Vultr, Hetzner, DigitalOcean, Linode, OCI, AWS Lightsail, etc.)
- SSH access to your VPS
- Rust toolchain installed locally (or use Docker)

### Option A: Native Binary + systemd (Recommended)

```bash
# 1. Build the proxy
cargo build --release -p lightspeed-proxy

# 2. Copy to your VPS
scp target/release/lightspeed-proxy root@YOUR_VPS_IP:/usr/local/bin/
scp proxy/proxy.toml.default root@YOUR_VPS_IP:/etc/lightspeed/proxy.toml

# 3. Install systemd service on the VPS
ssh root@YOUR_VPS_IP 'cat > /etc/systemd/system/lightspeed-proxy.service' << 'EOF'
[Unit]
Description=LightSpeed Proxy
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
DynamicUser=yes
ExecStart=/usr/local/bin/lightspeed-proxy --config /etc/lightspeed/proxy.toml
Restart=always
RestartSec=5
NoNewPrivileges=yes
ProtectSystem=strict
MemoryDenyWriteExecute=yes

[Install]
WantedBy=multi-user.target
EOF

# 4. Start the service
ssh root@YOUR_VPS_IP 'systemctl daemon-reload && systemctl enable --now lightspeed-proxy'

# 5. Verify
curl http://YOUR_VPS_IP:8080/health
```

### Option B: Docker

```bash
# Build and run
docker build -f infra/docker/Dockerfile -t lightspeed-proxy .
docker run -d --name ls-proxy -p 4434:4434/udp -p 8080:8080 lightspeed-proxy

# Or pull from GHCR
docker pull ghcr.io/shibbityshwab/lightspeed-proxy:latest
docker run -d --name ls-proxy -p 4434:4434/udp -p 8080:8080 ghcr.io/shibbityshwab/lightspeed-proxy:latest
```

### Option C: Automated Deploy Script

```bash
# Configure your nodes
export LIGHTSPEED_NODES='{"proxy-1":{"ip":"1.2.3.4"},"proxy-2":{"ip":"5.6.7.8"}}'

# Deploy to all nodes
./infra/scripts/deploy.sh
```

## Directory Structure

```
infra/
├── docker/                 # Container builds
│   ├── Dockerfile          # Multi-stage, multi-arch build
│   └── docker-compose.yml  # Local dev / single-node
├── monitoring/             # Prometheus + Grafana (optional)
│   ├── docker-compose.yml
│   ├── prometheus/
│   └── grafana/
├── scripts/                # Operations
│   ├── deploy.sh           # Build + SCP + restart
│   ├── deploy-all.sh       # Rolling deploy to all nodes
│   ├── provision.sh        # Provision new VPS instances
│   ├── setup-new-node.sh   # First-time node setup
│   └── mesh-health.sh      # Health check all nodes
└── README.md               # This file
```

## Security

### Network
- Restrict ingress to only required ports (UDP 4434, TCP 8080)
- SSH restricted by key-only auth

### Host
- systemd sandboxing: `DynamicUser`, `ProtectSystem=strict`, `NoNewPrivileges`
- fail2ban for SSH brute force protection

### Application
- Built-in rate limiting (1000 pps, 1 MB/s per client default)
- Abuse detection (amplification + reflection attacks)
- Destination IP validation (blocks private IP ranges, localhost, multicast)
- Session management with automatic timeout (300s default)

## Monitoring (Optional)

### Health Checks

All nodes expose `GET /health` on port 8080:

```json
{
  "status": "ok",
  "node_id": "proxy-1",
  "uptime_secs": 86400,
  "active_sessions": 3,
  "total_packets_relayed": 1234567
}
```

### Prometheus + Grafana

```bash
cd infra/monitoring
docker compose up -d
# Grafana: http://localhost:3000 (admin/admin)
# Prometheus: http://localhost:9090
```

## Troubleshooting

### Health check failing
```bash
ssh root@<vps-ip>
systemctl status lightspeed-proxy
journalctl -u lightspeed-proxy --tail 50
ss -ulnp | grep 4434
```

### Binary not starting
```bash
# Check permissions
ls -la /usr/local/bin/lightspeed-proxy
# Verify config
cat /etc/lightspeed/proxy.toml
```

### Updating the proxy
```bash
# Rebuild and redeploy
cargo build --release -p lightspeed-proxy
./infra/scripts/deploy.sh
```
