use std::path::{Path, PathBuf};
use std::process::Command;

use crate::open_in::{command_exists, command_in_path, OpenInAppKind};
use crate::process::{RawProcessSample, TrackedProcess};

use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct LinuxPlatform;

impl HeadlessPlatform for LinuxPlatform {
    fn name() -> &'static str {
        "linux"
    }

    fn modifier_label() -> &'static str {
        "Super"
    }

    fn open_external_url(url: &str) -> Result<(), String> {
        Command::new("xdg-open")
            .arg(url)
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("Could not open URL externally: {err}"))
    }

    fn total_system_memory_bytes() -> Option<u64> {
        proc_meminfo_total_bytes()
    }

    fn read_process_samples(
        _app_pid: u32,
        _tracked_processes: &[TrackedProcess],
    ) -> Vec<RawProcessSample> {
        procfs_read_process_samples()
    }

    fn is_open_in_app_available(app: OpenInAppKind) -> bool {
        if matches!(app, OpenInAppKind::FileManager) {
            return command_exists(&["xdg-open"]);
        }
        find_launcher_on_host(app).is_some()
    }

    fn command_for_open_in(app: OpenInAppKind, path: &Path) -> Command {
        if matches!(app, OpenInAppKind::FileManager) {
            let mut command = Command::new("xdg-open");
            command.arg(path);
            return command;
        }

        let launcher = find_launcher_on_host(app);
        let mut command = match launcher {
            Some(LinuxLauncher::Binary(bin)) => Command::new(bin),
            Some(LinuxLauncher::Flatpak(app_id)) => {
                let mut command = Command::new("flatpak");
                command.args(["run", app_id.as_str()]);
                command
            }
            None => Command::new(default_binary_name(app)),
        };
        apply_open_in_args(&mut command, app, path);
        command
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum LinuxLauncher {
    Binary(PathBuf),
    Flatpak(String),
}

fn default_binary_name(app: OpenInAppKind) -> &'static str {
    match app {
        OpenInAppKind::Cursor => "cursor",
        OpenInAppKind::Zed => "zed",
        OpenInAppKind::VsCode => "code",
        OpenInAppKind::Ghostty => "ghostty",
        OpenInAppKind::WezTerm => "wezterm",
        OpenInAppKind::FileManager => "xdg-open",
    }
}

fn binary_candidates(app: OpenInAppKind) -> &'static [&'static str] {
    match app {
        OpenInAppKind::Cursor => &["cursor"],
        OpenInAppKind::Zed => &["zed", "zeditor"],
        OpenInAppKind::VsCode => &["code", "code-insiders"],
        OpenInAppKind::Ghostty => &["ghostty"],
        OpenInAppKind::WezTerm => &["wezterm", "wezterm-gui"],
        OpenInAppKind::FileManager => &["xdg-open"],
    }
}

fn flatpak_candidates(app: OpenInAppKind) -> &'static [&'static str] {
    match app {
        OpenInAppKind::Cursor => &[],
        OpenInAppKind::Zed => &["dev.zed.Zed"],
        OpenInAppKind::VsCode => &["com.visualstudio.code"],
        OpenInAppKind::Ghostty => &["com.mitchellh.ghostty"],
        OpenInAppKind::WezTerm => &["org.wezfurlong.wezterm"],
        OpenInAppKind::FileManager => &[],
    }
}

fn apply_open_in_args(command: &mut Command, app: OpenInAppKind, path: &Path) {
    match app {
        OpenInAppKind::Cursor | OpenInAppKind::Zed | OpenInAppKind::VsCode => {
            command.arg(path);
        }
        OpenInAppKind::Ghostty => {
            command
                .arg("+new-window")
                .arg(format!("--working-directory={}", path.display()));
        }
        OpenInAppKind::WezTerm => {
            command
                .arg("start")
                .arg("--always-new-process")
                .arg("--cwd")
                .arg(path);
        }
        OpenInAppKind::FileManager => {
            command.arg(path);
        }
    }
}

fn find_launcher_on_host(app: OpenInAppKind) -> Option<LinuxLauncher> {
    for name in binary_candidates(app) {
        if let Some(path) = command_in_path(name) {
            return Some(LinuxLauncher::Binary(path));
        }
    }
    let extra_dirs = host_extra_dirs();
    if let Some(launcher) = find_launcher_in_dirs(app, &extra_dirs) {
        return Some(launcher);
    }
    for app_id in flatpak_candidates(app) {
        if flatpak_app_installed(app_id) {
            return Some(LinuxLauncher::Flatpak((*app_id).to_string()));
        }
    }
    None
}

