#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — Deployment Script
# Runs on the proxy node to start/update the proxy container.
# ──────────────────────────────────────────────────────────────
set -euo pipefail

IMAGE="ghcr.io/shibbityshwab/lightspeed-proxy:latest"
CONTAINER="lightspeed-proxy"
CONFIG="/etc/lightspeed/proxy.toml"

echo "⚡ LightSpeed Proxy deployment starting..."

# Pull latest image
echo "Pulling latest image..."
docker pull "$IMAGE" || {
    echo "WARNING: Could not pull image. Using local image if available."
}

# Stop existing container (if running)
if docker ps -q --filter "name=$CONTAINER" | grep -q .; then
    echo "Stopping existing container..."
    docker stop "$CONTAINER" 2>/dev/null || true
    docker rm "$CONTAINER" 2>/dev/null || true
fi

# Remove old container (if exists but stopped)
docker rm "$CONTAINER" 2>/dev/null || true

# Start new container
echo "Starting proxy container..."
docker run -d \
    --name "$CONTAINER" \
    --network host \
    --restart unless-stopped \
    --memory 4g \
    --cpus 0.9 \
    --log-driver json-file \
    --log-opt max-size=50m \
    --log-opt max-file=3 \
    -v "$CONFIG":/etc/lightspeed/proxy.toml:ro \
    "$IMAGE" \
        --config /etc/lightspeed/proxy.toml \
        --data-bind 0.0.0.0:4434 \
        --control-bind 0.0.0.0:4433 \
        --health-bind 0.0.0.0:8080

# Wait for health check
echo "Waiting for proxy to become healthy..."
for i in $(seq 1 30); do
    if curl -sf http://localhost:8080/health > /dev/null 2>&1; then
        echo "✅ Proxy is healthy!"
        docker logs --tail 5 "$CONTAINER"
        exit 0
    fi
    sleep 1
done

echo "⚠️  Proxy did not become healthy within 30s"
echo "Container logs:"
docker logs --tail 20 "$CONTAINER"
exit 1
