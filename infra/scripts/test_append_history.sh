#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for append-history.sh
#
# Proves the three load-bearing behaviours of the history appender:
#   (a) normal monotonic counters  -> exact interval deltas
#   (b) a counter that decreases   -> restart/reset branch, totals never drop
#   (c) 400 appended snapshots     -> capped at the last 360
# Plus robustness: missing / garbage inputs still yield valid JSON, exit 0.
#
# Usage: bash infra/scripts/test_append_history.sh
# Exits 0 and prints "append-history: all assertions passed" on success.
# Requires: bash, jq. No network access.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
APPEND="$SCRIPT_DIR/append-history.sh"

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

assert_jq() {
    # assert_jq <file> <jq-expr> <message>
    local file="$1" expr="$2" msg="$3"
    if jq -e "$expr" "$file" >/dev/null 2>&1; then
        PASS=$((PASS + 1))
    else
        printf '  FAIL: %s\n        expr: %s\n' "$msg" "$expr" >&2
        FAILURES=$((FAILURES + 1))
    fi
}

if [ ! -f "$APPEND" ]; then
    printf 'append-history: FAIL - implementation not found: %s\n' "$APPEND" >&2
    exit 1
fi

# ── Fixture writers ──────────────────────────────────────────
write_stats() {
    # write_stats <path> <generated_at> <relayed> <dropped> <sessions>
    cat > "$1" <<JSON
{
  "generated_at": $2,
  "relay_count": 1,
  "healthy_count": 1,
  "relays": [
    {
      "node_id": "relay-a",
      "area": "North America",
      "region": "us-west",
      "status": "healthy",
      "version": "1.3.2",
      "uptime_secs": 10,
      "packets_relayed": $3,
      "packets_dropped": $4,
      "sessions_created": $5
    }
  ]
}
JSON
}

run_append() {
    # run_append <history> <stats> ; uses history as out (in-place)
    bash "$APPEND" "$1" "$2" "$1" >/dev/null 2>&1
    local rc=$?
    if [ "$rc" -ne 0 ]; then
        printf '  FAIL: append-history exited %s (expected 0)\n' "$rc" >&2
        FAILURES=$((FAILURES + 1))
    fi
}

# ── (a) two snapshots, counters increase -> exact interval deltas ──
S1="$TMP/stats1.json"; S2="$TMP/stats2.json"; H="$TMP/history.json"
write_stats "$S1" 1000 100 5 2
write_stats "$S2" 2000 150 9 3

run_append "$H" "$S1"
assert_jq "$H" '.version == 1 and .generated_at == 1000' "doc version + generated_at after first append"
assert_jq "$H" '.snapshots | length == 1' "first append yields one snapshot"
assert_jq "$H" '.snapshots[0].t == 1000' "snapshot t mirrors stats.generated_at"
assert_jq "$H" '.snapshots[0].relay_count == 1 and .snapshots[0].healthy_count == 1' "snapshot relay/healthy counts"
assert_jq "$H" '.snapshots[0].interval == {"packets_relayed":100,"packets_dropped":5,"sessions_created":2}' "(a) first snapshot interval = raw counters"
assert_jq "$H" '.snapshots[0].totals == {"packets_relayed":100,"packets_dropped":5,"sessions_created":2}' "(a) first snapshot totals = interval"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"] == {"packets_relayed":100,"packets_dropped":5,"sessions_created":2,"reset":true}' "(a) unseen relay treated as reset"

run_append "$H" "$S2"
assert_jq "$H" '.snapshots | length == 2' "second append grows history"
assert_jq "$H" '.generated_at == 2000' "generated_at tracks latest stats"
assert_jq "$H" '.snapshots[1].interval == {"packets_relayed":50,"packets_dropped":4,"sessions_created":1}' "(a) exact interval deltas"
assert_jq "$H" '.snapshots[1].totals == {"packets_relayed":150,"packets_dropped":9,"sessions_created":3}' "(a) totals accumulate"
assert_jq "$H" '.snapshots[1].per_relay["relay-a"].reset == false' "(a) monotonic relay is not a reset"
assert_jq "$H" '.snapshots[1].per_relay["relay-a"] == {"packets_relayed":150,"packets_dropped":9,"sessions_created":3,"reset":false}' "(a) per_relay stores cumulative counters"

