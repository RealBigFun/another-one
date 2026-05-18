//! "New Task" modal dialog shown when clicking the "+" button on a project.

use std::collections::HashSet;

use gpui::{
    div, hsla, prelude::*, px, relative, rems, rgb, svg, ClipboardItem, Context, KeyDownEvent,
    MouseButton, MouseDownEvent, SharedString,
};
use uuid::Uuid;

use crate::agent_icons::branded_icon;
use crate::agents::{AgentDef, AGENTS};
use crate::app::AnotherOneApp;
use crate::theme::{self, ResolvedTheme};

#[derive(Clone)]
pub(crate) struct NewTaskModalState {
    pub project_id: String,
    pub project_name: String,
    pub task_name: String,
    pub generated_task_name: String,
    pub source_branch: String,
    pub branch_mode: NewTaskBranchMode,
    pub branch_dropdown_open: bool,
    pub branch_filter: String,
    pub branch_filter_focused: bool,
    pub branch_filter_cursor: usize,
    pub branch_filter_selection_anchor: Option<usize>,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NewTaskBranchMode {
    NewBranch,
    ExistingBranch,
}

const CLI_ONLY_ICON: &str = "assets/icons/icons__terminal.svg";
const CLI_ONLY_LABEL: &str = "Terminal";

fn new_task_theme() -> theme::AppTheme {
    match theme::current_terminal_theme() {
        ResolvedTheme::Light => theme::light_theme(),
        ResolvedTheme::Dark => theme::dark_theme(),
    }
}

fn card_bg() -> gpui::Hsla {
    new_task_theme().card_bg
}

fn scrim_bg() -> gpui::Hsla {
    new_task_theme().scrim_bg
}

fn title_col() -> gpui::Hsla {
    new_task_theme().text_primary
}

fn body_col() -> gpui::Hsla {
    new_task_theme().text_secondary
}

fn muted_col() -> gpui::Hsla {
    new_task_theme().text_muted
}

fn placeholder_col() -> gpui::Hsla {
    new_task_theme().text_placeholder
}

fn danger_col() -> gpui::Hsla {
    new_task_theme().error.text
}

fn border_col() -> gpui::Hsla {
    new_task_theme().border
}

fn hover_bg() -> gpui::Hsla {
    new_task_theme().overlay_hover
}

fn subtle_bg() -> gpui::Hsla {
    new_task_theme().overlay_rest
}

fn active_bg() -> gpui::Hsla {
    new_task_theme().overlay_active
}

fn focus_col() -> gpui::Hsla {
    new_task_theme().focus_ring
}

fn primary_button_bg() -> gpui::Hsla {
    match theme::current_terminal_theme() {
        ResolvedTheme::Light => rgb(0x1f2328).into(),
        ResolvedTheme::Dark => gpui::white(),
    }
}

fn primary_button_hover_bg() -> gpui::Hsla {
    match theme::current_terminal_theme() {
        ResolvedTheme::Light => rgb(0x374151).into(),
        ResolvedTheme::Dark => hsla(0., 0., 0.90, 1.),
    }
}

fn primary_button_text_col() -> gpui::Hsla {
    match theme::current_terminal_theme() {
        ResolvedTheme::Light => gpui::white(),
        ResolvedTheme::Dark => rgb(0x1e1f22).into(),
    }
}

struct SourceBranchSectionProps<'a> {
    project_name: SharedString,
    selected_branch: SharedString,
    current_branch: SharedString,
    branches: &'a [String],
    worktree_mode: bool,
    is_git_backed: bool,
    branch_mode: NewTaskBranchMode,
    dropdown_open: bool,
    branch_filter: SharedString,
    branch_filter_focused: bool,
    branch_filter_cursor: usize,
    branch_filter_selection: Option<std::ops::Range<usize>>,
    submitting: bool,
}

fn default_new_task_agent_id(
    enabled_agents: &[&'static AgentDef],
    default_agent_id: Option<&'static str>,
) -> Option<&'static str> {
    default_agent_id
        .filter(|agent_id| enabled_agents.iter().any(|agent| agent.id == *agent_id))
        .or_else(|| enabled_agents.first().map(|agent| agent.id))
}

