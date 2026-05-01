#!/bin/sh

set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)

package_name=$(
  sed -n 's/^name = "\(.*\)"/\1/p' "$ROOT_DIR/app/Cargo.toml" | head -n 1
)

if [ -z "$package_name" ]; then
  echo "Could not determine package name from app/Cargo.toml." >&2
  exit 1
fi

BINARY_PATH="$ROOT_DIR/target/debug/$package_name"
STATE_DIR=${AO_DEV_WATCH_STATE_DIR:-"$ROOT_DIR/target/dev-watch"}
PID_FILE="$STATE_DIR/app.pid"
FIFO_PATH="$STATE_DIR/reload.fifo"
BACKEND=${AO_DEV_WATCH_BACKEND:-auto}
DEBOUNCE_SECONDS=${AO_DEV_WATCH_DEBOUNCE_SECONDS:-1}
POLL_INTERVAL_SECONDS=${AO_DEV_WATCH_POLL_INTERVAL_SECONDS:-3}
APP_PID=""
WATCHER_PID=""

# Keep this list explicit so non-cargo watcher backends do not recurse into
# target/, vendor/, .git/, or other high-churn directories. The cargo-watch
# backend uses Cargo's dependency graph instead, so it also catches local path
# dependencies if new workspace crates are added later.
WATCH_RELS="
Cargo.toml
Cargo.lock
app/Cargo.toml
app/src
core/Cargo.toml
core/src
daemon/Cargo.toml
daemon/src
daemon-client/Cargo.toml
daemon-client/src
mcp-shim/Cargo.toml
mcp-shim/src
"

usage() {
  cat <<EOF
Usage: scripts/dev-watch.sh

Builds the debug app, starts it, then rebuilds/restarts after source changes.

Watcher selection is event-based when possible:
  1. cargo-watch (macOS/Linux, recommended)
  2. fswatch     (macOS/Linux)
  3. inotifywait (Linux)
  4. portable polling fallback

Environment:
  AO_DEV_WATCH_BACKEND=auto|cargo-watch|fswatch|inotifywait|poll
  AO_DEV_WATCH_DEBOUNCE_SECONDS=1
  AO_DEV_WATCH_POLL_INTERVAL_SECONDS=3
EOF
}

watch_paths() {
  for rel in $WATCH_RELS; do
    path="$ROOT_DIR/$rel"
    if [ -e "$path" ]; then
      printf '%s\n' "$path"
    fi
  done
}

watch_path_args() {
  # Echoes shell words for paths in this repository. Repository paths should not
  # contain whitespace; keeping this helper simple lets the script stay POSIX sh.
  watch_paths | tr '\n' ' '
}

have_cargo_watch() {
  command -v cargo-watch >/dev/null 2>&1 || cargo watch --version >/dev/null 2>&1
}

choose_backend() {
  case "$BACKEND" in
    auto)
      if have_cargo_watch; then
        printf '%s\n' cargo-watch
      elif command -v fswatch >/dev/null 2>&1; then
        printf '%s\n' fswatch
      elif command -v inotifywait >/dev/null 2>&1; then
        printf '%s\n' inotifywait
      else
        printf '%s\n' poll
      fi
      ;;
    cargo-watch|fswatch|inotifywait|poll)
      printf '%s\n' "$BACKEND"
      ;;
    *)
      echo "Unknown AO_DEV_WATCH_BACKEND=$BACKEND" >&2
      usage >&2
      exit 2
      ;;
  esac
}

stop_app() {
  pid=""

  if [ -n "$APP_PID" ]; then
    pid="$APP_PID"
  elif [ -f "$PID_FILE" ]; then
    pid=$(sed -n '1p' "$PID_FILE" 2>/dev/null || true)
  fi

  case "$pid" in
    ''|*[!0-9]*)
      rm -f "$PID_FILE"
      APP_PID=""
      return 0
      ;;
  esac

  if kill -0 "$pid" 2>/dev/null; then
    echo "Stopping $package_name (pid $pid)"
    kill "$pid" 2>/dev/null || true
    wait "$pid" 2>/dev/null || true
  fi

  rm -f "$PID_FILE"
  APP_PID=""
}

start_app() {
  echo "Starting $package_name"
  (
    cd "$ROOT_DIR"
    exec "$BINARY_PATH"
  ) &
  APP_PID=$!
  printf '%s\n' "$APP_PID" > "$PID_FILE"
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
    return 1
  fi
}

