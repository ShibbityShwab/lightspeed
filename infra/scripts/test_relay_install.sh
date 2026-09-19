#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for the relay installer (relay-install.sh)
#
# Runs the layout / activation / rollback / pruning logic against a
# throwaway fixture root with systemctl, curl, and file stubbed out
# through the installer's env seams (LIGHTSPEED_SYSTEMCTL,
# LIGHTSPEED_CURL, LIGHTSPEED_FILE_BIN, LIGHTSPEED_LAYOUT_ROOT, ...).
#
# Asserted behaviours:
#   (a) install activates the release and repoints `current`
#   (b) a failing health gate restores the previous release, leaves the
#       service healthy, and exits non-zero
#   (c) pruning keeps the newest 3 releases and never removes the active one
#   (d) running the same install twice is idempotent
#   (e) a release whose --check fails is never activated and never left behind
#   (f) a non-ELF staged file is rejected before any layout change
#
# Usage: bash infra/scripts/test_relay_install.sh
# Exits 0 and prints "relay-install: all assertions passed" on success.
# Requires: bash, find, readlink. No network access.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
INSTALLER="$SCRIPT_DIR/relay-install.sh"

if [ ! -f "$INSTALLER" ]; then
    printf 'relay-install: FAIL - installer not found: %s\n' "$INSTALLER" >&2
    exit 1
fi

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

FIXTURE="$TMP/fixture"
ROOT="$TMP/opt/lightspeed"
BIN_DIR="$TMP/bin"
mkdir -p "$FIXTURE" "$ROOT/releases" "$BIN_DIR"

note_pass() { PASS=$((PASS + 1)); }
note_fail() { printf '  FAIL: %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); }

assert_eq() {
    local actual="$1" expected="$2" msg="$3"
    if [ "$actual" = "$expected" ]; then note_pass; else
        note_fail "$msg (got '$actual', expected '$expected')"
    fi
}

assert_rc_nonzero() {
    local rc="$1" msg="$2"
    if [ "$rc" -ne 0 ]; then note_pass; else note_fail "$msg (exit was 0)"; fi
}

assert_rc_zero() {
    local rc="$1" msg="$2"
    if [ "$rc" -eq 0 ]; then note_pass; else note_fail "$msg (exit was $rc)"; fi
}

assert_current_exists() {
    if [ -e "$ROOT/current" ]; then note_pass; else note_fail "$1 (current is dangling/missing)"; fi
}

assert_grep() {
    local file="$1" needle="$2" msg="$3"
    if grep -q -- "$needle" "$file" 2>/dev/null; then note_pass; else
        note_fail "$msg (missing '$needle' in $file)"
    fi
}

# ── Stub systemctl ───────────────────────────────────────────
# `restart` inspects where `current` points: a version listed in
# $FIXTURE/fail_versions makes the (stubbed) health check fail, which
# is how the rollback path is exercised.
cat > "$BIN_DIR/systemctl" <<STUB
#!/usr/bin/env bash
F="$FIXTURE"
ROOT="$ROOT"
printf 'systemctl %s\n' "\$*" >> "\$F/systemctl.log"
cmd="\${1:-}"
case "\$cmd" in
    restart)
        tgt="\$(readlink "\$ROOT/current" 2>/dev/null || true)"
        base="\$(basename "\${tgt:-}")"
        if [ -f "\$F/fail_versions" ] && grep -qx -- "\$base" "\$F/fail_versions"; then
            printf 'fail\n' > "\$F/health_mode"
        else
            printf 'ok\n' > "\$F/health_mode"
        fi
        if [ -f "\$F/restart_fail" ]; then exit 1; fi
        exit 0
        ;;
    *) exit 0 ;;
esac
STUB

