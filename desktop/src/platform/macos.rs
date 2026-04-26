use std::path::Path;
use std::process::Command;

use another_one_core::platform::{CurrentPlatform as CorePlatform, HeadlessPlatform};
use gpui::{point, px, App, TitlebarOptions, Window, WindowDecorations};

use super::PlatformServices;
use crate::assets::asset_root;
use crate::open_in::OpenInAppKind;
use crate::resource_usage::{RawProcessSample, TrackedProcess};

pub struct MacPlatform;

impl PlatformServices for MacPlatform {
    fn open_external_url(url: &str) -> Result<(), String> {
        // Single source of truth lives in `core::platform::HeadlessPlatform`.
        CorePlatform::open_external_url(url)
    }

    fn platform_modifier_label() -> &'static str {
        // Single source of truth lives in `core::platform::HeadlessPlatform`.
        // This wrapper exists only because the desktop `PlatformServices`
        // trait predates the core abstraction; it'll be removed when the
        // GPUI binary is deleted in the Flutter migration's Phase 6.
        CorePlatform::modifier_label()
    }

    fn read_process_samples(
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
    ) -> Vec<RawProcessSample> {
        // See the matching comment in this file's `open_external_url`.
        CorePlatform::read_process_samples(app_pid, tracked_processes)
    }

    fn total_system_memory_bytes() -> Option<u64> {
        // See the matching comment in `desktop/src/platform/macos.rs`'s
        // `open_external_url` impl. Single source of truth in core.
        CorePlatform::total_system_memory_bytes()
    }

    fn is_open_in_app_available(app: OpenInAppKind) -> bool {
        // See the matching comment in this file's `open_external_url`.
        CorePlatform::is_open_in_app_available(app)
    }

    fn command_for_open_in(app: OpenInAppKind, path: &Path) -> Command {
        // See the matching comment in this file's `open_external_url`.
        CorePlatform::command_for_open_in(app, path)
    }

    fn titlebar_options(_title: &str) -> TitlebarOptions {
        TitlebarOptions {
            title: None,
            appears_transparent: true,
            traffic_light_position: Some(point(px(13.), px(10.))),
        }
    }

    fn window_decorations() -> Option<WindowDecorations> {
        None
    }

    fn traffic_light_pad_px() -> f32 {
        76.
    }

    fn toggle_left_margin_px() -> f32 {
        0.
    }

    fn set_app_dock_icon(_cx: &mut App) {
        use cocoa::appkit::{NSApp, NSApplication, NSImage};
        use cocoa::base::nil;
        use cocoa::foundation::NSString;
        use objc::runtime::Object;

        let asset_root = asset_root();
        let icon_path = [
            asset_root.join("assets/app-icon/source/another-one.png"),
            asset_root.join("assets/app-icon/macos/AnotherOne.icns"),
            asset_root.join("AnotherOne.icns"),
        ]
        .into_iter()
        .find(|path| path.exists());

        let Some(icon_path) = icon_path else {
            return;
        };

        unsafe {
            let path_str = NSString::alloc(nil).init_str(icon_path.to_str().unwrap());
            let image: *mut Object = NSImage::alloc(nil).initWithContentsOfFile_(path_str);
            if image != nil {
                let app = NSApp();
                app.setApplicationIconImage_(image);
            }
        }
    }

    fn supports_custom_chrome(_window: &Window) -> bool {
        true
    }
}
