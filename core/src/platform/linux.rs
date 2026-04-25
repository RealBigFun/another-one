use std::process::Command;

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
}
