#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Provision New Vultr Proxy Nodes
#
# Creates new Vultr instances for mesh expansion using the API.
# Installs the proxy binary, systemd service, and proxy config.
#
# Usage:
#   export VULTR_API_KEY="your-api-key"
#   ./provision-vultr.sh [region...]
#
# Examples:
#   ./provision-vultr.sh ewr ams         # US-East + EU-West
#   ./provision-vultr.sh ewr ams nrt     # + Japan
#   ./provision-vultr.sh --list-regions  # Show available regions
#
# Prerequisites:
#   - Vultr API key (https://my.vultr.com/settings/#settingsapi)
#   - SSH key already uploaded to Vultr (or specify VULTR_SSH_KEY_ID)
#   - jq, curl
# ──────────────────────────────────────────────────────────────
set -euo pipefail

VULTR_API="${VULTR_API_KEY:-}"
VULTR_API_URL="https://api.vultr.com/v2"
PLAN="vc2-1c-1gb"  # $6/mo — 1 vCPU, 1GB RAM, 25GB SSD (cheapest with IPv4)
OS_ID=2136          # Ubuntu 24.04 LTS
SSH_KEY_ID="${VULTR_SSH_KEY_ID:-}"

# ── Node name mapping ────────────────────────────────────────
declare -A REGION_NAMES
REGION_NAMES["ewr"]="proxy-ewr"      # New Jersey (US-East)
REGION_NAMES["ams"]="proxy-ams"      # Amsterdam (EU-West)
REGION_NAMES["nrt"]="proxy-nrt"      # Tokyo (Asia-NE)
REGION_NAMES["lhr"]="proxy-lhr"      # London (EU)
REGION_NAMES["fra"]="proxy-fra"      # Frankfurt (EU-Central)
REGION_NAMES["atl"]="proxy-atl"      # Atlanta (US-Southeast)
REGION_NAMES["mia"]="proxy-mia"      # Miami (US-Southeast)
REGION_NAMES["ord"]="proxy-ord"      # Chicago (US-Central)
REGION_NAMES["syd"]="proxy-syd"      # Sydney (Oceania)
REGION_NAMES["blr"]="proxy-blr"      # Bangalore (India)
REGION_NAMES["sao"]="proxy-sao"      # São Paulo (Brazil)

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

# ── Functions ────────────────────────────────────────────────
api() {
    local method="$1" endpoint="$2"
    shift 2
    curl -sf -X "$method" \
        -H "Authorization: Bearer $VULTR_API" \
        -H "Content-Type: application/json" \
        "$VULTR_API_URL/$endpoint" "$@"
}

list_regions() {
    echo "Available Vultr regions:"
    api GET "regions" | jq -r '.regions[] | select(.options | index("vc2")) | "\(.id)\t\(.city), \(.country)"' | sort
}

get_ssh_keys() {
    api GET "ssh-keys" | jq -r '.ssh_keys[0].id'
}

# ── Deploy script (runs on new instance via startup script) ──
STARTUP_SCRIPT='#!/bin/bash
set -euo pipefail

# Wait for network
sleep 5

# Install dependencies
apt-get update -qq && apt-get install -y -qq curl jq

# Create user and directories
useradd -r -s /bin/false lightspeed 2>/dev/null || true
mkdir -p /etc/lightspeed

# Download latest proxy binary from GitHub release (or GHCR)
# For now, placeholder — will be replaced by SCP deploy
echo "LightSpeed proxy node provisioned. Run deploy-vultr.sh to install binary."

# Create systemd service
cat > /etc/systemd/system/lightspeed-proxy.service << "UNIT"
[Unit]
Description=LightSpeed Proxy Server
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
DynamicUser=yes
ExecStart=/usr/local/bin/lightspeed-proxy --config /etc/lightspeed/proxy.toml --data-bind 0.0.0.0:4434 --control-bind 0.0.0.0:4433 --health-bind 0.0.0.0:8080
Restart=always
RestartSec=5
LimitNOFILE=65535

# Security hardening
ProtectSystem=strict
ProtectHome=yes
NoNewPrivileges=yes
PrivateTmp=yes
ReadWritePaths=/etc/lightspeed

[Install]
WantedBy=multi-user.target
UNIT

systemctl daemon-reload
systemctl enable lightspeed-proxy

# Firewall: allow health + data + control
ufw allow 8080/tcp comment "LightSpeed health"
ufw allow 4434/udp comment "LightSpeed data"
ufw allow 4433/udp comment "LightSpeed control"
ufw --force enable
'

# ── Main ─────────────────────────────────────────────────────
if [ -z "$VULTR_API" ]; then
    echo -e "${RED}Error: VULTR_API_KEY not set${NC}"
    echo "  export VULTR_API_KEY='your-api-key-here'"
    echo "  Get it from: https://my.vultr.com/settings/#settingsapi"
    exit 1
