//! Capture build-time identity for the desktop binary.
//!
//! The titlebar's build chip and any future updater both need to
//! answer "what is this exact binary?" — so emit short SHA, dirty
//! flag, branch, and an ISO-8601-ish build timestamp as `rustc-env`
//! variables. They get baked into the binary as `&'static str` via
//! `env!()` and are accessible from any module without a runtime
//! cost.
//!
//! Re-run triggers cover the cases where the answer would actually
//! change: a new commit (HEAD moves), a different branch checked
//! out (HEAD's contents change), or *staged* changes (index moves).
//! Two known caveats, both bounded:
//!
//! * Unstaged changes don't move `.git/index`, so the `·dirty` flag
//!   may be stale until the next cargo invalidation. The SHA is
//!   still correct in that window. `cargo clean -p another-one`
//!   forces a refresh.
//! * In git worktrees the `.git` entry is a file, not a directory,
//!   and `.git/HEAD` doesn't exist as a literal filesystem path.
//!   Cargo treats absent `rerun-if-changed` targets conservatively
//!   (reruns every build), so worktree builds re-run this script
//!   on every cargo invocation — over-triggering but never
//!   under-triggering.

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

    // Re-run when commit, branch, or working-tree changes. Without
    // these, cargo would cache build.rs output and the SHA would
    // drift away from reality after the first build.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    // Some checkouts (worktrees, submodules) keep HEAD as a file
    // pointing into a ref; watch the ref too if we can resolve it.
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
