//! Terminal cell + text-run helpers used by `AlacrittySnapshot`'s
//! `snapshot_surface` to flatten alacritty cells into the
//! `TerminalSurface` Slint UI consumes.
//!
//! GPUI source of truth: `desktop/src/panels.rs` cell-style resolution
//! and run construction. Slint mirrors the same rules so wide cells,
//! grapheme clusters, ZWJ joins, hidden cells, OSC8 link spans, cursor
//! shape mapping, and inverse/dim/bold flag handling all match the GPUI
//! baseline byte-for-byte.

use alacritty_terminal::term::cell::Cell;
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};

use crate::terminal_colors::{default_background_color, resolve_color};
use crate::{TerminalBackgroundSpan, TerminalCursorSpan, TerminalLinkSpan, TerminalTextRun};

#[derive(Clone, PartialEq)]
pub(crate) struct ResolvedCellStyle {
    pub(crate) foreground: u32,
    pub(crate) background: u32,
    pub(crate) bold: bool,
}

pub(crate) struct PendingTerminalRun {
    pub(crate) line: usize,
    pub(crate) column: usize,
    pub(crate) cell_count: usize,
    pub(crate) text: String,
    pub(crate) style: ResolvedCellStyle,
}

pub(crate) fn joins_previous_terminal_grapheme(
    pending_run: &Option<PendingTerminalRun>,
    line: usize,
    text: &str,
    style: &ResolvedCellStyle,
) -> bool {
    let Some(run) = pending_run else {
        return false;
    };

    run.line == line && run.style == *style && !text.is_empty() && run.text.ends_with('\u{200d}')
}

pub(crate) fn append_terminal_run(
    pending_run: &mut Option<PendingTerminalRun>,
    runs: &mut Vec<TerminalTextRun>,
    line: usize,
    column: usize,
    cell_count: usize,
    text: String,
    style: ResolvedCellStyle,
) {
    if let Some(run) = pending_run {
        if run.line == line && run.column + run.cell_count == column && run.style == style {
            run.cell_count += cell_count;
            run.text.push_str(&text);
            return;
        }

        if let Some(finished) = pending_run.take() {
            push_terminal_run(runs, finished);
        }
    }

    *pending_run = Some(PendingTerminalRun {
        line,
        column,
        cell_count,
        text,
        style,
    });
}

pub(crate) fn push_terminal_run(runs: &mut Vec<TerminalTextRun>, run: PendingTerminalRun) {
    runs.push(TerminalTextRun {
        line: to_i32(run.line),
        column: to_i32(run.column),
        cell_count: to_i32(run.cell_count),
        text: run.text.into(),
        color: slint::Color::from_argb_encoded(run.style.foreground),
        bold: run.style.bold,
    });
}

pub(crate) fn maybe_push_background_span(
    spans: &mut Vec<TerminalBackgroundSpan>,
    line: usize,
    column: usize,
    cell_count: usize,
    color: u32,
    force: bool,
) {
    if !force && color == default_background_color() {
        return;
    }

    let line = to_i32(line);
    let column = to_i32(column);
    let cell_count = to_i32(cell_count);
    if let Some(last) = spans.last_mut() {
        if last.line == line
            && last.column + last.cell_count == column
            && last.color.as_argb_encoded() == color
        {
            last.cell_count += cell_count;
            return;
        }
    }

    spans.push(TerminalBackgroundSpan {
        line,
        column,
        cell_count,
        color: slint::Color::from_argb_encoded(color),
    });
}

pub(crate) fn maybe_push_cursor_span(
    spans: &mut Vec<TerminalCursorSpan>,
    line: usize,
    column: usize,
    cell_count: usize,
    shape: CursorShape,
    color: u32,
) {
    let Some(shape) = cursor_shape_name(shape) else {
        return;
    };

    spans.push(TerminalCursorSpan {
        line: to_i32(line),
        column: to_i32(column),
        cell_count: to_i32(cell_count),
        shape: shape.into(),
        color: slint::Color::from_argb_encoded(color),
    });
}

pub(crate) fn cursor_shape_name(shape: CursorShape) -> Option<&'static str> {
    match shape {
        CursorShape::Block | CursorShape::Hidden => None,
        CursorShape::Underline => Some("underline"),
        CursorShape::Beam => Some("beam"),
        CursorShape::HollowBlock => Some("hollow-block"),
    }
}

pub(crate) fn maybe_push_link_span(
    spans: &mut Vec<TerminalLinkSpan>,
    line: usize,
    column: usize,
    cell_count: usize,
    uri: &str,
) {
    let line = to_i32(line);
    let column = to_i32(column);
    let cell_count = to_i32(cell_count);
    if let Some(last) = spans.last_mut() {
        if last.line == line && last.column + last.cell_count == column && last.uri.as_str() == uri
        {
            last.cell_count += cell_count;
            return;
        }
    }

    spans.push(TerminalLinkSpan {
        line,
        column,
        cell_count,
        uri: uri.into(),
    });
}

pub(crate) fn visible_cell_text(cell: &Cell) -> Option<String> {
    if cell.flags.contains(Flags::HIDDEN) || cell_is_render_blank(cell) {
        return None;
    }

    let mut text = String::new();
    text.push(if cell.c == ' ' { '\u{00a0}' } else { cell.c });
    for zero_width in cell.zerowidth().into_iter().flatten() {
        text.push(*zero_width);
    }

    Some(text)
}

pub(crate) fn selected_cell_text(cell: &Cell) -> String {
    if cell.flags.contains(Flags::HIDDEN) {
        return String::new();
    }

    let mut text = String::new();
    text.push(cell.c);
    for zero_width in cell.zerowidth().into_iter().flatten() {
        text.push(*zero_width);
    }

    text
}

pub(crate) fn cell_is_render_blank(cell: &Cell) -> bool {
    if cell.c != ' ' {
        return false;
    }

    if cell.bg != Color::Named(NamedColor::Background) {
        return false;
    }

    !cell
        .flags
        .intersects(Flags::ALL_UNDERLINES | Flags::INVERSE | Flags::STRIKEOUT)
}

pub(crate) fn terminal_cell_width(cell: &Cell) -> usize {
    if cell.flags.contains(Flags::WIDE_CHAR) {
        2
    } else {
        1
    }
}

pub(crate) fn resolve_cell_style(cell: &Cell, colors: &Colors) -> ResolvedCellStyle {
    let mut foreground = resolve_color(cell.fg, cell.flags, true, colors);
    let mut background = resolve_color(cell.bg, cell.flags, false, colors);

    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }

    if cell.flags.contains(Flags::HIDDEN) {
        foreground = background;
    }

    ResolvedCellStyle {
        foreground,
        background,
        bold: cell.flags.contains(Flags::BOLD),
    }
}

pub(crate) fn to_i32(value: usize) -> i32 {
    value.min(i32::MAX as usize) as i32
}
