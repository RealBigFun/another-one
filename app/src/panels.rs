//! Reusable panel helper and main content assembly.

use gpui::{
    canvas, div, fill, hsla, outline, point, prelude::*, px, rems, size, svg, App, BorderStyle,
    Bounds, ClipboardItem, Context, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    Pixels, Render, ScrollWheelEvent, SharedString, Window,
};

use crate::agent_icons::branded_icon;
use crate::agents::AGENTS;
use crate::app::{
    terminal_link_ranges, terminal_open_link_modifier_held, AnotherOneApp, TabCloseScope,
    TerminalLinkRange, TerminalMouseAction, TerminalMouseButton, TerminalSelectionRange,
    WorkspaceKeyboardFocus, WorkspacePane,
};
use crate::layout::{TERMINAL_TAB_BAR_H, TERMINAL_VIEW_PADDING};
use crate::terminal_runtime::{
    TerminalCursorKind, TerminalRuntimeKey, TerminalSurfaceSnapshot, TERMINAL_CELL_WIDTH_RATIO,
    TERMINAL_LINE_HEIGHT_RATIO,
};
use crate::theme::{self, AppTheme};

fn tab_icon_element(
    provider: Option<crate::agents::AgentProviderKind>,
    fallback_color: gpui::Hsla,
) -> impl IntoElement {
    provider
        .and_then(|provider| {
            AGENTS
                .iter()
                .find(|agent| agent.provider == Some(provider))
                .map(|agent| branded_icon(agent.icon, 14., Some(fallback_color)))
        })
        .unwrap_or_else(|| {
            svg()
                .path("assets/icons/icons__terminal.svg")
                .size(px(14.))
                .text_color(fallback_color)
                .into_any_element()
        })
}

impl AnotherOneApp {
    pub(crate) fn terminal_tab_menu_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(menu) = self.workspace_pane.read(cx).terminal_tab_menu.clone() else {
            return div().id("terminal-tab-menu-popover");
        };

        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let menu_w = 206.0;
        let menu_h = 176.0;
        let window_w = f32::from(window.bounds().size.width);
        let window_h = f32::from(window.bounds().size.height);
        let left = (menu.anchor_x + 4.0).min((window_w - menu_w - 8.0).max(8.0));
        let top = (menu.anchor_y + 4.0).min((window_h - menu_h - 8.0).max(8.0));
        let (is_pinned, tab_index, tab_count) = {
            let workspace = self.workspace_pane.read(cx);
            let state = workspace.section_states.get(&menu.section_id);
            let tab_index =
                state.and_then(|state| state.tabs.iter().position(|tab| tab.id == menu.tab_id));
            let is_pinned = state
                .and_then(|state| tab_index.and_then(|index| state.tabs.get(index)))
                .is_some_and(|tab| tab.pinned);
            let tab_count = state.map_or(0, |state| state.tabs.len());
            (is_pinned, tab_index, tab_count)
        };
        let label: SharedString = if is_pinned { "Unpin Tab" } else { "Pin Tab" }.into();
        let close_other_enabled = tab_index.is_some() && tab_count > 1;
        let close_left_enabled = tab_index.is_some_and(|index| index > 0);
        let close_right_enabled = tab_index.is_some_and(|index| index + 1 < tab_count);
        let section_id = menu.section_id.clone();
        let tab_id = menu.tab_id.clone();

        let mut items = div()
            .flex()
            .flex_col()
            .py(px(4.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        let rename_section_id = section_id.clone();
        let rename_tab_id = tab_id.clone();
        items = items.child(terminal_context_menu_item(
            "terminal-tab-menu-rename",
            "Rename Tab",
            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
                let section_id = rename_section_id.clone();
                let tab_id = rename_tab_id.clone();
                this.workspace_pane.update(cx, |workspace, cx| {
                    workspace.begin_tab_rename(&section_id, &tab_id, cx);
                });
                cx.stop_propagation();
            }),
            tab_index.is_some(),
            app_theme,
        ));

