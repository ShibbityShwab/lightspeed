#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for collect-metrics.sh
#
# Proves the load-bearing behaviours of the live metrics collector:
#   (a) an absent history file initializes cleanly
#   (b) monotonic counters -> exact interval deltas, relay reset=false
#   (c) a counter decrease -> delta is the current value, reset:true for
#       that metric, and cumulative totals never regress
#   (d) a 360-snapshot history trims the oldest on append
#   (e) an unreachable relay still yields a valid document
#   (f) a corrupt history file starts fresh
#   (g) the labeled lightspeed_geo_sessions_total family is parsed,
#       coarsened through the region catalog, and capped at 64 keys
#       without touching the scalar delta math
#
# No network: the fake registry points health/metrics URLs at file://
# fixtures, which curl reads locally.
#
# Usage: bash infra/scripts/test_collect_metrics.sh
# Exits 0 and prints "collect-metrics: all assertions passed" on success.
# Requires: bash, curl, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
COLLECT="$SCRIPT_DIR/collect-metrics.sh"

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

assert_rc0() {
    # assert_rc0 <rc> <message>
    if [ "$1" -eq 0 ]; then
        PASS=$((PASS + 1))
    else
        printf '  FAIL: %s (exit %s, expected 0)\n' "$2" "$1" >&2
        FAILURES=$((FAILURES + 1))
    fi
}

if [ ! -f "$COLLECT" ]; then
    printf 'collect-metrics: FAIL - implementation not found: %s\n' "$COLLECT" >&2
    exit 1
fi

# ── Fixture writers ──────────────────────────────────────────
# write_fixture <dir> <packets> <dropped> <bytes> <fec> <sessions> <active> <malformed> <lat_sum> <lat_count> <fec_losses> <parity>
write_fixture() {
    local dir="$1" p="$2" d="$3" b="$4" f="$5" s="$6" a="$7" mal="$8" lsum="$9" lcnt="${10}" floss="${11}" fpar="${12}"
    cat > "$dir/relay-a.health.json" <<JSON
{"status":"healthy","version":"1.3.2","active_connections":$a,"packets_relayed":$p,"packets_dropped":$d,"bytes_relayed":$b,"fec_recoveries":$f,"sessions_created":$s,"drops_malformed":$mal}
JSON
    cat > "$dir/relay-a.metrics.txt" <<METRICS
lightspeed_packets_relayed_total{node_id="relay-a"} $p
lightspeed_bytes_relayed_total{node_id="relay-a"} $b
lightspeed_packets_dropped_total{node_id="relay-a"} $d
lightspeed_active_connections{node_id="relay-a"} $a
lightspeed_relay_latency_us_sum{node_id="relay-a"} $lsum
lightspeed_relay_latency_us_count{node_id="relay-a"} $lcnt
lightspeed_fec_data_packets_total{node_id="relay-a"} 7
lightspeed_fec_parity_received_total{node_id="relay-a"} $fpar
lightspeed_fec_recoveries_total{node_id="relay-a"} $f
lightspeed_telemetry_fec_losses_total{node_id="relay-a"} $floss
lightspeed_rate_limit_hits_total{node_id="relay-a"} 4
lightspeed_rate_limit_ip_hits_total{node_id="relay-a"} 0
lightspeed_rate_limit_overflow_total{node_id="relay-a"} 0
lightspeed_sessions_created_total{node_id="relay-a"} $s
lightspeed_drops_malformed_total{node_id="relay-a"} $mal
lightspeed_auth_rejections_total{node_id="relay-a"} 2
lightspeed_abuse_blocks_total{node_id="relay-a"} 3
lightspeed_drops_fec_malformed_total{node_id="relay-a"} 0
lightspeed_drops_session_setup_total{node_id="relay-a"} 0
lightspeed_drops_relay_send_errors_total{node_id="relay-a"} 0
lightspeed_build_info{node_id="relay-a",version="1.3.2"} 1
METRICS
}

# append_geo <dir> <prometheus-line>...
append_geo() {
    local dir="$1"; shift
    local line
    for line in "$@"; do
        printf '%s\n' "$line" >> "$dir/relay-a.metrics.txt"
    done
}

