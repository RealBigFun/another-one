# Spacing, Radii, Shadow

## Spacing scale

The codebase uses hand-tuned values rather than a strict geometric scale
(4, 8, 12, 16 or 2, 4, 8, 16). The set-in-use is `{4, 6, 8, 10, 12, 14,
16, 18, 20, 24}`. We expose each as a named constant so new code
doesn't invent fresh values.

| Token | px | Typical use |
|---|---|---|
| `SPACE_1` | 4 | Tight gap, icon-to-label |
| `SPACE_2` | 6 | Row-internal gap |
| `SPACE_3` | 8 | Standard padding, gap between related controls |
| `SPACE_4` | 10 | Button intra-padding, small card inset |
| `SPACE_5` | 12 | Common padding for list rows, card headers |
| `SPACE_6` | 14 | Medium padding |
| `SPACE_7` | 16 | Section padding, larger gap |
| `SPACE_8` | 18 | Loose gap (reach-for if 16 feels cramped) |
| `SPACE_9` | 20 | Modal content inset |
| `SPACE_10` | 24 | Section separator |

**When in doubt, reach for `SPACE_3` (8px)** — it's the most-used rung.

If you need a value outside the set, document *why* in a comment at the
call site. A new rung joining the scale needs a comment here too.

## Border radii

| Token | px | Use |
|---|---|---|
| `RADIUS_XS` | 4 | Small badges, keyboard-hint chips |
| `RADIUS_SM` | 6 | Cards, list-row highlights |
| `RADIUS_MD` | 8 | Standard component corners (buttons, inputs) |
| `RADIUS_LG` | 10 | Larger cards, resource indicator |
| `RADIUS_XL` | 12 | Modal corners |
| `RADIUS_2XL` | 14 | Big panel cards (agent dropdowns) |
| `RADIUS_PILL` | 999 | Icon-button pills, capsule badges |

## Shadow depths

GPUI abstracts shadow via `.shadow_md()` / `.shadow_lg()`. We don't have
pixel-level control over blur/spread (nor do we need it), so:

| Depth | GPUI | Use |
|---|---|---|
| Medium | `.shadow_md()` | Popovers, dropdowns, floating menus |
| Large | `.shadow_lg()` | Modals, dialogs |

Buttons and inputs don't use shadow — they rely on
border + overlay for depth.

## Component sizes

Recurring literal sizes worth naming (otherwise they drift):

| Token | px | Component |
|---|---|---|
| `MODAL_WIDTH_PX` | 440 | New-task modal, add-agent modal |
| `STATUS_BUTTON_WIDTH_PX` | 176 | Resource indicator in titlebar |
| `ICON_SIZE_DEFAULT_PX` | 16 | Default icon in buttons / rows |
| `ICON_SIZE_SM_PX` | 11 | Small inline icons (chevrons, checks) |
| `ICON_SIZE_LG_PX` | 26 | Titlebar icon buttons |

Sidebar widths (320, 364, 460, 720, 760 px) are *draggable breakpoints*,
not fixed sizes — they live in `left_sidebar.rs` / `right_sidebar.rs`
with surrounding layout logic rather than as tokens.

## Border width

Always 1px. GPUI helpers: `.border_1()`, `.border_b_1()`, `.border_t_1()`,
`.border_r_1()`. If you ever reach for a multi-pixel border, reconsider:
depth should come from surface + overlay, not stroke weight.
