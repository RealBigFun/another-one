//! Fallback [`GitHubProvider`] for installs without `gh`.
//!
//! Every method returns [`GhError::NotInstalled`]. Call sites that
//! cared about gh-backed features (PR list, PR checks, Create PR)
//! surface a "GitHub CLI not installed" toast and disable
//! themselves; everything else continues to work.

use std::path::Path;

use crate::github::{
    AuthStatus, CreatePrArgs, CreatePrOutcome, GhError, GitHubProvider, PrFilter,
};

pub struct MissingProvider;

impl GitHubProvider for MissingProvider {
    fn probe_auth(&self) -> AuthStatus {
        AuthStatus::GhMissing
    }

    fn find_pull_request(
        &self,
        _repo: &Path,
        _head_branch: &str,
    ) -> Result<Option<crate::git_actions::PullRequestStatus>, GhError> {
        Err(GhError::NotInstalled)
    }

    fn pull_request_checks(
        &self,
        _repo: &Path,
        _number: Option<u64>,
    ) -> Result<Option<Vec<crate::git_actions::PullRequestCheck>>, GhError> {
        Err(GhError::NotInstalled)
    }

    fn create_pull_request(
        &self,
        _repo: &Path,
        _args: CreatePrArgs,
    ) -> Result<CreatePrOutcome, GhError> {
        Err(GhError::NotInstalled)
    }

    fn list_pull_requests(
        &self,
        _repo: &Path,
        _filter: PrFilter,
        _query: Option<&str>,
        _limit: usize,
    ) -> Result<Vec<crate::git_actions::ProjectPagePullRequest>, GhError> {
        Err(GhError::NotInstalled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_method_returns_not_installed() {
        let p = MissingProvider;
        assert_eq!(p.probe_auth(), AuthStatus::GhMissing);
        assert_eq!(
            p.find_pull_request(Path::new("/tmp"), "main")
                .unwrap_err(),
            GhError::NotInstalled
        );
        assert_eq!(
            p.pull_request_checks(Path::new("/tmp"), Some(1))
                .unwrap_err(),
            GhError::NotInstalled
        );
        assert_eq!(
            p.create_pull_request(
                Path::new("/tmp"),
                CreatePrArgs {
                    head_branch: "feature/x".into(),
                    draft: false,
                    base_branch: None,
                    title: "title".into(),
                    body: String::new(),
                },
            )
            .unwrap_err(),
            GhError::NotInstalled
        );
        assert_eq!(
            p.list_pull_requests(Path::new("/tmp"), PrFilter::AllOpen, None, 10)
                .unwrap_err(),
            GhError::NotInstalled
        );
    }
}