# write_relay_fixture <dir> <name> <packets> <malformed> <sessions>
# Minimal per-relay health+metrics for an arbitrary relay id, used to
# drive the gap/reset/join reconciliation scenario.
write_relay_fixture() {
    local dir="$1" name="$2" p="$3" mal="$4" s="$5"
    cat > "$dir/relay-$name.health.json" <<JSON
{"status":"healthy","version":"1.3.2","active_connections":0,"packets_relayed":$p,"sessions_created":$s,"drops_malformed":$mal}
JSON
    cat > "$dir/relay-$name.metrics.txt" <<METRICS
lightspeed_packets_relayed_total{node_id="relay-$name"} $p
lightspeed_sessions_created_total{node_id="relay-$name"} $s
lightspeed_drops_malformed_total{node_id="relay-$name"} $mal
lightspeed_build_info{node_id="relay-$name",version="1.3.2"} 1
METRICS
}

# write_registry <out> <nodes-json-array>
write_registry() {
    jq -n --argjson nodes "$2" \
        '{registry: ({schema_version:1, nodes:$nodes} | tojson), signature:"test"}' > "$1"
}

node_a="$(jq -cn \
    --arg h "file://$TMP/relay-a.health.json" \
    --arg m "file://$TMP/relay-a.metrics.txt" \
    '{node_id:"relay-a",region:"us-west",health_url:$h,metrics_url:$m}')"
node_b="$(jq -cn \
    --arg h "file://$TMP/relay-b.health.json" \
    --arg m "file://$TMP/relay-b.metrics.txt" \
    '{node_id:"relay-b",region:"us-east",health_url:$h,metrics_url:$m}')"

REG_A="$TMP/registry-a.json"
write_registry "$REG_A" "[$node_a]"

# run_collect <history-path> <registry-path> [regions-path]
run_collect() {
    LIGHTSPEED_NODES= LIGHTSPEED_REGISTRY_PATH="$2" LIGHTSPEED_REGIONS_PATH="${3:-}" \
        bash "$COLLECT" "$1" >/dev/null 2>&1
    return $?
}

# ── (a) absent history initializes cleanly ───────────────────
H="$TMP/history.json"
write_fixture "$TMP" 100 5 1000 2 3 1 1 500 10 0 25
run_collect "$H" "$REG_A"; assert_rc0 $? "(a) first run exits 0"
assert_jq "$H" '.version == 1' "(a) document version is 1"
assert_jq "$H" '.snapshots | length == 1' "(a) absent history starts with one snapshot"
assert_jq "$H" '.snapshots[0].relay_count == 1 and .snapshots[0].healthy_count == 1' "(a) relay/healthy counts"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"].version == "1.3.2"' "(a) version parsed from health"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"].active_sessions == 1' "(a) active_sessions gauge recorded"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"].cumulative.packets_relayed == 100' "(a) cumulative packets_relayed"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"].lifetime.packets_relayed == 100' "(a) lifetime accumulator seeded on the first snapshot"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"].cumulative.drops_malformed == 1' "(a) flat health drop field parsed"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"].cumulative.relay_latency_us_sum == 500 and .snapshots[0].per_relay["relay-a"].cumulative.relay_latency_us_count == 10' "(a) latency histogram sum/count parsed"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"].reset == true' "(a) unseen relay treated as reset"
assert_jq "$H" '.snapshots[0].interval.packets_relayed == 100 and .snapshots[0].totals.packets_relayed == 100' "(a) first snapshot interval == totals == raw"
assert_jq "$H" '.snapshots[0].per_relay["relay-a"].cumulative.fec_parity_received == 25' "(a) cumulative fec_parity_received parsed from Prometheus"
assert_jq "$H" '.snapshots[0].interval.fec_parity_received == 25 and .snapshots[0].totals.fec_parity_received == 25' "(a) first snapshot parity interval == totals == raw"

