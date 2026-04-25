use std::process::Command;

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
}
