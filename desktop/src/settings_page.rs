//! App-level settings page with a sidebar navigation and content area.

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, ClipboardItem, Context, KeyDownEvent, MouseButton,
    MouseDownEvent, Window,
};

use crate::agent_icons::branded_icon;
use crate::agents::AGENTS;
use crate::app::AnotherOneApp;
use crate::layout::TITLEBAR_CHROME_H;
use crate::shortcuts::{
    capture_shortcut, keybinding_token_label, ShortcutAction, ALL_SHORTCUT_ACTIONS,
};

const TEXT_PRIMARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.92, 1.);
const TEXT_SECONDARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.55, 1.);
const BORDER_SUBTLE: fn() -> gpui::Hsla = || gpui::white().opacity(0.08);

const SETTINGS_SIDEBAR_W: f32 = 180.;

/// Which settings section is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Agents,
    OpenIn,
    Keybindings,
}

impl SettingsSection {
    fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::OpenIn => "Open In",
            Self::Keybindings => "Keybindings",
        }
    }
}

impl AnotherOneApp {
    pub(crate) fn handle_settings_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        if self.settings_section == SettingsSection::Agents
            && self.handle_settings_agent_input_key_down(ev, cx)
        {
            return true;
        }

        if self.settings_section != SettingsSection::Keybindings {
            return false;
        }

        let Some(action) = self.shortcut_capture_action else {
            return false;
        };

        cx.stop_propagation();

        match ev.keystroke.key.as_str() {
            "escape" => {
                self.shortcut_capture_action = None;
                cx.notify();
                return true;
            }
            "backspace" | "delete"
                if !ev.keystroke.modifiers.platform
                    && !ev.keystroke.modifiers.alt
                    && !ev.keystroke.modifiers.control
                    && !ev.keystroke.modifiers.function =>
            {
                self.project_store.clear_shortcut_binding(action);
                self.shortcut_capture_action = None;
                self.show_success_toast(format!("Cleared {}.", action.label()), cx);
                cx.notify();
                return true;
            }
            _ => {}
        }

