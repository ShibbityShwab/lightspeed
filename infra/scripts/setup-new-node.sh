#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Setup New Proxy Node (from scratch)
#
# Complete setup for a fresh Vultr (or any Ubuntu/Debian) instance.
# Installs the proxy binary, config, systemd service, firewall,
# and verifies health. Run from your LOCAL machine.
#
# Usage:
#   ./setup-new-node.sh <ip> <node-id> <region>
#
# Examples:
#   ./setup-new-node.sh 1.2.3.4 proxy-ewr ewr
#   ./setup-new-node.sh 5.6.7.8 proxy-ams ams
#
# Prerequisites:
#   - SSH access to the node (key at ~/.ssh/id_ed25519)
#   - Linux proxy binary at target/release/lightspeed-proxy
#     (build with: docker run --rm -v "$PWD:/src" -w /src rust:latest cargo build --release --bin lightspeed-proxy)
# ──────────────────────────────────────────────────────────────
set -euo pipefail

# ── Args ─────────────────────────────────────────────────────
if [ $# -lt 3 ]; then
    echo "Usage: $0 <ip> <node-id> <region>"
    echo ""
    echo "Examples:"
    echo "  $0 1.2.3.4 proxy-ewr ewr    # New Jersey"
    echo "  $0 5.6.7.8 proxy-ams ams    # Amsterdam"
    echo ""
    echo "Create instances at: https://my.vultr.com/deploy/"
    echo "  Type: Cloud Compute (Regular)"
    echo "  Plan: vc2-1c-1gb (\$6/mo)"
    echo "  OS:   Ubuntu 24.04 LTS"
    echo "  Add your SSH key"
    exit 1
fi

NODE_IP="$1"
NODE_ID="$2"
REGION="$3"

SSH_KEY="${DEPLOY_SSH_KEY:-$HOME/.ssh/id_ed25519}"
SSH_USER="root"
SSH_OPTS="-o StrictHostKeyChecking=no -o ConnectTimeout=10 -o BatchMode=yes"

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
BINARY="$PROJECT_ROOT/target/release/lightspeed-proxy"

GREEN='\033[0;32m'
RED='\033[0;31m'
CYAN='\033[0;36m'
NC='\033[0m'

echo "⚡ LightSpeed — New Node Setup"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  Node:   $NODE_ID"
echo "  IP:     $NODE_IP"
echo "  Region: $REGION"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ── Step 1: Verify binary exists ─────────────────────────────
echo -e "\n${CYAN}[1/6] Checking binary...${NC}"
if [ ! -f "$BINARY" ]; then
    echo -e "${RED}Binary not found at $BINARY${NC}"
    echo "Build it with:"
    echo "  cd $PROJECT_ROOT"
    echo "  docker run --rm -v \"\$PWD:/src\" -w /src rust:latest cargo build --release --bin lightspeed-proxy"
    exit 1
fi
echo "  Binary: $BINARY ($(du -h "$BINARY" | cut -f1))"

# ── Step 2: Test SSH ─────────────────────────────────────────
echo -e "\n${CYAN}[2/6] Testing SSH...${NC}"
if ! ssh $SSH_OPTS -i "$SSH_KEY" "$SSH_USER@$NODE_IP" "echo OK" 2>/dev/null; then
    echo -e "${RED}Cannot SSH to $NODE_IP${NC}"
    echo "  Make sure your SSH key is added to the instance."
    exit 1
fi
echo "  SSH OK"

# ── Step 3: Upload binary ────────────────────────────────────
echo -e "\n${CYAN}[3/6] Uploading binary...${NC}"
scp $SSH_OPTS -i "$SSH_KEY" "$BINARY" "$SSH_USER@$NODE_IP:/tmp/lightspeed-proxy"
echo "  Uploaded"

# ── Step 4: Configure node ───────────────────────────────────
echo -e "\n${CYAN}[4/6] Configuring node...${NC}"
ssh $SSH_OPTS -i "$SSH_KEY" "$SSH_USER@$NODE_IP" bash -s "$NODE_ID" "$REGION" << 'REMOTE'
set -euo pipefail
NODE_ID="$1"
REGION="$2"

# Install binary
chmod +x /tmp/lightspeed-proxy
mv /tmp/lightspeed-proxy /usr/local/bin/lightspeed-proxy

# Create config directory
mkdir -p /etc/lightspeed

# Write proxy.toml
cat > /etc/lightspeed/proxy.toml << EOF
[server]
node_id     = "$NODE_ID"
region      = "$REGION"
max_clients = 100

[security]
require_auth                = false
max_amplification_ratio     = 2.0
max_destinations_per_window = 10
ban_duration_secs           = 3600

[rate_limit]
max_pps_per_client = 1000
max_bps_per_client = 1000000
max_connections    = 200

[metrics]
enabled       = true
interval_secs = 10
EOF

# Write systemd service
cat > /etc/systemd/system/lightspeed-proxy.service << 'UNIT'
[Unit]
Description=LightSpeed Proxy — UDP game latency optimizer
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
Restart=always
RestartSec=5
ExecStart=/usr/local/bin/lightspeed-proxy \
  --config /etc/lightspeed/proxy.toml \
  --data-bind 0.0.0.0:4434 \
  --control-bind 0.0.0.0:4433 \
  --health-bind 0.0.0.0:8080
DynamicUser=true
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadOnlyPaths=/etc/lightspeed
PrivateTmp=true
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

# Configure firewall (if ufw available)
if command -v ufw &>/dev/null; then
    ufw allow 22/tcp   comment "SSH" 2>/dev/null || true
    ufw allow 8080/tcp comment "LightSpeed health" 2>/dev/null || true
    ufw allow 4434/udp comment "LightSpeed data" 2>/dev/null || true
    ufw allow 4433/udp comment "LightSpeed control" 2>/dev/null || true
    echo "y" | ufw enable 2>/dev/null || true
fi

# Enable and start
systemctl daemon-reload
systemctl enable lightspeed-proxy
systemctl start lightspeed-proxy

echo "Service started"
REMOTE
echo "  Configured"

# ── Step 5: Verify health ────────────────────────────────────
echo -e "\n${CYAN}[5/6] Verifying health...${NC}"
sleep 3
HEALTH=$(curl -sf --max-time 5 "http://$NODE_IP:8080/health" 2>/dev/null || echo "FAIL")
if echo "$HEALTH" | grep -q "healthy"; then
    echo -e "  ${GREEN}✅ HEALTHY${NC}"
    echo "  $HEALTH" | python3 -m json.tool 2>/dev/null || echo "  $HEALTH"
else
    echo -e "  ${RED}⚠️ Health check failed${NC}"
    echo "  Response: $HEALTH"
    echo "  Check logs: ssh root@$NODE_IP 'journalctl -u lightspeed-proxy -n 20'"
fi

# ── Step 6: Print config snippets ────────────────────────────
echo -e "\n${CYAN}[6/6] Config snippets for your files:${NC}"
echo ""
echo "  ── Add to prometheus.yml ──"
echo "      - targets: [\"$NODE_IP:8080\"]"
echo "        labels:"
echo "          node_id: \"$NODE_ID\""
echo "          region: \"$REGION\""
echo "          provider: \"vultr\""
echo ""
echo "  ── Add to deploy-vultr.sh ──"
echo "      NODES[\"$NODE_ID\"]=\"$NODE_IP\""
echo ""
echo "  ── Add to mesh-health.sh BUILTIN_NODES ──"
echo "      \"$REGION\": {"
echo "        \"health_url\": \"http://$NODE_IP:8080/health\","
echo "        \"metrics_url\": \"http://$NODE_IP:8080/metrics\","
echo "        \"node_id\": \"$NODE_ID\","
echo "        \"region\": \"$REGION\""
echo "      }"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo -e "${GREEN}✅ Node $NODE_ID ($NODE_IP) is live!${NC}"
echo ""
echo "Test with:"
echo "  curl http://$NODE_IP:8080/health"
echo "  curl http://$NODE_IP:8080/metrics"
echo "  python tools/load_test.py $NODE_IP -c 10 -d 30"
