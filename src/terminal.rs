//! Real terminal backend using portable-pty + alacritty_terminal.
//!
//! Each `TerminalInstance` owns:
//!   - A PTY child process (via portable-pty)
//!   - An `alacritty_terminal::Term` for grid state / VT parsing
//!   - A background reader thread that feeds PTY output into the Term
//!   - A writer handle to send keyboard input to the PTY

use std::io::{Read, Write};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use gpui::{ScrollDelta, ScrollWheelEvent, TouchPhase};

use alacritty_terminal::event::{Event as AlacEvent, EventListener};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line, Point, Side};
use alacritty_terminal::selection::{Selection, SelectionType};
use alacritty_terminal::term::{self, Config as TermConfig};
use alacritty_terminal::vte;

/// Default scrollback history lines.
const DEFAULT_SCROLL_HISTORY: usize = 10_000;

use portable_pty::{CommandBuilder, NativePtySystem, PtySize, PtySystem};

use crate::agents::TerminalLaunchConfig;

// ── Event listener (no-op; we poll the grid directly) ────────────────

#[derive(Clone)]
pub struct Listener;

impl EventListener for Listener {
    fn send_event(&self, _event: AlacEvent) {}
}

// ── Simple Dimensions wrapper ────────────────────────────────────────

struct TermSize {
    cols: usize,
    lines: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.lines
    }
    fn screen_lines(&self) -> usize {
        self.lines
    }
    fn columns(&self) -> usize {
        self.cols
    }
}

// ── Public terminal instance ─────────────────────────────────────────

pub struct TerminalInstance {
    /// Shared terminal state (grid + VT state). Lock to read grid for rendering.
    pub term: Arc<Mutex<term::Term<Listener>>>,
    /// Writer to send bytes (keyboard input) to the PTY.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
    /// PTY master handle (needed for resize).
    master: Option<Box<dyn portable_pty::MasterPty + Send>>,
    /// Number of columns.
    pub cols: u16,
    /// Number of rows.
    pub rows: u16,
    /// Latest requested resize, coalesced and applied outside render.
    pending_resize: Option<(u16, u16)>,
    /// Whether the reader thread is still alive.
    pub alive: Arc<Mutex<bool>>,
    /// Whether the terminal grid changed and needs repainting.
    dirty: Arc<AtomicBool>,
    /// Accumulated scroll pixels for smooth trackpad scrolling.
    scroll_px: f32,
}

impl TerminalInstance {
    /// Spawn a new terminal with the given grid size and working directory.
    pub fn new(
        cols: u16,
        rows: u16,
        cwd: Option<&std::path::Path>,
        launch_config: Option<&TerminalLaunchConfig>,
    ) -> anyhow::Result<Self> {
        // 1. Open PTY.
        let pty_system = NativePtySystem::default();
        let pair = pty_system.openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;

        // 2. Build shell command.
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/bash".into());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.arg("-l"); // login shell
        if let Some(dir) = cwd {
            cmd.cwd(dir);
        }

        // 3. Spawn child.
        let _child = pair.slave.spawn_command(cmd)?;
        // Drop slave — master is all we need.
        drop(pair.slave);

        // 4. Create alacritty Term with scrollback history.
        let config = TermConfig {
            scrolling_history: DEFAULT_SCROLL_HISTORY,
            ..TermConfig::default()
        };
        let size = TermSize {
            cols: cols as usize,
            lines: rows as usize,
        };
        let term = term::Term::new(config, &size, Listener);
        let term = Arc::new(Mutex::new(term));

        // 5. Set up reader + writer.
        let reader = pair.master.try_clone_reader()?;
        let writer: Box<dyn Write + Send> = pair.master.take_writer()?;
        let writer = Arc::new(Mutex::new(writer));
        // Keep master around for resize support.
        let master = pair.master;

        let alive = Arc::new(Mutex::new(true));
        let dirty = Arc::new(AtomicBool::new(true));

        // 6. Background thread: read PTY → parse → update Term.
        {
            let term = Arc::clone(&term);
            let alive = Arc::clone(&alive);
            let dirty = Arc::clone(&dirty);
            thread::spawn(move || {
                Self::reader_loop(reader, term, alive, dirty);
            });
        }

        let instance = Self {
            term,
            writer,
            master: Some(master),
            cols,
            rows,
            pending_resize: None,
            alive,
            dirty,
            scroll_px: 0.0,
        };

        if let Some(startup_command) =
            launch_config.and_then(TerminalLaunchConfig::startup_command_line)
        {
            instance.write_to_pty(startup_command.as_bytes());
            instance.write_to_pty(b"\n");
        }

        Ok(instance)
    }

