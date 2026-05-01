use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{point_to_viewport, viewport_to_point, Config, Term};
use alacritty_terminal::vte::ansi::{self, Color, CursorShape, NamedColor, Rgb};
use gpui::{font, px, rgb, FontWeight, Hsla, StrikethroughStyle, TextRun, UnderlineStyle};
use portable_pty::MasterPty;

// Shared with `core/src/terminal_types.rs` — the launcher side also
// produces `PreparedTerminalRuntime` / `TerminalGridSize` /
// `TerminalRuntimeKey`, and both sides need the clamp constants and
// cell/line ratios in lockstep.
pub(crate) use another_one_core::terminal_types::{
    PreparedTerminalRuntime, TerminalChildKiller, TerminalGridSize, TerminalRuntimeKey,
    TERMINAL_CELL_WIDTH_RATIO, TERMINAL_LINE_HEIGHT_RATIO,
};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalSurfaceSnapshot {
    pub text: String,
    pub columns: usize,
    pub lines: Vec<TerminalLineSnapshot>,
    pub positioned_runs: Vec<TerminalPositionedRunSnapshot>,
    pub cursor: Option<TerminalCursorSnapshot>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalLineSnapshot {
    pub text: String,
    pub cells: Vec<TerminalCellSnapshot>,
    pub runs: Vec<TextRun>,
    pub background_spans: Vec<TerminalBackgroundSpanSnapshot>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCellSnapshot {
    pub column: usize,
    pub width: usize,
    pub text: String,
    pub copy_text: String,
    pub hyperlink: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalPositionedRunSnapshot {
    pub line: usize,
    pub column: usize,
    pub cell_count: usize,
    pub text: String,
    pub style: TextRun,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalBackgroundSpanSnapshot {
    pub column: usize,
    pub width: usize,
    pub color: Hsla,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalCursorKind {
    Block,
    HollowBlock,
    Beam,
    Underline,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct TerminalCursorSnapshot {
    pub line: usize,
    pub column: usize,
    pub width: usize,
    pub kind: TerminalCursorKind,
    pub color: Hsla,
}

#[derive(Clone)]
struct ResolvedCellStyle {
    foreground: Hsla,
    background: Hsla,
    font: gpui::Font,
    underline: Option<UnderlineStyle>,
    strikethrough: Option<StrikethroughStyle>,
}

#[derive(Default)]
struct PendingTextRun {
    text: String,
    len: usize,
    style: Option<ResolvedCellStyle>,
}

struct PendingPositionedRun {
    line: usize,
    column: usize,
    cell_count: usize,
    text: String,
    style: TextRun,
}

#[derive(Default)]
pub(crate) struct TerminalRuntimeUpdate {
    pub title: Option<String>,
    pub reset_title: bool,
}

#[derive(Clone)]
struct RuntimeEventProxy {
    queue: Arc<Mutex<VecDeque<Event>>>,
}

impl EventListener for RuntimeEventProxy {
    fn send_event(&self, event: Event) {
        match self.queue.lock() {
            Ok(mut queue) => queue.push_back(event),
            Err(error) => eprintln!("failed to queue terminal runtime event: {error}"),
        }
    }
}

pub(crate) struct LiveTerminalRuntime {
    term: Term<RuntimeEventProxy>,
    parser: ansi::Processor,
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child_killer: TerminalChildKiller,
    event_queue: Arc<Mutex<VecDeque<Event>>>,
    size: TerminalGridSize,
    dirty: bool,
    cached_snapshot: TerminalSurfaceSnapshot,
    /// Kept around so `daemon_host` can clone it on attach (including
    /// the deferred warm-launch attach path, where the runtime is
    /// stashed on the prewarm slot and later moved into the live map
    /// under a real `TerminalRuntimeKey`). The reader thread retains
    /// its own clone for pushing bytes; this copy is read-only from
    /// the registry's perspective.
    output_broadcast: tokio::sync::broadcast::Sender<Vec<u8>>,
}

impl LiveTerminalRuntime {
    pub fn from_prepared(prepared: PreparedTerminalRuntime) -> Self {
        let event_queue = Arc::new(Mutex::new(VecDeque::new()));
        let event_proxy = RuntimeEventProxy {
            queue: event_queue.clone(),
        };
        let term = Term::new(Config::default(), &prepared.size, event_proxy);
        Self {
            term,
            parser: ansi::Processor::default(),
            master: prepared.master,
            writer: Arc::new(Mutex::new(prepared.writer)),
            child_killer: prepared.child_killer,
            event_queue,
            size: prepared.size,
            dirty: true,
            cached_snapshot: TerminalSurfaceSnapshot::default(),
            output_broadcast: prepared.output_broadcast,
        }
    }

    /// Clone of the PTY byte broadcast sender that
    /// `core::terminal_launch` tees every read into. The embedded
    /// daemon subscribes to this to forward bytes to mobile clients.
    pub fn output_broadcast(&self) -> tokio::sync::broadcast::Sender<Vec<u8>> {
        self.output_broadcast.clone()
    }

    pub fn resize(&mut self, size: TerminalGridSize) -> anyhow::Result<bool> {
        if self.size == size {
            return Ok(false);
        }

        self.master.resize(size.as_pty_size())?;
        self.term.resize(size);
        self.size = size;
        self.dirty = true;
        Ok(true)
    }

    pub fn apply_output(&mut self, bytes: &[u8]) -> TerminalRuntimeUpdate {
        self.parser.advance(&mut self.term, bytes);
        self.dirty = true;

        let mut update = TerminalRuntimeUpdate::default();
        let mut pty_writes = Vec::new();

        if let Ok(mut queue) = self.event_queue.lock() {
            while let Some(event) = queue.pop_front() {
                match event {
                    Event::Title(title) => update.title = Some(title),
                    Event::ResetTitle => update.reset_title = true,
                    Event::PtyWrite(text) => pty_writes.push(text.into_bytes()),
                    Event::ColorRequest(index, formatter) => {
                        let color = resolve_color_request(index, self.term.colors());
                        pty_writes.push(formatter(color).into_bytes());
                    }
                    Event::TextAreaSizeRequest(formatter) => {
                        pty_writes.push(formatter(window_size_from_grid(self.size)).into_bytes());
                    }
                    Event::Wakeup
                    | Event::Bell
                    | Event::MouseCursorDirty
                    | Event::CursorBlinkingChange
                    | Event::ClipboardStore(_, _)
                    | Event::ClipboardLoad(_, _)
                    | Event::Exit
                    | Event::ChildExit(_) => {}
                }
            }
        }

        for bytes in pty_writes {
            let _ = self.write_input(&bytes);
        }

        update
    }

    pub fn write_input(&self, bytes: &[u8]) -> io::Result<()> {
        let mut writer = self
            .writer
            .lock()
            .map_err(|_| io::Error::other("terminal writer lock poisoned"))?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    pub fn paste_text(&self, text: &str) -> io::Result<()> {
        let payload = if self
            .term
            .mode()
            .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE)
        {
            format!("\u{1b}[200~{}\u{1b}[201~", text)
        } else {
            text.to_string()
        };
        self.write_input(payload.as_bytes())
    }

    pub fn is_alternate_screen(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
    }

    pub fn request_soft_redraw(&self) -> io::Result<()> {
        self.write_input(b"\x0c")
    }

    pub fn snapshot(&mut self) -> TerminalSurfaceSnapshot {
        if self.dirty {
            self.cached_snapshot = build_surface_snapshot(&self.term, self.size);
            self.dirty = false;
        }
        self.cached_snapshot.clone()
    }

    /// Does this runtime have accumulated output the snapshot hasn't
    /// caught up with yet? Used by the drain loop to decide whether a
    /// focused-but-previously-backgrounded tab needs a rebuild this tick.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    pub fn scroll_display(&mut self, lines: i32) -> bool {
        if lines == 0 {
            return false;
        }

        let old_display_offset = self.term.grid().display_offset();
        self.term.scroll_display(Scroll::Delta(lines));
        let changed = self.term.grid().display_offset() != old_display_offset;
        if changed {
            self.dirty = true;
        }
        changed
    }

    pub fn kill(&mut self) {
        #[cfg(unix)]
        if let Some(process_group_id) = self.master.process_group_leader() {
            self.child_killer.add_process_group(process_group_id);
        }
        let _ = self.child_killer.kill();
    }

    /// Clone the shared writer handle. The embedded daemon's
    /// `DaemonRegistry` clones this on tab launch so incoming mobile
    /// keystrokes can feed into the same `Arc<Mutex<…>>` the desktop
    /// writes to. See `daemon_host::DesktopTerminalRegistry::tab_input`.
    pub fn writer_handle(&self) -> Arc<Mutex<Box<dyn Write + Send>>> {
        self.writer.clone()
    }
}

fn build_surface_snapshot<T: EventListener>(
    term: &Term<T>,
    size: TerminalGridSize,
) -> TerminalSurfaceSnapshot {
    let renderable = term.renderable_content();
    let display_offset = renderable.display_offset;
    let cursor = (renderable.cursor.shape != CursorShape::Hidden)
        .then(|| point_to_viewport(display_offset, renderable.cursor.point))
        .flatten();
    let mut lines = Vec::with_capacity(size.rows as usize);
    let mut positioned_runs = Vec::new();
    let mut cursor_snapshot = None;

    for viewport_line in 0..size.rows as usize {
        let point = viewport_to_point(display_offset, Point::new(viewport_line, Column(0)));
        let grid_line = &term.grid()[point.line];
        let mut text = String::new();
        let mut cells = Vec::new();
        let mut runs = Vec::new();
        let mut background_spans = Vec::new();
        let mut pending_blank_run = PendingTextRun::default();
        let mut pending_positioned_run: Option<PendingPositionedRun> = None;
        let mut previous_cell_had_zero_width = false;

        for column in 0..size.cols as usize {
            let cell = &grid_line[Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            if cell.c == ' ' && previous_cell_had_zero_width {
                previous_cell_had_zero_width = false;
                continue;
            }
            previous_cell_had_zero_width =
                matches!(cell.zerowidth(), Some(chars) if !chars.is_empty());

            let is_cursor = cursor
                .is_some_and(|cursor| cursor.line == viewport_line && cursor.column.0 == column);
            let mut cell_style = resolve_cell_style(cell, renderable.colors);
            let chunk = cell_display_text(cell);
            let copy_text = cell_copy_text(cell);
            let cell_width = terminal_cell_width(cell);
            if chunk.is_empty() {
                continue;
            }

            maybe_push_background_span(
                &mut background_spans,
                column,
                cell_width,
                cell_style.background,
            );
            cells.push(TerminalCellSnapshot {
                column,
                width: cell_width,
                text: chunk.clone(),
                copy_text,
                hyperlink: cell.hyperlink().map(|link| link.uri().to_string()),
            });

            if is_cursor {
                if let Some(snapshot) = cursor_snapshot_from_cell(
                    viewport_line,
                    column,
                    cell_width,
                    renderable.cursor.shape,
                    &cell_style,
                ) {
                    if snapshot.kind == TerminalCursorKind::Block {
                        cell_style.foreground = cell_style.background;
                    }
                    cursor_snapshot = Some(snapshot);
                }
            }

            let positioned_style = text_run_from_style(cell_style.clone());
            if !cell_is_render_blank(cell) {
                let mut char_text = String::new();
                char_text.push(cell.c);
                for zero_width in cell.zerowidth().into_iter().flatten() {
                    char_text.push(*zero_width);
                }
                append_positioned_run(
                    &mut pending_positioned_run,
                    &mut positioned_runs,
                    viewport_line,
                    column,
                    cell_width,
                    char_text,
                    positioned_style,
                );
            } else if let Some(batch) = pending_positioned_run.take() {
                positioned_runs.push(TerminalPositionedRunSnapshot {
                    line: batch.line,
                    column: batch.column,
                    cell_count: batch.cell_count,
                    text: batch.text,
                    style: batch.style,
                });
            }

            if cell_is_trimmable_blank(cell) && !is_cursor {
                pending_blank_run.text.push_str(&chunk);
                pending_blank_run.len += chunk.len();
                pending_blank_run.style = Some(cell_style);
                continue;
            }

            if pending_blank_run.len > 0 {
                text.push_str(&pending_blank_run.text);
                push_text_run(
                    &mut runs,
                    pending_blank_run.len,
                    text_run_from_style(
                        pending_blank_run
                            .style
                            .clone()
                            .unwrap_or_else(default_cell_style),
                    ),
                );
                pending_blank_run = PendingTextRun::default();
            }

            text.push_str(&chunk);
            push_text_run(&mut runs, chunk.len(), text_run_from_style(cell_style));
        }

        if text.is_empty() {
            text.push('\u{00a0}');
            push_text_run(
                &mut runs,
                text.len(),
                text_run_from_style(default_cell_style()),
            );
        }

        if pending_blank_run.len > 0 {
            text.push_str(&pending_blank_run.text);
            push_text_run(
                &mut runs,
                pending_blank_run.len,
                text_run_from_style(
                    pending_blank_run
                        .style
                        .clone()
                        .unwrap_or_else(default_cell_style),
                ),
            );
        }

        if let Some(batch) = pending_positioned_run.take() {
            positioned_runs.push(TerminalPositionedRunSnapshot {
                line: batch.line,
                column: batch.column,
                cell_count: batch.cell_count,
                text: batch.text,
                style: batch.style,
            });
        }

        lines.push(TerminalLineSnapshot {
            text,
            cells,
            runs,
            background_spans,
        });
    }

    let text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    TerminalSurfaceSnapshot {
        text,
        columns: size.cols as usize,
        lines,
        positioned_runs,
        cursor: cursor_snapshot,
    }
}

fn append_positioned_run(
    pending_run: &mut Option<PendingPositionedRun>,
    positioned_runs: &mut Vec<TerminalPositionedRunSnapshot>,
    line: usize,
    column: usize,
    cell_width: usize,
    text: String,
    style: TextRun,
) {
    if let Some(run) = pending_run {
        if run.line == line
            && run.column + run.cell_count == column
            && same_text_style(&run.style, &style)
        {
            run.cell_count += cell_width;
            run.style.len += text.len();
            run.text.push_str(&text);
            return;
        }

        let finished = pending_run.take().unwrap();
        positioned_runs.push(TerminalPositionedRunSnapshot {
            line: finished.line,
            column: finished.column,
            cell_count: finished.cell_count,
            text: finished.text,
            style: finished.style,
        });
    }

    let mut style = style;
    style.len = text.len();
    *pending_run = Some(PendingPositionedRun {
        line,
        column,
        cell_count: cell_width,
        text,
        style,
    });
}

fn push_text_run(runs: &mut Vec<TextRun>, len: usize, mut run: TextRun) {
    run.len = len;

    if let Some(last) = runs.last_mut() {
        if same_text_style(last, &run) {
            last.len += len;
            return;
        }
    }

    runs.push(run);
}

fn same_text_style(a: &TextRun, b: &TextRun) -> bool {
    a.font == b.font
        && a.color == b.color
        && a.background_color == b.background_color
        && a.underline == b.underline
        && a.strikethrough == b.strikethrough
}

fn cell_display_text(cell: &alacritty_terminal::term::cell::Cell) -> String {
    let mut text = String::new();

    let ch = if cell.flags.contains(Flags::HIDDEN) || cell.c == ' ' {
        '\u{00a0}'
    } else {
        cell.c
    };
    text.push(ch);

    for zero_width in cell.zerowidth().into_iter().flatten() {
        text.push(*zero_width);
    }

    text
}

fn cell_is_trimmable_blank(cell: &alacritty_terminal::term::cell::Cell) -> bool {
    (cell.flags.contains(Flags::HIDDEN) || cell.c == ' ') && cell.zerowidth().is_none()
}

fn cell_copy_text(cell: &alacritty_terminal::term::cell::Cell) -> String {
    if cell.flags.contains(Flags::HIDDEN) {
        return " ".to_string();
    }

    let mut text = String::new();
    text.push(if cell.c == ' ' { ' ' } else { cell.c });

    for zero_width in cell.zerowidth().into_iter().flatten() {
        text.push(*zero_width);
    }

    text
}

fn cell_is_render_blank(cell: &alacritty_terminal::term::cell::Cell) -> bool {
    if cell.c != ' ' {
        return false;
    }

    if cell.bg != Color::Named(NamedColor::Background) {
        return false;
    }

    if cell
        .flags
        .intersects(Flags::ALL_UNDERLINES | Flags::INVERSE | Flags::STRIKEOUT)
    {
        return false;
    }

    true
}

fn terminal_cell_width(cell: &alacritty_terminal::term::cell::Cell) -> usize {
    if cell.flags.contains(Flags::WIDE_CHAR) {
        2
    } else {
        1
    }
}

fn maybe_push_background_span(
    spans: &mut Vec<TerminalBackgroundSpanSnapshot>,
    column: usize,
    width: usize,
    color: Hsla,
) {
    if color == default_background_color() {
        return;
    }

    if let Some(last) = spans.last_mut() {
        if last.color == color && last.column + last.width == column {
            last.width += width;
            return;
        }
    }

    spans.push(TerminalBackgroundSpanSnapshot {
        column,
        width,
        color,
    });
}

fn cursor_snapshot_from_cell(
    line: usize,
    column: usize,
    width: usize,
    cursor_shape: CursorShape,
    cell_style: &ResolvedCellStyle,
) -> Option<TerminalCursorSnapshot> {
    let kind = match cursor_shape {
        CursorShape::Block => TerminalCursorKind::Block,
        CursorShape::HollowBlock => TerminalCursorKind::HollowBlock,
        CursorShape::Beam => TerminalCursorKind::Beam,
        CursorShape::Underline => TerminalCursorKind::Underline,
        CursorShape::Hidden => return None,
    };

    Some(TerminalCursorSnapshot {
        line,
        column,
        width,
        kind,
        color: cell_style.foreground,
    })
}

fn resolve_cell_style(
    cell: &alacritty_terminal::term::cell::Cell,
    colors: &alacritty_terminal::term::color::Colors,
) -> ResolvedCellStyle {
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
        font: terminal_font(cell.flags),
        underline: underline_style(cell, colors, foreground),
        strikethrough: cell
            .flags
            .contains(Flags::STRIKEOUT)
            .then(|| StrikethroughStyle {
                thickness: px(1.),
                color: Some(foreground),
            }),
    }
}

fn terminal_font(flags: Flags) -> gpui::Font {
    let mut font = font("Lilex Nerd Font Mono");
    if flags.contains(Flags::BOLD) {
        font.weight = FontWeight::BOLD;
    }
    if flags.contains(Flags::ITALIC) {
        font = font.italic();
    }
    font
}

fn underline_style(
    cell: &alacritty_terminal::term::cell::Cell,
    colors: &alacritty_terminal::term::color::Colors,
    foreground: Hsla,
) -> Option<UnderlineStyle> {
    if !cell.flags.intersects(Flags::ALL_UNDERLINES) {
        return None;
    }

    let color = cell
        .underline_color()
        .map(|color| resolve_color(color, cell.flags, true, colors))
        .unwrap_or(foreground);

    Some(UnderlineStyle {
        thickness: px(if cell.flags.contains(Flags::DOUBLE_UNDERLINE) {
            2.
        } else {
            1.
        }),
        color: Some(color),
        wavy: cell.flags.contains(Flags::UNDERCURL),
    })
}

fn default_cell_style() -> ResolvedCellStyle {
    ResolvedCellStyle {
        foreground: default_foreground_color(),
        background: default_background_color(),
        font: font("Lilex Nerd Font Mono"),
        underline: None,
        strikethrough: None,
    }
}

fn text_run_from_style(style: ResolvedCellStyle) -> TextRun {
    TextRun {
        len: 0,
        font: style.font,
        color: style.foreground,
        background_color: None,
        underline: style.underline,
        strikethrough: style.strikethrough,
    }
}

fn resolve_color(
    mut color: Color,
    flags: Flags,
    is_foreground: bool,
    colors: &alacritty_terminal::term::color::Colors,
) -> Hsla {
    if is_foreground {
        if flags.contains(Flags::DIM) {
            if let Color::Named(named) = color {
                color = Color::Named(named.to_dim());
            }
        } else if flags.contains(Flags::BOLD) {
            if let Color::Named(named) = color {
                color = Color::Named(named.to_bright());
            }
        }
    }

    let rgb = match color {
        Color::Named(named) => resolve_named_color(named, colors),
        Color::Spec(rgb) => rgb,
        Color::Indexed(index) => resolve_indexed_color(index, colors),
    };

    gpui::rgb(((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32).into()
}

fn resolve_named_color(named: NamedColor, colors: &alacritty_terminal::term::color::Colors) -> Rgb {
    colors[named].unwrap_or_else(|| default_named_color(named))
}

fn resolve_indexed_color(index: u8, colors: &alacritty_terminal::term::color::Colors) -> Rgb {
    colors[index as usize].unwrap_or_else(|| default_indexed_color(index))
}

fn default_named_color(named: NamedColor) -> Rgb {
    match named {
        NamedColor::Black => rgb_to_vte(0x1f242d),
        NamedColor::Red => rgb_to_vte(0xe06c75),
        NamedColor::Green => rgb_to_vte(0x98c379),
        NamedColor::Yellow => rgb_to_vte(0xe5c07b),
        NamedColor::Blue => rgb_to_vte(0x61afef),
        NamedColor::Magenta => rgb_to_vte(0xc678dd),
        NamedColor::Cyan => rgb_to_vte(0x56b6c2),
        NamedColor::White => rgb_to_vte(0xd7dae0),
        NamedColor::BrightBlack => rgb_to_vte(0x5c6370),
        NamedColor::BrightRed => rgb_to_vte(0xf28b95),
        NamedColor::BrightGreen => rgb_to_vte(0xb8db87),
        NamedColor::BrightYellow => rgb_to_vte(0xf2d48f),
        NamedColor::BrightBlue => rgb_to_vte(0x8fc7ff),
        NamedColor::BrightMagenta => rgb_to_vte(0xd7a8ff),
        NamedColor::BrightCyan => rgb_to_vte(0x7fd7e6),
        NamedColor::BrightWhite => rgb_to_vte(0xffffff),
        NamedColor::Foreground => hsla_to_vte(default_foreground_color()),
        NamedColor::Background => hsla_to_vte(default_background_color()),
        NamedColor::Cursor => hsla_to_vte(default_foreground_color()),
        NamedColor::DimBlack => scale_rgb(default_named_color(NamedColor::Black), 0.72),
        NamedColor::DimRed => scale_rgb(default_named_color(NamedColor::Red), 0.72),
        NamedColor::DimGreen => scale_rgb(default_named_color(NamedColor::Green), 0.72),
        NamedColor::DimYellow => scale_rgb(default_named_color(NamedColor::Yellow), 0.72),
        NamedColor::DimBlue => scale_rgb(default_named_color(NamedColor::Blue), 0.72),
        NamedColor::DimMagenta => scale_rgb(default_named_color(NamedColor::Magenta), 0.72),
        NamedColor::DimCyan => scale_rgb(default_named_color(NamedColor::Cyan), 0.72),
        NamedColor::DimWhite => scale_rgb(default_named_color(NamedColor::White), 0.72),
        NamedColor::BrightForeground => rgb_to_vte(0xffffff),
        NamedColor::DimForeground => scale_rgb(hsla_to_vte(default_foreground_color()), 0.72),
    }
}

fn default_indexed_color(index: u8) -> Rgb {
    match index {
        0 => default_named_color(NamedColor::Black),
        1 => default_named_color(NamedColor::Red),
        2 => default_named_color(NamedColor::Green),
        3 => default_named_color(NamedColor::Yellow),
        4 => default_named_color(NamedColor::Blue),
        5 => default_named_color(NamedColor::Magenta),
        6 => default_named_color(NamedColor::Cyan),
        7 => default_named_color(NamedColor::White),
        8 => default_named_color(NamedColor::BrightBlack),
        9 => default_named_color(NamedColor::BrightRed),
        10 => default_named_color(NamedColor::BrightGreen),
        11 => default_named_color(NamedColor::BrightYellow),
        12 => default_named_color(NamedColor::BrightBlue),
        13 => default_named_color(NamedColor::BrightMagenta),
        14 => default_named_color(NamedColor::BrightCyan),
        15 => default_named_color(NamedColor::BrightWhite),
        16..=231 => {
            let index = index - 16;
            let red = index / 36;
            let green = (index % 36) / 6;
            let blue = index % 6;
            let cube = [0, 95, 135, 175, 215, 255];
            Rgb {
                r: cube[red as usize],
                g: cube[green as usize],
                b: cube[blue as usize],
            }
        }
        232..=255 => {
            let value = 8 + (index - 232) * 10;
            Rgb {
                r: value,
                g: value,
                b: value,
            }
        }
    }
}

fn default_background_color() -> Hsla {
    rgb(0x1e1f22).into()
}

fn default_foreground_color() -> Hsla {
    rgb(0xd7dae0).into()
}

fn window_size_from_grid(size: TerminalGridSize) -> WindowSize {
    WindowSize {
        num_lines: size.rows,
        num_cols: size.cols,
        cell_width: if size.cols == 0 {
            0
        } else {
            size.pixel_width / size.cols
        },
        cell_height: if size.rows == 0 {
            0
        } else {
            size.pixel_height / size.rows
        },
    }
}

fn resolve_color_request(index: usize, colors: &Colors) -> Rgb {
    colors[index].unwrap_or_else(|| default_color_request(index))
}

fn default_color_request(index: usize) -> Rgb {
    if index <= u8::MAX as usize {
        return default_indexed_color(index as u8);
    }

    match index {
        x if x == NamedColor::Foreground as usize => hsla_to_vte(default_foreground_color()),
        x if x == NamedColor::Background as usize => hsla_to_vte(default_background_color()),
        x if x == NamedColor::Cursor as usize => hsla_to_vte(default_foreground_color()),
        x if x == NamedColor::BrightForeground as usize => rgb_to_vte(0xffffff),
        x if x == NamedColor::DimForeground as usize => {
            scale_rgb(hsla_to_vte(default_foreground_color()), 0.72)
        }
        x if x == NamedColor::DimBlack as usize => {
            scale_rgb(default_named_color(NamedColor::Black), 0.72)
        }
        x if x == NamedColor::DimRed as usize => {
            scale_rgb(default_named_color(NamedColor::Red), 0.72)
        }
        x if x == NamedColor::DimGreen as usize => {
            scale_rgb(default_named_color(NamedColor::Green), 0.72)
        }
        x if x == NamedColor::DimYellow as usize => {
            scale_rgb(default_named_color(NamedColor::Yellow), 0.72)
        }
        x if x == NamedColor::DimBlue as usize => {
            scale_rgb(default_named_color(NamedColor::Blue), 0.72)
        }
        x if x == NamedColor::DimMagenta as usize => {
            scale_rgb(default_named_color(NamedColor::Magenta), 0.72)
        }
        x if x == NamedColor::DimCyan as usize => {
            scale_rgb(default_named_color(NamedColor::Cyan), 0.72)
        }
        x if x == NamedColor::DimWhite as usize => {
            scale_rgb(default_named_color(NamedColor::White), 0.72)
        }
        _ => hsla_to_vte(default_background_color()),
    }
}

fn scale_rgb(rgb: Rgb, factor: f32) -> Rgb {
    Rgb {
        r: (f32::from(rgb.r) * factor).round().clamp(0.0, 255.0) as u8,
        g: (f32::from(rgb.g) * factor).round().clamp(0.0, 255.0) as u8,
        b: (f32::from(rgb.b) * factor).round().clamp(0.0, 255.0) as u8,
    }
}

fn hsla_to_vte(color: Hsla) -> Rgb {
    let rgba: u32 = color.to_rgb().into();
    Rgb {
        r: ((rgba >> 24) & 0xff) as u8,
        g: ((rgba >> 16) & 0xff) as u8,
        b: ((rgba >> 8) & 0xff) as u8,
    }
}

fn rgb_to_vte(color: u32) -> Rgb {
    Rgb {
        r: ((color >> 16) & 0xff) as u8,
        g: ((color >> 8) & 0xff) as u8,
        b: (color & 0xff) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::test::TermSize;

    fn snapshot_from_ansi(bytes: &[u8]) -> TerminalSurfaceSnapshot {
        let size = TerminalGridSize {
            cols: 32,
            rows: 4,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut term = Term::new(Config::default(), &TermSize::new(32, 4), VoidListener);
        let mut parser = ansi::Processor::<ansi::StdSyncHandler>::default();
        parser.advance(&mut term, bytes);
        build_surface_snapshot(&term, size)
    }

    #[test]
    fn snapshot_preserves_truecolor_background_and_bold_runs() {
        let snapshot = snapshot_from_ansi(b"\x1b[1;38;2;255;140;0;48;2;30;50;90mhi\x1b[0m");
        let line = &snapshot.lines[0];
        let styled_run = line
            .runs
            .iter()
            .find(|run| run.font.weight == FontWeight::BOLD)
            .expect("expected bold styled run");
        let background_span = line
            .background_spans
            .iter()
            .find(|span| span.color == rgb(0x1e325a).into())
            .expect("expected truecolor background span");

        assert!(line.text.starts_with("hi"));
        assert_eq!(styled_run.font.weight, FontWeight::BOLD);
        assert_eq!(background_span.column, 0);
        assert!(background_span.width >= 2);
    }

    #[test]
    fn snapshot_preserves_indexed_and_underlined_runs() {
        let snapshot = snapshot_from_ansi(b"\x1b[38;5;141;4mansi\x1b[0m");
        let line = &snapshot.lines[0];
        let styled_run = line
            .runs
            .iter()
            .find(|run| run.color == rgb(0xaf87ff).into())
            .expect("expected styled run with indexed color");

        assert!(line.text.starts_with("ansi"));
        assert!(styled_run.underline.is_some());
        assert_eq!(styled_run.color, rgb(0xaf87ff).into());
    }
}
