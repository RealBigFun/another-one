//! Background git workers, extracted from `desktop/src/app.rs`.
//!
//! Each function here spawns an OS thread that does a blocking git
//! read or `gh` CLI call, then sends a reply on a
//! [`tokio::sync::broadcast::Sender`]. The desktop `AnotherOneApp`
//! drains the matching `Receiver` on its render-timer tick and folds
//! the reply back into UI state; a future daemon or mobile client can
//! subscribe to the same `Sender` to receive the same stream.
//!
//! No async runtime is needed. `broadcast::Sender::send` and
//! `broadcast::Receiver::try_recv` are both synchronous, so the GPUI
//! render loop can drive the drain without a tokio `Runtime`. Reply
//! types are `Clone` so `broadcast` can fan out a single send to N
//! subscribers.
//!
//! Reply types (`GitRefreshReply`, …) live here so desktop and any
//! future daemon/mobile client share the same vocabulary.

use std::path::{Path, PathBuf};
use std::thread;

use tokio::sync::broadcast;

use crate::git_actions::{
    execute_toolbar_git_action, find_github_repo_url, find_project_open_issues,
    probe_github_issue_availability, GitActionSettings, GitHubIssueAvailability, GitHubIssueRecord,
    ProjectPagePullRequest, PullRequestCheck, PullRequestStatus, ToolbarActionError,
    ToolbarActionOutcome, ToolbarGitAction,
};
use crate::project_store::{
    fetch_project_git_state, read_changed_file_diff, read_project_branch_commit_state,
    read_project_git_state, revert_changed_file, stage_all_changes, stage_changed_file,
    unstage_all_changes, unstage_changed_file, ChangedFile, GitDiff, GitDiffSelection,
    ProjectBranchCommitState, ProjectGitState,
};

/// Result payload from `spawn_refresh` — one message per refresh call.
#[derive(Clone)]
pub struct GitRefreshReply {
    pub project_id: String,
    pub include_metadata: bool,
    pub state: ProjectGitState,
    pub commit_state: Option<Result<ProjectBranchCommitState, String>>,
}

#[derive(Clone)]
pub struct RemoteBranchRefreshReply {
    pub project_id: String,
    pub result: Result<ProjectGitState, String>,
}

/// Spawn a background git-status / metadata / commit read for
/// one project and return a receiver that will yield exactly one
/// [`GitRefreshReply`] when it completes.
///
/// Arguments:
/// - `project_id` — echoed back in the reply so the caller knows which
///   project this refresh is for (the drain loop may race multiple
///   refreshes for different projects).
/// - `project_path` — on-disk path git commands will run against.
/// - `include_metadata` — if `true`, the worker also reads branch
///   order / ahead-behind / project kind metadata, not just the
///   working-tree state.
/// - `commit_limit` — if `Some(n)`, request the last `n` commits for
///   the current branch; drives the commit-list sidebar.
pub fn spawn_refresh(
    project_id: String,
    project_path: PathBuf,
    include_metadata: bool,
    commit_limit: Option<usize>,
) -> broadcast::Receiver<GitRefreshReply> {
    let (tx, rx) = broadcast::channel(1);
    thread::spawn(move || {
        let state = read_project_git_state(&project_path, include_metadata);
        let commit_state = commit_limit.map(|requested_limit| {
            read_project_branch_commit_state(&project_path, requested_limit)
        });
        let _ = tx.send(GitRefreshReply {
            project_id,
            include_metadata,
            state,
            commit_state,
        });
    });
    rx
}

/// Fetch remote refs and return fresh branch metadata for a project.
///
/// This is intentionally separate from the automatic metadata refresh:
/// the periodic path must stay local-only, while branch-picking UI can
/// opt into a network fetch when the user is actively looking for a
/// branch that may only exist on a remote.
pub fn spawn_remote_branch_refresh(
    project_id: String,
    project_path: PathBuf,
) -> broadcast::Receiver<RemoteBranchRefreshReply> {
    let (tx, rx) = broadcast::channel(1);
    thread::spawn(move || {
        let result = fetch_project_git_state(&project_path);
        let _ = tx.send(RemoteBranchRefreshReply { project_id, result });
    });
    rx
}

