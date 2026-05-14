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
    CursorShape, CursorState, GridCell, GridCellFlags, GridColor, GridMatch, GridRow,
    GridSnapshot, ModeFlags, ScrollbackRange, TerminalCaseFold, TerminalFrame,
    TerminalScrollbackReply, TerminalSearchRequest, TerminalSearchReply,
};

/// Serialize the Term's current visible viewport into a `Full`
/// frame. Caller supplies the monotonic `seq`. The returned `Arc`
/// is the same one the per-tab `watch::Sender<Arc<TerminalFrame>>`
/// will hold so viewers receive a zero-copy frame in-process.
pub fn serialize_full_frame<E: EventListener>(
    term: &Term<E>,
    seq: u64,
) -> Arc<TerminalFrame> {
    Arc::new(TerminalFrame::Full {
        seq,
        snapshot: Arc::new(serialize_snapshot(term)),
    })
}

/// Serialize the Term's current visible viewport into a
/// [`GridSnapshot`]. Public so renderer-side tests can drive the
/// canonical alacritty→proto pipeline without reaching into the
/// per-tab task plumbing.
pub fn serialize_snapshot<E: EventListener>(term: &Term<E>) -> GridSnapshot {
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
    let history_lines = clamp_u32(grid.history_size());

    GridSnapshot {
        cols: clamp_u16(cols as i32),
        rows: clamp_u16(rows as i32),
        viewport,
        backbuffer: Vec::new(),
        cursor,
        mode,
        scroll_offset,
        history_lines,
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
    let zero_width: Vec<char> = cell
        .zerowidth()
        .map(|chars| chars.iter().copied().collect())
        .unwrap_or_default();
    GridCell {
        ch: if cell.c == '\0' { ' ' } else { cell.c },
        fg: serialize_color(cell.fg),
        bg: serialize_color(cell.bg),
        flags: serialize_flags(cell.flags),
        underline_color: cell
            .underline_color()
            .map(serialize_color)
            .unwrap_or(GridColor::Default),
        hyperlink: cell.hyperlink().map(|link| link.uri().to_string()),
        zero_width,
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
        // alacritty's `HollowBlock` (cursor outline only) maps to
        // the dedicated wire variant so viewers can render the
        // outline-only cursor (typical when the window loses
        // focus). Older clients that don't recognise the variant
        // can collapse it back to `Block` themselves.
        (VteCursorShape::HollowBlock, false) => CursorShape::HollowBlock,
        (VteCursorShape::HollowBlock, true) => CursorShape::HollowBlockBlinking,
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

/// Snapshot a slice of the daemon's scrollback history. Rows return
/// oldest-first. `range.start = 0` is the topmost line of the live
/// screen; increasing `start` walks into the past. Returns an
/// `actual` range describing what was returned (may be shorter than
/// requested if the request runs off the end of the history).
pub(super) fn read_scrollback<E: EventListener>(
    term: &Term<E>,
    range: ScrollbackRange,
) -> TerminalScrollbackReply {
    let grid = term.grid();
    let history = grid.history_size();
    // Live-screen rows are addressable as `Line(0)` through
    // `Line(rows - 1)`; rows above the live screen are
    // `Line(-1)`, `Line(-2)`, ..., `Line(-history)`. Map
    // `start = 0` -> `Line(0)` so the wire range stays positive
    // and intuitive ("give me 5 rows starting from the top of the
    // live screen and walking back through scrollback").
    let start = range.start as i32;
    let count = range.count as usize;
    // Highest valid `offset`: the oldest scrollback row,
    // `Line(-history)`. Offsets above that walk off the end of
    // the grid — stop early and report what we returned.
    let max_offset = history as i32;
    let mut rows = Vec::with_capacity(count);
    let mut returned = 0_u32;
    for i in 0..count {
        let offset = start + i as i32;
        if offset > max_offset {
            break;
        }
        let line_index = -offset; // 0 -> Line(0), 1 -> Line(-1), ...
        let line = Line(line_index);
        rows.push(serialize_row(term, line));
        returned += 1;
    }
    TerminalScrollbackReply {
        rows,
        range_actual: ScrollbackRange {
            start: range.start,
            count: returned,
        },
    }
}

/// Search the visible viewport + scrollback for matches against
/// `request`. Returns matches in grid coordinates with `line` signed:
/// negative values are scrollback above the live screen,
/// `0..screen_lines` is the live viewport.
pub(super) fn search<E: EventListener>(
    term: &Term<E>,
    request: &TerminalSearchRequest,
) -> TerminalSearchReply {
    let grid = term.grid();
    let cols = grid.columns();
    let history = grid.history_size() as i32;
    let screen = grid.screen_lines() as i32;

    let matcher = match build_matcher(request) {
        Some(m) => m,
        None => return TerminalSearchReply { matches: Vec::new() },
    };

    // Walk every line, oldest-first: -history .. (screen - 1).
    let mut matches = Vec::new();
    for line_index in -history..screen {
        let line = Line(line_index);
        // Build (text, byte_offset -> column_index) so a regex byte
        // span can be mapped back to the cell range. Skip
        // wide-char spacer cells so a match doesn't span a half-cell.
        let mut text = String::with_capacity(cols);
        let mut byte_to_col = Vec::with_capacity(cols);
        for col in 0..cols {
            let cell = &grid[Point::new(line, Column(col))];
            if cell.flags.contains(Flags::WIDE_CHAR_SPACER)
                || cell.flags.contains(Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            let ch = if cell.c == '\0' { ' ' } else { cell.c };
            byte_to_col.push((text.len(), col as u16));
            text.push(ch);
        }
        // Sentinel for the end-of-line byte offset.
        byte_to_col.push((text.len(), cols as u16));

        for (start_byte, end_byte) in matcher.find_all(&text) {
            let start_col = byte_offset_to_col(&byte_to_col, start_byte);
            let end_col = byte_offset_to_col(&byte_to_col, end_byte);
            matches.push(GridMatch {
                line: line_index as i64,
                start_col,
                end_col,
            });
        }
    }
    TerminalSearchReply { matches }
}

fn byte_offset_to_col(map: &[(usize, u16)], byte: usize) -> u16 {
    // Linear scan; cols are bounded by terminal width (low
    // hundreds) so binary-searching wouldn't pay off.
    let mut last = 0_u16;
    for &(b, c) in map {
        if b > byte {
            return last;
        }
        last = c;
    }
    last
}

enum Matcher {
    Literal { needle: String },
    LiteralCi { needle: String },
    Regex(regex::Regex),
}

impl Matcher {
    fn find_all<'a>(&'a self, haystack: &'a str) -> Vec<(usize, usize)> {
        match self {
            Matcher::Literal { needle } => {
                if needle.is_empty() {
                    return Vec::new();
                }
                let mut hits = Vec::new();
                let mut start = 0;
                while let Some(idx) = haystack[start..].find(needle.as_str()) {
                    let abs = start + idx;
                    hits.push((abs, abs + needle.len()));
                    start = abs + needle.len().max(1);
                }
                hits
            }
            Matcher::LiteralCi { needle } => {
                if needle.is_empty() {
                    return Vec::new();
                }
                let lower_hay = haystack.to_lowercase();
                let mut hits = Vec::new();
                let mut start = 0;
                while let Some(idx) = lower_hay[start..].find(needle.as_str()) {
                    let abs = start + idx;
                    hits.push((abs, abs + needle.len()));
                    start = abs + needle.len().max(1);
                }
                hits
            }
            Matcher::Regex(re) => re
                .find_iter(haystack)
                .map(|m| (m.start(), m.end()))
                .collect(),
        }
    }
}

fn build_matcher(request: &TerminalSearchRequest) -> Option<Matcher> {
    if request.pattern.is_empty() {
        return None;
    }
    if request.regex {
        let mut builder = regex::RegexBuilder::new(&request.pattern);
        builder.case_insensitive(matches!(request.case_fold, TerminalCaseFold::Insensitive));
        match builder.build() {
            Ok(re) => Some(Matcher::Regex(re)),
            Err(_) => None,
        }
    } else {
        match request.case_fold {
            TerminalCaseFold::Sensitive => Some(Matcher::Literal {
                needle: request.pattern.clone(),
            }),
            TerminalCaseFold::Insensitive => Some(Matcher::LiteralCi {
                needle: request.pattern.to_lowercase(),
            }),
        }
    }
}
