#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Self-test for the shared node inventory
#
# Proves the load-bearing behaviours of lib-nodes.sh and the
# fail-closed path in deploy.sh:
#   (a) a 5-node fixture registry resolves all 5 nodes with the
#       derived metrics_url / control_port fields
#   (b) the LIGHTSPEED_NODES override works, with and without a
#       registry present, and takes precedence over the registry
#   (c) the default registry resolves relative to the repo root,
#       not the current working directory
#   (d) a missing / empty registry with no override is non-zero
#   (e) deploy.sh fails closed on an empty inventory without
#       starting a build
# Plus: the signed registry still lists exactly 5 nodes.
#
# Usage: bash infra/scripts/test_inventory.sh
# Exits 0 and prints "inventory: all assertions passed" on success.
# Requires: bash, jq. No network access.
# ──────────────────────────────────────────────────────────────
set -uo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
LIB="$SCRIPT_DIR/lib-nodes.sh"
DEPLOY="$SCRIPT_DIR/deploy.sh"
REGISTRY="$SCRIPT_DIR/../../web/registry.json"

PASS=0
FAILURES=0
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

note_pass() { PASS=$((PASS + 1)); }
note_fail() { printf '  FAIL: %s\n' "$1" >&2; FAILURES=$((FAILURES + 1)); }

assert_eq() {
    local actual="$1" expected="$2" msg="$3"
    if [ "$actual" = "$expected" ]; then
        note_pass
    else
        note_fail "$msg (got '$actual', expected '$expected')"
    fi
}

assert_rc_nonzero() {
    local rc="$1" msg="$2"
    if [ "$rc" -ne 0 ]; then note_pass; else note_fail "$msg (exit was 0)"; fi
}

assert_contains() {
    local file="$1" needle="$2" msg="$3"
    if grep -q -- "$needle" "$file" 2>/dev/null; then
        note_pass
    else
        note_fail "$msg (missing '$needle' in $file)"
    fi
}

write_registry() {
    local path="$1" count="$2" inner
    inner="$(jq -cn --argjson n "$count" '
        {schema_version: 1, published_at: 1,
         nodes: [range(0; $n) as $i
                 | {node_id: ("relay-" + ($i | tostring)),
                    region: ("region-" + ($i | tostring)),
                    data_addr: ("10.0.0." + ($i | tostring) + ":4434"),
                    health_url: ("http://10.0.0." + ($i | tostring) + ":8080/health")}],
         revoked: []}')"
    jq -n --arg reg "$inner" '{registry: $reg, signature: "test"}' > "$path"
}

resolve() {
    local reg="${1-}" ovr="${2-}"
    LIGHTSPEED_REGISTRY_PATH="$reg" LIGHTSPEED_NODES="$ovr" \
        bash -c 'source "$1"; lightspeed_resolve_nodes' _ "$LIB"
}

for dep in "$LIB" "$DEPLOY" "$REGISTRY"; do
    if [ ! -f "$dep" ]; then
        printf 'inventory: FAIL - required file not found: %s\n' "$dep" >&2
        exit 1
    fi
done

# ── (a) five-node fixture registry ───────────────────────────
REG5="$TMP/reg5.json"
write_registry "$REG5" 5
OUT5="$(resolve "$REG5" "")"

assert_eq "$(printf '%s' "$OUT5" | jq -r 'length')" "5" "(a) fixture registry resolves 5 nodes"
assert_eq "$(printf '%s' "$OUT5" | jq -r '.[0].node_id')" "relay-0" "(a) first node_id"
assert_eq "$(printf '%s' "$OUT5" | jq -r '.[0].region')" "region-0" "(a) first region"
assert_eq "$(printf '%s' "$OUT5" | jq -r '.[0].ip')" "10.0.0.0" "(a) ip derived from data_addr"
assert_eq "$(printf '%s' "$OUT5" | jq -r '.[0].data_addr')" "10.0.0.0:4434" "(a) data_addr preserved"
assert_eq "$(printf '%s' "$OUT5" | jq -r '.[0].health_url')" "http://10.0.0.0:8080/health" "(a) registry health_url preserved"
assert_eq "$(printf '%s' "$OUT5" | jq -r '.[0].metrics_url')" "http://10.0.0.0:8080/metrics" "(a) metrics_url derived from data_addr"
assert_eq "$(printf '%s' "$OUT5" | jq -r '.[0].control_port')" "4433" "(a) control_port defaults to 4433"
assert_eq "$(printf '%s' "$OUT5" | jq -r '[.[].node_id] | join(",")')" \
    "relay-0,relay-1,relay-2,relay-3,relay-4" "(a) all five node ids present"

