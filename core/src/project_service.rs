//! Background project / task workers, extracted from `desktop/src/app.rs`.
//!
//! Two workers share this module because they both produce
//! `PreparedProject`s: `spawn_project_add` on "add an existing
//! directory", `spawn_task_creation` on "start a new worktree task".
//! Each returns a `mpsc::Receiver<…>` that yields exactly one reply
//! when the background thread finishes, mirroring the shape of
//! `git_service::spawn_refresh` / `spawn_toolbar_action`.
//!
//! Pure plumbing: the interesting work lives in `project_store`
//! helpers (`create_task_worktree`, `prepare_project`) already in
//! core. These spawn fns are the desktop's thread-creating shims
//! relocated verbatim so the daemon / mobile clients can reuse them
//! later without having to also host the GPUI binary.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::mpsc;
use std::thread;

use crate::agents::TerminalLaunchConfig;
use crate::project_store::{
    create_task_worktree, prepare_project, PreparedProject, Project, ProjectBranchSettings,
    ProjectCheckoutState, ProjectKind, RepoRecord,
};

// ---- project add ----------------------------------------------------

pub struct ProjectAddReply {
    pub result: Result<PreparedProject, String>,
}

/// Read an on-disk project directory (`git` metadata + working-tree
/// state) into a [`PreparedProject`]. One-shot: the returned receiver
/// yields exactly one reply.
pub fn spawn_project_add(path: PathBuf) -> mpsc::Receiver<ProjectAddReply> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = prepare_project(&path);
        let _ = tx.send(ProjectAddReply { result });
    });
    rx
}

// ---- task creation --------------------------------------------------

pub struct TaskCreationSuccess {
    pub original_project_id: String,
    pub project: PreparedProject,
    pub branch_name: String,
    pub task_name: String,
    pub launch_config: TerminalLaunchConfig,
}

pub struct TaskCreationFailure {
    pub message: String,
}

pub struct TaskCreationReply {
    pub result: Result<TaskCreationSuccess, TaskCreationFailure>,
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
    source_branch: String,
    launch_config: TerminalLaunchConfig,
) -> mpsc::Receiver<TaskCreationReply> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = create_task_worktree(
            &project_path,
            &project_name,
            &task_name,
            &generated_task_name,
            &source_branch,
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
                    checkout: ProjectCheckoutState::default(),
                    branch_settings: ProjectBranchSettings::default(),
                    worktree_name: created
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned()),
                    repo_common_dir: None,
                },
                repo: RepoRecord {
                    id: uuid::Uuid::new_v4().to_string(),
                    common_dir: None,
                    branch_order: Vec::new(),
                    branches_by_name: HashMap::new(),
                },
            }),
            branch_name: created.branch_name,
            task_name: created.task_name,
            launch_config,
        })
        .map_err(|message| TaskCreationFailure { message });
        let _ = tx.send(TaskCreationReply { result });
    });
    rx
}
