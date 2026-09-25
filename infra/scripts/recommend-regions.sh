#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Relay Placement Recommender
#
# Scores candidate relay locations against the demand matrix
# reconstructed from the reset-safe metrics history and emits an
# ADD / MOVE / NONE recommendation with a stability gate.
#
# Usage:
#   recommend-regions.sh --history <path> --previous <path|missing>
#                        --registry <path> --geo-dir <path>
#                        --out <path> [--window-secs N]
#
#   --history   Metrics history document (see collect-metrics.sh).
#   --previous  Prior recommender output (for stability.runs), or the
#               literal string "missing". Optional.
#   --registry  Signed relay registry (web/registry.json). Optional;
#               defaults to <repo>/web/registry.json.
#   --geo-dir   Directory holding regions.json and candidates.json.
#               Defaults to <repo>/infra/geo.
#   --out       Output document path. Omitted -> stdout.
#   --window-secs  Override candidates.json params.window_secs.
#
# Guarantees:
#   * Always exits 0.
#   * Always writes valid JSON (minimal INSUFFICIENT_DATA document on
#     fatal parse failure or an internal error).
#   * Only bash + jq are used. No network, no SSH.
#
# Algorithm (see wat and infra/geo/README.md for the catalog):
#   1. params come from candidates.json; --window-secs overrides.
#   2. Snapshots with t >= (latest_t - window_secs) are selected; at
#      least two are needed to reconstruct deltas.
#   3. The demand matrix M[src][dst] sums reset-safe per-relay geo
#      counter deltas across consecutive snapshot pairs. A relay whose
#      later snapshot has reset=true contributes its current count,
#      otherwise max(0, current - previous). Cells below
#      min_cell_sessions are dropped.
#   4. window_sessions = sum(M). Below min_window_sessions the run is
#      INSUFFICIENT_DATA; it still emits the matrix and ranking.
#   5. Cells are smoothed toward the row mean by alpha (the prior).
#   6. Existing relay positions come from regions.json relays, or the
#      region centroid of the registry's region alias.
#   7. coverage_gain is the demand-weighted latency improvement a
#      candidate adds; redundancy_gain rewards cells currently served
#      by exactly one relay when the candidate is "within 10 percent".
#   8. Distances are haversine km * 0.01 = approximate RTT ms.
#   9. Ranking is score desc, candidate id asc. margin is the relative
#      lead of the top candidate over the best candidate in a *different*
#      region (candidates sharing the leader's region are alternatives,
#      not rivals, so they must not zero out the margin). It is capped
#      at 1.0, which also covers a runner-up region scoring ~0.
#  10. stability.runs keeps the last 10 runs; streak counts trailing
#      runs with the same top candidate.
#  11. ADD needs an unserved leader region, a streak of stability_runs, and
#      at least add_min_window_sessions window sessions; a single margin lead
#      is never enough. It is suppressed while the measured network shows a
#      negative-saving share above max_negative_saving_share once at least
#      min_measured_samples saved-app samples exist (rule 14). MOVE needs
#      margin >= move_margin, stability, and some existing relay whose
#      removal retains at least move_coverage_keep of every kept cell's
#      current cost.
#  12. Candidates within near_duplicate_ms of an existing relay are
#      rejected as near_duplicate.
#  13. Every existing relay is reported in relay_necessity with its window
#      session count, the coverage retention if it were dropped, and its
#      measured saved-app mean / negative-saving share; prune_candidates
#      lists relays that are both idle and redundant.
#  14. The measured block aggregates window saved-app samples, mean, and
#      negative-saving share plus route jitter mean and loss ratio across
#      existing relays, so a human can see whether relays actually help.
#      With no measured samples the ADD gate is inert and existing
#      decisions stand.
#  15. The per-source measured block rebuilds reset-safe saved-app deltas
#      per (relay, coarse source region) across the window pairs, exactly
#      as the demand matrix rebuilds geo. A cell is reported only once it
#      clears the k=3 report floor the relay already applies; thinner cells
#      are withheld and counted in source_quality.suppressed_cells, and
#      every reported row carries its sample count and meets_min_samples
#      (>= min_measured_samples). The block is advisory and never changes
#      the ADD/MOVE decision.
#
# Requires: bash, jq (>= 1.6 for sin/cos/asin/sqrt).
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
GEO_DIR_DEFAULT="$SCRIPT_DIR/../geo"
_repo_root="$(cd "$SCRIPT_DIR/../.." 2>/dev/null && pwd || true)"
REGISTRY_DEFAULT="${_repo_root:+$_repo_root/web/registry.json}"

HISTORY_PATH=""
PREVIOUS_PATH=""
REGISTRY_PATH=""
GEO_DIR=""
OUT_PATH=""
WINDOW_SECS=""
NOW="$(date +%s)"

