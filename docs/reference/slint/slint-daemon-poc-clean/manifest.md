# Slint Capture Manifest

Source: `slint-daemon-poc-clean`.

Captured on: 2026-04-28, Hyprland workspace 1, monitor `DP-4`.

## Captures

- `captures/workspace-shell-dark.png`: Slint client shell in dark mode, captured from the live `AnotherOne` / `com.anotherone.Slint` window.
- `captures/window/desktop-main-dark.png`: matched-geometry full Slint shell at 1902x1023.
- `captures/window/desktop-layout-collapsed.png`: deterministic layout fixture with the left drawer and right inspector collapsed to GPUI-style rails and the resource popover anchored from the titlebar indicator.
- `captures/window/desktop-main-light.png`: forced light appearance shell using `ANOTHERONE_SLINT_APPEARANCE=light`.
- `captures/window/compact-main-dark.png`: compact-width dark shell at 740x1023 after resizing the Hyprland window.
- `captures/window/tablet-main-dark.png`: tablet-width dark shell at 1024x768.
- `captures/window/mobile-portrait-dark.png`: mobile portrait dark shell at 390x844.
- `captures/window/mobile-landscape-dark.png`: mobile landscape dark shell at 844x390.
- `captures/titlebar/default.png`: crop from `window/desktop-main-dark.png`.
- `captures/sidebar/project-list-default.png`: crop from `window/desktop-main-dark.png`.
- `captures/sidebar/task-list-active.png`: crop from `window/desktop-main-dark.png`.
- `captures/terminal/text-quality.png`: crop from `window/desktop-main-dark.png`.
- `captures/right-sidebar/changes.png`: crop from `window/desktop-main-dark.png`.
- `captures/terminal-resize-wide.png`: Slint terminal shell before compositor-driven resize.
- `captures/terminal-resize-after.png`: Slint terminal shell after compositor-driven resize; used as manual responsiveness evidence for terminal dimension recalculation.
- `captures/terminal-fidelity-fixture.png`: Slint terminal fidelity fixture showing ANSI named colors, indexed color 208, RGB truecolor, combining marks, CJK wide cells, emoji, OSC8 link underline, selection overlay, and underline cursor state.
- `captures/modal/new-task.png`: deterministic Slint visual-state fixture using `ANOTHERONE_SLINT_VISUAL_STATE=new-task-modal`.
- `captures/toast/error.png`: deterministic Slint visual-state fixture using `ANOTHERONE_SLINT_VISUAL_STATE=toast-error`.
- `captures/components/state-fixture.png`: deterministic Slint base-component fixture using `ANOTHERONE_SLINT_FIXTURE=component-states`; captured as a floating 1380x860 Hyprland window.

## Metrics

- `metrics/visual-diff-ae.json`: first-pass raw ImageMagick diff output for matched GPUI/Slint desktop shell, titlebar, sidebar, and right-sidebar crops. These are intentionally unmasked and do not claim visual pass.
- `metrics/*-ae.png`: raw visual diff images for the same first-pass comparisons.

## Notes

- This capture is paired with `docs/reference/gpui-baseline/current/captures/workspace-shell-dark.png` for first-pass shell comparison.
- `window/desktop-main-dark.png` and its crops are captured at the same geometry as the GPUI baseline for first-pass diffing.
- Tablet/mobile captures are Linux compositor viewport proofs only; Android rotation remains blocked until a device is visible to `adb`.
- The terminal fidelity capture is a deterministic Slint renderer fixture (`ANOTHERONE_SLINT_FIXTURE=terminal-fidelity`) and is accepted as Slint-side evidence for terminal renderer gates. Matched GPUI terminal captures remain tracked by the broader visual corpus task.
