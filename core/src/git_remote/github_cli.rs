//! `gh` CLI–backed `GitRemoteProvider` for GitHub.
//!
//! Wraps the existing free helpers in `crate::git_actions` (same
//! ones the old `core::github::cli::GhCliProvider` wrapped before
//! this refactor). The only meaningful change vs. PR #185 is that
//! this impl is one of many possible `GitRemoteProvider`s — it
//! self-identifies via `matches_remote` rather than being chosen by
//! a `make_provider` factory.

use std::path::Path;

use super::types::{AuthStatus, CreatePrArgs, CreatePrOutcome, PrFilter, RemoteError, RemoteHost};
use super::GitRemoteProvider;
use crate::capability::CapabilityImpl;
use crate::scope::Scope;

pub struct GhCliRemoteProvider;

impl CapabilityImpl for GhCliRemoteProvider {
    fn applies(&self, scope: &Scope) -> bool {
        // Available whenever `gh` is on PATH. Whether *this* repo's
        // remote is github.com is a separate question handled by
        // `matches_remote`.
        scope.system().tool_probe.has("gh")
    }
}

impl GhCliRemoteProvider {
    /// Resolve the `gh` binary for `repo`, or return the
    /// `NotInstalled` error all create/list/probe paths use when
    /// PATH discovery fails.
    fn gh_path(&self, repo: &Path) -> Result<std::path::PathBuf, RemoteError> {
        crate::git_actions::find_gh_cli(repo).ok_or(RemoteError::NotInstalled)
    }
}

impl GitRemoteProvider for GhCliRemoteProvider {
    fn host(&self) -> RemoteHost {
        RemoteHost::GitHub
    }

    fn matches_remote(&self, remote_url: &str) -> bool {
        let s = remote_url.trim();
        // Cover the three shapes git stores remotes in:
        //   git@github.com:owner/repo.git
        //   https://github.com/owner/repo(.git)
        //   ssh://git@github.com/owner/repo.git
        s.contains("github.com")
    }

    fn probe_auth(&self, repo: &Path) -> AuthStatus {
        let Ok(gh) = self.gh_path(repo) else {
            return AuthStatus::ToolMissing;
        };
        match std::process::Command::new(&gh)
            .args(["auth", "status"])
            .output()
        {
            Ok(output) if output.status.success() => AuthStatus::Authenticated,
            Ok(_) => AuthStatus::NotAuthenticated,
            Err(_) => AuthStatus::ToolMissing,
        }
    }

    fn find_pull_request(
        &self,
        repo: &Path,
        head_branch: &str,
    ) -> Result<Option<crate::git_actions::PullRequestStatus>, RemoteError> {
        // Legacy free function collapses every failure to `None`,
        // matching pre-trait behaviour: "no PR / lookup failed" was
        // a single UI state. Preserve that here.
        Ok(crate::git_actions::find_latest_pull_request_status(
            repo,
            head_branch,
        ))
    }

    fn pull_request_checks(
        &self,
        repo: &Path,
        number: Option<u64>,
    ) -> Result<Option<Vec<crate::git_actions::PullRequestCheck>>, RemoteError> {
        crate::git_actions::find_pull_request_checks(repo, number).map_err(RemoteError::Other)
    }

    fn create_pull_request(
        &self,
        repo: &Path,
        args: CreatePrArgs,
    ) -> Result<CreatePrOutcome, RemoteError> {
        let gh = self.gh_path(repo)?;
        let mut cmd = crate::git_actions::external_command_for_provider(&gh, repo);
        cmd.args(crate::git_actions::create_pull_request_args(
            &args.head_branch,
            args.draft,
            args.base_branch.as_deref(),
            &args.title,
            &args.body,
        ));
        let output = cmd
            .output()
            .map_err(|err| RemoteError::Other(format!("gh pr create spawn failed: {err}")))?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let trimmed = stderr.trim();
            let body = if trimmed.is_empty() {
                format!("gh pr create exited {:?}", output.status.code())
            } else {
                trimmed.to_string()
            };
            return Err(RemoteError::Other(body));
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
    ) -> Result<Vec<crate::git_actions::ProjectPagePullRequest>, RemoteError> {
        let _ = limit;
        let filter_index = match filter {
            PrFilter::AllOpen => 0,
            PrFilter::ReviewRequested => 1,
            PrFilter::Author => 2,
        };
        crate::git_actions::find_project_pull_requests(repo, filter_index, query)
            .map_err(RemoteError::Other)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_remote_for_github_url_shapes() {
        let p = GhCliRemoteProvider;
        assert!(p.matches_remote("git@github.com:owner/repo.git"));
        assert!(p.matches_remote("https://github.com/owner/repo"));
        assert!(p.matches_remote("https://github.com/owner/repo.git"));
        assert!(p.matches_remote("ssh://git@github.com/owner/repo.git"));
    }

    #[test]
    fn matches_remote_rejects_non_github_urls() {
        let p = GhCliRemoteProvider;
        assert!(!p.matches_remote("git@bitbucket.org:owner/repo.git"));
        assert!(!p.matches_remote("https://gitlab.com/owner/repo.git"));
        assert!(!p.matches_remote("https://gitea.example.com/owner/repo.git"));
    }
}
