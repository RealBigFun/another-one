//! `gh` CLI-backed [`GitHubProvider`] implementation.
//!
//! Stub for commit 1 of the github-provider-trait branch — every
//! method returns `Err(GhError::Other)` until commit 2 ports the
//! real shell-out logic from `core::git_actions`.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use crate::github::{
    AuthStatus, CreatePrArgs, CreatePrOutcome, GhError, GitHubProvider, PrFilter,
};

/// Active provider for installs with the `gh` binary on PATH.
pub struct GhCliProvider {
    /// Resolved path to the `gh` binary at construction time.
    /// Cached so subsequent calls don't re-walk PATH on every
    /// shell-out (the discovery walk also probes the user's login
    /// shell, which can be slow).
    gh_path: PathBuf,
    /// CWD discovery anchor used when re-resolving (e.g. on
    /// `RecheckGhAuth`). Stored so the auth-status probe can run
    /// against the same shell-PATH the lookup used.
    #[allow(dead_code)] // wired into commit 2 helpers
    cwd: PathBuf,
}

impl GhCliProvider {
    /// Construct a provider against the gh binary discovered at
    /// `cwd`. Callers should check [`is_gh_available`] first; this
    /// constructor panics if the binary isn't found, since the
    /// happy-path factory always pre-checks.
    pub fn new(cwd: &Path) -> Self {
        let gh_path = crate::git_actions::find_gh_cli(cwd)
            .expect("GhCliProvider::new called without gh on PATH");
        Self {
            gh_path,
            cwd: cwd.to_path_buf(),
        }
    }
}

/// Cheap PATH lookup for the factory's branching. Caches the result
/// for the lifetime of the process; `RecheckGhAuth` invalidates by
/// re-running the factory, which constructs a fresh provider via
/// [`crate::git_actions::find_gh_cli`] (no cache there).
pub fn is_gh_available(cwd: &Path) -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| crate::git_actions::find_gh_cli(cwd).is_some())
}

impl GitHubProvider for GhCliProvider {
    fn probe_auth(&self) -> AuthStatus {
        // Wired in commit 2.
        let _ = &self.gh_path;
        AuthStatus::Checking
    }

    fn find_pull_request(
        &self,
        _repo: &Path,
        _head_branch: &str,
    ) -> Result<Option<crate::git_actions::PullRequestStatus>, GhError> {
        Err(GhError::Other(
            "GhCliProvider::find_pull_request not yet wired (commit 2)".into(),
        ))
    }

    fn pull_request_checks(
        &self,
        _repo: &Path,
        _number: Option<u64>,
    ) -> Result<Option<Vec<crate::git_actions::PullRequestCheck>>, GhError> {
        Err(GhError::Other(
            "GhCliProvider::pull_request_checks not yet wired (commit 2)".into(),
        ))
    }

    fn create_pull_request(
        &self,
        _repo: &Path,
        _args: CreatePrArgs,
    ) -> Result<CreatePrOutcome, GhError> {
        Err(GhError::Other(
            "GhCliProvider::create_pull_request not yet wired (commit 2)".into(),
        ))
    }

    fn list_pull_requests(
        &self,
        _repo: &Path,
        _filter: PrFilter,
        _limit: usize,
    ) -> Result<Vec<crate::git_actions::ProjectPagePullRequest>, GhError> {
        Err(GhError::Other(
            "GhCliProvider::list_pull_requests not yet wired (commit 2)".into(),
        ))
    }
}
