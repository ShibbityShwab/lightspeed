#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for analyze-mesh.sh
#
# Fixtures only, no network: hand-built history documents exercise the
# anomaly engine, and one case drives the real collector against
# file:// fixtures to prove the live-fallback path end to end.
#
# Cases:
#   (a) a drop-rate spike on one relay fires
#   (b) a differing relay version fires version_skew, and nothing else
#   (c) a clean fixture yields zero flags
#   (d) an empty history does not crash
#   (e) a missing history falls back to a live collection (file:// fixture)
#   (f) --no-collect prints a clear instruction instead of collecting
#   (g) high FEC overhead with low relay-side recovery fires fec_overhead_high
#   (h) low overhead (or high relay-side recovery) does not fire it
#   Plus: the analyzer passes `bash -n`.
#
# Usage: bash infra/scripts/test_analyze_mesh.sh
# Exits 0 and prints "analyze-mesh: all assertions passed" on success.
# Requires: bash, curl, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ANALYZE="$SCRIPT_DIR/analyze-mesh.sh"
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

assert_eq() {
    local actual="$1" expected="$2" msg="$3"
    if [ "$actual" = "$expected" ]; then
        PASS=$((PASS + 1))
    else
        printf '  FAIL: %s (got %s, expected %s)\n' "$msg" "$actual" "$expected" >&2
        FAILURES=$((FAILURES + 1))
    fi
}

for dep in "$ANALYZE" "$COLLECT"; do
    if [ ! -f "$dep" ]; then
        printf 'analyze-mesh: FAIL - required file not found: %s\n' "$dep" >&2
        exit 1
    fi
done

# ── bash -n on the analyzer ──────────────────────────────────
if bash -n "$ANALYZE" 2>/dev/null; then
    PASS=$((PASS + 1))
else
    printf '  FAIL: analyze-mesh.sh failed bash -n\n' >&2
    FAILURES=$((FAILURES + 1))
fi

# ── History fixture builder ──────────────────────────────────
# Spec shape: [ {t:<int>, relays:{ "<id>": {v,reach,d:{...},c:{...},reset} } }, ... ]
build_history() {
    # build_history <out> <spec-json> <generated-at>
    jq -n --argjson spec "$2" --argjson gen "$3" '
      def counters: ["packets_relayed","bytes_relayed","packets_dropped","drops_malformed",
        "drops_auth_rejected","drops_abuse_blocked","drops_rate_limited","drops_fec_malformed",
        "drops_session_setup","drops_relay_send_errors","fec_data_packets","fec_parity_received",
        "fec_recoveries","fec_losses","relay_latency_us_sum","relay_latency_us_count","rate_limit_hits",
        "rate_limit_ip_hits","rate_limit_overflow","sessions_created"];
      def zero: reduce counters[] as $k ({}; .[$k] = 0);
      [ $spec[] as $s
        | ( $s.relays | to_entries | map(
              .key as $id | .value as $r
              | (zero + ($r.d // {})) as $d
              | {key: $id, value: {
                  reachable: ($r.reach // true),
                  version: ($r.v // ""),
                  active_sessions: ($r.active // 0),
                  cumulative: (zero + ($r.c // $d)),
                  delta: $d,
                  reset: ($r.reset // false),
                  reset_metrics: []
                }}
            ) | from_entries) as $pr
        | { t: $s.t,
            relay_count: ($pr | length),
            healthy_count: ([$pr[] | select(.reachable)] | length),
            interval: {},
            totals: {},
            per_relay: $pr }
      ] as $snaps
      | {version: 1, generated_at: $gen, snapshots: $snaps}
    ' > "$1"
}

# ── (a) drop-rate spike on one relay ─────────────────────────
SPEC_A="$(cat <<'JSON'
[
 {"t":1000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}},
   "relay-b":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}}
 }},
 {"t":2000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":300,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}},
   "relay-b":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}}
 }}
]
JSON
)"
HA="$TMP/history-a.json"
build_history "$HA" "$SPEC_A" 2000
NODES_AB='{"relay-a":{"ip":"10.0.0.1"},"relay-b":{"ip":"10.0.0.2"}}'
if LIGHTSPEED_NODES="$NODES_AB" LIGHTSPEED_REGISTRY_PATH="$TMP/none.json" \
        bash "$ANALYZE" --json --no-collect "$HA" >"$TMP/a.json" 2>"$TMP/a.err"; then
    rc=0
