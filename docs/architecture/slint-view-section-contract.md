# Slint View Section Contract

This contract maps GPUI view sections to Slint composition boundaries. Views consume `docs/architecture/slint-layout-contract.md` for geometry and `slint-poc/ui/components.slint` for reusable controls.

## Shell View

Sections:

- Titlebar: drawer toggle, active project/task labels, branch pill, Actions split button, Open In action, resource indicator, close button.
- Left navigation: project header, project rows, task header, New Task action, task rows.
- Workspace: tab strip, terminal status line, terminal grid, terminal cursor/background/link spans.
- Right inspector: section tabs, state cards for working tree/checks/project path.
- Footer: settings/create controls, branch/worktree labels, layout/theme/platform labels.

States:

- Empty project list renders seeded placeholders until daemon `ProjectList` arrives.
- Daemon errors route through `AoToast`.
- Mobile layout hides persistent sidebars and uses a terminal context card.

Base components:

- `AoIconButton`, `AoSplitButton`, `AoStatusPill`, `AoResourceIndicator`, `AoProjectRow`, `AoTaskRow`, `AoTabChip`, `AoStateCard`, `AoToast`.

## Terminal Workspace View

Sections:

- Tab strip: daemon-backed active/pinned/running tab chips.
- Status line: connection/attach/worker error text.
- Grid renderer: Slint text/background/cursor/link models derived from `alacritty_terminal`.
- Focus/input scope: keyboard input forwarded to the daemon protocol.

States:

- Waiting for ticket, dialing daemon, attached, worker error, daemon closed, task switch reattach.
- Cursor/link models exist; selection/copy/mouse/focus-reporting remain under terminal production readiness.

Base components:

- `AoTabChip`, `AoIconButton`, terminal span structs in `app.slint`.

## New Task Modal View

Sections:

- Scrim.
- Modal header with close action.
- Task name field with required validation.
- Source branch field with branch fallback to active task/project.
- Footer actions: cancel and create.

States:

- Create disabled when task name is empty.
- Submit sends `Control::SubmitNewTask` through the daemon protocol.
- `WorkerReply::SubmitNewTaskAck` attaches tab `0` and refreshes projects.
- `WorkerReply::Err` surfaces as `AoToast`.

Base components:

- `AoModalCard`, `AoTextInput`, `AoButton`, `AoIconButton`, `AoToast`.

## Right Inspector View

Sections:

- Inspector tab row.
- Working tree state card.
- Checks state card.
- Project path state card.

States:

- Populated, empty, loading, and error states are represented by `AoStateCard`; real git/checks data remains a follow-up view integration.

Base components:

- `AoButton`, `AoStateCard`.

## Deferred View Contracts

These GPUI surfaces are inventoried but not yet implemented in the active Slint shell:

- Project page: project metadata, branch defaults, pull requests, project actions.
- Settings: agents, Open In, updates/build identity, preferences, shortcuts.
- MCP page: server catalog, provider enable/configuration state.
- Pairing: QR/pairing code, allowlist reset.
- Add-agent/create-branch/custom-action modals.
- Terminal menus, tab close confirmation, link/copy selection overlays.

Deferred means not implemented yet, not removed from parity scope.
