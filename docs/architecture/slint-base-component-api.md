# Slint Base Component API Catalog

The Slint component layer lives in `slint-poc/ui/components.slint`. This catalog is the public contract views may compose against. It is intentionally smaller than a generic widget kit; every entry maps back to a GPUI facet in `docs/architecture/gpui-base-component-inventory.md`.

## Implemented Components

### `AoButton`

Props: `label`, `control-label`, `active`, `disabled`, `loading`, `destructive`, `bg`, `hover-bg`, `border`, `focus-ring`, `text-color`, `danger-color`.

Events: `clicked()`.

States: normal, hover, pressed, focused, active, disabled, loading, destructive. Focused buttons activate from Space/Enter-equivalent text events.

Baseline use: titlebar buttons, footer buttons, sidebar add buttons, modal primary action.

### `AoIconButton`

Props: `icon`, required `control-label`, `active`, `disabled`, `bg`, `hover-bg`, `border`, `focus-ring`, `icon-color`.

Events: `clicked()`.

States: normal, hover, pressed, focused, active, disabled.

Baseline use: titlebar drawer/close controls, footer icon-only controls, terminal tab add, modal close.

### `AoSplitButton`

Props: `label`, `control-label`, `menu-control-label`, `active`, `disabled`, `bg`, `hover-bg`, `border`, `focus-ring`, `text-color`.

Events: `clicked()`, `menu-clicked()`.

States: normal, primary hover, menu hover, pressed, focused primary, focused menu, active, disabled, separate primary/menu hit regions.

Baseline use: titlebar Actions/Open-In style split controls.

### `AoCheckbox`

Props: `checked`, `label`, `control-label`, `disabled`, `error`, `bg`, `hover-bg`, `border`, `focus-ring`, `text-color`, `danger-color`.

Events: `toggled(bool checked)`.

States: checked, unchecked, hover, focused, disabled, validation error.

Baseline use: settings toggles, modal options, confirmation rows.

### `AoSegmentedControl`

Props: `entries`, `bg`, `hover-bg`, `border`, `focus-ring`, `text-color`, `muted-color`.

Events: `selected(string id)`.

States: normal, hover, focused segment, selected segment, disabled segment.

Baseline use: appearance selection, settings scope switches, inspector tab-style controls where a full tab strip is too heavy.

### `AoStatusPill`

Props: `label`, `kind`, `bg`, `border`, `text-color`, `success-color`, `warning-color`, `danger-color`.

Events: none.

States: neutral, success, warning, danger.

Baseline use: branch pill, resource indicator, PR/check status, footer metadata.

### `AoResourceIndicator`

Props: `label`, `bg`, `border`, `text-color`, `warning-color`, `danger-color`, `warning`, `danger`.

Events: none.

States: neutral, warning, danger.

Baseline use: app-window CPU/RSS indicator and future process resource rows.

### `AoSectionLabel`

Props: `label-color`.

Events: none.

States: static section label.

Baseline use: sidebar section headings, inspector headings, settings nav labels.

### `AoAvatar`

Props: `label`, `accent`, `text-color`.

Events: none.

States: static avatar. Future variants should cover selected/focused row context through parent rows, not this primitive.

Baseline use: project and task initials chips.

### `AoProjectRow`

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `text-secondary`, `focus-ring`, `danger-color`, `warning-color`.

Events: `clicked(string project_id)`, `menu-clicked(string project_id)`, `add-clicked(string project_id)`, `github-clicked(string project_id)`.

States: normal, hover, focused, active, expanded/collapsed disclosure, loading, error, add-hover, GitHub-hover, menu-hover.

Baseline use: component-state fixtures and expanded project cards.

### `AoSidebarProjectRow`

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `focus-ring`, `danger-color`, `warning-color`.

Events: `clicked(string project_id)`, `menu-clicked(string project_id)`, `add-clicked(string project_id)`, `github-clicked(string project_id)`.

States: normal, hover, focused, active, expanded/collapsed disclosure, loading, error, project menu, GitHub, and add-task affordances.

Baseline use: legacy compact sidebar row fixture. Production navigation uses `AoSidebarProjectTreeRow` so project rows own nested task/worktree rows.

### `AoSidebarProjectTreeRow`

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `focus-ring`, `danger-color`, `warning-color`.

Events: `clicked(string project_id)`, `menu-clicked(string project_id)`, `add-clicked(string project_id)`.

States: normal, hover, focused, active when the project overview is active, expanded/collapsed disclosure, loading, error, project menu, and add-task affordance. GitHub affordance is intentionally absent until daemon data exposes a repository URL.

