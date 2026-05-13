# Design System

A snapshot of the visual language used in the GPUI app (`app/src/`),
kept so desktop and Android surfaces can share terminology and avoid
inventing one-off colors, sizes, and component conventions.

## Why this exists

The desktop is ~276 colour literals and ~60 ad-hoc `hsla(…)` / `rgb(…)`
calls scattered across its files. That was fine for a single-platform
app, but with Android support and more shared UI surfaces we need:

1. A single source of truth to mirror, so non-desktop visuals don't drift.
2. A place to say "this colour means *this*" so shared logic doesn't
   accidentally bake styling into business logic.
3. Documentation for a future contributor who asks "what's our button
   convention?" — the answer lives here, not in a 500-line file that
   reimplements it inline.

## Structure

- **[[tokens.md]]** — palette, text levels, overlays, semantics. The
  visual vocabulary.
- **[[typography.md]]** — fonts, size scale, weights.
- **[[spacing.md]]** — spacing scale, radii, shadows, component sizes.
- **[[components.md]]** — idioms for buttons, inputs, modals, list
  rows. Cross-references the token docs with real code.
- **tokens.json** — machine-readable mirror of the token values for
  non-Rust consumers and design tooling.

All token values are also defined in `app/src/tokens.rs` — that's where
GPUI call sites import from. `tokens.json` and the Rust module must stay
in sync; edit both, or add a build-time generator.

## Scope boundary

This is a *catalogue*, not a rewrite. The tokens module exists to
stop new code from inventing fresh literals; existing call sites will
migrate opportunistically as files are touched. The design doesn't
introduce new values — every entry is mined from current source.

## Conventions

- **Dark and light modes.** Persistent theme preference is app state;
  tokens should stay semantic so both modes can map surfaces consistently.
- **Monospace only.** Lilex NerdFont Mono for every surface. Terminal
  and UI share the same face to reinforce the "the terminal *is* the
  product" stance.
- **Named over numbered.** Tokens have semantic names
  (`card_bg`, `overlay_hover`, `text_muted`) rather than shade
  numbers. Easier to grep, harder to use wrong.

## How to use — Rust (GPUI)

```rust
use crate::tokens;

fn surface(cx: &mut Context<Self>) -> Div {
    div()
        .bg(tokens::card_bg())
        .border_1()
        .border_color(tokens::border())
        .rounded(px(tokens::RADIUS_MD))
        .p(px(tokens::SPACE_5))
        .text_color(tokens::text_primary())
}
```

## How to use outside GPUI

Use `tokens.json` as the interchange format for Android or future design
tooling. Do not hand-copy values from screenshots; mirror semantic token
names and keep them in sync with `app/src/tokens.rs`.

## Related

- [[../architecture/terminal-wrapping-principle]] — why typography is
  monospace-only.
- [[../apps/desktop]] — where the tokens live today.