/// Result payload from `spawn_toolbar_action`. Carries the project id
/// plus the raw outcome/error so the UI layer can decide how to
/// surface it (toast kind, refresh scheduling, modal dismissal, etc.)
/// without needing to know anything about `ToastKind` on the core
/// side.
#[derive(Clone)]
pub struct GitActionReply {
    pub project_id: String,
    pub result: Result<ToolbarActionOutcome, ToolbarActionError>,
}

/// Spawn a background toolbar git action (commit, push, fetch, pull,
/// create PR, undo) and return a receiver that will yield exactly one
/// [`GitActionReply`] when the operation completes.
pub fn spawn_toolbar_action(
    project_id: String,
    project_path: PathBuf,
    action: ToolbarGitAction,
) -> broadcast::Receiver<GitActionReply> {
    let (tx, rx) = broadcast::channel(1);
    thread::spawn(move || {
        let mut on_progress = |_message: String| {};
        let result = execute_toolbar_git_action(
            &project_path,
            action,
            GitActionSettings::default(),
            &mut on_progress,
        );
        let _ = tx.send(GitActionReply { project_id, result });
    });
    rx
}

// ---- staged-file mutations (right-sidebar) --------------------------
//
// Unlike the two spawn fns above, the mutations path is queue-shaped:
// the desktop app maintains one persistent `(tx, rx)` pair and drains
// many replies over the UI's lifetime. Core exposes a `Sender`-taking
// worker to match.

/// One stage / unstage operation on the right-sidebar changed-files
/// view. Moves verbatim from the desktop crate because the worker
/// needs to dispatch on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ChangedFilesGitMutation {
    StageFile { changed: ChangedFile },
    UnstageFile { changed: ChangedFile },
    StageAll,
    UnstageAll,
    RevertFiles { changed_files: Vec<ChangedFile> },
}

impl ChangedFilesGitMutation {
    pub fn stages_file(&self, path: &str) -> bool {
        matches!(self, Self::StageFile { changed } if changed.path == path)
    }

    pub fn unstages_file(&self, path: &str) -> bool {
        matches!(self, Self::UnstageFile { changed } if changed.path == path)
    }

    pub fn stages_all(&self) -> bool {
        matches!(self, Self::StageAll)
    }

    pub fn unstages_all(&self) -> bool {
        matches!(self, Self::UnstageAll)
    }
}

/// Reply carrying the post-mutation git state (or an error string) so
/// the drain loop can reconcile optimistic UI with real disk state.
#[derive(Clone)]
pub struct ChangedFilesGitMutationReply {
    pub project_id: String,
    pub result: Result<ProjectGitState, String>,
}

/// Run one staged-file mutation on a background thread and send the
/// result on `sender`. Re-reads the full project git state after a
/// successful mutation so the drain loop has fresh data to replace
/// the optimistic snapshot with.
pub fn spawn_changed_files_mutation(
    sender: broadcast::Sender<ChangedFilesGitMutationReply>,
    project_id: String,
    project_path: PathBuf,
    mutation: ChangedFilesGitMutation,
) {
    thread::spawn(move || {
        let result =
            crate::git_operation::run_serialized_git_operation_for_path(&project_path, || {
                match mutation {
                    ChangedFilesGitMutation::StageFile { changed } => {
                        stage_changed_file(&project_path, &changed)
                            .map(|_| read_project_git_state(&project_path, false))
                    }
                    ChangedFilesGitMutation::UnstageFile { changed } => {
                        unstage_changed_file(&project_path, &changed)
                            .map(|_| read_project_git_state(&project_path, false))
                    }
                    ChangedFilesGitMutation::StageAll => stage_all_changes(&project_path)
                        .map(|_| read_project_git_state(&project_path, false)),
                    ChangedFilesGitMutation::UnstageAll => unstage_all_changes(&project_path)
                        .map(|_| read_project_git_state(&project_path, false)),
                    ChangedFilesGitMutation::RevertFiles { changed_files } => {
                        let reverted_any =
                            changed_files.iter().fold(false, |reverted_any, changed| {
                                revert_changed_file(&project_path, changed) || reverted_any
                            });

                        if reverted_any {
                            Ok(read_project_git_state(&project_path, false))
                        } else {
                            Err("Could not discard the selected file changes.".to_string())
                        }
                    }
                }
            });
        let _ = sender.send(ChangedFilesGitMutationReply { project_id, result });
    });
}