# ── (b) monotonic counters -> exact deltas ───────────────────
write_fixture "$TMP" 150 9 1600 6 5 2 4 900 20 0 40
run_collect "$H" "$REG_A"; assert_rc0 $? "(b) second run exits 0"
assert_jq "$H" '.snapshots | length == 2' "(b) history grows to two snapshots"
assert_jq "$H" '.snapshots[1].interval.packets_relayed == 50' "(b) exact packets_relayed delta"
assert_jq "$H" '.snapshots[1].interval.drops_malformed == 3' "(b) exact drops_malformed delta"
assert_jq "$H" '.snapshots[1].interval.relay_latency_us_sum == 400' "(b) latency sum delta"
assert_jq "$H" '.snapshots[1].interval.fec_parity_received == 15' "(b) exact fec parity delta"
assert_jq "$H" '.snapshots[1].totals.fec_parity_received == 40' "(b) fec parity totals accumulate"
assert_jq "$H" '.snapshots[1].totals.packets_relayed == 150' "(b) totals accumulate"
assert_jq "$H" '.snapshots[1].per_relay["relay-a"].reset == false' "(b) monotonic relay is not a reset"
assert_jq "$H" '.snapshots[1].per_relay["relay-a"].cumulative.packets_relayed == 150' "(b) cumulative stored post-scrape"

# ── (c) counter decrease -> reset delta, totals never regress ──
write_fixture "$TMP" 30 2 200 0 1 1 1 100 3 0 10
run_collect "$H" "$REG_A"; assert_rc0 $? "(c) third run exits 0"
assert_jq "$H" '.snapshots | length == 3' "(c) reset append grows history"
assert_jq "$H" '.snapshots[2].interval.packets_relayed == 30' "(c) reset delta equals current value"
assert_jq "$H" '.snapshots[2].per_relay["relay-a"].reset == true' "(c) decreased counter marks reset=true"
assert_jq "$H" '.snapshots[2].per_relay["relay-a"].reset_metrics | index("packets_relayed") != null' "(c) reset_metrics names the decreased counter"
assert_jq "$H" '.snapshots[2].per_relay["relay-a"].reset_metrics | index("drops_malformed") != null' "(c) multiple decreased counters flagged"
assert_jq "$H" '.snapshots[2].per_relay["relay-a"].reset_metrics | index("fec_parity_received") != null' "(c) decreased parity counter flagged"
assert_jq "$H" '.snapshots[2].interval.fec_parity_received == 10' "(c) parity reset delta equals current value"
assert_jq "$H" '.snapshots[2].totals.packets_relayed == 180' "(c) totals add reset deltas"
assert_jq "$H" '(.snapshots[2].totals as $n | .snapshots[1].totals as $p | [ ($n | keys_unsorted[]) as $k | ($n[$k] >= $p[$k]) ] | all)' "(c) totals never regress across every counter"

# ── (d) 360-snapshot cap trims the oldest ────────────────────
CAP="$TMP/cap-history.json"
jq -n '[range(1;361) | {t: ., relay_count:1, healthy_count:1, interval:{}, totals:{}, per_relay:{}}]
       | {version:1, generated_at:360, snapshots:.}' > "$CAP"
write_fixture "$TMP" 500 10 5000 3 2 1 1 100 5 0 50
run_collect "$CAP" "$REG_A"; assert_rc0 $? "(d) cap run exits 0"
assert_jq "$CAP" '.snapshots | length == 360' "(d) history capped at 360 snapshots"
assert_jq "$CAP" '.snapshots[0].t == 2' "(d) oldest retained snapshot is the second"
assert_jq "$CAP" '.snapshots[-1].per_relay["relay-a"].cumulative.packets_relayed == 500' "(d) newest snapshot is the fresh scrape"

# ── (e) unreachable relay still yields a valid document ──────
REG_AB="$TMP/registry-ab.json"
write_registry "$REG_AB" "[$node_a,$node_b]"
MISS="$TMP/missing-history.json"
write_fixture "$TMP" 100 5 1000 2 3 1 1 500 10 0 25
run_collect "$MISS" "$REG_AB"; assert_rc0 $? "(e) run with a missing relay exits 0"
assert_jq "$MISS" '.version == 1 and (.snapshots | length) == 1' "(e) missing relay still writes a valid doc"
assert_jq "$MISS" '.snapshots[0].relay_count == 2' "(e) both resolved relays appear"
assert_jq "$MISS" '.snapshots[0].healthy_count == 1' "(e) only the reachable relay is healthy"
assert_jq "$MISS" '.snapshots[0].per_relay["relay-b"].reachable == false' "(e) missing relay marked unreachable"
assert_jq "$MISS" '.snapshots[0].per_relay["relay-b"].cumulative.packets_relayed == 0' "(e) missing relay contributes zero"
assert_jq "$MISS" '.snapshots[0].interval.packets_relayed == 100' "(e) unreachable relay does not inflate the interval"

