//! "Create Branch" modal shown from the titlebar Git Actions menu.

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, ClipboardItem, Context, KeyDownEvent, MouseButton,
    MouseDownEvent, SharedString,
};

use crate::app::AnotherOneApp;

#[derive(Clone)]
pub(crate) struct CreateBranchModalState {
    pub branch_name: String,
    pub generated_branch_name: String,
    pub branch_name_cursor: usize,
    pub branch_name_selection_anchor: Option<usize>,
    pub use_current_task: bool,
    pub migrate_changes: bool,
    pub submitting: bool,
}

#[derive(Clone, Copy)]
enum CursorDirection {
    Left,
    Right,
}

const CARD_BG: u32 = 0x2b2d31;

fn title_col() -> gpui::Hsla {
    hsla(0., 0., 0.92, 1.)
}

fn body_col() -> gpui::Hsla {
    hsla(0., 0., 0.78, 1.)
}

fn muted_col() -> gpui::Hsla {
    hsla(0., 0., 0.58, 1.)
}

fn placeholder_col() -> gpui::Hsla {
    hsla(0., 0., 0.38, 1.)
}

fn border_col() -> gpui::Hsla {
    gpui::white().opacity(0.08)
}

fn hover_bg() -> gpui::Hsla {
    gpui::white().opacity(0.06)
}

fn selected_branch_name_range(state: &CreateBranchModalState) -> Option<std::ops::Range<usize>> {
    let anchor = state.branch_name_selection_anchor?;
    let cursor = state.branch_name_cursor;
    (anchor != cursor).then_some(anchor.min(cursor)..anchor.max(cursor))
}

fn previous_boundary(text: &str, cursor: usize) -> usize {
    text[..cursor.min(text.len())]
        .char_indices()
        .map(|(index, _)| index)
        .next_back()
        .unwrap_or(0)
}

fn next_boundary(text: &str, cursor: usize) -> usize {
    text[cursor.min(text.len())..]
        .char_indices()
        .nth(1)
        .map(|(index, _)| cursor + index)
        .unwrap_or(text.len())
}

pub(crate) fn replace_create_branch_name_range(
    state: &mut CreateBranchModalState,
    range: std::ops::Range<usize>,
    new_text: &str,
) {
    state.branch_name.replace_range(range.clone(), new_text);
    state.branch_name_cursor = range.start + new_text.len();
    state.branch_name_selection_anchor = None;
}

pub(crate) fn insert_create_branch_name_text(state: &mut CreateBranchModalState, text: &str) {
    let range = selected_branch_name_range(state)
        .unwrap_or(state.branch_name_cursor..state.branch_name_cursor);
    replace_create_branch_name_range(state, range, &sanitize_create_branch_input(text));
}

pub(crate) fn delete_backward_in_create_branch_name(state: &mut CreateBranchModalState) {
    if let Some(range) = selected_branch_name_range(state) {
        replace_create_branch_name_range(state, range, "");
    } else if state.branch_name_cursor > 0 {
        let start = previous_boundary(&state.branch_name, state.branch_name_cursor);
        replace_create_branch_name_range(state, start..state.branch_name_cursor, "");
    }
}

pub(crate) fn delete_forward_in_create_branch_name(state: &mut CreateBranchModalState) {
    if let Some(range) = selected_branch_name_range(state) {
        replace_create_branch_name_range(state, range, "");
    } else if state.branch_name_cursor < state.branch_name.len() {
        let end = next_boundary(&state.branch_name, state.branch_name_cursor);
        replace_create_branch_name_range(state, state.branch_name_cursor..end, "");
    }
}

fn move_cursor(
    state: &mut CreateBranchModalState,
    direction: CursorDirection,
    extend_selection: bool,
) {
    let next_cursor = match direction {
        CursorDirection::Left => selected_branch_name_range(state)
            .filter(|_| !extend_selection)
            .map(|range| range.start)
            .unwrap_or_else(|| previous_boundary(&state.branch_name, state.branch_name_cursor)),
        CursorDirection::Right => selected_branch_name_range(state)
            .filter(|_| !extend_selection)
            .map(|range| range.end)
            .unwrap_or_else(|| next_boundary(&state.branch_name, state.branch_name_cursor)),
    };
    if extend_selection {
        state
            .branch_name_selection_anchor
            .get_or_insert(state.branch_name_cursor);
    } else {
        state.branch_name_selection_anchor = None;
    }
    state.branch_name_cursor = next_cursor;
}

