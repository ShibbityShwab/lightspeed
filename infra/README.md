# ⚡ LightSpeed Infrastructure

> Deploy a lightweight proxy mesh on Vultr Cloud — ~500KB RAM per node.

## Architecture

```
┌──────────────────────────────────────────────────────────────────┐
│                         Vultr Cloud                              │
│                                                                  │
│  ┌────────────────────┐          ┌────────────────────┐         │
│  │  proxy-lax          │          │  relay-sgp          │         │
│  │  149.28.84.139      │          │  149.28.144.74      │         │
│  │  US-West (LA)       │          │  Asia (Singapore)   │         │
│  │                     │          │                     │         │
│  │  vc2-1c-1gb         │          │  vc2-1c-1gb         │         │
│  │  1 vCPU / 1GB RAM   │          │  1 vCPU / 1GB RAM   │         │
│  │  25 GB SSD          │          │  25 GB SSD          │         │
│  │                     │          │                     │         │
│  │  UDP :4434 (data)   │          │  UDP :4434 (data)   │         │
│  │  HTTP :8080 (health)│          │  HTTP :8080 (health)│         │
│  │  504KB actual RAM   │          │  496KB actual RAM   │         │
│  └────────────────────┘          └────────────────────┘         │
│                                                                  │
│  Deployment: Native binary + systemd (no Docker)                 │
│  Cost: $0/mo ($300 Vultr credit = 60+ months free)               │
└──────────────────────────────────────────────────────────────────┘
```

## Region Selection Rationale

| Region | Provider | Game Server Coverage | Strategic Value |
|--------|----------|---------------------|-----------------|
| **US-West (LA)** | Vultr | Fortnite NA-West, CS2 US-West, Dota 2 US-West | Best Asia peering (206ms from BKK), closest to ExitLag speed |
| **Asia (SGP)** | Vultr | SEA game servers, FEC multipath relay | 31ms from Bangkok, FEC redundant path, SEA regional coverage |

### Why Vultr (Not OCI)

| Factor | OCI Always Free | Vultr |
|--------|----------------|-------|
| **ARM capacity** | "Out of host capacity" errors | Always available |
| **Asia peering** | 213ms from BKK (San Jose) | 206ms from BKK (LA) — 7ms better |
| **Cost** | Free (when capacity available) | $300 credit = 60 months free |
| **Deployment** | Complex cloud-init | Simple SCP + systemd |
| **Decision** | Decommissioned (D-001, D-005) | ✅ Primary provider |

### Previous Infrastructure (Decommissioned)

| Node | Provider | Status | Reason |
|------|----------|--------|--------|
| proxy-us-west (163.192.3.134) | OCI San Jose | ❌ Dropped | ARM capacity issues, worse peering than Vultr LA |

## Quick Start

### Prerequisites

- SSH access to Vultr instances
- Compiled `lightspeed-proxy` binary for linux/amd64

### 1. Build the Proxy

```bash
# Cross-compile for Linux (from any platform)
cargo build --release -p lightspeed-proxy --target x86_64-unknown-linux-gnu

# Or build natively on a Linux machine
cargo build --release -p lightspeed-proxy
```

### 2. Deploy

```bash
# Copy binary to server
scp target/release/lightspeed-proxy root@149.28.84.139:/usr/local/bin/

# Copy config
scp proxy/proxy.toml.default root@149.28.84.139:/etc/lightspeed/proxy.toml

# Install systemd service
ssh root@149.28.84.139 'cat > /etc/systemd/system/lightspeed-proxy.service' << 'EOF'
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

# Enable and start
ssh root@149.28.84.139 'systemctl daemon-reload && systemctl enable --now lightspeed-proxy'
```

### 3. Verify

```bash
# Health check
curl http://149.28.84.139:8080/health
# Expected: {"status":"ok","node_id":"proxy-lax","region":"us-west-lax",...}

# Health check all nodes
cd infra/scripts
bash mesh-health.sh
```

### 4. Update Proxy

```bash
# Rolling update via script
cd infra/scripts
bash deploy-all.sh
```

## Directory Structure

