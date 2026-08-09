#!/bin/bash
# Build Chromazen in release mode, assemble the macOS app bundle,
# ad-hoc sign it, and install it to /Applications.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="Chromazen"
DIST="$ROOT/dist/$APP.app"
INSTALL="/Applications/$APP.app"

cd "$ROOT"
cargo build --release

mkdir -p "$DIST/Contents/MacOS" "$DIST/Contents/Resources"
cp "target/release/$APP" "$DIST/Contents/MacOS/$APP"
cp "packaging/macos/Info.plist" "$DIST/Contents/Info.plist"
cp "assets/AppIcon.icns" "$DIST/Contents/Resources/AppIcon.icns"

codesign --force --deep --sign - "$DIST"

rm -rf "$INSTALL"
cp -R "$DIST" "$INSTALL"
touch "$INSTALL"

echo "Installed $INSTALL"
