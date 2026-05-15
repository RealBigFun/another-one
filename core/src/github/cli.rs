//! `gh` CLI-backed [`GitHubProvider`] implementation.
//!
//! Wraps the existing free functions in [`crate::git_actions`] so
//! the call-site migration in commit 4 of refactor/github-provider-trait
//! is a small, mechanical swap. Once every caller routes through
//! the provider, the wrapped free functions can either become
//! private helpers or be folded into the methods directly; that's
//! follow-on cleanup.
//!
//! Errors from the wrapped functions translate into [`GhError`] as
//! best the source allows: today most surface as `String`, which
//! we treat as `GhError::Other`. A future provider revision can
//! sniff the underlying gh exit code / stderr to distinguish
//! `NotAuthenticated` from `NetworkError`.

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
}

impl GhCliProvider {
    /// Construct a provider against the gh binary discovered at
    /// `cwd`. Callers should check [`is_gh_available`] first; this
    /// constructor panics if the binary isn't found, since the
    /// happy-path factory always pre-checks.
    pub fn new(cwd: &Path) -> Self {
        let gh_path = crate::git_actions::find_gh_cli(cwd)
            .expect("GhCliProvider::new called without gh on PATH");
        Self { gh_path }
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
        // Direct shell-out: doesn't need a wrapped free function.
        // Mirrors what `app::daemon_host::perform_gh_auth_check`
        // does today; commit 5 retires the duplicate.
        match std::process::Command::new(&self.gh_path)
            .args(["auth", "status"])
            .output()
        {
            Ok(output) if output.status.success() => AuthStatus::Authenticated,
            Ok(_) => AuthStatus::NotAuthenticated,
            // Spawn failure is unexpected here — we already verified
            // gh exists at construction time. Surface as GhMissing
            // so the renderer's overlay logic stays simple (any
            // “can't reach gh” state → same UX).
            Err(_) => AuthStatus::GhMissing,
        }
    }

    fn find_pull_request(
        &self,
        repo: &Path,
        head_branch: &str,
    ) -> Result<Option<crate::git_actions::PullRequestStatus>, GhError> {
        // Existing free function returns `Option<PullRequestStatus>`,
        // collapsing every failure mode (gh missing, gh exited
        // non-zero, parse failure) to `None`. We can't recover the
        // distinction without rewriting it, so commit 2 preserves
        // the existing semantics: `Ok(None)` covers both “no PR”
        // and “gh failed”. The migration in commit 4 keeps the
        // existing user-facing behaviour (no toast on this path),
        // and a follow-on can split the cases when needed.
        Ok(crate::git_actions::find_latest_pull_request_status(
            repo,
            head_branch,
        ))
    }

    fn pull_request_checks(
        &self,
        repo: &Path,
        number: Option<u64>,
    ) -> Result<Option<Vec<crate::git_actions::PullRequestCheck>>, GhError> {
        crate::git_actions::find_pull_request_checks(repo, number).map_err(GhError::Other)
    }

    fn create_pull_request(
        &self,
        repo: &Path,
        args: CreatePrArgs,
    ) -> Result<CreatePrOutcome, GhError> {
        let mut cmd = crate::git_actions::external_command_for_provider(&self.gh_path, repo);
        cmd.args(crate::git_actions::create_pull_request_args(
            &args.head_branch,
            args.draft,
            args.base_branch.as_deref(),
            &args.title,
            &args.body,
        ));
        let output = cmd
            .output()
            .map_err(|err| GhError::Other(format!("gh pr create spawn failed: {err}")))?;
        if !output.status.success() {
            // Approximate legacy toast wording: stderr trimmed, or
            // a fallback when stderr is empty. The legacy
            // `command_failure` helper prefixes "PR creation
            // failed:" / "Draft PR creation failed:" — the call
            // site (still in `git_actions::create_pull_request`)
            // wraps the GhError message with that prefix when
            // building the `ToolbarActionError`, so we don't
            // duplicate it here.
            let stderr = String::from_utf8_lossy(&output.stderr);
            let trimmed = stderr.trim();
            let body = if trimmed.is_empty() {
                format!("gh pr create exited {:?}", output.status.code())
            } else {
                trimmed.to_string()
            };
            return Err(GhError::Other(body));
        }
        let stdout = String::from_utf8_lossy(&output.stdout);
        let url = crate::git_actions::extract_url_for_provider(&stdout);
        Ok(CreatePrOutcome { number: None, url })
    }

    fn list_pull_requests(
        &self,
        repo: &Path,
        filter: PrFilter,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::git_actions::ProjectPagePullRequest>, GhError> {
        // Legacy signature is `(repo, filter_index: usize, query:
        // Option<&str>)` and ignores any explicit limit (the gh
        // command caps internally at 100). For commit 2 we
        // preserve that: pass the equivalent filter_index, ignore
        // the limit. Commit 4 either threads `limit` through or
        // documents that gh's internal cap is the floor.
        let _ = limit;
        let filter_index = match filter {
            PrFilter::AllOpen => 0,
            PrFilter::ReviewRequested => 1,
            PrFilter::Author => 2,
        };
        crate::git_actions::find_project_pull_requests(repo, filter_index, query)
            .map_err(GhError::Other)
    }
}
