//! Titlebar strip and sidebar toggle button (platform-aware).

use gpui::{
    div, hsla, prelude::*, px, rems, svg, AnyElement, App, Context, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, SharedString, Window, WindowControlArea,
};

use crate::app::AnotherOneApp;
use crate::git_actions::ToolbarGitAction;
use crate::layout::*;
use crate::platform::PlatformServices;
use crate::project_store::{ProjectAction, ProjectActionScope, RepoDefaultCommitAction};
use crate::resource_indicator::RESOURCE_INDICATOR_BUTTON_W;
use crate::theme;
use crate::tokens;

const TITLEBAR_OPEN_IN_BUTTON_W: f32 = 114.;
const TITLEBAR_OPEN_IN_BUTTON_MARGIN_RIGHT: f32 = 6.;
const TITLEBAR_GITHUB_BUTTON_W: f32 = 32.;
const TITLEBAR_GITHUB_BUTTON_MARGIN_RIGHT: f32 = 6.;
const TITLEBAR_PULL_REQUEST_BUTTON_W: f32 = 32.;
const TITLEBAR_PULL_REQUEST_BUTTON_MARGIN_RIGHT: f32 = 6.;
const TITLEBAR_CUSTOM_ACTIONS_BUTTON_W: f32 = 148.;
const TITLEBAR_CUSTOM_ACTIONS_BUTTON_MARGIN_RIGHT: f32 = 6.;
const TITLEBAR_GIT_ACTIONS_BUTTON_W: f32 = 156.;
const TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT: f32 = 6.;
const TITLEBAR_RIGHT_TOGGLE_SPACE: f32 = 36.;
const TITLEBAR_OPEN_IN_MENU_W: f32 = TITLEBAR_OPEN_IN_BUTTON_W;
const TITLEBAR_CUSTOM_ACTIONS_MENU_W: f32 = 260.;
const TITLEBAR_GIT_ACTIONS_MENU_W: f32 = 188.;
const TITLEBAR_MENU_OFFSET_TOP: f32 = 6.;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TitlebarPrimaryGitAction {
    Commit,
    CommitAndPush,
    Push { ahead_count: usize },
}

impl TitlebarPrimaryGitAction {
    fn toolbar_action(self) -> ToolbarGitAction {
        match self {
            Self::Commit => ToolbarGitAction::Commit,
            Self::CommitAndPush => ToolbarGitAction::CommitAndPush,
            Self::Push { .. } => ToolbarGitAction::Push { force: false },
        }
    }

