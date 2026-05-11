#!/usr/bin/env bash
# Automated mobile pair + smoke-test driver.
#
# Inputs:
#   - A running desktop daemon (on the same machine) that has
#     exposed a pairing URL via the QR overlay.
#   - An Android device with `dev.anotherone.app` installed and
#     reachable over `adb`.
#
# What it does:
#   1. Reads the desktop's active pair URL from the well-known path.
#   2. Writes that URL into the phone's internal-data pair-trigger
#      file via `adb shell run-as`. The app's render tick picks
#      it up in `drain_qr_scan_results`, issues the dial, and the
#      daemon consumes the nonce + auto-rotates per the #TODO
#      fix bundle.
#   3. Polls the desktop leakscope log for `TOFU pair complete`
#      with the phone's fresh viewer_id, then polls phone logcat
#      for `installing freshly-paired session via replace_session`
#      and the subsequent `serve_session: pushing initial ProjectList`
#      absorption.
#
# Exits 0 on successful pair + projection-absorb round-trip,
# non-zero otherwise. Designed to run in CI or as a loop from a
# dev iterating on the mobile code.
#
# Reads:
#   - $HOME/.cache/another-one/pair-url.txt  (desktop writes this
#     on every `rotate_pair_state` call; canonical source of
#     truth for the current scannable URL)
#   - /tmp/aone-leakscope.log                 (desktop stderr)
#
# Writes:
#   - /data/data/dev.anotherone.app/files/pair-trigger  (via run-as)
#
# Environment:
#   ADB=<path>                            path to adb binary
#   AONE_PAIR_URL_FILE=<path>             override the default URL file
#   AONE_DESKTOP_LOG=<path>               override the default leakscope log path
#   AONE_PAIR_TIMEOUT_SEC=<seconds>       default 30

set -euo pipefail

ADB=${ADB:-adb}
PAIR_URL_FILE=${AONE_PAIR_URL_FILE:-"$HOME/.cache/another-one/pair-url.txt"}
DESKTOP_LOG=${AONE_DESKTOP_LOG:-/tmp/aone-leakscope.log}
TIMEOUT=${AONE_PAIR_TIMEOUT_SEC:-30}

die() {
  echo "[test-mobile-pair] $*" >&2
  exit 1
}

need() {
  command -v "$1" >/dev/null 2>&1 || die "missing tool: $1"
}

need "$ADB"

# 1. Read the current pair URL. This file is written by the daemon
#    every time `rotate_pair_state` runs (see daemon/src/transport_iroh.rs).
[[ -s "$PAIR_URL_FILE" ]] || die "pair URL file missing or empty: $PAIR_URL_FILE"
URL=$(cat "$PAIR_URL_FILE")
echo "[test-mobile-pair] pair URL: ${URL:0:60}..."

# 2. Write into the phone's pair-trigger path. `run-as` sandboxes
#    into the package-private dir. Works on debug builds without
#    root. Wrap the whole sub-command in a single quoted string for
#    `adb shell` — passing the args split doesn't preserve cwd
#    semantics inside the nested shell on every Android build we've
#    tested.
printf '%s' "$URL" \
  | $ADB shell 'run-as dev.anotherone.app sh -c "cat > files/pair-trigger"' \
  || die "failed to write pair-trigger on device"
echo "[test-mobile-pair] wrote pair-trigger to device"

# 3. Wait for the round-trip. The phone's dial lands one of two
#    ways: (a) first-pair — daemon runs `consume_hello` and logs
#    `TOFU pair complete`; (b) reconnect as already-paired — the
#    viewer's stable secret key (persisted on device since
#    #TODO) matches an allowlist entry, and the daemon skips
#    Hello, going straight to `serve_session: pushing initial
#    ProjectList viewer_id=<hex>` (not the gui:desktop one). Either
#    is success; failure is no progress within the timeout.
count_matches() {
  local pat=$1 path=$2 n
  if [[ -f "$path" ]]; then
    n=$(grep -cE "$pat" "$path" || true)
  else
    n=0
  fi
  echo "${n:-0}"
}

# Matches either the first-pair log line or an already-paired
# reconnect from a non-desktop viewer_id. Pattern is lenient about
# ANSI color codes the tracing-subscriber prints on an isatty
# stderr — each field in the `key=value` structured part is
# wrapped in escape sequences that don't show when cat-ing the
# file through `grep`.
SUCCESS_PATTERN='TOFU pair complete|pushing initial ProjectList.*"[0-9a-f]{64}"'

start=$(date +%s)
initial=$(count_matches "$SUCCESS_PATTERN" "$DESKTOP_LOG")
echo "[test-mobile-pair] waiting up to ${TIMEOUT}s for pair/reconnect (baseline=$initial)…"
while :; do
  current=$(count_matches "$SUCCESS_PATTERN" "$DESKTOP_LOG")
  if (( current > initial )); then
    echo "[test-mobile-pair] pair/reconnect observed"
    break
  fi
  now=$(date +%s)
  if (( now - start > TIMEOUT )); then
    echo "---desktop log tail---"
    tail -30 "$DESKTOP_LOG"
    die "timed out waiting for pair/reconnect"
  fi
  sleep 1
done

# 4. Follow up by grepping phone logcat for the replace_session
#    log. This confirms the client side made it past the Hello
#    and installed the IrohSession.
echo "[test-mobile-pair] waiting up to 5s for phone replace_session…"
for _ in $(seq 1 10); do
  if $ADB logcat -d 2>/dev/null | grep -q 'installing freshly-paired session via replace_session'; then
    echo "[test-mobile-pair] phone replace_session observed"
    echo "[test-mobile-pair] SUCCESS"
    exit 0
  fi
  sleep 0.5
done

die "timed out waiting for phone replace_session (desktop paired OK but client-side didn't)"
