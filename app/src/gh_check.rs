//! Blocking overlay rendered when the daemon-side `gh auth status`
//! probe reports the host is missing the GitHub CLI or isn't signed
//! in. The probe itself runs daemon-side now (#156) and lands here
//! through `UiSnapshot.gh_auth_status`; this module only owns the
//! rendering + the Recheck dispatch.

use daemon_proto::GhAuthStatusWire;
use gpui::{div, prelude::*, px, rems, AnyElement, Context, MouseButton, MouseDownEvent};

use crate::app::AnotherOneApp;
use crate::theme;

impl AnotherOneApp {
    /// Fire `Control::RecheckGhAuth` over the active session. The
    /// daemon re-runs the probe and publishes the new status through
    /// the next `UiSnapshot.gh_auth_status` projection — the overlay
    /// re-renders from the absorbed state, so we don't need to wait
    /// inline on the Ack.
    pub(crate) fn recheck_gh(&mut self) {
        let session = self.session_handle();
        crate::session_host::dispatch_fire_and_forget(
            session,
            daemon_proto::Control::RecheckGhAuth,
            |result| {
                if let Err(err) = result {
                    log::warn!("RecheckGhAuth failed: {err}");
                }
            },
        );
    }

    pub(crate) fn gh_check_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        // Don't paint anything until the daemon has reported a
        // status — older daemons (or a connection that just
        // landed and hasn't pushed a projection yet) leave the
        // field `None`, which we treat as "unknown, don't flash
        // an overlay" exactly like the pre-projection version
        // gated on `gh_check_completed`.
        let Some(status) = self.project_store.ui.gh_auth_status.as_ref() else {
            return div().id("gh-check-overlay").into_any_element();
        };
        if matches!(status, GhAuthStatusWire::Authenticated) {
            return div().id("gh-check-overlay").into_any_element();
        }

        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let checking = matches!(status, GhAuthStatusWire::Checking);
        let (title, body, hint) = match status {
            GhAuthStatusWire::GhMissing | GhAuthStatusWire::Checking => (
                "GitHub CLI not found",
                "AnotherOne uses the GitHub CLI (gh) for pull-request workflows. Install it, then click Recheck.",
                "Install from https://cli.github.com or run: brew install gh",
            ),
            GhAuthStatusWire::NotAuthenticated => (
                "GitHub CLI not signed in",
                "AnotherOne found gh but it isn't signed in. Authenticate, then click Recheck.",
                "Run: gh auth login",
            ),
            GhAuthStatusWire::Authenticated => unreachable!(),
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
