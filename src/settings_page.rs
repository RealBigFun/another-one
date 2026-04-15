//! App-level settings page with a sidebar navigation and content area.

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, Context, MouseButton, MouseDownEvent, Window,
};

use crate::app::ThreeColumnApp;
use crate::layout::TITLEBAR_CHROME_H;

const TEXT_PRIMARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.92, 1.);
const TEXT_SECONDARY: fn() -> gpui::Hsla = || hsla(0., 0., 0.55, 1.);

const SETTINGS_SIDEBAR_W: f32 = 180.;

/// Which settings section is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsSection {
    Agents,
}

impl SettingsSection {
    fn label(self) -> &'static str {
        match self {
            Self::Agents => "Agents",
        }
    }
}

impl ThreeColumnApp {
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
}
