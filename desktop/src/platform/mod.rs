//! Platform services: OS-specific implementations behind a shared trait.
//!
//! Dispatch is static. `CurrentPlatform` is a `pub use` alias to the
//! zero-sized struct matching the build target, so call sites write
//! `CurrentPlatform::foo()` with no runtime cost.
//!
//! See `/home/mason/.claude/plans/make-a-new-branch-harmonic-meerkat.md` for
//! the full migration plan.

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "linux")]
pub use linux::LinuxPlatform as CurrentPlatform;
#[cfg(target_os = "macos")]
pub use macos::MacPlatform as CurrentPlatform;
#[cfg(target_os = "windows")]
pub use windows::WindowsPlatform as CurrentPlatform;

use std::path::Path;
use std::process::Command;

use gpui::{App, TitlebarOptions, Window, WindowDecorations};

use crate::open_in::OpenInAppKind;
use crate::resource_usage::{RawProcessSample, TrackedProcess};

pub trait PlatformServices {
    fn open_external_url(url: &str) -> Result<(), String>;

    fn platform_modifier_label() -> &'static str;

    fn read_process_samples(
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
    ) -> Vec<RawProcessSample>;

    fn total_system_memory_bytes() -> Option<u64>;

    fn is_open_in_app_available(app: OpenInAppKind) -> bool;

    fn command_for_open_in(app: OpenInAppKind, path: &Path) -> Command;

    fn titlebar_options(title: &str) -> TitlebarOptions;

    fn window_decorations() -> Option<WindowDecorations>;

    fn traffic_light_pad_px() -> f32;

    fn toggle_left_margin_px() -> f32;

    fn set_app_dock_icon(cx: &mut App);

    fn supports_custom_chrome(window: &Window) -> bool;
}