else
    rc=$?
fi
assert_rc0 "$rc" "(a) analysis exits 0"
assert_jq "$TMP/a.json" '[.flags[] | select(.type == "drop_rate_spike")] | length == 1' "(a) exactly one drop_rate_spike flag"
assert_jq "$TMP/a.json" '[.flags[] | select(.type == "drop_rate_spike" and .relay == "relay-a")] | length == 1' "(a) the spike is on relay-a"
assert_jq "$TMP/a.json" '[.flags[] | select(.type == "drop_rate_spike" and .relay == "relay-b")] | length == 0' "(a) relay-b is not flagged"
assert_jq "$TMP/a.json" '[.flags[] | select(.type == "version_skew")] | length == 0' "(a) no version skew on identical versions"
assert_jq "$TMP/a.json" '.relays[] | select(.node_id == "relay-a") | .drop_rate > 0.2' "(a) relay-a drop rate is the high interval value"

# ── (b) version skew across the fleet ────────────────────────
SPEC_B="$(cat <<'JSON'
[
 {"t":1000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}},
   "relay-b":{"v":"1.2.0","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}},
   "relay-c":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}}
 }},
 {"t":2000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}},
   "relay-b":{"v":"1.2.0","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}},
   "relay-c":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10}}
 }}
]
JSON
)"
HB="$TMP/history-b.json"
build_history "$HB" "$SPEC_B" 2000
NODES_ABC='{"relay-a":{"ip":"10.0.0.1"},"relay-b":{"ip":"10.0.0.2"},"relay-c":{"ip":"10.0.0.3"}}'
if LIGHTSPEED_NODES="$NODES_ABC" LIGHTSPEED_REGISTRY_PATH="$TMP/none.json" \
        bash "$ANALYZE" --json --no-collect "$HB" >"$TMP/b.json" 2>"$TMP/b.err"; then
    rc=0
else
    rc=$?
fi
assert_rc0 "$rc" "(b) analysis exits 0"
assert_jq "$TMP/b.json" '[.flags[] | select(.type == "version_skew")] | length == 1' "(b) exactly one version_skew flag"
assert_jq "$TMP/b.json" '[.flags[] | select(.type == "version_skew" and .relay == "relay-b")] | length == 1' "(b) the skew is relay-b"
assert_jq "$TMP/b.json" '[.flags[] | select(.type == "version_skew")][0].fleet_majority == "1.3.2"' "(b) the majority version is recorded"
assert_jq "$TMP/b.json" '[.flags[] | select(.type == "drop_rate_spike")] | length == 0' "(b) no drop-rate spike on a clean fixture"

# ── (c) clean fixture -> zero flags ──────────────────────────
SPEC_C="$(cat <<'JSON'
[
 {"t":1000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10,"rate_limit_overflow":0}},
   "relay-b":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10,"rate_limit_overflow":0}}
 }},
 {"t":2000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10,"rate_limit_overflow":0}},
   "relay-b":{"v":"1.3.2","reach":true,"d":{"packets_relayed":1000,"packets_dropped":5,"relay_latency_us_sum":100000,"relay_latency_us_count":100,"fec_recoveries":90,"fec_losses":10,"rate_limit_overflow":0}}
 }}
]
JSON
)"
HC="$TMP/history-c.json"
build_history "$HC" "$SPEC_C" 2000
if LIGHTSPEED_NODES="$NODES_AB" LIGHTSPEED_REGISTRY_PATH="$TMP/none.json" \
        bash "$ANALYZE" --json --no-collect "$HC" >"$TMP/c.json" 2>"$TMP/c.err"; then
    rc=0
