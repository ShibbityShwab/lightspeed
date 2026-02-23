#!/bin/bash
# ──────────────────────────────────────────────────────────────
# LightSpeed Proxy — Deployment Script
# Deploys the proxy binary directly (no Docker) for minimal RAM usage.
# Supports both fresh install and in-place upgrade.
# ──────────────────────────────────────────────────────────────
set -euo pipefail

BINARY="/usr/local/bin/lightspeed-proxy"
SERVICE="lightspeed-proxy"
CONFIG="/etc/lightspeed/proxy.toml"
GHCR_IMAGE="ghcr.io/shibbityshwab/lightspeed-proxy:latest"

echo "⚡ LightSpeed Proxy deployment starting..."

# ── Strategy: Extract binary from GHCR image (if Docker/podman available) ──
# Otherwise, expect binary to be pre-deployed via SCP/ansible.
extract_binary() {
    local runtime=""
    if command -v docker &>/dev/null; then
        runtime="docker"
    elif command -v podman &>/dev/null; then
        runtime="podman"
    else
        echo "INFO: No container runtime found. Skipping image pull."
        echo "      Deploy binary manually: scp lightspeed-proxy-linux-amd64 host:$BINARY"
        return 1
    fi

    echo "Pulling latest image via $runtime..."
    if $runtime pull "$GHCR_IMAGE"; then
        local cid
        cid=$($runtime create "$GHCR_IMAGE" 2>/dev/null)
        $runtime cp "$cid:/usr/local/bin/lightspeed-proxy" /tmp/lightspeed-proxy-new
        $runtime rm "$cid" >/dev/null 2>&1
        return 0
    else
        echo "WARNING: Could not pull image."
        return 1
    fi
}

# ── Deploy binary ──
if extract_binary; then
    echo "Installing new binary..."
    chmod +x /tmp/lightspeed-proxy-new
    # SELinux context for Oracle Linux / RHEL
    chcon -t bin_t /tmp/lightspeed-proxy-new 2>/dev/null || true
    mv /tmp/lightspeed-proxy-new "$BINARY"
    chown root:root "$BINARY"
    restorecon "$BINARY" 2>/dev/null || true
elif [ ! -f "$BINARY" ]; then
    echo "ERROR: No binary at $BINARY and cannot pull from GHCR."
    echo "       Deploy manually: scp lightspeed-proxy-linux-amd64 host:$BINARY"
    exit 1
else
    echo "Using existing binary at $BINARY"
fi

# ── Verify config exists ──
if [ ! -f "$CONFIG" ]; then
    echo "ERROR: Config not found at $CONFIG"
    exit 1
fi

# ── Restart service ──
echo "Reloading systemd and restarting proxy..."
systemctl daemon-reload
systemctl enable --now "$SERVICE"

# ── Wait for health check ──
echo "Waiting for proxy to become healthy..."
for i in $(seq 1 30); do
    if curl -sf http://localhost:8080/health > /dev/null 2>&1; then
        echo "✅ Proxy is healthy!"
        systemctl status "$SERVICE" --no-pager --lines=5
        exit 0
    fi
    sleep 1
done

echo "⚠️  Proxy did not become healthy within 30s"
journalctl -u "$SERVICE" --no-pager -n 20
exit 1
