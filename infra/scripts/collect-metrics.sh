#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Live Relay Metrics Collector
#
# Resolves the relay inventory (web/registry.json by default, or a
# LIGHTSPEED_NODES override), scrapes each relay's /health and
# /metrics, and appends a reset-safe snapshot to a bounded history
# document. The delta discipline mirrors append-history.sh.
#
# Usage: collect-metrics.sh <history-path>
#   history-path  History document to read (previous snapshot) and
#                 overwrite. May be absent/empty/garbage (fresh start).
#                 When omitted, the document is printed to stdout.
#
# Guarantees:
#   * Always writes valid JSON.
#   * Always exits 0; one unreachable relay never aborts the run.
#   * Only bash + curl + jq are used.
#
# Reset rule: for each relay + cumulative metric, if current >= previous
# then delta = current - previous (reset=false); otherwise delta =
# current with a reset flag for that metric (the relay restarted). A
# relay absent from the prior snapshot is treated as newly seen
# (reset=true). Per-snapshot interval.* is the sum of per-relay deltas
# and totals.* only ever grows.
#
# Non-counter fields: `active_sessions` is a gauge and `version` is a
# label. They are recorded per relay but excluded from delta/totals,
# because differencing a gauge is meaningless.
#
# Requires: bash, curl, jq
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=lib-nodes.sh
source "$SCRIPT_DIR/lib-nodes.sh"

HISTORY_PATH="${1:-}"
MAX_SNAPSHOTS=360
TIMEOUT=4
NOW="$(date +%s)"
DEFAULT_HISTORY='{"version":1,"generated_at":0,"snapshots":[]}'

# Cumulative counters tracked for reset-safe deltas. Keep this list in
# sync with RELAY_JQ's `cumulative` object.
COUNTERS='["packets_relayed","bytes_relayed","packets_dropped","drops_malformed","drops_auth_rejected","drops_abuse_blocked","drops_rate_limited","drops_fec_malformed","drops_session_setup","drops_relay_send_errors","fec_data_packets","fec_recoveries","fec_losses","relay_latency_us_sum","relay_latency_us_count","rate_limit_hits","rate_limit_ip_hits","rate_limit_overflow","sessions_created"]'

# ── Always write valid JSON; never fail the caller ───────────
write_json() {
    local json="$1"
    if ! printf '%s' "$json" | jq -e . >/dev/null 2>&1; then
        json="$DEFAULT_HISTORY"
    fi

    if [ -z "$HISTORY_PATH" ]; then
        printf '%s\n' "$json"
        return 0
    fi

    mkdir -p "$(dirname "$HISTORY_PATH")" 2>/dev/null || true
    local tmp
    tmp="$(mktemp "$(dirname "$HISTORY_PATH")/.collect-metrics.XXXXXX" 2>/dev/null || true)"
    if [ -n "$tmp" ]; then
        if printf '%s\n' "$json" > "$tmp" 2>/dev/null; then
            mv "$tmp" "$HISTORY_PATH" 2>/dev/null && return 0
        fi
        rm -f "$tmp" 2>/dev/null || true
    fi
    printf '%s\n' "$json" > "$HISTORY_PATH" 2>/dev/null || true
    return 0
}

