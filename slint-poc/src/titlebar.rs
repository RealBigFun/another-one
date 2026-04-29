//! Slint titlebar helpers.
//!
//! GPUI source of truth: `desktop/src/titlebar.rs`. This module owns the
//! Slint-side data shaping for the build chip, debug banner, and Open In
//! menu projection. App-thread state mutations remain in lib.rs callers
//! through `slint::Weak<AppWindow>` until the rest of the titlebar module
//! is extracted.
//!
//! Phase A scope: bounded utilities that do not depend on workspace state
//! or any other surface module. Custom-actions and git-toolbar helpers
//! still live in lib.rs because their dispatchers are entangled with
//! workspace shell wiring; they move in a later extraction.

use crate::{frame, AppWindow, MenuEntry};

pub(crate) fn build_chip_label() -> String {
    let profile = if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    };
    let sha = option_env!("ANOTHER_ONE_BUILD_SHA").unwrap_or("unknown");
    let dirty = option_env!("ANOTHER_ONE_BUILD_DIRTY") == Some("true");
    if dirty {
        format!("{profile} · {sha} · dirty")
    } else {
        format!("{profile} · {sha}")
    }
}

pub(crate) fn debug_banner_text() -> &'static str {
    if cfg!(debug_assertions) {
        "DEBUG BUILD - not for daily use"
    } else {
        ""
    }
}

pub(crate) fn open_in_menu_entries(state: &frame::OpenInStateWire) -> (String, Vec<MenuEntry>) {
    let preferred_app_id = state.preferred_app_id.clone().unwrap_or_default();
    if state.enabled_apps.is_empty() {
        return (
            String::new(),
            vec![MenuEntry {
                id: "__no-open-in-apps".into(),
                label: "No apps enabled".into(),
                shortcut: "".into(),
                selected: false,
                disabled: true,
                destructive: false,
            }],
        );
    }

    let entries = state
        .enabled_apps
        .iter()
        .map(|app| MenuEntry {
            id: app.id.clone().into(),
            label: app.label.clone().into(),
            shortcut: "".into(),
            selected: app.id == preferred_app_id,
            disabled: false,
            destructive: false,
        })
        .collect();
    (preferred_app_id, entries)
}

pub(crate) fn set_open_in_state(app_weak: &slint::Weak<AppWindow>, state: frame::OpenInStateWire) {
    let (preferred_app_id, entries) = open_in_menu_entries(&state);
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_titlebar_preferred_open_in_app_id(preferred_app_id.into());
            app.set_titlebar_open_in_entries(slint::ModelRc::new(slint::VecModel::from(entries)));
        }
    });
}

pub(crate) fn set_open_in_unavailable(
    app_weak: &slint::Weak<AppWindow>,
    detail: impl Into<String>,
) {
    let detail = detail.into();
    let app_weak = app_weak.clone();
    let _ = slint::invoke_from_event_loop(move || {
        if let Some(app) = app_weak.upgrade() {
            app.set_titlebar_preferred_open_in_app_id("".into());
            app.set_titlebar_open_in_entries(slint::ModelRc::new(slint::VecModel::from(vec![
                MenuEntry {
                    id: "__open-in-unavailable".into(),
                    label: detail.into(),
                    shortcut: "".into(),
                    selected: false,
                    disabled: true,
                    destructive: false,
                },
            ])));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_chip_label_includes_profile_and_sha() {
        let label = build_chip_label();

        assert!(label.starts_with("dev · ") || label.starts_with("release · "));
        assert!(label.split(" · ").nth(1).is_some_and(|sha| !sha.is_empty()));
    }

    #[test]
    fn open_in_menu_entries_mark_preferred_app_and_empty_state() {
        let state = frame::OpenInStateWire {
            enabled_apps: vec![
                frame::OpenInAppWire {
                    id: "cursor".to_string(),
                    label: "Cursor".to_string(),
                    description: "Open in Cursor".to_string(),
                    icon_path: "icons/cursor.svg".to_string(),
                },
                frame::OpenInAppWire {
                    id: "zed".to_string(),
                    label: "Zed".to_string(),
                    description: "Open in Zed".to_string(),
                    icon_path: "icons/zed.svg".to_string(),
                },
            ],
            preferred_app_id: Some("zed".to_string()),
        };

        let (preferred, entries) = open_in_menu_entries(&state);

        assert_eq!(preferred, "zed");
        assert!(entries
            .iter()
            .any(|entry| entry.id.as_str() == "zed" && entry.selected));
        assert!(entries
            .iter()
            .any(|entry| entry.id.as_str() == "cursor" && !entry.selected));

        let empty = frame::OpenInStateWire {
            enabled_apps: Vec::new(),
            preferred_app_id: None,
        };
        let (preferred, entries) = open_in_menu_entries(&empty);

        assert_eq!(preferred, "");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].disabled);
    }

    /// Pin the Slint titlebar module to GPUI titlebar symbols so a rename in
    /// `desktop/src/titlebar.rs` forces a Slint-side review. The current
    /// module covers the Open In menu projection plus build chip/debug
    /// banner; the full custom-actions and git toolbar dispatchers extract
    /// in a later slice.
    #[test]
    fn slint_titlebar_module_pins_to_gpui_titlebar_symbols() {
        let gpui = include_str!("../../desktop/src/titlebar.rs");
        for symbol in [
            "titlebar_open_in_overlay",
            "titlebar_custom_actions_overlay",
            "titlebar_git_actions_overlay",
        ] {
            assert!(
                gpui.contains(symbol),
                "GPUI titlebar overlay symbol missing or renamed: {symbol}"
            );
        }
    }
}
