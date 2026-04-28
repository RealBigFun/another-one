# Slint Component State Matrix

This matrix defines the GPUI state surface each Slint base component must cover before the component can be treated as production-ready. Source references are GPUI source files until screenshot crops are available through the visual-fidelity gate.

| Component | Normal | Hover | Active/selected | Focus | Disabled | Loading | Error/destructive | GPUI source |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `AoButton` | implemented | implemented | implemented | implemented focus ring + keyboard activation | implemented | implemented | implemented destructive | `desktop/src/titlebar.rs`, `desktop/src/settings_page.rs` |
| `AoIconButton` | implemented | implemented | implemented | implemented focus ring + keyboard activation | implemented | n/a | n/a | `desktop/src/titlebar.rs`, `desktop/src/left_sidebar.rs` |
| `AoSplitButton` | implemented | implemented primary/menu regions | implemented | implemented separate primary/menu focus rings + keyboard activation | implemented | n/a | n/a | `desktop/src/titlebar.rs` |
| `AoCheckbox` | implemented | implemented | implemented checked | implemented focus ring + keyboard toggle | implemented | n/a | implemented validation tint | `desktop/src/settings_page.rs` |
| `AoSegmentedControl` | implemented | implemented | implemented selected segment | implemented segment focus ring + keyboard activation | implemented per segment | n/a | n/a | `desktop/src/settings_page.rs`, `desktop/src/app.rs` |
| `AoStatusPill` | implemented | n/a | implemented semantic palette | n/a | n/a | n/a | implemented danger | `desktop/src/titlebar.rs`, `desktop/src/app.rs` |
| `AoResourceIndicator` | implemented | n/a | warning/danger implemented | n/a | n/a | n/a | danger implemented | `desktop/src/titlebar.rs`, `desktop/src/app.rs` |
| `AoSectionLabel` | implemented | n/a | n/a | n/a | n/a | n/a | n/a | `desktop/src/left_sidebar.rs`, `desktop/src/right_sidebar.rs` |
| `AoAvatar` | implemented | inherited from parent row | inherited from parent row | inherited from parent row | n/a | n/a | n/a | `desktop/src/left_sidebar.rs` |
| `AoProjectRow` | implemented | implemented | implemented | pending | n/a | pending | pending | `desktop/src/left_sidebar.rs` |
| `AoTaskRow` | implemented | implemented | implemented | pending | n/a | pending | pending | `desktop/src/left_sidebar.rs` |
| `AoTabChip` | implemented | implemented | implemented | pending | n/a | pending | failed restore pending | `desktop/src/app.rs`, `desktop/src/titlebar.rs` |
| `AoCard` | implemented | n/a | n/a | n/a | n/a | pending | pending | `desktop/src/right_sidebar.rs`, `desktop/src/project_page.rs` |
| `AoStateCard` | implemented | n/a | n/a | n/a | n/a | implemented | implemented | `desktop/src/right_sidebar.rs`, `desktop/src/project_page.rs` |
| `AoTextInput` | implemented | n/a | n/a | implemented via `LineEdit` border | implemented | n/a | implemented validation border | `desktop/src/settings_page.rs`, `desktop/src/create_branch_modal.rs` |
| `AoModalCard` | implemented | n/a | n/a | pending modal focus trap | n/a | pending | pending | `desktop/src/create_branch_modal.rs` |
| `AoToast` | implemented | copy/dismiss hover through icon buttons | n/a | implemented through icon-button focus rings | n/a | n/a | implemented error state | `desktop/src/app.rs` |
| `AoTooltip` | implemented | n/a | n/a | n/a | n/a | n/a | n/a | `desktop/src/app.rs` |
| `AoMenuItem` | implemented | implemented | implemented selected | implemented focus ring + keyboard activation | implemented | n/a | implemented destructive | `desktop/src/titlebar.rs`, `desktop/src/left_sidebar.rs` |
| `AoMenu` | implemented | item-owned | item-owned | item focus implemented; trap remains parent-view responsibility | item-owned | n/a | item-owned | `desktop/src/titlebar.rs`, `desktop/src/left_sidebar.rs` |

## Acceptance Notes

- Implemented means the component exposes the state in Slint code today.
- Pending means the state exists in the GPUI source and must be added before final component readiness.
- Missing means no Slint component exists yet.
- Source-only evidence is not a substitute for pixel evidence. The visual-fidelity gate must still attach GPUI and Slint crops before final close.

## Required Fixture States

- Button: normal, hover, active, focus, disabled, loading, destructive.
- Icon button: normal, hover, active, disabled, required label.
- Split button: primary hover, dropdown hover, active, disabled.
- Checkbox: checked, unchecked, focus, disabled, validation error.
- Segmented control: selected, hover, focus, disabled segment.
- Sidebar row: normal, hover, active, pinned, running, editing, delete-confirm.
- Tab chip: normal, hover, active, pinned, running, failed restore, launching.
- Text input: empty, filled, focused, selected text, validation error, disabled.
- Modal: centered desktop, mobile full-width card, scrim, close hover, primary action active.
- Toast: success, error with copy details, warning, info, dragged-dismiss threshold.
- Right card/state card: empty, loading, failed, populated.
- Menu: normal item, hover item, selected item, disabled item, destructive item, shortcut label.
