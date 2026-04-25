//! FRB-exposed build identity — feeds the Flutter titlebar's build
//! chip so a glance tells you what binary you're looking at. Same
//! shape as `desktop::build_info`: profile (debug vs release),
//! short SHA, branch, dirty flag, and a single-line tooltip.
//!
//! Values come from `env!()` against the `rustc-env` vars emitted
//! by `another-one-bridge/build.rs` — no runtime cost, no
//! filesystem access at call time.

const GIT_SHA: &str = env!("ANOTHER_ONE_BUILD_SHA");
const GIT_BRANCH: &str = env!("ANOTHER_ONE_BUILD_BRANCH");
const GIT_DIRTY: &str = env!("ANOTHER_ONE_BUILD_DIRTY");
const BUILD_TIME_UNIX: &str = env!("ANOTHER_ONE_BUILD_TIME_UNIX");

/// Single point-in-time view of the bridge binary's build identity.
/// Stable across a process's lifetime, so the Dart side reads it
/// once at startup rather than polling.
pub struct BuildInfo {
    /// `dev` for debug builds, empty for release. The titlebar chip
    /// uses this to decide colour: amber on debug, subtle on release.
    pub is_dev: bool,
    /// True if the working tree had uncommitted changes when
    /// `build.rs` ran. The chip shows red when this is true on a
    /// debug build — the binary contains code that can't be
    /// reproduced from a SHA alone.
    pub is_dirty: bool,
    /// Short git SHA, e.g. `225a501`, or `unknown` if `build.rs`
    /// couldn't shell out to git.
    pub git_sha: String,
    /// Branch checked out at build time. Used in the tooltip; the
    /// chip itself shows only the SHA.
    pub git_branch: String,
    /// One-line chip label, e.g. `dev · 225a501 · dirty` or just
    /// `225a501`. Pre-formatted on the Rust side so the Dart code
    /// stays a thin renderer.
    pub chip_label: String,
    /// Single-line tooltip with profile, branch, sha, dirty, and a
    /// human-readable build time, e.g.
    /// `debug · main · 225a501 · built 2026-04-25 14:32 UTC`.
    pub tooltip: String,
}

#[inline]
fn is_dirty() -> bool {
    GIT_DIRTY == "true"
}

#[inline]
const fn is_dev_build() -> bool {
    cfg!(debug_assertions)
}

fn chip_label() -> String {
    let mut out = String::new();
    if is_dev_build() {
        out.push_str("dev · ");
    }
    out.push_str(GIT_SHA);
    if is_dirty() {
        out.push_str(" · dirty");
    }
    out
}

fn tooltip_text() -> String {
    let profile = if is_dev_build() { "debug" } else { "release" };
    let dirty = if is_dirty() { " · dirty" } else { "" };
    format!(
        "{profile} · {GIT_BRANCH} · {GIT_SHA}{dirty} · built {ts}",
        ts = format_build_time(),
    )
}

/// Format `BUILD_TIME_UNIX` as `YYYY-MM-DD HH:MM UTC`. Hand-rolled
/// rather than pulling in a date crate just for one cosmetic line.
fn format_build_time() -> String {
    let secs: i64 = BUILD_TIME_UNIX.parse().unwrap_or(0);
    if secs == 0 {
        return "unknown".into();
    }
    let (y, mo, d, h, mi) = unix_to_ymdhm(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02} UTC")
}

/// Minimal civil-date conversion based on Howard Hinnant's
/// `days_from_civil` algorithm
/// (<https://howardhinnant.github.io/date_algorithms.html>),
/// trimmed to date+hour+minute. Mirrors the same helper in
/// `desktop::build_info`.
fn unix_to_ymdhm(secs: i64) -> (i32, u32, u32, u32, u32) {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    let h = (rem / 3600) as u32;
    let mi = ((rem % 3600) / 60) as u32;
    let z = days + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as i64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let mo = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = (y + if mo <= 2 { 1 } else { 0 }) as i32;
    (y, mo, d, h, mi)
}

/// One-shot read of the bridge's build identity. Cheap — just
/// reads compiled-in `&'static str` constants and formats two
/// strings. Safe to call from app startup.
pub fn read_build_info() -> BuildInfo {
    BuildInfo {
        is_dev: is_dev_build(),
        is_dirty: is_dirty(),
        git_sha: GIT_SHA.to_string(),
        git_branch: GIT_BRANCH.to_string(),
        chip_label: chip_label(),
        tooltip: tooltip_text(),
    }
}
