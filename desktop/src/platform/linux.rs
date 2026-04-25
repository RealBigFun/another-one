use std::path::Path;
use std::process::Command;

use another_one_core::platform::{CurrentPlatform as CorePlatform, HeadlessPlatform};
use gpui::{App, TitlebarOptions, Window, WindowDecorations};

use super::PlatformServices;
use crate::open_in::OpenInAppKind;
use crate::resource_usage::{RawProcessSample, TrackedProcess};

pub struct LinuxPlatform;

impl PlatformServices for LinuxPlatform {
    fn open_external_url(url: &str) -> Result<(), String> {
        // See the matching comment in `desktop/src/platform/macos.rs`.
        CorePlatform::open_external_url(url)
    }

    fn platform_modifier_label() -> &'static str {
        // See the matching comment in `desktop/src/platform/macos.rs`.
        CorePlatform::modifier_label()
    }

    fn read_process_samples(
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
    ) -> Vec<RawProcessSample> {
        // See the matching comment in `desktop/src/platform/macos.rs`.
        CorePlatform::read_process_samples(app_pid, tracked_processes)
    }

    fn total_system_memory_bytes() -> Option<u64> {
        // See the matching comment in `desktop/src/platform/macos.rs`.
        CorePlatform::total_system_memory_bytes()
    }

    fn is_open_in_app_available(app: OpenInAppKind) -> bool {
        // See the matching comment in `desktop/src/platform/macos.rs`.
        CorePlatform::is_open_in_app_available(app)
    }

    fn command_for_open_in(app: OpenInAppKind, path: &Path) -> Command {
        // See the matching comment in `desktop/src/platform/macos.rs`.
        CorePlatform::command_for_open_in(app, path)
    }

    fn titlebar_options(title: &str) -> TitlebarOptions {
        TitlebarOptions {
            title: Some(title.to_string().into()),
            appears_transparent: false,
            traffic_light_position: None,
        }
    }

    fn window_decorations() -> Option<WindowDecorations> {
        // Server-side decorations: the compositor draws the title bar.
        // Enabling `WindowDecorations::Client` requires us to fill the shadow
        // inset GPUI reserves around the window — otherwise the edges render
        // transparent. Deferred until the in-app chrome draws its own rounded
        // bg + shadow.
        None
    }

    fn traffic_light_pad_px() -> f32 {
        12.
    }

    fn toggle_left_margin_px() -> f32 {
        0.
    }

    fn set_app_dock_icon(_cx: &mut App) {
        // Linux dock/taskbar icon is driven by the window `app_id` + an installed
        // `.desktop` file whose `StartupWMClass=` matches. Nothing to do at runtime
        // unless we start writing `_NET_WM_ICON` directly — deferred follow-up.
    }

    fn supports_custom_chrome(_window: &Window) -> bool {
        // The strip renders beneath the system titlebar (no CSD) — it just
        // hosts the in-app controls (sidebar toggle, open-in menu, git
        // actions). No traffic-light padding needed on Linux.
        true
    }
}

