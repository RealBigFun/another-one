//! Background git workers, extracted from `desktop/src/app.rs`.
//!
//! Each function here spawns an OS thread that does a blocking git
//! read or `gh` CLI call, then sends a reply on an `mpsc::Sender`. The
//! desktop `AnotherOneApp` drains the matching `Receiver` on its
//! render-timer tick and folds the reply back into UI state.
//!
//! No async runtime: desktop isn't async today. This module is the
//! smallest viable "workers in core, UI in desktop" seam; a future PR
//! can swap the `std::sync::mpsc` ↔ `tokio::sync::broadcast` pair
//! without touching the worker bodies.
//!
//! Reply types (`GitRefreshReply`, …) live here so desktop and any
//! future daemon/mobile client share the same vocabulary.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use crate::git_actions::{
    execute_toolbar_git_action, find_github_repo_url, find_latest_pull_request_status,
    find_project_pull_requests, find_pull_request_checks, GitActionSettings,
    ProjectPagePullRequest, PullRequestCheck, PullRequestStatus, ToolbarActionError,
    ToolbarActionOutcome, ToolbarGitAction,
};
use crate::project_store::{
    read_project_branch_commit_state, read_project_branch_compare_state, read_project_git_state,
    stage_all_changes, stage_changed_file, unstage_all_changes, unstage_changed_file, ChangedFile,
    ProjectBranchCommitState, ProjectBranchCompareState, ProjectGitState,
};

/// Result payload from `spawn_refresh` — one message per refresh call.
pub struct GitRefreshReply {
    pub project_id: String,
    pub include_metadata: bool,
    pub state: ProjectGitState,
    pub commit_state: Option<Result<ProjectBranchCommitState, String>>,
    pub compare_state: Option<Result<ProjectBranchCompareState, String>>,
}

/// Spawn a background git-status / metadata / commit / compare read for
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
/// - `compare_target_branch` — if `Some(branch)`, diff the current
///   branch against `branch` for the compare view.
pub fn spawn_refresh(
    project_id: String,
    project_path: PathBuf,
    include_metadata: bool,
    commit_limit: Option<usize>,
    compare_target_branch: Option<String>,
) -> mpsc::Receiver<GitRefreshReply> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let state = read_project_git_state(&project_path, include_metadata);
        let commit_state = commit_limit.map(|requested_limit| {
            read_project_branch_commit_state(&project_path, requested_limit)
        });
        let compare_state = compare_target_branch.as_deref().map(|target_branch| {
            read_project_branch_compare_state(&project_path, target_branch)
        });
        let _ = tx.send(GitRefreshReply {
            project_id,
            include_metadata,
            state,
            commit_state,
            compare_state,
        });
    });
    rx
}

/// Result payload from `spawn_toolbar_action`. Carries the project id
/// plus the raw outcome/error so the UI layer can decide how to
/// surface it (toast kind, refresh scheduling, modal dismissal, etc.)
/// without needing to know anything about `ToastKind` on the core
/// side.
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
) -> mpsc::Receiver<GitActionReply> {
    let (tx, rx) = mpsc::channel();
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
pub struct ChangedFilesGitMutationReply {
    pub project_id: String,
    pub result: Result<ProjectGitState, String>,
}

/// Run one staged-file mutation on a background thread and send the
/// result on `sender`. Re-reads the full project git state after a
/// successful mutation so the drain loop has fresh data to replace
/// the optimistic snapshot with.
pub fn spawn_changed_files_mutation(
    sender: mpsc::Sender<ChangedFilesGitMutationReply>,
    project_id: String,
    project_path: PathBuf,
    mutation: ChangedFilesGitMutation,
) {
    thread::spawn(move || {
        let result = match mutation {
            ChangedFilesGitMutation::StageFile { changed } => stage_changed_file(&project_path, &changed)
                .map(|_| read_project_git_state(&project_path, false)),
            ChangedFilesGitMutation::UnstageFile { changed } => unstage_changed_file(&project_path, &changed)
                .map(|_| read_project_git_state(&project_path, false)),
            ChangedFilesGitMutation::StageAll => stage_all_changes(&project_path)
                .map(|_| read_project_git_state(&project_path, false)),
            ChangedFilesGitMutation::UnstageAll => unstage_all_changes(&project_path)
                .map(|_| read_project_git_state(&project_path, false)),
        };
        let _ = sender.send(ChangedFilesGitMutationReply { project_id, result });
    });
}

// ---- GitHub lookups -------------------------------------------------
//
// Four workers that all share the same queue-shape as
// `spawn_changed_files_mutation`: the desktop app owns a persistent
// `Sender` for each, the drain loop reads a stream of replies over
// the app's lifetime, and the worker is a thin shim around a
// `git_actions` helper. Grouped together for easy review; the reply
// structs mirror the on-disk shape the helpers already return.

pub struct ProjectGitHubLinkReply {
    pub project_id: String,
    pub github_url: Option<String>,
}

/// Resolve a project's GitHub remote URL in the background.
pub fn spawn_github_link_lookup(
    sender: mpsc::Sender<ProjectGitHubLinkReply>,
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

pub struct ProjectPullRequestReply {
    pub lookup_key: String,
    pub pull_request: Option<PullRequestStatus>,
}

/// Look up the latest pull-request status for a branch.
pub fn spawn_pull_request_lookup(
    sender: mpsc::Sender<ProjectPullRequestReply>,
    lookup_key: String,
    project_path: PathBuf,
    branch_name: String,
) {
    thread::spawn(move || {
        let pull_request = find_latest_pull_request_status(&project_path, &branch_name);
        let _ = sender.send(ProjectPullRequestReply {
            lookup_key,
            pull_request,
        });
    });
}

pub struct ProjectPagePullRequestsReply {
    pub project_id: String,
    pub filter_index: usize,
    pub query: String,
    pub result: Result<Vec<ProjectPagePullRequest>, String>,
}

/// Query the project-page PR list (filter + text search).
pub fn spawn_project_page_pull_requests(
    sender: mpsc::Sender<ProjectPagePullRequestsReply>,
    project_id: String,
    project_path: PathBuf,
    filter_index: usize,
    query: String,
) {
    thread::spawn(move || {
        let result = find_project_pull_requests(&project_path, filter_index, Some(&query));
        let _ = sender.send(ProjectPagePullRequestsReply {
            project_id,
            filter_index,
            query,
            result,
        });
    });
}

pub struct ProjectCheckRunsReply {
    pub lookup_key: String,
    pub result: Result<Option<Vec<PullRequestCheck>>, String>,
}

/// Fetch the GitHub check-runs (CI status) for a PR.
pub fn spawn_check_runs_lookup(
    sender: mpsc::Sender<ProjectCheckRunsReply>,
    lookup_key: String,
    project_path: PathBuf,
    pull_request_number: Option<u64>,
) {
    thread::spawn(move || {
        let result = find_pull_request_checks(&project_path, pull_request_number);
        let _ = sender.send(ProjectCheckRunsReply { lookup_key, result });
    });
}
