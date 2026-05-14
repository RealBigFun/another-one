//! Colour palette, chrome helpers, and theme utilities.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU8, Ordering};

use gpui::{hsla, rgb, Hsla, Window, WindowAppearance};

use crate::project_store::ThemeMode;

const LIGHT_TERMINAL_BACKGROUND_RGB: u32 = 0xfcfcfc;
const DARK_TERMINAL_BACKGROUND_RGB: u32 = 0x0d1016;
const LIGHT_TERMINAL_FOREGROUND_RGB: u32 = 0x5c6166;
const DARK_TERMINAL_FOREGROUND_RGB: u32 = 0xbfbdb6;
const LIGHT_TERMINAL_CURSOR_RGB: u32 = 0x3b9ee5;
const DARK_TERMINAL_CURSOR_RGB: u32 = 0x5ac1fe;

/// Globally-visible resolved app theme. Updated by the render path so
/// code that doesn't have access to the `Window` / `App` can still resolve
/// `ThemeMode::System` consistently with the current OS appearance. The
/// alacritty-backed cell renderer also uses this to pick default fg/bg colors.
/// 0 = Dark (default), 1 = Light.
static TERMINAL_THEME: AtomicU8 = AtomicU8::new(0);

pub fn set_terminal_theme(resolved: ResolvedTheme) {
    let v = match resolved {
        ResolvedTheme::Light => 1,
        ResolvedTheme::Dark => 0,
    };
    TERMINAL_THEME.store(v, Ordering::Relaxed);
}

pub fn current_terminal_theme() -> ResolvedTheme {
    match TERMINAL_THEME.load(Ordering::Relaxed) {
        1 => ResolvedTheme::Light,
        _ => ResolvedTheme::Dark,
    }
}

pub(crate) fn terminal_background_rgb(resolved: ResolvedTheme) -> u32 {
    match resolved {
        ResolvedTheme::Light => LIGHT_TERMINAL_BACKGROUND_RGB,
        ResolvedTheme::Dark => DARK_TERMINAL_BACKGROUND_RGB,
    }
}

pub(crate) fn terminal_foreground_rgb(resolved: ResolvedTheme) -> u32 {
    match resolved {
        ResolvedTheme::Light => LIGHT_TERMINAL_FOREGROUND_RGB,
        ResolvedTheme::Dark => DARK_TERMINAL_FOREGROUND_RGB,
    }
}

pub(crate) fn terminal_cursor_rgb(resolved: ResolvedTheme) -> u32 {
    match resolved {
        ResolvedTheme::Light => LIGHT_TERMINAL_CURSOR_RGB,
        ResolvedTheme::Dark => DARK_TERMINAL_CURSOR_RGB,
    }
}

pub(crate) fn terminal_background_for_theme(resolved: ResolvedTheme) -> Hsla {
    rgb(terminal_background_rgb(resolved)).into()
}

pub(crate) fn terminal_foreground_for_theme(resolved: ResolvedTheme) -> Hsla {
    rgb(terminal_foreground_rgb(resolved)).into()
}

pub(crate) fn terminal_cursor_for_theme(resolved: ResolvedTheme) -> Hsla {
    rgb(terminal_cursor_rgb(resolved)).into()
}

pub fn terminal_default_background() -> Hsla {
    terminal_background_for_theme(current_terminal_theme())
}

pub fn terminal_default_foreground() -> Hsla {
    terminal_foreground_for_theme(current_terminal_theme())
}

pub fn terminal_default_cursor() -> Hsla {
    terminal_cursor_for_theme(current_terminal_theme())
}

/// Full terminal colour table consumed by the cell renderer.
///
/// All entries are stored as `[r, g, b]` u8 triples so this module
/// has no dependency on the alacritty-side `Rgb` type. The renderer
/// converts to its own colour types at the call site.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalPalette {
    pub background: [u8; 3],
    pub foreground: [u8; 3],
    pub cursor: [u8; 3],
    pub bright_foreground: [u8; 3],
    pub dim_foreground: [u8; 3],
    pub normal: [[u8; 3]; 8],
    pub bright: [[u8; 3]; 8],
    pub dim: [[u8; 3]; 8],
}

/// Resolve `hex` (0xRRGGBB) to a u8 triple at compile time.
const fn hex_rgb(hex: u32) -> [u8; 3] {
    [
        ((hex >> 16) & 0xff) as u8,
        ((hex >> 8) & 0xff) as u8,
        (hex & 0xff) as u8,
    ]
}

