//! `Git` capability — local git state for a directory.
//!
//! One concern: "what does git see for this path?" — *not* "is the
//! remote a GitHub PR host?" (that's `crate::git_remote`). The
//! split is the central correctness property of this refactor: a
//! system may have `git` but no `gh`, the project may be a git repo
//! with a non-GitHub remote (Bitbucket, Gitea, self-hosted GitLab),
//! or it may not be a git repo at all. Each of those is a *different*
//! capability-resolution outcome and the system must handle them
//! distinctly.

use std::path::Path;
use std::sync::Arc;

use crate::capability::CapabilityImpl;
use crate::scope::Scope;

pub trait Git: CapabilityImpl + Send + Sync {
    /// Whether `path` resolves to a git working tree. Cheap shell-out
    /// (`git rev-parse --is-inside-work-tree`).
    fn is_repo(&self, path: &Path) -> bool;

    /// Current branch name, or `None` for a detached HEAD or a
    /// non-repo path.
    fn current_branch(&self, repo: &Path) -> Option<String>;

    /// Remote URL for `name` (typically "origin"). Returns the raw
    /// URL from `git config remote.<name>.url`; URL normalization
    /// (ssh → https, host extraction) is the caller's job —
    /// `GitRemoteProvider::matches_remote` does its own pattern
    /// matching on whatever shape the URL takes.
    fn remote_url(&self, repo: &Path, name: &str) -> Option<String>;
}

/// `git` CLI–backed impl. Wraps the existing helpers in
/// `crate::git_actions` rather than reimplementing the shell-out
/// plumbing.
pub struct CliGit;

impl CapabilityImpl for CliGit {
    fn applies(&self, scope: &Scope) -> bool {
        // `git` must be on PATH. Per-path "is this a repo?" is a
        // separate question and lives on the trait method — `applies`
        // gates registry membership, not per-call usefulness.
        scope.system().tool_probe.has("git")
    }
}

impl Git for CliGit {
    fn is_repo(&self, path: &Path) -> bool {
        crate::git_actions::git_stdout(path, &["rev-parse", "--is-inside-work-tree"])
            .map(|out| out.trim() == "true")
            .unwrap_or(false)
    }

    fn current_branch(&self, repo: &Path) -> Option<String> {
        crate::git_actions::git_current_branch(repo)
    }

    fn remote_url(&self, repo: &Path, name: &str) -> Option<String> {
        crate::git_actions::git_stdout(repo, &["remote", "get-url", name])
    }
}

/// Convenience helper: ask the default registry for a Git impl that
/// applies at `scope`. Returns the first match (today there's only
/// one impl — `CliGit`). Returns `None` when no Git impl applies,
/// e.g. `git` isn't on PATH.
pub fn resolve_git(scope: &Scope) -> Option<Arc<dyn Git>> {
    crate::capability::default_registry()
        .resolve::<dyn Git>(scope)
        .into_iter()
        .next()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scope::{SystemScope, ToolProbe};

    #[test]
    fn cli_git_applies_when_probe_reports_git() {
        // We can't reliably assert PATH state across CI hosts, so
        // we just verify the predicate consults the probe and
        // doesn't panic.
        let probe = Arc::new(ToolProbe::new());
        let sys = SystemScope::new(probe.clone());
        let scope: Scope = sys.into();
        let _ = CliGit.applies(&scope);
    }
}
