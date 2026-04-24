//! Add/Edit project custom action modal.

use gpui::{
    div, hsla, prelude::*, px, relative, rems, rgb, svg, AnyElement, ClipboardItem, Context,
    KeyDownEvent, MouseButton, MouseDownEvent, SharedString,
};

use crate::agent_icons::branded_icon;
use crate::agents::{AgentProviderKind, AGENTS};
use crate::app::AnotherOneApp;
use crate::project_store::{
    ProjectAction, ProjectActionAccess, ProjectActionIcon, ProjectActionKind, ProjectActionScope,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomActionKindDraft {
    Shell,
    Agent,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomActionField {
    Name,
    Command,
    Prompt,
    Model,
    Traits,
    Mode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomActionDropdown {
    Provider,
    Model,
    Traits,
    Mode,
    Access,
}

#[derive(Clone)]
pub(crate) struct CustomActionModalState {
    pub editing_id: Option<String>,
    pub name: String,
    pub icon: ProjectActionIcon,
    pub kind: CustomActionKindDraft,
    pub command: String,
    pub prompt: String,
    pub provider: AgentProviderKind,
    pub model: String,
    pub traits: String,
    pub mode: String,
    pub access: ProjectActionAccess,
    pub run_on_worktree_create: bool,
    pub save_global_copy: bool,
    pub focused_field: CustomActionField,
    pub text_cursor: usize,
    pub text_selection_anchor: Option<usize>,
    pub open_dropdown: Option<CustomActionDropdown>,
}

const CARD_BG: u32 = 0x2b2d31;
const DEFAULT_OPTION: &str = "";

const CODEX_MODEL_OPTIONS: &[(&str, &str)] = &[
    ("gpt-5.4", "GPT-5.4"),
    ("gpt-5.4-mini", "GPT-5.4 Mini"),
    ("gpt-5.3-codex", "GPT-5.3 Codex"),
    ("gpt-5.3-codex-spark", "GPT-5.3 Codex Spark"),
];

const CLAUDE_MODEL_OPTIONS: &[(&str, &str)] = &[
    ("claude-opus-4-7", "Claude Opus 4.7"),
    ("claude-opus-4-6", "Claude Opus 4.6"),
    ("claude-opus-4-5", "Claude Opus 4.5"),
    ("claude-sonnet-4-6", "Claude Sonnet 4.6"),
    ("claude-haiku-4-5", "Claude Haiku 4.5"),
];

const CODEX_TRAITS_OPTIONS: &[(&str, &str)] = &[
    ("xhigh", "Extra high"),
    ("high", "High"),
    ("medium", "Medium"),
    ("low", "Low"),
];

const CLAUDE_OPUS_47_TRAITS_OPTIONS: &[(&str, &str)] = &[
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("xhigh", "Extra high"),
    ("max", "Max"),
    ("ultrathink", "Ultrathink"),
];

const CLAUDE_OPUS_46_TRAITS_OPTIONS: &[(&str, &str)] = &[
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("max", "Max"),
    ("ultrathink", "Ultrathink"),
];

const CLAUDE_OPUS_45_TRAITS_OPTIONS: &[(&str, &str)] = &[
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("max", "Max"),
];

const CLAUDE_SONNET_46_TRAITS_OPTIONS: &[(&str, &str)] = &[
    ("low", "Low"),
    ("medium", "Medium"),
    ("high", "High"),
    ("ultrathink", "Ultrathink"),
];

const MODE_OPTIONS: &[(&str, &str)] = &[("default", "Build"), ("plan", "Plan")];

const ACCESS_OPTIONS: &[(ProjectActionAccess, &str)] = &[
    (ProjectActionAccess::Default, "Default"),
    (ProjectActionAccess::ReadOnly, "Read only"),
    (ProjectActionAccess::WorkspaceWrite, "Workspace write"),
    (ProjectActionAccess::FullAccess, "Full access"),
];

#[derive(Clone)]
struct SelectOption {
    value: String,
    label: String,
}

fn title_col() -> gpui::Hsla {
    hsla(0., 0., 0.92, 1.)
}

fn body_col() -> gpui::Hsla {
    hsla(0., 0., 0.78, 1.)
}

fn muted_col() -> gpui::Hsla {
    hsla(0., 0., 0.58, 1.)
}

fn border_col() -> gpui::Hsla {
    gpui::white().opacity(0.08)
}

fn hover_bg() -> gpui::Hsla {
    gpui::white().opacity(0.06)
}

fn subtle_bg() -> gpui::Hsla {
    gpui::white().opacity(0.04)
}

fn active_bg() -> gpui::Hsla {
    gpui::white().opacity(0.10)
}

fn trim_to_option(text: &str) -> Option<String> {
    let text = text.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn field_value(state: &CustomActionModalState, field: CustomActionField) -> &str {
    match field {
        CustomActionField::Name => &state.name,
        CustomActionField::Command => &state.command,
        CustomActionField::Prompt => &state.prompt,
        CustomActionField::Model => &state.model,
        CustomActionField::Traits => &state.traits,
        CustomActionField::Mode => &state.mode,
    }
}

#[derive(Clone, Copy)]
enum CursorDirection {
    Left,
    Right,
}

fn action_provider_agent(provider: AgentProviderKind) -> Option<&'static crate::agents::AgentDef> {
    AGENTS.iter().find(|agent| agent.provider == Some(provider))
}

fn sanitize_custom_action_text_input(text: String) -> String {
    text.replace(['\n', '\r', '\t'], " ")
}

fn sanitize_custom_action_field_input(field: CustomActionField, text: String) -> String {
    if field == CustomActionField::Prompt {
        text.replace('\r', "")
    } else {
        sanitize_custom_action_text_input(text)
    }
}

fn custom_action_text_selected_range(
    cursor: usize,
    selection_anchor: Option<usize>,
) -> Option<std::ops::Range<usize>> {
    let anchor = selection_anchor?;
    if anchor == cursor {
        None
    } else if anchor < cursor {
        Some(anchor..cursor)
    } else {
        Some(cursor..anchor)
    }
}

fn previous_custom_action_text_boundary(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .rev()
        .find_map(|(index, _)| (index < cursor).then_some(index))
        .unwrap_or(0)
}

fn next_custom_action_text_boundary(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .find_map(|(index, _)| (index > cursor).then_some(index))
        .unwrap_or(text.len())
}

fn replace_custom_action_text_range(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    range: std::ops::Range<usize>,
    replacement: &str,
) {
    text.replace_range(range.clone(), replacement);
    *cursor = range.start + replacement.len();
    *selection_anchor = None;
}

fn insert_custom_action_text(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    inserted: &str,
) {
    let selected = custom_action_text_selected_range(*cursor, *selection_anchor);
    let range = selected.unwrap_or(*cursor..*cursor);
    replace_custom_action_text_range(text, cursor, selection_anchor, range, inserted);
}

fn delete_custom_action_text_backward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    if let Some(range) = custom_action_text_selected_range(*cursor, *selection_anchor) {
        replace_custom_action_text_range(text, cursor, selection_anchor, range, "");
        return;
    }
    if *cursor == 0 {
        return;
    }
    let start = previous_custom_action_text_boundary(text, *cursor);
    replace_custom_action_text_range(text, cursor, selection_anchor, start..*cursor, "");
}

fn previous_custom_action_text_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = cursor;
    while idx > 0 {
        let start = previous_custom_action_text_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if !ch.is_whitespace() {
            break;
        }
        idx = start;
    }

    while idx > 0 {
        let start = previous_custom_action_text_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if ch.is_alphanumeric() || matches!(ch, '_' | '-') {
            idx = start;
        } else {
            break;
        }
    }

    idx
}

fn delete_custom_action_text_word_backward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    if let Some(range) = custom_action_text_selected_range(*cursor, *selection_anchor) {
        replace_custom_action_text_range(text, cursor, selection_anchor, range, "");
        return;
    }
    if *cursor == 0 {
        return;
    }
    let start = previous_custom_action_text_word_boundary(text, *cursor);
    replace_custom_action_text_range(text, cursor, selection_anchor, start..*cursor, "");
}

