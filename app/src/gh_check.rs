//! Background GitHub CLI installation/auth check + blocking overlay.
//!
//! On app boot the desktop spawns a worker thread that resolves `gh` on
//! the user's `PATH` (with homebrew fallbacks) and runs `gh auth status`.
//! The render-tick drain picks up the reply. If `gh` is missing or not
//! authenticated, [`AnotherOneApp::gh_check_overlay`] renders a scrim +
//! card asking the user to install / sign in, with a "Recheck" button
//! that re-runs the same probe.

use std::path::PathBuf;
use std::process::Command;
use std::sync::mpsc;

use gpui::{div, prelude::*, px, rems, AnyElement, Context, MouseButton, MouseDownEvent};

use crate::app::AnotherOneApp;
use crate::theme;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GhCheckStatus {
    Checking,
    Ok,
    NotInstalled,
    NotAuthenticated,
}

pub fn spawn_check() -> mpsc::Receiver<GhCheckStatus> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(perform_check());
    });
    rx
}

fn perform_check() -> GhCheckStatus {
    let cwd = dirs::home_dir().unwrap_or_else(|| PathBuf::from("/"));
    let Some(gh) = another_one_core::git_actions::find_gh_cli(&cwd) else {
        return GhCheckStatus::NotInstalled;
    };
    match Command::new(&gh).args(["auth", "status"]).output() {
        Ok(output) if output.status.success() => GhCheckStatus::Ok,
        Ok(_) => GhCheckStatus::NotAuthenticated,
        Err(_) => GhCheckStatus::NotInstalled,
    }
}

impl AnotherOneApp {
    pub(crate) fn drain_gh_check(&mut self) -> bool {
        let Some(rx) = self.gh_check_receiver.as_ref() else {
            return false;
        };
        match rx.try_recv() {
            Ok(status) => {
                self.gh_check_status = status;
                self.gh_check_receiver = None;
                self.gh_check_completed = true;
                true
            }
            Err(mpsc::TryRecvError::Empty) => false,
            Err(mpsc::TryRecvError::Disconnected) => {
                self.gh_check_receiver = None;
                false
            }
        }
    }

    pub(crate) fn recheck_gh(&mut self) {
        if self.gh_check_receiver.is_some() {
            return;
        }
        self.gh_check_status = GhCheckStatus::Checking;
        self.gh_check_receiver = Some(spawn_check());
    }

    pub(crate) fn gh_check_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        // Don't paint anything until at least one check has finished —
        // overlay would otherwise flash for whatever fraction of a
        // second the worker takes on cold boot.
        if !self.gh_check_completed {
            return div().id("gh-check-overlay").into_any_element();
        }
        if matches!(self.gh_check_status, GhCheckStatus::Ok) {
            return div().id("gh-check-overlay").into_any_element();
        }

        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let checking = matches!(self.gh_check_status, GhCheckStatus::Checking);
        let (title, body, hint) = match self.gh_check_status {
            GhCheckStatus::NotInstalled | GhCheckStatus::Checking => (
                "GitHub CLI not found",
                "AnotherOne uses the GitHub CLI (gh) for pull-request workflows. Install it, then click Recheck.",
                "Install from https://cli.github.com or run: brew install gh",
            ),
            GhCheckStatus::NotAuthenticated => (
                "GitHub CLI not signed in",
                "AnotherOne found gh but it isn't signed in. Authenticate, then click Recheck.",
                "Run: gh auth login",
            ),
            GhCheckStatus::Ok => unreachable!(),
        };

        let button_label = if checking { "Checking..." } else { "Recheck" };
        let overlay_rest = app_theme.overlay_rest;
        let overlay_hover_strong = app_theme.overlay_hover_strong;

        div()
            .id("gh-check-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(app_theme.scrim_bg)
            .flex()
            .items_center()
            .justify_center()
            .on_mouse_down(MouseButton::Left, |_ev: &MouseDownEvent, _window, _cx| {})
            .child(
                div()
                    .w(px(480.))
                    .bg(app_theme.card_bg)
                    .rounded(px(12.))
                    .border_1()
                    .border_color(app_theme.border)
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .child(
                        div()
                            .text_size(rems(1.0))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(app_theme.text_primary)
                            .child(title),
                    )
                    .child(
                        div()
                            .text_size(rems(13. / 16.))
                            .text_color(app_theme.text_secondary)
                            .child(body),
                    )
                    .child(
                        div()
                            .px(px(10.))
                            .py(px(8.))
                            .rounded(px(8.))
                            .bg(app_theme.sunken_bg)
                            .text_size(rems(12. / 16.))
                            .font_family("monospace")
                            .text_color(app_theme.text_muted)
                            .child(hint),
                    )
                    .child(
                        div().flex().flex_row().justify_end().gap(px(8.)).child(
                            div()
                                .id("gh-check-recheck")
                                .px(px(16.))
                                .py(px(6.))
                                .rounded(px(8.))
                                .bg(overlay_rest)
                                .border_1()
                                .border_color(app_theme.border)
                                .cursor_pointer()
                                .hover(move |s| s.bg(overlay_hover_strong))
                                .on_mouse_down(
                                    MouseButton::Left,
                                    cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                        this.recheck_gh();
                                        cx.notify();
                                    }),
                                )
                                .child(
                                    div()
                                        .text_size(rems(12. / 16.))
                                        .text_color(app_theme.text_primary)
                                        .child(button_label),
                                ),
                        ),
                    ),
            )
            .into_any_element()
    }
}
