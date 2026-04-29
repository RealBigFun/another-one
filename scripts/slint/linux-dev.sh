#!/usr/bin/env bash

set -euo pipefail

# shellcheck source=scripts/slint/common.sh
SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

RUN_AFTER=0

usage() {
  cat <<EOF
Usage: $0 [--run]

Build the Slint POC with the Linux desktop profile.

Options:
  --run       Launch target/debug/slint-poc after a successful build.
  -h, --help Show this help message.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --run)
      RUN_AFTER=1
      shift
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

need_host Linux "Slint Linux dev build"
need_cmd cargo "Install Rust/Cargo with rustup."

info "checking $SLINT_PACKAGE"
( cd "$ROOT_DIR" && cargo check -p "$SLINT_PACKAGE" )

info "building $SLINT_PACKAGE debug binary"
( cd "$ROOT_DIR" && cargo build -p "$SLINT_PACKAGE" )

BINARY_PATH="$ROOT_DIR/target/debug/$SLINT_PACKAGE"
[[ -x "$BINARY_PATH" ]] || fail "expected executable at $BINARY_PATH after build."

if [[ "$RUN_AFTER" -eq 1 ]]; then
  need_graphical_linux_session
  info "launching $BINARY_PATH"
  cd "$ROOT_DIR"
  exec "$BINARY_PATH"
fi
