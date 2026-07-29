#!/usr/bin/env bash
set -euo pipefail
VERSION=$(grep '^version' Cargo.toml | head -1 | sed 's/.*"\(.*\)".*/\1/')
ARCHIVE_DIR="target/release-archives"
mkdir -p "$ARCHIVE_DIR"
echo "⚡ Building LightSpeed v$VERSION"
cargo build --release -p lightspeed-client -p lightspeed-proxy
strip target/release/lightspeed 2>/dev/null || true
strip target/release/lightspeed-proxy 2>/dev/null || true
ARCH=$(rustc -vV | grep 'host:' | awk '{print $2}')
TARNAME="lightspeed-${VERSION}-${ARCH}.tar.gz"
tar -czf "$ARCHIVE_DIR/$TARNAME" -C target/release lightspeed lightspeed-proxy -C ../.. README.md CHANGELOG.md LICENSE docs/ 2>/dev/null || tar -czf "$ARCHIVE_DIR/$TARNAME" -C target/release lightspeed lightspeed-proxy
echo "✅ $ARCHIVE_DIR/$TARNAME"
echo "   lightspeed         ($(du -h target/release/lightspeed 2>/dev/null | cut -f1 || echo 'N/A'))"
echo "   lightspeed-proxy   ($(du -h target/release/lightspeed-proxy 2>/dev/null | cut -f1 || echo 'N/A'))"
