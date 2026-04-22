//! Right sidebar content: changed files and branch actions.

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use gpui::{
    div, ease_in_out, hsla, percentage, prelude::*, px, rems, rgb, svg, Animation,
    AnimationExt as _, AnyElement, Context, KeyDownEvent, MouseButton, MouseDownEvent,
    SharedString, Transformation, Window,
};

use crate::app::{AnotherOneApp, CommitFileChangesState, RightSidebarMode};
use crate::theme;

#[derive(Clone, Copy, PartialEq, Eq)]
enum ChangeGroup {
    Staged,
    Uncommitted,
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

struct ChangedFileActionButtonProps {
    button_id: gpui::ElementId,
    icon_path: &'static str,
    enabled: bool,
    hover_bg: gpui::Hsla,
    icon_color: gpui::Hsla,
    tooltip_label: Option<&'static str>,
}

struct GitToolbarButtonProps {
    label: &'static str,
    leading_icon: Option<&'static str>,
    trailing_icon: Option<&'static str>,
    enabled: bool,
    active: bool,
    tooltip_label: Option<&'static str>,
}

struct ChangedFileSectionHeaderProps {
    section_key: &'static str,
    title: &'static str,
    project_id: String,
    section_indices: Arc<[usize]>,
    group: ChangeGroup,
    file_count: usize,
    additions: i32,
    deletions: i32,
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