# ── Stub curl (honours the health mode written by the stubbed restart) ──
cat > "$BIN_DIR/curl" <<STUB
#!/usr/bin/env bash
F="$FIXTURE"
mode="\$(cat "\$F/health_mode" 2>/dev/null || printf ok)"
if [ "\$mode" != "ok" ]; then exit 1; fi
printf '{"status":"healthy","version":"1.4.4","node_id":"fixture","uptime_secs":1}\n'
exit 0
STUB

# ── Stub file (`$FIXTURE/file_not_elf` forces a non-ELF description) ──
cat > "$BIN_DIR/file" <<STUB
#!/usr/bin/env bash
F="$FIXTURE"
if [ -f "\$F/file_not_elf" ]; then
    printf 'ASCII text\n'
else
    printf 'ELF 64-bit LSB pie executable, x86-64, version 1 (SYSV), dynamically linked\n'
fi
exit 0
STUB

chmod +x "$BIN_DIR/systemctl" "$BIN_DIR/curl" "$BIN_DIR/file"
printf '# fixture config\n' > "$FIXTURE/proxy.toml"

# ── Helpers ──────────────────────────────────────────────────
reset_fixture() {
    rm -rf "$ROOT"
    mkdir -p "$ROOT/releases"
    : > "$FIXTURE/systemctl.log"
    rm -f "$FIXTURE/fail_versions" "$FIXTURE/file_not_elf" "$FIXTURE/restart_fail"
    printf 'ok\n' > "$FIXTURE/health_mode"
}

# make_binary <path> <check_rc>
make_binary() {
    cat > "$1" <<STUB
#!/usr/bin/env bash
if [ "\${1:-}" = "--check" ]; then exit $2; fi
echo "lightspeed-proxy 1.4.4"
exit 0
STUB
    chmod +x "$1"
}

# seed_release <version> <touch-date>
seed_release() {
    mkdir -p "$ROOT/releases/$1"
    make_binary "$ROOT/releases/$1/lightspeed-proxy" 0
    touch -d "$2" "$ROOT/releases/$1" "$ROOT/releases/$1/lightspeed-proxy"
}

# run_install <version> <binary-path>; stdout/stderr into $OUT/$ERR
run_install() {
    env \
        LIGHTSPEED_LAYOUT_ROOT="$ROOT" \
        LIGHTSPEED_SYSTEMCTL="$BIN_DIR/systemctl" \
        LIGHTSPEED_CURL="$BIN_DIR/curl" \
        LIGHTSPEED_FILE_BIN="$BIN_DIR/file" \
        LIGHTSPEED_HOST_ARCH="x86_64" \
        LIGHTSPEED_HEALTH_TIMEOUT="1" \
        LIGHTSPEED_HEALTH_INTERVAL="0" \
        LIGHTSPEED_SERVICE_NAME="lightspeed-proxy" \
        LIGHTSPEED_CONFIG_PATH="$FIXTURE/proxy.toml" \
        LIGHTSPEED_KEEP_RELEASES="3" \
        LIGHTSPEED_SLEEP_BIN="true" \
        bash "$INSTALLER" --binary "$2" --version "$1" >"$OUT" 2>"$ERR"
}

release_count() {
    find "$ROOT/releases" -mindepth 1 -maxdepth 1 -type d 2>/dev/null | wc -l | tr -d ' '
}

OUT="$TMP/out.txt"
ERR="$TMP/err.txt"

# ── (a) install activates and repoints current ───────────────
reset_fixture
STAGED="$TMP/staged-a"
make_binary "$STAGED" 0
if run_install "v1" "$STAGED"; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(a) install exits 0 on a healthy activation"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/v1" "(a) current points at the new release"
assert_current_exists "(a) current is not dangling"
assert_eq "$(release_count)" "1" "(a) a single release dir exists"
assert_grep "$FIXTURE/systemctl.log" "daemon-reload" "(a) systemctl daemon-reload was called"
assert_grep "$FIXTURE/systemctl.log" "restart lightspeed-proxy" "(a) systemctl restart was called"

