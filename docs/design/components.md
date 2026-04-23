# Component Idioms

How the desktop app assembles tokens into recognisable components. Each
pattern has a code reference so you can see the real implementation.

## Buttons

The app has one button style — a filled surface with overlay-based
hover and active states. There is deliberately no outline or ghost
variant; differences in emphasis come from **where** a button lives
(chrome vs card) and **which overlay rung** it uses on hover.

```rust
div()
    .bg(tokens::card_bg())
    .hover(|s| s.bg(tokens::overlay_hover()))
    .when(is_pressed, |s| s.bg(tokens::overlay_active()))
    .rounded(px(tokens::RADIUS_MD))
    .px(px(tokens::SPACE_5))
    .py(px(tokens::SPACE_2))
    .text_size(rems(tokens::FONT_SIZE_BODY_PX / 16.))
    .text_color(tokens::text_primary())
```

- **Rest state**: surface colour from the parent context (chrome for
  titlebar buttons, card for modal buttons).
- **Hover**: `overlay_hover` (0.06) on most; `overlay_hover_strong`
  (0.08) on primary CTAs.
- **Pressed**: `overlay_active` (0.10).
- **Destructive**: hover overlay switches to `ErrorColors::overlay_hover()`
  (red-tinted 0.26 alpha).

Icon buttons mirror the same pattern but swap rectangular padding for
`RADIUS_PILL` rounding. See `titlebar.rs` for the canonical icon button.

## Inputs (text fields)

Inputs are a bordered card-surface rectangle. Focus is communicated by
switching the border from `tokens::border()` to `tokens::focus_ring()`.

```rust
div()
    .bg(tokens::card_bg())
    .border_1()
    .border_color(if is_focused {
        tokens::focus_ring()
    } else {
        tokens::border()
    })
    .rounded(px(tokens::RADIUS_SM))
    .px(px(tokens::SPACE_3))
    .py(px(tokens::SPACE_2))
```

- **Placeholder**: `tokens::text_placeholder()` (38% brightness).
- **No background change on focus** — only the border moves. Keeps the
  eye on content, not chrome.

Examples: `new_task_modal.rs`, `add_agent_modal.rs` (search fields).

## List rows

Rows sit on the parent's background (sunken or chrome) at rest, pick up
`overlay_hover` on mouse-over, and `overlay_active` when selected.

```rust
div()
    .when(is_selected, |s| s.bg(tokens::overlay_active()))
    .hover(|s| s.bg(tokens::overlay_hover()))
    .px(px(tokens::SPACE_5))
    .py(px(tokens::SPACE_3))
    .text_color(tokens::text_primary())
```

Metadata (timestamps, counts, status) on a row uses `tokens::text_muted()`.
Destructive row actions (delete) paint with `ErrorColors::text()` and
use `ErrorColors::overlay_hover()` on hover.

Examples: project list in `left_sidebar.rs`, task rows.

## Modals

A modal is a scrim over the whole window + a centered card.

```rust
// Scrim
div().bg(tokens::scrim_bg()).absolute().size_full()

// Card
div()
    .w(px(tokens::MODAL_WIDTH_PX))
    .bg(tokens::card_bg())
    .border_1()
    .border_color(tokens::border())
    .rounded(px(tokens::RADIUS_XL))
    .shadow_lg()
    .p(px(tokens::SPACE_9))
```

- **Width**: 440px unless content genuinely demands more (none do today).
- **Padding**: 20px all sides (`SPACE_9`).
- **Shadow**: always `shadow_lg` for modals; `shadow_md` only for
  popovers/dropdowns.

See `new_task_modal.rs`, `add_agent_modal.rs`.

## Toasts / banners

Toasts use the full 3-part semantic swatch.

```rust
div()
    .bg(SuccessColors::bg())
    .border_1()
    .border_color(SuccessColors::muted())
    .rounded(px(tokens::RADIUS_MD))
    .p(px(tokens::SPACE_5))
    .child(Icon::new(...).text_color(SuccessColors::icon()))
    .child(text.text_color(tokens::text_primary()))
```

Apply the same pattern for Error / Warning / Info by swapping the
semantic struct.

## Dividers

Horizontal rules between sections of a sidebar, list, or settings page:

```rust
div().h(px(1.)).bg(tokens::divider())
```

For container borders (distinct sections): use `.border_b_1()` on the
upper element with `.border_color(tokens::border())`. Divider is for
visual *separation without structural meaning*; border is for structural
grouping.

## Sidebar chrome

The left/right sidebars and titlebar share `tokens::chrome_bg()`. Their
backgrounds are flat — no gradient, no inner shadow. Items inside are
the only source of visual interest.

See `left_sidebar.rs`, `right_sidebar.rs`, `titlebar.rs`.

## Terminal

Terminal surface uses `tokens::terminal_bg()` (darker than chrome), with
PTY output rendered by alacritty's VT parser. The terminal is its own
typographic and interaction zone — it doesn't use the UI spacing /
radius scale. Don't wrap terminal contents with UI components.