fn delete_custom_action_text_to_start(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    if let Some(range) = custom_action_text_selected_range(*cursor, *selection_anchor) {
        replace_custom_action_text_range(text, cursor, selection_anchor, range, "");
        return;
    }
    if *cursor == 0 {
        return;
    }
    replace_custom_action_text_range(text, cursor, selection_anchor, 0..*cursor, "");
}

fn delete_custom_action_text_forward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    if let Some(range) = custom_action_text_selected_range(*cursor, *selection_anchor) {
        replace_custom_action_text_range(text, cursor, selection_anchor, range, "");
        return;
    }
    if *cursor >= text.len() {
        return;
    }
    let end = next_custom_action_text_boundary(text, *cursor);
    replace_custom_action_text_range(text, cursor, selection_anchor, *cursor..end, "");
}

fn move_custom_action_text_cursor(
    text: &str,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    direction: CursorDirection,
    extend_selection: bool,
) {
    let next_cursor = match direction {
        CursorDirection::Left => {
            if let Some(range) = custom_action_text_selected_range(*cursor, *selection_anchor) {
                if extend_selection {
                    previous_custom_action_text_boundary(text, *cursor)
                } else {
                    range.start
                }
            } else {
                previous_custom_action_text_boundary(text, *cursor)
            }
        }
        CursorDirection::Right => {
            if let Some(range) = custom_action_text_selected_range(*cursor, *selection_anchor) {
                if extend_selection {
                    next_custom_action_text_boundary(text, *cursor)
                } else {
                    range.end
                }
            } else {
                next_custom_action_text_boundary(text, *cursor)
            }
        }
    };

    if extend_selection && selection_anchor.is_none() {
        *selection_anchor = Some(*cursor);
    }
    if !extend_selection {
        *selection_anchor = None;
    }
    *cursor = next_cursor;
}

fn move_custom_action_text_cursor_to_edge(
    text: &str,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    to_end: bool,
    extend_selection: bool,
) {
    if extend_selection && selection_anchor.is_none() {
        *selection_anchor = Some(*cursor);
    }
    if !extend_selection {
        *selection_anchor = None;
    }
    *cursor = if to_end { text.len() } else { 0 };
}

fn intersect_byte_ranges(
    left: std::ops::Range<usize>,
    right: std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    let start = left.start.max(right.start);
    let end = left.end.min(right.end);
    (start < end).then_some(start..end)
}

fn custom_action_text_visible_range(
    text: &str,
    cursor: usize,
    selection: Option<&std::ops::Range<usize>>,
    max_chars: usize,
) -> std::ops::Range<usize> {
    let boundaries = text
        .char_indices()
        .map(|(idx, _)| idx)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let total_chars = boundaries.len().saturating_sub(1);
    if total_chars <= max_chars {
        return 0..text.len();
    }

    let cursor_char = text[..cursor.min(text.len())].chars().count();
    let mut start_char = cursor_char.saturating_sub(max_chars / 2);
    let mut end_char = (start_char + max_chars).min(total_chars);
    start_char = end_char.saturating_sub(max_chars);

    if cursor_char >= total_chars.saturating_sub(max_chars / 3) {
        end_char = total_chars;
        start_char = total_chars.saturating_sub(max_chars);
    }

    if let Some(selection) = selection {
        let selection_start_char = text[..selection.start.min(text.len())].chars().count();
        let selection_end_char = text[..selection.end.min(text.len())].chars().count();
        if selection_start_char < start_char {
            start_char = selection_start_char;
            end_char = (start_char + max_chars).min(total_chars);
        }
        if selection_end_char > end_char {
            end_char = selection_end_char.min(total_chars);
            start_char = end_char.saturating_sub(max_chars);
        }
    }

    boundaries[start_char]..boundaries[end_char]
}

fn custom_action_text_line_ranges(text: &str) -> Vec<std::ops::Range<usize>> {
    if text.is_empty() {
        return vec![0..0];
    }

    let mut ranges = Vec::new();
    let mut start = 0usize;
    for (idx, ch) in text.char_indices() {
        if ch == '\n' {
            ranges.push(start..idx);
            start = idx + ch.len_utf8();
        }
    }
    ranges.push(start..text.len());
    ranges
}

fn custom_action_text_wrapped_line_ranges(
    text: &str,
    max_chars: usize,
) -> Vec<std::ops::Range<usize>> {
    let max_chars = max_chars.max(1);
    let mut wrapped = Vec::new();

    for line_range in custom_action_text_line_ranges(text) {
        let line = &text[line_range.clone()];
        let mut start = line_range.start;

        while text[start..line_range.end].chars().count() > max_chars {
            let mut fallback_end = line_range.end;
            let mut word_end = None;

            for (char_count, (idx, ch)) in text[start..line_range.end].char_indices().enumerate() {
                if char_count == max_chars {
                    fallback_end = start + idx;
                    break;
                }
                if ch.is_whitespace() && idx > 0 {
                    word_end = Some(start + idx + ch.len_utf8());
                }
            }

            let end = word_end.unwrap_or(fallback_end).max(start);
            if end == start {
                break;
            }
            wrapped.push(start..end);
            start = end;
        }

        if line.is_empty() || start < line_range.end {
            wrapped.push(start..line_range.end);
        }
    }

    wrapped
}

