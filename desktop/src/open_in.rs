//! Platform-coupled side of "Open In…": OS detection for which apps are
//! installed, and `Command` construction to actually launch them.
//!
//! The pure data types (`OpenInAppKind` + [`effective_enabled_open_in_apps`])
//! live in the core crate at `another_one_core::open_in` — this file
//! re-exports them so pre-existing `crate::open_in::…` paths keep
//! resolving.
//!
//! Why the split: desktop's `platform/` module is GPUI-coupled
//! (titlebar, window, appkit dock), which means anything that calls
//! `CurrentPlatform::is_open_in_app_available` has to stay in desktop.
//! But `ProjectStore` only cares about *which* apps are configured, not
//! how they're detected or launched — so the enum + filter moved to
//! core while the detection + exec stayed here.

use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

pub use another_one_core::open_in::{effective_enabled_open_in_apps, OpenInAppKind};

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

pub(crate) fn command_exists(commands: &[&str]) -> bool {
    commands
        .iter()
        .any(|command| command_in_path(command).is_some())
}

pub(crate) fn command_in_path(command: &str) -> Option<PathBuf> {
    let path = env::var_os("PATH")?;

    env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(command);
        if is_executable(&candidate) {
            return Some(candidate);
        }

        #[cfg(target_os = "windows")]
        {
            let candidate = dir.join(format!("{command}.exe"));
            if is_executable(&candidate) {
                return Some(candidate);
            }
        }

        None
    })
}

fn is_executable(path: &Path) -> bool {
    path.is_file()
}
