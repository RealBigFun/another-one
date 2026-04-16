//! Titlebar strip and sidebar toggle button (platform-aware).

use gpui::{div, prelude::*, px, rgb, svg, Context, MouseButton, MouseDownEvent, Window};

use crate::app::AnotherOneApp;
use crate::layout::*;
use crate::theme;

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

    #[cfg(target_os = "macos")]
    pub fn mac_title_strip(
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
            .child(
                div()
                    .id("right-sidebar-toggle-titlebar")
                    .mr(px(12.))
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
