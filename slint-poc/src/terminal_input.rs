//! Terminal input event types and pure terminal protocol encoders.
//!
//! GPUI source of truth: `desktop/src/panels.rs` keyboard handling and
//! `daemon-sandbox/src/frame.rs` for the `TerminalInputEvent` wire shape.
//! This module owns the Slint-side input event types
//! (`SlintKeyEvent`, `SlintPointerEvent`, `TerminalCellPoint`,
//! `TerminalInputModeState`) and the pure helpers that encode them into
//! `frame::TerminalInputEvent` payloads the daemon understands —
//! cursor sequences, control bytes, SGR / legacy mouse encodings,
//! selection-span computation, and the link-uri lookup.
//!
//! Phase A scope: pure pure functions and POD types only. `AppWindow`
//! mutation, daemon dispatching, and the live PTY runtime stay in
//! `lib.rs` (and will move with the bigger terminal-renderer extraction
//! in a later slice).

use daemon_sandbox::frame::TerminalInputEvent;
use tokio::time::Instant;

use crate::{TerminalLinkSpan, TerminalSelectionSpan, TERMINAL_FRAME_INTERVAL};

pub(crate) const SHELL_COLOR_SMOKE_PROBE: &[u8] =
    b"printf '\\033[31mRED \\033[32mGREEN \\033[34mBLUE\\033[0m DEFAULT\\n'\nprintf 'ANOTHERONE_SLINT_READY\\n'\r";