cleanup() {
  trap - INT TERM EXIT

  if [ -n "$WATCHER_PID" ] && kill -0 "$WATCHER_PID" 2>/dev/null; then
    kill "$WATCHER_PID" 2>/dev/null || true
    wait "$WATCHER_PID" 2>/dev/null || true
  fi

  stop_app
  rm -f "$FIFO_PATH"
}

print_common_header() {
  echo "Watching AnotherOne sources. Backend: $1. Debounce: ${DEBOUNCE_SECONDS}s."
  echo "Override with AO_DEV_WATCH_BACKEND=auto|cargo-watch|fswatch|inotifywait|poll."
}

run_cargo_watch() {
  if ! have_cargo_watch; then
    echo "cargo-watch is not installed. Try: cargo install cargo-watch" >&2
    exit 1
  fi

  mkdir -p "$STATE_DIR"
  rm -f "$FIFO_PATH"
  mkfifo "$FIFO_PATH"

  print_common_header cargo-watch
  echo "cargo-watch uses OS file events and Cargo's workspace/dependency graph."

  # cargo-watch handles the initial trigger and debounce. The command only
  # notifies this long-lived supervisor; build_and_reload runs here so a failed
  # build does not kill the currently running app.
  (
    cd "$ROOT_DIR"
    cargo watch --why --delay "$DEBOUNCE_SECONDS" -s "printf '%s\n' reload > '$FIFO_PATH'"
  ) &
  WATCHER_PID=$!

  while :; do
    if IFS= read -r _ < "$FIFO_PATH"; then
      build_and_reload || true
    fi
  done
}

run_fswatch() {
  if ! command -v fswatch >/dev/null 2>&1; then
    echo "fswatch is not installed." >&2
    exit 1
  fi

  print_common_header fswatch
  build_and_reload || true

  while :; do
    # shellcheck disable=SC2046 # Paths are project-local and whitespace-free.
    fswatch -1 -r $(watch_path_args) >/dev/null
    sleep "$DEBOUNCE_SECONDS"
    build_and_reload || true
  done
}

run_inotifywait() {
  if ! command -v inotifywait >/dev/null 2>&1; then
    echo "inotifywait is not installed. On many distros it is in inotify-tools." >&2
    exit 1
  fi

  print_common_header inotifywait
  build_and_reload || true

  while :; do
    # shellcheck disable=SC2046 # Paths are project-local and whitespace-free.
    inotifywait -qq -r -e close_write,create,delete,move $(watch_path_args) >/dev/null
    sleep "$DEBOUNCE_SECONDS"
    build_and_reload || true
  done
}

stat_snapshot() {
  # Polling fallback. Use batched stat calls instead of spawning one stat per
  # file, which keeps the fallback much cheaper than the original loop.
  if stat -f '%m %N' "$ROOT_DIR/app/Cargo.toml" >/dev/null 2>&1; then
    # shellcheck disable=SC2046 # Paths are project-local and whitespace-free.
    find $(watch_path_args) -type f -exec stat -f '%m %N' {} + | LC_ALL=C sort
  else
    # shellcheck disable=SC2046 # Paths are project-local and whitespace-free.
    find $(watch_path_args) -type f -exec stat -c '%Y %n' {} + | LC_ALL=C sort
  fi
}

run_poll() {
  print_common_header poll
  echo "No supported event watcher found; using portable polling every ${POLL_INTERVAL_SECONDS}s." >&2
  echo "For event-based watching, install cargo-watch: cargo install cargo-watch" >&2

  last_snapshot=$(stat_snapshot)
  build_and_reload || true

  while :; do
    sleep "$POLL_INTERVAL_SECONDS"
    current_snapshot=$(stat_snapshot)

    if [ "$current_snapshot" = "$last_snapshot" ]; then
      continue
    fi

    pending_snapshot=$current_snapshot
    while :; do
      sleep "$DEBOUNCE_SECONDS"
      current_snapshot=$(stat_snapshot)
      if [ "$current_snapshot" = "$pending_snapshot" ]; then
        break
      fi
      pending_snapshot=$current_snapshot
    done

    last_snapshot=$pending_snapshot
    echo "Source changes settled. Reloading."
    build_and_reload || true
  done
}

case "${1:-}" in
  -h|--help)
    usage
    exit 0
    ;;
  '')
    ;;
  *)
    echo "Unknown argument: $1" >&2
    usage >&2
    exit 2
    ;;
esac

mkdir -p "$STATE_DIR"
trap cleanup INT TERM EXIT

backend=$(choose_backend)
case "$backend" in
  cargo-watch) run_cargo_watch ;;
  fswatch) run_fswatch ;;
  inotifywait) run_inotifywait ;;
  poll) run_poll ;;
esac
