use std::collections::{HashMap, HashSet};
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
        app_pid: u32,
        tracked_processes: &[TrackedProcess],
    ) -> Vec<RawProcessSample> {
        procfs_read_process_samples(app_pid, tracked_processes)
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
                let mut c = Command::new("flatpak");
                c.args(["run", app_id.as_str()]);
                c
            }
            None => Command::new(default_binary_name(app)),
        };
        command.arg(path);
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
        OpenInAppKind::FileManager => "xdg-open",
    }
}

fn binary_candidates(app: OpenInAppKind) -> &'static [&'static str] {
    match app {
        OpenInAppKind::Cursor => &["cursor"],
        OpenInAppKind::Zed => &["zed", "zeditor"],
        OpenInAppKind::VsCode => &["code", "code-insiders"],
        OpenInAppKind::FileManager => &["xdg-open"],
    }
}

fn flatpak_candidates(app: OpenInAppKind) -> &'static [&'static str] {
    match app {
        OpenInAppKind::Cursor => &[],
        OpenInAppKind::Zed => &["dev.zed.Zed"],
        OpenInAppKind::VsCode => &["com.visualstudio.code"],
        OpenInAppKind::FileManager => &[],
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

/// Sample the app PID plus tracked process trees from procfs.
/// Shared with `AndroidPlatform`, which has the same procfs layout
/// (though sandboxing may hide descendants outside the app's own
/// tree — `Option`s in the caller absorb that gracefully).
///
/// `smaps_rollup` is accurate but expensive; reading it for every
/// readable process was enough to create visible idle CPU spikes on
/// desktops with many processes. Keep app-window memory on cheap RSS
/// and limit PSS reads to tracked subprocess trees shown in the UI.
pub(super) fn procfs_read_process_samples(
    app_pid: u32,
    tracked_processes: &[TrackedProcess],
) -> Vec<RawProcessSample> {
    let clock_ticks_per_second = match sysconf_u64(libc::_SC_CLK_TCK) {
        Some(value) if value > 0 => value,
        _ => return Vec::new(),
    };
    let page_size = match sysconf_u64(libc::_SC_PAGESIZE) {
        Some(value) if value > 0 => value,
        _ => return Vec::new(),
    };

    if tracked_processes.is_empty() {
        return read_procfs_stat_sample(app_pid, clock_ticks_per_second, page_size)
            .map(|sample| sample.into_raw_sample(LinuxMemoryMode::Rss))
            .into_iter()
            .collect();
    }

    let mut sample_by_pid = HashMap::new();
    if let Some(sample) = read_procfs_stat_sample(app_pid, clock_ticks_per_second, page_size) {
        sample_by_pid.insert(app_pid, sample);
    }
    for tracked in tracked_processes {
        collect_procfs_process_tree(
            tracked.pid,
            clock_ticks_per_second,
            page_size,
            &mut sample_by_pid,
        );
    }

    sample_by_pid
        .into_values()
        .map(|sample| {
            let memory_mode = if sample.pid == app_pid {
                LinuxMemoryMode::Rss
            } else {
                LinuxMemoryMode::Pss
            };
            sample.into_raw_sample(memory_mode)
        })
        .collect()
}

fn read_procfs_stat_sample(
    pid: u32,
    clock_ticks_per_second: u64,
    page_size: u64,
) -> Option<LinuxStatSample> {
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).ok()?;
    parse_linux_stat_sample(&stat, pid, clock_ticks_per_second, page_size)
}

fn collect_procfs_process_tree(
    root_pid: u32,
    clock_ticks_per_second: u64,
    page_size: u64,
    samples_by_pid: &mut HashMap<u32, LinuxStatSample>,
) {
    let mut visited = HashSet::new();
    let mut stack = vec![root_pid];
    while let Some(pid) = stack.pop() {
        if !visited.insert(pid) {
            continue;
        }
        if let Some(sample) = read_procfs_stat_sample(pid, clock_ticks_per_second, page_size) {
            samples_by_pid.entry(pid).or_insert(sample);
            stack.extend(procfs_child_pids(pid));
        }
    }
}

fn procfs_child_pids(pid: u32) -> Vec<u32> {
    let Ok(task_entries) = std::fs::read_dir(format!("/proc/{pid}/task")) else {
        return read_procfs_task_children(pid, pid);
    };

    let mut children = HashSet::new();
    for entry in task_entries.flatten() {
        let Some(file_name) = entry.file_name().to_str().map(str::to_string) else {
            continue;
        };
        let Ok(tid) = file_name.parse::<u32>() else {
            continue;
        };
        children.extend(read_procfs_task_children(pid, tid));
    }
    children.into_iter().collect()
}

fn read_procfs_task_children(pid: u32, tid: u32) -> Vec<u32> {
    let Ok(children) = std::fs::read_to_string(format!("/proc/{pid}/task/{tid}/children")) else {
        return Vec::new();
    };
    children
        .split_whitespace()
        .filter_map(|child| child.parse::<u32>().ok())
        .collect()
}

fn parse_linux_stat_sample(
    stat_line: &str,
    pid: u32,
    clock_ticks_per_second: u64,
    page_size: u64,
) -> Option<LinuxStatSample> {
    let comm_end = stat_line.rfind(") ")?;
    let mut fields = stat_line.get(comm_end + 2..)?.split_whitespace();
    fields.next()?;
    let ppid = fields.next()?.parse::<u32>().ok()?;
    let utime_ticks = fields.nth(9)?.parse::<u64>().ok()?;
    let stime_ticks = fields.next()?.parse::<u64>().ok()?;
    let rss_pages = fields.nth(8)?.parse::<i64>().ok()?.max(0) as u64;

    Some(LinuxStatSample {
        pid,
        ppid,
        total_cpu_time_ns: ticks_to_nanos(
            utime_ticks.saturating_add(stime_ticks),
            clock_ticks_per_second,
        ),
        rss_memory_bytes: rss_pages.saturating_mul(page_size),
    })
}

#[derive(Clone, Copy, Debug)]
struct LinuxStatSample {
    pid: u32,
    ppid: u32,
    total_cpu_time_ns: u64,
    rss_memory_bytes: u64,
}

#[derive(Clone, Copy, Debug)]
enum LinuxMemoryMode {
    Rss,
    Pss,
}

impl LinuxStatSample {
    fn into_raw_sample(self, memory_mode: LinuxMemoryMode) -> RawProcessSample {
        let memory_bytes = match memory_mode {
            // App-window memory is a single PID, so RSS is cheap and
            // does not risk double-counting a subprocess tree.
            LinuxMemoryMode::Rss => self.rss_memory_bytes,
            // Tracked subprocess rows can include multiple child
            // processes. PSS avoids double-counting shared library
            // pages when those trees are summed in the UI.
            LinuxMemoryMode::Pss => read_smaps_pss_bytes(self.pid).unwrap_or(self.rss_memory_bytes),
        };
        RawProcessSample {
            pid: self.pid,
            ppid: self.ppid,
            total_cpu_time_ns: self.total_cpu_time_ns,
            memory_bytes,
        }
    }
}

fn read_smaps_pss_bytes(pid: u32) -> Option<u64> {
    let rollup = std::fs::read_to_string(format!("/proc/{pid}/smaps_rollup")).ok()?;
    for line in rollup.lines() {
        let trimmed = line.trim_start();
        if let Some(rest) = trimmed.strip_prefix("Pss:") {
            // "Pss:    1234 kB" → 1234. Always reported in kB.
            let kb: u64 = rest.split_whitespace().next()?.parse().ok()?;
            return Some(kb.saturating_mul(1024));
        }
    }
    None
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

    #[test]
    fn read_process_samples_excludes_untracked_children() {
        let mut tracked_child = std::process::Command::new("sleep")
            .arg("2")
            .spawn()
            .unwrap();
        let mut untracked_child = std::process::Command::new("sleep")
            .arg("2")
            .spawn()
            .unwrap();

        let app_pid = std::process::id();
        let tracked_pid = tracked_child.id();
        let untracked_pid = untracked_child.id();
        let tracked = [tracked_process(tracked_pid)];
        let samples = LinuxPlatform::read_process_samples(app_pid, &tracked);

        let _ = tracked_child.kill();
        let _ = tracked_child.wait();
        let _ = untracked_child.kill();
        let _ = untracked_child.wait();

        assert!(
            samples.iter().any(|sample| sample.pid == app_pid),
            "expected samples to include the app pid {app_pid}"
        );
        assert!(
            samples.iter().any(|sample| sample.pid == tracked_pid),
            "expected samples to include tracked child pid {tracked_pid}"
        );
        assert!(
            !samples.iter().any(|sample| sample.pid == untracked_pid),
            "expected samples to exclude untracked child pid {untracked_pid}"
        );
    }

    fn tracked_process(pid: u32) -> TrackedProcess {
        TrackedProcess {
            pid,
            key: format!("session-{pid}"),
            label: format!("Session {pid}"),
            project_key: "project".to_string(),
            project_label: "Project".to_string(),
            task_key: "task".to_string(),
            task_label: "Task".to_string(),
            icon_path: "",
        }
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
    }
}
