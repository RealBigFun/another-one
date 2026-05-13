use std::sync::OnceLock;

use gpui::{div, prelude::*, px, rems, svg, Context, MouseButton, MouseDownEvent, Window};

use crate::agent_icons::branded_icon;
use crate::app::AnotherOneApp;
use crate::platform::PlatformServices;
use crate::resource_usage::{
    format_memory, ResourceUsageProject, ResourceUsageSession, ResourceUsageSnapshot,
    ResourceUsageTask,
};
use crate::theme;

const PANEL_W: f32 = 420.;
const PANEL_TOP_MARGIN: f32 = 46.;
pub(crate) const RESOURCE_INDICATOR_BUTTON_W: f32 = 176.;
const RESOURCE_CPU_LABEL_W: f32 = 46.;
const RESOURCE_MEMORY_LABEL_W: f32 = 74.;
const PANEL_BOTTOM_MARGIN: f32 = crate::layout::FOOTER_H + 10.;

impl AnotherOneApp {
    /// Live resource-usage snapshot, sourced from the daemon's
    /// projection (`UiSnapshot.daemon_resource_usage`). Returns an
    /// empty snapshot when the projection hasn't yet carried one
    /// (older daemon, or pre-first-sample) so the indicator widget
    /// renders zeros instead of stale client-side data. See #156.
    fn resource_usage(&self) -> &ResourceUsageSnapshot {
        static EMPTY: OnceLock<ResourceUsageSnapshot> = OnceLock::new();
        self.project_store
            .ui
            .daemon_resource_usage
            .as_ref()
            .unwrap_or_else(|| EMPTY.get_or_init(ResourceUsageSnapshot::default))
    }

    pub(crate) fn toggle_resource_indicator(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_titlebar_dropdowns();
        self.resource_indicator_open = !self.resource_indicator_open;
        if self.resource_indicator_open {
            self.refresh_resource_usage();
        }
        cx.stop_propagation();
        cx.notify();
    }

    pub(crate) fn refresh_resource_indicator(
        &mut self,
        _: &MouseDownEvent,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.refresh_resource_usage();
        cx.stop_propagation();
        cx.notify();
    }

    fn toggle_resource_node(&mut self, node_key: &str, cx: &mut Context<Self>) {
        if !self.resource_collapsed_nodes.insert(node_key.to_string()) {
            self.resource_collapsed_nodes.remove(node_key);
        }
        cx.notify();
    }

