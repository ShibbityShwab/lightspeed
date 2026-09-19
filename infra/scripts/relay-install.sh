#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Reusable Remote Relay Installer
#
# Installs a staged proxy binary into a versioned release layout,
# verifies it, then activates it either by in-place handoff (SIGUSR2
# on a handoff-capable proxy) or by repointing `current` and
# restarting systemd. Health-gates the result and rolls back on
# failure.
#
# Layout (per relay):
#   /opt/lightspeed/releases/<version>/lightspeed-proxy   (immutable)
#   /opt/lightspeed/current -> releases/<version>         (symlink)
#
# The systemd unit runs /opt/lightspeed/current/lightspeed-proxy, so
# a restart activation is a single atomic symlink swap. A handoff
# leaves `current` untouched until the new process is healthy, then
# repoints it for the next restart.
#
# Usage:
#   relay-install.sh --binary /tmp/lightspeed-proxy.staged --version <ver>
#                    [--handoff | --no-handoff] [--force]
#                    [--public-ip <ipv4>] [--updater <path>]
#
# A same-version install is a no-op: when `current` already points at a
# release whose label equals <ver>, the installer logs and exits 0 without
# touching the layout. Pass --force to reinstall anyway.
#
# Activation mode (default: auto):
#   auto         in-place handoff when /health reports handoff.supported=true,
#                the new binary's --handoff-schema matches the running schema,
#                and the target version differs; otherwise repoint + restart
#   --handoff    require the handoff path; error when it is not possible
#   --no-handoff force the repoint + restart path
#   --public-ip  ensure [server] public_ip matches this IPv4 in the config
#   --updater    install the given relay-updater.sh to LIGHTSPEED_UPDATER_DEST
#
# Env seams (defaults in parentheses):
#   LIGHTSPEED_LAYOUT_ROOT       (/opt/lightspeed)
#   LIGHTSPEED_SERVICE_NAME      (lightspeed-proxy)
#   LIGHTSPEED_CONFIG_PATH       (/etc/lightspeed/proxy.toml)
#   LIGHTSPEED_UPDATER_DEST      (/usr/local/lib/lightspeed/relay-updater.sh)
#   LIGHTSPEED_KEEP_RELEASES     (3)
#   LIGHTSPEED_HEALTH_URL        (http://127.0.0.1:8080/health)
#   LIGHTSPEED_HEALTH_TIMEOUT    (15 seconds)
#   LIGHTSPEED_HEALTH_INTERVAL   (1 second; 0 disables the sleep)
#   LIGHTSPEED_HANDOFF_REQUEST   (/run/lightspeed/handoff-request.json)
#   LIGHTSPEED_HANDOFF_SOAK_SECS (5 seconds)
#   LIGHTSPEED_SYSTEMCTL         (systemctl)
#   LIGHTSPEED_CURL              (curl)
#   LIGHTSPEED_FILE_BIN          (file)
#   LIGHTSPEED_SHA256SUM         (sha256sum)
#   LIGHTSPEED_HOST_ARCH         (uname -m)
#   LIGHTSPEED_SLEEP_BIN         (sleep)
#
# Exit codes: 0 when the new release is active and healthy, non-zero
# on any failure (with the previous release restored when possible).
# ──────────────────────────────────────────────────────────────
set -euo pipefail

# allow: SIZE_OK - deployed as a single file that setup/provision sync on
# every relay; splitting the handoff logic into a sourced library would force
# a multi-file deploy and break the self-updater's single-file model.

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
HANDOFF_REQUEST="${LIGHTSPEED_HANDOFF_REQUEST:-/run/lightspeed/handoff-request.json}"
HANDOFF_SOAK_SECS="${LIGHTSPEED_HANDOFF_SOAK_SECS:-5}"
SYSTEMCTL="${LIGHTSPEED_SYSTEMCTL:-systemctl}"
CURL="${LIGHTSPEED_CURL:-curl}"
FILE_BIN="${LIGHTSPEED_FILE_BIN:-file}"
SHA256SUM="${LIGHTSPEED_SHA256SUM:-sha256sum}"
HOST_ARCH="${LIGHTSPEED_HOST_ARCH:-$(uname -m)}"
SLEEP_BIN="${LIGHTSPEED_SLEEP_BIN:-sleep}"

