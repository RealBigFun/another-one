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

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OpenInAppKind {
    Cursor,
    Zed,
    VsCode,
    FileManager,
}

impl OpenInAppKind {
    pub const fn all() -> [Self; 4] {
        [Self::Cursor, Self::Zed, Self::VsCode, Self::FileManager]
    }

    pub const fn label(self) -> &'static str {
        match self {
            Self::Cursor => "Cursor",
            Self::Zed => "Zed",
            Self::VsCode => "VS Code",
            Self::FileManager => file_manager_label(),
        }
    }

    pub const fn description(self) -> &'static str {
        match self {
            Self::Cursor => "Open the project directory in Cursor.",
            Self::Zed => "Open the project directory in Zed.",
            Self::VsCode => "Open the project directory in VS Code.",
            Self::FileManager => file_manager_description(),
        }
    }

    pub const fn icon_path(self) -> &'static str {
        match self {
            Self::Cursor => "assets/icons/open_in__cursor.svg",
            Self::Zed => "assets/icons/open_in__zed.svg",
            Self::VsCode => "assets/icons/open_in__vscode.svg",
            Self::FileManager => "assets/icons/open_in__folder_closed.svg",
        }
    }

    pub const fn id(self) -> &'static str {
        match self {
            Self::Cursor => "cursor",
            Self::Zed => "zed",
            Self::VsCode => "vscode",
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
            OpenInAppKind::Cursor,
        ];
        let configured = HashSet::from([OpenInAppKind::VsCode, OpenInAppKind::Cursor]);

        assert_eq!(
            effective_enabled_open_in_apps(&available, Some(&configured)),
            vec![OpenInAppKind::Cursor, OpenInAppKind::VsCode]
        );
    }
}
