//! App-level settings page with a sidebar navigation and content area.

use gpui::{
    div, hsla, point, prelude::*, px, rems, rgb, size, svg, AnyElement, App, Bounds, ClipboardItem,
    Context, Element, ElementId, Entity, GlobalElementId, InspectorElementId, KeyDownEvent,
    LayoutId, MouseButton, MouseDownEvent, Pixels, ShapedLine, TextRun, Window,
};

use crate::agent_icons::branded_icon;
use crate::agents::AGENTS;
use crate::app::AnotherOneApp;
use crate::git_actions::{default_commit_generation_script, default_pr_generation_script};
use crate::layout::TITLEBAR_CHROME_H;
use crate::shortcuts::{
    capture_shortcut, keybinding_token_label, ShortcutAction, ALL_SHORTCUT_ACTIONS,
};
use crate::text_edit::{CursorDirection, TextEditState};

const TEXT_PRIMARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.92, 1.);
const TEXT_SECONDARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.55, 1.);
const BORDER_SUBTLE: fn() -> gpui::Hsla = || gpui::white().opacity(0.08);

const SETTINGS_SIDEBAR_W: f32 = 180.;

/// Which settings section is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    General,
    Agents,
    OpenIn,
    GitActions,
    Keybindings,
    Mcp,
}

impl SettingsSection {
    fn label(self) -> &'static str {
        match self {
            Self::General => "General",
            Self::Agents => "Agents",
            Self::OpenIn => "Open In",
            Self::GitActions => "Git Actions",
            Self::Keybindings => "Keybindings",
            Self::Mcp => "MCP",
        }
    }
}

impl AnotherOneApp {
    pub(crate) fn handle_settings_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings_section == SettingsSection::Agents
            && self.handle_settings_agent_input_key_down(ev, cx)
        {
            return true;
        }

        if self.settings_section == SettingsSection::GitActions
            && self.handle_settings_git_action_script_key_down(ev, cx)
        {
            return true;
        }

        if self.settings_section != SettingsSection::Keybindings {
            return false;
        }

        let Some(action) = self.shortcut_capture_action else {
            return false;
        };

        cx.stop_propagation();

        match ev.keystroke.key.as_str() {
            "escape" => {
                self.shortcut_capture_action = None;
                cx.notify();
                return true;
            }
            "backspace" | "delete"
                if !ev.keystroke.modifiers.platform
                    && !ev.keystroke.modifiers.alt
                    && !ev.keystroke.modifiers.control
                    && !ev.keystroke.modifiers.function =>
            {
                self.project_store.clear_shortcut_binding(action);
                self.shortcut_capture_action = None;
                self.show_success_toast(format!("Cleared {}.", action.label()), cx);
                cx.notify();
                return true;
            }
            _ => {}
        }

