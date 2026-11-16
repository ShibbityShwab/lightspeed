#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Offline self-test for relay-control-alpn.sh
#
# Drives the real flip script as a subprocess against temp fixtures with
# ssh, curl, systemctl and lightspeed-proxy stubs, so no network, no real
# ssh and no service state are touched:
#   - the ssh stub drops the connection arguments, records the remote
#     command, and runs it locally, so the remote edit / --check / restore
#     steps operate on the fixture config;
#   - the curl stub serves the fixture /health JSON, switching to the
#     "after restart" body once a restart was recorded (that is how the
#     bounded health gate is exercised);
#   - the systemctl stub records restarts and can fail on demand;
#   - the lightspeed-proxy stub answers --check and can fail on demand.
#
# This is the verifier for the POSIX scripts on a Windows dev host (they
# cannot run there); it runs for real in CI's "Infra Script Tests" job.
#
# Asserted behaviours:
#   (a) sourceable, syntax-checkable, --help and unknown options
#   (b) adds the key inside an existing [network] section, before the next
#       section, atomically, keeping one timestamped backup of the original
#   (c) appends a [network] section when the config has none
#   (d) replaces an existing key; a commented key is not an active key
#   (e) idempotent: the mode already in /health changes nothing
#   (f) rolls back when /health never reports the new mode
#   (g) rolls back when the deployed binary's --check fails (no restart)
#   (h) rolls back when the restart fails
#   (i) refuses when /health has no control_alpn (stage-2 not deployed)
#   (j) refuses an unknown mode and an unresolvable node
#   (k) --dry-run changes nothing
#   (l) --ip overrides the resolved address, including when the inventory
#       is unavailable
#   (m) refuses when /health is unreachable
#   (n) the config file is byte-identical after every refused or rolled
#       back run
#
# Usage: bash infra/scripts/test_relay_control_alpn.sh
# Exits 0 and prints "relay-control-alpn: all assertions passed" on success.
# Requires: bash, jq, coreutils. No network access.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SCRIPT="$SCRIPT_DIR/relay-control-alpn.sh"

if [ ! -f "$SCRIPT" ]; then
	printf 'relay-control-alpn-test: FAIL - script not found: %s\n' "$SCRIPT" >&2
	exit 1
fi
if ! command -v jq >/dev/null 2>&1; then
	printf 'relay-control-alpn-test: FAIL - jq is required\n' >&2
	exit 1
fi

FIXTURE="$(mktemp -d)"
trap 'rm -rf "$FIXTURE"' EXIT

BIN_DIR="$FIXTURE/bin"
mkdir -p "$BIN_DIR"
CONFIG="$FIXTURE/proxy.toml"
CONFIG_ORIG="$FIXTURE/proxy.toml.orig"
HEALTH_BEFORE="$FIXTURE/health-before.json"
HEALTH_AFTER="$FIXTURE/health-after.json"
SSH_LOG="$FIXTURE/ssh.log"
SYSTEMCTL_LOG="$FIXTURE/systemctl.log"
CHECK_LOG="$FIXTURE/check.log"
KEY="$FIXTURE/id_lightspeed_deploy"
OUT="$FIXTURE/out.txt"
ERR="$FIXTURE/err.txt"

PASS=0
FAILURES=0
note_pass() { PASS=$((PASS + 1)); }
note_fail() {
	printf '  FAIL: %s\n' "$1" >&2
	FAILURES=$((FAILURES + 1))
}

