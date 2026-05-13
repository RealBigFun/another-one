//! AnotherOne — same source for desktop and mobile.
//!
//! Desktop (`mac`, `linux`) enters via `src/main.rs` → [`run_desktop`].
//! Android enters via [`android_main`] (called by `NativeActivity` once
//! the `.so` is loaded). Both paths build a `gpui::Application`,
//! configure it, and call `.run(finish_launching)` — the per-platform
//! divergence is contained to platform construction; the UI code below
//! (`mod app`, `mod left_sidebar`, …) is identical on both sides.

// Test-harness surface: re-exports the bare minimum integration-test
// infrastructure (`RegistryState` + `DesktopTerminalRegistry`)
// without widening crate-wide visibility on the daemon-host module
// itself. Gated behind `cfg(test)` for in-crate tests AND the
// `test-harness` feature so `app/tests/*.rs` integration tests can
// flip the feature on without otherwise altering the build.
#[cfg(any(test, feature = "test-harness"))]
pub mod __test_harness {
    pub use crate::daemon_host::{DesktopTerminalRegistry, RegistryState};
}

mod add_agent_modal;
mod agent_icons;
mod app;
mod assets;
mod background_ops;
mod build_info;
mod create_branch_modal;
mod custom_actions_modal;
mod daemon_host;
mod gh_check;
mod git_diff_pane;
mod git_workspace;
mod iroh_client;
mod layout;
mod leakscope;
mod left_sidebar;
mod mcp_orchestrator;
mod mcp_page;
mod mobile;
mod new_task_modal;
mod open_in;
mod pair_mobile;
mod panels;
mod platform;
mod project_page;
mod project_workflows;
mod resource_indicator;
mod right_sidebar;
mod session_host;
mod settings_page;
mod shortcuts;
mod terminal_runtime;
mod text_edit;
mod theme;
mod titlebar;
mod tokens;
mod updater;
mod updater_install;

// These modules live in the `another-one-core` crate and are re-exported
// at the crate root so pre-existing `crate::<module>::…` paths throughout
// the codebase keep resolving without a global find-and-replace.
//
// `open_in` and `shortcuts` are *not* here — they keep local modules in
// desktop (the GPUI/platform-coupled halves) that internally re-export
// the core-side pure types.
pub(crate) use another_one_core::{
    agents, git_actions, project_store, task_launcher, terminal_launch,
};

use std::borrow::Cow;

use gpui::{px, size, App, AppContext, Bounds, KeyBinding, WindowBounds, WindowOptions};

use app::{AnotherOneApp, TerminalFind, TerminalSearchNext, TerminalSearchPrev};
use assets::ProjectAssets;
use platform::{CurrentPlatform, PlatformServices};

/// Bridge `log` records into the `tracing` subscriber and install a fmt
/// subscriber that prints to stderr. Default filter keeps our crates +
/// iroh at info, everything else at warn. Override with `RUST_LOG=…`.
/// Has to run before any thread that might log — including
/// `leakscope::start_sampler` and the GPUI app — so callers invoke it
/// first.
fn init_logging() {
    let _ = tracing_log::LogTracer::init();
    let filter = tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        "another_one=info,another_one_core=info,daemon=info,iroh=info,warn".into()
    });
    let _ = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .try_init();
}

