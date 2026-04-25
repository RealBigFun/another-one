//! Platform-coupled side of "Open In…": OS detection for which apps are
//! installed, and `Command` construction to actually launch them.
//!
//! The pure data type `OpenInAppKind` lives in the core crate at
//! `another_one_core::open_in`; we re-export it so pre-existing
//! `crate::open_in::OpenInAppKind` paths keep resolving.
//! `effective_enabled_open_in_apps` also lives in core, but only
//! `project_store` consumed it — nothing in desktop does today, so
//! the re-export isn't maintained here.
//!
//! Why the split: desktop's `platform/` module is GPUI-coupled
//! (titlebar, window, appkit dock), which means anything that calls
//! `CurrentPlatform::is_open_in_app_available` has to stay in desktop.
//! But `ProjectStore` only cares about *which* apps are configured, not
//! how they're detected or launched — so the enum + filter moved to
//! core while the detection + exec stayed here.

use std::path::Path;
use std::process::Command;

pub use another_one_core::open_in::OpenInAppKind;

use crate::platform::{CurrentPlatform, PlatformServices};

pub fn detect_available_open_in_apps() -> Vec<OpenInAppKind> {
    OpenInAppKind::all()
        .into_iter()
        .filter(|app| is_app_available(*app))
        .collect()
}

pub fn open_path_in_app(path: &Path, app: OpenInAppKind) -> Result<(), String> {
    let mut command = command_for_app(app, path);
    command
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open {}: {err}", app.label()))
}

fn is_app_available(app: OpenInAppKind) -> bool {
    CurrentPlatform::is_open_in_app_available(app)
}

fn command_for_app(app: OpenInAppKind, path: &Path) -> Command {
    CurrentPlatform::command_for_open_in(app, path)
}

