#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Verified Relay Self-Updater
#
# Checks the GitHub release channel for a newer lightspeed-proxy,
# downloads the matching arch tarball, verifies its SHA-256, extracts
# the binary, and hands it to relay-install.sh, which runs --check,
# activates the release, health-gates it, and rolls back on failure.
#
# The state file is written on every run (no-op, update, and failure)
# so operators can see the last check/apply outcome.
#
# Usage:
#   relay-updater.sh [--dry-run] [--force] [--allow-prerelease] [--geoip-only]
#
# --geoip-only syncs only the pinned GeoIP MMDB and exits; it never touches
# the binary or the versioned release layout.
#
# Exit codes:
#   0  up to date, dry-run, or another run holds the lock
#   1  update failed (the state file records why)
#   2  usage error / unsupported host arch
#
# Env seams (defaults in parentheses):
#   LIGHTSPEED_LAYOUT_ROOT   (/opt/lightspeed)
#   LIGHTSPEED_INSTALL_BIN   (<script dir>/relay-install.sh)
#   LIGHTSPEED_UPDATE_STATE  (/var/lib/lightspeed/update-state.json)
#   LIGHTSPEED_UPDATE_LOCK   (/var/lock/lightspeed-update.lock)
#   LIGHTSPEED_RELEASE_REPO  (ShibbityShwab/lightspeed)
#   LIGHTSPEED_RELEASE_API   (https://api.github.com/repos/<repo>/releases/latest)
#   LIGHTSPEED_GEOIP_API     (https://api.github.com/repos/<repo>/releases?per_page=100)
#   LIGHTSPEED_GEOIP_DIR     (/opt/lightspeed/geoip)
#   GITHUB_TOKEN             (unset; sent as a bearer token when present)
#   LIGHTSPEED_HOST_ARCH     (uname -m)
#   LIGHTSPEED_CURL / _TAR / _JQ / _SHA256SUM / _FLOCK
#                            (curl, tar, jq, sha256sum, flock)
# ──────────────────────────────────────────────────────────────
# allow: SIZE_OK - single systemd ExecStart entrypoint; splitting into sourced
# libraries would force a multi-file deploy that setup/provision must sync on
# every relay.
set -euo pipefail

BINARY_NAME="lightspeed-proxy"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

LAYOUT_ROOT="${LIGHTSPEED_LAYOUT_ROOT:-/opt/lightspeed}"
INSTALL_BIN="${LIGHTSPEED_INSTALL_BIN:-$SCRIPT_DIR/relay-install.sh}"
STATE_FILE="${LIGHTSPEED_UPDATE_STATE:-/var/lib/lightspeed/update-state.json}"
LOCK_FILE="${LIGHTSPEED_UPDATE_LOCK:-/var/lock/lightspeed-update.lock}"
RELEASE_REPO="${LIGHTSPEED_RELEASE_REPO:-ShibbityShwab/lightspeed}"
RELEASE_API="${LIGHTSPEED_RELEASE_API:-https://api.github.com/repos/$RELEASE_REPO/releases/latest}"
GITHUB_TOKEN="${GITHUB_TOKEN:-}"
HOST_ARCH="${LIGHTSPEED_HOST_ARCH:-$(uname -m)}"
CURL="${LIGHTSPEED_CURL:-curl}"
TAR="${LIGHTSPEED_TAR:-tar}"
JQ="${LIGHTSPEED_JQ:-jq}"
SHA256SUM="${LIGHTSPEED_SHA256SUM:-sha256sum}"
FLOCK="${LIGHTSPEED_FLOCK:-flock}"
GEOIP_DIR="${LIGHTSPEED_GEOIP_DIR:-/opt/lightspeed/geoip}"
GEOIP_ASSET_NAME="dbip-country-lite.mmdb"
GEOIP_API="${LIGHTSPEED_GEOIP_API:-https://api.github.com/repos/$RELEASE_REPO/releases?per_page=100}"

