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
# Catches the two ways this has broken:
#   1. a build without the quic feature (control plane compiled out), and
#   2. a startup panic from an ambiguous rustls CryptoProvider.
#
# Usage: bash infra/scripts/test_proxy_quic_smoke.sh
# Requires: cargo, bash. No network access.
# ──────────────────────────────────────────────────────────────
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
cd "$PROJECT_ROOT"

DATA_PORT=14534
CONTROL_PORT=14533
HEALTH_PORT=18580
SMOKE_DIR="$(mktemp -d)"
PID=""

cleanup() {
    if [ -n "$PID" ]; then
        kill "$PID" 2>/dev/null || true
    fi
    rm -rf "$SMOKE_DIR"
}
trap cleanup EXIT

echo "proxy-quic-smoke: building release proxy with --features quic..."
cargo build --release --bin lightspeed-proxy --features quic

BIN="$PROJECT_ROOT/target/release/lightspeed-proxy"
if [ ! -x "$BIN" ]; then
    echo "proxy-quic-smoke: FAIL - binary not found: $BIN" >&2
    exit 1
fi

printf '[server]\nnode_id = "quic-smoke"\nregion = "test"\nmax_clients = 10\n' \
    > "$SMOKE_DIR/proxy.toml"

LIGHTSPEED_TLS_DIR="$SMOKE_DIR/tls" \
LIGHTSPEED_GEO_DISABLED=1 \
"$BIN" --config "$SMOKE_DIR/proxy.toml" \
    --data-bind "127.0.0.1:$DATA_PORT" \
    --control-bind "127.0.0.1:$CONTROL_PORT" \
    --health-bind "127.0.0.1:$HEALTH_PORT" \
    > "$SMOKE_DIR/out.log" 2>&1 &
PID=$!

for _ in $(seq 1 50); do
    if grep -aq "QUIC control plane listening" "$SMOKE_DIR/out.log"; then
        break
    fi
    if ! kill -0 "$PID" 2>/dev/null; then
        break
    fi
    sleep 0.2
done

if grep -aq "QUIC control plane listening" "$SMOKE_DIR/out.log"; then
    echo "proxy-quic-smoke: all assertions passed"
    echo "  (control plane listening on 127.0.0.1:$CONTROL_PORT)"
    exit 0
fi

echo "proxy-quic-smoke: FAIL - the proxy did not start its QUIC control plane" >&2
tail -20 "$SMOKE_DIR/out.log" >&2
exit 1
