#!/usr/bin/env bash

set -euo pipefail

# shellcheck source=scripts/slint/common.sh
SCRIPT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/common.sh"

INSTALL_AFTER=0
RELEASE=0
NDK_LIB_PROOF=0

usage() {
  cat <<EOF
Usage: $0 [--release] [--install] [--ndk-lib-proof]

Build the Slint POC Android APK through cargo-apk.

Options:
  --release          Build a release APK.
  --install          Install the resulting APK with adb after a successful build.
  --ndk-lib-proof    Also build the JNI library with cargo-ndk into
                     target/slint-android-jni/.
  -h, --help         Show this help message.

Environment:
  ANDROID_HOME or ANDROID_SDK_ROOT must point at an Android SDK.
  ANDROID_NDK_HOME may point at a specific NDK; otherwise the newest
  SDK-managed NDK under \$ANDROID_HOME/ndk is selected.
  ANDROID_TARGET defaults to $ANDROID_TARGET.
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --release)
      RELEASE=1
      shift
      ;;
    --install)
      INSTALL_AFTER=1
      shift
      ;;
    --ndk-lib-proof)
      NDK_LIB_PROOF=1
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

need_android_toolchain

profile_dir="debug"
apk_args=(apk build -p "$SLINT_PACKAGE" --target "$ANDROID_TARGET" --lib)
if [[ "$RELEASE" -eq 1 ]]; then
  apk_args+=(--release)
  profile_dir="release"
fi

info "building Android APK for $ANDROID_TARGET"
( cd "$ROOT_DIR" && cargo "${apk_args[@]}" )

APK_PATH="$(apk_path_for_profile "$profile_dir")"
[[ -n "$APK_PATH" && -f "$APK_PATH" ]] || fail "cargo-apk completed, but $ANDROID_APK_NAME was not found under target/*/apk."
info "built $APK_PATH"

if [[ "$NDK_LIB_PROOF" -eq 1 ]]; then
  need_cargo_ndk
  JNI_OUT="$ROOT_DIR/target/slint-android-jni"
  info "building native library proof with cargo-ndk into $JNI_OUT"
  (
    cd "$ROOT_DIR"
    cargo ndk \
      -t "$ANDROID_NDK_ABI" \
      -P "$ANDROID_MIN_API" \
      -o "$JNI_OUT" \
      build -p "$SLINT_PACKAGE" --lib
  )
  [[ -f "$JNI_OUT/$ANDROID_NDK_ABI/libslint_poc.so" ]] || fail "expected JNI library at $JNI_OUT/$ANDROID_NDK_ABI/libslint_poc.so."
fi

if [[ "$INSTALL_AFTER" -eq 1 ]]; then
  need_adb_device
  info "installing $ANDROID_PACKAGE_ID from $APK_PATH"
  adb install -r "$APK_PATH"
fi
