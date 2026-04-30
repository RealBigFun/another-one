//! Open-in-app configuration types.
//!
//! Pure data: the [`OpenInAppKind`] enum and the filtering helper
//! [`effective_enabled_open_in_apps`] live here so they can be
//! consumed by the headless desktop-store layer and (eventually) the
//! daemon/mobile without pulling the GPUI-coupled platform dispatch
//! that actually *runs* those commands.
//!
//! The matching platform-coupled side — OS detection for which apps
//! are installed, and `Command` construction — lives in
//! `desktop/src/open_in.rs`.

use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenInAppKind {
    Cursor,
    Zed,
    VsCode,
    Ghostty,
    FileManager,
}

impl OpenInAppKind {
    pub const fn all() -> [Self; 5] {
        [
            Self::Cursor,
            Self::Zed,
            Self::VsCode,
            Self::Ghostty,
            Self::FileManager,
        ]
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Zed => "Zed",
            Self::VsCode => "VS Code",
            Self::Ghostty => "Ghostty",
            Self::FileManager => file_manager_label(),
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Cursor => "Open the project directory in Cursor.",
            Self::Zed => "Open the project directory in Zed.",
            Self::VsCode => "Open the project directory in VS Code.",
            Self::Ghostty => "Open the project directory in Ghostty.",
            Self::FileManager => file_manager_description(),
        }
    }

    pub const fn icon_path(self) -> &'static str {
        match self {
            Self::Cursor => "assets/icons/open_in__cursor.svg",
            Self::Zed => "assets/icons/open_in__zed.svg",
            Self::VsCode => "assets/icons/open_in__vscode.svg",
            Self::Ghostty => "assets/icons/open_in__ghostty.svg",
            Self::FileManager => "assets/icons/open_in__folder_closed.svg",
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Zed => "zed",
            Self::VsCode => "vscode",
            Self::Ghostty => "ghostty",
            Self::FileManager => "file-manager",
        }
    }
}

/// Intersect the user-configured enabled set with what's actually
/// installed on the host, returning apps in `OpenInAppKind::all()`
/// order. A `None` configured-set means "all available are enabled"
/// — that's the default on first launch.
pub fn effective_enabled_open_in_apps(
    available: &[OpenInAppKind],
    configured: Option<&HashSet<OpenInAppKind>>,
) -> Vec<OpenInAppKind> {
    let available = available.iter().copied().collect::<HashSet<_>>();

    OpenInAppKind::all()
        .into_iter()
        .filter(|app| available.contains(app))
        .filter(|app| configured.is_none_or(|enabled| enabled.contains(app)))
        .collect()
}

const fn file_manager_label() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Finder"
    }

    #[cfg(target_os = "linux")]
    {
        "File Manager"
    }

    #[cfg(target_os = "windows")]
    {
        "File Explorer"
    }

    #[cfg(target_os = "ios")]
    {
        "Files"
    }

    #[cfg(target_os = "android")]
    {
        "Files"
    }
}

const fn file_manager_description() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "Open the project directory in Finder."
    }

    #[cfg(target_os = "linux")]
    {
        "Open the project directory in your system file manager."
    }

    #[cfg(target_os = "windows")]
    {
        "Open the project directory in File Explorer."
    }

    #[cfg(target_os = "ios")]
    {
        "Open the project directory in the Files app."
    }

    #[cfg(target_os = "android")]
    {
        "Open the project directory in Files."
    }
}

/// Returns `true` if any of `commands` is in the user's `$PATH`.
/// Lifted from desktop because every platform's
/// `is_open_in_app_available` impl needs it; pure Rust, no
/// platform-specific syscalls.
pub fn command_exists(commands: &[&str]) -> bool {
    commands
        .iter()
        .any(|command| command_in_path(command).is_some())
}

/// Returns the absolute path to `command` if it's found in `$PATH`,
/// honouring Windows's `.exe` suffix convention. Used by Linux's
/// flatpak/snap launcher discovery and by every desktop platform's
/// "open in app" availability check.
pub fn command_in_path(command: &str) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::{effective_enabled_open_in_apps, OpenInAppKind};

    #[test]
    fn enabled_apps_default_to_all_available_apps() {
        let available = vec![OpenInAppKind::Cursor, OpenInAppKind::FileManager];

        assert_eq!(
            effective_enabled_open_in_apps(&available, None),
            vec![OpenInAppKind::Cursor, OpenInAppKind::FileManager]
        );
    }

    #[test]
    fn enabled_apps_follow_saved_subset_in_stable_order() {
        let available = vec![
            OpenInAppKind::FileManager,
            OpenInAppKind::VsCode,
            OpenInAppKind::Ghostty,
            OpenInAppKind::Cursor,
        ];
        let configured = HashSet::from([
            OpenInAppKind::VsCode,
            OpenInAppKind::Ghostty,
            OpenInAppKind::Cursor,
        ]);

        assert_eq!(
            effective_enabled_open_in_apps(&available, Some(&configured)),
            vec![
                OpenInAppKind::Cursor,
                OpenInAppKind::VsCode,
                OpenInAppKind::Ghostty
            ]
        );
    }

    #[test]
    fn ghostty_deserializes_from_saved_settings() {
        assert_eq!(
            serde_json::from_str::<OpenInAppKind>("\"ghostty\"").unwrap(),
            OpenInAppKind::Ghostty
        );
    }
}
