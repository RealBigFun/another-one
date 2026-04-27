use std::path::Path;
use std::process::Command;

use crate::open_in::OpenInAppKind;

use super::HeadlessPlatform;

#[derive(Clone, Copy, Debug, Default)]
pub struct AndroidPlatform;

impl HeadlessPlatform for AndroidPlatform {
    fn name() -> &'static str {
        "android"
    }

    fn modifier_label() -> &'static str {
        // Android external keyboards (Bluetooth, USB) almost
        // universally ship Ctrl as the primary modifier — Search
        // is technically the Android-flavoured super-key but few
        // physical keyboards expose it. Pragmatically: "Ctrl"
        // matches what users see in app shortcut hints.
        "Ctrl"
    }

    fn open_external_url(_url: &str) -> Result<(), String> {
        // Android opens URLs via `Intent.ACTION_VIEW`, which is
        // reachable from Java/Kotlin only. The future Flutter UI
        // will route URL opens through a Dart platform channel;
        // this Rust-side implementation exists only so the trait
        // shape is the same on every target.
        Err(
            "open_external_url not supported from Rust on Android; use a Dart platform channel"
                .into(),
        )
    }

    fn total_system_memory_bytes() -> Option<u64> {
        // Android's procfs layout matches Linux's for `/proc/meminfo`,
        // so reuse the same parser. Reads may fail on locked-down
        // OEM builds; `Option` lets the resource indicator just
        // hide the total in that case.
        super::linux::proc_meminfo_total_bytes()
    }

    fn read_process_samples(
        app_pid: u32,
        tracked_processes: &[crate::process::TrackedProcess],
    ) -> Vec<crate::process::RawProcessSample> {
        // Same procfs layout as Linux. Note that on modern Android
        // the SELinux policy + per-app sandboxing severely restricts
        // which `/proc/<pid>` directories the app can read — the
        // returned vec on Android will typically only contain the app
        // process plus requested tracked descendants, which is exactly
        // what the resource indicator wants anyway.
        super::linux::procfs_read_process_samples(app_pid, tracked_processes)
    }

    fn is_open_in_app_available(_app: OpenInAppKind) -> bool {
        // Android's open-in story is driven by `Intent.ACTION_VIEW`,
        // which is reachable from Java/Kotlin only. The future
        // Flutter UI will route any "open in" through a Dart
        // platform channel; from Rust we always report unavailable.
        false
    }

    fn command_for_open_in(_app: OpenInAppKind, _path: &Path) -> Command {
        // See [`Self::is_open_in_app_available`]. Placeholder so
        // the trait shape is uniform; should never be invoked.
        // The path is intentionally nonexistent so the spawn fails
        // with ENOENT and the error message points at the right
        // diagnosis ("not supported on this platform").
        Command::new("/nonexistent/another-one-unsupported-on-this-platform")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_returns_android() {
        assert_eq!(AndroidPlatform::name(), "android");
    }

    #[test]
    fn modifier_label_returns_ctrl() {
        assert_eq!(AndroidPlatform::modifier_label(), "Ctrl");
    }

    #[test]
    fn open_external_url_returns_unsupported_error() {
        let result = AndroidPlatform::open_external_url("https://example.com");
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
}
