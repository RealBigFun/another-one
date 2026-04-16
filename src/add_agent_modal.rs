//! "Add Agent" modal dialog shown when clicking the "+" button in the tab bar.

use std::collections::HashSet;

use gpui::{
    div, hsla, prelude::*, px, relative, rems, rgb, svg, Context, KeyDownEvent, MouseButton,
    MouseDownEvent, SharedString,
};

use crate::agents::{terminal_launch_config_for_selected_agents, AGENTS, DEFAULT_AGENT_ID};
use crate::app::{AnotherOneApp, SectionId};

#[derive(Clone)]
pub(crate) struct AddAgentModalState {
    pub section_id: SectionId,
    pub selected_agent_id: String,
    pub agent_dropdown_open: bool,
}

const CARD_BG: u32 = 0x2b2d31;
const TITLE_COL: (f32, f32, f32, f32) = (0., 0., 0.92, 1.);
const BODY_COL: (f32, f32, f32, f32) = (0., 0., 0.78, 1.);
const MUTED_COL: (f32, f32, f32, f32) = (0., 0., 0.58, 1.);

fn title_col() -> gpui::Hsla {
    hsla(TITLE_COL.0, TITLE_COL.1, TITLE_COL.2, TITLE_COL.3)
}

fn body_col() -> gpui::Hsla {
    hsla(BODY_COL.0, BODY_COL.1, BODY_COL.2, BODY_COL.3)
}