# run_handoff return codes.
HANDOFF_RC_NOOP=3      # proxy never switched; current left on the old release
HANDOFF_RC_ROLLBACK=4  # proxy switched but did not stay healthy; roll back

usage() {
    awk 'NR == 1 { next } /^set -euo pipefail/ { exit } { sub(/^# ?/, ""); print }' \
        "${BASH_SOURCE[0]}"
}

log()  { printf '  relay-install: %s\n' "$*"; }
fail() { printf 'relay-install: ERROR: %s\n' "$*" >&2; }

# ── Node-local config: exact self-tunnel filtering ───────────
# Without public_ip the proxy can only guess a relay-to-self session from the
# control port. Setting it lets the proxy match the node's own address exactly.
# Non-fatal when the config is absent; fatal only on a malformed address.
ensure_public_ip() {
    local ip="$1" cfg="$CONFIG_PATH"
    [ -n "$ip" ] || return 0
    [ -f "$cfg" ] || { log "no config at $cfg; skipping public_ip"; return 0; }

    local o1 o2 o3 o4 o
    IFS=. read -r o1 o2 o3 o4 <<< "$ip"
    for o in "$o1" "$o2" "$o3" "$o4"; do
        case "$o" in ''|*[!0-9]*) fail "invalid --public-ip: $ip"; return 1 ;; esac
        [ "$o" -le 255 ] || { fail "invalid --public-ip: $ip"; return 1; }
    done

    if grep -qE '^[[:space:]]*public_ip[[:space:]]*=' "$cfg"; then
        if grep -qE "^[[:space:]]*public_ip[[:space:]]*=[[:space:]]*\"?${ip}\"?[[:space:]]*$" "$cfg"; then
            return 0
        fi
        sed -i -E "s|^[[:space:]]*public_ip[[:space:]]*=.*|public_ip = \"${ip}\"|" "$cfg" \
            || { fail "cannot update public_ip in $cfg"; return 1; }
    elif grep -qE '^\[server\]' "$cfg"; then
        sed -i -E "0,/^\[server\]/s//[server]\npublic_ip = \"${ip}\"/" "$cfg" \
            || { fail "cannot add public_ip to $cfg"; return 1; }
    else
        printf '[server]\npublic_ip = "%s"\n\n' "$ip" | cat - "$cfg" > "$cfg.tmp.$$" \
            && mv "$cfg.tmp.$$" "$cfg" \
            || { fail "cannot add [server] to $cfg"; return 1; }
    fi
    log "config public_ip set to $ip"
    return 0
}

# ── Self-updater refresh ─────────────────────────────────────
# The updater is a standalone script that the deploy pipeline otherwise never
# ships, so a new copy is installed here on every deploy. Best-effort.
install_updater() {
    local src="$1"
    [ -n "$src" ] || return 0
    [ -f "$src" ] || { log "warning: updater not found: $src"; return 0; }
    if ! bash -n "$src" 2>/dev/null; then
        log "warning: updater failed syntax check: $src"
        return 0
    fi
    if install -d -m 0755 "$(dirname "$UPDATER_DEST")" \
        && install -m 0755 "$src" "$UPDATER_DEST"; then
        log "installed updater to $UPDATER_DEST"
    else
        log "warning: cannot install updater to $UPDATER_DEST"
    fi
    return 0
}

# ── Arg parsing ──────────────────────────────────────────────
BINARY_PATH=""
VERSION=""
HANDOFF_MODE="auto"
FORCE=0
PUBLIC_IP=""
UPDATER_PATH=""
UPDATER_DEST="${LIGHTSPEED_UPDATER_DEST:-/usr/local/lib/lightspeed/relay-updater.sh}"
while [ $# -gt 0 ]; do
    case "$1" in
        --binary)     BINARY_PATH="${2:-}"; shift 2 ;;
        --version)    VERSION="${2:-}"; shift 2 ;;
        --handoff)    HANDOFF_MODE="handoff"; shift ;;
        --no-handoff) HANDOFF_MODE="restart"; shift ;;
        --force)      FORCE=1; shift ;;
        --public-ip)  PUBLIC_IP="${2:-}"; shift 2 ;;
        --updater)    UPDATER_PATH="${2:-}"; shift 2 ;;
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

