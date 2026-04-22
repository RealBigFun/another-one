use std::collections::HashSet;
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

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
            Self::Cursor => "assets/icons/icons__cursor-ai.svg",
            Self::Zed => "assets/icons/icons__layout-stacked.svg",
            Self::VsCode => "assets/icons/icons__file_icons__code.svg",
            Self::FileManager => "assets/icons/icons__folder-open.svg",
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

pub fn detect_available_open_in_apps() -> Vec<OpenInAppKind> {
    OpenInAppKind::all()
        .into_iter()
        .filter(|app| is_app_available(*app))
        .collect()
}

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

pub fn open_path_in_app(path: &Path, app: OpenInAppKind) -> Result<(), String> {
    let mut command = command_for_app(app, path);
    command
        .spawn()
        .map(|_| ())
        .map_err(|err| format!("Could not open {}: {err}", app.label()))
}

fn is_app_available(app: OpenInAppKind) -> bool {
    #[cfg(target_os = "macos")]
    {
        match app {
            OpenInAppKind::Cursor => {
                macos_app_exists("Cursor") || command_exists(&["cursor", "cursor-cli"])
            }
            OpenInAppKind::Zed => macos_app_exists("Zed") || command_exists(&["zed"]),
            OpenInAppKind::VsCode => {
                macos_app_exists("Visual Studio Code") || command_exists(&["code"])
            }
            OpenInAppKind::FileManager => macos_app_exists("Finder"),
        }
    }

    #[cfg(target_os = "linux")]
    {
        match app {
            OpenInAppKind::Cursor => command_exists(&["cursor"]),
            OpenInAppKind::Zed => command_exists(&["zed"]),
            OpenInAppKind::VsCode => command_exists(&["code"]),
            OpenInAppKind::FileManager => command_exists(&["xdg-open"]),
        }
    }

    #[cfg(target_os = "windows")]
    {
        match app {
            OpenInAppKind::Cursor => command_exists(&["cursor"]),
            OpenInAppKind::Zed => command_exists(&["zed"]),
            OpenInAppKind::VsCode => command_exists(&["code"]),
            OpenInAppKind::FileManager => true,
        }
    }
}

fn command_for_app(app: OpenInAppKind, path: &Path) -> Command {
    #[cfg(target_os = "macos")]
    {
        let mut command = Command::new("open");
        match app {
            OpenInAppKind::Cursor => {
                command.args(["-a", "Cursor"]).arg(path);
            }
            OpenInAppKind::Zed => {
                command.args(["-a", "Zed"]).arg(path);
            }
            OpenInAppKind::VsCode => {
                command.args(["-a", "Visual Studio Code"]).arg(path);
            }
            OpenInAppKind::FileManager => {
                command.arg(path);
            }
        }
        return command;
    }

    #[cfg(target_os = "linux")]
    {
        let mut command = match app {
            OpenInAppKind::Cursor => Command::new("cursor"),
            OpenInAppKind::Zed => Command::new("zed"),
            OpenInAppKind::VsCode => Command::new("code"),
            OpenInAppKind::FileManager => Command::new("xdg-open"),
        };
        command.arg(path);
        return command;
    }

    #[cfg(target_os = "windows")]
    {
        let mut command = match app {
            OpenInAppKind::Cursor => Command::new("cursor"),
            OpenInAppKind::Zed => Command::new("zed"),
            OpenInAppKind::VsCode => Command::new("code"),
            OpenInAppKind::FileManager => Command::new("explorer"),
        };
        command.arg(path);
        command
    }
}

#[cfg(target_os = "macos")]
fn macos_app_exists(app_name: &str) -> bool {
    macos_app_candidates(app_name)
        .into_iter()
        .any(|path| path.exists())
}

#[cfg(target_os = "macos")]
fn macos_app_candidates(app_name: &str) -> Vec<PathBuf> {
    let bundle_name = format!("{app_name}.app");
    let mut candidates = vec![
        PathBuf::from("/Applications").join(&bundle_name),
        PathBuf::from("/System/Applications").join(&bundle_name),
        PathBuf::from("/System/Library/CoreServices").join(&bundle_name),
    ];

    if let Some(home_dir) = dirs::home_dir() {
        candidates.push(home_dir.join("Applications").join(bundle_name));
    }

    candidates
}

fn command_exists(commands: &[&str]) -> bool {
    commands.iter().any(|command| command_in_path(command).is_some())
}

fn command_in_path(command: &str) -> Option<PathBuf> {
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
        let configured =
            HashSet::from([OpenInAppKind::VsCode, OpenInAppKind::Cursor]);

        assert_eq!(
            effective_enabled_open_in_apps(&available, Some(&configured)),
            vec![OpenInAppKind::Cursor, OpenInAppKind::VsCode]
        );
    }
}
