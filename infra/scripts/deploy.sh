#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Deploy Updated Proxy to Mesh Nodes
#
# Cross-compiles the proxy binary for Linux x86_64, uploads it to a
# staging path on each node, then runs relay-install.sh over SSH. The
# installer drops the binary into /opt/lightspeed/releases/<version>/,
# repoints /opt/lightspeed/current atomically, restarts systemd, and
# health-gates the result (rolling back on failure).
#
# Usage:
#   ./deploy.sh                    # Deploy to all nodes
#   ./deploy.sh relay-lax          # Deploy to specific node
#   ./deploy.sh --build-only       # Just compile, don't deploy
#
# Environment:
#   LIGHTSPEED_VERSION   Override the release version (default: the built
#                        binary's own version; an override must equal it).
#
# Prerequisites:
#   - SSH key at ~/.ssh/lightspeed_deploy (or set DEPLOY_SSH_KEY)
#   - Rust cross-compilation target: rustup target add x86_64-unknown-linux-gnu
#   - Or use cross: cargo install cross
# ──────────────────────────────────────────────────────────────
set -euo pipefail

# ── Configuration ────────────────────────────────────────────
SSH_KEY="${DEPLOY_SSH_KEY:-$HOME/.ssh/lightspeed_deploy}"
SSH_USER="${DEPLOY_SSH_USER:-root}"
SSH_OPTS="-o ConnectTimeout=10 -o BatchMode=yes -o StrictHostKeyChecking=accept-new"
BINARY_NAME="lightspeed-proxy"
REMOTE_STAGING="/tmp/${BINARY_NAME}.staged"
REMOTE_INSTALLER="/tmp/lightspeed-relay-install.sh"
REMOTE_UPDATER="/tmp/lightspeed-relay-updater.sh"

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
RELAY_INSTALLER="$SCRIPT_DIR/relay-install.sh"
RELAY_UPDATER="$SCRIPT_DIR/relay-updater.sh"

# ── Resolve node inventory (fail closed) ─────────────────────
# shellcheck source=lib-nodes.sh
source "$SCRIPT_DIR/lib-nodes.sh"

if ! NODES_JSON="$(lightspeed_resolve_nodes)"; then
    echo -e "${RED}❌ No proxy node inventory available — refusing to deploy.${NC}" >&2
    echo "   Set LIGHTSPEED_NODES or populate $LIGHTSPEED_REGISTRY_PATH." >&2
    exit 1
fi

if [ "$(printf '%s' "$NODES_JSON" | jq 'length')" -eq 0 ]; then
    echo -e "${RED}❌ Proxy node inventory is empty — refusing to deploy.${NC}" >&2
    echo "   Set LIGHTSPEED_NODES or populate $LIGHTSPEED_REGISTRY_PATH." >&2
    exit 1
fi

declare -A NODES
declare -A REGION_TO_NODE
NODE_NAMES=()
while IFS=$'\t' read -r _name _region _ip; do
    [ -n "$_name" ] || continue
    NODES["$_name"]="$_ip"
    NODE_NAMES+=("$_name")
    if [ -n "$_region" ] && [ -z "${REGION_TO_NODE[$_region]:-}" ]; then
        REGION_TO_NODE["$_region"]="$_name"
    fi
done < <(printf '%s' "$NODES_JSON" | jq -r '.[] | [.node_id, .region, .ip] | @tsv')

# ── Parse args ───────────────────────────────────────────────
TARGET_NODE=""
BUILD_ONLY=false

for arg in "$@"; do
    case "$arg" in
        --build-only) BUILD_ONLY=true ;;
        *)            TARGET_NODE="$arg" ;;
    esac
done

echo "⚡ LightSpeed Proxy — Proxy Deployment"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# ── Step 1: Build ────────────────────────────────────────────
echo -e "\n${CYAN}[1/4] Building release binary...${NC}"

cd "$PROJECT_ROOT"

