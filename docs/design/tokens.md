# Palette

All values below are drawn from `desktop/src/tokens.rs` and the GPUI
source. HSL-A triples are the authoritative form; RGB hex is a
lossy-rounded reference. Every `hsla(…)` uses hue in turns (0..1), not
degrees — the channel numbers here mirror the Rust source, so `220./360.`
means "220° hue."

## Surfaces

The app stacks surfaces from "deepest chrome" to "lightest card."

| Token | Rust | Value | Used for |
|---|---|---|---|
| Chrome | `tokens::chrome_bg()` | `rgb(0x27292e)` | Titlebar, sidebars, footer |
| Card | `tokens::card_bg()` | `rgb(0x2b2d31)` | Modals, popovers, dropdown menus |
| Sunken | `tokens::sunken_bg()` | `rgb(0x202329)` | Subtle sub-surface (list-row bg) |
| Terminal | `tokens::terminal_bg()` | `rgb(0x000000)` | PTY / agent session canvas |
| Scrim | `tokens::scrim_bg()` | `hsla(0,0,0,0.50)` | Modal overlay, blur background |

## Text

Four levels of brightness for visual hierarchy. All four are pure neutral
(zero saturation) — colour is reserved for semantic meaning.

| Token | Rust | HSL-A | Approx hex |
|---|---|---|---|
| Primary | `tokens::text_primary()` | `(0, 0, 0.92, 1)` | `#ebebeb` |
| Secondary | `tokens::text_secondary()` | `(0, 0, 0.78, 1)` | `#c7c7c7` |
| Muted | `tokens::text_muted()` | `(0, 0, 0.58, 1)` | `#949494` |
| Placeholder | `tokens::text_placeholder()` | `(0, 0, 0.38, 1)` | `#616161` |

Rule of thumb: labels & body → primary; timestamps, counts, captions →
muted. Use secondary when a whole block needs to recede without being
text-grey — think "the second most important thing on the row."

## Borders & dividers

Subtractive — both are white with low alpha over the underlying surface,
never solid lines.

| Token | Rust | Value |
|---|---|---|
| Border | `tokens::border()` | `hsla(0,0,1,0.08)` |
| Divider | `tokens::divider()` | `hsla(0,0,1,0.06)` |

## Interactive overlays

Interactive elements communicate state by tinting the surface underneath.
These four rungs cover rest/hover/pressed/active across all component
types. If a new component needs a state *between* these, pick the closest
— don't mint a new alpha.

| Token | Rust | Alpha | When |
|---|---|---|---|
| Rest (subtle) | `tokens::overlay_rest()` | `0.04` | Quiet inactive state (unfocused sidebar row) |
| Hover | `tokens::overlay_hover()` | `0.06` | Default list-row / button hover |
| Hover (strong) | `tokens::overlay_hover_strong()` | `0.08` | Primary CTA hover |
| Active | `tokens::overlay_active()` | `0.10` | Pressed / selected state |

## Focus

Keyboard-focused controls pick up a cool-periwinkle ring. Used on text
inputs today (border colour switches from `border` → `focus_ring` when
focused); extend to custom buttons when we add keyboard nav.

| Token | Rust | HSL-A |
|---|---|---|
| Focus | `tokens::focus_ring()` | `(220°, 0.55, 0.60, 1)` |

## Semantic colours

Toasts, badges, and banners use a 3-part swatch per semantic:

- **icon** — foreground colour (icon + primary text on the banner).
- **bg** — the banner's background.
- **muted** — borders and subtle accents within the banner.

Values mined from `app.rs::toast_color_details`.

### Success (green/teal)

| Role | Value |
|---|---|
| icon | `hsla(138°, 0.52, 0.66, 1)` |
| bg | `hsla(136°, 0.40, 0.24, 0.90)` |
| muted | `hsla(136°, 0.42, 0.34, 0.55)` |

### Error (red)

| Role | Value |
|---|---|
| icon | `hsla(0°, 0.68, 0.72, 1)` |
| bg | `hsla(0°, 0.40, 0.24, 0.90)` |
| muted | `hsla(0°, 0.45, 0.36, 0.58)` |
| text (destructive label) | `hsla(0°, 0.78, 0.68, 1)` |
| overlay on hover | `hsla(0°, 0.45, 0.34, 0.26)` |

### Warning (amber/orange)

| Role | Value |
|---|---|
| icon | `hsla(42°, 0.70, 0.68, 1)` |
| bg | `hsla(40°, 0.42, 0.24, 0.90)` |
| muted | `hsla(42°, 0.46, 0.34, 0.58)` |

### Info (blue)

| Role | Value |
|---|---|
| icon | `hsla(208°, 0.62, 0.72, 1)` |
| bg | `hsla(210°, 0.40, 0.24, 0.90)` |
| muted | `hsla(208°, 0.42, 0.34, 0.58)` |

## Project-avatar palette

Deterministic 8-hue rotation for project letter avatars. See
`desktop/src/theme.rs`.

```
0x5B4A9E purple   0x2E7D6F teal     0xB85C38 burnt orange   0x3A6EA5 blue
0x8B5E3C brown    0x7B2D5F magenta  0x4A7C4B green          0x9C5151 rose
```

`theme::project_color(project_id)` assigns one by hashing the ID.