# ── Assertions ───────────────────────────────────────────────
assert_rc_zero() {
	if [ "$1" -eq 0 ]; then note_pass; else note_fail "$2 (exit was $1)"; fi
}
assert_rc_nonzero() {
	if [ "$1" -ne 0 ]; then note_pass; else note_fail "$2 (exit was 0)"; fi
}
assert_eq() {
	if [ "$1" = "$2" ]; then note_pass; else note_fail "$3 (got '$1', expected '$2')"; fi
}
assert_contains() {
	case "$1" in
	*"$2"*) note_pass ;;
	*) note_fail "$3 (missing '$2' in output)" ;;
	esac
}
assert_not_contains() {
	case "$1" in
	*"$2"*) note_fail "$3 (unexpected '$2' in output)" ;;
	*) note_pass ;;
	esac
}
assert_grep() { # assert_grep <file> <literal> <msg>
	if grep -qF -- "$2" "$1" 2>/dev/null; then note_pass; else
		note_fail "$3 (missing '$2' in $(basename "$1"))"
	fi
}
assert_not_grep() {
	if grep -qF -- "$2" "$1" 2>/dev/null; then
		note_fail "$3 (unexpected '$2' in $(basename "$1"))"
	else
		note_pass
	fi
}
assert_file_exists() {
	if [ -f "$1" ]; then note_pass; else note_fail "$2 (missing $1)"; fi
}
assert_config_unchanged() {
	if cmp -s "$CONFIG" "$CONFIG_ORIG"; then note_pass; else
		note_fail "$1 (config bytes changed)"
	fi
}
assert_config_changed() {
	if cmp -s "$CONFIG" "$CONFIG_ORIG"; then note_fail "$1 (config bytes unchanged)"; else
		note_pass
	fi
}
assert_active_key_count() { # exactly one active (uncommented) control_alpn line
	local got
	got="$(grep -cE '^[[:space:]]*control_alpn[[:space:]]*=' "$CONFIG" 2>/dev/null || true)"
	[ -n "$got" ] || got=0
	assert_eq "$got" "$1" "$2"
}
assert_no_active_value() { # assert_no_active_value <literal value> <msg>
	if grep -qE "^[[:space:]]*control_alpn[[:space:]]*=[[:space:]]*\"?$1\"?[[:space:]]*$" "$CONFIG" 2>/dev/null; then
		note_fail "$2 (active value '$1' is still present)"
	else
		note_pass
	fi
}
assert_line_empty() {
	if [ ! -s "$1" ]; then note_pass; else
		note_fail "$2 ($(basename "$1") is not empty)"
	fi
}
backup_count() {
	local b n=0
	for b in "$CONFIG".bak.*; do
		[ -e "$b" ] && n=$((n + 1))
	done
	printf '%s' "$n"
}
backup_path() {
	local b
	for b in "$CONFIG".bak.*; do
		if [ -e "$b" ]; then
			printf '%s' "$b"
			return 0
		fi
	done
	return 1
}
section_of_key() {
	awk '/^[[:space:]]*\[/{ section = $0 } /^[[:space:]]*control_alpn[[:space:]]*=/{ print section; exit }' "$CONFIG"
}
line_of_key() {
	awk '/^[[:space:]]*control_alpn[[:space:]]*=/{ print NR; exit }' "$CONFIG"
}
line_of_header() { # line_of_header <header literal>
	awk -v h="$1" '$0 == h { print NR; exit }' "$CONFIG"
}

# ── Stubs ────────────────────────────────────────────────────
write_stubs() {
	cat >"$BIN_DIR/ssh" <<'STUB'
#!/usr/bin/env bash
# Test stub for ssh: drop connection options and the destination, record the
# remote command, and run it locally so the remote steps act on the fixture.
set -uo pipefail
: "${ALPN_TEST_FIXTURE:?}"
dest=""
while [ "$#" -gt 1 ]; do
	case "$1" in
	-o | -i | -p | -l | -F | -E) shift 2 ;;
	-*) shift ;;
	*) dest="$1"; shift ;;
	esac
done
printf 'SSH dest=%s\n' "$dest" >>"$ALPN_TEST_FIXTURE/ssh.log"
printf 'CMD: %s\n' "$1" >>"$ALPN_TEST_FIXTURE/ssh.log"
exec bash -c "$1"
STUB

	cat >"$BIN_DIR/curl" <<'STUB'
