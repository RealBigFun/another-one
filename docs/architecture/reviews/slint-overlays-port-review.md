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
- Menus are full-window transient overlays with an anchored panel: the full overlay owns outside-click dismissal and the panel stops propagation.
- Resource usage is an anchored popover, not a layout section. In GPUI it is toggled by the resource indicator, refreshes on open, and the panel itself stops propagation; it does not have a separate outside-click scrim in `resource_indicator_overlay`.
- Titlebar menus are siblings. Opening Actions closes Open In/Git menus, and opening Open In closes Actions/Git menus.
- Sidebar menus are row-scoped and separate from row activation. Project menu actions operate on project ids; task menu actions operate on task/worktree ids.
- Toasts are a top-level overlay layer and do not participate in outside-click dismissal.

## Data Ownership And Activation

- GPUI modal form state is app-owned and short-lived. Slint mirrors that with `new_task_*` and `action_*` in-out properties until daemon-backed action persistence is ported.
- New Task submission remains Rust-owned through `submit_new_task(name, branch, project_id)` so the Slint shell keeps existing daemon behavior.
- Menu open state is shell-owned because menu visibility is presentation state. Menu item effects either activate existing callbacks (`project_selected`, `task_selected`) or route through the toast surface for current-slice placeholders.
- Resource popover display consumes the existing `resource_summary` property today, with component-level slots for app CPU, app memory, session count, terminal-session state, and terminal-session summary. Full daemon process-tree data remains a later integration point.
- User-facing failures and confirmations should route through `AoToast` or a modal confirmation surface, matching the GPUI `show_*_toast` contract.
- Toast routing contract: overlay components may emit requests or set the app toast properties, but they must not render ad hoc inline notifications. The app-level equivalent remains GPUI's `show_success_toast`, `show_error_toast`, `show_warning_toast`, `show_info_toast`, and `toast_layer`.

## Behavior States

- New Task modal: closed, open, invalid empty task name with visible status, submitting status, outside-click dismiss, close-button dismiss, cancel dismiss, Enter submit in GPUI.
- Add/Edit Action modal: closed, add mode, edit mode, invalid empty action name, invalid empty shell command/agent prompt, outside-click dismiss, close/cancel dismiss, Command+Enter submit in GPUI, Escape closes an open dropdown before dismissing the modal.
- Resource popover: closed, open, refresh requested, app CPU/memory/session stat cards, empty/loading/error/populated terminal-session summary, project/task/session tree data pending daemon model wiring.
- Titlebar menus: closed, Actions open, Open In open, item selected, outside-click dismiss.
- Sidebar menus: closed, project menu open, task menu open, row target changes, item selected, outside-click dismiss.
- Toast layer: hidden, info/success/warning/error visible, copy requested, dismiss requested.

## Slint Mapping

- `slint-poc/ui/overlays.slint` introduces reusable overlay components: `AoMenuPopover`, `AoResourcePopover`, `AoNewTaskModal`, and `AoActionModal`.
- `AoMenuPopover` wraps `AoMenu` with full-window outside-click dismissal and anchor geometry so titlebar/sidebar menus share one contract. The `dismisses-on-outside-click` property keeps the dismissal contract explicit for future non-scrim uses.
- `AoNewTaskModal` and `AoActionModal` preserve the GPUI scrim/card/close/cancel/submit structure while keeping business effects in parent callbacks. This slice adds visible validation/status text and disabled submit states for required GPUI fields.
- `AoResourcePopover` now separates app stat cards from terminal-session status, exposes refresh affordances from both the header and session status card, and keeps process-tree wiring as a later app-data integration.
- `slint-poc/src/overlays.rs` records source-backed overlay contracts and tests GPUI symbol coverage, outside-click requirements, inner-click propagation, modal/menu keyboard behavior, resource semantics, menu groups, and toast routing.

## Interaction Contract

- Outside click: GPUI modals and menu overlays dismiss on outside click; resource popover and toast layer do not own outside-click dismissal. Current Slint app-level resource wrapping still closes on outside click and remains an integration gap outside this internals slice.
- Escape: GPUI New Task dismisses; Add/Edit Action closes an open dropdown first, then dismisses; titlebar and sidebar menus dismiss. The Slint internals record this contract, but global Escape wiring remains app-level work.
- Enter: GPUI New Task submits on Enter unless the branch filter is focused; Add/Edit Action saves on platform Enter; menus activate focused items. Slint component buttons expose Enter/Space activation, but global modal/menu Enter routing remains app-level work.
- Toasts: overlay validation, placeholder actions, daemon errors, and copy feedback should route through the app toast path. Inline status text is limited to local modal validation/submitting state.

## Verification Gate

- Run `cargo fmt -p slint-poc`.
- Run `cargo check -p slint-poc`.
- Run `cargo test -p slint-poc --lib`.
- Keep `another-one-dn3.10` open after this slice: Create Branch, Pair/daemon surfaces, full Git menu behavior, persisted custom actions, app-level keyboard Escape/Enter focus handling in Slint, resource process-tree wiring, current Slint resource outside-click mismatch, and capture parity remain integration work.
- Keep `another-one-y4n.8` open after this slice: reusable tooltip API adoption, full toast stack behavior, copy feedback animation, resource tree rows, status badges beyond current cards, and fixture/capture parity remain integration work.
