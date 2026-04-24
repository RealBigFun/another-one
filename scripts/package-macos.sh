#!/usr/bin/env bash

set -euo pipefail

if [[ "$(uname -s)" != "Darwin" ]]; then
  echo "macOS packaging requires Darwin; current platform is $(uname -s)." >&2
  exit 1
fi

ROOT_DIR="$(
  cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd
)"

APP_NAME="AnotherOne"
PACKAGE_NAME="another-one"
BUNDLE_ID="dev.anotherone.desktop"
MIN_MACOS_VERSION="11.0"

RELEASE_DIR="$ROOT_DIR/target/release"
PACKAGE_DIR="$RELEASE_DIR/macos"
APP_BUNDLE="$PACKAGE_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
ASSETS_DIR="$RESOURCES_DIR/assets"
BINARY_PATH="$RELEASE_DIR/$PACKAGE_NAME"
DMG_PATH="$PACKAGE_DIR/$APP_NAME.dmg"
STAGING_DIR="$PACKAGE_DIR/dmg-staging"

ICON_PATH="$ROOT_DIR/desktop/assets/app-icon/macos/$APP_NAME.icns"
ASSETS_SOURCE="$ROOT_DIR/desktop/assets"

if [[ ! -f "$ICON_PATH" ]]; then
  echo "Expected macOS app icon at $ICON_PATH." >&2
  exit 1
fi

echo "Building $PACKAGE_NAME for release..."
(
  cd "$ROOT_DIR"
  cargo build -p "$PACKAGE_NAME" --release
)

if [[ ! -x "$BINARY_PATH" ]]; then
  echo "Expected built binary at $BINARY_PATH, but it was not found or is not executable." >&2
  exit 1
fi

echo "Assembling $APP_NAME.app..."
rm -rf "$APP_BUNDLE" "$STAGING_DIR" "$DMG_PATH"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

install -m 755 "$BINARY_PATH" "$MACOS_DIR/$PACKAGE_NAME"
ditto "$ASSETS_SOURCE" "$ASSETS_DIR"
install -m 644 "$ICON_PATH" "$RESOURCES_DIR/$APP_NAME.icns"

cat > "$CONTENTS_DIR/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>$APP_NAME</string>
  <key>CFBundleExecutable</key>
  <string>$PACKAGE_NAME</string>
  <key>CFBundleIconFile</key>
  <string>$APP_NAME</string>
  <key>CFBundleIdentifier</key>
  <string>$BUNDLE_ID</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>$APP_NAME</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>0.1.0</string>
  <key>CFBundleVersion</key>
  <string>0.1.0</string>
  <key>LSMinimumSystemVersion</key>
  <string>$MIN_MACOS_VERSION</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

echo "Signing $APP_NAME.app with an ad-hoc identity..."
codesign --force --deep --sign - "$APP_BUNDLE"

echo "Creating $APP_NAME.dmg..."
mkdir -p "$STAGING_DIR"
ditto "$APP_BUNDLE" "$STAGING_DIR/$APP_NAME.app"
ln -s /Applications "$STAGING_DIR/Applications"
hdiutil create \
  -volname "$APP_NAME" \
  -srcfolder "$STAGING_DIR" \
  -ov \
  -format UDZO \
  "$DMG_PATH"
rm -rf "$STAGING_DIR"

echo "Created:"
echo "  $APP_BUNDLE"
echo "  $DMG_PATH"

echo "Opening $APP_NAME.dmg..."
open "$DMG_PATH"