else
    rc=$?
fi
assert_rc0 "$rc" "(c) clean analysis exits 0"
assert_jq "$TMP/c.json" '.flags | length == 0' "(c) clean fixture yields zero flags"
assert_jq "$TMP/c.json" '.relay_count == 2' "(c) both relays are reported"
assert_jq "$TMP/c.json" '.notes.latency | test("NOT client RTT")' "(c) latency note disclaims client RTT"

# ── (d) empty history does not crash ─────────────────────────
HD="$TMP/history-d.json"
printf '{"version":1,"generated_at":0,"snapshots":[]}\n' > "$HD"
if LIGHTSPEED_NODES="$NODES_AB" LIGHTSPEED_REGISTRY_PATH="$TMP/none.json" \
        bash "$ANALYZE" --json --no-collect "$HD" >"$TMP/d.json" 2>"$TMP/d.err"; then
    rc=0
else
    rc=$?
fi
assert_rc0 "$rc" "(d) empty history exits 0"
assert_jq "$TMP/d.json" '.data_available == false and .relay_count == 0' "(d) empty history reports no data"
assert_jq "$TMP/d.json" '.flags | type == "array"' "(d) empty history still carries a flags array"

# ── (e) missing history -> live collection against file:// fixtures ──
cat > "$TMP/relay-a.health.json" <<'JSON'
{"status":"healthy","version":"1.3.2","active_connections":0,"packets_relayed":100,"packets_dropped":20,"bytes_relayed":1000,"fec_recoveries":9,"sessions_created":2,"drops_malformed":0}
JSON
cat > "$TMP/relay-a.metrics.txt" <<'METRICS'
lightspeed_packets_relayed_total{node_id="relay-a"} 100
lightspeed_bytes_relayed_total{node_id="relay-a"} 1000
lightspeed_packets_dropped_total{node_id="relay-a"} 20
lightspeed_active_connections{node_id="relay-a"} 0
lightspeed_relay_latency_us_sum{node_id="relay-a"} 5000
lightspeed_relay_latency_us_count{node_id="relay-a"} 10
lightspeed_fec_data_packets_total{node_id="relay-a"} 7
lightspeed_fec_recoveries_total{node_id="relay-a"} 9
lightspeed_telemetry_fec_losses_total{node_id="relay-a"} 1
lightspeed_rate_limit_hits_total{node_id="relay-a"} 0
lightspeed_rate_limit_ip_hits_total{node_id="relay-a"} 0
lightspeed_rate_limit_overflow_total{node_id="relay-a"} 0
lightspeed_sessions_created_total{node_id="relay-a"} 2
lightspeed_drops_malformed_total{node_id="relay-a"} 0
lightspeed_build_info{node_id="relay-a",version="1.3.2"} 1
METRICS
node_live="$(jq -cn \
    --arg h "file://$TMP/relay-a.health.json" \
    --arg m "file://$TMP/relay-a.metrics.txt" \
    '{node_id:"relay-a",region:"test",health_url:$h,metrics_url:$m}')"
jq -n --argjson nodes "[$node_live]" \
    '{registry: ({schema_version:1, nodes:$nodes} | tojson), signature:"test"}' > "$TMP/registry-live.json"
LIVE_HIST="$TMP/live-history.json"
if LIGHTSPEED_NODES= LIGHTSPEED_REGISTRY_PATH="$TMP/registry-live.json" \
        bash "$ANALYZE" --json "$LIVE_HIST" >"$TMP/e.json" 2>"$TMP/e.err"; then
    rc=0
else
    rc=$?
