//! Project page rendered in the centre panel when a project is selected.

use std::path::PathBuf;

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, Context, MouseButton, MouseDownEvent, SharedString,
    Window,
};

use crate::app::{AnotherOneApp, SectionId, SidebarTaskDeleteRequest, WorkspacePane};
use crate::left_sidebar::open_external_url;
use crate::settings_page::SettingsSection;

#[derive(Clone)]
struct ProjectPageTaskEntry {
    project_id: String,
    project_path: PathBuf,
    task_id: String,
    name: String,
    branch_name: String,
    is_worktree: bool,
    pinned: bool,
}

// ── Mock PR data (UI only, not wired) ────────────────────────────────

struct MockPr {
    number: u32,
    title: &'static str,
    branch: &'static str,
    author: &'static str,
    lines_added: i32,
    lines_removed: i32,
    ci_passed: bool,
    review_required: bool,
    reviewers: &'static [(&'static str, u32)], // (name, colour)
}

const MOCK_PRS: &[MockPr] = &[
    MockPr {
        number: 1800,
        title: "SMP-247: Custom form option group buttons",
        branch: "SMP-247-custom-f-btns",
        author: "brian-lifemd",
        lines_added: 612,
        lines_removed: 20,
        ci_passed: false,
        review_required: true,
        reviewers: &[("fazulk", 0xE8A838), ("MasonRhodesDev", 0x4CAF50)],
    },
    MockPr {
        number: 1795,
        title: "SMP-251: Fix patient intake validation",
        branch: "SMP-251-intake-fix",
        author: "fazulk",
        lines_added: 42,
        lines_removed: 8,
        ci_passed: true,
        review_required: false,
        reviewers: &[("brian-lifemd", 0x5B4A9E)],
    },
];

const PR_FILTER_TABS: &[&str] = &["All Open", "Needs My Review", "My PRs", "Draft"];

// ── Colours ──────────────────────────────────────────────────────────

const TEXT_PRIMARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.92, 1.);
const TEXT_SECONDARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.55, 1.);
const TEXT_MUTED: fn() -> gpui::Hsla = || hsla(0., 0., 0.40, 1.);
const GREEN: fn() -> gpui::Hsla = || hsla(138. / 360., 0.50, 0.74, 1.);
const RED: fn() -> gpui::Hsla = || hsla(352. / 360., 0.52, 0.76, 1.);

impl WorkspacePane {
    fn project_page_tasks(app: &AnotherOneApp, project_id: &str) -> Vec<ProjectPageTaskEntry> {
        let Some(project) = app
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
        else {
            return Vec::new();
        };

        let mut tasks = Vec::new();

        if let Some(task_list) = app.project_store.tasks.get(&project.id) {
            for task in task_list {
                let is_worktree = task.kind == crate::project_store::TaskKind::Worktree
                    || task.kind == crate::project_store::TaskKind::MultiWorktree;
                let (pid, ppath) = if let Some(wt_id) = task.worktree_project_id.as_ref() {
                    app.project_store
                        .projects
                        .iter()
                        .find(|p| p.id == *wt_id)
                        .map(|p| (p.id.clone(), p.path.clone()))
                        .unwrap_or_else(|| (project.id.clone(), project.path.clone()))
                } else {
                    (project.id.clone(), project.path.clone())
                };
                tasks.push(ProjectPageTaskEntry {
                    project_id: pid,
                    project_path: ppath,
                    task_id: task.id.clone(),
                    name: task.name.clone(),
                    branch_name: task.branch_name.clone(),
                    is_worktree,
                    pinned: app.project_store.ui.pinned_task_ids.contains(&task.id),
                });
            }
        }

        tasks.sort_by_key(|task| !task.pinned);
        tasks
    }

    // ── Public entry point ───────────────────────────────────────────

