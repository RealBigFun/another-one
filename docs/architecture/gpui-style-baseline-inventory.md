# GPUI Style Baseline Inventory

The GPUI app is the dark-mode source of truth for Slint styling. Slint must preserve these values unless a bd decision explicitly accepts a difference.

## Source Files

- `desktop/src/tokens.rs`: semantic color, typography, spacing, radius, size tokens.
- `desktop/src/theme.rs`: project accent colors and appearance-sensitive chrome helpers.
- `desktop/src/layout.rs`: shell geometry and drawer constraints.
- `desktop/src/app.rs`: toast palettes, resource indicator use, titlebar/footer integration.
- `desktop/src/left_sidebar.rs`: sidebar row hover/active/menu/destructive states.
- `desktop/src/right_sidebar.rs`: working-tree, commits, checks, and card states.
- `desktop/src/settings_page.rs`: settings panels, text inputs, focus rings, keyboard rows.

## Dark Baseline Tokens

| Role | GPUI value | Slint value |
| --- | --- | --- |
| `chrome_bg` | `0x27292e` | `0x27292e` |
| `card_bg` | `0x2b2d31` | `0x2b2d31` |
| `sunken_bg` | `0x202329` | `0x202329` |
| `terminal_bg` | `0x17191d` | `0x17191d` |
| `overlay_hover` | white alpha `0.06` | `#0fffffff` |
| `overlay_active` | white alpha `0.10` | `#1affffff` |
| `border` | white alpha `0.08` | `#16ffffff` |
| `divider` | white alpha `0.06` | `#10ffffff` |
| `focus_ring` | HSL `220deg 55% 60%` | `0x5d7ad5` |
| `text_primary` | lightness `0.92` | `0xebebeb` |
| `text_secondary` | lightness `0.78` | `0xc7c7c7` |
| `text_muted` | lightness `0.58` | `0x949494` |
| `success` | HSL `138deg 52% 66%` | `0x7ad591` |
| `warning` | HSL `42deg 70% 68%` | `0xe5c07b` |
| `danger` | HSL `0deg 68% 72%` | `0xe06c75` |

## Typography

GPUI uses `Lilex NerdFont Mono` as the primary family and generally renders body text between 10px and 14px. Slint currently uses `"monospace"` as a fallback until the font packaging path is wired. Pixel-perfect typography remains blocked on font asset registration and capture evidence.

## Style Rules

- Components consume semantic roles, not raw literals.
- Dark mode is GPUI parity.
- Light mode is a new extension and requires separate review.
- System mode resolves through the Slint appearance profile seam and falls back to dark when platform preference is unavailable.
- Any visual drift must be tracked as a bd decision or a visual-fidelity failure.
