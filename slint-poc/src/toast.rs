//! Slint-side toast plumbing.
//!
//! GPUI source of truth: `desktop/src/app.rs` `show_success_toast`,
//! `show_error_toast`, `show_warning_toast`, `show_info_toast`, and the
//! top-level `toast_layer`. Slint mirrors GPUI's contract that all
//! user-facing notifications route through one toast surface — overlays
//! must not render inline status text in place of a toast.
//!
//! See `docs/architecture/reviews/slint-overlays-port-review.md` for the
//! "Toast routing contract" the components in `slint-poc/src/overlays.rs`
//! validate.

use crate::AppWindow;

pub(crate) fn set_toast(
    app_weak: &slint::Weak<AppWindow>,
    kind: impl Into<String>,
    message: impl Into<String>,
    detail: impl Into<String>,
) {
    let app_weak = app_weak.clone();
    let kind = kind.into();
    let message = message.into();
    let detail = detail.into();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_toast_kind(kind.into());
            app.set_toast_message(message.into());
            app.set_toast_detail(detail.into());
        }
    });
}

pub(crate) fn clear_toast(app_weak: &slint::Weak<AppWindow>) {
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_toast_kind("info".into());
            app.set_toast_message("".into());
            app.set_toast_detail("".into());
        }
    });
}

pub(crate) fn toast_clipboard_text(message: &str, detail: &str) -> String {
    match (message.trim(), detail.trim()) {
        ("", "") => String::new(),
        (message, "") => message.to_string(),
        ("", detail) => detail.to_string(),
        (message, detail) => format!("{message}\n{detail}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pin the Slint toast routing module to GPUI's toast helper surface so a
    /// rename of any `show_*_toast` symbol forces a Slint-side review. The
    /// overlay-routing contract lives in `slint-poc/src/overlays.rs`; this test
    /// only checks that the GPUI symbols Slint mirrors still exist.
    #[test]
    fn slint_toast_module_pins_to_gpui_toast_helpers() {
        let gpui = include_str!("../../desktop/src/app.rs");
        for symbol in [
            "fn show_success_toast",
            "fn show_error_toast",
            "fn show_warning_toast",
            "fn show_info_toast",
            "fn toast_layer",
        ] {
            assert!(
                gpui.contains(symbol),
                "GPUI toast helper missing or renamed: {symbol}; update Slint toast module."
            );
        }
    }

    #[test]
    fn toast_clipboard_text_combines_message_and_detail() {
        assert_eq!(toast_clipboard_text("", ""), "");
        assert_eq!(toast_clipboard_text("Copied", ""), "Copied");
        assert_eq!(toast_clipboard_text("", "details"), "details");
        assert_eq!(
            toast_clipboard_text("Copied", "12 bytes"),
            "Copied\n12 bytes"
        );
        assert_eq!(
            toast_clipboard_text("  Copied  ", "  12 bytes  "),
            "Copied\n12 bytes"
        );
    }
}