        match capture_shortcut(ev) {
            Ok(binding) => {
                if let Some(conflict) = self
                    .project_store
                    .ui
                    .shortcuts
                    .action_for_binding(action, &binding)
                {
                    self.show_error_toast(
                        format!("{} already uses that shortcut.", conflict.label()),
                        cx,
                    );
                    return true;
                }

                self.project_store.set_shortcut_binding(action, binding);
                self.shortcut_capture_action = None;
                self.show_success_toast(format!("Updated {}.", action.label()), cx);
                cx.notify();
                true
            }
            Err(message) => {
                self.show_warning_toast(message, cx);
                true
            }
        }
    }

    fn begin_shortcut_capture(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        self.shortcut_capture_action = Some(action);
        cx.notify();
    }

    fn clear_shortcut_binding(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        self.project_store.clear_shortcut_binding(action);
        if self.shortcut_capture_action == Some(action) {
            self.shortcut_capture_action = None;
        }
        self.show_success_toast(format!("Cleared {}.", action.label()), cx);
        cx.notify();
    }

    fn reset_shortcut_binding(&mut self, action: ShortcutAction, cx: &mut Context<Self>) {
        self.project_store.reset_shortcut_binding(action);
        if self.shortcut_capture_action == Some(action) {
            self.shortcut_capture_action = None;
        }
        self.show_success_toast(format!("Reset {}.", action.label()), cx);
        cx.notify();
    }

    fn reset_all_shortcuts(&mut self, cx: &mut Context<Self>) {
        self.project_store.reset_shortcuts();
        self.shortcut_capture_action = None;
        self.show_success_toast("Reset all shortcuts.", cx);
        cx.notify();
    }

    pub(crate) fn focus_settings_agent_input(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        let draft = self
            .settings_agent_input
            .drafts
            .entry(agent_id.to_string())
            .or_default();
        self.settings_agent_input.focused_agent_id = Some(agent_id.to_string());
        self.settings_agent_input.cursor = draft.len();
        self.settings_agent_input.selection_anchor = None;
        self.shortcut_capture_action = None;
        cx.notify();
    }

    fn blur_settings_agent_input(&mut self, cx: &mut Context<Self>) {
        if self.settings_agent_input.focused_agent_id.take().is_none() {
            return;
        }
        self.settings_agent_input.selection_anchor = None;
        cx.notify();
    }

    fn add_agent_launch_arg(&mut self, agent_id: &str, cx: &mut Context<Self>) {
        let Some(agent) = AGENTS.iter().find(|agent| agent.id == agent_id) else {
            return;
        };
        let draft = self
            .settings_agent_input
            .drafts
            .get(agent_id)
            .cloned()
            .unwrap_or_default();
        let token = match validate_agent_launch_arg(&draft) {
            Ok(token) => token,
            Err(message) => {
                self.show_error_toast(message, cx);
                return;
            }
        };

        let mut args = self.project_store.agent_launch_args(agent_id).to_vec();
        args.push(token.clone());
        self.project_store.set_agent_launch_args(agent_id, args);
        self.settings_agent_input
            .drafts
            .insert(agent_id.to_string(), String::new());
        self.settings_agent_input.focused_agent_id = Some(agent_id.to_string());
        self.settings_agent_input.cursor = 0;
        self.settings_agent_input.selection_anchor = None;
        self.show_success_toast(format!("Added {} arg for {}.", token, agent.label), cx);
        cx.notify();
    }

    fn remove_agent_launch_arg(&mut self, agent_id: &str, index: usize, cx: &mut Context<Self>) {
        let Some(agent) = AGENTS.iter().find(|agent| agent.id == agent_id) else {
            return;
        };
        let mut args = self.project_store.agent_launch_args(agent_id).to_vec();
        if index >= args.len() {
            return;
        }
        let removed = args.remove(index);
        self.project_store.set_agent_launch_args(agent_id, args);
        self.show_success_toast(format!("Removed {} arg from {}.", removed, agent.label), cx);
        cx.notify();
    }

    fn handle_settings_agent_input_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(agent_id) = self.settings_agent_input.focused_agent_id.clone() else {
            return false;
        };

        cx.stop_propagation();

        let modifiers = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "backspace" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                if modifiers.platform {
                    delete_settings_input_to_start(
                        draft,
                        &mut self.settings_agent_input.cursor,
                        &mut self.settings_agent_input.selection_anchor,
                    );
                } else if modifiers.alt {
                    delete_settings_input_word_backward(
                        draft,
                        &mut self.settings_agent_input.cursor,
                        &mut self.settings_agent_input.selection_anchor,
                    );
                } else {
                    delete_settings_input_backward(
                        draft,
                        &mut self.settings_agent_input.cursor,
                        &mut self.settings_agent_input.selection_anchor,
                    );
                }
                cx.notify();
                return true;
            }
            "delete" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                delete_settings_input_forward(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                );
                cx.notify();
                return true;
            }
            "left" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                move_settings_input_cursor(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    CursorDirection::Left,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "right" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                move_settings_input_cursor(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    CursorDirection::Right,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "home" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                move_settings_input_cursor_to_edge(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    false,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "end" => {
                let draft = self
                    .settings_agent_input
                    .drafts
                    .entry(agent_id)
                    .or_default();
                move_settings_input_cursor_to_edge(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    true,
                    modifiers.shift,
                );
                cx.notify();
                return true;
            }
            "enter" => {
                self.add_agent_launch_arg(&agent_id, cx);
                return true;
            }
            "escape" | "tab" => {
                self.blur_settings_agent_input(cx);
                return true;
            }
            _ => {}
        }

        let draft = self
            .settings_agent_input
            .drafts
            .entry(agent_id)
            .or_default();

        if modifiers.platform && ev.keystroke.key.as_str() == "a" {
            self.settings_agent_input.cursor = draft.len();
            self.settings_agent_input.selection_anchor = Some(0);
            cx.notify();
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "c" {
            if let Some(range) = settings_agent_input_selected_range(
                self.settings_agent_input.cursor,
                self.settings_agent_input.selection_anchor,
            ) {
                cx.write_to_clipboard(ClipboardItem::new_string(draft[range].to_string()));
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "x" {
            if let Some(range) = settings_agent_input_selected_range(
                self.settings_agent_input.cursor,
                self.settings_agent_input.selection_anchor,
            ) {
                cx.write_to_clipboard(ClipboardItem::new_string(draft[range.clone()].to_string()));
                replace_settings_input_range(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    range,
                    "",
                );
                cx.notify();
            }
            return true;
        }

        if modifiers.platform && ev.keystroke.key.as_str() == "v" {
            if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                insert_settings_input_text(
                    draft,
                    &mut self.settings_agent_input.cursor,
                    &mut self.settings_agent_input.selection_anchor,
                    &text,
                );
                cx.notify();
            }
            return true;
        }

        if modifiers.control || modifiers.platform || modifiers.function {
            return true;
        }

        if let Some(key_char) = ev.keystroke.key_char.as_deref() {
            insert_settings_input_text(
                draft,
                &mut self.settings_agent_input.cursor,
                &mut self.settings_agent_input.selection_anchor,
                key_char,
            );
            cx.notify();
        }

        true
    }

    /// Render the full-window settings page (sidebar + content).
    pub(crate) fn render_settings_page(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        div()
            .flex()
            .flex_row()
            .size_full()
            .bg(rgb(0x1e1f22))
            .child(self.settings_sidebar(window, cx))
            .child(
                div()
                    .id("settings-page-scroll")
                    .flex_1()
                    .min_w(px(0.))
                    .min_h_0()
                    .overflow_y_scroll()
                    .child(self.settings_content(cx)),
            )
    }

    fn settings_sidebar(&self, window: &mut Window, cx: &mut Context<Self>) -> gpui::Div {
        let bg = crate::theme::chrome_bg(window);
        let back_text = hsla(0., 0., 0.55, 1.);
        let back_hover = gpui::white().opacity(0.06);
        let section_active_bg = hsla(215. / 360., 0.60, 0.45, 1.);

        let active = self.settings_section;

        div()
            .flex()
            .flex_col()
            .w(px(SETTINGS_SIDEBAR_W))
            .flex_shrink_0()
            .bg(bg)
            .overflow_hidden()
            .pt(px(TITLEBAR_CHROME_H + 4.))
            .child(
                div()
                    .id("settings-back-btn")
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(4.))
                    .mx(px(12.))
                    .mb(px(16.))
                    .px(px(4.))
                    .py(px(4.))
                    .rounded(px(5.))
                    .cursor_pointer()
                    .hover(move |s| s.bg(back_hover))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.settings_open = false;
                            this.shortcut_capture_action = None;
                            this.settings_agent_input.focused_agent_id = None;
                            this.settings_agent_input.selection_anchor = None;
                            cx.notify();
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__chevron-left.svg")
                            .size(px(14.))
                            .text_color(back_text),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .text_color(back_text)
                            .child("Back to app"),
                    ),
            )
            .child(self.settings_nav_item(SettingsSection::Agents, active, section_active_bg, cx))
            .child(self.settings_nav_item(SettingsSection::OpenIn, active, section_active_bg, cx))
            .child(self.settings_nav_item(
                SettingsSection::Keybindings,
                active,
                section_active_bg,
                cx,
            ))
    }

    fn settings_nav_item(
        &self,
        section: SettingsSection,
        active: SettingsSection,
        active_bg: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_active = section == active;
        let label = section.label();
        let text_col = if is_active {
            gpui::white()
        } else {
            TEXT_SECONDARY()
        };
        let hover_bg = gpui::white().opacity(0.06);

        div()
            .id(label)
            .flex()
            .flex_row()
            .items_center()
            .h(px(30.))
            .mx(px(8.))
            .px(px(10.))
            .rounded(px(5.))
            .cursor_pointer()
            .when(is_active, move |d| d.bg(active_bg))
            .when(!is_active, move |d| d.hover(move |s| s.bg(hover_bg)))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    this.settings_section = section;
                    this.shortcut_capture_action = None;
                    this.settings_agent_input.focused_agent_id = None;
                    this.settings_agent_input.selection_anchor = None;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .text_size(rems(13. / 16.))
                    .text_color(text_col)
                    .child(label),
            )
    }

    fn settings_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        match self.settings_section {
            SettingsSection::Agents => self.settings_agents_content(cx),
            SettingsSection::OpenIn => self.settings_open_in_content(cx),
            SettingsSection::Keybindings => self.settings_keybindings_content(cx),
        }
    }

    fn settings_agents_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        let panel_bg = rgb(0x23252a);
        let row_bg = rgb(0x1f2125);
        let pill_bg = rgb(0x2a2d33);
        let pill_border = gpui::white().opacity(0.10);
        let button_bg = gpui::white().opacity(0.04);
        let button_hover = gpui::white().opacity(0.08);
        let active_button_bg = hsla(215. / 360., 0.60, 0.45, 1.);
        let enabled_agents = self.enabled_agents();

        let mut rows = div().flex().flex_col();
        for (index, agent) in AGENTS.iter().enumerate() {
            let args = self.project_store.agent_launch_args(agent.id);
            let is_enabled = self.agent_enabled(agent.id);
            let draft = self
                .settings_agent_input
                .drafts
                .get(agent.id)
                .cloned()
                .unwrap_or_default();
            let is_focused =
                self.settings_agent_input.focused_agent_id.as_deref() == Some(agent.id);
            let selection = settings_agent_input_selected_range(
                self.settings_agent_input.cursor,
                self.settings_agent_input.selection_anchor,
            );

            let mut row = div()
                .id(("settings-agent-row", index))
                .flex()
                .flex_row()
                .items_start()
                .justify_between()
                .gap(px(20.))
                .px(px(18.))
                .py(px(16.))
                .bg(row_bg);

            if index > 0 {
                row = row.border_t_1().border_color(BORDER_SUBTLE());
            }

            let mut arg_pills = div().flex().flex_row().flex_wrap().gap(px(8.));
            if args.is_empty() {
                arg_pills = arg_pills.child(
                    div()
                        .px(px(10.))
                        .py(px(6.))
                        .rounded(px(8.))
                        .border_1()
                        .border_color(pill_border)
                        .bg(pill_bg)
                        .text_size(rems(12. / 16.))
                        .font_family("Lilex Nerd Font Mono")
                        .text_color(TEXT_SECONDARY())
                        .child("No extra args"),
                );
            } else {
                for (arg_index, arg) in args.iter().enumerate() {
                    let arg_label = arg.clone();
                    arg_pills = arg_pills.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .px(px(10.))
                            .py(px(6.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(pill_border)
                            .bg(pill_bg)
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_family("Lilex Nerd Font Mono")
                                    .text_color(TEXT_PRIMARY())
                                    .child(arg_label),
                            )
                            .child(
                                div()
                                    .w(px(18.))
                                    .h(px(18.))
                                    .rounded(px(5.))
                                    .cursor_pointer()
                                    .hover(move |style| style.bg(gpui::white().opacity(0.08)))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(
                                            move |this, _ev: &MouseDownEvent, _window, cx| {
                                                this.remove_agent_launch_arg(
                                                    agent.id, arg_index, cx,
                                                );
                                                cx.stop_propagation();
                                            },
                                        ),
                                    )
                                    .child(
                                        div()
                                            .h_full()
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .text_size(rems(11. / 16.))
                                            .text_color(TEXT_SECONDARY())
                                            .child("x"),
                                    ),
                            ),
                    );
                }
            }

            rows = rows.child(row.child(
                div()
                    .flex()
                    .flex_col()
                    .w_full()
                    .min_w(px(0.))
                    .gap(px(12.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_start()
                            .justify_between()
                            .gap(px(20.))
                            .child(
                                div()
                                    .flex()
                                    .flex_col()
                                    .gap(px(10.))
                                    .min_w(px(0.))
                                    .max_w(px(540.))
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(12.))
                                            .child(branded_icon(
                                                agent.icon,
                                                18.,
                                                Some(TEXT_PRIMARY()),
                                            ))
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .gap(px(4.))
                                                    .child(
                                                        div()
                                                            .text_size(rems(13. / 16.))
                                                            .font_weight(
                                                                gpui::FontWeight::MEDIUM,
                                                            )
                                                            .text_color(TEXT_PRIMARY())
                                                            .child(agent.label),
                                                    )
                                                    .child(
                                                        div()
                                                            .text_size(rems(11. / 16.))
                                                            .text_color(TEXT_SECONDARY())
                                                            .child(format!(
                                                                "Extra argv tokens passed to {} on every launch and resume.",
                                                                agent.label
                                                            )),
                                                    ),
                                            ),
                                    ),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(8.))
                                    .child(
                                        div()
                                            .id(("settings-agent-input", index))
                                            .h(px(34.))
                                            .w(px(180.))
                                            .min_w(px(0.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(if is_focused {
                                                active_button_bg.opacity(0.85)
                                            } else {
                                                BORDER_SUBTLE()
                                            })
                                            .bg(button_bg)
                                            .pl(px(10.))
                                            .pr(px(1.4))
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(button_hover))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _ev: &MouseDownEvent, window, cx| {
                                                        this.focus_handle.focus(window);
                                                        this.focus_settings_agent_input(
                                                            agent.id, cx,
                                                        );
                                                        cx.stop_propagation();
                                                    },
                                                ),
                                            )
                                            .child(render_settings_agent_input_content(
                                                &draft,
                                                is_focused,
                                                self.settings_agent_input.cursor,
                                                selection,
                                            )),
                                    )
                                    .child(
                                        div()
                                            .id(("settings-agent-add", index))
                                            .h(px(34.))
                                            .px(px(12.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(BORDER_SUBTLE())
                                            .bg(button_bg)
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(button_hover))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _ev: &MouseDownEvent, _window, cx| {
                                                        this.add_agent_launch_arg(agent.id, cx);
                                                        cx.stop_propagation();
                                                    },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .h_full()
                                                    .flex()
                                                    .items_center()
                                                    .text_size(rems(12. / 16.))
                                                    .font_weight(
                                                        gpui::FontWeight::MEDIUM,
                                                    )
                                                    .text_color(TEXT_PRIMARY())
                                                    .child("Add"),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .h(px(34.))
                                            .px(px(10.))
                                            .rounded(px(8.))
                                            .border_1()
                                            .border_color(BORDER_SUBTLE())
                                            .bg(button_bg)
                                            .cursor_pointer()
                                            .hover(move |style| style.bg(button_hover))
                                            .on_mouse_down(
                                                MouseButton::Left,
                                                cx.listener(
                                                    move |this, _ev: &MouseDownEvent, _window, cx| {
                                                        this.set_agent_enabled(
                                                            agent.id,
                                                            !is_enabled,
                                                            cx,
                                                        );
                                                        cx.stop_propagation();
                                                    },
                                                ),
                                            )
                                            .child(
                                                div()
                                                    .h_full()
                                                    .flex()
                                                    .flex_row()
                                                    .items_center()
                                                    .gap(px(10.))
                                                    .child(
                                                        div()
                                                            .text_size(rems(11. / 16.))
                                                            .font_weight(
                                                                gpui::FontWeight::MEDIUM,
                                                            )
                                                            .text_color(if is_enabled {
                                                                gpui::white()
                                                            } else {
                                                                TEXT_SECONDARY()
                                                            })
                                                            .child(if is_enabled {
                                                                "Enabled"
                                                            } else {
                                                                "Disabled"
                                                            }),
                                                    )
                                                    .child(
                                                        div()
                                                            .w(px(18.))
                                                            .h(px(18.))
                                                            .rounded(px(5.))
                                                            .border_1()
                                                            .border_color(if is_enabled {
                                                                active_button_bg.opacity(0.85)
                                                            } else {
                                                                BORDER_SUBTLE()
                                                            })
                                                            .bg(if is_enabled {
                                                                active_button_bg
                                                            } else {
                                                                button_bg
                                                            })
                                                            .flex()
                                                            .items_center()
                                                            .justify_center()
                                                            .when(is_enabled, |container| {
                                                                container.child(
                                                                    svg()
                                                                        .path(
                                                                            "assets/icons/icons__check.svg",
                                                                        )
                                                                        .size(px(11.))
                                                                        .text_color(gpui::white()),
                                                                )
                                                            }),
                                                    ),
                                            ),
                                    ),
                            ),
                    )
                    .child(div().w_full().min_w(px(0.)).child(arg_pills)),
            ));
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .p(px(32.))
            .child(
                div()
                    .text_size(rems(18. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT_PRIMARY())
                    .child("Agents"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(760.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child(
                        "Manage per-agent argv tokens and availability. Disabled agents stay here so they can be re-enabled, but they are hidden from New Task and Add Agent pickers. Changes save immediately.",
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(16.))
                    .max_w(px(860.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Availability"),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child(
                                        "Toggle agents off to hide them from creation pickers. Arg editing still applies while an agent is disabled.",
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT_PRIMARY())
                            .child(format!("{} enabled", enabled_agents.len())),
                    ),
            )
            .child(
                div()
                    .mt(px(12.))
                    .mb(px(16.))
                    .max_w(px(860.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Token rules"),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child(
                                        "Whitespace is rejected because spaces would create multiple argv tokens. Reorder by removing and re-adding.",
                                    ),
                            ),
                    ),
            )
            .child(
                div()
                    .max_w(px(860.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .overflow_hidden()
                    .child(rows),
            )
    }

    fn settings_open_in_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        let panel_bg = rgb(0x23252a);
        let row_bg = rgb(0x1f2125);
        let button_bg = gpui::white().opacity(0.04);
        let button_hover = gpui::white().opacity(0.08);
        let active_button_bg = hsla(215. / 360., 0.60, 0.45, 1.);
        let enabled_apps = self.enabled_open_in_apps();

        let mut rows = div().flex().flex_col();
        for (index, app) in self.available_open_in_apps.iter().copied().enumerate() {
            let is_enabled = self.open_in_app_enabled(app);

            let mut row = div()
                .id(("settings-open-in-row", index))
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(20.))
                .px(px(18.))
                .py(px(14.))
                .bg(row_bg)
                .cursor_pointer()
                .hover(move |style| style.bg(gpui::white().opacity(0.06)))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                        this.set_open_in_app_enabled(app, !is_enabled, cx);
                        cx.stop_propagation();
                    }),
                );

            if index > 0 {
                row = row.border_t_1().border_color(BORDER_SUBTLE());
            }

            rows = rows.child(
                row.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(12.))
                        .child(
                            svg()
                                .path(app.icon_path())
                                .size(px(16.))
                                .text_color(TEXT_PRIMARY()),
                        )
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .gap(px(4.))
                                .child(
                                    div()
                                        .text_size(rems(13. / 16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(TEXT_PRIMARY())
                                        .child(app.label()),
                                )
                                .child(
                                    div()
                                        .text_size(rems(11. / 16.))
                                        .text_color(TEXT_SECONDARY())
                                        .child(app.description()),
                                ),
                        ),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(10.))
                        .child(
                            div()
                                .text_size(rems(11. / 16.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(if is_enabled {
                                    gpui::white()
                                } else {
                                    TEXT_SECONDARY()
                                })
                                .child(if is_enabled { "Enabled" } else { "Disabled" }),
                        )
                        .child(
                            div()
                                .w(px(18.))
                                .h(px(18.))
                                .rounded(px(5.))
                                .border_1()
                                .border_color(if is_enabled {
                                    active_button_bg.opacity(0.85)
                                } else {
                                    BORDER_SUBTLE()
                                })
                                .bg(if is_enabled {
                                    active_button_bg
                                } else {
                                    button_bg
                                })
                                .hover(move |style| {
                                    style.bg(if is_enabled {
                                        active_button_bg
                                    } else {
                                        button_hover
                                    })
                                })
                                .flex()
                                .items_center()
                                .justify_center()
                                .when(is_enabled, |container| {
                                    container.child(
                                        svg()
                                            .path("assets/icons/icons__check.svg")
                                            .size(px(11.))
                                            .text_color(gpui::white()),
                                    )
                                }),
                        ),
                ),
            );
        }

        let availability_note = if self.available_open_in_apps.is_empty() {
            "No supported apps were detected on this machine."
        } else {
            "Only apps detected on this machine appear here. Changes save immediately."
        };

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .p(px(32.))
            .child(
                div()
                    .text_size(rems(18. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT_PRIMARY())
                    .child("Open In"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(760.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child(
                        "Choose which detected apps appear in the project header's Open In menu.",
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(16.))
                    .max_w(px(860.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Detected apps"),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child(availability_note),
                            ),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(TEXT_PRIMARY())
                            .child(format!("{} enabled", enabled_apps.len())),
                    ),
            )
            .when(self.available_open_in_apps.is_empty(), |section| {
                section.child(
                    div()
                        .max_w(px(860.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(BORDER_SUBTLE())
                        .bg(panel_bg)
                        .px(px(20.))
                        .py(px(18.))
                        .child(
                            div()
                                .text_size(rems(12. / 16.))
                                .line_height(rems(18. / 16.))
                                .text_color(TEXT_SECONDARY())
                                .child(
                                    "Install Cursor, Zed, VS Code, or use your system file manager, then restart the app to refresh the menu.",
                                ),
                        ),
                )
            })
            .when(!self.available_open_in_apps.is_empty(), |section| {
                section.child(
                    div()
                        .max_w(px(860.))
                        .rounded(px(12.))
                        .border_1()
                        .border_color(BORDER_SUBTLE())
                        .bg(panel_bg)
                        .overflow_hidden()
                        .child(rows),
                )
            })
    }

    fn settings_keybindings_content(&self, cx: &mut Context<Self>) -> gpui::Div {
        let panel_bg = rgb(0x23252a);
        let row_bg = rgb(0x1f2125);
        let table_header = hsla(0., 0., 0.45, 1.);
        let pill_bg = rgb(0x2a2d33);
        let pill_border = gpui::white().opacity(0.10);
        let button_bg = gpui::white().opacity(0.04);
        let button_hover = gpui::white().opacity(0.08);
        let active_button_bg = hsla(215. / 360., 0.60, 0.45, 1.);
        let destructive_text = hsla(0.0, 0.73, 0.67, 1.);

        let mut rows = div().flex().flex_col();
        for (index, action) in ALL_SHORTCUT_ACTIONS.into_iter().enumerate() {
            let is_capturing = self.shortcut_capture_action == Some(action);
            let shortcut = self.project_store.ui.shortcuts.binding_for(action);

            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(20.))
                .px(px(18.))
                .py(px(14.))
                .bg(row_bg);

            if index > 0 {
                row = row.border_t_1().border_color(BORDER_SUBTLE());
            }

            let shortcut_display = if is_capturing {
                div()
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(gpui::white())
                            .child("Press shortcut now"),
                    )
                    .child(
                        div()
                            .text_size(rems(11. / 16.))
                            .text_color(TEXT_SECONDARY())
                            .child("Esc cancels. Delete clears."),
                    )
            } else {
                self.render_shortcut_pills(shortcut, pill_bg, pill_border)
            };

            let capture_label = if is_capturing { "Listening…" } else { "Edit" };
            let capture_text = if is_capturing {
                gpui::white()
            } else {
                TEXT_PRIMARY()
            };

            rows = rows.child(
                row.child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(6.))
                        .child(
                            div()
                                .text_size(rems(13. / 16.))
                                .font_weight(gpui::FontWeight::MEDIUM)
                                .text_color(TEXT_PRIMARY())
                                .child(action.label()),
                        )
                        .child(shortcut_display),
                )
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .child(
                            div()
                                .id(("settings-shortcut-capture", index))
                                .h(px(30.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(if is_capturing {
                                    active_button_bg.opacity(0.85)
                                } else {
                                    BORDER_SUBTLE()
                                })
                                .bg(if is_capturing {
                                    active_button_bg
                                } else {
                                    button_bg
                                })
                                .cursor_pointer()
                                .hover(move |s| {
                                    s.bg(if is_capturing {
                                        active_button_bg
                                    } else {
                                        button_hover
                                    })
                                })
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                        this.begin_shortcut_capture(action, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .child(
                                    div()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .text_size(rems(12. / 16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(capture_text)
                                        .child(capture_label),
                                ),
                        )
                        .child(
                            div()
                                .id(("settings-shortcut-reset", index))
                                .h(px(30.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(BORDER_SUBTLE())
                                .bg(button_bg)
                                .cursor_pointer()
                                .hover(move |s| s.bg(button_hover))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                        this.reset_shortcut_binding(action, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .child(
                                    div()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .text_size(rems(12. / 16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(TEXT_PRIMARY())
                                        .child("Reset"),
                                ),
                        )
                        .child(
                            div()
                                .id(("settings-shortcut-clear", index))
                                .h(px(30.))
                                .px(px(12.))
                                .rounded(px(8.))
                                .border_1()
                                .border_color(BORDER_SUBTLE())
                                .bg(button_bg)
                                .cursor_pointer()
                                .hover(move |s| s.bg(button_hover))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                        this.clear_shortcut_binding(action, cx);
                                        cx.stop_propagation();
                                    }),
                                )
                                .child(
                                    div()
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .text_size(rems(12. / 16.))
                                        .font_weight(gpui::FontWeight::MEDIUM)
                                        .text_color(destructive_text)
                                        .child("Clear"),
                                ),
                        ),
                ),
            );
        }

        div()
            .flex()
            .flex_col()
            .w_full()
            .min_w(px(0.))
            .p(px(32.))
            .child(
                div()
                    .text_size(rems(18. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(TEXT_PRIMARY())
                    .child("Keybindings"),
            )
            .child(
                div()
                    .mt(px(4.))
                    .max_w(px(720.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child(
                        "Choose Edit on a command, then press the new key combination. Changes save immediately and apply to tab and navigation shortcuts across the app.",
                    ),
            )
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(16.))
                    .max_w(px(860.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(16.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .px(px(16.))
                    .py(px(14.))
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(4.))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Capture rules"),
                            )
                            .child(
                                div()
                                    .text_size(rems(11. / 16.))
                                    .text_color(TEXT_SECONDARY())
                                    .child("Use at least one modifier key. Duplicate shortcuts are blocked."),
                            ),
                    )
                    .child(
                        div()
                            .id("settings-shortcuts-reset-all")
                            .h(px(30.))
                            .px(px(12.))
                            .rounded(px(8.))
                            .border_1()
                            .border_color(BORDER_SUBTLE())
                            .bg(button_bg)
                            .cursor_pointer()
                            .hover(move |s| s.bg(button_hover))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.reset_all_shortcuts(cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                div()
                                    .h_full()
                                    .flex()
                                    .items_center()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(TEXT_PRIMARY())
                                    .child("Reset All"),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_col()
                    .flex_1()
                    .min_h(px(0.))
                    .max_w(px(860.))
                    .rounded(px(12.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(panel_bg)
                    .overflow_hidden()
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .px(px(18.))
                            .h(px(38.))
                            .border_b_1()
                            .border_color(BORDER_SUBTLE())
                            .text_size(rems(11. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(table_header)
                            .child("COMMAND")
                            .child("SHORTCUT"),
                    )
                    .child(div().flex_1().min_h(px(0.)).child(rows)),
            )
    }

    fn render_shortcut_pills(
        &self,
        shortcut: &str,
        pill_bg: gpui::Rgba,
        pill_border: gpui::Hsla,
    ) -> gpui::Div {
        if shortcut.is_empty() {
            return div().flex().flex_row().items_center().gap(px(8.)).child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(pill_border)
                    .bg(pill_bg)
                    .text_size(rems(12. / 16.))
                    .font_family("Lilex Nerd Font Mono")
                    .text_color(TEXT_SECONDARY())
                    .child("Unassigned"),
            );
        }

        let mut shortcut_pills = div().flex().flex_row().items_center().gap(px(8.));
        for token in shortcut.split('-') {
            shortcut_pills = shortcut_pills.child(
                div()
                    .px(px(10.))
                    .py(px(6.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(pill_border)
                    .bg(pill_bg)
                    .text_size(rems(12. / 16.))
                    .font_family("Lilex Nerd Font Mono")
                    .text_color(TEXT_PRIMARY())
                    .child(keybinding_token_label(token)),
            );
        }
        shortcut_pills
    }
}

#[derive(Clone, Copy)]
enum CursorDirection {
    Left,
    Right,
}

fn validate_agent_launch_arg(value: &str) -> Result<String, &'static str> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("Enter one argv token before adding it.");
    }

    if trimmed != value || value.chars().any(char::is_whitespace) {
        return Err("Launch args must be a single argv token without whitespace.");
    }

    Ok(value.to_string())
}

fn settings_agent_input_selected_range(
    cursor: usize,
    selection_anchor: Option<usize>,
) -> Option<std::ops::Range<usize>> {
    let anchor = selection_anchor?;
    if anchor == cursor {
        None
    } else if anchor < cursor {
        Some(anchor..cursor)
    } else {
        Some(cursor..anchor)
    }
}

fn previous_settings_input_boundary(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .rev()
        .find_map(|(index, _)| (index < cursor).then_some(index))
        .unwrap_or(0)
}

fn next_settings_input_boundary(text: &str, cursor: usize) -> usize {
    text.char_indices()
        .find_map(|(index, _)| (index > cursor).then_some(index))
        .unwrap_or(text.len())
}

fn replace_settings_input_range(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    range: std::ops::Range<usize>,
    replacement: &str,
) {
    text.replace_range(range.clone(), replacement);
    *cursor = range.start + replacement.len();
    *selection_anchor = None;
}

fn insert_settings_input_text(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    inserted: &str,
) {
    if let Some(range) = settings_agent_input_selected_range(*cursor, *selection_anchor) {
        replace_settings_input_range(text, cursor, selection_anchor, range, inserted);
        return;
    }

    text.insert_str(*cursor, inserted);
    *cursor += inserted.len();
    *selection_anchor = None;
}

fn delete_settings_input_backward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    if let Some(range) = settings_agent_input_selected_range(*cursor, *selection_anchor) {
        replace_settings_input_range(text, cursor, selection_anchor, range, "");
        return;
    }

    if *cursor == 0 {
        return;
    }

    let start = previous_settings_input_boundary(text, *cursor);
    replace_settings_input_range(text, cursor, selection_anchor, start..*cursor, "");
}

fn previous_settings_input_word_boundary(text: &str, cursor: usize) -> usize {
    let mut idx = cursor;
    while idx > 0 {
        let start = previous_settings_input_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if !ch.is_whitespace() {
            break;
        }
        idx = start;
    }

    while idx > 0 {
        let start = previous_settings_input_boundary(text, idx);
        let ch = text[start..idx].chars().next().unwrap_or_default();
        if ch.is_alphanumeric() || matches!(ch, '_' | '-') {
            idx = start;
        } else {
            break;
        }
    }

    idx
}

fn delete_settings_input_word_backward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    if let Some(range) = settings_agent_input_selected_range(*cursor, *selection_anchor) {
        replace_settings_input_range(text, cursor, selection_anchor, range, "");
        return;
    }

    if *cursor == 0 {
        return;
    }

    let start = previous_settings_input_word_boundary(text, *cursor);
    replace_settings_input_range(text, cursor, selection_anchor, start..*cursor, "");
}

fn delete_settings_input_to_start(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    if let Some(range) = settings_agent_input_selected_range(*cursor, *selection_anchor) {
        replace_settings_input_range(text, cursor, selection_anchor, range, "");
        return;
    }

    if *cursor == 0 {
        return;
    }

    replace_settings_input_range(text, cursor, selection_anchor, 0..*cursor, "");
}

fn delete_settings_input_forward(
    text: &mut String,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
) {
    if let Some(range) = settings_agent_input_selected_range(*cursor, *selection_anchor) {
        replace_settings_input_range(text, cursor, selection_anchor, range, "");
        return;
    }

    if *cursor >= text.len() {
        return;
    }

    let end = next_settings_input_boundary(text, *cursor);
    replace_settings_input_range(text, cursor, selection_anchor, *cursor..end, "");
}

fn move_settings_input_cursor(
    text: &str,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    direction: CursorDirection,
    extend_selection: bool,
) {
    let next_cursor = match direction {
        CursorDirection::Left => {
            if let Some(range) = settings_agent_input_selected_range(*cursor, *selection_anchor) {
                if extend_selection {
                    previous_settings_input_boundary(text, *cursor)
                } else {
                    range.start
                }
            } else {
                previous_settings_input_boundary(text, *cursor)
            }
        }
        CursorDirection::Right => {
            if let Some(range) = settings_agent_input_selected_range(*cursor, *selection_anchor) {
                if extend_selection {
                    next_settings_input_boundary(text, *cursor)
                } else {
                    range.end
                }
            } else {
                next_settings_input_boundary(text, *cursor)
            }
        }
    };

    if extend_selection {
        if selection_anchor.is_none() {
            *selection_anchor = Some(*cursor);
        }
    } else {
        *selection_anchor = None;
    }

    *cursor = next_cursor;
}

fn move_settings_input_cursor_to_edge(
    text: &str,
    cursor: &mut usize,
    selection_anchor: &mut Option<usize>,
    to_end: bool,
    extend_selection: bool,
) {
    if extend_selection && selection_anchor.is_none() {
        *selection_anchor = Some(*cursor);
    }
    if !extend_selection {
        *selection_anchor = None;
    }
    *cursor = if to_end { text.len() } else { 0 };
}

fn intersect_byte_ranges(
    left: std::ops::Range<usize>,
    right: std::ops::Range<usize>,
) -> Option<std::ops::Range<usize>> {
    let start = left.start.max(right.start);
    let end = left.end.min(right.end);
    (start < end).then_some(start..end)
}

fn visible_input_range(
    text: &str,
    cursor: usize,
    selection: Option<&std::ops::Range<usize>>,
    max_chars: usize,
    extra_reserved_chars: usize,
) -> std::ops::Range<usize> {
    let boundaries = text
        .char_indices()
        .map(|(idx, _)| idx)
        .chain(std::iter::once(text.len()))
        .collect::<Vec<_>>();
    let total_chars = boundaries.len().saturating_sub(1);
    if total_chars <= max_chars.saturating_sub(extra_reserved_chars) {
        return 0..text.len();
    }

    let cursor_char = text[..cursor.min(text.len())].chars().count();
    let selection_chars = selection.map(|selection| {
        (
            text[..selection.start.min(text.len())].chars().count(),
            text[..selection.end.min(text.len())].chars().count(),
        )
    });
    let visible_chars = max_chars.saturating_sub(extra_reserved_chars).max(1);
    let mut start_char = cursor_char.saturating_sub(visible_chars / 2);
    let mut end_char = (start_char + visible_chars).min(total_chars);
    start_char = end_char.saturating_sub(visible_chars);

    if cursor_char >= total_chars.saturating_sub(visible_chars / 3) {
        end_char = total_chars;
        start_char = total_chars.saturating_sub(visible_chars);
    }

    if let Some((selection_start_char, selection_end_char)) = selection_chars {
        if selection_start_char < start_char {
            start_char = selection_start_char;
            end_char = (start_char + visible_chars).min(total_chars);
        }
        if selection_end_char > end_char {
            end_char = selection_end_char.min(total_chars);
            start_char = end_char.saturating_sub(visible_chars);
        }
    }

    boundaries[start_char]..boundaries[end_char]
}

fn render_settings_agent_input_content(
    text: &str,
    focused: bool,
    cursor: usize,
    selection: Option<std::ops::Range<usize>>,
) -> gpui::Div {
    let cursor = cursor.min(text.len());
    let selection = selection.map(|range| range.start.min(text.len())..range.end.min(text.len()));

    if text.is_empty() {
        return div()
            .h_full()
            .flex()
            .items_center()
            .gap(px(0.))
            .text_size(rems(12. / 16.))
            .font_family("Lilex Nerd Font Mono")
            .child(if focused {
                div().w(px(1.)).h(px(16.)).mr(px(1.)).bg(TEXT_PRIMARY())
            } else {
                div().w(px(0.))
            })
            .child(div().text_color(TEXT_SECONDARY()).child("argv-token"));
    }

    let selected = selection.filter(|range| range.start < range.end);
    let visible_range =
        visible_input_range(text, cursor, selected.as_ref(), 20, usize::from(focused));
    let visible_start = visible_range.start;
    let visible_text = text[visible_range.clone()].to_string();
    let local_cursor = cursor.saturating_sub(visible_start).min(visible_text.len());
    let visible_selection = selected
        .as_ref()
        .and_then(|range| intersect_byte_ranges(range.clone(), visible_range.clone()))
        .map(|range| range.start - visible_start..range.end - visible_start);

    let mut row = div()
        .h_full()
        .flex()
        .items_center()
        .gap(px(0.))
        .overflow_hidden()
        .text_size(rems(12. / 16.))
        .font_family("Lilex Nerd Font Mono");

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
    let middle = visible_selection
        .as_ref()
        .map(|range| visible_text[range.clone()].to_string())
        .unwrap_or_default();
    let trailing_start = visible_selection
        .as_ref()
        .filter(|range| range.end <= local_cursor)
        .map(|_| selected_end)
        .unwrap_or(local_cursor.min(visible_text.len()));
    let trailing = visible_text[trailing_start..].to_string();

    if !prefix.is_empty() {
        row = row.child(div().text_color(TEXT_PRIMARY()).child(prefix));
    }

    if focused {
        row = row.child(div().w(px(1.)).h(px(16.)).bg(TEXT_PRIMARY()));
    }

    if !middle.is_empty() {
        row = row.child(
            div()
                .px(px(1.))
                .bg(hsla(220. / 360., 0.55, 0.55, 0.35))
                .text_color(TEXT_PRIMARY())
                .child(middle),
        );
    }

    if !trailing.is_empty() {
        row = row.child(div().text_color(TEXT_PRIMARY()).child(trailing));
    }

    row
}

#[cfg(test)]
mod tests {
    use super::{
        insert_settings_input_text, settings_agent_input_selected_range, validate_agent_launch_arg,
        visible_input_range,
    };

    #[test]
    fn validates_single_token_launch_args() {
        assert_eq!(
            validate_agent_launch_arg("--yolo"),
            Ok("--yolo".to_string())
        );
    }

    #[test]
    fn rejects_empty_launch_args() {
        assert_eq!(
            validate_agent_launch_arg(""),
            Err("Enter one argv token before adding it.")
        );
    }

    #[test]
    fn rejects_whitespace_launch_args() {
        assert_eq!(
            validate_agent_launch_arg(" --yolo"),
            Err("Launch args must be a single argv token without whitespace.")
        );
        assert_eq!(
            validate_agent_launch_arg("--profile debug"),
            Err("Launch args must be a single argv token without whitespace.")
        );
    }

    #[test]
    fn inserts_text_at_cursor() {
        let mut text = String::from("--model");
        let mut cursor = 2;
        let mut selection_anchor = None;

        insert_settings_input_text(&mut text, &mut cursor, &mut selection_anchor, "agent-");

        assert_eq!(text, "--agent-model");
        assert_eq!(cursor, "--agent-".len());
        assert_eq!(selection_anchor, None);
    }

    #[test]
    fn replaces_selected_text_when_inserting() {
        let mut text = String::from("--model");
        let mut cursor = text.len();
        let mut selection_anchor = Some(2);

        insert_settings_input_text(&mut text, &mut cursor, &mut selection_anchor, "agent");

        assert_eq!(text, "--agent");
        assert_eq!(cursor, text.len());
        assert_eq!(selection_anchor, None);
        assert_eq!(
            settings_agent_input_selected_range(cursor, selection_anchor),
            None
        );
    }

    #[test]
    fn visible_input_range_keeps_end_visible_when_clipped() {
        let text = "--dangerously-skip-permissions";

        let visible = visible_input_range(text, text.len(), None, 20, 1);

        assert!(visible.start > 0);
        assert_eq!(visible.end, text.len());
        assert_eq!(text[visible].chars().count(), 19);
    }
}