        let pin_section_id = section_id.clone();
        let pin_tab_id = tab_id.clone();
        items = items.child(terminal_context_menu_item(
            "terminal-tab-menu-pin",
            label,
            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
                let section_id = pin_section_id.clone();
                let tab_id = pin_tab_id.clone();
                this.workspace_pane.update(cx, |workspace, cx| {
                    workspace.terminal_tab_menu = None;
                    workspace.toggle_tab_pinned(&section_id, &tab_id, cx);
                });
                cx.stop_propagation();
            }),
            tab_index.is_some(),
            app_theme,
        ));
        items = items.child(div().h(px(1.)).mx(px(8.)).my(px(3.)).bg(app_theme.divider));

        let close_other_section_id = section_id.clone();
        let close_other_tab_id = tab_id.clone();
        items = items.child(terminal_context_menu_item(
            "terminal-tab-menu-close-others",
            "Clear Other Tabs",
            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
                let section_id = close_other_section_id.clone();
                let tab_id = close_other_tab_id.clone();
                this.workspace_pane.update(cx, |workspace, cx| {
                    workspace.terminal_tab_menu = None;
                    workspace.request_close_tabs_for_scope(
                        &section_id,
                        &tab_id,
                        TabCloseScope::Other,
                        cx,
                    );
                });
                cx.stop_propagation();
            }),
            close_other_enabled,
            app_theme,
        ));

        let close_right_section_id = section_id.clone();
        let close_right_tab_id = tab_id.clone();
        items = items.child(terminal_context_menu_item(
            "terminal-tab-menu-close-right",
            "Close Tabs to the Right",
            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
                let section_id = close_right_section_id.clone();
                let tab_id = close_right_tab_id.clone();
                this.workspace_pane.update(cx, |workspace, cx| {
                    workspace.terminal_tab_menu = None;
                    workspace.request_close_tabs_for_scope(
                        &section_id,
                        &tab_id,
                        TabCloseScope::Right,
                        cx,
                    );
                });
                cx.stop_propagation();
            }),
            close_right_enabled,
            app_theme,
        ));

        let close_left_section_id = section_id.clone();
        let close_left_tab_id = tab_id.clone();
        items = items.child(terminal_context_menu_item(
            "terminal-tab-menu-close-left",
            "Close Tabs to the Left",
            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                this.focus_handle.focus(window, cx);
                let section_id = close_left_section_id.clone();
                let tab_id = close_left_tab_id.clone();
                this.workspace_pane.update(cx, |workspace, cx| {
                    workspace.terminal_tab_menu = None;
                    workspace.request_close_tabs_for_scope(
                        &section_id,
                        &tab_id,
                        TabCloseScope::Left,
                        cx,
                    );
                });
                cx.stop_propagation();
            }),
            close_left_enabled,
            app_theme,
        ));

        div()
            .id("terminal-tab-menu-popover")
            .absolute()
            .left(px(left.max(8.0)))
            .top(px(top.max(8.0)))
            .on_mouse_down_out(cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                this.workspace_pane.update(cx, |workspace, cx| {
                    workspace.terminal_tab_menu = None;
                    cx.notify();
                });
            }))
            .child(
                div()
                    .w(px(menu_w))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(app_theme.border)
                    .bg(app_theme.card_bg)
                    .shadow_lg()
                    .overflow_hidden()
                    .child(items),
            )
    }

    /// Top-anchored search bar shown over the active terminal pane
    /// when `Cmd-F` is pressed. Non-IME — captures plain ASCII and
    /// Cmd-V paste through `handle_terminal_search_key_down`.
    pub(crate) fn terminal_search_bar_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(state) = self.terminal_search.clone() else {
            return div().id("terminal-search-bar");
        };

        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let count_label = if state.matches.is_empty() {
            if state.query.is_empty() {
                "".to_string()
            } else {
                "0/0".to_string()
            }
        } else {
            format!("{}/{}", state.current_index + 1, state.matches.len())
        };
        let query_text = state.query.clone();
        let placeholder = if query_text.is_empty() {
            Some("Search scrollback…")
        } else {
            None
        };

        div()
            .id("terminal-search-bar")
            .absolute()
            .top(px(12.))
            .right(px(20.))
            .flex()
            .items_center()
            .gap(px(8.))
            .h(px(34.))
            .px(px(10.))
            .rounded(px(8.))
            .border_1()
            .border_color(app_theme.border)
            .bg(app_theme.card_bg)
            .shadow_lg()
            .child(
                div()
                    .min_w(px(220.))
                    .text_sm()
                    .text_color(if query_text.is_empty() {
                        app_theme.text_placeholder
                    } else {
                        app_theme.text_primary
                    })
                    .child(if let Some(text) = placeholder {
                        text.to_string()
                    } else {
                        query_text.clone()
                    }),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(app_theme.text_secondary)
                    .min_w(px(48.))
                    .child(count_label),
            )
            .child(terminal_search_button(
                "terminal-search-prev",
                "↑",
                app_theme,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.terminal_search_advance(false, cx);
                    cx.stop_propagation();
                }),
            ))
            .child(terminal_search_button(
                "terminal-search-next",
                "↓",
                app_theme,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.terminal_search_advance(true, cx);
                    cx.stop_propagation();
                }),
            ))
            .child(terminal_search_button(
                "terminal-search-close",
                "×",
                app_theme,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.close_terminal_search(cx);
                    cx.stop_propagation();
                }),
            ))
    }

    pub(crate) fn terminal_context_menu_overlay(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(menu) = self.workspace_pane.read(cx).terminal_context_menu.clone() else {
            return div().id("terminal-context-menu-popover");
        };

        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let has_link = menu.link.is_some();
        let has_selection = menu.selected_text.is_some();
        let item_count = if has_link { 3 } else { 2 };
        let menu_w = 168.0;
        let menu_h = (item_count as f32) * 32.0 + 8.0;
        let window_w = f32::from(window.bounds().size.width);
        let window_h = f32::from(window.bounds().size.height);
        let left = (menu.anchor_x + 4.0).min((window_w - menu_w - 8.0).max(8.0));
        let top = (menu.anchor_y + 4.0).min((window_h - menu_h - 8.0).max(8.0));

        let mut items = div()
            .flex()
            .flex_col()
            .py(px(4.))
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        if has_link {
            items = items.child(terminal_context_menu_item(
                "terminal-context-menu-open-link",
                "Open Link",
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.terminal_context_menu_open_link(cx);
                    cx.stop_propagation();
                }),
                true,
                app_theme,
            ));
        }

        items = items.child(terminal_context_menu_item(
            "terminal-context-menu-copy",
            "Copy",
            cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                if has_selection {
                    this.terminal_context_menu_copy(cx);
                } else {
                    this.dismiss_terminal_context_menu(cx);
                }
                cx.stop_propagation();
            }),
            has_selection,
            app_theme,
        ));

        items = items.child(terminal_context_menu_item(
            "terminal-context-menu-paste",
            "Paste",
            cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                this.terminal_context_menu_paste(cx);
                cx.stop_propagation();
            }),
            true,
            app_theme,
        ));

        div()
            .id("terminal-context-menu-popover")
            .absolute()
            .left(px(left.max(8.0)))
            .top(px(top.max(8.0)))
            .on_mouse_down_out(cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                this.dismiss_terminal_context_menu(cx);
            }))
            .child(
                div()
                    .w(px(menu_w))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(app_theme.border)
                    .bg(app_theme.card_bg)
                    .shadow_lg()
                    .overflow_hidden()
                    .child(items),
            )
    }

    pub(crate) fn pinned_tab_close_confirm_modal(
        &self,
        window: &Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let Some(confirm) = self
            .workspace_pane
            .read(cx)
            .pinned_tab_close_confirm
            .clone()
        else {
            return div().id("pinned-tab-close-confirm-overlay");
        };
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let total_count = confirm.tab_ids.len().max(1);
        let pinned_count = confirm.pinned_tab_count.max(1);
        let title: SharedString = if total_count == 1 {
            "Close pinned tab?".into()
        } else {
            "Close tabs?".into()
        };
        let message: SharedString = if total_count == 1 {
            format!(
                "Close pinned tab \"{}\"? It will be removed from this task.",
                confirm.title
            )
            .into()
        } else if pinned_count == 1 {
            format!(
                "Close {total_count} tabs? This includes pinned tab \"{}\". They will be removed from this task.",
                confirm.title
            )
            .into()
        } else {
            format!(
                "Close {total_count} tabs? This includes {pinned_count} pinned tabs. They will be removed from this task."
            )
            .into()
        };
        let confirm_label: SharedString = if total_count == 1 {
            "Close".into()
        } else {
            "Close Tabs".into()
        };

        div()
            .id("pinned-tab-close-confirm-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(app_theme.scrim_bg)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.workspace_pane.update(cx, |workspace, cx| {
                        workspace.pinned_tab_close_confirm = None;
                        cx.notify();
                    });
                    cx.stop_propagation();
                }),
            )
            .on_key_down(cx.listener(|this, ev: &gpui::KeyDownEvent, _window, cx| {
                if ev.keystroke.key.as_str() == "escape" {
                    this.workspace_pane.update(cx, |workspace, cx| {
                        workspace.pinned_tab_close_confirm = None;
                        cx.notify();
                    });
                    cx.stop_propagation();
                }
            }))
            .child(
                div()
                    .w(px(364.))
                    .rounded_lg()
                    .bg(app_theme.card_bg)
                    .border_1()
                    .border_color(app_theme.border)
                    .shadow_lg()
                    .overflow_hidden()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(8.))
                            .px(px(20.))
                            .pt(px(20.))
                            .pb(px(14.))
                            .child(
                                div()
                                    .text_lg()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(app_theme.text_primary)
                                    .child(title),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(app_theme.text_secondary)
                                    .child(message),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .justify_end()
                            .gap(px(8.))
                            .px(px(20.))
                            .pb(px(18.))
                            .child(
                                div()
                                    .id("pinned-tab-close-cancel")
                                    .px(px(12.))
                                    .h(px(30.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .bg(app_theme.overlay_rest)
                                    .hover(move |s| s.bg(app_theme.overlay_hover_strong))
                                    .text_sm()
                                    .text_color(app_theme.text_primary)
                                    .child("Cancel")
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            this.workspace_pane.update(cx, |workspace, cx| {
                                                workspace.pinned_tab_close_confirm = None;
                                                cx.notify();
                                            });
                                            cx.stop_propagation();
                                        }),
                                    ),
                            )
                            .child(
                                div()
                                    .id("pinned-tab-close-confirm")
                                    .px(px(12.))
                                    .h(px(30.))
                                    .flex()
                                    .items_center()
                                    .rounded(px(6.))
                                    .cursor_pointer()
                                    .bg(hsla(0., 0.62, 0.50, 1.))
                                    .hover(|s| s.bg(hsla(0., 0.62, 0.58, 1.)))
                                    .text_sm()
                                    .text_color(hsla(0., 0., 1., 1.))
                                    .child(confirm_label)
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            this.workspace_pane.update(cx, |workspace, cx| {
                                                workspace.confirm_close_pinned_tab(cx);
                                            });
                                            cx.stop_propagation();
                                        }),
                                    ),
                            ),
                    ),
            )
    }

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
    fn panel_theme(&self, window: &Window, cx: &mut Context<Self>) -> AppTheme {
        self.app
            .upgrade()
            .map(|entity| theme::app_theme(window, entity.read(cx).project_store.ui.theme_mode))
            .unwrap_or_else(theme::dark_theme)
    }

    fn section_main_panel(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        if self.active_git_diff.is_some() {
            return self.render_git_diff_pane(window, cx);
        }

        if let Some(ref project_id) = self.active_project_page.clone() {
            return self.render_project_page(project_id, window, cx);
        }

        let app_theme = self.panel_theme(window, cx);

        let Some(ref section_id) = self.active_section.clone() else {
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
                        .child("Select a branch to get started"),
                );
        };

        let tab_bar_bg = app_theme.chrome_bg;
        let tab_bg_active = app_theme.terminal_bg;
        let tab_bg_inactive = app_theme.card_bg;
        let tab_text_active = app_theme.text_primary;
        let tab_text_inactive = app_theme.text_muted;
        let tab_hover = app_theme.overlay_hover;
        let close_col = app_theme.text_placeholder;
        let close_hover = app_theme.text_secondary;
        let border_col = app_theme.divider;
        let plus_col = app_theme.text_muted;
        let terminal_bg = app_theme.terminal_bg;

        let sid_for_add = section_id.clone();

        let tab_bar = div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(TERMINAL_TAB_BAR_H))
            .bg(tab_bar_bg)
            .border_b_1()
            .border_color(border_col)
            .overflow_hidden();
        let mut tab_strip = div()
            .id("terminal-tab-strip")
            .flex()
            .flex_row()
            .items_center()
            .h_full()
            // Size to the tabs when they fit so the add button hugs the
            // latest tab, but still shrink before the add button when the
            // tab list overflows.
            .flex_shrink()
            .min_w_0()
            .overflow_scroll()
            .overflow_y_hidden();

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
                let sid_menu = section_id.clone();
                let close_index = i;
                let tab_id_val = tab.id.clone();
                let tab_id_for_menu = tab.id.clone();
                let is_pinned = tab.pinned;
                let rename = self
                    .terminal_tab_rename
                    .as_ref()
                    .filter(|rename| rename.section_id == *section_id && rename.tab_id == tab.id);

                tab_strip = tab_strip.child(
                    div()
                        .id(SharedString::from(format!("tab-{}", tab_id_val)))
                        .flex()
                        .flex_none()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .h_full()
                        .px(px(12.))
                        .whitespace_nowrap()
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
                            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                                this.keyboard_focus = WorkspaceKeyboardFocus::MainPane;
                                this.focus_handle.focus(window, cx);
                                this.activate_tab(&sid_click, tab_index, cx);
                            }),
                        )
                        .on_mouse_down(
                            MouseButton::Right,
                            cx.listener(move |this, ev: &MouseDownEvent, _window, cx| {
                                this.terminal_context_menu = None;
                                this.terminal_tab_menu = Some(crate::app::TerminalTabMenuState {
                                    section_id: sid_menu.clone(),
                                    tab_id: tab_id_for_menu.clone(),
                                    anchor_x: f32::from(ev.position.x),
                                    anchor_y: f32::from(ev.position.y),
                                });
                                cx.stop_propagation();
                                cx.notify();
                            }),
                        )
                        .when(is_pinned, |tab| {
                            tab.child(
                                svg()
                                    .path("assets/icons/icons__pin-off.svg")
                                    .size(px(12.))
                                    .text_color(if is_active {
                                        tab_text_active
                                    } else {
                                        tab_text_inactive
                                    }),
                            )
                        })
                        .child(tab_icon_element(
                            tab.launch_config.provider,
                            if is_active {
                                tab_text_active
                            } else {
                                tab_text_inactive
                            },
                        ))
                        .child(if let Some(rename) = rename {
                            let before: SharedString =
                                rename.draft[..rename.cursor].to_string().into();
                            let after: SharedString =
                                rename.draft[rename.cursor..].to_string().into();
                            div()
                                .flex()
                                .items_center()
                                .min_w(px(90.))
                                .max_w(px(220.))
                                .px(px(5.))
                                .py(px(2.))
                                .rounded(px(4.))
                                .bg(app_theme.overlay_active)
                                .border_1()
                                .border_color(app_theme.focus_ring)
                                .text_sm()
                                .text_color(tab_text_active)
                                .child(before)
                                .child(div().w(px(1.)).h(px(14.)).bg(tab_text_active))
                                .child(after)
                                .into_any_element()
                        } else {
                            div()
                                .text_sm()
                                .text_color(if is_active {
                                    tab_text_active
                                } else {
                                    tab_text_inactive
                                })
                                .child(tab_title)
                                .into_any_element()
                        })
                        .child(
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
                                    s.bg(app_theme.overlay_hover).text_color(close_hover)
                                })
                                .tooltip(move |_window, cx| {
                                    AnotherOneApp::action_tooltip_view("Close this tab", cx)
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                        cx.stop_propagation();
                                        this.request_close_tab(&sid_close, close_index, cx);
                                    }),
                                )
                                .child(
                                    svg()
                                        .path("assets/icons/icons__close.svg")
                                        .size(px(12.))
                                        .text_color(close_col),
                                ),
                        ),
                );
            }
        }

        let tab_bar = tab_bar.child(tab_strip).child(
            div()
                .id("add-terminal-tab")
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .w(px(28.))
                .h(px(28.))
                .ml(px(4.))
                .mr(px(4.))
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

        let tab_content = self.render_terminal_tab(section_id, window, cx);

        div()
            .flex()
            .flex_col()
            .size_full()
            .bg(terminal_bg)
            .child(tab_bar)
            .child(tab_content)
    }

    fn render_terminal_tab(
        &self,
        section_id: &crate::app::SectionId,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::AnyElement {
        let app_theme = self.panel_theme(window, cx);
        let terminal_bg = app_theme.terminal_bg;
        let panel_bg = app_theme.card_bg;
        let border = app_theme.border;
        let title_col = app_theme.text_primary;
        let body_col = app_theme.text_secondary;
        let accent_col = app_theme.info.text;

        let Some(state) = self.section_states.get(section_id) else {
            return div().flex_1().bg(terminal_bg).into_any_element();
        };
        let Some(tab) = state.tabs.get(state.active_tab) else {
            let task_label = section_id.task_id.as_deref().unwrap_or("Not available");
            let cwd_label = state
                .cwd
                .as_ref()
                .map(|cwd| cwd.display().to_string())
                .unwrap_or_else(|| "Not available".to_string());
            return div()
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
                                .child("No active tabs"),
                        )
                        .child(div().text_sm().text_color(body_col).child(
                            "This task has no open tabs. Add an agent tab to start working.",
                        ))
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
                                .child(format!("CWD: {}", cwd_label)),
                        ),
                )
                .into_any_element();
        };

        let key = TerminalRuntimeKey {
            section_id: section_id.clone(),
            tab_id: tab.id.clone(),
        };
        let (snapshot, pending, error) = self
            .app
            .upgrade()
            .map(|app_entity| {
                let app = app_entity.read(cx);
                (
                    app.terminal_snapshot_for(&key),
                    app.terminal_is_pending(&key),
                    app.terminal_error_for(&key).map(str::to_string),
                )
            })
            .unwrap_or((None, false, None));

        if let Some(snapshot) = snapshot {
            let line_height = px((self.font_size * TERMINAL_LINE_HEIGHT_RATIO).max(14.0));
            let cell_width = terminal_cell_width(window, self.font_size);
            let padding = px(TERMINAL_VIEW_PADDING);
            let canvas_snapshot = snapshot.clone();
            let selection_key = key.clone();
            let scroll_key = key.clone();
            let mouse_up_key = key.clone();
            let mouse_move_key = key.clone();
            let mouse_right_key = key.clone();
            let mouse_middle_key = key.clone();
            let selection = self
                .app
                .upgrade()
                .and_then(|app_entity| app_entity.read(cx).terminal_selection_for(&key));
            let search_highlights = self
                .app
                .upgrade()
                .map(|app_entity| {
                    app_entity
                        .read(cx)
                        .terminal_search_viewport_highlights(&key)
                })
                .unwrap_or_default();
            let bell_intensity = self
                .app
                .upgrade()
                .map(|app_entity| app_entity.read(cx).terminal_bell_intensity(&key))
                .unwrap_or(0.0);
            let font_size = px(self.font_size);
            // Swap cursor to a pointer when the user is hovering a
            // link AND the open-link modifier is held — matches the
            // visual the underline already shows. Without the
            // modifier, the underline is just an affordance and the
            // cursor stays on text-select.
            let mods = window.modifiers();
            let modifier_held = terminal_open_link_modifier_held(mods);
            let hovering_link = self
                .terminal_link_hover
                .as_ref()
                .is_some_and(|h| &h.section_id == section_id && h.tab_id == tab.id);
            let pane_section_id = section_id.clone();
            let pane_tab_id = tab.id.clone();
            let mut pane_div = div()
                .id(SharedString::from(format!(
                    "terminal-pane-{}-{}",
                    section_id.store_key(),
                    tab.id
                )))
                .relative()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .bg(terminal_bg)
                // Clear the per-tab hover state the moment the
                // cursor leaves this pane so the underline +
                // tooltip don't linger after the mouse moves
                // somewhere else in the window.
                .on_hover(cx.listener(move |this, hovered: &bool, _window, cx| {
                    if *hovered {
                        return;
                    }
                    let should_clear = this.terminal_link_hover.as_ref().is_some_and(|h| {
                        h.section_id == pane_section_id && h.tab_id == pane_tab_id
                    });
                    if should_clear {
                        this.terminal_link_hover = None;
                        cx.notify();
                    }
                }))
                // Pressing/releasing Cmd or Ctrl while the cursor is
                // sitting still over a link must refresh the cursor
                // swap and tooltip without requiring mouse motion.
                .on_modifiers_changed(cx.listener(|_this, _ev, _window, cx| {
                    cx.notify();
                }));
            if hovering_link && modifier_held {
                pane_div = pane_div.cursor_pointer();
            } else {
                pane_div = pane_div.cursor_text();
            }
            return pane_div
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        this.keyboard_focus = WorkspaceKeyboardFocus::MainPane;
                        this.focus_handle.focus(window, cx);
                        // Raise the Android soft keyboard on tap.
                        // The terminal surface isn't a GPUI
                        // `TextInput`, so the IME wouldn't come up
                        // from the usual focus-driven path.
                        // No-op on desktop.
                        crate::mobile::show_soft_keyboard();
                        let _ = this.app.update(cx, |app, app_cx| {
                            if app.open_terminal_link_at_click(&selection_key, ev, window, app_cx) {
                                app_cx.stop_propagation();
                                return;
                            }
                            if app.forward_terminal_mouse_event(
                                &selection_key,
                                TerminalMouseButton::Left,
                                TerminalMouseAction::Press,
                                ev.position,
                                ev.modifiers,
                                window,
                            ) {
                                app_cx.stop_propagation();
                                return;
                            }
                            app.start_terminal_selection(selection_key.clone(), ev, window, app_cx);
                        });
                    }),
                )
                .on_mouse_down(
                    MouseButton::Middle,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        let _ = this.app.update(cx, |app, app_cx| {
                            if app.forward_terminal_mouse_event(
                                &mouse_middle_key,
                                TerminalMouseButton::Middle,
                                TerminalMouseAction::Press,
                                ev.position,
                                ev.modifiers,
                                window,
                            ) {
                                app_cx.stop_propagation();
                            }
                        });
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        let _ = this.app.update(cx, |app, app_cx| {
                            if app.forward_terminal_mouse_event(
                                &mouse_right_key,
                                TerminalMouseButton::Right,
                                TerminalMouseAction::Press,
                                ev.position,
                                ev.modifiers,
                                window,
                            ) {
                                app_cx.stop_propagation();
                                return;
                            }
                            app.open_terminal_context_menu(
                                &mouse_right_key,
                                ev.position,
                                window,
                                app_cx,
                            );
                            app_cx.stop_propagation();
                        });
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseUpEvent, window, cx| {
                        let _ = this.app.update(cx, |app, app_cx| {
                            if app.forward_terminal_mouse_event(
                                &mouse_up_key,
                                TerminalMouseButton::Left,
                                TerminalMouseAction::Release,
                                ev.position,
                                ev.modifiers,
                                window,
                            ) {
                                app_cx.stop_propagation();
                            }
                        });
                    }),
                )
                .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, window, cx| {
                    let forwarded = this
                        .app
                        .update(cx, |app, app_cx| {
                            let action = if ev.dragging() {
                                TerminalMouseAction::Drag
                            } else {
                                TerminalMouseAction::Motion
                            };
                            let button = if ev.dragging() {
                                match ev.pressed_button {
                                    Some(MouseButton::Left) => TerminalMouseButton::Left,
                                    Some(MouseButton::Middle) => TerminalMouseButton::Middle,
                                    Some(MouseButton::Right) => TerminalMouseButton::Right,
                                    _ => TerminalMouseButton::Left,
                                }
                            } else {
                                TerminalMouseButton::None
                            };
                            if app.forward_terminal_mouse_event(
                                &mouse_move_key,
                                button,
                                action,
                                ev.position,
                                ev.modifiers,
                                window,
                            ) {
                                app_cx.stop_propagation();
                                return true;
                            }
                            false
                        })
                        .unwrap_or(false);
                    if forwarded {
                        return;
                    }
                    // Mouse mode is off: refresh link-hover state.
                    // Compute via the app (read-only) and apply
                    // directly to `this` to avoid the nested-
                    // update panic of touching WorkspacePane
                    // through `app.update` while WorkspacePane is
                    // already locked by the listener.
                    let next = this
                        .app
                        .update(cx, |app, _| {
                            app.compute_terminal_link_hover(&mouse_move_key, ev.position, window)
                        })
                        .unwrap_or(None);
                    if this.terminal_link_hover != next {
                        this.terminal_link_hover = next;
                        cx.notify();
                    }
                }))
                .on_scroll_wheel(cx.listener(move |this, ev: &ScrollWheelEvent, window, cx| {
                    let _ = this.app.update(cx, |app, app_cx| {
                        let pixel_delta = ev.delta.pixel_delta(px(1.));
                        let dy = f32::from(pixel_delta.y);
                        let dx = f32::from(pixel_delta.x);
                        // Vertical wheel maps to 64 (up) / 65 (down);
                        // horizontal to 66 (left) / 67 (right) per
                        // xterm. Pick the dominant axis for a single
                        // forwarded event — apps that care about both
                        // axes get them as separate scroll ticks.
                        let dominant_button = if dy.abs() >= dx.abs() && dy != 0.0 {
                            Some(if dy > 0.0 {
                                TerminalMouseButton::WheelUp
                            } else {
                                TerminalMouseButton::WheelDown
                            })
                        } else if dx != 0.0 {
                            Some(if dx > 0.0 {
                                TerminalMouseButton::WheelRight
                            } else {
                                TerminalMouseButton::WheelLeft
                            })
                        } else {
                            None
                        };
                        if let Some(button) = dominant_button {
                            if app.forward_terminal_mouse_event(
                                &scroll_key,
                                button,
                                TerminalMouseAction::Press,
                                ev.position,
                                ev.modifiers,
                                window,
                            ) {
                                app_cx.stop_propagation();
                                return;
                            }
                        }
                        if app.scroll_terminal(&scroll_key, ev.delta, app_cx) {
                            app_cx.stop_propagation();
                        }
                    });
                }))
                .child(
                    canvas(
                        move |bounds, _, _| bounds,
                        move |bounds, _, window, cx| {
                            // Underline links on plain hover so they're
                            // discoverable without trial-and-error
                            // modifier presses. Cmd/Ctrl is still
                            // required to *open* (handled in
                            // `open_terminal_link_at_click`); the hover
                            // is just a visual affordance.
                            let hovered_link = hovered_terminal_link_range(
                                bounds,
                                &canvas_snapshot,
                                window.mouse_position(),
                                padding,
                                cell_width,
                                line_height,
                            );
                            paint_terminal_snapshot(
                                bounds,
                                &canvas_snapshot,
                                &app_theme,
                                window,
                                cx,
                                padding,
                                cell_width,
                                line_height,
                                font_size,
                                selection,
                                hovered_link,
                                &search_highlights,
                            );
                        },
                    )
                    .absolute()
                    .inset_0(),
                )
                .children((bell_intensity > 0.0).then(|| {
                    div()
                        .absolute()
                        .inset_0()
                        .bg(hsla(0.13, 0.95, 0.65, 0.18 * bell_intensity))
                }))
                .children(self.terminal_link_hover.as_ref().and_then(|hover| {
                    if hover.section_id != key.section_id || hover.tab_id != key.tab_id {
                        return None;
                    }
                    // Place the tooltip just below the cursor, clamped
                    // to the window so it doesn't paint off-screen.
                    let tip_left = (hover.anchor_x + 12).max(8) as f32;
                    let tip_top = (hover.anchor_y + 18).max(8) as f32;
                    let url_preview: String = if hover.link.len() > 80 {
                        format!("{}…", &hover.link[..80])
                    } else {
                        hover.link.clone()
                    };
                    let modifier_label = if cfg!(target_os = "macos") {
                        "⌘+click"
                    } else {
                        "Ctrl+click"
                    };
                    Some(
                        div()
                            .absolute()
                            .left(px(tip_left))
                            .top(px(tip_top))
                            .max_w(px(420.))
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(6.))
                            .border_1()
                            .border_color(app_theme.border)
                            .bg(app_theme.card_bg)
                            .shadow_lg()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(app_theme.text_secondary)
                                    .child(format!("{modifier_label} to open")),
                            )
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(app_theme.text_primary)
                                    .child(url_preview),
                            ),
                    )
                }))
                .into_any_element();
        }

        // Error path first — early-return the error UI so the
        // fallthrough below can stay focused on the
        // NotStarted/Launching happy path.
        if let Some(error) = error.as_deref() {
            let error_copy = error.to_string();
            return div()
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
                        .gap(px(12.))
                        .w_full()
                        .max_w(px(720.))
                        .max_h(px(520.))
                        .p_6()
                        .rounded(px(14.))
                        .bg(panel_bg)
                        .border_1()
                        .border_color(border)
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(12.))
                                .w_full()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::SEMIBOLD)
                                        .text_color(title_col)
                                        .child("Terminal launch failed"),
                                )
                                .child(
                                    div()
                                        .id("terminal-error-copy")
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flex_shrink_0()
                                        .w(px(28.))
                                        .h(px(28.))
                                        .rounded(px(7.))
                                        .cursor_pointer()
                                        .hover(move |style| style.bg(app_theme.overlay_hover))
                                        .tooltip(move |_window, cx| {
                                            AnotherOneApp::action_tooltip_view(
                                                "Copy error details",
                                                cx,
                                            )
                                        })
                                        .on_mouse_down(
                                            MouseButton::Left,
                                            cx.listener(
                                                move |this, _ev: &MouseDownEvent, _window, cx| {
                                                    cx.write_to_clipboard(
                                                        ClipboardItem::new_string(
                                                            error_copy.clone(),
                                                        ),
                                                    );
                                                    let _ = this.app.update(cx, |app, app_cx| {
                                                        app.show_info_toast(
                                                            "Copied terminal error details.",
                                                            app_cx,
                                                        );
                                                    });
                                                    cx.stop_propagation();
                                                },
                                            ),
                                        )
                                        .child(
                                            svg()
                                                .path("assets/icons/icons__copy.svg")
                                                .size(px(15.))
                                                .text_color(body_col),
                                        ),
                                ),
                        )
                        .child(terminal_error_details(error.to_string(), body_col, app_theme)),
                )
                .into_any_element();
        }

        // Non-error happy path: the tab was just created (or
        // restored) and its PTY is about to spawn / already
        // spawning. Show a consistent "Launching" status until
        // `ensure_active_terminal_runtime` produces a runtime and
        // `terminal_surface_snapshots` picks up the live grid.
        let status_title = "Launching terminal";
        let status_body =
            "The tab was created immediately and its PTY is launching in the background.";

        let cwd_label = state
            .cwd
            .as_ref()
            .map(|cwd| cwd.display().to_string())
            .unwrap_or_else(|| "Not available".to_string());
        let task_label = section_id.task_id.as_deref().unwrap_or("Not available");
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
                            .child(status_title),
                    )
                    .child(div().text_sm().text_color(body_col).child(status_body))
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
            .into_any_element()
    }
}