fn muted_col() -> gpui::Hsla {
    hsla(MUTED_COL.0, MUTED_COL.1, MUTED_COL.2, MUTED_COL.3)
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

impl AnotherOneApp {
    pub(crate) fn open_add_agent_modal(
        &mut self,
        section_id: SectionId,
        selected_agent_id: String,
    ) {
        let selected_agent_id = if AGENTS.iter().any(|agent| agent.id == selected_agent_id) {
            selected_agent_id
        } else {
            DEFAULT_AGENT_ID.to_string()
        };

        self.add_agent_modal = Some(AddAgentModalState {
            section_id,
            selected_agent_id,
            agent_dropdown_open: false,
        });
    }

    pub(crate) fn add_agent_modal_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.add_agent_modal.clone() else {
            return div().id("add-agent-modal-overlay");
        };

        let selected_agent = AGENTS
            .iter()
            .find(|agent| agent.id == state.selected_agent_id)
            .unwrap_or(&AGENTS[0]);
        let trigger_icon: SharedString = selected_agent.icon.into();
        let trigger_label: SharedString = selected_agent.label.into();

        let card = div()
            .w(px(440.))
            .max_h(relative(0.9))
            .max_w(relative(0.92))
            .rounded_lg()
            .bg(rgb(CARD_BG))
            .border_1()
            .border_color(border_col())
            .shadow_lg()
            .overflow_hidden()
            .flex()
            .flex_col()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(self.render_add_agent_modal_header(cx))
            .child(
                div()
                    .id("add-agent-modal-scroll")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .flex()
                    .flex_col()
                    .child(
                        div()
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
                                    .child("Agent"),
                            )
                            .child(
                                div()
                                    .relative()
                                    .flex()
                                    .flex_col()
                                    .gap(px(4.))
                                    .child(
                                        div()
                                            .id("add-agent-trigger")
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
                                            .hover(move |s| s.bg(hover_bg()))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    |this, _ev: &MouseDownEvent, _window, cx| {
                                                        if let Some(state) =
                                                            this.add_agent_modal.as_mut()
                                                        {
                                                            state.agent_dropdown_open =
                                                                !state.agent_dropdown_open;
                                                        }
                                                        cx.stop_propagation();
                                                        cx.notify();
                                                    },
                                                ),
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
                                    )
                                    .child(
                                        div()
                                            .text_size(rems(11. / 16.))
                                            .text_color(muted_col())
                                            .child(
                                                "The new tab will open in this task’s existing worktree.",
                                            ),
                                    )
                                    .when(state.agent_dropdown_open, |container| {
                                        container.child(
                                            self.render_add_agent_dropdown(
                                                &state.selected_agent_id,
                                                cx,
                                            ),
                                        )
                                    }),
                            ),
                    ),
            )
            .child(self.render_add_agent_modal_footer(cx));

        div()
            .id("add-agent-modal-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.50))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.dismiss_add_agent_modal(cx);
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                this.handle_add_agent_modal_key_down(ev, cx);
            }))
            .child(card)
    }

    pub(crate) fn handle_add_agent_modal_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        if self.add_agent_modal.is_none() {
            return;
        }

        cx.stop_propagation();

        match ev.keystroke.key.as_str() {
            "escape" => {
                self.dismiss_add_agent_modal(cx);
            }
            "enter" => {
                self.submit_add_agent_modal(cx);
            }
            _ => {}
        }
    }

    fn dismiss_add_agent_modal(&mut self, cx: &mut Context<Self>) {
        self.add_agent_modal = None;
        cx.notify();
    }

    fn submit_add_agent_modal(&mut self, cx: &mut Context<Self>) {
        let Some(state) = self.add_agent_modal.clone() else {
            return;
        };

        if !AGENTS
            .iter()
            .any(|agent| agent.id == state.selected_agent_id)
        {
            self.show_error_toast("Could not determine which agent to launch.", cx);
            return;
        }

        let launch_config = terminal_launch_config_for_selected_agents(&HashSet::from([state
            .selected_agent_id
            .clone()]));
        let section_id = state.section_id.clone();
        let added = self.workspace_pane.update(cx, |workspace, cx| {
            let added =
                workspace.add_tab_with_launch_config(&section_id, launch_config.clone(), cx);
            if added {
                workspace.ensure_active_terminal_spawned(&section_id, cx);
            }
            added
        });

        if !added {
            self.show_error_toast("Could not add an agent tab for this section.", cx);
            return;
        }

        self.add_agent_modal = None;
        cx.notify();
    }

    fn render_add_agent_modal_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
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
                            .child("Add Agent to Task"),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .text_color(muted_col())
                            .child(
                                "Open another agent chat in the same task without changing the worktree.",
                            ),
                    ),
            )
            .child(
                div()
                    .id("add-agent-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(24.))
                    .h(px(24.))
                    .rounded_md()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.dismiss_add_agent_modal(cx);
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

    fn render_add_agent_dropdown(
        &self,
        selected_agent_id: &str,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let visible_rows = AGENTS.len().min(6) as f32;
        let dropdown_height = px(visible_rows * 36. + 8.);

        let mut list = div()
            .id("add-agent-dropdown")
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
            let is_selected = agent.id == selected_agent_id;
            let agent_id = agent.id.to_string();
            let icon_path: SharedString = agent.icon.into();
            let label: SharedString = agent.label.into();

            list = list.child(
                div()
                    .id(SharedString::from(format!("add-agent-option-{}", agent.id)))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .h(px(36.))
                    .px(px(12.))
                    .cursor_pointer()
                    .bg(if is_selected {
                        active_bg()
                    } else {
                        gpui::transparent_black()
                    })
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            if let Some(state) = this.add_agent_modal.as_mut() {
                                state.selected_agent_id = agent_id.clone();
                                state.agent_dropdown_open = false;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        div()
                            .w(px(18.))
                            .h(px(18.))
                            .rounded(px(999.))
                            .border_1()
                            .border_color(if is_selected {
                                hsla(220. / 360., 0.55, 0.58, 1.)
                            } else {
                                border_col()
                            })
                            .bg(if is_selected {
                                hsla(220. / 360., 0.55, 0.58, 1.)
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
                                        .size(px(11.))
                                        .text_color(gpui::white()),
                                )
                            }),
                    )
                    .child(svg().path(icon_path).size(px(18.)).text_color(title_col()))
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(body_col())
                            .child(label),
                    ),
            );
        }

        list
    }

    fn render_add_agent_modal_footer(&self, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_center()
            .flex_row()
            .justify_end()
            .gap(px(10.))
            .px(px(20.))
            .py(px(16.))
            .border_t_1()
            .border_color(gpui::white().opacity(0.06))
            .mt(px(16.))
            .child(
                div()
                    .id("add-agent-cancel")
                    .cursor_pointer()
                    .px(px(14.))
                    .py(px(7.))
                    .rounded_md()
                    .border_1()
                    .border_color(border_col())
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(body_col())
                    .hover(move |s| s.bg(hover_bg()))
                    .child("Cancel")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.dismiss_add_agent_modal(cx);
                            cx.stop_propagation();
                        }),
                    ),
            )
            .child(
                div()
                    .id("add-agent-submit")
                    .cursor_pointer()
                    .px(px(16.))
                    .py(px(7.))
                    .rounded_md()
                    .bg(gpui::white())
                    .hover(move |s| s.bg(hsla(0., 0., 0.90, 1.)))
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(rgb(0x1e1f22))
                    .child("Create")
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.submit_add_agent_modal(cx);
                            cx.stop_propagation();
                        }),
                    ),
            )
    }
}
