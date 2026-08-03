#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Deploy/Update All Proxy Nodes
#
# Connects to each proxy node via SSH and runs the deploy script
# to pull the latest Docker image and restart the container.
#
# Usage: ./deploy-all.sh [terraform-dir]
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SSH_KEY="${DEPLOY_SSH_KEY:-$HOME/.ssh/lightspeed_deploy}"
SSH_USER="${DEPLOY_SSH_USER:-root}"
SSH_OPTS="-o StrictHostKeyChecking=no -o ConnectTimeout=10 -o BatchMode=yes"

echo "⚡ LightSpeed Proxy — Rolling Deployment"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Check SSH key exists
if [ ! -f "$SSH_KEY" ]; then
    echo "SSH key not found: $SSH_KEY"
    echo "Set DEPLOY_SSH_KEY or place key at ~/.ssh/lightspeed_deploy"
    exit 1
fi

# Get node IPs from environment or BUILTIN_NODES
NODES_JSON="${LIGHTSPEED_NODES:-{}}"
if [ "$NODES_JSON" = "{}" ]; then
    echo "No nodes configured. Set LIGHTSPEED_NODES with a JSON object of node IPs."
    echo 'Example: export LIGHTSPEED_NODES='\''{"proxy-lax":{"ip":"1.2.3.4","health_url":"http://1.2.3.4:8080/health"}}'\''
    exit 1
fi

for region in $(echo "$NODES_JSON" | jq -r 'keys[]'); do
    node_id=$(echo "$NODES_JSON" | jq -r ".\"$region\".node_id // \"$region\"")
    public_ip=$(echo "$NODES_JSON" | jq -r ".\"$region\".ip // .\"$region\"")
    health_url=$(echo "$NODES_JSON" | jq -r ".\"$region\".health_url // \"http://\${public_ip}:8080/health\"")

    echo ""
    echo "── Deploying $node_id ($region) @ $public_ip ──"

    # SSH and run deploy script
    ssh $SSH_OPTS -i "$SSH_KEY" "$SSH_USER@$public_ip" \
        "sudo bash /etc/lightspeed/deploy.sh" && deploy_ok=true || deploy_ok=false

    if $deploy_ok; then
        # Verify health
        sleep 3
        if curl -sf --max-time 10 "$health_url" > /dev/null 2>&1; then
            echo "✅ $node_id deployed and healthy"
        else
            echo "⚠️  $node_id deployed but health check failed"
        fi
    else
        echo "❌ $node_id deployment failed"
    fi
done

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "Deployment complete. Running mesh health check..."
echo ""
bash "$(dirname "$0")/mesh-health.sh"
