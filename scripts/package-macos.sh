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

Environment overrides (used by CI):
  CARGO_VERSION       Override the bundled CFBundleShortVersionString
                      (defaults to the version in app/Cargo.toml).
  RELEASE_ID          Full git SHA stamped into output filenames.
  BUILD_NUMBER        Monotonic CFBundleVersion value (default: 1).
  TARGET_TRIPLE       Cargo target triple, e.g. aarch64-apple-darwin.
  OUTPUT_DIR          Where to drop the .app bundle, .dmg, and the
                      updater payload (.app.tar.gz). Defaults to
                      target/release/macos.
  ARTIFACT_PREFIX     Filename prefix for release-named outputs.
  MACOS_SIGN_IDENTITY Developer ID Application identity used for
                      distribution signing. Defaults to ad-hoc signing.
  MACOS_NOTARIZE      Set to 1 to notarize and staple the app/DMG. Requires
                      MACOS_SIGN_IDENTITY and notarytool credentials.
  MACOS_NOTARY_PROFILE
                      Keychain profile name for xcrun notarytool.
                      Alternatively set APPLE_ID, APPLE_TEAM_ID, and
                      APPLE_APP_SPECIFIC_PASSWORD.
  ANOTHER_ONE_BUILD_FULL_SHA
                      Forwarded to cargo so build.rs can stamp the
                      full SHA into the binary even when the worktree
                      lacks .git history (CI shallow clones).
  ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX
                      Forwarded to cargo so the binary embeds the
                      Ed25519 public key used to verify update
                      manifests.
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

CARGO_VERSION="${CARGO_VERSION:-}"
if [[ -z "$CARGO_VERSION" ]]; then
  CARGO_VERSION="$(awk -F '"' '/^version = / { print $2; exit }' "$ROOT_DIR/app/Cargo.toml")"
fi
RELEASE_ID="${RELEASE_ID:-}"
BUILD_NUMBER="${BUILD_NUMBER:-1}"
TARGET_TRIPLE="${TARGET_TRIPLE:-}"
ARTIFACT_PREFIX="${ARTIFACT_PREFIX:-}"
MACOS_SIGN_IDENTITY="${MACOS_SIGN_IDENTITY:-}"
# Strip stray whitespace that creeps in when secrets are populated
# via `echo value | gh secret set ...` (echo appends a newline).
MACOS_SIGN_IDENTITY="$(printf %s "$MACOS_SIGN_IDENTITY" | tr -d '\r\n' | sed -e 's/^[[:space:]]*//' -e 's/[[:space:]]*$//')"
MACOS_SIGN_KEYCHAIN="${MACOS_SIGN_KEYCHAIN:-}"
MACOS_NOTARIZE="${MACOS_NOTARIZE:-0}"
MACOS_NOTARY_PROFILE="${MACOS_NOTARY_PROFILE:-}"
APPLE_ID="${APPLE_ID:-}"
APPLE_TEAM_ID="${APPLE_TEAM_ID:-}"
APPLE_APP_SPECIFIC_PASSWORD="${APPLE_APP_SPECIFIC_PASSWORD:-}"

if [[ "$MACOS_NOTARIZE" != "0" && "$MACOS_NOTARIZE" != "1" ]]; then
  echo "MACOS_NOTARIZE must be 0 or 1." >&2
  exit 1
fi

if [[ "$MACOS_NOTARIZE" == "1" ]]; then
  if [[ -z "$MACOS_SIGN_IDENTITY" ]]; then
    echo "MACOS_SIGN_IDENTITY is required when MACOS_NOTARIZE=1." >&2
    exit 1
  fi
  if [[ -z "$MACOS_NOTARY_PROFILE" ]]; then
    if [[ -z "$APPLE_ID" || -z "$APPLE_TEAM_ID" || -z "$APPLE_APP_SPECIFIC_PASSWORD" ]]; then
      echo "MACOS_NOTARIZE=1 requires either MACOS_NOTARY_PROFILE or APPLE_ID, APPLE_TEAM_ID, and APPLE_APP_SPECIFIC_PASSWORD." >&2
      exit 1
    fi
  fi
fi

