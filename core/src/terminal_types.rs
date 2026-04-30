//! Shared data types for terminal launch + runtime.
//!
//! These sit in core so both the launcher (pure PTY spawning, also in
//! core) and the runtime renderer (GPUI-coupled, in desktop) can
//! reference them without one side reaching into the other.
//!
//! Nothing here touches GPUI. `TerminalGridSize` does implement
//! `alacritty_terminal::Dimensions` so both the launcher and the
//! renderer can feed the same value to alacritty, but alacritty_terminal
//! itself is portable.

use std::io::Write;

use alacritty_terminal::grid::Dimensions;
use portable_pty::{ChildKiller, MasterPty, PtySize};

use crate::section::SectionId;

/// Clamp floor: terminal must be at least 20 cols wide so prompts render.
/// Crate-private — only `from_panel_size` uses it.
const MIN_TERMINAL_COLS: u16 = 20;

/// Clamp floor: at least 4 rows tall. Crate-private.
const MIN_TERMINAL_ROWS: u16 = 4;

/// How wide one cell is, as a fraction of the font size. Tuned to match
/// Lilex NerdFont Mono at the default weight. `pub` because the desktop
/// renderer uses the same ratio when sizing its grid.
pub const TERMINAL_CELL_WIDTH_RATIO: f32 = 0.62;

/// How tall one line is, as a multiple of the font size. `pub` for the
/// same reason.
pub const TERMINAL_LINE_HEIGHT_RATIO: f32 = 1.25;

/// Stable handle for a live terminal: the section it belongs to + the
/// tab id. Used as a map key across launch/runtime/app state.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct TerminalRuntimeKey {
    pub section_id: SectionId,
    pub tab_id: String,
}

/// Grid size in both character units (cols/rows for the VT state
/// machine) and pixel units (for the rendering layer). The launcher
/// sets this at spawn time; the renderer may update it on resize.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TerminalGridSize {
    pub cols: u16,
    pub rows: u16,
    pub pixel_width: u16,
    pub pixel_height: u16,
}

impl Default for TerminalGridSize {
    fn default() -> Self {
        Self {
            cols: 80,
            rows: 24,
            pixel_width: 0,
            pixel_height: 0,
        }
    }
}

impl TerminalGridSize {
    /// Derive a grid size from a pixel-sized panel and the current font
    /// size. Clamped to [`MIN_TERMINAL_COLS`] / [`MIN_TERMINAL_ROWS`]
    /// so a tiny panel doesn't produce a 0x0 PTY.
    pub fn from_panel_size(width_px: f32, height_px: f32, font_size: f32) -> Self {
        let cell_width = (font_size * TERMINAL_CELL_WIDTH_RATIO).max(7.0);
        let cell_height = (font_size * TERMINAL_LINE_HEIGHT_RATIO).max(14.0);
        let cols = (width_px / cell_width)
            .floor()
            .max(MIN_TERMINAL_COLS as f32) as u16;
        let rows = (height_px / cell_height)
            .floor()
            .max(MIN_TERMINAL_ROWS as f32) as u16;
        Self {
            cols,
            rows,
            pixel_width: width_px.max(0.0).min(u16::MAX as f32) as u16,
            pixel_height: height_px.max(0.0).min(u16::MAX as f32) as u16,
        }
    }

    pub fn as_pty_size(self) -> PtySize {
        PtySize {
            rows: self.rows,
            cols: self.cols,
            pixel_width: self.pixel_width,
            pixel_height: self.pixel_height,
        }
    }
}

impl Dimensions for TerminalGridSize {
    fn total_lines(&self) -> usize {
        self.rows as usize
    }

    fn screen_lines(&self) -> usize {
        self.rows as usize
    }

    fn columns(&self) -> usize {
        self.cols as usize
    }
}

/// The launch-time result of spawning a PTY: the size the shell was
/// started at plus owning handles to read/write/kill it. Desktop wraps
/// this in a `LiveTerminalRuntime` that drives an alacritty `Term` +
/// renders into GPUI.
pub struct PreparedTerminalRuntime {
    pub size: TerminalGridSize,
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child_killer: Box<dyn ChildKiller + Send + Sync>,
    /// Broadcast tee of every byte read from the master PTY. The
    /// existing mpsc `TerminalLaunchReply::Output` path stays as-is;
    /// embedders that need remote viewers forward those output chunks
    /// through this sender after any replay bookkeeping.
    /// `send()` is non-blocking and returns Ok(subscriber_count) or
    /// Err when there are no receivers — lagged mobile subscribers
    /// just drop chunks rather than stalling the reader.
    pub output_broadcast: tokio::sync::broadcast::Sender<Vec<u8>>,
}