CURL_AUTH=()
if [ -n "$GITHUB_TOKEN" ]; then
    CURL_AUTH=(-H "Authorization: Bearer $GITHUB_TOKEN")
fi

DRY_RUN=0
FORCE=0
ALLOW_PRE=0
GEOIP_ONLY=0

log()  { printf 'relay-updater: %s\n' "$*"; }
warn() { printf 'relay-updater: WARNING: %s\n' "$*" >&2; }

usage() {
    sed -n '2,37p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
}

# --help is answered before any state or trap setup so it never writes
# update-state.json and always exits 0.
for arg in "$@"; do
    case "$arg" in
        -h|--help) usage; exit 0 ;;
    esac
done

# ── State accumulators (written by write_state) ──────────────
STATE_WRITTEN=0
LAST_RESULT="never"
LAST_ERROR=""
LAST_APPLY_AT=""
AVAILABLE_VERSION=""
TMP_DIR=""

read_active_release() {
    local target
    target="$(readlink -f "$LAYOUT_ROOT/current" 2>/dev/null || true)"
    if [ -n "$target" ] && [ -d "$target" ]; then
        printf '%s' "$target"
    fi
}
ACTIVE_RELEASE="$(read_active_release)"

# Read `lightspeed-proxy --version` from the active release, falling
# back to the basename of the `current` symlink target.
detect_active_version() {
    [ -n "$ACTIVE_RELEASE" ] || return 0
    local bin="$ACTIVE_RELEASE/$BINARY_NAME" out
    if [ -x "$bin" ]; then
        out="$("$bin" --version 2>/dev/null || true)"
        local v
        v="$(printf '%s\n' "$out" \
            | grep -oE '[0-9]+\.[0-9]+\.[0-9]+([-+][0-9A-Za-z.-]+)?' 2>/dev/null | head -1 || true)"
        if [ -n "$v" ]; then printf '%s' "$v"; return 0; fi
    fi
    basename "$ACTIVE_RELEASE"
}
CURRENT_VERSION="$(detect_active_version)"
ROLLBACK_VERSION="$CURRENT_VERSION"

# Preserve the previous successful apply timestamp across runs.
if [ -n "$STATE_FILE" ] && [ -f "$STATE_FILE" ] && "$JQ" -e . "$STATE_FILE" >/dev/null 2>&1; then
    LAST_APPLY_AT="$("$JQ" -r '.last_apply_at // empty' "$STATE_FILE" 2>/dev/null || true)"
fi

# ── State file ───────────────────────────────────────────────
# Always called exactly once per run; builds the JSON with jq, never
# by hand. State-directory or write failures are non-fatal (warned).
write_state() {
    STATE_WRITTEN=1
    local dir tmp
    dir="$(dirname "$STATE_FILE")"
    if [ ! -d "$dir" ] && ! mkdir -p "$dir" 2>/dev/null; then
        warn "cannot create state directory: $dir"
        return 0
    fi
    tmp="$STATE_FILE.tmp.$$"
    if ! "$JQ" -n \
        --argjson schema_version 1 \
        --arg current_version "$CURRENT_VERSION" \
        --arg active_release "$ACTIVE_RELEASE" \
        --arg available_version "$AVAILABLE_VERSION" \
        --arg rollback_version "$ROLLBACK_VERSION" \
        --argjson last_check_at "$(date +%s)" \
        --arg last_apply_at "$LAST_APPLY_AT" \
        --arg last_result "$LAST_RESULT" \
        --arg last_error "$LAST_ERROR" \
        '{
            schema_version: $schema_version,
            current_version: (if $current_version == "" then null else $current_version end),
            active_release:  (if $active_release  == "" then null else $active_release  end),
            available_version: (if $available_version == "" then null else $available_version end),
            rollback_version: (if $rollback_version == "" then null else $rollback_version end),
            last_check_at: $last_check_at,
            last_apply_at: (if $last_apply_at == "" then null else ($last_apply_at | tonumber) end),
            last_result: $last_result,
            last_error: (if $last_error == "" then null else $last_error end)
        }' > "$tmp" 2>/dev/null; then
        warn "failed to build state JSON"
        rm -f "$tmp"
        return 0
    fi
    mv -f "$tmp" "$STATE_FILE" 2>/dev/null || { warn "failed to write state file: $STATE_FILE"; rm -f "$tmp"; }
}

