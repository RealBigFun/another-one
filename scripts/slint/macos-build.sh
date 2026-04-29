#!/usr/bin/env bash

set -euo pipefail

# shellcheck source=scripts/slint/common.sh
SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

RUN_AFTER=0
RELEASE=0
TARGET_TRIPLE="${TARGET_TRIPLE:-}"

usage() {
  cat <<EOF
Usage: $0 [--release] [--target <triple>] [--run]

Build the Slint POC with the macOS desktop profile.

Options:
  --release           Build with cargo --release.
  --target <triple>   Forward a macOS cargo target triple, for example
                      aarch64-apple-darwin or x86_64-apple-darwin.
  --run               Launch the built app. Only valid for the host target.
  -h, --help          Show this help message.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      RELEASE=1
      shift
      ;;
    --target)
      [[ $# -ge 2 ]] || fail "--target requires a value."
      TARGET_TRIPLE="$2"
      shift 2
      ;;
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

need_host Darwin "Slint macOS build"
need_cmd cargo "Install Rust/Cargo with rustup."
need_cmd xcrun "Install Xcode Command Line Tools with: xcode-select --install"

build_args=(build -p "$SLINT_PACKAGE")
profile_dir="debug"
if [[ "$RELEASE" -eq 1 ]]; then
  build_args+=(--release)
  profile_dir="release"
fi
if [[ -n "$TARGET_TRIPLE" ]]; then
  need_rust_target "$TARGET_TRIPLE"
  build_args+=(--target "$TARGET_TRIPLE")
  profile_dir="$TARGET_TRIPLE/$profile_dir"
fi

info "building $SLINT_PACKAGE for macOS"
( cd "$ROOT_DIR" && cargo "${build_args[@]}" )

BINARY_PATH="$ROOT_DIR/target/$profile_dir/$SLINT_PACKAGE"
[[ -x "$BINARY_PATH" ]] || fail "expected executable at $BINARY_PATH after build."

info "built $BINARY_PATH"

if [[ "$RUN_AFTER" -eq 1 ]]; then
  if [[ -n "$TARGET_TRIPLE" ]]; then
    fail "--run is only supported for the native host target; omit --target."
  fi
  cd "$ROOT_DIR"
  exec "$BINARY_PATH"
fi