pub(crate) const SHELL_READINESS_PROBE: &[u8] = b"printf 'ANOTHERONE_SLINT_READY\\n'\r";

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SlintKeyEvent {
    pub(crate) text: String,
    pub(crate) control: bool,
    pub(crate) alt: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SlintPointerEvent {
    pub(crate) kind: String,
    pub(crate) button: String,
    pub(crate) column: i32,
    pub(crate) line: i32,
    pub(crate) control: bool,
    pub(crate) alt: bool,
    pub(crate) shift: bool,
}

impl SlintPointerEvent {
    pub(crate) fn is_primary_down(&self) -> bool {
        self.kind == "down" && self.button == "left"
    }

    pub(crate) fn is_primary_move(&self) -> bool {
        self.kind == "move"
    }

    pub(crate) fn is_primary_up(&self) -> bool {
        self.kind == "up" && self.button == "left"
    }

    pub(crate) fn terminal_point(&self) -> Option<TerminalCellPoint> {
        Some(TerminalCellPoint {
            line: self.line,
            column: self.column,
        })
        .filter(|point| point.line >= 0 && point.column >= 0)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct TerminalCellPoint {
    pub(crate) line: i32,
    pub(crate) column: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct TerminalInputModeState {
    pub(crate) app_cursor: bool,
    pub(crate) bracketed_paste: bool,
    pub(crate) focus_in_out: bool,
    pub(crate) mouse_report_click: bool,
    pub(crate) mouse_drag: bool,
    pub(crate) mouse_motion: bool,
    pub(crate) sgr_mouse: bool,
}

pub(crate) fn next_terminal_flush_deadline(now: Instant, last_flush: Instant) -> Instant {
    let earliest = last_flush + TERMINAL_FRAME_INTERVAL;
    if earliest > now {
        earliest
    } else {
        now
    }
}

pub(crate) fn clamp_terminal_dimension(value: i32, min: u16, max: u16) -> u16 {
    value.clamp(i32::from(min), i32::from(max)) as u16
}

pub(crate) fn is_copy_shortcut(input: &SlintKeyEvent) -> bool {
    input.control && !input.alt && input.text.eq_ignore_ascii_case("c")
}

pub(crate) fn encode_terminal_key(
    input: &SlintKeyEvent,
    modes: TerminalInputModeState,
) -> Option<TerminalInputEvent> {
    let text = input.text.as_str();
    let mut bytes = match text {
        "\u{0008}" => vec![0x7f],
        "\u{0009}" => b"\t".to_vec(),
        "\u{000a}" => b"\r".to_vec(),
        "\u{001b}" => b"\x1b".to_vec(),
        "\u{007f}" => b"\x1b[3~".to_vec(),
        "\u{f700}" => cursor_key_sequence(b'A', modes.app_cursor),
        "\u{f701}" => cursor_key_sequence(b'B', modes.app_cursor),
        "\u{f702}" => cursor_key_sequence(b'D', modes.app_cursor),
        "\u{f703}" => cursor_key_sequence(b'C', modes.app_cursor),
        "\u{f729}" => cursor_key_sequence(b'H', modes.app_cursor),
        "\u{f72b}" => cursor_key_sequence(b'F', modes.app_cursor),
        "\u{f72c}" => b"\x1b[5~".to_vec(),
        "\u{f72d}" => b"\x1b[6~".to_vec(),
        value if value.chars().count() == 1 => {
            let ch = value.chars().next()?;
            if input.control {
                control_key_byte(ch)?
            } else if input.alt {
                value.as_bytes().to_vec()
            } else {
                return Some(TerminalInputEvent::Text {
                    text: value.to_string(),
                });
            }
        }
        value if !input.control && input.alt => value.as_bytes().to_vec(),
        value if !input.control => {
            return Some(TerminalInputEvent::Paste {
                text: value.to_string(),
                bracketed: modes.bracketed_paste,
            });
        }
        _ => return None,
    };

    if input.alt {
        bytes.insert(0, 0x1b);
    }

    Some(TerminalInputEvent::Key { bytes })
}

pub(crate) fn encode_terminal_pointer_event(
    input: &SlintPointerEvent,
    modes: TerminalInputModeState,
) -> Option<TerminalInputEvent> {
    if !mouse_reporting_enabled(modes) {
        return None;
    }

    let column = u16::try_from(input.column).ok()?;
    let line = u16::try_from(input.line).ok()?;
    let modifiers = mouse_modifier_bits(input);
    let release = input.kind == "up";
    let code = match input.kind.as_str() {
        "down" | "up" => mouse_button_code(&input.button)?.checked_add(modifiers)?,
        "move" => {
            if !modes.mouse_motion && !modes.mouse_drag {
                return None;
            }
            let button = if input.button == "other" {
                if modes.mouse_motion {
                    3
                } else {
                    return None;
                }
            } else {
                mouse_button_code(&input.button)?
            };
            32u8.checked_add(button)?.checked_add(modifiers)?
        }
        _ => return None,
    };

    let bytes = if modes.sgr_mouse {
        encode_sgr_mouse_button(code, column, line, release)
    } else {
        let legacy_code = if release {
            3u8.checked_add(modifiers)?
        } else {
            code
        };
        encode_legacy_mouse_button(legacy_code, column, line)?
    };

    Some(TerminalInputEvent::Mouse { bytes })
}

pub(crate) fn mouse_reporting_enabled(modes: TerminalInputModeState) -> bool {
    modes.mouse_report_click || modes.mouse_drag || modes.mouse_motion
}

pub(crate) fn mouse_button_code(button: &str) -> Option<u8> {
    match button {
        "left" => Some(0),
        "middle" => Some(1),
        "right" => Some(2),
        _ => None,
    }
}

pub(crate) fn mouse_modifier_bits(input: &SlintPointerEvent) -> u8 {
    let mut bits = 0;
    if input.shift {
        bits |= 4;
    }
    if input.alt {
        bits |= 8;
    }
    if input.control {
        bits |= 16;
    }
    bits
}

pub(crate) fn encode_sgr_mouse_button(code: u8, column: u16, line: u16, release: bool) -> Vec<u8> {
    let suffix = if release { 'm' } else { 'M' };
    format!(
        "\x1b[<{code};{};{}{suffix}",
        u32::from(column) + 1,
        u32::from(line) + 1
    )
    .into_bytes()
}

pub(crate) fn encode_legacy_mouse_button(code: u8, column: u16, line: u16) -> Option<Vec<u8>> {
    Some(vec![
        0x1b,
        b'[',
        b'M',
        code.checked_add(32)?,
        legacy_mouse_coord(column)?,
        legacy_mouse_coord(line)?,
    ])
}

pub(crate) fn legacy_mouse_coord(coord: u16) -> Option<u8> {
    u8::try_from(u32::from(coord) + 33).ok()
}

pub(crate) fn link_uri_at(spans: &[TerminalLinkSpan], line: i32, column: i32) -> Option<String> {
    spans
        .iter()
        .find(|span| {
            span.line == line && column >= span.column && column < span.column + span.cell_count
        })
        .map(|span| span.uri.to_string())
}

pub(crate) fn selection_spans_for_points(
    anchor: TerminalCellPoint,
    focus: TerminalCellPoint,
    columns: u16,
    rows: u16,
) -> Vec<TerminalSelectionSpan> {
    let Some((start, end)) = normalized_selection_points(anchor, focus) else {
        return Vec::new();
    };
    let columns = i32::from(columns);
    let rows = i32::from(rows);
    if columns <= 0 || rows <= 0 {
        return Vec::new();
    }

    let first_line = start.line.clamp(0, rows.saturating_sub(1));
    let last_line = end.line.clamp(0, rows.saturating_sub(1));
    let mut spans = Vec::new();

    for line in first_line..=last_line {
        let column_start = if line == start.line { start.column } else { 0 }.clamp(0, columns);
        let column_end = if line == end.line {
            end.column.saturating_add(1)
        } else {
            columns
        }
        .clamp(0, columns);

        if column_end > column_start {
            spans.push(TerminalSelectionSpan {
                line,
                column: column_start,
                cell_count: column_end - column_start,
            });
        }
    }

    spans
}

pub(crate) fn normalized_selection_points(
    anchor: TerminalCellPoint,
    focus: TerminalCellPoint,
) -> Option<(TerminalCellPoint, TerminalCellPoint)> {
    (anchor != focus).then(|| {
        if anchor <= focus {
            (anchor, focus)
        } else {
            (focus, anchor)
        }
    })
}

pub(crate) fn cursor_key_sequence(final_byte: u8, app_cursor: bool) -> Vec<u8> {
    if app_cursor {
        vec![0x1b, b'O', final_byte]
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

pub(crate) fn control_key_byte(ch: char) -> Option<Vec<u8>> {
    let lower = ch.to_ascii_lowercase();
    if lower.is_ascii_lowercase() {
        Some(vec![(lower as u8) - b'a' + 1])
    } else if ch == ' ' {
        Some(vec![0])
    } else {
        None
    }
}

pub(crate) fn startup_probe() -> Option<&'static [u8]> {
    match std::env::var("ANOTHERONE_SLINT_STARTUP_PROBE").as_deref() {
        Ok("shell-color") => Some(SHELL_COLOR_SMOKE_PROBE),
        Ok("shell-ready") => Some(SHELL_READINESS_PROBE),
        _ => None,
    }
}