# ── Per-relay parser: health JSON + Prometheus text -> one object ──
RELAY_JQ='
def mval($m; $name):
  ($m // "") as $txt
  | ($txt | split("\n")
     | map(select((startswith($name + "{")) or (startswith($name + " "))))
     | (.[0] // "")) as $line
  | (try ($line | capture("^[^ ]+ +(?<v>[-+0-9.eE]+)").v | tonumber) catch 0) // 0;
def pick($H; $m; $hk; $mn):
  ($H[$hk]) as $v
  | if $v != null then ($v | tonumber? // 0) else mval($m; $mn) end;
def mver($m):
  ($m // "") as $txt
  | ($txt | split("\n")
     | map(select(startswith("lightspeed_build_info{")))
     | (.[0] // "")) as $line
  | (try ($line | capture("version=\"(?<v>[^\"]*)\"").v) catch "") // "";
($h | try fromjson catch null) as $H
| {
    node_id: $id,
    reachable: $reach,
    version: ((($H.version) // "") | tostring | if . == "" then mver($m) else . end),
    active_sessions: ((($H.active_connections) // null) as $a
                      | if $a != null then ($a | tonumber? // 0)
                        else mval($m; "lightspeed_active_connections") end),
    cumulative: {
      packets_relayed: pick($H; $m; "packets_relayed"; "lightspeed_packets_relayed_total"),
      bytes_relayed: pick($H; $m; "bytes_relayed"; "lightspeed_bytes_relayed_total"),
      packets_dropped: pick($H; $m; "packets_dropped"; "lightspeed_packets_dropped_total"),
      drops_malformed: pick($H; $m; "drops_malformed"; "lightspeed_drops_malformed_total"),
      drops_auth_rejected: pick($H; $m; "drops_auth_rejected"; "lightspeed_auth_rejections_total"),
      drops_abuse_blocked: pick($H; $m; "drops_abuse_blocked"; "lightspeed_abuse_blocks_total"),
      drops_rate_limited: pick($H; $m; "drops_rate_limited"; "lightspeed_rate_limit_hits_total"),
      drops_fec_malformed: pick($H; $m; "drops_fec_malformed"; "lightspeed_drops_fec_malformed_total"),
      drops_session_setup: pick($H; $m; "drops_session_setup"; "lightspeed_drops_session_setup_total"),
      drops_relay_send_errors: pick($H; $m; "drops_relay_send_errors"; "lightspeed_drops_relay_send_errors_total"),
      fec_data_packets: mval($m; "lightspeed_fec_data_packets_total"),
      fec_recoveries: pick($H; $m; "fec_recoveries"; "lightspeed_fec_recoveries_total"),
      fec_losses: mval($m; "lightspeed_telemetry_fec_losses_total"),
      relay_latency_us_sum: mval($m; "lightspeed_relay_latency_us_sum"),
      relay_latency_us_count: mval($m; "lightspeed_relay_latency_us_count"),
      rate_limit_hits: mval($m; "lightspeed_rate_limit_hits_total"),
      rate_limit_ip_hits: mval($m; "lightspeed_rate_limit_ip_hits_total"),
      rate_limit_overflow: mval($m; "lightspeed_rate_limit_overflow_total"),
      sessions_created: pick($H; $m; "sessions_created"; "lightspeed_sessions_created_total")
    }
  }
'

# ── Delta engine: prev history + current scrape -> bounded document ──
DELTA_JQ='
def n: (tonumber? // 0);
($current // []) as $cur
| (($prev.snapshots) // []) as $snaps
| ($snaps | last) as $lastsnap
| (($lastsnap.per_relay) // {}) as $prevrelay
| (($lastsnap.totals) // {}) as $prevtotals
| [ $cur[]
    | (.node_id) as $id
    | ((.reachable // false)) as $reach
    | (.cumulative // {}) as $cc
    | (($prevrelay[$id].cumulative) // null) as $pc
    | (reduce $counters[] as $k (
         {cum:{}, delta:{}, reset_metrics:[]};
         ($cc[$k] // 0 | n) as $cv
         | (if $pc == null then 0 else ($pc[$k] // 0 | n) end) as $pv
         | (if $reach then $cv else $pv end) as $effc
         | (if $reach
            then (if $cv >= $pv then $cv - $pv else $cv end)
            else 0 end) as $d
         | (if $reach and ($pc == null or $cv < $pv) then true else false end) as $rst
         | .cum[$k] = $effc
         | .delta[$k] = $d
         | (if $rst then .reset_metrics += [$k] else . end)
       )) as $r
    | {
        id: $id,
        reachable: $reach,
        version: (.version // ""),
        active_sessions: (.active_sessions // 0 | n),
        cumulative: $r.cum,
        delta: $r.delta,
        reset_metrics: $r.reset_metrics,
        reset: (($r.reset_metrics | length) > 0)
      }
  ] as $rows
| (reduce $counters[] as $k (
     {};
     .[$k] = ([ $rows[].delta[$k] ] | add // 0)
   )) as $interval
| (reduce $counters[] as $k (
     {};
     .[$k] = (($prevtotals[$k] // 0 | n) + ($interval[$k] // 0))
   )) as $totals
| (reduce $rows[] as $r (
     {};
     .[$r.id] = {
        reachable: $r.reachable,
        version: $r.version,
        active_sessions: $r.active_sessions,
        cumulative: $r.cumulative,
        delta: $r.delta,
        reset: $r.reset,
        reset_metrics: $r.reset_metrics
     }
   )) as $per_relay
| ([ $snaps[],
     {
       t: ($now | n),
       relay_count: ($cur | length),
       healthy_count: ([ $rows[] | select(.reachable) ] | length),
       interval: $interval,
       totals: $totals,
       per_relay: $per_relay
     }
   ] | if length > $max then .[length - $max:] else . end) as $newsnaps
| {
    version: 1,
    generated_at: ($now | n),
    snapshots: $newsnaps
  }
'

# ── Load prior history (normalize to a known-good shape) ─────
prev="$DEFAULT_HISTORY"
if [ -n "$HISTORY_PATH" ] && [ -f "$HISTORY_PATH" ] && [ -s "$HISTORY_PATH" ]; then
    candidate="$(jq -c '{
        version: 1,
        generated_at: (.generated_at // 0),
        snapshots: (.snapshots // [])
    } | select(.snapshots | type == "array")' "$HISTORY_PATH" 2>/dev/null || true)"
    if [ -n "$candidate" ]; then
        prev="$candidate"
    fi
fi

# ── Resolve the relay inventory; failure -> empty list ───────
nodes_json="[]"
if resolved="$(lightspeed_resolve_nodes 2>/dev/null)"; then
    if printf '%s' "$resolved" | jq -e 'type == "array"' >/dev/null 2>&1; then
        nodes_json="$resolved"
    fi
fi

# ── Scrape each relay; never abort on one ────────────────────
TMP_ROWS="$(mktemp 2>/dev/null || true)"
cleanup() { [ -n "${TMP_ROWS:-}" ] && rm -f "$TMP_ROWS" 2>/dev/null || true; }
trap cleanup EXIT

while IFS= read -r node; do
    [ -z "$node" ] && continue
    node_id="$(printf '%s' "$node" | jq -r '.node_id // empty' 2>/dev/null || true)"
    health_url="$(printf '%s' "$node" | jq -r '.health_url // empty' 2>/dev/null || true)"
    metrics_url="$(printf '%s' "$node" | jq -r '.metrics_url // empty' 2>/dev/null || true)"
    [ -z "$node_id" ] && continue

    health=""
    metrics=""
    if [ -n "$health_url" ]; then
        health="$(curl --silent --max-time "$TIMEOUT" "$health_url" 2>/dev/null || true)"
    fi
    if [ -n "$metrics_url" ]; then
        metrics="$(curl --silent --max-time "$TIMEOUT" "$metrics_url" 2>/dev/null || true)"
    fi

    reach=false
    if [ -n "$health" ] && printf '%s' "$health" | jq -e 'type == "object"' >/dev/null 2>&1; then
        reach=true
    elif [ -n "$metrics" ] && printf '%s' "$metrics" | jq -Rs -e 'test("lightspeed_")' >/dev/null 2>&1; then
        reach=true
    fi

    row="$(jq -cn \
        --arg id "$node_id" \
        --arg h "$health" \
        --arg m "$metrics" \
        --argjson reach "$reach" \
        "$RELAY_JQ" 2>/dev/null || true)"
    if [ -n "$row" ] && [ -n "${TMP_ROWS:-}" ]; then
        printf '%s\n' "$row" >> "$TMP_ROWS"
    fi
done < <(printf '%s' "$nodes_json" | jq -c '.[]?' 2>/dev/null || true)

if [ -n "${TMP_ROWS:-}" ]; then
    current_json="$(jq -s '.' "$TMP_ROWS" 2>/dev/null || echo '[]')"
else
    current_json="[]"
fi
if ! printf '%s' "$current_json" | jq -e 'type == "array"' >/dev/null 2>&1; then
    current_json="[]"
fi

# ── Compute deltas, totals, and the bounded snapshot list ────
result="$(jq -c \
    --argjson prev "$prev" \
    --argjson current "$current_json" \
    --argjson counters "$COUNTERS" \
    --argjson max "$MAX_SNAPSHOTS" \
    --argjson now "$NOW" \
    "$DELTA_JQ" <<<'null' 2>/dev/null || true)"

if [ -z "$result" ]; then
    result="$prev"
fi

write_json "$result"

# ── Summary (stdout only; never affects the document) ────────
if [ -n "$result" ]; then
    relay_n="$(printf '%s' "$result" | jq -r '.snapshots[-1].relay_count // 0' 2>/dev/null || echo 0)"
    snap_n="$(printf '%s' "$result" | jq -r '.snapshots | length' 2>/dev/null || echo 0)"
    reset_n="$(printf '%s' "$result" | jq -r '[.snapshots[-1].per_relay[]? | select(.reset)] | length' 2>/dev/null || echo 0)"
    printf 'collect-metrics: %s relay(s), %s snapshot(s), %s reset relay(s) -> %s\n' \
        "$relay_n" "$snap_n" "$reset_n" "${HISTORY_PATH:-<stdout>}"
fi

exit 0
