use gpui::KeyDownEvent;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ShortcutAction {
    CycleProjects,
    NewTabInCurrentTask,
    NewTask,
    CloseCurrentTab,
    NextTab,
    PreviousTab,
    NextTask,
    PreviousTask,
}

pub const ALL_SHORTCUT_ACTIONS: [ShortcutAction; 8] = [
    ShortcutAction::CycleProjects,
    ShortcutAction::NewTabInCurrentTask,
    ShortcutAction::NewTask,
    ShortcutAction::CloseCurrentTab,
    ShortcutAction::NextTab,
    ShortcutAction::PreviousTab,
    ShortcutAction::NextTask,
    ShortcutAction::PreviousTask,
];

impl ShortcutAction {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::CycleProjects => "Cycle Projects",
            Self::NewTabInCurrentTask => "New Tab in Current Task",
            Self::NewTask => "New Task",
            Self::CloseCurrentTab => "Close Current Tab",
            Self::NextTab => "Next Tab",
            Self::PreviousTab => "Previous Tab",
            Self::NextTask => "Next Task",
            Self::PreviousTask => "Previous Task",
        }
    }

    pub(crate) fn default_binding(self) -> &'static str {
        match self {
            Self::CycleProjects => "cmd-o",
            Self::NewTabInCurrentTask => "cmd-n",
            Self::NewTask => "cmd-t",
            Self::CloseCurrentTab => close_current_tab_default_binding(),
            Self::NextTab => "cmd-shift-]",
            Self::PreviousTab => "cmd-shift-[",
            Self::NextTask => "cmd-alt-down",
            Self::PreviousTask => "cmd-alt-up",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShortcutSettings {
    #[serde(default = "default_cycle_projects_shortcut")]
    pub cycle_projects: String,
    #[serde(default = "default_new_tab_in_current_task_shortcut")]
    pub new_tab_in_current_task: String,
    #[serde(default = "default_new_task_shortcut")]
    pub new_task: String,
    #[serde(default = "default_close_current_tab_shortcut")]
    pub close_current_tab: String,
    #[serde(default = "default_next_tab_shortcut")]
    pub next_tab: String,
    #[serde(default = "default_previous_tab_shortcut")]
    pub previous_tab: String,
    #[serde(default = "default_next_task_shortcut")]
    pub next_task: String,
    #[serde(default = "default_previous_task_shortcut")]
    pub previous_task: String,
}

impl Default for ShortcutSettings {
    fn default() -> Self {
        Self {
            cycle_projects: default_cycle_projects_shortcut(),
            new_tab_in_current_task: default_new_tab_in_current_task_shortcut(),
            new_task: default_new_task_shortcut(),
            close_current_tab: default_close_current_tab_shortcut(),
            next_tab: default_next_tab_shortcut(),
            previous_tab: default_previous_tab_shortcut(),
            next_task: default_next_task_shortcut(),
            previous_task: default_previous_task_shortcut(),
        }
    }
}

impl ShortcutSettings {
    pub(crate) fn binding_for(&self, action: ShortcutAction) -> &str {
        match action {
            ShortcutAction::CycleProjects => &self.cycle_projects,
            ShortcutAction::NewTabInCurrentTask => &self.new_tab_in_current_task,
            ShortcutAction::NewTask => &self.new_task,
            ShortcutAction::CloseCurrentTab => &self.close_current_tab,
            ShortcutAction::NextTab => &self.next_tab,
            ShortcutAction::PreviousTab => &self.previous_tab,
            ShortcutAction::NextTask => &self.next_task,
            ShortcutAction::PreviousTask => &self.previous_task,
        }
    }

    pub(crate) fn set_binding(&mut self, action: ShortcutAction, binding: impl Into<String>) {
        let binding = binding.into();
        match action {
            ShortcutAction::CycleProjects => self.cycle_projects = binding,
            ShortcutAction::NewTabInCurrentTask => self.new_tab_in_current_task = binding,
            ShortcutAction::NewTask => self.new_task = binding,
            ShortcutAction::CloseCurrentTab => self.close_current_tab = binding,
            ShortcutAction::NextTab => self.next_tab = binding,
            ShortcutAction::PreviousTab => self.previous_tab = binding,
            ShortcutAction::NextTask => self.next_task = binding,
            ShortcutAction::PreviousTask => self.previous_task = binding,
        }
    }

    pub(crate) fn clear_binding(&mut self, action: ShortcutAction) {
        self.set_binding(action, String::new());
    }

    pub(crate) fn reset_binding(&mut self, action: ShortcutAction) {
        self.set_binding(action, action.default_binding().to_string());
    }

    pub(crate) fn reset_all(&mut self) {
        *self = Self::default();
    }

    pub(crate) fn action_for_binding(
        &self,
        ignored_action: ShortcutAction,
        binding: &str,
    ) -> Option<ShortcutAction> {
        ALL_SHORTCUT_ACTIONS.into_iter().find(|action| {
            *action != ignored_action
                && !self.binding_for(*action).is_empty()
                && self.binding_for(*action) == binding
        })
    }
}

fn default_cycle_projects_shortcut() -> String {
    ShortcutAction::CycleProjects.default_binding().to_string()
}

fn default_new_tab_in_current_task_shortcut() -> String {
    ShortcutAction::NewTabInCurrentTask
        .default_binding()
        .to_string()
}

fn default_new_task_shortcut() -> String {
    ShortcutAction::NewTask.default_binding().to_string()
}

fn default_close_current_tab_shortcut() -> String {
    ShortcutAction::CloseCurrentTab
        .default_binding()
        .to_string()
}

fn default_next_tab_shortcut() -> String {
    ShortcutAction::NextTab.default_binding().to_string()
}

fn default_previous_tab_shortcut() -> String {
    ShortcutAction::PreviousTab.default_binding().to_string()
}

fn default_next_task_shortcut() -> String {
    ShortcutAction::NextTask.default_binding().to_string()
}

fn default_previous_task_shortcut() -> String {
    ShortcutAction::PreviousTask.default_binding().to_string()
}

#[cfg(target_os = "macos")]
fn close_current_tab_default_binding() -> &'static str {
    "cmd-w"
}

