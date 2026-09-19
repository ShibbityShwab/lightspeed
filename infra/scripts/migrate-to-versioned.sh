#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Migrate an existing relay to the versioned layout
#
# Older relays run an in-place binary (/usr/local/bin/lightspeed-proxy)
# under a Type=simple unit. This migrates them, idempotently, to:
#   * /opt/lightspeed/releases/<version>/lightspeed-proxy + `current`
#   * a Type=notify unit that execs /opt/lightspeed/current/lightspeed-proxy
#
# It seeds the currently-running binary as a `-legacy` release and points
# `current` at it, so a later install can roll back to the exact prior
# binary. It does NOT restart the service: the caller runs relay-install.sh
# (or the self-updater) to activate the new release with one health-gated
# restart. Already-migrated relays only get their unit refreshed.
#
# Usage (on the relay, as root):
#   bash migrate-to-versioned.sh
#
# Env seams:
#   LIGHTSPEED_LAYOUT_ROOT   (/opt/lightspeed)
#   LIGHTSPEED_SERVICE_NAME  (lightspeed-proxy)
#   LIGHTSPEED_SYSTEMCTL     (systemctl)
# ──────────────────────────────────────────────────────────────
set -euo pipefail

LAYOUT_ROOT="${LIGHTSPEED_LAYOUT_ROOT:-/opt/lightspeed}"
RELEASES_DIR="$LAYOUT_ROOT/releases"
CURRENT_LINK="$LAYOUT_ROOT/current"
SERVICE_NAME="${LIGHTSPEED_SERVICE_NAME:-lightspeed-proxy}"
UNIT_PATH="/etc/systemd/system/${SERVICE_NAME}.service"
OLD_BIN_DEFAULT="/usr/local/bin/lightspeed-proxy"
SYSTEMCTL="${LIGHTSPEED_SYSTEMCTL:-systemctl}"

log()  { printf '  migrate: %s\n' "$*"; }
fail() { printf 'migrate: ERROR: %s\n' "$*" >&2; }

# 1. Seed the layout from the running binary once (idempotent).
if [ -L "$CURRENT_LINK" ] && [ -d "$(readlink -f "$CURRENT_LINK")" ]; then
    log "already versioned: current -> $(readlink -f "$CURRENT_LINK")"
else
    OLD_BIN="$OLD_BIN_DEFAULT"
    if [ ! -x "$OLD_BIN" ]; then
        OLD_BIN="$("$SYSTEMCTL" show -p ExecStart --value "$SERVICE_NAME" 2>/dev/null \
            | sed -n 's/.*path=\([^ ;]*\).*/\1/p' | head -1)"
    fi
    [ -n "$OLD_BIN" ] && [ -x "$OLD_BIN" ] || { fail "cannot locate the running proxy binary"; exit 1; }

    OLD_VERSION="$("$OLD_BIN" --version 2>/dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -1 || true)"
    [ -n "$OLD_VERSION" ] || OLD_VERSION="legacy"
    LEGACY_DIR="$RELEASES_DIR/${OLD_VERSION}-legacy"

    log "seeding legacy release $OLD_VERSION from $OLD_BIN"
    install -d "$RELEASES_DIR" "$LEGACY_DIR"
    install -m 0755 "$OLD_BIN" "$LEGACY_DIR/lightspeed-proxy"
    ln -sfn "$LEGACY_DIR" "$CURRENT_LINK"
fi

# 2. Install the canonical Type=notify unit (back up any different one).
TMP_UNIT="$(mktemp)"
trap 'rm -f "$TMP_UNIT"' EXIT
cat > "$TMP_UNIT" <<'UNIT'
[Unit]
Description=LightSpeed Proxy - UDP game latency optimizer
After=network-online.target
Wants=network-online.target

[Service]
Type=notify
NotifyAccess=main
WatchdogSec=30
ExecStart=/opt/lightspeed/current/lightspeed-proxy --config /etc/lightspeed/proxy.toml --data-bind 0.0.0.0:4434 --control-bind 0.0.0.0:4433 --health-bind 0.0.0.0:8080
DynamicUser=true
StateDirectory=lightspeed
RuntimeDirectory=lightspeed
RuntimeDirectoryMode=0750
Environment=LIGHTSPEED_TLS_DIR=/var/lib/lightspeed/tls
Restart=on-failure
RestartSec=2
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadOnlyPaths=/etc/lightspeed
PrivateTmp=true
LimitNOFILE=65536

[Install]
WantedBy=multi-user.target
UNIT

if [ -f "$UNIT_PATH" ] && cmp -s "$TMP_UNIT" "$UNIT_PATH"; then
    log "systemd unit already current"
else
    if [ -f "$UNIT_PATH" ]; then
        BACKUP="$UNIT_PATH.pre-versioned.$(date +%Y%m%d%H%M%S)"
        cp -a "$UNIT_PATH" "$BACKUP"
        log "backed up old unit to $BACKUP"
    fi
    install -m 0644 "$TMP_UNIT" "$UNIT_PATH"
    log "installed canonical Type=notify unit"
fi

"$SYSTEMCTL" daemon-reload
"$SYSTEMCTL" enable "$SERVICE_NAME" >/dev/null 2>&1 || true

# 3. Install the self-updater when the caller staged its files (idempotent).
if [ -f /tmp/lightspeed-relay-updater.sh ] && [ -f /tmp/lightspeed-relay-install.sh ] \
   && [ -f /tmp/lightspeed-update.service ] && [ -f /tmp/lightspeed-update.timer ]; then
    install -d /usr/local/lib/lightspeed
    install -m 0755 /tmp/lightspeed-relay-install.sh /usr/local/lib/lightspeed/relay-install.sh
    install -m 0755 /tmp/lightspeed-relay-updater.sh /usr/local/lib/lightspeed/relay-updater.sh
    install -m 0644 /tmp/lightspeed-update.service /etc/systemd/system/lightspeed-update.service
    install -m 0644 /tmp/lightspeed-update.timer /etc/systemd/system/lightspeed-update.timer
    "$SYSTEMCTL" daemon-reload
    "$SYSTEMCTL" enable --now lightspeed-update.timer >/dev/null 2>&1 || true
    log "installed the self-update timer"
else
    log "self-updater files not staged in /tmp; skipping timer install"
fi

log "ready: run relay-install.sh (or the updater) to activate a release"
