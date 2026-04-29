# Slint Port Review Checklist

Every GPUI feature port must complete this review before Slint implementation changes are made or closed in bd.

## Required Review Sections

1. Source inventory

- List the GPUI source files and functions that own the feature.
- Identify supporting store/model/protocol types used by those functions.
- Note captured GPUI reference images, if available.
- List GPUI icon/image/font assets used by the feature and whether Slint reuses the exact asset or documents a protocol/data blocker.

2. Section relationship model

- Define each visible section and which parent section owns it.
- Define whether sections are siblings, nested children, overlays, or mutually-exclusive modes.
- Define which section owns scrolling, clipping, selection, and keyboard focus.

3. Data ownership and activation

- Identify which model/store/protocol object owns the data.
- Identify row/group keys and how identity is preserved across refreshes.
- Define what click, double-click, right-click, keyboard, and hover actions activate.
- Distinguish visually-active state from data-active state when GPUI does.

4. Behavior/state matrix

- List normal, hover, focus, active, loading, empty, error, editing, destructive, disabled, and menu-open states that exist in GPUI.
- Record which states must be implemented now and which are explicitly deferred to another bd bead.
- Record how user-facing failures route to toast.
- Record source GPUI color literals/tokens for those states; do not substitute generic app colors when GPUI uses feature-specific values.

5. Slint mapping

- Map GPUI sections to Slint structs/components/callbacks.
- Record any daemon protocol gaps or temporary fallback behavior.
- Record what must not be invented or simplified from the GPUI baseline.

6. Verification gate

- Add or update deterministic fixtures where possible.
- Capture before closing the bead.
- Run `cargo test -p slint-poc --lib` unless the bead is docs-only.
- Close the bead only when the review artifact, implementation, and verification all agree.

## Required bd Discipline

- Add a bd note with the review artifact path before implementation.
- If review invalidates previous work, reopen the bead and say exactly what relationship was wrong.
- Each bead commit must include its review artifact or a note pointing at an already-current artifact.
