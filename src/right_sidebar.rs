//! Right sidebar content: changed files and branch actions.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, ease_in_out, hsla, percentage, prelude::*, px, rems, rgb, svg, uniform_list, Animation,
    AnimationExt as _, AnyElement, Context, KeyDownEvent, MouseButton, MouseDownEvent,
    SharedString, Transformation, Window,
};

use crate::app::AnotherOneApp;
use crate::git_actions::ToolbarGitAction;
use crate::theme;

const TOOLBAR_RIGHT_PAD: f32 = 8.;
const TOOLBAR_ACTION_GAP: f32 = 4.;
const TOOLBAR_MENU_TOP: f32 = 40.;
const PUSH_SPLIT_BUTTON_W: f32 = 68.;
const PUSH_SPLIT_BUTTON_W_WITH_COUNT: f32 = 82.;
const CREATE_PR_SPLIT_BUTTON_W: f32 = 96.;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChangeGroup {
    Staged,
    Uncommitted,
}

#[derive(Clone)]
enum ChangedFilesListEntry {
    Header {
        section_key: &'static str,
        title: &'static str,
        group: ChangeGroup,
        file_count: usize,
        additions: i32,
        deletions: i32,
    },
    File {
        group: ChangeGroup,
        file_index: usize,
    },
}

#[derive(Clone)]
struct ChangedFilesRowSnapshot {
    path: SharedString,
    file_name: SharedString,
    parent_dir: Option<SharedString>,
    staged_status: char,
    staged_status_color: gpui::Hsla,
    unstaged_status: char,
    unstaged_status_color: gpui::Hsla,
    staged_additions: i32,
    staged_deletions: i32,
    unstaged_additions: i32,
    unstaged_deletions: i32,
    can_stage: bool,
    can_unstage: bool,
}

#[derive(Clone)]
pub(crate) struct ChangedFilesListSnapshot {
    project_id: String,
    files: Arc<[crate::project_store::ChangedFile]>,
    rows: Arc<[ChangedFilesRowSnapshot]>,
    staged_indices: Arc<[usize]>,
    unstaged_indices: Arc<[usize]>,
    staged_additions: i32,
    staged_deletions: i32,
    unstaged_additions: i32,
    unstaged_deletions: i32,
}

impl ChangedFilesListSnapshot {
    fn item_count(&self, staged_collapsed: bool, uncommitted_collapsed: bool) -> usize {
        let mut count = 0;

        if !self.staged_indices.is_empty() {
            count += 1;
            if !staged_collapsed {
                count += self.staged_indices.len();
            }
        }

        if !self.unstaged_indices.is_empty() {
            count += 1;
            if !uncommitted_collapsed {
                count += self.unstaged_indices.len();
            }
        }

        count
    }

    fn entry_at(
        &self,
        staged_collapsed: bool,
        uncommitted_collapsed: bool,
        mut index: usize,
    ) -> Option<ChangedFilesListEntry> {
        if !self.staged_indices.is_empty() {
            if index == 0 {
                return Some(ChangedFilesListEntry::Header {
                    section_key: "staged",
                    title: "Staged Changes",
                    group: ChangeGroup::Staged,
                    file_count: self.staged_indices.len(),
                    additions: self.staged_additions,
                    deletions: self.staged_deletions,
                });
            }

            index -= 1;
            if !staged_collapsed {
                if index < self.staged_indices.len() {
                    return Some(ChangedFilesListEntry::File {
                        group: ChangeGroup::Staged,
                        file_index: self.staged_indices[index],
                    });
                }
                index -= self.staged_indices.len();
            }
        }

        if !self.unstaged_indices.is_empty() {
            if index == 0 {
                return Some(ChangedFilesListEntry::Header {
                    section_key: "uncommitted",
                    title: "Changes",
                    group: ChangeGroup::Uncommitted,
                    file_count: self.unstaged_indices.len(),
                    additions: self.unstaged_additions,
                    deletions: self.unstaged_deletions,
                });
            }

            index -= 1;
            if !uncommitted_collapsed && index < self.unstaged_indices.len() {
                return Some(ChangedFilesListEntry::File {
                    group: ChangeGroup::Uncommitted,
                    file_index: self.unstaged_indices[index],
                });
            }
        }

        None
    }
}

impl AnotherOneApp {
    fn changed_files_list_snapshot(
        &mut self,
        project_id: &str,
        changed_files: &Arc<[crate::project_store::ChangedFile]>,
    ) -> ChangedFilesListSnapshot {
        if let Some(snapshot) = self.changed_files_list_snapshots.get(project_id) {
            if Arc::ptr_eq(&snapshot.files, changed_files) {
                return snapshot.clone();
            }
        }

        let snapshot =
            self.build_changed_files_list_snapshot(project_id.to_string(), changed_files.clone());
        self.changed_files_list_snapshots
            .insert(project_id.to_string(), snapshot.clone());
        snapshot
    }

