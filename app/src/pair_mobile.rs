//! "Pair mobile" titlebar button + modal overlay.
//!
//! The embedded daemon library (`daemon::run_endpoint`,
//! booted on a dedicated tokio runtime by `daemon_host`) produces an
//! [`daemon::EndpointHandle`] that carries the pairing URL
//! and a pre-rendered QR PNG. `AnotherOneApp` stashes the handle as
//! [`AnotherOneApp::daemon_handle`] once the daemon-host thread
//! finishes booting; this module reads from that handle directly.
//!
//! No `/tmp` files anymore — the standalone `daemon-sandbox` binary
//! still publishes `/tmp/daemon-sandbox.*` for its own smoke tests,
//! but the desktop app no longer depends on them. Between app start
//! and the daemon's first handle delivery (a few hundred
//! milliseconds), the modal shows a "daemon not ready" empty state;
//! after the handle arrives, the QR + URL render.

use gpui::{
    div, prelude::*, px, rems, svg, AnyElement, Context, Image, ImageFormat, MouseButton,
    MouseDownEvent,
};

use crate::app::AnotherOneApp;
use crate::theme;

const MODAL_WIDTH: f32 = 520.;
const QR_SIZE: f32 = 320.;

pub const TITLEBAR_PAIR_MOBILE_BUTTON_W: f32 = 40.;
pub const TITLEBAR_PAIR_MOBILE_BUTTON_MARGIN_RIGHT: f32 = 6.;

