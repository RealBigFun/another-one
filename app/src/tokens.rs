//! Design tokens: the centralized source of truth for colour, typography,
//! spacing, radii, and shadow depth used by the desktop UI.
//!
//! This module intentionally returns GPUI types (`Hsla`, etc.) so call
//! sites are ergonomic. The underlying numeric values (HSL channels,
//! pixel sizes) are the *real* tokens and are duplicated in
//! [`docs/design/tokens.json`][tokens-json] for future cross-platform
//! consumption (Flutter mobile, a shared `core` crate after Phase 1 of
//! the mobile plan, etc.). Keep that file in lockstep when editing here.
//!
//! [tokens-json]: ../../docs/design/tokens.json
//!
//! **Scope boundary.** This module *catalogues* the values already in use
//! across `desktop/src/`; it does **not** refactor the 60+ scattered
//! `hsla(…)` / `rgb(…)` literal call sites. Those migrations are expected
//! to happen opportunistically as files are touched. Adding a new UI
//! surface? Pull from here. Touching an existing one? Replace literals
//! with token accessors as a drive-by.
//!
//! Every token reflects the app's current dark-only aesthetic. A future
//! light theme would live as a parallel module (`tokens_light.rs`) with a
//! `ThemeKind` selector; we don't pretend to support that today.

// Tokens are deliberately unused at first — the codebase still reaches
// for literals at most call sites. Each opportunistic migration picks a
// few off this list and drops the `#[allow]` on this module when it's
// fully consumed.
#![allow(dead_code)]

use gpui::{hsla, rgb, Hsla};

// ─────────────────────────────────────────────────────────────────────────
// Surface colours — chrome, cards, backgrounds
// ─────────────────────────────────────────────────────────────────────────

/// Titlebar + sidebar chrome. The darkest *neutral* surface.
pub fn chrome_bg() -> Hsla {
    rgb(0x27292e).into()
}

/// Modal + dropdown card surface. One step lighter than chrome.
pub fn card_bg() -> Hsla {
    rgb(0x2b2d31).into()
}

/// Subtle sunken background, used for list-row backgrounds and footer.
pub fn sunken_bg() -> Hsla {
    rgb(0x202329).into()
}

/// Terminal/editor surface. Darker than chrome.
pub fn terminal_bg() -> Hsla {
    rgb(0x17191d).into()
}

/// Modal scrim — translucent black over the full window.
pub fn scrim_bg() -> Hsla {
    hsla(0., 0., 0., 0.50)
}

// ─────────────────────────────────────────────────────────────────────────
// Text colours — four brightness levels for hierarchy
// ─────────────────────────────────────────────────────────────────────────

/// Primary text — headings, interactive labels, body copy.
pub fn text_primary() -> Hsla {
    hsla(0., 0., 0.92, 1.)
}

/// Secondary text — subheadings, less-emphasized content.
pub fn text_secondary() -> Hsla {
    hsla(0., 0., 0.78, 1.)
}

/// Muted text — timestamps, captions, metadata.
pub fn text_muted() -> Hsla {
    hsla(0., 0., 0.58, 1.)
}

/// Placeholder text in inputs, disabled labels.
pub fn text_placeholder() -> Hsla {
    hsla(0., 0., 0.38, 1.)
}

// ─────────────────────────────────────────────────────────────────────────
// Borders & dividers
// ─────────────────────────────────────────────────────────────────────────

/// Standard 1px border colour — inputs, cards, divider lines.
pub fn border() -> Hsla {
    hsla(0., 0., 1.0, 0.08)
}

/// A hair lighter than [`border`] for horizontal rules / quiet dividers.
pub fn divider() -> Hsla {
    hsla(0., 0., 1.0, 0.06)
}

// ─────────────────────────────────────────────────────────────────────────
// Hover / active overlays — applied as background tint over whatever
// surface is underneath. Every interactive element should land on one of
// these four rungs.
// ─────────────────────────────────────────────────────────────────────────

/// Subtle tint, for quiet inactive states (e.g. unfocused sidebar rows).
pub fn overlay_rest() -> Hsla {
    hsla(0., 0., 1.0, 0.04)
}

/// Subtle hover — most list rows, tabs, quiet buttons.
pub fn overlay_hover() -> Hsla {
    hsla(0., 0., 1.0, 0.06)
}

/// Medium hover — stronger affordance (primary buttons, CTAs).
pub fn overlay_hover_strong() -> Hsla {
    hsla(0., 0., 1.0, 0.08)
}

/// Active / pressed state — goes under [`overlay_hover_strong`] in the
/// input stack.
pub fn overlay_active() -> Hsla {
    hsla(0., 0., 1.0, 0.10)
}

// ─────────────────────────────────────────────────────────────────────────
// Focus & accents
// ─────────────────────────────────────────────────────────────────────────

/// Cool-periwinkle focus ring colour (keyboard-focused controls).
pub fn focus_ring() -> Hsla {
    hsla(220. / 360., 0.55, 0.60, 1.0)
}

// ─────────────────────────────────────────────────────────────────────────
// Semantic status colours — toasts, badges, banners
//
// Each semantic has a 3-part swatch: `icon` (foreground), `bg`
// (background surface of the banner), `muted` (borders / subtle accents
// of the banner). Values mined from `app.rs::toast_color_details`.
// ─────────────────────────────────────────────────────────────────────────

