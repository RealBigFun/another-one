use std::path::Path;
use std::process::Command;

use crate::open_in::{command_exists, OpenInAppKind};

use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct WindowsPlatform;

impl HeadlessPlatform for WindowsPlatform {
    fn name() -> &'static str {
        "windows"
    }

    fn modifier_label() -> &'static str {
        "Win"
    }

    fn open_external_url(url: &str) -> Result<(), String> {
        // The empty string after `start` is the window-title
        // placeholder — without it `start` would interpret a
        // quoted URL itself as the title.
        Command::new("cmd")
            .args(["/C", "start", "", url])
            .spawn()
            .map(|_| ())
            .map_err(|err| format!("Could not open URL externally: {err}"))
    }

    fn total_system_memory_bytes() -> Option<u64> {
        // Matches the existing GPUI desktop behaviour: no Windows
        // implementation yet. Wiring `GlobalMemoryStatusEx` from
        // `windows-sys` is straightforward but pulls in the
        // `windows-sys` dep; defer until someone needs the value
        // on Windows. The resource indicator UI hides the total
        // when this is `None`.
        None
    }

    fn read_process_samples(
        _app_pid: u32,
        _tracked_processes: &[crate::process::TrackedProcess],
    ) -> Vec<crate::process::RawProcessSample> {
        // Matches existing desktop behaviour. Windows process
        // enumeration via `CreateToolhelp32Snapshot` +
        // `Process32NextW` is straightforward but again wants
        // `windows-sys`; deferred until Windows becomes a real
        // target. The resource indicator hides per-process rows
        // when this is empty.
        Vec::new()
    }

    fn is_open_in_app_available(app: OpenInAppKind) -> bool {
        match app {
            OpenInAppKind::Cursor => command_exists(&["cursor"]),
            OpenInAppKind::Zed => command_exists(&["zed"]),
            OpenInAppKind::VsCode => command_exists(&["code"]),
            OpenInAppKind::FileManager => true,
        }
    }

    fn command_for_open_in(app: OpenInAppKind, path: &Path) -> Command {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_windows() {
        assert_eq!(WindowsPlatform::name(), "windows");
    }

    #[test]
    fn modifier_label_returns_win() {
        assert_eq!(WindowsPlatform::modifier_label(), "Win");
    }

    #[test]
    fn total_system_memory_bytes_returns_none() {
        assert!(WindowsPlatform::total_system_memory_bytes().is_none());
    }

    #[test]
    fn read_process_samples_returns_empty() {
        assert!(WindowsPlatform::read_process_samples(0, &[]).is_empty());
    }
}
