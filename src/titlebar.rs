//! Titlebar strip and sidebar toggle button (platform-aware).

use gpui::{
    div, prelude::*, px, rems, rgb, svg, AnyElement, Context, MouseButton, MouseDownEvent,
    SharedString, Window,
};

use crate::app::AnotherOneApp;
use crate::layout::*;
use crate::resource_indicator::RESOURCE_INDICATOR_BUTTON_W;
use crate::theme;

const TITLEBAR_OPEN_IN_BUTTON_W: f32 = 114.;
const TITLEBAR_OPEN_IN_BUTTON_MARGIN_RIGHT: f32 = 6.;
const TITLEBAR_RIGHT_TOGGLE_SPACE: f32 = 36.;
const TITLEBAR_OPEN_IN_MENU_W: f32 = 220.;
const TITLEBAR_OPEN_IN_MENU_TOP: f32 = TITLEBAR_CHROME_H + 6.;

impl AnotherOneApp {
    pub fn titlebar_toggle_mouse(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.toggle_sidebar(window, cx);
    }

    pub fn titlebar_right_toggle_mouse(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        cx.stop_propagation();
        self.toggle_right_sidebar(window, cx);
    }

    pub fn sidebar_toggle_svg(window: &Window) -> impl IntoElement {
        let color = theme::toggle_icon_color(window);
        svg()
            .path("assets/sidebar_toggle.svg")
            .size(px(15.))
            .text_color(color)
    }

    pub fn right_sidebar_toggle_svg(window: &Window) -> impl IntoElement {
        let color = theme::toggle_icon_color(window);
        svg()
            .path("assets/right_sidebar_toggle.svg")
            .size(px(15.))
            .text_color(color)
    }

    pub fn titlebar_open_in_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(project_id) = self.active_open_in_project_id(cx) else {
            return div().into_any_element();
        };

        let menu_open =
            self.project_page_open_in_menu_project_id.as_deref() == Some(project_id.as_str());
        let has_apps = !self.enabled_open_in_apps().is_empty();
        let label = "Open In";
        let button_bg = if menu_open {
            gpui::white().opacity(0.10)
        } else {
            gpui::white().opacity(0.05)
        };
        let hover_bg = if has_apps {
            gpui::white().opacity(0.08)
        } else {
            gpui::white().opacity(0.06)
        };
        let project_id_for_chevron = project_id.clone();

        div()
            .id(SharedString::from(format!("titlebar-open-in-trigger-{project_id}")))
            .flex()
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .w(px(TITLEBAR_OPEN_IN_BUTTON_W))
            .h(px(28.))
            .mr(px(TITLEBAR_OPEN_IN_BUTTON_MARGIN_RIGHT))
            .rounded(px(11.))
            .bg(button_bg)
            .border_1()
            .border_color(gpui::white().opacity(0.08))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .h_full()
                    .px(px(9.))
                    .border_r_1()
                    .border_color(gpui::white().opacity(0.06))
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.open_active_directory_in_default_app(cx);
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__folder-open.svg")
                            .size(px(12.))
                            .text_color(gpui::white().opacity(0.92)),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(gpui::white().opacity(0.86))
                            .child(label),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(26.))
                    .h_full()
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.toggle_project_page_open_in_menu(&project_id_for_chevron, cx);
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__chevron-down.svg")
                            .size(px(11.))
                            .text_color(gpui::white().opacity(0.68)),
                    ),
            )
            .into_any_element()
    }

    pub fn titlebar_open_in_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(project_id) = self.project_page_open_in_menu_project_id.clone() else {
            return div().id("titlebar-open-in-overlay").into_any_element();
        };

        let enabled_open_in_apps = self.enabled_open_in_apps();
        if enabled_open_in_apps.is_empty() {
            return div().id("titlebar-open-in-overlay").into_any_element();
        }

        let overlay_right = TITLEBAR_RIGHT_TOGGLE_SPACE
            + RESOURCE_INDICATOR_BUTTON_W
            + TITLEBAR_OPEN_IN_BUTTON_MARGIN_RIGHT;

        let mut menu = div()
            .id("titlebar-open-in-menu")
            .absolute()
            .right(px(overlay_right))
            .top(px(TITLEBAR_OPEN_IN_MENU_TOP))
            .w(px(TITLEBAR_OPEN_IN_MENU_W))
            .rounded(px(12.))
            .bg(rgb(0x2b2d31))
            .border_1()
            .border_color(gpui::white().opacity(0.08))
            .shadow_md()
            .occlude()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        for app in enabled_open_in_apps {
            let project_id_for_open = project_id.clone();

            menu = menu.child(
                div()
                    .id(SharedString::from(format!(
                        "titlebar-open-in-{project_id}-{}",
                        app.id()
                    )))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .h(px(38.))
                    .px(px(12.))
                    .cursor_pointer()
                    .hover(|style| style.bg(gpui::white().opacity(0.06)))
                    .tooltip(move |_window, cx| Self::action_tooltip_view(app.description(), cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.open_project_directory_in_app(&project_id_for_open, app, cx);
                        }),
                    )
                    .child(
                        svg()
                            .path(app.icon_path())
                            .size(px(16.))
                            .text_color(gpui::white().opacity(0.92)),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(gpui::white().opacity(0.92))
                            .child(app.label()),
                    ),
            );
        }

        div()
            .id("titlebar-open-in-overlay")
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.project_page_open_in_menu_project_id = None;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(menu)
            .into_any_element()
    }

    #[cfg(target_os = "macos")]
    pub fn mac_title_strip(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        busy: bool,
    ) -> impl IntoElement {
        let chrome = theme::chrome_bg(window);
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(TITLEBAR_CHROME_H))
            .flex_shrink_0()
            .bg(chrome)
            .border_b_1()
            .border_color(rgb(0x27292e))
            .child(div().w(px(TRAFFIC_LIGHT_PAD)).flex_shrink_0())
            .child(
                div()
                    .id("sidebar-toggle-titlebar")
                    .ml(px(TOGGLE_LEFT_MARGIN))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(1.))
                    .rounded_md()
                    .when(!busy, |d| {
                        d.cursor_pointer()
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view("Show or hide the projects sidebar", cx)
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::titlebar_toggle_mouse),
                            )
                            .hover(|s| s.bg(gpui::white().opacity(0.06)))
                    })
                    .when(busy, |d| d.opacity(0.45))
                    .child(Self::sidebar_toggle_svg(window)),
            )
            .child(
                div()
                    .flex_1()
                    .h_full()
                    .on_mouse_down(MouseButton::Left, |ev, window, _cx| {
                        if ev.click_count == 2 {
                            window.titlebar_double_click();
                        } else {
                            window.start_window_move();
                        }
                    }),
            )
            .child(self.titlebar_open_in_button(cx))
            .child(self.resource_indicator_button(window, cx))
            .child(
                div()
                    .id("right-sidebar-toggle-titlebar")
                    .mr(px(8.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(1.))
                    .rounded_md()
                    .when(!busy, |d| {
                        d.cursor_pointer()
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Show or hide the changed files sidebar",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(Self::titlebar_right_toggle_mouse),
                            )
                            .hover(|s| s.bg(gpui::white().opacity(0.06)))
                    })
                    .when(busy, |d| d.opacity(0.45))
                    .child(Self::right_sidebar_toggle_svg(window)),
            )
    }
}
