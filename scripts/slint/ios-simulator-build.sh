#!/usr/bin/env bash

set -euo pipefail

# shellcheck source=scripts/slint/common.sh
SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

RELEASE=0

usage() {
  cat <<EOF
Usage: $0 [--release]

Build the Slint POC Rust library for the iOS simulator target.

Options:
  --release  Build with cargo --release.
  -h, --help Show this help message.

Environment:
  IOS_SIM_TARGET defaults to $IOS_SIM_TARGET.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      RELEASE=1
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

need_host Darwin "Slint iOS simulator build"
need_cmd cargo "Install Rust/Cargo with rustup."
need_cmd xcrun "Install Xcode Command Line Tools with: xcode-select --install"
need_rust_target "$IOS_SIM_TARGET"

if ! xcrun --sdk iphonesimulator --show-sdk-path >/dev/null 2>&1; then
  fail "iOS simulator SDK is unavailable. Install Xcode and accept its license."
fi

build_args=(build -p "$SLINT_PACKAGE" --target "$IOS_SIM_TARGET" --lib)
profile_dir="debug"
if [[ "$RELEASE" -eq 1 ]]; then
  build_args+=(--release)
  profile_dir="release"
fi

info "building $SLINT_PACKAGE library for iOS simulator target $IOS_SIM_TARGET"
( cd "$ROOT_DIR" && cargo "${build_args[@]}" )

ARTIFACT_DIR="$ROOT_DIR/target/$IOS_SIM_TARGET/$profile_dir"
if [[ ! -f "$ARTIFACT_DIR/libslint_poc.rlib" && ! -f "$ARTIFACT_DIR/libslint_poc.a" && ! -f "$ARTIFACT_DIR/libslint_poc.dylib" ]]; then
  fail "expected an iOS simulator library artifact under $ARTIFACT_DIR."
fi

info "built iOS simulator artifacts under $ARTIFACT_DIR"
