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
# relay absent from the current scrape is carried forward from the prior
# snapshot (same lifetime, zero delta, reachable=false). Per-relay
# lifetime.* is the reset-safe accumulator and totals.* is the sum of
# those accumulators, so the published fleet total always reconciles with
# the per-relay lifetimes.
#
# Non-counter fields: `active_sessions` is a gauge and `version` is a
# label. They are recorded per relay but excluded from delta/totals,
# because differencing a gauge is meaningless. The per-relay `geo`
# region-pair map, `geo_capped`, and `geo_unmapped_cells` are cumulative
# snapshot metadata and are likewise passed through untouched.
#
# Requires: bash, curl, jq
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

# shellcheck source=lib-nodes.sh
source "$SCRIPT_DIR/lib-nodes.sh"

HISTORY_PATH="${1:-}"
MAX_SNAPSHOTS=360
# Bounded per-relay geo map. With the current 8-region catalog there are at
# most 64 ordered region pairs, so this cap never truncates today; it bounds
# the snapshot size once the catalog grows past that.
MAX_GEO_CELLS=64
TIMEOUT=4
NOW="$(date +%s)"
DEFAULT_HISTORY='{"version":1,"generated_at":0,"snapshots":[]}'
REGIONS_PATH="${LIGHTSPEED_REGIONS_PATH:-$REPO_ROOT/infra/geo/regions.json}"

