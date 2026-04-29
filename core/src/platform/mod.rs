//! Headless platform abstraction shared by every target shell.
//!
//! Today's `desktop/src/platform/` covers macOS, Linux, and Windows
//! and is GPUI-coupled (returns `gpui::TitlebarOptions`, takes
//! `&mut App`, etc.). That trait is doomed to die with the GPUI
//! binary — but a *subset* of it is pure Rust (URL opening, process
//! samples, system memory, modifier labels, open-in-app detection)
//! and survives.
//!
//! This module is the surviving subset, hosted in `core/` so every
//! target can link against it. iOS and Android land here as
//! first-class siblings of the existing three desktop platforms.
//!
//! Per-platform module structure follows the same convention used
//! in `desktop/src/platform/`:
//!
//! * One file per target (`macos.rs`, `linux.rs`, etc.)
//! * Each file declares a unit struct (e.g. `MacosPlatform`) that
//!   implements the [`HeadlessPlatform`] trait.
//! * `CurrentPlatform` is a `pub use` alias selected by `cfg(target_os)`,
//!   so call sites write `CurrentPlatform::foo()` with no runtime
//!   cost — same shape the desktop crate has today.
//!
//! Each `*.rs` is a stub at this commit; subsequent PRs migrate the
//! actual methods out of `desktop/src/platform/`.

// Each `mod` declaration is cfg-gated to its own target so unused
// platforms don't trigger dead-code warnings during normal builds.
// All five files live on disk regardless — `cargo check
// --target=…` against any of them is the way to verify a foreign
// platform still compiles after a refactor.

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "ios")]
mod ios;
// Compile the Linux module on Android too — `AndroidPlatform`
// reuses `proc_meminfo_total_bytes` + `procfs_read_process_samples`
// from it (Android's procfs layout matches Linux's). Keeps the
// memory + sample helpers in a single place rather than duplicating
// them across two `target_os` impls. The `LinuxPlatform` re-export
// below stays gated to `target_os = "linux"`, so Android still
// resolves `CurrentPlatform` to `AndroidPlatform`.
#[cfg(any(target_os = "linux", target_os = "android"))]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "android")]
pub use android::AndroidPlatform as CurrentPlatform;
#[cfg(target_os = "ios")]
pub use ios::IosPlatform as CurrentPlatform;
#[cfg(target_os = "linux")]
pub use linux::LinuxPlatform as CurrentPlatform;
#[cfg(target_os = "macos")]
pub use macos::MacosPlatform as CurrentPlatform;
#[cfg(target_os = "windows")]
pub use windows::WindowsPlatform as CurrentPlatform;

/// The headless half of the platform abstraction.
///
/// Methods here must compile and behave correctly on every target.
/// GPUI-shaped methods (titlebar metrics, dock icon, custom chrome)
/// live in `desktop/src/platform/` and are deleted with the GPUI
/// binary; they should NOT be added here.
///
/// New methods land here when:
/// * They have a sensible implementation on at least three of the
///   five targets (otherwise it's probably platform-specific glue
///   that belongs elsewhere), AND
/// * The shape is portable (no `gpui::*` types, no `&App`, no
///   `Window`).
pub trait HeadlessPlatform {
    /// Identifier used in logs and the build-mode tooltip. Free-form
    /// but stable; today's targets return `"macos"`, `"linux"`,
    /// `"windows"`, `"ios"`, `"android"`.
    fn name() -> &'static str;

    /// Display label for the platform's primary keyboard-shortcut
    /// modifier key, as shown to the user in keybinding UI.
    ///
    /// Examples: macOS → `"Cmd"`, Linux → `"Super"`, Windows →
    /// `"Win"`. The strings are exactly what the desktop UI has
    /// rendered in the keybindings list historically; preserve them
    /// verbatim so existing screenshots and muscle memory don't drift.
    fn modifier_label() -> &'static str;

    /// Open `url` in the system's default external handler.
    ///
    /// Side-effecting: spawns a child process (`open`, `xdg-open`,
    /// `cmd /C start "" …` on the three desktop platforms). On
    /// iOS and Android, where the shell must invoke platform UI APIs,
    /// this returns `Err` so callers can surface the limitation and
    /// delegate to platform integration code.
    fn open_external_url(url: &str) -> Result<(), String>;

    /// Total physical RAM in bytes, or `None` if the platform
    /// doesn't expose a cheap query for it.
    ///
    /// macOS / iOS use `sysctlbyname("hw.memsize")`; Linux /
    /// Android parse `/proc/meminfo`. Windows currently returns
    /// `None` (the desktop UI's resource indicator hides the
    /// total when this is missing). The values are reported
    /// once at startup and don't update — so a syscall per call
    /// is fine; no caching layer is warranted.
    fn total_system_memory_bytes() -> Option<u64>;

    /// Sample CPU + memory for the given process tree.
    ///
    /// `app_pid` is the host UI process; `tracked_processes` are
    /// child processes the UI is interested in by name (PTY-spawned
    /// agents, etc.). The implementation walks descendants of each
    /// root and returns one [`RawProcessSample`] per process it
    /// can read.
    ///
    /// macOS / iOS go through `proc_pidinfo` + `proc_pid_rusage`;
    /// Linux / Android parse `/proc/<pid>/stat`. Windows returns
    /// an empty vec — the desktop's resource indicator hides
    /// per-process rows when the sampler returns nothing.
    fn read_process_samples(
        app_pid: u32,
        tracked_processes: &[crate::process::TrackedProcess],
    ) -> Vec<crate::process::RawProcessSample>;

    /// Whether `app` looks installed on the current host — i.e.
    /// whether the "Open in …" menu should offer it.
    ///
    /// Detection is best-effort and platform-specific:
    ///   * macOS — checks `/Applications`-style bundle paths plus
    ///     `$PATH` for the CLI shim.
    ///   * Linux — checks `$PATH`, snap (`/snap/bin`), and flatpak
    ///     install dirs.
    ///   * Windows — `$PATH` only.
    ///   * iOS / Android — always `false`; Slint platform integration
    ///     owns app-specific open-in routing on those targets.
    fn is_open_in_app_available(app: crate::open_in::OpenInAppKind) -> bool;

    /// A `Command` ready to spawn that opens `path` in `app`.
    ///
    /// Caller spawns and handles errors; this method just builds
    /// the invocation. iOS / Android return a placeholder
    /// `Command` that won't successfully spawn (matches the
    /// "always unavailable" contract from
    /// [`Self::is_open_in_app_available`]); mobile platform shells
    /// should not reach this method on those targets.
    fn command_for_open_in(
        app: crate::open_in::OpenInAppKind,
        path: &std::path::Path,
    ) -> std::process::Command;
}