fi
assert_rc0 "$rc" "(e) live fallback exits 0"
assert_jq "$TMP/e.json" '.source == "live" and .data_available == true' "(e) fallback collected a live snapshot"
assert_jq "$TMP/e.json" '.relay_count == 1 and .relays[0].node_id == "relay-a"' "(e) the relay from the registry is analyzed"
assert_jq "$TMP/e.json" '.relays[0].latency_mean_us == 500' "(e) latency mean is sum/count"
assert_jq "$TMP/e.json" '(.relays[0].drop_category_shares | length) == 7' "(e) all seven drop categories are reported"
assert_jq "$TMP/e.json" '[.flags[] | select(.type == "relay_reset")] | length == 1' "(e) a first snapshot is flagged as a reset"
assert_jq "$TMP/e.json" '[.flags[] | select(.type == "drop_rate_spike")] | length == 0' "(e) reset snapshots suppress the interval drop spike"
if [ -s "$LIVE_HIST" ]; then PASS=$((PASS + 1)); else printf '  FAIL: (e) fallback did not write history\n' >&2; FAILURES=$((FAILURES + 1)); fi

# ── (f) --no-collect prints an instruction, never collects ───
MISSING="$TMP/absent-history.json"
if LIGHTSPEED_NODES="$NODES_AB" LIGHTSPEED_REGISTRY_PATH="$TMP/none.json" \
        bash "$ANALYZE" --json --no-collect "$MISSING" >"$TMP/f.json" 2>"$TMP/f.err"; then
    rc=0
else
    rc=$?
fi
assert_rc0 "$rc" "(f) --no-collect on a missing history exits 0"
assert_jq "$TMP/f.json" '.source == "none" and .instruction != null' "(f) an instruction is emitted"
assert_jq "$TMP/f.json" '.instruction | test("collect-metrics.sh")' "(f) the instruction names the collector"
if [ -f "$MISSING" ]; then
    printf '  FAIL: (f) --no-collect wrote a history file\n' >&2
    FAILURES=$((FAILURES + 1))
else
    PASS=$((PASS + 1))
fi

# ── (g) high FEC overhead + low relay-side recovery -> fec_overhead_high ─
# fec_losses is 0, so the telemetry-only fec_recovery_ratio reads 1.0 and
# cannot fire the flag; fec_recovery_rate (4/100) is the signal that does.
SPEC_G="$(cat <<'JSON'
[
 {"t":1000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":100,"fec_parity_received":25,"fec_recoveries":4,"fec_losses":0}},
   "relay-b":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":100,"fec_parity_received":5,"fec_recoveries":4,"fec_losses":0}},
   "relay-c":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":0,"fec_parity_received":0,"fec_recoveries":0,"fec_losses":0}}
 }},
 {"t":2000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":100,"fec_parity_received":25,"fec_recoveries":4,"fec_losses":0}},
   "relay-b":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":100,"fec_parity_received":5,"fec_recoveries":4,"fec_losses":0}},
   "relay-c":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":0,"fec_parity_received":0,"fec_recoveries":0,"fec_losses":0}}
 }}
]
JSON
)"
HG="$TMP/history-g.json"
build_history "$HG" "$SPEC_G" 2000
if LIGHTSPEED_NODES="$NODES_ABC" LIGHTSPEED_REGISTRY_PATH="$TMP/none.json" \
        bash "$ANALYZE" --json --no-collect "$HG" >"$TMP/g.json" 2>"$TMP/g.err"; then
    rc=0
else
    rc=$?