#!/usr/bin/env bash
# Test stub for curl: serve the fixture /health body. Once a restart has been
# recorded, serve the "after restart" body instead, which is how the bounded
# /health gate sees either the new mode or a relay that never picked it up.
set -uo pipefail
: "${ALPN_TEST_FIXTURE:?}"
if [ -f "$ALPN_TEST_FIXTURE/curl_unreachable" ]; then
	exit 7
fi
if [ -s "$ALPN_TEST_FIXTURE/systemctl.log" ] && [ -f "$ALPN_TEST_FIXTURE/health-after.json" ]; then
	cat "$ALPN_TEST_FIXTURE/health-after.json"
else
	cat "$ALPN_TEST_FIXTURE/health-before.json"
fi
STUB

	cat >"$BIN_DIR/systemctl" <<'STUB'
#!/usr/bin/env bash
# Test stub for systemctl: record the call and fail only when asked to.
set -uo pipefail
: "${ALPN_TEST_FIXTURE:?}"
printf 'systemctl %s\n' "$*" >>"$ALPN_TEST_FIXTURE/systemctl.log"
if [ -f "$ALPN_TEST_FIXTURE/restart_fail" ]; then
	exit 1
fi
exit 0
STUB

	cat >"$BIN_DIR/lightspeed-proxy" <<'STUB'
#!/usr/bin/env bash
# Test stub for the deployed relay binary: record --check and fail on demand.
set -uo pipefail
: "${ALPN_TEST_FIXTURE:?}"
printf 'check %s\n' "$*" >>"$ALPN_TEST_FIXTURE/check.log"
if [ -f "$ALPN_TEST_FIXTURE/check_fail" ]; then
	exit 1
fi
exit 0
STUB

	chmod +x "$BIN_DIR/ssh" "$BIN_DIR/curl" "$BIN_DIR/systemctl" "$BIN_DIR/lightspeed-proxy"
}

# ── Fixtures ─────────────────────────────────────────────────
write_health() { # write_health <before|after> <off|required|none>
	local file
	case "$1" in
	before) file="$HEALTH_BEFORE" ;;
	after) file="$HEALTH_AFTER" ;;
	*) note_fail "write_health: bad slot '$1'"; return ;;
	esac
	case "$2" in
	off | required)
		printf '{"status":"ok","version":"1.11.0","control_alpn":"%s"}\n' "$2" >"$file"
		;;
	none)
		printf '{"status":"ok","version":"1.10.3"}\n' >"$file"
		;;
	*)
		note_fail "write_health: bad mode '$2'"
		return
		;;
	esac
}

reset_fixture() {
	local b
	rm -f "$SSH_LOG" "$SYSTEMCTL_LOG" "$CHECK_LOG" \
		"$FIXTURE/restart_fail" "$FIXTURE/check_fail" "$FIXTURE/curl_unreachable"
	for b in "$CONFIG".bak.*; do
		[ -e "$b" ] && rm -f "$b"
	done
	: >"$SSH_LOG"
	: >"$SYSTEMCTL_LOG"
	: >"$CHECK_LOG"
	: >"$KEY"
	write_health before off
	write_health after required
}

seed_config() { # seed_config <content>
	printf '%s' "$1" >"$CONFIG"
	cp -f "$CONFIG" "$CONFIG_ORIG"
}

NODES_JSON='{"alpha":{"ip":"10.0.0.1"},"beta":{"ip":"10.0.0.2"},"us_east":{"ip":"10.0.0.3"}}'

run_flip() { # run_flip <script args...>; sets RC and ALL
	env \
		ALPN_TEST_FIXTURE="$FIXTURE" \
		PATH="$BIN_DIR:$PATH" \
		LIGHTSPEED_NODES="$NODES_JSON" \
		LIGHTSPEED_REGISTRY_PATH="$FIXTURE/no-registry.json" \
		LIGHTSPEED_SSH="$BIN_DIR/ssh" \
		LIGHTSPEED_CURL="$BIN_DIR/curl" \
		LIGHTSPEED_CONFIG_PATH="$CONFIG" \
		LIGHTSPEED_PROXY_BINARY="$BIN_DIR/lightspeed-proxy" \
		LIGHTSPEED_SERVICE_NAME="lightspeed-proxy" \
		LIGHTSPEED_ALPN_HEALTH_TIMEOUT=1 \
		LIGHTSPEED_ALPN_HEALTH_INTERVAL=0 \
		DEPLOY_SSH_KEY="$KEY" \
		bash "$SCRIPT" "$@" >"$OUT" 2>"$ERR"
	RC=$?
	ALL="$(cat "$OUT" "$ERR")"
}

