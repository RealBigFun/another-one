//! FRB surface for the alacritty-backed [`TerminalEngine`].
//!
//! Phase 0 spike: keyed by `(section_id, tab_id)` so the Dart side
//! mirrors what it already does for `attach_tab` / `tab_resize` and
//! doesn't have to thread an opaque handle through Riverpod. The
//! registry lives in a process-wide `OnceLock<Mutex<HashMap>>`; one
//! engine per tab, dropped on `engine_close`.
//!
//! Concurrency: each engine is wrapped in its own `Mutex`. The
//! bridge's PTY drain pumps `engine_write_pty` from the Tokio
//! runtime, while Dart's render loop reads `engine_snapshot` from a
//! Flutter isolate-thread call. They serialize through the per-tab
//! mutex.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

use another_one_core::platform::{CurrentPlatform, HeadlessPlatform};
use another_one_core::terminal_engine::{
    self as core_engine, InputEvent, TerminalEngine,
};

type EngineMap = HashMap<EngineKey, Arc<Mutex<Box<dyn TerminalEngine>>>>;

#[derive(Clone, Eq, PartialEq, Hash, Debug)]
struct EngineKey {
    section_id: String,
    tab_id: String,
}

fn registry() -> &'static Mutex<EngineMap> {
    static R: OnceLock<Mutex<EngineMap>> = OnceLock::new();
    R.get_or_init(|| Mutex::new(HashMap::new()))
}

fn lookup(section_id: &str, tab_id: &str) -> Option<Arc<Mutex<Box<dyn TerminalEngine>>>> {
    let map = registry().lock().ok()?;
    map.get(&EngineKey {
        section_id: section_id.to_string(),
        tab_id: tab_id.to_string(),
    })
    .cloned()
}

/// Allocate a terminal engine for `(section_id, tab_id)`. Idempotent:
/// reopening with new dimensions resizes the existing engine rather
/// than dropping its grid state.
pub fn engine_open(
    section_id: String,
    tab_id: String,
    cols: u16,
    rows: u16,
) -> anyhow::Result<()> {
    let key = EngineKey { section_id, tab_id };
    let mut map = registry()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine_open: registry mutex poisoned"))?;
    if let Some(existing) = map.get(&key) {
        if let Ok(mut e) = existing.lock() {
            e.resize(cols, rows);
        }
        return Ok(());
    }
    let engine = CurrentPlatform::create_terminal_engine(cols, rows);
    map.insert(key, Arc::new(Mutex::new(engine)));
    Ok(())
}

pub fn engine_write_pty(
    section_id: String,
    tab_id: String,
    bytes: Vec<u8>,
) -> anyhow::Result<()> {
    let Some(handle) = lookup(&section_id, &tab_id) else {
        return Err(anyhow::anyhow!(
            "engine_write_pty: no engine for section={section_id} tab={tab_id}"
        ));
    };
    let mut engine = handle
        .lock()
        .map_err(|_| anyhow::anyhow!("engine_write_pty: engine mutex poisoned"))?;
    engine.write_pty(&bytes);
    Ok(())
}

pub fn engine_resize(
    section_id: String,
    tab_id: String,
    cols: u16,
    rows: u16,
) -> anyhow::Result<()> {
    let Some(handle) = lookup(&section_id, &tab_id) else {
        return Err(anyhow::anyhow!(
            "engine_resize: no engine for section={section_id} tab={tab_id}"
        ));
    };
    let mut engine = handle
        .lock()
        .map_err(|_| anyhow::anyhow!("engine_resize: engine mutex poisoned"))?;
    engine.resize(cols, rows);
    Ok(())
}

/// FRB-flat snapshot DTO. Mirrors [`core_engine::Snapshot`] but with
/// concrete field types FRB can encode (no `bitflags`, no `Option<T>`
/// unless trivial, no `Box<dyn>`).
pub struct SnapshotDto {
    pub cols: u16,
    pub rows: u16,
    pub cursor_col: u16,
    pub cursor_row: u16,
    pub cursor_visible: bool,
    /// 0 = block, 1 = bar, 2 = underline. Kept as `u8` so future
    /// shapes don't bump the wire tag set.
    pub cursor_style: u8,
    /// `cells.len() == cols * rows`, row-major. Each entry is a
    /// 16-byte `CellDto`.
    pub cells: Vec<CellDto>,
    pub revision: u64,
}

