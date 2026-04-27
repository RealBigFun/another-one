# AnotherOne

AnotherOne is a greenfield desktop and mobile app built around local agent
workflows. The desktop client is a Flutter app talking to a Rust bridge
crate (`another-one-bridge`) that hosts an in-process iroh daemon for
mobile pairing.

## Layout

```
another-one/          flutter desktop app (Dart)
another-one-bridge/   FRB bridge — exposes core to Dart + hosts the embedded daemon
core/                 headless Rust library (project store, git, mcp, terminal launch)
daemon-sandbox/       iroh daemon library used by the bridge + standalone test binary
mcp-shim/             tiny Rust binary the daemon catalog can advertise
docs/                 architecture notes, postmortems, design docs
scripts/              dev + packaging scripts
```

## Development

Run the desktop app with hot reload:

```sh
scripts/dev-watch.sh
# or, equivalently:
cd another-one && flutter run -d "$(uname -s | sed 's/Darwin/macos/;s/Linux/linux/')"
```

Saves under `another-one/lib/` hot-reload immediately. Rust changes
under `another-one-bridge/` or `core/` hot-restart automatically once
`cargo build` finishes.

To regenerate the FFI bindings after editing the bridge:

```sh
cd another-one
flutter_rust_bridge_codegen generate
```

## Releasing for your own machine

Linux AppImage:

```sh
scripts/package-linux.sh           # builds, lands under target/release/linux/
scripts/package-linux.sh --open    # builds and launches it
scripts/package-linux.sh --install # builds and replaces $HOME/Applications/AnotherOne.AppImage
```

macOS `.app` + `.dmg`:

```sh
scripts/package-macos.sh
scripts/package-macos.sh --open
```

Both use ad-hoc codesigning — fine for personal installs on your own
machine, not a notarized public-distribution build.