    fn changed_file_for_action(
        &self,
        project_id: &str,
        file_index: usize,
    ) -> Option<crate::project_store::ChangedFile> {
        self.changed_files
            .get(project_id)
            .and_then(|files| files.get(file_index))
            .cloned()
    }

    fn changed_files_for_action_indices(
        &self,
        project_id: &str,
        file_indices: &[usize],
    ) -> Vec<crate::project_store::ChangedFile> {
        let Some(files) = self.changed_files.get(project_id) else {
            return Vec::new();
        };

        file_indices
            .iter()
            .filter_map(|index| files.get(*index))
            .cloned()
            .collect()
    }

    fn toolbar_spinner(icon_color: gpui::Hsla, size_px: f32) -> impl IntoElement {
        svg()
            .path("assets/icons/icons__refresh.svg")
            .size(px(size_px))
            .text_color(icon_color)
            .with_animation(
                "toolbar-spinner",
                Animation::new(Duration::from_secs_f64(0.8))
                    .repeat()
                    .with_easing(ease_in_out),
                |svg, delta| svg.with_transformation(Transformation::rotate(percentage(delta))),
            )
    }

    fn toolbar_action_active(&self, action: ToolbarGitAction) -> bool {
        self.active_git_action == Some(action)
    }

    fn push_action_active(&self) -> bool {
        matches!(self.active_git_action, Some(ToolbarGitAction::Push { .. }))
    }

    fn create_pr_action_active(&self) -> bool {
        matches!(
            self.active_git_action,
            Some(ToolbarGitAction::CreatePr { .. })
        )
    }

    fn active_branch_ahead_count(&self) -> usize {
        let Some(section) = self.active_section.as_ref() else {
            return 0;
        };

        self.project_store
            .projects
            .iter()
            .find(|project| project.id == section.project_id)
            .and_then(|project| {
                project
                    .branches
                    .iter()
                    .find(|branch| branch.name == section.branch_name)
            })
            .map(|branch| branch.ahead_count)
            .unwrap_or(0)
    }

