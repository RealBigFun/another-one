#!/bin/sh
# Run the AnotherOne Flutter desktop with hot reload.
#
# Flutter's `run` already watches `lib/` and triggers hot reloads
# on each save (and a Rust rebuild + hot restart when the bridge
# changes), so this script is now a thin wrapper. Kept around so
# muscle-memory `./scripts/dev-watch.sh` keeps working.
#
# Defaults to the host desktop device:
#   - macOS  -> macos
#   - Linux  -> linux
#
# Override with ANOTHER_ONE_FLUTTER_DEVICE=... or pass Flutter's own
# -d/--device-id argument.

set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
APP_DIR="$ROOT_DIR/another-one"

has_device_arg=0
for arg in "$@"; do
  case "$arg" in
    -d|--device-id|--device-id=*|--device=*)
      has_device_arg=1
      ;;
  esac
done

if [ "$has_device_arg" -eq 0 ]; then
  DEVICE=${ANOTHER_ONE_FLUTTER_DEVICE:-}
  if [ -z "$DEVICE" ]; then
    case "$(uname -s)" in
      Darwin)
        DEVICE=macos
        ;;
      Linux)
        if [ -z "${DISPLAY:-}" ] && [ -z "${WAYLAND_DISPLAY:-}" ]; then
          echo "No graphical session detected. Set DISPLAY or WAYLAND_DISPLAY before launching the app." >&2
          exit 1
        fi
        DEVICE=linux
        ;;
      *)
        echo "Unsupported operating system: $(uname -s)" >&2
        exit 1
        ;;
    esac
  fi
fi

cd "$APP_DIR"
if [ "$has_device_arg" -eq 1 ]; then
  exec flutter run "$@"
fi
exec flutter run -d "$DEVICE" "$@"