fn sanitize_create_branch_input(text: &str) -> String {
    text.replace(['\n', '\r', '\t'], " ")
}

impl AnotherOneApp {
    pub(crate) fn open_create_branch_modal(&mut self, cx: &mut Context<Self>) {
        self.create_branch_modal = Some(CreateBranchModalState {
            branch_name: String::new(),
            generated_branch_name: crate::new_task_modal::generate_task_name(),
            branch_name_cursor: 0,
            branch_name_selection_anchor: None,
            use_current_task: false,
            migrate_changes: false,
            submitting: false,
        });
        cx.notify();
    }

    pub(crate) fn dismiss_create_branch_modal(&mut self, cx: &mut Context<Self>) {
        if self
            .create_branch_modal
            .as_ref()
            .is_some_and(|state| state.submitting)
        {
            return;
        }
        self.create_branch_modal = None;
        cx.notify();
    }

    pub(crate) fn create_branch_modal_overlay(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(state) = self.create_branch_modal.as_ref() else {
            return div().id("create-branch-modal-overlay");
        };

        let branch_name: SharedString = state.branch_name.clone().into();
        let cursor = state.branch_name_cursor;
        let selection = selected_branch_name_range(state);
        let use_current_task = state.use_current_task;
        let migrate_changes = state.migrate_changes || use_current_task;
        let submitting = state.submitting;
        let branch_source = if state.branch_name.trim().is_empty() {
            state.generated_branch_name.as_str()
        } else {
            state.branch_name.as_str()
        };
        let branch_slug = another_one_core::project_store::slugify_branch_name(branch_source);

        div()
            .id("create-branch-modal-overlay")
            .absolute()
            .inset_0()
            .flex()
            .items_center()
            .justify_center()
            .bg(hsla(0., 0., 0., 0.50))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.dismiss_create_branch_modal(cx);
                    cx.stop_propagation();
                }),
            )
            .on_scroll_wheel(|_, _, cx| cx.stop_propagation())
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, _window, cx| {
                this.handle_create_branch_modal_key_down(ev, cx);
            }))
            .child(
                div()
                    .w(px(420.))
                    .rounded_lg()
                    .bg(rgb(CARD_BG))
                    .border_1()
                    .border_color(border_col())
                    .shadow_lg()
                    .overflow_hidden()
                    .flex()
                    .flex_col()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(Self::render_create_branch_header(submitting, cx))
                    .child(
                        div()
                            .px(px(20.))
                            .py(px(14.))
                            .flex()
                            .flex_col()
                            .gap(px(14.))
                            .child(Self::render_create_branch_input(
                                branch_name,
                                cursor,
                                selection,
                                submitting,
                                cx,
                            ))
                            .child(
                                div()
                                    .text_size(rems(12. / 16.))
                                    .text_color(muted_col())
                                    .child(format!("Branch: {branch_slug}")),
                            )
                            .child(Self::render_create_branch_toggle(
                                "create-branch-use-current-task",
                                "Use current task",
                                use_current_task,
                                submitting,
                                false,
                                |state| {
                                    state.use_current_task = !state.use_current_task;
                                    if state.use_current_task {
                                        state.migrate_changes = true;
                                    }
                                },
                                cx,
                            ))
                            .child(Self::render_create_branch_toggle(
                                "create-branch-migrate-changes",
                                "Migrate changes to new branch",
                                migrate_changes,
                                submitting || use_current_task,
                                use_current_task,
                                |state| state.migrate_changes = !state.migrate_changes,
                                cx,
                            )),
                    )
                    .child(Self::render_create_branch_footer(submitting, cx)),
            )
    }

    fn render_create_branch_header(submitting: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .items_start()
            .justify_between()
            .px(px(20.))
            .pt(px(20.))
            .pb(px(10.))
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
                            .child("Create Branch"),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .text_color(muted_col())
                            .child("Create a branch here or in a separate worktree."),
                    ),
            )
            .child(
                div()
                    .id("create-branch-close")
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(24.))
                    .h(px(24.))
                    .rounded_md()
                    .opacity(if submitting { 0.45 } else { 1.0 })
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .tooltip(move |_window, cx| {
                        Self::action_tooltip_view("Close the create branch modal", cx)
                    })
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.dismiss_create_branch_modal(cx);
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

    fn render_create_branch_input(
        branch_name: SharedString,
        cursor: usize,
        selection: Option<std::ops::Range<usize>>,
        submitting: bool,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let is_placeholder = branch_name.is_empty();
        let display = if is_placeholder {
            "Branch name".into()
        } else {
            branch_name
        };
        div()
            .id("create-branch-name-input")
            .h(px(38.))
            .px(px(10.))
            .flex()
            .items_center()
            .rounded_md()
            .border_1()
            .border_color(gpui::white().opacity(0.12))
            .bg(gpui::black().opacity(0.14))
            .opacity(if submitting { 0.55 } else { 1.0 })
            .cursor_text()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    if let Some(state) = this.create_branch_modal.as_mut() {
                        state.branch_name_cursor = state.branch_name.len();
                        state.branch_name_selection_anchor = None;
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(render_inline_text_input(
                display,
                cursor,
                selection,
                is_placeholder,
                cx,
            ))
    }

    fn render_create_branch_toggle(
        id: &'static str,
        label: &'static str,
        checked: bool,
        submitting: bool,
        disabled: bool,
        toggle: fn(&mut CreateBranchModalState),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div()
            .id(id)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .opacity(if submitting || disabled { 0.55 } else { 1.0 })
            .when(!submitting && !disabled, |row| {
                row.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            if let Some(state) = this.create_branch_modal.as_mut() {
                                toggle(state);
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
            })
            .child(
                div()
                    .text_size(rems(13. / 16.))
                    .font_weight(gpui::FontWeight::MEDIUM)
                    .text_color(body_col())
                    .child(label),
            )
            .child(
                div()
                    .w(px(34.))
                    .h(px(20.))
                    .rounded_full()
                    .bg(if checked {
                        hsla(0.58, 0.62, 0.48, 1.)
                    } else {
                        gpui::white().opacity(0.12)
                    })
                    .flex()
                    .items_center()
                    .when(checked, |d| d.justify_end())
                    .when(!checked, |d| d.justify_start())
                    .p(px(2.))
                    .child(div().size(px(16.)).rounded_full().bg(gpui::white())),
            )
    }

    fn render_create_branch_footer(submitting: bool, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .flex()
            .justify_end()
            .gap(px(8.))
            .px(px(20.))
            .py(px(16.))
            .border_t_1()
            .border_color(border_col())
            .child(
                div()
                    .id("create-branch-cancel")
                    .px(px(12.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .rounded_md()
                    .text_size(rems(12. / 16.))
                    .text_color(body_col())
                    .opacity(if submitting { 0.45 } else { 1.0 })
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg()))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.dismiss_create_branch_modal(cx);
                            cx.stop_propagation();
                        }),
                    )
                    .child("Cancel"),
            )
            .child(
                div()
                    .id("create-branch-submit")
                    .px(px(12.))
                    .h(px(32.))
                    .flex()
                    .items_center()
                    .rounded_md()
                    .text_size(rems(12. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(gpui::white())
                    .bg(hsla(0.58, 0.62, 0.48, 1.))
                    .opacity(if submitting { 0.60 } else { 1.0 })
                    .cursor_pointer()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                            this.submit_create_branch_modal(cx);
                            cx.stop_propagation();
                        }),
                    )
                    .child(if submitting {
                        "Creating..."
                    } else {
                        "Create Branch"
                    }),
            )
    }

    pub(crate) fn handle_create_branch_modal_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) {
        if self.create_branch_modal.is_none() {
            return;
        }
        cx.stop_propagation();
        if self
            .create_branch_modal
            .as_ref()
            .is_some_and(|state| state.submitting)
        {
            return;
        }

        match ev.keystroke.key.as_str() {
            "escape" => self.dismiss_create_branch_modal(cx),
            "enter" => self.submit_create_branch_modal(cx),
            _ => {
                self.handle_create_branch_name_key_down(ev, cx);
            }
        }
    }

    fn handle_create_branch_name_key_down(
        &mut self,
        ev: &KeyDownEvent,
        cx: &mut Context<Self>,
    ) -> bool {
        let Some(state) = self.create_branch_modal.as_mut() else {
            return false;
        };
        let modifiers = ev.keystroke.modifiers;
        match ev.keystroke.key.as_str() {
            "backspace" => delete_backward_in_create_branch_name(state),
            "delete" => delete_forward_in_create_branch_name(state),
            "left" => move_cursor(state, CursorDirection::Left, modifiers.shift),
            "right" => move_cursor(state, CursorDirection::Right, modifiers.shift),
            "home" => {
                if modifiers.shift {
                    state
                        .branch_name_selection_anchor
                        .get_or_insert(state.branch_name_cursor);
                } else {
                    state.branch_name_selection_anchor = None;
                }
                state.branch_name_cursor = 0;
            }
            "end" => {
                if modifiers.shift {
                    state
                        .branch_name_selection_anchor
                        .get_or_insert(state.branch_name_cursor);
                } else {
                    state.branch_name_selection_anchor = None;
                }
                state.branch_name_cursor = state.branch_name.len();
            }
            _ => {
                if modifiers.platform && ev.keystroke.key.as_str() == "a" {
                    state.branch_name_cursor = state.branch_name.len();
                    state.branch_name_selection_anchor = Some(0);
                } else if modifiers.platform && ev.keystroke.key.as_str() == "c" {
                    if let Some(range) = selected_branch_name_range(state) {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            state.branch_name[range].to_string(),
                        ));
                    }
                } else if modifiers.platform && ev.keystroke.key.as_str() == "x" {
                    if let Some(range) = selected_branch_name_range(state) {
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            state.branch_name[range.clone()].to_string(),
                        ));
                        replace_create_branch_name_range(state, range, "");
                    }
                } else if modifiers.platform && ev.keystroke.key.as_str() == "v" {
                    if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
                        insert_create_branch_name_text(state, &text);
                    }
                } else if modifiers.control || modifiers.platform || modifiers.function {
                    return false;
                } else if let Some(key_char) = ev.keystroke.key_char.as_deref() {
                    insert_create_branch_name_text(state, key_char);
                } else {
                    return false;
                }
            }
        }
        cx.notify();
        true
    }
}