fn host_extra_dirs() -> Vec<PathBuf> {
    let mut dirs = vec![PathBuf::from("/snap/bin")];
    if let Some(home) = dirs::home_dir() {
        dirs.push(home.join(".local/share/flatpak/exports/bin"));
    }
    dirs.push(PathBuf::from("/var/lib/flatpak/exports/bin"));
    dirs
}

fn find_launcher_in_dirs(app: OpenInAppKind, dirs: &[PathBuf]) -> Option<LinuxLauncher> {
    for name in binary_candidates(app) {
        for dir in dirs {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(LinuxLauncher::Binary(candidate));
            }
        }
    }
    for app_id in flatpak_candidates(app) {
        for dir in dirs {
            if dir.join(app_id).is_file() {
                return Some(LinuxLauncher::Flatpak((*app_id).to_string()));
            }
        }
    }
    None
}

fn flatpak_app_installed(app_id: &str) -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    let user = home.join(".local/share/flatpak/app").join(app_id);
    let system = PathBuf::from("/var/lib/flatpak/app").join(app_id);
    user.is_dir() || system.is_dir()
}

/// Parse `MemTotal:` from `/proc/meminfo` and convert KiB to bytes.
/// Shared with `AndroidPlatform`, which uses the same procfs layout.
pub(super) fn proc_meminfo_total_bytes() -> Option<u64> {
    let meminfo = std::fs::read_to_string("/proc/meminfo").ok()?;
    let line = meminfo.lines().find(|line| line.starts_with("MemTotal:"))?;
    let kib = line
        .split_whitespace()
        .nth(1)
        .and_then(|value| value.parse::<u64>().ok())?;
    Some(kib.saturating_mul(1024))
}

/// Walk every PID under `/proc`, parsing `stat` for CPU + RSS.
/// Shared with `AndroidPlatform`, which has the same procfs layout
/// (though sandboxing may hide descendants outside the app's own
/// tree — `Option`s in the caller absorb that gracefully).
///
/// Note: unlike the Darwin impl, this doesn't take `app_pid` /
/// `tracked_processes` because `/proc` enumeration already
/// surfaces every readable process; the caller filters down by
/// PID match.
pub(super) fn procfs_read_process_samples() -> Vec<RawProcessSample> {
    let clock_ticks_per_second = match sysconf_u64(libc::_SC_CLK_TCK) {
        Some(value) if value > 0 => value,
        _ => return Vec::new(),
    };
    let page_size = match sysconf_u64(libc::_SC_PAGESIZE) {
        Some(value) if value > 0 => value,
        _ => return Vec::new(),
    };

    let Ok(entries) = std::fs::read_dir("/proc") else {
        return Vec::new();
    };

    entries
        .filter_map(|entry| {
            let entry = entry.ok()?;
            let file_name = entry.file_name();
            let file_name = file_name.to_str()?;
            let pid = file_name.parse::<u32>().ok()?;
            let stat_path = entry.path().join("stat");
            let stat = std::fs::read_to_string(stat_path).ok()?;
            parse_linux_process_sample(&stat, pid, clock_ticks_per_second, page_size)
        })
        .collect()
}

pub(super) fn parse_linux_process_sample(
    stat_line: &str,
    pid: u32,
    clock_ticks_per_second: u64,
    page_size: u64,
) -> Option<RawProcessSample> {
    let comm_end = stat_line.rfind(") ")?;
    let fields = stat_line
        .get(comm_end + 2..)?
        .split_whitespace()
        .collect::<Vec<_>>();
    let ppid = fields.get(1)?.parse::<u32>().ok()?;
    let utime_ticks = fields.get(11)?.parse::<u64>().ok()?;
    let stime_ticks = fields.get(12)?.parse::<u64>().ok()?;
    let rss_pages = fields.get(21)?.parse::<i64>().ok()?.max(0) as u64;

    Some(RawProcessSample {
        pid,
        ppid,
        total_cpu_time_ns: ticks_to_nanos(
            utime_ticks.saturating_add(stime_ticks),
            clock_ticks_per_second,
        ),
        memory_bytes: rss_pages.saturating_mul(page_size),
    })
}

pub(super) fn ticks_to_nanos(ticks: u64, clock_ticks_per_second: u64) -> u64 {
    ticks.saturating_mul(1_000_000_000) / clock_ticks_per_second
}