#[derive(Clone)]
pub struct ChangedFileDiffReply {
    pub selection: GitDiffSelection,
    pub result: Result<GitDiff, String>,
}

pub fn spawn_changed_file_diff_load(
    selection: GitDiffSelection,
    project_path: PathBuf,
) -> broadcast::Receiver<ChangedFileDiffReply> {
    let (tx, rx) = broadcast::channel(1);
    thread::spawn(move || {
        let result = read_changed_file_diff(&project_path, selection.clone());
        let _ = tx.send(ChangedFileDiffReply { selection, result });
    });
    rx
}

// ---- GitHub lookups -------------------------------------------------
//
// Four workers that all share the same queue-shape as
// `spawn_changed_files_mutation`: the desktop app owns a persistent
// `Sender` for each, the drain loop reads a stream of replies over
// the app's lifetime, and the worker is a thin shim around a
// `git_actions` helper. Grouped together for easy review; the reply
// structs mirror the on-disk shape the helpers already return.

#[derive(Clone)]
pub struct ProjectGitHubLinkReply {
    pub project_id: String,
    pub github_url: Option<String>,
}

/// Resolve a project's GitHub remote URL in the background.
pub fn spawn_github_link_lookup(
    sender: broadcast::Sender<ProjectGitHubLinkReply>,
    project_id: String,
    project_path: PathBuf,
) {
    thread::spawn(move || {
        let github_url = find_github_repo_url(&project_path);
        let _ = sender.send(ProjectGitHubLinkReply {
            project_id,
            github_url,
        });
    });
}

#[derive(Clone)]
pub struct ProjectPullRequestReply {
    pub lookup_key: String,
    pub pull_request: Option<PullRequestStatus>,
}

/// Look up the latest pull-request status for a branch. Routes
/// through the capability registry: resolves `Git` to read the
/// project's remote URL, then asks each registered
/// `GitRemoteProvider` whether it owns that URL. Returns `None`
/// for any of: no git on PATH, no remote configured, no remote
/// provider claims the URL, or the provider succeeded but found
/// no PR — same observable outcome as the pre-trait path.
pub fn spawn_pull_request_lookup(
    sender: broadcast::Sender<ProjectPullRequestReply>,
    lookup_key: String,
    project_path: PathBuf,
    branch_name: String,
) {
    thread::spawn(move || {
        let pull_request = lookup_pull_request(&project_path, &branch_name);
        let _ = sender.send(ProjectPullRequestReply {
            lookup_key,
            pull_request,
        });
    });
}

fn lookup_pull_request(project_path: &Path, branch_name: &str) -> Option<PullRequestStatus> {
    let scope = crate::capability::project_scope(String::new(), project_path.to_path_buf());
    let git = crate::git::resolve_git(&scope)?;
    let remote = git.remote_url(project_path, "origin")?;
    let provider = crate::git_remote::resolve_for_remote(&scope, &remote)?;
    provider
        .find_pull_request(project_path, branch_name)
        .ok()
        .flatten()
}

#[derive(Clone)]
pub struct ProjectPagePullRequestsReply {
    pub project_id: String,
    pub filter_index: usize,
    pub query: String,
    pub result: Result<Vec<ProjectPagePullRequest>, String>,
}