impl AnotherOneApp {
    /// Compact titlebar button — icon only (the label "Pair mobile"
    /// reveals on hover via tooltip). Toggles
    /// [`Self::pair_mobile_modal_open`].
    pub fn titlebar_pair_mobile_button(&self, cx: &mut Context<Self>) -> AnyElement {
        let open = self.pair_mobile_modal_open;
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let bg = if open {
            app_theme.overlay_active
        } else {
            app_theme.overlay_rest
        };

        div()
            .id("titlebar-pair-mobile-button")
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .w(px(TITLEBAR_PAIR_MOBILE_BUTTON_W))
            .h(px(28.))
            .mr(px(TITLEBAR_PAIR_MOBILE_BUTTON_MARGIN_RIGHT))
            .rounded(px(11.))
            .bg(bg)
            .border_1()
            .border_color(app_theme.border)
            .cursor_pointer()
            .hover(move |style| style.bg(app_theme.overlay_hover_strong))
            .tooltip(move |_window, cx| {
                Self::action_tooltip_view("Pair a mobile device with the embedded daemon", cx)
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.pair_mobile_modal_open = !this.pair_mobile_modal_open;
                    // Whichever way we toggle, cancel any armed
                    // reset — re-opening should not inherit armed
                    // state from a prior session.
                    this.pair_mobile_reset_pending = false;
                    cx.notify();
                }),
            )
            .child(
                svg()
                    .path("assets/icons/icons__qr-code.svg")
                    .size(px(16.))
                    .text_color(app_theme.text_primary),
            )
            .into_any_element()
    }

    /// Delete the daemon's `paired_peers` allowlist on disk AND
    /// rotate the in-memory TOFU nonce (which also rebuilds the
    /// pairing URL + QR PNG). Rotating the nonce is essential — if
    /// we only deleted the allowlist, a stale QR a previously paired
    /// phone still had on-screen could walk right back in. After this
    /// returns, the next `pair_mobile_overlay` paint snapshots the
    /// new QR from the handle. No-ops on any step that can't proceed
    /// (missing config dir, handle not yet booted).
    pub fn reset_paired_peers(&self) {
        if let Ok(path) = crate::daemon_host::paired_peers_path() {
            let _ = std::fs::remove_file(&path);
        }
        if let Some(handle) = self.daemon_handle.as_ref() {
            if let Err(e) = handle.regenerate_pairing() {
                log::warn!("regenerate_pairing failed: {e:?}");
            }
        }
    }

    /// Full-window overlay — rendered conditionally by the top-level
    /// render tree. Returns a no-op `div` when the modal is closed so
    /// the render cost is zero on the common path.
    pub fn pair_mobile_overlay(&self, cx: &mut Context<Self>) -> AnyElement {
        if !self.pair_mobile_modal_open {
            return div().id("pair-mobile-overlay").into_any_element();
        }

        // Build the modal body from the current daemon handle. A
        // missing handle (daemon still booting, or it failed to
        // start) yields the "not ready" empty state.
        let app_theme = theme::app_theme_for_preference(self.project_store.ui.theme_mode);
        let body = match self.daemon_handle.as_ref() {
            Some(handle) => modal_body_ready(
                std::sync::Arc::new(Image::from_bytes(ImageFormat::Png, handle.qr_png_bytes())),
                handle.pairing_url(),
                app_theme.text_muted,
            ),
            None => modal_body_daemon_not_ready(app_theme.text_primary, app_theme.text_muted),
        };

        div()
            .id("pair-mobile-overlay")
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(app_theme.scrim_bg)
            .flex()
            .items_center()
            .justify_center()
            // Click outside the modal body → close.
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                    this.pair_mobile_modal_open = false;
                    cx.notify();
                }),
            )
            .child(
                div()
                    .id("pair-mobile-body")
                    .w(px(MODAL_WIDTH))
                    .bg(app_theme.card_bg)
                    .rounded(px(12.))
                    .border_1()
                    .border_color(app_theme.border)
                    .p(px(24.))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap(px(16.))
                    // Swallow inner clicks so they don't propagate to
                    // the backdrop's close handler.
                    .on_mouse_down(MouseButton::Left, |_ev: &MouseDownEvent, _window, _cx| {})
                    .child(
                        div()
                            .text_size(rems(16. / 16.))
                            .font_weight(gpui::FontWeight::SEMIBOLD)
                            .text_color(app_theme.text_primary)
                            .child("Pair mobile device"),
                    )
                    .child(body)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(8.))
                            .child({
                                // Two-click guard: first click arms
                                // `pair_mobile_reset_pending` and the
                                // button repaints as a stronger red
                                // with a "Confirm reset?" label.
                                // Second click actually deletes the
                                // allowlist. Prevents a stray modal
                                // click from nuking pair state.
                                let pending = self.pair_mobile_reset_pending;
                                let (label, bg_color, hover_color) = if pending {
                                    ("Confirm reset?", gpui::rgb(0x8a2a2a), gpui::rgb(0xa03a3a))
                                } else {
                                    ("Reset pairings", gpui::rgb(0x4a2a2a), gpui::rgb(0x5a3636))
                                };
                                div()
                                    .id("pair-mobile-reset")
                                    .px(px(14.))
                                    .py(px(6.))
                                    .rounded(px(8.))
                                    .bg(bg_color)
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(hover_color))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            if this.pair_mobile_reset_pending {
                                                this.reset_paired_peers();
                                                this.pair_mobile_reset_pending = false;
                                            } else {
                                                this.pair_mobile_reset_pending = true;
                                            }
                                            cx.notify();
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_size(rems(12. / 16.))
                                            .text_color(gpui::white().opacity(0.90))
                                            .child(label),
                                    )
                            })
                            .child(
                                div()
                                    .id("pair-mobile-close")
                                    .px(px(16.))
                                    .py(px(6.))
                                    .rounded(px(8.))
                                    .bg(app_theme.overlay_rest)
                                    .cursor_pointer()
                                    .hover(move |s| s.bg(app_theme.overlay_hover_strong))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(|this, _ev: &MouseDownEvent, _window, cx| {
                                            this.pair_mobile_modal_open = false;
                                            this.pair_mobile_reset_pending = false;
                                            cx.notify();
                                        }),
                                    )
                                    .child(
                                        div()
                                            .text_size(rems(12. / 16.))
                                            .text_color(app_theme.text_secondary)
                                            .child("Close"),
                                    ),
                            ),
                    ),
            )
            .into_any_element()
    }
}

fn modal_body_ready(
    qr: std::sync::Arc<Image>,
    pairing_url: String,
    url_text_col: gpui::Hsla,
) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap(px(16.))
        .child(
            // White background helps the scanner under dim ambient
            // light; the PNG itself is black-on-transparent.
            div()
                .w(px(QR_SIZE))
                .h(px(QR_SIZE))
                .bg(gpui::white())
                .rounded(px(8.))
                .flex()
                .items_center()
                .justify_center()
                .child(gpui::img(qr).w(px(QR_SIZE - 16.)).h(px(QR_SIZE - 16.))),
        )
        .child(
            div()
                .max_w(px(MODAL_WIDTH - 48.))
                .text_size(rems(11. / 16.))
                .font_family("monospace")
                .text_color(url_text_col)
                .child(pairing_url),
        )
        .into_any_element()
}

fn modal_body_daemon_not_ready(title_col: gpui::Hsla, body_col: gpui::Hsla) -> AnyElement {
    div()
        .flex()
        .flex_col()
        .gap(px(10.))
        .child(
            div()
                .text_size(rems(13. / 16.))
                .text_color(title_col)
                .child("Mobile daemon is still starting…"),
        )
        .child(
            div()
                .text_size(rems(12. / 16.))
                .text_color(body_col)
                .child(
                    "The embedded iroh endpoint boots on app start. Close and reopen this dialog in a moment.",
                ),
        )
        .into_any_element()
}
