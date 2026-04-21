use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener};
use alacritty_terminal::grid::{Dimensions, GridCell};
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::{point_to_viewport, viewport_to_point, Config, Term};
use alacritty_terminal::vte::ansi::{self, Color, CursorShape, NamedColor, Rgb};
use gpui::{
    font, px, rgb, FontWeight, Hsla, StrikethroughStyle, TextRun, UnderlineStyle,
};
use portable_pty::{ChildKiller, MasterPty, PtySize};

use crate::app::SectionId;

const MIN_TERMINAL_COLS: u16 = 20;
const MIN_TERMINAL_ROWS: u16 = 4;
const CELL_WIDTH_RATIO: f32 = 0.62;
pub(crate) const TERMINAL_LINE_HEIGHT_RATIO: f32 = 1.25;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) struct TerminalRuntimeKey {
    pub section_id: SectionId,
    pub tab_id: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalGridSize {
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
    pub fn from_panel_size(width_px: f32, height_px: f32, font_size: f32) -> Self {
        let cell_width = (font_size * CELL_WIDTH_RATIO).max(7.0);
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

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalSurfaceSnapshot {
    pub text: String,
    pub lines: Vec<TerminalLineSnapshot>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalLineSnapshot {
    pub text: String,
    pub runs: Vec<TextRun>,
}

pub(crate) struct PreparedTerminalRuntime {
    pub size: TerminalGridSize,
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Box<dyn Write + Send>,
    pub child_killer: Box<dyn ChildKiller + Send + Sync>,
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
        if let Ok(mut queue) = self.queue.lock() {
            queue.push_back(event);
        }
    }
}

pub(crate) struct LiveTerminalRuntime {
    term: Term<RuntimeEventProxy>,
    parser: ansi::Processor,
    master: Box<dyn MasterPty + Send>,
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    child_killer: Box<dyn ChildKiller + Send + Sync>,
    event_queue: Arc<Mutex<VecDeque<Event>>>,
    size: TerminalGridSize,
    dirty: bool,
    cached_snapshot: TerminalSurfaceSnapshot,
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
        }
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
                    Event::Wakeup
                    | Event::Bell
                    | Event::MouseCursorDirty
                    | Event::CursorBlinkingChange
                    | Event::ClipboardStore(_, _)
                    | Event::ClipboardLoad(_, _)
                    | Event::ColorRequest(_, _)
                    | Event::TextAreaSizeRequest(_)
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

    pub fn snapshot(&mut self) -> TerminalSurfaceSnapshot {
        if !self.dirty {
            return self.cached_snapshot.clone();
        }

        let snapshot = build_surface_snapshot(&self.term, self.size);
        self.cached_snapshot = snapshot;
        self.dirty = false;
        self.cached_snapshot.clone()
    }

    pub fn kill(&mut self) {
        let _ = self.child_killer.kill();
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

    for viewport_line in 0..size.rows as usize {
        let point = viewport_to_point(display_offset, Point::new(viewport_line, Column(0)));
        let grid_line = &term.grid()[point.line];
        let mut text = String::new();
        let mut runs = Vec::new();
        let mut pending_blank_text = String::new();
        let mut pending_blank_len = 0usize;

        for column in 0..size.cols as usize {
            let cell = &grid_line[Column(column)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }

            let is_cursor = cursor.is_some_and(|cursor| {
                cursor.line == viewport_line && cursor.column.0 == column
            });
            let chunk = cell_display_text(cell);
            if chunk.is_empty() {
                continue;
            }

            if cell.is_empty() && !is_cursor {
                pending_blank_len += chunk.len();
                pending_blank_text.push_str(&chunk);
                continue;
            }

            if pending_blank_len > 0 {
                text.push_str(&pending_blank_text);
                push_text_run(
                    &mut runs,
                    pending_blank_len,
                    default_text_run(default_foreground_color(), None),
                );
                pending_blank_text.clear();
                pending_blank_len = 0;
            }

            text.push_str(&chunk);
            push_text_run(
                &mut runs,
                chunk.len(),
                build_text_run(
                    cell,
                    renderable.colors,
                    renderable.cursor.shape,
                    is_cursor,
                ),
            );
        }

        if text.is_empty() {
            text.push('\u{00a0}');
            push_text_run(
                &mut runs,
                text.len(),
                default_text_run(default_foreground_color(), None),
            );
        }

        lines.push(TerminalLineSnapshot { text, runs });
    }

    let text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    TerminalSurfaceSnapshot { text, lines }
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

fn build_text_run(
    cell: &alacritty_terminal::term::cell::Cell,
    colors: &alacritty_terminal::term::color::Colors,
    cursor_shape: CursorShape,
    is_cursor: bool,
) -> TextRun {
    let mut foreground = resolve_color(cell.fg, cell.flags, true, colors);
    let mut background = resolve_color(cell.bg, cell.flags, false, colors);

    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }

    if cell.flags.contains(Flags::HIDDEN) {
        foreground = background;
    }

    let mut underline = underline_style(cell, colors, foreground);
    if is_cursor {
        match cursor_shape {
            CursorShape::Underline => {
                underline = Some(UnderlineStyle {
                    thickness: px(2.),
                    color: Some(foreground),
                    wavy: false,
                });
            }
            CursorShape::Beam | CursorShape::HollowBlock | CursorShape::Block => {
                std::mem::swap(&mut foreground, &mut background);
            }
            CursorShape::Hidden => {}
        }
    }

    let background_color = if background == default_background_color() {
        None
    } else {
        Some(background)
    };

    TextRun {
        len: 0,
        font: terminal_font(cell.flags),
        color: foreground,
        background_color,
        underline,
        strikethrough: cell.flags.contains(Flags::STRIKEOUT).then(|| StrikethroughStyle {
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

fn default_text_run(color: Hsla, background_color: Option<Hsla>) -> TextRun {
    TextRun {
        len: 0,
        font: font("Lilex Nerd Font Mono"),
        color,
        background_color,
        underline: None,
        strikethrough: None,
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

    let mut color: Hsla =
        gpui::rgb(((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32).into();
    if is_foreground && flags.contains(Flags::DIM) {
        color.a *= 0.72;
    }
    color
}

fn resolve_named_color(
    named: NamedColor,
    colors: &alacritty_terminal::term::color::Colors,
) -> Rgb {
    colors[named].unwrap_or_else(|| default_named_color(named))
}

fn resolve_indexed_color(
    index: u8,
    colors: &alacritty_terminal::term::color::Colors,
) -> Rgb {
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
        let snapshot =
            snapshot_from_ansi(b"\x1b[1;38;2;255;140;0;48;2;30;50;90mhi\x1b[0m");
        let line = &snapshot.lines[0];
        let styled_run = line
            .runs
            .iter()
            .find(|run| run.background_color == Some(rgb(0x1e325a).into()))
            .expect("expected styled run with truecolor background");

        assert!(line.text.starts_with("hi"));
        assert_eq!(styled_run.font.weight, FontWeight::BOLD);
        assert_eq!(styled_run.background_color, Some(rgb(0x1e325a).into()));
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
