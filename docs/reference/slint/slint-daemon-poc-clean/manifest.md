# Slint Capture Manifest

Source: `slint-daemon-poc-clean` at `9020c85`.

Captured on: 2026-04-28, Hyprland workspace 1, monitor `DP-4`.

## Captures

- `captures/workspace-shell-dark.png`: Slint client shell in dark mode, captured from the live `AnotherOne` / `com.anotherone.Slint` window.
- `captures/terminal-resize-wide.png`: Slint terminal shell before compositor-driven resize.
- `captures/terminal-resize-after.png`: Slint terminal shell after compositor-driven resize; used as manual responsiveness evidence for terminal dimension recalculation.

## Notes

- This capture is paired with `docs/reference/gpui-baseline/current/captures/workspace-shell-dark.png` for first-pass shell comparison.
- Dimensions differ from the GPUI capture because the Slint hot-reload window was already running in the right pane before GPUI was launched. Re-capture at matched geometry remains required for pixel-level diffing.
