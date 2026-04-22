#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(
  cd -- "$(dirname "${BASH_SOURCE[0]}")/.." && pwd
)"

PACKAGE_NAME="$(
  sed -n 's/^name = "\(.*\)"/\1/p' "$ROOT_DIR/desktop/Cargo.toml" | head -n 1
)"

if [[ -z "$PACKAGE_NAME" ]]; then
  echo "Could not determine the package name from desktop/Cargo.toml." >&2
  exit 1
fi

BINARY_PATH="$ROOT_DIR/target/release/$PACKAGE_NAME"

echo "Building $PACKAGE_NAME for production..."
(
  cd "$ROOT_DIR"
  cargo build --release
)

if [[ ! -x "$BINARY_PATH" ]]; then
  echo "Expected built binary at $BINARY_PATH, but it was not found or is not executable." >&2
  exit 1
fi

case "$(uname -s)" in
  Darwin)
    ;;
  Linux)
    if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
      echo "No graphical session detected. Set DISPLAY or WAYLAND_DISPLAY before launching the app." >&2
      exit 1
    fi
    ;;
  *)
    echo "Unsupported operating system: $(uname -s)" >&2
    exit 1
    ;;
esac

echo "Launching $PACKAGE_NAME..."
cd "$ROOT_DIR"
exec "$BINARY_PATH" "$@"
