//! Android `PlatformServices` stub.
//!
//! Most desktop platform calls (dock icon, traffic-light padding, window
//! decorations, "open in finder") are meaningless on a phone. This impl
//! returns sensible no-ops so the cross-platform UI source compiles
//! against the same `CurrentPlatform` trait without per-call cfgs.
//! Hooks that genuinely matter on Android (external URLs via JNI, real
//! memory readings, etc.) can be filled in later — for the MVP we just
//! need the symbol to exist.

use std::path::Path;
use std::process::Command;

use gpui::{App, TitlebarOptions, Window, WindowDecorations};

use super::PlatformServices;
use crate::open_in::OpenInAppKind;

pub struct AndroidPlatform;

impl PlatformServices for AndroidPlatform {
    fn open_external_url(_url: &str) -> Result<(), String> {
        // Real impl needs a JNI call into Android's Intent.ACTION_VIEW.
        Err("open_external_url is not yet wired on Android".into())
    }

    fn platform_modifier_label() -> &'static str {
        // No "command" key on Android. Most physical-keyboard users on
        // Android expect Ctrl, matching the Linux convention.
        "ctrl"
    }

    fn is_open_in_app_available(_app: OpenInAppKind) -> bool {
        false
    }

    fn command_for_open_in(_app: OpenInAppKind, _path: &Path) -> Command {
        // Never invoked because `is_open_in_app_available` returns false,
        // but the trait demands a concrete `Command`. `true(1)` is a
        // harmless placeholder if the call site is reached.
        Command::new("true")
    }

    fn titlebar_options(_title: &str) -> TitlebarOptions {
        // Android NativeActivity windows have no titlebar; defaults are
        // fine because the renderer never reads them.
        TitlebarOptions::default()
    }

    fn window_decorations() -> Option<WindowDecorations> {
        // The system handles framing.
        None
    }

    fn traffic_light_pad_px() -> f32 {
        0.
    }

    fn toggle_left_margin_px() -> f32 {
        0.
    }

    fn set_app_dock_icon(_cx: &mut App) {
        // No app dock on Android.
    }

    fn supports_custom_chrome(_window: &Window) -> bool {
        // The full window is ours to draw into; no system chrome to
        // accommodate.
        true
    }
}
