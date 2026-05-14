//! Conversions from `alacritty_terminal::Term` to
//! `daemon_proto::TerminalFrame::Full`.
//!
//! Phase 2b lands the basic `Full` frame: visible viewport rows, the
//! cursor, and the mode flags renderers care about. Backbuffer is
//! intentionally empty in this commit (the design's 2× viewport
//! backbuffer lands as part of search / scrollback work in
//! Phase 2c, and viewers reach for scrollback proper via
//! `Control::TerminalReadScrollback`). `Diff` emission is Phase 8.

use std::sync::Arc;

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::{Term, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape as VteCursorShape, NamedColor};
use daemon_proto::{
    CursorShape, CursorState, GridCell, GridCellFlags, GridColor, GridRow, GridSnapshot, ModeFlags,
    TerminalFrame,
};

/// Serialize the Term's current visible viewport into a `Full`
/// frame. Caller supplies the monotonic `seq`. The returned `Arc`
/// is the same one the per-tab `watch::Sender<Arc<TerminalFrame>>`
/// will hold so viewers receive a zero-copy frame in-process.
pub(super) fn serialize_full_frame<E: EventListener>(
    term: &Term<E>,
    seq: u64,
) -> Arc<TerminalFrame> {
    Arc::new(TerminalFrame::Full {
        seq,
        snapshot: Arc::new(serialize_snapshot(term)),
    })
}

fn serialize_snapshot<E: EventListener>(term: &Term<E>) -> GridSnapshot {
    let grid = term.grid();
    let cols = grid.columns();
    let rows = grid.screen_lines();

    let viewport = (0..rows)
        .map(|row| serialize_row(term, Line(row as i32)))
        .collect();

    let cursor_point = grid.cursor.point;
    let cursor_style = term.cursor_style();
    // SHOW_CURSOR is set by default; the running TUI clears it via
    // `?25l` when it wants the cursor hidden. The cursor shape's
    // `Hidden` variant is a separate hide-via-DECSCUSR path; either
    // way means "don't draw it".
    let visible = !matches!(cursor_style.shape, VteCursorShape::Hidden)
        && term.mode().contains(TermMode::SHOW_CURSOR);

    let cursor = CursorState {
        line: clamp_u16(cursor_point.line.0),
        col: clamp_u16(cursor_point.column.0 as i32),
        shape: map_cursor_shape(cursor_style.shape, cursor_style.blinking),
        visible,
    };

    let mode = serialize_mode_flags(term.mode());

    let scroll_offset = clamp_u32(grid.display_offset());

    GridSnapshot {
        cols: clamp_u16(cols as i32),
        rows: clamp_u16(rows as i32),
        viewport,
        backbuffer: Vec::new(),
        cursor,
        mode,
        scroll_offset,
    }
}

fn serialize_row<E: EventListener>(term: &Term<E>, line: Line) -> GridRow {
    let grid = term.grid();
    let cols = grid.columns();
    let mut cells = Vec::with_capacity(cols);
    let mut last_non_default = 0_usize;
    for col in 0..cols {
        let cell = &grid[Point::new(line, Column(col))];
        let mapped = serialize_cell(cell);
        if mapped != GridCell::default() {
            last_non_default = col + 1;
        }
        cells.push(mapped);
    }
    // Trim trailing default cells to keep the wire small. The
    // renderer pads with `GridCell::default()` back out to
    // `GridSnapshot::cols`.
    cells.truncate(last_non_default);
    GridRow { cells }
}

fn serialize_cell(cell: &Cell) -> GridCell {
    GridCell {
        ch: if cell.c == '\0' { ' ' } else { cell.c },
        fg: serialize_color(cell.fg),
        bg: serialize_color(cell.bg),
        flags: serialize_flags(cell.flags),
    }
}

fn serialize_color(color: Color) -> GridColor {
    match color {
        Color::Spec(rgb) => GridColor::Rgb {
            r: rgb.r,
            g: rgb.g,
            b: rgb.b,
        },
        Color::Indexed(i) => GridColor::Indexed { index: i },
        Color::Named(named) => match named {
            // Renderer-themable defaults — let the viewer's theme
            // pick the actual pixels.
            NamedColor::Foreground
            | NamedColor::Background
            | NamedColor::Cursor
            | NamedColor::BrightForeground
            | NamedColor::DimForeground => GridColor::Default,
            // Other named colors (the 8 standard + 8 bright + dim
            // variants) are stable indexed entries; encode as
            // indexed so the viewer's palette resolves them.
            other => GridColor::Indexed {
                index: other as u8,
            },
        },
    }
}

