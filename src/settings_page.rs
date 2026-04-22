//! App-level settings page with a sidebar navigation and content area.

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, Context, MouseButton, MouseDownEvent, Window,
};

use crate::app::AnotherOneApp;
use crate::layout::TITLEBAR_CHROME_H;

const TEXT_PRIMARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.92, 1.);
const TEXT_SECONDARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.55, 1.);
const BORDER_SUBTLE: fn() -> gpui::Hsla = || gpui::white().opacity(0.08);

const SETTINGS_SIDEBAR_W: f32 = 180.;

const KEYBINDING_ROWS: [(&str, &str); 7] = [
    ("Cycle Projects", "cmd-o"),
    ("New Tab in Current Task", "cmd-n"),
    ("New Task", "cmd-t"),
    ("Next Tab", "cmd-shift-]"),
    ("Previous Tab", "cmd-shift-["),
    ("Next Task", "cmd-alt-down"),
    ("Previous Task", "cmd-alt-up"),
];

/// Which settings section is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Agents,
    Keybindings,
}

impl SettingsSection {
    fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
            Self::Keybindings => "Keybindings",
        }
    }
}

impl AnotherOneApp {
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
            // ── Settings sidebar ─────────────────────────────────
            .child(self.settings_sidebar(window, cx))
            // ── Content area ─────────────────────────────────────
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
            // Top padding to clear macOS traffic lights
            .pt(px(TITLEBAR_CHROME_H + 4.))
            // ── Back to app ──────────────────────────────────────
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
            // ── Section list ─────────────────────────────────────
            .child(self.settings_nav_item(SettingsSection::Agents, active, section_active_bg, cx))
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

    fn settings_content(&self, _cx: &mut Context<Self>) -> gpui::Div {
        match self.settings_section {
            SettingsSection::Agents => self.settings_agents_content(),
            SettingsSection::Keybindings => self.settings_keybindings_content(),
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

    fn settings_keybindings_content(&self) -> gpui::Div {
        let panel_bg = rgb(0x23252a);
        let row_bg = rgb(0x1f2125);
        let search_bg = rgb(0x191b1f);
        let search_icon = hsla(0., 0., 0.45, 1.);
        let table_header = hsla(0., 0., 0.45, 1.);
        let pill_bg = rgb(0x2a2d33);
        let pill_border = gpui::white().opacity(0.10);

        let mut rows = div().flex().flex_col();
        for (index, (action, shortcut)) in KEYBINDING_ROWS.iter().enumerate() {
            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .gap(px(16.))
                .px(px(18.))
                .py(px(14.))
                .bg(row_bg);

            if index > 0 {
                row = row.border_t_1().border_color(BORDER_SUBTLE());
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
                        .child(Self::keybinding_token_label(token)),
                );
            }

            rows = rows.child(
                row.child(
                    div()
                        .text_size(rems(13. / 16.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(TEXT_PRIMARY())
                        .child(*action),
                )
                .child(shortcut_pills),
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
                    .max_w(px(640.))
                    .text_size(rems(12. / 16.))
                    .line_height(rems(18. / 16.))
                    .text_color(TEXT_SECONDARY())
                    .child("Keyboard shortcuts for cycling projects, creating tabs and tasks, then moving through them. This first pass is read-only and shows the current defaults."),
            )
            .child(
                div()
                    .mt(px(24.))
                    .mb(px(16.))
                    .max_w(px(420.))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .px(px(12.))
                    .h(px(38.))
                    .rounded(px(9.))
                    .border_1()
                    .border_color(BORDER_SUBTLE())
                    .bg(search_bg)
                    .child(
                        svg()
                            .path("assets/icons/icons__tool-search.svg")
                            .size(px(14.))
                            .text_color(search_icon),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .text_color(TEXT_SECONDARY())
                            .child("Search keybindings"),
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
                            .child("KEYBINDING"),
                    )
                    .child(div().flex_1().min_h(px(0.)).child(rows)),
            )
    }

    fn keybinding_token_label(token: &str) -> String {
        match token {
            "cmd" => "Cmd".to_string(),
            "shift" => "Shift".to_string(),
            "alt" => "Alt".to_string(),
            "up" => "Up".to_string(),
            "down" => "Down".to_string(),
            "[" => "[".to_string(),
            "]" => "]".to_string(),
            _ => token.to_string(),
        }
    }
}