fn provider_value_label(provider: AgentProviderKind) -> &'static str {
    match provider {
        AgentProviderKind::ClaudeCode => "Claude Code",
        AgentProviderKind::Codex => "Codex",
        _ => provider.label(),
    }
}

fn append_current_option(options: &mut Vec<SelectOption>, current: &str) {
    let current = current.trim();
    if current.is_empty() || options.iter().any(|option| option.value == current) {
        return;
    }
    options.push(SelectOption {
        value: current.to_string(),
        label: current.to_string(),
    });
}

fn option_label(options: &[SelectOption], value: &str) -> String {
    options
        .iter()
        .find(|option| option.value == value)
        .map(|option| option.label.clone())
        .unwrap_or_else(|| {
            let value = value.trim();
            if value.is_empty() {
                "Default".to_string()
            } else {
                value.to_string()
            }
        })
}

fn mode_label(value: &str) -> &'static str {
    MODE_OPTIONS
        .iter()
        .find(|(candidate, _)| *candidate == value)
        .map(|(_, label)| *label)
        .unwrap_or("Default")
}

fn provider_model_options(provider: AgentProviderKind, current: &str) -> Vec<SelectOption> {
    let mut options = vec![SelectOption {
        value: DEFAULT_OPTION.to_string(),
        label: "Default".to_string(),
    }];
    let source = match provider {
        AgentProviderKind::Codex => CODEX_MODEL_OPTIONS,
        AgentProviderKind::ClaudeCode => CLAUDE_MODEL_OPTIONS,
        _ => &[],
    };
    options.extend(source.iter().map(|(value, label)| SelectOption {
        value: (*value).to_string(),
        label: (*label).to_string(),
    }));
    append_current_option(&mut options, current);
    options
}

fn provider_traits_options(
    provider: AgentProviderKind,
    model: &str,
    current: &str,
) -> Vec<SelectOption> {
    let mut options = vec![SelectOption {
        value: DEFAULT_OPTION.to_string(),
        label: "Default".to_string(),
    }];
    let source = match provider {
        AgentProviderKind::Codex => CODEX_TRAITS_OPTIONS,
        AgentProviderKind::ClaudeCode => match model {
            "claude-opus-4-7" => CLAUDE_OPUS_47_TRAITS_OPTIONS,
            "claude-opus-4-6" => CLAUDE_OPUS_46_TRAITS_OPTIONS,
            "claude-opus-4-5" => CLAUDE_OPUS_45_TRAITS_OPTIONS,
            "claude-sonnet-4-6" => CLAUDE_SONNET_46_TRAITS_OPTIONS,
            _ => &[],
        },
        _ => &[],
    };
    options.extend(source.iter().map(|(value, label)| SelectOption {
        value: (*value).to_string(),
        label: (*label).to_string(),
    }));
    append_current_option(&mut options, current);
    options
}

impl CustomActionModalState {
    fn new() -> Self {
        Self {
            editing_id: None,
            name: String::new(),
            icon: ProjectActionIcon::Play,
            kind: CustomActionKindDraft::Shell,
            command: String::new(),
            prompt: String::new(),
            provider: AgentProviderKind::Codex,
            model: String::new(),
            traits: String::new(),
            mode: String::new(),
            access: ProjectActionAccess::Default,
            run_on_worktree_create: false,
            save_global_copy: false,
            focused_field: CustomActionField::Name,
            text_cursor: 0,
            text_selection_anchor: None,
            open_dropdown: None,
        }
    }

    fn from_action(action: ProjectAction) -> Self {
        match action.kind {
            ProjectActionKind::Shell { command } => Self {
                editing_id: Some(action.id),
                name: action.name,
                icon: action.icon,
                kind: CustomActionKindDraft::Shell,
                command,
                prompt: String::new(),
                provider: AgentProviderKind::Codex,
                model: String::new(),
                traits: String::new(),
                mode: String::new(),
                access: ProjectActionAccess::Default,
                run_on_worktree_create: action.run_on_worktree_create,
                save_global_copy: action.scope == ProjectActionScope::Global,
                focused_field: CustomActionField::Name,
                text_cursor: 0,
                text_selection_anchor: None,
                open_dropdown: None,
            },
            ProjectActionKind::Agent {
                prompt,
                provider,
                model,
                traits,
                mode,
                access,
            } => Self {
                editing_id: Some(action.id),
                name: action.name,
                icon: action.icon,
                kind: CustomActionKindDraft::Agent,
                command: String::new(),
                prompt,
                provider,
                model: model.unwrap_or_default(),
                traits: traits.unwrap_or_default(),
                mode: mode.unwrap_or_default(),
                access,
                run_on_worktree_create: action.run_on_worktree_create,
                save_global_copy: action.scope == ProjectActionScope::Global,
                focused_field: CustomActionField::Name,
                text_cursor: 0,
                text_selection_anchor: None,
                open_dropdown: None,
            },
        }
    }

    pub(crate) fn focused_text_value(&self) -> Option<&str> {
        match self.focused_field {
            CustomActionField::Name => Some(&self.name),
            CustomActionField::Command => Some(&self.command),
            CustomActionField::Prompt => Some(&self.prompt),
            CustomActionField::Model | CustomActionField::Traits | CustomActionField::Mode => None,
        }
    }

    pub(crate) fn focused_text_parts(
        &mut self,
    ) -> Option<(&mut String, &mut usize, &mut Option<usize>)> {
        match self.focused_field {
            CustomActionField::Name => Some((
                &mut self.name,
                &mut self.text_cursor,
                &mut self.text_selection_anchor,
            )),
            CustomActionField::Command => Some((
                &mut self.command,
                &mut self.text_cursor,
                &mut self.text_selection_anchor,
            )),
            CustomActionField::Prompt => Some((
                &mut self.prompt,
                &mut self.text_cursor,
                &mut self.text_selection_anchor,
            )),
            CustomActionField::Model | CustomActionField::Traits | CustomActionField::Mode => None,
        }
    }

