#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Self-test for lib-nodes.sh
#
# lib-nodes.sh is the shared inventory resolver that deploy.sh and
# mesh-health.sh both source. When it resolves the wrong host, an ops
# command targets the wrong machine, so its contracts are worth
# pinning: which input wins, how each shape is normalized, and that
# every failure path fails loudly instead of emitting an empty list
# that a caller would silently iterate over zero times.
#
# Asserted behaviours:
#   (a) an LIGHTSPEED_NODES array override wins over the registry
#   (b) a region-keyed object override normalizes to an array
#   (c) a bare string value is treated as the node ip
#   (d) defaults: data_addr :4434, health/metrics :8080, control 4433
#   (e) an explicit field is never overwritten by a default
#   (f) a custom LIGHTSPEED_CONTROL_PORT applies to nodes without one
#   (g) an empty override ({}/[]) falls through to the registry
#   (h) the registry is read from the repo root, not the CWD
#   (i) invalid override JSON fails loudly
#   (j) missing registry / missing payload / zero nodes each fail
#   (k) --jsonl emits exactly one object per node
#
# No network: every case is a local fixture file or an env override.
#
# Usage: bash infra/scripts/test_lib_nodes.sh
# Exits 0 and prints "lib-nodes: all assertions passed" on success.
# Requires: bash, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="$SCRIPT_DIR/lib-nodes.sh"

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

pass() { PASS=$((PASS + 1)); }
fail() {
	printf '  FAIL: %s\n' "$1" >&2
	shift
	for line in "$@"; do printf '        %s\n' "$line" >&2; done
	FAILURES=$((FAILURES + 1))
}

assert_eq() {
	if [ "$1" = "$2" ]; then
		pass
	else
		fail "$3" "got:      $1" "expected: $2"
	fi
}

assert_jq() {
	# assert_jq <json> <expr> <message>
	if printf '%s' "$1" | jq -e "$2" >/dev/null 2>&1; then
		pass
	else
		fail "$3" "expr: $2" "json: $(printf '%s' "$1" | head -c 300)"
	fi
}

if [ ! -f "$LIB" ]; then
	printf 'lib-nodes: FAIL - not found: %s\n' "$LIB" >&2
	exit 1
fi

# Resolve in a subshell so each case gets a clean environment and the
# sourced library's globals cannot leak between cases.
# run_nodes <registry-path> <override-json-or-empty> [control-port]
run_nodes() {
	local reg="$1" override="${2:-}" port="${3:-}"
	OUT="$(
		LIGHTSPEED_NODES="$override" LIGHTSPEED_REGISTRY_PATH="$reg" \
			LIGHTSPEED_CONTROL_PORT="$port" \
			bash -c 'source "$1"; lightspeed_resolve_nodes' _ "$LIB" 2>&1
	)"
	RC=$?
}

run_jsonl() {
	local reg="$1" override="${2:-}"
	OUT="$(
		LIGHTSPEED_NODES="$override" LIGHTSPEED_REGISTRY_PATH="$reg" \
			bash -c 'source "$1"; lightspeed_resolve_nodes_jsonl' _ "$LIB" 2>&1
	)"
	RC=$?
}

# A registry fixture in the real signed shape (registry is a JSON string).
mk_registry() {
	# mk_registry <path> <inner-nodes-array-json>
	# The outer document is {"registry": "<json string>", "signature": ...}
	# and the inner string is {"schema_version": n, "nodes": [...]}: the
	# `nodes` wrapper is required, which is what lib-nodes.sh validates.
	local inner
	inner="$(jq -cn --argjson n "$2" '{schema_version: 1, nodes: $n}')"
	jq -cn --arg r "$inner" '{registry: $r, signature: "test"}' > "$1"
}

REG_OK="$TMP/registry-ok.json"
mk_registry "$REG_OK" '[{"node_id":"relay-a","region":"us-west","ip":"203.0.113.10"},{"node_id":"relay-b","region":"eu-central","data_addr":"198.51.100.7:4434"}]'

# ══════════════════════════════════════════════════════════════
# (a) an array override wins over the registry
# ══════════════════════════════════════════════════════════════
run_nodes "$REG_OK" '[{"node_id":"override-node","ip":"192.0.2.5"}]'
assert_eq "$RC" "0" "(a) array override resolves"
assert_jq "$OUT" 'length == 1 and .[0].node_id == "override-node"' "(a) override replaces the registry"
assert_jq "$OUT" '.[0].ip == "192.0.2.5"' "(a) override ip carried through"

# ══════════════════════════════════════════════════════════════
# (b)-(d) object override, bare string, and defaults
# The object form is REGION-KEYED ({"tokyo": "1.2.3.4"}), not a single
# node object; a node-shaped object here would be exploded key by key.
# ══════════════════════════════════════════════════════════════
run_nodes "$REG_OK" '{"tokyo":"167.179.77.127"}'
assert_eq "$RC" "0" "(b) object override resolves"
assert_jq "$OUT" 'length == 1 and .[0].node_id == "tokyo" and .[0].region == "tokyo"' \
	"(b) the key becomes node_id and region"
assert_jq "$OUT" '.[0].ip == "167.179.77.127"' "(c) a bare string is the node ip"
assert_jq "$OUT" '.[0].data_addr == "167.179.77.127:4434"' "(d) data_addr defaults to :4434"
assert_jq "$OUT" '.[0].health_url == "http://167.179.77.127:8080/health"' "(d) health_url defaults to :8080"
assert_jq "$OUT" '.[0].metrics_url == "http://167.179.77.127:8080/metrics"' "(d) metrics_url defaults to :8080"
assert_jq "$OUT" '.[0].control_port == 4433' "(d) control_port defaults to 4433"

