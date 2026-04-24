#!/usr/bin/env bash

set -euo pipefail

OPEN_DMG=0

usage() {
  cat <<EOF
Usage: $0 [--open]

Build, sign, and package the macOS app.

Options:
  --open    Open the generated DMG after packaging.
  -h, --help
            Show this help message.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --open)
      OPEN_DMG=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

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

SHIM_NAME="another-one-mcp-shim"
SHIM_BINARY_PATH="$ROOT_DIR/target/release/$SHIM_NAME"

echo "Building $PACKAGE_NAME + $SHIM_NAME for release..."
(
  cd "$ROOT_DIR"
  cargo build -p "$PACKAGE_NAME" -p "$SHIM_NAME" --release
)

if [[ ! -x "$BINARY_PATH" ]]; then
  echo "Expected built binary at $BINARY_PATH, but it was not found or is not executable." >&2
  exit 1
fi

if [[ ! -x "$SHIM_BINARY_PATH" ]]; then
  echo "Expected shim binary at $SHIM_BINARY_PATH, but it was not found or is not executable." >&2
  echo "The daemon MCP catalog entry will not work without it." >&2
  exit 1
fi

echo "Assembling $APP_NAME.app..."
rm -rf "$APP_BUNDLE" "$STAGING_DIR" "$DMG_PATH"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

install -m 755 "$BINARY_PATH" "$MACOS_DIR/$PACKAGE_NAME"
# Bundle the shim next to the main exe. `shim_binary_path()` in
# desktop/src/app.rs resolves the shim relative to current_exe,
# so it has to live in Contents/MacOS/ alongside the main binary.
install -m 755 "$SHIM_BINARY_PATH" "$MACOS_DIR/$SHIM_NAME"
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

if [[ "$OPEN_DMG" -eq 1 ]]; then
  echo "Opening $APP_NAME.dmg..."
  open "$DMG_PATH"
fi