# The proxy's `default` feature set is empty, so the QUIC control plane (client
# registration) only exists with `--features quic`. Building without it yields a
# data-only proxy that auth-rejects every packet. Never drop these flags.
if command -v cross &>/dev/null; then
    echo "  Using 'cross' for Linux x86_64 cross-compilation"
    cross build --release --bin "$BINARY_NAME" --features quic --target x86_64-unknown-linux-gnu
    BINARY_PATH="target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
elif rustup target list --installed | grep -q "x86_64-unknown-linux-gnu"; then
    echo "  Using cargo with x86_64-unknown-linux-gnu target"
    cargo build --release --bin "$BINARY_NAME" --features quic --target x86_64-unknown-linux-gnu
    BINARY_PATH="target/x86_64-unknown-linux-gnu/release/${BINARY_NAME}"
else
    echo "  ⚠️  No Linux cross-compilation target available."
    echo "  Building for current platform (deploy only works if building on Linux)."
    cargo build --release --bin "$BINARY_NAME" --features quic
    BINARY_PATH="target/release/${BINARY_NAME}"
fi

if [ ! -f "$BINARY_PATH" ]; then
    echo -e "${RED}❌ Binary not found at $BINARY_PATH${NC}"
    exit 1
fi

BINARY_SIZE=$(du -h "$BINARY_PATH" | cut -f1)
echo -e "  ${GREEN}✅ Built: $BINARY_PATH ($BINARY_SIZE)${NC}"

# Hard gate: the release string only exists in a binary compiled with the quic
# feature, so a miss means the control plane was compiled out. Search the file
# directly (a `strings | grep -q` pipeline would false-fail under pipefail when
# grep exits before strings finishes).
if ! grep -aqF "QUIC control plane listening" "$BINARY_PATH"; then
    echo -e "${RED}❌ The built proxy has no QUIC control plane (built without --features quic). Refusing to deploy.${NC}" >&2
    exit 1
fi

if [ "$BUILD_ONLY" = true ]; then
    echo -e "\n${GREEN}Build complete. Skipping deployment (--build-only).${NC}"
    exit 0
fi

# ── Step 2: Release version ──────────────────────────────────
# The label must equal the binary's own version: the in-place handoff checks
# that the requested version matches the new binary's compiled version, and a
# mismatch makes the handoff refuse it. Deriving the label from the binary also
# lets a redeploy of the same version no-op instead of reinstalling.
BINARY_VERSION="$("$BINARY_PATH" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+[A-Za-z0-9.-]*' | head -1 || true)"
if [ -n "${LIGHTSPEED_VERSION:-}" ] && [ "$LIGHTSPEED_VERSION" != "$BINARY_VERSION" ]; then
    echo -e "${RED}❌ LIGHTSPEED_VERSION '$LIGHTSPEED_VERSION' must equal the binary version '$BINARY_VERSION' for in-place handoff${NC}" >&2
    exit 1
fi
VERSION="${LIGHTSPEED_VERSION:-$BINARY_VERSION}"
if [ -z "$VERSION" ]; then
    echo -e "${RED}❌ Could not determine the release version from $BINARY_PATH${NC}" >&2
    exit 1
fi

case "$VERSION" in
    *[!A-Za-z0-9._-]*|.|..)
        echo -e "${RED}❌ Invalid LIGHTSPEED_VERSION: '$VERSION' (allowed: A-Za-z0-9._-)${NC}" >&2
        exit 1
        ;;
esac

if [ ! -f "$RELAY_INSTALLER" ]; then
    echo -e "${RED}❌ Relay installer not found: $RELAY_INSTALLER${NC}" >&2
    exit 1
fi

echo -e "  Release version: ${VERSION}"

# ── Step 3: Verify SSH ───────────────────────────────────────
echo -e "\n${CYAN}[3/4] Verifying SSH access...${NC}"

if [ ! -f "$SSH_KEY" ]; then
    echo -e "${RED}❌ SSH key not found: $SSH_KEY${NC}"
    echo "  Set DEPLOY_SSH_KEY or place key at ~/.ssh/lightspeed_deploy"
    exit 1
fi

chmod 600 "$SSH_KEY" 2>/dev/null || true

# ── Step 4: Deploy to nodes ─────────────────────────────────
echo -e "\n${CYAN}[4/4] Deploying to nodes...${NC}"

