//! Shared types for the `GitRemoteProvider` trait.
//!
//! Carried over from PR #185's `core::github` module verbatim (only
//! the type-name spellings change — `GhError` → `RemoteError` — so
//! the trait stays neutral about which remote host an impl talks
//! to). The string in `RemoteError::Display` for `NotInstalled` is
//! preserved so existing toast wording in `app/src/app.rs` and
//! `core::git_actions::create_pull_request` doesn't drift.

/// Remote-host identifier. Today only `GitHub` ships; the variant
/// exists so a Bitbucket/GitLab impl drops in without churn.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteHost {
    GitHub,
    /// Reserved — no impl ships yet, but the variant pins the
    /// shape.
    Bitbucket,
    /// Reserved — no impl ships yet.
    GitLab,
    /// Catch-all for self-hosted / future hosts (Gitea, sourcehut,
    /// codeberg).
    Other,
}

/// Errors a provider can return. Distinguishes the cases callers
/// surface as different toasts: tool not installed (the dominant
/// case on a fresh machine), installed but not signed in, network
/// failure, anything else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RemoteError {
    /// The provider's backing tool isn't available (e.g. `gh` not
    /// on PATH for the GitHub-via-`gh` impl). UI should show
    /// "install …" or silently disable PR features depending on
    /// context. String in `Display` matches the legacy toast wording
    /// from `core::github::GhError::NotInstalled`.
    NotInstalled,
    /// The tool is available but reports no active account.
    NotAuthenticated,
    /// Reaching the provider's backend failed.
    NetworkError(String),
    /// Anything else: parse failure, unexpected exit code, etc.
    Other(String),
}

impl std::fmt::Display for RemoteError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RemoteError::NotInstalled => write!(
                f,
                "GitHub CLI (`gh`) is not installed or not on the app PATH."
            ),
            RemoteError::NotAuthenticated => {
                write!(f, "GitHub CLI is not signed in. Run: gh auth login")
            }
            RemoteError::NetworkError(msg) => write!(f, "GitHub network error: {msg}"),
            RemoteError::Other(msg) => write!(f, "{msg}"),
        }
    }
}

impl std::error::Error for RemoteError {}

/// Auth-probe result. Mirrors `daemon_proto::GhAuthStatusWire` so
/// the trait can stay independent of the wire crate; the daemon
/// translates between the two when projecting `gh_auth_status`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthStatus {
    Checking,
    /// Tool not on PATH (e.g. `gh` missing).
    ToolMissing,
    NotAuthenticated,
    Authenticated,
}

/// Args for `GitRemoteProvider::create_pull_request`. Mirrors the
/// shape `core::git_actions::create_pull_request_args` already
/// expects; kept as a struct so call sites can omit fields without
/// remembering positional argument order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePrArgs {
    pub head_branch: String,
    pub draft: bool,
    pub base_branch: Option<String>,
    pub title: String,
    pub body: String,
}

/// Filter for `GitRemoteProvider::list_pull_requests`. Maps to the
/// existing sidebar-PR-list filter index (0 = all open, 1 =
/// review-requested, 2 = author).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrFilter {
    AllOpen,
    ReviewRequested,
    Author,
}

/// Outcome of a successful create-PR call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatePrOutcome {
    pub number: Option<u64>,
    pub url: Option<String>,
}
