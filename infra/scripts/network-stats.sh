#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Live Network Stats Generator
#
# Reads the signed registry (web/registry.json), probes each relay's
# live /health and /metrics endpoints, and writes web/network-stats.json
# with network-wide, currently-tested relay stats plus the fleet-wide
# telemetry "RTT saved" figure and the trained route model's self-reported
# R²/MAE and trained-on date.
#
# Usage: ./network-stats.sh [output-path]
#   output-path  Optional. Defaults to <repo>/web/network-stats.json
#
# Guarantees:
#   * Always writes valid, pretty JSON (even if every relay is down).
#   * Exits 0 when the site can render honestly: a number when the
#     telemetry k>=3 threshold has data, "collecting" otherwise.
#   * Self-check: exits non-zero when telemetry saved-app data is present
#     but the published hero metric would still render "collecting", so a
#     wiring regression can never silently ship the placeholder.
#
# Requires: bash, curl, jq
# ──────────────────────────────────────────────────────────────
set -euo pipefail

# ── Resolve repo root from the script's own location ─────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"

OUT_PATH="${1:-$REPO_ROOT/web/network-stats.json}"
REGISTRY_PATH="$REPO_ROOT/web/registry.json"
TIMEOUT=5
GENERATED_AT="$(date +%s)"

mkdir -p "$(dirname "$OUT_PATH")"

TMP_RELAYS="$(mktemp)"
TMP_TELEM="$(mktemp)"
trap 'rm -f "$TMP_RELAYS" "$TMP_TELEM"' EXIT

# ── Region -> display field mapping ──────────────────────────
# Emits "area|location|country|flag". Falls back to a substring
# match on the node_id when the region string is missing/unknown.
region_display() {
    local region="$1"
    local node_id="$2"
    local hay="${region,,} ${node_id,,}"

    case "$hay" in
        *ap-southeast-2*|*syd*|*sydney*)
            echo "AP-Southeast-2|Sydney|AU|🇦🇺" ;;
        *ap-southeast*|*sgp*|*singapore*)
            echo "AP-Southeast|Singapore|SG|🇸🇬" ;;
        *ap-northeast*|*nrt*|*tokyo*)
            echo "AP-Northeast|Tokyo|JP|🇯🇵" ;;
        *ap-south*|*bom*|*mumbai*)
            echo "AP-South|Mumbai|IN|🇮🇳" ;;
        *us-west*|*lax*|*"los angeles"*)
            echo "North America|Los Angeles|US|🇺🇸" ;;
        *us-east*|*ewr*|*"new jersey"*)
            echo "North America|New Jersey|US|🇺🇸" ;;
        *eu-central*|*fra*|*frankfurt*)
            echo "Europe|Frankfurt|DE|🇩🇪" ;;
        *eu-south*|*mad*|*madrid*)
            echo "Europe|Madrid|ES|🇪🇸" ;;
        *)
            echo "Other|${region}||🏳️" ;;
    esac
}

# ── Prometheus metrics parsing (telemetry savings + model self-report) ──
# Parses a relay's /metrics text into one compact JSON row. Telemetry
# counters are summed across label sets; the model gauges (a relay's single
# latest self-report) keep a missing family null rather than a fake 0.
TELEM_JQ='def fam($name; $mode):
  (split("\n")
   | map(select(startswith($name + "{") or startswith($name + " ")))
   | map(try (capture("^[^ ]+ +(?<v>[-+0-9.eE]+)").v | tonumber) catch null)
   | map(select(. != null)))
  | if $mode == "sum" then add // 0
    elif length == 0 then null
    else max end;
{
  saved_app_ms_sum: fam("lightspeed_telemetry_saved_app_ms_sum"; "sum"),
  saved_app_ms_count: fam("lightspeed_telemetry_saved_app_ms_count"; "sum"),
  saved_app_ms_negative_count: fam("lightspeed_telemetry_saved_app_ms_negative_count"; "sum"),
  saved_ms_sum: fam("lightspeed_telemetry_saved_ms_sum"; "sum"),
  saved_ms_count: fam("lightspeed_telemetry_saved_ms_count"; "sum"),
  model_r_squared: fam("lightspeed_model_r_squared"; "max"),
  model_mae_ms: fam("lightspeed_model_mae_ms"; "max"),
  model_trained_at: fam("lightspeed_model_trained_at"; "max")
}'