fn terminal_error_details(
    error: String,
    body_col: gpui::Hsla,
    app_theme: AppTheme,
) -> impl IntoElement {
    let mut details = div()
        .id("terminal-error-details")
        .w_full()
        .max_h(px(420.))
        .overflow_scroll()
        .rounded(px(8.))
        .border_1()
        .border_color(app_theme.border)
        .bg(app_theme.sunken_bg)
        .px(px(12.))
        .py(px(10.))
        .text_size(rems(12. / 16.))
        .line_height(rems(18. / 16.))
        .font_family("Lilex Nerd Font Mono")
        .text_color(body_col);

    for line in error.lines() {
        details = details.child(
            div()
                .min_w_0()
                .whitespace_nowrap()
                .child(if line.is_empty() {
                    " ".to_string()
                } else {
                    line.to_string()
                }),
        );
    }

    details
}

fn terminal_search_button<F>(
    id: &'static str,
    label: &'static str,
    app_theme: AppTheme,
    on_click: F,
) -> impl IntoElement
where
    F: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
{
    div()
        .id(id)
        .flex()
        .items_center()
        .justify_center()
        .w(px(24.))
        .h(px(24.))
        .rounded(px(4.))
        .text_sm()
        .text_color(app_theme.text_secondary)
        .cursor_pointer()
        .hover(move |hover| hover.bg(app_theme.overlay_hover))
        .on_mouse_down(MouseButton::Left, on_click)
        .child(label)
}

