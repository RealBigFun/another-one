use std::io::{self, Write};
use std::sync::{Arc, Mutex};

use alacritty_terminal::vte::ansi::Rgb;
#[cfg(test)]
use gpui::rgb;
use gpui::{font, px, FontWeight, Hsla, StrikethroughStyle, TextRun, UnderlineStyle};
use portable_pty::MasterPty;

// Phase 5a (design 01 / #158): daemon-canonical TerminalFrame is
// the snapshot the GPUI renderer is migrating to. Imported here so
// `LiveTerminalRuntime::ingest_frame` can ingest a `Full` frame
// into the existing surface-snapshot cache.
use daemon_proto::{
    GridCell as ProtoGridCell, GridCellFlags as ProtoGridCellFlags, GridColor as ProtoGridColor,
    GridSnapshot as ProtoGridSnapshot, TerminalFrame,
};

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
    /// `Some` for locally-spawned PTYs (desktop); `None` for
    /// session-attached viewers (mobile). When `None`, input must
    /// route through `daemon_transport::Session::push_data` and
    /// resize requests must travel as `Control::TabResize`.
    local_pty: Option<LocalPty>,
    size: TerminalGridSize,
    dirty: bool,
    /// Live (un-scrolled) surface composed from the latest
    /// `ingest_frame`. Renderer reads this when `viewer_scroll_offset`
    /// is 0; non-zero offsets compose a fresh `TerminalSurfaceSnapshot`
    /// on the fly using `scrollback_cache`.
    cached_snapshot: TerminalSurfaceSnapshot,
    /// Latest live `GridSnapshot` retained from `ingest_frame` so
    /// the viewer can recompose the visible rows on the fly when
    /// `viewer_scroll_offset > 0`. `None` until the first frame is
    /// ingested.
    live_snapshot: Option<Arc<ProtoGridSnapshot>>,
    /// Lines of scrollback the daemon currently retains. Read from
    /// the most recent `ingest_frame`'s `history_lines` so the viewer
    /// can clamp scroll requests without an extra round-trip.
    history_lines: u32,
    /// Lines the viewer has scrolled up from the live screen. `0`
    /// means "viewing the live screen". Bounded by `history_lines`.
    viewer_scroll_offset: u32,
    /// Cached scrollback rows fetched via `Control::TerminalReadScrollback`.
    /// Keyed by the `start` value the daemon's `read_scrollback` uses
    /// (`1` = `Line(-1)` = one row above the live screen, increasing
    /// into the past). `start = 0` is the topmost live row and lives
    /// in `live_snapshot.viewport[0]`, so the cache stores entries for
    /// `start >= 1` only.
    scrollback_cache: std::collections::BTreeMap<u32, daemon_proto::GridRow>,
}

impl LiveTerminalRuntime {
    pub fn from_prepared(prepared: PreparedTerminalRuntime) -> Self {
        Self {
            local_pty: Some(LocalPty {
                master: prepared.master,
                writer: Arc::new(Mutex::new(prepared.writer)),
                child_killer: prepared.child_killer,
                output_broadcast: prepared.output_broadcast,
            }),
            size: prepared.size,
            dirty: true,
            cached_snapshot: TerminalSurfaceSnapshot::default(),
            live_snapshot: None,
            history_lines: 0,
            viewer_scroll_offset: 0,
            scrollback_cache: std::collections::BTreeMap::new(),
        }
    }