    fn icon_path(self) -> &'static str {
        match self {
            Self::Commit => "assets/icons/icons__git-commit.svg",
            Self::CommitAndPush | Self::Push { .. } => "assets/icons/icons__cloud-upload.svg",
        }
    }

    fn label(self) -> SharedString {
        match self {
            Self::Commit => SharedString::from("Commit"),
            Self::CommitAndPush => SharedString::from("Commit & Push"),
            Self::Push { ahead_count } => count_git_action_label("Push", ahead_count),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct ActiveToolbarGitActionPresentation {
    label: &'static str,
    icon_path: &'static str,
    danger: bool,
}

fn count_git_action_label(action: &'static str, count: usize) -> SharedString {
    if count > 0 {
        SharedString::from(format!("{action} ({count})"))
    } else {
        SharedString::from(action)
    }
}

fn resolve_idle_primary_git_action(
    has_local_changes: bool,
    preferred_commit_action: Option<RepoDefaultCommitAction>,
    ahead_count: usize,
) -> TitlebarPrimaryGitAction {
    if has_local_changes {
        match preferred_commit_action {
            Some(RepoDefaultCommitAction::CommitAndPush) => TitlebarPrimaryGitAction::CommitAndPush,
            Some(RepoDefaultCommitAction::Commit) | None => TitlebarPrimaryGitAction::Commit,
        }
    } else {
        TitlebarPrimaryGitAction::Push { ahead_count }
    }
}

fn resolve_active_git_action_presentation(
    action: ToolbarGitAction,
) -> ActiveToolbarGitActionPresentation {
    match action {
        ToolbarGitAction::Commit => ActiveToolbarGitActionPresentation {
            label: "Committing...",
            icon_path: "assets/icons/icons__git-commit.svg",
            danger: false,
        },
        ToolbarGitAction::CommitAndPush => ActiveToolbarGitActionPresentation {
            label: "Committing & Pushing...",
            icon_path: "assets/icons/icons__cloud-upload.svg",
            danger: false,
        },
        ToolbarGitAction::UndoLastCommit => ActiveToolbarGitActionPresentation {
            label: "Undoing Last Commit...",
            icon_path: "assets/icons/icons__discard.svg",
            danger: true,
        },
        ToolbarGitAction::Fetch => ActiveToolbarGitActionPresentation {
            label: "Fetching...",
            icon_path: "assets/icons/icons__tool-download.svg",
            danger: false,
        },
        ToolbarGitAction::Pull => ActiveToolbarGitActionPresentation {
            label: "Pulling...",
            icon_path: "assets/icons/icons__git-pull.svg",
            danger: false,
        },
        ToolbarGitAction::Push { force: false } => ActiveToolbarGitActionPresentation {
            label: "Pushing...",
            icon_path: "assets/icons/icons__cloud-upload.svg",
            danger: false,
        },
        ToolbarGitAction::Push { force: true } => ActiveToolbarGitActionPresentation {
            label: "Force Pushing...",
            icon_path: "assets/icons/icons__cloud-upload.svg",
            danger: true,
        },
        ToolbarGitAction::CreatePr { draft: false, .. } => ActiveToolbarGitActionPresentation {
            label: "Creating PR...",
            icon_path: "assets/icons/icons__github.svg",
            danger: false,
        },
        ToolbarGitAction::CreatePr { draft: true, .. } => ActiveToolbarGitActionPresentation {
            label: "Creating Draft PR...",
            icon_path: "assets/icons/icons__github.svg",
            danger: false,
        },
    }
}

impl AnotherOneApp {
    fn active_titlebar_task_project_id(&self, cx: &App) -> Option<String> {
        self.workspace_pane
            .read(cx)
            .active_section
            .as_ref()
            .and_then(|section| section.task_id.as_ref().map(|_| section.project_id.clone()))
    }

    fn disabled_titlebar_split_button(
        &self,
        id: &'static str,
        label: &'static str,
        icon_path: &'static str,
        width: f32,
        margin_right: f32,
    ) -> AnyElement {
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);

        div()
            .id(id)
            .flex()
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .w(px(width))
            .h(px(28.))
            .mr(px(margin_right))
            .rounded(px(11.))
            .bg(app_theme.overlay_rest)
            .border_1()
            .border_color(app_theme.border)
            .opacity(0.45)
            .tooltip(|_window, cx| {
                Self::action_tooltip_view("Select a task in the sidebar to use this", cx)
            })
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.))
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .h_full()
                    .px(px(9.))
                    .border_r_1()
                    .border_color(app_theme.divider)
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(14.))
                            .text_color(app_theme.text_muted),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(app_theme.text_muted)
                            .truncate()
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
                    .child(
                        svg()
                            .path("assets/icons/icons__chevron-down.svg")
                            .size(px(11.))
                            .text_color(app_theme.text_muted),
                    ),
            )
            .into_any_element()
    }

    pub fn titlebar_toggle_mouse(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_titlebar_dropdowns();
        cx.stop_propagation();
        self.toggle_sidebar(window, cx);
    }

    pub fn titlebar_right_toggle_mouse(
        &mut self,
        _: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.dismiss_titlebar_dropdowns();
        cx.stop_propagation();
        self.toggle_right_sidebar(window, cx);
    }

    pub fn titlebar_background_mouse(
        &mut self,
        _: &MouseDownEvent,
        _: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let dismissed = self.dismiss_titlebar_dropdowns();
        self.titlebar_drag_pending = true;
        cx.stop_propagation();
        if dismissed {
            cx.notify();
        }
    }

    pub fn titlebar_background_mouse_up(
        &mut self,
        ev: &MouseUpEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.titlebar_drag_pending = false;
        cx.stop_propagation();
        if ev.click_count == 2 {
            window.titlebar_double_click();
        }
    }

    pub fn titlebar_background_mouse_move(
        &mut self,
        _: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.titlebar_drag_pending {
            return;
        }

        self.titlebar_drag_pending = false;
        cx.stop_propagation();
        window.start_window_move();
    }

    pub fn sidebar_toggle_svg(
        window: &Window,
        mode: crate::project_store::ThemeMode,
    ) -> impl IntoElement {
        let color = theme::toggle_icon_color_for_mode(window, mode);
        svg()
            .path("assets/sidebar_toggle.svg")
            .size(px(15.))
            .text_color(color)
    }

    pub fn right_sidebar_toggle_svg(
        window: &Window,
        mode: crate::project_store::ThemeMode,
    ) -> impl IntoElement {
        let color = theme::toggle_icon_color_for_mode(window, mode);
        svg()
            .path("assets/right_sidebar_toggle.svg")
            .size(px(15.))
            .text_color(color)
    }

    fn idle_titlebar_primary_git_action(&self, cx: &App) -> TitlebarPrimaryGitAction {
        let preferred_commit_action = self
            .active_toolbar_repo_id(cx)
            .as_deref()
            .and_then(|repo_id| self.project_store.repo_default_commit_action(repo_id));

        resolve_idle_primary_git_action(
            !self.active_changed_files(cx).is_empty(),
            preferred_commit_action,
            self.active_project_ahead_count(cx),
        )
    }

    fn selected_custom_action(&self, cx: &App) -> Option<ProjectAction> {
        let project_id = self.active_titlebar_task_project_id(cx)?;
        let actions = self.project_store.project_actions(&project_id);
        self.last_used_custom_action_id
            .as_ref()
            .and_then(|last_used_id| {
                actions
                    .iter()
                    .find(|action| action.id == *last_used_id)
                    .cloned()
            })
            .or_else(|| actions.into_iter().next())
    }

    /// Small build-identity chip rendered in the titlebar.
    ///
    /// * Debug + dirty: red background — uncommitted changes are
    ///   live in this binary; whatever you see is unique to this
    ///   build and can't be reproduced from a SHA alone.
    /// * Debug + clean: amber background — debug profile, but at
    ///   least the working tree was clean at build time.
    /// * Release: subtle white-on-dark; informative but not noisy.
    ///
    /// The chip exists primarily to make it impossible to confuse a
    /// debug binary for a release one — that's the one mistake the
    /// updater will need not to silently propagate.
    pub fn titlebar_build_chip(&self, _cx: &mut Context<Self>) -> AnyElement {
        let label = SharedString::from(crate::build_info::chip_label());
        let dev = crate::build_info::is_dev_build();
        let dirty = crate::build_info::is_dirty();

        let (bg, border, text) = if dev && dirty {
            (
                hsla(0.0, 0.65, 0.42, 0.55),
                hsla(0.0, 0.85, 0.62, 0.85),
                hsla(0.0, 0.30, 0.97, 1.0),
            )
        } else if dev {
            (
                hsla(35.0 / 360.0, 0.85, 0.50, 0.45),
                hsla(35.0 / 360.0, 0.95, 0.62, 0.75),
                hsla(35.0 / 360.0, 0.30, 0.97, 1.0),
            )
        } else {
            (
                gpui::white().opacity(0.05),
                gpui::white().opacity(0.10),
                gpui::white().opacity(0.55),
            )
        };

        div()
            .id("titlebar-build-chip")
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .h(px(20.))
            .max_w(px(156.))
            .px(px(8.))
            .mr(px(6.))
            .rounded(px(10.))
            .bg(bg)
            .border_1()
            .border_color(border)
            .overflow_hidden()
            .text_size(px(11.))
            .font_weight(gpui::FontWeight::MEDIUM)
            .text_color(text)
            .tooltip(|_window, cx| Self::action_tooltip_view(crate::build_info::tooltip_text(), cx))
            .child(div().min_w(px(0.)).truncate().child(label))
            .into_any_element()
    }

    pub fn titlebar_custom_actions_button(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.active_titlebar_task_project_id(cx).is_none() {
            return self.disabled_titlebar_split_button(
                "titlebar-custom-actions-trigger-disabled",
                "Actions",
                "assets/icons/icons__tool-bolt.svg",
                TITLEBAR_CUSTOM_ACTIONS_BUTTON_W,
                TITLEBAR_CUSTOM_ACTIONS_BUTTON_MARGIN_RIGHT,
            );
        }

        let selected_action = self.selected_custom_action(cx);
        let has_actions = selected_action.is_some();
        let is_open = self.custom_actions_menu_open;
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let button_bg = if is_open {
            app_theme.overlay_active
        } else {
            app_theme.overlay_rest
        };
        let hover_bg = app_theme.overlay_hover_strong;
        let label = selected_action
            .as_ref()
            .map(|action| SharedString::from(action.display_name().to_string()))
            .unwrap_or_else(|| SharedString::from("Actions"));
        let icon_path = selected_action
            .as_ref()
            .map(|action| action.icon.icon_path())
            .unwrap_or("assets/icons/icons__tool-bolt.svg");
        let selected_for_run = selected_action.clone();

        div()
            .id("titlebar-custom-actions-trigger")
            .flex()
            .flex_shrink_0()
            .flex_row()
            .items_center()
            .w(px(TITLEBAR_CUSTOM_ACTIONS_BUTTON_W))
            .h(px(28.))
            .mr(px(TITLEBAR_CUSTOM_ACTIONS_BUTTON_MARGIN_RIGHT))
            .rounded(px(11.))
            .bg(button_bg)
            .border_1()
            .border_color(app_theme.overlay_hover_strong)
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.))
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .h_full()
                    .px(px(9.))
                    .border_r_1()
                    .border_color(app_theme.divider)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                            this.project_page_open_in_menu_project_id = None;
                            this.git_actions_menu_open = false;
                            this.custom_actions_menu_open = false;
                            if let Some(action) = selected_for_run.clone() {
                                this.run_project_action(action, Some(window), cx);
                            } else {
                                this.open_custom_action_modal(None, cx);
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(14.))
                            .text_color(app_theme.text_primary),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(app_theme.text_secondary)
                            .truncate()
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
                            this.project_page_open_in_menu_project_id = None;
                            this.git_actions_menu_open = false;
                            this.custom_actions_menu_open = !this.custom_actions_menu_open;
                            if !has_actions && this.custom_actions_menu_open {
                                this.open_custom_action_modal(None, cx);
                                this.custom_actions_menu_open = false;
                            }
                            cx.stop_propagation();
                            cx.notify();
                        }),
                    )
                    .child(
                        svg()
                            .path("assets/icons/icons__chevron-down.svg")
                            .size(px(11.))
                            .text_color(app_theme.text_muted),
                    ),
            )
            .into_any_element()
    }

    pub fn titlebar_custom_actions_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.custom_actions_menu_open {
            return div()
                .id("titlebar-custom-actions-overlay")
                .into_any_element();
        }
        let Some(project_id) = self.active_titlebar_task_project_id(cx) else {
            return div()
                .id("titlebar-custom-actions-overlay")
                .into_any_element();
        };

        let actions = self.project_store.project_actions(&project_id);
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let bg = app_theme.card_bg;
        let text_col = app_theme.text_primary;
        let muted_text = app_theme.text_muted;
        let hover_bg = app_theme.overlay_hover;
        let divider = app_theme.divider;

        let mut menu = div()
            .id("titlebar-custom-actions-menu")
            .absolute()
            .right(px(TITLEBAR_RIGHT_TOGGLE_SPACE
                + RESOURCE_INDICATOR_BUTTON_W
                + TITLEBAR_GIT_ACTIONS_BUTTON_W
                + TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT
                + TITLEBAR_PULL_REQUEST_BUTTON_W
                + TITLEBAR_PULL_REQUEST_BUTTON_MARGIN_RIGHT
                + TITLEBAR_GITHUB_BUTTON_W
                + TITLEBAR_GITHUB_BUTTON_MARGIN_RIGHT
                + TITLEBAR_OPEN_IN_BUTTON_W
                + TITLEBAR_OPEN_IN_BUTTON_MARGIN_RIGHT))
            .top(px(TITLEBAR_MENU_OFFSET_TOP))
            .w(px(TITLEBAR_CUSTOM_ACTIONS_MENU_W))
            .rounded(px(12.))
            .bg(bg)
            .border_1()
            .border_color(app_theme.border)
            .shadow_md()
            .occlude()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation());

        for action in actions {
            let run_action = action.clone();
            let edit_action = action.clone();
            let action_label = SharedString::from(action.display_name().to_string());
            let is_global = action.scope == ProjectActionScope::Global;
            menu = menu.child(
                div()
                    .id(SharedString::from(format!(
                        "titlebar-custom-action-{}",
                        action.id
                    )))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .h(px(36.))
                    .px(px(10.))
                    .hover(move |s| s.bg(hover_bg))
                    .child(
                        div()
                            .flex()
                            .flex_1()
                            .min_w(px(0.))
                            .items_center()
                            .gap(px(8.))
                            .cursor_pointer()
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, window, cx| {
                                    this.custom_actions_menu_open = false;
                                    this.run_project_action(run_action.clone(), Some(window), cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                svg()
                                    .path(action.icon.icon_path())
                                    .size(px(14.))
                                    .text_color(text_col),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(0.))
                                    .truncate()
                                    .text_size(rems(12. / 16.))
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(text_col)
                                    .child(action_label),
                            ),
                    )
                    .when(is_global, |row| {
                        row.child(
                            div()
                                .w(px(20.))
                                .h(px(24.))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    svg()
                                        .path("assets/icons/icons__globe.svg")
                                        .size(px(13.))
                                        .text_color(muted_text),
                                ),
                        )
                    })
                    .child(
                        div()
                            .w(px(24.))
                            .h(px(24.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded_md()
                            .cursor_pointer()
                            .hover(move |s| s.bg(app_theme.overlay_hover_strong))
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                    this.custom_actions_menu_open = false;
                                    this.open_custom_action_modal(Some(edit_action.clone()), cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                            .child(
                                svg()
                                    .path("assets/icons/icons__settings.svg")
                                    .size(px(13.))
                                    .text_color(muted_text),
                            ),
                    ),
            );
        }

        menu = menu.child(div().h(px(1.)).mx(px(8.)).bg(divider)).child(
            div()
                .id("titlebar-custom-action-add")
                .flex()
                .items_center()
                .gap(px(8.))
                .h(px(36.))
                .px(px(12.))
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                        this.custom_actions_menu_open = false;
                        this.open_custom_action_modal(None, cx);
                        cx.stop_propagation();
                        cx.notify();
                    }),
                )
                .child(
                    svg()
                        .path("assets/icons/icons__plus.svg")
                        .size(px(14.))
                        .text_color(text_col),
                )
                .child(
                    div()
                        .text_size(rems(12. / 16.))
                        .font_weight(gpui::FontWeight::MEDIUM)
                        .text_color(text_col)
                        .child("Add action"),
                ),
        );

        div()
            .id("titlebar-custom-actions-overlay")
            .absolute()
            .top(px(TITLEBAR_CHROME_H))
            .left(px(0.))
            .right(px(0.))
            .bottom(px(0.))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.custom_actions_menu_open = false;
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(menu)
            .into_any_element()
    }

    pub fn titlebar_open_in_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(project_id) = self.active_titlebar_task_project_id(cx) else {
            return self.disabled_titlebar_split_button(
                "titlebar-open-in-trigger-disabled",
                "Open In",
                "assets/icons/open_in__folder_closed.svg",
                TITLEBAR_OPEN_IN_BUTTON_W,
                TITLEBAR_OPEN_IN_BUTTON_MARGIN_RIGHT,
            );
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
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let button_bg = if menu_open {
            app_theme.overlay_active
        } else {
            app_theme.overlay_rest
        };
        let hover_bg = if has_apps {
            app_theme.overlay_hover_strong
        } else {
            app_theme.divider
        };
        let project_id_for_chevron = project_id.clone();

        div()
            .id(SharedString::from(format!(
                "titlebar-open-in-trigger-{project_id}"
            )))
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
            .border_color(app_theme.overlay_hover_strong)
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
                    .border_color(app_theme.divider)
                    .cursor_pointer()
                    .hover(move |style| style.bg(hover_bg))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.open_active_open_in_target_in_default_app(cx);
                        }),
                    )
                    .child(
                        svg()
                            .path(primary_icon)
                            .size(px(14.))
                            .text_color(app_theme.text_primary),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(app_theme.text_secondary)
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
                            .text_color(app_theme.text_muted),
                    ),
            )
            .into_any_element()
    }

    pub fn titlebar_open_in_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(project_id) = self.project_page_open_in_menu_project_id.clone() else {
            return div().id("titlebar-open-in-overlay").into_any_element();
        };
        if self.active_titlebar_task_project_id(cx).as_deref() != Some(project_id.as_str()) {
            return div().id("titlebar-open-in-overlay").into_any_element();
        }

        let enabled_open_in_apps = self.enabled_open_in_apps();
        if enabled_open_in_apps.is_empty() {
            return div().id("titlebar-open-in-overlay").into_any_element();
        }

        let overlay_right = TITLEBAR_RIGHT_TOGGLE_SPACE
            + RESOURCE_INDICATOR_BUTTON_W
            + TITLEBAR_GIT_ACTIONS_BUTTON_W
            + TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT
            + TITLEBAR_PULL_REQUEST_BUTTON_W
            + TITLEBAR_PULL_REQUEST_BUTTON_MARGIN_RIGHT
            + TITLEBAR_GITHUB_BUTTON_W
            + TITLEBAR_GITHUB_BUTTON_MARGIN_RIGHT
            + TITLEBAR_OPEN_IN_BUTTON_MARGIN_RIGHT;

        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);

        let mut menu = div()
            .id("titlebar-open-in-menu")
            .absolute()
            .right(px(overlay_right))
            .top(px(TITLEBAR_MENU_OFFSET_TOP))
            .w(px(TITLEBAR_OPEN_IN_MENU_W))
            .rounded(px(12.))
            .bg(app_theme.card_bg)
            .border_1()
            .border_color(app_theme.border)
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
                    .hover(move |style| style.bg(app_theme.overlay_hover))
                    .tooltip(move |_window, cx| Self::action_tooltip_view(app.description(), cx))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                            this.open_project_open_in_target_in_app(&project_id_for_open, app, cx);
                        }),
                    )
                    .child(
                        svg()
                            .path(app.icon_path())
                            .size(px(16.))
                            .text_color(app_theme.text_primary),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(app_theme.text_primary)
                            .child(app.label()),
                    ),
            );
        }

        div()
            .id("titlebar-open-in-overlay")
            .absolute()
            .top(px(TITLEBAR_CHROME_H))
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

    pub fn titlebar_github_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(project_id) = self.active_open_in_project_id(cx) else {
            return div().into_any_element();
        };
        let Some(github_url) = self.project_github_links.get(&project_id).cloned() else {
            return div().into_any_element();
        };

        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);

        div()
            .id(SharedString::from(format!(
                "titlebar-github-trigger-{project_id}"
            )))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .w(px(TITLEBAR_GITHUB_BUTTON_W))
            .h(px(28.))
            .mr(px(TITLEBAR_GITHUB_BUTTON_MARGIN_RIGHT))
            .rounded(px(11.))
            .bg(app_theme.overlay_rest)
            .border_1()
            .border_color(app_theme.border)
            .cursor_pointer()
            .hover(move |style| style.bg(app_theme.overlay_hover_strong))
            .tooltip(move |_window, cx| Self::action_tooltip_view("Open repository in GitHub", cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    this.project_page_open_in_menu_project_id = None;
                    this.custom_actions_menu_open = false;
                    this.git_actions_menu_open = false;
                    if let Err(err) =
                        crate::platform::CurrentPlatform::open_external_url(&github_url)
                    {
                        this.show_error_toast(err, cx);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                svg()
                    .path("assets/icons/icons__github.svg")
                    .size(px(15.))
                    .text_color(app_theme.text_primary),
            )
            .into_any_element()
    }

    pub fn titlebar_pull_request_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let Some(pull_request) = self.active_project_pull_request(cx).cloned() else {
            return div().into_any_element();
        };

        let (state_color, tooltip) = match pull_request.state {
            crate::git_actions::PullRequestState::Open => (
                hsla(160. / 360., 0.84, 0.35, 1.),
                "Open pull request in GitHub",
            ),
            crate::git_actions::PullRequestState::Closed => (
                hsla(240. / 360., 0.04, 0.46, 1.),
                "Open closed pull request in GitHub",
            ),
            crate::git_actions::PullRequestState::Merged => (
                hsla(262. / 360., 0.83, 0.58, 1.),
                "Open merged pull request in GitHub",
            ),
        };
        let pull_request_url = pull_request.url.clone();

        div()
            .id(SharedString::from(format!(
                "titlebar-pull-request-trigger-{}",
                pull_request.number
            )))
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .w(px(TITLEBAR_PULL_REQUEST_BUTTON_W))
            .h(px(28.))
            .mr(px(TITLEBAR_PULL_REQUEST_BUTTON_MARGIN_RIGHT))
            .rounded(px(11.))
            .bg(state_color.opacity(0.13))
            .border_1()
            .border_color(state_color.opacity(0.46))
            .cursor_pointer()
            .hover(move |style| style.bg(state_color.opacity(0.20)))
            .tooltip(move |_window, cx| Self::action_tooltip_view(tooltip, cx))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                    this.project_page_open_in_menu_project_id = None;
                    this.custom_actions_menu_open = false;
                    this.git_actions_menu_open = false;
                    if let Err(err) =
                        crate::platform::CurrentPlatform::open_external_url(&pull_request_url)
                    {
                        this.show_error_toast(err, cx);
                    }
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
            .child(
                svg()
                    .path("assets/icons/icons__pull-request.svg")
                    .size(px(13.))
                    .text_color(state_color),
            )
            .into_any_element()
    }

    pub fn titlebar_git_actions_button(&self, cx: &mut Context<Self>) -> AnyElement {
        if self.active_titlebar_task_project_id(cx).is_none() {
            return self.disabled_titlebar_split_button(
                "titlebar-git-actions-trigger-disabled",
                "Git Actions",
                "assets/icons/icons__git-commit.svg",
                TITLEBAR_GIT_ACTIONS_BUTTON_W,
                TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT,
            );
        }

        let primary_action = self.idle_titlebar_primary_git_action(cx);
        let active_action = self
            .active_git_action_for_current_project(cx)
            .map(|active| active.action.clone());
        let active_presentation = active_action
            .clone()
            .map(resolve_active_git_action_presentation);
        let active = active_action.is_some();
        let interactive = !active;
        let is_open = self.git_actions_menu_open;
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let button_bg = if is_open {
            app_theme.overlay_active
        } else {
            app_theme.overlay_rest
        };
        let hover_bg = app_theme.overlay_hover_strong;
        let border = app_theme.overlay_hover_strong;
        let divider = app_theme.divider;
        let danger_col = hsla(0., 0.78, 0.72, 1.);
        let text_col = active_presentation
            .filter(|presentation| presentation.danger)
            .map(|_| danger_col)
            .unwrap_or_else(|| app_theme.text_secondary);
        let icon_col = active_presentation
            .filter(|presentation| presentation.danger)
            .map(|_| danger_col)
            .unwrap_or_else(|| app_theme.text_primary);
        let chevron_col = app_theme.text_muted;
        let trigger_label = active_presentation
            .map(|presentation| SharedString::from(presentation.label))
            .unwrap_or_else(|| primary_action.label());
        let primary_toolbar_action = primary_action.toolbar_action();

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
            .child(
                div()
                    .flex()
                    .flex_1()
                    .min_w(px(0.))
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
                                cx.listener(move |this, _ev: &MouseDownEvent, _window, cx| {
                                    this.project_page_open_in_menu_project_id = None;
                                    this.git_actions_menu_open = false;
                                    this.custom_actions_menu_open = false;
                                    this.start_toolbar_git_action(
                                        primary_toolbar_action.clone(),
                                        cx,
                                    );
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(if active {
                        Self::toolbar_spinner(icon_col, 12.).into_any_element()
                    } else {
                        svg()
                            .path(primary_action.icon_path())
                            .size(px(14.))
                            .text_color(icon_col)
                            .into_any_element()
                    })
                    .child(
                        div()
                            .flex_1()
                            .min_w(px(0.))
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .truncate()
                            .child(trigger_label),
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
                                    this.custom_actions_menu_open = false;
                                    let opening = !this.git_actions_menu_open;
                                    this.git_actions_menu_open = opening;
                                    if opening {
                                        this.refresh_active_project_pull_request_lookup(cx);
                                    }
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
            .into_any_element()
    }

    pub fn titlebar_git_actions_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.git_actions_menu_open || self.active_git_action_for_current_project(cx).is_some() {
            return div().id("titlebar-git-actions-overlay").into_any_element();
        }

        if self.active_titlebar_task_project_id(cx).is_none() {
            return div().id("titlebar-git-actions-overlay").into_any_element();
        }

        let has_changes = !self.active_changed_files(cx).is_empty();
        let can_commit = has_changes;
        let toolbar_enabled = self.active_git_action_for_current_project(cx).is_none();
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let bg = app_theme.card_bg;
        let border = app_theme.border;
        let text_col = app_theme.text_primary;
        let hover_bg = app_theme.overlay_hover;
        let danger_col = app_theme.error.text;
        let danger_hover = app_theme.error.bg;
        let divider = app_theme.divider;
        let push_label = count_git_action_label("Push", self.active_project_ahead_count(cx));
        let pull_label = count_git_action_label("Pull", self.active_project_behind_count(cx));
        let pull_request_url = self.active_project_pull_request_url(cx);
        let pull_request_lookup_checked = self.active_project_pull_request_lookup_checked(cx);
        let has_existing_pull_request = pull_request_url.is_some();
        let can_create_pull_request =
            toolbar_enabled && pull_request_lookup_checked && !has_existing_pull_request;

        let menu = div()
            .id("titlebar-git-actions-menu")
            .absolute()
            .right(px(
                TITLEBAR_RIGHT_TOGGLE_SPACE
                    + RESOURCE_INDICATOR_BUTTON_W
                    + TITLEBAR_GIT_ACTIONS_BUTTON_MARGIN_RIGHT,
            ))
            .top(px(TITLEBAR_MENU_OFFSET_TOP))
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
                    .id("titlebar-git-actions-fetch")
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
                                    "Fetch remote updates without changing the local checkout",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.start_toolbar_git_action(ToolbarGitAction::Fetch, cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__tool-download.svg")
                            .size(px(14.))
                            .text_color(text_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child("Fetch"),
                    ),
            )
            .child(
                div()
                    .id("titlebar-git-actions-pull")
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
                                    "Pull remote updates with fast-forward only",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.start_toolbar_git_action(ToolbarGitAction::Pull, cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__git-pull.svg")
                            .size(px(14.))
                            .text_color(text_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child(pull_label),
                    ),
            )
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
                    .opacity(if can_create_pull_request { 1. } else { 0.55 })
                    .when(can_create_pull_request, |d| {
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
                                        ToolbarGitAction::CreatePr {
                                            draft: false,
                                            base_branch: None,
                                        },
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
                    .opacity(if can_create_pull_request { 1. } else { 0.55 })
                    .when(can_create_pull_request, |d| {
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
                                        ToolbarGitAction::CreatePr {
                                            draft: true,
                                            base_branch: None,
                                        },
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
            )
            .child(div().h(px(1.)).mx(px(8.)).bg(divider))
            .child(
                div()
                    .id("titlebar-git-actions-create-branch")
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
                                    "Create a branch in this task or a new worktree",
                                    cx,
                                )
                            })
                            .on_mouse_down(
                                MouseButton::Left,
                                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                    this.git_actions_menu_open = false;
                                    this.open_create_branch_modal(cx);
                                    cx.stop_propagation();
                                    cx.notify();
                                }),
                            )
                    })
                    .child(
                        svg()
                            .path("assets/icons/icons__git-branch.svg")
                            .size(px(14.))
                            .text_color(text_col),
                    )
                    .child(
                        div()
                            .text_size(rems(12. / 16.))
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(text_col)
                            .child("Create Branch"),
                    ),
            );

        div()
            .id("titlebar-git-actions-overlay")
            .absolute()
            .top(px(TITLEBAR_CHROME_H))
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

    pub fn custom_title_strip(
        &self,
        window: &mut Window,
        cx: &mut Context<Self>,
        busy: bool,
    ) -> impl IntoElement {
        let app_theme = theme::app_theme(window, self.project_store.ui.theme_mode);
        let chrome = app_theme.chrome_bg;
        div()
            .flex()
            .flex_row()
            .items_center()
            .relative()
            .h(px(TITLEBAR_CHROME_H))
            .flex_shrink_0()
            .bg(chrome)
            .border_b_1()
            .border_color(app_theme.border)
            .child(
                div()
                    .w(px(crate::platform::CurrentPlatform::traffic_light_pad_px()))
                    .flex_shrink_0(),
            )
            .child(
                div()
                    .id("sidebar-toggle-titlebar")
                    .ml(px(crate::platform::CurrentPlatform::toggle_left_margin_px()))
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
                    .child(Self::sidebar_toggle_svg(
                        window,
                        self.project_store.ui.theme_mode,
                    )),
            )
            .child(
                div()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .overflow_hidden()
                    .window_control_area(WindowControlArea::Drag)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(Self::titlebar_background_mouse),
                    )
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(Self::titlebar_background_mouse_up),
                    )
                    .on_mouse_move(cx.listener(Self::titlebar_background_mouse_move))
                    .when(cfg!(debug_assertions), |d| {
                        d.flex().items_center().justify_center().px(px(8.)).child(
                            div()
                                .min_w(px(0.))
                                .truncate()
                                .text_color(tokens::ErrorColors::text())
                                .text_size(rems(11. / 16.))
                                .font_weight(gpui::FontWeight::SEMIBOLD)
                                .child(gpui::SharedString::new_static(
                                    "DEBUG BUILD - not for daily use",
                                )),
                        )
                    }),
            )
            .child(self.titlebar_build_chip(cx))
            .child(self.titlebar_custom_actions_button(cx))
            .child(self.titlebar_open_in_button(cx))
            .child(self.titlebar_github_button(cx))
            .child(self.titlebar_pull_request_button(cx))
            .child(self.titlebar_git_actions_button(cx))
            .child(self.titlebar_pair_mobile_button(cx))
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
                    .child(Self::right_sidebar_toggle_svg(
                        window,
                        self.project_store.ui.theme_mode,
                    )),
            )
    }
}

