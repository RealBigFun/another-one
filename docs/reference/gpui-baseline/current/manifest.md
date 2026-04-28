# GPUI Baseline Capture Manifest

Source: current `slint-daemon-poc-clean` workspace at `9020c85`.

Captured on: 2026-04-28, Hyprland workspace 1, monitor `DP-4`.

## Captures

- `captures/workspace-shell-dark.png`: GPUI desktop app shell in dark mode, captured with `grim -g "1929,48 1514x1023"`.

## Notes

- This is the first successful compositor capture after `hyprctl monitors -j` started returning the active monitor and `grim` succeeded.
- The GPUI window was launched with `cargo run -p another-one`.
- Broader GPUI state coverage is still required for the full visual corpus: modals, menus, settings/style surfaces, terminal states, and component states.