#[cfg(not(target_os = "macos"))]
fn close_current_tab_default_binding() -> &'static str {
    "control-w"
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ShortcutBinding {
    modifiers: ShortcutModifiers,
    key: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
struct ShortcutModifiers {
    platform: bool,
    shift: bool,
    alt: bool,
    control: bool,
    function: bool,
}

impl ShortcutBinding {
    fn parse(value: &str) -> Result<Self, String> {
        let normalized = value.trim().to_ascii_lowercase();
        if normalized.is_empty() {
            return Err("Shortcut is empty.".to_string());
        }

        let mut rest = normalized.as_str();
        let mut modifiers = ShortcutModifiers::default();
        while let Some((name, next)) = consume_modifier_prefix(rest) {
            let already_set = match name {
                "cmd" => &mut modifiers.platform,
                "shift" => &mut modifiers.shift,
                "alt" => &mut modifiers.alt,
                "control" => &mut modifiers.control,
                "function" => &mut modifiers.function,
                _ => unreachable!(),
            };

            if *already_set {
                return Err(format!("Duplicate {name} modifier."));
            }
            *already_set = true;
            rest = next;
        }

        if rest.is_empty() {
            return Err("Shortcut needs a non-modifier key.".to_string());
        }

        let (key, implied_shift) =
            normalize_key_token(rest).ok_or_else(|| format!("Unsupported key: {rest}"))?;
        modifiers.shift |= implied_shift;

        Ok(Self { modifiers, key })
    }

    fn from_event(ev: &KeyDownEvent) -> Result<Self, &'static str> {
        let modifiers = ev.keystroke.modifiers;
        if !modifiers.platform && !modifiers.alt && !modifiers.control && !modifiers.function {
            return Err("Use at least one modifier key.");
        }

        let key_name = ev.keystroke.key.as_str();
        if is_modifier_key(key_name) {
            return Err("Press a non-modifier key.");
        }

        let (key, implied_shift) = normalize_event_key(key_name, ev.keystroke.key_char.as_deref())
            .ok_or("That key is not supported for shortcuts.")?;

        Ok(Self {
            modifiers: ShortcutModifiers {
                platform: modifiers.platform,
                shift: modifiers.shift || implied_shift,
                alt: modifiers.alt,
                control: modifiers.control,
                function: modifiers.function,
            },
            key,
        })
    }

    fn to_canonical_string(&self) -> String {
        let mut parts = Vec::with_capacity(6);
        if self.modifiers.platform {
            parts.push("cmd");
        }
        if self.modifiers.control {
            parts.push("control");
        }
        if self.modifiers.alt {
            parts.push("alt");
        }
        if self.modifiers.shift {
            parts.push("shift");
        }
        if self.modifiers.function {
            parts.push("function");
        }
        parts.push(self.key.as_str());
        parts.join("-")
    }
}

