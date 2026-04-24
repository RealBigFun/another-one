#!/usr/bin/env bash
# Repro harness for the AnotherOne memory-leak investigation.
#
# Builds the desktop binary with `--features leakscope` (so PTY reader
# threads and the GPUI drain both bump shared counters), launches it,
# and captures:
#   - the app's own 1 Hz LEAKSCOPE stderr samples (rss, in_flight, ...)
#   - an external ps-based RSS sample in case the in-app sampler stalls
# into timestamped logs under ./leakscope-runs/<ts>/.
#
# You supply the workload: open N terminal tabs in the running app and
# run something chatty (e.g. `yes | head -c 4G | hexdump -C`, or a real
# Claude Code session). The script does NOT automate tab creation —
# there's no CLI hook for that, and the plan's target repro is a human
# operating the UI.
#
# Exit with Ctrl-C when done; the script prints a summary and exits.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

TS="$(date +%Y%m%d-%H%M%S)"
OUT_DIR="$REPO_ROOT/leakscope-runs/$TS"
mkdir -p "$OUT_DIR"

APP_LOG="$OUT_DIR/app.log"
RSS_LOG="$OUT_DIR/rss.csv"
META="$OUT_DIR/meta.txt"

{
  echo "leakscope repro run"
  echo "started:       $(date -Iseconds)"
  echo "repo HEAD:     $(git rev-parse HEAD)"
  echo "branch:        $(git rev-parse --abbrev-ref HEAD)"
  echo "dirty files:   $(git status --porcelain | wc -l)"
  uname -a
} > "$META"

echo "==> building with --features leakscope (release profile)"
cargo build --release -p another-one --features leakscope

BIN="$REPO_ROOT/target/release/another-one"
if [ ! -x "$BIN" ]; then
  echo "expected binary at $BIN but it's not executable" >&2
  exit 1
fi

echo "==> launching $BIN"
echo "==> app stderr → $APP_LOG"
echo "==> ps rss samples → $RSS_LOG (t_sec,rss_kb,vsz_kb)"
echo ""
echo "open tabs in the app and start your chatty workload. Ctrl-C here to stop."
echo ""

# stdbuf -oL disables stdout buffering so LEAKSCOPE lines flush at
# 1 Hz rather than piling up in a 4 KB pipe buffer.
stdbuf -oL -eL "$BIN" >"$APP_LOG" 2>&1 &
APP_PID=$!

echo "app pid: $APP_PID" | tee -a "$META"

cleanup() {
  echo ""
  echo "==> stopping app pid $APP_PID"
  kill -TERM "$APP_PID" 2>/dev/null || true
  # Give it a moment, then SIGKILL if still running — important because
  # an OOM'd-but-not-dead instance can leave FUSE mounts lingering.
  for _ in 1 2 3 4 5; do
    sleep 1
    if ! kill -0 "$APP_PID" 2>/dev/null; then
      break
    fi
  done
  kill -KILL "$APP_PID" 2>/dev/null || true
  summarize
}
trap cleanup EXIT INT TERM

summarize() {
  echo ""
  echo "==> summary ($OUT_DIR)"
  if [ -s "$RSS_LOG" ]; then
    local first last peak
    first="$(head -1 "$RSS_LOG" | cut -d, -f2)"
    last="$(tail -1 "$RSS_LOG" | cut -d, -f2)"
    peak="$(cut -d, -f2 "$RSS_LOG" | sort -n | tail -1)"
    echo "    rss start:  ${first} kB"
    echo "    rss end:    ${last} kB"
    echo "    rss peak:   ${peak} kB"
  fi
  echo "    last 5 LEAKSCOPE samples:"
  grep LEAKSCOPE "$APP_LOG" | tail -5 | sed 's/^/      /'
  echo ""
  echo "    full logs in: $OUT_DIR"
}

START_T="$(date +%s)"
echo "t_sec,rss_kb,vsz_kb" > "$RSS_LOG"
while kill -0 "$APP_PID" 2>/dev/null; do
  NOW="$(date +%s)"
  T=$((NOW - START_T))
  # ps prints leading whitespace; strip it so the CSV stays clean.
  STATS="$(ps -o rss=,vsz= -p "$APP_PID" 2>/dev/null | awk '{printf "%s,%s", $1, $2}')"
  if [ -n "$STATS" ]; then
    echo "${T},${STATS}" >> "$RSS_LOG"
  fi
  sleep 1
done