fn serialize_flags(flags: Flags) -> GridCellFlags {
    let mut out: u16 = 0;
    if flags.contains(Flags::INVERSE) {
        out |= GridCellFlags::INVERSE;
    }
    if flags.contains(Flags::BOLD) {
        out |= GridCellFlags::BOLD;
    }
    if flags.contains(Flags::ITALIC) {
        out |= GridCellFlags::ITALIC;
    }
    if flags.contains(Flags::UNDERLINE) {
        out |= GridCellFlags::UNDERLINE;
    }
    if flags.contains(Flags::WRAPLINE) {
        out |= GridCellFlags::WRAPLINE;
    }
    if flags.contains(Flags::WIDE_CHAR) {
        out |= GridCellFlags::WIDE_CHAR;
    }
    if flags.contains(Flags::WIDE_CHAR_SPACER) {
        out |= GridCellFlags::WIDE_CHAR_SPACER;
    }
    if flags.contains(Flags::DIM) {
        out |= GridCellFlags::DIM;
    }
    if flags.contains(Flags::HIDDEN) {
        out |= GridCellFlags::HIDDEN;
    }
    if flags.contains(Flags::STRIKEOUT) {
        out |= GridCellFlags::STRIKEOUT;
    }
    if flags.contains(Flags::LEADING_WIDE_CHAR_SPACER) {
        out |= GridCellFlags::LEADING_WIDE_CHAR_SPACER;
    }
    if flags.contains(Flags::DOUBLE_UNDERLINE) {
        out |= GridCellFlags::DOUBLE_UNDERLINE;
    }
    if flags.contains(Flags::UNDERCURL) {
        out |= GridCellFlags::UNDERCURL;
    }
    if flags.contains(Flags::DOTTED_UNDERLINE) {
        out |= GridCellFlags::DOTTED_UNDERLINE;
    }
    if flags.contains(Flags::DASHED_UNDERLINE) {
        out |= GridCellFlags::DASHED_UNDERLINE;
    }
    GridCellFlags(out)
}

fn map_cursor_shape(shape: VteCursorShape, blinking: bool) -> CursorShape {
    match (shape, blinking) {
        (VteCursorShape::Block, false) => CursorShape::Block,
        (VteCursorShape::Block, true) => CursorShape::BlockBlinking,
        // alacritty's `HollowBlock` (cursor outline only) renders
        // closest to a block on the wire; viewers can still pick
        // their own outline rendering. Promoting to a typed
        // `HollowBlock` variant if needed is a wire-additive
        // change later.
        (VteCursorShape::HollowBlock, false) => CursorShape::Block,
        (VteCursorShape::HollowBlock, true) => CursorShape::BlockBlinking,
        (VteCursorShape::Underline, false) => CursorShape::Underline,
        (VteCursorShape::Underline, true) => CursorShape::UnderlineBlinking,
        (VteCursorShape::Beam, false) => CursorShape::Beam,
        (VteCursorShape::Beam, true) => CursorShape::BeamBlinking,
        (VteCursorShape::Hidden, _) => CursorShape::Block,
    }
}

fn serialize_mode_flags(mode: &TermMode) -> ModeFlags {
    ModeFlags {
        alt_screen: mode.contains(TermMode::ALT_SCREEN),
        bracketed_paste: mode.contains(TermMode::BRACKETED_PASTE),
        mouse_motion: mode.contains(TermMode::MOUSE_MOTION),
        mouse_drag: mode.contains(TermMode::MOUSE_DRAG),
        mouse_click: mode.contains(TermMode::MOUSE_REPORT_CLICK),
        sgr_mouse: mode.contains(TermMode::SGR_MOUSE),
        utf8_mouse: mode.contains(TermMode::UTF8_MOUSE),
    }
}

fn clamp_u16(v: i32) -> u16 {
    v.try_into().unwrap_or(0)
}

fn clamp_u32(v: usize) -> u32 {
    v.try_into().unwrap_or(u32::MAX)
}
