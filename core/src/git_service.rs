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
    execute_toolbar_git_action, ToolbarActionError, ToolbarActionOutcome, ToolbarGitAction,
};
use crate::project_store::{
    read_project_branch_commit_state, read_project_branch_compare_state, read_project_git_state,
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
        let result = execute_toolbar_git_action(&project_path, action);
        let _ = tx.send(GitActionReply { project_id, result });
    });
    rx
}
