#!/usr/bin/env bash
# Capture a screenshot of the AnotherOne window and downscale it
# so it fits under the agent's image-read limit.
#
# Defaults to /tmp/aone-shot.png (large) +
# /tmp/aone-shot-1600.png (downscaled). Pass --window to crop
# to just the window — requires slurp + a click. Otherwise the
# whole screen is captured.
#
# Wayland-only today: `grim` is the capture tool, `magick` (or
# the legacy `convert`) does the resize. macOS has its own
# screencapture(1); it'd take a few lines to wire up but isn't
# in scope for the dev-host-on-Linux flow.

set -euo pipefail

WINDOW=0
OUT_FULL="/tmp/aone-shot.png"
OUT_SMALL="/tmp/aone-shot-1600.png"
RESIZE_W=1600

usage() {
  cat <<EOF
Usage: $0 [--window]

  --window      Use slurp to pick the window region. Without
                this, captures the full screen.
  -h, --help    Show this help.

Outputs:
  $OUT_FULL          full-resolution PNG
  $OUT_SMALL          downscaled to ${RESIZE_W}px wide
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --window)
      WINDOW=1
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

if ! command -v grim >/dev/null; then
  echo "grim is required (Wayland screenshot tool)." >&2
  exit 1
fi

if [[ "$WINDOW" -eq 1 ]]; then
  if ! command -v slurp >/dev/null; then
    echo "--window needs slurp installed." >&2
    exit 1
  fi
  echo "Click + drag the window region…"
  region="$(slurp)"
  grim -g "$region" "$OUT_FULL"
else
  grim "$OUT_FULL"
fi

if command -v magick >/dev/null; then
  magick "$OUT_FULL" -resize "${RESIZE_W}x" "$OUT_SMALL"
elif command -v convert >/dev/null; then
  convert "$OUT_FULL" -resize "${RESIZE_W}x" "$OUT_SMALL"
else
  echo "magick / convert not installed; only full-res output was saved." >&2
  exit 0
fi

echo "wrote:"
echo "  $OUT_FULL"
echo "  $OUT_SMALL"
