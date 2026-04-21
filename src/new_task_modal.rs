//! "New Task" modal dialog shown when clicking the "+" button on a project.

use std::collections::HashSet;

use gpui::{
    div, hsla, prelude::*, px, relative, rems, rgb, svg, ClipboardItem, Context, KeyDownEvent,
    MouseButton, MouseDownEvent, SharedString,
};
use uuid::Uuid;

use crate::agents::{AGENTS, DEFAULT_AGENT_ID};
use crate::app::AnotherOneApp;

#[derive(Clone)]
pub(crate) struct NewTaskModalState {
    pub project_id: String,
    pub project_name: String,
    pub task_name: String,
    pub generated_task_name: String,
    pub source_branch: String,
    pub branch_dropdown_open: bool,
    pub agent_dropdown_open: bool,
    pub selected_agents: HashSet<String>,
    /// true = Worktree, false = Direct.
    pub worktree_mode: bool,
    pub task_name_focused: bool,
    pub task_name_cursor: usize,
    pub task_name_selection_anchor: Option<usize>,
    pub advanced_expanded: bool,
    pub submitting: bool,
}

const CARD_BG: u32 = 0x2b2d31;
const TITLE_COL: (f32, f32, f32, f32) = (0., 0., 0.92, 1.);
const BODY_COL: (f32, f32, f32, f32) = (0., 0., 0.78, 1.);
const MUTED_COL: (f32, f32, f32, f32) = (0., 0., 0.58, 1.);
const PLACEHOLDER_COL: (f32, f32, f32, f32) = (0., 0., 0.38, 1.);
const DANGER_COL: (f32, f32, f32, f32) = (0.0, 0.78, 0.68, 1.);

fn title_col() -> gpui::Hsla {
    hsla(TITLE_COL.0, TITLE_COL.1, TITLE_COL.2, TITLE_COL.3)
}

fn body_col() -> gpui::Hsla {
    hsla(BODY_COL.0, BODY_COL.1, BODY_COL.2, BODY_COL.3)
}

fn muted_col() -> gpui::Hsla {
    hsla(MUTED_COL.0, MUTED_COL.1, MUTED_COL.2, MUTED_COL.3)
}

fn placeholder_col() -> gpui::Hsla {
    hsla(
        PLACEHOLDER_COL.0,
        PLACEHOLDER_COL.1,
        PLACEHOLDER_COL.2,
        PLACEHOLDER_COL.3,
    )
}

fn danger_col() -> gpui::Hsla {
    hsla(DANGER_COL.0, DANGER_COL.1, DANGER_COL.2, DANGER_COL.3)
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

struct SourceBranchSectionProps<'a> {
    project_name: SharedString,
    selected_branch: SharedString,
    current_branch: SharedString,
    branches: &'a [String],
    worktree_mode: bool,
    dropdown_open: bool,
    submitting: bool,
}

impl AnotherOneApp {
    pub(crate) fn open_new_task_modal(&mut self, project_id: &str) {
        let Some(project) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return;
        };

