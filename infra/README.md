# ⚡ LightSpeed Infrastructure

> Deploy a zero-cost proxy mesh on Oracle Cloud Always Free tier.

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    Oracle Cloud Always Free                      │
│                                                                 │
│  ┌───────────────┐  ┌───────────────┐  ┌───────────────┐      │
│  │  US-East       │  │  EU-West       │  │  Asia-SE       │      │
│  │  (Ashburn)     │  │  (Frankfurt)   │  │  (Singapore)   │      │
│  │                │  │                │  │                │      │
│  │  1 OCPU ARM    │  │  1 OCPU ARM    │  │  1 OCPU ARM    │      │
│  │  6 GB RAM      │  │  6 GB RAM      │  │  6 GB RAM      │      │
│  │  50 GB disk    │  │  50 GB disk    │  │  50 GB disk    │      │
│  │                │  │                │  │                │      │
│  │  UDP :4434     │  │  UDP :4434     │  │  UDP :4434     │      │
│  │  QUIC :4433    │  │  QUIC :4433    │  │  QUIC :4433    │      │
│  │  HTTP :8080    │  │  HTTP :8080    │  │  HTTP :8080    │      │
│  └───────────────┘  └───────────────┘  └───────────────┘      │
│                                                                 │
│  Total: 3 OCPUs / 18 GB RAM / 150 GB disk (of 4/24/200 free)  │
└─────────────────────────────────────────────────────────────────┘
```

## Region Selection Rationale

| Region | OCI Identifier | Game Server Coverage | Strategic Value |
|--------|---------------|---------------------|-----------------|
| **US-East** | `us-ashburn-1` | Fortnite NA-East, CS2 US-East, Dota 2 US-East | AWS peering (Fortnite), Valve peering |
| **EU-West** | `eu-frankfurt-1` | All EU game servers, Russia-adjacent | Major IX hub (DE-CIX), broad EU coverage |
| **Asia-SE** | `ap-singapore-1` | SEA servers, OCE routing, India overflow | Major SEA hub, high latency improvement potential |

### Free Tier Budget

| Resource | Limit | Used (3 nodes) | Remaining |
|----------|-------|-----------------|-----------|
| ARM OCPUs | 4 | 3 | 1 |
| RAM | 24 GB | 18 GB | 6 GB |
| Block Storage | 200 GB | 150 GB | 50 GB |
| Outbound Data | 10 TB/mo | ~3 TB (est. 1000 users) | ~7 TB |

## Quick Start

### Prerequisites

- [Terraform](https://www.terraform.io/downloads) ≥ 1.5
- [Oracle Cloud account](https://cloud.oracle.com/free) (Always Free tier)
- OCI API key configured (`~/.oci/config`)

### 1. Configure

```bash
cd infra/terraform
cp terraform.tfvars.example terraform.tfvars
# Edit terraform.tfvars with your OCI credentials
```

### 2. Deploy

```bash
terraform init
terraform plan          # Review what will be created
terraform apply         # Deploy (type 'yes' to confirm)
```

### 3. Verify

```bash
# Check outputs
terraform output proxy_nodes

# Health check all nodes
cd ../scripts
bash mesh-health.sh
```

### 4. Update Proxy

```bash
# After pushing new proxy code (triggers Docker build via CI):
cd infra/scripts
bash deploy-all.sh
```

## Directory Structure

```
infra/
├── terraform/                  # Infrastructure as Code
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
├── docker/                     # Container build
│   ├── Dockerfile              # Multi-stage ARM64/AMD64
│   └── docker-compose.yml      # Local dev / single node
├── scripts/                    # Operations scripts
│   ├── mesh-health.sh          # Check all node health
│   └── deploy-all.sh           # Rolling update all nodes
└── README.md                   # This file
```

## Security

### Network
- Security lists restrict ingress to only required ports
- SSH restricted by CIDR (configure `ssh_allowed_cidrs`)
- Stateless UDP rules for minimal overhead

### Host
- Docker containers run as non-root user
- fail2ban protects against SSH brute force and proxy abuse
- firewalld as secondary firewall layer
- Kernel tuned for UDP performance

### Application
- Built-in rate limiting (1000 pps, 1 MB/s per client)
- Abuse detection (amplification + reflection attacks)
- Destination IP validation (blocks private IP ranges)
- Session management with automatic cleanup

## Scaling

### Phase 2 Regions (after MVP)

| Region | OCI Identifier | Coverage |
|--------|---------------|----------|
| US-West | `us-phoenix-1` | NA-West game servers |
| Brazil | `sa-saopaulo-1` | South America servers |

### Phase 3 Regions

| Region | OCI Identifier | Coverage |
|--------|---------------|----------|
| Japan | `ap-tokyo-1` | East Asia competitive |
| India | `ap-mumbai-1` | India/South Asia |

> **Note:** Additional regions require additional Oracle Cloud accounts
> (each account gets its own Always Free allocation).

## Troubleshooting

### Instance fails to create
Oracle Cloud ARM instances are high-demand. If you get "Out of capacity":
1. Try a different availability domain
2. Try at off-peak hours (early morning UTC)
3. Use the OCI Console to retry manually

### Docker image not pulling
The Docker image is built by GitHub Actions on push to master.
Ensure the `docker.yml` workflow has run successfully first.

### Health check failing
```bash
# SSH into the node
ssh -i lightspeed_deploy_key opc@<ip>

# Check container status
sudo docker ps -a
sudo docker logs lightspeed-proxy --tail 50

# Check port binding
sudo ss -ulnp | grep 4434
sudo ss -tlnp | grep 8080
```