# ── (f) corrupt history starts fresh ─────────────────────────
COR=$TMP/corrupt-history.json
printf 'not json at all {{{' > "$COR"
write_fixture "$TMP" 100 5 1000 2 3 1 1 500 10 0 25
run_collect "$COR" "$REG_A"; assert_rc0 $? "(f) corrupt history run exits 0"
assert_jq "$COR" '.version == 1 and (.snapshots | length) == 1' "(f) corrupt history reinitializes cleanly"

# ── (g) proxy geo: parse, coarsen, cap, degrade ──────────────
# The proxy emits a labeled multi-line family
#   lightspeed_geo_sessions_total{region=...,node_id=...,src="XX",dst="YY"} <n>
# which the scalar mval() helper cannot parse. US/DE/JP resolve through
# the shipped catalog to na/eu/apac; ZZ and QQ are deliberately unmapped.
GEO_H="$TMP/history-geo.json"
write_fixture "$TMP" 100 5 1000 2 3 1 1 500 10 0 25
append_geo "$TMP" \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="US",dst="DE"} 5' \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",dst="JP",src="US"} 4' \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="US",dst="US"} 7' \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="DE",dst="DE"} 3' \
    'lightspeed_geo_lookup_misses_total{region="us-west",node_id="relay-a",side="src"} 11' \
    'lightspeed_geo_lookup_misses_total{region="us-west",node_id="relay-a",side="dst"} 12' \
    'lightspeed_geo_sessions_skipped_total{region="us-west",node_id="relay-a",reason="self_tunnel"} 13' \
    'lightspeed_geo_sessions_rejected_total{region="us-west",node_id="relay-a"} 14'
run_collect "$GEO_H" "$REG_A"; assert_rc0 $? "(g) geo run exits 0"
assert_jq "$GEO_H" '.snapshots[-1].per_relay["relay-a"].geo["na-eu"] == 5' "(g) US->DE coarsens to na-eu"
assert_jq "$GEO_H" '.snapshots[-1].per_relay["relay-a"].geo["na-apac"] == 4' "(g) dst-before-src label order parses identically"
assert_jq "$GEO_H" '.snapshots[-1].per_relay["relay-a"].geo["na-na"] == 7 and .snapshots[-1].per_relay["relay-a"].geo["eu-eu"] == 3' "(g) all mapped cells coarsen"
assert_jq "$GEO_H" '(.snapshots[-1].per_relay["relay-a"].geo | length) == 4' "(g) exactly four session cells recorded"
assert_jq "$GEO_H" '([.snapshots[-1].per_relay["relay-a"].geo[]] | add) == 19' "(g) misses/skipped/rejected are not sessions"
assert_jq "$GEO_H" '(.snapshots[-1].per_relay["relay-a"].geo | has("apac-na")) | not' "(g) reversed region pair never fabricated"
assert_jq "$GEO_H" '.snapshots[-1].per_relay["relay-a"].geo_capped == false' "(g) no truncation below the cap"
assert_jq "$GEO_H" '(.snapshots[-1].per_relay["relay-a"] | has("geo_unmapped_cells")) | not' "(g) no unmapped field when every cell maps"

# ── (h) cap: >64 coarsened keys keep the first 64 lexicographically ──
# The shipped catalog has 8 regions (64 ordered pairs max), so the cap
# is exercised with a synthetic 10-region catalog and 70 distinct cells.
jq -n '{regions: (reduce range(0;10) as $i ({}; .["r\($i)"] = {label: "r\($i)"})),
        countries: (reduce range(0;10) as $i ({}; .["A\($i)"] = "r\($i)" | .["B\($i)"] = "r\($i)"))}' \
    > "$TMP/regions-cap.json"
