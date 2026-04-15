//! Colour palette, chrome helpers, and theme utilities.

use gpui::{hsla, rgb, Window, WindowAppearance};

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

/// Sidebar-toggle icon colour, adapts to window appearance.
pub fn toggle_icon_color(window: &Window) -> gpui::Hsla {
    match window.appearance() {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => {
            // Soft periwinkle / cool accent on dark chrome.
            hsla(226. / 360., 0.42, 0.72, 0.95)
        }
        _ => hsla(226. / 360., 0.35, 0.38, 0.9),
    }
}

/// Shared dark charcoal used for titlebar + sidebar chrome.
pub fn chrome_bg(window: &Window) -> gpui::Hsla {
    match window.appearance() {
        WindowAppearance::Dark | WindowAppearance::VibrantDark => rgb(0x27292e).into(),
        _ => rgb(0x27292e).into(),
    }
}