# ── Argument parsing (never fatal) ───────────────────────────
val=""
while [ "$#" -gt 0 ]; do
	key="$1"
	case "$key" in
	--history | --previous | --registry | --geo-dir | --out | --window-secs)
		if [ "$#" -ge 2 ]; then
			val="$2"
			shift 2
		else
			val=""
			shift
		fi
		;;
	-h | --help)
		sed -n '2,20p' "${BASH_SOURCE[0]}" | sed 's/^# \{0,1\}//'
		exit 0
		;;
	*)
		val=""
		shift
		;;
	esac
	case "$key" in
	--history) [ -n "$val" ] && HISTORY_PATH="$val" ;;
	--previous) [ -n "$val" ] && PREVIOUS_PATH="$val" ;;
	--registry) [ -n "$val" ] && REGISTRY_PATH="$val" ;;
	--geo-dir) [ -n "$val" ] && GEO_DIR="$val" ;;
	--out) [ -n "$val" ] && OUT_PATH="$val" ;;
	--window-secs) [ -n "$val" ] && WINDOW_SECS="$val" ;;
	esac
done

[ -n "$GEO_DIR" ] || GEO_DIR="$GEO_DIR_DEFAULT"
[ -n "$REGISTRY_PATH" ] || REGISTRY_PATH="$REGISTRY_DEFAULT"

# ── Minimal documents (valid JSON, no jq required) ───────────
STATIC_MINIMAL='{"schema_version":1,"generated_at":0,"status":"INSUFFICIENT_DATA","window":{"from_t":0,"to_t":0,"snapshots":0,"sessions":0,"cells":0,"min_window_sessions":20,"min_cell_sessions":3},"matrix":[],"existing":[],"ranking":[],"rejected":[],"recommendation":{"action":"NONE","candidate_id":null,"remove_node_id":null,"reason":"insufficient data"},"stability":{"top_id":null,"streak":0,"required":3,"runs":[]},"measured":{"saved_app_samples":0,"saved_app_mean_ms":null,"saved_app_negative_share":null,"route_jitter_mean_ms":null,"route_loss_ratio":null,"min_measured_samples":0,"max_negative_saving_share":0},"source_quality":{"min_samples":3,"min_measured_samples":0,"suppressed_cells":0,"relays":[]},"notes":"no data"}'
DEFAULT_PARAMS='{"alpha":2,"window_secs":604800,"min_window_sessions":20,"min_cell_sessions":3,"stability_runs":3,"add_margin":0.10,"add_min_window_sessions":50,"move_margin":0.20,"redundancy_weight":0.5,"move_coverage_keep":0.9,"proximity_ms_floor":15,"near_duplicate_ms":25,"idle_sessions_max":0,"max_negative_saving_share":0.5,"min_measured_samples":5}'

write_json() {
	local json="$1" minimal="$2"
	if command -v jq >/dev/null 2>&1; then
		if ! printf '%s' "$json" | jq -e . >/dev/null 2>&1; then
			json="$minimal"
		fi
	fi

	if [ -z "$OUT_PATH" ]; then
		printf '%s\n' "$json"
		return 0
	fi

	mkdir -p "$(dirname "$OUT_PATH")" 2>/dev/null || true
	local tmp
	tmp="$(mktemp "$(dirname "$OUT_PATH")/.recommend-regions.XXXXXX" 2>/dev/null || true)"
	if [ -n "$tmp" ]; then
		if printf '%s\n' "$json" >"$tmp" 2>/dev/null; then
			mv "$tmp" "$OUT_PATH" 2>/dev/null && return 0
		fi
		rm -f "$tmp" 2>/dev/null || true
	fi
	printf '%s\n' "$json" >"$OUT_PATH" 2>/dev/null || true
	return 0
}

if ! command -v jq >/dev/null 2>&1; then
	write_json "$STATIC_MINIMAL" "$STATIC_MINIMAL"
	exit 0
fi

# ── Input loading (every failure degrades to a default) ──────
read_json() {
	# read_json <path> -> compact JSON on stdout, non-zero on failure
	local path="${1:-}"
	[ -n "$path" ] || return 1
	[ -f "$path" ] || return 1
	[ -s "$path" ] || return 1
	jq -c '.' "$path" 2>/dev/null
}

