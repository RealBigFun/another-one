# AnotherOne

AnotherOne is a greenfield desktop and mobile app built around local agent
workflows.

![AnotherOne screenshot](docs/assets/screenshot.png)

## Downloads

Download the latest desktop builds from the current GitHub release:

- [macOS Apple Silicon DMG](https://github.com/RealBigFun/another-one/releases/latest/download/AnotherOne-macos-aarch64.dmg)
- [macOS Intel DMG](https://github.com/RealBigFun/another-one/releases/latest/download/AnotherOne-macos-x86_64.dmg)
- [Linux x86_64 AppImage](https://github.com/RealBigFun/another-one/releases/latest/download/AnotherOne-linux-x86_64.AppImage)

## Development

Run the desktop app:

```sh
bash ./scripts/dev-watch.sh
```

The desktop target is macOS and Linux.

## Releasing for your own Mac

On macOS, build a locally signed `.app` bundle and `.dmg` with:

```sh
scripts/package-macos.sh
```

The package lands under `target/release/macos/`. To open the generated DMG
when packaging finishes, pass `--open`:

```sh
scripts/package-macos.sh --open
```

This is intended for personal installs on your own Mac. It uses ad-hoc
codesigning, so it is not a notarized public distribution build.
