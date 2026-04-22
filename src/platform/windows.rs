use std::path::Path;
use std::process::Command;

use gpui::{App, TitlebarOptions, Window, WindowDecorations};

use super::PlatformServices;
use crate::open_in::{command_exists, OpenInAppKind};
use crate::resource_usage::{RawProcessSample, TrackedProcess};

pub struct WindowsPlatform;

impl PlatformServices for WindowsPlatform {
    fn open_external_url(url: &str) -> Result<(), String> {
        let mut command = Command::new("cmd");
        command.args(["/C", "start", "", url]);
        command
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("Could not open the GitHub link: {err}"))
    }

    fn platform_modifier_label() -> &'static str {
        "Win"
    }

    fn default_close_current_tab_binding() -> &'static str {
        "control-w"
    }

    fn read_process_samples(
        _app_pid: u32,
        _tracked_processes: &[TrackedProcess],
    ) -> Vec<RawProcessSample> {
        Vec::new()
    }

    fn total_system_memory_bytes() -> Option<u64> {
        None
    }

    fn is_open_in_app_available(app: OpenInAppKind) -> bool {
        match app {
            OpenInAppKind::Cursor => command_exists(&["cursor"]),
            OpenInAppKind::Zed => command_exists(&["zed"]),
            OpenInAppKind::VsCode => command_exists(&["code"]),
            OpenInAppKind::FileManager => true,
        }
    }

    fn command_for_open_in(app: OpenInAppKind, path: &Path) -> Command {
        let mut command = match app {
            OpenInAppKind::Cursor => Command::new("cursor"),
            OpenInAppKind::Zed => Command::new("zed"),
            OpenInAppKind::VsCode => Command::new("code"),
            OpenInAppKind::FileManager => Command::new("explorer"),
        };
        command.arg(path);
        command
    }

    fn titlebar_options(title: &str) -> TitlebarOptions {
        TitlebarOptions {
            title: Some(title.to_string().into()),
            appears_transparent: false,
            traffic_light_position: None,
        }
    }

    fn window_decorations() -> Option<WindowDecorations> {
        None
    }

    fn traffic_light_pad_px() -> f32 {
        12.
    }

    fn toggle_left_margin_px() -> f32 {
        0.
    }

    fn set_app_dock_icon(_cx: &mut App) {}

    fn supports_custom_chrome(_window: &Window) -> bool {
        false
    }
}