restart_count() {
	grep -c 'systemctl restart' "$SYSTEMCTL_LOG" 2>/dev/null || true
}

# ── Run ──────────────────────────────────────────────────────
write_stubs
reset_fixture

# ── (a) sourceability, syntax, --help, unknown option ────────
if bash -c 'source "$1"; declare -F relay_control_alpn_main >/dev/null' _ "$SCRIPT" 2>/dev/null; then
	note_pass
else
	note_fail "(a) script is not sourceable (no relay_control_alpn_main entry point)"
fi
if bash -n "$SCRIPT" 2>/dev/null; then note_pass; else note_fail "(a) bash -n rejects the script"; fi

seed_config '[server]
node_id = "alpha"
'
run_flip --help
assert_rc_zero "$RC" "(a) --help exits 0"
assert_contains "$ALL" "Usage:" "(a) --help prints usage"

run_flip --bogus
assert_rc_nonzero "$RC" "(a) an unknown option exits non-zero"
assert_contains "$ALL" "Unknown option" "(a) unknown option is named"
assert_config_unchanged "(a) config unchanged after an unknown option"

# ── (b) add the key inside an existing [network] section ─────
reset_fixture
seed_config '[server]
node_id = "alpha"

[network]
data_port = 4434

[metrics]
enabled = true
'
run_flip --node alpha --mode required
assert_rc_zero "$RC" "(b) flip to required exits 0"
assert_config_changed "(b) config was edited"
assert_contains "$ALL" "off -> required" "(b) summary names the mode transition"
assert_contains "$ALL" "http://10.0.0.1:8080/health" "(b) summary names the /health URL"
assert_contains "$ALL" "lightspeed_control_handshakes_total{outcome=\"rejected_no_application_protocol\"}" \
	"(b) summary names the §7.4 metric to watch"
assert_active_key_count 1 "(b) exactly one active control_alpn key"
assert_grep "$CONFIG" 'control_alpn = "required"' "(b) config carries the requested value"
assert_eq "$(section_of_key)" "[network]" "(b) the key is inside [network]"
if [ "$(line_of_key)" -lt "$(line_of_header '[metrics]')" ]; then note_pass; else
	note_fail "(b) the key was appended after [metrics] instead of inside [network]"
fi
assert_eq "$(backup_count)" "1" "(b) exactly one timestamped backup next to the config"
assert_file_exists "$(backup_path)" "(b) the backup file exists"
if cmp -s "$(backup_path)" "$CONFIG_ORIG"; then note_pass; else
	note_fail "(b) the backup does not hold the pre-flip config bytes"
fi
assert_eq "$(stat -c %a "$CONFIG")" "$(stat -c %a "$CONFIG_ORIG")" \
	"(b) the config mode is preserved (the service can still read it)"
assert_grep "$CHECK_LOG" "--check --config" "(b) the deployed binary --check ran before the restart"
assert_grep "$SYSTEMCTL_LOG" "restart lightspeed-proxy" "(b) the service was restarted"
assert_grep "$SSH_LOG" "dest=root@10.0.0.1" "(b) commands went to the resolved relay address"
assert_contains "$ALL" "flipped; /health reports control_alpn=required" "(b) success is reported"

# ── (c) append a [network] section when none exists ─────────
reset_fixture
seed_config '[server]
node_id = "alpha"
'
run_flip --node alpha --mode required
assert_rc_zero "$RC" "(c) flip to required with no [network] exits 0"
assert_grep "$CONFIG" "[network]" "(c) a [network] section was appended"
assert_grep "$CONFIG" 'control_alpn = "required"' "(c) the key was appended"
assert_grep "$CONFIG" 'node_id = "alpha"' "(c) the existing [server] content is preserved"
assert_eq "$(section_of_key)" "[network]" "(c) the appended key is under [network]"