    /// Construct a viewer-only runtime backed by a remote PTY (i.e.
    /// the daemon owns the actual shell process). Input must be
    /// routed through `daemon_transport::Session::push_data` by the
    /// caller — the runtime's `write_input` is a no-op when there is
    /// no local PTY.
    pub fn from_remote(size: TerminalGridSize) -> Self {
        Self {
            local_pty: None,
            size,
            dirty: true,
            cached_snapshot: TerminalSurfaceSnapshot::default(),
            live_snapshot: None,
            history_lines: 0,
            viewer_scroll_offset: 0,
            scrollback_cache: std::collections::BTreeMap::new(),
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
        self.size = size;
        self.dirty = true;
        Ok(true)
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
        let payload = encode_paste_payload(text, self.bracketed_paste());
        self.write_input(payload.as_bytes())
    }

    /// Lines the viewer has scrolled up from the live screen. `0` =
    /// viewing the live bottom; positive values walk into history.
    /// Drives the search-overlay's grid-coord → viewport-row mapping
    /// (formerly read from `self.term.grid().display_offset()` before
    /// the daemon-canonical cutover).
    pub fn display_offset(&self) -> usize {
        self.viewer_scroll_offset as usize
    }

    /// Current grid dimensions. Phase 5b uses this when spawning a
    /// daemon-canonical Term task alongside the legacy runtime so
    /// both sides agree on the initial cols/rows.
    pub fn size(&self) -> TerminalGridSize {
        self.size
    }

    pub fn screen_lines(&self) -> usize {
        // Authoritative source post-cutover is the latest ingested
        // snapshot; fall back to the runtime's announced size when
        // no frame has landed yet.
        self.live_snapshot
            .as_ref()
            .map(|s| s.rows as usize)
            .unwrap_or(self.size.rows as usize)
    }

    /// Total scrollback lines the daemon currently retains. Sourced
    /// from the most recent `ingest_frame`; viewers use it to clamp
    /// `viewer_scroll_lines` without an extra round-trip.
    #[allow(dead_code)]
    pub fn history_lines(&self) -> u32 {
        self.history_lines
    }

    /// True when the running TUI has switched to the alternate
    /// screen (vim, less, htop, …). Read from the most recent
    /// `ingest_frame`'s `mode.alt_screen`.
    pub fn is_alternate_screen(&self) -> bool {
        self.live_snapshot
            .as_ref()
            .map(|s| s.mode.alt_screen)
            .unwrap_or(false)
    }

    /// Inspect the active mouse-tracking mode, if any. Returns `None` when
    /// the application has not enabled mouse reporting — in which case the
    /// host should treat mouse events as native (selection, link click).
    /// Sourced from the daemon-pushed `GridSnapshot::mode` flags.
    pub fn mouse_protocol(&self) -> Option<TerminalMouseProtocol> {
        let mode = self.live_snapshot.as_ref().map(|s| s.mode)?;
        let level = if mode.mouse_motion {
            TerminalMouseLevel::AnyMotion
        } else if mode.mouse_drag {
            TerminalMouseLevel::ButtonDrag
        } else if mode.mouse_click {
            TerminalMouseLevel::ClickOnly
        } else {
            return None;
        };
        let encoding = if mode.sgr_mouse {
            TerminalMouseEncoding::Sgr
        } else if mode.utf8_mouse {
            TerminalMouseEncoding::Utf8
        } else {
            TerminalMouseEncoding::Default
        };
        Some(TerminalMouseProtocol { level, encoding })
    }

    /// Whether the running TUI has enabled DECSET 2004 (bracketed
    /// paste). Sourced from the latest daemon frame's mode flags.
    fn bracketed_paste(&self) -> bool {
        self.live_snapshot
            .as_ref()
            .map(|s| s.mode.bracketed_paste)
            .unwrap_or(false)
    }

    pub fn request_soft_redraw(&self) -> io::Result<()> {
        self.write_input(b"\x0c")
    }

    /// Ingest a daemon-canonical [`TerminalFrame`] into the cached
    /// surface snapshot. `Full` frames replace the cache wholesale.
    /// `Diff` is reserved for Phase 8 of the design (deferred);
    /// receiving one here logs and discards.
    ///
    /// On `Full` ingest the runtime also refreshes its `history_lines`
    /// hint and (when `viewer_scroll_offset > 0`) recomposes the
    /// visible surface so any newly-arrived live rows or cleared
    /// scrollback land before the next `snapshot()` call.
    pub fn ingest_frame(&mut self, frame: &TerminalFrame) {
        match frame {
            TerminalFrame::Full { seq, snapshot } => {
                let span = tracing::trace_span!(
                    "terminal.ingest_frame",
                    seq = *seq,
                    cols = snapshot.cols,
                    rows = snapshot.rows,
                    history = snapshot.history_lines,
                );
                let _enter = span.enter();
                self.live_snapshot = Some(Arc::clone(snapshot));
                self.history_lines = snapshot.history_lines;
                if self.viewer_scroll_offset > self.history_lines {
                    self.viewer_scroll_offset = self.history_lines;
                }
                self.recompute_cached_snapshot();
                self.dirty = false;
            }
            TerminalFrame::Diff { seq, .. } => {
                log::debug!(
                    "LiveTerminalRuntime::ingest_frame: Diff seq={seq} not yet supported (Phase 8)"
                );
            }
        }
    }

    pub fn snapshot(&mut self) -> TerminalSurfaceSnapshot {
        // Daemon-canonical (design 01 / #158, Phase 5b): the cached
        // snapshot is authoritative. `ingest_frame` and viewer
        // scroll changes both refresh it via `recompute_cached_snapshot`;
        // `dirty` becomes a pure invalidation hint here.
        self.dirty = false;
        self.cached_snapshot.clone()
    }

    /// Recompose `cached_snapshot` from `live_snapshot` overlaid with
    /// any cached scrollback rows in view at the current
    /// `viewer_scroll_offset`.
    fn recompute_cached_snapshot(&mut self) {
        let Some(live) = self.live_snapshot.as_ref() else {
            self.cached_snapshot = TerminalSurfaceSnapshot::default();
            return;
        };
        if self.viewer_scroll_offset == 0 {
            self.cached_snapshot = build_surface_snapshot(live);
            return;
        }
        let composed = self.compose_scrolled_snapshot(live);
        self.cached_snapshot = build_surface_snapshot(&composed);
    }

    /// Build a `ProtoGridSnapshot` whose `viewport` is the visible
    /// rows for the current `viewer_scroll_offset`. Missing
    /// scrollback rows are filled with `GridRow::default()` (blank
    /// rows) until the corresponding `Control::TerminalReadScrollback`
    /// reply lands and `apply_scrollback_reply` extends the cache.
    ///
    /// Row mapping (with viewer offset `N`, live screen `rows` tall):
    /// viewer row `k` corresponds to grid line `k - N`. Non-negative
    /// lines come from `live.viewport[line]`; negative lines
    /// (`-1`, `-2`, …) come from the scrollback cache at daemon
    /// offsets `1`, `2`, … respectively. Top of viewer = oldest row
    /// in window; bottom of viewer = `rows - 1 - N` lines above the
    /// live bottom.
    fn compose_scrolled_snapshot(&self, live: &ProtoGridSnapshot) -> ProtoGridSnapshot {
        let rows = live.rows as usize;
        let offset = self.viewer_scroll_offset as i64;
        let mut viewport = Vec::with_capacity(rows);
        for k in 0..rows {
            let line = k as i64 - offset;
            let row = if line >= 0 {
                live
                    .viewport
                    .get(line as usize)
                    .cloned()
                    .unwrap_or_default()
            } else {
                let daemon_offset = (-line) as u32;
                self.scrollback_cache
                    .get(&daemon_offset)
                    .cloned()
                    .unwrap_or_default()
            };
            viewport.push(row);
        }
        // Hide the cursor when the viewer is scrolled off the live
        // bottom — typical terminal UX (xterm, kitty, alacritty all
        // do this), and avoids painting a stale caret position over
        // historical content.
        let mut cursor = live.cursor;
        cursor.visible = false;
        ProtoGridSnapshot {
            cols: live.cols,
            rows: live.rows,
            viewport,
            backbuffer: Vec::new(),
            cursor,
            mode: live.mode,
            scroll_offset: self.viewer_scroll_offset,
            history_lines: live.history_lines,
        }
    }

    /// The current viewer scrollback offset. `0` = viewing the live
    /// screen.
    #[allow(dead_code)]
    pub fn viewer_scroll_offset(&self) -> u32 {
        self.viewer_scroll_offset
    }

    /// Adjust the viewer scroll offset by `delta` lines (positive =
    /// scroll up into history, negative = scroll back down toward the
    /// live bottom). Clamps to `[0, history_lines]`. Returns `true`
    /// when the offset moved.
    pub fn viewer_scroll_lines(&mut self, delta: i32) -> bool {
        if delta == 0 {
            return false;
        }
        let current = self.viewer_scroll_offset as i64;
        let max = self.history_lines as i64;
        let next = (current + delta as i64).clamp(0, max) as u32;
        if next == self.viewer_scroll_offset {
            return false;
        }
        self.viewer_scroll_offset = next;
        self.recompute_cached_snapshot();
        true
    }

    /// Scroll the viewer to a specific absolute offset (clamped).
    /// Returns `true` when the offset moved.
    pub fn viewer_set_scroll_offset(&mut self, offset: u32) -> bool {
        let next = offset.min(self.history_lines);
        if next == self.viewer_scroll_offset {
            return false;
        }
        self.viewer_scroll_offset = next;
        self.recompute_cached_snapshot();
        true
    }

    /// Compute the smallest contiguous scrollback range the viewer is
    /// missing for the current scroll offset, or `None` when no fetch
    /// is needed (cache covers the visible window, or viewer is on
    /// the live bottom). Caller dispatches
    /// `Control::TerminalReadScrollback` with the returned range and
    /// feeds the reply through `apply_scrollback_reply`.
    pub fn missing_scrollback_window(&self) -> Option<daemon_proto::ScrollbackRange> {
        if self.viewer_scroll_offset == 0 {
            return None;
        }
        let rows = self
            .live_snapshot
            .as_ref()
            .map(|s| s.rows as u32)
            .unwrap_or(self.size.rows as u32);
        if rows == 0 {
            return None;
        }
        // Scrollback rows visible at the current offset have daemon
        // offsets `[1..=offset]` when offset < rows (top portion is
        // the rest of live). When offset >= rows, they're
        // `[offset - rows + 1 ..= offset]`. Deduplicate by walking
        // the closed range and finding the first/last missing entry.
        let lo = if self.viewer_scroll_offset >= rows {
            self.viewer_scroll_offset - (rows - 1)
        } else {
            1
        };
        let hi = self.viewer_scroll_offset;
        if lo > hi {
            return None;
        }
        let mut first_missing: Option<u32> = None;
        let mut last_missing: Option<u32> = None;
        for s in lo..=hi {
            if !self.scrollback_cache.contains_key(&s) {
                first_missing.get_or_insert(s);
                last_missing = Some(s);
            }
        }
        let (start, end) = (first_missing?, last_missing?);
        Some(daemon_proto::ScrollbackRange {
            start,
            count: end - start + 1,
        })
    }

    /// Merge a `Control::TerminalReadScrollback` reply into the
    /// viewer-side cache and re-compose the visible surface if the
    /// new rows fall inside the current scroll window.
    pub fn apply_scrollback_reply(&mut self, reply: &daemon_proto::TerminalScrollbackReply) {
        let actual = reply.range_actual;
        // The reply is oldest-first relative to the requested range:
        // index 0 -> daemon offset `actual.start`, index 1 -> `start + 1`, …
        for (i, row) in reply.rows.iter().enumerate() {
            let key = actual.start + i as u32;
            // Skip start=0 — that's the topmost live row (already in
            // `live_snapshot.viewport[0]`). Caching it would just
            // duplicate live state and risk drifting out of sync
            // when the next frame ingests.
            if key == 0 {
                continue;
            }
            self.scrollback_cache.insert(key, row.clone());
        }
        if self.viewer_scroll_offset > 0 {
            self.recompute_cached_snapshot();
        }
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

    /// Adjust the viewer scroll offset so the given match lies near
    /// the vertical middle of the viewport. No-op if the match is
    /// already visible without scrolling. Returns `true` when the
    /// offset changed.
    pub fn scroll_to_match(&mut self, target: &TerminalScrollbackMatch) -> bool {
        let screen_lines = self.screen_lines() as i32;
        if screen_lines == 0 {
            return false;
        }
        let history = self.history_lines as i32;
        let current = self.viewer_scroll_offset as i32;
        // Visible lines at offset `current` span `[-current, rows - 1 - current]`
        // (matches the alacritty `display_offset` math the legacy
        // implementation used). Skip the scroll if the match is
        // already on screen.
        let top = -current;
        let bot = (screen_lines - 1) - current;
        if target.line >= top && target.line <= bot {
            return false;
        }
        // Land the match at viewer row `screen_lines / 2`. Solving
        // `target.line = (screen_lines / 2) - offset` gives:
        let centered = (screen_lines / 2) - target.line;
        let target_offset = centered.clamp(0, history);
        self.viewer_set_scroll_offset(target_offset as u32)
    }

    /// Move the viewer's scroll offset by `lines` (positive = scroll
    /// up into history). Daemon-canonical replacement for the
    /// legacy `term.scroll_display(Scroll::Delta)`. Returns `true`
    /// when the offset moved.
    pub fn scroll_display(&mut self, lines: i32) -> bool {
        self.viewer_scroll_lines(lines)
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


/// Build a [`TerminalSurfaceSnapshot`] from a daemon-canonical
/// [`ProtoGridSnapshot`]. Mirrors the alacritty-driven
/// [`build_surface_snapshot`] but reads from the wire types.
///
/// Phase 5a parity scope (design 01 / #158): produce a snapshot
/// whose `text`, per-line `text`, `columns`, and basic `cells`
/// match the alacritty path for equivalent grid contents.
/// Positioned runs (ligature handling), background spans, and
/// hyperlink resolution are deferred — they cost a renderer
/// round-trip the wire frame doesn't carry today and the
/// renderer falls back gracefully when those slices are empty.
fn build_surface_snapshot(snapshot: &ProtoGridSnapshot) -> TerminalSurfaceSnapshot {
    let columns = snapshot.cols as usize;
    let rows = snapshot.rows as usize;
    let cursor_pos = snapshot
        .cursor
        .visible
        .then(|| (snapshot.cursor.line as usize, snapshot.cursor.col as usize));
    let mut lines = Vec::with_capacity(rows);
    let mut positioned_runs = Vec::new();
    let mut cursor_snapshot: Option<TerminalCursorSnapshot> = None;

    for row_idx in 0..rows {
        let row = snapshot.viewport.get(row_idx);
        let mut text = String::new();
        let mut cells = Vec::new();
        let mut runs = Vec::new();
        let mut background_spans = Vec::new();
        let mut pending_blank_run = PendingTextRun::default();
        let mut pending_positioned_run: Option<PendingPositionedRun> = None;

        for column in 0..columns {
            let default_cell = ProtoGridCell::default();
            let cell = row
                .and_then(|r| r.cells.get(column))
                .unwrap_or(&default_cell);

            // Wide-char spacer cells are emitted on the wire to
            // preserve column indexing but don't render as their
            // own glyph — the preceding wide character covers
            // their column. Skipping here mirrors the alacritty
            // path's `Flags::WIDE_CHAR_SPACER` handling.
            if cell.flags.0
                & (ProtoGridCellFlags::WIDE_CHAR_SPACER
                    | ProtoGridCellFlags::LEADING_WIDE_CHAR_SPACER)
                != 0
            {
                continue;
            }

            let is_cursor =
                cursor_pos.is_some_and(|(line, col)| line == row_idx && col == column);
            let mut cell_style = resolve_cell_style(cell);
            let has_explicit_background =
                effective_background_color(cell) != ProtoGridColor::Default;
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
                hyperlink: cell.hyperlink.clone(),
            });

            if is_cursor {
                if let Some(snap) = cursor_snapshot_for_cell(
                    row_idx,
                    column,
                    cell_width,
                    snapshot.cursor.shape,
                ) {
                    if snap.kind == TerminalCursorKind::Block {
                        cell_style.foreground = default_background_color();
                    }
                    cursor_snapshot = Some(snap);
                }
            }

            let positioned_style = text_run_from_style(cell_style.clone());
            if !cell_is_render_blank(cell) {
                append_positioned_run(
                    &mut pending_positioned_run,
                    &mut positioned_runs,
                    row_idx,
                    column,
                    cell_width,
                    chunk.clone(),
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

    let surface_text = lines
        .iter()
        .map(|line| line.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");

    // Cursor on an empty/out-of-range cell: synthesize a block at the
    // reported position so the renderer still draws a caret.
    if cursor_snapshot.is_none() {
        if let Some((line, column)) = cursor_pos {
            cursor_snapshot = cursor_snapshot_for_cell(line, column, 1, snapshot.cursor.shape);
        }
    }

    TerminalSurfaceSnapshot {
        text: surface_text,
        columns,
        lines,
        positioned_runs,
        cursor: cursor_snapshot,
    }
}

// ───────── cell helpers (proto-frame) ─────────
//
// One source of truth post-cutover: every renderer-visible cell
// derives from the wire types in `daemon_proto`. The alacritty-grid
// siblings retired with design 01 / #158 Phase 5b. Keep the
// signatures unprefixed so call sites read naturally.

fn cell_display_text(cell: &ProtoGridCell) -> String {
    let ch = if cell.flags.contains(ProtoGridCellFlags::HIDDEN)
        || cell.ch == ' '
        || cell.ch == '\0'
    {
        '\u{00a0}'
    } else {
        cell.ch
    };
    let mut out = String::with_capacity(1 + cell.zero_width.len());
    out.push(ch);
    // Combining marks / ZWJ tail (e.g. accented letters, flag
    // emoji, family ZWJ sequences) render over the same cell.
    // Forward them so the renderer composes the glyph correctly.
    for combiner in &cell.zero_width {
        out.push(*combiner);
    }
    out
}

fn cell_copy_text(cell: &ProtoGridCell) -> String {
    if cell.flags.contains(ProtoGridCellFlags::HIDDEN) {
        return " ".to_string();
    }
    let ch = if cell.ch == '\0' { ' ' } else { cell.ch };
    let mut out = String::with_capacity(1 + cell.zero_width.len());
    out.push(ch);
    for combiner in &cell.zero_width {
        out.push(*combiner);
    }
    out
}

fn cell_is_render_blank(cell: &ProtoGridCell) -> bool {
    if cell.ch != ' ' && cell.ch != '\0' {
        return false;
    }
    if !cell.zero_width.is_empty() {
        return false;
    }
    if cell.bg != ProtoGridColor::Default {
        return false;
    }
    if cell.flags.0
        & (ProtoGridCellFlags::UNDERLINE
            | ProtoGridCellFlags::DOUBLE_UNDERLINE
            | ProtoGridCellFlags::UNDERCURL
            | ProtoGridCellFlags::DOTTED_UNDERLINE
            | ProtoGridCellFlags::DASHED_UNDERLINE
            | ProtoGridCellFlags::INVERSE
            | ProtoGridCellFlags::STRIKEOUT)
        != 0
    {
        return false;
    }
    true
}

fn cell_is_trimmable_blank(cell: &ProtoGridCell) -> bool {
    if !cell.zero_width.is_empty() {
        return false;
    }
    cell.flags.contains(ProtoGridCellFlags::HIDDEN) || cell.ch == ' ' || cell.ch == '\0'
}

fn terminal_cell_width(cell: &ProtoGridCell) -> usize {
    if cell.flags.contains(ProtoGridCellFlags::WIDE_CHAR) {
        2
    } else {
        1
    }
}

fn effective_background_color(cell: &ProtoGridCell) -> ProtoGridColor {
    if cell.flags.contains(ProtoGridCellFlags::INVERSE) {
        cell.fg
    } else {
        cell.bg
    }
}

fn resolve_cell_style(cell: &ProtoGridCell) -> ResolvedCellStyle {
    let raw_foreground = cell.fg;
    let mut foreground = resolve_grid_color(cell.fg, cell.flags, true);
    let mut background = resolve_grid_color(cell.bg, cell.flags, false);

    if cell.flags.contains(ProtoGridCellFlags::INVERSE) {
        std::mem::swap(&mut foreground, &mut background);
    }

    if cell.flags.contains(ProtoGridCellFlags::HIDDEN) {
        foreground = background;
    } else {
        foreground = ensure_light_terminal_foreground_contrast(
            foreground,
            background,
            matches!(raw_foreground, ProtoGridColor::Rgb { .. }),
            crate::theme::current_terminal_theme(),
        );
    }

    ResolvedCellStyle {
        foreground,
        background,
        font: terminal_font(cell.flags),
        underline: underline_style(cell, foreground),
        strikethrough: cell
            .flags
            .contains(ProtoGridCellFlags::STRIKEOUT)
            .then(|| StrikethroughStyle {
                thickness: px(1.),
                color: Some(foreground),
            }),
    }
}

fn terminal_font(flags: ProtoGridCellFlags) -> gpui::Font {
    let mut font = font("Lilex Nerd Font Mono");
    if flags.contains(ProtoGridCellFlags::BOLD) {
        font.weight = FontWeight::BOLD;
    }
    if flags.contains(ProtoGridCellFlags::ITALIC) {
        font = font.italic();
    }
    font
}

fn underline_style(cell: &ProtoGridCell, foreground: Hsla) -> Option<UnderlineStyle> {
    let any_underline = cell.flags.0
        & (ProtoGridCellFlags::UNDERLINE
            | ProtoGridCellFlags::DOUBLE_UNDERLINE
            | ProtoGridCellFlags::UNDERCURL
            | ProtoGridCellFlags::DOTTED_UNDERLINE
            | ProtoGridCellFlags::DASHED_UNDERLINE)
        != 0;
    if !any_underline {
        return None;
    }
    // SGR 58 (`4:N` underline-color) rides on the cell's
    // `underline_color` wire field. `Default` falls back to the
    // resolved foreground so plain SGR 4 keeps painting in the text
    // colour, matching alacritty's `cell.underline_color()` returning
    // `None`.
    let color = match cell.underline_color {
        ProtoGridColor::Default => foreground,
        other => resolve_grid_color(other, cell.flags, true),
    };
    Some(UnderlineStyle {
        thickness: px(if cell.flags.contains(ProtoGridCellFlags::DOUBLE_UNDERLINE) {
            2.
        } else {
            1.
        }),
        color: Some(color),
        wavy: cell.flags.contains(ProtoGridCellFlags::UNDERCURL),
    })
}

/// Resolve a wire-format `GridColor` to an `Hsla`, applying the same
/// dim/bold-on-named promotion the alacritty `resolve_color` does.
/// `Default` colors snap to the active theme's terminal default fg/bg;
/// `Indexed` colors fall through to the same xterm 256-color cube
/// `default_indexed_color` builds; `Rgb` is taken verbatim.
fn resolve_grid_color(color: ProtoGridColor, flags: ProtoGridCellFlags, is_foreground: bool) -> Hsla {
    match color {
        ProtoGridColor::Default => {
            if is_foreground {
                if flags.contains(ProtoGridCellFlags::DIM) {
                    let [r, g, b] = crate::theme::current_terminal_palette().dim_foreground;
                    gpui::rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into()
                } else {
                    default_foreground_color()
                }
            } else {
                default_background_color()
            }
        }
        ProtoGridColor::Rgb { r, g, b } => {
            gpui::rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32).into()
        }
        ProtoGridColor::Indexed { index } => {
            let mut idx = index;
            // Bold promotes the low-8 ANSI palette to its bright
            // counterpart for foreground only, matching alacritty's
            // `NamedColor::to_bright`. Dim demotes; for indexed we
            // do not have a dim palette so leave as-is.
            if is_foreground && flags.contains(ProtoGridCellFlags::BOLD) && idx < 8 {
                idx += 8;
            }
            let rgb = default_indexed_color(idx);
            gpui::rgb(((rgb.r as u32) << 16) | ((rgb.g as u32) << 8) | rgb.b as u32).into()
        }
    }
}

fn cursor_snapshot_for_cell(
    line: usize,
    column: usize,
    width: usize,
    shape: daemon_proto::CursorShape,
) -> Option<TerminalCursorSnapshot> {
    use daemon_proto::CursorShape as Shape;
    let kind = match shape {
        Shape::Underline | Shape::UnderlineBlinking => TerminalCursorKind::Underline,
        Shape::Beam | Shape::BeamBlinking => TerminalCursorKind::Beam,
        Shape::HollowBlock | Shape::HollowBlockBlinking => TerminalCursorKind::HollowBlock,
        _ => TerminalCursorKind::Block,
    };
    Some(TerminalCursorSnapshot {
        line,
        column,
        width,
        kind,
        color: crate::theme::terminal_default_cursor(),
        blinking: matches!(
            shape,
            Shape::BlockBlinking
                | Shape::UnderlineBlinking
                | Shape::BeamBlinking
                | Shape::HollowBlockBlinking
        ),
    })
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


fn default_indexed_color(index: u8) -> Rgb {
    let palette = crate::theme::current_terminal_palette();
    match index {
        0..=7 => rgb_from_triple(palette.normal[index as usize]),
        8..=15 => rgb_from_triple(palette.bright[(index - 8) as usize]),
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

fn rgb_from_triple([r, g, b]: [u8; 3]) -> Rgb {
    Rgb { r, g, b }
}

fn default_background_color() -> Hsla {
    crate::theme::terminal_default_background()
}

fn default_foreground_color() -> Hsla {
    crate::theme::terminal_default_foreground()
}

#[cfg(test)]
mod tests {
    use super::*;

    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::vte::ansi;

    fn snapshot_from_ansi(bytes: &[u8]) -> TerminalSurfaceSnapshot {
        // Drive the canonical pipeline end-to-end: alacritty Term
        // → daemon_proto::GridSnapshot (via the daemon's serializer)
        // → the unified renderer-side `build_surface_snapshot`. Tests
        // that captured the legacy alacritty-grid builder's output
        // before Phase 5b cutover stay green so long as both halves
        // of the pipeline preserve the same behaviour.
        let _termsize = TermSize::new(32, 4);
        let size = TerminalGridSize {
            cols: 32,
            rows: 4,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut term = daemon::terminal::term_config::make_term(size, VoidListener);
        let mut parser = ansi::Processor::<ansi::StdSyncHandler>::default();
        parser.advance(&mut term, bytes);
        let frame = daemon::terminal::frame::serialize_full_frame(&term, 1);
        match &*frame {
            TerminalFrame::Full { snapshot, .. } => build_surface_snapshot(snapshot),
            other => panic!("expected Full frame, got {other:?}"),
        }
    }

    /// Phase 5a parity helper. Builds a synthetic
    /// `daemon_proto::GridSnapshot` whose viewport mirrors the
    /// alacritty grid at the same dims, with the supplied per-row
    /// text. Wide chars / colored cells are out of scope for the
    /// parity test; this is the simplest input shape that
    /// exercises the path.
    fn proto_snapshot_from_lines(cols: u16, rows: u16, lines: &[&str]) -> ProtoGridSnapshot {
        use daemon_proto::{CursorState, CursorShape, GridCell, GridRow, ModeFlags};
        let viewport: Vec<GridRow> = (0..rows as usize)
            .map(|i| {
                let line = lines.get(i).copied().unwrap_or("");
                let cells: Vec<GridCell> = line
                    .chars()
                    .take(cols as usize)
                    .map(|c| GridCell {
                        ch: c,
                        ..GridCell::default()
                    })
                    .collect();
                GridRow { cells }
            })
            .collect();
        ProtoGridSnapshot {
            cols,
            rows,
            viewport,
            backbuffer: Vec::new(),
            cursor: CursorState {
                line: 0,
                col: lines.get(0).map(|s| s.len() as u16).unwrap_or(0),
                shape: CursorShape::Block,
                visible: true,
            },
            mode: ModeFlags::default(),
            scroll_offset: 0,
            history_lines: 0,
        }
    }

    #[test]
    fn ingest_full_frame_matches_apply_output_text() {
        // Phase 5a (design 01 / #158): assert text-level parity
        // between the daemon-canonical ingest path and the
        // legacy alacritty-driven `apply_output`. Per-line text
        // and column count must match for the same logical input.
        let alacritty_surface = snapshot_from_ansi(b"hello\r\nworld");
        let proto_snapshot = proto_snapshot_from_lines(32, 4, &["hello", "world", "", ""]);
        let proto_surface = build_surface_snapshot(&proto_snapshot);

        assert_eq!(proto_surface.columns, alacritty_surface.columns);
        assert_eq!(
            proto_surface.lines.len(),
            alacritty_surface.lines.len(),
            "row counts match"
        );
        // Strip trailing whitespace from each line for comparison
        // — the alacritty path right-pads with default cells, the
        // proto path mirrors that since cells are length-cols.
        for (i, (proto_line, ala_line)) in proto_surface
            .lines
            .iter()
            .zip(alacritty_surface.lines.iter())
            .enumerate()
        {
            let proto_trimmed = proto_line.text.trim_end();
            let ala_trimmed = ala_line.text.trim_end();
            assert_eq!(
                proto_trimmed, ala_trimmed,
                "line {i} content differs: proto={proto_trimmed:?} alacritty={ala_trimmed:?}"
            );
        }
    }

    #[test]
    fn proto_cursor_uses_terminal_cursor_theme_color() {
        let proto = proto_snapshot_from_lines(16, 2, &["hi"]);
        let surface = build_surface_snapshot(&proto);
        let cursor = surface.cursor.expect("visible cursor");
        assert_eq!(cursor.color, crate::theme::terminal_default_cursor());
    }

    #[test]
    fn ingest_full_frame_updates_cached_snapshot() {
        // Stand up a viewer-only LiveTerminalRuntime; ingest a
        // synthetic Full frame; assert the cached snapshot now
        // reflects the proto contents.
        let size = TerminalGridSize {
            cols: 16,
            rows: 2,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut runtime = LiveTerminalRuntime::from_remote(size);
        let proto = proto_snapshot_from_lines(16, 2, &["hi", "there"]);
        let frame = TerminalFrame::Full {
            seq: 1,
            snapshot: std::sync::Arc::new(proto),
        };
        runtime.ingest_frame(&frame);
        let surface = runtime.snapshot();
        assert_eq!(surface.columns, 16);
        assert_eq!(surface.lines.len(), 2);
        assert!(
            surface.lines[0].text.starts_with("hi"),
            "row 0 starts with 'hi', got {:?}",
            surface.lines[0].text
        );
        assert!(
            surface.lines[1].text.starts_with("there"),
            "row 1 starts with 'there', got {:?}",
            surface.lines[1].text
        );
    }

    /// Helper: ingest a `Full` frame for a tiny viewer-only runtime.
    fn ingest_lines(
        runtime: &mut LiveTerminalRuntime,
        cols: u16,
        rows: u16,
        lines: &[&str],
        history_lines: u32,
    ) {
        let mut snapshot = proto_snapshot_from_lines(cols, rows, lines);
        snapshot.history_lines = history_lines;
        let frame = TerminalFrame::Full {
            seq: 1,
            snapshot: std::sync::Arc::new(snapshot),
        };
        runtime.ingest_frame(&frame);
    }

    fn proto_row_from_text(text: &str, cols: u16) -> daemon_proto::GridRow {
        use daemon_proto::{GridCell, GridRow};
        let cells: Vec<GridCell> = text
            .chars()
            .take(cols as usize)
            .map(|c| GridCell {
                ch: c,
                ..GridCell::default()
            })
            .collect();
        GridRow { cells }
    }

    #[test]
    fn viewer_scroll_lines_clamps_to_history() {
        let size = TerminalGridSize {
            cols: 8,
            rows: 2,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut runtime = LiveTerminalRuntime::from_remote(size);
        // No frame yet — nothing to scroll.
        assert!(!runtime.viewer_scroll_lines(5));
        assert_eq!(runtime.viewer_scroll_offset(), 0);

        ingest_lines(&mut runtime, 8, 2, &["top", "bot"], 3);

        // Scroll up two lines is allowed (history is 3).
        assert!(runtime.viewer_scroll_lines(2));
        assert_eq!(runtime.viewer_scroll_offset(), 2);

        // A third "up" is allowed (still <= 3); a fourth gets clamped.
        assert!(runtime.viewer_scroll_lines(1));
        assert_eq!(runtime.viewer_scroll_offset(), 3);
        assert!(!runtime.viewer_scroll_lines(1));
        assert_eq!(runtime.viewer_scroll_offset(), 3);

        // Scroll back down past zero clamps at zero.
        assert!(runtime.viewer_scroll_lines(-10));
        assert_eq!(runtime.viewer_scroll_offset(), 0);
    }

    #[test]
    fn missing_scrollback_window_reports_visible_uncached_rows() {
        let size = TerminalGridSize {
            cols: 8,
            rows: 2,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut runtime = LiveTerminalRuntime::from_remote(size);
        ingest_lines(&mut runtime, 8, 2, &["top", "bot"], 5);

        // No scroll — nothing missing.
        assert!(runtime.missing_scrollback_window().is_none());

        // Scroll up 1: only daemon-offset 1 is visible (the rest of
        // the viewport stays in live).
        assert!(runtime.viewer_scroll_lines(1));
        let win = runtime.missing_scrollback_window().expect("window");
        assert_eq!(win.start, 1);
        assert_eq!(win.count, 1);

        // Scroll up 4 (offset = 5 = history_lines): visible scrollback
        // covers offsets [4, 5] (rows=2 viewport, top row at line=-5,
        // bottom row at line=-4). lo = offset - rows + 1 = 4, hi = 5.
        assert!(runtime.viewer_scroll_lines(4));
        let win = runtime.missing_scrollback_window().expect("window");
        assert_eq!(win.start, 4);
        assert_eq!(win.count, 2);
    }

    #[test]
    fn apply_scrollback_reply_composes_visible_rows() {
        use daemon_proto::{ScrollbackRange, TerminalScrollbackReply};
        let size = TerminalGridSize {
            cols: 8,
            rows: 3,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut runtime = LiveTerminalRuntime::from_remote(size);
        ingest_lines(&mut runtime, 8, 3, &["liveA", "liveB", "liveC"], 5);

        // Scroll up 2: viewer rows top-to-bottom map to grid lines
        // [-2, -1, 0]. So the top two rows come from scrollback at
        // daemon offsets 2 and 1; the bottom row is liveA.
        assert!(runtime.viewer_scroll_lines(2));

        // Before the reply lands, the scrollback rows render blank
        // (cache empty) but the live row anchored at the bottom still
        // shows.
        let surface = runtime.snapshot();
        assert_eq!(surface.lines.len(), 3);
        // surface.lines[2] is the bottom row — liveA (line index 0
        // in live.viewport, the topmost live row).
        assert!(
            surface.lines[2].text.starts_with("liveA"),
            "bottom row from live: got {:?}",
            surface.lines[2].text
        );

        // Reply with rows for daemon offsets [1, 2]: "hist1" and "hist2".
        let reply = TerminalScrollbackReply {
            rows: vec![
                proto_row_from_text("hist1", 8),
                proto_row_from_text("hist2", 8),
            ],
            range_actual: ScrollbackRange { start: 1, count: 2 },
        };
        runtime.apply_scrollback_reply(&reply);

        let surface = runtime.snapshot();
        // viewer row 0 = line -2 = scrollback offset 2 = "hist2".
        assert!(
            surface.lines[0].text.starts_with("hist2"),
            "top row from oldest scrollback: got {:?}",
            surface.lines[0].text
        );
        // viewer row 1 = line -1 = scrollback offset 1 = "hist1".
        assert!(
            surface.lines[1].text.starts_with("hist1"),
            "middle row from newer scrollback: got {:?}",
            surface.lines[1].text
        );
        // viewer row 2 still = liveA.
        assert!(
            surface.lines[2].text.starts_with("liveA"),
            "bottom row stays on live top: got {:?}",
            surface.lines[2].text
        );
        // Cursor hidden when scrolled off the live bottom.
        assert!(
            surface.cursor.is_none(),
            "cursor hidden during scrollback view"
        );
    }

    #[test]
    fn ingest_clamps_viewer_offset_when_history_shrinks() {
        let size = TerminalGridSize {
            cols: 8,
            rows: 2,
            pixel_width: 0,
            pixel_height: 0,
        };
        let mut runtime = LiveTerminalRuntime::from_remote(size);
        ingest_lines(&mut runtime, 8, 2, &["a", "b"], 10);
        assert!(runtime.viewer_scroll_lines(7));
        assert_eq!(runtime.viewer_scroll_offset(), 7);

        // Daemon clears scrollback (e.g. `clear` command). New
        // history_lines = 0; viewer offset must clamp.
        ingest_lines(&mut runtime, 8, 2, &["a", "b"], 0);
        assert_eq!(runtime.viewer_scroll_offset(), 0);
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

    fn triple_hex([r, g, b]: [u8; 3]) -> u32 {
        ((r as u32) << 16) | ((g as u32) << 8) | b as u32
    }

    fn triple_hexes(colors: [[u8; 3]; 8]) -> [u32; 8] {
        colors.map(triple_hex)
    }

    #[test]
    fn ayu_dark_terminal_palette_matches_zed() {
        let palette = crate::theme::terminal_palette(crate::theme::ResolvedTheme::Dark);

        assert_eq!(triple_hex(palette.background), 0x0d1016);
        assert_eq!(triple_hex(palette.foreground), 0xbfbdb6);
        assert_eq!(triple_hex(palette.cursor), 0x5ac1fe);
        assert_eq!(triple_hex(palette.bright_foreground), 0xbfbdb6);
        assert_eq!(triple_hex(palette.dim_foreground), 0x85847f);
        assert_eq!(
            triple_hexes(palette.normal),
            [0x0d1016, 0xef7177, 0xaad84c, 0xfeb454, 0x5ac1fe, 0x39bae5, 0x95e5cb, 0xbfbdb6,]
        );
        assert_eq!(
            triple_hexes(palette.bright),
            [0x545557, 0x83353b, 0x567627, 0x92582b, 0x27618c, 0x205a78, 0x4c806f, 0xfafafa,]
        );
        assert_eq!(
            triple_hexes(palette.dim),
            [0x3a3b3c, 0xa74f53, 0x769735, 0xb17d3a, 0x3e87b1, 0x2782a0, 0x68a08e, 0x85847f,]
        );
    }

    #[test]
    fn ayu_light_terminal_palette_matches_zed() {
        let palette = crate::theme::terminal_palette(crate::theme::ResolvedTheme::Light);

        assert_eq!(triple_hex(palette.background), 0xfcfcfc);
        assert_eq!(triple_hex(palette.foreground), 0x5c6166);
        assert_eq!(triple_hex(palette.cursor), 0x3b9ee5);
        assert_eq!(triple_hex(palette.bright_foreground), 0x5c6166);
        assert_eq!(triple_hex(palette.dim_foreground), 0xfcfcfc);
        assert_eq!(
            triple_hexes(palette.normal),
            [0x5c6166, 0xef7271, 0x85b304, 0xf1ad49, 0x3b9ee5, 0x55b4d3, 0x4dbf99, 0xfcfcfc,]
        );
        assert_eq!(
            triple_hexes(palette.bright),
            [0x3b9ee5, 0xfebab6, 0xc7d98f, 0xfed5a3, 0xabcdf2, 0xb1d8e8, 0xace0cb, 0xffffff,]
        );
        assert_eq!(
            triple_hexes(palette.dim),
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
        assert_eq!(default_indexed_color(232), Rgb { r: 0x08, g: 0x08, b: 0x08 });
        assert_eq!(default_indexed_color(244), Rgb { r: 0x80, g: 0x80, b: 0x80 });
        assert_eq!(default_indexed_color(255), Rgb { r: 0xee, g: 0xee, b: 0xee });
    }
}
