# Android / mobile support

The current mobile target is a native Android shell under `app/android/` that packages the Rust `another-one` library into an APK using `NativeActivity`. It is not a Flutter app.

## Entry points

- `scripts/build-mobile.sh` — builds the Rust Android library with `cargo ndk`, then runs Gradle.
- `app/android/settings.gradle.kts` — minimal Android Gradle project; comments clarify that all UI lives in Rust.
- `app/android/app/src/main/AndroidManifest.xml` — `NativeActivity` manifest.
- `daemon-client/` — Rust client session code used for paired daemon connections.
- `daemon-proto/` and `daemon-transport/` — shared control/projection/session contracts.

## Running

Prereqs on the dev machine:

- Android SDK + NDK.
- Rust Android target for the ABI being built.
- `cargo-ndk` on `$PATH`.
- Java for Gradle.

```sh
# Build only
scripts/build-mobile.sh

# Build and install
scripts/build-mobile.sh install

# Build, install, and restart the app
scripts/build-mobile.sh restart
```

The script currently builds `arm64-v8a` with `--no-default-features` so the Android client does not embed the daemon host or write desktop `projects.json` state.

## Architecture notes

Android is a client of daemon-owned state, like desktop. Durable actions travel through `daemon_proto::Control`; projections arrive as `WorkerReply::ProjectList`; PTY bytes are tagged by `(section_id, tab_id)`.

The Android shell is intentionally small. Keep platform-specific code at the edges and prefer shared Rust session/state logic where possible.

## Historical note

Earlier experiments used a separate Flutter mobile sandbox and a `mobile-core` bridge crate. Those docs are obsolete and retained only for postmortem context; do not use them as current architecture guidance.
