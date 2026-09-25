#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for the canary fleet rollout in deploy.sh
#
# deploy.sh is sourceable: sourcing it must define the rollout
# functions and must NOT execute the build/deploy main flow. This
# suite sources it and drives the pure orchestration functions with
# function doubles, so no SSH, curl, network, cargo, or fleet
# inventory is touched.
#
# Asserted behaviours:
#   (a) canary selection: default first node, explicit node id,
#       explicit region, and a hard failure on an unknown canary
#   (b) batch ordering: fixed-size batches in inventory order, a
#       trailing partial batch, and a rejection of size 0 / non-numeric
#   (c) the verification gate: /health version match, UDP 4433 and
#       4434 listening, and a real control-plane registration, each
#       able to fail the gate on its own with evidence
#   (d) canary-then-rest ordering: canary is deployed and verified
#       before any remaining node, then remaining nodes ship in batches
#   (e) rollback: a failed canary gate reverts the canary to the
#       recorded previous version, stops the rollout, and exits non-zero
#   (f) a failed batch member is reverted and later nodes are never
#       attempted (rollout stops)
#   (g) a canary install failure (installer already rolled back) stops
#       the rollout without a second revert
#   (h) canary disabled: the legacy all-nodes pass still runs and does
#       not need the gate
#   (i) single-relay manual path deploys exactly one node
#
# Usage: bash infra/scripts/test_deploy_canary.sh
# Exits 0 and prints "deploy-canary: all assertions passed" on success.
# Requires: bash. No network access.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DEPLOY="$SCRIPT_DIR/deploy.sh"

if [ ! -f "$DEPLOY" ]; then
	printf 'deploy-canary: FAIL - deploy.sh not found: %s\n' "$DEPLOY" >&2
	exit 1
fi

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

note_pass() { PASS=$((PASS + 1)); }
note_fail() {
	printf '  FAIL: %s\n' "$1" >&2
	FAILURES=$((FAILURES + 1))
}

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

assert_contains() {
	local haystack="$1" needle="$2" msg="$3"
	case "$haystack" in
	*"$needle"*) note_pass ;;
	*) note_fail "$msg (missing '$needle' in '$haystack')" ;;
	esac
}

assert_calls_eq() {
	local expected="$1" msg="$2"
	local actual
	actual="$(cat "$CALLS" 2>/dev/null || true)"
	if [ "$actual" = "$expected" ]; then
		note_pass
	else
		note_fail "$msg (calls differ)
--- got ---
$actual
--- want ---
$expected"
	fi
}

# ── Sourceability ────────────────────────────────────────────
# The script must expose its main entry point so tests can source it
# without executing a deploy. Refuse to source anything that still has
# top-level side effects.
if ! grep -q 'lightspeed_deploy_main' "$DEPLOY"; then
	note_fail "deploy.sh is not sourceable: no lightspeed_deploy_main entry point"
	SOURCED=0
else
	# shellcheck source=deploy.sh
	source "$DEPLOY"
	SOURCED=1
fi

require_sourceable() {
	if [ "$SOURCED" -ne 1 ]; then
		note_fail "$1 (deploy.sh is not sourceable)"
		return 1
	fi
	return 0
}

# ── Shared fixtures ──────────────────────────────────────────
CALLS="$TMP/calls"
: >"$CALLS"
rec() { printf '%s\n' "$*" >>"$CALLS"; }
reset_calls() { : >"$CALLS"; }

# Determine the per-port openness for the real udp_port_listening double.
udp_port_listening() {
	case " ${OPEN_PORTS:-} " in
	*" $2 "*) return 0 ;;
	*) return 1 ;;
	esac
}
probe_control_plane() { return "${PROBE_RC:-0}"; }
health_version_of() { printf '%s' "${HV_VERSION:-}"; }

VERSION=9.9.9
# shellcheck disable=SC2034  # consumed by sourced deploy.sh functions
NODE_NAMES=(alpha beta gamma delta)
# shellcheck disable=SC2034
declare -A NODES=(
	[alpha]=10.0.0.1
	[beta]=10.0.0.2
	[gamma]=10.0.0.3
	[delta]=10.0.0.4
)
# shellcheck disable=SC2034
declare -A REGION_TO_NODE=(
	[us_west]=alpha
	[us_east]=beta
	[eu_central]=gamma
	[ap_south]=delta
)
BATCH_SIZE=2
CANARY_NODE=""
CANARY_ENABLED=1