    /// Send raw bytes to the PTY (keyboard input).
    pub fn write_to_pty(&self, bytes: &[u8]) {
        if let Ok(mut w) = self.writer.lock() {
            let _ = w.write_all(bytes);
            let _ = w.flush();
        }
    }

    /// Paste text into the PTY, respecting bracketed paste mode when enabled.
    pub fn paste_text(&self, text: &str) {
        let bracketed_paste = self
            .term
            .lock()
            .ok()
            .is_some_and(|term| term.mode().contains(term::TermMode::BRACKETED_PASTE));

        if bracketed_paste {
            let mut bytes = Vec::with_capacity(text.len() + 12);
            bytes.extend_from_slice(b"\x1b[200~");
            bytes.extend_from_slice(text.as_bytes());
            bytes.extend_from_slice(b"\x1b[201~");
            self.write_to_pty(&bytes);
        } else {
            self.write_to_pty(text.as_bytes());
        }
    }

    /// Check if the terminal process is still running.
    pub fn is_alive(&self) -> bool {
        *self.alive.lock().unwrap()
    }

    /// Queue a resize to be applied outside the render path.
    pub fn queue_resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            self.pending_resize = None;
            return;
        }
        self.pending_resize = Some((cols, rows));
    }

    /// Apply the most recent queued resize, if any.
    pub fn flush_resize(&mut self) -> bool {
        let Some((cols, rows)) = self.pending_resize.take() else {
            return false;
        };
        self.resize(cols, rows);
        true
    }

    /// Resize the terminal grid AND the PTY.
    fn resize(&mut self, cols: u16, rows: u16) {
        if cols == self.cols && rows == self.rows {
            return;
        }
        self.cols = cols;
        self.rows = rows;
        if let Ok(mut term) = self.term.lock() {
            let size = TermSize {
                cols: cols as usize,
                lines: rows as usize,
            };
            term.resize(size);
        }
        // Resize the PTY as well so the child process knows the new window size.
        if let Some(ref master) = self.master {
            let _ = master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            });
        }
        self.dirty.store(true, Ordering::Release);
    }

    /// Scroll the terminal display by the given number of lines (positive = up/back).
    pub fn scroll(&self, delta: i32) {
        if delta == 0 {
            return;
        }
        if let Ok(mut term) = self.term.lock() {
            term.scroll_display(alacritty_terminal::grid::Scroll::Delta(delta));
        }
        self.dirty.store(true, Ordering::Release);
    }

    /// Zed-style scroll wheel handling with pixel accumulation.
    pub fn scroll_wheel(&mut self, ev: &ScrollWheelEvent, line_height: f32) {
        match ev.touch_phase {
            TouchPhase::Started => {
                self.scroll_px = 0.0;
            }
            TouchPhase::Moved => {
                let old_offset = (self.scroll_px / line_height) as i32;

                match ev.delta {
                    ScrollDelta::Pixels(delta) => {
                        self.scroll_px += f32::from(delta.y);
                    }
                    ScrollDelta::Lines(delta) => {
                        self.scroll(delta.y.round() as i32);
                        return;
                    }
                }

                let new_offset = (self.scroll_px / line_height) as i32;
                let lines = new_offset - old_offset;
                self.scroll_px %= line_height.max(1.0);
                self.scroll(lines);
            }
            TouchPhase::Ended => {}
        }
    }

    /// Returns whether the terminal changed since the last poll.
    pub fn is_dirty(&self) -> bool {
        self.dirty.load(Ordering::Acquire)
    }

    /// Returns whether the terminal changed since the last poll.
    pub fn take_dirty(&self) -> bool {
        self.dirty.swap(false, Ordering::AcqRel)
    }

    /// Start a new mouse selection at the given visible viewport cell.
    pub fn begin_selection(&self, row: usize, col: usize, selection_type: SelectionType) {
        if let Ok(mut term) = self.term.lock() {
            let Some(point) = viewport_point(&term, row, col) else {
                return;
            };
            term.selection = Some(Selection::new(selection_type, point, Side::Left));
            self.dirty.store(true, Ordering::Release);
        }
    }

    /// Extend the current mouse selection to the given visible viewport cell.
    pub fn update_selection(&self, row: usize, col: usize) {
        if let Ok(mut term) = self.term.lock() {
            let Some(point) = viewport_point(&term, row, col) else {
                return;
            };
            if let Some(selection) = term.selection.as_mut() {
                selection.update(point, Side::Right);
                selection.include_all();
                self.dirty.store(true, Ordering::Release);
            }
        }
    }

    /// Clear any active selection.
    pub fn clear_selection(&self) {
        if let Ok(mut term) = self.term.lock() {
            if term.selection.take().is_some() {
                self.dirty.store(true, Ordering::Release);
            }
        }
    }

    /// Copy the current terminal selection, if any.
    pub fn selection_text(&self) -> Option<String> {
        self.term
            .lock()
            .ok()
            .and_then(|term| term.selection_to_string())
            .filter(|text| !text.is_empty())
    }

    /// Select the entire terminal buffer, including scrollback.
    pub fn select_all(&self) {
        if let Ok(mut term) = self.term.lock() {
            let columns = term.columns();
            if columns == 0 {
                return;
            }

            let start = Point::new(term.grid().topmost_line(), Column(0));
            let end = Point::new(
                term.grid().bottommost_line(),
                Column(columns.saturating_sub(1)),
            );
            let mut selection = Selection::new(SelectionType::Simple, start, Side::Left);
            selection.update(end, Side::Right);
            term.selection = Some(selection);
            self.dirty.store(true, Ordering::Release);
        }
    }

    // ── Private ──────────────────────────────────────────────────────

    fn reader_loop(
        mut reader: Box<dyn Read + Send>,
        term: Arc<Mutex<term::Term<Listener>>>,
        alive: Arc<Mutex<bool>>,
        dirty: Arc<AtomicBool>,
    ) {
        let mut processor = vte::ansi::Processor::<vte::ansi::StdSyncHandler>::new();
        let mut buf = [0u8; 8192];

        loop {
            match reader.read(&mut buf) {
                Ok(0) => break,  // EOF — shell exited
                Err(_) => break, // PTY closed
                Ok(n) => {
                    if let Ok(mut term) = term.lock() {
                        processor.advance(&mut *term, &buf[..n]);
                        dirty.store(true, Ordering::Release);
                    }
                }
            }
        }

        *alive.lock().unwrap() = false;
        dirty.store(true, Ordering::Release);
    }
}

fn viewport_point(term: &term::Term<Listener>, row: usize, col: usize) -> Option<Point> {
    let columns = term.columns();
    let rows = term.screen_lines();
    if columns == 0 || rows == 0 {
        return None;
    }

    let row = row.min(rows.saturating_sub(1));
    let col = col.min(columns.saturating_sub(1));
    let display_offset = term.grid().display_offset() as i32;

    Some(Point::new(Line(row as i32 - display_offset), Column(col)))
}
