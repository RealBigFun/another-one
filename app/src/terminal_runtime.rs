use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::{Dimensions, Scroll};
use alacritty_terminal::index::{Column, Point};
use alacritty_terminal::term::cell::Flags;
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{point_to_viewport, viewport_to_point, Config, Term};
use alacritty_terminal::vte::ansi::{self, Color, CursorShape, NamedColor, Rgb};
#[cfg(test)]
use gpui::rgb;
use gpui::{font, px, FontWeight, Hsla, StrikethroughStyle, TextRun, UnderlineStyle};
use portable_pty::MasterPty;

// Shared with `core/src/terminal_types.rs` — the launcher side also
// produces `PreparedTerminalRuntime` / `TerminalGridSize` /
// `TerminalRuntimeKey`, and both sides need the clamp constants and
// cell/line ratios in lockstep.
pub(crate) use another_one_core::terminal_types::{
    PreparedTerminalRuntime, TerminalChildKiller, TerminalGridSize, TerminalRuntimeKey,
    TERMINAL_CELL_WIDTH_RATIO, TERMINAL_LINE_HEIGHT_RATIO,
};

/// xterm-style mouse-tracking level negotiated by the running TUI.
/// Drives whether the host reports up, drag, or any-motion events.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalMouseLevel {
    /// `?9h` — X10: button presses only.
    ClickOnly,
    /// `?1000h` baseline + `?1002h`: presses, releases, and drags
    /// while a button is held.
    ButtonDrag,
    /// `?1003h`: every motion event in addition to drags/clicks.
    AnyMotion,
}

/// Wire encoding used to serialize a mouse event back to the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TerminalMouseEncoding {
    /// Original CSI M payload, columns clamped to 223.
    Default,
    /// `?1006h`: SGR-style `CSI < b ; col ; row ; M/m`.
    Sgr,
    /// `?1005h`: UTF-8 columns clamped to 2015.
    Utf8,
}

/// Single hit returned by [`LiveTerminalRuntime::search_scrollback`].
/// Coordinates are in alacritty's grid frame: `line` is `Line.0`
/// (negative = scrollback above the active screen, 0..=screen_lines-1
/// is in viewport), `[start_col, end_col)` is a half-open cell range.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalScrollbackMatch {
    pub line: i32,
    pub start_col: usize,
    pub end_col: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct TerminalMouseProtocol {
    pub level: TerminalMouseLevel,
    pub encoding: TerminalMouseEncoding,
}

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
    /// `true` when the running TUI requested a blinking cursor variant
    /// via DECSCUSR (`CSI Ps SP q`). The renderer modulates opacity over
    /// time when this is set; the host bumps its refresh cadence so the
    /// blink animation is visible at idle.
    pub blinking: bool,
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
    /// True when the running TUI rang the terminal bell (`\x07`) during
    /// this drain pass. The host briefly flashes the pane to surface it.
    pub bell: bool,
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

/// Local PTY backing for a `LiveTerminalRuntime`. Present when the
/// renderer owns the PTY directly (desktop's `spawn_terminal_launch`
/// path); `None` when the renderer is a viewer of a remote PTY (mobile
/// post-`AttachTab`, where bytes arrive via `SessionEvent::PtyBytes`
/// and input flows back over `Session::push_data`).
pub(crate) struct LocalPty {
    pub master: Box<dyn MasterPty + Send>,
    pub writer: Arc<Mutex<Box<dyn Write + Send>>>,
    pub child_killer: TerminalChildKiller,
    /// Kept around so `daemon_host` can clone it on attach. The PTY
    /// reader thread retains its own clone for pushing bytes; this
    /// copy is read-only from the registry's perspective.
    pub output_broadcast: tokio::sync::broadcast::Sender<Vec<u8>>,
}

