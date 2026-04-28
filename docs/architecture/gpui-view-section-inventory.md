# GPUI View And Section Inventory

Slint views must recreate these GPUI surfaces before claiming parity. This is source inventory, not visual approval; screenshot evidence is still owned by the visual fidelity gate.

## Primary Views

| GPUI surface | Source | Slint owner |
| --- | --- | --- |
| Shell frame: custom titlebar, left project/task sidebar, workspace, right inspector, footer/status strip | `desktop/src/app.rs`, `desktop/src/titlebar.rs` | Layout, Views, Base Components |
| Project/task navigation: root projects, worktree projects, task rows, selected task state, dirty/ahead-behind metadata | `desktop/src/left_sidebar.rs`, `desktop/src/project_page.rs` | Views, Base Components |
| Terminal workspace: tab strip, pinned tabs, active tab, terminal surface, empty/restore/failure states | `desktop/src/app.rs`, `desktop/src/terminal_runtime.rs`, `desktop/src/panels.rs` | Views, Terminal Production Readiness |
| Right inspector: files, commits, checks, branch compare, pull requests, action state | `desktop/src/right_sidebar.rs`, `desktop/src/app.rs`, `desktop/src/project_page.rs` | Views, Base Components |
| Project page: project details, branch/actions, pull requests, settings-like project controls | `desktop/src/project_page.rs` | Views |
| Settings: agents, Open-In apps, updates/build identity, preferences, shortcuts | `desktop/src/settings_page.rs`, `desktop/src/shortcuts.rs` | Views, Platform Integrations |
| MCP page: server catalog, provider status, enable/configuration surfaces | `desktop/src/mcp_page.rs` | Views, Platform Integrations |
| Pairing: QR/pairing code, allowlist/reset state | `desktop/src/pairing.rs`, `desktop/src/daemon_host.rs` | Views, Platform Integrations |

## Modal And Overlay Views

| GPUI surface | Source | Slint owner |
| --- | --- | --- |
| New task modal | `desktop/src/new_task_modal.rs`, `desktop/src/app.rs` | Views, Base Components, Platform Integrations |
| Add agent modal | `desktop/src/add_agent_modal.rs` | Views, Base Components |
| Create branch modal | `desktop/src/create_branch_modal.rs` | Views, Base Components |
| Custom action modal | `desktop/src/custom_actions_modal.rs` | Views, Base Components |
| Pinned tab close confirmation | `desktop/src/app.rs` | Views, Terminal Production Readiness |
| Titlebar dropdowns and terminal tab menus | `desktop/src/titlebar.rs`, `desktop/src/app.rs` | Views, Base Components |
| Toast stack and copy-details error surface | `desktop/src/app.rs`, `desktop/src/panels.rs` | Views, Base Components |

## State Ownership

- Views own section composition, selected item state, loading/empty/error placement, and modal/menu routing.
- Base Components own reusable control props, labels, hover/active/focus/disabled/error states, and token plumbing.
- Style owns semantic colors, typography, spacing, density, radius, shadows, and appearance modes.
- Layout owns shell regions, breakpoints, drawer behavior, and mobile/desktop geometry differences.
- Platform Integrations own build/runtime profile differences, system appearance, file/open-in hooks, notifications, permissions, and pairing.
- Terminal Production Readiness owns terminal grid, input modes, resize flow, selection, links, copy, focus, mouse, and throughput evidence.

## Explicit Slint Gaps

- Full settings, MCP, pairing, project page, and right-inspector data are not yet implemented in `slint-poc`; their view contracts must be added before parity is claimed.
- Terminal selection/link/copy/focus/mouse and throughput gates remain separate under `another-one-4vk`.
- Pixel fidelity still requires GPUI and Slint capture artifacts under the visual-fidelity protocol.