GEO_CAP_H="$TMP/history-geo-cap.json"
write_fixture "$TMP" 100 5 1000 2 3 1 1 500 10 0 25
for i in $(seq 0 69); do
    printf 'lightspeed_geo_sessions_total{region="r",node_id="relay-a",src="A%s",dst="B%s"} 3\n' \
        "$((i % 10))" "$((i / 10))" >> "$TMP/relay-a.metrics.txt"
done
run_collect "$GEO_CAP_H" "$REG_A" "$TMP/regions-cap.json"; assert_rc0 $? "(h) cap run exits 0"
assert_jq "$GEO_CAP_H" '(.snapshots[-1].per_relay["relay-a"].geo | length) == 64' "(h) 70 cells cap to exactly 64 keys"
assert_jq "$GEO_CAP_H" '.snapshots[-1].per_relay["relay-a"].geo_capped == true' "(h) truncation sets geo_capped"
assert_jq "$GEO_CAP_H" '.snapshots[-1].per_relay["relay-a"].geo | has("r0-r0") and has("r9-r0")' "(h) lexicographically first keys retained"
assert_jq "$GEO_CAP_H" '(.snapshots[-1].per_relay["relay-a"].geo | has("r9-r6")) | not' "(h) lexicographically last keys dropped"
assert_jq "$GEO_CAP_H" '([.snapshots[-1].per_relay["relay-a"].geo[]] | add) == 192' "(h) surviving cells keep their counts"

# ── (i) unmapped countries are skipped and counted ───────────
GEO_UNMAP_H="$TMP/history-geo-unmapped.json"
write_fixture "$TMP" 100 5 1000 2 3 1 1 500 10 0 25
append_geo "$TMP" \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="US",dst="DE"} 5' \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="ZZ",dst="US"} 6' \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="US",dst="QQ"} 5'
run_collect "$GEO_UNMAP_H" "$REG_A"; assert_rc0 $? "(i) unmapped run exits 0"
assert_jq "$GEO_UNMAP_H" '.snapshots[-1].per_relay["relay-a"].geo["na-eu"] == 5 and (.snapshots[-1].per_relay["relay-a"].geo | length) == 1' "(i) unmapped cells are skipped, never fabricated"
assert_jq "$GEO_UNMAP_H" '.snapshots[-1].per_relay["relay-a"].geo_unmapped_cells == 2' "(i) each unmapped side-counted cell increments the counter"
assert_jq "$GEO_UNMAP_H" '.snapshots[-1].per_relay["relay-a"].geo_capped == false' "(i) skipped cells are not cap truncation"

# ── (j) missing region catalog degrades to empty geo ─────────
GEO_MISSING_H="$TMP/history-geo-missing.json"
write_fixture "$TMP" 100 5 1000 2 3 1 1 500 10 0 25
append_geo "$TMP" \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="US",dst="DE"} 5'
run_collect "$GEO_MISSING_H" "$REG_A" /nonexistent
assert_rc0 $? "(j) missing catalog still exits 0"
assert_jq "$GEO_MISSING_H" '.snapshots[-1].per_relay["relay-a"].geo == {}' "(j) missing catalog degrades to empty geo"
assert_jq "$GEO_MISSING_H" '.snapshots[-1].per_relay["relay-a"].geo_capped == false' "(j) missing catalog is not a truncation"
assert_jq "$GEO_MISSING_H" '.version == 1 and (.snapshots | length) == 1' "(j) document stays valid and versioned"

# ── (k) geo passes through the delta engine untouched ────────
write_fixture "$TMP" 150 9 1600 6 5 2 4 900 20 0 40
append_geo "$TMP" \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="US",dst="DE"} 9' \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",dst="JP",src="US"} 4' \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="US",dst="US"} 7' \
    'lightspeed_geo_sessions_total{region="us-west",node_id="relay-a",src="DE",dst="DE"} 3' \
    'lightspeed_geo_lookup_misses_total{region="us-west",node_id="relay-a",side="src"} 11' \
    'lightspeed_geo_lookup_misses_total{region="us-west",node_id="relay-a",side="dst"} 12' \
    'lightspeed_geo_sessions_skipped_total{region="us-west",node_id="relay-a",reason="self_tunnel"} 13' \
    'lightspeed_geo_sessions_rejected_total{region="us-west",node_id="relay-a"} 14'