# ══════════════════════════════════════════════════════════════
# (e) explicit fields survive normalization
# ══════════════════════════════════════════════════════════════
run_nodes "$REG_OK" '[{"node_id":"explicit","region":"custom","ip":"203.0.113.9","data_addr":"203.0.113.9:5000","health_url":"http://203.0.113.9:9000/probe","metrics_url":"http://203.0.113.9:9001/m","control_port":5555}]'
assert_eq "$RC" "0" "(e) fully explicit node resolves"
assert_jq "$OUT" '.[0].data_addr == "203.0.113.9:5000"' "(e) explicit data_addr is not defaulted"
assert_jq "$OUT" '.[0].health_url == "http://203.0.113.9:9000/probe"' "(e) explicit health_url is not defaulted"
assert_jq "$OUT" '.[0].metrics_url == "http://203.0.113.9:9001/m"' "(e) explicit metrics_url is not defaulted"
assert_jq "$OUT" '.[0].control_port == 5555' "(e) explicit control_port is not defaulted"

# ══════════════════════════════════════════════════════════════
# (f) LIGHTSPEED_CONTROL_PORT applies where a node omits it
# ══════════════════════════════════════════════════════════════
run_nodes "$REG_OK" '[{"node_id":"p","ip":"192.0.2.1"}]' 6000
assert_eq "$RC" "0" "(f) custom control port resolves"
assert_jq "$OUT" '.[0].control_port == 6000' "(f) custom control port applied"

# A non-numeric control port must fall back rather than emit junk.
run_nodes "$REG_OK" '[{"node_id":"p","ip":"192.0.2.1"}]' notaport
assert_eq "$RC" "0" "(f guard) non-numeric control port still resolves"
assert_jq "$OUT" '.[0].control_port == 4433' "(f guard) non-numeric control port falls back to 4433"

# ══════════════════════════════════════════════════════════════
# (g) an empty override falls through to the registry
# ══════════════════════════════════════════════════════════════
run_nodes "$REG_OK" '{}'
assert_eq "$RC" "0" "(g) empty object override falls through"
assert_jq "$OUT" 'length == 2' "(g) registry nodes used when override is empty"

run_nodes "$REG_OK" '[]'
assert_eq "$RC" "0" "(g) empty array override falls through"
assert_jq "$OUT" 'length == 2' "(g) registry nodes used when array override is empty"

# ══════════════════════════════════════════════════════════════
# (h) the registry path is independent of the CWD
# ══════════════════════════════════════════════════════════════
OUT="$(cd / && LIGHTSPEED_NODES= LIGHTSPEED_REGISTRY_PATH="$REG_OK" \
	bash -c 'source "$1"; lightspeed_resolve_nodes' _ "$LIB" 2>&1)"
RC=$?
assert_eq "$RC" "0" "(h) resolves with an unrelated CWD"
assert_jq "$OUT" 'length == 2' "(h) same nodes regardless of CWD"

# ══════════════════════════════════════════════════════════════
# (i) invalid override JSON fails loudly
# ══════════════════════════════════════════════════════════════
run_nodes "$REG_OK" '{not json'
assert_eq "$RC" "1" "(i) invalid override JSON exits non-zero"
case "$OUT" in
*"not valid JSON"*) pass ;;
*) fail "(i) invalid override explains itself" "got: $OUT" ;;
esac

# ══════════════════════════════════════════════════════════════
# (j) each registry failure path fails loudly
# ══════════════════════════════════════════════════════════════
run_nodes "$TMP/does-not-exist.json" ''
assert_eq "$RC" "1" "(j) missing registry exits non-zero"
case "$OUT" in
*"not found"*) pass ;;
*) fail "(j) missing registry names the cause" "got: $OUT" ;;
esac

REG_NOPAYLOAD="$TMP/registry-nopayload.json"
printf '{"signature":"test"}' > "$REG_NOPAYLOAD"
run_nodes "$REG_NOPAYLOAD" ''
assert_eq "$RC" "1" "(j) registry without payload exits non-zero"

REG_EMPTY="$TMP/registry-empty.json"
mk_registry "$REG_EMPTY" '[]'
run_nodes "$REG_EMPTY" ''
assert_eq "$RC" "1" "(j) registry with zero nodes exits non-zero"

# A failure must not emit a usable-looking array: two nodes expected,
# and an empty list is the dangerous outcome a caller would iterate.
case "$OUT" in
*"[]"*) fail "(j) zero-node failure must not print an empty array" "got: $OUT" ;;
*) pass ;;
esac

# ══════════════════════════════════════════════════════════════
# (k) jsonl emits one object per node
# ══════════════════════════════════════════════════════════════
run_jsonl "$REG_OK" ''
assert_eq "$RC" "0" "(k) jsonl resolves"
assert_eq "$(printf '%s\n' "$OUT" | grep -c '[^[:space:]]')" "2" "(k) jsonl emits one line per node"
assert_eq "$(printf '%s\n' "$OUT" | head -1 | jq -r '.node_id')" "relay-a" "(k) jsonl preserves order"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
	printf 'lib-nodes: all assertions passed\n'
	printf '  (%s checks)\n' "$PASS"
	exit 0
fi

printf 'lib-nodes: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
