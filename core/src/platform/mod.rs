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
//! actual methods out of `desktop/src/platform/` and add
//! [`HeadlessPlatform::terminal_engine`] for the alacritty/xterm
//! per-platform render-engine choice.

// Each `mod` declaration is cfg-gated to its own target so unused
// platforms don't trigger dead-code warnings during normal builds.
// All five files live on disk regardless — `cargo check
// --target=…` against any of them is the way to verify a foreign
// platform still compiles after a refactor.

#[cfg(target_os = "android")]
mod android;
#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "linux")]
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
}