# The staged binary is written under a temp name and only renamed over the
# release binary after --check passes, so a failed reinstall of the active
# version can never truncate or corrupt the live binary.
STAGED_BIN=""
cleanup_staged() {
    if [ -n "$STAGED_BIN" ]; then
        rm -f "$STAGED_BIN"
    fi
}
trap cleanup_staged EXIT

# ── Verify the staged binary ─────────────────────────────────
# Executable, and (when `file` is available) an ELF matching host arch.
# Falls back to ELF magic-byte detection when `file` is missing.
verify_binary() {
    [ -f "$BINARY_PATH" ] || { fail "staged binary not found: $BINARY_PATH"; return 1; }
    [ -x "$BINARY_PATH" ] || { fail "staged binary is not executable: $BINARY_PATH"; return 1; }

    local desc=""
    if command -v "$FILE_BIN" >/dev/null 2>&1; then
        desc="$("$FILE_BIN" --brief "$BINARY_PATH" 2>/dev/null || true)"
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

# Active release's version label (its directory name), or empty when there is
# no active `current` (first install or a legacy layout).
running_version() {
    basename "$(active_release)"
}

# Atomically repoint `current`; returns non-zero if the target is missing.
# The new symlink is built beside the old one and renamed over it, so a
# reader never sees `current` absent or dangling mid-swap.
repoint() {
    [ -e "$1" ] || return 1
    local tmp="$CURRENT_LINK.tmp.$$"
    ln -sfn "$1" "$tmp" || { rm -f "$tmp"; return 1; }
    if ! mv -Tf "$tmp" "$CURRENT_LINK"; then
        rm -f "$tmp"
        return 1
    fi
    [ -e "$CURRENT_LINK" ] || return 1
    return 0
}

# ── /health helpers ──────────────────────────────────────────
# One bounded health fetch. Never fails the caller: an unreachable or
# unhealthy endpoint yields an empty string.
fetch_health() {
    "$CURL" -sf --max-time 2 "$HEALTH_URL" 2>/dev/null || true
}

# Extract the top-level "handoff" JSON value. serde emits it last, so the
# slice runs to the end of the body. Empty when /health has no handoff key.
health_handoff_segment() {
    local body="$1" marker='"handoff":'
    case "$body" in
        *"$marker"*) printf '%s' "${body#*"$marker"}" ;;
        *) return 0 ;;
    esac
}

# String field value from stdin (`"key":"value"` -> value). Empty on no match.
json_string_field() {
    grep -oE "\"$1\":\"[^\"]*\"" | head -1 | cut -d'"' -f4 || true
}

# Numeric field value from stdin (`"key":123` -> 123). Empty on no match.
json_number_field() {
    grep -oE "\"$1\":[0-9]+" | head -1 | cut -d: -f2 || true
}

health_version()         { printf '%s' "$1" | json_string_field version; }
health_status_healthy()  { printf '%s' "$1" | grep -Eq '"status"[[:space:]]*:[[:space:]]*"healthy"'; }

health_handoff_supported() {
    local seg; seg="$(health_handoff_segment "$1")"
    [ -n "$seg" ] || return 0
    printf '%s' "$seg" | grep -oE '"supported":(true|false)' | head -1 | cut -d: -f2 || true
}
health_handoff_schema() {
    local seg; seg="$(health_handoff_segment "$1")"
    [ -n "$seg" ] || return 0
    printf '%s' "$seg" | json_number_field schema_version
}
health_handoff_result() {
    local seg; seg="$(health_handoff_segment "$1")"
    [ -n "$seg" ] || return 0
    printf '%s' "$seg" | json_string_field result
}
health_handoff_id() {
    local seg; seg="$(health_handoff_segment "$1")"
    [ -n "$seg" ] || return 0
    printf '%s' "$seg" | json_string_field handoff_id
}
health_handoff_to_version() {
    local seg; seg="$(health_handoff_segment "$1")"
    [ -n "$seg" ] || return 0
    printf '%s' "$seg" | json_string_field to_version
}

