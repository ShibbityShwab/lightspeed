#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Deploy Updated Proxy to Vultr Mesh
#
# Cross-compiles the proxy binary for Linux x86_64, uploads via
# SCP, and restarts the systemd service on each node.
#
# Usage:
#   ./deploy-vultr.sh                    # Deploy to all nodes
#   ./deploy-vultr.sh proxy-lax          # Deploy to specific node
#   ./deploy-vultr.sh --build-only       # Just compile, don't deploy
#
# Prerequisites:
#   - SSH key at ~/.ssh/lightspeed_vultr (or set DEPLOY_SSH_KEY)
#   - Rust cross-compilation target: rustup target add x86_64-unknown-linux-gnu
#   - Or use cross: cargo install cross
# ──────────────────────────────────────────────────────────────
set -euo pipefail

# ── Configuration ────────────────────────────────────────────
SSH_KEY="${DEPLOY_SSH_KEY:-$HOME/.ssh/lightspeed_vultr}"
SSH_USER="${DEPLOY_SSH_USER:-root}"
SSH_OPTS="-o StrictHostKeyChecking=no -o ConnectTimeout=10 -o BatchMode=yes"
BINARY_NAME="lightspeed-proxy"
REMOTE_BINARY="/usr/local/bin/${BINARY_NAME}"
SERVICE_NAME="lightspeed-proxy"

# Vultr mesh nodes
declare -A NODES
NODES["proxy-lax"]="149.28.84.139"
NODES["relay-sgp"]="149.28.144.74"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# ── Parse args ───────────────────────────────────────────────
TARGET_NODE=""
BUILD_ONLY=false

for arg in "$@"; do
    case "$arg" in
        --build-only) BUILD_ONLY=true ;;
        *)            TARGET_NODE="$arg" ;;
    esac
done

echo "⚡ LightSpeed Proxy — Vultr Deployment"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ── Step 1: Build ────────────────────────────────────────────
echo -e "\n${CYAN}[1/3] Building release binary...${NC}"

cd "$PROJECT_ROOT"

# Try cross-compilation, fall back to native
if command -v cross &>/dev/null; then
    echo "  Using 'cross' for Linux x86_64 cross-compilation"
    cross build --release --bin "$BINARY_NAME" --target x86_64-unknown-linux-gnu
    BINARY_PATH="target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
elif rustup target list --installed | grep -q "x86_64-unknown-linux-gnu"; then
    echo "  Using cargo with x86_64-unknown-linux-gnu target"
    cargo build --release --bin "$BINARY_NAME" --target x86_64-unknown-linux-gnu
    BINARY_PATH="target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
else
    echo "  ⚠️  No Linux cross-compilation target available."
    echo "  Building for current platform (deploy only works if building on Linux)."
    cargo build --release --bin "$BINARY_NAME"
    BINARY_PATH="target/release/${BINARY_NAME}"
fi

if [ ! -f "$BINARY_PATH" ]; then
    echo -e "${RED}❌ Binary not found at $BINARY_PATH${NC}"
    exit 1
fi

BINARY_SIZE=$(du -h "$BINARY_PATH" | cut -f1)
echo -e "  ${GREEN}✅ Built: $BINARY_PATH ($BINARY_SIZE)${NC}"

if [ "$BUILD_ONLY" = true ]; then
    echo -e "\n${GREEN}Build complete. Skipping deployment (--build-only).${NC}"
    exit 0
fi

# ── Step 2: Verify SSH ───────────────────────────────────────
echo -e "\n${CYAN}[2/3] Verifying SSH access...${NC}"

if [ ! -f "$SSH_KEY" ]; then
    echo -e "${RED}❌ SSH key not found: $SSH_KEY${NC}"
    echo "  Set DEPLOY_SSH_KEY or place key at ~/.ssh/lightspeed_vultr"
    exit 1
fi

chmod 600 "$SSH_KEY" 2>/dev/null || true

# ── Step 3: Deploy to nodes ─────────────────────────────────
echo -e "\n${CYAN}[3/3] Deploying to nodes...${NC}"

deploy_node() {
    local name="$1"
    local ip="$2"

    printf "  %-12s %-16s " "$name" "$ip"

    # Pre-deploy health check
    local pre_health
    pre_health=$(curl -sf --max-time 5 "http://${ip}:8080/health" 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'v{d.get(\"version\",\"?\")}, up {d.get(\"uptime_secs\",0)}s')" 2>/dev/null || echo "unreachable")

    # Upload binary
    if ! scp $SSH_OPTS -i "$SSH_KEY" "$BINARY_PATH" "${SSH_USER}@${ip}:/tmp/${BINARY_NAME}.new" 2>/dev/null; then
        echo -e "${RED}❌ SCP failed${NC}"
        return 1
    fi

    # Stop service, swap binary, start service
    ssh $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ip}" bash -s <<'REMOTE_SCRIPT'
set -e
systemctl stop lightspeed-proxy 2>/dev/null || true
mv /tmp/lightspeed-proxy.new /usr/local/bin/lightspeed-proxy
chmod +x /usr/local/bin/lightspeed-proxy
systemctl start lightspeed-proxy
sleep 2
systemctl is-active lightspeed-proxy >/dev/null
REMOTE_SCRIPT

    if [ $? -eq 0 ]; then
        # Post-deploy health check
        sleep 3
        local post_health
        post_health=$(curl -sf --max-time 5 "http://${ip}:8080/health" 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'v{d.get(\"version\",\"?\")}, up {d.get(\"uptime_secs\",0)}s')" 2>/dev/null || echo "starting...")
        echo -e "${GREEN}✅ OK${NC}  [${pre_health}] → [${post_health}]"
    else
        echo -e "${RED}❌ Service failed to start${NC}"
        return 1
    fi
}

PASS=0
FAIL=0

if [ -n "$TARGET_NODE" ]; then
    # Deploy to specific node
    if [ -z "${NODES[$TARGET_NODE]:-}" ]; then
        echo -e "${RED}Unknown node: $TARGET_NODE${NC}"
        echo "Available: ${!NODES[*]}"
        exit 1
    fi
    deploy_node "$TARGET_NODE" "${NODES[$TARGET_NODE]}" && PASS=$((PASS+1)) || FAIL=$((FAIL+1))
else
    # Deploy to all nodes (rolling)
    for name in "${!NODES[@]}"; do
        deploy_node "$name" "${NODES[$name]}" && PASS=$((PASS+1)) || FAIL=$((FAIL+1))
    done
fi

# ── Summary ──────────────────────────────────────────────────
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Deployed: $PASS  Failed: $FAIL"

if [ $FAIL -gt 0 ]; then
    echo -e "${RED}⚠️  Some deployments failed!${NC}"
    exit 1
else
    echo -e "${GREEN}✅ All nodes updated${NC}"
fi