    pub(crate) fn toolbar_spinner(icon_color: gpui::Hsla, size_px: f32) -> impl IntoElement {
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
        props: ChangedFileActionButtonProps,
        on_click: impl Fn(&mut Self, &MouseDownEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let mut button = div()
            .id(props.button_id)
            .flex()
            .items_center()
            .justify_center()
            .w(px(28.))
            .h(px(28.))
            .rounded_md()
            .opacity(if props.enabled { 1. } else { 0.35 });

        if props.enabled {
            button = button
                .cursor_pointer()
                .hover(move |style| style.bg(props.hover_bg))
                .on_mouse_down(MouseButton::Left, cx.listener(on_click));

            if let Some(label) = props.tooltip_label {
                button = button.tooltip(move |_window, cx| Self::action_tooltip_view(label, cx));
            }
        }

        button.child(
            svg()
                .path(props.icon_path)
                .size(px(16.))
                .text_color(props.icon_color),
        )
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
        props: GitToolbarButtonProps,
        on_click: impl Fn(&mut Self, &MouseDownEvent, &mut Window, &mut Context<Self>) + 'static,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visually_enabled = props.enabled || props.active;
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
        let hover_bg = gpui::white().opacity(0.06);
        let border_color = if props.active {
            gpui::white().opacity(0.14)
        } else {
            gpui::transparent_black()
        };

        let button = div()
            .id(SharedString::from(format!("git-toolbar-{}", props.label)))
            .flex()
            .items_center()
            .h(px(30.))
            .px(px(7.))
            .rounded(px(7.))
            .border_1()
            .border_color(border_color)
            .opacity(if visually_enabled { 1. } else { 0.55 })
            .when(props.active, |button| button.bg(rgb(0x262a30)));

        let mut content = div().flex().flex_row().items_center().gap(px(5.));

        if let Some(icon_path) = props.leading_icon {
            content = content.child(svg().path(icon_path).size(px(12.)).text_color(icon_col));
        }

        content = content.child(
            div()
                .text_size(rems(11. / 16.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text_col)
                .child(props.label),
        );

        if let Some(icon_path) = props.trailing_icon {
            content = content.child(svg().path(icon_path).size(px(11.)).text_color(icon_col));
        }

        let mut button = button.child(content);

        if let Some(tip) = props.tooltip_label {
            button = button.tooltip(move |_window, cx| Self::action_tooltip_view(tip, cx));
        }

        button.when(props.enabled && !props.active, |button| {
            button
                .cursor_pointer()
                .hover(move |style| style.bg(hover_bg))
                .on_mouse_down(MouseButton::Left, cx.listener(on_click))
        })
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
                    ChangedFileActionButtonProps {
                        button_id: ("changed-file-unstage", file_index).into(),
                        icon_path: "assets/icons/icons__minus.svg",
                        enabled: can_unstage && !actions_busy,
                        hover_bg: action_hover,
                        icon_color: action_icon,
                        tooltip_label: Some("Unstage file"),
                    },
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
                        ChangedFileActionButtonProps {
                            button_id: ("changed-file-stage", file_index).into(),
                            icon_path: "assets/icons/icons__plus.svg",
                            enabled: can_stage && !actions_busy,
                            hover_bg: action_hover,
                            icon_color: action_icon,
                            tooltip_label: Some("Stage File"),
                        },
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
                    ChangedFileActionButtonProps {
                        button_id: ("changed-file-discard", file_index).into(),
                        icon_path: "assets/icons/icons__discard.svg",
                        enabled: !actions_busy && !project_mutations_pending,
                        hover_bg: action_hover,
                        icon_color: action_icon,
                        tooltip_label: Some("Discard File Changes"),
                    },
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
        props: ChangedFileSectionHeaderProps,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let ChangedFileSectionHeaderProps {
            section_key,
            title,
            project_id,
            section_indices,
            group,
            file_count,
            additions: section_additions,
            deletions: section_deletions,
        } = props;
        let border = gpui::white().opacity(0.06);
        let title_col = hsla(0., 0., 0.92, 1.);
        let count_col = hsla(0., 0., 0.74, 1.);
        let action_hover = gpui::white().opacity(0.08);
        let action_icon = hsla(0., 0., 0.72, 1.);
        let header_hover = gpui::white().opacity(0.03);
        let collapsed = self.collapsed_change_sections.contains(section_key);
        let actions_busy = self.changed_files_actions_busy(&project_id);
        let project_mutations_pending = self.changed_files_project_mutations_pending(&project_id);
        let stage_all_pending = self.changed_files_stage_all_pending(&project_id);
        let unstage_all_pending = self.changed_files_unstage_all_pending(&project_id);

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
                            ChangedFileActionButtonProps {
                                button_id: SharedString::from(format!(
                                    "changed-section-action-{}-staged-unstage",
                                    project_id
                                ))
                                .into(),
                                icon_path: "assets/icons/icons__minus.svg",
                                enabled: !actions_busy,
                                hover_bg: action_hover,
                                icon_color: action_icon,
                                tooltip_label: Some("Unstage all files in this section"),
                            },
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
                            ChangedFileActionButtonProps {
                                button_id: SharedString::from(format!(
                                    "changed-section-action-{}-changes-stage",
                                    project_id
                                ))
                                .into(),
                                icon_path: "assets/icons/icons__plus.svg",
                                enabled: !actions_busy,
                                hover_bg: action_hover,
                                icon_color: action_icon,
                                tooltip_label: Some("Stage All Changes"),
                            },
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
                        ChangedFileActionButtonProps {
                            button_id: SharedString::from(format!(
                                "changed-section-action-{}-changes-discard",
                                project_id
                            ))
                            .into(),
                            icon_path: "assets/icons/icons__discard.svg",
                            enabled: !actions_busy && !project_mutations_pending,
                            hover_bg: action_hover,
                            icon_color: action_icon,
                            tooltip_label: Some("Discard All Changes"),
                        },
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
            .min_h(px(44.))
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
                    .min_w(px(0.))
                    .flex_1()
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
                            .text_size(rems(11. / 16.))
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

    fn branch_compare_row(
        &self,
        project_id: &str,
        file: &crate::project_store::BranchCompareFile,
    ) -> AnyElement {
        let title_col = hsla(0., 0., 0.94, 1.);
        let path_col = hsla(0., 0., 0.58, 1.);
        let row_hover = gpui::white().opacity(0.04);
        let file_name = Path::new(&file.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(file.path.as_str())
            .to_string();
        let parent_dir = Path::new(&file.path)
            .parent()
            .and_then(|parent| parent.to_str())
            .filter(|parent| !parent.is_empty() && *parent != ".")
            .unwrap_or_default()
            .to_string();
        let status_color = Self::changed_file_status_color(file.status);

        let mut stats = div().flex().flex_row().items_center().gap(px(8.));
        if file.additions > 0 {
            stats = stats.child(Self::git_diff_badge(file.additions, true, 12.));
        }
        if file.deletions > 0 {
            stats = stats.child(Self::git_diff_badge(file.deletions, false, 12.));
        }

        div()
            .id(SharedString::from(format!(
                "branch-compare-row-{project_id}-{}",
                file.path
            )))
            .w_full()
            .min_h(px(34.))
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .pl(px(18.))
            .pr(px(14.))
            .py(px(8.))
            .rounded_md()
            .mx(px(4.))
            .hover(move |style| style.bg(row_hover))
            .child(
                div()
                    .flex()
                    .items_start()
                    .gap(px(12.))
                    .min_w(px(0.))
                    .flex_1()
                    .child(
                        div()
                            .min_w(px(18.))
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(status_color)
                            .child(file.status.to_string()),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .min_w(px(0.))
                            .flex_1()
                            .child(
                                div()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(title_col)
                                    .child(file_name),
                            )
                            .when(!parent_dir.is_empty(), |entry| {
                                entry.child(
                                    div()
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_size(rems(11. / 16.))
                                        .text_color(path_col)
                                        .child(parent_dir.clone()),
                                )
                            })
                            .when(file.original_path.is_some(), |entry| {
                                entry.child(
                                    div()
                                        .min_w(px(0.))
                                        .truncate()
                                        .text_size(rems(11. / 16.))
                                        .text_color(path_col)
                                        .child(format!(
                                            "Renamed from {}",
                                            file.original_path.clone().unwrap_or_default()
                                        )),
                                )
                            }),
                    ),
            )
            .child(stats)
            .into_any_element()
    }

    fn branch_commit_file_row(
        &self,
        project_id: &str,
        commit_id: &str,
        file: &crate::project_store::BranchCompareFile,
        file_index: usize,
    ) -> AnyElement {
        let title_col = hsla(0., 0., 0.92, 1.);
        let path_col = hsla(0., 0., 0.56, 1.);
        let status_color = Self::changed_file_status_color(file.status);
        let file_name = Path::new(&file.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(file.path.as_str())
            .to_string();
        let parent_dir = Path::new(&file.path)
            .parent()
            .and_then(|parent| parent.to_str())
            .filter(|parent| !parent.is_empty() && *parent != ".")
            .unwrap_or_default()
            .to_string();

        let mut stats = div().flex().flex_row().items_center().gap(px(8.));
        if file.additions > 0 {
            stats = stats.child(Self::git_diff_badge(file.additions, true, 12.));
        }
        if file.deletions > 0 {
            stats = stats.child(Self::git_diff_badge(file.deletions, false, 12.));
        }

        div()
            .id(SharedString::from(format!(
                "branch-commit-file-row-{project_id}-{commit_id}-{file_index}"
            )))
            .w_full()
            .min_w(px(0.))
            .flex()
            .flex_col()
            .gap(px(2.))
            .px(px(12.))
            .py(px(6.))
            .child(
                div()
                    .w_full()
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
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
                                    .child(file.status.to_string()),
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
                                            .child(file_name),
                                    )
                                    .when(!parent_dir.is_empty(), |entry| {
                                        entry.child(
                                            div()
                                                .min_w(px(0.))
                                                .truncate()
                                                .text_size(rems(11. / 16.))
                                                .text_color(path_col)
                                                .child(parent_dir.clone()),
                                        )
                                    }),
                            ),
                    )
                    .child(div().flex_shrink_0().child(stats)),
            )
            .when(file.original_path.is_some(), |row| {
                row.child(
                    div()
                        .ml(px(30.))
                        .min_w(px(0.))
                        .truncate()
                        .text_size(rems(11. / 16.))
                        .text_color(path_col)
                        .child(format!(
                            "Renamed from {}",
                            file.original_path.clone().unwrap_or_default()
                        )),
                )
            })
            .into_any_element()
    }

    fn branch_commit_row(
        &self,
        project_id: &str,
        commit: &crate::project_store::BranchCommit,
        show_undo_button: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let title_col = hsla(0., 0., 0.94, 1.);
        let meta_col = hsla(0., 0., 0.58, 1.);
        let row_hover = gpui::white().opacity(0.04);
        let undo_icon_col = hsla(0., 0., 0.72, 1.);
        let undo_hover = gpui::white().opacity(0.08);
        let details_bg = gpui::white().opacity(0.03);
        let details_border = gpui::white().opacity(0.06);
        let expanded = self.commit_row_expanded(project_id, &commit.id);
        let commit_file_changes_state = self
            .commit_file_changes_state(project_id, &commit.id)
            .cloned();
        let toggle_project_id = project_id.to_string();
        let toggle_commit_id = commit.id.clone();
        let undo_busy = matches!(
            self.active_git_action.as_ref(),
            Some(crate::git_actions::ToolbarGitAction::UndoLastCommit)
        );
        let undo_enabled = show_undo_button && self.active_git_action.is_none();

        let mut expanded_details = div()
            .w_full()
            .min_w(px(0.))
            .ml(px(28.))
            .mr(px(28.))
            .mb(px(8.))
            .flex()
            .flex_col()
            .gap(px(6.))
            .rounded_md()
            .border_1()
            .border_color(details_border)
            .bg(details_bg)
            .overflow_hidden()
            .child(
                div().w_full().min_w(px(0.)).px(px(12.)).pt(px(10.)).child(
                    div()
                        .w_full()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .min_w(px(0.))
                        .child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_size(rems(11. / 16.))
                                .text_color(meta_col)
                                .child(commit.author_name.clone()),
                        )
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .text_color(meta_col)
                                .child("\u{00B7}"),
                        )
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .text_color(meta_col)
                                .child(commit.authored_relative.clone()),
                        ),
                ),
            );

