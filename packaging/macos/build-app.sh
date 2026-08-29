#!/bin/bash
# Build a distributable macOS app bundle (universal binary) and DMG.
#
# Environment (all optional — falls back to ad-hoc signing when unset):
#   MACOS_SIGNING_IDENTITY   e.g. "Developer ID Application: Your Name (TEAMID)"
#   APPLE_ID                 Apple ID for notarization
#   APPLE_APP_SPECIFIC_PASSWORD  app-specific password for APPLE_ID
#   APPLE_TEAM_ID            App Store Connect team ID
#
# ./packaging/macos/build-app.sh --install installs to /Applications instead
# of (after) producing a DMG, for local use.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="Chromazen"
BUNDLE="$ROOT/dist/$APP.app"
DMG="$ROOT/dist/$APP.dmg"

cd "$ROOT"
VERSION=$(grep -m1 '^version' Cargo.toml | cut -d'"' -f2)
echo "Building $APP $VERSION (universal)"

# Universal binary: arm64 (default) + x86_64, merged with lipo.
rustup target add x86_64-apple-darwin
cargo build --release --target aarch64-apple-darwin
cargo build --release --target x86_64-apple-darwin
lipo -create -output "$APP" \
  "target/aarch64-apple-darwin/release/$APP" \
  "target/x86_64-apple-darwin/release/$APP"

mkdir -p "dist/$APP.app/Contents/MacOS" "dist/$APP.app/Contents/Resources"
cp "$APP" "dist/$APP.app/Contents/MacOS/$APP"
cp "packaging/macos/Info.plist" "dist/$APP.app/Contents/Info.plist"
sed -i '' "s/<string>0\.1\.0<\/string>/<string>$VERSION<\/string>/g" "dist/$APP.app/Contents/Info.plist"
cp "assets/AppIcon.icns" "dist/$APP.app/Contents/Resources/AppIcon.icns"
rm "$APP"

# Sign. Hardened runtime + timestamp are required for notarization.
if [ -n "${MACOS_SIGNING_IDENTITY:-}" ]; then
  codesign --force --deep --options runtime --timestamp \
    --sign "$MACOS_SIGNING_IDENTITY" "$BUNDLE"
else
  codesign --force --deep --sign - "$BUNDLE"
  echo "WARNING: ad-hoc signed (MACOS_SIGNING_IDENTITY unset) — users will need to bypass Gatekeeper."
fi

hdiutil create -volname "$APP" -srcfolder "$BUNDLE" -ov -format UDZO "$DMG"

# Notarize and staple, when credentials are provided.
if [ -n "${APPLE_ID:-}" ]; then
  xcrun notarytool store-credentials chromazen-notary \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --team-id "$APPLE_TEAM_ID"
  xcrun notarytool submit "$DMG" --keychain-profile chromazen-notary --wait
  xcrun stapler staple "$DMG"
fi

if [ "${1:-}" = "--install" ]; then
  rm -rf "/Applications/$APP.app"
  cp -R "$BUNDLE" "/Applications/$APP.app"
  touch "/Applications/$APP.app"
  echo "Installed /Applications/$APP.app"
else
  echo "Packaged $DMG"
fi