# ── (d) replace an existing key; a comment is not a key ─────
reset_fixture
seed_config '[server]
node_id = "alpha"

[network]
# control_alpn = "off"
control_port = 4433
control_alpn = "off"

[metrics]
enabled = true
'
run_flip --node alpha --mode required
assert_rc_zero "$RC" "(d) flipping an existing key exits 0"
assert_active_key_count 1 "(d) the replaced key is not duplicated"
assert_grep "$CONFIG" 'control_alpn = "required"' "(d) the value was replaced"
assert_no_active_value "off" "(d) no active off value remains"
assert_grep "$CONFIG" '# control_alpn = "off"' "(d) the commented example is preserved"
assert_eq "$(section_of_key)" "[network]" "(d) the key stays under [network]"

# ── (e) idempotent: the mode already in effect ──────────────
reset_fixture
write_health before required
seed_config '[server]
node_id = "alpha"

[network]
control_alpn = "required"
'
run_flip --node alpha --mode required
assert_rc_zero "$RC" "(e) requesting the mode already in effect exits 0"
assert_contains "$ALL" "already in effect" "(e) the no-op is reported"
assert_config_unchanged "(e) the config is byte-identical"
assert_line_empty "$SSH_LOG" "(e) no remote command ran"
assert_line_empty "$SYSTEMCTL_LOG" "(e) the service was not restarted"
assert_line_empty "$CHECK_LOG" "(e) no --check ran"
assert_eq "$(backup_count)" "0" "(e) no backup was created"

# ── (f) roll back when /health never reports the new mode ───
reset_fixture
write_health before off
write_health after off
seed_config '[server]
node_id = "alpha"

[network]
control_alpn = "off"
'
run_flip --node alpha --mode required
assert_rc_nonzero "$RC" "(f) a health gate that never sees the new mode exits non-zero"
assert_contains "$ALL" "did not report control_alpn='required'" "(f) the timeout is reported"
assert_contains "$ALL" "Rolling back" "(f) the rollback is reported"
assert_contains "$ALL" "back on control_alpn=off" "(f) the rollback health gate proves the previous mode"
assert_config_unchanged "(f) the config is restored byte-identically"
assert_eq "$(backup_count)" "1" "(f) the backup is retained as evidence"
if [ "$(restart_count)" -ge 2 ]; then note_pass; else
	note_fail "(f) the rollback restart did not run (restarts: $(restart_count))"
fi

# ── (g) roll back when the pre-restart --check fails ────────
reset_fixture
seed_config '[server]
node_id = "alpha"

[network]
control_alpn = "off"
'
: >"$FIXTURE/check_fail"
run_flip --node alpha --mode required
assert_rc_nonzero "$RC" "(g) a failed --check exits non-zero"
assert_contains "$ALL" "--check failed" "(g) the failed --check is reported"
assert_config_unchanged "(g) the config is restored byte-identically"
assert_eq "$(backup_count)" "1" "(g) the backup is retained as evidence"
assert_grep "$CHECK_LOG" "--check --config" "(g) --check was executed"
assert_line_empty "$SYSTEMCTL_LOG" "(g) the service was never restarted"

# ── (h) roll back when the restart fails ────────────────────
reset_fixture
seed_config '[server]
node_id = "alpha"

[network]
control_alpn = "off"
'
: >"$FIXTURE/restart_fail"
run_flip --node alpha --mode required
assert_rc_nonzero "$RC" "(h) a failed restart exits non-zero"
assert_contains "$ALL" "Restart of lightspeed-proxy failed" "(h) the failed restart is reported"
assert_contains "$ALL" "Rollback restart failed" "(h) the failed rollback restart is reported"
assert_config_unchanged "(h) the config is restored byte-identically"
assert_grep "$SYSTEMCTL_LOG" "restart lightspeed-proxy" "(h) a restart was attempted"

