# Slint Capture Manifest

Source: `slint-daemon-poc-clean`.

Captured on: 2026-04-28, Hyprland workspace 1, monitor `DP-4`.

## Captures

- `captures/workspace-shell-dark.png`: Slint client shell in dark mode, captured from the live `AnotherOne` / `com.anotherone.Slint` window.
- `captures/terminal-resize-wide.png`: Slint terminal shell before compositor-driven resize.
- `captures/terminal-resize-after.png`: Slint terminal shell after compositor-driven resize; used as manual responsiveness evidence for terminal dimension recalculation.
- `captures/terminal-fidelity-fixture.png`: Slint terminal fidelity fixture showing ANSI named colors, indexed color 208, RGB truecolor, combining marks, CJK wide cells, emoji, OSC8 link underline, selection overlay, and underline cursor state.

## Notes

- This capture is paired with `docs/reference/gpui-baseline/current/captures/workspace-shell-dark.png` for first-pass shell comparison.
- Dimensions differ from the GPUI capture because the Slint hot-reload window was already running in the right pane before GPUI was launched. Re-capture at matched geometry remains required for pixel-level diffing.
- The terminal fidelity capture is a deterministic Slint renderer fixture (`ANOTHERONE_SLINT_FIXTURE=terminal-fidelity`) and is accepted as Slint-side evidence for terminal renderer gates. Matched GPUI terminal captures remain tracked by the broader visual corpus task.
