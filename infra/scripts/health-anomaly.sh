#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Fleet Anomaly Monitor
#
# Reads the collector history (collect-metrics.sh output, e.g. the
# stats-branch web/network-history.json) plus the signed registry and
# the live /health endpoint, and flags silent fleet degradation that
# the liveness /health gate cannot see.
#
# Detectors (all from data the repo already collects; no new metrics):
#   (a) zero_relay                relay reachable for the whole window
#                                 but relayed 0 packets while the fleet
#                                 was busy (the 1.6.3 outage signature)
#   (b) auth_spike_no_sessions    auth_rejections climb while
#                                 sessions_created stays flat (the
#                                 data-only-proxy signature)
#   (c) version_lag               a relay behind the fleet's majority
#                                 version while the fleet moved on
#   (d) saved_regression          ping-saved drops hard vs the relay's
#                                 own recent baseline
#   (d) negative_saving_spike     negative-saving share spikes
#   (e) relay_missing_from_registry / relay_health_failed
#
# The window is sustained (default 3 snapshots), never a single
# snapshot, so a legitimately idle relay is not mistaken for an outage.
# An all-idle fleet raises nothing.
#
# Usage:
#   health-anomaly.sh [--history PATH] [--registry PATH] [--window N]
#                     [--baseline N] [--no-probe] [--json]
#                     [--timeout SECS]
#
# Exit codes:
#   0  no anomaly
#   1  at least one anomaly (after posting to Discord when configured)
#   2  usage error
#
# Alerts to Discord when DISCORD_WEBHOOK is set, exactly like the
# existing health monitor. Safe to run against production repeatedly:
# it only reads local history and issues read-only /health probes.
#
# Requires: bash, curl, jq.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# ── Defaults ─────────────────────────────────────────────────
WINDOW=3       # snapshots that must show the anomaly (sustained)
BASELINE=5     # prior snapshots used for a relay's own baseline
NO_PROBE=false # curl /health unless asked not to
JSON_OUT=false
TIMEOUT=5
REGISTRY_OVERRIDE=""
HISTORY_OVERRIDE=""

AUTH_MIN=100       # window auth_rejections floor for a spike
AUTH_FACTOR=3      # window auth must be this multiple of the baseline
SESSION_FLAT_MAX=0 # sessions_created growth still considered "flat"
MIN_SAVED_SAMPLES=5
SAVED_REGRESSION=0.5
NEG_SHARE_MAX=0.5
NEG_MIN=5
MIN_VERSION_AHEAD=2

usage() {
	cat <<'EOF'
Usage: health-anomaly.sh [options]

  --history PATH    Collector history document (default:
                    $LIGHTSPEED_MESH_HISTORY or web/network-history.json)
  --registry PATH   Signed registry (default: $LIGHTSPEED_REGISTRY_PATH
                    or web/registry.json)
  --window N        Sustained window in snapshots (default 3)
  --baseline N      Prior snapshots for a relay's own baseline (default 5)
  --no-probe        Do not curl /health; use the latest history reachability
  --json            Emit the anomaly document as JSON on stdout
  --timeout SECS    Per-probe curl timeout (default 5)
  -h, --help        Show this help

Alerts to Discord when DISCORD_WEBHOOK is set.
Requires: bash, curl, jq.
EOF
}

while [ $# -gt 0 ]; do
	case "$1" in
	--history)
		shift
		HISTORY_OVERRIDE="${1:-}"
		;;
	--registry)
		shift
		REGISTRY_OVERRIDE="${1:-}"
		;;
	--window)
		shift
		WINDOW="${1:-3}"
		;;
	--baseline)
		shift
		BASELINE="${1:-5}"
		;;
	--no-probe) NO_PROBE=true ;;
	--json) JSON_OUT=true ;;
	--timeout)
		shift
		TIMEOUT="${1:-5}"
		;;
	-h | --help)
		usage
		exit 0
		;;
	-*)
		printf 'health-anomaly: unknown option: %s\n' "$1" >&2
		usage >&2
		exit 2
		;;
	*)
		printf 'health-anomaly: unexpected argument: %s\n' "$1" >&2
		usage >&2
		exit 2
		;;
	esac
	shift
