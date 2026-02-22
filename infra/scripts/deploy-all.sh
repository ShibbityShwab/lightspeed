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

TERRAFORM_DIR="${1:-../terraform}"
SSH_KEY="$TERRAFORM_DIR/lightspeed_deploy_key"
SSH_USER="opc"
SSH_OPTS="-o StrictHostKeyChecking=no -o ConnectTimeout=10 -o BatchMode=yes"

echo "⚡ LightSpeed Proxy — Rolling Deployment"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"

# Check SSH key exists
if [ ! -f "$SSH_KEY" ]; then
    echo "SSH key not found: $SSH_KEY"
    echo "Run 'terraform output -raw ssh_private_key > $SSH_KEY && chmod 600 $SSH_KEY'"
    exit 1
fi

# Get node IPs from Terraform
NODES=$(cd "$TERRAFORM_DIR" && terraform output -json proxy_nodes 2>/dev/null)

for region in $(echo "$NODES" | jq -r 'keys[]'); do
    node_id=$(echo "$NODES" | jq -r ".\"$region\".node_id")
    public_ip=$(echo "$NODES" | jq -r ".\"$region\".public_ip")
    health_url=$(echo "$NODES" | jq -r ".\"$region\".health_url")

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
bash "$(dirname "$0")/mesh-health.sh" "$TERRAFORM_DIR"