fn terminal_context_menu_item<F>(
    id: &'static str,
    label: impl Into<SharedString>,
    on_click: F,
    enabled: bool,
    app_theme: AppTheme,
) -> impl IntoElement
where
    F: Fn(&MouseDownEvent, &mut Window, &mut App) + 'static,
{
    let mut item = div()
        .id(id)
        .flex()
        .items_center()
        .h(px(30.))
        .px(px(14.))
        .text_sm()
        .font_weight(gpui::FontWeight::MEDIUM)
        .child(label.into());

    if enabled {
        item = item
            .text_color(app_theme.text_primary)
            .cursor_pointer()
            .hover(move |hover| hover.bg(app_theme.overlay_hover));
    } else {
        item = item.text_color(app_theme.text_placeholder);
    }
    // Always attach the listener: when disabled, the closure dismisses the
    // menu instead of acting. Without this the outer container's
    // `stop_propagation` would swallow the click and leave the menu open.
    item.on_mouse_down(MouseButton::Left, on_click)
}

fn paint_terminal_snapshot(
    bounds: Bounds<Pixels>,
    snapshot: &TerminalSurfaceSnapshot,
    _app_theme: &AppTheme,
    window: &mut Window,
    cx: &mut App,
    padding: Pixels,
    cell_width: Pixels,
    cell_height: Pixels,
    font_size: Pixels,
    selection: Option<TerminalSelectionRange>,
    hovered_link: Option<TerminalLinkRange>,
    search_highlights: &[(usize, usize, usize, bool)],
) {
    for (line_index, line) in snapshot.lines.iter().enumerate() {
        let top = bounds.origin.y + padding + cell_height * line_index as f32;

        for span in &line.background_spans {
            let left = bounds.origin.x + padding + cell_width * span.column as f32;
            window.paint_quad(fill(
                Bounds::new(
                    point(left, top),
                    size(cell_width * span.width as f32, cell_height),
                ),
                span.color,
            ));
        }
    }

    // Search highlights paint above cell backgrounds (so vim/htop's
    // colored cells don't obscure them) but underneath text glyphs.
    for &(line, start_col, end_col, is_current) in search_highlights {
        let top = bounds.origin.y + padding + cell_height * line as f32;
        let left = bounds.origin.x + padding + cell_width * start_col as f32;
        let width = cell_width * (end_col.saturating_sub(start_col)) as f32;
        let bg = if is_current {
            hsla(0.13, 0.85, 0.55, 0.85)
        } else {
            hsla(0.13, 0.5, 0.45, 0.55)
        };
        window.paint_quad(fill(
            Bounds::new(point(left, top), size(width, cell_height)),
            bg,
        ));
    }

    if let Some(selection) = selection {
        paint_terminal_selection(
            bounds,
            snapshot,
            window,
            padding,
            cell_width,
            cell_height,
            selection,
        );
    }

    for run in &snapshot.positioned_runs {
        let position = point(
            bounds.origin.x + padding + cell_width * run.column as f32,
            bounds.origin.y + padding + cell_height * run.line as f32,
        );
        let _ = window
            .text_system()
            .shape_line(
                run.text.clone().into(),
                font_size,
                &[run.style.clone()],
                Some(cell_width),
            )
            .paint(
                position,
                cell_height,
                gpui::TextAlign::Left,
                None,
                window,
                cx,
            );
    }

    if let Some(range) = hovered_link {
        paint_terminal_link_underline(bounds, window, padding, cell_width, cell_height, range);
    }

    let Some(cursor) = &snapshot.cursor else {
        return;
    };

    let left = bounds.origin.x + padding + cell_width * cursor.column as f32;
    let top = bounds.origin.y + padding + cell_height * cursor.line as f32;
    let width = cell_width * cursor.width as f32;
    let rect = Bounds::new(point(left, top), size(width, cell_height));

    let color = if cursor.blinking {
        let mut faded = cursor.color;
        faded.a *= cursor_blink_opacity();
        faded
    } else {
        cursor.color
    };

    match cursor.kind {
        TerminalCursorKind::Block => window.paint_quad(fill(rect, color)),
        TerminalCursorKind::HollowBlock => {
            window.paint_quad(outline(rect, color, BorderStyle::default()));
        }
        TerminalCursorKind::Beam => {
            window.paint_quad(fill(
                Bounds::new(point(left, top), size(px(2.), cell_height)),
                color,
            ));
        }
        TerminalCursorKind::Underline => {
            window.paint_quad(fill(
                Bounds::new(
                    point(left, top + cell_height - px(2.)),
                    size(width.max(px(1.)), px(2.)),
                ),
                color,
            ));
        }
    }
}