fn render_inline_text_input(
    text: SharedString,
    cursor: usize,
    selection: Option<std::ops::Range<usize>>,
    placeholder: bool,
    _cx: &mut Context<AnotherOneApp>,
) -> impl IntoElement {
    let mut row = div().flex().items_center().min_w_0();
    let text = text.to_string();
    let selection = selection.unwrap_or(cursor..cursor);
    for (char_index, ch) in text
        .char_indices()
        .chain(std::iter::once((text.len(), '\0')))
    {
        if char_index == cursor {
            row = row.child(div().w(px(1.)).h(px(16.)).bg(body_col()));
        }
        if ch == '\0' {
            break;
        }
        let next = char_index + ch.len_utf8();
        let selected = selection.start <= char_index && next <= selection.end;
        let label: SharedString = ch.to_string().into();
        row = row.child(
            div()
                .bg(if selected {
                    hsla(0.58, 0.62, 0.48, 0.35)
                } else {
                    gpui::black().opacity(0.0)
                })
                .text_color(if placeholder {
                    placeholder_col()
                } else {
                    body_col()
                })
                .text_size(rems(13. / 16.))
                .child(label),
        );
    }
    row
}

#[cfg(test)]
mod tests {
    use super::*;

    fn state(text: &str) -> CreateBranchModalState {
        CreateBranchModalState {
            branch_name: text.to_string(),
            generated_branch_name: "random-branch".to_string(),
            branch_name_cursor: text.len(),
            branch_name_selection_anchor: None,
            use_current_task: false,
            migrate_changes: false,
            submitting: false,
        }
    }

    #[test]
    fn insert_replaces_selection() {
        let mut state = state("feature-old");
        state.branch_name_cursor = "feature".len();
        state.branch_name_selection_anchor = Some(state.branch_name.len());
        insert_create_branch_name_text(&mut state, "new");
        assert_eq!(state.branch_name, "featurenew");
        assert_eq!(state.branch_name_selection_anchor, None);
    }

    #[test]
    fn backspace_and_delete_remove_chars() {
        let mut state = state("abc");
        delete_backward_in_create_branch_name(&mut state);
        assert_eq!(state.branch_name, "ab");
        state.branch_name_cursor = 0;
        delete_forward_in_create_branch_name(&mut state);
        assert_eq!(state.branch_name, "b");
    }

    #[test]
    fn paste_sanitizes_newlines_and_tabs() {
        let mut state = state("");
        insert_create_branch_name_text(&mut state, "a\nb\tc");
        assert_eq!(state.branch_name, "a b c");
    }
}