# ── (a) canary selection ─────────────────────────────────────
if require_sourceable "(a) canary selection"; then
	CANARY_NODE=""
	out="$(lightspeed_select_canary 2>/dev/null)"
	rc=$?
	assert_rc_zero "$rc" "(a) default canary selection succeeds"
	assert_eq "$out" "alpha" "(a) default canary is the first inventory node"

	CANARY_NODE="gamma"
	out="$(lightspeed_select_canary 2>/dev/null)"
	rc=$?
	assert_rc_zero "$rc" "(a) explicit canary node id succeeds"
	assert_eq "$out" "gamma" "(a) explicit canary node id is honored"

	CANARY_NODE="eu_central"
	out="$(lightspeed_select_canary 2>/dev/null)"
	rc=$?
	assert_rc_zero "$rc" "(a) explicit canary region succeeds"
	assert_eq "$out" "gamma" "(a) explicit canary region maps to its node"

	CANARY_NODE="nowhere"
	err="$(lightspeed_select_canary 2>&1 >/dev/null)"
	rc=$?
	assert_rc_nonzero "$rc" "(a) unknown canary is rejected"
	assert_contains "$err" "canary" "(a) unknown canary names the offending input"
	CANARY_NODE=""
fi

# ── (b) batch ordering ───────────────────────────────────────
if require_sourceable "(b) batch ordering"; then
	out="$(lightspeed_batch_nodes 2 alpha beta gamma delta)"
	assert_eq "$out" "$(printf 'alpha beta\ngamma delta')" \
		"(b) size 2 splits into two ordered batches"

	out="$(lightspeed_batch_nodes 3 alpha beta gamma delta)"
	assert_eq "$out" "$(printf 'alpha beta gamma\ndelta')" \
		"(b) size 3 leaves a trailing partial batch"

	out="$(lightspeed_batch_nodes 10 alpha beta gamma delta)"
	assert_eq "$out" "alpha beta gamma delta" \
		"(b) size larger than the fleet is one batch"

	out="$(lightspeed_batch_nodes 1 alpha beta gamma delta)"
	assert_eq "$out" "$(printf 'alpha\nbeta\ngamma\ndelta')" \
		"(b) size 1 splits every node"

	if lightspeed_batch_nodes 0 alpha beta >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(b) batch size 0 is rejected"
	if lightspeed_batch_nodes two alpha beta >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(b) non-numeric batch size is rejected"
fi

# ── (c) verification gate ────────────────────────────────────
if require_sourceable "(c) verification gate"; then
	# all green
	HV_VERSION="$VERSION"
	OPEN_PORTS="4433 4434"
	PROBE_RC=0
	if verify_node alpha 10.0.0.1 "$VERSION" >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_zero "$rc" "(c) gate passes when health, ports, and probe are green"
	assert_contains "$VERIFY_EVIDENCE" "health_version=$VERSION" "(c) evidence records the health version"
	assert_contains "$VERIFY_EVIDENCE" "ports=ok" "(c) evidence records open ports"
	assert_contains "$VERIFY_EVIDENCE" "control_probe=ok" "(c) evidence records a working probe"

	# wrong version
	HV_VERSION="1.0.0"
	if verify_node alpha 10.0.0.1 "$VERSION" >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(c) gate rejects a stale health version"
	assert_contains "$VERIFY_EVIDENCE" "health_version=1.0.0" "(c) stale-version evidence names the observed version"

	# data plane port closed
	HV_VERSION="$VERSION"
	OPEN_PORTS="4433"
	PROBE_RC=0
	if verify_node alpha 10.0.0.1 "$VERSION" >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(c) gate rejects a closed UDP data port"
	assert_contains "$VERIFY_EVIDENCE" "4434" "(c) closed-port evidence names UDP 4434"

	# control plane port closed
	OPEN_PORTS="4434"
	if verify_node alpha 10.0.0.1 "$VERSION" >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(c) gate rejects a closed UDP control port"
	assert_contains "$VERIFY_EVIDENCE" "4433" "(c) closed-control evidence names UDP 4433"

	# real registration fails even though everything else is up
	OPEN_PORTS="4433 4434"
	PROBE_RC=1
	if verify_node alpha 10.0.0.1 "$VERSION" >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(c) gate rejects a failed control-plane registration"
	assert_contains "$VERIFY_EVIDENCE" "control_probe=failed" "(c) probe-failure evidence is recorded"
fi

# ── Rollout doubles ──────────────────────────────────────────
# Redefine the leaf operations; the orchestration under test is the
# sequencing and the stop/revert decisions.
DEPLOY_FAIL_NODE=""
VERIFY_FAIL_NODE=""
deploy_node() {
	rec "deploy_node $1"
	[ "$1" = "$DEPLOY_FAIL_NODE" ] && return 1
	return 0
}
verify_node() {
	rec "verify_node $1"
	VERIFY_EVIDENCE="node=$1 expected=$VERSION"
	[ "$1" = "$VERIFY_FAIL_NODE" ] && return 1
	return 0
}
revert_node() {
	rec "revert_node $1 $3"
	return "${REVERT_RC:-0}"
}
health_version_of() { printf '%s' "${PRE_VERSION:-1.0.0}"; }

