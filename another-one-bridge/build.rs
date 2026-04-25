//! Capture build-time identity for the bridge crate, which is what
//! the Flutter desktop links against. Mirrors `desktop/build.rs` so
//! the chip in the Flutter titlebar reports identical values to the
//! GPUI titlebar's chip — same SHA, branch, dirty flag, build time.
//!
//! The values are emitted as `rustc-env` variables and baked into
//! the bridge's binary via `env!()` in `api::build_info`. Re-run
//! triggers below cover the cases where the answer would actually
//! change; see desktop/build.rs for the caveats.

use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    let sha = git(&["rev-parse", "--short=7", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_else(|| "unknown".into());
    let dirty = match git(&["status", "--porcelain"]) {
        Some(s) => !s.is_empty(),
        None => false,
    };
    let build_time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    println!("cargo:rustc-env=ANOTHER_ONE_BUILD_SHA={sha}");
    println!("cargo:rustc-env=ANOTHER_ONE_BUILD_BRANCH={branch}");
    println!("cargo:rustc-env=ANOTHER_ONE_BUILD_DIRTY={dirty}");
    println!("cargo:rustc-env=ANOTHER_ONE_BUILD_TIME_UNIX={build_time}");

    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    if let Some(ref_path) = git(&["symbolic-ref", "-q", "HEAD"]) {
        println!("cargo:rerun-if-changed=.git/{ref_path}");
    }
}

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    Some(s.trim().to_string())
}
