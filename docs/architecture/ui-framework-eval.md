# UI Framework Evaluation: Slint POC

Branch: `slint-daemon-poc-clean`

Baseline:
- Iroh-only production transport.
- `alacritty_terminal` owns terminal parsing and grid state.
- The UI renders `Grid<Cell>` snapshots and forwards input/resize events.
- Modern escape sequences and resize framing are handled below the UI layer.

## Current Decision

Slint remains the active Rust UI POC. The branch keeps only daemon transport
work and `slint-poc`; discarded framework experiments and legacy app stacks
are intentionally absent.

## POC Scope

The POC must render:
- Custom titlebar with one close button.
- Left sidebar with fake project rows and active/hover/rest visual states.
- New Task modal with scrim, centered card, and two text fields.
- PTY-backed terminal pane.
- Design tokens from `desktop/src/tokens.rs`: `chrome_bg`, `card_bg`,
  `terminal_bg`, `overlay_hover`, `focus_ring`.

## Commands

```sh
cargo run -p daemon-sandbox
cargo run -p slint-poc
cargo-apk apk build -p slint-poc --target aarch64-linux-android --lib
```

## Decision Matrix

| Criterion | Weight | Slint |
| --- | --- | --- |
| Terminal rendering correctness (vim, htop, agent CLIs) | High | In progress |
| Terminal perf under flood (fps, dropped frames, CPU%) | High | In progress |
| Design token fidelity | High | In progress |
| Custom titlebar feasibility | Medium | Proven in POC |
| Mobile build stability (toolchain friction) | High | Android build/install proven |
| Hot reload responsiveness | Medium | Not evaluated |
| POC LOC | Low | Track during hardening |
| Fluency: how the framework felt | Medium | Acceptable so far |

## Running Log

### Day 1

- Scaffolded `slint-poc` as a workspace member.
- Encoded shared fixture chrome and token colors.
- Added Linux `fontique/fontconfig-dlopen` feature shim because GPUI enables
  `yeslogic-fontconfig-sys/dlopen` in the shared workspace graph.

### Day 2

- Added Slint vertical terminal slice: reads `/tmp/daemon-sandbox.ticket`,
  dials daemon-sandbox over Iroh, pre-authorizes the local POC endpoint in the
  sandbox allowlist, lists the synthetic sandbox project, attaches the shell
  tab, forwards keyboard input as raw PTY bytes, feeds PTY output into
  `alacritty_terminal`, and displays a throttled text snapshot.
- Replaced plaintext terminal snapshot with grouped Slint text/background
  models derived from `alacritty_terminal` cells. The slice resolves named,
  indexed, bright, dim, inverse, hidden, and truecolor colors and renders a
  block cursor.
- Rendering is still Slint `Text`/`Rectangle` repeaters, not the final
  CPU-rasterized image path. It is enough to validate transport, parser
  integration, and color fidelity before investing in rasterization.
- Validation passed: `cargo check -p slint-poc`.
- Runtime smoke passed: launched `daemon-sandbox`, launched `slint-poc`, and
  daemon logs showed the Slint client connected as a paired Iroh peer.
- Android build/install proof passed on a Pixel 7 Pro over wireless ADB.
