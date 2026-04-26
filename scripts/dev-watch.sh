#!/bin/sh
# Run the AnotherOne flutter desktop with hot reload.
#
# Flutter's `run` already watches `lib/` and triggers hot reloads
# on each save (and a Rust rebuild + hot restart when the bridge
# changes), so this script is now a thin wrapper. Kept around so
# muscle-memory `./scripts/dev-watch.sh` keeps working.

set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
APP_DIR="$ROOT_DIR/another-one"

cd "$APP_DIR"
exec flutter run -d linux "$@"
