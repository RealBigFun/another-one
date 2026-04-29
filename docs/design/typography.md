# Typography

The desktop is monospace-only. Every surface — terminal, sidebar labels,
modal body text, toast messages — uses Lilex NerdFont Mono. This is
deliberate: we wrap terminals as a first-class UI element, and matching
the UI font to the terminal font avoids a visual "mode shift" when eye
crosses the boundary.

See [[../architecture/terminal-wrapping-principle]] for the broader
principle.

## Font family

Lilex NerdFont Mono. Ships under `desktop/assets/fonts/` in six variants:

| File | Weight / style |
|---|---|
| `LilexNerdFontMono-Regular.ttf` | 400 |
| `LilexNerdFontMono-Medium.ttf` | 500 |
| `LilexNerdFontMono-Bold.ttf` | 700 |
| `LilexNerdFontMono-Italic.ttf` | 400 italic |
| `LilexNerdFontMono-MediumItalic.ttf` | 500 italic |
| `LilexNerdFontMono-BoldItalic.ttf` | 700 italic |

Mobile hasn't bundled this font yet; when it does, expect a ~300KB per
weight addition to the APK.

Rust token: `tokens::FONT_FAMILY` (the literal string `"Lilex NerdFont Mono"`).

## Weights

GPUI weight constants, matched to what the codebase actually uses:

| Constant | Numeric | Use |
|---|---|---|
| `FontWeight::NORMAL` | 400 | Default body, list rows, inputs |
| `FontWeight::MEDIUM` | 500 | Buttons, interactive labels |
| `FontWeight::SEMIBOLD` | 600 | Subheadings |
| `FontWeight::BOLD` | 700 | Section headings, page titles |

The app has never needed light/thin/black weights. Add one only if a new
surface genuinely demands it.

## Size scale

All sizes are declared in pixels; GPUI's `rems(n/16.)` idiom divides down
at the call site. The scale is ad-hoc (hand-tuned per surface), not
geometric, but most surfaces hit one of the seven named rungs below.

| Token | px | rems | Role |
|---|---|---|---|
| `FONT_SIZE_CAPTION_PX` | 10 | `rems(10./16.)` | Small annotations, keyboard hints |
| `FONT_SIZE_SMALL_PX` | 11 | `rems(11./16.)` | Captions, counts, metadata |
| `FONT_SIZE_BODY_PX` | 12 | `rems(12./16.)` | Default body, list rows, buttons |
| `FONT_SIZE_BODY_LG_PX` | 13 | `rems(13./16.)` | Emphasised body, dense labels |
| `FONT_SIZE_HEADING_SM_PX` | 14 | `rems(14./16.)` | Subheading |
| `FONT_SIZE_HEADING_PX` | 18 | `rems(18./16.)` | Section heading |
| `FONT_SIZE_HEADING_LG_PX` | 20 | `rems(20./16.)` | Page title (settings, modal title) |

If you feel the need for 9px, 12.5px, 15px — don't. Pick the nearest
rung; if that's materially wrong, something bigger is wrong about the
layout.

## Terminal text

Terminal font size is user-adjustable (`ZoomIn`/`ZoomOut`/`ZoomReset`
actions) and defaults to a separate `16px` baseline (see
`terminal_runtime.rs`). The terminal is *not* subject to the UI scale —
treat it as its own thing. Line-height is multiplied by
`TERMINAL_LINE_HEIGHT_RATIO` for readability.

## What Slint clients should do

Bundle Lilex NerdFont Mono and request it explicitly for chrome and
terminal rendering. The Slint implementation imports all six bundled TTF
faces from `desktop/assets/fonts/` and embeds them through `slint-build`;
platform fallback monospace fonts are acceptable only as a temporary
diagnostic state because glyph coverage and metrics drift from the GPUI
baseline.
