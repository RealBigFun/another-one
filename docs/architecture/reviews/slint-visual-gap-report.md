# Slint Visual Gap Report

Reviewed on: 2026-04-29

Scope: `another-one-p4r.3` GPUI visual corpus and `another-one-p4r.6`
Slint visual diff closure. This report maps the required visual-fidelity
surfaces to committed evidence only. Temporary `/tmp` screenshots from this
pass are treated as capture attempts, not accepted artifacts.

## Evidence Matrix

| Surface | GPUI evidence | Slint evidence | Current status |
| --- | --- | --- | --- |
| Shell | `docs/reference/gpui-baseline/current/captures/workspace-shell-dark.png`; `docs/reference/gpui-baseline/current/captures/window/desktop-main-dark.png` | `docs/reference/slint/slint-daemon-poc-clean/captures/workspace-shell-dark.png`; `docs/reference/slint/slint-daemon-poc-clean/captures/window/desktop-main-dark.png`; light, compact, tablet, and mobile viewport captures under `captures/window/` | Pair exists for dark desktop shell. Raw unmasked diff exists at `docs/reference/slint/slint-daemon-poc-clean/metrics/desktop-main-dark-ae.png` / `visual-diff-ae.json` and is fail-state evidence, not a parity pass. |
| Titlebar | `docs/reference/gpui-baseline/current/captures/titlebar/default.png` | `docs/reference/slint/slint-daemon-poc-clean/captures/titlebar/default.png` | Default pair exists with raw unmasked diff. `titlebar/dirty-branch.png` from the visual gate naming list is still missing on both sides. |
| Sidebar | `docs/reference/gpui-baseline/current/captures/sidebar/project-list-default.png`; `docs/reference/gpui-baseline/current/captures/sidebar/task-list-active.png` | `docs/reference/slint/slint-daemon-poc-clean/captures/sidebar/project-list-default.png`; `docs/reference/slint/slint-daemon-poc-clean/captures/sidebar/task-list-active.png`; `docs/reference/slint/slint-daemon-poc-clean/captures/sidebar/grouped-project-tree.png` | Default and active evidence exists. Hover/focus/control-state crops remain missing, and `sidebar/task-list-hover-active.png` from the gate naming list is not captured as a matched pair. |
| Terminal | `docs/reference/gpui-baseline/current/captures/terminal/text-quality.png` | `docs/reference/slint/slint-daemon-poc-clean/captures/terminal/text-quality.png`; `docs/reference/slint/slint-daemon-poc-clean/captures/terminal-fidelity-fixture.png`; resize captures at `captures/terminal-resize-*.png` | Text-quality pair exists, but terminal parity remains partial. GPUI `terminal/color-smoke.png` and `terminal/selection-cursor-links.png` are still missing, so the Slint terminal fidelity fixture has no GPUI-matched corpus pair for those states. |
| Right-inspector | `docs/reference/gpui-baseline/current/captures/right-sidebar/changes.png` | `docs/reference/slint/slint-daemon-poc-clean/captures/right-sidebar/changes.png` | Changes-mode pair exists with raw unmasked diff. Commits/checks inspector modes remain unpaired; `right-sidebar/commits.png` from the gate naming list is missing. |
| Modal | `docs/reference/gpui-baseline/current/captures/modal/new-task.png`; `docs/reference/gpui-baseline/current/captures/modal/add-action.png` | `docs/reference/slint/slint-daemon-poc-clean/captures/modal/new-task.png` | New-task modal pair exists but has no committed diff output or manual pass note. GPUI add-action modal has no Slint paired artifact. |
| Settings | `docs/reference/gpui-baseline/current/captures/settings/general.png`; `docs/reference/gpui-baseline/current/captures/settings/agents.png` | none committed | GPUI settings evidence exists. Slint settings was visible during this pass, but no clean committed Slint settings screenshot was retained because the capture target changed/was covered before save verification. |
| Resource | `docs/reference/gpui-baseline/current/captures/resource/usage-popover.png` | `docs/reference/slint/slint-daemon-poc-clean/captures/window/desktop-layout-collapsed.png` includes a resource popover state | GPUI resource popover exists. Slint has contextual resource-popover evidence inside a shell fixture, but no matched `captures/resource/usage-popover.png` pair or diff artifact. |
| Toast | `docs/reference/gpui-baseline/current/captures/toast/error.png` | `docs/reference/slint/slint-daemon-poc-clean/captures/toast/error.png` | Error-toast pair exists. It still needs manual review and/or masked diff output before it can satisfy p4r.6. |

## Capture Attempts This Pass

- `hyprctl monitors -j` returned active monitor `eDP-2`; `grim` was present at `/usr/sbin/grim`.
- `hyprctl clients -j` showed a live Slint window (`class: com.anotherone.Slint`) on workspace 1.
- A temporary `grim` capture of the live Slint window initially showed Settings > General with a transient toast, proving the state was reachable locally. The later save attempt was invalid because another surface covered or replaced the target geometry, so the bad artifact was removed and no settings screenshot was accepted.
- `ydotool click --help` could not connect to `/tmp/.ydotool_socket` due permission denial, so click-driven Slint settings/resource captures were not automated from this session.
- The existing `./target/debug/another-one` binary launched without rebuilding and produced a live GPUI window, but the available state contained non-deterministic live task/user content and did not cover the missing terminal color/selection/link fixture states. No GPUI screenshot from that launch was accepted.

## Remaining Gaps

- Produce matched Slint settings captures for at least General, and preferably Agents to match the GPUI corpus.
- Produce a matched Slint `resource/usage-popover.png` artifact instead of relying on the contextual collapsed-shell fixture.
- Capture or explicitly decide GPUI terminal `color-smoke` and `selection-cursor-links` states so Slint terminal fidelity can be judged against a GPUI baseline.
- Capture titlebar dirty-branch, sidebar hover/focus, right-inspector commits/checks, and Slint add-action modal pairs, or record bd deviation decisions.
- Replace first-pass raw diffs with masked/fixture-aware comparisons and manual review notes before treating any pair as p4r.6 closure evidence.
