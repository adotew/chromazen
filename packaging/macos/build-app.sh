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

# Drag-to-install DMG with a polished layout: background image + positioned icons.
STAGING=$(mktemp -d "$ROOT/dist/dmg-staging.XXXXXX")
TMP_DMG="$ROOT/dist/$APP-tmp-$$.dmg"
MOUNT=""
DEVICE=""

# Finder can briefly keep the image busy after writing its layout.
detach_dmg() {
  local delay
  for delay in 1 2 4; do
    if hdiutil detach "$1"; then
      return 0
    fi
    sleep "$delay"
  done
  hdiutil detach -force "$1"
}

cleanup_dmg() {
  if [ -n "$DEVICE" ] && ! detach_dmg "$DEVICE" >/dev/null 2>&1; then
    echo "WARNING: Could not detach $DEVICE; temporary files were preserved." >&2
    return
  fi
  rm -rf "$STAGING" "$TMP_DMG" "$DMG"
}
trap cleanup_dmg EXIT

rm -f "$TMP_DMG" "$DMG"
mkdir -p "$STAGING/.background"
cp -R "$BUNDLE" "$STAGING/"
ln -s /Applications "$STAGING/Applications"
cp "$ROOT/packaging/macos/dmg-background.png" "$STAGING/.background/background.png"
# A 2x, 144-DPI image fills Finder's 660x400-point window on Retina displays.
sips --resampleHeightWidth 800 1320 \
  --setProperty dpiWidth 144 --setProperty dpiHeight 144 \
  "$STAGING/.background/background.png" >/dev/null

# Build writable, mount, lay out icons with Finder, compress to UDZO.
hdiutil create -volname "$APP" -srcfolder "$STAGING" -ov -fs HFS+ -format UDRW "$TMP_DMG"
ATTACH_OUTPUT=$(hdiutil attach "$TMP_DMG" -readwrite -nobrowse -noautoopen -mountrandom /Volumes)
DEVICE=$(printf '%s\n' "$ATTACH_OUTPUT" | awk '/^\/dev\// {print $1; exit}')
MOUNT=$(printf '%s\n' "$ATTACH_OUTPUT" | awk -F '\t' '/\/Volumes\// {print $NF; exit}')
if [ -z "$DEVICE" ] || [ -z "$MOUNT" ]; then
  echo "Could not identify the mounted DMG." >&2
  exit 1
fi
chflags hidden "$MOUNT/.background"

osascript - "$(basename "$MOUNT")" <<'OSA'
on run argv
  tell application "Finder"
    tell disk (item 1 of argv)
      open
      set w to container window
      tell w
        set current view to icon view
        set toolbar visible to false
        set statusbar visible to false
        set the bounds to {100, 150, 760, 550}
      end tell
      set opts to icon view options of w
      tell opts
        set arrangement to not arranged
        set icon size to 128
        set shows item info to false
      end tell
      set background picture of opts to file ".background:background.png"
      set position of item "Chromazen.app" to {170, 180}
      set position of item "Applications" to {490, 180}
      close
      open
    end tell
    delay 3
  end tell
end run
OSA

if [ ! -f "$MOUNT/.DS_Store" ]; then
  echo "Finder did not save the DMG layout." >&2
  exit 1
fi
sync
detach_dmg "$DEVICE"
DEVICE=""
hdiutil convert "$TMP_DMG" -format UDZO -imagekey zlib-level=9 -o "$DMG"

if [ -n "${MACOS_SIGNING_IDENTITY:-}" ]; then
  codesign --force --timestamp --sign "$MACOS_SIGNING_IDENTITY" "$DMG"
fi
hdiutil verify "$DMG"
rm -rf "$STAGING" "$TMP_DMG"
trap - EXIT

# Notarize and staple, when credentials are provided.
if [ -n "${APPLE_ID:-}" ]; then
  xcrun notarytool store-credentials chromazen-notary \
    --apple-id "$APPLE_ID" \
    --password "$APPLE_APP_SPECIFIC_PASSWORD" \
    --team-id "$APPLE_TEAM_ID"
  xcrun notarytool submit "$DMG" --keychain-profile chromazen-notary --wait
  xcrun stapler staple "$DMG"
  xcrun stapler validate "$DMG"
fi

if [ "${1:-}" = "--install" ]; then
  rm -rf "/Applications/$APP.app"
  cp -R "$BUNDLE" "/Applications/$APP.app"
  touch "/Applications/$APP.app"
  echo "Installed /Applications/$APP.app"
else
  echo "Packaged $DMG"
fi
