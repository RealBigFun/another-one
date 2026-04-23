//! AnotherOne desktop app (sidebar | main | right) using GPUI from Zed
//! (https://github.com/zed-industries/zed, crate `gpui`).

mod add_agent_modal;
mod agent_icons;
mod app;
mod assets;
mod layout;
mod left_sidebar;
mod new_task_modal;
mod open_in;
mod panels;
mod platform;
mod project_page;
mod resource_indicator;
mod resource_usage;
mod right_sidebar;
mod settings_page;
mod shortcuts;
mod terminal_runtime;
mod theme;
mod titlebar;
mod tokens;

// These modules live in the `another-one-core` crate and are re-exported
// at the crate root so pre-existing `crate::<module>::…` paths throughout
// the binary keep resolving without a global find-and-replace. Phase 1
// of the plan extracts more modules in subsequent PRs; each one goes
// through this same re-export shim first, then paths can migrate
// opportunistically.
//
// `open_in` and `shortcuts` are *not* here — they keep local modules in
// desktop (the GPUI/platform-coupled halves) that internally re-export
// the core-side pure types, so `crate::open_in::OpenInAppKind` still
// resolves via those local modules.
pub(crate) use another_one_core::{agents, git_actions, project_store, terminal_launch};

use std::path::PathBuf;

use gpui::{
    px, size, App, AppContext, Application, Bounds, KeyBinding, WindowBounds, WindowOptions,
};

use app::{AnotherOneApp, ZoomIn, ZoomOut, ZoomReset};
use assets::ProjectAssets;
use platform::{CurrentPlatform, PlatformServices};

#[hotpath::main]
fn main() {
    Application::new()
        .with_assets(ProjectAssets {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        })
        .run(|cx: &mut App| {
            CurrentPlatform::set_app_dock_icon(cx);
            // Register bundled Lilex Nerd Font Mono so it's available without
            // the user having to install it system-wide.
            let font_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("assets/fonts");
            let font_files: Vec<std::borrow::Cow<'static, [u8]>> = [
                "LilexNerdFontMono-Regular.ttf",
                "LilexNerdFontMono-Bold.ttf",
                "LilexNerdFontMono-Italic.ttf",
                "LilexNerdFontMono-BoldItalic.ttf",
                "LilexNerdFontMono-Medium.ttf",
                "LilexNerdFontMono-MediumItalic.ttf",
            ]
            .iter()
            .filter_map(|name| std::fs::read(font_dir.join(name)).ok().map(Into::into))
            .collect();
            if !font_files.is_empty() {
                cx.text_system().add_fonts(font_files).ok();
            }
            let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
            let titlebar = CurrentPlatform::titlebar_options("AnotherOne");
            // Global zoom key bindings (work regardless of focus).
            cx.bind_keys([
                KeyBinding::new("cmd-=", ZoomIn, None),
                KeyBinding::new("cmd-+", ZoomIn, None),
                KeyBinding::new("cmd--", ZoomOut, None),
                KeyBinding::new("cmd-0", ZoomReset, None),
            ]);

            cx.open_window(
                WindowOptions {
                    titlebar: Some(titlebar),
                    window_bounds: Some(WindowBounds::Windowed(bounds)),
                    app_id: Some("another-one".into()),
                    window_decorations: CurrentPlatform::window_decorations(),
                    ..Default::default()
                },
                |window, cx| cx.new(|cx| AnotherOneApp::new(window, cx)),
            )
            .unwrap();
            cx.on_window_closed(|cx| {
                cx.quit();
            })
            .detach();
            cx.activate(true);
        });
}
