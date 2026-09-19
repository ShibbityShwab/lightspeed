#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Mesh Metrics Analyzer
#
# Turns the collector history (collect-metrics.sh output) into per-relay
# insight and a machine-readable list of things to optimize. Read-only:
# it never mutates the relays and never writes production state.
#
# Usage: analyze-mesh.sh [--json] [--no-collect] [--always-collect] [history-path]
#
#   history-path    Collector history document to read. Defaults to
#                   $LIGHTSPEED_MESH_HISTORY, else
#                   $XDG_CACHE_HOME/lightspeed/mesh-history.json
#                   ($HOME/.cache/lightspeed/mesh-history.json).
#   --json          Emit only the analysis JSON on stdout (human
#                   notes go to stderr).
#   --no-collect    Never fall back to a live collection; if there is
#                   no usable history, emit an empty analysis plus a
#                   clear instruction.
#   --always-collect  Run collect-metrics.sh first, then analyze.
#
# Fallback: when the history document is missing/empty/garbage the
# script runs one live collection into that path (unless --no-collect)
# and analyzes the result. It never aborts on missing data or an
# unreachable relay: a valid JSON document is always produced.
#
# Metrics semantics:
#   * packets/drops/sessions/rate-limit figures are per-snapshot
#     interval deltas; *_cumulative values are since relay boot.
#   * latency_mean_us = relay_latency_us_sum / relay_latency_us_count.
#     It is the mean of PROXY-OBSERVED upstream response lag, NOT the
#     client RTT. Percentiles are not exposed because the underlying
#     histogram only stores sums and counts.
#   * fec_recovery_ratio = fec_recoveries / (fec_recoveries + fec_losses)
#     over the interval; null when the denominator is zero. fec_losses is
#     opt-in client telemetry, so without it the ratio reads 1.0.
#   * fec_recovery_rate = fec_recoveries / fec_data_packets over the
#     interval: relay-side recoveries per data packet. It needs no client
#     telemetry and is null when fec_data_packets is zero.
#   * fec_overhead_ratio = fec_parity_received / fec_data_packets over the
#     interval: the redundancy the client spends on the uplink to recover
#     losses. null when fec_data_packets is zero. A high ratio paired with
#     a low recovery rate means the parity is not buying much.
#
# Anomaly flags use explicit thresholds (see THRESHOLDS below) and a
# relay's OWN recent baseline, never fleet percentiles.
#
# Requires: bash, curl, jq.
# ──────────────────────────────────────────────────────────────
# allow: SIZE_OK - single responsibility; the bulk is one embedded jq query, and a split would add a third file.
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# shellcheck source=lib-nodes.sh
source "$SCRIPT_DIR/lib-nodes.sh"

COLLECT="$SCRIPT_DIR/collect-metrics.sh"
JSON_OUT=false
COLLECT_MODE=auto # auto | never | always
HISTORY_ARG=""

# Explicit anomaly thresholds. Surfaced verbatim in the --json output.
THRESHOLDS='{
  "drop_rate_warn": 0.05,
  "drop_spike_factor": 2.0,
  "drop_min_packets": 100,
  "fec_regression_delta": 0.15,
  "fec_min_packets": 10,
  "fec_overhead_high_ratio": 0.15,
  "fec_overhead_low_recovery": 0.05,
  "latency_jump_factor": 1.5,
  "latency_min_samples": 10,
  "baseline_window": 5,
  "rate_limit_overflow": 0
}'

DEFAULT_HISTORY="${LIGHTSPEED_MESH_HISTORY:-${XDG_CACHE_HOME:-$HOME/.cache}/lightspeed/mesh-history.json}"
EMPTY_HISTORY='{"version":1,"generated_at":0,"snapshots":[]}'

usage() {
    cat <<'EOF'
Usage: analyze-mesh.sh [--json] [--no-collect] [--always-collect] [history-path]

  history-path      Collector history document. Defaults to
                    $LIGHTSPEED_MESH_HISTORY, else
                    ~/.cache/lightspeed/mesh-history.json
  --json            Print only the analysis JSON on stdout.
  --no-collect      Never fall back to a live collection.
  --always-collect  Run collect-metrics.sh before analyzing.
  -h, --help        Show this help.

Persists nothing but a history document collected on fallback.
Requires: bash, curl, jq.
EOF
}

