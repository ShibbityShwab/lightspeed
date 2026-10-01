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
#   (a) zero_relay                relay reachable for the whole window,
#                                 relayed 0 packets while the fleet was
#                                 busy, AND rejected clients - the 1.6.3
#                                 outage signature. Silence WITHOUT any
#                                 rejection is reported separately as
#                                 idle_relay (warning): all eight relays
#                                 share one uptime, so a relay that simply
#                                 received no traffic is not broken, and
#                                 paging on it would train operators to
#                                 ignore the detector that matters.
#   (b) auth_spike_no_sessions    auth_rejections climb while
#                                 sessions_created stays flat (the
#                                 data-only-proxy signature)
#   (c) version_lag               a relay behind the fleet's majority
#                                 version while the fleet moved on
#   (d) saved_regression          ping-saved drops hard vs the relay's
#                                 own recent baseline
#   (d) negative_saving_spike     negative-saving share spikes
#   (e) abuse_flood               abuse_blocked dominates a relay's
#                                 handled traffic while real sessions
#                                 stay flat (the relay-fra signature:
#                                 an attempted flood absorbed by the
#                                 abuse detector, invisible to the
#                                 liveness gate and to detectors a-d)
#   (f) stale_history             the newest snapshot is older than the
#                                 collector's own interval (the
#                                 silent-collector signature: on
#                                 2026-10-01 the Pages cron stopped
#                                 firing at 01:46Z and this monitor
#                                 still reported "no anomalies" at
#                                 05:57Z, because every detector below
#                                 happily describes frozen data)
#   (g) fleet_idle                EVERY reachable relay relayed zero
#                                 packets and created zero sessions for
#                                 the whole window. zero_relay covers
#                                 one silent relay on a busy fleet; it
#                                 deliberately excludes a fully idle
#                                 fleet, so a fleet-wide traffic stop
#                                 had no detector at all (observed live
#                                 on 2026-10-01: all eight relays up,
#                                 counters byte-identical for hours)
#   (h) relay_missing_from_registry / relay_health_failed
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

# abuse_flood: abuse_blocked must be a large absolute count AND a large
# share of the relay's drops, sustained over the window. The share is
# measured against DROPS, not total handled traffic: a flood absorbed
# by the abuse detector is almost entirely drops, whereas a busy
# healthy relay has a large relayed count that would mask it
# (relay-fra's real 2026-10-01 flood was 99.9% of its drops but only
# 46.8% of its relayed+dropped volume). The absolute floor keeps a tiny
# relay from tripping it on noise.
#
# Unlike auth_spike_no_sessions this detector does NOT require flat
# sessions: a real flood arrives alongside legitimate traffic (the
# 2026-10-01 relay-fra window carried +1,153,206 abuse blocks AND +37
# real sessions), so gating on flat sessions suppressed the very event
# this detector exists to catch.
ABUSE_MIN=1000   # window drops_abuse_blocked floor
ABUSE_SHARE=0.9  # abuse_blocked / packets_dropped floor

# stale_history: the newest snapshot must be recent. The collector runs
# hourly, so an age past this many seconds means the pipeline (not the
# relays) has stopped and every other detector is describing a frozen
# past. 0 disables the check, for an intentionally static fixture.
MAX_STALENESS=10800 # 3h; tolerates a couple of missed cron slots

# Clock seam: tests set this to pin "now" so staleness assertions are
# deterministic instead of racing the wall clock.
NOW_EPOCH="${LIGHTSPEED_NOW_EPOCH:-$(date +%s)}"

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
  --abuse-min N     Window drops_abuse_blocked floor for abuse_flood
                    (default: $ABUSE_MIN)
  --abuse-share P   abuse_blocked share of the relay's drops floor for
                    abuse_flood, 0..1 (default: $ABUSE_SHARE)
  --max-staleness S Newest-snapshot age in seconds before stale_history
                    fires; 0 disables (default: $MAX_STALENESS)
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
	--abuse-min)
		shift
		ABUSE_MIN="${1:-1000}"
		;;
	--abuse-share)
		shift
		ABUSE_SHARE="${1:-0.9}"
		;;
	--max-staleness)
		shift
		MAX_STALENESS="${1:-10800}"
		;;
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
     | (($win[-1] | rel_life(.; $id; "drops_auth_rejected"))
        - ($wstart | rel_life(.; $id; "drops_auth_rejected"))) as $authRaw
     | ($authRaw | if . < 0 then 0 else . end) as $auth
     | select($up and $relayed != null and $relayed <= 0 and $fleet_pkts > 0)
     | (if $auth > 0
        then { type: "zero_relay", severity: "critical", relay: $id,
               window: $WL, packets_relayed: 0, fleet_packets: $fleet_pkts,
               auth_rejections: $auth,
               message: ($id + ": reachable for " + ($WL | tostring)
                         + " snapshots but relayed 0 packets while the fleet relayed "
                         + ($fleet_pkts | tostring) + ", and " + ($auth | tostring)
                         + " client(s) were rejected - the 1.6.3 outage signature") }
        else { type: "idle_relay", severity: "warning", relay: $id,
               window: $WL, packets_relayed: 0, fleet_packets: $fleet_pkts,
               auth_rejections: 0,
               message: ($id + ": reachable for " + ($WL | tostring)
                         + " snapshots and handled no traffic while the fleet relayed "
                         + ($fleet_pkts | tostring)
                         + ", but it rejected no clients either - this is "
                         + "traffic distribution, not a failure (severity warning)") }
        end)
   ]) as $zeroFlags