        match commit_file_changes_state {
            Some(CommitFileChangesState::Loading) => {
                expanded_details = expanded_details.child(
                    div()
                        .flex()
                        .items_center()
                        .gap(px(8.))
                        .px(px(12.))
                        .pb(px(10.))
                        .child(Self::toolbar_spinner(meta_col, 12.))
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .text_color(meta_col)
                                .child("Loading file changes..."),
                        ),
                );
            }
            Some(CommitFileChangesState::Failed(_)) => {
                expanded_details = expanded_details.child(
                    div()
                        .px(px(12.))
                        .pb(px(10.))
                        .text_size(rems(11. / 16.))
                        .text_color(meta_col)
                        .child("Couldn't load file changes."),
                );
            }
            Some(CommitFileChangesState::Loaded(files)) if files.is_empty() => {
                expanded_details = expanded_details.child(
                    div()
                        .px(px(12.))
                        .pb(px(10.))
                        .text_size(rems(11. / 16.))
                        .text_color(meta_col)
                        .child("No file changes in this commit."),
                );
            }
            Some(CommitFileChangesState::Loaded(files)) => {
                let file_count = files.len();
                let mut file_rows = div()
                    .w_full()
                    .min_w(px(0.))
                    .flex()
                    .flex_col()
                    .gap(px(0.))
                    .pb(px(10.));
                for (file_index, file) in files.iter().enumerate() {
                    file_rows = file_rows.child(
                        self.branch_commit_file_row(project_id, &commit.id, file, file_index),
                    );
                }

                expanded_details = expanded_details
                    .child(
                        div()
                            .px(px(12.))
                            .pb(px(2.))
                            .text_size(rems(10. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(meta_col)
                            .child(if file_count == 1 {
                                "1 file changed".to_string()
                            } else {
                                format!("{file_count} files changed")
                            }),
                    )
                    .child(file_rows);
            }
            None => {}
        }

        div()
            .id(SharedString::from(format!(
                "branch-commit-row-{project_id}-{}",
                commit.id
            )))
            .w_full()
            .flex()
            .flex_col()
            .child(
                div()
                    .w_full()
                    .min_h(px(30.))
                    .min_w(px(0.))
                    .flex()
                    .items_center()
                    .gap(px(6.))
                    .pl(px(14.))
                    .pr(px(14.))
                    .py(if expanded { px(7.) } else { px(5.) })
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |style| style.bg(row_hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.toggle_commit_row_expanded(
                                &toggle_project_id,
                                &toggle_commit_id,
                                cx,
                            );
                        }),
                    )
                    .child(
                        svg()
                            .path(if expanded {
                                "assets/icons/icons__chevron-down.svg"
                            } else {
                                "assets/icons/icons__chevron-right.svg"
                            })
                            .flex_shrink_0()
                            .size(px(8.))
                            .text_color(meta_col),
                    )
                    .child(
                        div()
                            .min_w(px(0.))
                            .flex_1()
                            .truncate()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(title_col)
                            .child(commit.subject.clone()),
                    )
                    .when(show_undo_button && undo_busy, |row| {
                        row.child(Self::changed_file_action_pending(undo_icon_col))
                    })
                    .when(show_undo_button && !undo_busy, |row| {
                        row.child(Self::changed_file_action_button(
                            ChangedFileActionButtonProps {
                                button_id: SharedString::from(format!(
                                    "commit-row-undo-{project_id}-{}",
                                    commit.id
                                ))
                                .into(),
                                icon_path: "assets/icons/icons__discard.svg",
                                enabled: undo_enabled,
                                hover_bg: undo_hover,
                                icon_color: undo_icon_col,
                                tooltip_label: Some("Undo the most recent commit"),
                            },
                            move |this, _ev, _window, cx| {
                                cx.stop_propagation();
                                this.start_toolbar_git_action(
                                    crate::git_actions::ToolbarGitAction::UndoLastCommit,
                                    cx,
                                );
                            },
                            cx,
                        ))
                    }),
            )
            .when(expanded, |row| row.child(expanded_details))
            .into_any_element()
    }

    pub(crate) fn changed_files_panel(
        &mut self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let bg = theme::chrome_bg(window);
        let muted_col = hsla(0., 0., 0.54, 1.);
        let Some(active_section) = self.workspace_pane.read(cx).active_section.clone() else {
            return Self::panel("Changed files", "", bg, true).into_any_element();
        };
        let project_id = active_section.project_id.clone();
        let sidebar_mode = self.active_right_sidebar_mode(cx);
        let commit_state = self.active_branch_commit_state(cx).cloned();
        let compare_target_branch = self.active_compare_target_branch(cx);
        let compare_state = self.active_branch_compare_state(cx).cloned();

        let has_loaded_changed_files = self.changed_files.contains_key(&project_id);
        let changed_files = self.active_changed_files(cx);
        let mut body = div().flex_1().flex().flex_col().min_h_0();
        match sidebar_mode {
            RightSidebarMode::WorkingTree => {
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
                    let uncommitted_collapsed =
                        self.collapsed_change_sections.contains("uncommitted");
                    let list_snapshot =
                        self.changed_files_list_snapshot(&project_id, &changed_files);
                    let mut rows = div()
                        .id("right-sidebar-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .pt(px(8.));

                    if !list_snapshot.staged_indices.is_empty() {
                        rows = rows.child(self.changed_file_section_header(
                            ChangedFileSectionHeaderProps {
                                section_key: "staged",
                                title: "Staged Changes",
                                project_id: list_snapshot.project_id.clone(),
                                section_indices: list_snapshot.staged_indices.clone(),
                                group: ChangeGroup::Staged,
                                file_count: list_snapshot.staged_indices.len(),
                                additions: list_snapshot.staged_additions,
                                deletions: list_snapshot.staged_deletions,
                            },
                            cx,
                        ));

                        if !staged_collapsed {
                            for file_index in list_snapshot.staged_indices.iter().copied() {
                                if let Some(row) = list_snapshot.rows.get(file_index) {
                                    rows = rows.child(self.changed_file_row(
                                        &list_snapshot.project_id,
                                        file_index,
                                        row,
                                        ChangeGroup::Staged,
                                        cx,
                                    ));
                                }
                            }
                        }
                    }

                    if !list_snapshot.unstaged_indices.is_empty() {
                        rows = rows.child(self.changed_file_section_header(
                            ChangedFileSectionHeaderProps {
                                section_key: "uncommitted",
                                title: "Changes",
                                project_id: list_snapshot.project_id.clone(),
                                section_indices: list_snapshot.unstaged_indices.clone(),
                                group: ChangeGroup::Uncommitted,
                                file_count: list_snapshot.unstaged_indices.len(),
                                additions: list_snapshot.unstaged_additions,
                                deletions: list_snapshot.unstaged_deletions,
                            },
                            cx,
                        ));

                        if !uncommitted_collapsed {
                            for file_index in list_snapshot.unstaged_indices.iter().copied() {
                                if let Some(row) = list_snapshot.rows.get(file_index) {
                                    rows = rows.child(self.changed_file_row(
                                        &list_snapshot.project_id,
                                        file_index,
                                        row,
                                        ChangeGroup::Uncommitted,
                                        cx,
                                    ));
                                }
                            }
                        }
                    }

                    body = body.child(rows);
                }
            }
            RightSidebarMode::Commits => {
                let current_branch = commit_state
                    .as_ref()
                    .and_then(|state| state.current_branch.clone())
                    .unwrap_or_else(|| active_section.branch_name.clone());
                let commit_count = commit_state.as_ref().map_or(0, |state| state.commits.len());
                let commit_summary = commit_state.as_ref().map_or_else(
                    || "Recent commits from HEAD.".to_string(),
                    |state| {
                        if state.has_more {
                            format!("{commit_count} shown")
                        } else {
                            format!("{commit_count} recent commits")
                        }
                    },
                );

                body = body.child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .border_b_1()
                        .border_color(gpui::white().opacity(0.06))
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(hsla(0., 0., 0.88, 1.))
                                .child(format!("Recent commits on {}", current_branch)),
                        )
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .text_color(muted_col)
                                .child(commit_summary),
                        ),
                );

                if commit_state.is_none() {
                    body = body.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px(px(18.))
                            .text_sm()
                            .text_color(muted_col)
                            .child("Loading commits..."),
                    );
                } else if commit_state
                    .as_ref()
                    .is_some_and(|state| state.commits.is_empty())
                {
                    body = body.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px(px(18.))
                            .text_sm()
                            .text_color(muted_col)
                            .child("No commits yet on this branch."),
                    );
                } else if let Some(commit_state) = commit_state {
                    let mut rows = div()
                        .id("right-sidebar-commits-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .px(px(0.))
                        .py(px(8.))
                        .gap(px(0.));

                    for (index, commit) in commit_state.commits.iter().enumerate() {
                        rows =
                            rows.child(self.branch_commit_row(&project_id, commit, index == 0, cx));
                    }

                    if commit_state.has_more {
                        let load_more_project_id = project_id.clone();
                        rows = rows.child(
                            div()
                                .w_full()
                                .flex()
                                .justify_center()
                                .pt(px(10.))
                                .pb(px(6.))
                                .child(Self::git_toolbar_button(
                                    GitToolbarButtonProps {
                                        label: "Load more",
                                        leading_icon: None,
                                        trailing_icon: None,
                                        enabled: true,
                                        active: false,
                                        tooltip_label: Some("Show 20 more recent commits"),
                                    },
                                    move |this, _ev, _window, cx| {
                                        this.load_more_commits(&load_more_project_id, cx);
                                    },
                                    cx,
                                )),
                        );
                    }

                    body = body.child(rows);
                }
            }
            RightSidebarMode::Compare => {
                let target_branch = compare_target_branch.clone().unwrap_or_default();
                let current_branch = compare_state
                    .as_ref()
                    .and_then(|state| state.current_branch.clone())
                    .unwrap_or_else(|| active_section.branch_name.clone());

                body = body.child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .border_b_1()
                        .border_color(gpui::white().opacity(0.06))
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(hsla(0., 0., 0.88, 1.))
                                .child(format!(
                                    "Comparing {} against {}",
                                    current_branch, target_branch
                                )),
                        )
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .text_color(muted_col)
                                .child("Read-only branch diff. Stage, unstage, and discard actions are unavailable in compare mode."),
                        ),
                );

                if compare_state.is_none() {
                    body = body.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px(px(18.))
                            .text_sm()
                            .text_color(muted_col)
                            .child("Loading compare view..."),
                    );
                } else if compare_state
                    .as_ref()
                    .is_some_and(|state| state.files.is_empty())
                {
                    body = body.child(
                        div()
                            .flex_1()
                            .flex()
                            .items_center()
                            .justify_center()
                            .px(px(18.))
                            .text_sm()
                            .text_color(muted_col)
                            .child(format!("No differences from {}.", target_branch)),
                    );
                } else if let Some(compare_state) = compare_state {
                    let mut rows = div()
                        .id("right-sidebar-compare-scroll")
                        .flex_1()
                        .min_h_0()
                        .overflow_y_scroll()
                        .flex()
                        .flex_col()
                        .px(px(4.))
                        .py(px(8.))
                        .gap(px(2.));

                    for file in &compare_state.files {
                        rows = rows.child(self.branch_compare_row(&project_id, file));
                    }

                    body = body.child(rows);
                }
            }
        }

        let commits_button = Self::git_toolbar_button(
            GitToolbarButtonProps {
                label: "Commits",
                leading_icon: Some("assets/icons/icons__git-commit.svg"),
                trailing_icon: None,
                enabled: true,
                active: sidebar_mode == RightSidebarMode::Commits,
                tooltip_label: Some("View recent commits on the current branch"),
            },
            move |this, _ev, _window, cx| {
                this.set_right_sidebar_mode(RightSidebarMode::Commits, cx);
            },
            cx,
        );

        let compare_button = compare_target_branch.as_ref().map(|_target_branch| {
            Self::git_toolbar_button(
                GitToolbarButtonProps {
                    label: "Compare",
                    leading_icon: Some("assets/icons/icons__git-split.svg"),
                    trailing_icon: None,
                    enabled: true,
                    active: sidebar_mode == RightSidebarMode::Compare,
                    tooltip_label: Some(
                        "Compare the current branch against the configured target branch",
                    ),
                },
                move |this, _ev, _window, cx| {
                    this.set_right_sidebar_mode(RightSidebarMode::Compare, cx);
                },
                cx,
            )
        });

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
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(Self::git_toolbar_button(
                                GitToolbarButtonProps {
                                    label: "Changes",
                                    leading_icon: Some(
                                        "assets/icons/icons__file_icons__changes.svg",
                                    ),
                                    trailing_icon: None,
                                    enabled: true,
                                    active: sidebar_mode == RightSidebarMode::WorkingTree,
                                    tooltip_label: Some("View working tree changes"),
                                },
                                move |this, _ev, _window, cx| {
                                    this.set_right_sidebar_mode(RightSidebarMode::WorkingTree, cx);
                                },
                                cx,
                            ))
                            .child(commits_button)
                            .when_some(compare_button, |container, button| container.child(button)),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_end()
                            .flex_shrink_0(),
                    ),
            )
            .child(body)
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