done

# Numeric guards so a bad flag cannot produce a nonsense window.
case "$WINDOW" in '' | *[!0-9]*) WINDOW=3 ;; esac
case "$BASELINE" in '' | *[!0-9]*) BASELINE=5 ;; esac
case "$TIMEOUT" in '' | *[!0-9]*) TIMEOUT=5 ;; esac

# ── Resolve inventory through the shared resolver ────────────
if [ -n "$REGISTRY_OVERRIDE" ]; then
	export LIGHTSPEED_REGISTRY_PATH="$REGISTRY_OVERRIDE"
fi
# shellcheck source=lib-nodes.sh
source "$SCRIPT_DIR/lib-nodes.sh"

REPO_ROOT="$LIGHTSPEED_REPO_ROOT"
HISTORY_PATH="${HISTORY_OVERRIDE:-${LIGHTSPEED_MESH_HISTORY:-$REPO_ROOT/web/network-history.json}}"

# ── Normalize the history; absence is not an anomaly by itself ──
HIST_TMP="$(mktemp 2>/dev/null || mktemp -t lightspeed-anomaly)"
trap 'rm -f "$HIST_TMP" 2>/dev/null || true' EXIT

hist_json='{"version":1,"generated_at":0,"snapshots":[]}'
if [ -n "$HISTORY_PATH" ] && [ -f "$HISTORY_PATH" ] && [ -s "$HISTORY_PATH" ]; then
	candidate="$(jq -c '{
		version: (.version // 1),
		generated_at: (.generated_at // 0),
		snapshots: ((.snapshots // []) | if type == "array" then . else [] end)
	}' "$HISTORY_PATH" 2>/dev/null || true)"
	if [ -n "$candidate" ]; then
		hist_json="$candidate"
	fi
fi
printf '%s' "$hist_json" >"$HIST_TMP"

# ── Resolve relays; failure -> empty (history-only detectors) ──
nodes_json='[]'
if resolved="$(lightspeed_resolve_nodes 2>/dev/null)"; then
	if printf '%s' "$resolved" | jq -e 'type == "array"' >/dev/null 2>&1; then
		nodes_json="$resolved"
	fi
fi

# ── Build the probe status map {node_id: bool} ───────────────
# Live probe by default; with --no-probe, fall back to the latest
# history reachability. A relay with no history entry is treated as
# "unknown" (true) so we never flag it without evidence.
probe_json='{}'
if [ "$NO_PROBE" = true ]; then
	regids_json="$(printf '%s' "$nodes_json" | jq -c '[.[].node_id]')"
	probe_json="$(jq -c --argjson regids "$regids_json" '
		(.snapshots[-1].per_relay // {}) as $cur
		| reduce ($regids[]) as $id ({};
			.[$id] = (if ($cur[$id] // null) == null
				then true
				else (($cur[$id].reachable // false)) end))' \
		"$HIST_TMP" 2>/dev/null || echo '{}')"
else
	while IFS= read -r node; do
		[ -z "$node" ] && continue
		id="$(printf '%s' "$node" | jq -r '.node_id // empty' 2>/dev/null || true)"
		url="$(printf '%s' "$node" | jq -r '.health_url // empty' 2>/dev/null || true)"
		[ -z "$id" ] && continue
		ok=false
		if [ -n "$url" ] && curl -fsS --max-time "$TIMEOUT" "$url" >/dev/null 2>&1; then
			ok=true
		fi
		probe_json="$(printf '%s' "$probe_json" | jq -c --arg id "$id" --argjson ok "$ok" '. + {($id):$ok}' 2>/dev/null || printf '%s' "$probe_json")"
	done < <(printf '%s' "$nodes_json" | jq -c '.[]?' 2>/dev/null || true)
fi

# ── Detection engine ─────────────────────────────────────────
JQ_PROGRAM='
def n: (tonumber? // 0);
def life($k): ((.lifetime[$k]) // (.cumulative[$k]) // (.[$k]) // 0) | n;
def rel_life($snap; $id; $k):
  ((($snap.per_relay // {})[$id] // {}) | life($k));
def rel_reach($snap; $id):
  ((((($snap.per_relay // {})[$id] // {}).reachable) // false) == true);
def span($a; $b): (($b - $a) | if . < 0 then null else . end);
def pct($x): (((($x // 0) * 1000 | round) / 10) | tostring) + "%";
def one($x): (((($x // 0) * 10 | round) / 10) | tostring);
def vparts: ((. // "") | split("-")[0] | split(".") | map(tonumber? // 0));
def vcmp($a; $b):
  ($a | vparts) as $x | ($b | vparts) as $y
  | if $x[0] != $y[0] then ($x[0] - $y[0])
    elif $x[1] != $y[1] then ($x[1] - $y[1])
    else ($x[2] - $y[2]) end;

($hist[0]) as $hist
| (($hist.snapshots // []) | if type == "array" then map(select(type == "object")) else [] end) as $snaps
| ($snaps | length) as $N
| (if $N >= $W then $snaps[($N - $W):] else $snaps end) as $win
| (if $N > $W then $snaps[0:($N - $W)] else [] end) as $prewin
| ($prewin[ (if ($prewin | length) > $B then (($prewin | length) - $B) else 0 end) : ]) as $base
| (if ($prewin | length) > 0 then $prewin[-1] else ($win[0] // null) end) as $wstart
| (($snaps[-1] // {}).per_relay // {}) as $cur
| ([ $cur | keys[] ]) as $ids
| ([ $nodes[].node_id ] | map(select(. != null and . != "")) | unique) as $regids
| ([ $win[] | (.per_relay // {} | keys[]) ] | unique) as $winids
| ($win | length) as $WL
| ($base | length) as $BL
| ([ $winids[] as $id
     | ($win[-1] | rel_life(.; $id; "packets_relayed"))
       - ($wstart | rel_life(.; $id; "packets_relayed")) ] | add // 0) as $fleet_pkts
| ([ $winids[] as $id
     | ($win[-1] | rel_life(.; $id; "sessions_created"))
       - ($wstart | rel_life(.; $id; "sessions_created")) ] | add // 0) as $fleet_sess
| ([ $ids[] as $id
     | select($WL >= $W and $W >= 2)
     | select((($prewin | length) == 0) or ((($prewin[-1].per_relay // {})[$id] // null) != null))
     | ([ $win[] | rel_reach(.; $id) ] | all) as $up
     | (($win[-1] | rel_life(.; $id; "packets_relayed"))) as $end
     | (($wstart | rel_life(.; $id; "packets_relayed"))) as $start
     | (span($start; $end)) as $relayed
     | select($up and $relayed != null and $relayed <= 0 and $fleet_pkts > 0)
     | { type: "zero_relay", severity: "critical", relay: $id,
         window: $WL, packets_relayed: 0, fleet_packets: $fleet_pkts,
         message: ($id + ": reachable for " + ($WL | tostring)
                   + " snapshots but relayed 0 packets while the fleet relayed "
                   + ($fleet_pkts | tostring)) }
   ]) as $zeroFlags
| ([ $ids[] as $id
     | select($WL >= $W and $W >= 2)
     | ([ $win[] | rel_reach(.; $id) ] | all) as $up
     | (($win[-1] | rel_life(.; $id; "drops_auth_rejected"))
        - ($wstart | rel_life(.; $id; "drops_auth_rejected"))) as $authRaw
     | ($authRaw | if . < 0 then 0 else . end) as $auth
     | (($win[-1] | rel_life(.; $id; "sessions_created"))
        - ($wstart | rel_life(.; $id; "sessions_created"))) as $sessRaw
     | ($sessRaw | if . < 0 then 0 else . end) as $sess
     | (if $BL >= 2
        then (($base[-1] | rel_life(.; $id; "drops_auth_rejected"))
              - ($base[0] | rel_life(.; $id; "drops_auth_rejected")))
        else 0 end) as $baseAuthRaw
     | ($baseAuthRaw | if . < 0 then 0 else . end) as $baseAuth
     | select($up and $auth >= $auth_min and $sess <= $session_flat
              and $fleet_sess > 0
              and ($baseAuth <= 0 or $auth >= ($baseAuth * $auth_factor)))
     | { type: "auth_spike_no_sessions", severity: "critical", relay: $id,
         window: $WL, auth_rejections: $auth, sessions_created: $sess,
         baseline_auth: $baseAuth, fleet_sessions: $fleet_sess,
         message: ($id + ": auth_rejections +" + ($auth | tostring)
                   + " with sessions_created +" + ($sess | tostring)
                   + " over " + ($WL | tostring) + " snapshots (fleet sessions +"
                   + ($fleet_sess | tostring) + ")") }
   ]) as $authFlags
| ([ $cur | to_entries[]
     | select(((.value.version // "") != "") and ((.value.reachable // false)))
     | { id: .key, v: (.value.version | tostring) } ]) as $vrows
| ([ $vrows[].v ] | group_by(.) | map({ v: .[0], c: length }) | sort_by(-.c)) as $vgroups
| (if ($vgroups | length) > 0 then ($vgroups | map(.c) | max) else 0 end) as $maxc
| (if $maxc >= $min_ahead
   then ([ $vgroups[] | select(.c == $maxc) | .v ] | sort_by(vparts) | last)
   else null end) as $refV
| ([ $vrows[] | select($refV != null and .v != $refV and (vcmp(.v; $refV) < 0))
     | { type: "version_lag", severity: "warning", relay: .id,
         version: .v, fleet_version: $refV, relays_ahead: $maxc,
         message: (.id + ": version " + .v + " behind fleet " + $refV
                   + " (" + ($maxc | tostring) + " relays ahead)") }
   ]) as $verFlags
| ([ $ids[] as $id
     | select($WL >= 2)
     | (($win[-1] | rel_life(.; $id; "saved_ms_sum"))
        - ($wstart | rel_life(.; $id; "saved_ms_sum"))) as $ws
     | (($win[-1] | rel_life(.; $id; "saved_ms_count"))
        - ($wstart | rel_life(.; $id; "saved_ms_count"))) as $wc
     | (($base[-1] | rel_life(.; $id; "saved_ms_sum"))
        - ($base[0] | rel_life(.; $id; "saved_ms_sum"))) as $bs
     | (($base[-1] | rel_life(.; $id; "saved_ms_count"))
        - ($base[0] | rel_life(.; $id; "saved_ms_count"))) as $bc
     | (($win[-1] | rel_life(.; $id; "saved_app_ms_negative_count"))
        - ($wstart | rel_life(.; $id; "saved_app_ms_negative_count"))) as $wn
     | (($win[-1] | rel_life(.; $id; "saved_app_ms_count"))
        - ($wstart | rel_life(.; $id; "saved_app_ms_count"))) as $wa
     | (($base[-1] | rel_life(.; $id; "saved_app_ms_negative_count"))
        - ($base[0] | rel_life(.; $id; "saved_app_ms_negative_count"))) as $bn
     | (($base[-1] | rel_life(.; $id; "saved_app_ms_count"))
        - ($base[0] | rel_life(.; $id; "saved_app_ms_count"))) as $ba
     | (if $wc > 0 then ($ws / $wc) else null end) as $wavg
     | (if $bc > 0 then ($bs / $bc) else null end) as $bavg
     | (if $wa > 0 then ($wn / $wa) else null end) as $wneg
     | (if $ba > 0 then ($bn / $ba) else null end) as $bneg
     | ( if ($wc >= $min_saved and $bc >= $min_saved
             and $bavg != null and $bavg > 0 and $wavg != null
             and $wavg < ($bavg * (1 - $saved_reg)))
         then { type: "saved_regression", severity: "warning", relay: $id,
                saved_ms_avg: $wavg, baseline_saved_ms_avg: $bavg,
                window_samples: $wc, baseline_samples: $bc,
                message: ($id + ": ping-saved " + one($wavg) + "ms vs baseline "
                          + one($bavg) + "ms over " + ($wc | tostring) + " samples") }
         else empty end ),
       ( if ($wneg != null and $wneg >= $neg_share and $wn >= $neg_min
             and ($bneg == null or $wneg > $bneg))
         then { type: "negative_saving_spike", severity: "warning", relay: $id,
                negative_share: $wneg, negative_count: $wn, samples: $wa,
                baseline_negative_share: $bneg,
                message: ($id + ": negative ping-saved share " + pct($wneg)
                          + " (" + ($wn | tostring) + " of " + ($wa | tostring)
                          + " samples)") }
         else empty end )
   ]) as $savedFlags
| ([ $ids[] as $id
     | select(($regids | length) > 0 and (($regids | index($id)) == null))
     | { type: "relay_missing_from_registry", severity: "critical", relay: $id,
         message: ($id + ": present in the latest history snapshot but absent from the registry") }
   ]) as $missingFlags
| ([ $regids[] as $id
     | select($probe[$id] == false)
     | { type: "relay_health_failed", severity: "critical", relay: $id,
         message: ($id + ": /health probe failed") }
   ]) as $healthFlags
| ($zeroFlags + $authFlags + $verFlags + $savedFlags
   + $missingFlags + $healthFlags) as $flags
| {
    window: $W,
    baseline: $B,
    snapshot_count: $N,
    relay_count: ($ids | length),
    fleet_count: ($regids | length),
    anomalies: ($flags | sort_by(.severity, .relay, .type))
  }
'

result="$(jq -c \
	--slurpfile hist "$HIST_TMP" \
	--argjson nodes "$nodes_json" \
	--argjson probe "$probe_json" \
	--argjson W "$WINDOW" \
	--argjson B "$BASELINE" \
	--argjson auth_min "$AUTH_MIN" \
	--argjson auth_factor "$AUTH_FACTOR" \
	--argjson session_flat "$SESSION_FLAT_MAX" \
	--argjson min_saved "$MIN_SAVED_SAMPLES" \
	--argjson saved_reg "$SAVED_REGRESSION" \
	--argjson neg_share "$NEG_SHARE_MAX" \
	--argjson neg_min "$NEG_MIN" \
	--argjson min_ahead "$MIN_VERSION_AHEAD" \
	"$JQ_PROGRAM" <<<'null' 2>/dev/null || true)"

if [ -z "$result" ] || ! printf '%s' "$result" | jq -e . >/dev/null 2>&1; then
	result='{"window":0,"baseline":0,"snapshot_count":0,"relay_count":0,"fleet_count":0,"anomalies":[]}'
fi

anomaly_count="$(printf '%s' "$result" | jq -r '.anomalies | length' 2>/dev/null || echo 0)"
snapshot_count="$(printf '%s' "$result" | jq -r '.snapshot_count // 0' 2>/dev/null || echo 0)"
relay_count="$(printf '%s' "$result" | jq -r '.relay_count // 0' 2>/dev/null || echo 0)"

# ── Output ───────────────────────────────────────────────────
if [ "$JSON_OUT" = true ]; then
	printf '%s\n' "$result"
else
	if [ "$anomaly_count" -eq 0 ]; then
		printf 'health-anomaly: no anomalies (%s relay(s), %s snapshot(s), window=%s)\n' \
			"$relay_count" "$snapshot_count" "$WINDOW"
	else
		printf 'health-anomaly: %s anomaly(ies) (%s relay(s), %s snapshot(s), window=%s)\n' \
			"$anomaly_count" "$relay_count" "$snapshot_count" "$WINDOW"
		printf '%s' "$result" | jq -r '.anomalies[] | "  [" + .severity + "] " + .type + ": " + .message'
	fi
fi

# ── Discord alert (never fails the run itself) ───────────────
if [ "$anomaly_count" -gt 0 ] && [ -n "${DISCORD_WEBHOOK:-}" ]; then
	content="🚨 LightSpeed anomaly monitor: ${anomaly_count} anomaly(ies)
$(printf '%s' "$result" | jq -r '.anomalies[] | "[" + .severity + "] " + .type + ": " + (.message // "")')"
	content="${content:0:1900}"
	payload="$(jq -cn --arg c "$content" '{content: $c}' 2>/dev/null || true)"
	if [ -n "$payload" ]; then
		curl -fsS -X POST -H 'Content-Type: application/json' -d "$payload" "$DISCORD_WEBHOOK" >/dev/null 2>&1 || true
	fi
fi

if [ "$anomaly_count" -gt 0 ]; then
	exit 1
fi
exit 0