pub(crate) fn shortcut_matches_event(shortcut: &str, ev: &KeyDownEvent) -> bool {
    if shortcut.trim().is_empty() {
        return false;
    }

    let Ok(binding) = ShortcutBinding::parse(shortcut) else {
        return false;
    };
    let Ok(candidate) = ShortcutBinding::from_event(ev) else {
        return false;
    };
    binding == candidate
}

pub(crate) fn capture_shortcut(ev: &KeyDownEvent) -> Result<String, &'static str> {
    ShortcutBinding::from_event(ev).map(|binding| binding.to_canonical_string())
}

pub(crate) fn keybinding_token_label(token: &str) -> String {
    match token {
        "cmd" => platform_modifier_label().to_string(),
        "control" => "Ctrl".to_string(),
        "alt" => "Alt".to_string(),
        "shift" => "Shift".to_string(),
        "function" => "Fn".to_string(),
        "up" => "Up".to_string(),
        "down" => "Down".to_string(),
        "left" => "Left".to_string(),
        "right" => "Right".to_string(),
        "pageup" => "Page Up".to_string(),
        "pagedown" => "Page Down".to_string(),
        "escape" => "Esc".to_string(),
        "enter" => "Enter".to_string(),
        "tab" => "Tab".to_string(),
        "space" => "Space".to_string(),
        "backspace" => "Backspace".to_string(),
        "delete" => "Delete".to_string(),
        "home" => "Home".to_string(),
        "end" => "End".to_string(),
        "minus" => "-".to_string(),
        _ if token.len() == 1 => token.to_ascii_uppercase(),
        _ => token.to_string(),
    }
}

#[cfg(target_os = "macos")]
fn platform_modifier_label() -> &'static str {
    "Cmd"
}

#[cfg(not(target_os = "macos"))]
fn platform_modifier_label() -> &'static str {
    "Super"
}

fn consume_modifier_prefix(value: &str) -> Option<(&'static str, &str)> {
    const MODIFIER_PREFIXES: [(&str, &str); 9] = [
        ("platform-", "cmd"),
        ("cmd-", "cmd"),
        ("meta-", "cmd"),
        ("super-", "cmd"),
        ("shift-", "shift"),
        ("option-", "alt"),
        ("alt-", "alt"),
        ("ctrl-", "control"),
        ("control-", "control"),
    ];

    for (prefix, name) in MODIFIER_PREFIXES {
        if let Some(rest) = value.strip_prefix(prefix) {
            return Some((name, rest));
        }
    }

    if let Some(rest) = value.strip_prefix("fn-") {
        return Some(("function", rest));
    }
    if let Some(rest) = value.strip_prefix("function-") {
        return Some(("function", rest));
    }

    None
}

fn normalize_event_key(key: &str, key_char: Option<&str>) -> Option<(String, bool)> {
    normalize_key_token(key).or_else(|| key_char.and_then(normalize_key_token))
}