# Poll the local /health endpoint until it reports healthy or we time out.
wait_healthy() {
    local deadline=$((SECONDS + HEALTH_TIMEOUT))
    while [ "$SECONDS" -lt "$deadline" ]; do
        local body
        body="$(fetch_health)"
        if [ -n "$body" ] && health_status_healthy "$body"; then
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

# ── In-place handoff ─────────────────────────────────────────
# A version is a single path-safe component, but `LAYOUT_ROOT` comes from the
# environment, so escape the two bytes that would break the JSON document.
json_escape() {
    printf '%s' "$1" | sed -e 's/\\/\\\\/g' -e 's/"/\\"/g'
}

now_unix_ms() { printf '%s000' "$(date +%s)"; }

new_handoff_id() { printf 'install-%s-%s-%s' "$(date +%s)" "$$" "$RANDOM"; }

# sha256 of the released binary, lowercase; empty when it cannot be hashed.
sha256_of() {
    "$SHA256SUM" "$1" 2>/dev/null | awk '{print $1}' || true
}

# The new binary's manifest schema, or empty when it does not support handoff.
read_new_schema() {
    local schema
    schema="$("$RELEASE_BIN" --handoff-schema 2>/dev/null | head -1 || true)"
    case "$schema" in
        ''|*[!0-9]*) return 0 ;;
    esac
    printf '%s' "$schema"
}

# Does the /health body describe *this* attempt's successful handoff?
#
# The committed proxy records the manifest's own handoff id for a success, not
# the request id, so match on the request id when present and otherwise anchor
# on `to_version` (which only this attempt targets).
handoff_matches_attempt() {  # body id
    local body="$1" id="$2" res hid hto
    res="$(health_handoff_result "$body")"
    [ "$res" = "ok" ] || return 1
    hid="$(health_handoff_id "$body")"
    if [ -n "$id" ] && [ "$hid" = "$id" ]; then
        return 0
    fi
    hto="$(health_handoff_to_version "$body")"
    if [ -n "$hto" ] && [ "$hto" = "$VERSION" ]; then
        return 0
    fi
    return 1
}

# Decide whether to use the handoff path. 0 = yes, 1 = use restart,
# 2 = an explicit --handoff that cannot be satisfied (caller exits 2).
use_handoff() {
    [ "$HANDOFF_MODE" = "restart" ] && return 1

    local body supported new_schema running_schema running_version
    body="$(fetch_health)"
    supported="$(health_handoff_supported "$body")"
    running_version="$(health_version "$body")"
    new_schema="$(read_new_schema)"
    running_schema="$(health_handoff_schema "$body")"
    # /health does not expose the running schema directly; when it cannot be
    # read, require the new schema to be the baseline "1".
    [ -n "$running_schema" ] || running_schema="1"

    if [ "$HANDOFF_MODE" = "handoff" ]; then
        if [ -z "$new_schema" ]; then
            fail "--handoff requested but the new binary reports no handoff schema"
            return 2
        fi
        if [ "$new_schema" != "$running_schema" ]; then
            fail "--handoff schema mismatch (running=$running_schema new=$new_schema)"
            return 2
        fi
        if [ "$supported" != "true" ]; then
            fail "--handoff requested but the running proxy does not advertise handoff support"
            return 2
        fi
        if [ -n "$running_version" ] && [ "$running_version" = "$VERSION" ]; then
            fail "--handoff requested but $VERSION is already running"
            return 2
        fi
        return 0
    fi

    # auto: hand off only when the proxy supports it, the schemas match, and
    # the target version differs (the proxy refuses a same-version request).
    [ "$supported" = "true" ] || return 1
    [ -n "$new_schema" ] || return 1
    [ "$new_schema" = "$running_schema" ] || return 1
    [ -n "$running_version" ] || return 1
    [ "$running_version" != "$VERSION" ] || return 1
    return 0
}

