use std::path::Path;
use std::process::Command;

use crate::open_in::OpenInAppKind;

use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct IosPlatform;

impl HeadlessPlatform for IosPlatform {
    fn name() -> &'static str {
        "ios"
    }

    fn modifier_label() -> &'static str {
        // Hardware keyboards on iOS report the Cmd key as the
        // primary modifier (matching macOS), and the iPad's
        // Magic Keyboard glyph row mirrors the macOS layout.
        "Cmd"
    }

    fn open_external_url(_url: &str) -> Result<(), String> {
        // iOS sandboxes opening URLs behind `UIApplication.open(_:)`,
        // which is reachable from Swift/Objective-C only. The future
        // Flutter UI will route URL opens through a Dart platform
        // channel; this Rust-side implementation exists only so the
        // trait shape is the same on every target.
        Err("open_external_url not supported from Rust on iOS; use a Dart platform channel".into())
    }

    fn total_system_memory_bytes() -> Option<u64> {
        // iOS exposes `sysctlbyname("hw.memsize")` via libc just
        // like macOS. Reuse the same helper so any future fix
        // applies to both Apple platforms.
        super::macos::sysctl_hw_memsize()
    }

    fn read_process_samples(
        app_pid: u32,
        tracked_processes: &[crate::process::TrackedProcess],
    ) -> Vec<crate::process::RawProcessSample> {
        // iOS uses the same Darwin `proc_pidinfo` / `proc_pid_rusage`
        // interfaces as macOS, so reuse the macOS impl. Note that
        // the iOS sandbox may hide processes outside the app's own
        // tree; that's expected and the caller already treats the
        // returned vec as best-effort rather than authoritative.
        super::macos::darwin_read_process_samples(app_pid, tracked_processes)
    }

    fn is_open_in_app_available(_app: OpenInAppKind) -> bool {
        // iOS doesn't have an "open in arbitrary app" primitive
        // accessible to a sandboxed Rust library; the future
        // Flutter UI will route any "open in" gesture through a
        // Dart platform channel that talks directly to UIKit.
        false
    }

    fn command_for_open_in(_app: OpenInAppKind, _path: &Path) -> Command {
        // Placeholder so the trait shape is uniform across targets;
        // [`Self::is_open_in_app_available`] always returns `false`
        // on iOS so callers shouldn't reach this. If they do, the
        // path is intentionally nonexistent so the spawn fails with
        // ENOENT and the error message points at the right
        // diagnosis ("not supported on this platform").
        Command::new("/nonexistent/another-one-unsupported-on-this-platform")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_ios() {
        assert_eq!(IosPlatform::name(), "ios");
    }

    #[test]
    fn modifier_label_returns_cmd() {
        assert_eq!(IosPlatform::modifier_label(), "Cmd");
    }

    #[test]
    fn open_external_url_returns_unsupported_error() {
        let result = IosPlatform::open_external_url("https://example.com");
        assert!(result.is_err());
        assert!(
            result
                .as_ref()
                .unwrap_err()
                .contains("Dart platform channel"),
            "expected the error to point at the Dart-side workaround, got: {:?}",
            result.unwrap_err()
        );
    }

    #[test]
    fn total_system_memory_bytes_is_positive() {
        let memory = IosPlatform::total_system_memory_bytes();
        assert!(memory.is_some(), "expected sysctlbyname to succeed on iOS");
        assert!(
            memory.unwrap() > 0,
            "expected total memory > 0, got {:?}",
            memory
        );
    }
}