    fn git_diff_badge(value: i32, positive: bool, font_px: f32) -> impl IntoElement {
        let (fg, text) = if positive {
            (hsla(138. / 360., 0.50, 0.74, 1.), format!("+{value}"))
        } else {
            (hsla(352. / 360., 0.52, 0.76, 1.), format!("-{value}"))
        };

        div()
            .text_color(fg)
            .text_size(rems((font_px.min(11.)) / 16.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .child(text)
    }

    fn changed_file_action_button(
        button_id: impl Into<gpui::ElementId>,
        icon_path: &'static str,
        enabled: bool,
        hover_bg: gpui::Hsla,
        icon_color: gpui::Hsla,
        tooltip_label: Option<&'static str>,
        on_click: impl Fn(&mut Self, &MouseDownEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut button = div()
            .id(button_id)
            .flex()
            .items_center()
            .justify_center()
            .w(px(28.))
            .h(px(28.))
            .rounded_md()
            .opacity(if enabled { 1. } else { 0.35 });

        if enabled {
            button = button
                .cursor_pointer()
                .hover(move |style| style.bg(hover_bg))
                .on_mouse_down(MouseButton::Left, cx.listener(on_click));

            if let Some(label) = tooltip_label {
                button = button.tooltip(move |_window, cx| Self::action_tooltip_view(label, cx));
            }
        }

        button.child(svg().path(icon_path).size(px(16.)).text_color(icon_color))
    }

    fn changed_file_action_pending(icon_color: gpui::Hsla) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_center()
            .w(px(28.))
            .h(px(28.))
            .child(Self::toolbar_spinner(icon_color, 14.))
    }

    fn git_toolbar_button(
        label: &'static str,
        leading_icon: Option<&'static str>,
        trailing_icon: Option<&'static str>,
        enabled: bool,
        active: bool,
        tooltip_label: Option<&'static str>,
        on_click: impl Fn(&mut Self, &MouseDownEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visually_enabled = enabled || active;
        let text_col = if visually_enabled {
            hsla(0., 0., 0.94, 1.)
        } else {
            hsla(0., 0., 0.48, 1.)
        };
        let icon_col = if visually_enabled {
            hsla(0., 0., 0.82, 1.)
        } else {
            hsla(0., 0., 0.42, 1.)
        };
        let bg = rgb(0x1e2024);
        let border = gpui::white().opacity(0.08);
        let hover_bg = gpui::white().opacity(0.06);

        let mut button = div()
            .id(SharedString::from(format!("git-toolbar-{label}")))
            .relative()
            .flex()
            .items_center()
            .h(px(30.))
            .px(px(7.))
            .rounded(px(7.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .opacity(if visually_enabled { 1. } else { 0.55 });

        let mut content = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(5.))
            .opacity(if active { 0. } else { 1. });

        if let Some(icon_path) = leading_icon {
            content = content.child(svg().path(icon_path).size(px(12.)).text_color(icon_col));
        }

        content = content.child(
            div()
                .text_size(rems(11. / 16.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text_col)
                .child(label),
        );

        if let Some(icon_path) = trailing_icon {
            content = content.child(svg().path(icon_path).size(px(11.)).text_color(icon_col));
        }

        button = button.child(content);

        if active {
            button = button.child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(Self::toolbar_spinner(icon_col, 12.)),
            );
        }

        if let Some(tip) = tooltip_label {
            button = button.tooltip(move |_window, cx| Self::action_tooltip_view(tip, cx));
        }

        button.when(enabled && !active, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(hover_bg))
                .on_mouse_down(MouseButton::Left, cx.listener(on_click))
        })
    }

    fn create_pr_split_button(&self, enabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.create_pr_action_active();
        let interactive = enabled && !active;
        let visually_enabled = enabled || active;
        let text_col = if visually_enabled {
            hsla(0., 0., 0.94, 1.)
        } else {
            hsla(0., 0., 0.48, 1.)
        };
        let icon_col = if visually_enabled {
            hsla(0., 0., 0.82, 1.)
        } else {
            hsla(0., 0., 0.42, 1.)
        };
        let bg = rgb(0x1e2024);
        let border = gpui::white().opacity(0.08);
        let hover_bg = gpui::white().opacity(0.06);
        let divider = gpui::white().opacity(0.10);
        let is_open = self.create_pr_menu_open;

        div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .w(px(CREATE_PR_SPLIT_BUTTON_W))
            .h(px(30.))
            .rounded(px(7.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .opacity(if visually_enabled { 1. } else { 0.55 })
            .child(
                div()
                    .id("create-pr-main")
                    .relative()
                    .flex()
                    .items_center()
                    .flex_1()
                    .h_full()
                    .px(px(7.))
                    .rounded_l(px(6.))
                    .when(interactive, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Create a pull request for the current branch",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.push_menu_open = false;
                                    this.create_pr_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::CreatePr { draft: false },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        div()
                            .opacity(if active { 0. } else { 1. })
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(text_col)
                            .child("Create PR"),
                    ),
            )
            .child(div().w(px(1.)).h(px(14.)).bg(divider))
            .child(
                div()
                    .id("create-pr-chevron")
                    .flex()
                    .items_center()
                    .justify_center()
                    .h_full()
                    .w(px(22.))
                    .rounded_r(px(6.))
                    .when(is_open && interactive, |d| d.bg(hover_bg))
                    .when(interactive, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view("More create pull request options", cx)
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.push_menu_open = false;
                                    this.create_pr_menu_open = !this.create_pr_menu_open;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__chevron-down.svg")
                            .size(px(11.))
                            .text_color(icon_col),
                    ),
            )
            .when(active, |button| {
                button.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .bottom(px(0.))
                        .right(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Self::toolbar_spinner(icon_col, 12.)),
                )
            })
    }

    fn create_pr_dropdown_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.create_pr_menu_open || self.active_git_action.is_some() {
            return div().id("create-pr-menu");
        }

        let bg = rgb(0x2b2d31);
        let border = gpui::white().opacity(0.08);
        let text_col = hsla(0., 0., 0.92, 1.);
        let hover_bg = gpui::white().opacity(0.06);

        div()
            .id("create-pr-menu")
            .absolute()
            .right(px(TOOLBAR_RIGHT_PAD))
            .top(px(TOOLBAR_MENU_TOP))
            .on_mouse_down_out(cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                this.create_pr_menu_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .w(px(160.))
                    .rounded_md()
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .shadow_md()
                    .occlude()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .id("pr-menu-create")
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(34.))
                            .px(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Create a pull request for the current branch",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.create_pr_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::CreatePr { draft: false },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(text_col)
                                    .child("Create PR"),
                            ),
                    )
                    .child(
                        div()
                            .id("pr-menu-draft")
                            .flex()
                            .items_center()
                            .gap(px(8.))
                            .h(px(34.))
                            .px(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Create a draft pull request for the current branch",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.create_pr_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::CreatePr { draft: true },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(text_col)
                                    .child("Draft PR"),
                            ),
                    ),
            )
    }

    fn create_pr_control(&self, enabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        self.create_pr_split_button(enabled, cx)
    }

