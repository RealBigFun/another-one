# GPUI Base Component Inventory

This inventory freezes the GPUI component baseline that the Slint implementation must recreate. The source of truth is the current Rust desktop app, especially `desktop/src/app.rs`, `desktop/src/left_sidebar.rs`, `desktop/src/right_sidebar.rs`, `desktop/src/titlebar.rs`, `desktop/src/settings_page.rs`, `desktop/src/create_branch_modal.rs`, `desktop/src/tokens.rs`, and `desktop/src/layout.rs`.

## Component Targets

| GPUI facet | Source | Slint target | Notes |
| --- | --- | --- | --- |
| Titlebar chrome button | `desktop/src/titlebar.rs` | `AoButton` | Icon-only and text buttons must expose labels/tooltips, hover, active, disabled, and focus states. |
| Status/resource pill | `desktop/src/titlebar.rs`, `desktop/src/app.rs` | `AoStatusPill` | Used for branch, PR, resource, and app-status indicators. |
| Section label | `desktop/src/left_sidebar.rs`, `desktop/src/right_sidebar.rs`, `desktop/src/settings_page.rs` | `AoSectionLabel` | Uppercase section metadata with muted token. |
| Project avatar | `desktop/src/left_sidebar.rs` | `AoAvatar` | Initials/accent chip, deterministic per project/task. |
| Project sidebar row | `desktop/src/left_sidebar.rs` | `AoProjectRow` | Active row, hover state, project menu/GitHub/add affordances, metadata line. |
| Task sidebar row | `desktop/src/left_sidebar.rs` | `AoTaskRow` | Active/pinned/running states, rename/delete/menu affordances, branch metadata. |
| Terminal tab chip | `desktop/src/titlebar.rs`, `desktop/src/app.rs` | `AoTabChip` | Active/running/pinned state; close/pin/add-agent variants remain follow-up props. |
| Card/panel | `desktop/src/right_sidebar.rs`, `desktop/src/project_page.rs`, `desktop/src/settings_page.rs` | `AoCard` | Right-sidebar cards, settings panels, empty/error states. |
| Text input | `desktop/src/settings_page.rs`, `desktop/src/create_branch_modal.rs`, new-task modal | `AoTextInput` | Focus ring, cursor/selection, validation/error text, placeholder. |
| Modal card/scrim | `desktop/src/create_branch_modal.rs`, new-task modal | `AoModalCard` | Centered card, scrim, close action, keyboard escape, click swallowing. |
| Toast | `desktop/src/app.rs` | `AoToast` | Not implemented yet; must carry success/error/warning/info palettes and copy-details behavior. |
| Tooltip | `desktop/src/app.rs` `action_tooltip_view` callers | `AoTooltip` | Not implemented yet; required for icon-only controls and destructive actions. |
| Split/dropdown menu | `desktop/src/titlebar.rs`, `desktop/src/left_sidebar.rs` | `AoMenu`, `AoMenuItem` | Not implemented yet; required for Actions, project menu, task menu, settings rows. |
| Empty/loading/error state | `desktop/src/right_sidebar.rs`, `desktop/src/project_page.rs` | `AoStateCard` | Not implemented yet; should reuse `AoCard` surface and toast contract for errors. |

## Cross-Cutting Requirements

All interactive components must define:

- Label or tooltip text even when the rendered control is icon-only.
- Normal, hover, active/pressed, selected, disabled, focus, loading, and error states where the GPUI source uses them.
- Token-only styling. Components must not embed arbitrary color literals except where the GPUI baseline does and the Style epic has accepted the literal as a semantic role.
- A narrow event contract. Components emit IDs or explicit actions; view files own routing, daemon calls, and toast decisions.
- View-specific one-offs must stay in view files until two real GPUI facets share the behavior.

## Current Slint Coverage

`slint-poc/ui/components.slint` currently provides the first reusable surface:

- `AoButton`
- `AoStatusPill`
- `AoSectionLabel`
- `AoAvatar`
- `AoProjectRow`
- `AoTaskRow`
- `AoTabChip`
- `AoCard`
- `AoTextInput`
- `AoModalCard`

Missing from implementation:

- `AoToast`
- `AoTooltip`
- menu/split-button primitives
- explicit disabled/loading/error states
- screenshot fixture harness
- visual reference crops against GPUI