# Params + candidates from the geo catalog.
params_json="$DEFAULT_PARAMS"
candidates_json='[]'
if cand_doc="$(read_json "$GEO_DIR/candidates.json")"; then
	if jq -e '(.params | type) == "object"' <<<"$cand_doc" >/dev/null 2>&1; then
		merged="$(jq -c --argjson d "$DEFAULT_PARAMS" \
			'$d * ((.params // {}) | with_entries(select(.value != null)))' \
			<<<"$cand_doc" 2>/dev/null || true)"
		[ -n "$merged" ] && params_json="$merged"
	fi
	if jq -e '(.candidates | type) == "array"' <<<"$cand_doc" >/dev/null 2>&1; then
		candidates_json="$(jq -c '
            [ .candidates[]
              | select(type == "object"
                       and ((.id // "") | type) == "string" and (.id | length) > 0
                       and ((.lat | type) == "number")
                       and ((.lon | type) == "number"))
              | {id, region: (.region // ""), lat, lon,
                 provider: (.provider // ""), free_tier: (.free_tier // false),
                 viable: (.viable // true)}
            ]' <<<"$cand_doc" 2>/dev/null || echo '[]')"
	fi
fi

# --window-secs override (numeric only).
if [ -n "$WINDOW_SECS" ]; then
	case "$WINDOW_SECS" in
	'' | *[!0-9]*) : ;;
	*)
		params_json="$(jq -c --argjson w "$WINDOW_SECS" '.window_secs = $w' \
			<<<"$params_json" 2>/dev/null || printf '%s' "$params_json")"
		;;
	esac
fi

# Region catalog.
geo_json='{"schema_version":1,"regions":{},"countries":{},"region_aliases":{},"relays":{}}'
if geo_doc="$(read_json "$GEO_DIR/regions.json")"; then
	if jq -e '(.regions | type) == "object"' <<<"$geo_doc" >/dev/null 2>&1; then
		geo_json="$geo_doc"
	fi
fi

# Registry: pull the embedded signed payload, keep locatable fields.
nodes_json='[]'
if reg_doc="$(read_json "$REGISTRY_PATH")"; then
	nodes_json="$(jq -c '
        ((try (.registry | fromjson | (.nodes // [])) catch [])
         | [ .[]
             | select(type == "object"
                      and ((.node_id // "") | type) == "string"
                      and (.node_id | length) > 0)
             | {node_id, region: (.region // ""), data_addr: (.data_addr // "")}
           ])' <<<"$reg_doc" 2>/dev/null || echo '[]')"
fi

# History.
hist_json='{"version":1,"snapshots":[]}'
if hist_doc="$(read_json "$HISTORY_PATH")"; then
	if jq -e '((.snapshots // []) | type) == "array"' <<<"$hist_doc" >/dev/null 2>&1; then
		normalized="$(jq -c '{version:1, generated_at: (.generated_at // 0),
                              snapshots: [.snapshots[] | select(type == "object")]}' \
			<<<"$hist_doc" 2>/dev/null || true)"
		[ -n "$normalized" ] && hist_json="$normalized"
	fi
fi

# Previous recommender output (stability history only).
prev_json='{"stability":{"runs":[]}}'
if [ -n "$PREVIOUS_PATH" ] && [ "$PREVIOUS_PATH" != "missing" ]; then
	if prev_doc="$(read_json "$PREVIOUS_PATH")"; then
		if jq -e '((.stability.runs // []) | type) == "array"' <<<"$prev_doc" >/dev/null 2>&1; then
			prev_norm="$(jq -c '{stability:{runs: [(.stability.runs // [])[] | select(type == "object")]}}' \
				<<<"$prev_doc" 2>/dev/null || true)"
			[ -n "$prev_norm" ] && prev_json="$prev_norm"
		fi
	fi
fi

# Minimal document that respects the loaded params.
minimal_json="$(jq -cn --argjson p "$params_json" --argjson now "$NOW" '{
    schema_version: 1,
    generated_at: $now,
    status: "INSUFFICIENT_DATA",
    window: {from_t: 0, to_t: 0, snapshots: 0, sessions: 0, cells: 0,
             min_window_sessions: ($p.min_window_sessions // 20),
             min_cell_sessions: ($p.min_cell_sessions // 3)},
    matrix: [], existing: [], ranking: [], rejected: [],
    recommendation: {action: "NONE", candidate_id: null, remove_node_id: null,
                     reason: "insufficient data"},
    stability: {top_id: null, streak: 0, required: ($p.stability_runs // 3), runs: []},
    measured: {saved_app_samples: 0, saved_app_mean_ms: null,
               saved_app_negative_share: null, route_jitter_mean_ms: null,
               route_loss_ratio: null,
               min_measured_samples: ($p.min_measured_samples // 5),
               max_negative_saving_share: ($p.max_negative_saving_share // 0.5)},
    source_quality: {min_samples: 3,
                     min_measured_samples: ($p.min_measured_samples // 5),
                     suppressed_cells: 0, relays: []},
    notes: "fatal parse failure or internal error; emitted minimal document"
}' 2>/dev/null || printf '%s' "$STATIC_MINIMAL")"

# ── Main scorer ──────────────────────────────────────────────
MAIN_JQ='
def n: (try tonumber catch 0);
def pi: 3.141592653589793;
def rad: . * pi / 180;
def r6: (if type == "number" then ((. * 1000000) | round) / 1000000 else . end);
def clamp01: (if . < 0 then 0 elif . > 1 then 1 else . end);
def hav_km($lat1; $lon1; $lat2; $lon2):
  ($lat1 | rad) as $p1
  | ($lat2 | rad) as $p2
  | ((($p2 - $p1) / 2) | sin) as $sdp
  | (((($lon2 - $lon1) | rad) / 2) | sin) as $sdl
  | ((($sdp * $sdp) + (($p1 | cos) * ($p2 | cos) * $sdl * $sdl))) as $a
  | (2 * 6371 * (((($a | clamp01) | sqrt)) | asin));
def leg_ms($alat; $alon; $blat; $blon): hav_km($alat; $alon; $blat; $blon) * 0.01;
def parse_cell($key):
  ($key | split("-")) as $p
  | if (($p | length) == 2) and (($p[0] | length) > 0) and (($p[1] | length) > 0)
    then {src: $p[0], dst: $p[1]}
    else null end;
def streak_of($runs; $top_id):
  (reduce ($runs | reverse)[] as $run ([0, true];
     if (.[1] and ($run.top_id == $top_id)) then [.[0] + 1, true]
     else [.[0], false] end)) | .[0];
def sum_relay_metric($snaps; $k):
  (reduce ($snaps[]? | (.per_relay // {}) | to_entries[]) as $e
     ({}; .[$e.key] = ((.[$e.key] // 0)
                       + ((($e.value.delta // {}) | .[$k]) // 0))));

($params.alpha // 2 | n) as $alpha
| ($params.window_secs // 604800 | n) as $window
| ($params.min_window_sessions // 20 | n) as $min_sessions
| ($params.min_cell_sessions // 3 | n) as $min_cell
| ($params.stability_runs // 3 | n) as $stability_runs
| ($params.add_margin // 0.10 | n) as $add_margin
| ($params.add_min_window_sessions // 50 | n) as $add_min_window_sessions
| ($params.move_margin // 0.20 | n) as $move_margin
| ($params.redundancy_weight // 0.5 | n) as $redundancy_weight
| ($params.move_coverage_keep // 0.9 | n) as $coverage_keep
| ($params.idle_sessions_max // 0 | n) as $idle_sessions_max
| ($params.near_duplicate_ms // 25 | n) as $near_dup
| ($params.max_negative_saving_share // 0.5 | n) as $max_neg_saving_share
| ($params.min_measured_samples // 5 | n) as $min_measured_samples

# ── Snapshot window ──────────────────────────────────────────
| (($hist.snapshots // [])
   | map(select(type == "object" and ((.t | type) == "number")))) as $raw_snaps
| ($raw_snaps | sort_by(.t | n)) as $sorted_snaps
| ($sorted_snaps | length) as $sorted_n
| (if $sorted_n > 0 then ($sorted_snaps[$sorted_n - 1].t | n) else 0 end) as $latest_t
| ($sorted_snaps | map(select((.t | n) >= ($latest_t - $window)))) as $snaps
| ($snaps | length) as $snap_count
| (if $snap_count > 0 then ($snaps[0].t | n) else 0 end) as $from_t
| (if $snap_count > 0 then ($snaps[$snap_count - 1].t | n) else 0 end) as $to_t
| [range(0; (if $snap_count >= 2 then $snap_count - 1 else 0 end)) as $i
   | {prev: $snaps[$i], cur: $snaps[$i + 1]}] as $pairs

# ── Demand matrix M[src][dst] ────────────────────────────────
| (reduce $pairs[] as $pair (
     {};
     reduce (($pair.cur.per_relay // {}) | to_entries[]) as $entry (
       .;
       ($entry.key) as $relay_id
       | ($entry.value) as $relay_cur
       | if (($relay_cur.reachable // true) == false) then .
         else
           reduce (($relay_cur.geo // {}) | to_entries[]) as $geo_entry (
             .;
             (parse_cell($geo_entry.key)) as $cell
             | if $cell == null then .
               else
                 ($geo.regions[$cell.src] // null) as $sreg
                 | ($geo.regions[$cell.dst] // null) as $dreg
                 | if ($sreg == null or $dreg == null) then .
                   else
                     ($geo_entry.value | n) as $cur_v
                     | ((($pair.prev.per_relay // {})[$relay_id].geo // {})[$geo_entry.key] // 0 | n) as $prev_v
                     | (if (($relay_cur.reset // false) == true) then $cur_v
                        else (if $cur_v > $prev_v then $cur_v - $prev_v else 0 end) end) as $delta
                     | if $delta <= 0 then .
                       else .[$cell.src][$cell.dst] = (((.[$cell.src][$cell.dst]) // 0) + $delta)
                       end
                   end
               end
           )
         end
     )
   )) as $m_raw

# Kept cells (>= min_cell_sessions), sorted for determinism.
| ($m_raw
   | to_entries
   | map(.key as $src | (.value | to_entries[]) | {src: $src, dst: .key, sessions: (.value | n)})
   | map(select(.sessions >= $min_cell))
   | sort_by([.src, .dst])) as $cells
| ($cells | map(.sessions) | add // 0) as $window_sessions
| (if ($snap_count >= 2 and $window_sessions >= $min_sessions)
   then "OK" else "INSUFFICIENT_DATA" end) as $status

# ── Smoothing: M[s][d] + alpha * rowsum[s] ───────────────────
| ($cells | group_by(.src)
   | map({key: .[0].src, value: (map(.sessions) | add)}) | from_entries) as $row_sums
| ($cells | map(. + {weight: ((.sessions | n) + ($alpha * (($row_sums[.src]) // 0 | n)))})) as $weighted

# Annotate each cell with its region coordinates.
| ($weighted | map(
     ($geo.regions[.src] // {}) as $s
     | ($geo.regions[.dst] // {}) as $d
     | . + {s_lat: ($s.lat | n), s_lon: ($s.lon | n),
            d_lat: ($d.lat | n), d_lon: ($d.lon | n)}
   )) as $gcells

# ── Existing relay positions ─────────────────────────────────
| ($nodes | map(
     . as $node
     | (($geo.region_aliases[$node.region]) // $node.region) as $canon
     | (($geo.relays[$node.node_id]) // null) as $rp
     | if $rp != null then
         {node_id: $node.node_id, region: $canon, lat: ($rp.lat | n), lon: ($rp.lon | n)}
       else
         ($geo.regions[$canon] // null) as $reg
         | if $reg != null then
             {node_id: $node.node_id, region: $canon, lat: ($reg.lat | n), lon: ($reg.lon | n)}
           else empty end
       end
   ) | sort_by(.node_id)) as $existing

# ── Near-duplicate rejection ─────────────────────────────────
| ($cands | map(
     . as $c
     | (($c.lat | n)) as $clat
     | (($c.lon | n)) as $clon
     | ([ $existing[] | select(leg_ms($clat; $clon; .lat; .lon) <= $near_dup) ] | length) as $near_n
     | {c: $c, near_n: $near_n}
   )) as $cand_checked
| ([ $cand_checked[] | select(.near_n > 0)
     | {candidate_id: .c.id, reason: "near_duplicate"}] | sort_by(.candidate_id)) as $rejected
| ([ $cand_checked[] | select(.near_n == 0) | .c ]) as $cand_ok

# ── Candidate scoring ────────────────────────────────────────
| ([ $cand_ok[] as $c
     | (($c.lat | n)) as $clat
     | (($c.lon | n)) as $clon
     | (reduce $gcells[] as $cell (
          {coverage_gain: 0, redundancy_gain: 0};
          ($cell.s_lat) as $slat | ($cell.s_lon) as $slon
          | ($cell.d_lat) as $dlat | ($cell.d_lon) as $dlon
          | ([ $existing[]
               | leg_ms($slat; $slon; .lat; .lon) + leg_ms(.lat; .lon; $dlat; $dlon) ]) as $legs
          | (if ($legs | length) == 0 then null else ($legs | min) end) as $ce
          | (leg_ms($slat; $slon; $clat; $clon) + leg_ms($clat; $clon; $dlat; $dlon)) as $cc
          | ($cell.weight | n) as $w
          | if $ce == null then
              .coverage_gain += $w
            else
              (if $cc < $ce then $cc else $ce end) as $cw
              | ((if $ce > $cc then ($ce - $cc) else 0 end) / (if $ce > 1 then $ce else 1 end)) as $factor
              | .coverage_gain += ($w * $factor)
              | (([ $legs[] | select(. <= ($ce * 1.1)) ] | length) == 1) as $exactly_one
              | (if ($exactly_one and ($cc <= ($ce * 1.1))) then .redundancy_gain += $w else . end)
            end
        )) as $metrics
     | {candidate_id: $c.id,
        region: ($c.region // ""),
        coverage_gain: (($metrics.coverage_gain) | r6),
        redundancy_gain: (($metrics.redundancy_gain) | r6),
        score: (($metrics.coverage_gain + ($redundancy_weight * $metrics.redundancy_gain)) | r6)}
   ]) as $ranking0
| ($ranking0 | sort_by([(-.score), .candidate_id])) as $ranking
| (if ($ranking | length) > 0 then $ranking[0] else null end) as $top
| (if $top == null then null
   else (([ $ranking[] | select(.region != $top.region) ] | .[0]) // null)
   end) as $second
| (if $top == null or $second == null then null
   elif $second.score < 0.001 then 1
   else (($top.score - $second.score) / $second.score
         | if . > 1 then 1 else . end)
   end) as $margin_raw
| (if $margin_raw == null then null else ($margin_raw | r6) end) as $margin
| (if $margin == null then false else true end) as $has_margin
| ($ranking | map(. + {margin: $margin})) as $ranking_out

# ── MOVE feasibility ─────────────────────────────────────────
# For each existing relay, find the worst per-cell retention of the
# pre-removal cost if that relay is removed: retention = pre / post,
# where post is the minimum path cost over the remaining relays. A
# removal is feasible when even the worst retention >= move_coverage_keep.
| ([ $existing[] as $r
     | {node_id: $r.node_id,
        worst_retention: ([ $gcells[] as $cell
            | ([ $existing[] | select(.node_id != $r.node_id)
                 | leg_ms($cell.s_lat; $cell.s_lon; .lat; .lon)
                   + leg_ms(.lat; .lon; $cell.d_lat; $cell.d_lon) ]) as $post_legs
            | ([ $existing[]
                 | leg_ms($cell.s_lat; $cell.s_lon; .lat; .lon)
                   + leg_ms(.lat; .lon; $cell.d_lat; $cell.d_lon) ]) as $pre_legs
            | (if ($pre_legs | length) == 0 then null else ($pre_legs | min) end) as $pre
            | (if ($post_legs | length) == 0 then null else ($post_legs | min) end) as $post
            | (if $pre == null then 1
               elif $post == null then 0
               elif $post <= 0 then 1
               elif $pre <= 0 then 0
               else ($pre / $post) end)
          ] | (min // 0))}
   ]) as $removals
| ([ $removals[] | select(.worst_retention >= $coverage_keep and .worst_retention > 0) ]
   | sort_by([(-.worst_retention), .node_id]) | .[0] // null) as $move_target

# ── Relay necessity ──────────────────────────────────────────
# Pairs each relay window session count with the coverage it uniquely
# provides (retention if that relay were dropped). A relay that is both
# idle and redundant by those two measures is a prune candidate.
| (reduce ($snaps[]?.per_relay // {} | to_entries[]) as $e
    ({}; .[$e.key] = ((.[$e.key] // 0) + ($e.value.delta.sessions_created // 0)))) as $relay_sessions
| (sum_relay_metric($snaps; "saved_app_ms_sum")) as $saved_sum
| (sum_relay_metric($snaps; "saved_app_ms_count")) as $saved_count
| (sum_relay_metric($snaps; "saved_app_ms_negative_count")) as $saved_neg
| (sum_relay_metric($snaps; "route_jitter_ms_sum")) as $route_jitter_sum
| (sum_relay_metric($snaps; "route_jitter_ms_count")) as $route_jitter_count
| (sum_relay_metric($snaps; "route_lost")) as $route_lost_m
| (sum_relay_metric($snaps; "route_recovered")) as $route_recovered_m
| (($saved_count | [.[]] | add) // 0) as $meas_samples
| (($saved_sum | [.[]] | add) // 0) as $meas_sum
| (($saved_neg | [.[]] | add) // 0) as $meas_neg
| (if $meas_samples > 0 then (($meas_sum / $meas_samples) | r6) else null end) as $meas_mean
| (if $meas_samples > 0 then (($meas_neg / $meas_samples) | r6) else null end) as $meas_neg_share
| (($meas_samples >= $min_measured_samples) and ($meas_neg_share != null)
   and ($meas_neg_share > $max_neg_saving_share)) as $quality_blocked
| (($route_jitter_sum | [.[]] | add) // 0) as $jitter_sum
| (($route_jitter_count | [.[]] | add) // 0) as $jitter_count
| (if $jitter_count > 0 then (($jitter_sum / $jitter_count) | r6) else null end) as $jitter_mean
| (($route_lost_m | [.[]] | add) // 0) as $lost_sum
| (($route_recovered_m | [.[]] | add) // 0) as $rec_sum
| (if ($lost_sum + $rec_sum) > 0 then (($lost_sum / ($lost_sum + $rec_sum)) | r6) else null end) as $loss_ratio
| ([ $existing[] as $r
     | (($saved_count[$r.node_id] // 0) | n) as $r_count
     | {node_id: $r.node_id,
        region: $r.region,
        sessions: (($relay_sessions[$r.node_id] // 0) | n),
        worst_retention: (([ $removals[] | select(.node_id == $r.node_id) | .worst_retention ] | .[0]) // 1),
        saved_app_samples: $r_count,
        saved_app_mean_ms: (if $r_count > 0 then ((($saved_sum[$r.node_id] // 0) / $r_count) | r6) else null end),
        saved_app_negative_share: (if $r_count > 0 then ((($saved_neg[$r.node_id] // 0) / $r_count) | r6) else null end)}
   ] | sort_by(.node_id)) as $relay_necessity
| ([ $relay_necessity[]
     | select((.sessions // 0) <= $idle_sessions_max and .worst_retention >= $coverage_keep)
     | .node_id ]) as $prune_candidates

# ── Per-source measured quality (rule 15) ────────────────────
| ([ $pairs[] as $pair
     | (($pair.cur.per_relay // {}) | to_entries[]) as $re
     | ($re.value) as $rc
     | if (($rc.reachable // true) == false) then empty
       else
         (($rc.sources // {}) | to_entries[]) as $se
         | ((($pair.prev.per_relay // {})[$re.key] // {}).sources // {}) as $prev_src
         | (($prev_src[$se.key]) // {}) as $pc
         | ($se.value // {}) as $sc
         | (($rc.reset // false) == true) as $rst
         | {rid: $re.key, src: $se.key,
            sum_d: (if $rst then ($sc.saved_app_ms_sum // 0)
                    elif (($sc.saved_app_ms_sum // 0) > ($pc.saved_app_ms_sum // 0))
                    then (($sc.saved_app_ms_sum // 0) - ($pc.saved_app_ms_sum // 0))
                    else 0 end),
            count_d: (if $rst then ($sc.saved_app_ms_count // 0)
                      elif (($sc.saved_app_ms_count // 0) > ($pc.saved_app_ms_count // 0))
                      then (($sc.saved_app_ms_count // 0) - ($pc.saved_app_ms_count // 0))
                      else 0 end),
            neg_d: (if $rst then ($sc.saved_app_ms_negative_count // 0)
                    elif (($sc.saved_app_ms_negative_count // 0) > ($pc.saved_app_ms_negative_count // 0))
                    then (($sc.saved_app_ms_negative_count // 0) - ($pc.saved_app_ms_negative_count // 0))
                    else 0 end)}
       end
   ]) as $src_records
| ($src_records
   | group_by([.rid, .src])
   | map({rid: .[0].rid, src: .[0].src,
          sum: (map(.sum_d) | add // 0),
          count: (map(.count_d) | add // 0),
          neg: (map(.neg_d) | add // 0)})) as $src_totals
| 3 as $src_k_min
| ([ $existing[] as $r
     | {node_id: $r.node_id,
        region: $r.region,
        sources: ([ $src_totals[]
                    | select(.rid == $r.node_id and (.count | n) >= $src_k_min)
                    | {src_region: .src,
                       saved_app_samples: (.count | n),
                       saved_app_mean_ms: (if (.count | n) > 0
                                           then (((.sum | n) / (.count | n)) | r6)
                                           else null end),
                       saved_app_negative_share: (if (.count | n) > 0
                                                  then (((.neg | n) / (.count | n)) | r6)
                                                  else null end),
                       meets_min_samples: ((.count | n) >= $min_measured_samples)}
                  ] | sort_by(.src_region)),
        sources_suppressed: ([ $src_totals[]
                               | select(.rid == $r.node_id
                                        and (.count | n) > 0
                                        and (.count | n) < $src_k_min) ] | length)}
   ]) as $source_quality
| ([ $source_quality[].sources_suppressed ] | add // 0) as $source_suppressed

# ── Stability ────────────────────────────────────────────────
| (($prev.stability.runs // []) | .[-10:]) as $prev_runs
| ({t: $to_t,
    top_id: (if $top == null then null else $top.candidate_id end),
    score: (if $top == null then 0 else $top.score end),
    margin: $margin}) as $this_run
| (($prev_runs + [$this_run]) | .[-10:]) as $runs
| (streak_of($runs; (if $top == null then null else $top.candidate_id end))) as $streak

# ── Decision ─────────────────────────────────────────────────
| ($existing | map(.region) | unique) as $served
| (if $top == null then null
   else (($geo.region_aliases[($top.region // "")]) // ($top.region // "")) end) as $top_region
| (if $margin == null then "none" else ($margin | tostring) end) as $margin_display
| (if $status != "OK" then
     {action: "NONE", candidate_id: null, remove_node_id: null,
      reason: (if $snap_count < 2
               then "fewer than 2 snapshots in the window"
               else "window sessions \($window_sessions) below minimum \($min_sessions)" end)}
   elif ($top != null and (($served | index($top_region)) == null)
         and $streak >= $stability_runs
         and $window_sessions >= $add_min_window_sessions
         and $quality_blocked) then
     {action: "NONE", candidate_id: null, remove_node_id: null,
      reason: "measured saved-app negative share \($meas_neg_share) over \($meas_samples) sample(s) exceeds max_negative_saving_share \($max_neg_saving_share); ADD of \($top.candidate_id) suppressed"}
   elif ($top != null and (($served | index($top_region)) == null)
         and $streak >= $stability_runs
         and $window_sessions >= $add_min_window_sessions) then
     {action: "ADD", candidate_id: $top.candidate_id, remove_node_id: null,
      reason: "unserved leader \($top.candidate_id) stable for \($streak) run(s) with \($window_sessions) window sessions >= \($add_min_window_sessions)"}
   elif ($top != null and $has_margin and $margin >= $move_margin and $streak >= $stability_runs
         and $move_target != null) then
     {action: "MOVE", candidate_id: $top.candidate_id, remove_node_id: $move_target.node_id,
      reason: "leader \($top.candidate_id) has margin \($margin_display) >= move_margin \($move_margin) and \($move_target.node_id) is removable without losing \($coverage_keep) coverage"}
   else
     {action: "NONE", candidate_id: null, remove_node_id: null,
      reason: (if $top == null
               then "no candidate survived near-duplicate filtering"
               else "leader margin \($margin_display) and streak \($streak) below the add/move gates" end)}
   end) as $recommendation

# ── Document ─────────────────────────────────────────────────
| {
    schema_version: 1,
    generated_at: ($now | n),
    status: $status,
    window: {
      from_t: $from_t,
      to_t: $to_t,
      snapshots: $snap_count,
      sessions: $window_sessions,
      cells: ($cells | length),
      min_window_sessions: $min_sessions,
      min_cell_sessions: $min_cell
    },
    matrix: ($cells | map({src, dst, sessions})),
    existing: $existing,
    relay_necessity: $relay_necessity,
    prune_candidates: $prune_candidates,
    ranking: $ranking_out,
    rejected: $rejected,
    recommendation: $recommendation,
    stability: {
      top_id: (if $top == null then null else $top.candidate_id end),
      streak: $streak,
      required: $stability_runs,
      runs: $runs
    },
    measured: {
      saved_app_samples: $meas_samples,
      saved_app_mean_ms: $meas_mean,
      saved_app_negative_share: $meas_neg_share,
      route_jitter_mean_ms: $jitter_mean,
      route_loss_ratio: $loss_ratio,
      min_measured_samples: $min_measured_samples,
      max_negative_saving_share: $max_neg_saving_share
    },
    source_quality: {
      min_samples: $src_k_min,
      min_measured_samples: $min_measured_samples,
      suppressed_cells: $source_suppressed,
      relays: $source_quality
    },
    notes: ("window \($window)s; \($snap_count) snapshot(s); \(($cells | length)) kept cell(s); \($window_sessions) session(s); \(($cand_ok | length)) ranked candidate(s), \(($rejected | length)) rejected; geo-dir \($geo_dir); history \($history_path)")
  }
'

# Inputs go through files, not --argjson: the accumulated history eventually
# exceeds ARG_MAX ("Argument list too long"), which silently produced the
# INSUFFICIENT_DATA fallback on real data while the small test fixtures passed.
tmpdir="$(mktemp -d 2>/dev/null || mktemp -d -t lightspeed-rec)"
printf '%s' "$hist_json" >"$tmpdir/hist.json"
printf '%s' "$geo_json" >"$tmpdir/geo.json"
printf '%s' "$candidates_json" >"$tmpdir/cands.json"
printf '%s' "$nodes_json" >"$tmpdir/nodes.json"
printf '%s' "$prev_json" >"$tmpdir/prev.json"

PROLOGUE='($histf[0]) as $hist | ($geof[0]) as $geo | ($candsf[0]) as $cands | ($nodesf[0]) as $nodes | ($prevf[0]) as $prev | '

result="$(jq -cn \
	--slurpfile histf "$tmpdir/hist.json" \
	--slurpfile geof "$tmpdir/geo.json" \
	--slurpfile candsf "$tmpdir/cands.json" \
	--slurpfile nodesf "$tmpdir/nodes.json" \
	--slurpfile prevf "$tmpdir/prev.json" \
	--argjson params "$params_json" \
	--argjson now "$NOW" \
	--arg geo_dir "$GEO_DIR" \
	--arg history_path "${HISTORY_PATH:-<none>}" \
	"${PROLOGUE}${MAIN_JQ}" 2>/dev/null || true)"
rm -rf "$tmpdir"

[ -n "$result" ] || result="$minimal_json"
write_json "$result" "$minimal_json"
exit 0