pub(crate) struct LiveTerminalRuntime {
    term: Term<RuntimeEventProxy>,
    parser: ansi::Processor,
    /// `Some` for locally-spawned PTYs (desktop); `None` for
    /// session-attached viewers (mobile). When `None`, input must
    /// route through `daemon_transport::Session::push_data` and
    /// resize requests must travel as `Control::TabResize`.
    local_pty: Option<LocalPty>,
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
            local_pty: Some(LocalPty {
                master: prepared.master,
                writer: Arc::new(Mutex::new(prepared.writer)),
                child_killer: prepared.child_killer,
                output_broadcast: prepared.output_broadcast,
            }),
            event_queue,
            size: prepared.size,
            dirty: true,
            cached_snapshot: TerminalSurfaceSnapshot::default(),
        }
    }

    /// Construct a viewer-only runtime backed by a remote PTY (i.e.
    /// the daemon owns the actual shell process; this side just
    /// applies bytes pushed via `SessionEvent::PtyBytes` into a local
    /// VT grid). Input must be routed through
    /// `daemon_transport::Session::push_data` by the caller — the
    /// runtime's `write_input` is a no-op when there is no local PTY.
    #[allow(dead_code)] // only constructed on cfg(target_os = "android")
    pub fn from_remote(size: TerminalGridSize) -> Self {
        let event_queue = Arc::new(Mutex::new(VecDeque::new()));
        let event_proxy = RuntimeEventProxy {
            queue: event_queue.clone(),
        };
        let term = Term::new(Config::default(), &size, event_proxy);
        Self {
            term,
            parser: ansi::Processor::default(),
            local_pty: None,
            event_queue,
            size,
            dirty: true,
            cached_snapshot: TerminalSurfaceSnapshot::default(),
        }
    }

    /// Clone of the PTY byte broadcast sender that
    /// `core::terminal_launch` tees every read into. The embedded
    /// daemon subscribes to this to forward bytes to mobile clients.
    /// `None` for viewer-only runtimes — there is no local PTY to
    /// broadcast from.
    pub fn output_broadcast(&self) -> Option<tokio::sync::broadcast::Sender<Vec<u8>>> {
        self.local_pty.as_ref().map(|p| p.output_broadcast.clone())
    }

    /// True when this runtime owns a local PTY. False for viewer-only
    /// (mobile-attached) runtimes.
    pub fn has_local_pty(&self) -> bool {
        self.local_pty.is_some()
    }

    pub fn resize(&mut self, size: TerminalGridSize) -> anyhow::Result<bool> {
        if self.size == size {
            return Ok(false);
        }

        if let Some(local) = self.local_pty.as_mut() {
            local.master.resize(size.as_pty_size())?;
        }
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
                    Event::Bell => update.bell = true,
                    Event::Wakeup
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
        let Some(local) = self.local_pty.as_ref() else {
            // Viewer-only runtime — caller is expected to route input
            // through `daemon_transport::Session::push_data`. Return
            // Ok so existing call sites that don't yet branch on
            // platform stay quiet; the callers that need to push via
            // session call `tab_input_via_session` instead.
            return Ok(());
        };
        let mut writer = local
            .writer
            .lock()
            .map_err(|_| io::Error::other("terminal writer lock poisoned"))?;
        writer.write_all(bytes)?;
        writer.flush()
    }

    pub fn paste_text(&self, text: &str) -> io::Result<()> {
        let bracketed = self
            .term
            .mode()
            .contains(alacritty_terminal::term::TermMode::BRACKETED_PASTE);
        let payload = encode_paste_payload(text, bracketed);
        self.write_input(payload.as_bytes())
    }

    /// Current scrollback offset (`0` = bottom / live screen). Lifted
    /// to the host so the search overlay can map grid-coord matches
    /// onto the visible viewport.
    pub fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    pub fn screen_lines(&self) -> usize {
        self.term.grid().screen_lines()
    }

    pub fn is_alternate_screen(&self) -> bool {
        self.term
            .mode()
            .contains(alacritty_terminal::term::TermMode::ALT_SCREEN)
    }

    /// Inspect the active mouse-tracking mode, if any. Returns `None` when
    /// the application has not enabled mouse reporting — in which case the
    /// host should treat mouse events as native (selection, link click).
    pub fn mouse_protocol(&self) -> Option<TerminalMouseProtocol> {
        let mode = self.term.mode();
        let level = if mode.contains(alacritty_terminal::term::TermMode::MOUSE_MOTION) {
            TerminalMouseLevel::AnyMotion
        } else if mode.contains(alacritty_terminal::term::TermMode::MOUSE_DRAG) {
            TerminalMouseLevel::ButtonDrag
        } else if mode.contains(alacritty_terminal::term::TermMode::MOUSE_REPORT_CLICK) {
            TerminalMouseLevel::ClickOnly
        } else {
            return None;
        };
        let encoding = if mode.contains(alacritty_terminal::term::TermMode::SGR_MOUSE) {
            TerminalMouseEncoding::Sgr
        } else if mode.contains(alacritty_terminal::term::TermMode::UTF8_MOUSE) {
            TerminalMouseEncoding::Utf8
        } else {
            TerminalMouseEncoding::Default
        };
        Some(TerminalMouseProtocol { level, encoding })
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

    /// Force the next snapshot to rebuild even if the terminal grid has not
    /// changed. Theme changes alter default fg/bg resolution without touching
    /// alacritty's grid, so cached snapshots need explicit invalidation.
    pub fn invalidate_snapshot(&mut self) {
        self.dirty = true;
    }

    /// Does this runtime have accumulated output the snapshot hasn't
    /// caught up with yet? Used by the drain loop to decide whether a
    /// focused-but-previously-backgrounded tab needs a rebuild this tick.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Walk the full alacritty grid (history + viewport) and return all
    /// matches of `query`. Empty queries yield an empty list. Search is
    /// always case-insensitive — matches the typical "Cmd-F" UX where
    /// users don't think about case.
    ///
    /// Match positions are reported in alacritty grid coordinates: `line`
    /// is `Line.0` (negative for scrollback rows, 0..=screen_lines-1 for
    /// viewport), and `[start_col, end_col)` is a half-open cell range.
    /// Multi-byte UTF-8 chars occupy a single cell so column ranges line
    /// up with cell positions, not UTF-8 byte offsets.
    pub fn search_scrollback(&self, query: &str) -> Vec<TerminalScrollbackMatch> {
        search_scrollback_in_term(&self.term, query)
    }

    /// Adjust `display_offset` so the given match lies near the vertical
    /// middle of the viewport. No-op if the match is already visible
    /// without scrolling.
    pub fn scroll_to_match(&mut self, target: &TerminalScrollbackMatch) -> bool {
        let grid = self.term.grid();
        let screen_lines = grid.screen_lines() as i32;
        let history_size = grid.history_size() as i32;
        let current = grid.display_offset() as i32;
        // Viewport with offset D shows grid lines [-D - screen_lines + 1 ..= -D].
        // The match at row R inside the viewport satisfies:
        //   R = target.line + screen_lines - 1 + D
        // so to land R = screen_lines / 2 (vertical middle):
        //   D = screen_lines/2 - target.line - screen_lines + 1
        let top = -current - screen_lines + 1;
        let bot = -current;
        if target.line >= top && target.line <= bot {
            return false;
        }
        let centered = (screen_lines / 2) - target.line - screen_lines + 1;
        let target_offset = centered.clamp(0, history_size);
        let delta = target_offset - current;
        if delta == 0 {
            return false;
        }
        self.term.scroll_display(Scroll::Delta(delta));
        self.dirty = true;
        true
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
        let Some(local) = self.local_pty.as_mut() else {
            return;
        };
        #[cfg(unix)]
        if let Some(process_group_id) = local.master.process_group_leader() {
            local.child_killer.add_process_group(process_group_id);
        }
        let _ = local.child_killer.kill();
    }

    /// Clone the shared writer handle. The embedded daemon's
    /// `DaemonRegistry` clones this on tab launch so incoming mobile
    /// keystrokes can feed into the same `Arc<Mutex<…>>` the desktop
    /// writes to. See `daemon_host::DesktopTerminalRegistry::tab_input`.
    /// `None` for viewer-only runtimes that have no local PTY to write to.
    pub fn writer_handle(&self) -> Option<Arc<Mutex<Box<dyn Write + Send>>>> {
        self.local_pty.as_ref().map(|p| p.writer.clone())
    }
}