# ── (b) failing health gate restores previous and exits non-zero ──
reset_fixture
seed_release "v0" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/v0" "$ROOT/current"
printf 'ok\n' > "$FIXTURE/health_mode"
printf 'v1\n' > "$FIXTURE/fail_versions"
STAGED_B="$TMP/staged-b"
make_binary "$STAGED_B" 0
if run_install "v1" "$STAGED_B"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(b) unhealthy activation exits non-zero"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/v0" "(b) current restored to the previous release"
assert_current_exists "(b) current is not dangling after rollback"
assert_eq "$(cat "$FIXTURE/health_mode")" "ok" "(b) service healthy again on the previous release"
assert_grep "$OUT" "rolling back" "(b) rollback is reported"

# ── (c) pruning keeps 3 and never removes the active release ──
reset_fixture
seed_release "v0" "2020-01-01 00:00:00"
seed_release "v1" "2020-02-01 00:00:00"
seed_release "v2" "2020-03-01 00:00:00"
seed_release "v3" "2020-04-01 00:00:00"
ln -sfn "$ROOT/releases/v3" "$ROOT/current"
STAGED_C="$TMP/staged-c"
make_binary "$STAGED_C" 0
if run_install "v4" "$STAGED_C"; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(c) install with pruning exits 0"
assert_eq "$(release_count)" "3" "(c) exactly 3 release dirs remain"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/v4" "(c) current points at the newest release"
assert_current_exists "(c) active release was not pruned"
if [ -d "$ROOT/releases/v0" ] || [ -d "$ROOT/releases/v1" ]; then
    note_fail "(c) oldest releases were not pruned"
else
    note_pass
fi
if [ -d "$ROOT/releases/v2" ] && [ -d "$ROOT/releases/v3" ]; then
    note_pass
else
    note_fail "(c) newest releases were pruned unexpectedly"
fi

# ── (d) running twice is idempotent ──────────────────────────
reset_fixture
STAGED_D="$TMP/staged-d"
make_binary "$STAGED_D" 0
if run_install "v1" "$STAGED_D"; then rc1=0; else rc1=$?; fi
FIRST_TARGET="$(readlink "$ROOT/current" 2>/dev/null || true)"
FIRST_COUNT="$(release_count)"
if run_install "v1" "$STAGED_D"; then rc2=0; else rc2=$?; fi
assert_rc_zero "$rc1" "(d) first install exits 0"
assert_rc_zero "$rc2" "(d) second install exits 0"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$FIRST_TARGET" "(d) current unchanged on re-run"
assert_eq "$(release_count)" "$FIRST_COUNT" "(d) release count unchanged on re-run"
assert_current_exists "(d) current is not dangling after re-run"

# ── (e) --check failure aborts and leaves no fresh release ───
reset_fixture
seed_release "v1" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/v1" "$ROOT/current"
STAGED_E="$TMP/staged-e"
make_binary "$STAGED_E" 1   # --check exits 1
if run_install "v9" "$STAGED_E"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(e) failed --check exits non-zero"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/v1" "(e) current unchanged after --check failure"
assert_current_exists "(e) current is not dangling after --check failure"
if [ -d "$ROOT/releases/v9" ]; then
    note_fail "(e) failed release dir was left behind"
else
    note_pass
fi

# ── (f) non-ELF staged file is rejected before any layout change ──
reset_fixture
seed_release "v1" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/v1" "$ROOT/current"
: > "$FIXTURE/file_not_elf"
STAGED_F="$TMP/staged-f"
printf 'not a binary\n' > "$STAGED_F"
chmod +x "$STAGED_F"
if run_install "vbad" "$STAGED_F"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(f) non-ELF file is rejected"
assert_grep "$ERR" "not an ELF" "(f) rejection names the ELF problem"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/v1" "(f) current untouched by rejected file"
assert_current_exists "(f) current is not dangling after rejection"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
    printf 'relay-install: all assertions passed\n'
    printf '  (%s checks)\n' "$PASS"
    exit 0
fi

printf 'relay-install: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
