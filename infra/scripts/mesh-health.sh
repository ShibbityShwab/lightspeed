#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Proxy Mesh Health Check
#
# Checks all proxy nodes via /health (JSON) and /metrics (Prometheus).
# Usage: ./mesh-health.sh [--metrics] [--json] [terraform-dir]
#
# Options:
#   --metrics   Also fetch and display key Prometheus metrics
#   --json      Output machine-readable JSON summary
#
# Requires: curl, jq, terraform (or manual node list)
# ──────────────────────────────────────────────────────────────
set -euo pipefail

TERRAFORM_DIR="${TERRAFORM_DIR:-../terraform}"
TIMEOUT=5
PASS=0
FAIL=0
TOTAL=0
SHOW_METRICS=false
JSON_OUTPUT=false

# Colors
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# Parse flags
for arg in "$@"; do
    case "$arg" in
        --metrics) SHOW_METRICS=true ;;
        --json)    JSON_OUTPUT=true ;;
        *)         TERRAFORM_DIR="$arg" ;;
    esac
done

# ── Built-in node list (Vultr mesh) ─────────────────────────
# Fallback when no Terraform or env var is available.
BUILTIN_NODES='{
  "us-west-lax": {
    "health_url": "http://149.28.84.139:8080/health",
    "metrics_url": "http://149.28.84.139:8080/metrics",
    "node_id": "proxy-lax",
    "region": "us-west-lax"
  },
  "asia-sgp": {
    "health_url": "http://149.28.144.74:8080/health",
    "metrics_url": "http://149.28.144.74:8080/metrics",
    "node_id": "relay-sgp",
    "region": "asia-sgp"
  }
}'

# ── Resolve node list ───────────────────────────────────────
if [ -n "${LIGHTSPEED_NODES:-}" ]; then
    NODES="$LIGHTSPEED_NODES"
elif command -v terraform &>/dev/null && [ -d "$TERRAFORM_DIR" ]; then
    NODES=$(cd "$TERRAFORM_DIR" && terraform output -json proxy_nodes 2>/dev/null || echo "")
    [ -z "$NODES" ] || [ "$NODES" = "{}" ] && NODES="$BUILTIN_NODES"
else
    NODES="$BUILTIN_NODES"
fi

if [ "$JSON_OUTPUT" = false ]; then
    echo "⚡ LightSpeed Proxy Mesh Health Check"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
fi

JSON_RESULTS="[]"

# ── Check each node ─────────────────────────────────────────
for region in $(echo "$NODES" | jq -r 'keys[]'); do
    TOTAL=$((TOTAL + 1))
    node_id=$(echo "$NODES" | jq -r ".\"$region\".node_id")
    health_url=$(echo "$NODES" | jq -r ".\"$region\".health_url")
    metrics_url=$(echo "$NODES" | jq -r ".\"$region\".metrics_url // empty")

    if [ "$JSON_OUTPUT" = false ]; then
        printf "%-12s %-20s " "$region" "$node_id"
    fi

    # HTTP health check
    response=$(curl -sf --max-time "$TIMEOUT" "$health_url" 2>/dev/null) && status=0 || status=1

    if [ $status -eq 0 ]; then
        h_status=$(echo "$response" | jq -r '.status // "unknown"')
        h_conns=$(echo "$response" | jq -r '.active_connections // "?"')
        h_uptime=$(echo "$response" | jq -r '.uptime_secs // "?"')
        h_pkts=$(echo "$response" | jq -r '.packets_relayed // "?"')
        h_bytes=$(echo "$response" | jq -r '.bytes_relayed // "?"')
        h_fec=$(echo "$response" | jq -r '.fec_recoveries // "0"')
        h_version=$(echo "$response" | jq -r '.version // "?"')

        if [ "$h_status" = "healthy" ]; then
            if [ "$JSON_OUTPUT" = false ]; then
                printf "${GREEN}✅ HEALTHY${NC}  v%-6s conns=%-4s uptime=%ss  pkts=%s  fec_recovered=%s\n" \
                    "$h_version" "$h_conns" "$h_uptime" "$h_pkts" "$h_fec"
            fi
            PASS=$((PASS + 1))
            node_state="healthy"
        else
            if [ "$JSON_OUTPUT" = false ]; then
                printf "${YELLOW}⚠️  DEGRADED${NC} status=%s\n" "$h_status"
            fi
            FAIL=$((FAIL + 1))
            node_state="degraded"
        fi

        # Prometheus metrics (optional)
        if [ "$SHOW_METRICS" = true ] && [ -n "$metrics_url" ] && [ "$JSON_OUTPUT" = false ]; then
            metrics_data=$(curl -sf --max-time "$TIMEOUT" "$metrics_url" 2>/dev/null || echo "")
            if [ -n "$metrics_data" ]; then
                echo -e "             ${CYAN}Prometheus metrics:${NC}"
                echo "$metrics_data" | grep -E "^lightspeed_(packets_relayed|bytes_relayed|active_connections|relay_latency_avg|fec_recoveries|auth_rejections|abuse_blocks|sessions_created)" \
                    | sed 's/^/               /'
            fi
        fi

        # Build JSON result
        JSON_RESULTS=$(echo "$JSON_RESULTS" | jq --arg r "$region" --arg n "$node_id" --arg s "$node_state" \
            --arg c "$h_conns" --arg u "$h_uptime" --arg p "$h_pkts" --arg v "$h_version" \
            '. + [{"region": $r, "node_id": $n, "status": $s, "connections": ($c|tonumber // 0), "uptime_secs": ($u|tonumber // 0), "packets_relayed": ($p|tonumber // 0), "version": $v}]')
    else
        if [ "$JSON_OUTPUT" = false ]; then
            printf "${RED}❌ DOWN${NC}     (no response within ${TIMEOUT}s)\n"
        fi
        FAIL=$((FAIL + 1))
        JSON_RESULTS=$(echo "$JSON_RESULTS" | jq --arg r "$region" --arg n "$node_id" \
            '. + [{"region": $r, "node_id": $n, "status": "down", "connections": 0, "uptime_secs": 0, "packets_relayed": 0, "version": "unknown"}]')
    fi
done

# ── Summary ─────────────────────────────────────────────────
if [ "$JSON_OUTPUT" = true ]; then
    echo "$JSON_RESULTS" | jq --argjson total "$TOTAL" --argjson pass "$PASS" --argjson fail "$FAIL" \
        '{total: $total, healthy: $pass, failed: $fail, nodes: .}'
else
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    echo "Total: $TOTAL  Healthy: $PASS  Failed: $FAIL"

    if [ $FAIL -gt 0 ]; then
        echo -e "${RED}⚠️  Some nodes are unhealthy!${NC}"
        exit 1
    else
        echo -e "${GREEN}✅ All nodes healthy${NC}"
        exit 0
    fi
fi
