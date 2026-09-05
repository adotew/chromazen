#!/bin/bash
# Build x86_64 Linux release artifacts: an AppImage and a portable tarball.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"
APP="Chromazen"
APP_ID="io.github.adotew.chromazen"
ARCH="x86_64"
APPDIR="$ROOT/dist/$APP.AppDir"
APPIMAGE="$ROOT/dist/$APP-linux-$ARCH.AppImage"
ARCHIVE="$ROOT/dist/$APP-linux-$ARCH.tar.gz"
ARCHIVE_DIR="$ROOT/dist/$APP-linux-$ARCH"
APPIMAGETOOL_VERSION="13"
APPIMAGETOOL_SHA256="df3baf5ca5facbecfc2f3fa6713c29ab9cefa8fd8c1eac5d283b79cab33e4acb"
APPIMAGETOOL_URL="https://github.com/AppImage/AppImageKit/releases/download/$APPIMAGETOOL_VERSION/obsolete-appimagetool-x86_64.AppImage"

if [ "$(uname -s)" != "Linux" ] || [ "$(uname -m)" != "$ARCH" ]; then
  echo "Linux $ARCH is required to build these artifacts." >&2
  exit 1
fi

cd "$ROOT"
VERSION=$(cargo metadata --no-deps --format-version 1 | python3 -c \
  'import json, sys; print(json.load(sys.stdin)["packages"][0]["version"])')
echo "Building $APP $VERSION for Linux $ARCH"

cargo build --locked --release

rm -rf "$APPDIR" "$ARCHIVE_DIR"
rm -f "$APPIMAGE" "$ARCHIVE"
mkdir -p \
  "$APPDIR/usr/bin" \
  "$APPDIR/usr/share/applications" \
  "$APPDIR/usr/share/icons/hicolor/512x512/apps" \
  "$APPDIR/usr/share/metainfo" \
  "$ARCHIVE_DIR"

install -m 0755 "target/release/$APP" "$APPDIR/usr/bin/$APP"
install -m 0644 "packaging/linux/$APP_ID.desktop" \
  "$APPDIR/usr/share/applications/$APP_ID.desktop"
install -m 0644 "packaging/linux/$APP_ID.png" \
  "$APPDIR/usr/share/icons/hicolor/512x512/apps/$APP_ID.png"
install -m 0644 "packaging/linux/$APP_ID.appdata.xml" \
  "$APPDIR/usr/share/metainfo/$APP_ID.appdata.xml"
ln -s "usr/bin/$APP" "$APPDIR/AppRun"
ln -s "usr/share/applications/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
ln -s "usr/share/icons/hicolor/512x512/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"

APPIMAGETOOL=${APPIMAGETOOL:-"$ROOT/target/appimagetool-x86_64.AppImage"}
if [ ! -f "$APPIMAGETOOL" ]; then
  echo "Downloading appimagetool $APPIMAGETOOL_VERSION"
  curl --fail --location --silent --show-error "$APPIMAGETOOL_URL" -o "$APPIMAGETOOL"
fi
echo "$APPIMAGETOOL_SHA256  $APPIMAGETOOL" | sha256sum --check --status || {
  echo "appimagetool checksum verification failed." >&2
  exit 1
}
chmod +x "$APPIMAGETOOL"
ARCH="$ARCH" "$APPIMAGETOOL" --appimage-extract-and-run "$APPDIR" "$APPIMAGE"
chmod +x "$APPIMAGE"

install -m 0755 "target/release/$APP" "$ARCHIVE_DIR/$APP"
install -m 0644 README.md "$ARCHIVE_DIR/README.md"
install -m 0644 "packaging/linux/$APP_ID.desktop" "$ARCHIVE_DIR/$APP_ID.desktop"
install -m 0644 "packaging/linux/$APP_ID.png" "$ARCHIVE_DIR/$APP_ID.png"
install -m 0644 "packaging/linux/$APP_ID.appdata.xml" \
  "$ARCHIVE_DIR/$APP_ID.appdata.xml"
tar -C "$ROOT/dist" -czf "$ARCHIVE" "$APP-linux-$ARCH"
rm -rf "$APPDIR" "$ARCHIVE_DIR"

echo "Packaged $APPIMAGE"
echo "Packaged $ARCHIVE"
