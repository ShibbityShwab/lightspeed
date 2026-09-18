#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Network Stats History Appender
#
# Reads a prior history document (may be absent) plus the current
# web/network-stats.json, computes reset-safe deltas, and writes a
# bounded (last 360 snapshots) history document.
#
# Usage: append-history.sh <history_path> <stats_path> <out_path>
#   history_path  Prior history JSON. May be missing/empty/garbage.
#   stats_path    Current network-stats.json (see network-stats.sh).
#   out_path      Destination. May equal history_path (in-place).
#
# Guarantees:
#   * Always writes valid JSON.
#   * Always exits 0; bad input never aborts a Pages run.
#
# Reset rule: for each relay+metric, if current >= previous then
# delta = current - previous (reset=false) else delta = current
# (reset=true, the relay restarted). A relay absent from the prior
# snapshot is treated as a reset. Network interval.* is the sum of
# per-relay deltas; totals.* only ever grows.
#
# Requires: bash, jq
# ──────────────────────────────────────────────────────────────
set -euo pipefail

HISTORY_PATH="${1:-}"
STATS_PATH="${2:-}"
OUT_PATH="${3:-}"
MAX_SNAPSHOTS=360
DEFAULT_HISTORY='{"version":1,"generated_at":0,"snapshots":[]}'

# Always write valid JSON; never fail the caller.
write_json() {
    local json="$1"
    if ! printf '%s' "$json" | jq -e . >/dev/null 2>&1; then
        json="$DEFAULT_HISTORY"
    fi

    if [ -z "$OUT_PATH" ]; then
        printf '%s\n' "$json"
        return 0
    fi

    mkdir -p "$(dirname "$OUT_PATH")" 2>/dev/null || true
    local tmp
    tmp="$(mktemp "$(dirname "$OUT_PATH")/.append-history.XXXXXX" 2>/dev/null || true)"
    if [ -n "$tmp" ]; then
        if printf '%s\n' "$json" > "$tmp" 2>/dev/null; then
            mv "$tmp" "$OUT_PATH" 2>/dev/null && return 0
        fi
        rm -f "$tmp" 2>/dev/null || true
    fi
    printf '%s\n' "$json" > "$OUT_PATH" 2>/dev/null || true
    return 0
}

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

# ── Load current stats (strict enough to reject garbage) ─────
stats_json=""
if [ -n "$STATS_PATH" ] && [ -f "$STATS_PATH" ] && [ -s "$STATS_PATH" ]; then
    if jq -e 'type == "object"
              and (.generated_at | type == "number")
              and (.relays | type == "array")' "$STATS_PATH" >/dev/null 2>&1; then
        stats_json="$(jq -c '.' "$STATS_PATH" 2>/dev/null || true)"
    fi
fi

# No usable stats: preserve whatever history we had, still valid.
if [ -z "$stats_json" ]; then
    write_json "$prev"
    exit 0
fi

# ── Compute deltas, totals, and the bounded snapshot list ────
JQ_PROGRAM='
def n: (tonumber? // 0);
($prev.snapshots // []) as $snaps
| ($snaps | last) as $lastsnap
| (($lastsnap.per_relay) // {}) as $prevrelay
| (($lastsnap.totals) // {}) as $prevtotals
| (($stats.relays // []) | map(select(.node_id != null and .node_id != ""))) as $relays
| [ $relays[]
    | .node_id as $id
    | (.packets_relayed // 0 | n) as $cr
    | (.packets_dropped // 0 | n) as $cd
    | (.sessions_created // 0 | n) as $cs
    | ($prevrelay[$id]) as $p
    | (if $p == null then 0 else ($p.packets_relayed // 0 | n) end) as $pr
    | (if $p == null then 0 else ($p.packets_dropped // 0 | n) end) as $pd
    | (if $p == null then 0 else ($p.sessions_created // 0 | n) end) as $ps
    | {
        id: $id,
        cr: $cr, cd: $cd, cs: $cs,
        dr: (if $p != null and $cr >= $pr then $cr - $pr else $cr end),
        dd: (if $p != null and $cd >= $pd then $cd - $pd else $cd end),
        ds: (if $p != null and $cs >= $ps then $cs - $ps else $cs end),
        reset: (if $p == null then true
                else (($cr < $pr) or ($cd < $pd) or ($cs < $ps)) end)
      }
  ] as $delta
| {
    packets_relayed: ($delta | map(.dr) | add // 0),
    packets_dropped: ($delta | map(.dd) | add // 0),
    sessions_created: ($delta | map(.ds) | add // 0)
  } as $interval
| {
    packets_relayed: (($prevtotals.packets_relayed // 0 | n) + $interval.packets_relayed),
    packets_dropped: (($prevtotals.packets_dropped // 0 | n) + $interval.packets_dropped),
    sessions_created: (($prevtotals.sessions_created // 0 | n) + $interval.sessions_created)
  } as $totals
| ($delta
   | map({key: .id,
          value: {packets_relayed: .cr,
                  packets_dropped: .cd,
                  sessions_created: .cs,
                  reset: .reset}})
   | from_entries) as $per_relay
| ([ $snaps[],
     {
       t: ($stats.generated_at // 0 | n),
       relay_count: ($stats.relay_count // ($relays | length)),
       healthy_count: ($stats.healthy_count
                       // ([ $relays[] | select(.status == "healthy") ] | length)),
       interval: $interval,
       totals: $totals,
       per_relay: $per_relay
     }
   ] | if length > $max then .[length - $max:] else . end) as $newsnaps
| {
    version: 1,
    generated_at: ($stats.generated_at // 0 | n),
    snapshots: $newsnaps
  }
'

result="$(jq -c \
    --argjson prev "$prev" \
    --argjson stats "$stats_json" \
    --argjson max "$MAX_SNAPSHOTS" \
    "$JQ_PROGRAM" <<<'null' 2>/dev/null || true)"

if [ -z "$result" ]; then
    result="$prev"
fi

write_json "$result"
exit 0
