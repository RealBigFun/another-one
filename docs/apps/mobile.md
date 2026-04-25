# `mobile/` — Flutter sandbox client

Minimal Flutter app that talks to [[daemon-sandbox]] over WebSocket or Iroh
QUIC (via [[another-one-bridge]]) and renders the PTY byte stream with
`xterm.dart`. Android + iOS targets scaffolded; only Android verified
so far.

## Entry points

- `lib/main.dart` — `TerminalPage` state, `_buildTransport` dispatches by
  URL scheme (`ws://` → WebSocket, `iroh://<id>?direct=host:port` →
  Iroh). Init calls `RustLib.init()` to bring [[another-one-bridge]] online.
- `lib/src/transport.dart` — `TerminalTransport` interface (see
  [[../architecture/transport-abstraction]]).
- `lib/src/transport_websocket.dart` — WS implementation (web_socket_channel).
- `lib/src/transport_iroh.dart` — Iroh implementation, wraps
  FRB-generated `IrohSession`.
- `lib/src/rust/` — FRB-generated bindings, do not edit.
- `rust_builder/` — cargokit-backed Flutter plugin that cross-compiles
  [[another-one-bridge]] for every target as part of `flutter build`.

## Running

Prereqs on the dev machine:
- Flutter SDK, Android NDK (comes with Android Studio).
- Rust with android targets: `rustup target add aarch64-linux-android
  armv7-linux-androideabi x86_64-linux-android i686-linux-android`.
- `cargo-ndk` and `flutter_rust_bridge_codegen` installed.
- On zsh/bash, ensure `~/.cargo/bin` comes before `/usr/sbin` so
  cargokit's `rustup run stable cargo` hits the rustup toolchain, not
  Fedora's system cargo. See [[../postmortems/2026-04-23-iroh-android-hang]].

```sh
# From repo root
cargo run -p daemon-sandbox &          # terminal A
cd mobile
flutter build apk --debug              # builds Rust + APK
adb install -r build/app/outputs/flutter-apk/app-debug.apk
adb shell monkey -p com.anotherone.mobile -c android.intent.category.LAUNCHER 1
```

Type a URL into the "Endpoint" field:
- `ws://10.0.2.2:5617/pty` for WebSocket (Android emulator's host
  loopback alias).
- `iroh://<EndpointId>?direct=10.0.2.2:<PORT>` for Iroh (EndpointId + UDP
  port from the daemon's online-log line).

## Known limitations

- `xterm.dart` doesn't fully parse Claude Code's modern escape sequences
  (kitty keyboard protocol `\x1b[<u` / `\x1b[>1u`, synchronized output
  `\x1b[?2026h`, focus reporting). Result: TUI apps with these codes
  render blank on mobile. Bash/zsh/vim/htop render fine. Filed for
  followup — either patch xterm.dart, pre-strip these sequences daemon-
  side, or swap the renderer.
- Iroh path sends resize events but [[daemon-sandbox]] doesn't process
  them yet (no control framing). PTY stays 80×24 for Iroh connections.
- URL entry is manual; proper QR pairing is a planned next step.