#[cfg(test)]
mod tests {
    use super::{
        count_git_action_label, resolve_active_git_action_presentation,
        resolve_idle_primary_git_action, TitlebarPrimaryGitAction,
    };
    use crate::git_actions::ToolbarGitAction;
    use crate::project_store::RepoDefaultCommitAction;

    mod resolve_idle_primary_git_action_tests {
        use super::{
            resolve_idle_primary_git_action, RepoDefaultCommitAction, TitlebarPrimaryGitAction,
        };

        #[test]
        fn returns_commit_when_changes_exist_and_preference_is_commit() {
            let action =
                resolve_idle_primary_git_action(true, Some(RepoDefaultCommitAction::Commit), 3);

            assert_eq!(action, TitlebarPrimaryGitAction::Commit);
        }

        #[test]
        fn returns_commit_and_push_when_changes_exist_and_preference_is_commit_and_push() {
            let action = resolve_idle_primary_git_action(
                true,
                Some(RepoDefaultCommitAction::CommitAndPush),
                3,
            );

            assert_eq!(action, TitlebarPrimaryGitAction::CommitAndPush);
        }

        #[test]
        fn returns_commit_when_changes_exist_and_preference_is_missing() {
            let action = resolve_idle_primary_git_action(true, None, 3);

            assert_eq!(action, TitlebarPrimaryGitAction::Commit);
        }