        match capture_shortcut(ev) {
            Ok(binding) => {
                if let Some(conflict) = self
                    .project_store
                    .ui
                    .shortcuts
                    .action_for_binding(action, &binding)
                {
                    self.show_error_toast(
                        format!("{} already uses that shortcut.", conflict.label()),
                        cx,
                    );
                    return true;
                }

                self.project_store.set_shortcut_binding(action, binding);
                self.shortcut_capture_action = None;
                self.show_success_toast(format!("Updated {}.", action.label()), cx);
                cx.notify();
                true
            }
            Err(message) => {
                self.show_warning_toast(message, cx);
                true
            }
        }
    }

    fn begin_shortcut_capture(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        self.shortcut_capture_action = Some(action);
        cx.notify();
    }

    fn clear_shortcut_binding(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        self.project_store.clear_shortcut_binding(action);
        if self.shortcut_capture_action == Some(action) {
            self.shortcut_capture_action = None;
        }
        self.show_success_toast(format!("Cleared {}.", action.label()), cx);
        cx.notify();
    }

    fn reset_shortcut_binding(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        self.project_store.reset_shortcut_binding(action);
        if self.shortcut_capture_action == Some(action) {
            self.shortcut_capture_action = None;
        }
        self.show_success_toast(format!("Reset {}.", action.label()), cx);
        cx.notify();
    }

    fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.project_store.reset_shortcuts();
        self.shortcut_capture_action = None;
        self.show_success_toast("Reset all shortcuts.", cx);
        cx.notify();
    }

    pub(crate) fn focus_settings_agent_input(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        let draft = self
            .settings_agent_input
            .drafts
            .entry(agent_id.to_string())
            .or_default();
        self.settings_agent_input.focused_agent_id = Some(agent_id.to_string());
        self.settings_agent_input.cursor = draft.len();
        self.settings_agent_input.selection_anchor = None;
        self.shortcut_capture_action = None;
        cx.notify();
    }

    fn blur_settings_agent_input(&mut self, cx: &mut Context<Self>) {
        if self.settings_agent_input.focused_agent_id.take().is_none() {
            return;
        }
        self.settings_agent_input.selection_anchor = None;
        cx.notify();
    }

    pub(crate) fn sync_settings_git_action_script_from_store(
        &mut self,
        kind: crate::app::SettingsGitActionScriptKind,
    ) {
        let draft = match kind {
            crate::app::SettingsGitActionScriptKind::Commit => self
                .project_store
                .git_commit_generation_script()
                .to_string(),
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                self.project_store.git_pr_generation_script().to_string()
            }
        };
        let input = self.settings_git_action_script_input_mut(kind);
        input.draft = draft;
        input.cursor = input.cursor.min(input.draft.len());
        if let Some(anchor) = input.selection_anchor.as_mut() {
            *anchor = (*anchor).min(input.draft.len());
        }
    }

    fn focus_settings_git_action_script_input(
        &mut self,
        kind: crate::app::SettingsGitActionScriptKind,
        cx: &mut Context<Self>,
    ) {
        self.sync_settings_git_action_script_from_store(kind);
        self.settings_git_action_script_input_mut(match kind {
            crate::app::SettingsGitActionScriptKind::Commit => {
                crate::app::SettingsGitActionScriptKind::PullRequest
            }
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                crate::app::SettingsGitActionScriptKind::Commit
            }
        })
        .focused = false;
        let input = self.settings_git_action_script_input_mut(kind);
        if !input.focused {
            input.cursor = input.draft.len();
            input.selection_anchor = None;
        }
        input.focused = true;
        self.shortcut_capture_action = None;
        cx.notify();
    }

    fn blur_settings_git_action_script_input(
        &mut self,
        kind: crate::app::SettingsGitActionScriptKind,
        cx: &mut Context<Self>,
    ) {
        let input = self.settings_git_action_script_input_mut(kind);
        if !input.focused {
            return;
        }

        input.focused = false;
        input.selection_anchor = None;
        match kind {
            crate::app::SettingsGitActionScriptKind::Commit => {
                self.settings_git_commit_script_drag_anchor = None;
            }
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                self.settings_git_pr_script_drag_anchor = None;
            }
        }
        cx.notify();
    }

    fn reset_git_action_script_to_default(
        &mut self,
        kind: crate::app::SettingsGitActionScriptKind,
        cx: &mut Context<Self>,
    ) {
        let (draft, message) = match kind {
            crate::app::SettingsGitActionScriptKind::Commit => {
                let _ = self.project_store.reset_git_commit_generation_script();
                (
                    default_commit_generation_script().to_string(),
                    "Reset the git commit instructions to the default template.",
                )
            }
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                let _ = self.project_store.reset_git_pr_generation_script();
                (
                    default_pr_generation_script().to_string(),
                    "Reset the PR title/body instructions to the default template.",
                )
            }
        };
        let input = self.settings_git_action_script_input_mut(kind);
        input.draft = draft;
        input.cursor = input.draft.len();
        input.selection_anchor = None;
        match kind {
            crate::app::SettingsGitActionScriptKind::Commit => {
                self.settings_git_commit_script_drag_anchor = None;
            }
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                self.settings_git_pr_script_drag_anchor = None;
            }
        }
        self.show_success_toast(message, cx);
        cx.notify();
    }

    pub(crate) fn settings_git_action_script_index_for_point(
        &self,
        kind: crate::app::SettingsGitActionScriptKind,
        point: gpui::Point<Pixels>,
    ) -> usize {
        let input = self.settings_git_action_script_input(kind);
        if input.draft.is_empty() {
            return 0;
        }

        let layout = self.settings_git_action_script_layout(kind);
        let Some(first_line) = layout.first() else {
            return input.cursor;
        };
        let Some(last_line) = layout.last() else {
            return input.cursor;
        };

        if point.y <= first_line.bounds.top() {
            return 0;
        }
        if point.y >= last_line.bounds.bottom() {
            return input.draft.len();
        }

        let closest_line = layout
            .iter()
            .min_by(|left, right| {
                let left_distance = distance_to_vertical_bounds(point.y, left.bounds);
                let right_distance = distance_to_vertical_bounds(point.y, right.bounds);
                left_distance
                    .partial_cmp(&right_distance)
                    .unwrap_or(std::cmp::Ordering::Equal)
            })
            .unwrap_or(first_line);
        let x = point.x - closest_line.bounds.left();
        let offset = closest_line.line.closest_index_for_x(x);
        closest_line.range.start + offset.min(closest_line.range.len())
    }

    fn begin_settings_git_action_script_selection(
        &mut self,
        kind: crate::app::SettingsGitActionScriptKind,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_handle.focus(window, cx);
        self.focus_settings_git_action_script_input(kind, cx);

        let input = self.settings_git_action_script_input(kind);
        let selection_anchor = if ev.modifiers.shift {
            Some(input.selection_anchor.unwrap_or(input.cursor))
        } else {
            None
        };
        let cursor = self.settings_git_action_script_index_for_point(kind, ev.position);

        let input = self.settings_git_action_script_input_mut(kind);
        input.cursor = cursor;
        input.selection_anchor = selection_anchor.filter(|anchor| *anchor != cursor);
        match kind {
            crate::app::SettingsGitActionScriptKind::Commit => {
                self.settings_git_commit_script_drag_anchor =
                    Some(selection_anchor.unwrap_or(cursor))
            }
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                self.settings_git_pr_script_drag_anchor = Some(selection_anchor.unwrap_or(cursor))
            }
        }
        cx.notify();
    }

    pub(crate) fn update_settings_git_action_script_selection_drag(
        &mut self,
        ev: &gpui::MouseMoveEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if !ev.dragging() {
            return false;
        }

        for kind in [
            crate::app::SettingsGitActionScriptKind::Commit,
            crate::app::SettingsGitActionScriptKind::PullRequest,
        ] {
            let anchor = match kind {
                crate::app::SettingsGitActionScriptKind::Commit => {
                    self.settings_git_commit_script_drag_anchor
                }
                crate::app::SettingsGitActionScriptKind::PullRequest => {
                    self.settings_git_pr_script_drag_anchor
                }
            };
            let Some(anchor) = anchor else {
                continue;
            };

            let cursor = self.settings_git_action_script_index_for_point(kind, ev.position);
            let input = self.settings_git_action_script_input_mut(kind);
            input.cursor = cursor;
            input.selection_anchor = (anchor != cursor).then_some(anchor);
            cx.notify();
            return true;
        }

        false
    }

    pub(crate) fn finish_settings_git_action_script_selection_drag(&mut self) -> bool {
        let mut had_drag = false;
        for kind in [
            crate::app::SettingsGitActionScriptKind::Commit,
            crate::app::SettingsGitActionScriptKind::PullRequest,
        ] {
            let had_kind_drag = match kind {
                crate::app::SettingsGitActionScriptKind::Commit => {
                    self.settings_git_commit_script_drag_anchor.take().is_some()
                }
                crate::app::SettingsGitActionScriptKind::PullRequest => {
                    self.settings_git_pr_script_drag_anchor.take().is_some()
                }
            };
            had_drag |= had_kind_drag;
            let input = self.settings_git_action_script_input_mut(kind);
            if input.selection_anchor == Some(input.cursor) {
                input.selection_anchor = None;
            }
        }
        had_drag
    }

    fn add_agent_launch_arg(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        let Some(agent) = AGENTS.iter().find(|agent| agent.id == agent_id) else {
            return;
        };
        let draft = self
            .settings_agent_input
            .drafts
            .get(agent_id)
            .cloned()
            .unwrap_or_default();
        let token = match validate_agent_launch_arg(&draft) {
            Ok(token) => token,
            Err(message) => {
                self.show_error_toast(message, cx);
                return;
            }
        };

        let mut args = self.project_store.agent_launch_args(agent_id).to_vec();
        args.push(token.clone());
        self.project_store.set_agent_launch_args(agent_id, args);
        self.settings_agent_input
            .drafts
            .insert(agent_id.to_string(), String::new());
        self.settings_agent_input.focused_agent_id = Some(agent_id.to_string());
        self.settings_agent_input.cursor = 0;
        self.settings_agent_input.selection_anchor = None;
        self.show_success_toast(format!("Added {} arg for {}.", token, agent.label), cx);
        cx.notify();
    }

    fn remove_agent_launch_arg(&mut self, agent_id: &str, index: usize, cx: &mut Context<Self>) {
        let Some(agent) = AGENTS.iter().find(|agent| agent.id == agent_id) else {
            return;
        };
        let mut args = self.project_store.agent_launch_args(agent_id).to_vec();
        if index >= args.len() {
            return;
        }
        let removed = args.remove(index);
        self.project_store.set_agent_launch_args(agent_id, args);
        self.show_success_toast(format!("Removed {} arg from {}.", removed, agent.label), cx);
        cx.notify();
    }

    fn handle_settings_agent_input_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(agent_id) = self.settings_agent_input.focused_agent_id.clone() else {
            return false;
        };

        cx.stop_propagation();

        let modifiers = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "backspace" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                if modifiers.platform {
                    delete_settings_input_to_start(
                        draft,
                        &mut self.settings_agent_input.cursor,
                        &mut self.settings_agent_input.selection_anchor,
                    );
                } else if modifiers.alt {
                    delete_settings_input_word_backward(
                        draft,
                        &mut self.settings_agent_input.cursor,
                        &mut self.settings_agent_input.selection_anchor,
                    );
                } else {
                    delete_settings_input_backward(
                        draft,
                        &mut self.settings_agent_input.cursor,
                        &mut self.settings_agent_input.selection_anchor,
                    );
                }
                cx.notify();
                return true;
            }
            "delete" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                delete_settings_input_forward(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                );
                cx.notify();
                return true;
            }
            "left" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                move_settings_input_cursor(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    CursorDirection::Left,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "right" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                move_settings_input_cursor(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    CursorDirection::Right,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "home" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                move_settings_input_cursor_to_edge(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    false,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "end" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                move_settings_input_cursor_to_edge(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    true,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "enter" => {
                self.add_agent_launch_arg(&agent_id, cx);
                return true;
            }
            "escape" | "tab" => {
                self.blur_settings_agent_input(cx);
                return true;
            }
            _ => {}
        }

        let draft = self
            .settings_agent_input
            .drafts
            .entry(agent_id)
            .or_default();

        if modifiers.platform && ev.keystroke.key.as_str() == "a" {
            let mut state = TextEditState::new(
                self.settings_agent_input.cursor,
                self.settings_agent_input.selection_anchor,
            );
            state.select_all(draft);
            self.settings_agent_input.cursor = state.cursor;
            self.settings_agent_input.selection_anchor = state.selection_anchor;
            cx.notify();
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "c" {
            if let Some(range) = settings_agent_input_selected_range(
                self.settings_agent_input.cursor,
                self.settings_agent_input.selection_anchor,
            ) {
                cx.write_to_clipboard(ClipboardItem::new_string(draft[range].to_string()));
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "x" {
            if let Some(range) = settings_agent_input_selected_range(
                self.settings_agent_input.cursor,
                self.settings_agent_input.selection_anchor,
            ) {
                cx.write_to_clipboard(ClipboardItem::new_string(draft[range.clone()].to_string()));
                replace_settings_input_range(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    range,
                    "",
                );
                cx.notify();
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                insert_settings_input_text(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    &text,
                );
                cx.notify();
            }
            return true;
        }

        if modifiers.control || modifiers.platform || modifiers.function {
            return true;
        }

        if let Some(key_char) = ev.keystroke.key_char.as_deref() {
            insert_settings_input_text(
                draft,
                &mut self.settings_agent_input.cursor,
                &mut self.settings_agent_input.selection_anchor,
                key_char,
            );
            cx.notify();
        }

        true
    }

    fn handle_settings_git_action_script_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(kind) = self.focused_settings_git_action_script_kind() else {
            return false;
        };

        cx.stop_propagation();

        let modifiers = ev.keystroke.modifiers;
        let input = self.settings_git_action_script_input_mut(kind);
        let draft = &mut input.draft;
        match ev.keystroke.key.as_str() {
            "backspace" => {
                if modifiers.platform {
                    delete_settings_input_to_start(
                        draft,
                        &mut input.cursor,
                        &mut input.selection_anchor,
                    );
                } else if modifiers.alt {
                    delete_settings_input_word_backward(
                        draft,
                        &mut input.cursor,
                        &mut input.selection_anchor,
                    );
                } else {
                    delete_settings_input_backward(
                        draft,
                        &mut input.cursor,
                        &mut input.selection_anchor,
                    );
                }
            }
            "delete" => {
                delete_settings_input_forward(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                );
            }
            "left" => {
                move_settings_input_cursor(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                    CursorDirection::Left,
                    modifiers.shift,
                );
            }
            "right" => {
                move_settings_input_cursor(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                    CursorDirection::Right,
                    modifiers.shift,
                );
            }
            "up" => {
                move_settings_multiline_cursor_vertical(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                    false,
                    modifiers.shift,
                );
            }
            "down" => {
                move_settings_multiline_cursor_vertical(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                    true,
                    modifiers.shift,
                );
            }
            "home" => {
                move_settings_multiline_cursor_to_line_edge(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                    false,
                    modifiers.shift,
                );
            }
            "end" => {
                move_settings_multiline_cursor_to_line_edge(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                    true,
                    modifiers.shift,
                );
            }
            "enter" => {
                insert_settings_input_text(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                    "\n",
                );
            }
            "tab" => {
                insert_settings_input_text(
                    draft,
                    &mut input.cursor,
                    &mut input.selection_anchor,
                    "    ",
                );
            }
            "escape" => {
                self.blur_settings_git_action_script_input(kind, cx);
                return true;
            }
            _ => {
                if modifiers.platform && ev.keystroke.key.as_str() == "a" {
                    let mut state = TextEditState::new(input.cursor, input.selection_anchor);
                    state.select_all(draft);
                    input.cursor = state.cursor;
                    input.selection_anchor = state.selection_anchor;
                } else if modifiers.platform && ev.keystroke.key.as_str() == "c" {
                    if let Some(range) =
                        settings_agent_input_selected_range(input.cursor, input.selection_anchor)
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(draft[range].to_string()));
                    }
                    return true;
                } else if modifiers.platform && ev.keystroke.key.as_str() == "x" {
                    if let Some(range) =
                        settings_agent_input_selected_range(input.cursor, input.selection_anchor)
                    {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            draft[range.clone()].to_string(),
                        ));
                        replace_settings_input_range(
                            draft,
                            &mut input.cursor,
                            &mut input.selection_anchor,
                            range,
                            "",
                        );
                    }
                } else if modifiers.platform && ev.keystroke.key.as_str() == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        insert_settings_input_text(
                            draft,
                            &mut input.cursor,
                            &mut input.selection_anchor,
                            &text,
                        );
                    }
                } else if !(modifiers.control || modifiers.platform || modifiers.function) {
                    if let Some(key_char) = ev.keystroke.key_char.as_deref() {
                        insert_settings_input_text(
                            draft,
                            &mut input.cursor,
                            &mut input.selection_anchor,
                            key_char,
                        );
                    }
                }
            }
        }

        let saved_draft = input.draft.clone();
        match kind {
            crate::app::SettingsGitActionScriptKind::Commit => {
                let _ = self
                    .project_store
                    .set_git_commit_generation_script(saved_draft);
            }
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                let _ = self.project_store.set_git_pr_generation_script(saved_draft);
            }
        }
        cx.notify();
        true
    }

    /// Render the full-window settings page (sidebar + content).
    ///
    /// Wide: 180px left nav + scrollable content (the existing layout).
    /// Narrow: a back chevron + horizontal scrollable section pills on top,
    /// then the same scrollable content below. Same `settings_nav_item`
    /// builder is reused — the items are horizontally arranged instead of
    /// vertically; no per-item code changes.
    pub(crate) fn render_settings_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        if self.is_narrow(window) {
            return self.render_settings_page_narrow(window, cx);
        }
        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(rgb(0x1e1f22))
            .child(self.settings_sidebar(window, cx))
            .child(
                div()
                    .id("settings-page-scroll")
                    .flex_1()
                    .min_w(px(0.))
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.settings_content(cx)),
            )
    }

    fn render_settings_page_narrow(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let bg = crate::theme::chrome_bg(window);
        let active = self.settings_section;
        let section_active_bg = hsla(215. / 360., 0.60, 0.45, 1.);
        let back_hover = gpui::white().opacity(0.06);
        let back_text = hsla(0., 0., 0.55, 1.);
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1f22))
            .child(
                // Top bar: back chevron + horizontal section pills.
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .flex_shrink_0()
                    .h(px(crate::layout::PHONE_HEADER_H))
                    .bg(bg)
                    .child(
                        div()
                            .id("settings-back-btn-narrow")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(44.))
                            .h(px(crate::layout::PHONE_HEADER_H))
                            .cursor_pointer()
                            .hover(move |s| s.bg(back_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.settings_open = false;
                                    this.shortcut_capture_action = None;
                                    cx.notify();
                                }),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__chevron-left.svg")
                                    .size(px(20.))
                                    .text_color(back_text),
                            ),
                    )
                    .child(
                        // Horizontally-scrollable section pills. Reuses the
                        // same `settings_nav_item` builder as the wide
                        // sidebar — items are sized off their text with the
                        // existing `mx(8) px(10) h(30)` styling, which
                        // already works in a row.
                        div()
                            .id("settings-nav-strip")
                            .flex_1()
                            .min_w_0()
                            .overflow_x_scroll()
                            .flex()
                            .flex_row()
                            .items_center()
                            .child(self.settings_nav_item(
                                SettingsSection::General,
                                active,
                                section_active_bg,
                                cx,
                            ))
                            .child(self.settings_nav_item(
                                SettingsSection::Agents,
                                active,
                                section_active_bg,
                                cx,
                            ))
                            .child(self.settings_nav_item(
                                SettingsSection::OpenIn,
                                active,
                                section_active_bg,
                                cx,
                            ))
                            .child(self.settings_nav_item(
                                SettingsSection::GitActions,
                                active,
                                section_active_bg,
                                cx,
                            ))
                            .child(self.settings_nav_item(
                                SettingsSection::Keybindings,
                                active,
                                section_active_bg,
                                cx,
                            ))
                            .child(self.settings_nav_item(
                                SettingsSection::Mcp,
                                active,
                                section_active_bg,
                                cx,
                            )),
                    ),
            )
            .child(
                div()
                    .id("settings-page-scroll-narrow")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.settings_content(cx)),
            )
    }

    fn settings_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let bg = crate::theme::chrome_bg(window);
        let back_text = hsla(0., 0., 0.55, 1.);
        let back_hover = gpui::white().opacity(0.06);
        let section_active_bg = hsla(215. / 360., 0.60, 0.45, 1.);

        let active = self.settings_section;

        div()
            .flex()
            .flex_col()
            .w(px(SETTINGS_SIDEBAR_W))
            .flex_shrink_0()
            .bg(bg)
            .overflow_hidden()
            .pt(px(TITLEBAR_CHROME_H + 4.))
            .child(
                div()
                    .id("settings-back-btn")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .mx(px(12.))
                    .mb(px(16.))
                    .px(px(4.))
                    .py(px(4.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(back_hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.settings_open = false;
                            this.shortcut_capture_action = None;
                            this.settings_agent_input.focused_agent_id = None;
                            this.settings_agent_input.selection_anchor = None;
                            this.settings_git_commit_script_input.focused = false;
                            this.settings_git_commit_script_input.selection_anchor = None;
                            this.settings_git_pr_script_input.focused = false;
                            this.settings_git_pr_script_input.selection_anchor = None;
                            this.settings_git_commit_script_drag_anchor = None;
                            this.settings_git_pr_script_drag_anchor = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__chevron-left.svg")
                            .size(px(14.))
                            .text_color(back_text),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .text_color(back_text)
                            .child("Back to app"),
                    ),
            )
            .child(self.settings_nav_item(SettingsSection::General, active, section_active_bg, cx))
            .child(self.settings_nav_item(SettingsSection::Agents, active, section_active_bg, cx))
            .child(self.settings_nav_item(SettingsSection::OpenIn, active, section_active_bg, cx))
            .child(self.settings_nav_item(
                SettingsSection::GitActions,
                active,
                section_active_bg,
                cx,
            ))
            .child(self.settings_nav_item(
                SettingsSection::Keybindings,
                active,
                section_active_bg,
                cx,
            ))
            .child(self.settings_nav_item(SettingsSection::Mcp, active, section_active_bg, cx))
    }

    fn settings_nav_item(
        &self,
        section: SettingsSection,
        active: SettingsSection,
        active_bg: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_active = section == active;
        let label = section.label();
        let text_col = if is_active {
            gpui::white()
        } else {
            TEXT_SECONDARY()
        };
        let hover_bg = gpui::white().opacity(0.06);

        div()
            .id(label)
            .flex()
            .flex_row()
            .items_center()
            .h(px(30.))
            .mx(px(8.))
            .px(px(10.))
            .rounded(px(5.))
            .cursor_pointer()
            .when(is_active, move |d| d.bg(active_bg))
            .when(!is_active, move |d| d.hover(move |s| s.bg(hover_bg)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    this.settings_section = section;
                    this.shortcut_capture_action = None;
                    this.settings_agent_input.focused_agent_id = None;
                    this.settings_agent_input.selection_anchor = None;
                    this.settings_git_commit_script_input.focused = false;
                    this.settings_git_commit_script_input.selection_anchor = None;
                    this.settings_git_pr_script_input.focused = false;
                    this.settings_git_pr_script_input.selection_anchor = None;
                    this.settings_git_commit_script_drag_anchor = None;
                    this.settings_git_pr_script_drag_anchor = None;
                    if section == SettingsSection::GitActions {
                        this.sync_settings_git_action_script_from_store(
                            crate::app::SettingsGitActionScriptKind::Commit,
                        );
                        this.sync_settings_git_action_script_from_store(
                            crate::app::SettingsGitActionScriptKind::PullRequest,
                        );
                    }
                    cx.notify();
                }),
            )
            .child(
                div()
                    .text_size(rems(13. / 16.))
                    .text_color(text_col)
                    .child(label),
            )
    }

    fn settings_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        match self.settings_section {
            SettingsSection::General => self.settings_general_content(cx),
            SettingsSection::Agents => self.settings_agents_content(cx),
            SettingsSection::OpenIn => self.settings_open_in_content(cx),
            SettingsSection::GitActions => self.settings_git_actions_content(cx),
            SettingsSection::Keybindings => self.settings_keybindings_content(cx),
            SettingsSection::Mcp => self.settings_mcp_content(cx),
        }
    }

    fn settings_general_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        use crate::updater::{UpdateState, UpdaterCommand};

        let panel_bg = rgb(0x23252a);
        let row_bg = rgb(0x1f2125);
        let button_bg = gpui::white().opacity(0.04);
        let button_hover = gpui::white().opacity(0.08);
        let active_button_bg = hsla(215. / 360., 0.60, 0.45, 1.);

        let identity = self.updater.identity();
        let short_sha = identity.short_sha;
        let full_sha = identity.full_sha;
        let cargo_version = identity.cargo_version;
        let profile_label = if identity.is_dev_build {
            "debug"
        } else {
            "release"
        };

        let (status_label, status_detail) = updater_status_strings(&self.updater_state);
        let check_disabled =
            self.updater_state.is_checking() || self.updater_state.is_downloading();
        let install_enabled = matches!(self.updater_state, UpdateState::ReadyToInstall { .. });
        let show_sidebar_git_metadata = self.project_store.ui.show_sidebar_git_metadata;

        let copy_full_sha = full_sha.to_string();

        let sidebar_metadata_row = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(20.))
            .px(px(18.))
            .py(px(14.))
            .bg(row_bg)
            .cursor_pointer()
            .hover(move |style| style.bg(gpui::white().opacity(0.06)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    this.set_sidebar_git_metadata_visible(!show_sidebar_git_metadata, cx);
                    cx.stop_propagation();
                }),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .min_w(px(0.))
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT_PRIMARY())
                            .child("Sidebar git metadata"),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .text_color(TEXT_SECONDARY())
                            .child("Show relative commit time and +/- line counts in task rows."),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(if show_sidebar_git_metadata {
                                gpui::white()
                            } else {
                                TEXT_SECONDARY()
                            })
                            .child(if show_sidebar_git_metadata {
                                "Enabled"
                            } else {
                                "Disabled"
                            }),
                    )
                    .child(
                        div()
                            .w(px(18.))
                            .h(px(18.))
                            .rounded(px(5.))
                            .border_1()
                            .border_color(if show_sidebar_git_metadata {
                                active_button_bg.opacity(0.85)
                            } else {
                                BORDER_SUBTLE()
                            })
                            .bg(if show_sidebar_git_metadata {
                                active_button_bg
                            } else {
                                button_bg
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(show_sidebar_git_metadata, |container| {
                                container.child(
                                    svg()
                                        .path("assets/icons/icons__check.svg")
                                        .size(px(11.))
                                        .text_color(gpui::white()),
                                )
                            }),
                    ),
            );

        let build_row = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(20.))
            .px(px(18.))
            .py(px(14.))
            .bg(row_bg)
            .border_t_1()
            .border_color(BORDER_SUBTLE())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT_PRIMARY())
                            .child("Build"),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .text_color(TEXT_SECONDARY())
                            .child(format!("{short_sha} · {profile_label} · v{cargo_version}",)),
                    )
                    .child(
                        div()
                            .id("settings-general-full-sha")
                            .mt(px(2.))
                            .text_size(rems(11. / 16.))
                            .font_family("Lilex Nerd Font Mono")
                            .text_color(TEXT_SECONDARY())
                            .cursor_pointer()
                            .hover(|s| s.text_color(TEXT_PRIMARY()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                    cx.write_to_clipboard(ClipboardItem::new_string(
                                        copy_full_sha.clone(),
                                    ));
                                    this.show_success_toast("Copied commit SHA.", cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(format!("{full_sha} · click to copy")),
                    ),
            );

        let manifest_url = crate::updater::manifest_url().to_string();
        let updates_row = div()
            .flex()
            .flex_row()
            .items_start()
            .justify_between()
            .gap(px(20.))
            .px(px(18.))
            .py(px(14.))
            .bg(row_bg)
            .border_t_1()
            .border_color(BORDER_SUBTLE())
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .min_w(px(0.))
                    .flex_1()
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT_PRIMARY())
                            .child("Updates"),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .text_color(TEXT_PRIMARY())
                            .child(status_label),
                    )
                    .when_some(status_detail, |container, detail| {
                        container.child(
                            div()
                                .text_size(rems(11. / 16.))
                                .text_color(TEXT_SECONDARY())
                                .child(detail),
                        )
                    })
                    .child(
                        div()
                            .mt(px(4.))
                            .text_size(rems(10. / 16.))
                            .text_color(TEXT_SECONDARY())
                            .child(format!("Source: {manifest_url}")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(settings_general_button(
                        "settings-general-check",
                        "Check for updates",
                        !check_disabled,
                        button_bg,
                        button_hover,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.updater.send(UpdaterCommand::CheckNow);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    ))
                    .child(settings_general_button(
                        "settings-general-install",
                        "Install update",
                        install_enabled,
                        active_button_bg,
                        active_button_bg,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.updater.send(UpdaterCommand::Install);
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )),
            );

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .p(px(32.))
            .child(
                div()
                    .text_size(rems(18. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT_PRIMARY())
                    .child("General"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(760.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child("Identity for this installed build, plus controls for in-app updates."),
            )
            .child(
                div()
                    .mt(px(24.))
                    .max_w(px(860.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .overflow_hidden()
                    .child(sidebar_metadata_row)
                    .child(build_row)
                    .child(updates_row),
            )
    }

    fn settings_agents_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        let panel_bg = rgb(0x23252a);
        let row_bg = rgb(0x1f2125);
        let pill_bg = rgb(0x2a2d33);
        let pill_border = gpui::white().opacity(0.10);
        let button_bg = gpui::white().opacity(0.04);
        let button_hover = gpui::white().opacity(0.08);
        let active_button_bg = hsla(215. / 360., 0.60, 0.45, 1.);
        let enabled_agents = self.enabled_agents();

        let mut rows = div().flex().flex_col();
        for (index, agent) in AGENTS.iter().enumerate() {
            let args = self.project_store.agent_launch_args(agent.id);
            let is_enabled = self.agent_enabled(agent.id);
            let is_default = self.agent_is_default(agent.id);
            let draft = self
                .settings_agent_input
                .drafts
                .get(agent.id)
                .cloned()
                .unwrap_or_default();
            let is_focused =
                self.settings_agent_input.focused_agent_id.as_deref() == Some(agent.id);
            let selection = settings_agent_input_selected_range(
                self.settings_agent_input.cursor,
                self.settings_agent_input.selection_anchor,
            );

            let mut row = div()
                .id(("settings-agent-row", index))
                .flex()
                .flex_row()
                .items_start()
                .justify_between()
                .gap(px(20.))
                .px(px(18.))
                .py(px(16.))
                .bg(row_bg);

            if index > 0 {
                row = row.border_t_1().border_color(BORDER_SUBTLE());
            }

            let mut arg_pills = div().flex().flex_row().flex_wrap().gap(px(8.));
            if args.is_empty() {
                arg_pills = arg_pills.child(
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(pill_border)
                        .bg(pill_bg)
                        .text_size(rems(12. / 16.))
                        .font_family("Lilex Nerd Font Mono")
                        .text_color(TEXT_SECONDARY())
                        .child("No extra args"),
                );
            } else {
                for (arg_index, arg) in args.iter().enumerate() {
                    let arg_label = arg.clone();
                    arg_pills = arg_pills.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(pill_border)
                            .bg(pill_bg)
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_family("Lilex Nerd Font Mono")
                                    .text_color(TEXT_PRIMARY())
                                    .child(arg_label),
                            )
                            .child(
                                div()
                                    .w(px(18.))
                                    .h(px(18.))
                                    .rounded(px(5.))
                                    .cursor_pointer()
                                    .hover(move |style| style.bg(gpui::white().opacity(0.08)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, _ev: &MouseDownEvent, _window, cx| {
                                                this.remove_agent_launch_arg(
                                                    agent.id, arg_index, cx,
                                                );
                                                cx.stop_propagation();
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .h_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(rems(11. / 16.))
                                            .text_color(TEXT_SECONDARY())
                                            .child("x"),
                                    ),
                            ),
                    );
                }
            }

            rows = rows.child(row.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w(px(0.))
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .justify_between()
                            .gap(px(20.))
                            .child(
                                div()
                                    .flex()
                                    .flex_1()
                                    .flex_col()
                                    .gap(px(10.))
                                    .min_w(px(0.))
                                    .max_w(px(540.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(12.))
                                            .child(
                                                div().flex_none().child(branded_icon(
                                                    agent.icon,
                                                    18.,
                                                    Some(TEXT_PRIMARY()),
                                                )),
                                            )
                                            .child(
                                                div()
                                                    .min_w(px(0.))
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(rems(13. / 16.))
                                                            .font_weight(
                                                                gpui::FontWeight::MEDIUM,
                                                            )
                                                            .text_color(TEXT_PRIMARY())
                                                            .child(agent.label),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(rems(11. / 16.))
                                                            .text_color(TEXT_SECONDARY())
                                                            .child(format!(
                                                                "Extra argv tokens passed to {} on every launch and resume.",
                                                                agent.label
                                                            )),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .flex_wrap()
                                    .justify_end()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id(("settings-agent-input", index))
                                            .h(px(34.))
                                            .w(px(180.))
                                            .min_w(px(0.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(if is_focused {
                                                active_button_bg.opacity(0.85)
                                            } else {
                                                BORDER_SUBTLE()
                                            })
                                            .bg(button_bg)
                                            .pl(px(10.))
                                            .pr(px(1.4))
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(button_hover))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _ev: &MouseDownEvent, window, cx| {
                                                        this.focus_handle.focus(window, cx);
                                                        this.focus_settings_agent_input(
                                                            agent.id, cx,
                                                        );
                                                        cx.stop_propagation();
                                                    },
                                                ),
                                            )
                                            .child(render_settings_agent_input_content(
                                                &draft,
                                                is_focused,
                                                self.settings_agent_input.cursor,
                                                selection,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id(("settings-agent-add", index))
                                            .h(px(34.))
                                            .px(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(BORDER_SUBTLE())
                                            .bg(button_bg)
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(button_hover))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _ev: &MouseDownEvent, _window, cx| {
                                                        this.add_agent_launch_arg(agent.id, cx);
                                                        cx.stop_propagation();
                                                    },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .h_full()
                                                    .flex()
                                                    .items_center()
                                                    .text_size(rems(12. / 16.))
                                                    .font_weight(
                                                        gpui::FontWeight::MEDIUM,
                                                    )
                                                    .text_color(TEXT_PRIMARY())
                                                    .child("Add"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .h(px(34.))
                                            .px(px(10.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(if is_default {
                                                active_button_bg.opacity(0.85)
                                            } else {
                                                BORDER_SUBTLE()
                                            })
                                            .bg(if is_default { active_button_bg } else { button_bg })
                                            .cursor_pointer()
                                            .hover(move |style| {
                                                style.bg(if is_default {
                                                    active_button_bg
                                                } else {
                                                    button_hover
                                                })
                                            })
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _ev: &MouseDownEvent, _window, cx| {
                                                        this.set_default_agent(agent.id, cx);
                                                        cx.stop_propagation();
                                                    },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .h_full()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap(px(10.))
                                                    .child(
                                                        div()
                                                            .text_size(rems(11. / 16.))
                                                            .font_weight(
                                                                gpui::FontWeight::MEDIUM,
                                                            )
                                                            .text_color(if is_default {
                                                                gpui::white()
                                                            } else {
                                                                TEXT_SECONDARY()
                                                            })
                                                            .child(if is_default {
                                                                "Default"
                                                            } else {
                                                                "Make default"
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(18.))
                                                            .h(px(18.))
                                                            .rounded(px(999.))
                                                            .border_1()
                                                            .border_color(if is_default {
                                                                gpui::white().opacity(0.85)
                                                            } else {
                                                                BORDER_SUBTLE()
                                                            })
                                                            .bg(if is_default {
                                                                gpui::white().opacity(0.16)
                                                            } else {
                                                                button_bg
                                                            })
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .when(is_default, |container| {
                                                                container.child(
                                                                    div()
                                                                        .w(px(8.))
                                                                        .h(px(8.))
                                                                        .rounded(px(999.))
                                                                        .bg(gpui::white()),
                                                                )
                                                            }),
                                                    ),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .h(px(34.))
                                            .px(px(10.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(BORDER_SUBTLE())
                                            .bg(button_bg)
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(button_hover))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _ev: &MouseDownEvent, _window, cx| {
                                                        this.set_agent_enabled(
                                                            agent.id,
                                                            !is_enabled,
                                                            cx,
                                                        );
                                                        cx.stop_propagation();
                                                    },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .h_full()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap(px(10.))
                                                    .child(
                                                        div()
                                                            .text_size(rems(11. / 16.))
                                                            .font_weight(
                                                                gpui::FontWeight::MEDIUM,
                                                            )
                                                            .text_color(if is_enabled {
                                                                gpui::white()
                                                            } else {
                                                                TEXT_SECONDARY()
                                                            })
                                                            .child(if is_enabled {
                                                                "Enabled"
                                                            } else {
                                                                "Disabled"
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(18.))
                                                            .h(px(18.))
                                                            .rounded(px(5.))
                                                            .border_1()
                                                            .border_color(if is_enabled {
                                                                active_button_bg.opacity(0.85)
                                                            } else {
                                                                BORDER_SUBTLE()
                                                            })
                                                            .bg(if is_enabled {
                                                                active_button_bg
                                                            } else {
                                                                button_bg
                                                            })
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .when(is_enabled, |container| {
                                                                container.child(
                                                                    svg()
                                                                        .path(
                                                                            "assets/icons/icons__check.svg",
                                                                        )
                                                                        .size(px(11.))
                                                                        .text_color(gpui::white()),
                                                                )
                                                            }),
                                                    ),
                                            ),
                                    ),
                            ),
                    )
                    .child(div().w_full().min_w(px(0.)).child(arg_pills)),
            ));
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .p(px(32.))
            .child(
                div()
                    .text_size(rems(18. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT_PRIMARY())
                    .child("Agents"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(760.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child(
                        "Manage per-agent argv tokens and availability. Disabled agents stay here so they can be re-enabled, but they are hidden from New Task and Add Agent pickers. Changes save immediately.",
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(16.))
                    .max_w(px(860.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Availability"),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child(
                                        "Choose which enabled agent is used first for new tasks and new agent tabs. Disabled agents can still be re-enabled and edited here.",
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT_PRIMARY())
                            .child(format!("{} enabled", enabled_agents.len())),
                    ),
            )
            .child(
                div()
                    .mt(px(12.))
                    .mb(px(16.))
                    .max_w(px(860.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Token rules"),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child(
                                        "Whitespace is rejected because spaces would create multiple argv tokens. Reorder by removing and re-adding.",
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .max_w(px(860.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .overflow_hidden()
                    .child(rows),
            )
    }

    fn settings_open_in_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        let panel_bg = rgb(0x23252a);
        let row_bg = rgb(0x1f2125);
        let button_bg = gpui::white().opacity(0.04);
        let button_hover = gpui::white().opacity(0.08);
        let active_button_bg = hsla(215. / 360., 0.60, 0.45, 1.);
        let enabled_apps = self.enabled_open_in_apps();

        let mut rows = div().flex().flex_col();
        for (index, app) in self.available_open_in_apps.iter().copied().enumerate() {
            let is_enabled = self.open_in_app_enabled(app);

            let mut row = div()
                .id(("settings-open-in-row", index))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(20.))
                .px(px(18.))
                .py(px(14.))
                .bg(row_bg)
                .cursor_pointer()
                .hover(move |style| style.bg(gpui::white().opacity(0.06)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        this.set_open_in_app_enabled(app, !is_enabled, cx);
                        cx.stop_propagation();
                    }),
                );

            if index > 0 {
                row = row.border_t_1().border_color(BORDER_SUBTLE());
            }

            rows = rows.child(
                row.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            svg()
                                .path(app.icon_path())
                                .size(px(16.))
                                .text_color(TEXT_PRIMARY()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(rems(13. / 16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(TEXT_PRIMARY())
                                        .child(app.label()),
                                )
                                .child(
                                    div()
                                        .text_size(rems(11. / 16.))
                                        .text_color(TEXT_SECONDARY())
                                        .child(app.description()),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(if is_enabled {
                                    gpui::white()
                                } else {
                                    TEXT_SECONDARY()
                                })
                                .child(if is_enabled { "Enabled" } else { "Disabled" }),
                        )
                        .child(
                            div()
                                .w(px(18.))
                                .h(px(18.))
                                .rounded(px(5.))
                                .border_1()
                                .border_color(if is_enabled {
                                    active_button_bg.opacity(0.85)
                                } else {
                                    BORDER_SUBTLE()
                                })
                                .bg(if is_enabled {
                                    active_button_bg
                                } else {
                                    button_bg
                                })
                                .hover(move |style| {
                                    style.bg(if is_enabled {
                                        active_button_bg
                                    } else {
                                        button_hover
                                    })
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(is_enabled, |container| {
                                    container.child(
                                        svg()
                                            .path("assets/icons/icons__check.svg")
                                            .size(px(11.))
                                            .text_color(gpui::white()),
                                    )
                                }),
                        ),
                ),
            );
        }

        let availability_note = if self.available_open_in_apps.is_empty() {
            "No supported apps were detected on this machine."
        } else {
            "Only apps detected on this machine appear here. Changes save immediately."
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .p(px(32.))
            .child(
                div()
                    .text_size(rems(18. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT_PRIMARY())
                    .child("Open In"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(760.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child(
                        "Choose which detected apps appear in the project header's Open In menu.",
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(16.))
                    .max_w(px(860.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Detected apps"),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child(availability_note),
                            ),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT_PRIMARY())
                            .child(format!("{} enabled", enabled_apps.len())),
                    ),
            )
            .when(self.available_open_in_apps.is_empty(), |section| {
                section.child(
                    div()
                        .max_w(px(860.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(BORDER_SUBTLE())
                        .bg(panel_bg)
                        .px(px(20.))
                        .py(px(18.))
                        .child(
                            div()
                                .text_size(rems(12. / 16.))
                                .line_height(rems(18. / 16.))
                                .text_color(TEXT_SECONDARY())
                                .child(
                                    "Install Cursor, Zed, VS Code, Ghostty, WezTerm, or use your system file manager, then restart the app to refresh the menu.",
                                ),
                        ),
                )
            })
            .when(!self.available_open_in_apps.is_empty(), |section| {
                section.child(
                    div()
                        .max_w(px(860.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(BORDER_SUBTLE())
                        .bg(panel_bg)
                        .overflow_hidden()
                        .child(rows),
                )
            })
    }

    fn settings_git_actions_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .p(px(32.))
            .child(
                div()
                    .text_size(rems(18. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT_PRIMARY())
                    .child("Git Actions"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(760.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child(
                        "Customize the instructions sent to the LLM when the app generates commit messages and pull request title/body content. The app appends the relevant git context automatically. Changes save immediately, and you can reset back to the built-in instructions at any time.",
                    ),
            )
            .child(self.render_git_action_script_panel(
                crate::app::SettingsGitActionScriptKind::Commit,
                "Commit message instructions",
                "Currently using the default built-in template.",
                "Currently using a custom template from settings.",
                "Paste commit generation instructions here.",
                "settings-git-actions-commit",
                cx,
            ))
            .child(self.render_git_action_script_panel(
                crate::app::SettingsGitActionScriptKind::PullRequest,
                "PR title/body instructions",
                "Currently using the default built-in template.",
                "Currently using a custom template from settings.",
                "Paste PR title/body instructions here.",
                "settings-git-actions-pr",
                cx,
            ))
    }

    fn render_git_action_script_panel(
        &self,
        kind: crate::app::SettingsGitActionScriptKind,
        title: &'static str,
        default_label: &'static str,
        custom_label: &'static str,
        placeholder: &'static str,
        element_id_prefix: &'static str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let panel_bg = rgb(0x23252a);
        let button_bg = gpui::white().opacity(0.04);
        let button_hover = gpui::white().opacity(0.08);
        let active_button_bg = hsla(215. / 360., 0.60, 0.45, 1.);
        let editor_bg = rgb(0x191b1f);
        let using_default = match kind {
            crate::app::SettingsGitActionScriptKind::Commit => {
                self.project_store.ui.git_commit_generation_script.is_none()
            }
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                self.project_store.ui.git_pr_generation_script.is_none()
            }
        };
        let input = self.settings_git_action_script_input(kind);
        let draft = &input.draft;
        let is_focused = input.focused;
        let selection = settings_agent_input_selected_range(input.cursor, input.selection_anchor);

        div()
            .mt(px(24.))
            .max_w(px(960.))
            .rounded(px(12.))
            .border_1()
            .border_color(BORDER_SUBTLE())
            .bg(panel_bg)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .px(px(18.))
                    .py(px(14.))
                    .border_b_1()
                    .border_color(BORDER_SUBTLE())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(13. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(TEXT_PRIMARY())
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child(if using_default {
                                        default_label
                                    } else {
                                        custom_label
                                    }),
                            ),
                    )
                    .child(
                        div()
                            .id((element_id_prefix, 0usize))
                            .h(px(30.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(if using_default {
                                BORDER_SUBTLE()
                            } else {
                                active_button_bg.opacity(0.85)
                            })
                            .bg(if using_default {
                                button_bg
                            } else {
                                active_button_bg
                            })
                            .cursor_pointer()
                            .hover(move |s| {
                                s.bg(if using_default {
                                    button_hover
                                } else {
                                    active_button_bg
                                })
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                    this.reset_git_action_script_to_default(kind, cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if using_default {
                                        TEXT_PRIMARY()
                                    } else {
                                        gpui::white()
                                    })
                                    .child("Reset to Default"),
                            ),
                    ),
            )
            .child(
                div().px(px(18.)).py(px(18.)).child(
                    div()
                        .id((element_id_prefix, 1usize))
                        .min_h(px(280.))
                        .max_h(px(480.))
                        .w_full()
                        .overflow_scroll()
                        .rounded(px(10.))
                        .border_1()
                        .border_color(if is_focused {
                            active_button_bg.opacity(0.85)
                        } else {
                            BORDER_SUBTLE()
                        })
                        .bg(editor_bg)
                        .px(px(14.))
                        .py(px(12.))
                        .cursor_text()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                                this.begin_settings_git_action_script_selection(
                                    kind, ev, window, cx,
                                );
                                cx.stop_propagation();
                            }),
                        )
                        .child(
                            div()
                                .text_size(rems(12. / 16.))
                                .line_height(rems(18. / 16.))
                                .font_family("Lilex Nerd Font Mono")
                                .child(SettingsMultilineLayoutHost::new(
                                    cx.entity(),
                                    kind,
                                    draft.to_string(),
                                    render_settings_multiline_input_content(
                                        draft,
                                        is_focused,
                                        input.cursor,
                                        selection,
                                        placeholder,
                                    ),
                                )),
                        ),
                ),
            )
    }

    fn settings_keybindings_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        let panel_bg = rgb(0x23252a);
        let row_bg = rgb(0x1f2125);
        let table_header = hsla(0., 0., 0.45, 1.);
        let pill_bg = rgb(0x2a2d33);
        let pill_border = gpui::white().opacity(0.10);
        let button_bg = gpui::white().opacity(0.04);
        let button_hover = gpui::white().opacity(0.08);
        let active_button_bg = hsla(215. / 360., 0.60, 0.45, 1.);
        let destructive_text = hsla(0.0, 0.73, 0.67, 1.);

        let mut rows = div().flex().flex_col();
        for (index, action) in ALL_SHORTCUT_ACTIONS.into_iter().enumerate() {
            let is_capturing = self.shortcut_capture_action == Some(action);
            let shortcut = self.project_store.ui.shortcuts.binding_for(action);

            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(20.))
                .px(px(18.))
                .py(px(14.))
                .bg(row_bg);

            if index > 0 {
                row = row.border_t_1().border_color(BORDER_SUBTLE());
            }

            let shortcut_display = if is_capturing {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(gpui::white())
                            .child("Press shortcut now"),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .text_color(TEXT_SECONDARY())
                            .child("Esc cancels. Delete clears."),
                    )
            } else {
                self.render_shortcut_pills(shortcut, pill_bg, pill_border)
            };

            let capture_label = if is_capturing { "Listening…" } else { "Edit" };
            let capture_text = if is_capturing {
                gpui::white()
            } else {
                TEXT_PRIMARY()
            };

            rows = rows.child(
                row.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(rems(13. / 16.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(TEXT_PRIMARY())
                                .child(action.label()),
                        )
                        .child(shortcut_display),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .id(("settings-shortcut-capture", index))
                                .h(px(30.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(if is_capturing {
                                    active_button_bg.opacity(0.85)
                                } else {
                                    BORDER_SUBTLE()
                                })
                                .bg(if is_capturing {
                                    active_button_bg
                                } else {
                                    button_bg
                                })
                                .cursor_pointer()
                                .hover(move |s| {
                                    s.bg(if is_capturing {
                                        active_button_bg
                                    } else {
                                        button_hover
                                    })
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                        this.begin_shortcut_capture(action, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .child(
                                    div()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .text_size(rems(12. / 16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(capture_text)
                                        .child(capture_label),
                                ),
                        )
                        .child(
                            div()
                                .id(("settings-shortcut-reset", index))
                                .h(px(30.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(BORDER_SUBTLE())
                                .bg(button_bg)
                                .cursor_pointer()
                                .hover(move |s| s.bg(button_hover))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                        this.reset_shortcut_binding(action, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .child(
                                    div()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .text_size(rems(12. / 16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(TEXT_PRIMARY())
                                        .child("Reset"),
                                ),
                        )
                        .child(
                            div()
                                .id(("settings-shortcut-clear", index))
                                .h(px(30.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(BORDER_SUBTLE())
                                .bg(button_bg)
                                .cursor_pointer()
                                .hover(move |s| s.bg(button_hover))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                        this.clear_shortcut_binding(action, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .child(
                                    div()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .text_size(rems(12. / 16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(destructive_text)
                                        .child("Clear"),
                                ),
                        ),
                ),
            );
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .p(px(32.))
            .child(
                div()
                    .text_size(rems(18. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT_PRIMARY())
                    .child("Keybindings"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(720.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child(
                        "Choose Edit on a command, then press the new key combination. Changes save immediately and apply to tab and navigation shortcuts across the app.",
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(16.))
                    .max_w(px(860.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Capture rules"),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child("Use at least one modifier key. Duplicate shortcuts are blocked."),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-shortcuts-reset-all")
                            .h(px(30.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(BORDER_SUBTLE())
                            .bg(button_bg)
                            .cursor_pointer()
                            .hover(move |s| s.bg(button_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.reset_all_shortcuts(cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Reset All"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .max_w(px(860.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(18.))
                            .h(px(38.))
                            .border_b_1()
                            .border_color(BORDER_SUBTLE())
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(table_header)
                            .child("COMMAND")
                            .child("SHORTCUT"),
                    )
                    .child(div().flex_1().min_h(px(0.)).child(rows)),
            )
    }

    fn render_shortcut_pills(
        &self,
        shortcut: &str,
        pill_bg: gpui::Rgba,
        pill_border: gpui::Hsla,
    ) -> gpui::Div {
        if shortcut.is_empty() {
            return div().flex().flex_row().items_center().gap(px(8.)).child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(pill_border)
                    .bg(pill_bg)
                    .text_size(rems(12. / 16.))
                    .font_family("Lilex Nerd Font Mono")
                    .text_color(TEXT_SECONDARY())
                    .child("Unassigned"),
            );
        }

        let mut shortcut_pills = div().flex().flex_row().items_center().gap(px(8.));
        for token in shortcut.split('-') {
            shortcut_pills = shortcut_pills.child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(pill_border)
                    .bg(pill_bg)
                    .text_size(rems(12. / 16.))
                    .font_family("Lilex Nerd Font Mono")
                    .text_color(TEXT_PRIMARY())
                    .child(keybinding_token_label(token)),
            );
        }
        shortcut_pills
    }
}

struct SettingsMultilineLayoutHost {
    app: Entity<AnotherOneApp>,
    kind: crate::app::SettingsGitActionScriptKind,
    text: String,
    child: AnyElement,
}

impl SettingsMultilineLayoutHost {
    fn new(
        app: Entity<AnotherOneApp>,
        kind: crate::app::SettingsGitActionScriptKind,
        text: String,
        child: impl IntoElement,
    ) -> Self {
        Self {
            app,
            kind,
            text,
            child: child.into_any_element(),
        }
    }
}

impl IntoElement for SettingsMultilineLayoutHost {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for SettingsMultilineLayoutHost {
    type RequestLayoutState = ();
    type PrepaintState = Bounds<Pixels>;

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, Self::RequestLayoutState) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        self.child.prepaint(window, cx);
        bounds
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        _bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint_bounds: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        self.child.paint(window, cx);
        let measured_lines =
            measure_settings_multiline_input_lines(&self.text, *prepaint_bounds, window);
        let _ = self.app.update(cx, |app, _cx| match self.kind {
            crate::app::SettingsGitActionScriptKind::Commit => {
                app.settings_git_commit_script_layout = measured_lines;
            }
            crate::app::SettingsGitActionScriptKind::PullRequest => {
                app.settings_git_pr_script_layout = measured_lines;
            }
        });
    }
}

fn validate_agent_launch_arg(value: &str) -> Result<String, &'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Enter one argv token before adding it.");
    }

    if trimmed != value || value.chars().any(char::is_whitespace) {
        return Err("Launch args must be a single argv token without whitespace.");
    }

    Ok(value.to_string())
}

fn settings_agent_input_selected_range(
    cursor: usize,
    selection_anchor: Option<usize>,
) -> Option<std::ops::Range<usize>> {
    crate::text_edit::selected_range(cursor, selection_anchor)
}

fn with_settings_edit_state(
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    edit: impl FnOnce(&mut TextEditState),
) {
    let mut state = TextEditState::new(*cursor, *selection_anchor);
    edit(&mut state);
    *cursor = state.cursor;
    *selection_anchor = state.selection_anchor;
}

fn replace_settings_input_range(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    range: std::ops::Range<usize>,
    replacement: &str,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.replace_range(text, range, replacement);
    });
}

fn insert_settings_input_text(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    inserted: &str,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.insert_text(text, inserted)
    });
}

fn delete_settings_input_backward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.delete_backward(text)
    });
}

fn delete_settings_input_word_backward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.delete_word_backward(text)
    });
}

fn delete_settings_input_to_start(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.delete_to_start(text)
    });
}

fn delete_settings_input_forward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| state.delete_forward(text));
}

fn move_settings_input_cursor(
    text: &str,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    direction: CursorDirection,
    extend_selection: bool,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.move_horizontal(text, direction, extend_selection);
    });
}

fn move_settings_input_cursor_to_edge(
    text: &str,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    to_end: bool,
    extend_selection: bool,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.move_to_edge(text, to_end, extend_selection);
    });
}

fn intersect_byte_ranges(
    left: std::ops::Range<usize>,
    right: std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    let start = left.start.max(right.start);
    let end = left.end.min(right.end);
    (start < end).then_some(start..end)
}

fn visible_input_range(
    text: &str,
    cursor: usize,
    selection: Option<&std::ops::Range<usize>>,
    max_chars: usize,
    extra_reserved_chars: usize,
) -> std::ops::Range<usize> {
    crate::text_edit::visible_range(
        text,
        cursor,
        selection,
        max_chars.saturating_sub(extra_reserved_chars).max(1),
    )
}

fn settings_multiline_line_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    crate::text_edit::line_ranges(text)
}

fn measure_settings_multiline_input_lines(
    text: &str,
    bounds: Bounds<Pixels>,
    window: &mut Window,
) -> Vec<crate::app::SettingsGitActionScriptLineLayout> {
    if text.is_empty() {
        return Vec::new();
    }

    let style = window.text_style();
    let font_size = style.font_size.to_pixels(window.rem_size());
    let line_height = window.line_height();
    let row_step = line_height + px(2.);

    settings_multiline_line_ranges(text)
        .into_iter()
        .enumerate()
        .map(|(index, range)| {
            let line_text = &text[range.clone()];
            let line = shape_settings_input_line(line_text, font_size, &style, window);
            let top = bounds.top() + row_step * index as f32;
            crate::app::SettingsGitActionScriptLineLayout {
                range,
                bounds: Bounds::new(
                    point(bounds.left(), top),
                    size(bounds.size.width, line_height),
                ),
                line,
            }
        })
        .collect()
}

fn shape_settings_input_line(
    text: &str,
    font_size: Pixels,
    style: &gpui::TextStyle,
    window: &mut Window,
) -> ShapedLine {
    let run = TextRun {
        len: text.len(),
        font: style.font(),
        color: style.color,
        background_color: None,
        underline: None,
        strikethrough: None,
    };

    window
        .text_system()
        .shape_line(text.to_string().into(), font_size, &[run], None)
}

fn settings_general_button<F>(
    id: &'static str,
    label: &'static str,
    enabled: bool,
    bg: gpui::Hsla,
    hover_bg: gpui::Hsla,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&MouseDownEvent, &mut gpui::Window, &mut App) + 'static,
{
    let base = div()
        .id(id)
        .px(px(12.))
        .py(px(7.))
        .rounded(px(8.))
        .border_1()
        .border_color(BORDER_SUBTLE())
        .bg(bg)
        .text_size(rems(12. / 16.))
        .font_weight(gpui::FontWeight::MEDIUM)
        .text_color(if enabled {
            TEXT_PRIMARY()
        } else {
            TEXT_SECONDARY()
        })
        .child(label);

    if enabled {
        base.cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .on_mouse_down(MouseButton::Left, on_click)
            .into_any_element()
    } else {
        base.into_any_element()
    }
}

fn updater_status_strings(state: &crate::updater::UpdateState) -> (String, Option<String>) {
    use crate::updater::UpdateState;
    match state {
        UpdateState::Idle => ("Not yet checked".into(), None),
        UpdateState::Checking => ("Checking for updates…".into(), None),
        UpdateState::UpToDate { .. } => ("Up to date".into(), None),
        UpdateState::UpdateAvailable {
            manifest, asset, ..
        } => (
            format!("Update available: {}", &manifest.short_sha),
            Some(format!("{}/{} · {}", asset.os, asset.arch, asset.kind)),
        ),
        UpdateState::Downloading {
            manifest,
            downloaded,
            total,
            ..
        } => {
            let detail = match total {
                Some(total) if *total > 0 => Some(format!(
                    "{} of {} downloaded",
                    format_bytes(*downloaded),
                    format_bytes(*total)
                )),
                _ => Some(format!("{} downloaded", format_bytes(*downloaded))),
            };
            (format!("Downloading {}…", &manifest.short_sha), detail)
        }
        UpdateState::ReadyToInstall { manifest, path, .. } => (
            format!("Update {} ready to install", &manifest.short_sha),
            Some(path.display().to_string()),
        ),
        UpdateState::Installing => (
            "Installing update — the app will relaunch shortly.".into(),
            None,
        ),
        UpdateState::UnsupportedPlatform { manifest, .. } => (
            format!(
                "Update {} published, but no asset for this OS/arch.",
                &manifest.short_sha
            ),
            Some("Manual download from the release page is required.".into()),
        ),
        UpdateState::Error { message, .. } => ("Last check failed".into(), Some(message.clone())),
    }
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [(&str, f64); 4] = [
        ("GiB", 1024.0 * 1024.0 * 1024.0),
        ("MiB", 1024.0 * 1024.0),
        ("KiB", 1024.0),
        ("B", 1.0),
    ];
    let bytes_f = bytes as f64;
    for (suffix, factor) in UNITS {
        if bytes_f >= factor {
            return format!("{:.1} {suffix}", bytes_f / factor);
        }
    }
    format!("{bytes} B")
}

fn distance_to_vertical_bounds(y: Pixels, bounds: Bounds<Pixels>) -> f32 {
    if y < bounds.top() {
        f32::from(bounds.top() - y)
    } else if y > bounds.bottom() {
        f32::from(y - bounds.bottom())
    } else {
        0.
    }
}

fn move_settings_multiline_cursor_vertical(
    text: &str,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    move_down: bool,
    extend_selection: bool,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.move_vertical(text, !move_down, extend_selection);
    });
}

fn move_settings_multiline_cursor_to_line_edge(
    text: &str,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    to_end: bool,
    extend_selection: bool,
) {
    with_settings_edit_state(cursor, selection_anchor, |state| {
        state.move_to_line_edge(text, to_end, extend_selection);
    });
}

fn render_settings_agent_input_content(
    text: &str,
    focused: bool,
    cursor: usize,
    selection: Option<std::ops::Range<usize>>,
) -> gpui::Div {
    let cursor = cursor.min(text.len());
    let selection = selection.map(|range| range.start.min(text.len())..range.end.min(text.len()));

    if text.is_empty() {
        return div()
            .h_full()
            .flex()
            .items_center()
            .gap(px(0.))
            .text_size(rems(12. / 16.))
            .font_family("Lilex Nerd Font Mono")
            .child(if focused {
                div().w(px(1.)).h(px(16.)).mr(px(1.)).bg(TEXT_PRIMARY())
            } else {
                div().w(px(0.))
            })
            .child(div().text_color(TEXT_SECONDARY()).child("argv-token"));
    }

    let selected = selection.filter(|range| range.start < range.end);
    let visible_range =
        visible_input_range(text, cursor, selected.as_ref(), 20, usize::from(focused));
    let visible_start = visible_range.start;
    let visible_text = text[visible_range.clone()].to_string();
    let local_cursor = cursor.saturating_sub(visible_start).min(visible_text.len());
    let visible_selection = selected
        .as_ref()
        .and_then(|range| intersect_byte_ranges(range.clone(), visible_range.clone()))
        .map(|range| range.start - visible_start..range.end - visible_start);

    let mut row = div()
        .h_full()
        .flex()
        .items_center()
        .gap(px(0.))
        .overflow_hidden()
        .text_size(rems(12. / 16.))
        .font_family("Lilex Nerd Font Mono");

    let (prefix_end, selected_end) = if let Some(range) = visible_selection.as_ref() {
        (
            range.start.min(local_cursor),
            range.end.min(visible_text.len()),
        )
    } else {
        (
            local_cursor.min(visible_text.len()),
            local_cursor.min(visible_text.len()),
        )
    };

    let prefix = visible_text[..prefix_end].to_string();
    let middle = visible_selection
        .as_ref()
        .map(|range| visible_text[range.clone()].to_string())
        .unwrap_or_default();
    let trailing_start = visible_selection
        .as_ref()
        .filter(|range| range.end <= local_cursor)
        .map(|_| selected_end)
        .unwrap_or(local_cursor.min(visible_text.len()));
    let trailing = visible_text[trailing_start..].to_string();

    if !prefix.is_empty() {
        row = row.child(div().text_color(TEXT_PRIMARY()).child(prefix));
    }

    if focused {
        row = row.child(div().w(px(1.)).h(px(16.)).bg(TEXT_PRIMARY()));
    }

    if !middle.is_empty() {
        row = row.child(
            div()
                .px(px(1.))
                .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                .text_color(TEXT_PRIMARY())
                .child(middle),
        );
    }

    if !trailing.is_empty() {
        row = row.child(div().text_color(TEXT_PRIMARY()).child(trailing));
    }

    row
}

fn render_settings_multiline_input_content(
    text: &str,
    focused: bool,
    cursor: usize,
    selection: Option<std::ops::Range<usize>>,
    placeholder: &str,
) -> gpui::Div {
    let cursor = cursor.min(text.len());
    let selection = selection.map(|range| range.start.min(text.len())..range.end.min(text.len()));
    let selected = selection.filter(|range| range.start < range.end);
    let line_ranges = settings_multiline_line_ranges(text);

    let mut column = div()
        .flex()
        .flex_col()
        .gap(px(2.))
        .text_size(rems(12. / 16.))
        .line_height(rems(18. / 16.))
        .font_family("Lilex Nerd Font Mono");

    if text.is_empty() {
        return column.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(0.))
                .child(if focused {
                    div().w(px(1.)).h(px(16.)).mr(px(1.)).bg(TEXT_PRIMARY())
                } else {
                    div().w(px(0.))
                })
                .child(
                    div()
                        .text_color(TEXT_SECONDARY())
                        .child(placeholder.to_string()),
                ),
        );
    }

    for line_range in line_ranges {
        let line_text = &text[line_range.clone()];
        let visible_selection = selected
            .as_ref()
            .and_then(|range| intersect_byte_ranges(range.clone(), line_range.clone()))
            .map(|range| range.start - line_range.start..range.end - line_range.start);
        let local_cursor = if (line_range.start..=line_range.end).contains(&cursor) {
            Some(cursor - line_range.start)
        } else {
            None
        };

        let mut row = div()
            .min_h(px(18.))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(0.))
            .whitespace_nowrap();

        match (visible_selection, focused.then_some(local_cursor).flatten()) {
            (Some(range), _) => {
                let prefix = &line_text[..range.start];
                let middle = &line_text[range.clone()];
                let suffix = &line_text[range.end..];
                if !prefix.is_empty() {
                    row = row.child(div().text_color(TEXT_PRIMARY()).child(prefix.to_string()));
                }
                row = row.child(
                    div()
                        .px(px(1.))
                        .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                        .text_color(TEXT_PRIMARY())
                        .child(if middle.is_empty() {
                            " ".to_string()
                        } else {
                            middle.to_string()
                        }),
                );
                if !suffix.is_empty() {
                    row = row.child(div().text_color(TEXT_PRIMARY()).child(suffix.to_string()));
                }
            }
            (None, Some(local_cursor)) => {
                let prefix = &line_text[..local_cursor.min(line_text.len())];
                let suffix = &line_text[local_cursor.min(line_text.len())..];
                if !prefix.is_empty() {
                    row = row.child(div().text_color(TEXT_PRIMARY()).child(prefix.to_string()));
                }
                row = row.child(div().w(px(1.)).h(px(16.)).bg(TEXT_PRIMARY()));
                if !suffix.is_empty() {
                    row = row.child(div().text_color(TEXT_PRIMARY()).child(suffix.to_string()));
                }
                if prefix.is_empty() && suffix.is_empty() {
                    row = row.child(div().text_color(TEXT_PRIMARY().opacity(0.)).child(" "));
                }
            }
            (None, None) => {
                row = row.child(
                    div()
                        .text_color(if line_text.is_empty() {
                            TEXT_PRIMARY().opacity(0.)
                        } else {
                            TEXT_PRIMARY()
                        })
                        .child(if line_text.is_empty() {
                            " ".to_string()
                        } else {
                            line_text.to_string()
                        }),
                );
            }
        }

        column = column.child(row);
    }

    column
}

#[cfg(test)]
mod tests {
    use super::{
        insert_settings_input_text, settings_agent_input_selected_range, validate_agent_launch_arg,
        visible_input_range,
    };

    #[test]
    fn validates_single_token_launch_args() {
        assert_eq!(
            validate_agent_launch_arg("--yolo"),
            Ok("--yolo".to_string())
        );
    }

    #[test]
    fn rejects_empty_launch_args() {
        assert_eq!(
            validate_agent_launch_arg(""),
            Err("Enter one argv token before adding it.")
        );
    }

    #[test]
    fn rejects_whitespace_launch_args() {
        assert_eq!(
            validate_agent_launch_arg(" --yolo"),
            Err("Launch args must be a single argv token without whitespace.")
        );
        assert_eq!(
            validate_agent_launch_arg("--profile debug"),
            Err("Launch args must be a single argv token without whitespace.")
        );
    }

    #[test]
    fn inserts_text_at_cursor() {
        let mut text = String::from("--model");
        let mut cursor = 2;
        let mut selection_anchor = None;

        insert_settings_input_text(&mut text, &mut cursor, &mut selection_anchor, "agent-");

        assert_eq!(text, "--agent-model");
        assert_eq!(cursor, "--agent-".len());
        assert_eq!(selection_anchor, None);
    }

    #[test]
    fn replaces_selected_text_when_inserting() {
        let mut text = String::from("--model");
        let mut cursor = text.len();
        let mut selection_anchor = Some(2);

        insert_settings_input_text(&mut text, &mut cursor, &mut selection_anchor, "agent");

        assert_eq!(text, "--agent");
        assert_eq!(cursor, text.len());
        assert_eq!(selection_anchor, None);
        assert_eq!(
            settings_agent_input_selected_range(cursor, selection_anchor),
            None
        );
    }

    #[test]
    fn visible_input_range_keeps_end_visible_when_clipped() {
        let text = "--dangerously-skip-permissions";

        let visible = visible_input_range(text, text.len(), None, 20, 1);

        assert!(visible.start > 0);
        assert_eq!(visible.end, text.len());
        assert_eq!(text[visible].chars().count(), 19);
    }
}
