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
#   (g) a same-version reinstall (with --force) whose --check fails leaves the
#       active binary byte-identical and exits non-zero
#   (h) a same-version reinstall (with --force) whose --check passes replaces
#       the binary
#   (i) no `.staged.*` temp files remain after either path
#   (j) a supported handoff writes the request, sends SIGUSR2, soaks the new
#       version, and only then repoints `current` and prunes
#   (k) /health without a `handoff` field falls back to the restart path
#   (l) a handoff that never happens leaves `current` on the old release and
#       exits non-zero without a restart (zero downtime)
#   (m) a handoff that comes up then fails the soak rolls back and restarts
#   (n) a same-version install (with --force) never hands off (the proxy
#       refuses it)
#   (o) --no-handoff forces the restart path even when handoff is available
#   (p) --handoff refuses to signal a proxy that does not advertise support
#   (q) a same-version install without --force is a clean no-op: exits 0,
#       restarts nothing, writes no handoff request, and touches no bytes
#   (r) --force reinstalls the same version through the normal activation path
#   (s) --public-ip is inserted into a config that lacks it
#   (t) --public-ip replaces an existing value
#   (u) an invalid --public-ip aborts before any config change
#   (v) --updater installs the self-updater to LIGHTSPEED_UPDATER_DEST
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
HANDOFF_REQ="$TMP/run/handoff-request.json"
UPDATER_DEST="$TMP/usr/local/lib/lightspeed/relay-updater.sh"
SHA_BIN="${LIGHTSPEED_SHA256SUM:-sha256sum}"
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

# ── Stub curl ────────────────────────────────────────────────
# Default mode honours `health_mode` (written by the stubbed restart) and
# returns a legacy body with no `handoff` field. When `curl_script` exists it
# is a one-directive-per-call sequence (last line repeats): FAIL, OLD,
# OLD_NOSUP, or NEWOK. NEWOK reflects the handoff request back through
# /health so the installer can match the id it generated.
cat > "$BIN_DIR/curl" <<STUB
#!/usr/bin/env bash
F="$FIXTURE"
ROOT="$ROOT"
REQ="$HANDOFF_REQ"
printf '%s\n' "\$(readlink "\$ROOT/current" 2>/dev/null || true)" >> "\$F/curl-current.log"

if [ -f "\$F/curl_script" ]; then
    n="\$(cat "\$F/curl_calls" 2>/dev/null || printf 0)"
    n=\$((n + 1))
    printf '%s\n' "\$n" > "\$F/curl_calls"
    mode="\$(sed -n "\${n}p" "\$F/curl_script")"
    [ -n "\$mode" ] || mode="\$(tail -1 "\$F/curl_script")"
else
    mode="LEGACY"
fi

oldver="\$(cat "\$F/old_version" 2>/dev/null || printf 1.4.3)"
req_field() { [ -f "\$REQ" ] && grep -oE "\"\$1\":\"[^\"]*\"" "\$REQ" | head -1 | cut -d'"' -f4 || true; }

emit_old() {
    printf '{"status":"healthy","version":"%s","node_id":"fixture","uptime_secs":1,"handoff":{"supported":%s,"last":null}}\n' "\$oldver" "\$1"
}
emit_newok() {
    local v i
    v="\$(req_field version)"
    i="\$(req_field handoff_id)"
    printf '%s|%s\n' "\$i" "\$v" >> "\$F/handoff-echo.log"
    printf '{"status":"healthy","version":"%s","node_id":"fixture","uptime_secs":1,"handoff":{"supported":true,"last":{"schema_version":1,"supported":true,"handoff_id":"%s","from_version":"%s","to_version":"%s","result":"ok","sessions_transferred":2,"at_unix_ms":1,"error":null}}}\n' "\$v" "\$i" "\$oldver" "\$v"
}