fi

# Handle --list-regions
for arg in "$@"; do
    if [ "$arg" = "--list-regions" ]; then
        list_regions
        exit 0
    fi
done

if [ $# -eq 0 ]; then
    echo "Usage: $0 [region...]"
    echo "  Example: $0 ewr ams"
    echo "  Use --list-regions to see available regions"
    exit 1
fi

# Get SSH key if not specified
if [ -z "$SSH_KEY_ID" ]; then
    SSH_KEY_ID=$(get_ssh_keys)
    if [ -z "$SSH_KEY_ID" ]; then
        echo -e "${RED}No SSH key found in Vultr account. Upload one first.${NC}"
        exit 1
    fi
    echo "Using SSH key: $SSH_KEY_ID"
fi

echo "⚡ LightSpeed Mesh Expansion"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Plan:    $PLAN"
echo "  Regions: $*"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Create startup script in Vultr
echo -e "\n${CYAN}Creating startup script...${NC}"
SCRIPT_RESP=$(api POST "startup-scripts" -d "{
    \"name\": \"lightspeed-proxy-init\",
    \"type\": \"boot\",
    \"script\": $(echo "$STARTUP_SCRIPT" | jq -Rs .)
}")
SCRIPT_ID=$(echo "$SCRIPT_RESP" | jq -r '.startup_script.id')
echo "  Script ID: $SCRIPT_ID"

# Provision each region
for region in "$@"; do
    node_name="${REGION_NAMES[$region]:-proxy-$region}"
    echo -e "\n${CYAN}Provisioning $node_name ($region)...${NC}"

    # Create proxy.toml for this region
    PROXY_TOML="[server]\nnode_id = \"$node_name\"\nregion = \"$region\"\nmax_clients = 100\n\n[security]\nrequire_auth = false\nmax_amplification_ratio = 2.0\n\n[rate_limit]\nmax_pps_per_client = 1000\nmax_bps_per_client = 1000000\n\n[metrics]\nenabled = true\ninterval_secs = 10"

    RESP=$(api POST "instances" -d "{
        \"region\": \"$region\",
        \"plan\": \"$PLAN\",
        \"os_id\": $OS_ID,
        \"label\": \"$node_name\",
        \"hostname\": \"$node_name\",
        \"sshkey_id\": [\"$SSH_KEY_ID\"],
        \"script_id\": \"$SCRIPT_ID\",
        \"backups\": \"disabled\",
        \"tags\": [\"lightspeed\", \"proxy\"]
    }")

    INSTANCE_ID=$(echo "$RESP" | jq -r '.instance.id')
    echo "  Instance ID: $INSTANCE_ID"
    echo "  Waiting for IP assignment..."

    # Poll for IP
    for i in $(seq 1 30); do
        sleep 10
        INFO=$(api GET "instances/$INSTANCE_ID")
        IP=$(echo "$INFO" | jq -r '.instance.main_ip')
        STATUS=$(echo "$INFO" | jq -r '.instance.status')
        POWER=$(echo "$INFO" | jq -r '.instance.power_status')

        if [ "$IP" != "0.0.0.0" ] && [ -n "$IP" ] && [ "$IP" != "null" ]; then
            echo -e "  ${GREEN}✅ $node_name: $IP (status: $STATUS, power: $POWER)${NC}"

            # Output node info for updating configs
            echo ""
            echo "  Add to prometheus.yml:"
            echo "    - targets: [\"$IP:8080\"]"
            echo "      labels:"
            echo "        node_id: \"$node_name\""
            echo "        region: \"$region\""
            echo "        provider: \"vultr\""
            echo ""
            echo "  Add to deploy-vultr.sh:"
            echo "    NODES[\"$node_name\"]=\"$IP\""
            echo ""
            echo "  Deploy proxy:"
            echo "    scp lightspeed-proxy root@$IP:/usr/local/bin/"
            echo "    scp proxy.toml root@$IP:/etc/lightspeed/proxy.toml"
            echo "    ssh root@$IP 'systemctl start lightspeed-proxy'"
            break
        fi

        printf "  Waiting... (%d/30)\r" "$i"
    done
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}✅ Provisioning complete${NC}"
echo ""
echo "Next steps:"
echo "  1. Wait ~2min for instances to boot"
echo "  2. Run: ./deploy-vultr.sh  (deploys proxy binary to all nodes)"
echo "  3. Update infra/monitoring/prometheus/prometheus.yml with new IPs"
echo "  4. Restart Prometheus: curl -X POST http://localhost:9090/-/reload"