# Write the root-owned handoff request atomically (mode 0600). The release
# binary is already published, so its sha256 names exactly what will exec.
write_handoff_request() {  # id sha
    local id="$1" sha="$2" dir tmp json
    dir="$(dirname "$HANDOFF_REQUEST")"
    if [ ! -d "$dir" ]; then
        install -d -m 0750 "$dir" 2>/dev/null \
            || mkdir -p "$dir" 2>/dev/null \
            || { fail "cannot create handoff request directory: $dir"; return 1; }
    fi

    tmp="$HANDOFF_REQUEST.tmp.$$"
    json="$(printf '{"schema_version":1,"handoff_id":"%s","version":"%s","binary_path":"%s","sha256":"%s","requested_at_unix_ms":%s}' \
        "$(json_escape "$id")" "$(json_escape "$VERSION")" \
        "$(json_escape "$RELEASE_BIN")" "$(json_escape "$sha")" "$(now_unix_ms)")"
    if ! ( umask 077; printf '%s\n' "$json" > "$tmp" ); then
        fail "cannot write handoff request: $tmp"
        rm -f "$tmp"
        return 1
    fi
    if ! mv -f "$tmp" "$HANDOFF_REQUEST"; then
        fail "cannot publish handoff request: $HANDOFF_REQUEST"
        rm -f "$tmp"
        return 1
    fi
    # The proxy unit runs under DynamicUser, so the running process cannot read
    # a root-owned 0600 file. Give the request the runtime directory's owner
    # (the proxy user) while keeping it owner-only.
    chown --reference="$dir" "$HANDOFF_REQUEST" 2>/dev/null || true
    return 0
}

# Soak the new process: require continued healthy status, the new version,
# and this attempt's identity for HANDOFF_SOAK_SECS (at least one check).
handoff_soak() {  # id
    local id="$1" secs="$HANDOFF_SOAK_SECS"
    case "$secs" in ''|*[!0-9]*) secs=5 ;; esac
    local deadline=$((SECONDS + secs)) body
    while :; do
        body="$(fetch_health)"
        if [ -z "$body" ]; then return 1; fi
        if [ "$(health_version "$body")" != "$VERSION" ]; then return 1; fi
        if ! health_status_healthy "$body"; then return 1; fi
        if ! handoff_matches_attempt "$body" "$id"; then return 1; fi
        if [ "$SECONDS" -ge "$deadline" ]; then break; fi
        if [ "$HEALTH_INTERVAL" -gt 0 ]; then
            "$SLEEP_BIN" "$HEALTH_INTERVAL" || true
        fi
    done
    return 0
}

# Signal and await one in-place handoff. 0 = done and soaked.
run_handoff() {  # id sha
    local id="$1" sha="$2" body version saw_new=0 deadline
    write_handoff_request "$id" "$sha" || return "$HANDOFF_RC_NOOP"

    if ! "$SYSTEMCTL" kill -s SIGUSR2 "$SERVICE_NAME" >/dev/null 2>&1; then
        fail "systemctl kill -s SIGUSR2 $SERVICE_NAME failed"
        return "$HANDOFF_RC_NOOP"
    fi
    log "sent SIGUSR2 to $SERVICE_NAME (handoff_id=$id)"

    deadline=$((SECONDS + HEALTH_TIMEOUT))
    while [ "$SECONDS" -lt "$deadline" ]; do
        body="$(fetch_health)"
        version="$(health_version "$body")"
        if [ -n "$body" ] && [ "$version" = "$VERSION" ]; then
            saw_new=1
            if handoff_matches_attempt "$body" "$id"; then
                if handoff_soak "$id"; then
                    return 0
                fi
                return "$HANDOFF_RC_ROLLBACK"
            fi
        fi
        if [ "$HEALTH_INTERVAL" -gt 0 ]; then
            "$SLEEP_BIN" "$HEALTH_INTERVAL" || true
        fi
    done

    # Timeout. Re-read once to classify the outcome without a repoint.
    body="$(fetch_health)"
    version="$(health_version "$body")"
    if [ -n "$body" ] && [ "$version" = "$VERSION" ]; then
        return "$HANDOFF_RC_ROLLBACK"
    fi
    if [ -z "$body" ] && [ "$saw_new" -eq 0 ]; then
        # /health never came back after the signal: treat as switched-and-failed.
        return "$HANDOFF_RC_ROLLBACK"
    fi
    return "$HANDOFF_RC_NOOP"
}