cleanup() {
    if [ -n "$TMP_DIR" ]; then
        rm -rf "$TMP_DIR"
    fi
}
on_exit() {
    local rc=$?
    cleanup
    if [ "$STATE_WRITTEN" -eq 0 ]; then
        if [ "$LAST_RESULT" = "never" ]; then
            LAST_RESULT="failed"
            [ -n "$LAST_ERROR" ] || LAST_ERROR="unexpected exit (rc=$rc)"
        fi
        write_state || true
    fi
    exit "$rc"
}
trap on_exit EXIT
# Ensure the EXIT trap (and the state write) runs when systemd stops us.
trap 'exit 130' INT
trap 'exit 143' TERM

fail_run() {  # message
    LAST_RESULT="failed"
    LAST_ERROR="$1"
    warn "$1"
    write_state
    exit 1
}

# Usage / unsupported-arch failure: record it in the state file before
# exiting 2 (the documented usage error code).
usage_fail() {  # message [show_usage]
    LAST_RESULT="failed"
    LAST_ERROR="$1"
    warn "$1"
    if [ "${2:-0}" -eq 1 ]; then
        usage >&2
    fi
    write_state
    exit 2
}

# ── Arg parsing ──────────────────────────────────────────────
while [ $# -gt 0 ]; do
    case "$1" in
        --dry-run)          DRY_RUN=1; shift ;;
        --force)            FORCE=1; shift ;;
        --allow-prerelease) ALLOW_PRE=1; shift ;;
        --geoip-only)       GEOIP_ONLY=1; shift ;;
        -h|--help)          STATE_WRITTEN=1; usage; exit 0 ;;
        *) usage_fail "unknown argument: $1" 1 ;;
    esac
done

# ── Target triple ────────────────────────────────────────────
case "$HOST_ARCH" in
    x86_64|amd64)  TRIPLE="x86_64-unknown-linux-gnu" ;;
    aarch64|arm64) TRIPLE="aarch64-unknown-linux-gnu" ;;
    *) usage_fail "unsupported host arch: $HOST_ARCH" 0 ;;
esac
ASSET_NAME="$BINARY_NAME-$TRIPLE.tar.xz"

# semver_cmp A B -> exit 0 equal, 1 A<B, 2 A>B.
# Compares major.minor.patch numerically; a release outranks its own
# prerelease, and two prereleases compare lexically.
semver_cmp() {
    local a="${1#v}" b="${2#v}"
    a="${a%%+*}"; b="${b%%+*}"
    local a_pre="" b_pre=""
    case "$a" in *-*) a_pre="${a#*-}"; a="${a%%-*}";; esac
    case "$b" in *-*) b_pre="${b#*-}"; b="${b%%-*}";; esac

    local i ai bi
    for i in 1 2 3; do
        ai="$(printf '%s' "$a" | cut -d. -f"$i")"
        bi="$(printf '%s' "$b" | cut -d. -f"$i")"
        case "$ai" in ''|*[!0-9]*) ai=0;; esac
        case "$bi" in ''|*[!0-9]*) bi=0;; esac
        if [ "$((10#$ai))" -lt "$((10#$bi))" ]; then return 1; fi
        if [ "$((10#$ai))" -gt "$((10#$bi))" ]; then return 2; fi
    done
    if [ -z "$a_pre" ] && [ -z "$b_pre" ]; then return 0; fi
    if [ -z "$a_pre" ]; then return 2; fi
    if [ -z "$b_pre" ]; then return 1; fi
    if [ "$a_pre" = "$b_pre" ]; then return 0; fi
    if [[ "$a_pre" > "$b_pre" ]]; then return 2; fi
    return 1
}

