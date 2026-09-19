#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for relay-updater.sh
#
# Exercises the updater entirely against local fixtures: the release
# API is a `file://` JSON document, assets are real .tar.xz archives
# in a fixture directory, and relay-install.sh is a stub recorded
# through the LIGHTSPEED_INSTALL_BIN seam. No network, no install.
#
# Asserted behaviours:
#   (a) a strictly newer release is reported by --dry-run; no install call
#   (b) an equal version is a no-op (dry-run and real run)
#   (c) an older release never installs
#   (d) prereleases are skipped unless --allow-prerelease
#   (e) --force applies an equal version through the installer
#   (f) a SHA-256 mismatch aborts before the installer runs
#   (g) the state file is written for no-update and mismatch outcomes
#   (h) a held lock makes a run exit 0 with last_result=locked
#   (i) a relay-install failure that rolled back is recorded as rolled_back
#
# Usage: bash infra/scripts/test_relay_updater.sh
# Exits 0 and prints "relay-updater: all assertions passed" on success.
# Requires: bash, jq, tar, xz, sha256sum, flock. No network access.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
UPDATER="$SCRIPT_DIR/relay-updater.sh"

if [ ! -f "$UPDATER" ]; then
    printf 'relay-updater: FAIL - updater not found: %s\n' "$UPDATER" >&2
    exit 1
fi

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

API_DIR="$TMP/api"
ROOT="$TMP/opt/lightspeed"
BIN_DIR="$TMP/bin"
STATE="$TMP/state.json"
LOCK="$TMP/update.lock"
CALLS="$TMP/install-calls"
OUT="$TMP/out.txt"
ERR="$TMP/err.txt"
ASSET_NAME="lightspeed-proxy-x86_64-unknown-linux-gnu.tar.xz"

mkdir -p "$API_DIR" "$ROOT/releases" "$BIN_DIR"

note_pass() { PASS=$((PASS + 1)); }
note_fail() { printf '  FAIL: %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); }

assert_eq() {
    local actual="$1" expected="$2" msg="$3"
    if [ "$actual" = "$expected" ]; then note_pass; else
        note_fail "$msg (got '$actual', expected '$expected')"
    fi
}

assert_rc_zero() {
    if [ "$1" -eq 0 ]; then note_pass; else note_fail "$2 (exit was $1)"; fi
}

assert_rc_nonzero() {
    if [ "$1" -ne 0 ]; then note_pass; else note_fail "$2 (exit was 0)"; fi
}

assert_grep() {
    if grep -q -- "$2" "$1" 2>/dev/null; then note_pass; else
        note_fail "$3 (missing '$2' in $1)"
    fi
}

assert_not_null() {
    if [ -n "$1" ] && [ "$1" != "null" ] && [ "$1" != "MISSING" ]; then note_pass; else
        note_fail "$2 (was '$1')"
    fi
}

# ── Fixture: stub relay-install ──────────────────────────────
cat > "$BIN_DIR/relay-install" <<'STUB'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "$INSTALL_CALLS"
if [ -n "${INSTALL_MSG:-}" ]; then printf '%s\n' "$INSTALL_MSG"; fi
exit "${INSTALL_RC:-0}"
STUB
chmod +x "$BIN_DIR/relay-install"

# ── Fixture builders ─────────────────────────────────────────
make_asset() {  # $1 = version baked into the fake proxy
    local v="$1" d="$TMP/build"
    rm -rf "$d"; mkdir -p "$d"
    {
        printf '#!/usr/bin/env bash\n'
        printf 'echo "lightspeed-proxy %s"\n' "$v"
    } > "$d/lightspeed-proxy"
    chmod +x "$d/lightspeed-proxy"
    "$TAR_BIN" -cJf "$API_DIR/$ASSET_NAME" -C "$d" lightspeed-proxy
    "$SHA_BIN" "$API_DIR/$ASSET_NAME" \
        | awk '{print $1"  '"$ASSET_NAME"'"}' > "$API_DIR/$ASSET_NAME.sha256"
}

write_release() {  # $1 = tag, $2 = prerelease (true|false)
    jq -n --arg tag "$1" --argjson pre "$2" \
        --arg aurl "file://$API_DIR/$ASSET_NAME" \
        --arg surl "file://$API_DIR/$ASSET_NAME.sha256" \
        --arg an "$ASSET_NAME" --arg sn "$ASSET_NAME.sha256" \
        '{tag_name: $tag, prerelease: $pre,
          assets: [
            {name: $an, browser_download_url: $aurl},
            {name: $sn, browser_download_url: $surl}
          ]}' > "$API_DIR/release.json"
}

