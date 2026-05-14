//! Background project / task workers, extracted from `desktop/src/app.rs`.
//!
//! Two workers share this module because they both produce
//! `PreparedProject`s: `spawn_project_add` on "add an existing
//! directory", `spawn_task_creation` on "start a new worktree task".
//! Each returns a `broadcast::Receiver<…>` that yields exactly one
//! reply when the background thread finishes, mirroring the shape of
//! `git_service::spawn_refresh` / `spawn_toolbar_action`.
//!
//! Pure plumbing: the interesting work lives in `project_store`
//! helpers (`create_task_worktree`, `prepare_project`) already in
//! core. These spawn fns are the desktop's thread-creating shims
//! relocated verbatim so the daemon / mobile clients can reuse them
//! later without having to also host the GPUI binary.

use std::collections::HashMap;
use std::path::PathBuf;
use std::thread;

use tokio::sync::broadcast;

use crate::agents::TerminalLaunchConfig;
use crate::git_operation::run_serialized_git_operation_for_path;
use crate::project_store::{
    create_branch_from_head, create_review_task_worktree, create_task_worktree,
    delete_local_branch, prepare_project, remove_task_worktree, CreateBranchMode, PreparedProject,
    Project, ProjectBranchSettings, ProjectCheckoutState, ProjectKind, RepoRecord,
    TaskWorktreeBranchMode,
};

// ---- project add ----------------------------------------------------

#[derive(Clone)]
pub struct ProjectAddReply {
    pub result: Result<PreparedProject, String>,
}

/// Read an on-disk project directory (`git` metadata + working-tree
/// state) into a [`PreparedProject`]. One-shot: the returned receiver
/// yields exactly one reply.
pub fn spawn_project_add(path: PathBuf) -> broadcast::Receiver<ProjectAddReply> {
    let (tx, rx) = broadcast::channel(1);
    thread::spawn(move || {
        let result = prepare_project(&path);
        let _ = tx.send(ProjectAddReply { result });
    });
    rx
}

// ---- task creation --------------------------------------------------

#[derive(Clone)]
pub struct TaskCreationSuccess {
    pub original_project_id: String,
    pub project: PreparedProject,
    pub branch_name: String,
    pub task_name: String,
    pub launch_config: TerminalLaunchConfig,
    pub run_automatic_actions: bool,
    pub open_agent: bool,
}

#[derive(Clone)]
pub struct TaskCreationFailure {
    pub message: String,
}

#[derive(Clone)]
pub struct TaskCreationReply {
    pub result: Result<TaskCreationSuccess, TaskCreationFailure>,
}

#[derive(Clone)]
pub struct BranchCreationSuccess {
    pub original_project_id: String,
    pub project: Option<PreparedProject>,
    pub branch_name: String,
    pub task_name: String,
    pub use_current_task: bool,
}

#[derive(Clone)]
pub struct BranchCreationReply {
    pub result: Result<BranchCreationSuccess, TaskCreationFailure>,
}

/// Create a new git worktree for a task, prepare a `PreparedProject`
/// pointing at it, and return the bundle of "here's the new project +
/// the launch config to spawn an agent into it". One-shot.
///
/// If `prepare_project` fails after the worktree is created, we still
/// produce a minimal fallback `PreparedProject` so the UI can proceed
/// with degraded metadata rather than leaving the user with a
/// half-created worktree and no visible artifact.
#[allow(clippy::too_many_arguments)]
pub fn spawn_task_creation(
    project_id: String,
    project_path: PathBuf,
    project_name: String,
    task_name: String,
    generated_task_name: String,
    branch_mode: TaskWorktreeBranchMode,
    launch_config: TerminalLaunchConfig,
) -> broadcast::Receiver<TaskCreationReply> {
    let (tx, rx) = broadcast::channel(1);
    thread::spawn(move || {
        let result = run_serialized_git_operation_for_path(&project_path, || {
            create_task_worktree(
                &project_path,
                &project_name,
                &task_name,
                &generated_task_name,
                branch_mode,
            )
            .map(|created| TaskCreationSuccess {
                original_project_id: project_id,
                project: prepare_project(&created.path).unwrap_or_else(|_| PreparedProject {
                    project: Project {
                        id: uuid::Uuid::new_v4().to_string(),
                        repo_id: uuid::Uuid::new_v4().to_string(),
                        name: created
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| created.path.display().to_string()),
                        path: created.path.clone(),
                        kind: ProjectKind::Worktree,
                        archived: false,
                        checkout: ProjectCheckoutState::default(),
                        branch_settings: ProjectBranchSettings::default(),
                        actions: Vec::new(),
                        worktree_name: created
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned()),
                        repo_common_dir: None,
                    },
                    repo: RepoRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        common_dir: None,
                        actions: Vec::new(),
                        branch_order: Vec::new(),
                        branches_by_name: HashMap::new(),
                    },
                }),
                branch_name: created.branch_name,
                task_name: created.task_name,
                launch_config,
                run_automatic_actions: true,
                open_agent: true,
            })
            .map_err(|message| TaskCreationFailure { message })
        });
        let _ = tx.send(TaskCreationReply { result });
    });
    rx
}

