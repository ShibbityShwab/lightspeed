#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Provision New Proxy Nodes
#
# Creates new VPS for mesh expansion using the API.
# Installs the proxy binary, systemd service, and proxy config.
#
# Usage:
#   export VPS_API_KEY="your-api-key"
#   ./provision.sh [region...]
#
# Examples:
#   ./provision.sh ewr ams         # US-East + EU-West
#   ./provision.sh ewr ams nrt     # + Japan
#   ./provision.sh --list-regions  # Show available regions
#
# Prerequisites:
#   - YOUR API key (YOUR provider settings page)
#   - SSH key already uploaded to your provider (or specify VPS_SSH_KEY_ID)
#   - jq, curl
# ──────────────────────────────────────────────────────────────
set -euo pipefail

PROVIDER_API_KEY="${VPS_API_KEY:-}"
VPS_API_URL="https://YOUR_PROVIDER_API_ENDPOINT/v2"
PLAN="vc2-1c-1gb"  # $6/mo — 1 vCPU, 1GB RAM, 25GB SSD (cheapest with IPv4)
OS_ID=2136          # Ubuntu 24.04 LTS
SSH_KEY_ID="${VPS_SSH_KEY_ID:-}"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
SSH_KEY="${DEPLOY_SSH_KEY:-$HOME/.ssh/id_ed25519}"
SSH_OPTS="-o StrictHostKeyChecking=accept-new -o ConnectTimeout=10 -o BatchMode=yes"
RELAY_INSTALLER="$SCRIPT_DIR/relay-install.sh"
UPDATER="$SCRIPT_DIR/relay-updater.sh"
SYSTEMD_DIR="$SCRIPT_DIR/../systemd"

if [ -n "${LIGHTSPEED_VERSION:-}" ]; then
    RELEASE_VERSION="$LIGHTSPEED_VERSION"
else
    RELEASE_VERSION="$(date -u +%Y%m%dT%H%M%SZ)-$(git -C "$PROJECT_ROOT" rev-parse --short HEAD 2>/dev/null || printf 'nogit')"
fi

# ── Node name mapping ────────────────────────────────────────
declare -A REGION_NAMES
REGION_NAMES["ewr"]="relay-ewr"      # New Jersey (US-East)
REGION_NAMES["ams"]="relay-ams"      # Amsterdam (EU-West)
REGION_NAMES["nrt"]="relay-nrt"      # Tokyo (Asia-NE)
REGION_NAMES["lhr"]="relay-lhr"      # London (EU)
REGION_NAMES["fra"]="relay-fra"      # Frankfurt (EU-Central)
REGION_NAMES["atl"]="relay-atl"      # Atlanta (US-Southeast)
REGION_NAMES["mia"]="relay-mia"      # Miami (US-Southeast)
REGION_NAMES["ord"]="relay-ord"      # Chicago (US-Central)
REGION_NAMES["syd"]="relay-syd"      # Sydney (Oceania)
REGION_NAMES["blr"]="relay-blr"      # Bangalore (India)
REGION_NAMES["sao"]="relay-sao"      # São Paulo (Brazil)

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
        -H "Authorization: Bearer $PROVIDER_API_KEY" \
        -H "Content-Type: application/json" \
        "$VPS_API_URL/$endpoint" "$@"
}

list_regions() {
    echo "Available provider regions:"
    api GET "regions" | jq -r '.regions[] | select(.options | index("vc2")) | "\(.id)\t\(.city), \(.country)"' | sort
}

get_ssh_keys() {
    api GET "ssh-keys" | jq -r '.ssh_keys[0].id'
}

