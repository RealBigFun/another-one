# Slint Component State Matrix

This matrix defines the GPUI state surface each Slint base component must cover before the component can be treated as production-ready. Source references are GPUI source files until screenshot crops are available through the visual-fidelity gate.

| Component | Normal | Hover | Active/selected | Focus | Disabled | Loading | Error/destructive | GPUI source |
| --- | --- | --- | --- | --- | --- | --- | --- | --- |
| `AoButton` | implemented | implemented | implemented | pending | pending | pending | pending | `desktop/src/titlebar.rs`, `desktop/src/settings_page.rs` |
| `AoStatusPill` | implemented | n/a | semantic palette pending | n/a | n/a | pending | pending | `desktop/src/titlebar.rs`, `desktop/src/app.rs` |
| `AoSectionLabel` | implemented | n/a | n/a | n/a | n/a | n/a | n/a | `desktop/src/left_sidebar.rs`, `desktop/src/right_sidebar.rs` |
| `AoAvatar` | implemented | inherited from parent row | inherited from parent row | inherited from parent row | n/a | n/a | n/a | `desktop/src/left_sidebar.rs` |
| `AoProjectRow` | implemented | implemented | implemented | pending | n/a | pending | pending | `desktop/src/left_sidebar.rs` |
| `AoTaskRow` | implemented | implemented | implemented | pending | n/a | pending | pending | `desktop/src/left_sidebar.rs` |
| `AoTabChip` | implemented | implemented | implemented | pending | n/a | pending | failed restore pending | `desktop/src/app.rs`, `desktop/src/titlebar.rs` |
| `AoCard` | implemented | n/a | n/a | n/a | n/a | pending | pending | `desktop/src/right_sidebar.rs`, `desktop/src/project_page.rs` |
| `AoTextInput` | implemented | n/a | n/a | partial through `LineEdit` | pending | n/a | pending | `desktop/src/settings_page.rs`, `desktop/src/create_branch_modal.rs` |
| `AoModalCard` | implemented | n/a | n/a | pending modal focus trap | n/a | pending | pending | `desktop/src/create_branch_modal.rs` |
| `AoToast` | missing | drag hover pending | n/a | copy button focus pending | n/a | n/a | missing | `desktop/src/app.rs` |
| `AoTooltip` | missing | n/a | n/a | n/a | n/a | n/a | n/a | `desktop/src/app.rs` |
| `AoMenuItem` | missing | missing | selected pending | pending | missing | n/a | missing | `desktop/src/titlebar.rs`, `desktop/src/left_sidebar.rs` |

## Acceptance Notes

- Implemented means the component exposes the state in Slint code today.
- Pending means the state exists in the GPUI source and must be added before final component readiness.
- Missing means no Slint component exists yet.
- Source-only evidence is not a substitute for pixel evidence. The visual-fidelity gate must still attach GPUI and Slint crops before final close.

## Required Fixture States

- Button: normal, hover, active, focus, disabled, destructive.
- Sidebar row: normal, hover, active, pinned, running, editing, delete-confirm.
- Tab chip: normal, hover, active, pinned, running, failed restore, launching.
- Text input: empty, filled, focused, selected text, validation error, disabled.
- Modal: centered desktop, mobile full-width card, scrim, close hover, primary action active.
- Toast: success, error with copy details, warning, info, dragged-dismiss threshold.
- Right card/state card: empty, loading, failed, populated.
