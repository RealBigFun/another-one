//! Build-time identity surfaced by `desktop/build.rs`.
//!
//! Read by the titlebar build chip (so a glance tells you what's
//! installed) and ready to be read by the future in-app updater
//! ("which build is this and is it older than what's published?").
//!
//! All values are baked in at compile time — no runtime cost, no
//! filesystem access. `is_dev_build()` is a `cfg!` query, so the
//! compiler folds the branch in release builds.

/// Short git SHA at build time, e.g. `225a501`. `"unknown"` if
/// `build.rs` couldn't shell out to git (e.g. building from a
/// tarball with no `.git` dir).
pub const GIT_SHA: &str = env!("ANOTHER_ONE_BUILD_SHA");

/// Branch checked out at build time, e.g. `main` or
/// `feat-build-marker-and-release-action`. `"unknown"` if not in a
/// git checkout.
pub const GIT_BRANCH: &str = env!("ANOTHER_ONE_BUILD_BRANCH");

/// `"true"` if the working tree had uncommitted changes when
/// `build.rs` ran. String, not bool, because `env!` produces
/// `&'static str`; compare with `== "true"`.
pub const GIT_DIRTY: &str = env!("ANOTHER_ONE_BUILD_DIRTY");

/// Unix timestamp (seconds) when `build.rs` ran. Stringified for the
/// same reason as `GIT_DIRTY`.
pub const BUILD_TIME_UNIX: &str = env!("ANOTHER_ONE_BUILD_TIME_UNIX");

/// True if the working tree had uncommitted changes at build time.
#[inline]
pub fn is_dirty() -> bool {
    GIT_DIRTY == "true"
}

/// True for `cargo build` / `cargo run` (debug profile). False for
/// `--release`. The titlebar chip is most prominent in dev builds —
/// the goal is to make it impossible to confuse a debug binary for
/// a release one.
#[inline]
pub const fn is_dev_build() -> bool {
    cfg!(debug_assertions)
}

/// One-line summary suited for a chip label. Examples:
/// `dev · 225a501`, `dev · 225a501·dirty`, `225a501`.
pub fn chip_label() -> String {
    let mut out = String::new();
    if is_dev_build() {
        out.push_str("dev · ");
    }
    out.push_str(GIT_SHA);
    if is_dirty() {
        out.push_str("·dirty");
    }
    out
}

/// Single-line tooltip string with profile, branch, full short SHA,
/// dirty flag, and build time. Kept on one line because the
/// titlebar's existing `ActionTooltip` view renders a single text
/// child without whitespace preservation; a multi-line tooltip
/// would need its own view.
///
/// Returns `&'static str` (leaked once via `LazyLock`) so it slots
/// into the tooltip API without per-render allocation. The values
/// are immutable for the binary's lifetime, so there's nothing to
/// free.
pub fn tooltip_text() -> &'static str {
    use std::sync::LazyLock;
    static TEXT: LazyLock<String> = LazyLock::new(|| {
        let profile = if is_dev_build() { "debug" } else { "release" };
        let dirty = if is_dirty() { " · dirty" } else { "" };
        format!(
            "{profile} · {GIT_BRANCH} · {GIT_SHA}{dirty} · built {ts}",
            ts = format_build_time(),
        )
    });
    TEXT.as_str()
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

/// Minimal civil-date conversion (Howard Hinnant's algorithm,
/// trimmed to date+hour+minute). No crate dep, ~no perf cost since
/// this runs once on tooltip hover.
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