/// Cursor blink phase modulator: a square wave with a 1000 ms period
/// (500 ms on, 500 ms off) measured against process start. Returns 1.0
/// or 0.25 — we keep a sliver of opacity even on the "off" phase so
/// users see a clear "this cell still holds the cursor" hint.
pub(crate) fn cursor_blink_opacity() -> f32 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static EPOCH: OnceLock<Instant> = OnceLock::new();
    let epoch = EPOCH.get_or_init(Instant::now);
    let phase_ms = epoch.elapsed().as_millis() % 1000;
    if phase_ms < 500 {
        1.0
    } else {
        0.25
    }
}

fn hovered_terminal_link_range(
    bounds: Bounds<Pixels>,
    snapshot: &TerminalSurfaceSnapshot,
    mouse_position: gpui::Point<Pixels>,
    padding: Pixels,
    cell_width: Pixels,
    cell_height: Pixels,
) -> Option<TerminalLinkRange> {
    if snapshot.columns == 0 || snapshot.lines.is_empty() {
        return None;
    }

    let x = f32::from(mouse_position.x) - f32::from(bounds.origin.x) - f32::from(padding);
    let y = f32::from(mouse_position.y) - f32::from(bounds.origin.y) - f32::from(padding);
    if x < 0.0 || y < 0.0 {
        return None;
    }

    let column = (x / f32::from(cell_width)).floor() as usize;
    let line = (y / f32::from(cell_height)).floor() as usize;
    if line >= snapshot.lines.len() || column >= snapshot.columns {
        return None;
    }

    terminal_link_ranges(snapshot).into_iter().find(|range| {
        range.line == line && range.start_column <= column && column < range.end_column
    })
}

