# Unavailable Transient GPUI Surfaces

Captured on 2026-04-28 while building the Slint visual corpus.

## Current Coverage

- Full desktop shell, titlebar, project/task sidebar, terminal text pane, and right-sidebar changes mode are captured from the live GPUI window.
- These captures are sufficient for first-pass geometry, typography, color, and section-placement comparison against the current Slint shell.

## Not Captured Yet

- `modal/new-task.png`
- `toast/error.png`
- `settings/agents.png`
- `terminal/color-smoke.png`
- `terminal/selection-cursor-links.png`
- GPUI menu/popover hover states

## Capture Blocker

The current automation session can launch and capture the GPUI window, but reliable GPUI control activation was not achieved. The existing root `ydotoold` socket is not accessible to this user, and a user-scoped `ydotoold` socket can emit pointer events but did not activate the GPUI footer/settings/project controls in this session.

## Manual Reproduction

1. Launch the GPUI app from the frozen baseline or current desktop binary.
2. Use the footer create/settings controls, titlebar menus, right-sidebar tabs, and terminal prompt manually.
3. Capture the same relative paths listed above under `docs/reference/gpui-baseline/current/captures/`.
4. Re-run the matching Slint capture and visual diff commands.

## Ownership

This does not waive the missing surfaces. It records why the current automated corpus is partial; final visual fidelity remains gated by `another-one-p4r.3` and downstream Slint view/style parity work until the missing surfaces are captured or explicitly accepted by bd decision.
