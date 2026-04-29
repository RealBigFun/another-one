use std::path::Path;

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, AnyElement, Context, MouseButton, MouseDownEvent,
    SharedString, Window,
};

use crate::app::{AnotherOneApp, GitDiffPaneState, WorkspacePane};
use crate::project_store::{
    DiffFile, DiffRow, DiffRowKind, GitDiff, GitDiffSelection, GitDiffSource,
};

impl WorkspacePane {
    pub(crate) fn close_git_diff_pane(&mut self, cx: &mut Context<Self>) {
        self.active_git_diff = None;
        let app = self.app.clone();
        cx.defer(move |cx| {
            let _ = app.update(cx, |app, app_cx| {
                app.clear_changed_file_diff_state(app_cx);
            });
        });
        cx.notify();
    }

    pub(crate) fn render_git_diff_pane(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let selection = self.active_git_diff.clone();
        let state = self
            .app
            .upgrade()
            .and_then(|app| app.read(cx).git_diff_state.clone());
        let bg = rgb(0x1e1f22);
        let border = gpui::white().opacity(0.07);
        let title_col = hsla(0., 0., 0.92, 1.);
        let muted_col = hsla(0., 0., 0.58, 1.);

        let Some(selection) = selection else {
            return div().size_full().bg(bg);
        };

        let mut panel = div()
            .size_full()
            .flex()
            .flex_col()
            .bg(bg)
            .overflow_hidden()
            .child(self.git_diff_header(&selection, title_col, muted_col, border, cx));

        panel = match state {
            Some(GitDiffPaneState::Loading) | None => {
                panel.child(Self::git_diff_center_message("Loading diff...", muted_col))
            }
            Some(GitDiffPaneState::Failed(error)) => panel.child(Self::git_diff_failed(error)),
            Some(GitDiffPaneState::Loaded(diff)) => panel.child(Self::git_diff_body(&diff)),
        };

        panel
    }