# ── (i) refuse a relay without control_alpn in /health ──────
reset_fixture
write_health before none
seed_config '[server]
node_id = "alpha"

[network]
control_alpn = "off"
'
run_flip --node alpha --mode required
assert_rc_nonzero "$RC" "(i) a stage-2-less relay exits non-zero"
assert_contains "$ALL" "no control_alpn field" "(i) the missing field is named"
assert_contains "$ALL" "stage-2" "(i) the fix is to deploy the stage-2 build first"
assert_config_unchanged "(i) the config is untouched"
assert_line_empty "$SSH_LOG" "(i) nothing was edited over ssh"
assert_eq "$(backup_count)" "0" "(i) no backup was created"

# ── (j) refuse an unknown mode and an unresolvable node ─────
reset_fixture
seed_config '[server]
node_id = "alpha"
'
run_flip --node alpha --mode optional
assert_rc_nonzero "$RC" "(j) an unknown mode exits non-zero"
assert_contains "$ALL" "allowed values are off and required" "(j) the allowed modes are named"
assert_config_unchanged "(j) the config is untouched by an unknown mode"

run_flip --node nowhere --mode required
assert_rc_nonzero "$RC" "(j) an unresolvable node exits non-zero"
assert_contains "$ALL" "Unknown relay 'nowhere'" "(j) the unknown node is named"
assert_config_unchanged "(j) the config is untouched by an unknown node"

# ── (k) --dry-run changes nothing ───────────────────────────
reset_fixture
seed_config '[server]
node_id = "alpha"

[network]
control_alpn = "off"
'
run_flip --node alpha --mode required --dry-run
assert_rc_zero "$RC" "(k) a dry run exits 0"
assert_contains "$ALL" "dry run" "(k) the dry run is reported"
assert_contains "$ALL" "lightspeed_control_handshakes_total{outcome=\"rejected_no_application_protocol\"}" \
	"(k) the dry run names the §7.4 metric"
assert_config_unchanged "(k) the config is untouched"
assert_line_empty "$SSH_LOG" "(k) no remote command ran"
assert_line_empty "$SYSTEMCTL_LOG" "(k) no restart happened"
assert_eq "$(backup_count)" "0" "(k) no backup was created"

# ── (l) --ip overrides the resolved address ─────────────────
reset_fixture
seed_config '[server]
node_id = "alpha"

[network]
control_alpn = "off"
'
run_flip --node alpha --ip 10.0.0.99 --mode required
assert_rc_zero "$RC" "(l) a flip with --ip exits 0"
assert_grep "$SSH_LOG" "dest=root@10.0.0.99" "(l) commands went to the --ip address"
assert_contains "$ALL" "10.0.0.99" "(l) the summary names the overridden address"

# ── (l2) --ip works when the inventory is unavailable ───────
reset_fixture
seed_config '[server]
node_id = "alpha"

[network]
control_alpn = "off"
'
NODES_JSON='{}'
run_flip --node alpha --ip 10.0.0.9 --mode required
NODES_JSON='{"alpha":{"ip":"10.0.0.1"},"beta":{"ip":"10.0.0.2"},"us_east":{"ip":"10.0.0.3"}}'
assert_rc_zero "$RC" "(l2) --ip without an inventory exits 0"
assert_grep "$SSH_LOG" "dest=root@10.0.0.9" "(l2) commands went to the --ip address"

# ── (m) refuse an unreachable /health ───────────────────────
reset_fixture
seed_config '[server]
node_id = "alpha"
'
: >"$FIXTURE/curl_unreachable"
run_flip --node alpha --mode required
assert_rc_nonzero "$RC" "(m) an unreachable /health exits non-zero"
assert_contains "$ALL" "Cannot read" "(m) the unreachable health URL is reported"
assert_config_unchanged "(m) the config is untouched"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'relay-control-alpn: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'relay-control-alpn: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
