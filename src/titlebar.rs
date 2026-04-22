//! Titlebar strip and sidebar toggle button (platform-aware).

use gpui::{
    div, hsla, prelude::*, px, rems, rgb, svg, AnyElement, Context, MouseButton,
    MouseDownEvent, SharedString, Window,
};

use crate::app::AnotherOneApp;
use crate::git_actions::ToolbarGitAction;
use crate::layout::*;
use crate::resource_indicator::RESOURCE_INDICATOR_BUTTON_W;
use crate::theme;

const TITLEBAR_OPEN_IN_BUTTON_W: f32 = 114.;
const TITLEBAR_OPEN_IN_BUTTON_MARGIN_RIGHT: f32 = 6.;
const TITLEBAR_GIT_ACTIONS_BUTTON_W: f32 = 128.;
const TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT: f32 = 6.;
const TITLEBAR_RIGHT_TOGGLE_SPACE: f32 = 36.;
const TITLEBAR_OPEN_IN_MENU_W: f32 = TITLEBAR_OPEN_IN_BUTTON_W;
const TITLEBAR_GIT_ACTIONS_MENU_W: f32 = 176.;
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
        let enabled_open_in_apps = self.enabled_open_in_apps();
        let has_apps = !enabled_open_in_apps.is_empty();
        let primary_icon = self
            .preferred_open_in_app()
            .map(|app| app.icon_path())
            .unwrap_or("assets/icons/open_in__folder_closed.svg");
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
                    .flex_1()
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
                            .path(primary_icon)
                            .size(px(14.))
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
                    .flex_shrink_0()
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
            + TITLEBAR_GIT_ACTIONS_BUTTON_W
            + TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT
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

    pub fn titlebar_git_actions_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let has_project = self.active_open_in_project_id(cx).is_some();
        if !has_project {
            return div().into_any_element();
        }

        let active = self.active_git_action.is_some();
        let interactive = !active;
        let is_open = self.git_actions_menu_open;
        let button_bg = if is_open {
            gpui::white().opacity(0.10)
        } else {
            gpui::white().opacity(0.05)
        };
        let hover_bg = gpui::white().opacity(0.08);
        let border = gpui::white().opacity(0.08);
        let divider = gpui::white().opacity(0.06);
        let text_col = gpui::white().opacity(0.86);
        let icon_col = gpui::white().opacity(0.92);
        let chevron_col = gpui::white().opacity(0.68);

        div()
            .id("titlebar-git-actions-trigger")
            .flex()
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .w(px(TITLEBAR_GIT_ACTIONS_BUTTON_W))
            .h(px(28.))
            .mr(px(TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT))
            .rounded(px(11.))
            .bg(button_bg)
            .border_1()
            .border_color(border)
            .opacity(if interactive { 1. } else { 0.7 })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .h_full()
                    .px(px(9.))
                    .border_r_1()
                    .border_color(divider)
                    .when(interactive, |d| {
                        d.cursor_pointer()
                            .hover(move |style| style.bg(hover_bg))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.project_page_open_in_menu_project_id = None;
                                    this.git_actions_menu_open = !this.git_actions_menu_open;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__tool-git.svg")
                            .size(px(14.))
                            .text_color(icon_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child("Git actions"),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_shrink_0()
                    .items_center()
                    .justify_center()
                    .w(px(26.))
                    .h_full()
                    .when(interactive, |d| {
                        d.cursor_pointer()
                            .hover(move |style| style.bg(hover_bg))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.project_page_open_in_menu_project_id = None;
                                    this.git_actions_menu_open = !this.git_actions_menu_open;
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__chevron-down.svg")
                            .size(px(11.))
                            .text_color(chevron_col),
                    ),
            )
            .when(active, |button| {
                button.child(
                    div()
                        .absolute()
                        .inset_0()
                        .flex()
                        .items_center()
                        .justify_center()
                        .child(Self::toolbar_spinner(icon_col, 12.)),
                )
            })
            .into_any_element()
    }

    pub fn titlebar_git_actions_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.git_actions_menu_open || self.active_git_action.is_some() {
            return div().id("titlebar-git-actions-overlay").into_any_element();
        }

        if self.active_open_in_project_id(cx).is_none() {
            return div().id("titlebar-git-actions-overlay").into_any_element();
        }

        let has_changes = !self.active_changed_files(cx).is_empty();
        let can_commit = has_changes;
        let toolbar_enabled = self.active_git_action.is_none();
        let bg = rgb(0x2b2d31);
        let border = gpui::white().opacity(0.08);
        let text_col = hsla(0., 0., 0.92, 1.);
        let hover_bg = gpui::white().opacity(0.06);
        let muted_text = hsla(0., 0., 0.48, 1.);
        let danger_col = hsla(0., 0.78, 0.72, 1.);
        let danger_hover = hsla(0., 0.45, 0.34, 0.26);
        let divider = gpui::white().opacity(0.08);
        let push_label = {
            let ahead_count = self
                .workspace_pane
                .read(cx)
                .active_section
                .as_ref()
                .and_then(|section| {
                    self.project_store
                        .branch_view(&section.project_id, &section.branch_name)
                        .as_ref()
                        .map(|branch| branch.ahead_count)
                })
                .unwrap_or(0);
            if ahead_count > 0 {
                SharedString::from(format!("Push ({ahead_count})"))
            } else {
                SharedString::from("Push")
            }
        };

        let menu = div()
            .id("titlebar-git-actions-menu")
            .absolute()
            .right(px(
                TITLEBAR_RIGHT_TOGGLE_SPACE
                    + RESOURCE_INDICATOR_BUTTON_W
                    + TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT,
            ))
            .top(px(TITLEBAR_OPEN_IN_MENU_TOP))
            .w(px(TITLEBAR_GIT_ACTIONS_MENU_W))
            .rounded(px(12.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .shadow_md()
            .occlude()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .child(
                div()
                    .h(px(30.))
                    .px(px(12.))
                    .flex()
                    .items_center()
                    .text_size(rems(11. / 16.))
                    .font_weight(gpui::FontWeight::SEMIBOLD)
                    .text_color(muted_text)
                    .child("Git actions"),
            )
            .child(div().h(px(1.)).mx(px(8.)).bg(divider))
            .child(
                div()
                    .id("titlebar-git-actions-commit")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .px(px(12.))
                    .opacity(if can_commit { 1. } else { 0.55 })
                    .when(can_commit, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Commit changes, staging all files first if needed",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.start_toolbar_git_action(ToolbarGitAction::Commit, cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__git-commit.svg")
                            .size(px(14.))
                            .text_color(text_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child("Commit"),
                    ),
            )
            .child(
                div()
                    .id("titlebar-git-actions-commit-and-push")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .px(px(12.))
                    .opacity(if can_commit { 1. } else { 0.55 })
                    .when(can_commit, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Commit changes and push, staging all files first if needed",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::CommitAndPush,
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__cloud-upload.svg")
                            .size(px(14.))
                            .text_color(text_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child("Commit & Push"),
                    ),
            )
            .child(div().h(px(1.)).mx(px(8.)).bg(divider))
            .child(
                div()
                    .id("titlebar-git-actions-push")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .px(px(12.))
                    .opacity(if toolbar_enabled { 1. } else { 0.55 })
                    .when(toolbar_enabled, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Push the current checked-out branch to its remote",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::Push { force: false },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__cloud-upload.svg")
                            .size(px(14.))
                            .text_color(text_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child(push_label),
                    ),
            )
            .child(
                div()
                    .id("titlebar-git-actions-force-push")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .px(px(12.))
                    .opacity(if toolbar_enabled { 1. } else { 0.55 })
                    .when(toolbar_enabled, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(danger_hover))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Force-push with lease to overwrite the remote branch if needed",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::Push { force: true },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__cloud-upload.svg")
                            .size(px(14.))
                            .text_color(danger_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(danger_col)
                            .child("Force Push"),
                    ),
            )
            .child(div().h(px(1.)).mx(px(8.)).bg(divider))
            .child(
                div()
                    .id("titlebar-git-actions-create-pr")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .px(px(12.))
                    .opacity(if toolbar_enabled { 1. } else { 0.55 })
                    .when(toolbar_enabled, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Create a pull request for the current branch",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::CreatePr { draft: false },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__github.svg")
                            .size(px(14.))
                            .text_color(text_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child("Create PR"),
                    ),
            )
            .child(
                div()
                    .id("titlebar-git-actions-draft-pr")
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .h(px(34.))
                    .px(px(12.))
                    .opacity(if toolbar_enabled { 1. } else { 0.55 })
                    .when(toolbar_enabled, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .tooltip(move |_window, cx| {
                                Self::action_tooltip_view(
                                    "Create a draft pull request for the current branch",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.start_toolbar_git_action(
                                        ToolbarGitAction::CreatePr { draft: true },
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__github.svg")
                            .size(px(14.))
                            .text_color(text_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child("Draft PR"),
                    ),
            );

        div()
            .id("titlebar-git-actions-overlay")
            .absolute()
            .top(px(0.))
            .left(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.git_actions_menu_open = false;
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
            .child(self.titlebar_git_actions_button(cx))
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
