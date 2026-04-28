#!/bin/sh

set -eu

ROOT_DIR=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
APP_TARGET=${1:-slint}
HYPR_WORKSPACE=${HYPR_WORKSPACE:-1}
APP_PID=""
HYPR_RULE_SET=0

case "$APP_TARGET" in
  slint | slint-poc)
    PACKAGE_NAME="slint-poc"
    WATCH_PATHS="$ROOT_DIR/slint-poc/src $ROOT_DIR/slint-poc/ui $ROOT_DIR/slint-poc/build.rs"
    BINARY_PATH="$ROOT_DIR/target/debug/slint-poc"
    HYPR_TITLE_REGEX="AnotherOne Slint POC"
    HYPR_CLASS_REGEX="com[.]anotherone[.]SlintPoc"
    ;;
  desktop | gpui)
    PACKAGE_NAME=$(
      sed -n 's/^name = "\(.*\)"/\1/p' "$ROOT_DIR/desktop/Cargo.toml" | head -n 1
    )
    WATCH_PATHS="$ROOT_DIR/desktop/src"
    BINARY_PATH="$ROOT_DIR/target/debug/$PACKAGE_NAME"
    HYPR_TITLE_REGEX=""
    HYPR_CLASS_REGEX=""
    ;;
  *)
    echo "Usage: $0 [slint|desktop]" >&2
    exit 2
    ;;
esac

if [ -z "$PACKAGE_NAME" ]; then
  echo "Could not determine package name for $APP_TARGET." >&2
  exit 1
fi

stat_mtime() {
  if stat -f '%m' "$1" >/dev/null 2>&1; then
    stat -f '%m %N' "$1"
  else
    stat -c '%Y %n' "$1"
  fi
}

snapshot_tree() {
  for watch_path in $WATCH_PATHS; do
    if [ -d "$watch_path" ]; then
      find "$watch_path" -type f
    elif [ -f "$watch_path" ]; then
      printf '%s\n' "$watch_path"
    fi
  done | LC_ALL=C sort | while IFS= read -r path; do
    stat_mtime "$path"
  done
}

hyprctl_available() {
  [ -n "${HYPRLAND_INSTANCE_SIGNATURE:-}" ] && command -v hyprctl >/dev/null 2>&1
}

setup_hyprland_rule() {
  if ! hyprctl_available || [ -z "$HYPR_TITLE_REGEX$HYPR_CLASS_REGEX" ]; then
    return 0
  fi

  if [ "$HYPR_RULE_SET" -eq 1 ]; then
    return 0
  fi

  if [ -n "$HYPR_TITLE_REGEX" ]; then
    hyprctl keyword windowrulev2 "workspace $HYPR_WORKSPACE silent,title:^($HYPR_TITLE_REGEX)$" >/dev/null 2>&1 || true
  fi

  if [ -n "$HYPR_CLASS_REGEX" ]; then
    hyprctl keyword windowrulev2 "workspace $HYPR_WORKSPACE silent,class:^($HYPR_CLASS_REGEX)$" >/dev/null 2>&1 || true
  fi

  HYPR_RULE_SET=1
}

hyprland_client_visible() {
  clients=$(hyprctl clients -j 2>/dev/null || true)

  if [ -n "$HYPR_TITLE_REGEX" ] && printf '%s' "$clients" | grep -F 'AnotherOne Slint POC' >/dev/null 2>&1; then
    return 0
  fi

  if [ -n "$HYPR_CLASS_REGEX" ] && printf '%s' "$clients" | grep -F 'com.anotherone.SlintPoc' >/dev/null 2>&1; then
    return 0
  fi

  return 1
}

move_hyprland_window_to_workspace() {
  if ! hyprctl_available || [ -z "$HYPR_TITLE_REGEX$HYPR_CLASS_REGEX" ]; then
    return 0
  fi

  attempts=0
  while [ "$attempts" -lt 20 ]; do
    if hyprland_client_visible; then
      if [ -n "$HYPR_TITLE_REGEX" ]; then
        hyprctl dispatch movetoworkspacesilent "$HYPR_WORKSPACE,title:^($HYPR_TITLE_REGEX)$" >/dev/null 2>&1 || true
      fi

      if [ -n "$HYPR_CLASS_REGEX" ]; then
        hyprctl dispatch movetoworkspacesilent "$HYPR_WORKSPACE,class:^($HYPR_CLASS_REGEX)$" >/dev/null 2>&1 || true
      fi
      return 0
    fi

    attempts=$((attempts + 1))
    sleep 0.25
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
  setup_hyprland_rule
  echo "Starting $PACKAGE_NAME"
  (
    cd "$ROOT_DIR"
    exec "$BINARY_PATH"
  ) &
  APP_PID=$!
  move_hyprland_window_to_workspace
}

build_and_reload() {
  echo "Building $PACKAGE_NAME"
  if (
    cd "$ROOT_DIR"
    cargo build -p "$PACKAGE_NAME"
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

shutdown() {
  cleanup
  exit 0
}

trap cleanup EXIT
trap shutdown INT TERM

echo "Watching $WATCH_PATHS with a 1s debounce."

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