case "\$mode" in
    FAIL) exit 1 ;;
    OLD) emit_old true; exit 0 ;;
    OLD_NOSUP) emit_old false; exit 0 ;;
    NEWOK) emit_newok; exit 0 ;;
    LEGACY)
        m="\$(cat "\$F/health_mode" 2>/dev/null || printf ok)"
        [ "\$m" = "ok" ] || exit 1
        printf '{"status":"healthy","version":"%s","node_id":"fixture","uptime_secs":1}\n' "\$oldver"
        exit 0
        ;;
    *) exit 1 ;;
esac
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
    rm -rf "$ROOT" "$TMP/run"
    mkdir -p "$ROOT/releases"
    : > "$FIXTURE/systemctl.log"
    rm -f "$FIXTURE/fail_versions" "$FIXTURE/file_not_elf" "$FIXTURE/restart_fail" \
        "$FIXTURE/curl_script" "$FIXTURE/curl_calls" "$FIXTURE/curl-current.log" \
        "$FIXTURE/handoff-echo.log" "$FIXTURE/old_version"
    printf 'ok\n' > "$FIXTURE/health_mode"
}

# make_binary <path> <check_rc>
make_binary() {
    cat > "$1" <<STUB
#!/usr/bin/env bash
if [ "\${1:-}" = "--check" ]; then exit $2; fi
if [ "\${1:-}" = "--handoff-schema" ]; then echo 1; exit 0; fi
echo "lightspeed-proxy 1.4.4"
exit 0
STUB
    chmod +x "$1"
}

# make_binary_marked <path> <check_rc> <marker>
# Distinct bytes (the marker comment) so "left byte-identical" is meaningful.
make_binary_marked() {
    cat > "$1" <<STUB
#!/usr/bin/env bash
# $3
if [ "\${1:-}" = "--check" ]; then exit $2; fi
if [ "\${1:-}" = "--handoff-schema" ]; then echo 1; exit 0; fi
echo "lightspeed-proxy 1.4.4"
exit 0
STUB
    chmod +x "$1"
}

assert_no_staged() {
    local found
    found="$(find "$ROOT/releases" -name '*.staged.*' -print 2>/dev/null || true)"
    if [ -z "$found" ]; then note_pass; else
        note_fail "$1 (staged temp files remain: $found)"
    fi
}

assert_same_bytes() {
    if cmp -s "$1" "$2"; then note_pass; else note_fail "$3 (bytes differ)"; fi
}

assert_diff_bytes() {
    if cmp -s "$1" "$2"; then note_fail "$3 (bytes unexpectedly identical)"; else note_pass; fi
}

assert_not_grep() {
    local file="$1" needle="$2" msg="$3"
    if grep -q -- "$needle" "$file" 2>/dev/null; then
        note_fail "$msg ('$needle' present in $file)"
    else
        note_pass
    fi
}

assert_file_exists() {
    if [ -f "$1" ]; then note_pass; else note_fail "$2 (missing $1)"; fi
}

# seed_release <version> <touch-date>
seed_release() {
    mkdir -p "$ROOT/releases/$1"
    make_binary "$ROOT/releases/$1/lightspeed-proxy" 0
    touch -d "$2" "$ROOT/releases/$1" "$ROOT/releases/$1/lightspeed-proxy"
}