    pub(crate) fn render_project_page(
        &mut self,
        project_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let Some(app_entity) = self.app.upgrade() else {
            return div().flex().flex_col().size_full().bg(rgb(0x1e1f22)).child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(TEXT_MUTED())
                    .child("Project not found"),
            );
        };
        let app = app_entity.read(cx);
        let project = app
            .project_store
            .projects
            .iter()
            .find(|p| p.id == project_id);

        let Some(project) = project else {
            return div().flex().flex_col().size_full().bg(rgb(0x1e1f22)).child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(TEXT_MUTED())
                    .child("Project not found"),
            );
        };

        let project_name: SharedString = project.name.clone().into();
        let project_id_owned = project_id.to_string();

        let github_url = app.project_github_links.get(project_id).cloned();

        let tasks = Self::project_page_tasks(app, project_id);

        let task_count = tasks.len();
        let search = self.project_page_task_search.to_lowercase();
        let filtered_tasks: Vec<_> = if search.is_empty() {
            tasks
        } else {
            tasks
                .into_iter()
                .filter(|t| t.name.to_lowercase().contains(&search))
                .collect()
        };

        let pid_for_new_task = project_id_owned.clone();

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(rgb(0x1e1f22))
            // ── Header bar ───────────────────────────────────────
            .child(self.project_page_header(&project_id_owned, &project_name, github_url, cx))
            // ── Scrollable content ───────────────────────────────
            .child(
                div()
                    .id("project-page-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px(px(24.))
                    .py(px(20.))
                    .child(self.project_page_tasks_section(
                        &project_name,
                        task_count,
                        &filtered_tasks,
                        &pid_for_new_task,
                        cx,
                    ))
                    .child(self.project_page_prs_section(cx)),
            )
    }

    // ── Header bar ───────────────────────────────────────────────────

    fn project_page_header(
        &self,
        project_id: &str,
        project_name: &SharedString,
        github_url: Option<String>,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let project_id_for_remove = project_id.to_string();
        let has_github = github_url.is_some();

        div()
            .flex()
            .flex_row()
            .items_center()
            .px(px(24.))
            .py(px(16.))
            .gap(px(12.))
            .border_b_1()
            .border_color(gpui::white().opacity(0.06))
            // Project name
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_color(TEXT_PRIMARY())
                    .text_size(rems(1.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .truncate()
                    .child(project_name.clone()),
            )
            // Configuration button
            .child(
                div()
                    .id("project-page-config-btn")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(5.))
                    .h(px(30.))
                    .px(px(7.))
                    .rounded(px(7.))
                    .bg(rgb(0x1e2024))
                    .border_1()
                    .border_color(gpui::white().opacity(0.08))
                    .hover(|s| s.bg(gpui::white().opacity(0.06)))
                    .cursor_pointer()
                    .tooltip(move |_window, cx| {
                        AnotherOneApp::action_tooltip_view("Open project-specific settings", cx)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            let _ = this.app.update(cx, |app, app_cx| {
                                app.open_settings_section(SettingsSection::OpenIn, app_cx);
                            });
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__settings.svg")
                            .size(px(12.))
                            .text_color(TEXT_SECONDARY()),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(TEXT_PRIMARY())
                            .child("Configuration"),
                    ),
            )
            // View on GitHub button
            .when(has_github, |d| {
                d.child(
                    div()
                        .id("project-page-github-btn")
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(30.))
                        .h(px(30.))
                        .rounded(px(7.))
                        .bg(rgb(0x1e2024))
                        .border_1()
                        .border_color(gpui::white().opacity(0.08))
                        .hover(|s| s.bg(gpui::white().opacity(0.06)))
                        .cursor_pointer()
                        .tooltip(move |_window, cx| {
                            AnotherOneApp::action_tooltip_view(
                                "Open this project's GitHub repository",
                                cx,
                            )
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                if let Some(github_url) = github_url.clone() {
                                    if let Err(err) = open_external_url(&github_url) {
                                        this.show_error_toast(err, cx);
                                    }
                                }
                            }),
                        )
                        .child(
                            svg()
                                .path("assets/icons/icons__github.svg")
                                .size(px(14.))
                                .text_color(TEXT_SECONDARY()),
                        ),
                )
            })
            // Remove project button
            .child(
                div()
                    .id("project-page-remove-btn")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(30.))
                    .h(px(30.))
                    .rounded(px(7.))
                    .bg(rgb(0x1e2024))
                    .border_1()
                    .border_color(gpui::white().opacity(0.08))
                    .hover(|s| s.bg(gpui::white().opacity(0.06)))
                    .cursor_pointer()
                    .tooltip(move |_window, cx| {
                        AnotherOneApp::action_tooltip_view(
                            "Remove this project group from the sidebar",
                            cx,
                        )
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.request_remove_project_group(&project_id_for_remove, cx);
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__trash.svg")
                            .size(px(14.))
                            .text_color(TEXT_SECONDARY()),
                    ),
            )
    }

    // ── Tasks section ────────────────────────────────────────────────

    fn project_page_tasks_section(
        &self,
        project_name: &SharedString,
        task_count: usize,
        tasks: &[ProjectPageTaskEntry],
        pid_for_new_task: &str,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let pid_new = pid_for_new_task.to_string();
        let count_label: SharedString = format!("{task_count} tasks with {project_name}").into();

        let mut section = div().flex().flex_col().gap(px(12.)).mb(px(28.));

        // Title row
        section = section.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .child(
                            div()
                                .text_color(TEXT_PRIMARY())
                                .text_size(rems(13. / 16.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("Tasks"),
                        )
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .text_color(TEXT_MUTED())
                                .child("Spin up a fresh, isolated task for this project."),
                        ),
                )
                .child(
                    div()
                        .id("project-page-new-task-btn")
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(5.))
                        .h(px(30.))
                        .px(px(7.))
                        .rounded(px(7.))
                        .bg(rgb(0x1e2024))
                        .border_1()
                        .border_color(gpui::white().opacity(0.08))
                        .hover(|s| s.bg(gpui::white().opacity(0.06)))
                        .cursor_pointer()
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                                this.focus_handle.focus(window);
                                this.open_new_task_modal(&pid_new, cx);
                            }),
                        )
                        .child(
                            svg()
                                .path("assets/icons/icons__plus.svg")
                                .size(px(12.))
                                .text_color(TEXT_PRIMARY()),
                        )
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .text_color(TEXT_PRIMARY())
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child("New Task"),
                        ),
                ),
        );

        // Search bar
        section = section.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .px(px(12.))
                .py(px(8.))
                .rounded(px(7.))
                .bg(gpui::white().opacity(0.05))
                .border_1()
                .border_color(gpui::white().opacity(0.08))
                .child(
                    svg()
                        .path("assets/icons/icons__file_icons__magnifying_glass.svg")
                        .size(px(14.))
                        .text_color(TEXT_MUTED()),
                )
                .child(div().text_sm().text_color(TEXT_MUTED()).child(
                    if self.project_page_task_search.is_empty() {
                        "Search tasks...".to_string()
                    } else {
                        self.project_page_task_search.clone()
                    },
                )),
        );

        // Task count + Select
        section = section.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_sm()
                                .text_color(TEXT_SECONDARY())
                                .child(count_label),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(TEXT_PRIMARY())
                                .text_decoration_1()
                                .cursor_pointer()
                                .child("Select"),
                        ),
                )
                .child(
                    div().cursor_pointer().child(
                        svg()
                            .path("assets/icons/icons__ellipsis.svg")
                            .size(px(16.))
                            .text_color(TEXT_SECONDARY()),
                    ),
                ),
        );

        // Task rows
        for task in tasks {
            section = section.child(self.project_page_task_row(task, pid_for_new_task, cx));
        }

        // Empty state
        if tasks.is_empty() {
            section = section.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .py(px(24.))
                    .text_sm()
                    .text_color(TEXT_MUTED())
                    .child("No tasks yet. Create one to get started."),
            );
        }

        section
    }

    fn project_page_task_row(
        &self,
        task: &ProjectPageTaskEntry,
        preferred_project_id: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let task_name: SharedString = task.name.clone().into();
        let pid = task.project_id.clone();
        let project_path = task.project_path.clone();
        let branch = task.branch_name.clone();
        let task_id = task.task_id.clone();
        let is_worktree = task.is_worktree;
        let preferred_project_id = preferred_project_id.to_string();
        let pid_nav = pid.clone();
        let branch_nav = branch.clone();
        let task_id_nav = task_id.clone();
        let row_suffix = task_id.clone();
        let row_id = SharedString::from(format!("task-row-{}", row_suffix));
        let delete_tooltip = if is_worktree {
            "Delete this worktree task"
        } else {
            "Delete this direct task"
        };
        let delete_project_id = pid.clone();
        let delete_task_id = task_id.clone();
        let delete_task_name = task.name.clone();
        let delete_branch_name = branch.clone();
        let delete_preferred_project_id = preferred_project_id.clone();

        div()
            .id(row_id)
            .flex()
            .flex_row()
            .items_center()
            .py(px(10.))
            .px(px(4.))
            .border_b_1()
            .border_color(gpui::white().opacity(0.06))
            .hover(|s| s.bg(gpui::white().opacity(0.03)))
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    let sid = SectionId::for_task(&pid_nav, &branch_nav, &task_id_nav);
                    this.activate_section(sid, Some(project_path.clone()), None, cx);
                    this.mark_git_refresh_stale(cx);
                }),
            )
            // Status circle
            .child(
                div()
                    .w(px(12.))
                    .h(px(12.))
                    .rounded_full()
                    .border_2()
                    .border_color(TEXT_SECONDARY())
                    .mr(px(12.)),
            )
            // Task name
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .text_color(TEXT_PRIMARY())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .truncate()
                    .child(task_name),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .ml(px(12.))
                    .child(
                        div()
                            .px(px(6.))
                            .py(px(2.))
                            .rounded(px(10.))
                            .bg(gpui::white().opacity(0.06))
                            .text_xs()
                            .text_color(TEXT_SECONDARY())
                            .child(if is_worktree { "Worktree" } else { "Direct" }),
                    )
                    .child(
                        div()
                            .id(SharedString::from(format!("task-trash-{}", row_suffix)))
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(28.))
                            .h(px(28.))
                            .rounded(px(5.))
                            .cursor_pointer()
                            .hover(|s| s.bg(gpui::white().opacity(0.10)))
                            .tooltip(move |_window, cx| {
                                AnotherOneApp::action_tooltip_view(delete_tooltip, cx)
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                    cx.stop_propagation();
                                    this.request_sidebar_task_delete(
                                        SidebarTaskDeleteRequest {
                                            project_id: delete_project_id.clone(),
                                            task_id: delete_task_id.clone(),
                                            task_name: delete_task_name.clone(),
                                            branch_name: delete_branch_name.clone(),
                                            is_worktree,
                                            preferred_project_id: delete_preferred_project_id
                                                .clone(),
                                        },
                                        cx,
                                    );
                                }),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__trash.svg")
                                    .size(px(15.))
                                    .text_color(TEXT_SECONDARY()),
                            ),
                    ),
            )
    }

    // ── Open PRs section ─────────────────────────────────────────────

    fn project_page_prs_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        let collapsed = self.project_page_prs_collapsed;
        let pr_count = MOCK_PRS.len();
        let chevron_icon = if collapsed {
            "assets/icons/icons__chevron-right.svg"
        } else {
            "assets/icons/icons__chevron-down.svg"
        };

        let mut section = div().flex().flex_col().gap(px(12.));

        // Collapsible header
        section = section.child(
            div()
                .id("project-page-prs-header")
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                        this.project_page_prs_collapsed = !this.project_page_prs_collapsed;
                        cx.notify();
                    }),
                )
                .child(
                    svg()
                        .path(chevron_icon)
                        .size(px(16.))
                        .text_color(TEXT_SECONDARY()),
                )
                .child(
                    div()
                        .text_color(TEXT_PRIMARY())
                        .text_size(rems(13. / 16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child("Open PRs"),
                )
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .px(px(8.))
                        .py(px(2.))
                        .rounded(px(10.))
                        .bg(gpui::white().opacity(0.10))
                        .text_xs()
                        .text_color(TEXT_SECONDARY())
                        .child(format!("{pr_count}")),
                ),
        );

        if collapsed {
            return section;
        }

        // Filter tabs
        let active_filter = self.project_page_pr_filter;
        let mut tabs = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .mb(px(4.));

        for (i, label) in PR_FILTER_TABS.iter().enumerate() {
            let is_active = i == active_filter;
            let tab_label: SharedString = (*label).into();
            tabs = tabs.child(
                div()
                    .id(SharedString::from(format!("pr-filter-{i}")))
                    .h(px(26.))
                    .px(px(7.))
                    .rounded(px(5.))
                    .text_size(rems(11. / 16.))
                    .cursor_pointer()
                    .when(is_active, |d| {
                        d.bg(gpui::white().opacity(0.10))
                            .text_color(TEXT_PRIMARY())
                            .font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .when(!is_active, |d| {
                        d.text_color(TEXT_SECONDARY())
                            .hover(|s| s.bg(gpui::white().opacity(0.05)))
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.project_page_pr_filter = i;
                            cx.notify();
                        }),
                    )
                    .child(tab_label),
            );
        }
        section = section.child(tabs);

        // PR search bar
        section = section.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .px(px(12.))
                        .py(px(8.))
                        .rounded(px(7.))
                        .bg(gpui::white().opacity(0.05))
                        .border_1()
                        .border_color(gpui::white().opacity(0.08))
                        .child(
                            svg()
                                .path("assets/icons/icons__file_icons__magnifying_glass.svg")
                                .size(px(14.))
                                .text_color(TEXT_MUTED()),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(TEXT_MUTED())
                                .child("GitHub query, e.g. author:@me review-requested:@me"),
                        ),
                )
                .child(
                    div()
                        .id("pr-search-apply")
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .px(px(7.))
                        .rounded(px(7.))
                        .bg(rgb(0x1e2024))
                        .border_1()
                        .border_color(gpui::white().opacity(0.08))
                        .hover(|s| s.bg(gpui::white().opacity(0.06)))
                        .cursor_pointer()
                        .text_size(rems(11. / 16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(TEXT_PRIMARY())
                        .child("Apply"),
                )
                .child(
                    div()
                        .id("pr-search-clear")
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .px(px(7.))
                        .rounded(px(7.))
                        .hover(|s| s.bg(gpui::white().opacity(0.06)))
                        .cursor_pointer()
                        .text_size(rems(11. / 16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(TEXT_SECONDARY())
                        .child("Clear"),
                ),
        );

        // Syntax hint
        section = section.child(
            div()
                .text_xs()
                .text_color(TEXT_MUTED())
                .child(
                    "Use GitHub PR search syntax like review-requested:@me, author:@me, draft:true, or free-text terms.",
                ),
        );

        // PR rows
        for pr in MOCK_PRS {
            section = section.child(Self::project_page_pr_row(pr));
        }

        section
    }

    fn project_page_pr_row(pr: &MockPr) -> impl IntoElement {
        let number_label: SharedString = format!("#{}", pr.number).into();
        let title: SharedString = pr.title.into();
        let branch: SharedString = pr.branch.into();
        let author: SharedString = pr.author.into();
        let added: SharedString = format!("+{}", pr.lines_added).into();
        let removed: SharedString = format!("-{}", pr.lines_removed).into();

        let ci_icon = if pr.ci_passed {
            "assets/icons/icons__badge-check.svg"
        } else {
            "assets/icons/icons__badge-x.svg"
        };
        let ci_color = if pr.ci_passed { GREEN() } else { RED() };

        let row_id = SharedString::from(format!("pr-row-{}", pr.number));

        let mut row = div()
            .id(row_id)
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(12.))
            .rounded(px(8.))
            .bg(gpui::white().opacity(0.03))
            .hover(|s| s.bg(gpui::white().opacity(0.06)))
            .border_1()
            .border_color(gpui::white().opacity(0.06));

        // Top line: number + CI + title + review badge
        let mut top = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .bg(gpui::white().opacity(0.08))
                    .text_xs()
                    .text_color(TEXT_SECONDARY())
                    .child(number_label),
            )
            .child(svg().path(ci_icon).size(px(16.)).text_color(ci_color))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .text_color(TEXT_PRIMARY())
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .truncate()
                    .child(title),
            );

        if pr.review_required {
            top = top.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(px(10.))
                    .bg(hsla(30. / 360., 0.70, 0.35, 1.))
                    .text_xs()
                    .text_color(hsla(30. / 360., 0.90, 0.80, 1.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(div().w(px(6.)).h(px(6.)).rounded_full().bg(hsla(
                        30. / 360.,
                        0.90,
                        0.65,
                        1.,
                    )))
                    .child("Review required"),
            );
        }

        row = row.child(top);

        // Bottom line: branch, author, diff, reviewers, review button
        let mut bottom = div().flex().flex_row().items_center().gap(px(8.));

        // Branch name
        bottom = bottom.child(
            div()
                .text_xs()
                .text_color(TEXT_MUTED())
                .font_family("Lilex Nerd Font Mono")
                .truncate()
                .max_w(px(200.))
                .child(branch),
        );

        // Separator dot
        bottom = bottom.child(div().text_xs().text_color(TEXT_MUTED()).child("\u{00B7}"));

        // Author
        bottom = bottom.child(div().text_xs().text_color(TEXT_SECONDARY()).child(author));

        // Separator dot
        bottom = bottom.child(div().text_xs().text_color(TEXT_MUTED()).child("\u{00B7}"));

        // Diff stats
        bottom = bottom.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .text_xs()
                        .text_color(GREEN())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(added),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(RED())
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(removed),
                ),
        );

        // Spacer
        bottom = bottom.child(div().flex_1());

        // Reviewer dots
        for &(_name, color) in pr.reviewers {
            bottom = bottom.child(div().w(px(8.)).h(px(8.)).rounded_full().bg(rgb(color)));
        }

        // Review button
        let review_btn_id = SharedString::from(format!("pr-review-{}", pr.number));
        bottom = bottom.child(
            div()
                .id(review_btn_id)
                .px(px(12.))
                .py(px(4.))
                .rounded(px(6.))
                .bg(gpui::white().opacity(0.10))
                .hover(|s| s.bg(gpui::white().opacity(0.18)))
                .cursor_pointer()
                .text_xs()
                .text_color(TEXT_PRIMARY())
                .font_weight(gpui::FontWeight::MEDIUM)
                .child("Review"),
        );

        // Action icons
        bottom = bottom.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .child(
                    svg()
                        .path("assets/icons/icons__git-pull-request-create.svg")
                        .size(px(14.))
                        .text_color(TEXT_SECONDARY()),
                )
                .child(
                    svg()
                        .path("assets/icons/icons__external-link.svg")
                        .size(px(14.))
                        .text_color(TEXT_SECONDARY()),
                ),
        );

        row = row.child(bottom);
        row
    }
}