    pub(crate) fn resource_indicator_button(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let icon_col = theme::toggle_icon_color_for_mode(window, self.project_store.ui.theme_mode);
        let text_col = app_theme.text_secondary;
        let hover_bg = app_theme.overlay_hover_strong;
        let bg = if self.resource_indicator_open {
            app_theme.overlay_active
        } else {
            app_theme.overlay_rest
        };
        let border = app_theme.border;
        let cpu_label = format!("{:.1}%", self.resource_usage().app.cpu_percent);
        let memory_label = format_memory(self.resource_usage().app.memory_bytes);

        div()
            .id("resource-indicator-button")
            .flex()
            .flex_shrink_0()
            .items_center()
            .gap(px(6.))
            .w(px(RESOURCE_INDICATOR_BUTTON_W))
            .h(px(28.))
            .px(px(8.))
            .mr(px(6.))
            .rounded(px(11.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .cursor_pointer()
            .hover(move |style| style.bg(hover_bg))
            .tooltip(move |_window, cx| Self::action_tooltip_view("Show resource usage", cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(Self::toggle_resource_indicator),
            )
            .child(
                svg()
                    .path("assets/icons/icons__resource-usage.svg")
                    .size(px(11.))
                    .text_color(icon_col),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_end()
                    .flex_1()
                    .gap(px(4.))
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .child(
                        div()
                            .w(px(RESOURCE_CPU_LABEL_W))
                            .text_right()
                            .text_color(text_col)
                            .child(cpu_label),
                    )
                    .child(div().text_color(app_theme.divider).child("|"))
                    .child(
                        div()
                            .w(px(RESOURCE_MEMORY_LABEL_W))
                            .text_right()
                            .text_color(text_col)
                            .child(memory_label),
                    ),
            )
    }

    pub(crate) fn resource_indicator_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if !self.resource_indicator_open {
            return div().id("resource-indicator-overlay");
        }

        let panel = self.resource_indicator_panel(window, cx);
        if crate::platform::CurrentPlatform::supports_custom_chrome(window) {
            // Chrome present → anchor below the in-app titlebar.
            div()
                .id("resource-indicator-overlay")
                .absolute()
                .right(px(12.))
                .top(px(PANEL_TOP_MARGIN))
                .child(panel)
        } else {
            // No in-app chrome → anchor above the footer.
            div()
                .id("resource-indicator-overlay")
                .absolute()
                .right(px(12.))
                .bottom(px(PANEL_BOTTOM_MARGIN))
                .child(panel)
        }
    }

    fn resource_indicator_panel(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let panel_bg = app_theme.card_bg;
        let surface_bg = app_theme.sunken_bg;
        let border = app_theme.border;
        let title_col = app_theme.text_primary;
        let muted_col = app_theme.text_muted;
        let stat_col = app_theme.text_primary;
        let empty_col = app_theme.text_muted;
        let session_count = self.resource_usage().session_count.to_string();

        let mut tree = div().flex().flex_col().gap(px(4.));
        if self.resource_usage().projects.is_empty() {
            tree = tree.child(
                div()
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(px(10.))
                    .bg(app_theme.overlay_rest)
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .text_color(empty_col)
                            .child("No active terminal sessions"),
                    ),
            );
        } else {
            for project in &self.resource_usage().projects {
                tree = tree.child(self.resource_project_group(project, cx));
            }
        }

        div()
            .w(px(PANEL_W))
            .rounded(px(14.))
            .bg(panel_bg)
            .border_1()
            .border_color(border)
            .shadow_md()
            .occlude()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_between()
                    .px(px(20.))
                    .pt(px(18.))
                    .pb(px(12.))
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(muted_col)
                            .child("RESOURCE USAGE"),
                    )
                    .child(
                        div()
                            .id("resource-indicator-refresh")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(24.))
                            .h(px(24.))
                            .rounded_md()
                            .cursor_pointer()
                            .hover(move |style| style.bg(app_theme.overlay_hover_strong))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view("Refresh resource usage", cx)
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::refresh_resource_indicator),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__refresh.svg")
                                    .size(px(14.))
                                    .text_color(title_col),
                            ),
                    ),
            )
            .child(
                div()
                    .px(px(20.))
                    .pb(px(10.))
                    .child(Self::resource_section_heading("APP SHELL", title_col)),
            )
            .child(
                div()
                    .flex()
                    .gap(px(12.))
                    .px(px(20.))
                    .child(Self::resource_stat_card(
                        "APP CPU",
                        format!("{:.1}%", self.resource_usage().app.cpu_percent),
                        surface_bg.into(),
                        muted_col,
                        stat_col,
                    ))
                    .child(Self::resource_stat_card(
                        "APP MEM",
                        format_memory(self.resource_usage().app.memory_bytes),
                        surface_bg.into(),
                        muted_col,
                        stat_col,
                    ))
                    .child(Self::resource_stat_card(
                        "SESSIONS",
                        session_count,
                        surface_bg.into(),
                        muted_col,
                        stat_col,
                    )),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(10.))
                    .px(px(20.))
                    .pt(px(16.))
                    .pb(px(20.))
                    .child(Self::resource_section_heading(
                        "TERMINAL SESSIONS",
                        title_col,
                    ))
                    .child(tree),
            )
    }

    fn resource_stat_card(
        title: impl Into<gpui::SharedString>,
        value: impl Into<gpui::SharedString>,
        bg: gpui::Hsla,
        title_col: gpui::Hsla,
        value_col: gpui::Hsla,
    ) -> impl IntoElement {
        div()
            .flex_1()
            .min_w(px(0.))
            .rounded(px(12.))
            .bg(bg)
            .px(px(14.))
            .py(px(14.))
            .child(
                div()
                    .text_size(rems(11. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col)
                    .child(title.into()),
            )
            .child(
                div()
                    .pt(px(6.))
                    .text_size(rems(20. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(value_col)
                    .child(value.into()),
            )
    }

    fn resource_section_heading(label: &'static str, text_col: gpui::Hsla) -> impl IntoElement {
        div().child(
            div()
                .text_size(rems(14. / 16.))
                .font_weight(gpui::FontWeight::SEMIBOLD)
                .text_color(text_col)
                .child(label),
        )
    }

    fn resource_project_group(
        &self,
        project: &ResourceUsageProject,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let collapsed = self.resource_collapsed_nodes.contains(&project.key);
        let project_key = project.key.clone();
        let mut group = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(Self::resource_group_row(
                &project.label,
                project.cpu_percent,
                project.memory_bytes,
                0.0,
                collapsed,
                true,
                app_theme.text_primary,
                app_theme.text_muted,
                app_theme.overlay_hover,
                app_theme.text_muted,
                Some(move |this: &mut Self, cx: &mut Context<Self>| {
                    this.toggle_resource_node(&project_key, cx);
                }),
                cx,
            ));

        if !collapsed {
            for task in &project.tasks {
                group = group.child(self.resource_task_group(task, cx));
            }
        }

        group
    }

    fn resource_task_group(
        &self,
        task: &ResourceUsageTask,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let collapsed = self.resource_collapsed_nodes.contains(&task.key);
        let task_key = task.key.clone();
        let mut group = div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(Self::resource_group_row(
                &task.label,
                task.cpu_percent,
                task.memory_bytes,
                20.0,
                collapsed,
                true,
                app_theme.text_secondary,
                app_theme.text_muted,
                app_theme.overlay_hover,
                app_theme.text_muted,
                Some(move |this: &mut Self, cx: &mut Context<Self>| {
                    this.toggle_resource_node(&task_key, cx);
                }),
                cx,
            ));

        if !collapsed {
            for session in &task.sessions {
                group = group.child(Self::resource_session_row(
                    session,
                    app_theme.text_secondary,
                    app_theme.text_muted,
                ));
            }
        }

        group
    }

    fn resource_group_row(
        label: &str,
        cpu_percent: f32,
        memory_bytes: u64,
        indent: f32,
        collapsed: bool,
        collapsible: bool,
        text_col: gpui::Hsla,
        metric_col: gpui::Hsla,
        hover_bg: gpui::Hsla,
        chevron_col: gpui::Hsla,
        on_toggle: Option<impl Fn(&mut Self, &mut Context<Self>) + 'static>,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let row = div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .h(px(32.))
            .pl(px(indent))
            .pr(px(4.))
            .rounded(px(8.))
            .when(collapsible, |row| {
                row.cursor_pointer().hover(move |style| style.bg(hover_bg))
            })
            .when_some(on_toggle, |row, on_toggle| {
                row.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        on_toggle(this, cx);
                        cx.stop_propagation();
                    }),
                )
            })
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w(px(0.))
                    .flex_1()
                    .child(
                        div().w(px(14.)).flex().justify_center().child(
                            svg()
                                .path(if collapsed {
                                    "assets/icons/icons__chevron-right.svg"
                                } else {
                                    "assets/icons/icons__chevron-down.svg"
                                })
                                .size(px(10.))
                                .text_color(chevron_col),
                        ),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(text_col)
                            .child(label.to_string()),
                    ),
            )
            .child(Self::resource_metrics(
                cpu_percent,
                memory_bytes,
                metric_col,
            ));

        row
    }

    fn resource_session_row(
        session: &ResourceUsageSession,
        title_col: gpui::Hsla,
        metric_col: gpui::Hsla,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .h(px(32.))
            .pl(px(42.))
            .pr(px(4.))
            .rounded(px(8.))
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .min_w(px(0.))
                    .flex_1()
                    .child(branded_icon(session.icon_path.clone(), 16., Some(title_col)))
                    .child(
                        div()
                            .text_size(rems(12.5 / 16.))
                            .text_color(title_col)
                            .child(session.label.clone()),
                    ),
            )
            .child(Self::resource_metrics(
                session.cpu_percent,
                session.memory_bytes,
                metric_col,
            ))
    }

    fn resource_metrics(
        cpu_percent: f32,
        memory_bytes: u64,
        text_col: gpui::Hsla,
    ) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .gap(px(18.))
            .flex_shrink_0()
            .child(
                div()
                    .w(px(58.))
                    .text_size(rems(14. / 16.))
                    .text_color(text_col)
                    .child(format!("{:.1}%", cpu_percent)),
            )
            .child(
                div()
                    .w(px(84.))
                    .text_size(rems(14. / 16.))
                    .text_color(text_col)
                    .child(format_memory(memory_bytes)),
            )
    }
}
