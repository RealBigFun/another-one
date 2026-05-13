#!/usr/bin/env bash
# Build the native Android APK from a clean workspace state.
#
# This is the current non-Flutter Android path: cargo-ndk builds the
# Rust `another-one` library and Gradle packages it into a NativeActivity APK.
# The historical script name is kept so existing workflows keep working.
#
# Pure-client build: passes --no-default-features so the daemon-host
# feature is excluded. ProjectStore::save / load are no-ops, the
# binary cannot write the host's projects.json file, and the
# v0.1.18 store-corruption class of bug is impossible by
# construction. Default-feature builds (`cargo build` from the
# command line) are the registry-host build and write to disk.
#
# Usage:
#   scripts/build-mobile.sh                           # build + leave APK in place
#   scripts/build-mobile.sh install                   # also `adb install -r`
#   scripts/build-mobile.sh restart                   # also restart the app
#   ANDROID_NDK_HOME=... scripts/build-mobile.sh      # override NDK
#
# Prereqs:
# - ANDROID_NDK_HOME set (or detected from $HOME/Android/Sdk/ndk/<version>)
# - cargo-ndk on $PATH (cargo install cargo-ndk)
# - Java for gradlew

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WORKTREE="$(cd "$SCRIPT_DIR/.." && pwd)"
ACTION="${1:-build}"

# Auto-detect the Android NDK if not preset. Picks the lexicographically
# highest installed version under $HOME/Android/Sdk/ndk so a fresh
# install just works without env munging.
if [ -z "${ANDROID_NDK_HOME:-}" ]; then
  for candidate in "$HOME/Android/Sdk/ndk"/*/; do
    [ -d "$candidate" ] || continue
    ANDROID_NDK_HOME="${candidate%/}"
  done
  if [ -z "${ANDROID_NDK_HOME:-}" ]; then
    echo "error: ANDROID_NDK_HOME not set and no NDK found under \$HOME/Android/Sdk/ndk" >&2
    exit 1
  fi
fi
export ANDROID_NDK_HOME

# Linux/macOS dev workstations sometimes have a Fedora-shipped /usr/sbin/cargo
# that doesn't know about cargo-ndk; ensure $HOME/.cargo/bin wins.
export PATH="$HOME/.cargo/bin:$HOME/Android/Sdk/platform-tools:$PATH"

# fontconfig is only available at runtime via dlopen on the device. The build
# fails to link without this hint because gpui-mobile pulls in font-kit which
# tries to detect fontconfig at compile time.
export RUST_FONTCONFIG_DLOPEN=on

cd "$WORKTREE"

echo "==> cargo ndk build (arm64-v8a, --no-default-features for pure-client)"
cargo ndk \
  --target arm64-v8a \
  --platform 26 \
  -o app/android/app/src/main/jniLibs \
  build \
  -p another-one \
  --no-default-features

echo "==> gradle assembleDebug"
cd app/android
./gradlew assembleDebug

APK="$WORKTREE/app/android/app/build/outputs/apk/debug/app-debug.apk"

case "$ACTION" in
  build)
    echo "==> done. APK at: $APK"
    ;;
  install)
    echo "==> adb install -r"
    adb install -r "$APK"
    echo "==> done."
    ;;
  restart)
    echo "==> adb install -r"
    adb install -r "$APK"
    echo "==> force-stop + restart"
    adb shell am force-stop dev.anotherone.app
    adb shell am start -n dev.anotherone.app/android.app.NativeActivity >/dev/null
    echo "==> done."
    ;;
  *)
    echo "error: unknown action '$ACTION' (build | install | restart)" >&2
    exit 1
    ;;
esac