deploy_node() {
    local name="$1"
    local ip="$2"

    printf "  %-12s %-16s " "$name" "$ip"

    # Pre-deploy health check
    local pre_health
    pre_health=$(curl -sf --max-time 5 "http://${ip}:8080/health" 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'v{d.get(\"version\",\"?\")}, up {d.get(\"uptime_secs\",0)}s')" 2>/dev/null || echo "unreachable")

    # Upload the staged binary and the reusable installer.
    if ! scp $SSH_OPTS -i "$SSH_KEY" "$BINARY_PATH" \
            "${SSH_USER}@${ip}:${REMOTE_STAGING}" 2>/dev/null; then
        echo -e "${RED}❌ SCP (binary) failed${NC}"
        return 1
    fi

    if ! scp $SSH_OPTS -i "$SSH_KEY" "$RELAY_INSTALLER" \
            "${SSH_USER}@${ip}:${REMOTE_INSTALLER}" 2>/dev/null; then
        echo -e "${RED}❌ SCP (installer) failed${NC}"
        return 1
    fi

    # Ship the self-updater too: the deploy pipeline is the only place it can
    # be refreshed, since provisioning installs it once. Missing/failed upload
    # is non-fatal.
    local have_updater=0
    if [ -f "$RELAY_UPDATER" ]; then
        if scp $SSH_OPTS -i "$SSH_KEY" "$RELAY_UPDATER" \
                "${SSH_USER}@${ip}:${REMOTE_UPDATER}" 2>/dev/null; then
            have_updater=1
        else
            echo -e "${YELLOW}⚠️  SCP (updater) failed; deploying without it${NC}"
        fi
    fi

    # Versioned, health-gated activation with automatic rollback. public_ip
    # gives the proxy exact self-tunnel filtering; the updater is refreshed.
    local installer_cmd="bash ${REMOTE_INSTALLER} --binary ${REMOTE_STAGING} --version '${VERSION}' --public-ip '${ip}'"
    if [ "$have_updater" -eq 1 ]; then
        installer_cmd="$installer_cmd --updater ${REMOTE_UPDATER}"
    fi

    if ssh $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ip}" "$installer_cmd"; then
        sleep 2
        local post_health
        post_health=$(curl -sf --max-time 5 "http://${ip}:8080/health" 2>/dev/null | python3 -c "import sys,json; d=json.load(sys.stdin); print(f'v{d.get(\"version\",\"?\")}, up {d.get(\"uptime_secs\",0)}s')" 2>/dev/null || echo "starting...")
        echo -e "${GREEN}✅ OK${NC}  [${pre_health}] → [${post_health}]"

        # The health gate cannot see the control plane, so assert it here: a
        # binary without quic still serves /health while rejecting all clients.
        local control_up
        control_up=$(ssh $SSH_OPTS -i "$SSH_KEY" "${SSH_USER}@${ip}" \
            "ss -lunp 2>/dev/null | grep -q ':4433' && echo yes || echo no" 2>/dev/null || echo no)
        if [ "$control_up" != "yes" ]; then
            echo -e "${RED}❌ control plane (UDP 4433) is not listening after install${NC}"
            return 1
        fi
    else
        echo -e "${RED}❌ Install/health gate failed (rolled back)${NC}"
        return 1
    fi
}

PASS=0
FAIL=0

if [ -n "$TARGET_NODE" ]; then
    if [ -n "${NODES[$TARGET_NODE]:-}" ]; then
        TARGET_NAME="$TARGET_NODE"
    elif [ -n "${REGION_TO_NODE[$TARGET_NODE]:-}" ]; then
        TARGET_NAME="${REGION_TO_NODE[$TARGET_NODE]}"
    else
        echo -e "${RED}Unknown node: $TARGET_NODE${NC}"
        echo "Available: ${NODE_NAMES[*]}"
        exit 1
    fi
    deploy_node "$TARGET_NAME" "${NODES[$TARGET_NAME]}" && PASS=$((PASS+1)) || FAIL=$((FAIL+1))
else
    for name in "${NODE_NAMES[@]}"; do
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