# ── (b) counter decreases (relay restart) ────────────────────
S3="$TMP/stats3.json"
write_stats "$S3" 3000 30 2 1
run_append "$H" "$S3"
assert_jq "$H" '.snapshots | length == 3' "restart append grows history"
assert_jq "$H" '.snapshots[2].interval == {"packets_relayed":30,"packets_dropped":2,"sessions_created":1}' "(b) reset delta equals the new raw value"
assert_jq "$H" '.snapshots[2].per_relay["relay-a"].reset == true' "(b) decreased counter marks reset=true"
assert_jq "$H" '.snapshots[2].per_relay["relay-a"] == {"packets_relayed":30,"packets_dropped":2,"sessions_created":1,"reset":true}' "(b) per_relay reflects post-restart cumulative values"
assert_jq "$H" '.snapshots[2].totals == {"packets_relayed":180,"packets_dropped":11,"sessions_created":4}' "(b) totals add reset deltas"
assert_jq "$H" '(.snapshots[2].totals.packets_relayed >= .snapshots[1].totals.packets_relayed) and (.snapshots[2].totals.packets_dropped >= .snapshots[1].totals.packets_dropped) and (.snapshots[2].totals.sessions_created >= .snapshots[1].totals.sessions_created)' "(b) totals never decrease"

# ── (c) 400 appends -> capped at last 360 ────────────────────
C="$TMP/cap-history.json"
for i in $(seq 1 400); do
    write_stats "$TMP/cap-stats.json" "$i" "$i" "$i" "$i"
    bash "$APPEND" "$C" "$TMP/cap-stats.json" "$C" >/dev/null 2>&1 || {
        printf '  FAIL: append-history failed on cap iteration %s\n' "$i" >&2
        FAILURES=$((FAILURES + 1))
        break
    }
done
assert_jq "$C" '.snapshots | length == 360' "(c) history capped at 360 snapshots"
assert_jq "$C" '.snapshots[0].t == 41' "(c) oldest retained snapshot is the 41st"
assert_jq "$C" '.snapshots[-1].t == 400' "(c) newest snapshot retained"
assert_jq "$C" '.generated_at == 400' "(c) generated_at is newest t"

# ── (d) robustness: missing + garbage inputs stay valid, exit 0 ──
MISSING_H="$TMP/missing-history.json"
MISSING_S="$TMP/does-not-exist.json"
run_append "$MISSING_H" "$MISSING_S"
assert_jq "$MISSING_H" '.version == 1 and (.snapshots | type == "array")' "(d) missing inputs still write valid JSON"
assert_jq "$MISSING_H" '.snapshots | length == 0' "(d) missing stats appends nothing"

GARBAGE_H="$TMP/garbage-history.json"; GARBAGE_S="$TMP/garbage-stats.json"
printf 'not json at all {{{' > "$GARBAGE_S"
printf 'also not json' > "$GARBAGE_H"
run_append "$GARBAGE_H" "$GARBAGE_S"
assert_jq "$GARBAGE_H" '.version == 1 and (.snapshots | type == "array")' "(d) garbage inputs still write valid JSON"
assert_jq "$GARBAGE_H" '.snapshots | length == 0' "(d) garbage inputs initialize empty history"

# ── (e) relay absent from previous snapshot -> reset=true ─────
E_H="$TMP/edge-history.json"; E_S1="$TMP/edge-1.json"; E_S2="$TMP/edge-2.json"
write_stats "$E_S1" 10 1 1 1
cat > "$E_S2" <<'JSON'
{
  "generated_at": 20,
  "relay_count": 2,
  "healthy_count": 1,
  "relays": [
    {"node_id": "relay-a", "status": "healthy", "packets_relayed": 5, "packets_dropped": 3, "sessions_created": 2},
    {"node_id": "relay-b", "status": "down",    "packets_relayed": 7, "packets_dropped": 0, "sessions_created": 0}
  ]
}
JSON
run_append "$E_H" "$E_S1"
run_append "$E_H" "$E_S2"
assert_jq "$E_H" '.snapshots[1].per_relay["relay-a"].reset == false' "(e) known relay keeps reset=false"
assert_jq "$E_H" '.snapshots[1].per_relay["relay-b"].reset == true and .snapshots[1].per_relay["relay-b"].packets_relayed == 7' "(e) new relay treated as reset with raw delta"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
    printf 'append-history: all assertions passed\n'
    printf '  (%s checks)\n' "$PASS"
    exit 0
fi

printf 'append-history: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
