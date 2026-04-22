//! Reusable panel helper and main content assembly.

use gpui::{
    canvas, div, fill, hsla, outline, point, prelude::*, px, rgb, size, svg, App, BorderStyle,
    Bounds, Context, MouseButton, MouseDownEvent, Pixels, Render, ScrollWheelEvent, SharedString,
    Window,
};

use crate::app::{AnotherOneApp, TerminalSelectionRange, WorkspacePane};
use crate::terminal_runtime::{
    TerminalCursorKind, TerminalRuntimeKey, TerminalSurfaceSnapshot, TERMINAL_CELL_WIDTH_RATIO,
    TERMINAL_LINE_HEIGHT_RATIO,
};

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
                            cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                                this.focus_handle.focus(window);
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
    ) -> gpui::Div {
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
            let padding = px(12.);
            let canvas_snapshot = snapshot.clone();
            let selection_key = key.clone();
            let scroll_key = key.clone();
            let selection = self
                .app
                .upgrade()
                .and_then(|app_entity| app_entity.read(cx).terminal_selection_for(&key));
            let font_size = px(self.font_size);
            return div()
                .relative()
                .flex_1()
                .min_h_0()
                .overflow_hidden()
                .bg(terminal_bg)
                .cursor_text()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                        this.focus_handle.focus(window);
                        let _ = this.app.update(cx, |app, app_cx| {
                            app.start_terminal_selection(selection_key.clone(), ev, window, app_cx);
                        });
                    }),
                )
                .on_scroll_wheel(
                    cx.listener(move |this, ev: &ScrollWheelEvent, _window, cx| {
                        let _ = this.app.update(cx, |app, app_cx| {
                            if app.scroll_terminal(&scroll_key, ev.delta, app_cx) {
                                app_cx.stop_propagation();
                            }
                        });
                    }),
                )
                .child(
                    canvas(
                        move |bounds, _, _| bounds,
                        move |bounds, _, window, cx| {
                            paint_terminal_snapshot(
                                bounds,
                                &canvas_snapshot,
                                window,
                                cx,
                                padding,
                                cell_width,
                                line_height,
                                font_size,
                                selection,
                            );
                        },
                    )
                    .absolute()
                    .inset_0(),
                );
        }

        let status_title = if pending {
            "Launching terminal"
        } else if error.is_some() {
            "Terminal launch failed"
        } else {
            "Lazy restore"
        };
        let status_body = if pending {
            "The tab was created immediately and its PTY is launching in the background."
        } else if let Some(error) = error {
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
                        .max_w(px(520.))
                        .p_6()
                        .rounded(px(14.))
                        .bg(panel_bg)
                        .border_1()
                        .border_color(border)
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .text_color(title_col)
                                .child(status_title),
                        )
                        .child(div().text_sm().text_color(body_col).child(error)),
                );
        } else {
            "This restored tab has metadata only. Opening it triggers launch or resume on demand."
        };

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
    }
}

fn paint_terminal_snapshot(
    bounds: Bounds<Pixels>,
    snapshot: &TerminalSurfaceSnapshot,
    window: &mut Window,
    cx: &mut App,
    padding: Pixels,
    cell_width: Pixels,
    cell_height: Pixels,
    font_size: Pixels,
    selection: Option<TerminalSelectionRange>,
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
                std::slice::from_ref(&run.style),
                Some(cell_width),
            )
            .paint(position, cell_height, window, cx);
    }

    let Some(cursor) = &snapshot.cursor else {
        return;
    };

    let left = bounds.origin.x + padding + cell_width * cursor.column as f32;
    let top = bounds.origin.y + padding + cell_height * cursor.line as f32;
    let width = cell_width * cursor.width as f32;
    let rect = Bounds::new(point(left, top), size(width, cell_height));

    match cursor.kind {
        TerminalCursorKind::Block => window.paint_quad(fill(rect, cursor.color)),
        TerminalCursorKind::HollowBlock => {
            window.paint_quad(outline(rect, cursor.color, BorderStyle::default()));
        }
        TerminalCursorKind::Beam => {
            window.paint_quad(fill(
                Bounds::new(point(left, top), size(px(2.), cell_height)),
                cursor.color,
            ));
        }
        TerminalCursorKind::Underline => {
            window.paint_quad(fill(
                Bounds::new(
                    point(left, top + cell_height - px(2.)),
                    size(width.max(px(1.)), px(2.)),
                ),
                cursor.color,
            ));
        }
    }
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