# ── Load + unwrap the signed registry ────────────────────────
# registry.json = {"registry": "<JSON string>", "signature": "..."}
registry_raw="$(jq -r '.registry // empty' "$REGISTRY_PATH" 2>/dev/null || true)"

nodes_jsonl=""
if [ -n "$registry_raw" ]; then
    nodes_jsonl="$(printf '%s' "$registry_raw" | jq -c '.nodes[]?' 2>/dev/null || true)"
fi

# ── Probe each relay ─────────────────────────────────────────
while IFS= read -r node; do
    [ -z "$node" ] && continue
    node_id="$(printf '%s' "$node" | jq -r '.node_id // empty' 2>/dev/null || true)"
    region="$(printf '%s' "$node" | jq -r '.region // empty' 2>/dev/null || true)"
    health_url="$(printf '%s' "$node" | jq -r '.health_url // empty' 2>/dev/null || true)"
    [ -z "$node_id" ] && continue

    # Probe; any failure/timeout yields an empty response (never abort).
    resp=""
    if [ -n "${health_url:-}" ]; then
        resp="$(curl --silent --max-time "$TIMEOUT" "$health_url" 2>/dev/null || true)"
    fi

    # /metrics lives alongside /health on the same host:port.
    metrics_url=""
    if [ -n "${health_url:-}" ]; then
        metrics_url="${health_url%/*}/metrics"
    fi
    metrics=""
    if [ -n "${metrics_url:-}" ]; then
        metrics="$(curl --silent --max-time "$TIMEOUT" "$metrics_url" 2>/dev/null || true)"
    fi

    # Telemetry + model self-report, folded into the fleet aggregate below.
    telem_row=""
    if [ -n "$metrics" ]; then
        telem_row="$(printf '%s' "$metrics" | jq -Rsc "$TELEM_JQ" 2>/dev/null || true)"
    fi
    if [ -n "$telem_row" ] && [ "$telem_row" != "null" ]; then
        printf '%s\n' "$telem_row" >> "$TMP_TELEM"
    fi

    display="$(region_display "$region" "$node_id")"
    IFS='|' read -r area location country flag <<< "$display"

    relay="$(jq -n \
        --arg node_id "$node_id" \
        --arg region "$region" \
        --arg area "$area" \
        --arg location "$location" \
        --arg country "$country" \
        --arg flag "$flag" \
        --arg resp "$resp" \
        '
        ($resp | try fromjson catch null) as $h
        | {
            node_id: $node_id,
            area: $area,
            region: $region,
            location: $location,
            country: $country,
            flag: $flag,
            status: (($h.status? // "down") | tostring),
            version: (if ($h != null and ($h.version? != null) and ($h.version != ""))
                      then ($h.version | tostring) else null end),
            uptime_secs: (($h.uptime_secs? // 0) | tonumber? // 0 | floor),
            packets_relayed: (($h.packets_relayed? // 0) | tonumber? // 0 | floor),
            packets_dropped: (($h.packets_dropped? // 0) | tonumber? // 0 | floor),
            drops_malformed: (($h.drops_malformed? // 0) | tonumber? // 0 | floor),
            drops_auth_rejected: (($h.drops_auth_rejected? // 0) | tonumber? // 0 | floor),
            drops_abuse_blocked: (($h.drops_abuse_blocked? // 0) | tonumber? // 0 | floor),
            drops_rate_limited: (($h.drops_rate_limited? // 0) | tonumber? // 0 | floor),
            drops_fec_malformed: (($h.drops_fec_malformed? // 0) | tonumber? // 0 | floor),
            drops_session_setup: (($h.drops_session_setup? // 0) | tonumber? // 0 | floor),
            drops_relay_send_errors: (($h.drops_relay_send_errors? // 0) | tonumber? // 0 | floor),
            sessions_created: (($h.sessions_created? // 0) | tonumber? // 0 | floor)
          }' 2>/dev/null || true)"

    if [ -n "$relay" ]; then
        printf '%s\n' "$relay" >> "$TMP_RELAYS"
    fi
done <<< "$nodes_jsonl"

# ── Assemble + write the final document ──────────────────────
relays_json="$(jq -s '.' "$TMP_RELAYS" 2>/dev/null || echo '[]')"
telem_rows="$(jq -s '.' "$TMP_TELEM" 2>/dev/null || echo '[]')"

final="$(jq -n --argjson generated "$GENERATED_AT" --argjson relays "$relays_json" --argjson telem "$telem_rows" '
    def n: (try tonumber catch 0);
    def r2: ((. * 100) | round) / 100;
    ([$telem[].saved_app_ms_sum | n] | add // 0) as $app_sum
    | ([$telem[].saved_app_ms_count | n] | add // 0) as $app_count
    | ([$telem[].saved_app_ms_negative_count | n] | add // 0) as $app_neg
    | ([$telem[].saved_ms_sum | n] | add // 0) as $icmp_sum
    | ([$telem[].saved_ms_count | n] | add // 0) as $icmp_count
    | (if $app_count > 0 then ($app_sum / $app_count)
       elif $icmp_count > 0 then ($icmp_sum / $icmp_count)
       else null end) as $rtt_saved
    | ($telem
       | map(select((.model_trained_at | n) > 0))
       | sort_by(.model_trained_at | n) | last // null) as $latest_model
    | {
        generated_at: $generated,
        relay_count: ($relays | length),
        healthy_count: ([$relays[] | select(.status == "healthy")] | length),
        area_count: ([$relays[].area] | unique | length),
        software_versions: ([$relays[]
                             | select(.version != null and .version != "")
                             | .version] | unique),
        saved: {
          rtt_saved_ms: (if $rtt_saved == null then null else ($rtt_saved | r2) end),
          saved_app_ms_count: $app_count,
          negative_share: (if $app_count > 0 then (($app_neg / $app_count) | r2) else null end),
          icmp_rtt_saved_ms: (if $icmp_count > 0 then (($icmp_sum / $icmp_count) | r2) else null end),
          threshold: 3
        },
        model: (if $latest_model == null
                then {r_squared: null, mae_ms: null, trained_on: null}
                else {
                  r_squared: (if $latest_model.model_r_squared != null then ($latest_model.model_r_squared | r2) else null end),
                  mae_ms: (if $latest_model.model_mae_ms != null then ($latest_model.model_mae_ms | r2) else null end),
                  trained_on: (if ($latest_model.model_trained_at | n) > 0 then ($latest_model.model_trained_at | n) else null end)
                } end),
        relays: $relays
      }' 2>/dev/null || true)"

if [ -z "$final" ]; then
    final="$(printf '{"generated_at":%s,"relay_count":0,"healthy_count":0,"area_count":0,"software_versions":[],"saved":{"rtt_saved_ms":null,"saved_app_ms_count":0,"negative_share":null,"icmp_rtt_saved_ms":null,"threshold":3},"model":{"r_squared":null,"mae_ms":null,"trained_on":null},"relays":[]}' "$GENERATED_AT")"
fi

printf '%s\n' "$final" > "$OUT_PATH"

# ── Self-check: never render "collecting" while threshold data exists ──
# "Data present" is judged against the raw telemetry the relays exported
# (saved-app paired observations past the k>=3 anonymity floor), not the
# published aggregate, so a wiring regression that also drops the published
# count is still caught.
raw_saved_count="$(printf '%s' "$telem_rows" | jq -r '([.[].saved_app_ms_count // 0] | add // 0) | floor' 2>/dev/null || echo 0)"
saved_count="$(printf '%s' "$final" | jq -r '((.saved.saved_app_ms_count // 0) | floor)' 2>/dev/null || echo 0)"
if [ "$raw_saved_count" -gt 0 ]; then
    hero_ms="$(printf '%s' "$final" | jq -r '.saved.rtt_saved_ms // "null"' 2>/dev/null || echo null)"
    if [ "$hero_ms" = "null" ]; then
        printf 'self-check FAILED: relays exported %s paired saved-app samples but the published hero "Median RTT saved" is null (would render "collecting")\n' "$raw_saved_count" >&2
        exit 1
    fi
fi

# ── Summary (stdout only; never affects the JSON file) ───────
printf '⚡ wrote %s: %s relay(s), %s healthy, saved=%s ms (%s paired samples)\n' \
    "$OUT_PATH" \
    "$(printf '%s' "$final" | jq -r '.relay_count')" \
    "$(printf '%s' "$final" | jq -r '.healthy_count')" \
    "$(printf '%s' "$final" | jq -r '.saved.rtt_saved_ms // "collecting"')" \
    "$saved_count"

exit 0