fi
assert_rc0 "$rc" "(g) analysis exits 0"
assert_jq "$TMP/g.json" '.thresholds.fec_overhead_high_ratio == 0.15 and .thresholds.fec_overhead_low_recovery == 0.05' "(g) overhead thresholds exposed"
assert_jq "$TMP/g.json" '[.flags[] | select(.type == "fec_overhead_high")] | length == 1' "(g) exactly one fec_overhead_high flag"
assert_jq "$TMP/g.json" '[.flags[] | select(.type == "fec_overhead_high" and .relay == "relay-a")] | length == 1' "(g) the flag is on the high-overhead relay"
assert_jq "$TMP/g.json" '[.flags[] | select(.type == "fec_overhead_high" and .relay == "relay-b")] | length == 0' "(g) the low-overhead relay is not flagged"
assert_jq "$TMP/g.json" '.relays[] | select(.node_id == "relay-a") | .fec_overhead_ratio == 0.25' "(g) overhead ratio is parity/data"
assert_jq "$TMP/g.json" '.relays[] | select(.node_id == "relay-a") | .fec_recovery_rate == 0.04' "(g) relay-side recovery rate is recoveries/data"
assert_jq "$TMP/g.json" '.relays[] | select(.node_id == "relay-a") | .fec_recovery_ratio == 1.0' "(g) telemetry-only ratio still reads 1.0 without fec_losses"
assert_jq "$TMP/g.json" '.relays[] | select(.node_id == "relay-c") | .fec_overhead_ratio == null' "(g) overhead is null when data packets are zero"
assert_jq "$TMP/g.json" '.relays[] | select(.node_id == "relay-c") | .fec_recovery_rate == null' "(g) recovery rate is null when data packets are zero"
assert_jq "$TMP/g.json" '[.flags[] | select(.type == "fec_overhead_high")][0].fec_recovery_rate == 0.04' "(g) the flag carries the relay-side recovery rate"
assert_jq "$TMP/g.json" '[.flags[] | select(.type == "fec_overhead_high" and .relay == "relay-a" and .fec_recovery_rate <= 0.05)] | length == 1' "(g) the flag fires on relay-a's low relay-side recovery rate"
assert_jq "$TMP/g.json" '[.flags[] | select(.type == "fec_overhead_high")][0].message | test("--fec-k")' "(g) the message suggests a larger --fec-k"
assert_jq "$TMP/g.json" '[.flags[] | select(.type == "fec_overhead_high")][0].message | test("to recover 4%")' "(g) the message reports the relay-side recovery rate"
assert_jq "$TMP/g.json" '.notes.fec_overhead | test("fec_parity_received / fec_data_packets")' "(g) the note explains the overhead metric"
assert_jq "$TMP/g.json" '.notes.fec_overhead | test("fec_recoveries / fec_data_packets")' "(g) the note explains the relay-side recovery rate"

# ── (h) high relay-side recovery (or low overhead) does not fire ──
SPEC_H="$(cat <<'JSON'
[
 {"t":1000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":100,"fec_parity_received":25,"fec_recoveries":300,"fec_losses":0}},
   "relay-b":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":100,"fec_parity_received":5,"fec_recoveries":300,"fec_losses":0}}
 }},
 {"t":2000,"relays":{
   "relay-a":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":100,"fec_parity_received":25,"fec_recoveries":300,"fec_losses":0}},
   "relay-b":{"v":"1.3.2","reach":true,"d":{"fec_data_packets":100,"fec_parity_received":5,"fec_recoveries":300,"fec_losses":0}}
 }}
]
JSON
)"
HH="$TMP/history-h.json"
build_history "$HH" "$SPEC_H" 2000
if LIGHTSPEED_NODES="$NODES_AB" LIGHTSPEED_REGISTRY_PATH="$TMP/none.json" \
        bash "$ANALYZE" --json --no-collect "$HH" >"$TMP/h.json" 2>"$TMP/h.err"; then
    rc=0
else
    rc=$?
fi
assert_rc0 "$rc" "(h) analysis exits 0"
assert_jq "$TMP/h.json" '[.flags[] | select(.type == "fec_overhead_high")] | length == 0' "(h) no fec_overhead_high when relay-side recovery is high"
assert_jq "$TMP/h.json" '.relays[] | select(.node_id == "relay-a") | .fec_overhead_ratio == 0.25' "(h) relay-a still reports a high raw overhead ratio"
assert_jq "$TMP/h.json" '.relays[] | select(.node_id == "relay-a") | .fec_recovery_rate == 3.0' "(h) relay-side recovery rate exceeds the low-recovery threshold"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
    printf 'analyze-mesh: all assertions passed\n'
    printf '  (%s checks)\n' "$PASS"
    exit 0
fi

printf 'analyze-mesh: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