if [[ -n "$RELEASE_ID" ]]; then
  TRUST_PUBKEY_HEX="${ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX:-}"
  if [[ -z "$TRUST_PUBKEY_HEX" ]]; then
    echo "ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX is required when RELEASE_ID is set." >&2
    echo "Without it, the packaged app cannot verify update manifests." >&2
    exit 1
  fi
  if [[ ! "$TRUST_PUBKEY_HEX" =~ ^[0-9a-fA-F]{64}$ ]]; then
    echo "ANOTHER_ONE_UPDATE_TRUST_PUBKEY_HEX must be a 32-byte Ed25519 public key encoded as 64 hex characters." >&2
    exit 1
  fi
fi

if [[ -n "$TARGET_TRIPLE" ]]; then
  RELEASE_DIR="$ROOT_DIR/target/$TARGET_TRIPLE/release"
else
  RELEASE_DIR="$ROOT_DIR/target/release"
fi
PACKAGE_DIR="${OUTPUT_DIR:-$RELEASE_DIR/macos}"
APP_BUNDLE="$PACKAGE_DIR/$APP_NAME.app"
CONTENTS_DIR="$APP_BUNDLE/Contents"
MACOS_DIR="$CONTENTS_DIR/MacOS"
RESOURCES_DIR="$CONTENTS_DIR/Resources"
ASSETS_DIR="$RESOURCES_DIR/assets"
BINARY_PATH="$RELEASE_DIR/$PACKAGE_NAME"
DMG_PATH="$PACKAGE_DIR/$APP_NAME.dmg"
STAGING_DIR="$PACKAGE_DIR/dmg-staging"
NOTARY_ZIP="$PACKAGE_DIR/$APP_NAME.notary.zip"
UPDATE_PAYLOAD="$PACKAGE_DIR/$APP_NAME.app.tar.gz"

ICON_PATH="$ROOT_DIR/app/assets/app-icon/macos/$APP_NAME.icns"
ASSETS_SOURCE="$ROOT_DIR/app/assets"

if [[ ! -f "$ICON_PATH" ]]; then
  echo "Expected macOS app icon at $ICON_PATH." >&2
  exit 1
fi

SHIM_NAME="another-one-mcp-shim"
SHIM_BINARY_PATH="$RELEASE_DIR/$SHIM_NAME"

notarytool_args=()
if [[ -n "$MACOS_NOTARY_PROFILE" ]]; then
  notarytool_args=(--keychain-profile "$MACOS_NOTARY_PROFILE")
else
  notarytool_args=(
    --apple-id "$APPLE_ID"
    --team-id "$APPLE_TEAM_ID"
    --password "$APPLE_APP_SPECIFIC_PASSWORD"
  )
fi

sign_file() {
  local path="$1"
  if [[ -n "$MACOS_SIGN_IDENTITY" ]]; then
    local args=(--force --timestamp --options runtime --sign "$MACOS_SIGN_IDENTITY")
    if [[ -n "$MACOS_SIGN_KEYCHAIN" ]]; then
      args+=(--keychain "$MACOS_SIGN_KEYCHAIN")
    fi
    codesign "${args[@]}" "$path"
  else
    codesign --force --sign - "$path"
  fi
}

notarize_artifact() {
  local path="$1"
  if [[ "$MACOS_NOTARIZE" != "1" ]]; then
    return
  fi
  echo "Notarizing $path..."
  xcrun notarytool submit "$path" --wait "${notarytool_args[@]}"
}

echo "Building $PACKAGE_NAME + $SHIM_NAME for release..."
build_args=(build -p "$PACKAGE_NAME" -p "$SHIM_NAME" --release)
if [[ -n "$TARGET_TRIPLE" ]]; then
  build_args+=(--target "$TARGET_TRIPLE")
fi
cargo_cmd=(cargo)
if command -v rustup >/dev/null 2>&1; then
  cargo_cmd=("$(rustup which cargo)")