        let source_branch = self
            .project_store
            .primary_branch_for_project(&project.id, true)
            .map(|branch| branch.name)
            .unwrap_or_default();
        self.open_new_task_modal_with_branch(project_id, &source_branch);
    }

    pub(crate) fn open_new_task_modal_with_branch(
        &mut self,
        project_id: &str,
        source_branch: &str,
    ) {
        let Some(project) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return;
        };

        let mut selected_agents = HashSet::new();
        selected_agents.insert(DEFAULT_AGENT_ID.to_string());

        self.new_task_modal = Some(NewTaskModalState {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            task_name: String::new(),
            generated_task_name: generate_task_name(),
            source_branch: source_branch.to_string(),
            branch_dropdown_open: false,
            agent_dropdown_open: false,
            selected_agents,
            worktree_mode: true,
            task_name_focused: true,
            task_name_cursor: 0,
            task_name_selection_anchor: None,
            advanced_expanded: false,
            submitting: false,
        });
    }

    pub(crate) fn new_task_modal_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(ref state) = self.new_task_modal else {
            return div().id("new-task-modal-overlay");
        };

        let project = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == state.project_id);

        let available_branches = project
            .map(|project| self.project_store.branch_names(&project.id))
            .unwrap_or_default();
        let current_branch = project
            .and_then(|project| self.project_store.current_branch_name(&project.id))
            .unwrap_or_else(|| state.source_branch.clone());

        let project_name: SharedString = state.project_name.clone().into();
        let selected_branch: SharedString = state.source_branch.clone().into();
        let current_branch: SharedString = current_branch.into();
        let task_name: SharedString = state.task_name.clone().into();
        let generated_task_name: SharedString = state.generated_task_name.clone().into();
        let worktree_mode = state.worktree_mode;
        let branch_dropdown_open = state.branch_dropdown_open;
        let agent_dropdown_open = state.agent_dropdown_open;
        let selected_agents = state.selected_agents.clone();
        let task_name_focused = state.task_name_focused;
        let task_name_cursor = state.task_name_cursor;
        let task_name_selection = selected_task_name_range(state);
        let advanced_expanded = state.advanced_expanded;
        let submitting = state.submitting;

        div()
            .id("new-task-modal-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.50))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.dismiss_new_task_modal(cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                this.handle_new_task_modal_key_down(ev, cx);
            }))
            .child(
                div()
                    .w(px(440.))
                    .max_h(relative(0.9))
                    .rounded_lg()
                    .bg(rgb(CARD_BG))
                    .border_1()
                    .border_color(border_col())
                    .shadow_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(Self::render_header(submitting, cx))
                    .child(
                        div()
                            .id("new-task-modal-scroll")
                            .flex_1()
                            .min_h_0()
                            .overflow_y_scroll()
                            .child(Self::render_source_branch(
                                SourceBranchSectionProps {
                                    project_name,
                                    selected_branch,
                                    current_branch,
                                    branches: &available_branches,
                                    worktree_mode,
                                    dropdown_open: branch_dropdown_open,
                                    submitting,
                                },
                                cx,
                            ))
                            .child(Self::render_task_name_field(
                                task_name,
                                generated_task_name,
                                task_name_focused,
                                task_name_cursor,
                                task_name_selection,
                                submitting,
                                cx,
                            ))
                            .child(self.render_agent_selector(
                                agent_dropdown_open,
                                &selected_agents,
                                submitting,
                                cx,
                            ))
                            .child(Self::render_workspace_toggle(worktree_mode, submitting, cx))
                            .child(Self::render_advanced_options(
                                advanced_expanded,
                                submitting,
                                cx,
                            )),
                    )
                    .child(Self::render_footer(submitting, cx)),
            )
    }

    fn dismiss_new_task_modal(&mut self, cx: &mut Context<Self>) {
        if self
            .new_task_modal
            .as_ref()
            .is_some_and(|state| state.submitting)
        {
            return;
        }

        self.new_task_modal = None;
        cx.notify();
    }

    pub(crate) fn handle_new_task_modal_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        if self.new_task_modal.is_none() {
            return;
        }

        cx.stop_propagation();

        if self
            .new_task_modal
            .as_ref()
            .is_some_and(|state| state.submitting)
        {
            return;
        }

        match ev.keystroke.key.as_str() {
            "escape" => {
                self.new_task_modal = None;
                cx.notify();
            }
            "enter" => {
                self.submit_new_task_modal(cx);
            }
            _ => {
                self.handle_task_name_key_down(ev, cx);
            }
        }
    }

    fn handle_task_name_key_down(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.new_task_modal.as_mut() else {
            return false;
        };

        if !state.task_name_focused {
            return false;
        }

        let modifiers = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "backspace" => {
                if modifiers.platform {
                    delete_task_name_to_start(state);
                } else if modifiers.alt {
                    delete_task_name_word_backward(state);
                } else {
                    delete_backward_in_task_name(state);
                }
                cx.notify();
                return true;
            }
            "delete" => {
                delete_forward_in_task_name(state);
                cx.notify();
                return true;
            }
            "left" => {
                move_task_name_cursor(state, CursorDirection::Left, modifiers.shift);
                cx.notify();
                return true;
            }
            "right" => {
                move_task_name_cursor(state, CursorDirection::Right, modifiers.shift);
                cx.notify();
                return true;
            }
            "home" => {
                move_task_name_cursor_to_edge(state, false, modifiers.shift);
                cx.notify();
                return true;
            }
            "end" => {
                move_task_name_cursor_to_edge(state, true, modifiers.shift);
                cx.notify();
                return true;
            }
            "up" | "down" | "tab" => {
                return true;
            }
            _ => {}
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "a" {
            state.task_name_cursor = state.task_name.len();
            state.task_name_selection_anchor = Some(0);
            cx.notify();
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "c" {
            if let Some(range) = selected_task_name_range(state) {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    state.task_name[range].to_string(),
                ));
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "x" {
            if let Some(range) = selected_task_name_range(state) {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    state.task_name[range.clone()].to_string(),
                ));
                replace_task_name_range(state, range, "");
                cx.notify();
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "v" {
            if let Some(text) = cx
                .read_from_clipboard()
                .and_then(|item| item.text())
                .map(sanitize_task_name_input)
            {
                insert_task_name_text(state, &text);
                cx.notify();
            }
            return true;
        }

        if modifiers.control || modifiers.platform || modifiers.function {
            return false;
        }

        if let Some(key_char) = ev.keystroke.key_char.as_deref() {
            insert_task_name_text(state, key_char);
            cx.notify();
            return true;
        }

        false
    }

    fn render_header(submitting: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .flex_row()
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
                            .child("New Task"),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .text_color(muted_col())
                            .child("Open the original project directly or create a new worktree."),
                    ),
            )
            .child(
                div()
                    .id("new-task-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(24.))
                    .h(px(24.))
                    .rounded_md()
                    .cursor_pointer()
                    .opacity(if submitting { 0.45 } else { 1.0 })
                    .hover(move |s| s.bg(hover_bg()))
                    .tooltip(move |_window, cx| {
                        Self::action_tooltip_view("Close the new task modal", cx)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.dismiss_new_task_modal(cx);
                            cx.stop_propagation();
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

    fn render_source_branch(
        props: SourceBranchSectionProps<'_>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let SourceBranchSectionProps {
            project_name,
            selected_branch,
            current_branch,
            branches,
            worktree_mode,
            dropdown_open,
            submitting,
        } = props;
        let mut section = div()
            .mx(px(20.))
            .mt(px(4.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Project"),
            )
            .child(
                div()
                    .rounded_md()
                    .bg(subtle_bg())
                    .px(px(14.))
                    .py(px(10.))
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(title_col())
                            .child(project_name),
                    ),
            )
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Source branch"),
            );

        if worktree_mode {
            section = section.child(
                div().relative().child(
                    div()
                        .id("new-task-source-branch")
                        .h(px(38.))
                        .rounded_md()
                        .bg(subtle_bg())
                        .border_1()
                        .border_color(border_col())
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px(px(12.))
                        .cursor_pointer()
                        .opacity(if submitting { 0.45 } else { 1.0 })
                        .hover(move |s| s.bg(hover_bg()))
                        .tooltip(move |_window, cx| {
                            Self::action_tooltip_view(
                                "Choose the base branch for the new worktree",
                                cx,
                            )
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                if let Some(state) = this.new_task_modal.as_mut() {
                                    if state.submitting {
                                        return;
                                    }
                                    state.branch_dropdown_open = !state.branch_dropdown_open;
                                    state.agent_dropdown_open = false;
                                    state.task_name_focused = false;
                                }
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .text_size(rems(13. / 16.))
                                .text_color(title_col())
                                .child(selected_branch),
                        )
                        .child(
                            svg()
                                .path("assets/icons/icons__chevron-down.svg")
                                .size(px(11.))
                                .text_color(muted_col()),
                        ),
                ),
            );

            if dropdown_open {
                let mut list = div()
                    .mt(px(4.))
                    .rounded_md()
                    .bg(rgb(CARD_BG))
                    .border_1()
                    .border_color(border_col())
                    .shadow_md()
                    .overflow_hidden();

                for branch in branches {
                    let branch_name = branch.clone();
                    let branch_label: SharedString = branch.clone().into();
                    list = list.child(
                        div()
                            .id(SharedString::from(format!(
                                "new-task-source-branch-{}",
                                branch_name
                            )))
                            .h(px(36.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg()))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Use this branch as the worktree base",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                    if let Some(state) = this.new_task_modal.as_mut() {
                                        if state.submitting {
                                            return;
                                        }
                                        state.source_branch = branch_name.clone();
                                        state.branch_dropdown_open = false;
                                        state.agent_dropdown_open = false;
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .text_size(rems(13. / 16.))
                                    .text_color(title_col())
                                    .child(branch_label),
                            ),
                    );
                }

                section = section.child(list);
            }
        } else {
            section = section
                .child(
                    div()
                        .rounded_md()
                        .bg(subtle_bg())
                        .border_1()
                        .border_color(border_col())
                        .px(px(12.))
                        .py(px(10.))
                        .child(
                            div()
                                .text_size(rems(13. / 16.))
                                .text_color(title_col())
                                .child(current_branch),
                        ),
                )
                .child(
                    div()
                        .text_size(rems(11. / 16.))
                        .text_color(muted_col())
                        .child("Direct mode uses the branch currently checked out in the original project."),
                );
        }

        section
    }

    fn render_task_name_field(
        task_name: SharedString,
        generated_task_name: SharedString,
        focused: bool,
        cursor: usize,
        selection: Option<std::ops::Range<usize>>,
        submitting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_empty = task_name.is_empty();

        div()
            .mx(px(20.))
            .mt(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Task name (optional)"),
            )
            .child(
                div()
                    .id("new-task-name")
                    .h(px(40.))
                    .min_w(px(0.))
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
                    .overflow_hidden()
                    .px(px(12.))
                    .cursor_pointer()
                    .opacity(if submitting { 0.45 } else { 1.0 })
                    .hover(move |s| s.bg(hover_bg()))
                    .tooltip(move |_window, cx| {
                        Self::action_tooltip_view(
                            "Enter a task name or leave it blank to use a generated one",
                            cx,
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, window, cx| {
                            this.focus_handle.focus(window);
                            if let Some(state) = this.new_task_modal.as_mut() {
                                if state.submitting {
                                    return;
                                }
                                state.task_name_focused = true;
                                state.task_name_cursor = state.task_name.len();
                                state.task_name_selection_anchor = None;
                                state.branch_dropdown_open = false;
                                state.agent_dropdown_open = false;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(Self::render_task_name_content(
                        task_name.clone(),
                        generated_task_name.clone(),
                        focused,
                        cursor,
                        selection,
                        is_empty,
                    )),
            )
            .child(
                div()
                    .text_size(rems(11. / 16.))
                    .text_color(muted_col())
                    .child(format!("Leave blank to use {}", generated_task_name)),
            )
    }

    fn render_agent_selector(
        &self,
        dropdown_open: bool,
        selected: &HashSet<String>,
        submitting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let display_agent = AGENTS
            .iter()
            .find(|agent| selected.contains(agent.id))
            .unwrap_or(&AGENTS[0]);

        let trigger_icon: SharedString = display_agent.icon.into();
        let trigger_label: SharedString = if selected.len() > 1 {
            format!("{} agents selected", selected.len()).into()
        } else {
            display_agent.label.into()
        };

        let mut section = div()
            .mx(px(20.))
            .mt(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Agent"),
            )
            .child(
                div().relative().child(
                    div()
                        .id("new-task-agent-trigger")
                        .h(px(38.))
                        .rounded_md()
                        .bg(subtle_bg())
                        .border_1()
                        .border_color(border_col())
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .px(px(10.))
                        .cursor_pointer()
                        .opacity(if submitting { 0.45 } else { 1.0 })
                        .hover(move |s| s.bg(hover_bg()))
                        .tooltip(move |_window, cx| {
                            Self::action_tooltip_view("Choose the agents for this task", cx)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                if let Some(state) = this.new_task_modal.as_mut() {
                                    if state.submitting {
                                        return;
                                    }
                                    state.agent_dropdown_open = !state.agent_dropdown_open;
                                    state.branch_dropdown_open = false;
                                    state.task_name_focused = false;
                                }
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    svg()
                                        .path(trigger_icon)
                                        .size(px(18.))
                                        .text_color(title_col()),
                                )
                                .child(
                                    div()
                                        .text_size(rems(13. / 16.))
                                        .text_color(title_col())
                                        .child(trigger_label),
                                ),
                        )
                        .child(
                            svg()
                                .path("assets/icons/icons__chevron-down.svg")
                                .size(px(11.))
                                .text_color(muted_col()),
                        ),
                ),
            );

        if dropdown_open {
            section = section.child(self.render_agent_dropdown(selected, submitting, cx));
        }

        section
    }

    fn render_agent_dropdown(
        &self,
        selected: &HashSet<String>,
        submitting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visible_rows = AGENTS.len().min(6) as f32;
        let dropdown_height = px(visible_rows * 36. + 8.);

        let mut list = div()
            .id("new-task-agent-dropdown")
            .mt(px(4.))
            .h(dropdown_height)
            .rounded_md()
            .bg(rgb(CARD_BG))
            .border_1()
            .border_color(border_col())
            .shadow_md()
            .overflow_y_scroll()
            .py(px(4.));

        for agent in AGENTS {
            let is_selected = selected.contains(agent.id);
            let agent_id = agent.id.to_string();
            let icon_path: SharedString = agent.icon.into();
            let label: SharedString = agent.label.into();

            list = list.child(
                div()
                    .id(SharedString::from(format!("new-task-agent-{}", agent.id)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .h(px(36.))
                    .px(px(12.))
                    .cursor_pointer()
                    .opacity(if submitting { 0.45 } else { 1.0 })
                    .hover(move |s| s.bg(hover_bg()))
                    .tooltip(move |_window, cx| {
                        Self::action_tooltip_view("Toggle this agent for the task", cx)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            if let Some(state) = this.new_task_modal.as_mut() {
                                if state.submitting {
                                    return;
                                }
                                if state.selected_agents.contains(&agent_id) {
                                    state.selected_agents.remove(&agent_id);
                                } else {
                                    state.selected_agents.insert(agent_id.clone());
                                }
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
                            .border_color(if is_selected {
                                hsla(220. / 360., 0.55, 0.55, 1.)
                            } else {
                                border_col()
                            })
                            .bg(if is_selected {
                                hsla(220. / 360., 0.55, 0.55, 1.)
                            } else {
                                gpui::transparent_black()
                            })
                            .flex()
                            .items_center()
                            .justify_center()
                            .when(is_selected, |container| {
                                container.child(
                                    svg()
                                        .path("assets/icons/icons__check.svg")
                                        .size(px(12.))
                                        .text_color(gpui::white()),
                                )
                            }),
                    )
                    .child(svg().path(icon_path).size(px(18.)).text_color(title_col()))
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .text_color(title_col())
                            .child(label),
                    ),
            );
        }

        list
    }

    fn render_workspace_toggle(
        worktree_mode: bool,
        submitting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .mx(px(20.))
            .mt(px(16.))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col())
                    .child("Workspace"),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .rounded_md()
                    .bg(subtle_bg())
                    .p(px(3.))
                    .gap(px(2.))
                    .child(
                        div()
                            .id("new-task-workspace-worktree")
                            .flex_1()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap(px(6.))
                            .h(px(32.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .opacity(if submitting { 0.45 } else { 1.0 })
                            .bg(if worktree_mode {
                                active_bg()
                            } else {
                                gpui::transparent_black()
                            })
                            .hover(move |s| {
                                s.bg(if worktree_mode {
                                    active_bg()
                                } else {
                                    hover_bg()
                                })
                            })
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Create a sibling worktree for this task",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    if let Some(state) = this.new_task_modal.as_mut() {
                                        if state.submitting {
                                            return;
                                        }
                                        state.worktree_mode = true;
                                        state.agent_dropdown_open = false;
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__git-worktree.svg")
                                    .size(px(14.))
                                    .text_color(if worktree_mode {
                                        title_col()
                                    } else {
                                        muted_col()
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if worktree_mode {
                                        title_col()
                                    } else {
                                        muted_col()
                                    })
                                    .child("Worktree"),
                            ),
                    )
                    .child(
                        div()
                            .id("new-task-workspace-direct")
                            .flex_1()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_center()
                            .gap(px(6.))
                            .h(px(32.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .opacity(if submitting { 0.45 } else { 1.0 })
                            .bg(if !worktree_mode {
                                active_bg()
                            } else {
                                gpui::transparent_black()
                            })
                            .hover(move |s| {
                                s.bg(if !worktree_mode {
                                    active_bg()
                                } else {
                                    hover_bg()
                                })
                            })
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Open the original project directory directly",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    if let Some(state) = this.new_task_modal.as_mut() {
                                        if state.submitting {
                                            return;
                                        }
                                        state.worktree_mode = false;
                                        state.branch_dropdown_open = false;
                                        state.agent_dropdown_open = false;
                                    }
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__folder-open.svg")
                                    .size(px(14.))
                                    .text_color(if !worktree_mode {
                                        title_col()
                                    } else {
                                        muted_col()
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if !worktree_mode {
                                        title_col()
                                    } else {
                                        muted_col()
                                    })
                                    .child("Direct"),
                            ),
                    ),
            )
            .when(!worktree_mode, |container| {
                container.child(
                    div()
                        .text_size(rems(11. / 16.))
                        .text_color(danger_col())
                        .child(
                            "Direct uses the branch already checked out in the original project.",
                        ),
                )
            })
    }

    fn render_advanced_options(
        expanded: bool,
        submitting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let chevron = if expanded {
            "assets/icons/icons__chevron-up.svg"
        } else {
            "assets/icons/icons__chevron-down.svg"
        };

        let mut section = div().mx(px(20.)).mt(px(16.)).flex().flex_col();

        section = section.child(
            div()
                .id("new-task-advanced-toggle")
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .h(px(40.))
                .px(px(14.))
                .rounded_md()
                .bg(subtle_bg())
                .cursor_pointer()
                .opacity(if submitting { 0.45 } else { 1.0 })
                .hover(move |s| s.bg(hover_bg()))
                .tooltip(move |_window, cx| {
                    Self::action_tooltip_view(
                        "Show advanced task options that will be wired later",
                        cx,
                    )
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                        if let Some(state) = this.new_task_modal.as_mut() {
                            if state.submitting {
                                return;
                            }
                            state.advanced_expanded = !state.advanced_expanded;
                            state.agent_dropdown_open = false;
                            state.branch_dropdown_open = false;
                            state.task_name_focused = false;
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            svg()
                                .path("assets/icons/icons__settings.svg")
                                .size(px(15.))
                                .text_color(muted_col()),
                        )
                        .child(
                            div()
                                .text_size(rems(13. / 16.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(body_col())
                                .child("Advanced options"),
                        ),
                )
                .child(svg().path(chevron).size(px(11.)).text_color(muted_col())),
        );

        if expanded {
            section = section.child(
                div()
                    .mt(px(12.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(rems(13. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col())
                                    .w(px(100.))
                                    .child("GitHub issue"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.))
                                    .rounded_md()
                                    .bg(subtle_bg())
                                    .border_1()
                                    .border_color(border_col())
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .px(px(10.))
                                    .gap(px(8.))
                                    .child(
                                        svg()
                                            .path("assets/icons/icons__github.svg")
                                            .size(px(16.))
                                            .text_color(muted_col()),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(rems(12. / 16.))
                                            .text_color(placeholder_col())
                                            .child("Select a GitHub issue"),
                                    )
                                    .child(
                                        svg()
                                            .path("assets/icons/icons__chevron-down.svg")
                                            .size(px(11.))
                                            .text_color(muted_col()),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(12.))
                            .child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(rems(13. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col())
                                    .w(px(100.))
                                    .child("Jira issue"),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .h(px(36.))
                                    .rounded_md()
                                    .bg(subtle_bg())
                                    .border_1()
                                    .border_color(border_col())
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .px(px(10.))
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .w(px(16.))
                                            .h(px(16.))
                                            .rounded(px(3.))
                                            .bg(hsla(220. / 360., 0.65, 0.52, 1.))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .child(
                                                div()
                                                    .text_size(rems(9. / 16.))
                                                    .font_weight(gpui::FontWeight::BOLD)
                                                    .text_color(gpui::white())
                                                    .child("J"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .flex_1()
                                            .text_size(rems(12. / 16.))
                                            .text_color(placeholder_col())
                                            .child("Select a Jira issue"),
                                    )
                                    .child(
                                        svg()
                                            .path("assets/icons/icons__chevron-down.svg")
                                            .size(px(11.))
                                            .text_color(muted_col()),
                                    ),
                            ),
                    ),
            );
        }

        section
    }

    fn render_footer(submitting: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_end()
            .gap(px(10.))
            .px(px(20.))
            .py(px(16.))
            .border_t_1()
            .border_color(gpui::white().opacity(0.06))
            .mt(px(16.))
            .child(
                div()
                    .id("new-task-cancel")
                    .cursor_pointer()
                    .px(px(14.))
                    .py(px(7.))
                    .rounded_md()
                    .border_1()
                    .border_color(border_col())
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(body_col())
                    .opacity(if submitting { 0.45 } else { 1.0 })
                    .hover(move |s| s.bg(hover_bg()))
                    .tooltip(move |_window, cx| {
                        Self::action_tooltip_view("Close the modal without creating a task", cx)
                    })
                    .child("Cancel")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.dismiss_new_task_modal(cx);
                            cx.stop_propagation();
                        }),
                    ),
            )
            .child(
                div()
                    .id("new-task-create")
                    .cursor_pointer()
                    .px(px(16.))
                    .py(px(7.))
                    .rounded_md()
                    .bg(gpui::white())
                    .opacity(if submitting { 0.65 } else { 1.0 })
                    .hover(move |s| s.bg(hsla(0., 0., 0.90, 1.)))
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(0x1e1f22))
                    .tooltip(move |_window, cx| {
                        Self::action_tooltip_view("Create the task workspace", cx)
                    })
                    .child(if submitting { "Creating..." } else { "Create" })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.submit_new_task_modal(cx);
                            cx.stop_propagation();
                        }),
                    ),
            )
    }

    fn render_task_name_content(
        task_name: SharedString,
        generated_task_name: SharedString,
        focused: bool,
        cursor: usize,
        selection: Option<std::ops::Range<usize>>,
        is_empty: bool,
    ) -> impl IntoElement {
        let cursor = cursor.min(task_name.len());
        let selection =
            selection.map(|range| range.start.min(task_name.len())..range.end.min(task_name.len()));

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
                .child(
                    div()
                        .text_color(placeholder_col())
                        .child(generated_task_name),
                );
        }

        let selected = selection.filter(|range| range.start < range.end);
        let visible_range = task_name_visible_range(&task_name, cursor, selected.as_ref(), 42);
        let leading_clipped = visible_range.start > 0;
        let trailing_clipped = visible_range.end < task_name.len();
        let visible_start = visible_range.start;
        let visible_task_name = task_name[visible_range.clone()].to_string();
        let local_cursor = cursor
            .saturating_sub(visible_start)
            .min(visible_task_name.len());
        let visible_selection = selected
            .as_ref()
            .and_then(|range| intersect_byte_ranges(range.clone(), visible_range.clone()))
            .map(|range| range.start - visible_start..range.end - visible_start);
        let selected_contains_cursor = visible_selection
            .as_ref()
            .is_some_and(|range| range.start <= local_cursor && local_cursor <= range.end);

        let (prefix_end, selected_end) = if let Some(range) = visible_selection.as_ref() {
            (
                range.start.min(local_cursor),
                range.end.min(visible_task_name.len()),
            )
        } else {
            (
                local_cursor.min(visible_task_name.len()),
                local_cursor.min(visible_task_name.len()),
            )
        };

        let prefix = visible_task_name[..prefix_end].to_string();
        let middle = if let Some(range) = visible_selection.as_ref() {
            visible_task_name[range.clone()].to_string()
        } else {
            String::new()
        };
        let suffix_start = if selected_contains_cursor {
            selected_end
        } else {
            local_cursor.min(visible_task_name.len())
        };
        let between = if visible_selection
            .as_ref()
            .is_some_and(|range| range.end < local_cursor)
        {
            visible_task_name[selected_end..local_cursor.min(visible_task_name.len())].to_string()
        } else {
            String::new()
        };
        let trailing = visible_task_name[suffix_start..].to_string();

        let mut row = div()
            .flex()
            .items_center()
            .min_w(px(0.))
            .overflow_hidden()
            .gap(px(0.))
            .text_size(rems(13. / 16.));

        if leading_clipped {
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

        if trailing_clipped {
            row = row.child(div().text_color(muted_col()).child("..."));
        }

        row
    }
}

enum CursorDirection {
    Left,
    Right,
}

fn sanitize_task_name_input(text: String) -> String {
    text.replace(['\n', '\r', '\t'], " ")
}

fn intersect_byte_ranges(
    left: std::ops::Range<usize>,
    right: std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    let start = left.start.max(right.start);
    let end = left.end.min(right.end);
    (start < end).then_some(start..end)
}

fn task_name_visible_range(
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

fn selected_task_name_range(state: &NewTaskModalState) -> Option<std::ops::Range<usize>> {
    let anchor = state.task_name_selection_anchor?;
    if anchor == state.task_name_cursor {
        None
    } else if anchor < state.task_name_cursor {
        Some(anchor..state.task_name_cursor)
    } else {
        Some(state.task_name_cursor..anchor)
    }
}

fn previous_task_name_boundary(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .rev()
        .find_map(|(index, _)| (index < cursor).then_some(index))
        .unwrap_or(0)
}

fn next_task_name_boundary(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .find_map(|(index, _)| (index > cursor).then_some(index))
        .unwrap_or(text.len())
}

fn replace_task_name_range(
    state: &mut NewTaskModalState,
    range: std::ops::Range<usize>,
    new_text: &str,
) {
    state.task_name.replace_range(range.clone(), new_text);
    state.task_name_cursor = range.start + new_text.len();
    state.task_name_selection_anchor = None;
}

fn insert_task_name_text(state: &mut NewTaskModalState, text: &str) {
    let range =
        selected_task_name_range(state).unwrap_or(state.task_name_cursor..state.task_name_cursor);
    replace_task_name_range(state, range, text);
}

fn delete_backward_in_task_name(state: &mut NewTaskModalState) {
    if let Some(range) = selected_task_name_range(state) {
        replace_task_name_range(state, range, "");
        return;
    }

    if state.task_name_cursor == 0 {
        return;
    }

    let start = previous_task_name_boundary(&state.task_name, state.task_name_cursor);
    replace_task_name_range(state, start..state.task_name_cursor, "");
}

fn previous_task_name_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = cursor;
    while idx > 0 {
        let start = previous_task_name_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if !ch.is_whitespace() {
            break;
        }
        idx = start;
    }

    while idx > 0 {
        let start = previous_task_name_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if is_task_name_word_char(ch) {
            idx = start;
        } else {
            break;
        }
    }

    idx
}

fn is_task_name_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-')
}

fn delete_task_name_word_backward(state: &mut NewTaskModalState) {
    if let Some(range) = selected_task_name_range(state) {
        replace_task_name_range(state, range, "");
        return;
    }

    if state.task_name_cursor == 0 {
        return;
    }

    let start = previous_task_name_word_boundary(&state.task_name, state.task_name_cursor);
    replace_task_name_range(state, start..state.task_name_cursor, "");
}

fn delete_task_name_to_start(state: &mut NewTaskModalState) {
    if let Some(range) = selected_task_name_range(state) {
        replace_task_name_range(state, range, "");
        return;
    }

    if state.task_name_cursor == 0 {
        return;
    }

    replace_task_name_range(state, 0..state.task_name_cursor, "");
}

fn delete_forward_in_task_name(state: &mut NewTaskModalState) {
    if let Some(range) = selected_task_name_range(state) {
        replace_task_name_range(state, range, "");
        return;
    }

    if state.task_name_cursor >= state.task_name.len() {
        return;
    }

    let end = next_task_name_boundary(&state.task_name, state.task_name_cursor);
    replace_task_name_range(state, state.task_name_cursor..end, "");
}

fn move_task_name_cursor(
    state: &mut NewTaskModalState,
    direction: CursorDirection,
    extend_selection: bool,
) {
    let next_cursor = match direction {
        CursorDirection::Left => {
            if let Some(range) = selected_task_name_range(state) {
                if extend_selection {
                    previous_task_name_boundary(&state.task_name, state.task_name_cursor)
                } else {
                    range.start
                }
            } else {
                previous_task_name_boundary(&state.task_name, state.task_name_cursor)
            }
        }
        CursorDirection::Right => {
            if let Some(range) = selected_task_name_range(state) {
                if extend_selection {
                    next_task_name_boundary(&state.task_name, state.task_name_cursor)
                } else {
                    range.end
                }
            } else {
                next_task_name_boundary(&state.task_name, state.task_name_cursor)
            }
        }
    };

    if extend_selection {
        if state.task_name_selection_anchor.is_none() {
            state.task_name_selection_anchor = Some(state.task_name_cursor);
        }
    } else {
        state.task_name_selection_anchor = None;
    }

    state.task_name_cursor = next_cursor;
}

fn move_task_name_cursor_to_edge(
    state: &mut NewTaskModalState,
    to_end: bool,
    extend_selection: bool,
) {
    if extend_selection && state.task_name_selection_anchor.is_none() {
        state.task_name_selection_anchor = Some(state.task_name_cursor);
    }
    if !extend_selection {
        state.task_name_selection_anchor = None;
    }
    state.task_name_cursor = if to_end { state.task_name.len() } else { 0 };
}

fn generate_task_name() -> String {
    const FIRST: &[&str] = &[
        "quiet", "silver", "bright", "steady", "wild", "mellow", "brisk", "neat",
    ];
    const SECOND: &[&str] = &[
        "river", "meadow", "comet", "signal", "forest", "ember", "harbor", "planet",
    ];
    const THIRD: &[&str] = &[
        "sparks", "travels", "builds", "drifts", "guides", "moves", "lands", "echoes",
    ];

    let bytes = *Uuid::new_v4().as_bytes();
    format!(
        "{}-{}-{}",
        FIRST[bytes[0] as usize % FIRST.len()],
        SECOND[bytes[5] as usize % SECOND.len()],
        THIRD[bytes[10] as usize % THIRD.len()]
    )
}