    fn git_diff_header(
        &self,
        selection: &GitDiffSelection,
        title_col: gpui::Hsla,
        muted_col: gpui::Hsla,
        border: gpui::Hsla,
        cx: &mut Context<Self>,
    ) -> gpui::Div {
        let source_label = match selection.source {
            GitDiffSource::Staged => "Staged",
            GitDiffSource::Unstaged => "Unstaged",
        };
        let status_color = match selection.status {
            'A' => hsla(135. / 360., 0.70, 0.68, 1.),
            'D' => hsla(0., 0.72, 0.68, 1.),
            'R' | 'C' => hsla(210. / 360., 0.72, 0.72, 1.),
            _ => hsla(50. / 360., 0.90, 0.60, 1.),
        };
        let file_name = Path::new(&selection.path)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(selection.path.as_str())
            .to_string();
        let source_detail = format!("{source_label} - {}", selection.path);

        div()
            .h(px(52.))
            .flex_shrink_0()
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .px(px(14.))
            .border_b_1()
            .border_color(border)
            .bg(rgb(0x24262b))
            .child(
                div()
                    .w(px(620.))
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .child(
                        div()
                            .min_w(px(18.))
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::BOLD)
                            .text_color(status_color)
                            .child(selection.status.to_string()),
                    )
                    .child(
                        div()
                            .w(px(560.))
                            .flex_shrink_0()
                            .flex()
                            .flex_col()
                            .gap(px(2.))
                            .child(
                                div()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_size(rems(13. / 16.))
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_col)
                                    .child(file_name),
                            )
                            .child(
                                div()
                                    .whitespace_nowrap()
                                    .overflow_hidden()
                                    .text_size(rems(11. / 16.))
                                    .text_color(muted_col)
                                    .child(source_detail),
                            ),
                    ),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap(px(10.))
                    .flex_shrink_0()
                    .child(Self::git_diff_stat(selection.additions, true))
                    .child(Self::git_diff_stat(selection.deletions, false))
                    .child(
                        div()
                            .id("git-diff-close")
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(28.))
                            .h(px(28.))
                            .rounded(px(7.))
                            .cursor_pointer()
                            .hover(|style| style.bg(gpui::white().opacity(0.08)))
                            .tooltip(move |_window, cx| {
                                AnotherOneApp::action_tooltip_view("Close diff", cx)
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.close_git_diff_pane(cx);
                                    cx.stop_propagation();
                                }),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__close.svg")
                                    .size(px(13.))
                                    .text_color(muted_col),
                            ),
                    ),
            )
    }

    fn git_diff_stat(value: i32, positive: bool) -> gpui::Div {
        let color = if positive {
            hsla(138. / 360., 0.50, 0.74, 1.)
        } else {
            hsla(352. / 360., 0.52, 0.76, 1.)
        };
        let prefix = if positive { "+" } else { "-" };

        div()
            .text_size(rems(12. / 16.))
            .font_weight(gpui::FontWeight::SEMIBOLD)
            .text_color(color)
            .child(format!("{prefix}{}", value.max(0)))
    }

    fn git_diff_body(diff: &GitDiff) -> AnyElement {
        if diff.files.is_empty() {
            return Self::git_diff_center_message(
                "No diff available for this file.",
                hsla(0., 0., 0.58, 1.),
            )
            .into_any_element();
        }

        let mut body = div()
            .id("git-diff-body")
            .flex_1()
            .min_h_0()
            .overflow_scroll()
            .font_family("Lilex Nerd Font Mono")
            .text_size(rems(12. / 16.))
            .line_height(rems(18. / 16.))
            .bg(rgb(0x1e1f22));

        for file in &diff.files {
            body = body.child(Self::git_diff_file(file));
        }

        body.into_any_element()
    }

    fn git_diff_file(file: &DiffFile) -> gpui::Div {
        let mut content = div().min_w(px(760.)).flex().flex_col();

        if file.binary || file.hunks.is_empty() {
            let message = file
                .message
                .clone()
                .unwrap_or_else(|| "This file has no text hunks to display.".to_string());
            return content.child(Self::git_diff_center_message(
                message,
                hsla(0., 0., 0.58, 1.),
            ));
        }

        content = content.child(Self::git_diff_column_header(file));
        for hunk in &file.hunks {
            content = content.child(Self::git_diff_hunk_header(&hunk.header));
            for row in &hunk.rows {
                content = content.child(Self::git_diff_split_row(row));
            }
        }

        content
    }

    fn git_diff_column_header(file: &DiffFile) -> gpui::Div {
        let old_label = file.old_path.as_deref().unwrap_or("/dev/null");
        let new_label = file.new_path.as_deref().unwrap_or("/dev/null");

        div()
            .min_h(px(28.))
            .flex()
            .border_b_1()
            .border_color(gpui::white().opacity(0.06))
            .bg(rgb(0x24262b))
            .text_color(hsla(0., 0., 0.62, 1.))
            .child(Self::git_diff_header_cell(old_label))
            .child(Self::git_diff_header_cell(new_label))
    }

    fn git_diff_header_cell(label: &str) -> gpui::Div {
        div()
            .w_1_2()
            .min_w(px(0.))
            .px(px(12.))
            .py(px(6.))
            .truncate()
            .child(label.to_string())
    }

    fn git_diff_hunk_header(header: &str) -> gpui::Div {
        let cell = |label: SharedString| {
            div()
                .w_1_2()
                .min_w(px(0.))
                .px(px(12.))
                .py(px(4.))
                .text_color(hsla(210. / 360., 0.35, 0.70, 1.))
                .bg(hsla(215. / 360., 0.20, 0.18, 1.))
                .child(label)
        };

        div()
            .flex()
            .border_b_1()
            .border_color(gpui::white().opacity(0.04))
            .child(cell(SharedString::from(header.to_string())))
            .child(cell(SharedString::from(header.to_string())))
    }

    fn git_diff_split_row(row: &DiffRow) -> gpui::Div {
        let deleted_bg = hsla(352. / 360., 0.32, 0.18, 1.);
        let added_bg = hsla(138. / 360., 0.28, 0.17, 1.);
        let blank_bg = gpui::black().opacity(0.10);

        match row.kind {
            DiffRowKind::Context => div()
                .flex()
                .child(Self::git_diff_code_cell(row.old_line, &row.content, None))
                .child(Self::git_diff_code_cell(row.new_line, &row.content, None)),
            DiffRowKind::Deleted => div()
                .flex()
                .child(Self::git_diff_code_cell(
                    row.old_line,
                    &format!("-{}", row.content),
                    Some(deleted_bg),
                ))
                .child(Self::git_diff_code_cell(None, "", Some(blank_bg))),
            DiffRowKind::Added => div()
                .flex()
                .child(Self::git_diff_code_cell(None, "", Some(blank_bg)))
                .child(Self::git_diff_code_cell(
                    row.new_line,
                    &format!("+{}", row.content),
                    Some(added_bg),
                )),
        }
    }

    fn git_diff_code_cell(
        line_number: Option<usize>,
        content: &str,
        bg: Option<gpui::Hsla>,
    ) -> gpui::Div {
        let line_col = hsla(0., 0., 0.42, 1.);
        let text_col = hsla(0., 0., 0.84, 1.);

        div()
            .w_1_2()
            .min_w(px(0.))
            .flex()
            .border_b_1()
            .border_color(gpui::white().opacity(0.025))
            .when_some(bg, |cell, bg| cell.bg(bg))
            .child(
                div()
                    .w(px(56.))
                    .flex_shrink_0()
                    .pr(px(10.))
                    .py(px(1.))
                    .text_right()
                    .text_color(line_col)
                    .child(line_number.map(|n| n.to_string()).unwrap_or_default()),
            )
            .child(
                div()
                    .min_w(px(0.))
                    .flex_1()
                    .py(px(1.))
                    .pr(px(12.))
                    .whitespace_nowrap()
                    .text_color(text_col)
                    .child(if content.is_empty() {
                        " ".to_string()
                    } else {
                        content.to_string()
                    }),
            )
    }

    fn git_diff_center_message(message: impl Into<SharedString>, color: gpui::Hsla) -> gpui::Div {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .text_sm()
            .text_color(color)
            .child(message.into())
    }

    fn git_diff_failed(error: String) -> gpui::Div {
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .items_center()
            .justify_center()
            .p_6()
            .child(
                div()
                    .max_w(px(680.))
                    .rounded(px(8.))
                    .border_1()
                    .border_color(gpui::white().opacity(0.08))
                    .bg(rgb(0x25282d))
                    .p(px(16.))
                    .text_sm()
                    .text_color(hsla(0., 0., 0.72, 1.))
                    .child(format!("Could not load this diff. {error}")),
            )
    }
}