# Install the provided binary as the node's first versioned release.
install_first_release() {
    local ip="$1"
    local binary="$PROJECT_ROOT/target/release/lightspeed-proxy"
    if [ ! -f "$binary" ] || [ ! -f "$RELAY_INSTALLER" ]; then
        echo "  No local binary/installer; run deploy.sh to install the first release."
        return 0
    fi
    echo "  Installing first release ($RELEASE_VERSION)..."
    scp $SSH_OPTS -i "$SSH_KEY" "$binary" "root@$ip:/tmp/lightspeed-proxy.staged" || return 1
    scp $SSH_OPTS -i "$SSH_KEY" "$RELAY_INSTALLER" "root@$ip:/tmp/lightspeed-relay-install.sh" || return 1
    ssh $SSH_OPTS -i "$SSH_KEY" "root@$ip" \
        "bash /tmp/lightspeed-relay-install.sh --binary /tmp/lightspeed-proxy.staged --version '$RELEASE_VERSION'" || return 1

    # Install the self-updater and enable its timer once a release is live.
    if [ ! -f "$UPDATER" ] || [ ! -f "$SYSTEMD_DIR/lightspeed-update.service" ] \
        || [ ! -f "$SYSTEMD_DIR/lightspeed-update.timer" ]; then
        echo "  Updater/unit files missing; self-update timer not installed."
        return 0
    fi
    scp $SSH_OPTS -i "$SSH_KEY" "$UPDATER" "root@$ip:/tmp/lightspeed-relay-updater.sh" || return 1
    scp $SSH_OPTS -i "$SSH_KEY" "$SYSTEMD_DIR/lightspeed-update.service" "root@$ip:/tmp/lightspeed-update.service" || return 1
    scp $SSH_OPTS -i "$SSH_KEY" "$SYSTEMD_DIR/lightspeed-update.timer" "root@$ip:/tmp/lightspeed-update.timer" || return 1
    ssh $SSH_OPTS -i "$SSH_KEY" "root@$ip" \
        "install -d /usr/local/lib/lightspeed && \
         install -m 0755 /tmp/lightspeed-relay-install.sh /usr/local/lib/lightspeed/relay-install.sh && \
         install -m 0755 /tmp/lightspeed-relay-updater.sh /usr/local/lib/lightspeed/relay-updater.sh && \
         install -m 0644 /tmp/lightspeed-update.service /etc/systemd/system/lightspeed-update.service && \
         install -m 0644 /tmp/lightspeed-update.timer /etc/systemd/system/lightspeed-update.timer && \
         systemctl daemon-reload && \
         systemctl enable --now lightspeed-update.timer" || return 1
    echo "  Self-update timer enabled."
}

# ── Deploy script (runs on new instance via startup script) ──
STARTUP_SCRIPT='#!/bin/bash
set -euo pipefail

# Wait for network
sleep 5

# Install dependencies
apt-get update -qq && apt-get install -y -qq curl jq

# Create user and directories, plus the versioned release layout
useradd -r -s /bin/false lightspeed 2>/dev/null || true
mkdir -p /etc/lightspeed
install -d /opt/lightspeed/releases

# The first versioned release is installed over SSH by provision.sh after the
# instance boots (relay-install.sh), which also repoints /opt/lightspeed/current.
echo "LightSpeed proxy node provisioned. First release installs via relay-install.sh."

# Create systemd service (runs the active version via the current symlink)
cat > /etc/systemd/system/lightspeed-proxy.service << "UNIT"
[Unit]
Description=LightSpeed Proxy Server
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
NotifyAccess=main
WatchdogSec=30
DynamicUser=yes
StateDirectory=lightspeed
RuntimeDirectory=lightspeed
RuntimeDirectoryMode=0750
Environment=LIGHTSPEED_TLS_DIR=/var/lib/lightspeed/tls
ExecStart=/opt/lightspeed/current/lightspeed-proxy --config /etc/lightspeed/proxy.toml --data-bind 0.0.0.0:4434 --control-bind 0.0.0.0:4433 --health-bind 0.0.0.0:8080
Restart=on-failure
RestartSec=2
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
if [ -z "$PROVIDER_API_KEY" ]; then
    echo -e "${RED}Error: VPS_API_KEY not set${NC}"
    echo "  export VPS_API_KEY='your-api-key-here'"
    echo "  Get it from: YOUR provider settings page"
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
        echo -e "${RED}No SSH key found in provider account. Upload one first.${NC}"
        exit 1
    fi
    echo "Using SSH key: $SSH_KEY_ID"
fi

echo "⚡ LightSpeed Mesh Expansion"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Plan:    $PLAN"
echo "  Regions: $*"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Create startup script on provider
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
    PROXY_TOML="[server]\nnode_id = \"$node_name\"\nregion = \"$region\"\nmax_clients = 100\n\n[security]\nrequire_auth = true\nmax_amplification_ratio = 2.0\n\n[rate_limit]\nmax_pps_per_client = 1000\nmax_bps_per_client = 1000000\n\n[metrics]\nenabled = true\ninterval_secs = 10"

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
            echo "        provider: \"your-provider\""
            echo ""
            echo "  Add to deploy.sh:"
            echo "    NODES[\"$node_name\"]=\"$IP\""
            echo ""
            echo "  Deploy proxy:"
            echo "    ./infra/scripts/deploy.sh $node_name"
            echo ""
            if install_first_release "$IP"; then
                echo "  ✅ First release installed and healthy"
            else
                echo "  ⚠️  First release install failed; run deploy.sh"
            fi
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
echo "  2. Run: ./deploy.sh  (deploys proxy binary to all nodes)"
echo "  3. Update infra/monitoring/prometheus/prometheus.yml with new IPs"
echo "  4. Restart Prometheus: curl -X POST http://localhost:9090/-/reload"