# Blunt rollback for a handoff that left `current` on the old release or on
# a half-switched state. Never leaves `current` dangling.
handoff_rollback() {
    if [ -n "$PREV_RELEASE" ] && [ -d "$PREV_RELEASE" ]; then
        rollback   # repoints the previous release, restarts, exits 1
    fi
    log "no previous release to roll back to; activating $RELEASE_DIR"
    if repoint "$RELEASE_DIR"; then
        "$SYSTEMCTL" restart "$SERVICE_NAME" >/dev/null 2>&1 || true
        if wait_healthy; then
            log "service healthy on $RELEASE_DIR after fallback activation"
        else
            fail "service not healthy after fallback activation"
        fi
    else
        fail "could not repoint current to $RELEASE_DIR"
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

# Same-version no-op: `current` already names the target. Empty on a first
# install, so this never fires there. --force bypasses it.
if [ "$FORCE" -eq 0 ] && [ "$(running_version)" = "$VERSION" ]; then
    log "already at $VERSION; nothing to do (pass --force to reinstall)"
    exit 0
fi

printf 'relay-install: version=%s binary=%s\n' "$VERSION" "$BINARY_PATH"

# 1. Verify the staged binary before touching the layout.
verify_binary || exit 1

# 1b. Prepare node-local config and refresh the self-updater before activation
#     so the new process reads the corrected config on its first start.
if [ -n "$PUBLIC_IP" ]; then
    ensure_public_ip "$PUBLIC_IP" || exit 2
fi
if [ -n "$UPDATER_PATH" ]; then
    install_updater "$UPDATER_PATH"
fi

# 2. Create the immutable release directory and stage the binary under a
#    temp name. The release binary is not touched until --check passes.
install -d "$RELEASES_DIR"
install -d "$RELEASE_DIR"
STAGED_BIN="$RELEASE_DIR/.$BINARY_NAME.staged.$$"
install -m 0755 "$BINARY_PATH" "$STAGED_BIN"

# 3. Config gate: never activate a release whose --check fails. Checking the
#    staged file leaves the active release byte-for-byte untouched on failure.
if ! "$STAGED_BIN" --check --config "$CONFIG_PATH"; then
    fail "--check failed for $RELEASE_DIR; aborting before activation"
    rm -f "$STAGED_BIN"
    # Leave the active release untouched. Remove the fresh release dir only
    # when it is not what `current` points at (so current can never dangle).
    if [ "$RELEASE_DIR" != "$PREV_RELEASE" ] && [ "$RELEASE_DIR" != "$(active_release)" ]; then
        rm -rf "$RELEASE_DIR"
    fi
    exit 1
fi

# 4. Atomic publish: rename the verified staged binary over the release
#    binary. A running process keeps its old inode.
mv -f "$STAGED_BIN" "$RELEASE_BIN"
STAGED_BIN=""

# 5. Activate. Prefer the in-place handoff when possible; otherwise repoint
#    and restart. Each path health-gates and never leaves `current` broken.
use_handoff_rc=0
use_handoff || use_handoff_rc=$?
if [ "$use_handoff_rc" -eq 2 ]; then
    exit 2
fi

if [ "$use_handoff_rc" -eq 0 ]; then
    HANDOFF_SHA="$(sha256_of "$RELEASE_BIN" || true)"
    if [ -z "$HANDOFF_SHA" ]; then
        fail "cannot compute sha256 of $RELEASE_BIN"
        exit 1
    fi
    HANDOFF_ID="$(new_handoff_id)"

    run_handoff_rc=0
    run_handoff "$HANDOFF_ID" "$HANDOFF_SHA" || run_handoff_rc=$?
    case "$run_handoff_rc" in
        0)
            if ! repoint "$RELEASE_DIR"; then
                fail "handoff succeeded but current could not be repointed to $RELEASE_DIR"
                exit 1
            fi
            prune_releases
            log "handed off to $VERSION (current -> $RELEASE_DIR)"
            exit 0
            ;;
        "$HANDOFF_RC_ROLLBACK")
            fail "handoff to $VERSION did not stay healthy"
            handoff_rollback
            ;;
        *)
            fail "handoff to $VERSION did not run; $SERVICE_NAME left on ${PREV_RELEASE:-its release}"
            exit 1
            ;;
    esac
fi

if activate; then
    prune_releases
    log "activated $VERSION (current -> $RELEASE_DIR)"
    exit 0
fi

rollback
