//! Reusable panel helper and main content assembly.

use gpui::{
    div, hsla, prelude::*, px, rgb, svg, Context, MouseButton, MouseDownEvent, Render,
    SharedString, Window,
};

use crate::app::{AnotherOneApp, WorkspacePane};

impl AnotherOneApp {
    /// Generic bordered panel with a title strip and body text.
    pub fn panel(
        title: &'static str,
        body: &'static str,
        bg: gpui::Hsla,
        dark: bool,
    ) -> impl IntoElement {
        let title_col = if dark {
            hsla(0., 0., 0.85, 1.)
        } else {
            gpui::rgb(0x1a1a1a).into()
        };
        let body_col = if dark {
            hsla(0., 0., 0.55, 1.)
        } else {
            gpui::rgb(0x333333).into()
        };
        let border = if dark {
            gpui::white().opacity(0.06)
        } else {
            gpui::black().opacity(0.08)
        };
        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(bg)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .items_center()
                    .h(px(36.))
                    .px_3()
                    .border_b_1()
                    .border_color(border)
                    .text_sm()
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(title_col)
                    .child(title),
            )
            .child(
                div()
                    .flex_1()
                    .p_3()
                    .text_sm()
                    .text_color(body_col)
                    .child(body),
            )
    }
}

impl WorkspacePane {
    fn section_main_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if let Some(ref project_id) = self.active_project_page.clone() {
            return self.render_project_page(project_id, window, cx);
        }

        let Some(ref section_id) = self.active_section.clone() else {
            return div().flex().flex_col().size_full().bg(rgb(0x1e1f22)).child(
                div()
                    .flex_1()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_sm()
                    .text_color(hsla(0., 0., 0.40, 1.))
                    .child("Select a branch to get started"),
            );
        };

        let tab_bar_bg = rgb(0x27292e);
        let tab_bg_active = rgb(0x1e1f22);
        let tab_bg_inactive = rgb(0x2b2d31);
        let tab_text_active = hsla(0., 0., 0.92, 1.);
        let tab_text_inactive = hsla(0., 0., 0.55, 1.);
        let tab_hover = rgb(0x2f3136);
        let close_col = hsla(0., 0., 0.45, 1.);
        let close_hover = hsla(0., 0., 0.80, 1.);
        let border_col = gpui::white().opacity(0.06);
        let plus_col = hsla(0., 0., 0.50, 1.);
        let terminal_bg = rgb(0x1e1f22);

        let sid_for_add = section_id.clone();

