#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Live Network Stats Generator
#
# Reads the signed registry (web/registry.json), probes each relay's
# live /health endpoint, and writes web/network-stats.json with
# network-wide, currently-tested relay stats.
#
# Usage: ./network-stats.sh [output-path]
#   output-path  Optional. Defaults to <repo>/web/network-stats.json
#
# Guarantees:
#   * Always writes valid, pretty JSON (even if every relay is down).
#   * Always exits 0; one unreachable relay never aborts the run.
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
trap 'rm -f "$TMP_RELAYS"' EXIT

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

final="$(jq -n --argjson generated "$GENERATED_AT" --argjson relays "$relays_json" '
    {
      generated_at: $generated,
      relay_count: ($relays | length),
      healthy_count: ([$relays[] | select(.status == "healthy")] | length),
      area_count: ([$relays[].area] | unique | length),
      software_versions: ([$relays[]
                           | select(.version != null and .version != "")
                           | .version] | unique),
      relays: $relays
    }' 2>/dev/null || true)"

if [ -z "$final" ]; then
    final="$(printf '{"generated_at":%s,"relay_count":0,"healthy_count":0,"area_count":0,"software_versions":[],"relays":[]}' "$GENERATED_AT")"
fi

printf '%s\n' "$final" > "$OUT_PATH"

# ── Summary (stdout only; never affects the JSON file) ───────
printf '⚡ wrote %s: %s relay(s), %s healthy\n' \
    "$OUT_PATH" \
    "$(printf '%s' "$final" | jq -r '.relay_count')" \
    "$(printf '%s' "$final" | jq -r '.healthy_count')"

exit 0
