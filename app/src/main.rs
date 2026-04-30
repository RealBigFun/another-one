//! AnotherOne desktop binary entry point. The actual app lives in
//! `lib.rs` so the same source files compile both this binary
//! (mac/linux) and the Android `cdylib` (`libanother_one.so`).

#[hotpath::main]
fn main() {
    // On Android the actual entry is `another_one::android_main`,
    // exported by the cdylib and called by `NativeActivity` once
    // the `.so` is loaded — `fn main()` is never invoked. Keep this
    // binary target compiling for `aarch64-linux-android` (cargo-ndk
    // builds the whole package) by stubbing it out.
    #[cfg(not(target_os = "android"))]
    another_one::run_desktop();
}