while [ $# -gt 0 ]; do
    case "$1" in
        --json) JSON_OUT=true ;;
        --no-collect) COLLECT_MODE=never ;;
        --always-collect) COLLECT_MODE=always ;;
        -h|--help) usage; exit 0 ;;
        --) shift; [ $# -gt 0 ] && HISTORY_ARG="$1"; break ;;
        -*) printf 'analyze-mesh: unknown option: %s\n' "$1" >&2; usage >&2; exit 2 ;;
        *) HISTORY_ARG="$1" ;;
    esac
    shift
done

HISTORY_PATH="${HISTORY_ARG:-$DEFAULT_HISTORY}"

# ── Load a usable history document (sets the global history_json) ───
history_json=""
read_history() {
    history_json=""
    [ -n "$HISTORY_PATH" ] && [ -f "$HISTORY_PATH" ] && [ -s "$HISTORY_PATH" ] || return 0
    local candidate
    candidate="$(jq -c '{
        version: (.version // 1),
        generated_at: (.generated_at // 0),
        snapshots: ((.snapshots // []) | if type == "array" then . else [] end)
    } | select(type == "object")' "$HISTORY_PATH" 2>/dev/null || true)"
    if [ -n "$candidate" ]; then
        history_json="$candidate"
    fi
}

# ── One live collection into HISTORY_PATH (stdout/stderr suppressed) ─
run_collect() {
    [ -f "$COLLECT" ] || return 1
    bash "$COLLECT" "$HISTORY_PATH" >/dev/null 2>&1 || true
    read_history
    [ -n "$history_json" ]
}

read_history
SOURCE="history"
if [ "$COLLECT_MODE" = "always" ]; then
    if run_collect; then SOURCE="live"; fi
elif [ -z "$history_json" ] && [ "$COLLECT_MODE" != "never" ]; then
    if run_collect; then SOURCE="live"; fi
fi

if [ -z "$history_json" ]; then
    SOURCE="none"
    history_json="$EMPTY_HISTORY"
fi

# ── Resolve the fleet from the shared inventory; failure -> empty ──
nodes_json="[]"
if resolved="$(lightspeed_resolve_nodes 2>/dev/null)"; then
    if printf '%s' "$resolved" | jq -e 'type == "array"' >/dev/null 2>&1; then
        nodes_json="$resolved"
    fi
fi

# ── Analysis engine ──────────────────────────────────────────
JQ_PROGRAM='
def n: (tonumber? // 0);
def ratio($a; $b): (($b | n)) as $d | if $d > 0 then (($a | n) / $d) else null end;
def mean_nn($xs): ([$xs[] | select(. != null)]) as $v
    | if ($v | length) > 0 then (($v | add) / ($v | length)) else null end;
def latmean($d): ratio(($d.relay_latency_us_sum); ($d.relay_latency_us_count));
def droprate($d): ratio(($d.packets_dropped); (($d.packets_relayed | n) + ($d.packets_dropped | n)));
def fecratio($d):
    (($d.fec_recoveries | n) + ($d.fec_losses | n)) as $den
    | if $den > 0 then (($d.fec_recoveries | n) / $den) else null end;
def fecoverhead($d):
    (($d.fec_data_packets | n)) as $den
    | if $den > 0 then (($d.fec_parity_received | n) / $den) else null end;
def fecrecovery($d):
    (($d.fec_data_packets | n)) as $den
    | if $den > 0 then (($d.fec_recoveries | n) / $den) else null end;
def prior_deltas($prior; $id): [ $prior[] | (.per_relay // {})[$id] // empty | (.delta // {}) ];
def share($cats; $tot): ($cats | with_entries(.value = (if $tot > 0 then ((.value | n) / $tot) else 0 end)));
def pct($x): (($x * 10000 | round) / 100 | tostring) + "%";

. as $hist
| ($hist.snapshots // []) as $snapsRaw
| ($snapsRaw | if type == "array" then map(select(type == "object")) else [] end) as $snaps
| ($snaps | last // {}) as $last
| ($last.per_relay // {}) as $cur
| ($nodes // []) as $fleet
| ($th // {}) as $T
| ($snaps | .[0:((length) - 1)]) as $priorAll
| ($priorAll | if length > ($T.baseline_window) then .[length - ($T.baseline_window):] else . end) as $prior
| [ $cur | to_entries[] | .key as $id | .value as $r
    | ($r.delta // {}) as $d
    | ($r.cumulative // {}) as $c
    | (prior_deltas($prior; $id)) as $pd
    | (droprate($d)) as $dr
    | (mean_nn([ $pd[] | droprate(.) ])) as $drBase
    | (latmean($d)) as $lm
    | (mean_nn([ $pd[] | latmean(.) ])) as $lmBase
    | (fecratio($d)) as $fr
    | (fecoverhead($d)) as $fo
    | (fecrecovery($d)) as $frr
    | (mean_nn([ $pd[] | fecratio(.) ])) as $frBase
    | ({
        malformed: ($d.drops_malformed | n),
        auth_rejected: ($d.drops_auth_rejected | n),
        abuse_blocked: ($d.drops_abuse_blocked | n),
        rate_limited: ($d.drops_rate_limited | n),
        fec_malformed: ($d.drops_fec_malformed | n),
        session_setup: ($d.drops_session_setup | n),
        relay_send_errors: ($d.drops_relay_send_errors | n)
      }) as $cats
    | (([ $cats[] | n ] | add) // 0) as $ctot
    | ({
        malformed: ($c.drops_malformed | n),
        auth_rejected: ($c.drops_auth_rejected | n),
        abuse_blocked: ($c.drops_abuse_blocked | n),
        rate_limited: ($c.drops_rate_limited | n),
        fec_malformed: ($c.drops_fec_malformed | n),
        session_setup: ($c.drops_session_setup | n),
        relay_send_errors: ($c.drops_relay_send_errors | n)
      }) as $ccats
    | (([ $ccats[] | n ] | add) // 0) as $cctot
    | {
        node_id: $id,
        reachable: ($r.reachable // false),
        version: ($r.version // ""),
        active_sessions: ($r.active_sessions | n),
        sessions_created: ($d.sessions_created | n),
        packets_relayed: ($d.packets_relayed | n),
        packets_dropped: ($d.packets_dropped | n),
        drop_rate: $dr,
        drop_rate_baseline: $drBase,
        drop_rate_cumulative: (ratio(($c.packets_dropped); (($c.packets_relayed | n) + ($c.packets_dropped | n)))),
        latency_mean_cumulative_us: (latmean($c)),
        latency_mean_us: $lm,
        latency_samples: ($d.relay_latency_us_count | n),
        latency_baseline_us: $lmBase,
        latency_note: "mean of proxy-observed upstream lag, not client RTT",
        fec_recovery_ratio: $fr,
        fec_recovery_rate: $frr,
        fec_baseline_ratio: $frBase,
        fec_recovery_ratio_cumulative: (fecratio($c)),
        fec_overhead_ratio: $fo,
        fec_parity_received: ($d.fec_parity_received | n),
        fec_data_packets: ($d.fec_data_packets | n),
        fec_recoveries: ($d.fec_recoveries | n),
        fec_losses: ($d.fec_losses | n),
        fec_samples: (($d.fec_recoveries | n) + ($d.fec_losses | n)),
        drop_categories: $cats,
        drop_category_shares: share($cats; $ctot),
        drop_categories_cumulative: $ccats,
        drop_category_shares_cumulative: share($ccats; $cctot),
        rate_limit: {
          hits: ($d.rate_limit_hits | n),
          ip_hits: ($d.rate_limit_ip_hits | n),
          overflow: ($d.rate_limit_overflow | n),
          overflow_cumulative: ($c.rate_limit_overflow | n)
        },
        cumulative: {
          packets_relayed: ($c.packets_relayed | n),
          packets_dropped: ($c.packets_dropped | n),
          sessions_created: ($c.sessions_created | n)
        },
        reset: ($r.reset // false),
        reset_metrics: ($r.reset_metrics // [])
      }
  ] as $rows
| [ $rows[] | select(
      .reset == false
      and (.packets_relayed + .packets_dropped) >= ($T.drop_min_packets)
      and .drop_rate != null
      and .drop_rate >= ($T.drop_rate_warn)
      and ((.drop_rate_baseline == null) or (.drop_rate >= (((.drop_rate_baseline // 0)) * $T.drop_spike_factor)))
    )
    | {
        type: "drop_rate_spike",
        relay: .node_id,
        severity: "warning",
        metric: "drop_rate",
        value: .drop_rate,
        baseline: .drop_rate_baseline,
        threshold: $T.drop_rate_warn,
        factor: $T.drop_spike_factor,
        message: (.node_id + ": drop rate " + pct(.drop_rate)
                  + " (baseline "
                  + (if .drop_rate_baseline == null then "none" else pct(.drop_rate_baseline) end)
                  + ")")
      }
  ] as $dropFlags
| [ $rows[] | select(
      .reset == false
      and .fec_recovery_ratio != null
      and .fec_baseline_ratio != null
      and .fec_samples >= ($T.fec_min_packets)
      and .fec_recovery_ratio <= (((.fec_baseline_ratio // 0)) - $T.fec_regression_delta)
    )
    | {
        type: "fec_ratio_regression",
        relay: .node_id,
        severity: "warning",
        metric: "fec_recovery_ratio",
        value: .fec_recovery_ratio,
        baseline: .fec_baseline_ratio,
        threshold_delta: $T.fec_regression_delta,
        samples: .fec_samples,
        message: (.node_id + ": FEC recovery ratio " + pct(.fec_recovery_ratio)
                  + " below baseline " + pct(.fec_baseline_ratio))
      }
  ] as $fecFlags
| [ $rows[] | select(
      .reset == false
      and .fec_overhead_ratio != null
      and .fec_overhead_ratio >= ($T.fec_overhead_high_ratio)
      and .fec_recovery_rate != null
      and .fec_recovery_rate <= ($T.fec_overhead_low_recovery)
      and .fec_data_packets >= ($T.fec_min_packets)
    )
    | {
        type: "fec_overhead_high",
        relay: .node_id,
        severity: "warning",
        metric: "fec_overhead_ratio",
        value: .fec_overhead_ratio,
        fec_recovery_rate: .fec_recovery_rate,
        parity_packets: .fec_parity_received,
        data_packets: .fec_data_packets,
        threshold_ratio: $T.fec_overhead_high_ratio,
        threshold_recovery: $T.fec_overhead_low_recovery,
        message: (.node_id + ": FEC overhead " + pct(.fec_overhead_ratio)
                  + " to recover " + pct(.fec_recovery_rate)
                  + "; consider a larger client --fec-k")
      }
  ] as $fecOverheadFlags
| [ $rows[] | select(
      .reset == false
      and .latency_mean_us != null
      and .latency_baseline_us != null
      and .latency_baseline_us > 0
      and .latency_samples >= ($T.latency_min_samples)
      and .latency_mean_us >= (((.latency_baseline_us // 0)) * $T.latency_jump_factor)
    )
    | {
        type: "latency_jump",
        relay: .node_id,
        severity: "warning",
        metric: "latency_mean_us",
        value: .latency_mean_us,
        baseline: .latency_baseline_us,
        factor: $T.latency_jump_factor,
        samples: .latency_samples,
        message: (.node_id + ": mean proxy-observed lag "
                  + ((.latency_mean_us * 10 | round) / 10 | tostring) + "us vs baseline "
                  + ((.latency_baseline_us * 10 | round) / 10 | tostring) + "us")
      }
  ] as $latFlags
| ([ $rows[] | select(.version != "") ]) as $vrows
| ($vrows | group_by(.version) | map({version: .[0].version, count: length}) | sort_by(-.count)) as $vgroups
| ($vgroups[0] // null) as $vmode
| [ $vrows[] | select($vmode != null and .version != $vmode.version)
    | {
        type: "version_skew",
        relay: .node_id,
        severity: "warning",
        metric: "version",
        value: .version,
        fleet_majority: $vmode.version,
        versions: ($vrows | map({(.node_id): .version}) | add),
        message: (.node_id + ": version " + (.version | tostring)
                  + " differs from fleet majority " + ($vmode.version | tostring))
      }
  ] as $skewFlags
| [ $rows[] | select(.rate_limit.overflow > ($T.rate_limit_overflow) or .rate_limit.overflow_cumulative > ($T.rate_limit_overflow))
    | {
        type: "rate_limit_saturation",
        relay: .node_id,
        severity: "warning",
        metric: "rate_limit_overflow",
        value: .rate_limit.overflow,
        cumulative: .rate_limit.overflow_cumulative,
        threshold: $T.rate_limit_overflow,
        message: (.node_id + ": rate-limit table overflowed (interval "
                  + (.rate_limit.overflow | tostring) + ", cumulative "
                  + (.rate_limit.overflow_cumulative | tostring) + ")")
      }
  ] as $rlFlags
| [ $fleet[] | .node_id as $id | select(($cur[$id] // null) == null)
    | {
        type: "relay_absent",
        relay: $id,
        severity: "critical",
        metric: "presence",
        value: null,
        message: ($id + ": configured but absent from the latest snapshot")
      }
  ] as $absentFlags
| [ $fleet[] | .node_id as $id
    | select(($cur[$id] // null) != null and (($cur[$id].reachable // false) == false))
    | {
        type: "relay_unreachable",
        relay: $id,
        severity: "critical",
        metric: "reachable",
        value: false,
        message: ($id + ": did not answer the latest scrape")
      }
  ] as $unreachFlags
| [ $rows[] | select(.reset == true)
    | {
        type: "relay_reset",
        relay: .node_id,
        severity: "warning",
        metric: "reset",
        value: true,
        reset_metrics: .reset_metrics,
        message: (.node_id + ": counter reset observed (relay restart or metrics reset)")
      }
  ] as $resetFlags
| ($dropFlags + $fecFlags + $fecOverheadFlags + $latFlags + $skewFlags + $rlFlags + $absentFlags + $unreachFlags + $resetFlags) as $flags
| {
    generated_at: ($last.t // $hist.generated_at // 0),
    history_path: $path,
    source: $source,
    data_available: (($snaps | length) > 0),
    snapshot_count: ($snaps | length),
    relay_count: ($rows | length),
    fleet_count: ($fleet | length),
    thresholds: $T,
    notes: {
      latency: "latency_mean_us is the mean of proxy-observed upstream response lag (relay_latency_us_sum / relay_latency_us_count), NOT client RTT",
      fec: "fec_recovery_ratio = fec_recoveries / (fec_recoveries + fec_losses), interval deltas; null when the denominator is zero",
      fec_overhead: "fec_overhead_ratio = fec_parity_received / fec_data_packets and fec_recovery_rate = fec_recoveries / fec_data_packets, interval deltas; fec_recovery_rate is relay-side and reliable without client telemetry; both null when fec_data_packets is zero",
      rates: "packets, drops, sessions and rate-limit numbers are per-snapshot interval deltas; cumulative_* are since relay boot",
      baseline: "spike/regression/jump comparisons use each relay own recent baseline (up to baseline_window prior snapshots), never fleet percentiles",
      reset_guard: "interval flags (drop spike, FEC regression, latency jump) are suppressed when a relay latest snapshot is a reset, because its delta is cumulative-since-boot, not an interval"
    },
    instruction: (if $source == "none" then ("no history at " + $path + "; run: bash " + $collect + " " + $path) else null end),
    relays: $rows,
    flags: ($flags | sort_by(.severity, .relay, .type))
  }
'

analysis="$(printf '%s' "$history_json" | jq -c \
    --argjson nodes "$nodes_json" \
    --argjson th "$THRESHOLDS" \
    --arg path "$HISTORY_PATH" \
    --arg source "$SOURCE" \
    --arg collect "$COLLECT" \
    "$JQ_PROGRAM" 2>/dev/null || true)"

if [ -z "$analysis" ] || ! printf '%s' "$analysis" | jq -e . >/dev/null 2>&1; then
    analysis="$(jq -cn \
        --arg path "$HISTORY_PATH" \
        --arg source "$SOURCE" \
        --arg collect "$COLLECT" \
        '{generated_at:0, history_path:$path, source:$source, data_available:false,
          snapshot_count:0, relay_count:0, fleet_count:0,
          thresholds:{}, notes:{},
          instruction:("no history at " + $path + "; run: bash " + $collect + " " + $path),
          relays:[], flags:[]}')"
fi

# ── Machine-readable output ──────────────────────────────────
if [ "$JSON_OUT" = true ]; then
    printf '%s\n' "$analysis"
    exit 0
fi

# ── Human-readable table ─────────────────────────────────────
print_table() {
    local json="$1"
    printf '%-14s %-7s %9s %8s %7s %10s %6s %8s %6s %6s %6s %5s %9s %-4s %s\n' \
        RELAY VER RX DROP 'DROP%' 'LAT_us' 'FEC%' 'FEC_OVH' RL RLIP OVF 'A/C' RST FLAGS
    printf -- '----------------------------------------------------------------------------------------------------------\n'
    while IFS=$'\t' read -r id ver rx drop dpct lat fec ovh rl rlip ovf ac rst flags; do
        [ -z "$id" ] && continue
        printf '%-14s %-7s %9s %8s %7s %10s %6s %8s %6s %6s %6s %5s %9s %-4s %s\n' \
            "$id" "$ver" "$rx" "$drop" "$dpct" "$lat" "$fec" "$ovh" "$rl" "$rlip" "$ovf" "$ac" "$rst" "$flags"
    done < <(printf '%s' "$json" | jq -r '
        . as $root
        | $root.relays[]
        | .node_id as $id
        | [ $id,
            (if .version == "" then "-" else .version end),
            (.packets_relayed | tostring),
            (.packets_dropped | tostring),
            (if .drop_rate == null then "-" else (((.drop_rate * 10000) | round) / 100 | tostring) end),
            (if .latency_mean_us == null then "-" else (((.latency_mean_us * 10) | round) / 10 | tostring) end),
            (if .fec_recovery_rate == null then "-" else (((.fec_recovery_rate * 1000) | round) / 10 | tostring) end),
            (if .fec_overhead_ratio == null then "-" else (((.fec_overhead_ratio * 1000) | round) / 10 | tostring) end),
            (.rate_limit.hits | tostring),
            (.rate_limit.ip_hits | tostring),
            (.rate_limit.overflow | tostring),
            ((.active_sessions | tostring) + "/" + (.sessions_created | tostring)),
            (if .reset then "Y" else "-" end),
            ([ $root.flags[] | select(.relay == $id) | .type ] | unique | join(","))
          ] | @tsv')
}

print_table "$analysis"

snapshot_n="$(printf '%s' "$analysis" | jq -r '.snapshot_count // 0')"
relay_n="$(printf '%s' "$analysis" | jq -r '.relay_count // 0')"
flag_n="$(printf '%s' "$analysis" | jq -r '.flags | length')"
crit_n="$(printf '%s' "$analysis" | jq -r '[.flags[] | select(.severity == "critical")] | length')"

printf '\nrelays=%s snapshots=%s flags=%s critical=%s source=%s\n' \
    "$relay_n" "$snapshot_n" "$flag_n" "$crit_n" "$(printf '%s' "$analysis" | jq -r '.source')"

if [ "$flag_n" != "0" ]; then
    printf '\nflags:\n'
    printf '%s' "$analysis" | jq -r '.flags[] | "  [" + .severity + "] " + .type + ": " + (.message // "")'
fi

if printf '%s' "$analysis" | jq -e '.instruction != null' >/dev/null 2>&1; then
    printf '\n%s\n' "$(printf '%s' "$analysis" | jq -r '.instruction')"
fi

printf '\nnotes:\n'
printf '%s' "$analysis" | jq -r '.notes | to_entries[] | "  " + .key + ": " + .value'
printf '  lat_us/fec%%/fec_ovh: mean proxy-observed lag (not client RTT); recoveries/data_packets (relay-side); parity/data uplink overhead\n'

exit 0
