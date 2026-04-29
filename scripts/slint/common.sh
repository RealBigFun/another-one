#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd -- "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
SLINT_PACKAGE="slint-poc"
ANDROID_PACKAGE_ID="com.anotherone.slint"
ANDROID_APK_NAME="anotherone-slint.apk"
ANDROID_TARGET="${ANDROID_TARGET:-aarch64-linux-android}"
ANDROID_NDK_ABI="${ANDROID_NDK_ABI:-arm64-v8a}"
ANDROID_MIN_API="${ANDROID_MIN_API:-23}"
IOS_SIM_TARGET="${IOS_SIM_TARGET:-aarch64-apple-ios-sim}"

fail() {
  echo "error: $*" >&2
  exit 1
}

info() {
  echo "==> $*"
}

need_cmd() {
  local name="$1"
  local hint="$2"
  if ! command -v "$name" >/dev/null 2>&1; then
    fail "$name is required. $hint"
  fi
}

need_host() {
  local expected="$1"
  local label="$2"
  local actual
  actual="$(uname -s)"
  if [[ "$actual" != "$expected" ]]; then
    fail "$label requires $expected; current host is $actual."
  fi
}

need_graphical_linux_session() {
  if [[ -z "${DISPLAY:-}" && -z "${WAYLAND_DISPLAY:-}" ]]; then
    fail "no graphical Linux session detected. Set DISPLAY or WAYLAND_DISPLAY before launching $SLINT_PACKAGE."
  fi
}

need_rust_target() {
  local target="$1"
  need_cmd rustup "Install rustup from https://rustup.rs, then run: rustup target add $target"
  if ! rustup target list --installed | grep -Fx "$target" >/dev/null 2>&1; then
    fail "Rust target $target is not installed. Run: rustup target add $target"
  fi
}

need_cargo_apk() {
  need_cmd cargo "Install Rust/Cargo with rustup."
  if ! cargo apk --version >/dev/null 2>&1; then
    fail "cargo-apk is required. Install it with: cargo install cargo-apk"
  fi
}

need_cargo_ndk() {
  need_cmd cargo "Install Rust/Cargo with rustup."
  if ! cargo ndk --version >/dev/null 2>&1; then
    fail "cargo-ndk is required for native library proof. Install it with: cargo install cargo-ndk"
  fi
}

resolve_android_sdk() {
  if [[ -n "${ANDROID_HOME:-}" && -d "$ANDROID_HOME" ]]; then
    printf '%s\n' "$ANDROID_HOME"
    return
  fi
  if [[ -n "${ANDROID_SDK_ROOT:-}" && -d "$ANDROID_SDK_ROOT" ]]; then
    printf '%s\n' "$ANDROID_SDK_ROOT"
    return
  fi
  fail "ANDROID_HOME or ANDROID_SDK_ROOT must point to an installed Android SDK."
}

resolve_android_ndk() {
  if [[ -n "${ANDROID_NDK_HOME:-}" && -d "$ANDROID_NDK_HOME" ]]; then
    printf '%s\n' "$ANDROID_NDK_HOME"
    return
  fi
  if [[ -n "${ANDROID_NDK_ROOT:-}" && -d "$ANDROID_NDK_ROOT" ]]; then
    printf '%s\n' "$ANDROID_NDK_ROOT"
    return
  fi

  local sdk
  sdk="$(resolve_android_sdk)"
  if [[ -d "$sdk/ndk" ]]; then
    local newest
    newest="$(find "$sdk/ndk" -mindepth 1 -maxdepth 1 -type d | sort -V | tail -n 1)"
    if [[ -n "$newest" ]]; then
      printf '%s\n' "$newest"
      return
    fi
  fi

  fail "Android NDK is required. Set ANDROID_NDK_HOME or install one under \$ANDROID_HOME/ndk."
}

need_android_toolchain() {
  need_rust_target "$ANDROID_TARGET"
  need_cargo_apk
  need_cmd java "Install a JDK visible on PATH; cargo-apk needs Java tooling."
  local sdk
  sdk="$(resolve_android_sdk)"
  local ndk
  ndk="$(resolve_android_ndk)"
  export ANDROID_HOME="$sdk"
  export ANDROID_SDK_ROOT="$sdk"
  export ANDROID_NDK_HOME="$ndk"
  export ANDROID_NDK_ROOT="$ndk"
}

apk_path_for_profile() {
  local profile_dir="$1"
  local expected="$ROOT_DIR/target/$profile_dir/apk/$ANDROID_APK_NAME"
  if [[ -f "$expected" ]]; then
    printf '%s\n' "$expected"
    return
  fi
  find "$ROOT_DIR/target" -path "*/apk/$ANDROID_APK_NAME" -type f -print | sort | tail -n 1
}

need_adb_device() {
  need_cmd adb "Install Android platform-tools and connect a device or emulator."
  local devices
  devices="$(adb devices | awk 'NR > 1 && $2 == "device" { print $1 }')"
  if [[ -z "$devices" ]]; then
    fail "adb is available, but no installable Android device is connected. Check: adb devices -l"
  fi
}
