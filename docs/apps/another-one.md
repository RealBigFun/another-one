# `another-one/` — Flutter desktop app

The canonical desktop client. Flutter (Dart) on top of
[[another-one-bridge]], which exposes [[../apps/daemon-sandbox]] +
the headless `core` Rust library through `flutter_rust_bridge`.

The original GPUI Rust desktop (`desktop/`) was retired in
Phase 6 of the GPUI → Flutter migration; every surface that
lived there ported into Flutter widgets that read from the
bridge. The bridge runs the embedded iroh daemon in-process,
including a dedicated PTY-drain thread that pulls
`pending_tab_launches` off the registry and spawns real PTYs
the same way GPUI's render-tick used to.

## Entry points

- `another-one/lib/main.dart` — `runApp(ProviderScope(child: …))`.
  Calls `RustLib.init()` to bring [[another-one-bridge]] online,
  then `bootEmbeddedDaemon()` to start the in-process iroh
  endpoint and the PTY drain.
- `another-one/lib/src/screens/desktop_shell.dart` — top-level
  layout. Composes the titlebar, sidebar, main pane, right
  sidebar (or settings page when `settingsOpenProvider` is on).
- `another-one/lib/src/screens/desktop_titlebar/` — chrome row:
  build chip, custom-actions split-button, Open In, GitHub,
  pull-request pill, git-actions split-button, pair-mobile,
  resource indicator.
- `another-one/lib/src/screens/desktop_sidebar/` — project /
  task tree on the left.
- `another-one/lib/src/screens/desktop_terminal/` — main
  terminal pane (xterm.dart) + tab strip.
- `another-one/lib/src/screens/desktop_right_sidebar/` —
  changes / commits / checks / compare panes.
- `another-one/lib/src/screens/settings_page/` — full-page
  settings: Agents / Open In / Git Actions / Keybindings / MCP.
- `another-one/lib/src/state/` — riverpod providers.
- `another-one/lib/src/connection.dart` —
  `DaemonConnection` abstract class. Two implementors:
  `LocalTransport` (in-process FFI to the embedded daemon) +
  `IrohTransport` (remote daemons over iroh).
- `another-one/lib/src/rust/` — FRB-generated bindings (do not
  hand-edit; regen via `flutter_rust_bridge_codegen generate`).
- `another-one/rust_builder/` — cargokit-backed Flutter plugin
  that cross-compiles [[another-one-bridge]] for every target as
  part of `flutter build`.

## Running

```sh
# from repo root
scripts/dev-watch.sh
# or, equivalently:
cd another-one && flutter run -d "$(uname -s | sed 's/Darwin/macos/;s/Linux/linux/')"
```

`flutter run` watches `lib/` and triggers hot reloads on save.
Edits under `another-one-bridge/` or `core/` trigger a Rust
rebuild + hot-restart.

## Regenerating bindings

```sh
cd another-one
flutter_rust_bridge_codegen generate
```

## Packaging

See [[../README#Releasing-for-your-own-machine]] for `package-{linux,macos}.sh`.

## Architecture

- The Flutter UI is a thin shell — every persistent state lives
  in [[../architecture/peer-to-peer-nodes|core's `ProjectStore`]]
  (read-write through bridge verbs).
- PTY launches queue onto `RegistryState::pending_tab_launches`;
  the bridge's drain thread (added in Phase 6) consumes them
  and publishes `broadcast` + `writer` handles back to the
  registry so `LocalSession::attach_tab` and `send` resolve.
- Iroh-over-LAN still works for mobile — the daemon endpoint
  the bridge boots is the same one mobile pairs with.

## Known gaps

- xterm.dart doesn't fully parse modern Claude Code escape
  sequences (kitty keyboard protocol, synchronized output,
  focus reporting). TUIs that use those render blank; bash /
  zsh / vim / htop render fine.
- Drag-resize gutters between sidebar / main / right-sidebar
  aren't wired up yet — panel widths are fixed.
- Side-by-side staging build (Phase 5 #5) is deferred until a
  Flutter-aware visual diff harness lands.
