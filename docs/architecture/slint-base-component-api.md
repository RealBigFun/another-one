# Slint Base Component API Catalog

The Slint component layer lives in `slint-poc/ui/components.slint`. This catalog is the public contract views may compose against. It is intentionally smaller than a generic widget kit; every entry maps back to a GPUI facet in `docs/architecture/gpui-base-component-inventory.md`.

## Implemented Components

### `AoButton`

Props: `label`, `control-label`, `active`, `bg`, `hover-bg`, `border`, `text-color`.

Events: `clicked()`.

States: normal, hover, active. Disabled/loading/error are not implemented yet.

Baseline use: titlebar buttons, footer buttons, sidebar add buttons, modal primary action.

### `AoStatusPill`

Props: `label`, `bg`, `border`, `text-color`.

Events: none.

States: static display state. Future variants should cover warning/error/success/status semantics.

Baseline use: branch pill, resource indicator, PR/check status, footer metadata.

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

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `text-secondary`.

Events: `clicked(string project_id)`.

States: normal, hover, active. Project menu/GitHub/add affordances are not implemented yet.

Baseline use: left sidebar project rows.

### `AoTaskRow`

Props: `entry`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `text-secondary`, `success-color`, `row-border-color`.

Events: `clicked(string task_id)`.

States: normal, hover, active, pinned, running. Rename/delete/menu/error states are not implemented yet.

Baseline use: left sidebar task/worktree rows.

### `AoTabChip`

Props: `tab`, `overlay-hover`, `overlay-active`, `text-primary`, `text-muted`, `success-color`.

Events: `clicked(string tab_id)`.

States: normal, hover, active, pinned, running. Close, failed restore, launching, and disconnected states remain follow-up work.

Baseline use: terminal tab strip.

### `AoCard`

Props: `title`, `body`, `card-bg`, `card-border-color`, `text-primary`, `text-muted`.

Events: none.

States: static card. Loading/error/empty variants remain follow-up work.

Baseline use: right sidebar and settings cards.

### `AoTextInput`

Props: `placeholder-text`, `text`, `field-bg`, `field-border`.

Events: inherited text editing through `LineEdit`.

States: normal and focused through Slint `LineEdit`; explicit focus ring/error/disabled variants remain follow-up work.

Baseline use: new-task/create-branch/settings text inputs.

### `AoModalCard`

Props: `card-bg`, `card-border-color`.

Events: none.

States: static modal surface. Escape/click-outside/close action is owned by the parent view.

Baseline use: new-task, create-branch, add-agent, confirm dialogs.

## Required Next API Additions

- `AoToast`: `kind`, `message`, `copy-message`, `dismissed`, `copy-requested`.
- `AoTooltip`: `message`.
- `AoMenu` / `AoMenuItem`: selected, destructive, disabled, shortcut label.
- `AoStateCard`: empty/loading/error content.
- `AoIconButton`: explicit icon-only variant with required `control-label`.
- `AoSplitButton`: primary action plus dropdown menu.

## Non-Goals

- The component layer must not own daemon calls.
- The component layer must not decide global breakpoints.
- The component layer must not hardcode business copy beyond generic fixture/default labels.
- The component layer must not accept visual drift without a linked Style/visual-fidelity decision.