# ── (b) LIGHTSPEED_NODES override ────────────────────────────
OVERRIDE='{"edge-a":{"ip":"1.2.3.4"},"edge-b":{"ip":"5.6.7.8","health_url":"http://5.6.7.8/custom","metrics_url":"http://5.6.7.8/custom-metrics","node_id":"edge-bee","region":"eu-west","control_port":1443}}'
OUTOVR="$(resolve "$TMP/does-not-exist.json" "$OVERRIDE")"

assert_eq "$(printf '%s' "$OUTOVR" | jq -r 'length')" "2" "(b) override resolves 2 nodes without a registry"
assert_eq "$(printf '%s' "$OUTOVR" | jq -r '.[0].node_id')" "edge-a" "(b) override key becomes node_id"
assert_eq "$(printf '%s' "$OUTOVR" | jq -r '.[0].region')" "edge-a" "(b) override key becomes region"
assert_eq "$(printf '%s' "$OUTOVR" | jq -r '.[0].metrics_url')" "http://1.2.3.4:8080/metrics" "(b) override derives metrics_url"
assert_eq "$(printf '%s' "$OUTOVR" | jq -r '.[1].node_id')" "edge-bee" "(b) explicit node_id wins"
assert_eq "$(printf '%s' "$OUTOVR" | jq -r '.[1].health_url')" "http://5.6.7.8/custom" "(b) explicit health_url preserved"
assert_eq "$(printf '%s' "$OUTOVR" | jq -r '.[1].metrics_url')" "http://5.6.7.8/custom-metrics" "(b) explicit metrics_url preserved"
assert_eq "$(printf '%s' "$OUTOVR" | jq -r '.[1].control_port')" "1443" "(b) explicit control_port honored"

OUTPREC="$(resolve "$REG5" '{"solo":{"ip":"9.9.9.9"}}')"
assert_eq "$(printf '%s' "$OUTPREC" | jq -r 'length')" "1" "(b) override takes precedence over the registry"
assert_eq "$(printf '%s' "$OUTPREC" | jq -r '.[0].node_id')" "solo" "(b) overriding node id"

# ── (c) default registry resolves from any CWD ───────────────
OUT_CWD="$(cd "$TMP" && env -u LIGHTSPEED_NODES -u LIGHTSPEED_REGISTRY_PATH \
    bash -c 'source "$1"; lightspeed_resolve_nodes' _ "$LIB")"
assert_eq "$(printf '%s' "$OUT_CWD" | jq -r 'length')" "5" "(c) default registry resolves from another CWD"

# ── (d) missing / empty registry with no override ────────────
ERR_MISSING="$TMP/err-missing.txt"
if resolve "$TMP/does-not-exist.json" "" >/dev/null 2>"$ERR_MISSING"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(d) missing registry with no override is non-zero"
assert_contains "$ERR_MISSING" "not found" "(d) missing registry prints a clear error"

REG0="$TMP/reg0.json"
write_registry "$REG0" 0
ERR_EMPTY="$TMP/err-empty.txt"
if resolve "$REG0" "" >/dev/null 2>"$ERR_EMPTY"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(d) empty registry with no override is non-zero"
assert_contains "$ERR_EMPTY" "no nodes" "(d) empty registry prints a clear error"

# ── (e) deploy.sh fails closed on an empty inventory ─────────
DEPLOY_OUT="$TMP/deploy.out"
DEPLOY_ERR="$TMP/deploy.err"
if LIGHTSPEED_REGISTRY_PATH="$REG0" LIGHTSPEED_NODES='{}' \
        bash "$DEPLOY" >"$DEPLOY_OUT" 2>"$DEPLOY_ERR"; then rc=0; else rc=$?; fi
assert_rc_nonzero "$rc" "(e) deploy.sh fails closed on empty inventory"
assert_contains "$DEPLOY_ERR" "refus" "(e) deploy.sh prints a clear refusal"
if grep -q "Building release binary" "$DEPLOY_OUT" 2>/dev/null; then
    note_fail "(e) deploy.sh started building despite an empty inventory"
else
    note_pass
fi

# ── signed registry integrity ────────────────────────────────
assert_eq "$(jq -r '.registry | fromjson | .nodes | length' "$REGISTRY")" "5" \
    "signed registry still lists exactly 5 nodes"

# ── Verdict ──────────────────────────────────────────────────
if [ "$FAILURES" -eq 0 ]; then
    printf 'inventory: all assertions passed\n'
    printf '  (%s checks)\n' "$PASS"
    exit 0
fi

printf 'inventory: %s assertion(s) failed (%s passed)\n' "$FAILURES" "$PASS" >&2
exit 1