pub struct CellDto {
    pub ch: u32,
    pub fg: u32,
    pub bg: u32,
    pub flags: u16,
}

pub fn engine_snapshot(
    section_id: String,
    tab_id: String,
    scrollback_offset: u32,
    max_rows: u16,
) -> anyhow::Result<SnapshotDto> {
    let Some(handle) = lookup(&section_id, &tab_id) else {
        return Err(anyhow::anyhow!(
            "engine_snapshot: no engine for section={section_id} tab={tab_id}"
        ));
    };
    let engine = handle
        .lock()
        .map_err(|_| anyhow::anyhow!("engine_snapshot: engine mutex poisoned"))?;
    let snap = engine.snapshot(scrollback_offset, max_rows);
    Ok(snapshot_to_dto(snap))
}

fn snapshot_to_dto(snap: core_engine::Snapshot) -> SnapshotDto {
    let cursor_style = match snap.cursor.style {
        core_engine::CursorStyle::Block => 0,
        core_engine::CursorStyle::Bar => 1,
        core_engine::CursorStyle::Underline => 2,
    };
    let cells = snap
        .cells
        .into_iter()
        .map(|c| CellDto {
            ch: c.ch,
            fg: c.fg,
            bg: c.bg,
            flags: c.flags,
        })
        .collect();
    SnapshotDto {
        cols: snap.cols,
        rows: snap.rows,
        cursor_col: snap.cursor.col,
        cursor_row: snap.cursor.row,
        cursor_visible: snap.cursor.visible,
        cursor_style,
        cells,
        revision: snap.revision,
    }
}

/// Wire-side mirror of [`core_engine::InputEvent`]. Tagged-union via
/// `kind` + `code` to keep the FRB bridge tight; expanding the
/// variant set later just adds new `kind` values.
///
/// `kind`:
/// * 0 = Char (uses `code` as the codepoint)
/// * 1 = Enter
/// * 2 = Backspace
/// * 3 = Tab
/// * 4 = Escape
/// * 5 = ArrowUp
/// * 6 = ArrowDown
/// * 7 = ArrowLeft
/// * 8 = ArrowRight
/// * 9 = Resize (uses `code` low 16 bits = cols, high 16 bits = rows)
pub struct InputEventDto {
    pub kind: u8,
    pub code: u32,
}

pub fn engine_encode_input(
    section_id: String,
    tab_id: String,
    event: InputEventDto,
) -> anyhow::Result<Vec<u8>> {
    let Some(handle) = lookup(&section_id, &tab_id) else {
        return Err(anyhow::anyhow!(
            "engine_encode_input: no engine for section={section_id} tab={tab_id}"
        ));
    };
    let engine = handle
        .lock()
        .map_err(|_| anyhow::anyhow!("engine_encode_input: engine mutex poisoned"))?;
    let core_event = match event.kind {
        0 => InputEvent::Char(event.code),
        1 => InputEvent::Enter,
        2 => InputEvent::Backspace,
        3 => InputEvent::Tab,
        4 => InputEvent::Escape,
        5 => InputEvent::ArrowUp,
        6 => InputEvent::ArrowDown,
        7 => InputEvent::ArrowLeft,
        8 => InputEvent::ArrowRight,
        9 => InputEvent::Resize {
            cols: (event.code & 0xFFFF) as u16,
            rows: ((event.code >> 16) & 0xFFFF) as u16,
        },
        other => {
            return Err(anyhow::anyhow!(
                "engine_encode_input: unknown event kind {other}"
            ))
        }
    };
    Ok(engine.encode_input(core_event))
}

pub fn engine_close(section_id: String, tab_id: String) -> anyhow::Result<()> {
    let mut map = registry()
        .lock()
        .map_err(|_| anyhow::anyhow!("engine_close: registry mutex poisoned"))?;
    map.remove(&EngineKey { section_id, tab_id });
    Ok(())
}