/// Shared finish-launching callback used by both desktop and android
/// entries. Registers fonts, opens the main window with
/// `AnotherOneApp` mounted, wires global zoom keybindings, and (on
/// desktop) the close-quits-app behavior.
fn finish_launching(cx: &mut App) {
    CurrentPlatform::set_app_dock_icon(cx);

    // Register bundled Lilex Nerd Font Mono so it's available without
    // the user having to install it system-wide. Embedded via
    // `include_bytes!` so the bytes ride inside the binary / cdylib —
    // on Android the APK has no filesystem access to `desktop/assets/`,
    // and reading a missing font file silently leaves the family
    // unloaded, which then panics deep inside gpui's text-system the
    // first time anything tries to render text in `Lilex Nerd Font
    // Mono`. Costs ~14 MB of binary bloat for the six TTFs; fine.
    let font_files: Vec<std::borrow::Cow<'static, [u8]>> = vec![
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/LilexNerdFontMono-Regular.ttf"
        )),
        Cow::Borrowed(include_bytes!("../assets/fonts/LilexNerdFontMono-Bold.ttf")),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/LilexNerdFontMono-Italic.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/LilexNerdFontMono-BoldItalic.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/LilexNerdFontMono-Medium.ttf"
        )),
        Cow::Borrowed(include_bytes!(
            "../assets/fonts/LilexNerdFontMono-MediumItalic.ttf"
        )),
    ];
    cx.text_system().add_fonts(font_files).ok();

    let bounds = Bounds::centered(None, size(px(1100.), px(720.)), cx);
    let titlebar = CurrentPlatform::titlebar_options("AnotherOne");

    cx.bind_keys([
        KeyBinding::new("cmd-f", TerminalFind, None),
        KeyBinding::new("cmd-g", TerminalSearchNext, None),
        KeyBinding::new("cmd-shift-g", TerminalSearchPrev, None),
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

    // On mobile there's no window-close affordance; skip.
    #[cfg(not(target_os = "android"))]
    {
        cx.on_window_closed(|cx, _window_id| {
            cx.quit();
        })
        .detach();
        cx.activate(true);
    }
}

/// Desktop entry — called by `src/main.rs`. macOS/Linux/Windows.
#[cfg(not(target_os = "android"))]
pub fn run_desktop() {
    init_logging();
    leakscope::start_sampler();
    gpui_platform::application()
        .with_assets(ProjectAssets)
        .run(finish_launching);
}

/// Android entry — invoked on a dedicated native thread by the
/// `android-activity` glue once `NativeActivity` finishes loading the
/// `.so`. Mirrors gpui-mobile's example pattern: install logger +
/// panic hook, init the global `AndroidPlatform`, hand it to
/// `gpui::Application::with_platform`, then run.
#[cfg(target_os = "android")]
#[no_mangle]
pub fn android_main(android_app: android_activity::AndroidApp) {
    android_logger::init_once(
        android_logger::Config::default()
            .with_max_level(log::LevelFilter::Info)
            .with_tag("another_one"),
    );
    gpui_mobile::android::jni::install_panic_hook();
    log::info!("android_main: entered");

    // Stash the real Activity pointer for later JNI helpers (e.g. the
    // QR scanner launcher in `mobile.rs`). `ndk_context` is set up by
    // `android-activity`'s glue with the Application global, not the
    // Activity, so anything that needs an `Activity` argument has to
    // capture this here — the AndroidApp goes out of scope below
    // when control hands off to `Application::run`.
    crate::mobile::set_activity_ptr(android_app.activity_as_ptr());

    // Stash the app-private internal data dir so the mobile-side
    // iroh client can persist its secret key across reconnects and
    // `adb install -r` cycles. Without this, every dial generates a
    // fresh key — after the first successful pair the daemon's
    // allowlist has entry `<key_A>`, but the next dial presents
    // `<key_B>` and the daemon rejects with "no outstanding pair
    // nonce" (because the nonce was consumed by the first Hello and
    // never rolled). `AndroidApp::internal_data_path` is the
    // app-private path that survives `adb install -r` but wipes on
    // uninstall, which is exactly the lifecycle we want for pairing
    // identity.
    if let Some(dir) = android_app.internal_data_path() {
        crate::mobile::set_internal_data_path(dir);
    } else {
        log::warn!("android_main: internal_data_path unavailable; iroh key will stay ephemeral");
    }

    // Read the OS dark-mode preference before the first render.
    // gpui-mobile's window-appearance tracking only updates on
    // `ConfigChanged`, which doesn't fire at app start, so without
    // this seeding the first frame renders Light regardless of
    // the phone's setting. See `mobile::system_prefers_dark`.
    {
        use android_activity::ndk::configuration::UiModeNight;
        let prefers_dark = matches!(android_app.config().ui_mode_night(), UiModeNight::Yes);
        crate::mobile::set_system_prefers_dark(prefers_dark);
        log::info!("android_main: system prefers_dark={prefers_dark}");
    }

    let _platform = gpui_mobile::android::jni::init_platform(&android_app);
    let shared = match gpui_mobile::android::jni::shared_platform() {
        Some(s) => s,
        None => {
            log::error!("android_main: shared_platform() returned None");
            return;
        }
    };

    gpui::Application::with_platform(shared.into_rc())
        .with_assets(ProjectAssets)
        .run(finish_launching);

    log::info!("android_main: Application.run returned");
}
