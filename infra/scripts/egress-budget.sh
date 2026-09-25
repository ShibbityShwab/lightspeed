#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed - Per-Relay Egress Budget Report
#
# Reports each relay's cumulative egress against its configured
# egress budget, so an operator can see how close the fleet is to
# a metered-plan allowance before it starts costing money.
#
# The relay exposes these Prometheus series (see proxy/src/metrics.rs):
#   lightspeed_egress_bytes_total              cumulative relay egress (both data-plane directions)
#   lightspeed_egress_budget_bytes             configured budget limit (0 = disabled)
#   lightspeed_egress_budget_used_bytes        egress counted against the limit
#   lightspeed_egress_budget_soft_exceeded     1 when the soft threshold is reached
#   lightspeed_egress_budget_hard_exceeded     1 when the hard threshold is reached
#   lightspeed_drops_egress_budget_total       new sessions refused at the hard threshold
#
# The budget counter is process-lifetime, so a relay restart resets it. For a
# month-accurate fleet total, combine this report with the reset-safe history
# produced by collect-metrics.sh.
#
# Usage:
#   egress-budget.sh [--json] [METRICS_FILE]
#
#   METRICS_FILE  Parse one local Prometheus exposition file instead of
#                 scraping the relay inventory. Useful in CI and tests.
#   --json        Emit a JSON array instead of the aligned table.
#
# Guarantees:
#   * Always exits 0; one unreachable relay never aborts the run.
#   * Only bash + curl + jq are used.
#
# Requires: bash, curl, jq
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

JSON=0
METRICS_FILE=""
for arg in "$@"; do
	case "$arg" in
	--json) JSON=1 ;;
	*) METRICS_FILE="$arg" ;;
	esac
done

TIMEOUT=4

