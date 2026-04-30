#!/bin/sh

set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
WATCH_DIR="$ROOT_DIR/app/src"

package_name=$(
  sed -n 's/^name = "\(.*\)"/\1/p' "$ROOT_DIR/app/Cargo.toml" | head -n 1
)

if [ -z "$package_name" ]; then
  echo "Could not determine package name from app/Cargo.toml." >&2
  exit 1
fi

BINARY_PATH="$ROOT_DIR/target/debug/$package_name"
APP_PID=""

stat_mtime() {
  if stat -f '%m' "$1" >/dev/null 2>&1; then
    stat -f '%m %N' "$1"
  else
    stat -c '%Y %n' "$1"
  fi
}

snapshot_tree() {
  if [ ! -d "$WATCH_DIR" ]; then
    return 0
  fi

  find "$WATCH_DIR" -type f | LC_ALL=C sort | while IFS= read -r path; do
    stat_mtime "$path"
  done
}

stop_app() {
  if [ -n "$APP_PID" ] && kill -0 "$APP_PID" 2>/dev/null; then
    kill "$APP_PID" 2>/dev/null || true
    wait "$APP_PID" 2>/dev/null || true
  fi

  APP_PID=""
}

start_app() {
  echo "Starting $package_name"
  (
    cd "$ROOT_DIR"
    exec "$BINARY_PATH"
  ) &
  APP_PID=$!
}

build_and_reload() {
  echo "Building $package_name"
  if (
    cd "$ROOT_DIR"
    cargo build
  ); then
    stop_app
    start_app
  else
    echo "Build failed. Keeping current app process." >&2
  fi
}

cleanup() {
  stop_app
}

trap cleanup INT TERM EXIT

echo "Watching $WATCH_DIR with a 1s debounce."

last_snapshot=$(snapshot_tree)
build_and_reload

while :; do
  sleep 1
  current_snapshot=$(snapshot_tree)

  if [ "$current_snapshot" = "$last_snapshot" ]; then
    continue
  fi

  pending_snapshot=$current_snapshot
  while :; do
    sleep 1
    current_snapshot=$(snapshot_tree)
    if [ "$current_snapshot" = "$pending_snapshot" ]; then
      break
    fi
    pending_snapshot=$current_snapshot
  done

  last_snapshot=$pending_snapshot
  echo "Source changes settled. Reloading."
  build_and_reload
done