fn paint_terminal_link_underline(
    bounds: Bounds<Pixels>,
    window: &mut Window,
    padding: Pixels,
    cell_width: Pixels,
    cell_height: Pixels,
    range: TerminalLinkRange,
) {
    if range.start_column >= range.end_column {
        return;
    }

    let color = hsla(0.58, 0.72, 0.72, 0.9);
    let thickness = px(1.);
    let left = bounds.origin.x + padding + cell_width * range.start_column as f32;
    let top = bounds.origin.y + padding + cell_height * range.line as f32;
    let width = cell_width * (range.end_column - range.start_column) as f32;
    window.paint_quad(fill(
        Bounds::new(
            point(left, top + cell_height - px(2.)),
            size(width, thickness),
        ),
        color,
    ));
}

fn paint_terminal_selection(
    bounds: Bounds<Pixels>,
    snapshot: &TerminalSurfaceSnapshot,
    window: &mut Window,
    padding: Pixels,
    cell_width: Pixels,
    cell_height: Pixels,
    selection: TerminalSelectionRange,
) {
    let highlight = hsla(0.58, 0.62, 0.68, 0.35);
    let last_line = snapshot.lines.len().saturating_sub(1);
    if snapshot.columns == 0 || snapshot.lines.is_empty() {
        return;
    }

    let start_line = selection.start_line.min(last_line);
    let end_line = selection.end_line.min(last_line);
    for line in start_line..=end_line {
        let start_column = if line == start_line {
            selection
                .start_column
                .min(snapshot.columns.saturating_sub(1))
        } else {
            0
        };
        let end_column = if line == end_line {
            selection.end_column.min(snapshot.columns.saturating_sub(1))
        } else {
            snapshot.columns.saturating_sub(1)
        };
        if end_column < start_column {
            continue;
        }

        let left = bounds.origin.x + padding + cell_width * start_column as f32;
        let top = bounds.origin.y + padding + cell_height * line as f32;
        let width = cell_width * (end_column + 1 - start_column) as f32;
        window.paint_quad(fill(
            Bounds::new(point(left, top), size(width.max(px(1.)), cell_height)),
            highlight,
        ));
    }
}

pub(crate) fn terminal_cell_width(window: &mut Window, font_size: f32) -> Pixels {
    let font_pixels = px(font_size);
    let text_system = window.text_system();
    let font_id = text_system.resolve_font(&gpui::font("Lilex Nerd Font Mono"));

    text_system
        .advance(font_id, font_pixels, 'w')
        .map(|advance| advance.width.max(px(7.)))
        .unwrap_or_else(|_| px((font_size * TERMINAL_CELL_WIDTH_RATIO).max(7.0)))
}

impl Render for WorkspacePane {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.section_main_panel(window, cx)
    }
}
