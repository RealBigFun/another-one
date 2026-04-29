# Slint Sidebar Port Review

Source of truth: `desktop/src/left_sidebar.rs`.

## Section Relationships

- The left sidebar is one `PROJECTS` tree, not separate global project and task sections.
- `sidebar_groups()` groups all `Project` records by `sidebar_group_key()`, which prefers `repo_common_dir` and falls back to `project:<id>`.
- Each group has one `root_project` row. The root is the non-worktree project when present, otherwise the first project in that repo group.
- A group's child rows are `SidebarTaskEntry` values derived from tasks under the root project, resolved through `task_launcher::task_workspace_target()` to the target project/path/branch.
- Child task/worktree rows belong under their root project and render only when that project group is expanded.
- Pinned tasks sort before unpinned tasks within their project group only; there is no global task ordering across projects.

## Project Row Behavior

- Left-click opens the project overview page, clears task/project menus, and commits any pending sidebar rename.
- Chevron click expands/collapses that project group without opening the project page.
- The ellipsis opens the project menu for that root project.
- The GitHub icon appears only when a GitHub URL exists for the root project.
- The plus icon opens the New Task modal scoped to that root project.
- Active styling reflects the active project page, not the active terminal task.

## Task / Worktree Row Behavior

- A child row opens a terminal section for that task/worktree.
- Double-click begins inline task rename; Enter commits and Escape cancels.
- Right-click opens the task menu.
- Delete is a row affordance and worktree tasks require confirmation before deleting the worktree/branch.
- Task metadata is row-local: branch name when it differs from the task name, last commit relative time when enabled, and diff counts when available.
- Active styling reflects the active terminal section task id.
- Worktree rows include the split/git indicator; pinned rows include the pin indicator.

## Asset And Color Contract

- Project disclosure uses `desktop/assets/icons/icons__chevron-down.svg` and `icons__chevron-right.svg`.
- Project row actions use `icons__ellipsis.svg`, `icons__github.svg` when a daemon GitHub URL exists, and `icons__plus.svg`.
- Task/worktree rows use `icons__git-split.svg` for worktrees, `icons__pin-off.svg` for pinned tasks, `icons__trash.svg` for delete, and `icons__pull-request.svg` when PR state is available.
- GPUI sidebar row hover is white at `0.06` alpha. Active row fill is white at `0.03` alpha and active border is white at `0.18` alpha; these are sidebar-specific and must not be replaced by the generic `overlay_active` token.
- GPUI sidebar icon color is `hsla(0., 0., 0.55, 1.)`; Slint should colorize SVG assets from the shared desktop asset directory with the matching resolved theme color.

## Slint Port Rules

- Slint must render `PROJECTS` as a grouped tree: project row, then zero or more nested task rows for that project.
- Slint must not render a separate global `OPEN TASKS` section for desktop parity.
- The daemon-backed Slint model should emit a flattened sidebar tree with `kind = "project" | "task"`, `group-id`, `project-id`, `task-id`, `row-y`, `row-height`, and row indent implied by kind.
- Project click and task click must remain separate callbacks because they activate different GPUI concepts.
- The production capture for this bead must show nested task rows under the active project group and no global task section.

## Open Parity Gaps After This Pass

- Slint daemon data does not expose repo-common-dir/worktree-name, so grouping uses the daemon project id as the group key until the protocol carries repo grouping metadata.
- Slint does not yet implement inline rename or project/task context menus; row callbacks and visual affordances exist, but full flows remain in modal/menu parity beads.
- Slint does not yet expose GitHub URL or PR state per row; those require right-sidebar/git protocol integration.
