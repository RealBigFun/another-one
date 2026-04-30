//! Layout constants, gutter enum, and constraint logic.

use gpui::Window;

use crate::app::AnotherOneApp;

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
pub const FOOTER_H: f32 = TITLEBAR_CHROME_H;
pub const TERMINAL_TAB_BAR_H: f32 = 36.;
pub const TERMINAL_VIEW_PADDING: f32 = 12.;
pub const MAIN_PANE_BOTTOM_PAD: f32 = 8.;

/// Width below which the UI switches from the desktop three-column layout to
/// a phone-style master/detail navigation stack. Driven by the live viewport
/// (not `cfg(target_os)`) so resizing a desktop window narrow exercises the
/// same code path as a phone — there is exactly one source of truth for what
/// "mobile" means.
pub const NARROW_BREAKPOINT: f32 = 720.;

/// Phone header / app-bar height on narrow layouts. ~44dp matches Material's
/// touch-target guideline.
pub const PHONE_HEADER_H: f32 = 44.;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LayoutMode {
    /// Three-column desktop layout (left sidebar | workspace | right sidebar).
    Wide,
    /// Single-pane master/detail stack — used on phones and on narrowed
    /// desktop windows. The active pane is selected by
    /// `AnotherOneApp::mobile_view`.
    Narrow,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Gutter {
    Left,
    Right,
}

impl AnotherOneApp {
    pub fn sidebar_is_open(&self) -> bool {
        self.sidebar_w > SIDEBAR_COLLAPSED + 8.
    }

    pub fn right_sidebar_is_open(&self) -> bool {
        self.right_w > RIGHT_SIDEBAR_COLLAPSED + 8.
    }

    pub fn content_width(&self, window: &Window) -> f32 {
        f32::from(window.bounds().size.width)
    }

    pub fn layout_mode(&self, window: &Window) -> LayoutMode {
        if self.content_width(window) < NARROW_BREAKPOINT {
            LayoutMode::Narrow
        } else {
            LayoutMode::Wide
        }
    }

    pub fn is_narrow(&self, window: &Window) -> bool {
        matches!(self.layout_mode(window), LayoutMode::Narrow)
    }

    pub fn clamp_layout(&mut self, window: &Window) {
        // Narrow mode owns its own pane state (`mobile_view`) and ignores
        // `sidebar_w`/`right_w` while it is active. Skipping the desktop clamp
        // here preserves whatever widths the user had before the window was
        // resized into narrow mode, so resizing back to wide restores the
        // original three-column proportions exactly.
        if self.is_narrow(window) {
            return;
        }
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