run_collect "$GEO_H" "$REG_A"; assert_rc0 $? "(k) second geo run exits 0"
assert_jq "$GEO_H" '.version == 1 and (.snapshots | length) == 2' "(k) geo does not change the snapshot version"
assert_jq "$GEO_H" '.snapshots[-1].per_relay["relay-a"].geo["na-eu"] == 9' "(k) newest cumulative geo value passed through"
assert_jq "$GEO_H" '.snapshots[-1].interval.packets_relayed == 50' "(k) scalar delta math is unaffected by geo"
assert_jq "$GEO_H" '(.snapshots[-1].interval | has("na-eu")) | not' "(k) geo keys never enter interval"
assert_jq "$GEO_H" '(.snapshots[-1].totals | has("na-eu")) | not' "(k) geo keys never enter totals"

# ── (l) gap + reset + join: totals reconcile with sum(lifetime) ──
# A relay disappears from the inventory for one snapshot, then returns
# with a lower cumulative (a restart), while a second relay joins
# mid-history. The published totals must equal the sum of the persisted
# per-relay lifetimes, the absent relay must be carried forward, and the
# returning relay's lifetime must absorb the reset delta without
# regressing.
RECON="$TMP/history-reconcile.json"
REG_B="$TMP/registry-b.json"
write_registry "$REG_B" "[$node_b]"
write_registry "$REG_AB" "[$node_a,$node_b]"

write_relay_fixture "$TMP" a 100 1 3
run_collect "$RECON" "$REG_A"; assert_rc0 $? "(l) baseline run exits 0"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-a"].lifetime.packets_relayed == 100' "(l) first-seen relay seeds lifetime from cumulative"

write_relay_fixture "$TMP" b 50 0 2
run_collect "$RECON" "$REG_B"; assert_rc0 $? "(l) gap run exits 0"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-a"].reachable == false' "(l) absent relay carried forward as unreachable"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-a"].lifetime.packets_relayed == 100' "(l) carried-forward relay keeps its lifetime"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-a"].delta.packets_relayed == 0' "(l) carried-forward relay contributes zero delta"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-b"].lifetime.packets_relayed == 50' "(l) joining relay seeds lifetime from cumulative"
assert_jq "$RECON" '.snapshots[-1].totals.packets_relayed == ([.snapshots[-1].per_relay[].lifetime.packets_relayed] | add)' "(l) totals equal sum(lifetime) across the gap"

write_relay_fixture "$TMP" a 30 1 4
write_relay_fixture "$TMP" b 80 0 2
run_collect "$RECON" "$REG_AB"; assert_rc0 $? "(l) return-after-reset run exits 0"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-a"].reachable == true' "(l) returning relay is reachable again"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-a"].reset == true' "(l) backward cumulative marks a reset"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-a"].delta.packets_relayed == 30' "(l) reset delta equals the new cumulative"
assert_jq "$RECON" '.snapshots[-1].per_relay["relay-a"].lifetime.packets_relayed == 130' "(l) lifetime accumulates reset-safe (100 + 30)"
assert_jq "$RECON" '.snapshots[-1].totals.packets_relayed == ([.snapshots[-1].per_relay[].lifetime.packets_relayed] | add)' "(l) packets_relayed totals reconcile"
assert_jq "$RECON" '.snapshots[-1].totals.sessions_created == ([.snapshots[-1].per_relay[].lifetime.sessions_created] | add)' "(l) sessions_created totals reconcile"
assert_jq "$RECON" '.snapshots[-1].totals.drops_malformed == ([.snapshots[-1].per_relay[].lifetime.drops_malformed] | add)' "(l) drops_malformed totals reconcile"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
    printf 'collect-metrics: all assertions passed\n'
    printf '  (%s checks)\n' "$PASS"
    exit 0
fi

printf 'collect-metrics: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