# Cumulative counters tracked for reset-safe deltas. Keep this list in
# sync with RELAY_JQ's `cumulative` object.
COUNTERS='["packets_relayed","bytes_relayed","packets_dropped","drops_malformed","drops_auth_rejected","drops_abuse_blocked","drops_rate_limited","drops_fec_malformed","drops_session_setup","drops_relay_send_errors","fec_data_packets","fec_parity_received","fec_recoveries","fec_losses","relay_latency_us_sum","relay_latency_us_count","rate_limit_hits","rate_limit_ip_hits","rate_limit_overflow","sessions_created","direct_ms_sum","direct_ms_count","relayed_ms_sum","relayed_ms_count","saved_ms_sum","saved_ms_count","direct_app_ms_sum","direct_app_ms_count","saved_app_ms_sum","saved_app_ms_count","saved_app_ms_negative_count","route_reports","route_samples","route_rtt_p50_ms_sum","route_rtt_p50_ms_count","route_rtt_p95_ms_sum","route_rtt_p95_ms_count","route_rtt_p99_ms_sum","route_rtt_p99_ms_count","route_jitter_ms_sum","route_jitter_ms_count","route_lost","route_recovered","route_dedup_saved","route_rejected"]'

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
		if printf '%s\n' "$json" >"$tmp" 2>/dev/null; then
			mv "$tmp" "$HISTORY_PATH" 2>/dev/null && return 0
		fi
		rm -f "$tmp" 2>/dev/null || true
	fi
	printf '%s\n' "$json" >"$HISTORY_PATH" 2>/dev/null || true
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
def sum_family($m; $name):
  (($m // "") | split("\n")
   | map(select((startswith($name + "{")) or (startswith($name + " "))))
   | map(try (capture("^[^ ]+ +(?<v>[-+0-9.eE]+)").v | tonumber) catch null)
   | map(select(. != null))
   | add) // 0;
def pick($H; $m; $hk; $mn):
  ($H[$hk]) as $v
  | if $v != null then ($v | tonumber? // 0) else mval($m; $mn) end;
def mver($m):
  ($m // "") as $txt
  | ($txt | split("\n")
     | map(select(startswith("lightspeed_build_info{")))
     | (.[0] // "")) as $line
  | (try ($line | capture("version=\"(?<v>[^\"]*)\"").v) catch "") // "";
def geo_labels($labels):
  ($labels | split(",")
   | map(capture("^\\s*(?<k>[A-Za-z_][A-Za-z0-9_]*)\\s*=\\s*\"(?<v>[^\"]*)\"\\s*$") // empty)
   | map({(.k): .v})
   | add) // {};
def geo_cells($m; $countries; $max):
  (($m // "") | split("\n")
   | map(select(startswith("lightspeed_geo_sessions_total{")))
   | map(try (
        capture("^lightspeed_geo_sessions_total\\{(?<labels>[^}]*)\\}\\s+(?<value>[-+0-9.eE]+)")
        | ((.value | tonumber?) // null) as $v
        | (geo_labels(.labels)) as $l
        | select($v != null and ($l.src // null) != null and ($l.dst // null) != null)
        | { src: $l.src, dst: $l.dst, v: $v }
      ) catch null)
   | map(select(. != null))
  ) as $cells
  | (reduce $cells[] as $c
       ({ cells: {}, unmapped: 0 };
        ($countries[$c.src] // null) as $sr
        | ($countries[$c.dst] // null) as $dr
        | if $sr != null and $dr != null
          then ("\($sr)-\($dr)") as $key
               | .cells[$key] = ((.cells[$key] // 0) + $c.v)
          else .unmapped += 1
          end)) as $agg
  | (($agg.cells | keys) | sort) as $keys
  | { geo: (reduce $keys[0:$max][] as $k ({}; .[$k] = $agg.cells[$k])),
      geo_capped: (($keys | length) > $max),
      unmapped: $agg.unmapped };
def labeled_rows($m; $name):
  (($m // "") | split("\n")
   | map(select(startswith($name + "{")))
   | map(try (
        capture("^" + $name + "\\{(?<labels>[^}]*)\\}\\s+(?<value>[-+0-9.eE]+)")
        | ((.value | tonumber?) // null) as $v
        | (geo_labels(.labels)) as $l
        | select($v != null and ($l.country // null) != null)
        | { country: $l.country, v: $v }
      ) catch null)
   | map(select(. != null)));
def source_cells($m; $countries):
  (reduce labeled_rows($m; "lightspeed_telemetry_saved_app_ms_sum")[] as $r
     ({}; .[$r.country].saved_app_ms_sum =
            (((.[$r.country].saved_app_ms_sum) // 0) + $r.v))) as $sums
  | (reduce labeled_rows($m; "lightspeed_telemetry_saved_app_ms_count")[] as $r
     ({}; .[$r.country].saved_app_ms_count =
            (((.[$r.country].saved_app_ms_count) // 0) + $r.v))) as $counts
  | (reduce labeled_rows($m; "lightspeed_telemetry_saved_app_ms_negative_count")[] as $r
     ({}; .[$r.country].saved_app_ms_negative_count =
            (((.[$r.country].saved_app_ms_negative_count) // 0) + $r.v))) as $negs
  | ([ ($sums | keys), ($counts | keys), ($negs | keys) ] | add // []) as $cs
  | (reduce ($cs | unique)[] as $c
       ({ cells: {}, unmapped: 0 };
        ($countries[$c] // null) as $reg
        | if $reg != null
          then .cells[$reg] = {
                 saved_app_ms_sum: (((.cells[$reg].saved_app_ms_sum) // 0)
                                     + (($sums[$c].saved_app_ms_sum) // 0)),
                 saved_app_ms_count: (((.cells[$reg].saved_app_ms_count) // 0)
                                       + (($counts[$c].saved_app_ms_count) // 0)),
                 saved_app_ms_negative_count: (((.cells[$reg].saved_app_ms_negative_count) // 0)
                                               + (($negs[$c].saved_app_ms_negative_count) // 0))
               }
          else .unmapped += 1
          end)) as $agg
  | { sources: $agg.cells, unmapped: $agg.unmapped };
($h | try fromjson catch null) as $H
| (geo_cells($m; $countries; $maxgeo)) as $g
| (source_cells($m; $countries)) as $s
| {
    node_id: $id,
    reachable: $reach,
    version: ((($H.version) // "") | tostring | if . == "" then mver($m) else . end),
    active_sessions: ((($H.active_connections) // null) as $a
                      | if $a != null then ($a | tonumber? // 0)
                        else mval($m; "lightspeed_active_connections") end),
    geo: $g.geo,
    geo_capped: $g.geo_capped,
    sources: $s.sources,
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
      fec_parity_received: mval($m; "lightspeed_fec_parity_received_total"),
      fec_recoveries: pick($H; $m; "fec_recoveries"; "lightspeed_fec_recoveries_total"),
      fec_losses: mval($m; "lightspeed_telemetry_fec_losses_total"),
      relay_latency_us_sum: mval($m; "lightspeed_relay_latency_us_sum"),
      relay_latency_us_count: mval($m; "lightspeed_relay_latency_us_count"),
      direct_ms_sum: sum_family($m; "lightspeed_telemetry_direct_ms_sum"),
      direct_ms_count: sum_family($m; "lightspeed_telemetry_direct_ms_count"),
      direct_app_ms_sum: sum_family($m; "lightspeed_telemetry_direct_app_ms_sum"),
      direct_app_ms_count: sum_family($m; "lightspeed_telemetry_direct_app_ms_count"),
      saved_app_ms_sum: sum_family($m; "lightspeed_telemetry_saved_app_ms_sum"),
      saved_app_ms_count: sum_family($m; "lightspeed_telemetry_saved_app_ms_count"),
      saved_app_ms_negative_count: sum_family($m; "lightspeed_telemetry_saved_app_ms_negative_count"),
      relayed_ms_sum: sum_family($m; "lightspeed_telemetry_relayed_ms_sum"),
      relayed_ms_count: sum_family($m; "lightspeed_telemetry_relayed_ms_count"),
      saved_ms_sum: sum_family($m; "lightspeed_telemetry_saved_ms_sum"),
      saved_ms_count: sum_family($m; "lightspeed_telemetry_saved_ms_count"),
      route_reports: sum_family($m; "lightspeed_telemetry_route_reports_total"),
      route_samples: sum_family($m; "lightspeed_telemetry_route_samples_total"),
      route_rtt_p50_ms_sum: sum_family($m; "lightspeed_telemetry_route_rtt_p50_ms_sum"),
      route_rtt_p50_ms_count: sum_family($m; "lightspeed_telemetry_route_rtt_p50_ms_count"),
      route_rtt_p95_ms_sum: sum_family($m; "lightspeed_telemetry_route_rtt_p95_ms_sum"),
      route_rtt_p95_ms_count: sum_family($m; "lightspeed_telemetry_route_rtt_p95_ms_count"),
      route_rtt_p99_ms_sum: sum_family($m; "lightspeed_telemetry_route_rtt_p99_ms_sum"),
      route_rtt_p99_ms_count: sum_family($m; "lightspeed_telemetry_route_rtt_p99_ms_count"),
      route_jitter_ms_sum: sum_family($m; "lightspeed_telemetry_route_jitter_ms_sum"),
      route_jitter_ms_count: sum_family($m; "lightspeed_telemetry_route_jitter_ms_count"),
      route_lost: sum_family($m; "lightspeed_telemetry_route_lost_total"),
      route_recovered: sum_family($m; "lightspeed_telemetry_route_recovered_total"),
      route_dedup_saved: sum_family($m; "lightspeed_telemetry_route_dedup_saved_total"),
      route_rejected: sum_family($m; "lightspeed_telemetry_route_rejected_total"),
      rate_limit_hits: mval($m; "lightspeed_rate_limit_hits_total"),
      rate_limit_ip_hits: mval($m; "lightspeed_rate_limit_ip_hits_total"),
      rate_limit_overflow: mval($m; "lightspeed_rate_limit_overflow_total"),
      sessions_created: pick($H; $m; "sessions_created"; "lightspeed_sessions_created_total")
    }
  }
| if $g.unmapped > 0 then .geo_unmapped_cells = $g.unmapped else . end
| if $s.unmapped > 0 then .sources_unmapped = $s.unmapped else . end
'

# ── Delta engine: prev history + current scrape -> bounded document ──
DELTA_JQ='
def n: (tonumber? // 0);
# Reset-safe delta for one counter: monotonic growth -> difference;
# a backward counter (relay restarted) -> the new cumulative, because
# the lifetime up to the reset already lives in prev_lifetime.
def delta_of($cv; $pv; $reach):
  if $reach then (if $cv >= $pv then $cv - $pv else $cv end) else 0 end;
# First time a relay is seen, its lifetime is its current cumulative
# (the lifetime up to that point). If an older snapshot predates the
# lifetime field, seed from its cumulative instead of dropping history.
def prev_life($pr; $k):
  (($pr.lifetime[$k]) // ($pr.cumulative[$k]) // 0) | n;
($current // []) as $cur
| (($prev.snapshots) // []) as $snaps
| ($snaps | last) as $lastsnap
| (($lastsnap.per_relay) // {}) as $prevrelay
| ([ $cur[].node_id ] | unique) as $curids
| ([ $prevrelay | keys[] | select(. as $k | ($curids | index($k)) == null) ]) as $staleids
| [ $cur[]
    | (.node_id) as $id
    | ((.reachable // false)) as $reach
    | (.cumulative // {}) as $cc
    | (($prevrelay[$id]) // null) as $pr
    | (($pr.cumulative) // {}) as $pc
    | (reduce $counters[] as $k (
         {cum:{}, delta:{}, lifetime:{}, reset_metrics:[]};
         ($cc[$k] // 0 | n) as $cv
         | (if ($pc | type) == "object" then ($pc[$k] // 0 | n) else 0 end) as $pv
         | (if $reach then $cv else $pv end) as $effc
         | delta_of($cv; $pv; $reach) as $d
         | (if $reach and ($pr == null or $cv < $pv) then true else false end) as $rst
         | .cum[$k] = $effc
         | .delta[$k] = $d
         | .lifetime[$k] = (prev_life($pr; $k) + $d)
         | (if $rst then .reset_metrics += [$k] else . end)
       )) as $r
    | {
        id: $id,
        reachable: $reach,
        version: (.version // ""),
        active_sessions: (.active_sessions // 0 | n),
        geo: (.geo // {}),
        geo_capped: (.geo_capped // false),
        geo_unmapped_cells: (.geo_unmapped_cells // null),
        sources: (.sources // {}),
        sources_unmapped: (.sources_unmapped // null),
        cumulative: $r.cum,
        delta: $r.delta,
        lifetime: $r.lifetime,
        reset_metrics: $r.reset_metrics,
        reset: (($r.reset_metrics | length) > 0)
      }
  ] as $rows
| [ $staleids[]
    | . as $id
    | $prevrelay[$id] as $pr
    | {
        id: $id,
        reachable: false,
        version: ($pr.version // ""),
        active_sessions: ($pr.active_sessions // 0 | n),
        geo: ($pr.geo // {}),
        geo_capped: ($pr.geo_capped // false),
        geo_unmapped_cells: ($pr.geo_unmapped_cells // null),
        sources: ($pr.sources // {}),
        sources_unmapped: ($pr.sources_unmapped // null),
        cumulative: ($pr.cumulative // {}),
        delta: (reduce $counters[] as $k ({}; .[$k] = 0)),
        lifetime: (reduce $counters[] as $k ({}; .[$k] = prev_life($pr; $k))),
        reset_metrics: [],
        reset: false
      }
  ] as $stalerows
| ($rows + $stalerows) as $allrows
| (reduce $allrows[] as $r (
     {};
     .[$r.id] = ({
        reachable: $r.reachable,
        version: $r.version,
        active_sessions: $r.active_sessions,
         geo: $r.geo,
         geo_capped: $r.geo_capped,
         sources: $r.sources,
         lifetime: $r.lifetime,
         cumulative: $r.cumulative,
        delta: $r.delta,
        reset: $r.reset,
        reset_metrics: $r.reset_metrics
     } + (if $r.geo_unmapped_cells == null
          then {}
          else {geo_unmapped_cells: $r.geo_unmapped_cells}
          end)
       + (if $r.sources_unmapped == null
          then {}
          else {sources_unmapped: $r.sources_unmapped}
          end))
   )) as $per_relay
| (reduce $counters[] as $k (
     {};
     .[$k] = ([ $allrows[].delta[$k] ] | add // 0)
   )) as $interval
| (reduce $counters[] as $k (
     {};
     .[$k] = ([ $per_relay[].lifetime[$k] ] | add // 0)
   )) as $totals
| ([ $snaps[],
     {
       t: ($now | n),
       relay_count: ($cur | length),
       healthy_count: ([ $allrows[] | select(.reachable) ] | length),
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

# ── Load the country -> region catalog; absence -> empty map ─
COUNTRY_REGIONS="{}"
if [ -f "$REGIONS_PATH" ] && [ -r "$REGIONS_PATH" ]; then
	loaded_regions="$(jq -c 'if (.countries | type) == "object" then .countries else {} end' \
		"$REGIONS_PATH" 2>/dev/null || true)"
	if [ -n "$loaded_regions" ]; then
		COUNTRY_REGIONS="$loaded_regions"
	fi
fi

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
		--argjson countries "$COUNTRY_REGIONS" \
		--argjson maxgeo "$MAX_GEO_CELLS" \
		"$RELAY_JQ" 2>/dev/null || true)"
	if [ -n "$row" ] && [ -n "${TMP_ROWS:-}" ]; then
		printf '%s\n' "$row" >>"$TMP_ROWS"
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
# prev and current are large (accumulated history plus live metrics), so they
# travel as files rather than --argjson: past ARG_MAX the jq run produced
# nothing and the collector silently rewrote the previous history, freezing the
# snapshot list on real data while the small test fixtures passed.
ctmp="$(mktemp -d 2>/dev/null || mktemp -d -t lightspeed-collect)"
printf '%s' "$prev" >"$ctmp/prev.json"
printf '%s' "$current_json" >"$ctmp/current.json"
DPROLOGUE='($prevf[0]) as $prev | ($curf[0]) as $current | '
result="$(jq -c \
	--slurpfile prevf "$ctmp/prev.json" \
	--slurpfile curf "$ctmp/current.json" \
	--argjson counters "$COUNTERS" \
	--argjson max "$MAX_SNAPSHOTS" \
	--argjson now "$NOW" \
	"${DPROLOGUE}${DELTA_JQ}" <<<'null' 2>/dev/null || true)"
rm -rf "$ctmp"

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
