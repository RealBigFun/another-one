//! Layout constants, gutter enum, and constraint logic.

use gpui::Window;

use crate::app::ThreeColumnApp;

/// Horizontal space before the sidebar toggle (clears macOS traffic lights).
#[cfg(target_os = "macos")]
pub const TRAFFIC_LIGHT_PAD: f32 = 76.;
#[cfg(not(target_os = "macos"))]
pub const TRAFFIC_LIGHT_PAD: f32 = 12.;

/// Extra gap after the reserved traffic-light region so the icon sits further right.
#[cfg(target_os = "macos")]
pub const TOGGLE_LEFT_MARGIN: f32 = 0.;

pub const GUTTER: f32 = 4.;
pub const SIDEBAR_COLLAPSED: f32 = 4.;
pub const SIDEBAR_MIN_OPEN: f32 = 160.;
pub const RIGHT_SIDEBAR_COLLAPSED: f32 = 4.;
pub const MIN_MAIN: f32 = 200.;
pub const MIN_RIGHT: f32 = 140.;
pub const RIGHT_SIDEBAR_MIN_OPEN: f32 = MIN_RIGHT;

/// Title strip under unified titlebar (macOS full-size content). Slightly taller so the
/// traffic-light cluster has more vertical room.
pub const TITLEBAR_CHROME_H: f32 = 38.;
pub const SIDEBAR_TOOLBAR_H: f32 = 40.;
pub const FOOTER_H: f32 = TITLEBAR_CHROME_H;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gutter {
    Left,
    Right,
}

impl ThreeColumnApp {
    pub fn sidebar_is_open(&self) -> bool {
        self.sidebar_w > SIDEBAR_COLLAPSED + 8.
    }

    pub fn right_sidebar_is_open(&self) -> bool {
        self.right_w > RIGHT_SIDEBAR_COLLAPSED + 8.
    }

    pub fn content_width(&self, window: &Window) -> f32 {
        f32::from(window.bounds().size.width)
    }

    pub fn clamp_layout(&mut self, window: &Window) {
        let ww = self.content_width(window);
        let min_total = SIDEBAR_COLLAPSED + GUTTER + MIN_MAIN + GUTTER + RIGHT_SIDEBAR_COLLAPSED;
        if ww < min_total {
            return;
        }
        let max_sidebar = (ww - GUTTER - MIN_MAIN - GUTTER - MIN_RIGHT).max(SIDEBAR_COLLAPSED);
        let max_right =
            (ww - GUTTER - MIN_MAIN - GUTTER - SIDEBAR_COLLAPSED).max(RIGHT_SIDEBAR_COLLAPSED);
        self.sidebar_w = self.sidebar_w.clamp(SIDEBAR_COLLAPSED, max_sidebar);
        self.right_w = self.right_w.clamp(RIGHT_SIDEBAR_COLLAPSED, max_right);
        let main = ww - self.sidebar_w - self.right_w - GUTTER * 2.;
        if main < MIN_MAIN {
            let deficit = MIN_MAIN - main;
            let take = deficit.min(self.right_w - RIGHT_SIDEBAR_COLLAPSED);
            self.right_w -= take;
            let rest = deficit - take;
            self.sidebar_w = (self.sidebar_w - rest).max(SIDEBAR_COLLAPSED);
        }
    }
}