/// Look up the static palette for a resolved theme.
pub fn terminal_palette(resolved: ResolvedTheme) -> &'static TerminalPalette {
    match resolved {
        ResolvedTheme::Light => &AYU_LIGHT_TERMINAL,
        ResolvedTheme::Dark => &AYU_DARK_TERMINAL,
    }
}

/// Convenience wrapper that reads the global terminal-theme atomic
/// (`set_terminal_theme` / `current_terminal_theme`).
pub fn current_terminal_palette() -> &'static TerminalPalette {
    terminal_palette(current_terminal_theme())
}

const AYU_DARK_TERMINAL: TerminalPalette = TerminalPalette {
    background: hex_rgb(DARK_TERMINAL_BACKGROUND_RGB),
    foreground: hex_rgb(DARK_TERMINAL_FOREGROUND_RGB),
    cursor: hex_rgb(DARK_TERMINAL_CURSOR_RGB),
    bright_foreground: hex_rgb(0xbfbdb6),
    dim_foreground: hex_rgb(0x85847f),
    normal: [
        hex_rgb(0x0d1016),
        hex_rgb(0xef7177),
        hex_rgb(0xaad84c),
        hex_rgb(0xfeb454),
        hex_rgb(0x5ac1fe),
        hex_rgb(0x39bae5),
        hex_rgb(0x95e5cb),
        hex_rgb(0xbfbdb6),
    ],
    bright: [
        hex_rgb(0x545557),
        hex_rgb(0x83353b),
        hex_rgb(0x567627),
        hex_rgb(0x92582b),
        hex_rgb(0x27618c),
        hex_rgb(0x205a78),
        hex_rgb(0x4c806f),
        hex_rgb(0xfafafa),
    ],
    dim: [
        hex_rgb(0x3a3b3c),
        hex_rgb(0xa74f53),
        hex_rgb(0x769735),
        hex_rgb(0xb17d3a),
        hex_rgb(0x3e87b1),
        hex_rgb(0x2782a0),
        hex_rgb(0x68a08e),
        hex_rgb(0x85847f),
    ],
};

const AYU_LIGHT_TERMINAL: TerminalPalette = TerminalPalette {
    background: hex_rgb(LIGHT_TERMINAL_BACKGROUND_RGB),
    foreground: hex_rgb(LIGHT_TERMINAL_FOREGROUND_RGB),
    cursor: hex_rgb(LIGHT_TERMINAL_CURSOR_RGB),
    bright_foreground: hex_rgb(0x5c6166),
    dim_foreground: hex_rgb(0xfcfcfc),
    normal: [
        hex_rgb(0x5c6166),
        hex_rgb(0xef7271),
        hex_rgb(0x85b304),
        hex_rgb(0xf1ad49),
        hex_rgb(0x3b9ee5),
        hex_rgb(0x55b4d3),
        hex_rgb(0x4dbf99),
        hex_rgb(0xfcfcfc),
    ],
    bright: [
        hex_rgb(0x3b9ee5),
        hex_rgb(0xfebab6),
        hex_rgb(0xc7d98f),
        hex_rgb(0xfed5a3),
        hex_rgb(0xabcdf2),
        hex_rgb(0xb1d8e8),
        hex_rgb(0xace0cb),
        hex_rgb(0xffffff),
    ],
    dim: [
        hex_rgb(0x9c9fa2),
        hex_rgb(0x833538),
        hex_rgb(0x445613),
        hex_rgb(0x8a5227),
        hex_rgb(0x214c76),
        hex_rgb(0x2f5669),
        hex_rgb(0x2a5f4a),
        hex_rgb(0xbcbec0),
    ],
};

/// Stable colour palette for project letter avatars (8 hues).
pub const PROJECT_COLORS: [u32; 8] = [
    0x5B4A9E, // purple
    0x2E7D6F, // teal
    0xB85C38, // burnt orange
    0x3A6EA5, // blue
    0x8B5E3C, // brown
    0x7B2D5F, // magenta
    0x4A7C4B, // green
    0x9C5151, // rose
];