/// Query the project-page PR list (filter + text search). Routes
/// through the capability registry: a `GitRemoteProvider` is
/// selected by the project's remote URL. With no matching provider
/// (e.g. a self-hosted Gitea remote, or `gh` not installed), the
/// reply carries the "GitHub CLI not installed" string — same
/// observable behaviour as the pre-trait `MissingProvider`.
pub fn spawn_project_page_pull_requests(
    sender: broadcast::Sender<ProjectPagePullRequestsReply>,
    project_id: String,
    project_path: PathBuf,
    filter_index: usize,
    query: String,
) {
    thread::spawn(move || {
        let filter = match filter_index {
            1 => crate::git_remote::PrFilter::ReviewRequested,
            2 => crate::git_remote::PrFilter::Author,
            _ => crate::git_remote::PrFilter::AllOpen,
        };
        let result = list_project_pull_requests(&project_path, filter, &query);
        let _ = sender.send(ProjectPagePullRequestsReply {
            project_id,
            filter_index,
            query,
            result,
        });
    });
}

fn list_project_pull_requests(
    project_path: &Path,
    filter: crate::git_remote::PrFilter,
    query: &str,
) -> Result<Vec<ProjectPagePullRequest>, String> {
    let scope = crate::capability::project_scope(String::new(), project_path.to_path_buf());
    let provider = match crate::git::resolve_git(&scope)
        .and_then(|git| git.remote_url(project_path, "origin"))
        .and_then(|url| crate::git_remote::resolve_for_remote(&scope, &url))
    {
        Some(p) => p,
        None => return Err(crate::git_remote::RemoteError::NotInstalled.to_string()),
    };
    provider
        .list_pull_requests(project_path, filter, Some(query), 100)
        .map_err(|err| err.to_string())
}

#[derive(Clone)]
pub struct ProjectCheckRunsReply {
    pub lookup_key: String,
    pub result: Result<Option<Vec<PullRequestCheck>>, String>,
}

/// Fetch the GitHub check-runs (CI status) for a PR. Routes
/// through the capability registry; with no matching provider,
/// returns the "GitHub CLI not installed" string for parity with
/// the pre-trait fallback path.
pub fn spawn_check_runs_lookup(
    sender: broadcast::Sender<ProjectCheckRunsReply>,
    lookup_key: String,
    project_path: PathBuf,
    pull_request_number: Option<u64>,
) {
    thread::spawn(move || {
        let result = lookup_check_runs(&project_path, pull_request_number);
        let _ = sender.send(ProjectCheckRunsReply { lookup_key, result });
    });
}

fn lookup_check_runs(
    project_path: &Path,
    pull_request_number: Option<u64>,
) -> Result<Option<Vec<PullRequestCheck>>, String> {
    let scope = crate::capability::project_scope(String::new(), project_path.to_path_buf());
    let provider = match crate::git::resolve_git(&scope)
        .and_then(|git| git.remote_url(project_path, "origin"))
        .and_then(|url| crate::git_remote::resolve_for_remote(&scope, &url))
    {
        Some(p) => p,
        None => return Err(crate::git_remote::RemoteError::NotInstalled.to_string()),
    };
    provider
        .pull_request_checks(project_path, pull_request_number)
        .map_err(|err| err.to_string())
}

// ---- GitHub issue discovery -------------------------------------------------

#[derive(Clone)]
pub struct ProjectIssueDiscoveryReply {
    pub project_id: String,
    pub availability: GitHubIssueAvailability,
    /// Populated only when `availability` is `Available`; empty otherwise.
    pub issues: Result<Vec<GitHubIssueRecord>, String>,
}

/// Probe whether GitHub Issues are available for `project_path` and, if so,
/// fetch open issues — all in a single background hop.
pub fn spawn_project_issue_discovery(
    sender: broadcast::Sender<ProjectIssueDiscoveryReply>,
    project_id: String,
    project_path: PathBuf,
) {
    thread::spawn(move || {
        let availability = probe_github_issue_availability(&project_path);
        let issues = if matches!(availability, GitHubIssueAvailability::Available) {
            find_project_open_issues(&project_path)
        } else {
            Ok(Vec::new())
        };
        let _ = sender.send(ProjectIssueDiscoveryReply {
            project_id,
            availability,
            issues,
        });
    });
}