    fn push_split_button(&self, enabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        let active = self.push_action_active();
        let interactive = enabled && !active;
        let visually_enabled = enabled || active;
        let ahead_count = self.active_branch_ahead_count();
        let push_label = if ahead_count > 0 {
            format!("Push ({ahead_count})")
        } else {
            "Push".to_string()
        };
        let text_col = if visually_enabled {
            hsla(0., 0., 0.94, 1.)
        } else {
            hsla(0., 0., 0.48, 1.)
        };
        let icon_col = if visually_enabled {
            hsla(0., 0., 0.82, 1.)
        } else {
            hsla(0., 0., 0.42, 1.)
        };
        let bg = rgb(0x1e2024);
        let border = gpui::white().opacity(0.08);
        let hover_bg = gpui::white().opacity(0.06);
        let divider = gpui::white().opacity(0.10);
        let is_open = self.push_menu_open;

        div()
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .w(px(if ahead_count > 0 {
                PUSH_SPLIT_BUTTON_W_WITH_COUNT
            } else {
                PUSH_SPLIT_BUTTON_W
            }))
            .h(px(30.))
            .rounded(px(7.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .opacity(if visually_enabled { 1. } else { 0.55 })
            .child(
                div()
                    .id("push-main")
                    .relative()
                    .flex()
                    .items_center()
                    .flex_1()
                    .h_full()
                    .px(px(7.))
                    .rounded_l(px(6.))
                    .when(interactive, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Push the current checked-out branch to its remote",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.create_pr_menu_open = false;
                                    this.push_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::Push { force: false },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        div()
                            .opacity(if active { 0. } else { 1. })
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(text_col)
                            .child(push_label),
                    ),
            )
            .child(div().w(px(1.)).h(px(14.)).bg(divider))
            .child(
                div()
                    .id("push-chevron")
                    .flex()
                    .items_center()
                    .justify_center()
                    .h_full()
                    .w(px(22.))
                    .rounded_r(px(6.))
                    .when(is_open && interactive, |d| d.bg(hover_bg))
                    .when(interactive, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view("Show push options", cx)
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.create_pr_menu_open = false;
                                    this.push_menu_open = !this.push_menu_open;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__chevron-down.svg")
                            .size(px(11.))
                            .text_color(icon_col),
                    ),
            )
            .when(active, |button| {
                button.child(
                    div()
                        .absolute()
                        .left(px(0.))
                        .top(px(0.))
                        .bottom(px(0.))
                        .right(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Self::toolbar_spinner(icon_col, 12.)),
                )
            })
    }

    fn push_dropdown_menu(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.push_menu_open || self.active_git_action.is_some() {
            return div().id("push-menu");
        }

        let bg = rgb(0x2b2d31);
        let border = gpui::white().opacity(0.08);
        let danger_col = hsla(0., 0.78, 0.72, 1.);
        let danger_hover = hsla(0., 0.45, 0.34, 0.26);

        div()
            .id("push-menu")
            .absolute()
            .right(px(
                TOOLBAR_RIGHT_PAD + CREATE_PR_SPLIT_BUTTON_W + TOOLBAR_ACTION_GAP,
            ))
            .top(px(TOOLBAR_MENU_TOP))
            .on_mouse_down_out(cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                this.push_menu_open = false;
                cx.notify();
            }))
            .child(
                div()
                    .w(px(168.))
                    .rounded_md()
                    .bg(bg)
                    .border_1()
                    .border_color(border)
                    .shadow_md()
                    .occlude()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .id("push-menu-force")
                            .flex()
                            .items_center()
                            .h(px(34.))
                            .px(px(12.))
                            .cursor_pointer()
                            .hover(move |s| s.bg(danger_hover))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Force-push with lease to overwrite the remote branch if needed",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.create_pr_menu_open = false;
                                    this.push_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::Push { force: true },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(danger_col)
                                    .child("Force Push"),
                            ),
                    ),
            )
    }

    fn push_control(&self, enabled: bool, cx: &mut Context<Self>) -> impl IntoElement {
        self.push_split_button(enabled, cx)
    }

    fn changed_file_status_char(
        changed: &crate::project_store::ChangedFile,
        group: ChangeGroup,
    ) -> char {
        let raw = match group {
            ChangeGroup::Staged => changed.index_status,
            ChangeGroup::Uncommitted => {
                if changed.untracked {
                    'A'
                } else {
                    changed.worktree_status
                }
            }
        };

        match raw {
            '?' => 'A',
            ' ' => 'M',
            other => other,
        }
    }

    fn changed_file_status_color(status: char) -> gpui::Hsla {
        match status {
            'A' => hsla(135. / 360., 0.70, 0.68, 1.),
            'D' => hsla(0., 0.72, 0.68, 1.),
            'R' | 'C' => hsla(210. / 360., 0.72, 0.72, 1.),
            _ => hsla(50. / 360., 0.90, 0.60, 1.),
        }
    }

    fn build_changed_files_list_snapshot(
        &self,
        project_id: String,
        changed_files: Arc<[crate::project_store::ChangedFile]>,
    ) -> ChangedFilesListSnapshot {
        let mut rows = Vec::with_capacity(changed_files.len());
        let mut staged_indices = Vec::new();
        let mut unstaged_indices = Vec::new();
        let mut staged_additions = 0;
        let mut staged_deletions = 0;
        let mut unstaged_additions = 0;
        let mut unstaged_deletions = 0;

        for (file_index, changed) in changed_files.iter().enumerate() {
            let file_name = Path::new(&changed.path)
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(changed.path.as_str())
                .to_string();
            let parent_dir = Path::new(&changed.path)
                .parent()
                .and_then(|parent| parent.to_str())
                .filter(|parent| !parent.is_empty() && *parent != ".")
                .map(|parent| SharedString::from(parent.to_string()));
            let staged_status = Self::changed_file_status_char(changed, ChangeGroup::Staged);
            let unstaged_status = Self::changed_file_status_char(changed, ChangeGroup::Uncommitted);
            let can_stage = changed.can_stage();
            let can_unstage = changed.can_unstage();
            let has_staged_changes = changed.has_staged_changes();
            let has_unstaged_changes = changed.has_unstaged_changes();

            rows.push(ChangedFilesRowSnapshot {
                path: SharedString::from(changed.path.clone()),
                file_name: SharedString::from(file_name),
                parent_dir,
                staged_status,
                staged_status_color: Self::changed_file_status_color(staged_status),
                unstaged_status,
                unstaged_status_color: Self::changed_file_status_color(unstaged_status),
                staged_additions: changed.staged_additions,
                staged_deletions: changed.staged_deletions,
                unstaged_additions: changed.unstaged_additions,
                unstaged_deletions: changed.unstaged_deletions,
                can_stage,
                can_unstage,
            });

            if has_staged_changes {
                staged_indices.push(file_index);
                staged_additions += changed.staged_additions.max(0);
                staged_deletions += changed.staged_deletions.max(0);
            }
            if has_unstaged_changes {
                unstaged_indices.push(file_index);
                unstaged_additions += changed.unstaged_additions.max(0);
                unstaged_deletions += changed.unstaged_deletions.max(0);
            }
        }

        ChangedFilesListSnapshot {
            project_id,
            files: changed_files,
            rows: Arc::from(rows),
            staged_indices: Arc::from(staged_indices),
            unstaged_indices: Arc::from(unstaged_indices),
            staged_additions,
            staged_deletions,
            unstaged_additions,
            unstaged_deletions,
        }
    }

    fn changed_files_list_item(
        &self,
        snapshot: &ChangedFilesListSnapshot,
        entry: &ChangedFilesListEntry,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        match entry {
            ChangedFilesListEntry::Header {
                section_key,
                title,
                group,
                file_count,
                additions,
                deletions,
            } => {
                let section_indices = match group {
                    ChangeGroup::Staged => snapshot.staged_indices.clone(),
                    ChangeGroup::Uncommitted => snapshot.unstaged_indices.clone(),
                };
                self.changed_file_section_header(
                    section_key,
                    title,
                    &snapshot.project_id,
                    section_indices,
                    *group,
                    *file_count,
                    *additions,
                    *deletions,
                    cx,
                )
                .into_any_element()
            }
            ChangedFilesListEntry::File { group, file_index } => snapshot
                .rows
                .get(*file_index)
                .map(|row| {
                    self.changed_file_row(&snapshot.project_id, *file_index, row, *group, cx)
                })
                .unwrap_or_else(|| div().into_any_element()),
        }
    }

    fn changed_file_row(
        &self,
        project_id: &str,
        file_index: usize,
        row: &ChangedFilesRowSnapshot,
        group: ChangeGroup,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title_col = hsla(0., 0., 0.94, 1.);
        let path_col = hsla(0., 0., 0.58, 1.);
        let row_hover = gpui::white().opacity(0.04);
        let action_hover = gpui::white().opacity(0.08);
        let action_icon = hsla(0., 0., 0.72, 1.);
        let actions_busy = self.changed_files_actions_busy(project_id);
        let file_pending = self.changed_files_file_pending(project_id, row.path.as_ref());
        let project_mutations_pending = self.changed_files_project_mutations_pending(project_id);
        let (additions, deletions, status, status_color, can_stage, can_unstage) = match group {
            ChangeGroup::Staged => (
                row.staged_additions,
                row.staged_deletions,
                row.staged_status,
                row.staged_status_color,
                row.can_stage,
                row.can_unstage,
            ),
            ChangeGroup::Uncommitted => (
                row.unstaged_additions,
                row.unstaged_deletions,
                row.unstaged_status,
                row.unstaged_status_color,
                row.can_stage,
                row.can_unstage,
            ),
        };
        let stage_project_id = project_id.to_string();
        let unstage_project_id = project_id.to_string();
        let revert_project_id = project_id.to_string();
        let group_key = match group {
            ChangeGroup::Staged => "staged",
            ChangeGroup::Uncommitted => "uncommitted",
        };

        let mut stats = div().flex().flex_row().items_center().gap(px(8.));
        if additions > 0 {
            stats = stats.child(Self::git_diff_badge(additions, true, 12.));
        }
        if deletions > 0 {
            stats = stats.child(Self::git_diff_badge(deletions, false, 12.));
        }
        stats = match group {
            ChangeGroup::Staged => stats.child(div().child(if file_pending {
                Self::changed_file_action_pending(action_icon).into_any_element()
            } else {
                Self::changed_file_action_button(
                    ("changed-file-unstage", file_index),
                    "assets/icons/icons__minus.svg",
                    can_unstage && !actions_busy,
                    action_hover,
                    action_icon,
                    Some("Unstage file"),
                    move |this, _ev, _window, cx| {
                        if let Some(changed) =
                            this.changed_file_for_action(&unstage_project_id, file_index)
                        {
                            this.unstage_changed_file(&unstage_project_id, &changed, cx);
                        }
                        cx.notify();
                    },
                    cx,
                )
                .into_any_element()
            })),
            ChangeGroup::Uncommitted => stats
                .child(div().child(if file_pending {
                    Self::changed_file_action_pending(action_icon).into_any_element()
                } else {
                    Self::changed_file_action_button(
                        ("changed-file-stage", file_index),
                        "assets/icons/icons__plus.svg",
                        can_stage && !actions_busy,
                        action_hover,
                        action_icon,
                        Some("Stage File"),
                        move |this, _ev, _window, cx| {
                            if let Some(changed) =
                                this.changed_file_for_action(&stage_project_id, file_index)
                            {
                                this.stage_changed_file(&stage_project_id, &changed, cx);
                            }
                            cx.notify();
                        },
                        cx,
                    )
                    .into_any_element()
                }))
                .child(div().child(Self::changed_file_action_button(
                    ("changed-file-discard", file_index),
                    "assets/icons/icons__discard.svg",
                    !actions_busy && !project_mutations_pending,
                    action_hover,
                    action_icon,
                    Some("Discard File Changes"),
                    move |this, _ev, _window, cx| {
                        if let Some(changed) =
                            this.changed_file_for_action(&revert_project_id, file_index)
                        {
                            this.discard_confirm = Some((revert_project_id.clone(), vec![changed]));
                        }
                        cx.notify();
                    },
                    cx,
                ))),
        };

        div()
            .id(SharedString::from(format!(
                "changed-file-row-{project_id}-{group_key}-{file_index}"
            )))
            .w_full()
            .h(px(34.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .pl(px(22.))
            .pr(px(14.))
            .rounded_md()
            .mx(px(4.))
            .hover(move |style| style.bg(row_hover))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(12.))
                    .min_w(px(0.))
                    .flex_1()
                    .child(
                        div()
                            .min_w(px(18.))
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(status_color)
                            .child(status.to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .min_w(px(0.))
                            .flex_1()
                            .overflow_hidden()
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(title_col)
                                    .child(row.file_name.clone()),
                            )
                            .when(row.parent_dir.is_some(), |entry| {
                                entry.child(
                                    div()
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_size(rems(11. / 16.))
                                        .text_color(path_col)
                                        .child(row.parent_dir.clone().unwrap_or_default()),
                                )
                            }),
                    ),
            )
            .child(stats)
            .into_any_element()
    }

    fn changed_file_section_header(
        &self,
        section_key: &'static str,
        title: &'static str,
        project_id: &str,
        section_indices: Arc<[usize]>,
        group: ChangeGroup,
        file_count: usize,
        section_additions: i32,
        section_deletions: i32,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let border = gpui::white().opacity(0.06);
        let title_col = hsla(0., 0., 0.92, 1.);
        let count_col = hsla(0., 0., 0.74, 1.);
        let action_hover = gpui::white().opacity(0.08);
        let action_icon = hsla(0., 0., 0.72, 1.);
        let header_hover = gpui::white().opacity(0.03);
        let collapsed = self.collapsed_change_sections.contains(section_key);
        let actions_busy = self.changed_files_actions_busy(project_id);
        let project_mutations_pending = self.changed_files_project_mutations_pending(project_id);
        let stage_all_pending = self.changed_files_stage_all_pending(project_id);
        let unstage_all_pending = self.changed_files_unstage_all_pending(project_id);

        let section_actions = match group {
            ChangeGroup::Staged => {
                let unstage_project_id = project_id.to_string();
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(if unstage_all_pending {
                        Self::changed_file_action_pending(action_icon).into_any_element()
                    } else {
                        Self::changed_file_action_button(
                            SharedString::from(format!(
                                "changed-section-action-{}-staged-unstage",
                                project_id
                            )),
                            "assets/icons/icons__minus.svg",
                            !actions_busy,
                            action_hover,
                            action_icon,
                            Some("Unstage all files in this section"),
                            move |this, _ev, _window, cx| {
                                cx.stop_propagation();
                                this.unstage_all_changes(&unstage_project_id, cx);
                                cx.notify();
                            },
                            cx,
                        )
                        .into_any_element()
                    })
            }
            ChangeGroup::Uncommitted => {
                let stage_project_id = project_id.to_string();
                let discard_project_id = project_id.to_string();
                let discard_indices = section_indices.clone();

                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(if stage_all_pending {
                        Self::changed_file_action_pending(action_icon).into_any_element()
                    } else {
                        Self::changed_file_action_button(
                            SharedString::from(format!(
                                "changed-section-action-{}-changes-stage",
                                project_id
                            )),
                            "assets/icons/icons__plus.svg",
                            !actions_busy,
                            action_hover,
                            action_icon,
                            Some("Stage All Changes"),
                            move |this, _ev, _window, cx| {
                                cx.stop_propagation();
                                this.stage_all_changes(&stage_project_id, cx);
                                cx.notify();
                            },
                            cx,
                        )
                        .into_any_element()
                    })
                    .child(Self::changed_file_action_button(
                        SharedString::from(format!(
                            "changed-section-action-{}-changes-discard",
                            project_id
                        )),
                        "assets/icons/icons__discard.svg",
                        !actions_busy && !project_mutations_pending,
                        action_hover,
                        action_icon,
                        Some("Discard All Changes"),
                        move |this, _ev, _window, cx| {
                            cx.stop_propagation();
                            this.discard_confirm = Some((
                                discard_project_id.clone(),
                                this.changed_files_for_action_indices(
                                    &discard_project_id,
                                    discard_indices.as_ref(),
                                ),
                            ));
                            cx.notify();
                        },
                        cx,
                    ))
            }
        };

        div()
            .w_full()
            .h(px(34.))
            .id(SharedString::from(format!(
                "change-section-header-{}",
                section_key
            )))
            .flex()
            .items_center()
            .justify_between()
            .px(px(14.))
            .border_b_1()
            .border_color(border)
            .cursor_pointer()
            .hover(move |s| s.bg(header_hover))
            .tooltip(move |_window, cx| {
                Self::action_tooltip_view("Expand or collapse this section", cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    if this.collapsed_change_sections.contains(section_key) {
                        this.collapsed_change_sections.remove(section_key);
                    } else {
                        this.collapsed_change_sections
                            .insert(section_key.to_string());
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        svg()
                            .path(if collapsed {
                                "assets/icons/icons__chevron-right.svg"
                            } else {
                                "assets/icons/icons__chevron-down.svg"
                            })
                            .size(px(10.))
                            .text_color(count_col),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(title_col)
                            .child(format!("{title} ({file_count})")),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(Self::git_diff_badge(section_additions, true, 13.))
                    .child(Self::git_diff_badge(section_deletions, false, 13.))
                    .child(section_actions),
            )
    }

    pub(crate) fn changed_files_panel(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = theme::chrome_bg(window);
        let muted_col = hsla(0., 0., 0.54, 1.);
        let Some(project_id) = self
            .active_section
            .as_ref()
            .map(|section| section.project_id.clone())
        else {
            return Self::panel("Changed files", "", bg, true).into_any_element();
        };

        let has_loaded_changed_files = self.changed_files.contains_key(&project_id);
        let changed_files = self.active_changed_files();
        let has_changes = !changed_files.is_empty();
        let toolbar_enabled = self.active_git_action.is_none();
        let can_commit = has_changes && toolbar_enabled;

        let mut body = div().flex_1().flex().flex_col().min_h_0();
        if !has_loaded_changed_files {
            body = body.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(18.))
                    .text_sm()
                    .text_color(muted_col)
                    .child("Loading changes..."),
            );
        } else if changed_files.is_empty() {
            body = body.child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .px(px(18.))
                    .text_sm()
                    .text_color(muted_col)
                    .child("Working tree clean"),
            );
        } else {
            let staged_collapsed = self.collapsed_change_sections.contains("staged");
            let uncommitted_collapsed = self.collapsed_change_sections.contains("uncommitted");
            let list_snapshot = self.changed_files_list_snapshot(&project_id, &changed_files);
            let item_count = list_snapshot.item_count(staged_collapsed, uncommitted_collapsed);
            body = body.child(
                div()
                    .id("right-sidebar-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_hidden()
                    .child(
                        uniform_list(
                            SharedString::from(format!("changed-files-list-{project_id}")),
                            item_count,
                            cx.processor(
                                move |this, range: std::ops::Range<usize>, _window, cx| {
                                    let mut items = Vec::with_capacity(range.end - range.start);
                                    for index in range {
                                        if let Some(entry) = list_snapshot.entry_at(
                                            staged_collapsed,
                                            uncommitted_collapsed,
                                            index,
                                        ) {
                                            items.push(this.changed_files_list_item(
                                                &list_snapshot,
                                                &entry,
                                                cx,
                                            ));
                                        }
                                    }
                                    items
                                },
                            ),
                        )
                        .size_full(),
                    ),
            );
        }

        div()
            .relative()
            .flex()
            .flex_col()
            .size_full()
            .min_h_0()
            .bg(bg)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(8.))
                    .py(px(6.))
                    .child(Self::git_toolbar_button(
                        "Changes",
                        Some("assets/icons/icons__file_icons__changes.svg"),
                        None,
                        has_changes,
                        false,
                        Some("View changed files"),
                        move |_this, _ev, _window, cx| {
                            cx.stop_propagation();
                        },
                        cx,
                    ))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .flex_shrink_0()
                            .gap(px(TOOLBAR_ACTION_GAP))
                            .child(Self::git_toolbar_button(
                                "Commit",
                                None,
                                None,
                                can_commit,
                                self.toolbar_action_active(ToolbarGitAction::Commit),
                                Some("Commit changes, staging all files first if needed"),
                                move |this, _ev, _window, cx| {
                                    this.create_pr_menu_open = false;
                                    this.push_menu_open = false;
                                    this.start_toolbar_git_action(ToolbarGitAction::Commit, cx);
                                    cx.stop_propagation();
                                },
                                cx,
                            ))
                            .child(Self::git_toolbar_button(
                                "Commit & Push",
                                None,
                                None,
                                can_commit,
                                self.toolbar_action_active(ToolbarGitAction::CommitAndPush),
                                Some("Commit changes and push, staging all files first if needed"),
                                move |this, _ev, _window, cx| {
                                    this.create_pr_menu_open = false;
                                    this.push_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::CommitAndPush,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                },
                                cx,
                            ))
                            .child(self.push_control(toolbar_enabled, cx))
                            .child(self.create_pr_control(toolbar_enabled, cx)),
                    ),
            )
            .child(body)
            .child(self.push_dropdown_menu(cx))
            .child(self.create_pr_dropdown_menu(cx))
            .child(self.discard_confirm_modal(cx))
            .into_any_element()
    }

    fn discard_confirm_modal(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some((ref project_id, ref files)) = self.discard_confirm else {
            return div().id("discard-confirm-overlay");
        };

        let file_count = files.len();
        let message: SharedString = if file_count == 1 {
            let name = Path::new(&files[0].path)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| files[0].path.clone());
            format!("Discard changes to \"{}\"?", name).into()
        } else {
            format!("Discard changes to {} files?", file_count).into()
        };

        let confirm_project_id = project_id.clone();
        let confirm_files = files.clone();

        let border = gpui::white().opacity(0.08);
        let title_col = hsla(0., 0., 0.92, 1.);
        let body_col = hsla(0., 0., 0.74, 1.);
        let btn_bg = gpui::white().opacity(0.08);
        let btn_hover = gpui::white().opacity(0.14);
        let danger_bg = hsla(0., 0.62, 0.50, 1.);
        let danger_hover = hsla(0., 0.62, 0.58, 1.);

        div()
            .id("discard-confirm-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.50))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.discard_confirm = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                if this.discard_confirm.is_none() {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "escape" => {
                        this.discard_confirm = None;
                        cx.stop_propagation();
                        cx.notify();
                    }
                    "enter" => {
                        if let Some((project_id, files)) = this.discard_confirm.take() {
                            if files.len() == 1 {
                                this.revert_changed_file(&project_id, &files[0]);
                            } else {
                                this.revert_changed_files(&project_id, &files);
                            }
                        }
                        cx.stop_propagation();
                        cx.notify();
                    }
                    _ => {}
                }
            }))
            .child(
                div()
                    .w(px(320.))
                    .rounded_lg()
                    .bg(rgb(0x2b2d31))
                    .border_1()
                    .border_color(border)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|_this, _ev: &MouseDownEvent, _window, cx| {
                            cx.stop_propagation();
                        }),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .px(px(20.))
                            .pt(px(20.))
                            .pb(px(12.))
                            .child(
                                div()
                                    .text_size(rems(14. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col)
                                    .child("Confirm Discard"),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .text_color(body_col)
                                    .child(message),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(hsla(0., 0., 0.54, 1.))
                                    .child("This action cannot be undone."),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_end()
                            .gap(px(8.))
                            .px(px(20.))
                            .pb(px(16.))
                            .pt(px(8.))
                            .child(
                                div()
                                    .id("discard-confirm-cancel")
                                    .cursor_pointer()
                                    .px(px(14.))
                                    .py(px(6.))
                                    .rounded_md()
                                    .bg(btn_bg)
                                    .hover(move |style| style.bg(btn_hover))
                                    .tooltip(move |_window, cx| {
                                        Self::action_tooltip_view(
                                            "Close without discarding changes",
                                            cx,
                                        )
                                    })
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(title_col)
                                    .child("Cancel")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            this.discard_confirm = None;
                                            cx.stop_propagation();
                                            cx.notify();
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("discard-confirm-ok")
                                    .cursor_pointer()
                                    .px(px(14.))
                                    .py(px(6.))
                                    .rounded_md()
                                    .bg(danger_bg)
                                    .hover(move |style| style.bg(danger_hover))
                                    .tooltip(move |_window, cx| {
                                        Self::action_tooltip_view(
                                            "Permanently discard the selected changes",
                                            cx,
                                        )
                                    })
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col)
                                    .child("Discard")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, _ev: &MouseDownEvent, _window, cx| {
                                                if confirm_files.len() == 1 {
                                                    this.revert_changed_file(
                                                        &confirm_project_id,
                                                        &confirm_files[0],
                                                    );
                                                } else {
                                                    this.revert_changed_files(
                                                        &confirm_project_id,
                                                        &confirm_files,
                                                    );
                                                }
                                                this.discard_confirm = None;
                                                cx.stop_propagation();
                                                cx.notify();
                                            },
                                        ),
                                    ),
                            ),
                    ),
            )
    }
}
