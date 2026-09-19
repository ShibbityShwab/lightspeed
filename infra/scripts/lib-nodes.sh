#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Shared Relay Node Inventory Resolver
#
# Sourceable helper for the ops scripts (deploy.sh, mesh-health.sh).
# Resolves the proxy node inventory from the signed registry
# (web/registry.json) by default, or from an LIGHTSPEED_NODES JSON
# override for self-hosters.
#
# Usage:
#   source "$(dirname "$0")/lib-nodes.sh"
#   nodes_json="$(lightspeed_resolve_nodes)"      # compact JSON array
#   lightspeed_resolve_nodes_jsonl                # one object per line
#
# Every resolved node object carries:
#   node_id, region, ip, data_addr, health_url, metrics_url,
#   control_port
#
# Environment:
#   LIGHTSPEED_NODES          JSON object keyed by region, or an array
#                             of node objects. When set and non-empty,
#                             it overrides the registry entirely.
#   LIGHTSPEED_REGISTRY_PATH  Path to the signed registry. Defaults to
#                             <repo>/web/registry.json, resolved from
#                             this file's location (never the CWD).
#   LIGHTSPEED_CONTROL_PORT   Default control port for nodes that do
#                             not specify one. Defaults to 4433.
#
# Returns non-zero (with a message on stderr) when neither a non-empty
# override nor a non-empty registry is available.
#
# Requires: bash, jq
# ──────────────────────────────────────────────────────────────

# ── Locate the repo root relative to this file ───────────────
_LIGHTSPEED_LIB_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIGHTSPEED_REPO_ROOT="$(cd "$_LIGHTSPEED_LIB_DIR/../.." && pwd)"
LIGHTSPEED_REGISTRY_PATH="${LIGHTSPEED_REGISTRY_PATH:-$LIGHTSPEED_REPO_ROOT/web/registry.json}"

LIGHTSPEED_CONTROL_PORT="${LIGHTSPEED_CONTROL_PORT:-4433}"
case "$LIGHTSPEED_CONTROL_PORT" in
    ''|*[!0-9]*) LIGHTSPEED_CONTROL_PORT=4433 ;;
esac

# ── Shared jq normalizer ─────────────────────────────────────
# Accepts either an array of node objects (registry shape) or an
# object keyed by region/name (self-hoster override shape), and emits
# a normalized array. metrics_url defaults to http://<ip>:8080/metrics
# and data_addr defaults to <ip>:4434.
_LIGHTSPEED_NORM_JQ='
def ls_ip:
  if (.ip // "") != "" then .ip
  elif (.data_addr // "") != "" then (.data_addr | split(":")[0])
  else "" end;
def ls_url($ip; $path):
  if $ip == "" then "" else ("http://" + $ip + ":8080/" + $path) end;
def ls_norm:
  (ls_ip) as $ip
  | {
      node_id: (.node_id // .name // ""),
      region: (.region // ""),
      ip: $ip,
      data_addr: (.data_addr // (if $ip == "" then "" else ($ip + ":4434") end)),
      health_url: (.health_url // ls_url($ip; "health")),
      metrics_url: (.metrics_url // ls_url($ip; "metrics")),
      control_port: (.control_port // $control_port)
    };
if type == "array" then map(ls_norm)
else
  to_entries
  | map(
      (.key) as $k
      | (.value | if type == "string" then {ip: .} else . end)
      | . + {node_id: (.node_id // $k), region: (.region // $k)}
      | ls_norm
    )
end
'

# ── Error helper ─────────────────────────────────────────────
lightspeed_nodes_err() {
    printf 'lightspeed: %s\n' "$*" >&2
}

# ── lightspeed_resolve_nodes ─────────────────────────────────
# Emit a compact JSON array of resolved nodes. Non-zero on failure.
lightspeed_resolve_nodes() {
    local override="${LIGHTSPEED_NODES:-}"

    # 1) A non-empty LIGHTSPEED_NODES override wins (self-hosters).
    if [ -n "$override" ]; then
        if ! printf '%s' "$override" | jq empty >/dev/null 2>&1; then
            lightspeed_nodes_err "LIGHTSPEED_NODES is not valid JSON"
            return 1
        fi
        if [ "$(printf '%s' "$override" \
                | jq -r 'if (type == "object" or type == "array") and length == 0 then "empty" else "ok" end')" = "ok" ]; then
            printf '%s' "$override" \
                | jq -c --argjson control_port "$LIGHTSPEED_CONTROL_PORT" "$_LIGHTSPEED_NORM_JQ"
            return $?
        fi
        # An empty override ({} / []) falls through to the registry.
    fi

    # 2) Signed registry, resolved from the repo root (never the CWD).
    if [ ! -f "$LIGHTSPEED_REGISTRY_PATH" ]; then
        lightspeed_nodes_err "node registry not found: $LIGHTSPEED_REGISTRY_PATH"
        lightspeed_nodes_err "set LIGHTSPEED_NODES to override the registry"
        return 1
    fi

    if ! jq -e '(.registry? | type) == "string" and ((.registry | length) > 0)' \
            "$LIGHTSPEED_REGISTRY_PATH" >/dev/null 2>&1; then
        lightspeed_nodes_err "node registry is missing its 'registry' payload: $LIGHTSPEED_REGISTRY_PATH"
        lightspeed_nodes_err "set LIGHTSPEED_NODES to override the registry"
        return 1
    fi

    local node_count
    node_count="$(jq -r 'try (.registry | fromjson | (.nodes // []) | length) catch 0' \
        "$LIGHTSPEED_REGISTRY_PATH" 2>/dev/null || echo 0)"
    if [ "$node_count" = "0" ]; then
        lightspeed_nodes_err "node registry lists no nodes: $LIGHTSPEED_REGISTRY_PATH"
        lightspeed_nodes_err "set LIGHTSPEED_NODES to override the registry"
        return 1
    fi

    jq -c --argjson control_port "$LIGHTSPEED_CONTROL_PORT" \
        ".registry | fromjson | .nodes | ${_LIGHTSPEED_NORM_JQ}" \
        "$LIGHTSPEED_REGISTRY_PATH"
    return $?
}

# ── lightspeed_resolve_nodes_jsonl ───────────────────────────
# Emit one compact JSON object per line. Non-zero on failure.
lightspeed_resolve_nodes_jsonl() {
    lightspeed_resolve_nodes | jq -c '.[]?'
}