fn sanitized_new_task_selected_agents(
    selected_agents: &HashSet<String>,
    enabled_agents: &[&'static AgentDef],
) -> HashSet<String> {
    let enabled_ids = enabled_agents
        .iter()
        .map(|agent| agent.id)
        .collect::<HashSet<_>>();

    selected_agents
        .iter()
        .filter(|agent_id| enabled_ids.contains(agent_id.as_str()))
        .cloned()
        .collect()
}

impl AnotherOneApp {
    pub(crate) fn sanitize_new_task_modal_selected_agents(&mut self) -> Vec<&'static AgentDef> {
        let enabled_agents = self.enabled_agents();
        if let Some(state) = self.new_task_modal.as_mut() {
            state.selected_agents =
                sanitized_new_task_selected_agents(&state.selected_agents, &enabled_agents);
        }
        enabled_agents
    }

    pub(crate) fn open_new_task_modal(&mut self, project_id: &str, cx: &mut Context<Self>) {
        let root_project_id = self
            .project_store
            .root_project_id_for_project(project_id)
            .unwrap_or_else(|| project_id.to_string());
        let Some(project) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == root_project_id)
        else {
            return;
        };

        let source_branch = self
            .project_store
            .primary_branch_for_project(&project.id, true)
            .map(|branch| branch.name)
            .unwrap_or_default();
        self.open_new_task_modal_with_branch(&root_project_id, &source_branch, cx);
    }

    pub(crate) fn open_new_task_modal_with_branch(
        &mut self,
        project_id: &str,
        source_branch: &str,
        cx: &mut Context<Self>,
    ) {
        let root_project_id = self
            .project_store
            .root_project_id_for_project(project_id)
            .unwrap_or_else(|| project_id.to_string());
        let Some(project) = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == root_project_id)
        else {
            return;
        };
        let refresh_project_id = project.id.clone();
        let project_path = project.path.clone();

        let mut selected_agents = HashSet::new();
        if let Some(default_agent_id) =
            default_new_task_agent_id(&self.enabled_agents(), self.default_agent_id())
        {
            selected_agents.insert(default_agent_id.to_string());
        }

        self.new_task_modal = Some(NewTaskModalState {
            project_id: project.id.clone(),
            project_name: project.name.clone(),
            task_name: String::new(),
            generated_task_name: generate_task_name(),
            source_branch: source_branch.to_string(),
            branch_mode: NewTaskBranchMode::NewBranch,
            branch_dropdown_open: false,
            branch_filter: String::new(),
            branch_filter_focused: false,
            branch_filter_cursor: 0,
            branch_filter_selection_anchor: None,
            agent_dropdown_open: false,
            selected_agents,
            worktree_mode: true,
            task_name_focused: true,
            task_name_cursor: 0,
            task_name_selection_anchor: None,
            advanced_expanded: false,
            submitting: false,
        });
        self.start_new_task_branch_refresh(refresh_project_id, project_path);
        self.sync_new_task_modal_prewarm(cx);
    }

    pub(crate) fn new_task_modal_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(ref state) = self.new_task_modal else {
            return div().id("new-task-modal-overlay");
        };
        let enabled_agents = self.enabled_agents();

        let project = self
            .project_store
            .projects
            .iter()
            .find(|project| project.id == state.project_id);

        let is_git_backed = self.project_store.is_git_backed(&state.project_id);
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
        // Non-git projects can only create direct tasks (no worktrees, no branches).
        let worktree_mode = state.worktree_mode && is_git_backed;
        let branch_mode = state.branch_mode;
        let branch_dropdown_open = state.branch_dropdown_open;
        let branch_filter: SharedString = state.branch_filter.clone().into();
        let branch_filter_focused = state.branch_filter_focused;
        let branch_filter_cursor = state.branch_filter_cursor;
        let branch_filter_selection = selected_branch_filter_range(state);
        let agent_dropdown_open = state.agent_dropdown_open;
        let selected_agents =
            sanitized_new_task_selected_agents(&state.selected_agents, &enabled_agents);
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
            .bg(scrim_bg())
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
                    .bg(card_bg())
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
                                    is_git_backed,
                                    branch_mode,
                                    dropdown_open: branch_dropdown_open,
                                    branch_filter,
                                    branch_filter_focused,
                                    branch_filter_cursor,
                                    branch_filter_selection,
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
                                &enabled_agents,
                                agent_dropdown_open,
                                &selected_agents,
                                submitting,
                                cx,
                            ))
                            .when(is_git_backed, |d| {
                                d.child(Self::render_workspace_toggle(worktree_mode, submitting, cx))
                            })
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

        self.cancel_active_new_task_prewarm();
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
                self.cancel_active_new_task_prewarm();
                self.new_task_modal = None;
                cx.notify();
            }
            "enter" => {
                if self
                    .new_task_modal
                    .as_ref()
                    .is_some_and(|state| state.branch_filter_focused)
                {
                    return;
                }
                self.submit_new_task_modal(cx);
            }
            _ => {
                if !self.handle_branch_filter_key_down(ev, cx) {
                    self.handle_task_name_key_down(ev, cx);
                }
            }
        }
    }

    fn handle_branch_filter_key_down(&mut self, ev: &KeyDownEvent, cx: &mut Context<Self>) -> bool {
        let Some(state) = self.new_task_modal.as_mut() else {
            return false;
        };

        if !state.branch_filter_focused {
            return false;
        }

        let modifiers = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "backspace" => {
                if modifiers.platform {
                    delete_branch_filter_to_start(state);
                } else if modifiers.alt {
                    delete_branch_filter_word_backward(state);
                } else {
                    delete_backward_in_branch_filter(state);
                }
                cx.notify();
                return true;
            }
            "delete" => {
                delete_forward_in_branch_filter(state);
                cx.notify();
                return true;
            }
            "left" => {
                move_branch_filter_cursor(state, CursorDirection::Left, modifiers.shift);
                cx.notify();
                return true;
            }
            "right" => {
                move_branch_filter_cursor(state, CursorDirection::Right, modifiers.shift);
                cx.notify();
                return true;
            }
            "home" => {
                move_branch_filter_cursor_to_edge(state, false, modifiers.shift);
                cx.notify();
                return true;
            }
            "end" => {
                move_branch_filter_cursor_to_edge(state, true, modifiers.shift);
                cx.notify();
                return true;
            }
            "up" | "down" | "tab" => {
                return true;
            }
            _ => {}
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "a" {
            state.branch_filter_cursor = state.branch_filter.len();
            state.branch_filter_selection_anchor = Some(0);
            cx.notify();
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "c" {
            if let Some(range) = selected_branch_filter_range(state) {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    state.branch_filter[range].to_string(),
                ));
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "x" {
            if let Some(range) = selected_branch_filter_range(state) {
                cx.write_to_clipboard(ClipboardItem::new_string(
                    state.branch_filter[range.clone()].to_string(),
                ));
                replace_branch_filter_range(state, range, "");
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
                insert_branch_filter_text(state, &text);
                cx.notify();
            }
            return true;
        }

        if modifiers.control || modifiers.platform || modifiers.function {
            return false;
        }

        if let Some(key_char) = ev.keystroke.key_char.as_deref() {
            insert_branch_filter_text(state, key_char);
            cx.notify();
            return true;
        }

        false
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
            is_git_backed,
            branch_mode,
            dropdown_open,
            branch_filter,
            branch_filter_focused,
            branch_filter_cursor,
            branch_filter_selection,
            submitting,
        } = props;
        let branch_mode_help = match branch_mode {
            NewTaskBranchMode::NewBranch => {
                "Selected branch is the base branch for the generated task branch."
            }
            NewTaskBranchMode::ExistingBranch => {
                "Selected branch is checked out directly in the new worktree."
            }
        };
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
            );

        if worktree_mode {
            section = section
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .text_size(rems(12. / 16.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(title_col())
                                .child("Branch"),
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
                                        .id("new-task-branch-mode-new")
                                        .h(px(28.))
                                        .px(px(10.))
                                        .rounded(px(5.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .opacity(if submitting { 0.45 } else { 1.0 })
                                        .bg(if branch_mode == NewTaskBranchMode::NewBranch {
                                            active_bg()
                                        } else {
                                            gpui::transparent_black()
                                        })
                                        .hover(move |s| {
                                            s.bg(if branch_mode == NewTaskBranchMode::NewBranch {
                                                active_bg()
                                            } else {
                                                hover_bg()
                                            })
                                        })
                                        .tooltip(move |_window, cx| {
                                            Self::action_tooltip_view(
                                                "Create a generated task branch from the selected branch",
                                                cx,
                                            )
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                |this, _ev: &MouseDownEvent, _window, cx| {
                                                    if let Some(state) =
                                                        this.new_task_modal.as_mut()
                                                    {
                                                        if state.submitting {
                                                            return;
                                                        }
                                                        state.branch_mode =
                                                            NewTaskBranchMode::NewBranch;
                                                        state.agent_dropdown_open = false;
                                                        state.task_name_focused = false;
                                                        state.branch_filter_focused = false;
                                                    }
                                                    cx.stop_propagation();
                                                    cx.notify();
                                                },
                                            ),
                                        )
                                        .child(
                                            div()
                                                .text_size(rems(12. / 16.))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(
                                                    if branch_mode == NewTaskBranchMode::NewBranch {
                                                        title_col()
                                                    } else {
                                                        muted_col()
                                                    },
                                                )
                                                .child("New branch"),
                                        ),
                                )
                                .child(
                                    div()
                                        .id("new-task-branch-mode-existing")
                                        .h(px(28.))
                                        .px(px(10.))
                                        .rounded(px(5.))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .cursor_pointer()
                                        .opacity(if submitting { 0.45 } else { 1.0 })
                                        .bg(if branch_mode == NewTaskBranchMode::ExistingBranch {
                                            active_bg()
                                        } else {
                                            gpui::transparent_black()
                                        })
                                        .hover(move |s| {
                                            s.bg(
                                                if branch_mode
                                                    == NewTaskBranchMode::ExistingBranch
                                                {
                                                    active_bg()
                                                } else {
                                                    hover_bg()
                                                },
                                            )
                                        })
                                        .tooltip(move |_window, cx| {
                                            Self::action_tooltip_view(
                                                "Check out the selected branch directly in the new worktree",
                                                cx,
                                            )
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                |this, _ev: &MouseDownEvent, _window, cx| {
                                                    if let Some(state) =
                                                        this.new_task_modal.as_mut()
                                                    {
                                                        if state.submitting {
                                                            return;
                                                        }
                                                        state.branch_mode =
                                                            NewTaskBranchMode::ExistingBranch;
                                                        state.agent_dropdown_open = false;
                                                        state.task_name_focused = false;
                                                        state.branch_filter_focused = false;
                                                    }
                                                    cx.stop_propagation();
                                                    cx.notify();
                                                },
                                            ),
                                        )
                                        .child(
                                            div()
                                                .text_size(rems(12. / 16.))
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(
                                                    if branch_mode
                                                        == NewTaskBranchMode::ExistingBranch
                                                    {
                                                        title_col()
                                                    } else {
                                                        muted_col()
                                                    },
                                                )
                                                .child("Existing branch"),
                                        ),
                                ),
                        ),
                )
                .child(div().relative().child(
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
                            let message = if branch_mode == NewTaskBranchMode::NewBranch {
                                "Choose the base branch for the generated task branch"
                            } else {
                                "Choose the branch to check out in the new worktree"
                            };
                            Self::action_tooltip_view(message, cx)
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
                                    state.branch_filter_focused = state.branch_dropdown_open;
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
                ))
                .child(
                    div()
                        .text_size(rems(11. / 16.))
                        .text_color(muted_col())
                        .child(branch_mode_help),
                );

            if dropdown_open {
                let filter = branch_filter.to_string();
                let filter_trimmed = filter.trim().to_lowercase();
                let filtered_branches = branches
                    .iter()
                    .filter(|branch| {
                        filter_trimmed.is_empty()
                            || branch.to_lowercase().contains(filter_trimmed.as_str())
                    })
                    .collect::<Vec<_>>();
                let visible_rows = filtered_branches.len().min(7) as f32;
                let list_height = px((visible_rows * 36.).max(36.));
                let list = div()
                    .mt(px(4.))
                    .rounded_md()
                    .bg(card_bg())
                    .border_1()
                    .border_color(border_col())
                    .shadow_md()
                    .overflow_hidden()
                    .child(
                        div()
                            .p(px(6.))
                            .border_b_1()
                            .border_color(border_col())
                            .child(
                                div()
                                    .id("new-task-source-branch-filter")
                                    .h(px(34.))
                                    .rounded_md()
                                    .bg(subtle_bg())
                                    .border_1()
                                    .border_color(if branch_filter_focused {
                                        hsla(220. / 360., 0.55, 0.60, 1.)
                                    } else {
                                        border_col()
                                    })
                                    .px(px(10.))
                                    .flex()
                                    .items_center()
                                    .cursor_text()
                                    .tooltip(move |_window, cx| {
                                        Self::action_tooltip_view("Filter branches by name", cx)
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, window, cx| {
                                            this.focus_handle.focus(window, cx);
                                            if let Some(state) = this.new_task_modal.as_mut() {
                                                if state.submitting {
                                                    return;
                                                }
                                                state.branch_filter_focused = true;
                                                state.branch_filter_cursor =
                                                    state.branch_filter.len();
                                                state.branch_filter_selection_anchor = None;
                                                state.task_name_focused = false;
                                                state.agent_dropdown_open = false;
                                            }
                                            cx.stop_propagation();
                                            cx.notify();
                                        }),
                                    )
                                    .child(Self::render_text_input_content(
                                        branch_filter.clone(),
                                        "Filter branches".into(),
                                        branch_filter_focused,
                                        branch_filter_cursor,
                                        branch_filter_selection.clone(),
                                        branch_filter.is_empty(),
                                        38,
                                    )),
                            ),
                    );

                let mut branch_rows = div()
                    .id("new-task-source-branch-results")
                    .h(list_height)
                    .overflow_y_scroll();
                let no_matching_branches = filtered_branches.is_empty();
                for branch in &filtered_branches {
                    let branch_name = (*branch).clone();
                    let branch_label: SharedString = (*branch).clone().into();
                    branch_rows = branch_rows.child(
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
                                let message = if branch_mode == NewTaskBranchMode::NewBranch {
                                    "Use this branch as the generated branch base"
                                } else {
                                    "Check out this branch directly"
                                };
                                Self::action_tooltip_view(message, cx)
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
                                        state.branch_filter_focused = false;
                                        state.agent_dropdown_open = false;
                                    }
                                    this.sync_new_task_modal_prewarm(cx);
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

                if no_matching_branches {
                    branch_rows = branch_rows.child(
                        div()
                            .h(px(36.))
                            .px(px(12.))
                            .flex()
                            .items_center()
                            .text_size(rems(13. / 16.))
                            .text_color(muted_col())
                            .child("No branches match"),
                    );
                }

                section = section.child(list.child(branch_rows));
            }
        } else if is_git_backed {
            section = section
                .child(
                    div()
                        .text_size(rems(12. / 16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(title_col())
                        .child("Current branch"),
                )
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
                            this.focus_handle.focus(window, cx);
                            if let Some(state) = this.new_task_modal.as_mut() {
                                if state.submitting {
                                    return;
                                }
                                state.task_name_focused = true;
                                state.task_name_cursor = state.task_name.len();
                                state.task_name_selection_anchor = None;
                                state.branch_dropdown_open = false;
                                state.branch_filter_focused = false;
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
        enabled_agents: &[&'static AgentDef],
        dropdown_open: bool,
        selected: &HashSet<String>,
        submitting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let display_agent = enabled_agents
            .iter()
            .find(|agent| selected.contains(agent.id))
            .copied()
            .or_else(|| enabled_agents.first().copied());

        let trigger_icon: SharedString = if selected.is_empty() {
            CLI_ONLY_ICON.into()
        } else {
            display_agent
                .map(|agent| agent.icon)
                .unwrap_or(AGENTS[0].icon)
                .into()
        };
        let trigger_label: SharedString = if selected.is_empty() {
            CLI_ONLY_LABEL.into()
        } else if selected.len() > 1 {
            format!("{} agents selected", selected.len()).into()
        } else {
            display_agent
                .map(|agent| agent.label)
                .unwrap_or("No enabled agents")
                .into()
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
                    .child("Agent / Terminal"),
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
                            Self::action_tooltip_view(
                                "Choose the agent or terminal for this task",
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
                                    state.agent_dropdown_open = !state.agent_dropdown_open;
                                    state.branch_dropdown_open = false;
                                    state.branch_filter_focused = false;
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
                                .child(branded_icon(trigger_icon, 18., Some(title_col())))
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
            section =
                section.child(self.render_agent_dropdown(enabled_agents, selected, submitting, cx));
        }

        section
    }

    fn render_agent_dropdown(
        &self,
        enabled_agents: &[&'static AgentDef],
        selected: &HashSet<String>,
        submitting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visible_rows = (enabled_agents.len() + 1).min(6) as f32;
        let dropdown_height = px(visible_rows * 36. + 8.);

        let mut list = div()
            .id("new-task-agent-dropdown")
            .mt(px(4.))
            .h(dropdown_height)
            .rounded_md()
            .bg(card_bg())
            .border_1()
            .border_color(border_col())
            .shadow_md()
            .overflow_y_scroll()
            .py(px(4.));

        list = list.child(
            div()
                .id("new-task-agent-cli-only")
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
                    Self::action_tooltip_view("Use a plain terminal for this task", cx)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        if let Some(state) = this.new_task_modal.as_mut() {
                            if state.submitting {
                                return;
                            }
                            state.selected_agents.clear();
                        }
                        this.sync_new_task_modal_prewarm(cx);
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
                        .border_color(if selected.is_empty() {
                            focus_col()
                        } else {
                            border_col()
                        })
                        .bg(if selected.is_empty() {
                            focus_col()
                        } else {
                            gpui::transparent_black()
                        })
                        .flex()
                        .items_center()
                        .justify_center()
                        .when(selected.is_empty(), |container| {
                            container.child(
                                svg()
                                    .path("assets/icons/icons__check.svg")
                                    .size(px(12.))
                                    .text_color(gpui::white()),
                            )
                        }),
                )
                .child(branded_icon(CLI_ONLY_ICON, 18., Some(title_col())))
                .child(
                    div()
                        .text_size(rems(13. / 16.))
                        .text_color(title_col())
                        .child(CLI_ONLY_LABEL),
                ),
        );

        for agent in enabled_agents {
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
                                if state.selected_agents.is_empty() {
                                    state.selected_agents.insert(agent_id.clone());
                                } else if state.selected_agents.contains(&agent_id) {
                                    state.selected_agents.remove(&agent_id);
                                } else {
                                    state.selected_agents.insert(agent_id.clone());
                                }
                            }
                            this.sync_new_task_modal_prewarm(cx);
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
                                focus_col()
                            } else {
                                border_col()
                            })
                            .bg(if is_selected {
                                focus_col()
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
                    .child(branded_icon(icon_path, 18., Some(title_col())))
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
                                        state.branch_filter_focused = false;
                                    }
                                    this.sync_new_task_modal_prewarm(cx);
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
                                        state.branch_filter_focused = false;
                                        state.agent_dropdown_open = false;
                                    }
                                    this.sync_new_task_modal_prewarm(cx);
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
                            state.branch_filter_focused = false;
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
            .border_color(new_task_theme().divider)
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
                    .bg(primary_button_bg())
                    .opacity(if submitting { 0.65 } else { 1.0 })
                    .hover(move |s| s.bg(primary_button_hover_bg()))
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(primary_button_text_col())
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
        Self::render_text_input_content(
            task_name,
            generated_task_name,
            focused,
            cursor,
            selection,
            is_empty,
            42,
        )
    }

    fn render_text_input_content(
        text: SharedString,
        placeholder: SharedString,
        focused: bool,
        cursor: usize,
        selection: Option<std::ops::Range<usize>>,
        is_empty: bool,
        max_chars: usize,
    ) -> impl IntoElement {
        let cursor = cursor.min(text.len());
        let selection =
            selection.map(|range| range.start.min(text.len())..range.end.min(text.len()));

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
                .child(div().text_color(placeholder_col()).child(placeholder));
        }

        let selected = selection.filter(|range| range.start < range.end);
        let visible_range = text_visible_range(&text, cursor, selected.as_ref(), max_chars);
        let leading_clipped = visible_range.start > 0;
        let trailing_clipped = visible_range.end < text.len();
        let visible_start = visible_range.start;
        let visible_text = text[visible_range.clone()].to_string();
        let local_cursor = cursor.saturating_sub(visible_start).min(visible_text.len());
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
                range.end.min(visible_text.len()),
            )
        } else {
            (
                local_cursor.min(visible_text.len()),
                local_cursor.min(visible_text.len()),
            )
        };

        let prefix = visible_text[..prefix_end].to_string();
        let middle = if let Some(range) = visible_selection.as_ref() {
            visible_text[range.clone()].to_string()
        } else {
            String::new()
        };
        let suffix_start = if selected_contains_cursor {
            selected_end
        } else {
            local_cursor.min(visible_text.len())
        };
        let between = if visible_selection
            .as_ref()
            .is_some_and(|range| range.end < local_cursor)
        {
            visible_text[selected_end..local_cursor.min(visible_text.len())].to_string()
        } else {
            String::new()
        };
        let trailing = visible_text[suffix_start..].to_string();

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
                    .bg(new_task_theme().info.muted)
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
                    .bg(new_task_theme().info.muted)
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
                    .bg(new_task_theme().info.muted)
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

fn text_visible_range(
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

pub(crate) fn selected_branch_filter_range(
    state: &NewTaskModalState,
) -> Option<std::ops::Range<usize>> {
    let anchor = state.branch_filter_selection_anchor?;
    if anchor == state.branch_filter_cursor {
        None
    } else if anchor < state.branch_filter_cursor {
        Some(anchor..state.branch_filter_cursor)
    } else {
        Some(state.branch_filter_cursor..anchor)
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

fn replace_branch_filter_range(
    state: &mut NewTaskModalState,
    range: std::ops::Range<usize>,
    new_text: &str,
) {
    state.branch_filter.replace_range(range.clone(), new_text);
    state.branch_filter_cursor = range.start + new_text.len();
    state.branch_filter_selection_anchor = None;
}

fn insert_branch_filter_text(state: &mut NewTaskModalState, text: &str) {
    let range = selected_branch_filter_range(state)
        .unwrap_or(state.branch_filter_cursor..state.branch_filter_cursor);
    replace_branch_filter_range(state, range, text);
}

fn delete_backward_in_branch_filter(state: &mut NewTaskModalState) {
    if let Some(range) = selected_branch_filter_range(state) {
        replace_branch_filter_range(state, range, "");
        return;
    }

    if state.branch_filter_cursor == 0 {
        return;
    }

    let start = previous_task_name_boundary(&state.branch_filter, state.branch_filter_cursor);
    replace_branch_filter_range(state, start..state.branch_filter_cursor, "");
}

fn delete_branch_filter_word_backward(state: &mut NewTaskModalState) {
    if let Some(range) = selected_branch_filter_range(state) {
        replace_branch_filter_range(state, range, "");
        return;
    }

    if state.branch_filter_cursor == 0 {
        return;
    }

    let start = previous_task_name_word_boundary(&state.branch_filter, state.branch_filter_cursor);
    replace_branch_filter_range(state, start..state.branch_filter_cursor, "");
}

fn delete_branch_filter_to_start(state: &mut NewTaskModalState) {
    if let Some(range) = selected_branch_filter_range(state) {
        replace_branch_filter_range(state, range, "");
        return;
    }

    if state.branch_filter_cursor == 0 {
        return;
    }

    replace_branch_filter_range(state, 0..state.branch_filter_cursor, "");
}

fn delete_forward_in_branch_filter(state: &mut NewTaskModalState) {
    if let Some(range) = selected_branch_filter_range(state) {
        replace_branch_filter_range(state, range, "");
        return;
    }

    if state.branch_filter_cursor >= state.branch_filter.len() {
        return;
    }

    let end = next_task_name_boundary(&state.branch_filter, state.branch_filter_cursor);
    replace_branch_filter_range(state, state.branch_filter_cursor..end, "");
}

fn move_branch_filter_cursor(
    state: &mut NewTaskModalState,
    direction: CursorDirection,
    extend_selection: bool,
) {
    let next_cursor = match direction {
        CursorDirection::Left => {
            if let Some(range) = selected_branch_filter_range(state) {
                if extend_selection {
                    previous_task_name_boundary(&state.branch_filter, state.branch_filter_cursor)
                } else {
                    range.start
                }
            } else {
                previous_task_name_boundary(&state.branch_filter, state.branch_filter_cursor)
            }
        }
        CursorDirection::Right => {
            if let Some(range) = selected_branch_filter_range(state) {
                if extend_selection {
                    next_task_name_boundary(&state.branch_filter, state.branch_filter_cursor)
                } else {
                    range.end
                }
            } else {
                next_task_name_boundary(&state.branch_filter, state.branch_filter_cursor)
            }
        }
    };

    if extend_selection {
        if state.branch_filter_selection_anchor.is_none() {
            state.branch_filter_selection_anchor = Some(state.branch_filter_cursor);
        }
    } else {
        state.branch_filter_selection_anchor = None;
    }

    state.branch_filter_cursor = next_cursor;
}

fn move_branch_filter_cursor_to_edge(
    state: &mut NewTaskModalState,
    to_end: bool,
    extend_selection: bool,
) {
    if extend_selection && state.branch_filter_selection_anchor.is_none() {
        state.branch_filter_selection_anchor = Some(state.branch_filter_cursor);
    }
    if !extend_selection {
        state.branch_filter_selection_anchor = None;
    }
    state.branch_filter_cursor = if to_end { state.branch_filter.len() } else { 0 };
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

pub(crate) fn generate_task_name() -> String {
    const FIRST: &[&str] = &[
        "quiet",
        "silver",
        "bright",
        "steady",
        "wild",
        "mellow",
        "brisk",
        "neat",
        "rad",
        "fresh",
        "fly",
        "phat",
        "pixel",
        "neon",
        "grunge",
        "turbo",
        "cosmic",
        "mall",
        "arcade",
        "saturday",
        "vhs",
        "tamagotchi",
        "zelda",
        "sonic",
        "clueless",
        "spice",
        "matrix",
        "dialup",
    ];
    const SECOND: &[&str] = &[
        "river",
        "meadow",
        "comet",
        "signal",
        "forest",
        "ember",
        "harbor",
        "planet",
        "sitcom",
        "beeper",
        "rewind",
        "moonwalk",
        "blockbuster",
        "gameboy",
        "dreamcast",
        "trapper",
        "windbreaker",
        "discman",
        "boyband",
        "chatroom",
        "supernova",
        "slammer",
        "ranger",
        "tamagotchi",
        "seinfeld",
        "xfiles",
        "jukebox",
        "afterparty",
    ];
    const THIRD: &[&str] = &[
        "sparks",
        "travels",
        "builds",
        "drifts",
        "guides",
        "moves",
        "lands",
        "echoes",
        "remix",
        "rewinds",
        "glows",
        "bounces",
        "rips",
        "slaps",
        "glitches",
        "downloads",
        "pages",
        "beams",
        "boogies",
        "radicals",
        "jams",
        "surfs",
        "shuffles",
        "blasts",
        "hangs",
        "grooves",
        "rules",
        "zooms",
    ];
    const PHRASES: &[&str] = &[
        "you-sure-about-that",
        "why-the-tables",
        "coffin-flop",
        "corncob-tv",
        "sloppy-steaks",
        "lets-slop-em-up",
        "white-ferrari",
        "ghost-tour",
        "santa-brought-it-early",
        "baby-of-the-year",
        "karl-havoc",
        "im-so-hot",
        "dan-flashes",
        "jamie-taco",
        "gimme-dat",
        "brians-hat",
        "turbo-team",
        "tc-tuggers",
        "calico-cut-pants",
        "its-not-a-joke",
        "you-gotta-give",
        "motorcycle-guys",
    ];

    let bytes = *Uuid::new_v4().as_bytes();
    let combo_count = FIRST.len() * SECOND.len() * THIRD.len();
    let total_count = combo_count + PHRASES.len();
    let choice =
        u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize % total_count;

    if choice < combo_count {
        let third_count = THIRD.len();
        let second_third_count = SECOND.len() * third_count;
        let first_index = choice / second_third_count;
        let remainder = choice % second_third_count;
        let second_index = remainder / third_count;
        let third_index = remainder % third_count;

        format!(
            "{}-{}-{}",
            FIRST[first_index], SECOND[second_index], THIRD[third_index]
        )
    } else {
        PHRASES[choice - combo_count].to_string()
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{default_new_task_agent_id, sanitized_new_task_selected_agents};
    use crate::agents::{AGENTS, DEFAULT_AGENT_ID};

    #[test]
    fn default_selection_prefers_default_agent_when_enabled() {
        let enabled_agents = vec![&AGENTS[1], &AGENTS[4], &AGENTS[6]];

        assert_eq!(
            default_new_task_agent_id(&enabled_agents, Some(DEFAULT_AGENT_ID)),
            Some(DEFAULT_AGENT_ID)
        );
    }

    #[test]
    fn default_selection_falls_back_to_first_enabled_agent() {
        // DEFAULT_AGENT_ID ("codex" = AGENTS[1]) intentionally omitted
        // from the enabled list so the function exercises the
        // fallback-to-first branch. With it present the test would
        // (correctly) return DEFAULT_AGENT_ID and never hit the
        // fallback path it claims to cover.
        let enabled_agents = vec![&AGENTS[0], &AGENTS[2]];

        assert_eq!(
            default_new_task_agent_id(&enabled_agents, Some(DEFAULT_AGENT_ID)),
            Some(AGENTS[0].id)
        );
    }

    #[test]
    fn default_selection_returns_none_when_no_agents_are_enabled() {
        assert_eq!(default_new_task_agent_id(&[], Some(DEFAULT_AGENT_ID)), None);
    }

    #[test]
    fn sanitization_preserves_terminal_selection() {
        assert_eq!(
            sanitized_new_task_selected_agents(&HashSet::new(), &[]),
            HashSet::new()
        );
    }

    #[test]
    fn sanitization_removes_disabled_agent_ids_from_selection() {
        let enabled_agents = vec![&AGENTS[1], &AGENTS[4]];
        let selected_agents = HashSet::from([
            AGENTS[0].id.to_string(),
            AGENTS[1].id.to_string(),
            AGENTS[4].id.to_string(),
        ]);

        assert_eq!(
            sanitized_new_task_selected_agents(&selected_agents, &enabled_agents),
            HashSet::from([AGENTS[1].id.to_string(), AGENTS[4].id.to_string()])
        );
    }
}