# verify_sha256 FILE SHA_FILE -> prints the actual digest, returns 0 on match.
# Returns 1 on mismatch or an empty/unreadable checksum file. Shared by the
# tarball and GeoIP paths so neither can install unverified bytes.
verify_sha256() {  # file sha_file
    local file="$1" sha_file="$2" expected actual
    [ -f "$file" ] || return 1
    expected="$(awk '{print $1}' "$sha_file" 2>/dev/null | head -1 | tr 'A-F' 'a-f')"
    actual="$("$SHA256SUM" "$file" 2>/dev/null | awk '{print $1}')"
    if [ -z "$expected" ] || [ "$expected" != "$actual" ]; then
        return 1
    fi
    printf '%s' "$actual"
    return 0
}

# sync_geoip resolves the newest release carrying a lightspeed-geoip-*.mmdb
# asset, downloads the MMDB and its .sha256, verifies the digest, and installs
# the DB atomically. Every failure warns and returns; it never aborts the
# updater and never replaces an existing DB with an unverified download.
sync_geoip() {
    local json meta geo_tag asset_name asset_url sha_url
    local gtmp db_file sha_file actual staged

    json="$("$CURL" -sfL --max-time 30 \
        -H "Accept: application/vnd.github+json" \
        "${CURL_AUTH[@]}" "$GEOIP_API" 2>/dev/null || true)"
    if [ -z "$json" ]; then
        warn "geoip: releases API request failed: $GEOIP_API"
        return 0
    fi

    meta="$(printf '%s' "$json" | "$JQ" -r '
        [ .[] | select((.assets // []) | any(.name | (startswith("lightspeed-geoip-") and endswith(".mmdb")))) ][0] as $rel
        | (($rel.assets // []) | map(select(.name | (startswith("lightspeed-geoip-") and endswith(".mmdb"))))[0] // {}) as $mmdb
        | (($rel.assets // []) | map(select(.name == (($mmdb.name // "") + ".sha256")))[0] // {}) as $sum
        | select(($mmdb.name // "") != "")
        | "\($rel.tag_name)\t\($mmdb.name)\t\($mmdb.browser_download_url // "")\t\($sum.browser_download_url // "")"' 2>/dev/null || true)"
    IFS=$'\t' read -r geo_tag asset_name asset_url sha_url <<< "$meta"
    if [ -z "$asset_name" ]; then
        warn "geoip: no release carries a lightspeed-geoip-*.mmdb asset"
        return 0
    fi
    if [ -z "$asset_url" ]; then
        warn "geoip: release $geo_tag has no download URL for $asset_name"
        return 0
    fi
    if [ -z "$sha_url" ]; then
        warn "geoip: release $geo_tag is missing $asset_name.sha256"
        return 0
    fi

    gtmp="$(mktemp -d "${TMPDIR:-/tmp}/lightspeed-geoip.XXXXXX" 2>/dev/null)" || {
        warn "geoip: cannot create a temp directory"
        return 0
    }
    db_file="$gtmp/$asset_name"
    sha_file="$gtmp/$asset_name.sha256"

    if ! "$CURL" -sfL --max-time 300 -H "Accept: application/octet-stream" \
            "${CURL_AUTH[@]}" -o "$db_file.part" "$asset_url" 2>/dev/null; then
        warn "geoip: download failed: $asset_url"
        rm -rf "$gtmp"
        return 0
    fi
    mv -f "$db_file.part" "$db_file"

    if ! "$CURL" -sfL --max-time 60 -H "Accept: application/octet-stream" \
            "${CURL_AUTH[@]}" -o "$sha_file.part" "$sha_url" 2>/dev/null; then
        warn "geoip: checksum download failed: $sha_url"
        rm -rf "$gtmp"
        return 0
    fi
    mv -f "$sha_file.part" "$sha_file"

    if ! actual="$(verify_sha256 "$db_file" "$sha_file")"; then
        warn "geoip: SHA-256 mismatch for $asset_name; existing DB left untouched"
        rm -rf "$gtmp"
        return 0
    fi

    if ! install -d -m 0755 "$GEOIP_DIR" 2>/dev/null; then
        warn "geoip: cannot create $GEOIP_DIR"
        rm -rf "$gtmp"
        return 0
    fi
    staged="$GEOIP_DIR/.$GEOIP_ASSET_NAME.$$.tmp"
    if ! install -m 0644 "$db_file" "$staged" 2>/dev/null; then
        warn "geoip: cannot stage $GEOIP_DIR/$GEOIP_ASSET_NAME"
        rm -f "$staged"
        rm -rf "$gtmp"
        return 0
    fi
    if mv -f "$staged" "$GEOIP_DIR/$GEOIP_ASSET_NAME" 2>/dev/null; then
        log "geoip: installed $GEOIP_ASSET_NAME from $geo_tag (sha256 $actual)"
    else
        warn "geoip: failed to install $GEOIP_DIR/$GEOIP_ASSET_NAME"
        rm -f "$staged"
    fi
    rm -rf "$gtmp"
    return 0
}

# ── Exclusive lock ───────────────────────────────────────────
LOCK_DIR="$(dirname "$LOCK_FILE")"
if [ ! -d "$LOCK_DIR" ]; then
    mkdir -p "$LOCK_DIR" 2>/dev/null || true
fi
if ! ( : > "$LOCK_FILE" ) 2>/dev/null; then
    fail_run "cannot create lock file: $LOCK_FILE"
fi
exec 9>"$LOCK_FILE"
if ! "$FLOCK" -n 9; then
    LAST_RESULT="locked"
    LAST_ERROR="another update run holds $LOCK_FILE"
    log "locked: another update run is in progress; exiting"
    write_state
    exit 0
fi

# ── GeoIP sync ───────────────────────────────────────────────
# --geoip-only is a standalone path: sync the MMDB, record the run, exit.
# A normal (non-dry-run) invocation syncs the MMDB too, best-effort; a geoip
# failure warns and continues and never aborts the binary update path.
if [ "$GEOIP_ONLY" -eq 1 ]; then
    sync_geoip || true
    LAST_RESULT="ok"
    LAST_ERROR=""
    write_state
    exit 0
fi
if [ "$DRY_RUN" -eq 0 ]; then
    sync_geoip || true
fi

# ── Query the release channel ────────────────────────────────
json="$("$CURL" -sfL --max-time 30 \
    -H "Accept: application/vnd.github+json" \
    "${CURL_AUTH[@]}" "$RELEASE_API" 2>/dev/null || true)"
if [ -z "$json" ]; then
    fail_run "release API request failed: $RELEASE_API"
fi

TAG="$(printf '%s' "$json" | "$JQ" -r '.tag_name // empty' 2>/dev/null || true)"
PRE_FLAG="$(printf '%s' "$json" | "$JQ" -r '.prerelease // false' 2>/dev/null || true)"
if [ -z "$TAG" ]; then
    fail_run "release API response has no tag_name: $RELEASE_API"
fi

case "$TAG" in
    v*) VERSION="${TAG#v}" ;;
    *)  VERSION="$TAG" ;;
esac
AVAILABLE_VERSION="$VERSION"

IS_PRE=0
if [ "$PRE_FLAG" = "true" ]; then IS_PRE=1; fi
case "$VERSION" in *-*) IS_PRE=1 ;; esac
if [ "$IS_PRE" -eq 1 ] && [ "$ALLOW_PRE" -eq 0 ]; then
    LAST_RESULT="no_update"
    log "latest release $TAG is a prerelease; skipping (use --allow-prerelease)"
    write_state
    exit 0
fi

CMP=0
semver_cmp "$VERSION" "$CURRENT_VERSION" || CMP=$?
WOULD_UPDATE=0
if [ "$CMP" -eq 2 ]; then WOULD_UPDATE=1; fi
if [ "$FORCE" -eq 1 ]; then WOULD_UPDATE=1; fi

# ── Dry run: report and exit without downloading or installing ──
if [ "$DRY_RUN" -eq 1 ]; then
    if [ "$WOULD_UPDATE" -eq 1 ]; then
        LAST_RESULT="ok"
        log "dry-run: update available: ${CURRENT_VERSION:-none} -> $VERSION"
    else
        LAST_RESULT="no_update"
        log "dry-run: up to date (${CURRENT_VERSION:-none}); latest is $VERSION"
    fi
    log "dry-run: no download, no install"
    write_state
    exit 0
fi

# ── No update needed ─────────────────────────────────────────
if [ "$WOULD_UPDATE" -eq 0 ]; then
    LAST_RESULT="no_update"
    log "up to date (${CURRENT_VERSION:-none}); latest is $VERSION"
    write_state
    exit 0
fi

# ── Download, verify, extract ────────────────────────────────
asset_url="$(printf '%s' "$json" | "$JQ" -r --arg n "$ASSET_NAME" \
    '.assets[] | select(.name == $n) | .browser_download_url' 2>/dev/null | head -1 || true)"
sha_url="$(printf '%s' "$json" | "$JQ" -r --arg n "$ASSET_NAME.sha256" \
    '.assets[] | select(.name == $n) | .browser_download_url' 2>/dev/null | head -1 || true)"
if [ -z "$asset_url" ] || [ -z "$sha_url" ]; then
    fail_run "release $TAG is missing assets for $TRIPLE"
fi

TMP_DIR="$(mktemp -d "${TMPDIR:-/tmp}/lightspeed-update.XXXXXX")"
archive="$TMP_DIR/$ASSET_NAME"
sha_file="$TMP_DIR/$ASSET_NAME.sha256"

if ! "$CURL" -sfL --max-time 300 -H "Accept: application/octet-stream" \
        "${CURL_AUTH[@]}" -o "$archive.part" "$asset_url" 2>/dev/null; then
    fail_run "download failed: $asset_url"
fi
mv -f "$archive.part" "$archive"

if ! "$CURL" -sfL --max-time 60 -H "Accept: application/octet-stream" \
        "${CURL_AUTH[@]}" -o "$sha_file.part" "$sha_url" 2>/dev/null; then
    fail_run "checksum download failed: $sha_url"
fi
mv -f "$sha_file.part" "$sha_file"

actual="$(verify_sha256 "$archive" "$sha_file")" || fail_run "SHA-256 mismatch for $ASSET_NAME"
log "sha256 verified ($actual)"

mkdir -p "$TMP_DIR/extract"
if ! "$TAR" -xJf "$archive" -C "$TMP_DIR/extract" 2>/dev/null; then
    fail_run "failed to extract $ASSET_NAME"
fi
staged="$(find "$TMP_DIR/extract" -type f -name "$BINARY_NAME" -print -quit 2>/dev/null || true)"
if [ -z "$staged" ]; then
    fail_run "$ASSET_NAME does not contain $BINARY_NAME"
fi
chmod +x "$staged"

# ── Apply through relay-install.sh ───────────────────────────
LAST_APPLY_AT="$(date +%s)"
install_out=""
install_rc=0
install_out="$("$BASH" "$INSTALL_BIN" --binary "$staged" --version "$VERSION" 2>&1)" || install_rc=$?
printf '%s\n' "$install_out"

if [ "$install_rc" -eq 0 ]; then
    CURRENT_VERSION="$VERSION"
    ACTIVE_RELEASE="$LAYOUT_ROOT/releases/$VERSION"
    LAST_RESULT="updated"
    LAST_ERROR=""
    log "updated to $VERSION"
    write_state
    exit 0
fi

ACTIVE_RELEASE="$(read_active_release)"
CURRENT_VERSION="$(detect_active_version)"
if printf '%s' "$install_out" | grep -q 'rolling back'; then
    LAST_RESULT="rolled_back"
    LAST_ERROR="install of $VERSION failed; relay-install rolled back"
else
    LAST_RESULT="failed"
    LAST_ERROR="relay-install failed for $VERSION (rc=$install_rc)"
fi
warn "$LAST_ERROR"
write_state
exit "$install_rc"
