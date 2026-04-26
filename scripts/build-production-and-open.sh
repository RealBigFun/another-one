#!/usr/bin/env bash
# Build a release-mode flutter desktop and exec into it.
#
# Skips the AppImage / DMG packaging step `package-{linux,macos}.sh`
# does — useful for local "is this faster than the dev build?"
# spot-checks without paying the linuxdeploy / hdiutil cost.

set -euo pipefail

ROOT_DIR="$(
  cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd
)"
APP_DIR="$ROOT_DIR/another-one"

echo "==> building flutter desktop (release)"
(
  cd "$APP_DIR"
  flutter build linux --release
)

case "$(uname -s)" in
  Darwin)
    BINARY_PATH="$APP_DIR/build/macos/Build/Products/Release/AnotherOne.app/Contents/MacOS/another-one"
    ;;
  Linux)
    if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
      echo "No graphical session detected. Set DISPLAY or WAYLAND_DISPLAY before launching the app." >&2
      exit 1
    fi
    BINARY_PATH="$APP_DIR/build/linux/x64/release/bundle/another-one"
    ;;
  *)
    echo "Unsupported operating system: $(uname -s)" >&2
    exit 1
    ;;
esac

if [[ ! -x "$BINARY_PATH" ]]; then
  echo "Expected built binary at $BINARY_PATH, but it was not found or is not executable." >&2
  exit 1
fi

echo "==> launching $BINARY_PATH"
cd "$ROOT_DIR"
exec "$BINARY_PATH" "$@"
