//! Colour palette, chrome helpers, and theme utilities.

#![allow(dead_code)]

use std::sync::atomic::{AtomicU8, Ordering};

use gpui::{hsla, rgb, Hsla, Window, WindowAppearance};

use crate::project_store::ThemeMode;

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

pub fn terminal_default_background() -> Hsla {
    match current_terminal_theme() {
        ResolvedTheme::Light => rgb(0xfafbfc).into(),
        ResolvedTheme::Dark => rgb(0x1e1f22).into(),
    }
}

pub fn terminal_default_foreground() -> Hsla {
    match current_terminal_theme() {
        ResolvedTheme::Light => rgb(0x1f2328).into(),
        ResolvedTheme::Dark => rgb(0xd7dae0).into(),
    }
}

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
        ThemeMode::System => match window.appearance() {
            WindowAppearance::Dark | WindowAppearance::VibrantDark => ResolvedTheme::Dark,
            _ => ResolvedTheme::Light,
        },
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
        terminal_bg: rgb(0x17191d).into(),
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
        chrome_bg: rgb(0xf3f4f6).into(),
        card_bg: rgb(0xffffff).into(),
        sunken_bg: rgb(0xf6f7f9).into(),
        // Keep terminals dark for the first light-mode pass; ANSI palettes and many CLIs assume it.
        terminal_bg: rgb(0xfafbfc).into(),
        scrim_bg: hsla(0., 0., 0., 0.32),
        text_primary: rgb(0x1f2328).into(),
        text_secondary: rgb(0x4b5563).into(),
        text_muted: rgb(0x6b7280).into(),
        text_placeholder: rgb(0x9ca3af).into(),
        border: rgb(0xd8dee4).into(),
        divider: rgb(0xe5e7eb).into(),
        overlay_rest: hsla(0., 0., 0., 0.03),
        overlay_hover: hsla(0., 0., 0., 0.06),
        overlay_hover_strong: hsla(0., 0., 0., 0.09),
        overlay_active: hsla(0., 0., 0., 0.12),
        focus_ring: hsla(220. / 360., 0.70, 0.48, 1.0),
        success: SemanticColors {
            icon: hsla(150. / 360., 0.65, 0.32, 1.),
            bg: hsla(145. / 360., 0.55, 0.92, 1.),
            muted: hsla(145. / 360., 0.38, 0.76, 1.),
            text: hsla(150. / 360., 0.65, 0.26, 1.),
        },
        error: SemanticColors {
            icon: hsla(0., 0.70, 0.44, 1.),
            bg: hsla(0., 0.76, 0.95, 1.),
            muted: hsla(0., 0.52, 0.80, 1.),
            text: hsla(0., 0.70, 0.40, 1.),
        },
        warning: SemanticColors {
            icon: hsla(38. / 360., 0.82, 0.40, 1.),
            bg: hsla(42. / 360., 0.92, 0.93, 1.),
            muted: hsla(40. / 360., 0.68, 0.75, 1.),
            text: hsla(35. / 360., 0.82, 0.34, 1.),
        },
        info: SemanticColors {
            icon: hsla(210. / 360., 0.75, 0.42, 1.),
            bg: hsla(210. / 360., 0.84, 0.95, 1.),
            muted: hsla(210. / 360., 0.58, 0.78, 1.),
            text: hsla(210. / 360., 0.75, 0.36, 1.),
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