/// Deterministic colour for a project based on its id.
pub fn project_color(id: &str) -> u32 {
    let hash: u32 = id
        .bytes()
        .fold(0u32, |acc, b| acc.wrapping_mul(31).wrapping_add(b as u32));
    PROJECT_COLORS[(hash as usize) % PROJECT_COLORS.len()]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolvedTheme {
    Light,
    Dark,
}

#[derive(Clone, Copy, Debug)]
pub struct SemanticColors {
    pub icon: Hsla,
    pub bg: Hsla,
    pub muted: Hsla,
    pub text: Hsla,
}

#[derive(Clone, Copy, Debug)]
pub struct AppTheme {
    pub resolved: ResolvedTheme,
    pub chrome_bg: Hsla,
    pub card_bg: Hsla,
    pub sunken_bg: Hsla,
    pub terminal_bg: Hsla,
    pub scrim_bg: Hsla,
    pub text_primary: Hsla,
    pub text_secondary: Hsla,
    pub text_muted: Hsla,
    pub text_placeholder: Hsla,
    pub border: Hsla,
    pub divider: Hsla,
    pub overlay_rest: Hsla,
    pub overlay_hover: Hsla,
    pub overlay_hover_strong: Hsla,
    pub overlay_active: Hsla,
    pub focus_ring: Hsla,
    pub success: SemanticColors,
    pub error: SemanticColors,
    pub warning: SemanticColors,
    pub info: SemanticColors,
}

pub fn resolve_theme(window: &Window, mode: ThemeMode) -> ResolvedTheme {
    match mode {
        ThemeMode::Light => ResolvedTheme::Light,
        ThemeMode::Dark => ResolvedTheme::Dark,
        ThemeMode::System => {
            // Android's window.appearance() is stuck at Light until
            // the first ConfigChanged fires. Prefer the seeded
            // OS-preference from `android_main` when available
            // (target-gated no-op on desktop). See
            // `mobile::system_prefers_dark`.
            if let Some(dark) = crate::mobile::system_prefers_dark() {
                return if dark {
                    ResolvedTheme::Dark
                } else {
                    ResolvedTheme::Light
                };
            }
            match window.appearance() {
                WindowAppearance::Dark | WindowAppearance::VibrantDark => ResolvedTheme::Dark,
                _ => ResolvedTheme::Light,
            }
        }
    }
}

pub fn app_theme(window: &Window, mode: ThemeMode) -> AppTheme {
    match resolve_theme(window, mode) {
        ResolvedTheme::Light => light_theme(),
        ResolvedTheme::Dark => dark_theme(),
    }
}

/// Theme for render paths that do not currently receive a Window. `System`
/// follows the most recently resolved app theme, which the main render path
/// updates from the OS appearance before rendering child content.
pub fn app_theme_for_preference(mode: ThemeMode) -> AppTheme {
    let resolved = match mode {
        ThemeMode::Light => ResolvedTheme::Light,
        ThemeMode::Dark => ResolvedTheme::Dark,
        ThemeMode::System => current_terminal_theme(),
    };

    match resolved {
        ResolvedTheme::Light => light_theme(),
        ResolvedTheme::Dark => dark_theme(),
    }
}

pub fn dark_theme() -> AppTheme {
    AppTheme {
        resolved: ResolvedTheme::Dark,
        chrome_bg: rgb(0x27292e).into(),
        card_bg: rgb(0x2b2d31).into(),
        sunken_bg: rgb(0x202329).into(),
        terminal_bg: terminal_background_for_theme(ResolvedTheme::Dark),
        scrim_bg: hsla(0., 0., 0., 0.50),
        text_primary: hsla(0., 0., 0.92, 1.),
        text_secondary: hsla(0., 0., 0.78, 1.),
        text_muted: hsla(0., 0., 0.58, 1.),
        text_placeholder: hsla(0., 0., 0.38, 1.),
        border: hsla(0., 0., 1.0, 0.08),
        divider: hsla(0., 0., 1.0, 0.06),
        overlay_rest: hsla(0., 0., 1.0, 0.04),
        overlay_hover: hsla(0., 0., 1.0, 0.06),
        overlay_hover_strong: hsla(0., 0., 1.0, 0.08),
        overlay_active: hsla(0., 0., 1.0, 0.10),
        focus_ring: hsla(220. / 360., 0.55, 0.60, 1.0),
        success: SemanticColors {
            icon: hsla(138. / 360., 0.52, 0.66, 1.),
            bg: hsla(136. / 360., 0.40, 0.24, 0.90),
            muted: hsla(136. / 360., 0.42, 0.34, 0.55),
            text: hsla(138. / 360., 0.52, 0.66, 1.),
        },
        error: SemanticColors {
            icon: hsla(0., 0.68, 0.72, 1.),
            bg: hsla(0., 0.40, 0.24, 0.90),
            muted: hsla(0., 0.45, 0.36, 0.58),
            text: hsla(0., 0.78, 0.68, 1.),
        },
        warning: SemanticColors {
            icon: hsla(42. / 360., 0.70, 0.68, 1.),
            bg: hsla(40. / 360., 0.42, 0.24, 0.90),
            muted: hsla(42. / 360., 0.46, 0.34, 0.58),
            text: hsla(42. / 360., 0.70, 0.68, 1.),
        },
        info: SemanticColors {
            icon: hsla(208. / 360., 0.62, 0.72, 1.),
            bg: hsla(210. / 360., 0.40, 0.24, 0.90),
            muted: hsla(208. / 360., 0.42, 0.34, 0.58),
            text: hsla(208. / 360., 0.62, 0.72, 1.),
        },
    }
}

pub fn light_theme() -> AppTheme {
    AppTheme {
        resolved: ResolvedTheme::Light,
        chrome_bg: rgb(0xdcddde).into(),
        card_bg: rgb(0xfcfcfc).into(),
        sunken_bg: rgb(0xececed).into(),
        terminal_bg: terminal_background_for_theme(ResolvedTheme::Light),
        scrim_bg: hsla(0., 0., 0., 0.32),
        text_primary: rgb(0x5c6166).into(),
        text_secondary: rgb(0x8b8e92).into(),
        text_muted: rgb(0x8b8e92).into(),
        text_placeholder: rgb(0xa9acae).into(),
        border: rgb(0xcfd1d2).into(),
        divider: rgb(0xdfe0e1).into(),
        overlay_rest: hsla(0., 0., 0., 0.00),
        overlay_hover: rgb(0xdfe0e1).into(),
        overlay_hover_strong: rgb(0xcfd0d2).into(),
        overlay_active: rgb(0xcfd0d2).into(),
        focus_ring: rgb(0x3b9ee5).into(),
        success: SemanticColors {
            icon: rgb(0x85b304).into(),
            bg: rgb(0xe9efd2).into(),
            muted: rgb(0xd7e3ae).into(),
            text: rgb(0x85b304).into(),
        },
        error: SemanticColors {
            icon: rgb(0xef7271).into(),
            bg: rgb(0xffe3e1).into(),
            muted: rgb(0xffcdca).into(),
            text: rgb(0xef7271).into(),
        },
        warning: SemanticColors {
            icon: rgb(0xf1ad49).into(),
            bg: rgb(0xffeeda).into(),
            muted: rgb(0xffe1be).into(),
            text: rgb(0xf1ad49).into(),
        },
        info: SemanticColors {
            icon: rgb(0x3b9ee5).into(),
            bg: rgb(0xdeebfa).into(),
            muted: rgb(0xc4daf6).into(),
            text: rgb(0x3b9ee5).into(),
        },
    }
}

/// Sidebar-toggle icon colour, adapts to the resolved theme.
pub fn toggle_icon_color_for_mode(window: &Window, mode: ThemeMode) -> gpui::Hsla {
    match resolve_theme(window, mode) {
        ResolvedTheme::Dark => hsla(226. / 360., 0.42, 0.72, 0.95),
        ResolvedTheme::Light => hsla(226. / 360., 0.35, 0.38, 0.9),
    }
}

/// Sidebar-toggle icon colour, following the OS appearance.
pub fn toggle_icon_color(window: &Window) -> gpui::Hsla {
    toggle_icon_color_for_mode(window, ThemeMode::System)
}

/// Shared titlebar + sidebar chrome colour for an explicit user mode.
pub fn chrome_bg_for_mode(window: &Window, mode: ThemeMode) -> gpui::Hsla {
    app_theme(window, mode).chrome_bg
}

/// Shared titlebar + sidebar chrome colour, following the OS appearance.
pub fn chrome_bg(window: &Window) -> gpui::Hsla {
    chrome_bg_for_mode(window, ThemeMode::System)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_theme_uses_zed_ayu_light_chrome_values() {
        let theme = light_theme();

        assert_eq!(theme.chrome_bg, rgb(0xdcddde).into());
        assert_eq!(theme.card_bg, rgb(0xfcfcfc).into());
        assert_eq!(theme.sunken_bg, rgb(0xececed).into());
        assert_eq!(theme.text_primary, rgb(0x5c6166).into());
        assert_eq!(theme.text_secondary, rgb(0x8b8e92).into());
        assert_eq!(theme.text_placeholder, rgb(0xa9acae).into());
        assert_eq!(theme.border, rgb(0xcfd1d2).into());
        assert_eq!(theme.divider, rgb(0xdfe0e1).into());
        assert_eq!(theme.focus_ring, rgb(0x3b9ee5).into());
    }
}
