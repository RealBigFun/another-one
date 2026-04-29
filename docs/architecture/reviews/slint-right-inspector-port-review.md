# Slint Right Inspector Port Review

Source of truth: `desktop/src/right_sidebar.rs`, `desktop/src/app.rs`, `desktop/src/daemon_host.rs`, and `daemon-sandbox/src/frame.rs`.

## Source Inventory

- GPUI rendering owner is `AnotherOneApp::changed_files_panel` in `desktop/src/right_sidebar.rs`.
- GPUI support functions define changed-file grouping, row snapshots, git toolbar buttons, commit rows, check rows, compare rows, and discard confirmation overlay.
- GPUI app state lives in `AnotherOneApp`: `right_sidebar_mode`, `changed_files`, `changed_files_list_snapshots`, commit/check caches, pending git mutations, and discard confirmation state.
- Daemon wire controls already exist for right-inspector data and mutations: `ReadChangedFiles`, `ReadRecentCommits`, `ReadPullRequestChecks`, `StageChangedFile`, `UnstageChangedFile`, `StageAllChanges`, `UnstageAllChanges`, `DiscardChangedFile`, and `DiscardAllChanges`.
- Captured GPUI evidence exists at `docs/reference/gpui-baseline/current/captures/right-sidebar/changes.png`; additional Commits/Checks captures are still needed.

## Section Relationships

- The right inspector is a sibling of the workspace center and is owned by the shell layout, not by the terminal renderer.
- The inspector has one top toolbar and one mutually-exclusive mode body: Changes, Commits, Checks, and Compare when a compare target exists.
- Changes mode owns the scroll region and renders section headers plus rows. `Staged Changes` and `Changes` are sibling groups under one scroll container.
- Section headers own group collapse and group-level actions. File rows own per-file actions and are children of their section.
- Destructive discard confirmation is an overlay owned by the right inspector, not by a row.

## Data Ownership And Activation

- Active project identity comes from the active workspace section's `project_id`. Slint must use the daemon-projected task target project when available, not invent an inspector project.
- `ChangedFileWire` identity is `path + original_path`; rows are duplicated into staged and unstaged groups when one file has both kinds of changes.
- Top toolbar clicks change `right_inspector_mode` locally and request mode-specific daemon data.
- Stage/unstage actions send daemon git mutation controls and consume inline changed-file ack snapshots.
- User-facing daemon failures route through `AoToast`.

## Behavior And State Matrix

- Toolbar states: normal, hover, active, disabled, and loading action feedback.
- Changes states: loading, clean, dirty, unknown project/unavailable, daemon error, staged group, unstaged group, destructive discard confirmation, per-file pending, and group-level pending.
- Current slice implements the real toolbar, Changes loading/clean/dirty/error/unavailable states, file/group action callbacks, destructive discard confirmation, and daemon snapshot refresh.
- Deferred to later loops: commit row expansion/file-change details, full Checks rows, Compare mode, collapse persistence, pending action spinners, and capture parity.
- Feature-specific GPUI colors: inspector background `chrome_bg`, toolbar active background `#262a30`, hover white `0.06`, section divider white `0.06`, file row hover white `0.04`, title `hsla(0,0,0.94,1)`, metadata `hsla(0,0,0.58,1)`, added green `hsla(138,0.50,0.74,1)`, removed red `hsla(352,0.52,0.76,1)`, status add green, delete red, rename/copy blue, modified yellow.

## Slint Mapping

- `RightInspectorRow` is a flattened view model with section and file rows, row identity, row geometry, diff counts, status glyph/color, and mutation capability flags.
- `AoInspectorModeButton`, `AoInspectorSectionRow`, and `AoInspectorFileRow` mirror GPUI toolbar/section/file roles while reusing GPUI SVG assets.
- `SlintClientEvent` owns right-inspector mode selection and changed-file mutation events.
- `WorkspaceShellModel` remains responsible for active project/task labels; right-inspector git data is separate because it updates independently from `ProjectList`.

## Verification Gate

- Add deterministic tests for changed-file partitioning and GPUI asset/color contract.
- Run `cargo check -p slint-poc`.
- Run `cargo test -p slint-poc --lib`.
- Do not close `another-one-dn3.8` until Commits, Checks, Compare, destructive confirm, and captures are done.