```
infra/
├── terraform/                  # Infrastructure as Code (OCI — legacy)
│   ├── versions.tf             # Provider requirements
│   ├── provider.tf             # OCI provider config
│   ├── variables.tf            # Input variables
│   ├── networking.tf           # VCN, subnets, security lists
│   ├── instances.tf            # Compute instances + cloud-init
│   ├── outputs.tf              # Deployment outputs
│   ├── terraform.tfvars.example # Example configuration
│   ├── .gitignore              # Exclude secrets & state
│   └── templates/              # Cloud-init templates
│       ├── proxy.toml.tpl      # Per-region proxy config
│       ├── lightspeed-proxy.service  # Systemd unit
│       ├── fail2ban-lightspeed.conf  # Abuse protection
│       └── deploy.sh           # On-node deployment script
├── docker/                     # Container build (optional)
│   ├── Dockerfile              # Multi-stage ARM64/AMD64
│   └── docker-compose.yml      # Local dev / single node
├── fly/                        # Fly.io config (future)
│   └── fly.toml                # Fly.io deployment
├── scripts/                    # Operations scripts
│   ├── mesh-health.sh          # Check all node health
│   └── deploy-all.sh           # Rolling update all nodes
└── README.md                   # This file
```

## Deployment Options

### Native Binary (Recommended — Production)

Current production deployment. ~500KB RAM, no container overhead.

```bash
# Binary on server + systemd
/usr/local/bin/lightspeed-proxy
# Config at /etc/lightspeed/proxy.toml
# Systemd: systemctl status lightspeed-proxy
```

### Docker (Development / Testing)

Available but NOT used in production (adds ~175MB RAM overhead).

```bash
# Local development
cd infra/docker
docker-compose up

# Pull from GHCR
docker pull ghcr.io/shibbityshwab/lightspeed-proxy:latest
docker run -p 4434:4434/udp -p 8080:8080 ghcr.io/shibbityshwab/lightspeed-proxy:latest
```

### Terraform (OCI — Legacy)

Terraform configs exist for OCI Always Free deployment but are no longer the primary path.

```bash
cd infra/terraform
cp terraform.tfvars.example terraform.tfvars
terraform init && terraform plan && terraform apply
```

## Security

### Network
- UFW/iptables restrict ingress to only required ports (UDP 4434, TCP 8080)
- SSH restricted by CIDR or key-only

### Host
- systemd sandboxing: `DynamicUser`, `ProtectSystem=strict`, `NoNewPrivileges`, `MemoryDenyWriteExecute`
- fail2ban for SSH brute force protection
- SELinux context for binary (`bin_t`)

### Application
- Built-in rate limiting (1000 pps, 1 MB/s per client default)
- Abuse detection (amplification + reflection attacks)
- Destination IP validation (blocks private IP ranges, localhost, multicast)
- Session management with automatic timeout and cleanup (300s default)

## Scaling

### Current Mesh (2 nodes)

| Node | Role | Cost |
|------|------|------|
| proxy-lax | Primary US proxy | $0 (Vultr credit) |
| relay-sgp | FEC multipath + SEA | $0 (Vultr credit) |

### Future Expansion

| Region | Provider | Coverage | Priority |
|--------|----------|----------|----------|
| US-East | Vultr | NA-East game servers | P1 |
| EU-West | Vultr | EU game servers | P1 |
| Japan | Vultr | East Asia competitive | P2 |
| Brazil | Vultr | South America | P3 |
| Bangkok | Vultr (if available) | **Would beat ExitLag by 10ms!** | P0 (monitoring) |

> **Note:** Vultr $300 credit supports ~5 nodes × 60 months. Additional regions are trivially deployable with the same binary + systemd pattern.

## Monitoring

### Health Checks

All nodes expose `GET /health` on port 8080:

```json
{
  "status": "ok",
  "node_id": "proxy-lax",
  "region": "us-west-lax",
  "uptime_secs": 86400,
  "active_sessions": 3,
  "total_packets_relayed": 1234567
}
```

### Prometheus Metrics

Available at `/metrics` (when enabled in config):
- `lightspeed_connections_active` — current active sessions
- `lightspeed_packets_total` — total packets relayed
- `lightspeed_bytes_total` — total bytes relayed
- `lightspeed_latency_us` — relay latency histogram

## Troubleshooting

### Health check failing
```bash
# SSH into the node
ssh root@<ip>

# Check service status
systemctl status lightspeed-proxy
journalctl -u lightspeed-proxy --tail 50

# Check port binding
ss -ulnp | grep 4434
ss -tlnp | grep 8080
```

### Binary not starting
```bash
# Check SELinux context
ls -Z /usr/local/bin/lightspeed-proxy
# Should be: system_u:object_r:bin_t:s0

# Fix if needed
chcon -t bin_t /usr/local/bin/lightspeed-proxy
```

### High latency
```bash
# Check from client
lightspeed --game fortnite --proxy <ip>:4434 --test

# Traceroute to proxy
traceroute -U -p 4434 <ip>

# Consider enabling WARP for 5-10ms local route improvement
```
