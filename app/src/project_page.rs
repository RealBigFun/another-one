//! Project page rendered in the centre panel when a project is selected.

use gpui::{
    div, hsla, prelude::*, px, rems, svg, Context, MouseButton, MouseDownEvent, SharedString,
    Window,
};

use crate::agents::terminal_launch_config_for_selected_agent;
use crate::app::{AnotherOneApp, WorkspacePane};
use crate::left_sidebar::open_external_url;
use crate::project_store::{ProjectBranchSettingField, ResolvedProjectBranchSettings};
use crate::task_launcher::TaskLaunchRequest;
use crate::theme::{self, AppTheme};

const PR_FILTER_TABS: &[&str] = &["All Open", "Needs My Review", "My PRs", "Draft"];

impl WorkspacePane {
    fn project_page_theme(&self, cx: &mut Context<Self>) -> AppTheme {
        self.app
            .upgrade()
            .map(|entity| {
                theme::app_theme_for_preference(entity.read(cx).project_store.ui.theme_mode)
            })
            .unwrap_or_else(theme::dark_theme)
    }

    // ── Public entry point ───────────────────────────────────────────

    pub(crate) fn render_project_page(
        &mut self,
        project_id: &str,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let app_theme = self.project_page_theme(cx);
        let Some(app_entity) = self.app.upgrade() else {
            return div()
                .flex()
                .flex_col()
                .size_full()
                .bg(app_theme.sunken_bg)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(app_theme.text_muted)
                        .child("Project not found"),
                );
        };
        let project = {
            let app = app_entity.read(cx);
            app.project_store
                .projects
                .iter()
                .find(|p| p.id == project_id)
                .cloned()
        };

        let Some(project) = project else {
            return div()
                .flex()
                .flex_col()
                .size_full()
                .bg(app_theme.sunken_bg)
                .child(
                    div()
                        .flex_1()
                        .flex()
                        .items_center()
                        .justify_center()
                        .text_sm()
                        .text_color(app_theme.text_muted)
                        .child("Project not found"),
                );
        };

        let project_name: SharedString = project.name.clone().into();
        let project_id_owned = project_id.to_string();

