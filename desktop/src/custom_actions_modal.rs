//! Add/Edit project custom action modal.

use gpui::{
    div, hsla, prelude::*, px, relative, rems, rgb, svg, AnyElement, Context, KeyDownEvent,
    MouseButton, MouseDownEvent, SharedString,
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

fn field_mut(state: &mut CustomActionModalState, field: CustomActionField) -> &mut String {
    match field {
        CustomActionField::Name => &mut state.name,
        CustomActionField::Command => &mut state.command,
        CustomActionField::Prompt => &mut state.prompt,
        CustomActionField::Model => &mut state.model,
        CustomActionField::Traits => &mut state.traits,
        CustomActionField::Mode => &mut state.mode,
    }
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

fn action_provider_agent(provider: AgentProviderKind) -> Option<&'static crate::agents::AgentDef> {
    AGENTS.iter().find(|agent| agent.provider == Some(provider))
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
                open_dropdown: None,
            },
        }
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
        let is_empty = value.is_empty();
        let display = if is_empty {
            SharedString::from(placeholder)
        } else {
            SharedString::from(value)
        };
        let text_color = if is_empty { muted_col() } else { title_col() };

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
                    .h(px(38.))
                    .rounded_md()
                    .border_1()
                    .border_color(if focused {
                        hsla(220. / 360., 0.55, 0.60, 1.)
                    } else {
                        border_col()
                    })
                    .bg(subtle_bg())
                    .flex()
                    .items_center()
                    .px(px(12.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                            this.focus_handle.focus(window);
                            if let Some(state) = this.custom_action_modal.as_mut() {
                                state.focused_field = field;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .text_color(text_color)
                            .truncate()
                            .child(display),
                    ),
            )
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
            "tab" => {
                self.focus_next_custom_action_field(ev.keystroke.modifiers.shift, cx);
            }
            "backspace" => {
                if let Some(state) = self.custom_action_modal.as_mut() {
                    let field = state.focused_field;
                    if matches!(
                        field,
                        CustomActionField::Model
                            | CustomActionField::Traits
                            | CustomActionField::Mode
                    ) {
                        return;
                    }
                    let value = field_mut(state, field);
                    value.pop();
                    cx.notify();
                }
            }
            _ => {
                if ev.keystroke.modifiers.platform
                    || ev.keystroke.modifiers.control
                    || ev.keystroke.modifiers.alt
                {
                    return;
                }
                if let Some(key_char) = ev.keystroke.key_char.as_deref() {
                    if let Some(state) = self.custom_action_modal.as_mut() {
                        let field = state.focused_field;
                        if matches!(
                            field,
                            CustomActionField::Model
                                | CustomActionField::Traits
                                | CustomActionField::Mode
                        ) {
                            return;
                        }
                        field_mut(state, field).push_str(key_char);
                        cx.notify();
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