fn normalize_key_token(token: &str) -> Option<(String, bool)> {
    if token.is_empty() {
        return None;
    }

    match token {
        "up" | "down" | "left" | "right" | "home" | "end" | "pageup" | "pagedown" | "tab"
        | "enter" | "escape" | "backspace" | "delete" | "space" => {
            return Some((token.to_string(), false));
        }
        "minus" => return Some(("minus".to_string(), false)),
        _ => {}
    }

    let mut chars = token.chars();
    let ch = chars.next()?;
    if chars.next().is_some() {
        return None;
    }

    if ch.is_ascii_alphanumeric() {
        return Some((ch.to_ascii_lowercase().to_string(), false));
    }

    match ch {
        '-' => Some(("minus".to_string(), false)),
        '=' | '[' | ']' | '\\' | ';' | '\'' | ',' | '.' | '/' | '`' => {
            Some((ch.to_string(), false))
        }
        '_' => Some(("minus".to_string(), true)),
        '+' => Some(("=".to_string(), true)),
        '{' => Some(("[".to_string(), true)),
        '}' => Some(("]".to_string(), true)),
        '|' => Some(("\\".to_string(), true)),
        ':' => Some((";".to_string(), true)),
        '"' => Some(("'".to_string(), true)),
        '<' => Some((",".to_string(), true)),
        '>' => Some((".".to_string(), true)),
        '?' => Some(("/".to_string(), true)),
        '~' => Some(("`".to_string(), true)),
        '!' => Some(("1".to_string(), true)),
        '@' => Some(("2".to_string(), true)),
        '#' => Some(("3".to_string(), true)),
        '$' => Some(("4".to_string(), true)),
        '%' => Some(("5".to_string(), true)),
        '^' => Some(("6".to_string(), true)),
        '&' => Some(("7".to_string(), true)),
        '*' => Some(("8".to_string(), true)),
        '(' => Some(("9".to_string(), true)),
        ')' => Some(("0".to_string(), true)),
        _ => None,
    }
}

fn is_modifier_key(key: &str) -> bool {
    matches!(
        key,
        "shift"
            | "alt"
            | "option"
            | "control"
            | "ctrl"
            | "cmd"
            | "meta"
            | "super"
            | "platform"
            | "function"
            | "fn"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use gpui::{KeyDownEvent, Keystroke, Modifiers};

    fn key_event(key: &str, key_char: Option<&str>, modifiers: Modifiers) -> KeyDownEvent {
        KeyDownEvent {
            keystroke: Keystroke {
                modifiers,
                key: key.to_string(),
                key_char: key_char.map(str::to_string),
            },
            is_held: false,
        }
    }

    #[test]
    fn parses_shifted_symbol_shortcuts_into_base_keys() {
        let binding = ShortcutBinding::parse("cmd-}").expect("shortcut should parse");
        assert_eq!(binding.to_canonical_string(), "cmd-shift-]");
    }

    #[test]
    fn matches_tab_navigation_shortcuts_across_shifted_bracket_variants() {
        let shifted_modifiers = Modifiers {
            platform: true,
            shift: true,
            ..Modifiers::default()
        };
        let unshifted_modifiers = Modifiers {
            platform: true,
            ..Modifiers::default()
        };

        assert!(shortcut_matches_event(
            "cmd-shift-]",
            &key_event("]", None, shifted_modifiers)
        ));
        assert!(shortcut_matches_event(
            "cmd-shift-]",
            &key_event("]", Some("}"), shifted_modifiers)
        ));
        assert!(shortcut_matches_event(
            "cmd-shift-]",
            &key_event("}", None, unshifted_modifiers)
        ));
    }

    #[test]
    fn capture_normalizes_minus_key_shortcuts() {
        let captured = capture_shortcut(&key_event(
            "-",
            Some("-"),
            Modifiers {
                platform: true,
                ..Modifiers::default()
            },
        ))
        .expect("shortcut should capture");

        assert_eq!(captured, "cmd-minus");
    }

    #[test]
    fn capture_requires_non_shift_modifier() {
        let error = capture_shortcut(&key_event(
            "t",
            Some("t"),
            Modifiers {
                shift: true,
                ..Modifiers::default()
            },
        ))
        .expect_err("shortcut should be rejected");

        assert_eq!(error, "Use at least one modifier key.");
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn close_current_tab_default_is_command_w_on_macos() {
        assert_eq!(ShortcutAction::CloseCurrentTab.default_binding(), "cmd-w");
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn close_current_tab_default_is_control_w_off_macos() {
        assert_eq!(
            ShortcutAction::CloseCurrentTab.default_binding(),
            "control-w"
        );
    }
}