pub fn spawn_branch_creation(
    project_id: String,
    project_path: PathBuf,
    branch_name: String,
    use_current_task: bool,
    migrate_changes: bool,
) -> broadcast::Receiver<BranchCreationReply> {
    let (tx, rx) = broadcast::channel(1);
    thread::spawn(move || {
        let mode = if use_current_task {
            CreateBranchMode::CurrentTask
        } else {
            CreateBranchMode::Worktree { migrate_changes }
        };
        let result = run_serialized_git_operation_for_path(&project_path, || {
            create_branch_from_head(&project_path, &branch_name, mode)
                .map(|created| {
                    let project = if use_current_task {
                        None
                    } else {
                        Some(prepare_project(&created.path).unwrap_or_else(|_| {
                            PreparedProject {
                                project: Project {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    repo_id: uuid::Uuid::new_v4().to_string(),
                                    name: created
                                        .path
                                        .file_name()
                                        .map(|name| name.to_string_lossy().into_owned())
                                        .unwrap_or_else(|| created.path.display().to_string()),
                                    path: created.path.clone(),
                                    kind: ProjectKind::Worktree,
                                    archived: false,
                                    checkout: ProjectCheckoutState {
                                        current_branch: Some(created.branch_name.clone()),
                                        ..ProjectCheckoutState::default()
                                    },
                                    branch_settings: ProjectBranchSettings::default(),
                                    actions: Vec::new(),
                                    worktree_name: created
                                        .path
                                        .file_name()
                                        .map(|name| name.to_string_lossy().into_owned()),
                                    repo_common_dir: None,
                                },
                                repo: RepoRecord {
                                    id: uuid::Uuid::new_v4().to_string(),
                                    common_dir: None,
                                    actions: Vec::new(),
                                    branch_order: Vec::new(),
                                    branches_by_name: HashMap::new(),
                                },
                            }
                        }))
                    };

                    BranchCreationSuccess {
                        original_project_id: project_id,
                        project,
                        branch_name: created.branch_name,
                        task_name: created.task_name,
                        use_current_task,
                    }
                })
                .map_err(|message| TaskCreationFailure { message })
        });
        let _ = tx.send(BranchCreationReply { result });
    });
    rx
}

#[expect(
    clippy::too_many_arguments,
    reason = "Thread spawn boundary mirrors the request DTO."
)]
pub fn spawn_review_task_creation(
    project_id: String,
    project_path: PathBuf,
    task_name: String,
    pull_request_number: u64,
    head_branch: String,
    launch_config: TerminalLaunchConfig,
    run_automatic_actions: bool,
    open_agent: bool,
) -> broadcast::Receiver<TaskCreationReply> {
    let (tx, rx) = broadcast::channel(1);
    thread::spawn(move || {
        let result = run_serialized_git_operation_for_path(&project_path, || {
            create_review_task_worktree(
                &project_path,
                &task_name,
                pull_request_number,
                &head_branch,
            )
            .map(|created| TaskCreationSuccess {
                original_project_id: project_id,
                project: prepare_project(&created.path).unwrap_or_else(|_| PreparedProject {
                    project: Project {
                        id: uuid::Uuid::new_v4().to_string(),
                        repo_id: uuid::Uuid::new_v4().to_string(),
                        name: created
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_else(|| created.path.display().to_string()),
                        path: created.path.clone(),
                        kind: ProjectKind::Worktree,
                        archived: false,
                        checkout: ProjectCheckoutState::default(),
                        branch_settings: ProjectBranchSettings::default(),
                        actions: Vec::new(),
                        worktree_name: created
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned()),
                        repo_common_dir: None,
                    },
                    repo: RepoRecord {
                        id: uuid::Uuid::new_v4().to_string(),
                        common_dir: None,
                        actions: Vec::new(),
                        branch_order: Vec::new(),
                        branches_by_name: HashMap::new(),
                    },
                }),
                branch_name: created.branch_name,
                task_name: created.task_name,
                launch_config,
                run_automatic_actions,
                open_agent,
            })
            .map_err(|message| TaskCreationFailure { message })
        });
        let _ = tx.send(TaskCreationReply { result });
    });
    rx
}

/// Delete a task worktree, optionally deleting its local branch, behind
/// the process-wide git operation lock. Returns a branch-deletion
/// warning if the worktree was removed but `git branch -D` failed.
pub fn delete_task_worktree(
    repo_path: PathBuf,
    project_path: PathBuf,
    branch_name: String,
    force_delete_branch: bool,
) -> Result<Option<String>, String> {
    run_serialized_git_operation_for_path(&repo_path, || {
        remove_task_worktree(&repo_path, &project_path)?;

        let branch_warning = if force_delete_branch {
            delete_local_branch(&repo_path, &branch_name).err()
        } else {
            None
        };

        Ok(branch_warning)
    })
}
