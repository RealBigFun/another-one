//! GitHub provider abstraction.
//!
//! Every site that talks to GitHub goes through [`GitHubProvider`]
//! instead of forking `gh` directly. Two implementations ship today:
//!
//! - [`cli::GhCliProvider`] (default) shells out to the `gh` CLI,
//!   exactly the way pre-trait code did. Users with `gh` installed
//!   and authenticated keep the same behaviour.
//! - [`missing::MissingProvider`] is the no-op fallback when `gh`
//!   isn't on PATH. Every method returns
//!   [`GhError::NotInstalled`]; callers degrade gracefully (PR
//!   features show "GitHub CLI not installed" toasts instead of
//!   crashing or silently no-oping).
//!
//! [`make_provider`] picks one at construction time. Callers that
//! want to react to mid-session installs (e.g. the daemon's
//! `RecheckGhAuth` handler) re-run the factory.
//!
//! Wire-format / protocol types in `daemon_proto` are unaffected:
//! mobile peers continue to receive `GhAuthStatusWire` /
//! `WorkerReply::PullRequest*` from the desktop's daemon, which
//! holds the `Arc<dyn GitHubProvider>`.
//!
//! ## Why a trait, not a feature flag alone
//!
//! Most of the value here is the structural seam, not the binary
//! shrink. Once every gh shell-out routes through one method on a
//! trait, swapping in a direct REST client (octocrab, reqwest) or a
//! deterministic mock for tests becomes a localized change. A future
//! `github-cli` feature flag (Cargo) sits on top of this trait to
//! drop the CLI provider from builds where only the missing-fallback
//! is ever wanted.
//!
//! ## What this trait is *not* about
//!
//! Not a general "GitHub API client". The surface mirrors what the
//! app actually uses today: PR discovery, PR checks, PR creation,
//! PR list/search, plus the auth-status probe that drives the
//! overlay. New methods land here when a real call site needs them,
//! not speculatively.

use std::path::Path;
use std::sync::Arc;

#[cfg(feature = "github-cli")]
pub mod cli;
pub mod missing;

/// Errors a provider can return. Distinguishes the cases callers
/// surface as different toasts: gh not installed (the dominant case
/// on a fresh machine), gh installed but not signed in, network
/// failure, anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GhError {
    /// No `gh` binary on PATH (or whichever discovery mechanism the
    /// provider uses). UI should show "install GitHub CLI" or
    /// silently disable PR features depending on context.
    NotInstalled,
    /// `gh` is on PATH but `gh auth status` reports no active
    /// account. UI should show "run gh auth login".
    NotAuthenticated,
    /// Reaching api.github.com (or whatever the provider's backend
    /// is) failed. The contained string is the underlying error
    /// for surfacing in toasts / logs.
    NetworkError(String),
    /// Catch-all for anything else: parse failure, unexpected exit
    /// code, gh CLI bug, etc.
    Other(String),
}

impl std::fmt::Display for GhError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            GhError::NotInstalled => write!(
                f,
                "GitHub CLI (`gh`) is not installed or not on the app PATH."
            ),
            GhError::NotAuthenticated => {
                write!(f, "GitHub CLI is not signed in. Run: gh auth login")
            }
            GhError::NetworkError(msg) => write!(f, "GitHub network error: {msg}"),
            GhError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for GhError {}

/// Auth-probe result. Mirrors `daemon_proto::GhAuthStatusWire` so
/// the trait can stay independent of the wire crate, but the daemon
/// translates between the two when projecting `gh_auth_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    /// Not yet probed (factory hasn't run, or RecheckGhAuth in flight).
    Checking,
    /// `gh` not on PATH.
    GhMissing,
    /// `gh` found but `gh auth status` exited non-zero.
    NotAuthenticated,
    /// `gh auth status` exited 0.
    Authenticated,
}

/// Args for [`GitHubProvider::create_pull_request`]. Mirrors what
/// `core::git_actions::create_pull_request_args` already expects;
/// kept as a struct so call sites can omit fields without remembering
/// positional argument order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePrArgs {
    pub head_branch: String,
    pub draft: bool,
    pub base_branch: Option<String>,
    pub title: String,
    pub body: String,
}

