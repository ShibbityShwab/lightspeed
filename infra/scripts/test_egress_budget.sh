#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Self-test for egress-budget.sh
#
# Proves the report parses the relay's egress budget series and
# classifies each relay correctly:
#   (a) below the soft threshold -> status "ok"
#   (b) at the soft threshold -> status "SOFT", soft_exceeded true
#   (c) at the hard threshold -> status "HARD", hard_exceeded true,
#       and the refusal count is carried through
#   (d) an unconfigured relay (budget 0) -> status "disabled"
#   (e) a metrics document with no budget series at all -> disabled, 0
#   (f) a missing metrics file -> empty report, exit 0
#
# No network: each case is a local Prometheus exposition fixture.
#
# Usage: bash infra/scripts/test_egress_budget.sh
# Exits 0 and prints "egress-budget: all assertions passed" on success.
# Requires: bash, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPORT="$SCRIPT_DIR/egress-budget.sh"

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

assert_jq() {
	# assert_jq <json-file> <jq-expr> <message>
	local file="$1" expr="$2" msg="$3"
	if jq -e "$expr" "$file" >/dev/null 2>&1; then
		PASS=$((PASS + 1))
	else
		printf '  FAIL: %s\n        expr: %s\n' "$msg" "$expr" >&2
		FAILURES=$((FAILURES + 1))
	fi
}

assert_rc0() {
	if [ "$1" -eq 0 ]; then
		PASS=$((PASS + 1))
	else
		printf '  FAIL: %s (exit %s, expected 0)\n' "$2" "$1" >&2
		FAILURES=$((FAILURES + 1))
	fi
}

fixture() {
	# fixture <name> <egress> <limit> <used> <soft> <hard> <refusals>
	cat >"$TMP/$1.txt" <<EOF
lightspeed_egress_bytes_total{region="t",node_id="$1"} $2
lightspeed_egress_budget_bytes{region="t",node_id="$1"} $3
lightspeed_egress_budget_used_bytes{region="t",node_id="$1"} $4
lightspeed_egress_budget_soft_exceeded{region="t",node_id="$1"} $5
lightspeed_egress_budget_hard_exceeded{region="t",node_id="$1"} $6
lightspeed_drops_egress_budget_total{region="t",node_id="$1"} $7
EOF
}

run() {
	# run <fixture-name> <out-name>
	"$REPORT" --json "$TMP/$1.txt" >"$TMP/$2.json" 2>/dev/null
}

# (a) 40% of a 1000-byte budget: below the soft threshold.
fixture relay-ok 400 1000 400 0 0 0
run relay-ok ok
assert_jq "$TMP/ok.json" '.[0].status == "ok"' "below soft -> ok"
assert_jq "$TMP/ok.json" '.[0].used_pct == 40' "used_pct is 40"
assert_jq "$TMP/ok.json" '.[0].soft_exceeded == false' "soft false below threshold"

# (b) 80% of a 1000-byte budget: soft threshold reached.
fixture relay-soft 800 1000 800 1 0 0
run relay-soft soft
assert_jq "$TMP/soft.json" '.[0].status == "SOFT"' "soft threshold -> SOFT"
assert_jq "$TMP/soft.json" '.[0].soft_exceeded == true' "soft_exceeded true"

# (c) 100% of a 1000-byte budget: hard threshold, existing refusals counted.
fixture relay-hard 1000 1000 1000 1 1 12
run relay-hard hard
assert_jq "$TMP/hard.json" '.[0].status == "HARD"' "hard threshold -> HARD"
assert_jq "$TMP/hard.json" '.[0].hard_exceeded == true' "hard_exceeded true"
assert_jq "$TMP/hard.json" '.[0].refusals == 12' "refusals carried through"

# (d) budget disabled (limit 0) with huge egress.
fixture relay-off 999999 0 0 0 0 0
run relay-off off
assert_jq "$TMP/off.json" '.[0].status == "disabled"' "no limit -> disabled"
assert_jq "$TMP/off.json" '.[0].used_pct == 0' "disabled reports 0 pct"

# (e) a metrics document with only the egress series.
cat >"$TMP/plain.txt" <<'EOF'
lightspeed_egress_bytes_total{region="t",node_id="plain"} 123
EOF
"$REPORT" --json "$TMP/plain.txt" >"$TMP/plain.json" 2>/dev/null
assert_jq "$TMP/plain.json" '.[0].status == "disabled"' "missing budget series -> disabled"
assert_jq "$TMP/plain.json" '.[0].egress_bytes == 123' "egress still parsed"

# (f) missing file -> empty report, exit 0.
"$REPORT" --json "$TMP/does-not-exist.txt" >"$TMP/missing.json" 2>/dev/null
assert_rc0 "$?" "missing file exits 0"
assert_jq "$TMP/missing.json" 'length == 0' "missing file -> empty report"

if [ "$FAILURES" -gt 0 ]; then
	printf 'egress-budget: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
	exit 1
fi

printf 'egress-budget: all assertions passed (%s)\n' "$PASS"