fn sysconf_u64(name: libc::c_int) -> Option<u64> {
    let value = unsafe { libc::sysconf(name) };
    (value > 0).then_some(value as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_linux() {
        assert_eq!(LinuxPlatform::name(), "linux");
    }

    #[test]
    fn modifier_label_returns_super() {
        assert_eq!(LinuxPlatform::modifier_label(), "Super");
    }

    #[test]
    fn total_system_memory_bytes_is_positive() {
        let memory = LinuxPlatform::total_system_memory_bytes();
        assert!(
            memory.is_some(),
            "expected /proc/meminfo to be readable on Linux"
        );
        assert!(
            memory.unwrap() > 0,
            "expected total memory > 0, got {:?}",
            memory
        );
    }

    #[test]
    fn converts_linux_ticks_to_nanoseconds() {
        assert_eq!(ticks_to_nanos(250, 100), 2_500_000_000);
    }

    #[test]
    fn read_process_samples_returns_self() {
        let pid = std::process::id();
        let samples = LinuxPlatform::read_process_samples(pid, &[]);
        assert!(
            samples.iter().any(|s| s.pid == pid),
            "expected the /proc walk to include our own pid {}",
            pid
        );
    }

    mod find_launcher_in_dirs_tests {
        use super::super::{find_launcher_in_dirs, LinuxLauncher};
        use crate::open_in::OpenInAppKind;
        use std::fs;
        use std::os::unix::fs::PermissionsExt;
        use std::path::PathBuf;

        fn make_exec(dir: &PathBuf, name: &str) -> PathBuf {
            fs::create_dir_all(dir).unwrap();
            let path = dir.join(name);
            fs::write(&path, b"#!/bin/sh\n").unwrap();
            let mut perms = fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            fs::set_permissions(&path, perms).unwrap();
            path
        }

        #[test]
        fn finds_snap_wrapper_by_short_name() {
            let tmp = tempfile::tempdir().unwrap();
            let snap_dir = tmp.path().join("snap-bin");
            make_exec(&snap_dir, "code");
            let launcher = find_launcher_in_dirs(OpenInAppKind::VsCode, &[snap_dir.clone()]);
            assert_eq!(launcher, Some(LinuxLauncher::Binary(snap_dir.join("code"))));
        }

        #[test]
        fn finds_flatpak_wrapper_by_app_id() {
            let tmp = tempfile::tempdir().unwrap();
            let flatpak_dir = tmp.path().join("flatpak-exports-bin");
            make_exec(&flatpak_dir, "dev.zed.Zed");
            let launcher = find_launcher_in_dirs(OpenInAppKind::Zed, &[flatpak_dir]);
            assert_eq!(launcher, Some(LinuxLauncher::Flatpak("dev.zed.Zed".into())));
        }

        #[test]
        fn prefers_binary_short_name_over_flatpak_id() {
            let tmp = tempfile::tempdir().unwrap();
            let mixed_dir = tmp.path().join("mixed");
            make_exec(&mixed_dir, "zed");
            make_exec(&mixed_dir, "dev.zed.Zed");
            let launcher = find_launcher_in_dirs(OpenInAppKind::Zed, &[mixed_dir.clone()]);
            assert_eq!(launcher, Some(LinuxLauncher::Binary(mixed_dir.join("zed"))));
        }

        #[test]
        fn returns_none_when_nothing_in_extra_dirs() {
            let tmp = tempfile::tempdir().unwrap();
            let empty = tmp.path().join("empty");
            fs::create_dir_all(&empty).unwrap();
            assert!(find_launcher_in_dirs(OpenInAppKind::Cursor, &[empty]).is_none());
        }

        #[test]
        fn finds_ghostty_flatpak_wrapper_by_app_id() {
            let tmp = tempfile::tempdir().unwrap();
            let flatpak_dir = tmp.path().join("flatpak-exports-bin");
            make_exec(&flatpak_dir, "com.mitchellh.ghostty");
            let launcher = find_launcher_in_dirs(OpenInAppKind::Ghostty, &[flatpak_dir]);
            assert_eq!(launcher, Some(LinuxLauncher::Flatpak("com.mitchellh.ghostty".into())));
        }

        #[test]
        fn finds_wezterm_binary_by_gui_name() {
            let tmp = tempfile::tempdir().unwrap();
            let bin_dir = tmp.path().join("bin");
            make_exec(&bin_dir, "wezterm-gui");
            let launcher = find_launcher_in_dirs(OpenInAppKind::WezTerm, &[bin_dir.clone()]);
            assert_eq!(launcher, Some(LinuxLauncher::Binary(bin_dir.join("wezterm-gui"))));
        }
    }
}
