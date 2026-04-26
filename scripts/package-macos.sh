#!/usr/bin/env bash
# Build, ad-hoc sign, and DMG-package the AnotherOne macOS app.
#
# Flutter's macOS build already produces a `AnotherOne.app`
# bundle with a correct Info.plist + Frameworks/ + Resources/
# layout — we just need to drop the mcp-shim cargo binary into
# Contents/MacOS/ alongside the Flutter binary so the embedded
# daemon's MCP catalog entry can find it via current_exe-relative
# path resolution.

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
SHIM_NAME="another-one-mcp-shim"

FLUTTER_DIR="$ROOT_DIR/$PACKAGE_NAME"
RELEASE_DIR="$ROOT_DIR/target/release"
SHIM_BINARY_PATH="$RELEASE_DIR/$SHIM_NAME"

PACKAGE_DIR="$RELEASE_DIR/macos"
APP_BUNDLE="$PACKAGE_DIR/$APP_NAME.app"
DMG_PATH="$PACKAGE_DIR/$APP_NAME.dmg"
STAGING_DIR="$PACKAGE_DIR/dmg-staging"

echo "==> building $PACKAGE_NAME (flutter desktop, release)"
(
  cd "$FLUTTER_DIR"
  flutter build macos --release
)

FLUTTER_APP_BUNDLE="$FLUTTER_DIR/build/macos/Build/Products/Release/$APP_NAME.app"
if [[ ! -d "$FLUTTER_APP_BUNDLE" ]]; then
  echo "Expected Flutter app bundle at $FLUTTER_APP_BUNDLE after build." >&2
  exit 1
fi

echo "==> building $SHIM_NAME (cargo, release)"
(
  cd "$ROOT_DIR"
  cargo build -p "$SHIM_NAME" --release
)

if [[ ! -x "$SHIM_BINARY_PATH" ]]; then
  echo "Expected shim binary at $SHIM_BINARY_PATH after build." >&2
  echo "The daemon MCP catalog entry will not work without it." >&2
  exit 1
fi

echo "==> assembling $APP_NAME.app"
rm -rf "$APP_BUNDLE" "$STAGING_DIR" "$DMG_PATH"
mkdir -p "$PACKAGE_DIR"
# `ditto` preserves macOS metadata (extended attrs, the .app's
# Frameworks/ symlinks). Don't substitute `cp -r` — `cp` strips
# the symlink semantics that Flutter's bundle relies on.
ditto "$FLUTTER_APP_BUNDLE" "$APP_BUNDLE"

# Drop the shim binary next to the Flutter binary. The bridge's
# embedded daemon resolves `another-one-mcp-shim` relative to
# `current_exe` (Contents/MacOS/<binary>), so the shim has to
# live in the same directory.
install -m 755 "$SHIM_BINARY_PATH" "$APP_BUNDLE/Contents/MacOS/$SHIM_NAME"

echo "==> signing $APP_NAME.app with an ad-hoc identity"
# --deep re-signs every nested bundle (Frameworks/FlutterMacOS.framework,
# the embedded helpers Flutter ships, and the shim we just dropped in)
# so Gatekeeper doesn't flag the inner artefacts as untrusted.
codesign --force --deep --sign - "$APP_BUNDLE"

echo "==> creating $APP_NAME.dmg"
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

echo ""
echo "Created:"
echo "  $APP_BUNDLE"
echo "  $DMG_PATH"

if [[ "$OPEN_DMG" -eq 1 ]]; then
  echo "==> opening $DMG_PATH"
  open "$DMG_PATH"
fi