fi
(
  cd "$ROOT_DIR"
  "${cargo_cmd[@]}" "${build_args[@]}"
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
rm -rf "$APP_BUNDLE" "$STAGING_DIR" "$DMG_PATH" "$NOTARY_ZIP"
mkdir -p "$MACOS_DIR" "$RESOURCES_DIR"

install -m 755 "$BINARY_PATH" "$MACOS_DIR/$PACKAGE_NAME"
# Bundle the shim next to the main exe. `shim_binary_path()` in
# app/src/app.rs resolves the shim relative to current_exe,
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
  <string>$CARGO_VERSION</string>
  <key>CFBundleVersion</key>
  <string>$BUILD_NUMBER</string>
  <key>LSMinimumSystemVersion</key>
  <string>$MIN_MACOS_VERSION</string>
  <key>NSHighResolutionCapable</key>
  <true/>
</dict>
</plist>
PLIST

if [[ -n "$MACOS_SIGN_IDENTITY" ]]; then
  echo "Signing $APP_NAME.app with Developer ID identity..."
else
  echo "Signing $APP_NAME.app with an ad-hoc identity..."
fi
sign_file "$MACOS_DIR/$SHIM_NAME"
sign_file "$MACOS_DIR/$PACKAGE_NAME"
sign_file "$APP_BUNDLE"
codesign --verify --strict --deep --verbose=2 "$APP_BUNDLE"

if [[ "$MACOS_NOTARIZE" == "1" ]]; then
  echo "Creating notarization upload $NOTARY_ZIP..."
  ditto -c -k --keepParent "$APP_BUNDLE" "$NOTARY_ZIP"
  notarize_artifact "$NOTARY_ZIP"
  xcrun stapler staple "$APP_BUNDLE"
  rm -f "$NOTARY_ZIP"
fi

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

if [[ -n "$MACOS_SIGN_IDENTITY" ]]; then
  echo "Signing $APP_NAME.dmg with Developer ID identity..."
  dmg_sign_args=(--force --timestamp --sign "$MACOS_SIGN_IDENTITY")
  if [[ -n "$MACOS_SIGN_KEYCHAIN" ]]; then
    dmg_sign_args+=(--keychain "$MACOS_SIGN_KEYCHAIN")
  fi
  codesign "${dmg_sign_args[@]}" "$DMG_PATH"
  codesign --verify --verbose=2 "$DMG_PATH"
fi

if [[ "$MACOS_NOTARIZE" == "1" ]]; then
  notarize_artifact "$DMG_PATH"
  xcrun stapler staple "$DMG_PATH"
fi

echo "Creating updater payload $UPDATE_PAYLOAD..."
# Used by the in-app updater: a tarball containing the .app bundle
# at its root so the install helper can extract and atomically
# replace the running bundle. Uses gnu-tar/bsdtar's --format=ustar
# so xattrs/quarantine are stripped and the archive is reproducible.
( cd "$PACKAGE_DIR" && tar -czf "$UPDATE_PAYLOAD" "$APP_NAME.app" )

if [[ -n "$RELEASE_ID" ]]; then
  if [[ "$TARGET_TRIPLE" == aarch64-apple-darwin ]]; then
    ARCH_LABEL="aarch64"
  elif [[ "$TARGET_TRIPLE" == x86_64-apple-darwin ]]; then
    ARCH_LABEL="x86_64"
  else
    ARCH_LABEL="$(uname -m)"
    case "$ARCH_LABEL" in
      arm64) ARCH_LABEL="aarch64" ;;
      x86_64) ARCH_LABEL="x86_64" ;;
    esac
  fi
  PREFIX="${ARTIFACT_PREFIX:-AnotherOne}"
  RELEASE_DMG="$PACKAGE_DIR/${PREFIX}-macos-${ARCH_LABEL}.dmg"
  RELEASE_PAYLOAD="$PACKAGE_DIR/${PREFIX}-macos-${ARCH_LABEL}.app.tar.gz"
  cp -f "$DMG_PATH" "$RELEASE_DMG"
  cp -f "$UPDATE_PAYLOAD" "$RELEASE_PAYLOAD"
  echo "Release-named copies:"
  echo "  $RELEASE_DMG"
  echo "  $RELEASE_PAYLOAD"
fi

echo "Created:"
echo "  $APP_BUNDLE"
echo "  $DMG_PATH"
echo "  $UPDATE_PAYLOAD"

if [[ "$OPEN_DMG" -eq 1 ]]; then
  echo "Opening $APP_NAME.dmg..."
  open "$DMG_PATH"
fi