reset_rollout() {
	reset_calls
	PRE_VERSION="1.0.0"
	DEPLOY_FAIL_NODE=""
	VERIFY_FAIL_NODE=""
	REVERT_RC=0
	# shellcheck disable=SC2034  # consumed by sourced deploy.sh functions
	BATCH_SIZE=2
	# shellcheck disable=SC2034
	CANARY_NODE=""
	# shellcheck disable=SC2034
	CANARY_ENABLED=1
}

# ── (d) canary-then-rest ordering ────────────────────────────
if require_sourceable "(d) rollout ordering"; then
	reset_rollout
	if lightspeed_rollout_canary_then_rest >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_zero "$rc" "(d) a green rollout exits 0"
	assert_calls_eq "$(printf '%s\n' \
		'deploy_node alpha' \
		'verify_node alpha' \
		'deploy_node beta' \
		'verify_node beta' \
		'deploy_node gamma' \
		'verify_node gamma' \
		'deploy_node delta' \
		'verify_node delta')" \
		"(d) canary is verified before the rest and batches ship in order"

	reset_rollout
	# shellcheck disable=SC2034  # consumed by the sourced rollout function
	CANARY_NODE="gamma"
	if lightspeed_rollout_canary_then_rest >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_zero "$rc" "(d) an explicit canary rollout exits 0"
	assert_calls_eq "$(printf '%s\n' \
		'deploy_node gamma' \
		'verify_node gamma' \
		'deploy_node alpha' \
		'verify_node alpha' \
		'deploy_node beta' \
		'verify_node beta' \
		'deploy_node delta' \
		'verify_node delta')" \
		"(d) the chosen canary is first and excluded from the batches"
fi

# ── (e) canary gate failure reverts the canary ───────────────
if require_sourceable "(e) canary rollback"; then
	reset_rollout
	VERIFY_FAIL_NODE="alpha"
	if lightspeed_rollout_canary_then_rest >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(e) a failed canary gate exits non-zero"
	assert_calls_eq "$(printf '%s\n' \
		'deploy_node alpha' \
		'verify_node alpha' \
		'revert_node alpha 1.0.0')" \
		"(e) the canary is reverted to the previous version and nothing else is touched"
	assert_contains "$ROLLOUT_EVIDENCE" "alpha" "(e) failure evidence names the canary"

	reset_rollout
	VERIFY_FAIL_NODE="alpha"
	REVERT_RC=1
	if lightspeed_rollout_canary_then_rest >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(e) a failed revert still exits non-zero"
fi

# ── (f) batch failure stops the rollout and reverts that node ─
if require_sourceable "(f) batch rollback"; then
	reset_rollout
	VERIFY_FAIL_NODE="beta"
	if lightspeed_rollout_canary_then_rest >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(f) a failed batch member exits non-zero"
	assert_calls_eq "$(printf '%s\n' \
		'deploy_node alpha' \
		'verify_node alpha' \
		'deploy_node beta' \
		'verify_node beta' \
		'revert_node beta 1.0.0')" \
		"(f) the failing batch member is reverted and later nodes are never attempted"
fi

# ── (g) canary install failure: no double revert ─────────────
if require_sourceable "(g) install failure"; then
	reset_rollout
	DEPLOY_FAIL_NODE="alpha"
	if lightspeed_rollout_canary_then_rest >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(g) a failed canary install exits non-zero"
	assert_calls_eq "deploy_node alpha" \
		"(g) installer auto-rollback is trusted: no verify and no second revert"
fi

# ── (h) canary disabled keeps the legacy all-nodes pass ──────
if require_sourceable "(h) legacy path"; then
	reset_rollout
	if lightspeed_deploy_fleet_legacy >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_zero "$rc" "(h) legacy fleet deploy exits 0 when every node installs"
	assert_calls_eq "$(printf '%s\n' \
		'deploy_node alpha' \
		'deploy_node beta' \
		'deploy_node gamma' \
		'deploy_node delta')" \
		"(h) legacy path deploys every node and needs no verification gate"

	reset_rollout
	DEPLOY_FAIL_NODE="gamma"
	if lightspeed_deploy_fleet_legacy >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(h) legacy fleet deploy exits non-zero on any failure"
	assert_calls_eq "$(printf '%s\n' \
		'deploy_node alpha' \
		'deploy_node beta' \
		'deploy_node gamma' \
		'deploy_node delta')" \
		"(h) legacy path attempts every node (no fail-fast), as before"
fi

# ── (i) single-relay manual path ─────────────────────────────
if require_sourceable "(i) single relay"; then
	reset_rollout
	if lightspeed_deploy_single beta >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_zero "$rc" "(i) single-relay deploy exits 0"
	assert_calls_eq "deploy_node beta" "(i) single-relay deploy touches exactly one node"

	reset_rollout
	if lightspeed_deploy_single nowhere >/dev/null 2>&1; then rc=0; else rc=$?; fi
	assert_rc_nonzero "$rc" "(i) an unknown single-relay target is rejected"
fi

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'deploy-canary: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'deploy-canary: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
