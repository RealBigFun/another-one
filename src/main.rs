//! AnotherOne desktop app (sidebar | main | right) using GPUI from Zed
//! (https://github.com/zed-industries/zed, crate `gpui`).

mod agents;
mod app;
mod assets;
mod git_actions;
mod keys;
mod layout;
mod left_sidebar;
mod new_task_modal;
mod panels;
mod project_page;
mod project_store;
mod right_sidebar;
mod settings_page;
mod terminal;
mod theme;
mod titlebar;

use std::path::PathBuf;

use gpui::{
    point, px, size, App, AppContext, Application, Bounds, KeyBinding, WindowBounds, WindowOptions,
};

use app::{AnotherOneApp, ZoomIn, ZoomOut, ZoomReset};
use assets::ProjectAssets;

#[hotpath::main]
fn main() {
    Application::new()
        .with_assets(ProjectAssets {
            root: PathBuf::from(env!("CARGO_MANIFEST_DIR")),
        })
        .run(|cx: &mut App| {
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
            let titlebar = {
                #[cfg(target_os = "macos")]
                {
                    gpui::TitlebarOptions {
                        title: None,
                        appears_transparent: true,
                        traffic_light_position: Some(point(px(13.), px(10.))),
                    }
                }
                #[cfg(not(target_os = "macos"))]
                {
                    gpui::TitlebarOptions {
                        title: Some("AnotherOne".into()),
                        appears_transparent: false,
                        traffic_light_position: None,
                    }
                }
            };
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
                    ..Default::default()
                },
                |_, cx| cx.new(|cx| AnotherOneApp::new(cx)),
            )
            .unwrap();
            cx.on_window_closed(|cx| {
                cx.quit();
            })
            .detach();
            cx.activate(true);
        });
}
