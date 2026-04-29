# Slint Overlays Port Review

Source of truth: `desktop/src/new_task_modal.rs`, `desktop/src/custom_actions_modal.rs`, `desktop/src/resource_indicator.rs`, `desktop/src/titlebar.rs`, `desktop/src/left_sidebar.rs`, and `desktop/src/app.rs`.

## Source Inventory

- New Task modal is owned by `AnotherOneApp::new_task_modal_overlay`; state lives in `NewTaskModalState` and submission routes through `submit_new_task_modal`.
- Add/Edit Action modal is owned by `AnotherOneApp::custom_action_modal_overlay`; state lives in `CustomActionModalState` and submission routes through `submit_custom_action_modal`.
- Resource usage popover is owned by `AnotherOneApp::resource_indicator_overlay`; data comes from `resource_usage` snapshots and the panel has its own refresh action.
- Titlebar menus are owned by `titlebar_custom_actions_overlay`, `titlebar_open_in_overlay`, and `titlebar_git_actions_overlay`; their open flags are mutually exclusive with the other titlebar dropdowns.
- Sidebar project and task menus are owned by `project_menu_overlay` and `sidebar_task_menu_overlay`; they are anchored to row geometry and close through outside-click handlers.
- Toast rendering and user-facing notification routing are owned by `show_*_toast` helpers and `toast_layer` in `desktop/src/app.rs`.

## Section Relationships

- Modals are full-window transient overlays: a scrim owns outside-click dismissal, and the modal card stops propagation so inner controls do not dismiss it.
- Popovers and menus are anchored overlays, not layout sections. They float above the shell and preserve the underlying page state.
- Titlebar menus are siblings. Opening Actions closes Open In/Git menus, and opening Open In closes Actions/Git menus.
- Sidebar menus are row-scoped and separate from row activation. Project menu actions operate on project ids; task menu actions operate on task/worktree ids.
- Toasts are a top-level overlay layer and do not participate in outside-click dismissal.

## Data Ownership And Activation

- GPUI modal form state is app-owned and short-lived. Slint mirrors that with `new_task_*` and `action_*` in-out properties until daemon-backed action persistence is ported.
- New Task submission remains Rust-owned through `submit_new_task(name, branch, project_id)` so the Slint shell keeps existing daemon behavior.
- Menu open state is shell-owned because menu visibility is presentation state. Menu item effects either activate existing callbacks (`project_selected`, `task_selected`) or route through the toast surface for current-slice placeholders.
- Resource popover display consumes the existing `resource_summary` property. Full daemon process-tree data remains a later integration point.
- User-facing failures and confirmations should route through `AoToast` or a modal confirmation surface, matching the GPUI `show_*_toast` contract.

## Behavior States

- New Task modal: closed, open, invalid empty task name, outside-click dismiss, close-button dismiss, cancel dismiss, submit.
- Add/Edit Action modal: closed, add mode, edit mode, invalid empty action name, outside-click dismiss, close/cancel dismiss, save.
- Resource popover: closed, open, refresh requested, empty/loaded terminal-session summary.
- Titlebar menus: closed, Actions open, Open In open, item selected, outside-click dismiss.
- Sidebar menus: closed, project menu open, task menu open, row target changes, item selected, outside-click dismiss.
- Toast layer: hidden, info/success/warning/error visible, copy requested, dismiss requested.

## Slint Mapping

- `slint-poc/ui/overlays.slint` introduces reusable overlay components: `AoMenuPopover`, `AoResourcePopover`, `AoNewTaskModal`, and `AoActionModal`.
- `AoMenuPopover` wraps `AoMenu` with full-window outside-click dismissal and anchor geometry so titlebar/sidebar menus share one contract.
- `AoNewTaskModal` and `AoActionModal` preserve the GPUI scrim/card/close/cancel/submit structure while keeping business effects in parent callbacks.
- `AoResourcePopover` preserves GPUI card hierarchy and refresh affordance while consuming the existing Slint resource summary.
- `slint-poc/src/overlays.rs` records source-backed overlay contracts and tests GPUI symbol coverage, outside-click requirements, and keyboard behavior expectations.

## Verification Gate

- Run `cargo fmt -p slint-poc`.
- Run `cargo check -p slint-poc`.
- Run `cargo test -p slint-poc --lib`.
- Keep `another-one-dn3.10` open after this slice: Create Branch, Pair/daemon surfaces, full Git menu behavior, persisted custom actions, keyboard Escape/Enter focus handling in Slint, and capture parity remain integration work.
