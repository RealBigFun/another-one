# UI Framework Evaluation: Slint vs Makepad

Branch: `poc/slint-makepad-ui-eval`

Baseline:
- Iroh-only transport.
- `alacritty_terminal` owns terminal parsing and grid state.
- UI frameworks render `Grid<Cell>` snapshots and forward input/resize events.
- Modern escape sequences and resize framing are handled below the UI layer.

## Current Decision

- Makepad is eliminated from the eval.
- Reason: text rendering quality was unacceptable in the tuned 13px monospace canary, before any terminal-grid implementation work.
- Impact: stop Makepad implementation work. Keep `makepad-poc` in the branch as evaluation evidence only.
- Next focus: continue Slint unless it also trips a hard fail.

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
| Terminal rendering correctness (vim, htop, claude code) | High |  | DQ: text quality unacceptable |
| Terminal perf under flood (fps, dropped frames, CPU%) | High |  | Not evaluated after DQ |
| Design token fidelity | High |  | Not evaluated after DQ |
| Custom titlebar feasibility | Medium |  | Not evaluated after DQ |
| Mobile build stability (toolchain friction) | High |  | Not evaluated after DQ |
| Hot reload responsiveness | Medium |  | Not evaluated after DQ |
| POC LOC | Low |  | Not evaluated after DQ |
| Fluency: how the framework felt | Medium |  | DQ: unacceptable text for app baseline |

## Hard Fails

- Terminal renders incorrectly under realistic loads.
- Cannot build on iOS or Android in less than 1 hour of toolchain wrangling.
- Sustained flood performance is below 30 fps.

## Decisions

### Makepad Text Quality

- Status: eliminated.
- Evidence: Makepad's public release notes call out poor small-font rendering, lack of font hinting, and SDF glyph rendering as an intentional speed/memory tradeoff that can hurt low-resolution quality. Local crate source confirms the primary app-level knobs are `DrawText.text_style`, `font_scale`, `temp_y_shift`, and `TextStyle` font family/size/line spacing.
- POC response: avoided biased sub-12px fixture labels and added a 13px monospace terminal-text canary before investing deeper in transport wiring.
- Decision: the tuned canary still looked unacceptable. Makepad fails the terminal-rendering criterion regardless of chrome/widget ergonomics.

## Running Log

### Day 1

- Scaffolded `slint-poc` and `makepad-poc` as workspace members.
- Encoded shared fixture chrome and token colors in each framework.
- Terminal pane is still a fixture placeholder; real Iroh/alacritty wiring is next.
- Validation passed: `cargo check -p slint-poc -p makepad-poc`.
- Validation passed: `cargo check --workspace --all-targets`.
- Added a Linux-only `fontique/fontconfig-dlopen` feature shim in `slint-poc` because GPUI enables `yeslogic-fontconfig-sys/dlopen` in the shared workspace graph.
- Flagged Makepad small-font quality as a potential hard fail; raised fixture typography above 12px and added a monospace terminal-text canary to separate bad placeholder styling from framework text quality.
- User review found tuned Makepad text quality unacceptable. Makepad is DQ; no further Makepad implementation work planned.

### Day 2

- Continued Slint only after Makepad DQ.
- Added the first Slint vertical terminal slice: reads `/tmp/daemon-sandbox.ticket`, dials daemon-sandbox over Iroh, pre-authorizes the local POC endpoint in the sandbox allowlist, lists the synthetic sandbox project, attaches the shell tab, forwards keyboard input as raw PTY bytes, feeds PTY output into `alacritty_terminal`, and displays a throttled text snapshot in the Slint terminal pane.
- Replaced the plaintext terminal snapshot with grouped Slint text/background models derived from `alacritty_terminal` cells. The slice resolves named, indexed, bright, dim, inverse, hidden, and truecolor foreground/background colors and renders a block cursor.
- Rendering is still Slint `Text`/`Rectangle` repeaters, not the final CPU-rasterized `Grid<Cell>` image path. It is enough to validate transport, parser integration, and color fidelity before investing in rasterization.
- Validation passed: `cargo check -p slint-poc`.
- Runtime smoke passed: launched `daemon-sandbox`, launched `slint-poc`, and daemon logs showed the Slint client connected as a paired Iroh peer.
- Android build/install is not proven yet. Rust Android targets are installed, but `cargo-apk`, Android SDK/NDK env, and `aarch64-linux-android-clang` are missing in this environment.
