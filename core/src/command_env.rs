use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::sync::OnceLock;
use std::thread;
use std::time::{Duration, Instant};

const SHELL_PATH_DISCOVERY_TIMEOUT: Duration = Duration::from_secs(2);
const SHELL_PATH_DISCOVERY_POLL_INTERVAL: Duration = Duration::from_millis(25);

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
    static SHELL_INITIALIZED_PATH_DIRS: OnceLock<Vec<PathBuf>> = OnceLock::new();

    SHELL_INITIALIZED_PATH_DIRS
        .get_or_init(|| discover_shell_initialized_path_dirs(cwd))
        .clone()
}

fn discover_shell_initialized_path_dirs(cwd: &Path) -> Vec<PathBuf> {
    let Some(shell) = user_shell_path() else {
        return Vec::new();
    };

    let Some(output) = run_shell_path_discovery(shell, cwd) else {
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

fn run_shell_path_discovery(shell: OsString, cwd: &Path) -> Option<Output> {
    let mut command = Command::new(shell);
    command
        .args(["-lic", "printf '\\n__ANOTHER_ONE_PATH__%s\\n' \"$PATH\""])
        .current_dir(cwd)
        .stdout(Stdio::piped())
        .stderr(Stdio::null());

    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        // Keep shell startup hooks in their own process group so a timed-out
        // discovery kills any helper process they spawned (prompt hooks often
        // run git/node/ruby/etc.). Without this, every terminal launch could
        // leave a stuck login-shell helper behind.
        command.process_group(0);
    }

    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + SHELL_PATH_DISCOVERY_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(_)) => return child.wait_with_output().ok(),
            Ok(None) if Instant::now() < deadline => {
                thread::sleep(SHELL_PATH_DISCOVERY_POLL_INTERVAL);
            }
            Ok(None) => {
                #[cfg(unix)]
                kill_process_group(child.id());
                let _ = child.kill();
                let _ = child.wait_with_output();
                return None;
            }
            Err(_) => {
                let _ = child.kill();
                let _ = child.wait_with_output();
                return None;
            }
        }
    }
}

#[cfg(unix)]
fn kill_process_group(pid: u32) {
    if pid <= 1 {
        return;
    }
    unsafe {
        libc::kill(-(pid as i32), libc::SIGKILL);
    }
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
