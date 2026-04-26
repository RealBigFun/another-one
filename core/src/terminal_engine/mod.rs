//! Terminal engine abstraction shared across UI shells.
//!
//! The Phase 0 spike replaces `xterm.dart`'s parser with the
//! battle-tested `alacritty_terminal` VT engine on Linux desktop.
//! [`TerminalEngine`] is the seam: today only [`alacritty::AlacrittyEngine`]
//! implements it, but the trait gives mobile a place to plug an
//! `xterm.js` / SwiftTerm engine in later without leaking platform
//! choice into the bridge.
//!
//! Design constraint: every trait method is FRB-friendly. No
//! lifetimes, no closures, no Rc — bytes in, plain-old-data out, so
//! the bridge can hand snapshots back to Dart without copying through
//! a serializer.

pub mod alacritty;

/// VT engine seam. One instance per terminal tab.
///
/// Thread safety: `Send + Sync` so the bridge can stash instances in
/// a `Mutex<Box<dyn TerminalEngine>>` keyed by `(section_id, tab_id)`.
/// Concurrent `write_pty` + `snapshot` is serialized externally; the
/// trait itself doesn't lock.
pub trait TerminalEngine: Send + Sync {
    /// Feed bytes from the PTY into the parser. Caller must not
    /// double-feed; the engine consumes the slice synchronously.
    fn write_pty(&mut self, bytes: &[u8]);

    /// New viewport dimensions in cells. Triggers reflow.
    fn resize(&mut self, cols: u16, rows: u16);

    /// Snapshot the visible grid for the renderer.
    ///
    /// `scrollback_offset` is the number of lines above the viewport
    /// (0 = bottom). `max_rows` caps the number of rows returned —
    /// callers that only paint the visible viewport pass the current
    /// row count.
    fn snapshot(&self, scrollback_offset: u32, max_rows: u16) -> Snapshot;

    /// Convert a UI-side input event to the bytes the PTY expects.
    /// Pure: caller writes the result themselves.
    fn encode_input(&self, event: InputEvent) -> Vec<u8>;

    /// Replace the active selection. `None` clears it. Spike stub —
    /// alacritty supports this but we don't paint selections yet.
    fn selection_set(&mut self, range: Option<SelectionRange>);

    /// Plain-text rendering of the active selection, if any.
    fn selection_text(&self) -> Option<String>;

    /// OSC 8 hyperlink target at `(col, row)` in viewport coords, if
    /// the cell carries one. Spike returns `None` everywhere.
    fn hyperlink_at(&self, col: u16, row: u16) -> Option<String>;

    /// Find every match of `needle` across the visible grid +
    /// scrollback. Default: `Vec::new()` — searched cells aren't
    /// flushed during a spike.
    fn search(&self, _needle: &str) -> Vec<SearchHit> {
        Vec::new()
    }
}

/// Flat, FRB-friendly snapshot of one frame's worth of grid.
///
/// `cells.len() == cols * rows`, row-major. Empty cells encode `ch == 0`.
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub cols: u16,
    pub rows: u16,
    pub cursor: CursorState,
    pub cells: Vec<Cell>,
    pub hyperlinks: Vec<LinkRun>,
    pub revision: u64,
}

/// One grid cell. 16 bytes per cell so a 200x60 viewport fits in
/// ~190 KiB — the FRB encoder copies once on the way to Dart.
#[derive(Debug, Clone, Copy)]
pub struct Cell {
    pub ch: u32,
    pub fg: u32,
    pub bg: u32,
    pub flags: u16,
}

impl Default for Cell {
    fn default() -> Self {
        Self { ch: 0, fg: 0xFFFFFFFF, bg: 0x00000000, flags: 0 }
    }
}

/// Render flags packed into [`Cell::flags`]. Bit assignments are
/// stable contract with the Dart painter; only append new bits at
/// the high end.
pub mod cell_flags {
    pub const BOLD: u16 = 1 << 0;
    pub const ITALIC: u16 = 1 << 1;
    pub const UNDERLINE: u16 = 1 << 2;
    pub const INVERSE: u16 = 1 << 3;
    pub const STRIKETHROUGH: u16 = 1 << 4;
    pub const DIM: u16 = 1 << 5;
}

#[derive(Debug, Clone, Copy)]
pub struct CursorState {
    pub col: u16,
    pub row: u16,
    pub visible: bool,
    pub style: CursorStyle,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum CursorStyle {
    #[default]
    Block,
    Bar,
    Underline,
}

/// Range of cell indices (start..end, half-open) that share a single
/// hyperlink URL. Computed by `Snapshot` so the painter doesn't have
/// to re-scan cells.
#[derive(Debug, Clone)]
pub struct LinkRun {
    pub start_index: u32,
    pub end_index: u32,
    pub url: String,
}

#[derive(Debug, Clone, Copy)]
pub struct SelectionRange {
    pub start_col: u16,
    pub start_row: u16,
    pub end_col: u16,
    pub end_row: u16,
}

#[derive(Debug, Clone)]
pub struct SearchHit {
    pub col: u16,
    pub row: u16,
    pub len: u16,
}

/// UI-originated input event. Kept tight; bracketed-paste / mouse /
/// IME variants land in Phase 2 once the spike clears.
#[derive(Debug, Clone)]
pub enum InputEvent {
    Char(u32),
    Enter,
    Backspace,
    Tab,
    Escape,
    ArrowUp,
    ArrowDown,
    ArrowLeft,
    ArrowRight,
    Resize { cols: u16, rows: u16 },
}