# run_install <version> <binary-path> [extra installer flags...]
run_install() {
    env \
        LIGHTSPEED_LAYOUT_ROOT="$ROOT" \
        LIGHTSPEED_SYSTEMCTL="$BIN_DIR/systemctl" \
        LIGHTSPEED_CURL="$BIN_DIR/curl" \
        LIGHTSPEED_FILE_BIN="$BIN_DIR/file" \
        LIGHTSPEED_SHA256SUM="$SHA_BIN" \
        LIGHTSPEED_HANDOFF_REQUEST="$HANDOFF_REQ" \
        LIGHTSPEED_HANDOFF_SOAK_SECS="${SOAK_SECS:-1}" \
        LIGHTSPEED_HOST_ARCH="x86_64" \
        LIGHTSPEED_HEALTH_TIMEOUT="1" \
        LIGHTSPEED_HEALTH_INTERVAL="0" \
        LIGHTSPEED_SERVICE_NAME="lightspeed-proxy" \
        LIGHTSPEED_CONFIG_PATH="$FIXTURE/proxy.toml" \
        LIGHTSPEED_UPDATER_DEST="$UPDATER_DEST" \
        LIGHTSPEED_KEEP_RELEASES="3" \
        LIGHTSPEED_SLEEP_BIN="true" \
        bash "$INSTALLER" --binary "$2" --version "$1" "${@:3}" >"$OUT" 2>"$ERR"
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

# ── (g) same-version reinstall with failing --check keeps the active binary ──
reset_fixture
seed_release "v1" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/v1" "$ROOT/current"
ACTIVE_BIN_G="$ROOT/releases/v1/lightspeed-proxy"
cp "$ACTIVE_BIN_G" "$TMP/active-before-g"
STAGED_G="$TMP/staged-g"
make_binary_marked "$STAGED_G" 1 "g marker: check fails"
assert_diff_bytes "$TMP/active-before-g" "$STAGED_G" "(g) staged binary differs from active"
if run_install "v1" "$STAGED_G" --force; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(g) same-version failed --check exits non-zero"
assert_same_bytes "$TMP/active-before-g" "$ACTIVE_BIN_G" \
    "(g) active binary is byte-identical after failed same-version reinstall"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/v1" \
    "(g) current unchanged after failed same-version reinstall"
assert_current_exists "(g) current is not dangling"
assert_no_staged "(g) no staged temp file remains"

# ── (h) same-version reinstall with passing --check replaces the binary ──
reset_fixture
seed_release "v1" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/v1" "$ROOT/current"
ACTIVE_BIN_H="$ROOT/releases/v1/lightspeed-proxy"
cp "$ACTIVE_BIN_H" "$TMP/active-before-h"
STAGED_H="$TMP/staged-h"
make_binary_marked "$STAGED_H" 0 "h marker: check passes"
if run_install "v1" "$STAGED_H" --force; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(h) same-version passing --check exits 0"
assert_diff_bytes "$TMP/active-before-h" "$ACTIVE_BIN_H" \
    "(h) active binary was replaced by the same-version reinstall"
assert_same_bytes "$STAGED_H" "$ACTIVE_BIN_H" \
    "(h) active binary matches the staged binary"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/v1" \
    "(h) current still points at v1"
assert_current_exists "(h) current is not dangling"
assert_eq "$(cat "$FIXTURE/health_mode")" "ok" "(h) service healthy after same-version reinstall"
assert_no_staged "(h) no staged temp file remains"

# ── (j) handoff success: request, SIGUSR2, soak, then repoint + prune ──
reset_fixture
seed_release "1.4.1" "2020-01-01 00:00:00"
seed_release "1.4.2" "2020-02-01 00:00:00"
seed_release "1.4.3" "2020-03-01 00:00:00"
ln -sfn "$ROOT/releases/1.4.3" "$ROOT/current"
printf '1.4.3\n' > "$FIXTURE/old_version"
printf 'OLD\nNEWOK\n' > "$FIXTURE/curl_script"
STAGED_J="$TMP/staged-j"
make_binary "$STAGED_J" 0
SOAK_SECS=1
if run_install "1.4.4" "$STAGED_J"; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(j) handoff install exits 0"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.4.4" \
    "(j) current repointed to the handoff release"
assert_current_exists "(j) current is not dangling"
assert_eq "$(release_count)" "3" "(j) prune kept 3 releases after handoff"
assert_grep "$FIXTURE/systemctl.log" "kill -s SIGUSR2 lightspeed-proxy" "(j) SIGUSR2 was sent"
assert_not_grep "$FIXTURE/systemctl.log" "restart lightspeed-proxy" "(j) no restart on a successful handoff"

assert_file_exists "$HANDOFF_REQ" "(j) handoff request was written"
assert_eq "$(stat -c %a "$HANDOFF_REQ" 2>/dev/null || true)" "600" "(j) handoff request mode is 0600"
assert_grep "$HANDOFF_REQ" '"schema_version":1' "(j) request schema_version is 1"
assert_grep "$HANDOFF_REQ" '"version":"1.4.4"' "(j) request carries the new version"
assert_grep "$HANDOFF_REQ" "\"binary_path\":\"$ROOT/releases/1.4.4/lightspeed-proxy\"" \
    "(j) request carries the release binary path"
REQ_SHA_J="$("$SHA_BIN" "$ROOT/releases/1.4.4/lightspeed-proxy" | awk '{print $1}')"
assert_grep "$HANDOFF_REQ" "\"sha256\":\"$REQ_SHA_J\"" "(j) request carries the released binary sha256"
REQ_ID_J="$(grep -oE '"handoff_id":"[^"]*"' "$HANDOFF_REQ" 2>/dev/null | head -1 | cut -d'"' -f4 || true)"
if [ -n "$REQ_ID_J" ]; then
    note_pass
    assert_grep "$FIXTURE/handoff-echo.log" "$REQ_ID_J" "(j) /health echoed the request handoff_id"
else
    note_fail "(j) request has no handoff_id"
fi
assert_eq "$(tail -1 "$FIXTURE/curl-current.log" 2>/dev/null || true)" "$ROOT/releases/1.4.3" \
    "(j) current still pointed at the old release throughout the ok + soak"
if [ -f "$FIXTURE/handoff-echo.log" ] && [ "$(wc -l < "$FIXTURE/handoff-echo.log" | tr -d ' ')" -ge 2 ]; then
    note_pass
else
    note_fail "(j) the soak polled /health more than once"
fi

# ── (k) no handoff field -> restart path, no request ─────────
reset_fixture
seed_release "1.4.3" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/1.4.3" "$ROOT/current"
STAGED_K="$TMP/staged-k"
make_binary "$STAGED_K" 0
SOAK_SECS=0
if run_install "1.4.4" "$STAGED_K"; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(k) health without handoff falls back to restart and exits 0"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.4.4" \
    "(k) restart path repointed current"
assert_grep "$FIXTURE/systemctl.log" "restart lightspeed-proxy" "(k) restart path was used"
assert_not_grep "$FIXTURE/systemctl.log" "SIGUSR2" "(k) no handoff signal was sent"
if [ -e "$HANDOFF_REQ" ]; then
    note_fail "(k) handoff request was written on the restart path"
else
    note_pass
fi

# ── (l) handoff never happens -> no repoint, no restart, non-zero ──
reset_fixture
seed_release "1.4.3" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/1.4.3" "$ROOT/current"
printf '1.4.3\n' > "$FIXTURE/old_version"
printf 'OLD\n' > "$FIXTURE/curl_script"
STAGED_L="$TMP/staged-l"
make_binary "$STAGED_L" 0
SOAK_SECS=0
if run_install "1.4.4" "$STAGED_L"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(l) a handoff that never completes exits non-zero"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.4.3" \
    "(l) current unchanged when the handoff never happened"
assert_current_exists "(l) current is not dangling"
assert_grep "$FIXTURE/systemctl.log" "kill -s SIGUSR2 lightspeed-proxy" "(l) SIGUSR2 was attempted"
assert_not_grep "$FIXTURE/systemctl.log" "restart lightspeed-proxy" \
    "(l) no restart on a zero-downtime handoff miss"

# ── (m) handoff ok then soak fails -> rollback + restart ─────
reset_fixture
seed_release "1.4.3" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/1.4.3" "$ROOT/current"
printf '1.4.3\n' > "$FIXTURE/old_version"
printf 'OLD\nNEWOK\nFAIL\n' > "$FIXTURE/curl_script"
STAGED_M="$TMP/staged-m"
make_binary "$STAGED_M" 0
SOAK_SECS=1
if run_install "1.4.4" "$STAGED_M"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(m) a failed soak exits non-zero"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.4.3" \
    "(m) current rolled back to the previous release"
assert_current_exists "(m) current is not dangling after rollback"
assert_grep "$OUT" "rolling back" "(m) rollback is reported"
assert_grep "$FIXTURE/systemctl.log" "restart lightspeed-proxy" "(m) rollback restarted the service"

# ── (n) same-version install never hands off ─────────────────
reset_fixture
seed_release "1.4.4" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/1.4.4" "$ROOT/current"
printf '1.4.4\n' > "$FIXTURE/old_version"
printf 'OLD\n' > "$FIXTURE/curl_script"
STAGED_N="$TMP/staged-n"
make_binary_marked "$STAGED_N" 0 "n marker: same version"
SOAK_SECS=0
if run_install "1.4.4" "$STAGED_N" --force; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(n) same-version install exits 0"
assert_not_grep "$FIXTURE/systemctl.log" "SIGUSR2" "(n) same version sends no SIGUSR2"
if [ -e "$HANDOFF_REQ" ]; then
    note_fail "(n) same version wrote a handoff request"
else
    note_pass
fi
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.4.4" \
    "(n) current still on the same release"

# ── (o) --no-handoff forces the restart path ─────────────────
reset_fixture
seed_release "1.4.3" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/1.4.3" "$ROOT/current"
printf '1.4.3\n' > "$FIXTURE/old_version"
printf 'OLD\nNEWOK\n' > "$FIXTURE/curl_script"
STAGED_O="$TMP/staged-o"
make_binary "$STAGED_O" 0
SOAK_SECS=0
if run_install "1.4.4" "$STAGED_O" --no-handoff; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(o) --no-handoff install exits 0"
assert_grep "$FIXTURE/systemctl.log" "restart lightspeed-proxy" "(o) --no-handoff restarts"
assert_not_grep "$FIXTURE/systemctl.log" "SIGUSR2" "(o) --no-handoff sends no SIGUSR2"
if [ -e "$HANDOFF_REQ" ]; then
    note_fail "(o) --no-handoff wrote a handoff request"
else
    note_pass
fi
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.4.4" \
    "(o) --no-handoff repointed current"

# ── (p) --handoff refuses to signal an unsupported proxy ─────
reset_fixture
seed_release "1.4.3" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/1.4.3" "$ROOT/current"
STAGED_P="$TMP/staged-p"
make_binary "$STAGED_P" 0
SOAK_SECS=0
if run_install "1.4.4" "$STAGED_P" --handoff; then rc=0; else rc=$?; fi
assert_eq "$rc" "2" "(p) --handoff without proxy support exits 2"
assert_not_grep "$FIXTURE/systemctl.log" "SIGUSR2" \
    "(p) --handoff sends no SIGUSR2 to an unsupported proxy"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.4.3" \
    "(p) --handoff leaves current untouched when it cannot proceed"

# ── (q) same-version install is a clean no-op ────────────────
reset_fixture
seed_release "1.5.0" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/1.5.0" "$ROOT/current"
printf '1.5.0\n' > "$FIXTURE/old_version"
ACTIVE_BIN_Q="$ROOT/releases/1.5.0/lightspeed-proxy"
cp "$ACTIVE_BIN_Q" "$TMP/active-before-q"
STAGED_Q="$TMP/staged-q"
make_binary_marked "$STAGED_Q" 0 "q marker: same-version no-op"
if run_install "1.5.0" "$STAGED_Q"; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(q) same-version install exits 0"
assert_same_bytes "$TMP/active-before-q" "$ACTIVE_BIN_Q" \
    "(q) active binary untouched by the no-op"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.5.0" \
    "(q) current unchanged by the no-op"
assert_not_grep "$FIXTURE/systemctl.log" "restart lightspeed-proxy" \
    "(q) no systemd restart on the no-op"
assert_not_grep "$FIXTURE/systemctl.log" "daemon-reload" \
    "(q) no daemon-reload on the no-op"
assert_not_grep "$FIXTURE/systemctl.log" "SIGUSR2" \
    "(q) no handoff signal on the no-op"
if [ -e "$HANDOFF_REQ" ]; then
    note_fail "(q) no-op wrote a handoff request"
else
    note_pass
fi
assert_grep "$OUT" "already at 1.5.0; nothing to do" "(q) no-op is reported"
assert_no_staged "(q) no staged temp file remains"

# ── (r) --force reinstalls the same version through activation ──
reset_fixture
seed_release "1.5.0" "2020-01-01 00:00:00"
ln -sfn "$ROOT/releases/1.5.0" "$ROOT/current"
ACTIVE_BIN_R="$ROOT/releases/1.5.0/lightspeed-proxy"
cp "$ACTIVE_BIN_R" "$TMP/active-before-r"
STAGED_R="$TMP/staged-r"
make_binary_marked "$STAGED_R" 0 "r marker: forced same-version reinstall"
SOAK_SECS=0
if run_install "1.5.0" "$STAGED_R" --force; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(r) --force same-version install exits 0"
assert_diff_bytes "$TMP/active-before-r" "$ACTIVE_BIN_R" \
    "(r) --force replaced the active binary"
assert_same_bytes "$STAGED_R" "$ACTIVE_BIN_R" \
    "(r) active binary matches the forced staged binary"
assert_grep "$FIXTURE/systemctl.log" "restart lightspeed-proxy" \
    "(r) --force proceeds through the restart activation"
assert_eq "$(readlink "$ROOT/current" 2>/dev/null || true)" "$ROOT/releases/1.5.0" \
    "(r) current still on the release"
assert_no_staged "(r) no staged temp file remains"

# ── (s) --public-ip is inserted into a config without it ─────
reset_fixture
printf '[server]\nnode_id = "x"\n\n[metrics]\nenabled = true\n' > "$FIXTURE/proxy.toml"
STAGED_S="$TMP/staged-s"
make_binary "$STAGED_S" 0
if run_install "v1" "$STAGED_S" --public-ip "203.0.113.7"; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(s) install with --public-ip exits 0"
assert_grep "$FIXTURE/proxy.toml" 'public_ip = "203.0.113.7"' "(s) public_ip is inserted"
assert_eq "$(grep -c 'public_ip' "$FIXTURE/proxy.toml")" "1" "(s) public_ip appears exactly once"

# ── (t) --public-ip replaces an existing value ───────────────
reset_fixture
printf '[server]\npublic_ip = "198.51.100.1"\n' > "$FIXTURE/proxy.toml"
STAGED_T="$TMP/staged-t"
make_binary "$STAGED_T" 0
if run_install "v1" "$STAGED_T" --public-ip "203.0.113.9"; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(t) install with --public-ip replacing exits 0"
assert_grep "$FIXTURE/proxy.toml" 'public_ip = "203.0.113.9"' "(t) public_ip is replaced"
assert_not_grep "$FIXTURE/proxy.toml" '198.51.100.1' "(t) the old address is gone"
assert_eq "$(grep -c 'public_ip' "$FIXTURE/proxy.toml")" "1" "(t) public_ip appears exactly once"

# ── (u) an invalid --public-ip is rejected before any change ──
reset_fixture
printf '[server]\nnode_id = "x"\n' > "$FIXTURE/proxy.toml"
STAGED_U="$TMP/staged-u"
make_binary "$STAGED_U" 0
if run_install "v1" "$STAGED_U" --public-ip "not-an-ip"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(u) invalid --public-ip exits non-zero"
assert_not_grep "$FIXTURE/proxy.toml" 'public_ip' "(u) config is untouched on an invalid address"

# ── (v) --updater installs the self-updater script ───────────
reset_fixture
printf '[server]\n' > "$FIXTURE/proxy.toml"
UPDATER_SRC="$TMP/updater-src.sh"
printf '#!/usr/bin/env bash\necho updater\n' > "$UPDATER_SRC"
STAGED_V="$TMP/staged-v"
make_binary "$STAGED_V" 0
rm -f "$UPDATER_DEST"
if run_install "v1" "$STAGED_V" --updater "$UPDATER_SRC"; then rc=0; else rc=$?; fi
assert_rc_zero "$rc" "(v) install with --updater exits 0"
assert_file_exists "$UPDATER_DEST" "(v) updater is installed at the destination"
assert_same_bytes "$UPDATER_SRC" "$UPDATER_DEST" "(v) installed updater matches the source"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
    printf 'relay-install: all assertions passed\n'
    printf '  (%s checks)\n' "$PASS"
    exit 0
fi

printf 'relay-install: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
