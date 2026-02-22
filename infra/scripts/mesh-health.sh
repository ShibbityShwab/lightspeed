#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Proxy Mesh Health Check
#
# Checks all proxy nodes and reports status.
# Usage: ./mesh-health.sh [terraform-dir]
#
# Requires: curl, jq, terraform (or manual node list)
# ──────────────────────────────────────────────────────────────
set -euo pipefail

TERRAFORM_DIR="${1:-../terraform}"
TIMEOUT=5
PASS=0
FAIL=0
TOTAL=0

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m'

echo "⚡ LightSpeed Proxy Mesh Health Check"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Try to get nodes from Terraform output
if command -v terraform &>/dev/null && [ -d "$TERRAFORM_DIR" ]; then
    NODES=$(cd "$TERRAFORM_DIR" && terraform output -json proxy_nodes 2>/dev/null || echo "{}")
else
    # Fallback: read from environment or manual config
    if [ -z "${LIGHTSPEED_NODES:-}" ]; then
        echo "No Terraform state or LIGHTSPEED_NODES env var found."
        echo "Set LIGHTSPEED_NODES as JSON or provide terraform directory."
        echo ""
        echo "Example:"
        echo '  export LIGHTSPEED_NODES='"'"'{"us-east":{"health_url":"http://1.2.3.4:8080/health","node_id":"proxy-us-east","region":"us-ashburn-1"}}'
        exit 1
    fi
    NODES="$LIGHTSPEED_NODES"
fi

# Parse and check each node
for region in $(echo "$NODES" | jq -r 'keys[]'); do
    TOTAL=$((TOTAL + 1))
    node_id=$(echo "$NODES" | jq -r ".\"$region\".node_id")
    health_url=$(echo "$NODES" | jq -r ".\"$region\".health_url")
    oci_region=$(echo "$NODES" | jq -r ".\"$region\".region // .\"$region\".oci_region // \"unknown\"")

    printf "%-12s %-20s " "$region" "$node_id"

    # HTTP health check
    response=$(curl -sf --max-time "$TIMEOUT" "$health_url" 2>/dev/null) && status=0 || status=1

    if [ $status -eq 0 ]; then
        # Parse health response
        h_status=$(echo "$response" | jq -r '.status // "unknown"')
        h_conns=$(echo "$response" | jq -r '.active_connections // "?"')
        h_uptime=$(echo "$response" | jq -r '.uptime_secs // "?"')

        if [ "$h_status" = "healthy" ]; then
            printf "${GREEN}✅ HEALTHY${NC}  conns=%-4s uptime=%ss\n" "$h_conns" "$h_uptime"
            PASS=$((PASS + 1))
        else
            printf "${YELLOW}⚠️  DEGRADED${NC} status=%s\n" "$h_status"
            FAIL=$((FAIL + 1))
        fi
    else
        printf "${RED}❌ DOWN${NC}     (no response within ${TIMEOUT}s)\n"
        FAIL=$((FAIL + 1))
    fi
done

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Total: $TOTAL  Healthy: $PASS  Failed: $FAIL"

if [ $FAIL -gt 0 ]; then
    echo -e "${RED}⚠️  Some nodes are unhealthy!${NC}"
    exit 1
else
    echo -e "${GREEN}✅ All nodes healthy${NC}"
    exit 0
fi
