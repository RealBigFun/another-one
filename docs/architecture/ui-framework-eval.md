# UI Framework Evaluation: Slint vs Makepad

Branch: `poc/slint-makepad-ui-eval`

Baseline:
- Iroh-only transport.
- `alacritty_terminal` owns terminal parsing and grid state.
- UI frameworks render `Grid<Cell>` snapshots and forward input/resize events.
- Modern escape sequences and resize framing are handled below the UI layer.

## POC Scope

Both POCs must render the same fixture:
- Custom titlebar with one close button.
- Left sidebar with three fake project rows and active/hover/rest visual states.
- New Task modal with scrim, centered card, and two text fields.
- Terminal pane placeholder, then real PTY-backed grid.
- Design tokens from `desktop/src/tokens.rs`: `chrome_bg`, `card_bg`, `terminal_bg`, `overlay_hover`, `focus_ring`.

## Commands

```sh
cargo run -p slint-poc
cargo run -p makepad-poc
```

Mobile build commands are intentionally left as proof points for the platform pass:

```sh
cargo build -p slint-poc --target aarch64-apple-ios-sim
cargo apk build -p slint-poc --target aarch64-linux-android
cargo makepad apple ios --org=com.anotherone --app=poc run-sim aarch64 -p makepad-poc
cargo makepad android --abi=arm64 run -p makepad-poc
```

## Decision Matrix

| Criterion | Weight | Slint | Makepad |
| --- | --- | --- | --- |
| Terminal rendering correctness (vim, htop, claude code) | High |  |  |
| Terminal perf under flood (fps, dropped frames, CPU%) | High |  |  |
| Design token fidelity | High |  |  |
| Custom titlebar feasibility | Medium |  |  |
| Mobile build stability (toolchain friction) | High |  |  |
| Hot reload responsiveness | Medium |  |  |
| POC LOC | Low |  |  |
| Fluency: how the framework felt | Medium |  |  |

## Hard Fails

- Terminal renders incorrectly under realistic loads.
- Cannot build on iOS or Android in less than 1 hour of toolchain wrangling.
- Sustained flood performance is below 30 fps.

## Running Log

### Day 1

- Scaffolded `slint-poc` and `makepad-poc` as workspace members.
- Encoded shared fixture chrome and token colors in each framework.
- Terminal pane is still a fixture placeholder; real Iroh/alacritty wiring is next.
- Validation passed: `cargo check -p slint-poc -p makepad-poc`.
- Validation passed: `cargo check --workspace --all-targets`.
- Added a Linux-only `fontique/fontconfig-dlopen` feature shim in `slint-poc` because GPUI enables `yeslogic-fontconfig-sys/dlopen` in the shared workspace graph.