        let mut tab_bar = div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(36.))
            .bg(tab_bar_bg)
            .border_b_1()
            .border_color(border_col)
            .overflow_hidden();

        let section_state = self.section_states.get(section_id);

        if let Some(state) = section_state {
            for (i, tab) in state.tabs.iter().enumerate() {
                let is_active = i == state.active_tab;
                let tab_title: SharedString = if state.tabs.len() > 1 {
                    format!("{} {}", tab.title, i + 1).into()
                } else {
                    tab.title.clone().into()
                };

                let sid_click = section_id.clone();
                let tab_index = i;
                let sid_close = section_id.clone();
                let close_index = i;
                let can_close = state.tabs.len() > 1;
                let tab_id_val = tab.id.clone();

                tab_bar = tab_bar.child(
                    div()
                        .id(SharedString::from(format!("tab-{}", tab_id_val)))
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .h_full()
                        .px(px(12.))
                        .cursor_pointer()
                        .bg(if is_active {
                            tab_bg_active
                        } else {
                            tab_bg_inactive
                        })
                        .hover(move |s| s.bg(if is_active { tab_bg_active } else { tab_hover }))
                        .tooltip(move |_window, cx| {
                            AnotherOneApp::action_tooltip_view("Switch to this tab", cx)
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                this.activate_tab(&sid_click, tab_index, cx);
                            }),
                        )
                        .child(
                            svg()
                                .path("assets/icons/icons__terminal.svg")
                                .size(px(14.))
                                .text_color(if is_active {
                                    tab_text_active
                                } else {
                                    tab_text_inactive
                                }),
                        )
                        .child(
                            div()
                                .text_sm()
                                .text_color(if is_active {
                                    tab_text_active
                                } else {
                                    tab_text_inactive
                                })
                                .child(tab_title),
                        )
                        .when(can_close, |d| {
                            d.child(
                                div()
                                    .id(SharedString::from(format!("tab-close-{}", tab_id_val)))
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(18.))
                                    .h(px(18.))
                                    .rounded(px(4.))
                                    .cursor_pointer()
                                    .text_color(close_col)
                                    .hover(move |s| {
                                        s.bg(gpui::white().opacity(0.08)).text_color(close_hover)
                                    })
                                    .tooltip(move |_window, cx| {
                                        AnotherOneApp::action_tooltip_view("Close this tab", cx)
                                    })
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, _ev: &MouseDownEvent, _window, cx| {
                                                cx.stop_propagation();
                                                this.close_tab(&sid_close, close_index, cx);
                                            },
                                        ),
                                    )
                                    .child(
                                        svg()
                                            .path("assets/icons/icons__close.svg")
                                            .size(px(12.))
                                            .text_color(close_col),
                                    ),
                            )
                        }),
                );
            }
        }

        tab_bar = tab_bar.child(
            div()
                .id("add-terminal-tab")
                .flex()
                .items_center()
                .justify_center()
                .w(px(28.))
                .h(px(28.))
                .ml(px(4.))
                .rounded(px(5.))
                .cursor_pointer()
                .hover(move |s| s.bg(tab_hover))
                .tooltip(move |_window, cx| {
                    AnotherOneApp::action_tooltip_view("Add an agent tab", cx)
                })
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        this.open_add_agent_modal(&sid_for_add, cx);
                    }),
                )
                .child(
                    svg()
                        .path("assets/icons/icons__plus.svg")
                        .size(px(14.))
                        .text_color(plus_col),
                ),
        );

        let tab_content = self.render_tab_placeholder(section_id);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(terminal_bg)
            .child(tab_bar)
            .child(tab_content)
    }

    fn render_tab_placeholder(&self, section_id: &crate::app::SectionId) -> gpui::Div {
        let terminal_bg = rgb(0x1e1f22);
        let panel_bg = rgb(0x25282d);
        let border = gpui::white().opacity(0.08);
        let title_col = hsla(0., 0., 0.92, 1.);
        let body_col = hsla(0., 0., 0.62, 1.);
        let accent_col = hsla(0.58, 0.62, 0.68, 1.);

        let Some(state) = self.section_states.get(section_id) else {
            return div().flex_1().bg(terminal_bg);
        };
        let Some(tab) = state.tabs.get(state.active_tab) else {
            return div().flex_1().bg(terminal_bg);
        };

        let cwd_label = state
            .cwd
            .as_ref()
            .map(|cwd| cwd.display().to_string())
            .unwrap_or_else(|| "Not available".to_string());
        let task_label = section_id.task_id.as_deref().unwrap_or("Not available");
        let tab_label = if state.tabs.len() > 1 {
            format!("{} {}", tab.title, state.active_tab + 1)
        } else {
            tab.title.clone()
        };

        div()
            .flex_1()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .bg(terminal_bg)
            .child(
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(12.))
                    .w_full()
                    .max_w(px(460.))
                    .p_6()
                    .rounded(px(14.))
                    .bg(panel_bg)
                    .border_1()
                    .border_color(border)
                    .child(
                        svg()
                            .path("assets/icons/icons__terminal.svg")
                            .size(px(20.))
                            .text_color(accent_col),
                    )
                    .child(
                        div()
                            .text_sm()
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(title_col)
                            .child("Terminal Placeholder"),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(body_col)
                            .child(format!(
                                "\"{}\" is showing placeholder content only. You can keep creating and switching tabs here, and wire terminal rendering back in later.",
                                tab_label
                            )),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(body_col)
                            .child(format!("Project: {}", section_id.project_id)),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(body_col)
                            .child(format!("Branch: {}", section_id.branch_name)),
                    )
                    .child(
                        gpui::div()
                            .text_sm()
                            .text_color(body_col)
                            .child(format!("Task: {}", task_label)),
                    )
                    .child(
                        gpui::div()
                            .text_sm()
                            .text_color(body_col)
                            .child(format!("Agent/Tab: {}", tab.id)),
                    )
                    .child(
                        gpui::div()
                            .text_sm()
                            .text_color(body_col)
                            .child(format!("CWD: {}", cwd_label)),
                    ),
            )
    }
}

impl Render for WorkspacePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.section_main_panel(window, cx)
    }
}
