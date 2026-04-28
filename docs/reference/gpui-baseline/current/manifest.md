# GPUI Baseline Capture Manifest

Source: current GPUI desktop binary from the `slint-daemon-poc-clean` workspace.

Captured on: 2026-04-28, Hyprland workspace 1, monitor `DP-4`.

## Captures

- `captures/workspace-shell-dark.png`: GPUI desktop app shell in dark mode, captured with `grim -g "9,48 1902x1023"`.
- `captures/window/desktop-main-dark.png`: matched-geometry full GPUI shell at 1902x1023.
- `captures/titlebar/default.png`: crop from `window/desktop-main-dark.png`.
- `captures/sidebar/project-list-default.png`: crop from `window/desktop-main-dark.png`.
- `captures/sidebar/task-list-active.png`: crop from `window/desktop-main-dark.png`.
- `captures/terminal/text-quality.png`: crop from `window/desktop-main-dark.png`.
- `captures/right-sidebar/changes.png`: crop from `window/desktop-main-dark.png`.

## Notes

- This is the first successful compositor capture after `hyprctl monitors -j` started returning the active monitor and `grim` succeeded.
- The GPUI window was launched from `target/debug/another-one` to avoid rebuilding unrelated dirty desktop files in this Slint branch.
- Transient GPUI states that require interactive control activation are tracked in `notes/unavailable-transient-surfaces.md`.