    pub(crate) fn focused_field_preserves_newlines(&self) -> bool {
        self.focused_field == CustomActionField::Prompt
    }
}

impl AnotherOneApp {
    pub(crate) fn open_custom_action_modal(
        &mut self,
        action: Option<ProjectAction>,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_titlebar_dropdowns();
        self.custom_action_modal = Some(
            action
                .map(CustomActionModalState::from_action)
                .unwrap_or_else(CustomActionModalState::new),
        );
        if let Some(state) = self.custom_action_modal.as_mut() {
            state.text_cursor = field_value(state, state.focused_field).len();
            state.text_selection_anchor = None;
        }
        cx.notify();
    }

    pub(crate) fn custom_action_modal_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.custom_action_modal.clone() else {
            return div().id("custom-action-modal-overlay");
        };

        div()
            .id("custom-action-modal-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.50))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.custom_action_modal = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                this.handle_custom_action_modal_key_down(ev, cx);
            }))
            .child(
                div()
                    .w(px(520.))
                    .max_h(relative(0.92))
                    .max_w(relative(0.94))
                    .rounded_lg()
                    .bg(rgb(CARD_BG))
                    .border_1()
                    .border_color(border_col())
                    .shadow_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(self.render_custom_action_header(cx))
                    .child(self.render_custom_action_body(&state, cx))
                    .child(self.render_custom_action_footer(cx)),
            )
    }

    fn render_custom_action_body(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut body = div()
            .id("custom-action-modal-scroll")
            .flex_1()
            .min_h_0()
            .overflow_y_scroll()
            .px(px(20.))
            .pb(px(16.))
            .child(self.render_custom_action_kind_selector(state, cx))
            .child(self.render_custom_action_text_field(
                state,
                CustomActionField::Name,
                "Name",
                "Action name",
                cx,
            ))
            .child(self.render_custom_action_icon_picker(state, cx));

        body = if state.kind == CustomActionKindDraft::Shell {
            body.child(self.render_custom_action_text_field(
                state,
                CustomActionField::Command,
                "Command",
                "npm test",
                cx,
            ))
        } else {
            body.child(self.render_custom_action_provider_picker(state, cx))
                .child(self.render_custom_action_text_field(
                    state,
                    CustomActionField::Prompt,
                    "Prompt",
                    "Ask the agent to do something",
                    cx,
                ))
                .child(self.render_custom_action_model_picker(state, cx))
                .child(self.render_custom_action_traits_picker(state, cx))
                .child(self.render_custom_action_mode_picker(state, cx))
                .child(self.render_custom_action_access_picker(state, cx))
        };

        body.child(self.render_custom_action_toggles(state, cx))
    }

    fn render_custom_action_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let title = if self
            .custom_action_modal
            .as_ref()
            .and_then(|state| state.editing_id.as_ref())
            .is_some()
        {
            "Edit Action"
        } else {
            "Add Action"
        };

        div()
            .flex()
            .items_start()
            .justify_between()
            .px(px(20.))
            .pt(px(20.))
            .pb(px(12.))
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(rems(1.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(title_col())
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .text_color(muted_col())
                            .child("Save project commands or agent prompts for this repository."),
                    ),
            )
            .child(
                div()
                    .id("custom-action-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(24.))
                    .h(px(24.))
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .tooltip(move |_window, cx| Self::action_tooltip_view("Close", cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.custom_action_modal = None;
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__close.svg")
                            .size(px(14.))
                            .text_color(muted_col()),
                    ),
            )
    }

    fn render_custom_action_kind_selector(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let kind = state.kind;
        div()
            .mt(px(4.))
            .flex()
            .rounded_md()
            .bg(subtle_bg())
            .p(px(3.))
            .gap(px(2.))
            .child(self.render_custom_action_kind_option(
                CustomActionKindDraft::Shell,
                "Shell",
                kind,
                cx,
            ))
            .child(self.render_custom_action_kind_option(
                CustomActionKindDraft::Agent,
                "Agent",
                kind,
                cx,
            ))
    }

    fn render_custom_action_kind_option(
        &self,
        option: CustomActionKindDraft,
        label: &'static str,
        selected: CustomActionKindDraft,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(SharedString::from(format!("custom-action-kind-{label}")))
            .flex_1()
            .h(px(32.))
            .rounded(px(5.))
            .flex()
            .items_center()
            .justify_center()
            .cursor_pointer()
            .bg(if selected == option {
                active_bg()
            } else {
                gpui::transparent_black()
            })
            .hover(move |s| s.bg(hover_bg()))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    if let Some(state) = this.custom_action_modal.as_mut() {
                        state.kind = option;
                        state.focused_field = if option == CustomActionKindDraft::Shell {
                            CustomActionField::Command
                        } else {
                            CustomActionField::Prompt
                        };
                        state.text_cursor = field_value(state, state.focused_field).len();
                        state.text_selection_anchor = None;
                        state.open_dropdown = None;
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(body_col())
                    .child(label),
            )
    }

    fn render_custom_action_text_field(
        &self,
        state: &CustomActionModalState,
        field: CustomActionField,
        label: &'static str,
        placeholder: &'static str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let focused = state.focused_field == field;
        let value = field_value(state, field).to_string();
        let is_prompt = field == CustomActionField::Prompt;
        let selection = focused.then(|| {
            custom_action_text_selected_range(state.text_cursor, state.text_selection_anchor)
        });
        let is_empty = value.is_empty();

        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child(label),
            )
            .child(
                div()
                    .id(SharedString::from(format!("custom-action-field-{label}")))
                    .min_h(if is_prompt { px(190.) } else { px(38.) })
                    .rounded_md()
                    .border_1()
                    .border_color(if focused {
                        hsla(220. / 360., 0.55, 0.60, 1.)
                    } else {
                        border_col()
                    })
                    .bg(subtle_bg())
                    .flex()
                    .when(!is_prompt, |d| d.items_center())
                    .when(is_prompt, |d| d.items_start())
                    .px(px(12.))
                    .when(is_prompt, |d| d.py(px(10.)))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                            this.focus_handle.focus(window);
                            if let Some(state) = this.custom_action_modal.as_mut() {
                                state.focused_field = field;
                                let value_len = field_value(state, field).len();
                                state.text_cursor = value_len;
                                state.text_selection_anchor = None;
                                state.open_dropdown = None;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(Self::render_custom_action_text_field_content(
                        value,
                        placeholder,
                        focused,
                        state.text_cursor,
                        selection.flatten(),
                        is_empty,
                        is_prompt,
                    )),
            )
    }

    fn render_custom_action_text_field_content(
        value: String,
        placeholder: &'static str,
        focused: bool,
        cursor: usize,
        selection: Option<std::ops::Range<usize>>,
        is_empty: bool,
        multiline: bool,
    ) -> impl IntoElement {
        let cursor = cursor.min(value.len());
        let selection =
            selection.map(|range| range.start.min(value.len())..range.end.min(value.len()));

        if multiline {
            return Self::render_custom_action_multiline_text_content(
                value,
                placeholder,
                focused,
                cursor,
                selection,
                is_empty,
            );
        }

        if is_empty {
            return div()
                .flex()
                .items_center()
                .min_w(px(0.))
                .overflow_hidden()
                .gap(px(0.))
                .text_size(rems(13. / 16.))
                .child(if focused {
                    div().w(px(1.)).h(px(18.)).mr(px(1.)).bg(title_col())
                } else {
                    div().w(px(0.))
                })
                .child(div().text_color(muted_col()).child(placeholder));
        }

        let selected = selection.filter(|range| range.start < range.end);
        let visible_range = custom_action_text_visible_range(&value, cursor, selected.as_ref(), 56);
        let visible_start = visible_range.start;
        let visible_value = value[visible_range.clone()].to_string();
        let local_cursor = cursor
            .saturating_sub(visible_start)
            .min(visible_value.len());
        let visible_selection = selected
            .as_ref()
            .and_then(|range| intersect_byte_ranges(range.clone(), visible_range.clone()))
            .map(|range| range.start - visible_start..range.end - visible_start);

        let selected_contains_cursor = visible_selection
            .as_ref()
            .is_some_and(|range| range.start <= local_cursor && local_cursor <= range.end);

        let prefix_end = visible_selection
            .as_ref()
            .map_or(local_cursor, |range| range.start.min(local_cursor));
        let selected_end = visible_selection
            .as_ref()
            .map_or(local_cursor, |range| range.end.min(visible_value.len()));
        let prefix = visible_value[..prefix_end.min(visible_value.len())].to_string();
        let middle = visible_selection
            .as_ref()
            .map(|range| visible_value[range.clone()].to_string())
            .unwrap_or_default();
        let suffix_start = if selected_contains_cursor {
            selected_end
        } else {
            local_cursor.min(visible_value.len())
        };
        let between = if visible_selection
            .as_ref()
            .is_some_and(|range| range.end < local_cursor)
        {
            visible_value[selected_end..local_cursor.min(visible_value.len())].to_string()
        } else {
            String::new()
        };
        let trailing = visible_value[suffix_start..].to_string();

        let mut row = div()
            .flex()
            .items_center()
            .min_w(px(0.))
            .overflow_hidden()
            .gap(px(0.))
            .text_size(rems(13. / 16.));

        if visible_range.start > 0 {
            row = row.child(div().text_color(muted_col()).child("..."));
        }
        if !prefix.is_empty() {
            row = row.child(div().text_color(title_col()).child(prefix));
        }
        if visible_selection
            .as_ref()
            .is_some_and(|range| range.end < local_cursor)
            && !middle.is_empty()
        {
            row = row.child(
                div()
                    .px(px(1.))
                    .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                    .text_color(title_col())
                    .child(middle.clone()),
            );
        }
        if focused {
            row = row.child(div().w(px(1.)).h(px(18.)).bg(title_col()));
        }
        if selected_contains_cursor && !middle.is_empty() {
            row = row.child(
                div()
                    .px(px(1.))
                    .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                    .text_color(title_col())
                    .child(middle.clone()),
            );
        }
        if !between.is_empty() {
            row = row.child(div().text_color(title_col()).child(between));
        }
        if visible_selection
            .as_ref()
            .is_some_and(|range| range.start > local_cursor)
            && !middle.is_empty()
        {
            row = row.child(
                div()
                    .px(px(1.))
                    .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                    .text_color(title_col())
                    .child(middle),
            );
        }
        if !trailing.is_empty() {
            row = row.child(div().text_color(title_col()).child(trailing));
        }
        if visible_range.end < value.len() {
            row = row.child(div().text_color(muted_col()).child("..."));
        }

        row
    }

    fn render_custom_action_multiline_text_content(
        value: String,
        placeholder: &'static str,
        focused: bool,
        cursor: usize,
        selection: Option<std::ops::Range<usize>>,
        is_empty: bool,
    ) -> gpui::Div {
        let selected = selection.filter(|range| range.start < range.end);
        let mut column = div()
            .w_full()
            .min_h(px(170.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .overflow_hidden()
            .text_size(rems(13. / 16.))
            .line_height(rems(18. / 16.));

        if is_empty {
            return column.child(
                div()
                    .min_h(px(18.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(0.))
                    .child(if focused {
                        div().w(px(1.)).h(px(18.)).mr(px(1.)).bg(title_col())
                    } else {
                        div().w(px(0.))
                    })
                    .child(div().text_color(muted_col()).child(placeholder)),
            );
        }

        let line_ranges = custom_action_text_wrapped_line_ranges(&value, 64);
        let last_line_index = line_ranges.len().saturating_sub(1);
        for (line_index, line_range) in line_ranges.iter().cloned().enumerate() {
            let line_text = &value[line_range.clone()];
            let next_line_start = line_ranges
                .get(line_index + 1)
                .map_or(value.len() + 1, |range| range.start);
            let contains_cursor = line_range.start <= cursor
                && (cursor < line_range.end
                    || (cursor <= line_range.end
                        && (line_range.is_empty()
                            || line_index == last_line_index
                            || next_line_start != line_range.end)));
            let local_cursor = if contains_cursor {
                Some(cursor - line_range.start)
            } else {
                None
            };
            let visible_selection = selected
                .as_ref()
                .and_then(|range| intersect_byte_ranges(range.clone(), line_range.clone()))
                .map(|range| range.start - line_range.start..range.end - line_range.start);

            let mut row = div()
                .min_h(px(18.))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(0.))
                .overflow_hidden();

            match (visible_selection, focused.then_some(local_cursor).flatten()) {
                (Some(range), _) => {
                    let prefix = &line_text[..range.start];
                    let middle = &line_text[range.clone()];
                    let suffix = &line_text[range.end..];
                    if !prefix.is_empty() {
                        row = row.child(div().text_color(title_col()).child(prefix.to_string()));
                    }
                    row = row.child(
                        div()
                            .px(px(1.))
                            .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                            .text_color(title_col())
                            .child(if middle.is_empty() {
                                " ".to_string()
                            } else {
                                middle.to_string()
                            }),
                    );
                    if !suffix.is_empty() {
                        row = row.child(div().text_color(title_col()).child(suffix.to_string()));
                    }
                }
                (None, Some(local_cursor)) => {
                    let local_cursor = local_cursor.min(line_text.len());
                    let prefix = &line_text[..local_cursor];
                    let suffix = &line_text[local_cursor..];
                    if !prefix.is_empty() {
                        row = row.child(div().text_color(title_col()).child(prefix.to_string()));
                    }
                    row = row.child(div().w(px(1.)).h(px(18.)).bg(title_col()));
                    if !suffix.is_empty() {
                        row = row.child(div().text_color(title_col()).child(suffix.to_string()));
                    }
                    if prefix.is_empty() && suffix.is_empty() {
                        row = row.child(div().text_color(title_col().opacity(0.)).child(" "));
                    }
                }
                (None, None) => {
                    row = row.child(
                        div()
                            .text_color(if line_text.is_empty() {
                                title_col().opacity(0.)
                            } else {
                                title_col()
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

    fn render_custom_action_icon_picker(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut row = div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Icon"),
            )
            .child(div().flex().gap(px(6.)));
        let mut icons = div().flex().gap(px(6.));
        for icon in ProjectActionIcon::ALL {
            icons = icons.child(
                div()
                    .id(SharedString::from(format!(
                        "custom-action-icon-{}",
                        icon.label()
                    )))
                    .w(px(34.))
                    .h(px(30.))
                    .rounded_md()
                    .border_1()
                    .border_color(if icon == state.icon {
                        hsla(220. / 360., 0.55, 0.60, 1.)
                    } else {
                        border_col()
                    })
                    .bg(if icon == state.icon {
                        active_bg()
                    } else {
                        subtle_bg()
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .tooltip(move |_window, cx| Self::action_tooltip_view(icon.label(), cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            if let Some(state) = this.custom_action_modal.as_mut() {
                                state.icon = icon;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        svg()
                            .path(icon.icon_path())
                            .size(px(14.))
                            .text_color(title_col()),
                    ),
            );
        }
        row = row.child(icons);
        row
    }

    fn render_custom_action_provider_picker(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Provider"),
            )
            .child(self.render_custom_action_dropdown(
                CustomActionDropdown::Provider,
                state.open_dropdown == Some(CustomActionDropdown::Provider),
                SharedString::from(provider_value_label(state.provider)),
                self.provider_option_elements(state.provider, cx),
                cx,
            ))
    }

    fn render_custom_action_model_picker(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let options = provider_model_options(state.provider, &state.model);
        let selected_label = option_label(&options, &state.model);
        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Model"),
            )
            .child(self.render_custom_action_dropdown(
                CustomActionDropdown::Model,
                state.open_dropdown == Some(CustomActionDropdown::Model),
                SharedString::from(selected_label),
                self.string_option_elements(CustomActionDropdown::Model, options, cx),
                cx,
            ))
    }

    fn render_custom_action_traits_picker(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let options = provider_traits_options(state.provider, &state.model, &state.traits);
        let selected_label = option_label(&options, &state.traits);
        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Traits / effort"),
            )
            .child(self.render_custom_action_dropdown(
                CustomActionDropdown::Traits,
                state.open_dropdown == Some(CustomActionDropdown::Traits),
                SharedString::from(selected_label),
                self.string_option_elements(CustomActionDropdown::Traits, options, cx),
                cx,
            ))
    }

    fn render_custom_action_mode_picker(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let selected_label = mode_label(&state.mode);
        let mut options = vec![SelectOption {
            value: DEFAULT_OPTION.to_string(),
            label: "Default".to_string(),
        }];
        options.extend(MODE_OPTIONS.iter().map(|(value, label)| SelectOption {
            value: (*value).to_string(),
            label: (*label).to_string(),
        }));
        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Mode"),
            )
            .child(self.render_custom_action_dropdown(
                CustomActionDropdown::Mode,
                state.open_dropdown == Some(CustomActionDropdown::Mode),
                SharedString::from(selected_label),
                self.string_option_elements(CustomActionDropdown::Mode, options, cx),
                cx,
            ))
    }

    fn render_custom_action_dropdown(
        &self,
        dropdown: CustomActionDropdown,
        open: bool,
        selected_label: SharedString,
        option_elements: Vec<AnyElement>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut container = div().flex().flex_col().gap(px(4.)).child(
            div()
                .h(px(38.))
                .rounded_md()
                .border_1()
                .border_color(if open {
                    hsla(220. / 360., 0.55, 0.60, 1.)
                } else {
                    border_col()
                })
                .bg(subtle_bg())
                .flex()
                .items_center()
                .justify_between()
                .px(px(12.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg()))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        if let Some(state) = this.custom_action_modal.as_mut() {
                            state.open_dropdown = if state.open_dropdown == Some(dropdown) {
                                None
                            } else {
                                Some(dropdown)
                            };
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .min_w_0()
                        .text_size(rems(13. / 16.))
                        .text_color(title_col())
                        .truncate()
                        .child(selected_label),
                )
                .child(
                    svg()
                        .path("assets/icons/icons__chevron-down.svg")
                        .size(px(14.))
                        .text_color(muted_col()),
                ),
        );

        if open {
            let mut list = div()
                .rounded_md()
                .border_1()
                .border_color(border_col())
                .bg(rgb(CARD_BG))
                .shadow_md()
                .overflow_hidden();
            for option in option_elements {
                list = list.child(option);
            }
            container = container.child(list);
        }

        container
    }

    fn string_option_elements(
        &self,
        dropdown: CustomActionDropdown,
        options: Vec<SelectOption>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        options
            .into_iter()
            .map(|option| {
                let value = option.value.clone();
                let label = option.label.clone();
                div()
                    .h(px(32.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            if let Some(state) = this.custom_action_modal.as_mut() {
                                match dropdown {
                                    CustomActionDropdown::Model => {
                                        state.model = value.clone();
                                        state.traits.clear();
                                    }
                                    CustomActionDropdown::Traits => state.traits = value.clone(),
                                    CustomActionDropdown::Mode => state.mode = value.clone(),
                                    CustomActionDropdown::Provider
                                    | CustomActionDropdown::Access => {}
                                }
                                state.open_dropdown = None;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .text_color(body_col())
                            .child(SharedString::from(label)),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn provider_option_elements(
        &self,
        selected_provider: AgentProviderKind,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        [AgentProviderKind::Codex, AgentProviderKind::ClaudeCode]
            .into_iter()
            .map(|provider| {
                let selected = selected_provider == provider;
                let icon_path = action_provider_agent(provider)
                    .map(|agent| agent.icon)
                    .unwrap_or("assets/icons/action__agent.svg");
                div()
                    .h(px(36.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .cursor_pointer()
                    .bg(if selected {
                        active_bg()
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            if let Some(state) = this.custom_action_modal.as_mut() {
                                if state.provider != provider {
                                    state.provider = provider;
                                    state.model.clear();
                                    state.traits.clear();
                                    state.mode.clear();
                                }
                                state.open_dropdown = None;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(branded_icon(icon_path, 18., Some(title_col())))
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .text_color(body_col())
                            .child(provider_value_label(provider)),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn render_custom_action_access_picker(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .mt(px(14.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Access"),
            )
            .child(self.render_custom_action_dropdown(
                CustomActionDropdown::Access,
                state.open_dropdown == Some(CustomActionDropdown::Access),
                SharedString::from(state.access.label()),
                self.access_option_elements(state.access, cx),
                cx,
            ))
    }

    fn access_option_elements(
        &self,
        selected_access: ProjectActionAccess,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        ACCESS_OPTIONS
            .iter()
            .map(|(access, label)| {
                let access = *access;
                let selected = selected_access == access;
                div()
                    .h(px(32.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .bg(if selected {
                        active_bg()
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            if let Some(state) = this.custom_action_modal.as_mut() {
                                state.access = access;
                                state.open_dropdown = None;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .text_color(body_col())
                            .child(*label),
                    )
                    .into_any_element()
            })
            .collect()
    }

    fn render_custom_action_toggles(
        &self,
        state: &CustomActionModalState,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .mt(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(self.render_custom_action_toggle(
                "custom-action-auto-run",
                "Run automatically on worktree creation",
                state.run_on_worktree_create,
                |state| state.run_on_worktree_create = !state.run_on_worktree_create,
                cx,
            ))
            .child(self.render_custom_action_toggle(
                "custom-action-global",
                "Global action",
                state.save_global_copy,
                |state| state.save_global_copy = !state.save_global_copy,
                cx,
            ))
    }

    fn render_custom_action_toggle(
        &self,
        id: &'static str,
        label: &'static str,
        checked: bool,
        toggle: fn(&mut CustomActionModalState),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .h(px(34.))
            .flex()
            .items_center()
            .gap(px(10.))
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg()))
            .rounded_md()
            .px(px(8.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    if let Some(state) = this.custom_action_modal.as_mut() {
                        toggle(state);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .w(px(18.))
                    .h(px(18.))
                    .rounded(px(4.))
                    .border_1()
                    .border_color(if checked {
                        hsla(220. / 360., 0.55, 0.55, 1.)
                    } else {
                        border_col()
                    })
                    .bg(if checked {
                        hsla(220. / 360., 0.55, 0.55, 1.)
                    } else {
                        gpui::transparent_black()
                    })
                    .flex()
                    .items_center()
                    .justify_center()
                    .when(checked, |container| {
                        container.child(
                            svg()
                                .path("assets/icons/icons__check.svg")
                                .size(px(12.))
                                .text_color(gpui::white()),
                        )
                    }),
            )
            .child(
                div()
                    .text_size(rems(13. / 16.))
                    .text_color(body_col())
                    .child(label),
            )
    }

    fn render_custom_action_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let editing_action_id = self
            .custom_action_modal
            .as_ref()
            .and_then(|state| state.editing_id.clone());

        div()
            .flex()
            .items_center()
            .justify_between()
            .border_t_1()
            .border_color(border_col())
            .px(px(20.))
            .py(px(14.))
            .child(if let Some(action_id) = editing_action_id {
                div()
                    .h(px(32.))
                    .px(px(10.))
                    .rounded_md()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            let Some(project_id) = this.active_open_in_project_id(cx) else {
                                this.show_error_toast("No active project is selected.", cx);
                                cx.stop_propagation();
                                return;
                            };
                            if this
                                .project_store
                                .delete_project_action(&project_id, &action_id)
                            {
                                this.custom_action_modal = None;
                                this.show_success_toast("Action deleted.", cx);
                                cx.notify();
                            }
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__trash.svg")
                            .size(px(13.))
                            .text_color(hsla(0., 0.72, 0.62, 1.)),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(hsla(0., 0.72, 0.62, 1.))
                            .child("Delete"),
                    )
                    .into_any_element()
            } else {
                div().into_any_element()
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .h(px(32.))
                            .px(px(14.))
                            .rounded_md()
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg()))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.custom_action_modal = None;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .text_size(rems(13. / 16.))
                                    .text_color(body_col())
                                    .child("Cancel"),
                            ),
                    )
                    .child(
                        div()
                            .h(px(32.))
                            .px(px(14.))
                            .rounded_md()
                            .bg(hsla(220. / 360., 0.55, 0.55, 1.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(move |s| s.bg(hsla(220. / 360., 0.55, 0.60, 1.)))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.submit_custom_action_modal(cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div()
                                    .text_size(rems(13. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(gpui::white())
                                    .child("Save"),
                            ),
                    ),
            )
    }

    pub(crate) fn handle_custom_action_modal_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        if self.custom_action_modal.is_none() {
            return;
        }
        cx.stop_propagation();
        match ev.keystroke.key.as_str() {
            "escape" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    if state.open_dropdown.take().is_some() {
                        cx.notify();
                        return;
                    }
                }
                self.custom_action_modal = None;
                cx.notify();
            }
            "enter" if ev.keystroke.modifiers.platform => {
                self.submit_custom_action_modal(cx);
            }
            "enter" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    if state.focused_field == CustomActionField::Prompt {
                        if let Some((value, cursor, selection_anchor)) = state.focused_text_parts()
                        {
                            insert_custom_action_text(value, cursor, selection_anchor, "\n");
                            cx.notify();
                        }
                    }
                }
            }
            "tab" => {
                self.focus_next_custom_action_field(ev.keystroke.modifiers.shift, cx);
            }
            "backspace" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    let modifiers = ev.keystroke.modifiers;
                    if let Some((value, cursor, selection_anchor)) = state.focused_text_parts() {
                        if modifiers.platform {
                            delete_custom_action_text_to_start(value, cursor, selection_anchor);
                        } else if modifiers.alt {
                            delete_custom_action_text_word_backward(
                                value,
                                cursor,
                                selection_anchor,
                            );
                        } else {
                            delete_custom_action_text_backward(value, cursor, selection_anchor);
                        }
                        cx.notify();
                    }
                }
            }
            "delete" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    if let Some((value, cursor, selection_anchor)) = state.focused_text_parts() {
                        delete_custom_action_text_forward(value, cursor, selection_anchor);
                        cx.notify();
                    }
                }
            }
            "left" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    let extend = ev.keystroke.modifiers.shift;
                    if let Some((value, cursor, selection_anchor)) = state.focused_text_parts() {
                        move_custom_action_text_cursor(
                            value,
                            cursor,
                            selection_anchor,
                            CursorDirection::Left,
                            extend,
                        );
                        cx.notify();
                    }
                }
            }
            "right" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    let extend = ev.keystroke.modifiers.shift;
                    if let Some((value, cursor, selection_anchor)) = state.focused_text_parts() {
                        move_custom_action_text_cursor(
                            value,
                            cursor,
                            selection_anchor,
                            CursorDirection::Right,
                            extend,
                        );
                        cx.notify();
                    }
                }
            }
            "home" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    let extend = ev.keystroke.modifiers.shift;
                    if let Some((value, cursor, selection_anchor)) = state.focused_text_parts() {
                        move_custom_action_text_cursor_to_edge(
                            value,
                            cursor,
                            selection_anchor,
                            false,
                            extend,
                        );
                        cx.notify();
                    }
                }
            }
            "end" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    let extend = ev.keystroke.modifiers.shift;
                    if let Some((value, cursor, selection_anchor)) = state.focused_text_parts() {
                        move_custom_action_text_cursor_to_edge(
                            value,
                            cursor,
                            selection_anchor,
                            true,
                            extend,
                        );
                        cx.notify();
                    }
                }
            }
            _ => {
                let modifiers = ev.keystroke.modifiers;
                if let Some(state) = self.custom_action_modal.as_mut() {
                    let focused_field = state.focused_field;
                    if let Some((value, cursor, selection_anchor)) = state.focused_text_parts() {
                        if modifiers.platform && ev.keystroke.key.as_str() == "a" {
                            *cursor = value.len();
                            *selection_anchor = Some(0);
                            cx.notify();
                            return;
                        }
                        if modifiers.platform && ev.keystroke.key.as_str() == "c" {
                            if let Some(range) =
                                custom_action_text_selected_range(*cursor, *selection_anchor)
                            {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    value[range].to_string(),
                                ));
                            }
                            return;
                        }
                        if modifiers.platform && ev.keystroke.key.as_str() == "x" {
                            if let Some(range) =
                                custom_action_text_selected_range(*cursor, *selection_anchor)
                            {
                                cx.write_to_clipboard(ClipboardItem::new_string(
                                    value[range.clone()].to_string(),
                                ));
                                replace_custom_action_text_range(
                                    value,
                                    cursor,
                                    selection_anchor,
                                    range,
                                    "",
                                );
                                cx.notify();
                            }
                            return;
                        }
                        if modifiers.platform && ev.keystroke.key.as_str() == "v" {
                            if let Some(text) = cx
                                .read_from_clipboard()
                                .and_then(|item| item.text())
                                .map(|text| sanitize_custom_action_field_input(focused_field, text))
                            {
                                insert_custom_action_text(value, cursor, selection_anchor, &text);
                                cx.notify();
                            }
                            return;
                        }

                        if modifiers.platform || modifiers.control || modifiers.alt {
                            return;
                        }
                        if let Some(key_char) = ev.keystroke.key_char.as_deref() {
                            insert_custom_action_text(value, cursor, selection_anchor, key_char);
                            cx.notify();
                        }
                    }
                }
            }
        }
    }

    fn focus_next_custom_action_field(&mut self, backwards: bool, cx: &mut Context<Self>) {
        let Some(state) = self.custom_action_modal.as_mut() else {
            return;
        };
        let fields: &[CustomActionField] = if state.kind == CustomActionKindDraft::Shell {
            &[CustomActionField::Name, CustomActionField::Command]
        } else {
            &[
                CustomActionField::Name,
                CustomActionField::Prompt,
                CustomActionField::Model,
                CustomActionField::Traits,
                CustomActionField::Mode,
            ]
        };
        let current = fields
            .iter()
            .position(|field| *field == state.focused_field)
            .unwrap_or(0);
        let next = if backwards {
            (current + fields.len() - 1) % fields.len()
        } else {
            (current + 1) % fields.len()
        };
        state.focused_field = fields[next];
        state.text_cursor = field_value(state, state.focused_field).len();
        state.text_selection_anchor = None;
        cx.notify();
    }

    fn submit_custom_action_modal(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.custom_action_modal.clone() else {
            return;
        };
        let Some(project_id) = self.active_open_in_project_id(cx) else {
            self.show_error_toast("No active project is selected.", cx);
            return;
        };
        let name = state.name.trim().to_string();
        if name.is_empty() {
            self.show_error_toast("Custom actions need a name.", cx);
            return;
        }
        let id = state
            .editing_id
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
        let kind = match state.kind {
            CustomActionKindDraft::Shell => {
                let command = state.command.trim().to_string();
                if command.is_empty() {
                    self.show_error_toast("Shell actions need a command.", cx);
                    return;
                }
                ProjectActionKind::Shell { command }
            }
            CustomActionKindDraft::Agent => {
                let prompt = state.prompt.trim().to_string();
                if prompt.is_empty() {
                    self.show_error_toast("Agent actions need a prompt.", cx);
                    return;
                }
                ProjectActionKind::Agent {
                    prompt,
                    provider: state.provider,
                    model: trim_to_option(&state.model),
                    traits: trim_to_option(&state.traits),
                    mode: trim_to_option(&state.mode),
                    access: state.access,
                }
            }
        };
        let action = ProjectAction {
            id,
            name,
            icon: state.icon,
            run_on_worktree_create: state.run_on_worktree_create,
            scope: ProjectActionScope::Project,
            kind,
        };

        let saved =
            self.project_store
                .upsert_project_action(&project_id, action, state.save_global_copy);

        match saved {
            Ok(()) => {
                self.custom_action_modal = None;
                self.show_success_toast("Action saved.", cx);
                cx.notify();
            }
            Err(error) => self.show_error_toast(error, cx),
        }
    }
}
