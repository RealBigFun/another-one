# `slint-poc/` — Slint daemon client POC

Slint proof of concept for the Rust-only client direction. It connects to
[[daemon-sandbox]] over Iroh, feeds PTY bytes through `alacritty_terminal`,
and renders the resulting grid with GPUI-derived design tokens.

## Entry points

- `slint-poc/src/main.rs` — desktop binary entry point.
- `slint-poc/src/lib.rs` — shared app bootstrap, including Android entry.
- `slint-poc/ui/app.slint` — chrome, responsive layout, modal, and terminal grid.

## Running

```sh
cargo run -p daemon-sandbox
cargo run -p slint-poc
```

The reproducible platform entry points live under `scripts/slint/`:

```sh
./scripts/slint/linux-dev.sh
./scripts/slint/linux-release.sh
./scripts/slint/macos-build.sh --release
./scripts/slint/android-apk.sh --ndk-lib-proof
./scripts/slint/ios-simulator-build.sh
./scripts/slint/verify-platform-scripts.sh
```

Each script checks host/toolchain prerequisites before invoking Cargo and exits
with an explicit setup message when the host cannot support that target.

## Platform Builds

### Linux

Use `./scripts/slint/linux-dev.sh` for the development profile and
`./scripts/slint/linux-release.sh` for release. Both require a Linux host and
Cargo. Add `--run` to launch the resulting binary; the script then also requires
`DISPLAY` or `WAYLAND_DISPLAY`.

### macOS

Use `./scripts/slint/macos-build.sh --release` on Darwin hosts. It requires
Cargo plus Xcode Command Line Tools (`xcrun`). `--target aarch64-apple-darwin`
or `--target x86_64-apple-darwin` may be used for explicit cargo profiles.

### Android

Use `./scripts/slint/android-apk.sh` for the debug APK and add `--install` to
install it through `adb`. The script requires:

- `ANDROID_HOME` or `ANDROID_SDK_ROOT` pointing at an Android SDK.
- `ANDROID_NDK_HOME` or an SDK-managed NDK under `$ANDROID_HOME/ndk`.
- `rustup target add aarch64-linux-android`.
- `cargo install cargo-apk`.
- a JDK on `PATH`.
- `adb` plus a connected device only when `--install` is used.

The APK path is `target/debug/apk/anotherone-slint.apk` for debug builds and
`target/release/apk/anotherone-slint.apk` for release builds. The package id is
`com.anotherone.slint`, matching `slint-poc/Cargo.toml`.

Add `--ndk-lib-proof` to run the `cargo-ndk` native library proof. It requires
`cargo install cargo-ndk` and writes
`target/slint-android-jni/arm64-v8a/libslint_poc.so`.

### iOS Simulator

Use `./scripts/slint/ios-simulator-build.sh` on a macOS host with Xcode. It
requires `rustup target add aarch64-apple-ios-sim` and proves the Rust library
build for the iOS simulator profile. There is no app bundle or simulator install
step yet.

## CI Gates

`.github/workflows/slint-platform-gates.yml` runs:

- shell syntax and help-path verification for every Slint platform script;
- Linux dev and release Slint builds;
- Android APK packaging plus `cargo-ndk` JNI proof;
- macOS release build and iOS simulator library build.

The scripts are the local source of truth for platform selection. Slint view and
layout files should not branch on platform to compensate for packaging or host
toolchain differences.

## Known gaps

- Terminal rendering is still Slint repeater-based, not the final rasterized
  `Grid<Cell>` path.
- Android device install/runtime proof still depends on an attached `adb`
  device.
- iOS is currently a simulator library build proof only; a production iOS app
  bundle is not wired.
