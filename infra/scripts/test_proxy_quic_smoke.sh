#!/usr/bin/env bash
# ──────────────────────────────────────────────────────────────
# LightSpeed — Release QUIC control-plane smoke test
#
# Builds the release proxy with `--features quic` and asserts that it starts
# with the QUIC control plane bound. This exercises the real release artifact,
# which unit tests do not: `cargo test -p lightspeed-proxy --features quic`
# unifies crate features differently from `cargo build --release`, so a startup
# regression can pass the unit tests and still panic in the shipped binary.
#
# Catches the ways this has broken:
#   1. a build without the quic feature (control plane compiled out),
#   2. a startup panic from an ambiguous rustls CryptoProvider, and
#   3. a control-plane ALPN mode that the shipped binary does not actually
#      serve, which the startup line and GET /health must both report
#      (docs/quic-alpn-rollout.md §6.4/§7.2/§7.3).
#
# Usage: bash infra/scripts/test_proxy_quic_smoke.sh
# Requires: cargo, bash, curl. No network access.
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_ROOT"

# The two instances differ only in `[network] control_alpn`; the second one is
# the stage-3 shape and must prove its mode from the shipped artifact.
DATA_PORT=14534
CONTROL_PORT=14533
HEALTH_PORT=18580
REQUIRED_DATA_PORT=15534
REQUIRED_CONTROL_PORT=15533
REQUIRED_HEALTH_PORT=19580
SMOKE_DIR="$(mktemp -d)"
PID=""
REQUIRED_PID=""

cleanup() {
    if [ -n "$PID" ]; then
        kill "$PID" 2>/dev/null || true
    fi
    if [ -n "$REQUIRED_PID" ]; then
        kill "$REQUIRED_PID" 2>/dev/null || true
    fi
    rm -rf "$SMOKE_DIR"
}
trap cleanup EXIT

fail() {
    echo "proxy-quic-smoke: FAIL - $1" >&2
    exit 1
}

command -v curl >/dev/null 2>&1 || fail "curl is required to read GET /health"

# wait_for_log <logfile> <fixed string> <pid>: bounded wait for one startup line.
wait_for_log() {
    local log="$1" pattern="$2" i
    for i in $(seq 1 100); do
        if grep -aqF "$pattern" "$log"; then
            return 0
        fi
        if ! kill -0 "$3" 2>/dev/null; then
            return 1
        fi
        sleep 0.2
    done
    return 1
}

# health_body <port>: print GET /health once the HTTP server answers.
health_body() {
    local port="$1" i body
    for i in $(seq 1 100); do
        if body="$(curl -fsS "http://127.0.0.1:$port/health" 2>/dev/null)"; then
            printf "%s" "$body"
            return 0
        fi
        sleep 0.2
    done
    return 1
}

echo "proxy-quic-smoke: building release proxy with --features quic..."
cargo build --release --bin lightspeed-proxy --features quic

BIN="$PROJECT_ROOT/target/release/lightspeed-proxy"
if [ ! -x "$BIN" ]; then
    fail "binary not found: $BIN"
fi

# ── Instance 1: default mode (`control_alpn` absent = "off") ─────────
printf '[server]\nnode_id = "quic-smoke"\nregion = "test"\nmax_clients = 10\n' \
    > "$SMOKE_DIR/proxy.toml"

LIGHTSPEED_TLS_DIR="$SMOKE_DIR/tls-off" \
LIGHTSPEED_GEO_DISABLED=1 \
"$BIN" --config "$SMOKE_DIR/proxy.toml" \
    --data-bind "127.0.0.1:$DATA_PORT" \
    --control-bind "127.0.0.1:$CONTROL_PORT" \
    --health-bind "127.0.0.1:$HEALTH_PORT" \
    > "$SMOKE_DIR/out.log" 2>&1 &
PID=$!

if ! wait_for_log "$SMOKE_DIR/out.log" "QUIC control plane listening" "$PID"; then
    tail -20 "$SMOKE_DIR/out.log" >&2
    fail "the proxy did not start its QUIC control plane"
fi

if ! grep -aqF "alpn=off" "$SMOKE_DIR/out.log"; then
    grep -a "QUIC control plane listening" "$SMOKE_DIR/out.log" >&2 || true
    fail "the default startup line must say alpn=off"
fi

HEALTH="$(health_body "$HEALTH_PORT")" || fail "GET /health did not answer on :$HEALTH_PORT"
case "$HEALTH" in
    *'"control_alpn":"off"'*) ;;
    *) fail "/health must report control_alpn=off, got: $HEALTH" ;;
esac

# ── Instance 2: `control_alpn = "required"` ─────────────────────────
printf '[server]\nnode_id = "quic-smoke-required"\nregion = "test"\nmax_clients = 10\n\n[network]\ncontrol_alpn = "required"\n' \
    > "$SMOKE_DIR/required.toml"

LIGHTSPEED_TLS_DIR="$SMOKE_DIR/tls-required" \
LIGHTSPEED_GEO_DISABLED=1 \
"$BIN" --config "$SMOKE_DIR/required.toml" \
    --data-bind "127.0.0.1:$REQUIRED_DATA_PORT" \
    --control-bind "127.0.0.1:$REQUIRED_CONTROL_PORT" \
    --health-bind "127.0.0.1:$REQUIRED_HEALTH_PORT" \
    > "$SMOKE_DIR/required.log" 2>&1 &
REQUIRED_PID=$!

if ! wait_for_log "$SMOKE_DIR/required.log" "QUIC control plane listening" "$REQUIRED_PID"; then
    tail -20 "$SMOKE_DIR/required.log" >&2
    fail "the required-mode proxy did not start its QUIC control plane"
fi

if ! grep -aqF "alpn=required (lightspeed-control/1)" "$SMOKE_DIR/required.log"; then
    grep -a "QUIC control plane listening" "$SMOKE_DIR/required.log" >&2 || true
    fail "the required-mode startup line must say alpn=required (lightspeed-control/1)"
fi

REQUIRED_HEALTH="$(health_body "$REQUIRED_HEALTH_PORT")" \
    || fail "GET /health did not answer on :$REQUIRED_HEALTH_PORT"
case "$REQUIRED_HEALTH" in
    *'"control_alpn":"required"'*) ;;
    *) fail "/health must report control_alpn=required, got: $REQUIRED_HEALTH" ;;
esac

echo "proxy-quic-smoke: all assertions passed"
echo "  (off: control plane on 127.0.0.1:$CONTROL_PORT, alpn=off, /health agrees)"
echo "  (required: control plane on 127.0.0.1:$REQUIRED_CONTROL_PORT, alpn=required (lightspeed-control/1), /health agrees)"
exit 0