Baseline use: production left sidebar root project rows from `desktop/src/left_sidebar.rs`.

### `AoTaskRow`

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `text-secondary`, `success-color`, `row-border-color`, `focus-ring`, `danger-color`, `warning-color`.

Events: `clicked(string task_id)`, `menu-clicked(string task_id)`, `rename-clicked(string task_id)`, `delete-clicked(string task_id)`.

States: normal, hover, focused, active, pinned, running, loading, error, editing, delete-confirm, rename-hover, delete-hover, menu-hover.

Baseline use: component-state fixtures and expanded task cards.

### `AoSidebarTaskRow`

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `text-secondary`, `success-color`, `row-border-color`, `focus-ring`, `danger-color`, `warning-color`.

Events: `clicked(string task_id)`, `menu-clicked(string task_id)`, `rename-clicked(string task_id)`, `delete-clicked(string task_id)`.

States: normal, hover, focused, active, pinned, running, loading, error, editing, delete-confirm, rename, and menu affordances.

Baseline use: legacy compact sidebar row fixture. Production navigation uses `AoSidebarTaskTreeRow` as children of project rows.

### `AoSidebarTaskTreeRow`

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `text-secondary`, `success-color`, `row-border-color`, `focus-ring`, `danger-color`, `warning-color`.

Events: `clicked(string task_id)`, `menu-clicked(string task_id)`, `rename-clicked(string task_id)`, `delete-clicked(string task_id)`.

States: normal, hover, focused, active terminal task, pinned, running, loading, error, editing, delete-confirm, rename, and menu affordances.

Baseline use: production left sidebar task/worktree child rows from `desktop/src/left_sidebar.rs`.

### `AoTabChip`

Props: `tab`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `success-color`.

Events: `clicked(string tab_id)`.

States: normal, hover, active, pinned, running. Close, failed restore, launching, and disconnected states remain follow-up work under terminal readiness.

Baseline use: terminal tab strip.

### `AoCard`

Props: `title`, `body`, `card-bg`, `card-border-color`, `text-primary`, `text-muted`.

Events: none.

States: static card. Loading/error/empty variants remain follow-up work.

Baseline use: right sidebar and settings cards.

### `AoStateCard`

Props: `title`, `body`, `state`, `action-label`, `card-bg`, `card-border-color`, `text-primary`, `text-muted`, `success-color`, `warning-color`, `danger-color`.

Events: `action-clicked()`.

States: populated, empty, loading, error.

Baseline use: right sidebar empty/loading/error cards, project page section cards, settings status cards.

### `AoTextInput`

Props: `placeholder-text`, `text`, `field-bg`, `field-border`, `focus-border`, `error-color`, `error`, `disabled`.

Events: inherited text editing through `LineEdit`.

States: empty, filled, focused, validation error, disabled.

Baseline use: new-task/create-branch/settings text inputs.

### `AoModalCard`

Props: `card-bg`, `card-border-color`.

Events: none.

States: static modal surface. Escape/click-outside/close action is owned by the parent view.

Baseline use: new-task, create-branch, add-agent, confirm dialogs.

### `AoMenuItem`

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `danger-color`, `focus-ring`.

Events: `clicked(string id)`.

States: normal, hover, focused, selected, disabled, destructive, shortcut label.

Baseline use: titlebar dropdown items, task/project menus, terminal tab menus.

### `AoMenu`

Props: `entries`, `card-bg`, `menu-border-color`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `danger-color`.

Events: `item-clicked(string id)`.

States: static menu container; item states live in `AoMenuItem`.

Baseline use: titlebar/menu popovers and action menus.

### `AoTooltip`

Props: `message`, `card-bg`, `tooltip-border-color`, `text-color`.

Events: none.

States: static tooltip surface; anchor ownership and show/hide timing stay with the parent view.

Baseline use: labels for icon-only controls, destructive actions, and disabled/validation explanation.

### `AoToast`

Props: `kind`, `message`, `detail`, `card-bg`, `toast-border-color`, `text-primary`, `text-muted`, `success-color`, `warning-color`, `danger-color`.

Events: `dismissed()`, `copy-requested()`.

States: info, success, warning, error with details/copy affordance.

Baseline use: user-facing errors/notifications; Slint views must route errors through this path.

## Required Next API Additions

- `AoShortcutRow`: settings keybinding capture/display.
- `AoTerminalTabClose`: close/error restore affordance once terminal close semantics are wired.

## Non-Goals

- The component layer must not own daemon calls.
- The component layer must not decide global breakpoints.
- The component layer must not hardcode business copy beyond generic fixture/default labels.
- The component layer must not accept visual drift without a linked Style/visual-fidelity decision.
