# Unavailable Transient GPUI Surfaces

Captured on 2026-04-28 while building the Slint visual corpus.

## Current Coverage

- Full desktop shell, titlebar, project/task sidebar, terminal text pane, and right-sidebar changes mode are captured from the live GPUI window.
- New Task modal, Add Action modal, settings General/Agents sections, resource monitor popover, and an error toast are captured from interactive GPUI controls.
- These captures are sufficient for first-pass geometry, typography, color, and section-placement comparison against the current Slint shell.

## Not Captured Yet

- `terminal/color-smoke.png`
- `terminal/selection-cursor-links.png`
- GPUI menu/popover hover states

## Capture Blocker

The current automation session can launch and capture the GPUI window. Root-backed `ydotool` plus `hyprctl dispatch movecursor` can activate some GPUI controls and produced the modal/settings/resource/toast corpus listed above.

The remaining missing captures require terminal/content state setup rather than simple shell control activation. They need either a deterministic GPUI terminal fixture or manual terminal setup for color, selection, cursor, and OSC8/link states.

## Manual Reproduction

1. Launch the GPUI app from the frozen baseline or current desktop binary.
2. Use the footer create/settings controls, titlebar menus, right-sidebar tabs, and terminal prompt manually.
3. Capture the same relative paths listed above under `docs/reference/gpui-baseline/current/captures/`.
4. Re-run the matching Slint capture and visual diff commands.

## Ownership

This does not waive the missing surfaces. It records why the current automated corpus is partial; final visual fidelity remains gated by `another-one-p4r.3` and downstream Slint view/style parity work until the missing surfaces are captured or explicitly accepted by bd decision.