set_current() {  # $1 = active version
    rm -rf "$ROOT"; mkdir -p "$ROOT/releases/$1"
    {
        printf '#!/usr/bin/env bash\n'
        printf 'echo "lightspeed-proxy %s"\n' "$1"
    } > "$ROOT/releases/$1/lightspeed-proxy"
    chmod +x "$ROOT/releases/$1/lightspeed-proxy"
    ln -sfn "$ROOT/releases/$1" "$ROOT/current"
}

clear_current() {
    rm -rf "$ROOT"; mkdir -p "$ROOT/releases"
}

reset_state() { rm -f "$STATE" "$CALLS"; }

TAR_BIN="${LIGHTSPEED_TAR:-tar}"
SHA_BIN="${LIGHTSPEED_SHA256SUM:-sha256sum}"

run_updater() {
    env \
        LIGHTSPEED_RELEASE_API="file://$API_DIR/release.json" \
        LIGHTSPEED_LAYOUT_ROOT="$ROOT" \
        LIGHTSPEED_UPDATE_STATE="$STATE" \
        LIGHTSPEED_UPDATE_LOCK="$LOCK" \
        LIGHTSPEED_INSTALL_BIN="$BIN_DIR/relay-install" \
        LIGHTSPEED_HOST_ARCH="x86_64" \
        GITHUB_TOKEN= \
        INSTALL_CALLS="$CALLS" \
        bash "$UPDATER" "$@" >"$OUT" 2>"$ERR"
    RC=$?
}

state_field() { jq -r --arg k "$1" '.[$k] // "null"' "$STATE" 2>/dev/null || printf 'MISSING'; }
install_call_count() {
    if [ -f "$CALLS" ]; then wc -l < "$CALLS" | tr -d ' '; else printf '0'; fi
}

# ── (a) newer release is reported by --dry-run, no install ───
reset_state
set_current "1.4.3"
make_asset "1.4.4"
write_release "v1.4.4" false
run_updater --dry-run
assert_rc_zero "$RC" "(a) dry-run exits 0"
assert_grep "$OUT" "update available" "(a) dry-run reports an update"
assert_grep "$OUT" "no download, no install" "(a) dry-run states it will not install"
assert_eq "$(install_call_count)" "0" "(a) dry-run made no install call"
assert_eq "$(state_field last_result)" "ok" "(a) state records ok"
assert_eq "$(state_field current_version)" "1.4.3" "(a) state records the running version"
assert_eq "$(state_field available_version)" "1.4.4" "(a) state records the available version"

# ── (b) equal version is a no-op ─────────────────────────────
reset_state
set_current "1.4.4"
make_asset "1.4.4"
write_release "v1.4.4" false
run_updater --dry-run
assert_rc_zero "$RC" "(b) equal-version dry-run exits 0"
assert_eq "$(state_field last_result)" "no_update" "(b) dry-run records no_update"
assert_grep "$OUT" "up to date" "(b) dry-run says up to date"
reset_state
run_updater
assert_rc_zero "$RC" "(b) equal-version real run exits 0"
assert_eq "$(install_call_count)" "0" "(b) equal version does not install"
assert_eq "$(state_field last_result)" "no_update" "(b) state records no_update"

# ── (c) older release never installs ─────────────────────────
reset_state
set_current "1.4.5"
make_asset "1.4.4"
write_release "v1.4.4" false
run_updater
assert_rc_zero "$RC" "(c) older release run exits 0"
assert_eq "$(install_call_count)" "0" "(c) older release does not install"
assert_eq "$(state_field last_result)" "no_update" "(c) state records no_update"

