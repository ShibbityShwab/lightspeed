#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Reusable Remote Relay Installer
#
# Installs a staged proxy binary into a versioned release layout,
# verifies it, repoints the `current` symlink atomically, restarts
# systemd, and health-gates the result. On failure it rolls back to
# the previous release and exits non-zero.
#
# Layout (per relay):
#   /opt/lightspeed/releases/<version>/lightspeed-proxy   (immutable)
#   /opt/lightspeed/current -> releases/<version>         (symlink)
#
# The systemd unit must run /opt/lightspeed/current/lightspeed-proxy,
# so activation is a single atomic symlink swap.
#
# Usage:
#   relay-install.sh --binary /tmp/lightspeed-proxy.staged --version <ver>
#
# Env seams (defaults in parentheses):
#   LIGHTSPEED_LAYOUT_ROOT      (/opt/lightspeed)
#   LIGHTSPEED_SERVICE_NAME     (lightspeed-proxy)
#   LIGHTSPEED_CONFIG_PATH      (/etc/lightspeed/proxy.toml)
#   LIGHTSPEED_KEEP_RELEASES    (3)
#   LIGHTSPEED_HEALTH_URL       (http://127.0.0.1:8080/health)
#   LIGHTSPEED_HEALTH_TIMEOUT   (15 seconds)
#   LIGHTSPEED_HEALTH_INTERVAL  (1 second; 0 disables the sleep)
#   LIGHTSPEED_SYSTEMCTL        (systemctl)
#   LIGHTSPEED_CURL             (curl)
#   LIGHTSPEED_FILE_BIN         (file)
#   LIGHTSPEED_HOST_ARCH        (uname -m)
#   LIGHTSPEED_SLEEP_BIN        (sleep)
#
# Exit codes: 0 when the new release is active and healthy, non-zero
# on any failure (with the previous release restored when possible).
# ──────────────────────────────────────────────────────────────
set -euo pipefail

BINARY_NAME="lightspeed-proxy"

LAYOUT_ROOT="${LIGHTSPEED_LAYOUT_ROOT:-/opt/lightspeed}"
RELEASES_DIR="$LAYOUT_ROOT/releases"
CURRENT_LINK="$LAYOUT_ROOT/current"
SERVICE_NAME="${LIGHTSPEED_SERVICE_NAME:-lightspeed-proxy}"
CONFIG_PATH="${LIGHTSPEED_CONFIG_PATH:-/etc/lightspeed/proxy.toml}"
KEEP_RELEASES="${LIGHTSPEED_KEEP_RELEASES:-3}"
HEALTH_URL="${LIGHTSPEED_HEALTH_URL:-http://127.0.0.1:8080/health}"
HEALTH_TIMEOUT="${LIGHTSPEED_HEALTH_TIMEOUT:-15}"
HEALTH_INTERVAL="${LIGHTSPEED_HEALTH_INTERVAL:-1}"
SYSTEMCTL="${LIGHTSPEED_SYSTEMCTL:-systemctl}"
CURL="${LIGHTSPEED_CURL:-curl}"
FILE_BIN="${LIGHTSPEED_FILE_BIN:-file}"
HOST_ARCH="${LIGHTSPEED_HOST_ARCH:-$(uname -m)}"
SLEEP_BIN="${LIGHTSPEED_SLEEP_BIN:-sleep}"

usage() {
    sed -n '2,40p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

log()  { printf '  relay-install: %s\n' "$*"; }
fail() { printf 'relay-install: ERROR: %s\n' "$*" >&2; }

# ── Arg parsing ──────────────────────────────────────────────
BINARY_PATH=""
VERSION=""
while [ $# -gt 0 ]; do
    case "$1" in
        --binary)  BINARY_PATH="${2:-}"; shift 2 ;;
        --version) VERSION="${2:-}"; shift 2 ;;
        -h|--help) usage; exit 0 ;;
        *) fail "unknown argument: $1"; usage >&2; exit 2 ;;
    esac
done

[ -n "$BINARY_PATH" ] || { fail "--binary is required"; usage >&2; exit 2; }
[ -n "$VERSION" ]     || { fail "--version is required"; usage >&2; exit 2; }

# A version is a single path-safe path component: no separators, no
# traversal, no whitespace or shell metacharacters.
case "$VERSION" in
    *[!A-Za-z0-9._-]*|.|..)
        fail "invalid version '$VERSION' (allowed: A-Za-z0-9._-)" >&2
        exit 2
        ;;
esac

RELEASE_DIR="$RELEASES_DIR/$VERSION"
RELEASE_BIN="$RELEASE_DIR/$BINARY_NAME"