/// Wrap pasted text with the xterm bracketed-paste markers when the
/// running TUI has opted into the protocol (DECSET 2004). When it has
/// not, the bytes are forwarded as-is so naive shells still receive a
/// usable payload.
///
/// Two security/UX hardenings happen even when bracketed mode is on:
///
/// 1. **Paste-end smuggling.** A malicious payload that carries an
///    embedded `\x1b[201~` would close the paste early and let the rest
///    of the bytes be interpreted as commands. We strip both `\x1b[200~`
///    and `\x1b[201~` markers from the payload before wrapping (see
///    xterm's "Allow paste of binary data" notes and the CVE-2003-0063
///    family). The three replacement patterns can never resynthesize a
///    marker — `\x1b[200~` requires the literal `~` and neither strip
///    nor `\r\n→\r` introduces one — so a single pass is safe.
///
/// 2. **CRLF → CR.** xterm-style paste passes `\r` for line breaks (the
///    same byte the keyboard sends for Enter in raw mode). Lone `\n` is
///    deliberately preserved so a paste of literal LF survives intact.
pub(crate) fn encode_paste_payload(text: &str, bracketed: bool) -> String {
    if !bracketed {
        return text.to_string();
    }
    let sanitized = text
        .replace("\x1b[200~", "")
        .replace("\x1b[201~", "")
        .replace("\r\n", "\r");
    format!("\u{1b}[200~{}\u{1b}[201~", sanitized)
}

