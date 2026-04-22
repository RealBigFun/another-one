//! App-level settings page with a sidebar navigation and content area.

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, Context, KeyDownEvent, MouseButton, MouseDownEvent,
    Window,
};

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
            .child(self.settings_content(cx))
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
            .child(self.settings_nav_item(
                SettingsSection::OpenIn,
                active,
                section_active_bg,
                cx,
            ))
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
            SettingsSection::Agents => self.settings_agents_content(),
            SettingsSection::OpenIn => self.settings_open_in_content(cx),
            SettingsSection::Keybindings => self.settings_keybindings_content(cx),
        }
    }

    fn settings_agents_content(&self) -> gpui::Div {
        div()
            .flex()
            .flex_col()
            .flex_1()
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
                    .text_size(rems(12. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child(
                        "Provider routing, prompt templates, and execution rules per agent action.",
                    ),
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
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
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
            .flex_1()
            .min_w(px(0.))
            .min_h(px(0.))
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