# ── Verify the staged binary ─────────────────────────────────
# Executable, and (when `file` is available) an ELF matching host arch.
# Falls back to ELF magic-byte detection when `file` is missing.
verify_binary() {
    [ -f "$BINARY_PATH" ] || { fail "staged binary not found: $BINARY_PATH"; return 1; }
    [ -x "$BINARY_PATH" ] || { fail "staged binary is not executable: $BINARY_PATH"; return 1; }

    local desc=""
    if command -v "$FILE_BIN" >/dev/null 2>&1; then
        desc="$("$FILE_BIN" --brief "$BINARY_PATH" 2>/dev/null || true)"
    else
        desc="$("$FILE_BIN" "$BINARY_PATH" 2>/dev/null || true)"
    fi

    if [ -n "$desc" ]; then
        case "$desc" in
            *ELF*) ;;
            *) fail "not an ELF binary: $desc"; return 1 ;;
        esac
        local arch_pat
        case "$HOST_ARCH" in
            x86_64|amd64)  arch_pat='x86-64|x86_64' ;;
            aarch64|arm64) arch_pat='aarch64|ARM aarch64' ;;
            *)             arch_pat="$(printf '%s' "$HOST_ARCH" | sed 's/[][\\.^$*+?(){}|]/\\&/g')" ;;
        esac
        if ! printf '%s' "$desc" | grep -Eqi "$arch_pat"; then
            fail "binary arch does not match host ($HOST_ARCH): $desc"
            return 1
        fi
    else
        # No `file` binary: verify the ELF magic ourselves.
        local magic
        magic="$(od -An -tx1 -N4 "$BINARY_PATH" 2>/dev/null | tr -d ' \n' || true)"
        if [ "$magic" != "7f454c46" ]; then
            fail "not an ELF binary (magic=$magic)"
            return 1
        fi
        log "warning: '$FILE_BIN' unavailable; verified ELF magic, arch unchecked"
    fi
    return 0
}

# Current active release directory, or empty when unset/dangling.
active_release() {
    local target
    target="$(readlink -f "$CURRENT_LINK" 2>/dev/null || true)"
    if [ -n "$target" ] && [ -d "$target" ]; then
        printf '%s' "$target"
    fi
}

# Atomically repoint `current`; returns non-zero if it would dangle.
repoint() {
    ln -sfn "$1" "$CURRENT_LINK" || return 1
    [ -e "$CURRENT_LINK" ] || return 1
    return 0
}

# Poll the local /health endpoint until it reports healthy or we time out.
wait_healthy() {
    local deadline=$((SECONDS + HEALTH_TIMEOUT))
    while [ "$SECONDS" -lt "$deadline" ]; do
        local body
        body="$("$CURL" -sf --max-time 2 "$HEALTH_URL" 2>/dev/null || true)"
        if [ -n "$body" ] && \
           printf '%s' "$body" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"healthy"'; then
            return 0
        fi
        if [ "$HEALTH_INTERVAL" -gt 0 ]; then
            "$SLEEP_BIN" "$HEALTH_INTERVAL" || true
        fi
    done
    return 1
}

# Activate the new release: daemon-reload, repoint, restart, health-gate.
activate() {
    "$SYSTEMCTL" daemon-reload || return 1
    repoint "$RELEASE_DIR" || return 1
    "$SYSTEMCTL" restart "$SERVICE_NAME" || return 1
    wait_healthy
}

# Restore the previous release after a failed activation. Never leaves
# `current` dangling: it points at the previous release, or (with no
# previous release) the new release directory that still exists.
rollback() {
    if [ -n "$PREV_RELEASE" ] && [ -d "$PREV_RELEASE" ]; then
        log "rolling back to $PREV_RELEASE"
        repoint "$PREV_RELEASE" || fail "could not repoint current to previous release"
        "$SYSTEMCTL" restart "$SERVICE_NAME" >/dev/null 2>&1 || true
        if wait_healthy; then
            log "rollback healthy on previous release"
        else
            fail "service not healthy after rollback"
        fi
    else
        log "no previous release to roll back to; current left on $RELEASE_DIR"
    fi
    exit 1
}

# Keep the newest N releases; prune older ones only after a healthy
# activation. The active release and the previous release are never removed.
prune_releases() {
    case "$KEEP_RELEASES" in
        ''|*[!0-9]*) return 0 ;;
    esac
    [ "$KEEP_RELEASES" -gt 0 ] || return 0
    [ -d "$RELEASES_DIR" ] || return 0

    local active
    active="$(active_release)"
    local sorted
    sorted="$(find "$RELEASES_DIR" -mindepth 1 -maxdepth 1 -type d \
        -printf '%T@ %p\n' 2>/dev/null | sort -rn | awk '{print $2}')"

    local i=0 dir
    while IFS= read -r dir; do
        [ -n "$dir" ] || continue
        i=$((i + 1))
        [ "$i" -le "$KEEP_RELEASES" ] && continue
        [ "$dir" = "$active" ] && continue
        [ "$dir" = "$PREV_RELEASE" ] && continue
        rm -rf "$dir"
    done <<< "$sorted"
}

# ── Main flow ────────────────────────────────────────────────
PREV_RELEASE="$(active_release)"

printf 'relay-install: version=%s binary=%s\n' "$VERSION" "$BINARY_PATH"

# 1. Verify the staged binary before touching the layout.
verify_binary || exit 1

# 2. Create the immutable release directory and copy the binary in.
install -d "$RELEASES_DIR"
install -d "$RELEASE_DIR"
install -m 0755 "$BINARY_PATH" "$RELEASE_BIN"

# 3. Config gate: never activate a release whose --check fails.
if ! "$RELEASE_BIN" --check --config "$CONFIG_PATH"; then
    fail "--check failed for $RELEASE_DIR; aborting before activation"
    # Leave the active release untouched. Remove the fresh release dir only
    # when it is not what `current` points at (so current can never dangle).
    if [ "$RELEASE_DIR" != "$PREV_RELEASE" ] && [ "$RELEASE_DIR" != "$(active_release)" ]; then
        rm -rf "$RELEASE_DIR"
    fi
    exit 1
fi

# 4. Activate, health-gate, and roll back on failure.
if activate; then
    prune_releases
    log "activated $VERSION (current -> $RELEASE_DIR)"
    exit 0
fi

rollback
