//! Persistent project store.
//!
//! Projects are saved as JSON in `~/.config/another-one/projects.json`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::agents::{AgentProviderKind, TerminalLaunchConfig, TerminalRestoreStatus};

const STORE_VERSION: u8 = 3;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoBranchRecord {
    pub name: String,
    #[serde(default)]
    pub last_commit_relative: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub ahead_count: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoRecord {
    pub id: String,
    #[serde(default)]
    pub common_dir: Option<PathBuf>,
    #[serde(default)]
    pub branch_order: Vec<String>,
    #[serde(default)]
    pub branches_by_name: HashMap<String, RepoBranchRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectKind {
    Root,
    Worktree,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectCheckoutState {
    #[serde(default)]
    pub current_branch: Option<String>,
    #[serde(default)]
    pub lines_added: i32,
    #[serde(default)]
    pub lines_removed: i32,
}

/// A git branch with optional diff stats resolved for a specific project/worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub ahead_count: usize,
    pub last_commit_relative: String,
    pub is_default: bool,
    pub is_current: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    pub id: String,
    pub repo_id: String,
    pub name: String,
    pub path: PathBuf,
    pub kind: ProjectKind,
    #[serde(default)]
    pub checkout: ProjectCheckoutState,
    #[serde(skip)]
    pub worktree_name: Option<String>,
    #[serde(skip)]
    pub repo_common_dir: Option<PathBuf>,
}

impl Project {
    pub fn is_worktree(&self) -> bool {
        self.kind == ProjectKind::Worktree
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGitMetadata {
    pub common_dir: Option<PathBuf>,
    pub kind: ProjectKind,
    pub checkout: ProjectCheckoutState,
    pub branch_order: Vec<String>,
    pub branches_by_name: HashMap<String, RepoBranchRecord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGitState {
    pub changed_files: Vec<ChangedFile>,
    pub current_branch: Option<String>,
    pub ahead_count: usize,
    pub metadata: Option<ProjectGitMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskKind {
    Direct,
    Worktree,
    MultiWorktree,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Task {
    pub id: String,
    pub name: String,
    pub kind: TaskKind,
    pub root_project_id: String,
    pub target_project_id: String,
    pub branch_name: String,
    pub section_id: String,
    #[serde(skip)]
    pub worktree_project_id: Option<String>,
    #[serde(skip)]
    pub tabs: Vec<PersistedTerminalTab>,
    #[serde(skip)]
    pub active_tab_id: String,
    #[serde(skip)]
    pub next_tab_id: usize,
    #[serde(skip)]
    pub cwd: Option<PathBuf>,
}

#[derive(Debug, Clone)]
pub struct CreatedTaskWorktree {
    pub path: PathBuf,
    pub branch_name: String,
    pub task_name: String,
}

#[derive(Debug, Clone)]
pub struct PreparedProject {
    pub project: Project,
    pub repo: RepoRecord,
}

/// A single file with changes relative to HEAD.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub original_path: Option<String>,
    pub staged_additions: i32,
    pub staged_deletions: i32,
    pub unstaged_additions: i32,
    pub unstaged_deletions: i32,
    pub index_status: char,
    pub worktree_status: char,
    pub untracked: bool,
}

impl ChangedFile {
    pub fn has_staged_changes(&self) -> bool {
        self.index_status != ' ' && self.index_status != '?'
    }

    pub fn has_unstaged_changes(&self) -> bool {
        self.untracked || (self.worktree_status != ' ' && self.worktree_status != '?')
    }

    pub fn can_stage(&self) -> bool {
        self.has_unstaged_changes()
    }

    pub fn can_unstage(&self) -> bool {
        self.has_staged_changes()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedTerminalTab {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub provider: Option<AgentProviderKind>,
    #[serde(default)]
    pub launch_config: Option<TerminalLaunchConfig>,
    #[serde(default)]
    pub restore_status: TerminalRestoreStatus,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSectionState {
    pub active_tab_id: String,
    pub next_tab_id: usize,
    #[serde(default)]
    pub cwd: Option<PathBuf>,
    #[serde(default)]
    pub tabs: Vec<PersistedTerminalTab>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiState {
    #[serde(default = "default_left_sidebar_open")]
    pub left_sidebar_open: bool,
    #[serde(default)]
    pub expanded_repo_ids: HashSet<String>,
    #[serde(default)]
    pub pinned_task_ids: HashSet<String>,
    #[serde(default)]
    pub last_active_section_id: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            left_sidebar_open: default_left_sidebar_open(),
            expanded_repo_ids: HashSet::new(),
            pinned_task_ids: HashSet::new(),
            last_active_section_id: None,
        }
    }
}

fn default_left_sidebar_open() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct StoreFile {
    version: u8,
    #[serde(default)]
    repos: HashMap<String, RepoRecord>,
    #[serde(default)]
    projects: HashMap<String, Project>,
    #[serde(default)]
    project_order: Vec<String>,
    #[serde(default)]
    tasks: HashMap<String, Task>,
    #[serde(default)]
    task_ids_by_root_project: HashMap<String, Vec<String>>,
    #[serde(default)]
    sections: HashMap<String, PersistedSectionState>,
    #[serde(default)]
    ui: UiState,
}

impl Default for StoreFile {
    fn default() -> Self {
        Self {
            version: STORE_VERSION,
            repos: HashMap::new(),
            projects: HashMap::new(),
            project_order: Vec::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            sections: HashMap::new(),
            ui: UiState::default(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProjectStore {
    pub repos: HashMap<String, RepoRecord>,
    projects_by_id: HashMap<String, Project>,
    pub projects: Vec<Project>,
    pub project_order: Vec<String>,
    tasks_by_id: HashMap<String, Task>,
    pub tasks: HashMap<String, Vec<Task>>,
    pub task_ids_by_root_project: HashMap<String, Vec<String>>,
    pub terminal_sections: HashMap<String, PersistedSectionState>,
    pub ui: UiState,
    file_path: PathBuf,
}

impl ProjectStore {
    #[hotpath::measure]
    pub fn load() -> Self {
        let file_path = Self::config_path();
        let StoreFile {
            repos,
            projects,
            project_order,
            tasks,
            task_ids_by_root_project,
            sections,
            mut ui,
            ..
        } = Self::read_from_disk(&file_path);

        let mut store = Self {
            repos,
            projects_by_id: projects,
            projects: Vec::new(),
            project_order,
            tasks_by_id: tasks,
            tasks: HashMap::new(),
            task_ids_by_root_project,
            terminal_sections: sections,
            ui: UiState::default(),
            file_path,
        };
        store.sanitize();
        store.rebuild_runtime_views();
        ui.expanded_repo_ids
            .retain(|repo_id| store.repos.contains_key(repo_id));
        ui.pinned_task_ids
            .retain(|task_id| store.tasks_by_id.contains_key(task_id));
        if ui
            .last_active_section_id
            .as_ref()
            .is_some_and(|section_id| !store.terminal_sections.contains_key(section_id))
        {
            ui.last_active_section_id = None;
        }
        store.ui = ui;
        store
    }

    pub fn ordered_projects(&self) -> Vec<&Project> {
        self.projects.iter().collect()
    }

    pub fn first_project(&self) -> Option<&Project> {
        self.projects.first()
    }

    pub fn project(&self, project_id: &str) -> Option<&Project> {
        self.projects_by_id.get(project_id)
    }

    pub fn project_mut(&mut self, project_id: &str) -> Option<&mut Project> {
        self.projects_by_id.get_mut(project_id)
    }

    pub fn repo(&self, repo_id: &str) -> Option<&RepoRecord> {
        self.repos.get(repo_id)
    }

    pub fn repo_mut(&mut self, repo_id: &str) -> Option<&mut RepoRecord> {
        self.repos.get_mut(repo_id)
    }

    pub fn repo_for_project(&self, project_id: &str) -> Option<&RepoRecord> {
        let project = self.project(project_id)?;
        self.repo(&project.repo_id)
    }

    pub fn task(&self, task_id: &str) -> Option<&Task> {
        self.tasks_by_id.get(task_id)
    }

    pub fn task_mut(&mut self, task_id: &str) -> Option<&mut Task> {
        self.tasks_by_id.get_mut(task_id)
    }

    pub fn tasks_for_root_project(&self, root_project_id: &str) -> Vec<&Task> {
        self.task_ids_by_root_project
            .get(root_project_id)
            .into_iter()
            .flatten()
            .filter_map(|task_id| self.tasks_by_id.get(task_id))
            .collect()
    }

    pub fn root_project_for_project(&self, project_id: &str) -> Option<&Project> {
        let project = self.project(project_id)?;
        self.projects_by_id.values().find(|candidate| {
            candidate.repo_id == project.repo_id && candidate.kind == ProjectKind::Root
        })
    }

    pub fn branch_names(&self, project_id: &str) -> Vec<String> {
        self.repo_for_project(project_id)
            .map(|repo| repo.branch_order.clone())
            .unwrap_or_default()
    }

    pub fn branch_view(&self, project_id: &str, branch_name: &str) -> Option<Branch> {
        let project = self.project(project_id)?;
        let repo = self.repo(&project.repo_id)?;
        let branch = repo.branches_by_name.get(branch_name)?;
        let is_current = project.checkout.current_branch.as_deref() == Some(branch_name);

        Some(Branch {
            name: branch.name.clone(),
            lines_added: if is_current {
                project.checkout.lines_added
            } else {
                0
            },
            lines_removed: if is_current {
                project.checkout.lines_removed
            } else {
                0
            },
            ahead_count: branch.ahead_count,
            last_commit_relative: branch.last_commit_relative.clone(),
            is_default: branch.is_default,
            is_current,
        })
    }

    pub fn primary_branch_for_project(
        &self,
        project_id: &str,
        prefer_default: bool,
    ) -> Option<Branch> {
        let repo = self.repo_for_project(project_id)?;
        let branch_name = if prefer_default {
            repo.branch_order
                .iter()
                .find(|name| {
                    repo.branches_by_name
                        .get(name.as_str())
                        .is_some_and(|branch| branch.is_default)
                })
                .cloned()
                .or_else(|| {
                    self.project(project_id)
                        .and_then(|project| project.checkout.current_branch.clone())
                })
                .or_else(|| repo.branch_order.first().cloned())
        } else {
            self.project(project_id)
                .and_then(|project| project.checkout.current_branch.clone())
                .or_else(|| repo.branch_order.first().cloned())
        }?;

        self.branch_view(project_id, &branch_name)
    }

    pub fn default_branch_name(&self, project_id: &str) -> Option<String> {
        let repo = self.repo_for_project(project_id)?;
        repo.branch_order.iter().find_map(|branch_name| {
            repo.branches_by_name
                .get(branch_name.as_str())
                .filter(|branch| branch.is_default)
                .map(|branch| branch.name.clone())
        })
    }

    pub fn current_branch_name(&self, project_id: &str) -> Option<String> {
        self.project(project_id)
            .and_then(|project| project.checkout.current_branch.clone())
    }

    pub fn worktree_name(&self, project_id: &str) -> Option<String> {
        let project = self.project(project_id)?;
        if project.kind != ProjectKind::Worktree {
            return None;
        }

        project
            .path
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
    }

    pub fn project_repo_id(&self, project_id: &str) -> Option<&str> {
        self.project(project_id)
            .map(|project| project.repo_id.as_str())
    }

    #[allow(dead_code)]
    pub fn remove_project(&mut self, project_id: &str) {
        let Some(project) = self.projects_by_id.remove(project_id) else {
            return;
        };
        self.project_order.retain(|id| id != project_id);

        let removed_task_ids = self
            .tasks_by_id
            .values()
            .filter(|task| {
                task.root_project_id == project_id || task.target_project_id == project_id
            })
            .map(|task| task.id.clone())
            .collect::<Vec<_>>();
        for task_id in removed_task_ids {
            let _ = self.remove_task_by_id(&task_id);
        }
        self.terminal_sections
            .retain(|section_id, _| !section_id.starts_with(&format!("{project_id}::")));
        if self
            .ui
            .last_active_section_id
            .as_ref()
            .is_some_and(|section_id| section_id.starts_with(&format!("{project_id}::")))
        {
            self.ui.last_active_section_id = None;
        }

        let repo_id = project.repo_id.clone();
        if !self
            .projects_by_id
            .values()
            .any(|candidate| candidate.repo_id == repo_id)
        {
            self.repos.remove(&repo_id);
            self.ui.expanded_repo_ids.remove(&repo_id);
        }
        self.rebuild_runtime_views();
        self.save();
    }

    pub fn remove_task(&mut self, root_project_id: &str, task_id: &str) -> Option<Task> {
        let task = self.tasks_by_id.get(task_id)?;
        if task.root_project_id != root_project_id {
            return None;
        }
        self.remove_task_by_id(task_id)
    }

    fn remove_task_by_id(&mut self, task_id: &str) -> Option<Task> {
        let removed = self.tasks_by_id.remove(task_id)?;
        if let Some(task_ids) = self
            .task_ids_by_root_project
            .get_mut(&removed.root_project_id)
        {
            task_ids.retain(|id| id != task_id);
            if task_ids.is_empty() {
                self.task_ids_by_root_project
                    .remove(&removed.root_project_id);
            }
        }
        self.terminal_sections.remove(&removed.section_id);
        if self.ui.last_active_section_id.as_deref() == Some(&removed.section_id) {
            self.ui.last_active_section_id = None;
        }
        self.ui.pinned_task_ids.remove(task_id);
        self.rebuild_runtime_views();
        Some(removed)
    }

    pub fn insert_task(&mut self, task: Task) {
        self.task_ids_by_root_project
            .entry(task.root_project_id.clone())
            .or_default()
            .push(task.id.clone());
        self.tasks_by_id.insert(task.id.clone(), task);
        self.rebuild_runtime_views();
    }

    pub fn set_task_pinned(&mut self, task_id: &str, pinned: bool) -> bool {
        if pinned {
            self.ui.pinned_task_ids.insert(task_id.to_string())
        } else {
            self.ui.pinned_task_ids.remove(task_id)
        }
    }

    pub fn insert_prepared_project(&mut self, prepared: PreparedProject) -> bool {
        if self
            .projects_by_id
            .values()
            .any(|existing| existing.path == prepared.project.path)
        {
            return false;
        }

        let repo_id = prepared
            .repo
            .common_dir
            .as_ref()
            .and_then(|common_dir| {
                self.repos.iter().find_map(|(repo_id, repo)| {
                    (repo.common_dir.as_ref() == Some(common_dir)).then(|| repo_id.clone())
                })
            })
            .unwrap_or_else(|| prepared.repo.id.clone());

        if let Some(repo) = self.repos.get_mut(&repo_id) {
            *repo = merge_repo(repo.clone(), prepared.repo);
        } else {
            let mut repo = prepared.repo;
            repo.id = repo_id.clone();
            self.repos.insert(repo_id.clone(), repo);
        }

        let mut project = prepared.project;
        project.repo_id = repo_id;
        self.project_order.push(project.id.clone());
        self.projects_by_id.insert(project.id.clone(), project);
        self.rebuild_runtime_views();
        self.save();
        true
    }

    #[hotpath::measure]
    pub fn save(&self) {
        let store = StoreFile {
            version: STORE_VERSION,
            repos: self.repos.clone(),
            projects: self.projects_by_id.clone(),
            project_order: self.project_order.clone(),
            tasks: self.tasks_by_id.clone(),
            task_ids_by_root_project: self.task_ids_by_root_project.clone(),
            sections: self.terminal_sections.clone(),
            ui: self.ui.clone(),
        };
        if let Some(parent) = self.file_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(&store) {
            let _ = std::fs::write(&self.file_path, json);
        }
    }

    pub fn set_left_sidebar_open(&mut self, is_open: bool) {
        if self.ui.left_sidebar_open == is_open {
            return;
        }
        self.ui.left_sidebar_open = is_open;
        self.save();
    }

    pub fn set_expanded_repos(&mut self, expanded_repo_ids: &HashSet<String>) {
        self.ui.expanded_repo_ids = expanded_repo_ids.clone();
        self.save();
    }

    pub fn set_last_active_section_id(&mut self, section_id: Option<String>) {
        if self.ui.last_active_section_id == section_id {
            return;
        }

        self.ui.last_active_section_id = section_id;
        self.save();
    }

    pub fn set_section_state(
        &mut self,
        section_id: impl Into<String>,
        state: PersistedSectionState,
    ) {
        self.terminal_sections.insert(section_id.into(), state);
        self.rebuild_runtime_views();
        self.save();
    }

    pub fn remove_sections(&mut self, section_ids: &HashSet<String>) -> bool {
        let before = self.terminal_sections.len();
        self.terminal_sections
            .retain(|section_id, _| !section_ids.contains(section_id));
        let changed = before != self.terminal_sections.len();
        if changed {
            if self
                .ui
                .last_active_section_id
                .as_ref()
                .is_some_and(|section_id| section_ids.contains(section_id))
            {
                self.ui.last_active_section_id = None;
            }
            self.rebuild_runtime_views();
            self.save();
        }
        changed
    }

    fn sanitize(&mut self) {
        self.project_order
            .retain(|project_id| self.projects_by_id.contains_key(project_id));
        let ordered_set = self.project_order.iter().cloned().collect::<HashSet<_>>();
        let missing = self
            .projects_by_id
            .keys()
            .filter(|project_id| !ordered_set.contains(*project_id))
            .cloned()
            .collect::<Vec<_>>();
        self.project_order.extend(missing);

        self.projects_by_id
            .retain(|_, project| self.repos.contains_key(&project.repo_id));
        self.project_order
            .retain(|project_id| self.projects_by_id.contains_key(project_id));

        self.tasks_by_id.retain(|_, task| {
            self.projects_by_id.contains_key(&task.root_project_id)
                && self.projects_by_id.contains_key(&task.target_project_id)
        });

        self.task_ids_by_root_project
            .retain(|root_project_id, task_ids| {
                if !self.projects_by_id.contains_key(root_project_id) {
                    return false;
                }
                task_ids.retain(|task_id| {
                    self.tasks_by_id
                        .get(task_id)
                        .is_some_and(|task| task.root_project_id == *root_project_id)
                });
                !task_ids.is_empty()
            });

        let indexed_task_ids = self
            .task_ids_by_root_project
            .values()
            .flatten()
            .cloned()
            .collect::<HashSet<_>>();
        for task in self.tasks_by_id.values() {
            if !indexed_task_ids.contains(&task.id) {
                self.task_ids_by_root_project
                    .entry(task.root_project_id.clone())
                    .or_default()
                    .push(task.id.clone());
            }
        }

        self.terminal_sections.retain(|section_id, _| {
            let mut parts = section_id.splitn(3, "::");
            let Some(project_id) = parts.next() else {
                return false;
            };
            let Some(_branch_name) = parts.next() else {
                return false;
            };
            let Some(task_id) = parts.next() else {
                return false;
            };
            self.projects_by_id.contains_key(project_id)
                && (task_id.is_empty() || self.tasks_by_id.contains_key(task_id))
        });
    }

    fn rebuild_runtime_views(&mut self) {
        self.projects = self
            .project_order
            .iter()
            .filter_map(|project_id| {
                let mut project = self.projects_by_id.get(project_id)?.clone();
                project.worktree_name = if project.is_worktree() {
                    project
                        .path
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned())
                } else {
                    None
                };
                project.repo_common_dir = self
                    .repo_for_project(project_id)
                    .and_then(|repo| repo.common_dir.clone());
                Some(project)
            })
            .collect();

        self.tasks = self
            .task_ids_by_root_project
            .iter()
            .map(|(root_project_id, task_ids)| {
                let tasks = task_ids
                    .iter()
                    .filter_map(|task_id| {
                        let mut task = self.tasks_by_id.get(task_id)?.clone();
                        task.worktree_project_id = (task.target_project_id != task.root_project_id)
                            .then(|| task.target_project_id.clone());
                        if let Some(section) = self.terminal_sections.get(&task.section_id) {
                            task.tabs = section.tabs.clone();
                            task.active_tab_id = section.active_tab_id.clone();
                            task.next_tab_id = section.next_tab_id;
                            task.cwd = section.cwd.clone();
                        } else {
                            task.tabs = Vec::new();
                            task.active_tab_id = String::new();
                            task.next_tab_id = 0;
                            task.cwd = None;
                        }
                        Some(task)
                    })
                    .collect::<Vec<_>>();
                (root_project_id.clone(), tasks)
            })
            .collect();
    }

    pub fn refresh_runtime_views(&mut self) {
        self.rebuild_runtime_views();
    }

    fn config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("another-one").join("projects.json")
    }

    fn read_from_disk(path: &Path) -> StoreFile {
        let Ok(contents) = std::fs::read_to_string(path) else {
            return StoreFile::default();
        };
        match serde_json::from_str::<StoreFile>(&contents) {
            Ok(store) if store.version == STORE_VERSION => store,
            _ => {
                Self::backup_incompatible_store(path);
                StoreFile::default()
            }
        }
    }

    fn backup_incompatible_store(path: &Path) {
        let Some(parent) = path.parent() else {
            return;
        };
        let backup_path = parent.join("projects.v2.backup.json");
        let _ = std::fs::remove_file(&backup_path);
        let _ = std::fs::rename(path, backup_path);
    }

    pub fn set_expanded_projects(&mut self, expanded_repo_ids: &HashSet<String>) {
        self.set_expanded_repos(expanded_repo_ids);
    }

    pub fn set_last_active_section_key(&mut self, section_id: Option<String>) {
        self.set_last_active_section_id(section_id);
    }

    pub fn set_terminal_section(
        &mut self,
        section_id: impl Into<String>,
        state: PersistedSectionState,
    ) {
        self.set_section_state(section_id, state);
    }

    pub fn remove_terminal_sections(&mut self, section_ids: &HashSet<String>) -> bool {
        self.remove_sections(section_ids)
    }

    pub fn find_task_mut(&mut self, task_id: &str) -> Option<&mut Task> {
        self.task_mut(task_id)
    }

    pub fn update_task_tabs(&mut self, task_id: &str, state: &PersistedSectionState) {
        let section_id = self.task(task_id).map(|task| task.section_id.clone());
        if let Some(section_id) = section_id {
            self.terminal_sections.insert(section_id, state.clone());
            self.rebuild_runtime_views();
            self.save();
        }
    }
}

pub fn prepare_project(folder: &Path) -> Result<PreparedProject, String> {
    let canonical = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());

    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| canonical.display().to_string());

    let branches = detect_branches(&canonical);
    let (branch_order, branches_by_name) = repo_branch_catalog_from_resolved(&branches);
    let common_dir = detect_repo_common_dir(&canonical);
    let kind = detect_project_kind(&canonical);

    Ok(PreparedProject {
        project: Project {
            id: uuid::Uuid::new_v4().to_string(),
            repo_id: uuid::Uuid::new_v4().to_string(),
            name,
            path: canonical.clone(),
            kind,
            checkout: checkout_state_from_resolved(&branches),
            worktree_name: detect_worktree_name(&canonical),
            repo_common_dir: common_dir.clone(),
        },
        repo: RepoRecord {
            id: uuid::Uuid::new_v4().to_string(),
            common_dir,
            branch_order,
            branches_by_name,
        },
    })
}

#[hotpath::measure]
pub fn read_project_git_state(path: &Path, include_metadata: bool) -> ProjectGitState {
    let current_branch = git_current_branch(path);
    ProjectGitState {
        changed_files: list_changed_files(path),
        ahead_count: current_branch
            .as_deref()
            .map(|branch| git_ahead_count(path, branch))
            .unwrap_or(0),
        current_branch,
        metadata: include_metadata.then(|| read_project_git_metadata(path)),
    }
}

fn read_project_git_metadata(path: &Path) -> ProjectGitMetadata {
    let branches = detect_branches(path);
    let (branch_order, branches_by_name) = repo_branch_catalog_from_resolved(&branches);
    ProjectGitMetadata {
        common_dir: detect_repo_common_dir(path),
        kind: detect_project_kind(path),
        checkout: checkout_state_from_resolved(&branches),
        branch_order,
        branches_by_name,
    }
}

fn repo_branch_catalog_from_resolved(
    branches: &[Branch],
) -> (Vec<String>, HashMap<String, RepoBranchRecord>) {
    let branch_order = branches.iter().map(|branch| branch.name.clone()).collect();
    let branches_by_name = branches
        .iter()
        .map(|branch| {
            (
                branch.name.clone(),
                RepoBranchRecord {
                    name: branch.name.clone(),
                    last_commit_relative: branch.last_commit_relative.clone(),
                    is_default: branch.is_default,
                    ahead_count: branch.ahead_count,
                },
            )
        })
        .collect();
    (branch_order, branches_by_name)
}

fn checkout_state_from_resolved(branches: &[Branch]) -> ProjectCheckoutState {
    branches
        .iter()
        .find(|branch| branch.is_current)
        .map(|branch| ProjectCheckoutState {
            current_branch: Some(branch.name.clone()),
            lines_added: branch.lines_added,
            lines_removed: branch.lines_removed,
        })
        .unwrap_or_default()
}

fn detect_project_kind(path: &Path) -> ProjectKind {
    if detect_worktree_name(path).is_some() {
        ProjectKind::Worktree
    } else {
        ProjectKind::Root
    }
}

fn merge_repo(mut existing: RepoRecord, incoming: RepoRecord) -> RepoRecord {
    if existing.common_dir.is_none() {
        existing.common_dir = incoming.common_dir;
    }
    if !incoming.branch_order.is_empty() {
        existing.branch_order = incoming.branch_order;
    }
    if !incoming.branches_by_name.is_empty() {
        existing.branches_by_name = incoming.branches_by_name;
    }
    existing
}

fn git_command(path: &Path) -> Command {
    let mut command = Command::new("git");
    command.current_dir(path);
    command
}

fn git_status_ok(path: &Path, args: &[&str]) -> bool {
    git_command(path)
        .args(args)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn git_stdout(path: &Path, args: &[&str]) -> Option<String> {
    let output = git_command(path).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    (!stdout.is_empty()).then_some(stdout)
}

fn append_changed_paths(command: &mut Command, changed: &ChangedFile) {
    if let Some(original_path) = changed.original_path.as_deref() {
        command.arg(original_path);
    }
    command.arg(&changed.path);
}

/// Return changed files for the repo rooted at `path`.
pub fn list_changed_files(path: &Path) -> Vec<ChangedFile> {
    let mut changed = Vec::new();
    let staged_stats = git_numstat_by_path(path, true);
    let unstaged_stats = git_numstat_by_path(path, false);

    let Ok(output) = git_command(path)
        .args(["status", "--porcelain", "--untracked-files=all"])
        .output()
    else {
        return changed;
    };

    if !output.status.success() {
        return changed;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let Some((index_status, worktree_status, original_path, changed_path)) =
            parse_status_line(line)
        else {
            continue;
        };

        let (staged_additions, staged_deletions) =
            staged_stats.get(&changed_path).copied().unwrap_or((0, 0));
        let (unstaged_additions, unstaged_deletions) = unstaged_stats
            .get(&changed_path)
            .copied()
            .unwrap_or_else(|| {
                if index_status == '?' || worktree_status == '?' {
                    (count_file_lines(&path.join(&changed_path)), 0)
                } else {
                    (0, 0)
                }
            });

        changed.push(ChangedFile {
            path: changed_path,
            original_path,
            staged_additions,
            staged_deletions,
            unstaged_additions,
            unstaged_deletions,
            index_status,
            worktree_status,
            untracked: index_status == '?' || worktree_status == '?',
        });
    }

    changed
}

/// Stage a changed file in the repo rooted at `path`.
pub fn stage_changed_file(path: &Path, changed: &ChangedFile) -> Result<(), String> {
    let mut cmd = git_command(path);
    cmd.arg("add").arg("-A").arg("--");
    append_changed_paths(&mut cmd, changed);
    let output = cmd
        .output()
        .map_err(|error| format!("Could not stage {}: {error}", changed.path))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format_git_command_failure(
            &format!("Could not stage {}", changed.path),
            &output,
        ))
    }
}

/// Stage every change in the repo rooted at `path`.
pub fn stage_all_changes(path: &Path) -> Result<(), String> {
    let output = git_command(path)
        .args(["add", "-A"])
        .output()
        .map_err(|error| format!("Could not stage changes: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format_git_command_failure(
            "Could not stage changes",
            &output,
        ))
    }
}

/// Unstage every currently staged change in the repo rooted at `path`.
pub fn unstage_all_changes(path: &Path) -> Result<(), String> {
    let restore_output = git_command(path)
        .args(["restore", "--staged", "--", "."])
        .output()
        .map_err(|error| format!("Could not unstage staged changes: {error}"))?;

    if restore_output.status.success() {
        return Ok(());
    }

    let reset_output = git_command(path)
        .args(["reset", "HEAD", "--", "."])
        .output()
        .map_err(|error| format!("Could not unstage staged changes: {error}"))?;

    if reset_output.status.success() {
        return Ok(());
    }

    Err(format_git_command_failure(
        "Could not unstage staged changes",
        &reset_output,
    ))
}

/// Unstage a changed file in the repo rooted at `path`.
pub fn unstage_changed_file(path: &Path, changed: &ChangedFile) -> Result<(), String> {
    let error_prefix = format!("Could not unstage {}", changed.path);
    let mut restore = git_command(path);
    restore.args(["restore", "--staged", "--"]);
    append_changed_paths(&mut restore, changed);

    let restore_output = restore
        .output()
        .map_err(|error| format!("{error_prefix}: {error}"))?;

    if restore_output.status.success() {
        return Ok(());
    }

    let mut reset = git_command(path);
    reset.args(["reset", "HEAD", "--"]);
    append_changed_paths(&mut reset, changed);

    let reset_output = reset
        .output()
        .map_err(|error| format!("{error_prefix}: {error}"))?;

    if reset_output.status.success() {
        return Ok(());
    }

    Err(format_git_command_failure(&error_prefix, &reset_output))
}

fn format_git_command_failure(prefix: &str, output: &Output) -> String {
    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    format_git_command_error(prefix, &stderr, &stdout)
}

fn format_git_command_error(prefix: &str, stderr: &str, stdout: &str) -> String {
    let stderr = stderr.trim();
    let stdout = stdout.trim();

    if is_git_index_lock_error(stderr, stdout) {
        return format!(
            "{prefix}. Another git process or a stale `.git/index.lock` file is blocking this repository. Finish the other git command or remove the lock file, then try again."
        );
    }

    let detail = if !stderr.is_empty() {
        stderr
    } else if !stdout.is_empty() {
        stdout
    } else {
        "No additional details were reported."
    };

    format!("{prefix}. {detail}")
}

fn is_git_index_lock_error(stderr: &str, stdout: &str) -> bool {
    let combined = format!("{stderr}\n{stdout}");
    combined.contains("index.lock")
        && (combined.contains("Another git process seems to be running")
            || combined.contains("Unable to create")
            || combined.contains("File exists"))
}

pub fn current_branch(path: &Path) -> Option<String> {
    git_current_branch(path)
}

pub fn create_task_worktree(
    repo_path: &Path,
    project_name: &str,
    requested_task_name: &str,
    fallback_task_name: &str,
    source_branch: &str,
) -> Result<CreatedTaskWorktree, String> {
    let task_name = if requested_task_name.trim().is_empty() {
        fallback_task_name.trim().to_string()
    } else {
        requested_task_name.trim().to_string()
    };
    let task_name = if task_name.is_empty() {
        "task".to_string()
    } else {
        task_name
    };

    let base_branch = if source_branch.trim().is_empty() {
        current_branch(repo_path)
            .or_else(|| git_default_branch(repo_path))
            .ok_or_else(|| "Could not determine a base branch for the new worktree.".to_string())?
    } else {
        source_branch.trim().to_string()
    };

    let slug = slugify_task_name(&task_name);
    let branch_name = unique_branch_name(repo_path, &slug)?;
    let worktree_path = unique_worktree_path(repo_path, project_name, &slug);

    let output = git_command(repo_path)
        .args([
            "worktree",
            "add",
            "-b",
            &branch_name,
            worktree_path.to_string_lossy().as_ref(),
            &base_branch,
        ])
        .output()
        .map_err(|error| format!("Failed to create worktree: {error}"))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(if stderr.is_empty() {
            "Git failed to create the worktree.".to_string()
        } else {
            format!("Git failed to create the worktree: {stderr}")
        });
    }

    Ok(CreatedTaskWorktree {
        path: worktree_path,
        branch_name,
        task_name,
    })
}

pub fn remove_task_worktree(repo_path: &Path, worktree_path: &Path) -> Result<(), String> {
    let output = git_command(repo_path)
        .args([
            "worktree",
            "remove",
            "--force",
            worktree_path.to_string_lossy().as_ref(),
        ])
        .output()
        .map_err(|error| format!("Could not delete the worktree: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format_git_command_failure(
            "Could not delete the worktree",
            &output,
        ))
    }
}

pub fn delete_local_branch(repo_path: &Path, branch_name: &str) -> Result<(), String> {
    if branch_name.trim().is_empty() {
        return Ok(());
    }

    let output = git_command(repo_path)
        .args(["branch", "-D", branch_name])
        .output()
        .map_err(|error| format!("Could not delete the local branch: {error}"))?;

    if output.status.success() {
        Ok(())
    } else {
        Err(format_git_command_failure(
            "Could not delete the local branch",
            &output,
        ))
    }
}

/// Revert a changed file in the repo rooted at `path`.
pub fn revert_changed_file(path: &Path, changed: &ChangedFile) -> bool {
    if changed.untracked {
        return remove_untracked_path(&path.join(&changed.path));
    }

    let mut restore = git_command(path);
    restore.args(["restore", "--source=HEAD", "--staged", "--worktree", "--"]);
    append_changed_paths(&mut restore, changed);

    if restore
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
    {
        return true;
    }

    let mut checkout = git_command(path);
    checkout.args(["checkout", "--"]);
    append_changed_paths(&mut checkout, changed);

    checkout
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// Detect local branches for a git repo at `path`.
fn detect_branches(path: &Path) -> Vec<Branch> {
    let default_branch = git_default_branch(path);
    let current_branch = git_current_branch(path);
    let mut local_branch_names = HashSet::new();

    let Ok(out) = git_command(path)
        .args([
            "for-each-ref",
            "--format=%(HEAD)|%(refname:short)|%(committerdate:relative)",
            "refs/heads",
        ])
        .output()
    else {
        return fallback_branches(path, default_branch, current_branch);
    };

    if !out.status.success() {
        return fallback_branches(path, default_branch, current_branch);
    }

    let mut branches = Vec::new();
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        let mut parts = line.splitn(3, '|');
        let Some(head_marker) = parts.next() else {
            continue;
        };
        let Some(name) = parts.next() else {
            continue;
        };
        let Some(last_commit_relative) = parts.next() else {
            continue;
        };

        let name = name.trim();
        if name.is_empty() {
            continue;
        }

        let is_current = head_marker.trim() == "*"
            || current_branch
                .as_deref()
                .map(|current| current == name)
                .unwrap_or(false);
        local_branch_names.insert(name.to_string());
        let (lines_added, lines_removed) = if is_current {
            git_diff_stat(path, name)
        } else {
            (0, 0)
        };
        let ahead_count = git_ahead_count(path, name);

        branches.push(Branch {
            name: name.to_string(),
            lines_added,
            lines_removed,
            ahead_count,
            last_commit_relative: last_commit_relative.trim().to_string(),
            is_default: default_branch
                .as_deref()
                .map(|default| default == name)
                .unwrap_or(false),
            is_current,
        });
    }

    if let Ok(out) = git_command(path)
        .args([
            "for-each-ref",
            "--format=%(refname:short)|%(committerdate:relative)",
            "refs/remotes",
        ])
        .output()
    {
        if out.status.success() {
            let text = String::from_utf8_lossy(&out.stdout);
            for line in text.lines() {
                let mut parts = line.splitn(2, '|');
                let Some(name) = parts.next() else {
                    continue;
                };
                let Some(last_commit_relative) = parts.next() else {
                    continue;
                };

                let name = name.trim();
                if name.is_empty() || name.ends_with("/HEAD") {
                    continue;
                }

                let Some((_, local_branch_name)) = name.split_once('/') else {
                    continue;
                };
                if local_branch_names.contains(local_branch_name) {
                    continue;
                }

                branches.push(Branch {
                    name: name.to_string(),
                    lines_added: 0,
                    lines_removed: 0,
                    ahead_count: 0,
                    last_commit_relative: last_commit_relative.trim().to_string(),
                    is_default: false,
                    is_current: false,
                });
            }
        }
    }

    if branches.is_empty() {
        fallback_branches(path, default_branch, current_branch)
    } else {
        branches
    }
}

fn fallback_branches(
    path: &Path,
    default_branch: Option<String>,
    current_branch: Option<String>,
) -> Vec<Branch> {
    let Some(branch_name) = current_branch.or(default_branch.clone()) else {
        return Vec::new();
    };

    let (added, removed) = git_diff_stat(path, &branch_name);
    let last_commit = git_last_commit_relative(path, &branch_name);

    vec![Branch {
        name: branch_name.clone(),
        lines_added: added,
        lines_removed: removed,
        ahead_count: git_ahead_count(path, &branch_name),
        last_commit_relative: last_commit,
        is_default: default_branch
            .as_deref()
            .map(|default| default == branch_name)
            .unwrap_or(false),
        is_current: true,
    }]
}

/// Try to determine the default branch: check symbolic ref of origin/HEAD,
/// then fall back to checking if main or master exists.
fn git_default_branch(path: &Path) -> Option<String> {
    // Try origin/HEAD first.
    if let Some(remote_head) = git_stdout(
        path,
        &["symbolic-ref", "refs/remotes/origin/HEAD", "--short"],
    ) {
        if let Some(name) = remote_head.strip_prefix("origin/") {
            return Some(name.to_string());
        }
    }

    // Fall back: check if 'main' or 'master' branch exists.
    if let Some(branches) = git_stdout(path, &["branch", "--list", "main", "master"]) {
        for line in branches.lines() {
            let name = line.trim().trim_start_matches('*').trim();
            if name == "main" || name == "master" {
                return Some(name.to_string());
            }
        }
    }

    // Last resort: current branch
    git_current_branch(path)
}

fn git_current_branch(path: &Path) -> Option<String> {
    git_stdout(path, &["rev-parse", "--abbrev-ref", "HEAD"]).and_then(|name| {
        if name == "HEAD" {
            None
        } else {
            Some(name)
        }
    })
}

fn slugify_task_name(task_name: &str) -> String {
    let mut slug = String::new();
    let mut last_was_dash = false;

    for ch in task_name.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_was_dash = false;
        } else if !last_was_dash {
            slug.push('-');
            last_was_dash = true;
        }
    }

    let slug = slug.trim_matches('-').to_string();
    if slug.is_empty() {
        "task".to_string()
    } else {
        slug
    }
}

fn unique_branch_name(repo_path: &Path, base_slug: &str) -> Result<String, String> {
    for suffix in 0..1000 {
        let candidate = if suffix == 0 {
            base_slug.to_string()
        } else {
            format!("{base_slug}-{}", suffix + 1)
        };

        if !branch_exists_anywhere(repo_path, &candidate)? {
            return Ok(candidate);
        }
    }

    Err("Could not find an available branch name for the new worktree.".to_string())
}

fn branch_exists_anywhere(repo_path: &Path, branch_name: &str) -> Result<bool, String> {
    if git_status_ok(
        repo_path,
        &[
            "show-ref",
            "--verify",
            "--quiet",
            &format!("refs/heads/{branch_name}"),
        ],
    ) {
        return Ok(true);
    }

    let output = git_command(repo_path)
        .args(["for-each-ref", "--format=%(refname:short)", "refs/remotes"])
        .output()
        .map_err(|error| format!("Failed to inspect remote branches: {error}"))?;

    if !output.status.success() {
        return Ok(false);
    }

    let exists = String::from_utf8_lossy(&output.stdout).lines().any(|line| {
        line.split_once('/')
            .map(|(_, remote_branch)| remote_branch == branch_name)
            .unwrap_or(false)
    });

    Ok(exists)
}

fn unique_worktree_path(repo_path: &Path, project_name: &str, slug: &str) -> PathBuf {
    let base_name = slugify_task_name(project_name);
    let parent = repo_path.parent().unwrap_or(repo_path);

    for suffix in 0..1000 {
        let directory_name = if suffix == 0 {
            format!("{base_name}-{slug}")
        } else {
            format!("{base_name}-{slug}-{}", suffix + 1)
        };
        let candidate = parent.join(directory_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!("{base_name}-{slug}-overflow"))
}

/// Get diff stats (lines added, lines removed) for the branch.
/// Compares the working tree + staged changes against the branch tip.
fn git_diff_stat(path: &Path, _branch: &str) -> (i32, i32) {
    // `git diff --stat` of working tree against HEAD.
    if let Ok(out) = git_command(path)
        .args(["diff", "--shortstat", "HEAD"])
        .output()
    {
        if out.status.success() {
            return parse_shortstat(&String::from_utf8_lossy(&out.stdout));
        }
    }
    (0, 0)
}

/// Parse the output of `git diff --shortstat`, e.g.
/// " 3 files changed, 85 insertions(+), 91 deletions(-)"
fn parse_shortstat(s: &str) -> (i32, i32) {
    let mut added = 0i32;
    let mut removed = 0i32;
    for part in s.split(',') {
        let part = part.trim();
        if part.contains("insertion") {
            if let Some(n) = part.split_whitespace().next().and_then(|w| w.parse().ok()) {
                added = n;
            }
        } else if part.contains("deletion") {
            if let Some(n) = part.split_whitespace().next().and_then(|w| w.parse().ok()) {
                removed = n;
            }
        }
    }
    (added, removed)
}

/// Get the relative date of the last commit on a branch.
fn git_last_commit_relative(path: &Path, branch: &str) -> String {
    git_stdout(path, &["log", "-1", "--format=%cr", branch]).unwrap_or_default()
}

fn git_ahead_count(path: &Path, branch: &str) -> usize {
    let upstream = format!("{branch}@{{upstream}}");
    if let Ok(out) = git_command(path)
        .args(["rev-list", "--count", &format!("{upstream}..{branch}")])
        .output()
    {
        if out.status.success() {
            return String::from_utf8_lossy(&out.stdout)
                .trim()
                .parse()
                .unwrap_or(0);
        }
    }

    0
}

fn parse_status_line(line: &str) -> Option<(char, char, Option<String>, String)> {
    let mut chars = line.chars();
    let index_status = chars.next()?;
    let worktree_status = chars.next()?;
    let raw_path = line.get(3..)?.trim();

    if raw_path.is_empty() {
        return None;
    }

    let (original_path, changed_path) = if let Some((from, to)) = raw_path.split_once(" -> ") {
        (Some(from.to_string()), to.to_string())
    } else {
        (None, raw_path.to_string())
    };

    Some((index_status, worktree_status, original_path, changed_path))
}

fn git_numstat_by_path(path: &Path, staged: bool) -> HashMap<String, (i32, i32)> {
    let mut stats = HashMap::new();
    let mut cmd = git_command(path);
    cmd.arg("diff");
    if staged {
        cmd.arg("--cached");
    }
    let Ok(output) = cmd.args(["--numstat", "--no-renames"]).output() else {
        return stats;
    };

    if !output.status.success() {
        return stats;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    for line in text.lines() {
        let mut parts = line.splitn(3, '\t');
        let Some(additions) = parts.next() else {
            continue;
        };
        let Some(deletions) = parts.next() else {
            continue;
        };
        let Some(changed_path) = parts.next() else {
            continue;
        };

        let additions = additions.parse::<i32>().unwrap_or(0);
        let deletions = deletions.parse::<i32>().unwrap_or(0);
        stats.insert(changed_path.to_string(), (additions, deletions));
    }

    stats
}

fn count_file_lines(path: &Path) -> i32 {
    let Ok(bytes) = std::fs::read(path) else {
        return 0;
    };
    if bytes.is_empty() {
        return 0;
    }

    let newline_count = bytes.iter().filter(|&&b| b == b'\n').count() as i32;
    if matches!(bytes.last(), Some(b'\n')) {
        newline_count
    } else {
        newline_count + 1
    }
}

fn remove_untracked_path(path: &Path) -> bool {
    let Ok(metadata) = std::fs::symlink_metadata(path) else {
        return true;
    };

    if metadata.file_type().is_dir() {
        std::fs::remove_dir_all(path).is_ok()
    } else {
        std::fs::remove_file(path).is_ok()
    }
}

/// Detect if the given path is a git worktree (not the main working tree).
/// Returns the worktree name if so.
pub fn detect_worktree_name(path: &Path) -> Option<String> {
    let git_dir = git_stdout(path, &["rev-parse", "--git-dir"])?;
    // A worktree's git-dir looks like: /path/to/main/.git/worktrees/<name>
    if let Some(pos) = git_dir.find(".git/worktrees/") {
        let after = &git_dir[pos + ".git/worktrees/".len()..];
        let name = after.split('/').next().unwrap_or(after);
        if !name.is_empty() {
            return Some(name.to_string());
        }
    }
    None
}

fn detect_repo_common_dir(path: &Path) -> Option<PathBuf> {
    let candidate = PathBuf::from(git_stdout(path, &["rev-parse", "--git-common-dir"])?);
    let absolute = if candidate.is_absolute() {
        candidate
    } else {
        path.join(candidate)
    };

    absolute.canonicalize().ok().or(Some(absolute))
}

#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};
    use std::path::PathBuf;

    use crate::agents::{
        AgentProviderKind, TerminalLaunchConfig, TerminalRestoreStatus, TerminalSessionKind,
        TerminalSessionRef,
    };

    use super::{
        format_git_command_error, PersistedSectionState, PersistedTerminalTab, Project,
        ProjectCheckoutState, ProjectKind, RepoRecord, StoreFile, Task, TaskKind,
    };

    fn sample_project(id: &str, worktree_name: Option<&str>) -> Project {
        Project {
            id: id.to_string(),
            repo_id: "repo".to_string(),
            name: format!("Project {id}"),
            path: PathBuf::from(format!("/tmp/{id}")),
            kind: if worktree_name.is_some() {
                ProjectKind::Worktree
            } else {
                ProjectKind::Root
            },
            checkout: ProjectCheckoutState::default(),
            worktree_name: worktree_name.map(str::to_string),
            repo_common_dir: None,
        }
    }

    #[test]
    fn formats_index_lock_error_for_toasts() {
        let message = format_git_command_error(
            "Could not unstage staged changes",
            "Another git process seems to be running in this repository.\nfatal: Unable to create '/tmp/repo/.git/index.lock': File exists.",
            "",
        );

        assert_eq!(
            message,
            "Could not unstage staged changes. Another git process or a stale `.git/index.lock` file is blocking this repository. Finish the other git command or remove the lock file, then try again."
        );
    }

    #[test]
    fn prefers_command_detail_when_no_special_case_matches() {
        let message = format_git_command_error(
            "Could not unstage staged changes",
            "fatal: not a git repository",
            "",
        );

        assert_eq!(
            message,
            "Could not unstage staged changes. fatal: not a git repository"
        );
    }

    #[test]
    fn store_file_round_trip_preserves_task_state() {
        let store = StoreFile {
            version: super::STORE_VERSION,
            repos: HashMap::from([(
                "repo".to_string(),
                RepoRecord {
                    id: "repo".to_string(),
                    common_dir: Some(PathBuf::from("/tmp/root/.git")),
                    branch_order: vec![
                        "feature/persist-tasks".to_string(),
                        "feature/worktree".to_string(),
                    ],
                    branches_by_name: HashMap::new(),
                },
            )]),
            projects: HashMap::from([
                ("root".to_string(), sample_project("root", None)),
                ("wt1".to_string(), sample_project("wt1", Some("wt1"))),
            ]),
            project_order: vec!["root".to_string(), "wt1".to_string()],
            tasks: HashMap::from([
                (
                    "task-1".to_string(),
                    Task {
                        id: "task-1".to_string(),
                        name: "Investigate bug".to_string(),
                        kind: TaskKind::Direct,
                        root_project_id: "root".to_string(),
                        target_project_id: "root".to_string(),
                        branch_name: "feature/persist-tasks".to_string(),
                        section_id: "root::feature/persist-tasks::task-1".to_string(),
                        worktree_project_id: None,
                        tabs: vec![
                            PersistedTerminalTab {
                                id: "0".to_string(),
                                title: "Terminal".to_string(),
                                provider: None,
                                launch_config: Some(TerminalLaunchConfig::default()),
                                restore_status: TerminalRestoreStatus::NotStarted,
                            },
                            PersistedTerminalTab {
                                id: "1".to_string(),
                                title: "Claude Code".to_string(),
                                provider: Some(AgentProviderKind::ClaudeCode),
                                launch_config: Some(
                                    TerminalLaunchConfig::for_provider(
                                        AgentProviderKind::ClaudeCode,
                                    )
                                    .with_session(Some(
                                        TerminalSessionRef {
                                            kind: TerminalSessionKind::ClaudeSession,
                                            id: "session-123".to_string(),
                                        },
                                    )),
                                ),
                                restore_status: TerminalRestoreStatus::Ready,
                            },
                        ],
                        active_tab_id: "1".to_string(),
                        next_tab_id: 3,
                        cwd: Some(PathBuf::from("/tmp/root")),
                    },
                ),
                (
                    "task-2".to_string(),
                    Task {
                        id: "task-2".to_string(),
                        name: "Friendly worktree name".to_string(),
                        kind: TaskKind::Worktree,
                        root_project_id: "root".to_string(),
                        target_project_id: "wt1".to_string(),
                        branch_name: "feature/worktree".to_string(),
                        section_id: "wt1::feature/worktree::task-2".to_string(),
                        worktree_project_id: Some("wt1".to_string()),
                        tabs: vec![PersistedTerminalTab {
                            id: "0".to_string(),
                            title: "Pi".to_string(),
                            provider: Some(AgentProviderKind::Pi),
                            launch_config: Some(
                                TerminalLaunchConfig::for_provider(AgentProviderKind::Pi)
                                    .with_session(Some(TerminalSessionRef {
                                        kind: TerminalSessionKind::PiSession,
                                        id: "pi-session".to_string(),
                                    })),
                            ),
                            restore_status: TerminalRestoreStatus::Launching,
                        }],
                        active_tab_id: "0".to_string(),
                        next_tab_id: 1,
                        cwd: Some(PathBuf::from("/tmp/wt1")),
                    },
                ),
            ]),
            task_ids_by_root_project: HashMap::from([(
                "root".to_string(),
                vec!["task-1".to_string(), "task-2".to_string()],
            )]),
            sections: HashMap::from([
                (
                    "root::feature/persist-tasks::task-1".to_string(),
                    PersistedSectionState {
                        active_tab_id: "1".to_string(),
                        next_tab_id: 3,
                        cwd: Some(PathBuf::from("/tmp/root")),
                        tabs: vec![
                            PersistedTerminalTab {
                                id: "0".to_string(),
                                title: "Terminal".to_string(),
                                provider: None,
                                launch_config: Some(TerminalLaunchConfig::default()),
                                restore_status: TerminalRestoreStatus::NotStarted,
                            },
                            PersistedTerminalTab {
                                id: "1".to_string(),
                                title: "Claude Code".to_string(),
                                provider: Some(AgentProviderKind::ClaudeCode),
                                launch_config: Some(
                                    TerminalLaunchConfig::for_provider(
                                        AgentProviderKind::ClaudeCode,
                                    )
                                    .with_session(Some(
                                        TerminalSessionRef {
                                            kind: TerminalSessionKind::ClaudeSession,
                                            id: "session-123".to_string(),
                                        },
                                    )),
                                ),
                                restore_status: TerminalRestoreStatus::Ready,
                            },
                        ],
                    },
                ),
                (
                    "wt1::feature/worktree::task-2".to_string(),
                    PersistedSectionState {
                        active_tab_id: "0".to_string(),
                        next_tab_id: 1,
                        cwd: Some(PathBuf::from("/tmp/wt1")),
                        tabs: vec![PersistedTerminalTab {
                            id: "0".to_string(),
                            title: "Pi".to_string(),
                            provider: Some(AgentProviderKind::Pi),
                            launch_config: Some(
                                TerminalLaunchConfig::for_provider(AgentProviderKind::Pi)
                                    .with_session(Some(TerminalSessionRef {
                                        kind: TerminalSessionKind::PiSession,
                                        id: "pi-session".to_string(),
                                    })),
                            ),
                            restore_status: TerminalRestoreStatus::Launching,
                        }],
                    },
                ),
            ]),
            ui: super::UiState {
                left_sidebar_open: false,
                expanded_repo_ids: HashSet::from(["repo".to_string()]),
                pinned_task_ids: HashSet::from(["task-1".to_string(), "task-2".to_string()]),
                last_active_section_id: None,
            },
        };

        let json = serde_json::to_string(&store).expect("store JSON should serialize");
        let round_trip: StoreFile =
            serde_json::from_str(&json).expect("store JSON should deserialize");

        assert_eq!(round_trip.projects.len(), 2);
        let task_1 = round_trip
            .tasks
            .get("task-1")
            .expect("task 1 should round-trip");
        let task_2 = round_trip
            .tasks
            .get("task-2")
            .expect("task 2 should round-trip");
        assert_eq!(task_1.name, "Investigate bug");
        assert_eq!(task_1.kind, TaskKind::Direct);
        assert_eq!(task_2.name, "Friendly worktree name");
        assert_eq!(task_2.kind, TaskKind::Worktree);
        assert_eq!(task_2.target_project_id, "wt1");
        assert_eq!(
            round_trip.sections["wt1::feature/worktree::task-2"].tabs[0].provider,
            Some(AgentProviderKind::Pi)
        );
        assert_eq!(
            round_trip.sections["root::feature/persist-tasks::task-1"].tabs[1]
                .launch_config
                .as_ref()
                .and_then(|config| config.session.as_ref())
                .map(|session| session.id.as_str()),
            Some("session-123")
        );
        assert_eq!(
            round_trip.ui.expanded_repo_ids,
            HashSet::from(["repo".to_string()])
        );
        assert_eq!(
            round_trip.ui.pinned_task_ids,
            HashSet::from(["task-1".to_string(), "task-2".to_string()])
        );
    }
}
