use crate::agents::TerminalLaunchConfig;
use crate::project_store::{Project, ProjectKind, Task, TaskKind, TaskWorktreeBranchMode};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub enum TaskLaunchRequest {
    Direct {
        project_id: String,
        task_name: String,
        generated_task_name: String,
        source_branch: String,
        launch_config: TerminalLaunchConfig,
        warm_launch_id: Option<u64>,
    },
    Worktree {
        project_id: String,
        task_name: String,
        generated_task_name: String,
        branch_mode: TaskWorktreeBranchMode,
        launch_config: TerminalLaunchConfig,
    },
    Review {
        project_id: String,
        pull_request_number: u64,
        pull_request_url: String,
        head_branch: String,
        launch_config: TerminalLaunchConfig,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingTaskLaunch {
    NewTaskModal,
    Review,
}

pub fn review_task_title(pull_request_number: u64) -> String {
    format!("Review #{pull_request_number}")
}

pub fn review_worktree_name_prefix(pull_request_number: u64) -> String {
    format!("review-{pull_request_number}-wt")
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TaskWorkspaceTarget {
    pub root_project_id: String,
    pub project_id: String,
    pub task_id: String,
    pub branch_name: String,
    pub project_path: PathBuf,
}

pub fn task_workspace_target(
    projects: &[Project],
    root_project: &Project,
    task: &Task,
) -> Option<TaskWorkspaceTarget> {
    let project = match task.kind {
        TaskKind::Direct => projects
            .iter()
            .find(|project| project.id == task.target_project_id)
            .unwrap_or(root_project),
        TaskKind::Worktree | TaskKind::MultiWorktree => {
            if task.worktree.is_some() {
                root_project
            } else {
                let project_id = task
                    .worktree_project_id
                    .as_ref()
                    .unwrap_or(&task.target_project_id);
                projects.iter().find(|project| project.id == *project_id)?
            }
        }
    };
    let worktree = task.worktree.as_ref();

    Some(TaskWorkspaceTarget {
        root_project_id: root_project.id.clone(),
        project_id: worktree.map_or_else(|| project.id.clone(), |worktree| worktree.id.clone()),
        task_id: task.id.clone(),
        branch_name: project
            .checkout
            .current_branch
            .clone()
            .or_else(|| worktree.and_then(|worktree| worktree.checkout.current_branch.clone()))
            .unwrap_or_else(|| task.branch_name.clone()),
        project_path: task
            .cwd
            .clone()
            .or_else(|| worktree.map(|worktree| worktree.path.clone()))
            .unwrap_or_else(|| project.path.clone()),
    })
}

pub fn existing_review_worktree_project<'a>(
    projects: &'a [Project],
    root_project: &Project,
    pull_request_number: u64,
    head_branch: &str,
    mut current_branch_for: impl FnMut(&str) -> Option<String>,
) -> Option<&'a Project> {
    let review_worktree_name_prefix = review_worktree_name_prefix(pull_request_number);
    projects.iter().find(|candidate| {
        candidate.repo_id == root_project.repo_id
            && matches!(candidate.kind, ProjectKind::Worktree)
            && (candidate
                .worktree_name
                .as_deref()
                .is_some_and(|worktree_name| {
                    worktree_name == review_worktree_name_prefix
                        || worktree_name
                            .strip_prefix(&review_worktree_name_prefix)
                            .is_some_and(|suffix| suffix.starts_with('-'))
                })
                || current_branch_for(&candidate.id)
                    .as_deref()
                    .is_some_and(|branch| branch == head_branch))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn sample_project(id: &str, repo_id: &str, kind: ProjectKind, branch: &str) -> Project {
        Project {
            id: id.to_string(),
            repo_id: repo_id.to_string(),
            name: id.to_string(),
            path: PathBuf::from(format!("/tmp/{id}")),
            kind,
            archived: false,
            checkout: crate::project_store::ProjectCheckoutState {
                current_branch: Some(branch.to_string()),
                lines_added: 0,
                lines_removed: 0,
            },
            branch_settings: crate::project_store::ProjectBranchSettings::default(),
            actions: Vec::new(),
            worktree_name: None,
            repo_common_dir: None,
        }
    }

    fn sample_task(
        id: &str,
        kind: TaskKind,
        root_project_id: &str,
        target_project_id: &str,
        branch_name: &str,
        worktree_project_id: Option<&str>,
        cwd: Option<PathBuf>,
    ) -> Task {
        Task {
            id: id.to_string(),
            name: id.to_string(),
            kind,
            root_project_id: root_project_id.to_string(),
            target_project_id: target_project_id.to_string(),
            branch_name: branch_name.to_string(),
            section_id: format!("{target_project_id}::{branch_name}::{id}"),
            worktree: None,
            worktree_project_id: worktree_project_id.map(str::to_string),
            tabs: Vec::new(),
            active_tab_id: String::new(),
            next_tab_id: 0,
            cwd,
        }
    }

    #[test]
    fn review_task_title_uses_pull_request_number() {
        assert_eq!(review_task_title(1808), "Review #1808");
    }

    #[test]
    fn review_worktree_name_prefix_uses_pull_request_number() {
        assert_eq!(review_worktree_name_prefix(1808), "review-1808-wt");
    }

    #[test]
    fn task_workspace_target_uses_direct_task_cwd_and_current_branch() {
        let root = sample_project("root", "repo-1", ProjectKind::Root, "feature/current");
        let task = sample_task(
            "task-1",
            TaskKind::Direct,
            "root",
            "root",
            "feature/stale",
            None,
            Some(PathBuf::from("/tmp/root-current")),
        );

        let target = task_workspace_target(std::slice::from_ref(&root), &root, &task)
            .expect("direct task target should resolve");

        assert_eq!(target.project_id, "root");
        assert_eq!(target.branch_name, "feature/current");
        assert_eq!(target.project_path, PathBuf::from("/tmp/root-current"));
    }

    #[test]
    fn task_workspace_target_uses_worktree_project_when_available() {
        let root = sample_project("root", "repo-1", ProjectKind::Root, "main");
        let worktree = sample_project("worktree", "repo-1", ProjectKind::Worktree, "feature/wt");
        let task = sample_task(
            "task-1",
            TaskKind::Worktree,
            "root",
            "worktree",
            "feature/stale",
            Some("worktree"),
            None,
        );

        let target = task_workspace_target(&[root.clone(), worktree.clone()], &root, &task)
            .expect("worktree task target should resolve");

        assert_eq!(target.project_id, "worktree");
        assert_eq!(target.branch_name, "feature/wt");
        assert_eq!(target.project_path, worktree.path);
    }

    #[test]
    fn existing_review_worktree_project_matches_same_repo_and_head_branch() {
        let root = sample_project("root", "repo-1", ProjectKind::Root, "main");
        let matching = sample_project("matching", "repo-1", ProjectKind::Worktree, "feature/pr");
        let other_repo = sample_project("other", "repo-2", ProjectKind::Worktree, "feature/pr");
        let projects = vec![root.clone(), other_repo, matching.clone()];

        let found =
            existing_review_worktree_project(&projects, &root, 1808, "feature/pr", |project_id| {
                projects
                    .iter()
                    .find(|project| project.id == project_id)
                    .and_then(|project| project.checkout.current_branch.clone())
            });

        assert_eq!(found.map(|project| project.id.as_str()), Some("matching"));
    }

    #[test]
    fn existing_review_worktree_project_matches_detached_review_worktree_name() {
        let root = sample_project("root", "repo-1", ProjectKind::Root, "main");
        let mut matching = sample_project("matching", "repo-1", ProjectKind::Worktree, "");
        matching.checkout.current_branch = None;
        matching.worktree_name = Some("review-1808-wt".to_string());
        let mut suffixed = sample_project("suffixed", "repo-1", ProjectKind::Worktree, "");
        suffixed.checkout.current_branch = None;
        suffixed.worktree_name = Some("review-1808-wt-2".to_string());
        let mut other_pr = sample_project("other-pr", "repo-1", ProjectKind::Worktree, "");
        other_pr.checkout.current_branch = None;
        other_pr.worktree_name = Some("review-1809-wt".to_string());
        let projects = vec![root.clone(), other_pr, matching.clone(), suffixed];

        let found =
            existing_review_worktree_project(&projects, &root, 1808, "feature/pr", |project_id| {
                projects
                    .iter()
                    .find(|project| project.id == project_id)
                    .and_then(|project| project.checkout.current_branch.clone())
            });

        assert_eq!(found.map(|project| project.id.as_str()), Some("matching"));
    }
}
