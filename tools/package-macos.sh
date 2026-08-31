#!/usr/bin/env bash
# Build a macOS .app bundle + .dmg for the LightSpeed GUI.
#
# Run on a Mac after `cargo build --release`. Produces LightSpeed.app and
# LightSpeed-<version>.dmg in the current directory. The .app is ad-hoc signed,
# which is sufficient to run on Apple Silicon; users will need to right-click →
# Open the first time (or `xattr -dr com.apple.quarantine LightSpeed.app`).
#
# For Gatekeeper-clean distribution, replace the `codesign --sign -` line with a
# Developer ID cert (`--options runtime`) and run `xcrun notarytool submit` +
# `xcrun stapler staple`.
set -euo pipefail

BIN_NAME="lightspeed-gui"
APP_NAME="LightSpeed"
VERSION="${1:?usage: $0 <version> [binary-path]}"
BIN_PATH="${2:-target/release/$BIN_NAME}"
STAGING="dmg_staging"

APP="$APP_NAME.app"
rm -rf "$APP" "$STAGING"
mkdir -p "$APP/Contents/MacOS" "$APP/Contents/Resources"
cp "$BIN_PATH" "$APP/Contents/MacOS/$BIN_NAME"
chmod +x "$APP/Contents/MacOS/$BIN_NAME"
sed "s/__VERSION__/$VERSION/g" packaging/macos/Info.plist > "$APP/Contents/Info.plist"

# Ad-hoc code sign (required to run on Apple Silicon; a valid signature of any
# identity is enough).
codesign --force --deep --sign - "$APP"
codesign --verify --verbose "$APP"

# Stage a drag-to-Applications .dmg.
mkdir -p "$STAGING"
cp -R "$APP" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
hdiutil create -volname "$APP_NAME" -srcfolder "$STAGING" -ov -format UDZO "$APP_NAME-$VERSION.dmg"

echo "Built $APP and $APP_NAME-$VERSION.dmg"
