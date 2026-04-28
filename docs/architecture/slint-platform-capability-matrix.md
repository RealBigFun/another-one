# Slint Platform Capability Matrix

| Capability | Linux | macOS | Android | iOS | Windows |
| --- | --- | --- | --- | --- | --- |
| Slint window launches | implemented | compile-profile only | package metadata present | profile only | unsupported |
| App identity | `com.anotherone.Slint` | `com.anotherone.Slint` | `com.anotherone.slint` | `com.anotherone.slint` | unsupported |
| Input policy | desktop keyboard | desktop keyboard | touch IME | touch IME | unsupported |
| Terminal transport | Iroh daemon ticket | Iroh daemon ticket | Iroh daemon ticket planned | Iroh daemon ticket planned | unsupported |
| System appearance | env-backed seam | env-backed seam | env-backed seam | env-backed seam | unsupported |
| Open-In actions | shared core available | shared core available | platform hook required | platform hook required | unsupported |
| Resource sampling | shared core available | shared core available | procfs shared path | Darwin shared path | unsupported for Slint |
| Packaging proof | `cargo check -p slint-poc`, `cargo test -p slint-poc`, `scripts/dev-watch.sh slint` | target profile present; macOS host proof still required | target check, native library, and debug APK build pass locally; device install/runtime proof blocked by no `adb` target | profile present; macOS/Xcode proof required | no |

The matrix is backed by `slint-poc/src/platform.rs` and `slint-poc/src/style.rs`.

## 2026-04-28 Build Probe

- Linux Slint app verification passed with `cargo check -p slint-poc` and `cargo test -p slint-poc -- --nocapture`.
- Dev watcher verification passed with `sh -n scripts/dev-watch.sh` and timed `scripts/dev-watch.sh slint` smoke.
- Android target verification must use the rustup toolchain in this environment because `/usr/sbin/cargo` uses Fedora's system `rustc` and cannot see rustup target libraries.
- Android `cargo check -p slint-poc --target aarch64-linux-android` passes when `RUSTC=/home/mason/.cargo/bin/rustc`, `ANDROID_HOME=/home/mason/Android/Sdk`, `ANDROID_NDK_HOME=/home/mason/Android/Sdk/ndk/28.2.13676358`, and the NDK LLVM toolchain are selected.
- Android native library proof passes with `cargo ndk -t arm64-v8a -P 23 -o /tmp/anotherone-slint-jni build -p slint-poc --lib`, producing `/tmp/anotherone-slint-jni/arm64-v8a/libslint_poc.so`.
- Android APK package proof passes with `cargo apk build -p slint-poc --lib`, producing signed/verified `target/debug/apk/anotherone-slint.apk` for package `com.anotherone.slint`.
- Android device install, rotation, and touch/IME runtime proof remains blocked because `adb devices -l` returns no connected devices in this shell.