| ([ $ids[] as $id
     | select($WL >= $W and $W >= 2)
     | ([ $win[] | rel_reach(.; $id) ] | all) as $up
     | select($up)] | length) as $up_count
| (if ($WL >= $W and $W >= 2 and $up_count > 0
        and $fleet_pkts <= 0 and $fleet_sess <= 0)
   then [{ type: "fleet_idle", severity: "critical",
           window: $WL, relays_up: $up_count,
           fleet_packets: $fleet_pkts, fleet_sessions: $fleet_sess,
           message: (($up_count | tostring) + " relay(s) reachable for "
                     + ($WL | tostring) + " snapshots but the whole fleet relayed 0 packets "
                     + "and created 0 sessions; this is the fleet-wide-traffic-stop signature, "
                     + "not the single-relay zero_relay case") }]
   else [] end) as $idleFlags
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
     | select($WL >= $W and $W >= 2)
     | ([ $win[] | rel_reach(.; $id) ] | all) as $up
     | (($win[-1] | rel_life(.; $id; "drops_abuse_blocked"))
        - ($wstart | rel_life(.; $id; "drops_abuse_blocked"))) as $abuseRaw
     | ($abuseRaw | if . < 0 then 0 else . end) as $abuse
     | (($win[-1] | rel_life(.; $id; "packets_relayed"))
        - ($wstart | rel_life(.; $id; "packets_relayed"))) as $relayedRaw
     | ($relayedRaw | if . < 0 then 0 else . end) as $relayed
     | (($win[-1] | rel_life(.; $id; "packets_dropped"))
        - ($wstart | rel_life(.; $id; "packets_dropped"))) as $droppedRaw
     | ($droppedRaw | if . < 0 then 0 else . end) as $dropped
     | (($win[-1] | rel_life(.; $id; "sessions_created"))
        - ($wstart | rel_life(.; $id; "sessions_created"))) as $sessRaw
     | ($sessRaw | if . < 0 then 0 else . end) as $sess
     | (if $dropped > 0 then ($abuse / $dropped) else 0 end) as $share
     | select($up and $abuse >= $abuse_min and $dropped > 0 and $share >= $abuse_share)
     | { type: "abuse_flood", severity: "warning", relay: $id,
         window: $WL, abuse_blocked: $abuse, packets_dropped: $dropped,
         packets_relayed: $relayed, abuse_share: $share, sessions_created: $sess,
         message: ($id + ": abuse_blocked +" + ($abuse | tostring)
                   + " = " + pct($share) + " of drops ("
                   + ($dropped | tostring) + " dropped, "
                   + ($relayed | tostring) + " relayed, sessions +"
                   + ($sess | tostring) + ") over " + ($WL | tostring) + " snapshots") }
   ]) as $abuseFlags
| ([ (if $max_stale > 0
        then (($snaps[-1].t // 0) | n) as $newest
        | (($now - $newest)) as $age
        | (if $newest > 0 and $age > $max_stale
           then { type: "stale_history", severity: "warning",
                  newest_snapshot_t: $newest, age_secs: $age,
                  max_staleness: $max_stale,
                  message: ("newest snapshot is " + ($age | tostring)
                            + "s old (limit " + ($max_stale | tostring)
                            + "s); the collector has stopped publishing and every "
                            + "other detector is describing frozen data") }
           else empty end)
        else empty end) ]) as $staleFlags
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
| ($zeroFlags + $idleFlags + $authFlags + $verFlags + $savedFlags + $abuseFlags
   + $staleFlags + $missingFlags + $healthFlags) as $flags
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
	--argjson abuse_min "$ABUSE_MIN" \
	--argjson abuse_share "$ABUSE_SHARE" \
	--argjson max_stale "$MAX_STALENESS" \
	--argjson now "$NOW_EPOCH" \
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