        let (
            github_url,
            branch_settings,
            config_panel_expanded,
            config_panel_targeted,
            config_dropdown,
        ) = {
            let app = app_entity.read(cx);
            (
                app.project_github_links.get(project_id).cloned(),
                app.project_store.resolved_branch_settings(project_id),
                app.project_page_config_panel_expanded,
                app.project_page_config_panel_targeted,
                app.project_page_config_dropdown,
            )
        };
        app_entity.update(cx, |app, _cx| {
            app.request_project_page_pull_requests(
                project_id,
                &project.path,
                self.project_page_pr_filter,
                &self.project_page_pr_query,
            );
        });

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(app_theme.sunken_bg)
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
                    .child(self.project_page_prs_section(cx))
                    .when_some(branch_settings.as_ref(), |container, settings| {
                        container.child(self.project_page_configuration_section(
                            &project_id_owned,
                            settings,
                            config_panel_expanded,
                            config_panel_targeted,
                            config_dropdown,
                            cx,
                        ))
                    }),
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
        let app_theme = self.project_page_theme(cx);
        let project_id_for_new_task = project_id.to_string();
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
            .border_color(app_theme.overlay_hover)
            // Project name
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_color(app_theme.text_primary)
                    .text_size(rems(1.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .truncate()
                    .child(project_name.clone()),
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
                    .bg(app_theme.card_bg)
                    .border_1()
                    .border_color(app_theme.border)
                    .hover(|s| s.bg(app_theme.overlay_hover))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                            this.focus_handle.focus(window, cx);
                            this.open_new_task_modal(&project_id_for_new_task, cx);
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__plus.svg")
                            .size(px(12.))
                            .text_color(app_theme.text_primary),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .text_color(app_theme.text_primary)
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .child("New Task"),
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
                        .bg(app_theme.card_bg)
                        .border_1()
                        .border_color(app_theme.border)
                        .hover(|s| s.bg(app_theme.overlay_hover))
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
                                .text_color(app_theme.text_secondary),
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
                    .bg(app_theme.card_bg)
                    .border_1()
                    .border_color(app_theme.border)
                    .hover(|s| s.bg(app_theme.overlay_hover))
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
                            .text_color(app_theme.text_secondary),
                    ),
            )
    }

    // ── Open PRs section ─────────────────────────────────────────────

    fn project_page_prs_section(&self, cx: &mut Context<Self>) -> gpui::Div {
        let app_theme = self.project_page_theme(cx);
        let collapsed = self.project_page_prs_collapsed;
        let app = self.app.upgrade().map(|entity| entity.read(cx));
        let project_id = self.active_project_page.clone().unwrap_or_default();
        let query_key = crate::app::AnotherOneApp::project_page_pr_query_key(
            &project_id,
            self.project_page_pr_filter,
            &self.project_page_pr_query,
        );
        let prs = app
            .as_ref()
            .and_then(|app| app.project_page_pull_requests.get(&query_key))
            .cloned();
        let pr_count = prs.as_ref().map_or(0, |prs| prs.len());
        let loading = app
            .as_ref()
            .is_some_and(|app| app.project_page_pull_requests_loading.contains(&query_key));
        let load_error = app
            .as_ref()
            .and_then(|app| app.project_page_pull_requests_errors.get(&query_key))
            .cloned();
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
                        .text_color(app_theme.text_secondary),
                )
                .child(
                    div()
                        .text_color(app_theme.text_primary)
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
                        .bg(app_theme.overlay_active)
                        .text_xs()
                        .text_color(app_theme.text_secondary)
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
                    .flex()
                    .items_center()
                    .justify_center()
                    .h(px(26.))
                    .px(px(7.))
                    .rounded(px(5.))
                    .text_size(rems(11. / 16.))
                    .cursor_pointer()
                    .when(is_active, |d| {
                        d.bg(app_theme.overlay_active)
                            .text_color(app_theme.text_primary)
                            .font_weight(gpui::FontWeight::MEDIUM)
                    })
                    .when(!is_active, |d| {
                        d.text_color(app_theme.text_secondary)
                            .hover(|s| s.bg(app_theme.overlay_rest))
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
                        .bg(app_theme.overlay_rest)
                        .border_1()
                        .border_color(app_theme.border)
                        .child(
                            svg()
                                .path("assets/icons/icons__file_icons__magnifying_glass.svg")
                                .size(px(14.))
                                .text_color(app_theme.text_muted),
                        )
                        .child({
                            let pr_query_hint: SharedString =
                                if self.project_page_pr_query_draft.is_empty() {
                                    "GitHub query, e.g. author:@me review-requested:@me".into()
                                } else {
                                    self.project_page_pr_query_draft.clone().into()
                                };
                            div()
                                .text_sm()
                                .text_color(app_theme.text_muted)
                                .child(pr_query_hint)
                        }),
                )
                .child(
                    div()
                        .id("pr-search-apply")
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .px(px(7.))
                        .rounded(px(7.))
                        .bg(app_theme.card_bg)
                        .border_1()
                        .border_color(app_theme.border)
                        .hover(|s| s.bg(app_theme.overlay_hover))
                        .cursor_pointer()
                        .text_size(rems(11. / 16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(app_theme.text_primary)
                        .child("Apply")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                this.project_page_pr_query =
                                    this.project_page_pr_query_draft.clone();
                                if let Some(app) = this.app.upgrade() {
                                    app.update(cx, |app, _app_cx| {
                                        if let Some(project_id) = this.active_project_page.clone() {
                                            let project_path = app
                                                .project_store
                                                .project(&project_id)
                                                .map(|project| project.path.clone());
                                            if let Some(project_path) = project_path {
                                                app.request_project_page_pull_requests(
                                                    &project_id,
                                                    &project_path,
                                                    this.project_page_pr_filter,
                                                    &this.project_page_pr_query,
                                                );
                                            }
                                        }
                                    });
                                }
                                cx.notify();
                            }),
                        ),
                )
                .child(
                    div()
                        .id("pr-search-clear")
                        .h(px(30.))
                        .flex()
                        .items_center()
                        .px(px(7.))
                        .rounded(px(7.))
                        .hover(|s| s.bg(app_theme.overlay_hover))
                        .cursor_pointer()
                        .text_size(rems(11. / 16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(app_theme.text_secondary)
                        .child("Clear")
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                this.project_page_pr_query_draft.clear();
                                this.project_page_pr_query.clear();
                                if let Some(app) = this.app.upgrade() {
                                    app.update(cx, |app, _app_cx| {
                                        if let Some(project_id) = this.active_project_page.clone() {
                                            let project_path = app
                                                .project_store
                                                .project(&project_id)
                                                .map(|project| project.path.clone());
                                            if let Some(project_path) = project_path {
                                                app.request_project_page_pull_requests(
                                                    &project_id,
                                                    &project_path,
                                                    this.project_page_pr_filter,
                                                    "",
                                                );
                                            }
                                        }
                                    });
                                }
                                cx.notify();
                            }),
                        ),
                ),
        );

        // Syntax hint
        section = section.child(
            div()
                .text_xs()
                .text_color(app_theme.text_muted)
                .child(
                    "Use GitHub PR search syntax like review-requested:@me, author:@me, draft:true, or free-text terms.",
                ),
        );

        // PR rows
        if let Some(error) = load_error {
            section = section.child(
                div()
                    .text_sm()
                    .text_color(app_theme.text_muted)
                    .child(error),
            );
        } else if loading && prs.is_none() {
            section = section.child(
                div()
                    .text_sm()
                    .text_color(app_theme.text_muted)
                    .child("Loading pull requests..."),
            );
        } else if let Some(prs) = prs {
            if prs.is_empty() {
                section = section.child(
                    div()
                        .text_sm()
                        .text_color(app_theme.text_muted)
                        .child("No matching open pull requests."),
                );
            } else {
                for pr in prs.iter() {
                    section = section.child(self.project_page_pr_row(pr, cx));
                }
            }
        }

        section
    }

    fn project_page_pr_row(
        &self,
        pr: &crate::git_actions::ProjectPagePullRequest,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = self.project_page_theme(cx);
        let number_label: SharedString = format!("#{}", pr.number).into();
        let title: SharedString = pr.title.clone().into();
        let branch: SharedString = pr.branch.clone().into();
        let author: SharedString = pr.author.clone().into();
        let added: SharedString = format!("+{}", pr.lines_added).into();
        let removed: SharedString = format!("-{}", pr.lines_removed).into();
        let pr_url = pr.url.clone();
        let review_project_id = self.active_project_page.clone().unwrap_or_default();
        let review_pr_number = pr.number;
        let review_pr_url = pr.url.clone();
        let review_head_branch = pr.branch.clone();
        let number_link_id = SharedString::from(format!("pr-number-link-{}", pr.number));

        let ci_icon = if pr.review_required {
            "assets/icons/icons__badge-x.svg"
        } else {
            "assets/icons/icons__badge-check.svg"
        };
        let ci_color = if pr.review_required {
            app_theme.error.text
        } else {
            app_theme.success.text
        };

        let row_id = SharedString::from(format!("pr-row-{}", pr.number));

        let mut row = div()
            .id(row_id)
            .flex()
            .flex_col()
            .gap(px(6.))
            .px(px(12.))
            .py(px(12.))
            .rounded(px(8.))
            .bg(app_theme.overlay_rest)
            .hover(|s| s.bg(app_theme.overlay_hover))
            .border_1()
            .border_color(app_theme.overlay_hover);

        // Top line: number + CI + title + review badge
        let mut top = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                div()
                    .id(number_link_id)
                    .px(px(6.))
                    .py(px(2.))
                    .rounded(px(5.))
                    .bg(app_theme.border)
                    .text_xs()
                    .text_color(app_theme.text_secondary)
                    .hover(|s| {
                        s.bg(app_theme.overlay_hover_strong)
                            .text_color(app_theme.text_primary)
                    })
                    .cursor_pointer()
                    .tooltip(|_window, cx| {
                        AnotherOneApp::action_tooltip_view("Open pull request in GitHub", cx)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            if let Err(err) = open_external_url(&pr_url) {
                                this.show_error_toast(err, cx);
                            }
                        }),
                    )
                    .child(number_label),
            )
            .child(svg().path(ci_icon).size(px(16.)).text_color(ci_color))
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .text_sm()
                    .text_color(app_theme.text_primary)
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .truncate()
                    .child(title),
            );

        if pr.review_required || pr.draft {
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
                    .child(if pr.draft { "Draft" } else { "Review required" }),
            );
        }

        row = row.child(top);

        // Bottom line: branch, author, diff, reviewers, review button
        let mut bottom = div().flex().flex_row().items_center().gap(px(8.));

        // Branch name
        bottom = bottom.child(
            div()
                .text_xs()
                .text_color(app_theme.text_muted)
                .font_family("Lilex Nerd Font Mono")
                .truncate()
                .max_w(px(200.))
                .child(branch),
        );

        // Separator dot
        bottom = bottom.child(
            div()
                .text_xs()
                .text_color(app_theme.text_muted)
                .child("\u{00B7}"),
        );

        // Author
        bottom = bottom.child(
            div()
                .text_xs()
                .text_color(app_theme.text_secondary)
                .child(author),
        );

        // Separator dot
        bottom = bottom.child(
            div()
                .text_xs()
                .text_color(app_theme.text_muted)
                .child("\u{00B7}"),
        );

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
                        .text_color(app_theme.success.text)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(added),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(app_theme.error.text)
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .child(removed),
                ),
        );

        // Spacer
        bottom = bottom.child(div().flex_1());

        // Review button
        let review_btn_id = SharedString::from(format!("pr-review-{}", pr.number));
        bottom = bottom.child(
            div()
                .id(review_btn_id)
                .px(px(12.))
                .py(px(4.))
                .rounded(px(6.))
                .bg(app_theme.overlay_active)
                .hover(|s| s.bg(app_theme.overlay_hover_strong))
                .cursor_pointer()
                .text_xs()
                .text_color(app_theme.text_primary)
                .font_weight(gpui::FontWeight::MEDIUM)
                .tooltip(|_window, cx| {
                    AnotherOneApp::action_tooltip_view(
                        "Open a review task for this pull request",
                        cx,
                    )
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        let app = this.app.clone();
                        let project_id = review_project_id.clone();
                        let pull_request_url = review_pr_url.clone();
                        let head_branch = review_head_branch.clone();
                        cx.defer(move |cx| {
                            let _ = app.update(cx, |app, app_cx| {
                                let launch_config = terminal_launch_config_for_selected_agent(
                                    app.default_agent_id(),
                                )
                                .unwrap_or_default();
                                app.launch_task_request(
                                    TaskLaunchRequest::Review {
                                        project_id,
                                        pull_request_number: review_pr_number,
                                        pull_request_url,
                                        head_branch,
                                        launch_config,
                                    },
                                    app_cx,
                                );
                            });
                        });
                    }),
                )
                .child("Review"),
        );

        row = row.child(bottom);
        row
    }

    fn project_page_configuration_section(
        &self,
        project_id: &str,
        settings: &ResolvedProjectBranchSettings,
        expanded: bool,
        targeted: bool,
        open_dropdown: Option<ProjectBranchSettingField>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = self.project_page_theme(cx);
        let chevron_icon = if expanded {
            "assets/icons/icons__chevron-down.svg"
        } else {
            "assets/icons/icons__chevron-right.svg"
        };

        let mut section = div()
            .id("project-page-configuration-panel")
            .flex()
            .flex_col()
            .gap(px(12.))
            .mt(px(28.))
            .mb(px(24.))
            .child(
                div()
                    .id("project-page-configuration-header")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            let _ = this.app.update(cx, |app, app_cx| {
                                app.toggle_project_page_config_panel(app_cx);
                            });
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
                                    .path(chevron_icon)
                                    .size(px(15.))
                                    .text_color(app_theme.text_secondary),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(2.))
                                    .child(
                                        div()
                                            .text_size(rems(13. / 16.))
                                            .font_weight(gpui::FontWeight::SEMIBOLD)
                                            .text_color(app_theme.text_primary)
                                            .child("Configuration"),
                                    )
                                    .child(
                                        div()
                                            .text_size(rems(11. / 16.))
                                            .text_color(app_theme.text_muted)
                                            .child(
                                                "These defaults apply to the whole project group.",
                                            ),
                                    ),
                            ),
                    )
                    .when(targeted, |header| header.child(div().flex_1()))
                    .when(targeted, |header| {
                        header.child(
                            div()
                                .px(px(8.))
                                .py(px(3.))
                                .rounded(px(999.))
                                .bg(hsla(210. / 360., 0.45, 0.30, 1.))
                                .text_size(rems(10. / 16.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(hsla(210. / 360., 0.72, 0.80, 1.))
                                .child("Targeted"),
                        )
                    }),
            );

        if !expanded {
            return section;
        }

        section = section.child(
            div()
                .flex()
                .flex_col()
                .gap(px(12.))
                .child(self.project_page_branch_config_row(
                    project_id,
                    "Default Branch",
                    "Preferred base branch for new tasks and worktrees.",
                    settings,
                    ProjectBranchSettingField::DefaultBranch,
                    settings.configured_default_branch.as_deref(),
                    settings.effective_default_branch.as_deref(),
                    open_dropdown == Some(ProjectBranchSettingField::DefaultBranch),
                    cx,
                ))
                .child(self.project_page_branch_config_row(
                    project_id,
                    "Default Target Branch",
                    "Used for PR creation.",
                    settings,
                    ProjectBranchSettingField::DefaultTargetBranch,
                    settings.configured_default_target_branch.as_deref(),
                    settings.effective_default_target_branch.as_deref(),
                    open_dropdown == Some(ProjectBranchSettingField::DefaultTargetBranch),
                    cx,
                )),
        );

        section
    }

    #[allow(clippy::too_many_arguments)]
    fn project_page_branch_config_row(
        &self,
        project_id: &str,
        title: &'static str,
        description: &'static str,
        settings: &ResolvedProjectBranchSettings,
        field: ProjectBranchSettingField,
        configured_value: Option<&str>,
        effective_value: Option<&str>,
        dropdown_open: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = self.project_page_theme(cx);
        let trigger_id = match field {
            ProjectBranchSettingField::DefaultBranch => "project-page-default-branch",
            ProjectBranchSettingField::DefaultTargetBranch => "project-page-default-target-branch",
        };
        let selected_label: SharedString = configured_value
            .map(str::to_string)
            .unwrap_or_else(|| "Automatic".to_string())
            .into();
        let helper_text = match (field, configured_value, effective_value) {
            (ProjectBranchSettingField::DefaultBranch, Some(_), Some(branch)) => {
                format!("New worktree tasks will start from {}.", branch)
            }
            (ProjectBranchSettingField::DefaultBranch, None, Some(branch)) => {
                format!("Currently resolving automatically to {}.", branch)
            }
            (ProjectBranchSettingField::DefaultBranch, _, None) => {
                "No branches are currently available.".to_string()
            }
            (ProjectBranchSettingField::DefaultTargetBranch, Some(branch), _) => {
                format!("PRs currently target {}.", branch)
            }
            (ProjectBranchSettingField::DefaultTargetBranch, None, _) => {
                "Unset keeps GitHub PR targeting on its default base.".to_string()
            }
        };
        let project_id = project_id.to_string();

        let mut row = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .rounded(px(10.))
            .bg(app_theme.overlay_rest)
            .border_1()
            .border_color(app_theme.overlay_hover)
            .p(px(12.))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(app_theme.text_primary)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(app_theme.text_muted)
                                    .child(description),
                            ),
                    )
                    .child(
                        div()
                            .id(trigger_id)
                            .min_w(px(220.))
                            .h(px(36.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .bg(app_theme.card_bg)
                            .border_1()
                            .border_color(app_theme.border)
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .hover(|style| style.bg(app_theme.overlay_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                    let _ = this.app.update(cx, |app, app_cx| {
                                        app.toggle_project_page_config_dropdown(field, app_cx);
                                    });
                                }),
                            )
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .text_color(app_theme.text_primary)
                                    .child(selected_label.clone()),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__chevron-down.svg")
                                    .size(px(11.))
                                    .text_color(app_theme.text_secondary),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(rems(11. / 16.))
                    .text_color(app_theme.text_muted)
                    .child(helper_text),
            );

        if dropdown_open {
            let mut options = div()
                .rounded(px(8.))
                .bg(app_theme.card_bg)
                .border_1()
                .border_color(app_theme.border)
                .overflow_hidden()
                .child(self.project_page_branch_config_option(
                    &project_id,
                    field,
                    None,
                    configured_value.is_none(),
                    cx,
                ));

            for branch in &settings.available_branches {
                options = options.child(self.project_page_branch_config_option(
                    &project_id,
                    field,
                    Some(branch.as_str()),
                    configured_value == Some(branch.as_str()),
                    cx,
                ));
            }

            row = row.child(options);
        }

        row
    }

    fn project_page_branch_config_option(
        &self,
        project_id: &str,
        field: ProjectBranchSettingField,
        branch_name: Option<&str>,
        selected: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = self.project_page_theme(cx);
        let project_id = project_id.to_string();
        let branch_name_owned = branch_name.map(str::to_string);
        let label: SharedString = branch_name
            .map(str::to_string)
            .unwrap_or_else(|| "Automatic".to_string())
            .into();

        div()
            .id(SharedString::from(format!(
                "project-page-config-option-{:?}-{}",
                field, label
            )))
            .h(px(36.))
            .px(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .cursor_pointer()
            .bg(if selected {
                app_theme.border
            } else {
                gpui::white().opacity(0.0)
            })
            .hover(|style| style.bg(app_theme.overlay_hover))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    let _ = this.app.update(cx, |app, app_cx| {
                        app.update_project_page_branch_setting(
                            &project_id,
                            field,
                            branch_name_owned.clone(),
                            app_cx,
                        );
                    });
                }),
            )
            .child(
                div()
                    .text_size(rems(12. / 16.))
                    .text_color(app_theme.text_primary)
                    .child(label.clone()),
            )
            .when(selected, |option| {
                option.child(
                    div()
                        .text_size(rems(10. / 16.))
                        .font_weight(gpui::FontWeight::SEMIBOLD)
                        .text_color(app_theme.text_secondary)
                        .child("Selected"),
                )
            })
    }
}