/// Filter for [`GitHubProvider::list_pull_requests`]. Maps to the
/// existing sidebar-PR-list filter index (0 = all open, 1 =
/// review-requested, 2 = author).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrFilter {
    /// All open PRs in the repo.
    AllOpen,
    /// PRs where the authenticated user is requested as reviewer.
    ReviewRequested,
    /// PRs authored by the authenticated user.
    Author,
}

/// Outcome of a successful create-PR call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePrOutcome {
    /// PR number assigned by GitHub.
    pub number: u64,
    /// Web URL of the created PR.
    pub url: String,
}

/// Trait every gh-touching call site goes through. Implementations
/// are `Send + Sync` so the daemon's registry can hold an
/// `Arc<dyn GitHubProvider>` and clone it across threads.
///
/// All methods are synchronous; gh shell-outs already block worker
/// threads in today's code, and a future async backend wraps the
/// trait in an async-compat shim rather than turning every method
/// into `async fn`. Async-trait-in-stable-Rust isn't worth the
/// complexity tax here.
pub trait GitHubProvider: Send + Sync {
    /// Probe auth state. Fast (re-reads cached token / runs a tiny
    /// CLI invocation depending on impl). Drives the boot-time
    /// overlay and `Control::RecheckGhAuth`.
    fn probe_auth(&self) -> AuthStatus;

    /// Find the PR for `head_branch` in `repo`. `Ok(None)` means
    /// "gh succeeded but reports no matching PR"; `Err` means the
    /// lookup itself failed.
    fn find_pull_request(
        &self,
        repo: &Path,
        head_branch: &str,
    ) -> Result<Option<crate::git_actions::PullRequestStatus>, GhError>;

    /// PR checks for the PR identified by `number`, or for the PR
    /// associated with the current branch when `number` is `None`
    /// (matches the legacy behaviour). `Ok(None)` is "no PR found";
    /// `Ok(Some(vec![]))` is "PR found, zero checks reported".
    fn pull_request_checks(
        &self,
        repo: &Path,
        number: Option<u64>,
    ) -> Result<Option<Vec<crate::git_actions::PullRequestCheck>>, GhError>;

    /// Create a PR. Returns the assigned number + URL on success.
    fn create_pull_request(
        &self,
        repo: &Path,
        args: CreatePrArgs,
    ) -> Result<CreatePrOutcome, GhError>;

    /// List PRs matching `filter` in `repo`, capped at `limit`.
    fn list_pull_requests(
        &self,
        repo: &Path,
        filter: PrFilter,
        limit: usize,
    ) -> Result<Vec<crate::git_actions::ProjectPagePullRequest>, GhError>;
}

/// Construct the active provider. Picks `GhCliProvider` when `gh`
/// is on PATH at construction time *and* the `github-cli` feature
/// is enabled, otherwise [`missing::MissingProvider`]. Re-run on
/// `Control::RecheckGhAuth` so an install during the session takes
/// effect without an app restart.
///
/// With the `github-cli` feature off this always returns a
/// `MissingProvider`, and the `cli` module isn't compiled — the
/// binary has no path to `find_gh_cli` / `Command::new("gh")`.
pub fn make_provider(_cwd: &Path) -> Arc<dyn GitHubProvider> {
    #[cfg(feature = "github-cli")]
    {
        if cli::is_gh_available(_cwd) {
            return Arc::new(cli::GhCliProvider::new(_cwd));
        }
    }
    Arc::new(missing::MissingProvider)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gh_error_display_formats_match_user_facing_strings() {
        // The PR-creation toast wording is pinned by app/src/app.rs
        // and core::git_actions; if these strings drift the user-
        // facing experience drifts with them. Lock them here.
        assert_eq!(
            GhError::NotInstalled.to_string(),
            "GitHub CLI (`gh`) is not installed or not on the app PATH."
        );
        assert_eq!(
            GhError::NotAuthenticated.to_string(),
            "GitHub CLI is not signed in. Run: gh auth login"
        );
        assert!(GhError::NetworkError("timeout".into())
            .to_string()
            .contains("timeout"));
    }
}