# One metrics document -> one report row. `$id` and `$reach` are supplied by
# the caller; every referenced series defaults to 0 when absent.
ROW_JQ='
def mval($m; $name):
  (($m // "") | split("\n")
   | map(select(startswith($name + "{")))
   | (.[0] // "")) as $line
  | (try ($line | capture(" (?<v>[-+0-9.eE]+)$").v | tonumber) catch null) // 0;
def label_of($m; $name; $key):
  (($m // "") | split("\n")
   | map(select(startswith($name + "{")))
   | (.[0] // "")) as $line
  | (try ($line | capture($key + "=\"(?<v>[^\"]*)\"").v) catch "") // "";
($m) as $m
| (mval($m; "lightspeed_egress_bytes_total")) as $egress
| (mval($m; "lightspeed_egress_budget_bytes")) as $limit
| (mval($m; "lightspeed_egress_budget_used_bytes")) as $used
| (mval($m; "lightspeed_egress_budget_soft_exceeded")) as $soft
| (mval($m; "lightspeed_egress_budget_hard_exceeded")) as $hard
| (mval($m; "lightspeed_drops_egress_budget_total")) as $refusals
| {
    node_id: (if $id != "" then $id
              else (label_of($m; "lightspeed_egress_bytes_total"; "node_id") | if . == "" then "local" else . end)
              end),
    reachable: $reach,
    egress_bytes: $egress,
    budget_bytes: $limit,
    used_bytes: $used,
    used_pct: (if $limit > 0 then (($used * 100 / $limit) | floor) else 0 end),
    soft_exceeded: ($soft >= 1),
    hard_exceeded: ($hard >= 1),
    refusals: $refusals,
    status: (if $limit == 0 then "disabled"
             elif $hard >= 1 then "HARD"
             elif $soft >= 1 then "SOFT"
             else "ok" end)
  }
'

# Human-readable byte size, and the table renderer.
TABLE_JQ='
def human:
  if . >= 1073741824 then "\(((. * 100 / 1073741824) | floor) / 100) GiB"
  elif . >= 1048576 then "\(((. * 100 / 1048576) | floor) / 100) MiB"
  elif . >= 1024 then "\(((. * 100 / 1024) | floor) / 100) KiB"
  else "\(.) B" end;
.[] | [
  .node_id,
  (if .reachable then .status else "UNREACH" end),
  (if .reachable then (.egress_bytes | human) else "-" end),
  (if .reachable and .budget_bytes > 0 then (.used_bytes | human) else "-" end),
  (if .reachable and .budget_bytes > 0 then "\(.used_pct)%" else "-" end),
  (if .reachable and .budget_bytes > 0 then (.budget_bytes | human) else "-" end),
  (if .reachable then (.refusals | tostring) else "-" end)
] | @tsv
'

tmp="$(mktemp 2>/dev/null || true)"
cleanup() { [ -n "${tmp:-}" ] && rm -f "$tmp" 2>/dev/null || true; }
trap cleanup EXIT

if [ -n "$METRICS_FILE" ]; then
	if [ ! -r "$METRICS_FILE" ]; then
		printf 'egress-budget: cannot read %s\n' "$METRICS_FILE" >&2
		[ "$JSON" -eq 1 ] && printf '[]\n'
		exit 0
	fi
	body="$(cat "$METRICS_FILE" 2>/dev/null || true)"
	row="$(jq -cn --arg m "$body" --arg id "" --argjson reach true "$ROW_JQ" 2>/dev/null || true)"
	[ -n "$row" ] && printf '%s\n' "$row" >"$tmp"
else
	# shellcheck source=lib-nodes.sh
	source "$SCRIPT_DIR/lib-nodes.sh"
	nodes_json="[]"
	if resolved="$(lightspeed_resolve_nodes 2>/dev/null)"; then
		if printf '%s' "$resolved" | jq -e 'type == "array"' >/dev/null 2>&1; then
			nodes_json="$resolved"
		fi
	fi
	while IFS= read -r node; do
		[ -z "$node" ] && continue
		node_id="$(printf '%s' "$node" | jq -r '.node_id // empty' 2>/dev/null || true)"
		metrics_url="$(printf '%s' "$node" | jq -r '.metrics_url // empty' 2>/dev/null || true)"
		[ -z "$node_id" ] && continue
		metrics=""
		reach=false
		if [ -n "$metrics_url" ]; then
			metrics="$(curl --silent --max-time "$TIMEOUT" "$metrics_url" 2>/dev/null || true)"
		fi
		if [ -n "$metrics" ] && printf '%s' "$metrics" | jq -Rs -e 'test("lightspeed_")' >/dev/null 2>&1; then
			reach=true
		else
			metrics=""
		fi
		row="$(jq -cn --arg m "$metrics" --arg id "$node_id" --argjson reach "$reach" "$ROW_JQ" 2>/dev/null || true)"
		[ -n "$row" ] && printf '%s\n' "$row" >>"$tmp"
	done < <(printf '%s' "$nodes_json" | jq -c '.[]?' 2>/dev/null || true)
fi

rows="[]"
if [ -n "${tmp:-}" ] && [ -s "$tmp" ]; then
	rows="$(jq -s '.' "$tmp" 2>/dev/null || echo '[]')"
fi
printf '%s' "$rows" | jq -e 'type == "array"' >/dev/null 2>&1 || rows="[]"

if [ "$JSON" -eq 1 ]; then
	printf '%s\n' "$rows" | jq '.'
	exit 0
fi

# ── Aligned table ────────────────────────────────────────────
printf '%-18s %-9s %-12s %-12s %-6s %-8s %s\n' \
	"RELAY" "STATUS" "EGRESS" "USED" "USED%" "BUDGET" "REFUSALS"
printf '%s' "$rows" | jq -r "$TABLE_JQ" 2>/dev/null | while IFS=$'\t' read -r id status egress used pct budget refusals; do
	printf '%-18s %-9s %-12s %-12s %-6s %-8s %s\n' \
		"$id" "$status" "$egress" "$used" "$pct" "$budget" "$refusals"
done

exit 0
