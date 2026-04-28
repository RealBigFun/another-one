use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) fn apply_command_path(command: &mut Command, cwd: &Path) {
    command.env("PATH", command_path_env(cwd));
}

pub(crate) fn command_path_env(cwd: &Path) -> OsString {
    std::env::join_paths(command_path_dirs(
        std::env::var_os("PATH").as_deref(),
        shell_initialized_path_dirs(cwd),
        dirs::home_dir().as_deref(),
    ))
    .unwrap_or_else(|_| std::env::var_os("PATH").unwrap_or_else(|| OsString::from(default_path())))
}

pub(crate) fn command_path_dirs(
    current_path: Option<&OsStr>,
    shell_initialized_dirs: Vec<PathBuf>,
    home: Option<&Path>,
) -> Vec<PathBuf> {
    let mut dirs = shell_initialized_dirs;

    if let Some(path) = current_path {
        dirs.extend(std::env::split_paths(path));
    }

    if let Some(home) = home {
        dirs.push(home.join(".local/bin"));
        dirs.push(home.join(".cargo/bin"));
    }

    dirs.extend(default_path().split(':').map(PathBuf::from));

    let mut unique = Vec::new();
    for dir in dirs {
        if !unique.iter().any(|existing| existing == &dir) {
            unique.push(dir);
        }
    }
    unique
}

pub(crate) fn find_executable(command: &str, cwd: &Path, fallbacks: &[PathBuf]) -> Option<PathBuf> {
    for dir in command_path_dirs(
        std::env::var_os("PATH").as_deref(),
        shell_initialized_path_dirs(cwd),
        dirs::home_dir().as_deref(),
    ) {
        let candidate = dir.join(command);
        if candidate.is_file() {
            return Some(candidate);
        }
    }

    fallbacks.iter().find(|path| path.is_file()).cloned()
}

fn shell_initialized_path_dirs(cwd: &Path) -> Vec<PathBuf> {
    let Some(shell) = user_shell_path() else {
        return Vec::new();
    };

    let Ok(output) = Command::new(shell)
        .args(["-lic", "printf '\\n__ANOTHER_ONE_PATH__%s\\n' \"$PATH\""])
        .current_dir(cwd)
        .output()
    else {
        return Vec::new();
    };

    if !output.status.success() {
        return Vec::new();
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    let Some(path) = stdout
        .lines()
        .rev()
        .find_map(|line| line.strip_prefix("__ANOTHER_ONE_PATH__"))
    else {
        return Vec::new();
    };

    std::env::split_paths(path).collect()
}

fn user_shell_path() -> Option<OsString> {
    if let Some(shell) = std::env::var_os("SHELL").filter(|shell| !shell.is_empty()) {
        return Some(shell);
    }

    #[cfg(target_os = "macos")]
    {
        Some(OsString::from("/bin/zsh"))
    }

    #[cfg(all(unix, not(target_os = "macos")))]
    {
        Some(OsString::from("/bin/bash"))
    }

    #[cfg(not(unix))]
    {
        None
    }
}

fn default_path() -> &'static str {
    #[cfg(target_os = "macos")]
    {
        "/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    }

    #[cfg(not(target_os = "macos"))]
    {
        "/usr/local/bin:/usr/bin:/bin:/usr/local/sbin:/usr/sbin:/sbin:/snap/bin"
    }
}

#[cfg(test)]
mod tests {
    use super::command_path_dirs;
    use std::env;
    use std::path::{Path, PathBuf};

    #[test]
    fn command_path_dirs_prefers_worktree_shell_path() {
        let current_path =
            env::join_paths([PathBuf::from("/app/bin"), PathBuf::from("/shell/node")])
                .expect("test path should be joinable");

        let dirs = command_path_dirs(
            Some(current_path.as_os_str()),
            vec![PathBuf::from("/shell/node"), PathBuf::from("/shell/bin")],
            Some(Path::new("/home/tester")),
        );

        assert_eq!(
            &dirs[..4],
            [
                PathBuf::from("/shell/node"),
                PathBuf::from("/shell/bin"),
                PathBuf::from("/app/bin"),
                PathBuf::from("/home/tester/.local/bin"),
            ]
        );
    }
}