        #[test]
        fn returns_push_when_changes_do_not_exist() {
            let action = resolve_idle_primary_git_action(
                false,
                Some(RepoDefaultCommitAction::CommitAndPush),
                3,
            );

            assert_eq!(action, TitlebarPrimaryGitAction::Push { ahead_count: 3 });
        }
    }

    mod resolve_active_git_action_presentation_tests {
        use super::{resolve_active_git_action_presentation, ToolbarGitAction};

        #[test]
        fn maps_every_toolbar_action_to_the_expected_progress_label() {
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::Commit).label,
                "Committing..."
            );
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::CommitAndPush).label,
                "Committing & Pushing..."
            );
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::UndoLastCommit).label,
                "Undoing Last Commit..."
            );
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::Fetch).label,
                "Fetching..."
            );
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::Pull).label,
                "Pulling..."
            );
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::Push { force: false })
                    .label,
                "Pushing..."
            );
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::Push { force: true })
                    .label,
                "Force Pushing..."
            );
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::CreatePr {
                    draft: false,
                    base_branch: None,
                })
                .label,
                "Creating PR..."
            );
            assert_eq!(
                resolve_active_git_action_presentation(ToolbarGitAction::CreatePr {
                    draft: true,
                    base_branch: None,
                })
                .label,
                "Creating Draft PR..."
            );
        }
    }

    mod count_git_action_label_tests {
        use super::count_git_action_label;

        #[test]
        fn includes_the_count_only_when_non_zero() {
            assert_eq!(count_git_action_label("Pull", 0), "Pull");
            assert_eq!(count_git_action_label("Pull", 4), "Pull (4)");
        }
    }
}
