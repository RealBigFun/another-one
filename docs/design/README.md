# Design System

A snapshot of the visual language used in the desktop app
(`desktop/src/`), extracted so it can travel with us through Phase 1 of
the mobile plan (core refactor) and be mirrored by the Flutter mobile
sandbox.

## Why this exists

The desktop is ~276 colour literals and ~60 ad-hoc `hsla(…)` / `rgb(…)`
calls scattered across its files. That was fine for a single-platform
app, but with mobile coming online we need:

1. A single source of truth to mirror, so mobile visuals don't drift.
2. A place to say "this colour means *this*" so Phase 1 (core
   extraction) doesn't accidentally bake styling into business logic.
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
- **tokens.json** — machine-readable mirror of the token values.
  Intended to be consumed by the Flutter app later (hand-mirrored for
  now; auto-generated once a shared `core` crate exists).

All token values are also defined in `desktop/src/tokens.rs` — that's
where GPUI call sites import from. `tokens.json` and the Rust module
must stay in sync; edit both, or add a build-time generator.

## Scope boundary

This is a *catalogue*, not a rewrite. The tokens module exists to
stop new code from inventing fresh literals; existing call sites will
migrate opportunistically as files are touched. The design doesn't
introduce new values — every entry is mined from current source.

## Conventions

- **Dark theme only.** The app has never had a light mode; tokens
  reflect that. A future `tokens_light.rs` would live beside the
  current module with a `ThemeKind` selector, but we're not there.
- **Monospace only.** Lilex NerdFont Mono for every surface. Terminal
  and UI share the same face to reinforce the "the terminal *is* the
  product" stance.
- **Named over numbered.** Tokens have semantic names
  (`card_bg`, `overlay_hover`, `text_muted`) rather than shade
  numbers. Easier to grep, harder to use wrong.

## How to use — Rust (desktop)

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

## How to use — Dart (mobile, future)

Until a `core` crate arrives, mobile hand-mirrors `tokens.json` into a
Dart constants file. Example planned shape:

```dart
class Tokens {
  static const cardBg = Color(0xFF2b2d31);
  static const textPrimary = Color(0xEBFFFFFF); // 92% white
  static const radiusMd = 8.0;
  static const spaceSm = 8.0;
}
```

## Related

- [[../architecture/terminal-wrapping-principle]] — why typography is
  monospace-only.
- [[../apps/desktop]] — where the tokens live today.