/// Walk the full alacritty grid (history + viewport) and return all
/// case-insensitive substring matches of `query`. Empty queries yield
/// an empty list. Match positions are in alacritty grid coordinates:
/// `line` is `Line.0` (negative for scrollback rows, 0..=screen_lines-1
/// is in viewport), `[start_col, end_col)` is a half-open cell range.
pub(crate) fn search_scrollback_in_term<T: EventListener>(
    term: &Term<T>,
    query: &str,
) -> Vec<TerminalScrollbackMatch> {
    let query = query.trim_end_matches('\0');
    if query.is_empty() {
        return Vec::new();
    }
    // Lowercase the query into a Vec<char> so we can compare
    // char-by-char without re-walking UTF-8 bytes. Some chars
    // lowercase to multi-char sequences; flatten those eagerly.
    let needle_chars: Vec<char> = query.chars().flat_map(|ch| ch.to_lowercase()).collect();
    if needle_chars.is_empty() {
        return Vec::new();
    }
    let mut matches = Vec::new();
    let grid = term.grid();
    let columns = grid.columns();
    let topmost = grid.topmost_line().0;
    let bottommost = grid.bottommost_line().0;

    for line in topmost..=bottommost {
        let mut chars: Vec<char> = Vec::with_capacity(columns);
        let mut cols: Vec<usize> = Vec::with_capacity(columns);
        for col in 0..columns {
            let cell = &grid[alacritty_terminal::index::Line(line)]
                [alacritty_terminal::index::Column(col)];
            if cell
                .flags
                .intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER)
            {
                continue;
            }
            chars.push(cell.c);
            cols.push(col);
        }
        if chars.len() < needle_chars.len() {
            continue;
        }
        // Naive O(n·m) scan — terminal rows are short (≤ ~500 cols)
        // and the typical needle is 1–20 chars, so this stays fast
        // even on a 100k-row scrollback.
        'outer: for start in 0..=chars.len() - needle_chars.len() {
            for (offset, needle_ch) in needle_chars.iter().copied().enumerate() {
                let row_ch = chars[start + offset];
                let row_lowered = row_ch.to_lowercase().next().unwrap_or(row_ch);
                if row_lowered != needle_ch {
                    continue 'outer;
                }
            }
            let start_col = cols[start];
            let last_char = start + needle_chars.len() - 1;
            // Account for wide chars (CJK etc): the cell after the
            // anchor-cell is a `WIDE_CHAR_SPACER` and was filtered out
            // of `cols`, so the bare `cols[last] + 1` highlight would
            // only cover the left half of a 2-cell glyph.
            let last_col = cols[last_char];
            let last_anchor_cell = &grid[alacritty_terminal::index::Line(line)]
                [alacritty_terminal::index::Column(last_col)];
            let last_cell_width = if last_anchor_cell.flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            let end_col = last_col + last_cell_width;
            matches.push(TerminalScrollbackMatch {
                line,
                start_col,
                end_col,
            });
        }
    }
    matches
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
    // `RenderableCursor` only carries `shape`; `blinking` is on the full
    // `CursorStyle` returned by `term.cursor_style()`. Pull it once here
    // and thread through to the snapshot.
    let cursor_blinking = term.cursor_style().blinking;
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
            let has_explicit_background =
                effective_background_color(cell) != Color::Named(NamedColor::Background);
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
                has_explicit_background,
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
                    cursor_blinking,
                ) {
                    if snapshot.kind == TerminalCursorKind::Block {
                        cell_style.foreground = default_background_color();
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
    has_explicit_background: bool,
) {
    if !has_explicit_background && color == default_background_color() {
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

fn effective_background_color(cell: &alacritty_terminal::term::cell::Cell) -> Color {
    if cell.flags.contains(Flags::INVERSE) {
        cell.fg
    } else {
        cell.bg
    }
}

fn cursor_snapshot_from_cell(
    line: usize,
    column: usize,
    width: usize,
    cursor_shape: CursorShape,
    blinking: bool,
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
        color: crate::theme::terminal_default_cursor(),
        blinking,
    })
}

fn resolve_cell_style(
    cell: &alacritty_terminal::term::cell::Cell,
    colors: &alacritty_terminal::term::color::Colors,
) -> ResolvedCellStyle {
    let raw_foreground = cell.fg;
    let mut foreground = resolve_color(cell.fg, cell.flags, true, colors);
    let mut background = resolve_color(cell.bg, cell.flags, false, colors);

    if cell.flags.contains(Flags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }

    if cell.flags.contains(Flags::HIDDEN) {
        foreground = background;
    } else {
        foreground = ensure_light_terminal_foreground_contrast(
            foreground,
            background,
            matches!(raw_foreground, Color::Spec(_)),
            crate::theme::current_terminal_theme(),
        );
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

fn ensure_light_terminal_foreground_contrast(
    foreground: Hsla,
    background: Hsla,
    is_true_color: bool,
    resolved_theme: crate::theme::ResolvedTheme,
) -> Hsla {
    if is_true_color || resolved_theme != crate::theme::ResolvedTheme::Light {
        return foreground;
    }

    if background.l > 0.70 && foreground.l > 0.75 && (foreground.l - background.l).abs() < 0.24 {
        crate::theme::terminal_foreground_for_theme(resolved_theme)
    } else {
        foreground
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
    let palette = current_terminal_palette();
    match named {
        NamedColor::Black => palette.normal[0],
        NamedColor::Red => palette.normal[1],
        NamedColor::Green => palette.normal[2],
        NamedColor::Yellow => palette.normal[3],
        NamedColor::Blue => palette.normal[4],
        NamedColor::Magenta => palette.normal[5],
        NamedColor::Cyan => palette.normal[6],
        NamedColor::White => palette.normal[7],
        NamedColor::BrightBlack => palette.bright[0],
        NamedColor::BrightRed => palette.bright[1],
        NamedColor::BrightGreen => palette.bright[2],
        NamedColor::BrightYellow => palette.bright[3],
        NamedColor::BrightBlue => palette.bright[4],
        NamedColor::BrightMagenta => palette.bright[5],
        NamedColor::BrightCyan => palette.bright[6],
        NamedColor::BrightWhite => palette.bright[7],
        NamedColor::Foreground => palette.foreground,
        NamedColor::Cursor => palette.cursor,
        NamedColor::BrightForeground => palette.bright_foreground,
        NamedColor::Background => palette.background,
        NamedColor::DimBlack => palette.dim[0],
        NamedColor::DimRed => palette.dim[1],
        NamedColor::DimGreen => palette.dim[2],
        NamedColor::DimYellow => palette.dim[3],
        NamedColor::DimBlue => palette.dim[4],
        NamedColor::DimMagenta => palette.dim[5],
        NamedColor::DimCyan => palette.dim[6],
        NamedColor::DimWhite => palette.dim[7],
        NamedColor::DimForeground => palette.dim_foreground,
    }
}

fn default_indexed_color(index: u8) -> Rgb {
    let palette = current_terminal_palette();
    match index {
        0..=7 => palette.normal[index as usize],
        8..=15 => palette.bright[(index - 8) as usize],
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
    crate::theme::terminal_default_background()
}

fn default_foreground_color() -> Hsla {
    crate::theme::terminal_default_foreground()
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
        x if x == NamedColor::Foreground as usize => default_named_color(NamedColor::Foreground),
        x if x == NamedColor::Background as usize => default_named_color(NamedColor::Background),
        x if x == NamedColor::Cursor as usize => default_named_color(NamedColor::Cursor),
        x if x == NamedColor::BrightForeground as usize => {
            default_named_color(NamedColor::BrightForeground)
        }
        x if x == NamedColor::DimForeground as usize => {
            default_named_color(NamedColor::DimForeground)
        }
        x if x == NamedColor::DimBlack as usize => default_named_color(NamedColor::DimBlack),
        x if x == NamedColor::DimRed as usize => default_named_color(NamedColor::DimRed),
        x if x == NamedColor::DimGreen as usize => default_named_color(NamedColor::DimGreen),
        x if x == NamedColor::DimYellow as usize => default_named_color(NamedColor::DimYellow),
        x if x == NamedColor::DimBlue as usize => default_named_color(NamedColor::DimBlue),
        x if x == NamedColor::DimMagenta as usize => default_named_color(NamedColor::DimMagenta),
        x if x == NamedColor::DimCyan as usize => default_named_color(NamedColor::DimCyan),
        x if x == NamedColor::DimWhite as usize => default_named_color(NamedColor::DimWhite),
        _ => default_named_color(NamedColor::Background),
    }
}

#[derive(Clone, Copy)]
struct TerminalPalette {
    background: Rgb,
    foreground: Rgb,
    cursor: Rgb,
    bright_foreground: Rgb,
    dim_foreground: Rgb,
    normal: [Rgb; 8],
    bright: [Rgb; 8],
    dim: [Rgb; 8],
}

fn current_terminal_palette() -> &'static TerminalPalette {
    terminal_palette(crate::theme::current_terminal_theme())
}

fn terminal_palette(resolved: crate::theme::ResolvedTheme) -> &'static TerminalPalette {
    match resolved {
        crate::theme::ResolvedTheme::Light => &AYU_LIGHT_TERMINAL,
        crate::theme::ResolvedTheme::Dark => &AYU_DARK_TERMINAL,
    }
}

const AYU_DARK_TERMINAL: TerminalPalette = TerminalPalette {
    background: vte_rgb(0x0d1016),
    foreground: vte_rgb(0xbfbdb6),
    cursor: vte_rgb(0x5ac1fe),
    bright_foreground: vte_rgb(0xbfbdb6),
    dim_foreground: vte_rgb(0x85847f),
    normal: [
        vte_rgb(0x0d1016),
        vte_rgb(0xef7177),
        vte_rgb(0xaad84c),
        vte_rgb(0xfeb454),
        vte_rgb(0x5ac1fe),
        vte_rgb(0x39bae5),
        vte_rgb(0x95e5cb),
        vte_rgb(0xbfbdb6),
    ],
    bright: [
        vte_rgb(0x545557),
        vte_rgb(0x83353b),
        vte_rgb(0x567627),
        vte_rgb(0x92582b),
        vte_rgb(0x27618c),
        vte_rgb(0x205a78),
        vte_rgb(0x4c806f),
        vte_rgb(0xfafafa),
    ],
    dim: [
        vte_rgb(0x3a3b3c),
        vte_rgb(0xa74f53),
        vte_rgb(0x769735),
        vte_rgb(0xb17d3a),
        vte_rgb(0x3e87b1),
        vte_rgb(0x2782a0),
        vte_rgb(0x68a08e),
        vte_rgb(0x85847f),
    ],
};

const AYU_LIGHT_TERMINAL: TerminalPalette = TerminalPalette {
    background: vte_rgb(0xfcfcfc),
    foreground: vte_rgb(0x5c6166),
    cursor: vte_rgb(0x3b9ee5),
    bright_foreground: vte_rgb(0x5c6166),
    dim_foreground: vte_rgb(0xfcfcfc),
    normal: [
        vte_rgb(0x5c6166),
        vte_rgb(0xef7271),
        vte_rgb(0x85b304),
        vte_rgb(0xf1ad49),
        vte_rgb(0x3b9ee5),
        vte_rgb(0x55b4d3),
        vte_rgb(0x4dbf99),
        vte_rgb(0xfcfcfc),
    ],
    bright: [
        vte_rgb(0x3b9ee5),
        vte_rgb(0xfebab6),
        vte_rgb(0xc7d98f),
        vte_rgb(0xfed5a3),
        vte_rgb(0xabcdf2),
        vte_rgb(0xb1d8e8),
        vte_rgb(0xace0cb),
        vte_rgb(0xffffff),
    ],
    dim: [
        vte_rgb(0x9c9fa2),
        vte_rgb(0x833538),
        vte_rgb(0x445613),
        vte_rgb(0x8a5227),
        vte_rgb(0x214c76),
        vte_rgb(0x2f5669),
        vte_rgb(0x2a5f4a),
        vte_rgb(0xbcbec0),
    ],
};

const fn vte_rgb(hex: u32) -> Rgb {
    Rgb {
        r: ((hex >> 16) & 0xff) as u8,
        g: ((hex >> 8) & 0xff) as u8,
        b: (hex & 0xff) as u8,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::test::TermSize;

    fn term_from_ansi(rows: usize, cols: usize, bytes: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TermSize::new(cols, rows), VoidListener);
        let mut parser = ansi::Processor::<ansi::StdSyncHandler>::default();
        parser.advance(&mut term, bytes);
        term
    }

    #[test]
    fn search_scrollback_finds_match_in_viewport() {
        let term = term_from_ansi(4, 32, b"hello world\r\nrust hello rust\r\n");
        let matches = search_scrollback_in_term(&term, "hello");
        assert_eq!(matches.len(), 2);
        assert_eq!(matches[0].start_col, 0);
        assert_eq!(matches[0].end_col, 5);
        assert_eq!(matches[1].start_col, 5);
        assert_eq!(matches[1].end_col, 10);
    }

    #[test]
    fn search_scrollback_is_case_insensitive() {
        let term = term_from_ansi(4, 32, b"Hello World\r\n");
        let m = search_scrollback_in_term(&term, "WORLD");
        assert_eq!(m.len(), 1);
        assert_eq!(m[0].start_col, 6);
        assert_eq!(m[0].end_col, 11);
    }

    #[test]
    fn search_scrollback_empty_query_yields_empty() {
        let term = term_from_ansi(4, 32, b"hello\r\n");
        assert!(search_scrollback_in_term(&term, "").is_empty());
        assert!(search_scrollback_in_term(&term, "\0\0").is_empty());
    }

    #[test]
    fn search_scrollback_walks_history_above_viewport() {
        // 4-row terminal, push 8 rows so 4 lines fall into scrollback.
        let mut bytes: Vec<u8> = Vec::new();
        for i in 0..8 {
            bytes.extend_from_slice(format!("row{i}\r\n").as_bytes());
        }
        let term = term_from_ansi(4, 16, &bytes);
        // "row1" should now live in scrollback (above the visible viewport).
        let m = search_scrollback_in_term(&term, "row1");
        assert_eq!(m.len(), 1);
        assert!(
            m[0].line < 0,
            "expected scrollback (negative line), got {}",
            m[0].line
        );
    }

    #[test]
    fn search_scrollback_wide_char_match_spans_two_columns() {
        // Match anchored on a 2-cell-wide CJK glyph should report
        // end_col one past its right cell, not its anchor cell.
        let term = term_from_ansi(4, 16, "look 日本 here\r\n".as_bytes());
        let m = search_scrollback_in_term(&term, "日");
        assert_eq!(m.len(), 1);
        // "look " takes cols 0..5, then 日 occupies cols 5+6.
        assert_eq!(m[0].start_col, 5);
        assert_eq!(m[0].end_col, 7, "wide-char highlight covers 2 cells");
    }

    #[test]
    fn search_scrollback_no_match_yields_empty() {
        let term = term_from_ansi(4, 32, b"hello world\r\n");
        assert!(search_scrollback_in_term(&term, "zzz").is_empty());
    }

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
    fn bell_event_surfaces_in_runtime_update() {
        // We can't drive a full LiveTerminalRuntime without a PTY, but
        // we can verify Term raises Event::Bell on `\x07` and that our
        // drain loop sets `update.bell = true`. Mirror the drain loop
        // inline using a dedicated proxy so we don't need PTY plumbing.
        use std::collections::VecDeque;
        use std::sync::{Arc, Mutex};

        let queue: Arc<Mutex<VecDeque<Event>>> = Arc::new(Mutex::new(VecDeque::new()));
        let proxy_queue = queue.clone();
        struct Proxy {
            queue: Arc<Mutex<VecDeque<Event>>>,
        }
        impl EventListener for Proxy {
            fn send_event(&self, event: Event) {
                self.queue.lock().unwrap().push_back(event);
            }
        }
        let mut term = Term::new(
            Config::default(),
            &TermSize::new(8, 4),
            Proxy { queue: proxy_queue },
        );
        let mut parser = ansi::Processor::<ansi::StdSyncHandler>::default();
        parser.advance(&mut term, b"a\x07b");

        let mut update = TerminalRuntimeUpdate::default();
        while let Some(event) = queue.lock().unwrap().pop_front() {
            if matches!(event, Event::Bell) {
                update.bell = true;
            }
        }
        assert!(update.bell, "BEL byte should surface as update.bell");
    }

    #[test]
    fn snapshot_captures_decscusr_blinking_bar() {
        // DECSCUSR `5 q` = blinking bar.
        let snapshot = snapshot_from_ansi(b"\x1b[5 q hi");
        let cursor = snapshot.cursor.as_ref().expect("cursor present");
        assert_eq!(cursor.kind, TerminalCursorKind::Beam);
        assert!(cursor.blinking, "DECSCUSR 5 → blinking");
    }

    #[test]
    fn snapshot_captures_decscusr_blinking_block() {
        // DECSCUSR `1 q` = explicit blinking block.
        let snapshot = snapshot_from_ansi(b"\x1b[1 q hi");
        let cursor = snapshot
            .cursor
            .as_ref()
            .expect("cursor present after DECSCUSR 1");
        assert_eq!(cursor.kind, TerminalCursorKind::Block);
        assert!(cursor.blinking, "DECSCUSR 1 → blinking block");
    }

    #[test]
    fn snapshot_captures_decscusr_zero_resets_to_default_steady_block() {
        // VT520 says `0 q` is "blinking block", but alacritty's config
        // default is steady block — so `0 q` here lands on steady. Lock
        // that behavior so a future config-default change is noticed.
        let snapshot = snapshot_from_ansi(b"\x1b[0 q hi");
        let cursor = snapshot
            .cursor
            .as_ref()
            .expect("cursor present after DECSCUSR 0");
        assert_eq!(cursor.kind, TerminalCursorKind::Block);
        assert!(!cursor.blinking, "alacritty default = steady");
    }

    #[test]
    fn snapshot_captures_decscusr_steady_underline() {
        // DECSCUSR `4 q` = steady underline.
        let snapshot = snapshot_from_ansi(b"\x1b[4 q hi");
        let cursor = snapshot.cursor.as_ref().expect("cursor present");
        assert_eq!(cursor.kind, TerminalCursorKind::Underline);
        assert!(!cursor.blinking, "DECSCUSR 4 → steady");
    }

    #[test]
    fn encode_paste_payload_passes_through_when_unbracketed() {
        let raw = "hello world\n";
        assert_eq!(encode_paste_payload(raw, false), raw);
    }

    #[test]
    fn encode_paste_payload_wraps_with_markers_when_bracketed() {
        assert_eq!(encode_paste_payload("hi", true), "\u{1b}[200~hi\u{1b}[201~");
    }

    #[test]
    fn encode_paste_payload_strips_embedded_end_marker() {
        let input = "safe\u{1b}[201~rm -rf /\u{1b}[201~tail";
        let out = encode_paste_payload(input, true);
        // The end-marker only appears once — at the very end as our trailer.
        let occurrences = out.matches("\u{1b}[201~").count();
        assert_eq!(occurrences, 1, "got: {:?}", out);
        assert!(out.starts_with("\u{1b}[200~"));
        assert!(out.ends_with("\u{1b}[201~"));
        assert!(out.contains("safe"));
        assert!(out.contains("rm -rf /"));
        assert!(out.contains("tail"));
    }

    #[test]
    fn encode_paste_payload_strips_embedded_start_marker() {
        let input = "before\u{1b}[200~after";
        let out = encode_paste_payload(input, true);
        // start-marker only appears once: as our header.
        assert_eq!(out.matches("\u{1b}[200~").count(), 1, "got: {:?}", out);
        assert!(out.contains("beforeafter"));
    }

    #[test]
    fn encode_paste_payload_normalizes_crlf_to_cr() {
        let out = encode_paste_payload("line1\r\nline2\r\nline3", true);
        assert_eq!(out, "\u{1b}[200~line1\rline2\rline3\u{1b}[201~");
    }

    #[test]
    fn encode_paste_payload_preserves_lone_lf_and_cr() {
        let out = encode_paste_payload("a\nb\rc", true);
        assert_eq!(out, "\u{1b}[200~a\nb\rc\u{1b}[201~");
    }

    #[test]
    fn encode_paste_payload_preserves_multibyte_utf8() {
        let out = encode_paste_payload("café 日本語 🦀", true);
        assert_eq!(out, "\u{1b}[200~café 日本語 🦀\u{1b}[201~");
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
    fn snapshot_preserves_explicit_background_even_when_it_matches_theme_background() {
        let snapshot = snapshot_from_ansi(b"\x1b[48;2;13;16;22m  \x1b[0m");
        let line = &snapshot.lines[0];
        let background_span = line
            .background_spans
            .iter()
            .find(|span| span.color == rgb(0x0d1016).into())
            .expect("expected explicit terminal background span");

        assert_eq!(background_span.column, 0);
        assert!(background_span.width >= 2);
    }

    #[test]
    fn snapshot_preserves_reverse_video_blank_row_background() {
        let snapshot = snapshot_from_ansi(b"\x1b[7m> Explain this codebase      \x1b[0m");
        let line = &snapshot.lines[0];
        let background_span = line
            .background_spans
            .iter()
            .find(|span| span.column == 0)
            .expect("expected reverse-video row background span");

        assert!(background_span.width >= 26);
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

    fn rgb_hex(rgb: Rgb) -> u32 {
        ((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32
    }

    fn rgb_hexes(colors: [Rgb; 8]) -> [u32; 8] {
        colors.map(rgb_hex)
    }

    #[test]
    fn ayu_dark_terminal_palette_matches_zed() {
        let palette = terminal_palette(crate::theme::ResolvedTheme::Dark);

        assert_eq!(rgb_hex(palette.background), 0x0d1016);
        assert_eq!(rgb_hex(palette.foreground), 0xbfbdb6);
        assert_eq!(rgb_hex(palette.cursor), 0x5ac1fe);
        assert_eq!(rgb_hex(palette.bright_foreground), 0xbfbdb6);
        assert_eq!(rgb_hex(palette.dim_foreground), 0x85847f);
        assert_eq!(
            rgb_hexes(palette.normal),
            [0x0d1016, 0xef7177, 0xaad84c, 0xfeb454, 0x5ac1fe, 0x39bae5, 0x95e5cb, 0xbfbdb6,]
        );
        assert_eq!(
            rgb_hexes(palette.bright),
            [0x545557, 0x83353b, 0x567627, 0x92582b, 0x27618c, 0x205a78, 0x4c806f, 0xfafafa,]
        );
        assert_eq!(
            rgb_hexes(palette.dim),
            [0x3a3b3c, 0xa74f53, 0x769735, 0xb17d3a, 0x3e87b1, 0x2782a0, 0x68a08e, 0x85847f,]
        );
    }

    #[test]
    fn ayu_light_terminal_palette_matches_zed() {
        let palette = terminal_palette(crate::theme::ResolvedTheme::Light);

        assert_eq!(rgb_hex(palette.background), 0xfcfcfc);
        assert_eq!(rgb_hex(palette.foreground), 0x5c6166);
        assert_eq!(rgb_hex(palette.cursor), 0x3b9ee5);
        assert_eq!(rgb_hex(palette.bright_foreground), 0x5c6166);
        assert_eq!(rgb_hex(palette.dim_foreground), 0xfcfcfc);
        assert_eq!(
            rgb_hexes(palette.normal),
            [0x5c6166, 0xef7271, 0x85b304, 0xf1ad49, 0x3b9ee5, 0x55b4d3, 0x4dbf99, 0xfcfcfc,]
        );
        assert_eq!(
            rgb_hexes(palette.bright),
            [0x3b9ee5, 0xfebab6, 0xc7d98f, 0xfed5a3, 0xabcdf2, 0xb1d8e8, 0xace0cb, 0xffffff,]
        );
        assert_eq!(
            rgb_hexes(palette.dim),
            [0x9c9fa2, 0x833538, 0x445613, 0x8a5227, 0x214c76, 0x2f5669, 0x2a5f4a, 0xbcbec0,]
        );
    }

    #[test]
    fn light_terminal_contrast_darkens_low_contrast_named_foreground() {
        let adjusted = ensure_light_terminal_foreground_contrast(
            rgb(0xfcfcfc).into(),
            rgb(0xececed).into(),
            false,
            crate::theme::ResolvedTheme::Light,
        );

        assert_eq!(
            adjusted,
            crate::theme::terminal_foreground_for_theme(crate::theme::ResolvedTheme::Light)
        );
    }

    #[test]
    fn light_terminal_contrast_preserves_truecolor_foreground() {
        let foreground = rgb(0xfcfcfc).into();
        let adjusted = ensure_light_terminal_foreground_contrast(
            foreground,
            rgb(0xececed).into(),
            true,
            crate::theme::ResolvedTheme::Light,
        );

        assert_eq!(adjusted, foreground);
    }

    #[test]
    fn default_indexed_color_includes_xterm_gray_ramp() {
        assert_eq!(default_indexed_color(232), vte_rgb(0x080808));
        assert_eq!(default_indexed_color(244), vte_rgb(0x808080));
        assert_eq!(default_indexed_color(255), vte_rgb(0xeeeeee));
    }
}
