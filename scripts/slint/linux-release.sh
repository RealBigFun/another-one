#!/usr/bin/env bash

set -euo pipefail

# shellcheck source=scripts/slint/common.sh
SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

RUN_AFTER=0

usage() {
  cat <<EOF
Usage: $0 [--run]

Build the Slint POC Linux release binary.

Options:
  --run       Launch target/release/slint-poc after a successful build.
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

need_host Linux "Slint Linux release build"
need_cmd cargo "Install Rust/Cargo with rustup."

info "building $SLINT_PACKAGE release binary"
( cd "$ROOT_DIR" && cargo build -p "$SLINT_PACKAGE" --release )

BINARY_PATH="$ROOT_DIR/target/release/$SLINT_PACKAGE"
[[ -x "$BINARY_PATH" ]] || fail "expected executable at $BINARY_PATH after release build."

info "built $BINARY_PATH"

if [[ "$RUN_AFTER" -eq 1 ]]; then
  need_graphical_linux_session
  info "launching $BINARY_PATH"
  cd "$ROOT_DIR"
  exec "$BINARY_PATH"
fi