# ── (d) prereleases are skipped unless allowed ───────────────
reset_state
set_current "1.4.4"
make_asset "1.5.0-rc.1"
write_release "v1.5.0-rc.1" true
run_updater --dry-run
assert_rc_zero "$RC" "(d) prerelease dry-run exits 0"
assert_eq "$(state_field last_result)" "no_update" "(d) prerelease is skipped"
assert_grep "$OUT" "prerelease" "(d) skip names the prerelease"
run_updater --allow-prerelease --dry-run
assert_rc_zero "$RC" "(d) --allow-prerelease dry-run exits 0"
assert_eq "$(state_field last_result)" "ok" "(d) allowed prerelease is offered"
assert_eq "$(state_field available_version)" "1.5.0-rc.1" "(d) prerelease version is recorded"

# ── (e) --force applies through the installer ────────────────
reset_state
set_current "1.4.4"
make_asset "1.4.4"
write_release "v1.4.4" false
run_updater --force
assert_rc_zero "$RC" "(e) forced apply exits 0"
assert_eq "$(install_call_count)" "1" "(e) installer was called exactly once"
assert_grep "$CALLS" "--version 1.4.4" "(e) installer received the release version"
assert_eq "$(state_field last_result)" "updated" "(e) state records updated"
assert_eq "$(state_field current_version)" "1.4.4" "(e) state records the new version"
assert_eq "$(state_field active_release)" "$ROOT/releases/1.4.4" "(e) state records the new release path"
assert_not_null "$(state_field last_apply_at)" "(e) state records an apply timestamp"

# ── (f) SHA-256 mismatch aborts before install ───────────────
reset_state
set_current "1.4.3"
make_asset "1.4.4"
printf 'deadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef  %s\n' \
    "$ASSET_NAME" > "$API_DIR/$ASSET_NAME.sha256"
write_release "v1.4.4" false
run_updater
assert_rc_nonzero "$RC" "(f) mismatch exits non-zero"
assert_eq "$(install_call_count)" "0" "(f) mismatch does not reach the installer"
assert_eq "$(state_field last_result)" "failed" "(f) state records failed"
assert_grep "$ERR" "SHA-256 mismatch" "(f) error names the checksum mismatch"

# ── (g) state file exists for no-update and mismatch ─────────
if [ -s "$STATE" ]; then note_pass; else note_fail "(g) state file missing/empty after mismatch"; fi
reset_state
set_current "1.4.4"
write_release "v1.4.4" false
run_updater
if [ -s "$STATE" ]; then note_pass; else note_fail "(g) state file missing/empty after no-update"; fi

# ── (h) a held lock -> exit 0 with last_result=locked ────────
reset_state
set_current "1.4.4"
make_asset "1.4.4"
write_release "v1.4.4" false
: > "$LOCK"
( exec 8>"$LOCK"; flock 8; sleep 5 ) &
HOLDER=$!
sleep 0.6
run_updater --dry-run
assert_rc_zero "$RC" "(h) lock-held run exits 0"
assert_grep "$OUT" "locked" "(h) lock-held run reports locked"
assert_eq "$(state_field last_result)" "locked" "(h) state records locked"
kill "$HOLDER" 2>/dev/null || true
wait "$HOLDER" 2>/dev/null || true

# ── (i) a rolled-back install is recorded ────────────────────
reset_state
set_current "1.4.3"
make_asset "1.4.4"
write_release "v1.4.4" false
export INSTALL_RC=1
export INSTALL_MSG="  relay-install: rolling back to $ROOT/releases/1.4.3"
run_updater
unset INSTALL_RC INSTALL_MSG
assert_rc_nonzero "$RC" "(i) rolled-back run exits non-zero"
assert_grep "$OUT" "rolling back" "(i) installer output is surfaced"
assert_eq "$(state_field last_result)" "rolled_back" "(i) state records rolled_back"

# ── (j) --help exits 0 and writes no state ───────────────────
reset_state
run_updater --help
assert_rc_zero "$RC" "(j) --help exits 0"
if [ ! -e "$STATE" ]; then note_pass; else note_fail "(j) --help wrote a state file"; fi

# ── (k) an unknown argument records a failed state and exits 2 ──
reset_state
run_updater --bogus
assert_eq "$RC" "2" "(k) unknown argument exits 2"
assert_eq "$(state_field last_result)" "failed" "(k) state records failed"
assert_grep "$ERR" "unknown argument" "(k) error names the argument"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
    printf 'relay-updater: all assertions passed\n'
    printf '  (%s checks)\n' "$PASS"
    exit 0
fi

printf 'relay-updater: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
