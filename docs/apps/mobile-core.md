# Obsolete: `mobile-core/` Flutter bridge experiment

> Historical note only. `mobile-core/` was part of an abandoned Flutter/Dart mobile experiment. The current Android path is the native Gradle project under `app/android/` plus shared Rust crates (`daemon-client`, `daemon-proto`, `daemon-transport`, `core`, and `app`). Do not use this page as implementation guidance.

## What replaced it

- Iroh client/session code lives in `daemon-client/`.
- Shared wire types live in `daemon-proto/`.
- Transport abstraction lives in `daemon-transport/`.
- Android packaging lives in `app/android/` and is built by `scripts/build-mobile.sh`.

## Why keep this page

Some historical postmortems refer to the old bridge while explaining Android networking/runtime bugs. Keeping this stub preserves those links without presenting the old experiment as active architecture.
