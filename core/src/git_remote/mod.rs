//! `GitRemoteProvider` capability — server-side ops against a git
//! remote (open/list/check PRs, auth probe).
//!
//! GitHub is one such provider; Bitbucket and GitLab are sibling
//! impls we don't ship today but the trait shape must not preclude.
//! "github" is **not** a top-level concept in the registry — it's a
//! concrete impl of "git remote provider".
//!
//! ## Relationship to `crate::git`
//!
//! `GitRemoteProvider` does **not** depend on the `Git` capability
//! via trait inheritance. Instead, callers resolve `Git` first, use
//! it to read the project's remote URL, then ask each
//! `GitRemoteProvider` candidate `matches_remote(&url)` to find the
//! one that owns this host. That keeps the trait's single concern
//! crisp (provider ops) while still letting providers be selected
//! based on `Git`-derived inputs.

use std::path::Path;
use std::sync::Arc;

use crate::capability::CapabilityImpl;
use crate::scope::Scope;

#[cfg(feature = "github-cli")]
mod github_cli;

#[cfg(feature = "github-cli")]
pub use github_cli::GhCliRemoteProvider;

mod types;

pub use types::{AuthStatus, CreatePrArgs, CreatePrOutcome, PrFilter, RemoteError, RemoteHost};

pub trait GitRemoteProvider: CapabilityImpl + Send + Sync {
    /// Which remote host this provider speaks to. Today: only
    /// `RemoteHost::GitHub`; the variant exists so future providers
    /// (Bitbucket, GitLab) drop in without shape changes.
    fn host(&self) -> RemoteHost;

    /// Whether this provider claims `remote_url`. Pattern-matches
    /// host strings (`github.com`, `gitlab.com`, …). Callers
    /// use this to pick the right provider when multiple are
    /// registered.
    fn matches_remote(&self, remote_url: &str) -> bool;

    fn probe_auth(&self, repo: &Path) -> AuthStatus;

    fn find_pull_request(
        &self,
        repo: &Path,
        head_branch: &str,
    ) -> Result<Option<crate::git_actions::PullRequestStatus>, RemoteError>;

    fn pull_request_checks(
        &self,
        repo: &Path,
        number: Option<u64>,
    ) -> Result<Option<Vec<crate::git_actions::PullRequestCheck>>, RemoteError>;

    fn create_pull_request(
        &self,
        repo: &Path,
        args: CreatePrArgs,
    ) -> Result<CreatePrOutcome, RemoteError>;

    fn list_pull_requests(
        &self,
        repo: &Path,
        filter: PrFilter,
        query: Option<&str>,
        limit: usize,
    ) -> Result<Vec<crate::git_actions::ProjectPagePullRequest>, RemoteError>;
}

/// Resolve the `GitRemoteProvider` whose `matches_remote` accepts
/// `remote_url`. Returns `None` for:
/// - No `GitRemoteProvider` registered (feature off).
/// - None of the registered providers claim this URL (e.g. a
///   self-hosted Gitea remote — UI should hide PR affordances).
pub fn resolve_for_remote(scope: &Scope, remote_url: &str) -> Option<Arc<dyn GitRemoteProvider>> {
    crate::capability::default_registry()
        .resolve::<dyn GitRemoteProvider>(scope)
        .into_iter()
        .find(|p| p.matches_remote(remote_url))
}

/// Resolve the first applicable `GitRemoteProvider` without checking
/// `matches_remote`. Used by boot-time probes (e.g. `gh auth status`)
/// that care about provider availability but don't yet have a project
/// remote URL.
pub fn resolve_any(scope: &Scope) -> Option<Arc<dyn GitRemoteProvider>> {
    crate::capability::default_registry()
        .resolve::<dyn GitRemoteProvider>(scope)
        .into_iter()
        .next()
}