/// Success (green/teal).
pub struct SuccessColors;
impl SuccessColors {
    pub fn icon() -> Hsla {
        hsla(138. / 360., 0.52, 0.66, 1.)
    }
    pub fn bg() -> Hsla {
        hsla(136. / 360., 0.40, 0.24, 0.90)
    }
    pub fn muted() -> Hsla {
        hsla(136. / 360., 0.42, 0.34, 0.55)
    }
}

/// Error (red).
pub struct ErrorColors;
impl ErrorColors {
    pub fn icon() -> Hsla {
        hsla(0., 0.68, 0.72, 1.)
    }
    pub fn bg() -> Hsla {
        hsla(0., 0.40, 0.24, 0.90)
    }
    pub fn muted() -> Hsla {
        hsla(0., 0.45, 0.36, 0.58)
    }
    /// Saturated red for "delete" / destructive text labels.
    pub fn text() -> Hsla {
        hsla(0., 0.78, 0.68, 1.)
    }
    /// Hover-background tint for destructive list-row actions.
    pub fn overlay_hover() -> Hsla {
        hsla(0., 0.45, 0.34, 0.26)
    }
}

/// Warning (amber/orange).
pub struct WarningColors;
impl WarningColors {
    pub fn icon() -> Hsla {
        hsla(42. / 360., 0.70, 0.68, 1.)
    }
    pub fn bg() -> Hsla {
        hsla(40. / 360., 0.42, 0.24, 0.90)
    }
    pub fn muted() -> Hsla {
        hsla(42. / 360., 0.46, 0.34, 0.58)
    }
}

/// Info (blue).
pub struct InfoColors;
impl InfoColors {
    pub fn icon() -> Hsla {
        hsla(208. / 360., 0.62, 0.72, 1.)
    }
    pub fn bg() -> Hsla {
        hsla(210. / 360., 0.40, 0.24, 0.90)
    }
    pub fn muted() -> Hsla {
        hsla(208. / 360., 0.42, 0.34, 0.58)
    }
}

// ─────────────────────────────────────────────────────────────────────────
// Typography
//
// The desktop app is monospace-only (Lilex NerdFont Mono). Size tokens
// are expressed in CSS-style pixels; divide by `16.0` to get `rems(…)`
// at GPUI call sites.
// ─────────────────────────────────────────────────────────────────────────

/// Primary font family. Shipped under `desktop/assets/fonts/`.
pub const FONT_FAMILY: &str = "Lilex NerdFont Mono";

/// Caption / annotation (e.g. timestamps, keyboard hints).
pub const FONT_SIZE_CAPTION_PX: f32 = 10.0;

/// Small body (e.g. metadata, counts).
pub const FONT_SIZE_SMALL_PX: f32 = 11.0;

/// Default body text (list rows, buttons, inputs).
pub const FONT_SIZE_BODY_PX: f32 = 12.0;

/// Emphasized body (section labels).
pub const FONT_SIZE_BODY_LG_PX: f32 = 13.0;

/// Subheading.
pub const FONT_SIZE_HEADING_SM_PX: f32 = 14.0;

/// Section heading.
pub const FONT_SIZE_HEADING_PX: f32 = 18.0;

/// Page-level heading (modals, settings top bar).
pub const FONT_SIZE_HEADING_LG_PX: f32 = 20.0;

// ─────────────────────────────────────────────────────────────────────────
// Spacing scale — px values for margins, paddings, gaps.
//
// The current codebase uses roughly {4, 6, 8, 10, 12, 14, 16, 18, 20, 24}.
// That's hand-tuned; there's no strict geometric scale. We expose the set
// as named constants so new code doesn't invent fresh values — if you
// need something outside this list, document why.
// ─────────────────────────────────────────────────────────────────────────

pub const SPACE_1: f32 = 4.0;
pub const SPACE_2: f32 = 6.0;
pub const SPACE_3: f32 = 8.0;
pub const SPACE_4: f32 = 10.0;
pub const SPACE_5: f32 = 12.0;
pub const SPACE_6: f32 = 14.0;
pub const SPACE_7: f32 = 16.0;
pub const SPACE_8: f32 = 18.0;
pub const SPACE_9: f32 = 20.0;
pub const SPACE_10: f32 = 24.0;

// ─────────────────────────────────────────────────────────────────────────
// Border radii — px values.
// ─────────────────────────────────────────────────────────────────────────

pub const RADIUS_XS: f32 = 4.0;
pub const RADIUS_SM: f32 = 6.0;
pub const RADIUS_MD: f32 = 8.0;
pub const RADIUS_LG: f32 = 10.0;
pub const RADIUS_XL: f32 = 12.0;
pub const RADIUS_2XL: f32 = 14.0;
/// Pill / full-rounding for icon buttons and capsule elements.
pub const RADIUS_PILL: f32 = 999.0;

// ─────────────────────────────────────────────────────────────────────────
// Component-scale sizes — mined from recurring literals.
// ─────────────────────────────────────────────────────────────────────────

/// Modal content width (e.g. new-task, add-agent modals).
pub const MODAL_WIDTH_PX: f32 = 440.0;

/// Resource-indicator button width.
pub const STATUS_BUTTON_WIDTH_PX: f32 = 176.0;

/// Default icon size inside buttons / list rows.
pub const ICON_SIZE_DEFAULT_PX: f32 = 16.0;

/// Small inline icon (e.g. chevrons, checkmarks).
pub const ICON_SIZE_SM_PX: f32 = 11.0;

/// Large icon (titlebar, headline rows).
pub const ICON_SIZE_LG_PX: f32 = 26.0;
