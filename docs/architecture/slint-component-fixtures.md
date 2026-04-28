# Slint Component Screenshot Fixtures

Component fixtures are the review surface for base components before full views compose them. These fixtures must be captured for both the GPUI baseline and the Slint implementation when compositor capture is available.

## Fixture Set

| Fixture | Required states | GPUI reference source | Slint target |
| --- | --- | --- | --- |
| `button-row` | normal, hover, active, focus, disabled, destructive | `desktop/src/titlebar.rs`, `desktop/src/settings_page.rs` | `AoButton`, future `AoIconButton`, future `AoSplitButton` |
| `sidebar-project-row` | normal, hover, active, menu-hover, add-hover, GitHub-hover | `desktop/src/left_sidebar.rs` | `AoProjectRow` |
| `sidebar-task-row` | normal, hover, active, pinned, running, rename, delete-confirm | `desktop/src/left_sidebar.rs` | `AoTaskRow` |
| `terminal-tabs` | active, inactive, pinned, running, failed restore, launching | `desktop/src/app.rs`, `desktop/src/titlebar.rs` | `AoTabChip` |
| `modal-new-task` | empty, focused field, validation error, create disabled/enabled | new-task modal paths in `desktop/src/app.rs` | `AoModalCard`, `AoTextInput`, `AoButton` |
| `right-sidebar-cards` | empty, loading, changed files, commits, checks failure | `desktop/src/right_sidebar.rs` | `AoCard`, future `AoStateCard` |
| `toast-stack` | success, warning, error with copy, info, drag offset | `desktop/src/app.rs` | future `AoToast` |
| `settings-inputs` | agent args, script editor, keybinding row, focus ring | `desktop/src/settings_page.rs` | `AoTextInput`, future menu/shortcut components |

## Capture Requirements

- Capture dark mode first because GPUI dark is the current product baseline.
- Capture light mode after the Style epic defines explicit light tokens.
- Store screenshots under a deterministic visual artifact path once the harness exists.
- Include crop metadata: component name, state, platform, scale factor, viewport size, source commit.
- Pixel diffs follow `docs/architecture/slint-visual-fidelity-gate.md`.

## Manual Review Until Capture Is Unblocked

When compositor capture is unavailable, reviewers must record:

- The exact source file and state used as the GPUI reference.
- The Slint component and props used to approximate that state.
- A yes/no note for geometry, color, typography, and interaction behavior.
- Any deviation as a bd task or explicit decision before accepting the component.
