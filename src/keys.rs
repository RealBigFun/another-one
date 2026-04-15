//! Terminal key mappings — adapted from Zed's terminal/src/mappings/keys.rs
//!
//! Converts GPUI keystrokes into the escape sequences expected by a terminal.

use std::borrow::Cow;

use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

#[derive(Debug, PartialEq, Eq)]
enum Mods {
    None,
    Alt,
    Ctrl,
    Shift,
    CtrlShift,
    Other,
}

impl Mods {
    fn new(ks: &Keystroke) -> Self {
        match (
            ks.modifiers.alt,
            ks.modifiers.control,
            ks.modifiers.shift,
            ks.modifiers.platform,
        ) {
            (false, false, false, false) => Mods::None,
            (true, false, false, false) => Mods::Alt,
            (false, true, false, false) => Mods::Ctrl,
            (false, false, true, false) => Mods::Shift,
            (false, true, true, false) => Mods::CtrlShift,
            _ => Mods::Other,
        }
    }

    fn any(&self) -> bool {
        !matches!(self, Mods::None)
    }
}

/// Convert a GPUI keystroke to the terminal escape string.
/// Returns `None` if the keystroke should be handled as plain text.
pub fn to_esc_str(keystroke: &Keystroke, mode: &TermMode) -> Option<Cow<'static, str>> {
    let modifiers = Mods::new(keystroke);

    // Manual bindings including modifiers.
    let manual: Option<&'static str> = match (keystroke.key.as_ref(), &modifiers) {
        // Basic special keys
        ("tab", Mods::None) => Some("\x09"),
        ("escape", Mods::None) => Some("\x1b"),
        ("enter", Mods::None) => Some("\x0d"),
        ("enter", Mods::Shift) => Some("\x0a"),
        ("enter", Mods::Alt) => Some("\x1b\x0d"),
        ("backspace", Mods::None) => Some("\x7f"),
        ("tab", Mods::Shift) => Some("\x1b[Z"),
        ("backspace", Mods::Ctrl) => Some("\x08"),
        ("backspace", Mods::Alt) => Some("\x1b\x7f"),
        ("backspace", Mods::Shift) => Some("\x7f"),
        ("space", Mods::Ctrl) => Some("\x00"),
        ("space", Mods::None) => Some(" "),
        // Cursor movement (APP_CURSOR mode aware)
        ("home", Mods::Shift) if mode.contains(TermMode::ALT_SCREEN) => Some("\x1b[1;2H"),
        ("end", Mods::Shift) if mode.contains(TermMode::ALT_SCREEN) => Some("\x1b[1;2F"),
        ("pageup", Mods::Shift) if mode.contains(TermMode::ALT_SCREEN) => Some("\x1b[5;2~"),
        ("pagedown", Mods::Shift) if mode.contains(TermMode::ALT_SCREEN) => Some("\x1b[6;2~"),
        ("home", Mods::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOH"),
        ("home", Mods::None) => Some("\x1b[H"),
        ("end", Mods::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOF"),
        ("end", Mods::None) => Some("\x1b[F"),
        ("up", Mods::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOA"),
        ("up", Mods::None) => Some("\x1b[A"),
        ("down", Mods::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOB"),
        ("down", Mods::None) => Some("\x1b[B"),
        ("right", Mods::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOC"),
        ("right", Mods::None) => Some("\x1b[C"),
        ("left", Mods::None) if mode.contains(TermMode::APP_CURSOR) => Some("\x1bOD"),
        ("left", Mods::None) => Some("\x1b[D"),
        ("back", Mods::None) => Some("\x7f"),
        ("insert", Mods::None) => Some("\x1b[2~"),
        ("delete", Mods::None) => Some("\x1b[3~"),
        ("pageup", Mods::None) => Some("\x1b[5~"),
        ("pagedown", Mods::None) => Some("\x1b[6~"),
        // Function keys
        ("f1", Mods::None) => Some("\x1bOP"),
        ("f2", Mods::None) => Some("\x1bOQ"),
        ("f3", Mods::None) => Some("\x1bOR"),
        ("f4", Mods::None) => Some("\x1bOS"),
        ("f5", Mods::None) => Some("\x1b[15~"),
        ("f6", Mods::None) => Some("\x1b[17~"),
        ("f7", Mods::None) => Some("\x1b[18~"),
        ("f8", Mods::None) => Some("\x1b[19~"),
        ("f9", Mods::None) => Some("\x1b[20~"),
        ("f10", Mods::None) => Some("\x1b[21~"),
        ("f11", Mods::None) => Some("\x1b[23~"),
        ("f12", Mods::None) => Some("\x1b[24~"),
        // Ctrl key codes (caret notation)
        ("a", Mods::Ctrl) => Some("\x01"),
        ("A", Mods::CtrlShift) => Some("\x01"),
        ("b", Mods::Ctrl) => Some("\x02"),
        ("B", Mods::CtrlShift) => Some("\x02"),
        ("c", Mods::Ctrl) => Some("\x03"),
        ("C", Mods::CtrlShift) => Some("\x03"),
        ("d", Mods::Ctrl) => Some("\x04"),
        ("D", Mods::CtrlShift) => Some("\x04"),
        ("e", Mods::Ctrl) => Some("\x05"),
        ("E", Mods::CtrlShift) => Some("\x05"),
        ("f", Mods::Ctrl) => Some("\x06"),
        ("F", Mods::CtrlShift) => Some("\x06"),
        ("g", Mods::Ctrl) => Some("\x07"),
        ("G", Mods::CtrlShift) => Some("\x07"),
        ("h", Mods::Ctrl) => Some("\x08"),
        ("H", Mods::CtrlShift) => Some("\x08"),
        ("i", Mods::Ctrl) => Some("\x09"),
        ("I", Mods::CtrlShift) => Some("\x09"),
        ("j", Mods::Ctrl) => Some("\x0a"),
        ("J", Mods::CtrlShift) => Some("\x0a"),
        ("k", Mods::Ctrl) => Some("\x0b"),
        ("K", Mods::CtrlShift) => Some("\x0b"),
        ("l", Mods::Ctrl) => Some("\x0c"),
        ("L", Mods::CtrlShift) => Some("\x0c"),
        ("m", Mods::Ctrl) => Some("\x0d"),
        ("M", Mods::CtrlShift) => Some("\x0d"),
        ("n", Mods::Ctrl) => Some("\x0e"),
        ("N", Mods::CtrlShift) => Some("\x0e"),
        ("o", Mods::Ctrl) => Some("\x0f"),
        ("O", Mods::CtrlShift) => Some("\x0f"),
        ("p", Mods::Ctrl) => Some("\x10"),
        ("P", Mods::CtrlShift) => Some("\x10"),
        ("q", Mods::Ctrl) => Some("\x11"),
        ("Q", Mods::CtrlShift) => Some("\x11"),
        ("r", Mods::Ctrl) => Some("\x12"),
        ("R", Mods::CtrlShift) => Some("\x12"),
        ("s", Mods::Ctrl) => Some("\x13"),
        ("S", Mods::CtrlShift) => Some("\x13"),
        ("t", Mods::Ctrl) => Some("\x14"),
        ("T", Mods::CtrlShift) => Some("\x14"),
        ("u", Mods::Ctrl) => Some("\x15"),
        ("U", Mods::CtrlShift) => Some("\x15"),
        ("v", Mods::Ctrl) => Some("\x16"),
        ("V", Mods::CtrlShift) => Some("\x16"),
        ("w", Mods::Ctrl) => Some("\x17"),
        ("W", Mods::CtrlShift) => Some("\x17"),
        ("x", Mods::Ctrl) => Some("\x18"),
        ("X", Mods::CtrlShift) => Some("\x18"),
        ("y", Mods::Ctrl) => Some("\x19"),
        ("Y", Mods::CtrlShift) => Some("\x19"),
        ("z", Mods::Ctrl) => Some("\x1a"),
        ("Z", Mods::CtrlShift) => Some("\x1a"),
        ("@", Mods::Ctrl) => Some("\x00"),
        ("[", Mods::Ctrl) => Some("\x1b"),
        ("\\", Mods::Ctrl) => Some("\x1c"),
        ("]", Mods::Ctrl) => Some("\x1d"),
        ("^", Mods::Ctrl) => Some("\x1e"),
        ("_", Mods::Ctrl) => Some("\x1f"),
        ("?", Mods::Ctrl) => Some("\x7f"),
        _ => None,
    };

    if let Some(s) = manual {
        return Some(Cow::Borrowed(s));
    }

    // Modifier-modified function/nav keys.
    if modifiers.any() {
        let mc = modifier_code(keystroke);
        let modified: Option<String> = match keystroke.key.as_ref() {
            "up" => Some(format!("\x1b[1;{}A", mc)),
            "down" => Some(format!("\x1b[1;{}B", mc)),
            "right" => Some(format!("\x1b[1;{}C", mc)),
            "left" => Some(format!("\x1b[1;{}D", mc)),
            "f1" => Some(format!("\x1b[1;{}P", mc)),
            "f2" => Some(format!("\x1b[1;{}Q", mc)),
            "f3" => Some(format!("\x1b[1;{}R", mc)),
            "f4" => Some(format!("\x1b[1;{}S", mc)),
            "f5" => Some(format!("\x1b[15;{}~", mc)),
            "f6" => Some(format!("\x1b[17;{}~", mc)),
            "f7" => Some(format!("\x1b[18;{}~", mc)),
            "f8" => Some(format!("\x1b[19;{}~", mc)),
            "f9" => Some(format!("\x1b[20;{}~", mc)),
            "f10" => Some(format!("\x1b[21;{}~", mc)),
            "f11" => Some(format!("\x1b[23;{}~", mc)),
            "f12" => Some(format!("\x1b[24;{}~", mc)),
            _ if mc == 2 => None, // Shift alone on non-special keys → fall through
            "insert" => Some(format!("\x1b[2;{}~", mc)),
            "pageup" => Some(format!("\x1b[5;{}~", mc)),
            "pagedown" => Some(format!("\x1b[6;{}~", mc)),
            "end" => Some(format!("\x1b[1;{}F", mc)),
            "home" => Some(format!("\x1b[1;{}H", mc)),
            _ => None,
        };
        if let Some(s) = modified {
            return Some(Cow::Owned(s));
        }
    }

    // Alt as meta (macOS option key).
    {
        let is_alt_lower = modifiers == Mods::Alt && keystroke.key.is_ascii();
        let is_alt_upper =
            keystroke.modifiers.alt && keystroke.modifiers.shift && keystroke.key.is_ascii();
        if is_alt_lower || is_alt_upper {
            let key = if is_alt_upper {
                &keystroke.key.to_ascii_uppercase()
            } else {
                &keystroke.key
            };
            return Some(Cow::Owned(format!("\x1b{}", key)));
        }
    }

    None
}

fn modifier_code(ks: &Keystroke) -> u32 {
    let mut code = 0u32;
    if ks.modifiers.shift {
        code |= 1;
    }
    if ks.modifiers.alt {
        code |= 1 << 1;
    }
    if ks.modifiers.control {
        code |= 1 << 2;
    }
    code + 1
}
