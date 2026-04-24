//! Persistent project store.
//!
//! Projects are saved as JSON in `~/.config/another-one/projects.json`.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::agents::{
    effective_enabled_agents, AgentProviderKind, TerminalLaunchConfig, TerminalRestoreStatus,
    DEFAULT_AGENT_ID,
};
use crate::git_actions::{default_commit_generation_script, default_pr_generation_script};
use crate::open_in::{effective_enabled_open_in_apps, OpenInAppKind};
use crate::shortcuts::{ShortcutAction, ShortcutSettings};

const STORE_VERSION: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RepoDefaultCommitAction {
    Commit,
    CommitAndPush,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepoBranchRecord {
    pub name: String,
    #[serde(default)]
    pub last_commit_relative: String,
    #[serde(default)]
    pub is_default: bool,
    #[serde(default)]
    pub ahead_count: usize,
    #[serde(default)]
    pub behind_count: usize,
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

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectBranchSettings {
    #[serde(default)]
    pub default_branch: Option<String>,
    #[serde(default)]
    pub default_target_branch: Option<String>,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionIcon {
    #[default]
    Play,
    Test,
    Lint,
    Configure,
    Build,
    Debug,
    Agent,
}

impl ProjectActionIcon {
    pub fn icon_path(self) -> &'static str {
        match self {
            Self::Play => "assets/icons/action__play.svg",
            Self::Test => "assets/icons/action__test.svg",
            Self::Lint => "assets/icons/action__lint.svg",
            Self::Configure => "assets/icons/action__configure.svg",
            Self::Build => "assets/icons/action__build.svg",
            Self::Debug => "assets/icons/action__debug.svg",
            Self::Agent => "assets/icons/action__agent.svg",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Play => "Play",
            Self::Test => "Test",
            Self::Lint => "Lint",
            Self::Configure => "Configure",
            Self::Build => "Build",
            Self::Debug => "Debug",
            Self::Agent => "Agent",
        }
    }

    pub const ALL: [Self; 7] = [
        Self::Play,
        Self::Test,
        Self::Lint,
        Self::Configure,
        Self::Build,
        Self::Debug,
        Self::Agent,
    ];
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionScope {
    #[default]
    Project,
    Global,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionAccess {
    #[default]
    Default,
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

impl ProjectActionAccess {
    pub fn label(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::ReadOnly => "Read only",
            Self::WorkspaceWrite => "Workspace write",
            Self::FullAccess => "Full access",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ProjectActionKind {
    Shell {
        command: String,
    },
    Agent {
        prompt: String,
        provider: AgentProviderKind,
        #[serde(default)]
        model: Option<String>,
        #[serde(default)]
        traits: Option<String>,
        #[serde(default)]
        mode: Option<String>,
        #[serde(default)]
        access: ProjectActionAccess,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectAction {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub icon: ProjectActionIcon,
    #[serde(default)]
    pub run_on_worktree_create: bool,
    #[serde(default)]
    pub scope: ProjectActionScope,
    pub kind: ProjectActionKind,
}

impl ProjectAction {
    pub fn display_name(&self) -> &str {
        let name = self.name.trim();
        if name.is_empty() {
            match self.kind {
                ProjectActionKind::Shell { .. } => "Shell action",
                ProjectActionKind::Agent { .. } => "Agent action",
            }
        } else {
            name
        }
    }
}

/// A git branch with optional diff stats resolved for a specific project/worktree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Branch {
    pub name: String,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub ahead_count: usize,
    pub behind_count: usize,
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
    #[serde(default)]
    pub branch_settings: ProjectBranchSettings,
    #[serde(default)]
    pub actions: Vec<ProjectAction>,
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
    pub behind_count: usize,
    pub metadata: Option<ProjectGitMetadata>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectBranchSettingField {
    DefaultBranch,
    DefaultTargetBranch,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedProjectBranchSettings {
    pub root_project_id: String,
    pub available_branches: Vec<String>,
    pub configured_default_branch: Option<String>,
    pub effective_default_branch: Option<String>,
    pub configured_default_target_branch: Option<String>,
    pub effective_default_target_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidProjectBranchSetting {
    pub root_project_id: String,
    pub field: ProjectBranchSettingField,
    pub branch_name: String,
    pub fallback_branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCompareFile {
    pub path: String,
    pub original_path: Option<String>,
    pub status: char,
    pub additions: i32,
    pub deletions: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBranchCompareState {
    pub current_branch: Option<String>,
    pub target_branch: String,
    pub files: Vec<BranchCompareFile>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BranchCommit {
    pub id: String,
    pub short_id: String,
    pub subject: String,
    pub author_name: String,
    pub authored_relative: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectBranchCommitState {
    pub current_branch: Option<String>,
    pub requested_limit: usize,
    pub has_more: bool,
    pub commits: Vec<BranchCommit>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectCommitFileChanges {
    pub commit_id: String,
    pub files: Vec<BranchCompareFile>,
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TaskWorktreeBranchMode {
    NewBranchFrom { source_branch: String },
    ExistingBranch { branch: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CreateBranchMode {
    CurrentTask,
    Worktree { migrate_changes: bool },
}

#[derive(Debug, Clone)]
pub struct CreatedBranch {
    pub path: PathBuf,
    pub branch_name: String,
    pub task_name: String,
    pub migration_stash: Option<String>,
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
    pub pinned: bool,
    #[serde(default)]
    pub fixed_title: Option<String>,
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
    pub repo_default_commit_actions: HashMap<String, RepoDefaultCommitAction>,
    #[serde(default)]
    pub pinned_task_ids: HashSet<String>,
    #[serde(default)]
    pub last_active_section_id: Option<String>,
    #[serde(default)]
    pub enabled_open_in_apps: Option<HashSet<OpenInAppKind>>,
    #[serde(default)]
    pub preferred_open_in_app: Option<OpenInAppKind>,
    #[serde(default)]
    pub enabled_agents: Option<HashSet<String>>,
    #[serde(default)]
    pub default_agent_id: Option<String>,
    #[serde(default)]
    pub agent_launch_args: HashMap<String, Vec<String>>,
    #[serde(default)]
    pub git_commit_generation_script: Option<String>,
    #[serde(default)]
    pub git_pr_generation_script: Option<String>,
    #[serde(default)]
    pub shortcuts: ShortcutSettings,
    #[serde(default)]
    pub global_actions: Vec<ProjectAction>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            left_sidebar_open: default_left_sidebar_open(),
            expanded_repo_ids: HashSet::new(),
            repo_default_commit_actions: HashMap::new(),
            pinned_task_ids: HashSet::new(),
            last_active_section_id: None,
            enabled_open_in_apps: None,
            preferred_open_in_app: None,
            enabled_agents: None,
            default_agent_id: None,
            agent_launch_args: HashMap::new(),
            git_commit_generation_script: None,
            git_pr_generation_script: None,
            shortcuts: ShortcutSettings::default(),
            global_actions: Vec::new(),
        }
    }
}

fn default_left_sidebar_open() -> bool {
    true
}

pub fn project_action_agent_launch_args(action: &ProjectAction) -> Result<Vec<String>, String> {
    let ProjectActionKind::Agent {
        prompt,
        provider,
        model,
        traits,
        mode,
        access,
    } = &action.kind
    else {
        return Ok(Vec::new());
    };

    let mut args = Vec::new();
    let trimmed_model = model.as_deref().unwrap_or_default().trim();
    let trimmed_traits = traits.as_deref().unwrap_or_default().trim();
    let trimmed_mode = mode.as_deref().unwrap_or_default().trim();
    let mut effective_prompt = prompt.trim().to_string();
    if trimmed_mode == "plan" && !effective_prompt.is_empty() {
        effective_prompt = format!(
            "{}\n{}",
            [
                "You are in plan mode.",
                "Analyze the codebase and return a concrete implementation plan only.",
                "Do not modify files or execute mutating commands.",
                "",
                "User request:",
            ]
            .join("\n"),
            effective_prompt
        );
    }

    match provider {
        AgentProviderKind::Codex => {
            if !trimmed_model.is_empty() {
                args.extend(["--model".to_string(), trimmed_model.to_string()]);
            }
            if trimmed_mode == "plan" {
                args.extend(["--sandbox".to_string(), "read-only".to_string()]);
                args.extend(["--ask-for-approval".to_string(), "on-request".to_string()]);
            } else {
                match access {
                    ProjectActionAccess::Default => {}
                    ProjectActionAccess::ReadOnly => {
                        args.extend(["--sandbox".to_string(), "read-only".to_string()]);
                        args.extend(["--ask-for-approval".to_string(), "on-request".to_string()]);
                    }
                    ProjectActionAccess::WorkspaceWrite => {
                        args.extend(["--sandbox".to_string(), "workspace-write".to_string()]);
                        args.extend(["--ask-for-approval".to_string(), "on-request".to_string()]);
                    }
                    ProjectActionAccess::FullAccess => {
                        args.push("--dangerously-bypass-approvals-and-sandbox".to_string());
                    }
                }
            }
            if !trimmed_traits.is_empty() {
                args.extend([
                    "--config".to_string(),
                    format!("model_reasoning_effort=\"{trimmed_traits}\""),
                ]);
            }
            if !trimmed_mode.is_empty() && trimmed_mode != "default" && trimmed_mode != "plan" {
                return Err(format!("Unsupported Codex action mode: {trimmed_mode}."));
            }
        }
        AgentProviderKind::ClaudeCode => {
            if !trimmed_model.is_empty() {
                args.extend(["--model".to_string(), trimmed_model.to_string()]);
            }
            if !trimmed_traits.is_empty() {
                args.extend(["--effort".to_string(), trimmed_traits.to_string()]);
            }
            if trimmed_mode == "plan" {
                args.extend(["--permission-mode".to_string(), "plan".to_string()]);
            } else {
                match access {
                    ProjectActionAccess::Default => {}
                    ProjectActionAccess::ReadOnly => {
                        args.extend(["--permission-mode".to_string(), "plan".to_string()]);
                    }
                    ProjectActionAccess::WorkspaceWrite => {
                        args.extend(["--permission-mode".to_string(), "default".to_string()]);
                    }
                    ProjectActionAccess::FullAccess => {
                        args.extend([
                            "--permission-mode".to_string(),
                            "bypassPermissions".to_string(),
                        ]);
                    }
                }
            }
            if !trimmed_mode.is_empty() && trimmed_mode != "default" && trimmed_mode != "plan" {
                return Err(format!("Unsupported Claude action mode: {trimmed_mode}."));
            }
        }
        provider => {
            if !trimmed_model.is_empty()
                || !trimmed_traits.is_empty()
                || !trimmed_mode.is_empty()
                || *access != ProjectActionAccess::Default
            {
                return Err(format!(
                    "{} actions do not support custom model, traits, mode, or access options yet.",
                    provider.label()
                ));
            }
        }
    }

    if !effective_prompt.is_empty() {
        args.push(effective_prompt);
    }

    Ok(args)
}

fn upsert_action(actions: &mut Vec<ProjectAction>, action: ProjectAction) {
    if let Some(existing) = actions.iter_mut().find(|existing| existing.id == action.id) {
        *existing = action;
    } else {
        actions.push(action);
    }
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
        ui.repo_default_commit_actions
            .retain(|repo_id, _| store.repos.contains_key(repo_id));
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

    pub fn root_project_id_for_project(&self, project_id: &str) -> Option<String> {
        let project = self.project(project_id)?;

        self.projects_by_id
            .values()
            .find(|candidate| {
                candidate.repo_id == project.repo_id && candidate.kind == ProjectKind::Root
            })
            .map(|project| project.id.clone())
            .or_else(|| Some(project.id.clone()))
    }

    pub fn root_project_for_project(&self, project_id: &str) -> Option<&Project> {
        let root_project_id = self.root_project_id_for_project(project_id)?;
        self.project(&root_project_id)
    }

    pub fn branch_names(&self, project_id: &str) -> Vec<String> {
        self.repo_for_project(project_id)
            .map(|repo| repo.branch_order.clone())
            .unwrap_or_default()
    }

    fn automatic_primary_branch_name(
        &self,
        project_id: &str,
        prefer_default: bool,
    ) -> Option<String> {
        let repo = self.repo_for_project(project_id)?;

        if prefer_default {
            repo.branch_order
                .iter()
                .find(|name| {
                    repo.branches_by_name
                        .get(name.as_str())
                        .is_some_and(|branch| branch.is_default)
                })
                .cloned()
                .or_else(|| self.current_branch_name(project_id))
                .or_else(|| repo.branch_order.first().cloned())
        } else {
            self.current_branch_name(project_id)
                .or_else(|| repo.branch_order.first().cloned())
        }
    }

    pub fn resolved_branch_settings(
        &self,
        project_id: &str,
    ) -> Option<ResolvedProjectBranchSettings> {
        let root_project = self.root_project_for_project(project_id)?;
        let available_branches = self.branch_names(&root_project.id);
        let configured_default_branch = root_project.branch_settings.default_branch.clone();
        let configured_default_target_branch =
            root_project.branch_settings.default_target_branch.clone();

        let effective_default_branch = configured_default_branch
            .as_ref()
            .filter(|branch| {
                available_branches
                    .iter()
                    .any(|candidate| candidate == *branch)
            })
            .cloned()
            .or_else(|| self.automatic_primary_branch_name(project_id, true));
        let effective_default_target_branch = configured_default_target_branch
            .as_ref()
            .filter(|branch| {
                available_branches
                    .iter()
                    .any(|candidate| candidate == *branch)
            })
            .cloned();

        Some(ResolvedProjectBranchSettings {
            root_project_id: root_project.id.clone(),
            available_branches,
            configured_default_branch,
            effective_default_branch,
            configured_default_target_branch,
            effective_default_target_branch,
        })
    }

    fn update_branch_setting(
        &mut self,
        project_id: &str,
        field: ProjectBranchSettingField,
        branch_name: Option<String>,
    ) -> Result<bool, String> {
        let root_project_id = self
            .root_project_id_for_project(project_id)
            .ok_or_else(|| "Could not find the project group for this repository.".to_string())?;
        let available_branches = self.branch_names(&root_project_id);
        if let Some(branch_name) = branch_name.as_deref() {
            if !available_branches
                .iter()
                .any(|branch| branch == branch_name)
            {
                return Err(format!(
                    "The selected branch `{branch_name}` is not available in this repository."
                ));
            }
        }

        let Some(project) = self.project_mut(&root_project_id) else {
            return Err("Could not find the root project for this repository.".to_string());
        };

        let slot = match field {
            ProjectBranchSettingField::DefaultBranch => &mut project.branch_settings.default_branch,
            ProjectBranchSettingField::DefaultTargetBranch => {
                &mut project.branch_settings.default_target_branch
            }
        };

        if *slot == branch_name {
            return Ok(false);
        }

        *slot = branch_name;
        self.refresh_runtime_views();
        self.save();
        Ok(true)
    }

    pub fn update_default_branch(
        &mut self,
        project_id: &str,
        branch_name: Option<String>,
    ) -> Result<bool, String> {
        self.update_branch_setting(
            project_id,
            ProjectBranchSettingField::DefaultBranch,
            branch_name,
        )
    }

    pub fn update_default_target_branch(
        &mut self,
        project_id: &str,
        branch_name: Option<String>,
    ) -> Result<bool, String> {
        self.update_branch_setting(
            project_id,
            ProjectBranchSettingField::DefaultTargetBranch,
            branch_name,
        )
    }

    pub fn clear_missing_branch_settings(
        &mut self,
        project_id: &str,
    ) -> Vec<InvalidProjectBranchSetting> {
        let Some(root_project_id) = self.root_project_id_for_project(project_id) else {
            return Vec::new();
        };
        let available_branches = self.branch_names(&root_project_id);
        let default_branch_fallback = self.automatic_primary_branch_name(&root_project_id, true);
        let mut invalid = Vec::new();
        let mut changed = false;

        let Some(project) = self.project_mut(&root_project_id) else {
            return Vec::new();
        };

        if let Some(branch_name) = project.branch_settings.default_branch.clone() {
            if !available_branches
                .iter()
                .any(|branch| branch == &branch_name)
            {
                project.branch_settings.default_branch = None;
                invalid.push(InvalidProjectBranchSetting {
                    root_project_id: root_project_id.clone(),
                    field: ProjectBranchSettingField::DefaultBranch,
                    branch_name,
                    fallback_branch: default_branch_fallback.clone(),
                });
                changed = true;
            }
        }

        if let Some(branch_name) = project.branch_settings.default_target_branch.clone() {
            if !available_branches
                .iter()
                .any(|branch| branch == &branch_name)
            {
                project.branch_settings.default_target_branch = None;
                invalid.push(InvalidProjectBranchSetting {
                    root_project_id: root_project_id.clone(),
                    field: ProjectBranchSettingField::DefaultTargetBranch,
                    branch_name,
                    fallback_branch: None,
                });
                changed = true;
            }
        }

        if changed {
            self.refresh_runtime_views();
            self.save();
        }

        invalid
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
            behind_count: branch.behind_count,
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
        let branch_name = if prefer_default {
            self.resolved_branch_settings(project_id)
                .and_then(|settings| settings.effective_default_branch)
                .or_else(|| self.automatic_primary_branch_name(project_id, true))
        } else {
            self.automatic_primary_branch_name(project_id, false)
        }?;

        self.branch_view(project_id, &branch_name)
    }

    pub fn current_branch_name(&self, project_id: &str) -> Option<String> {
        self.project(project_id)
            .and_then(|project| project.checkout.current_branch.clone())
    }

    pub fn repo_default_commit_action(&self, repo_id: &str) -> Option<RepoDefaultCommitAction> {
        self.ui.repo_default_commit_actions.get(repo_id).copied()
    }

    pub fn project_actions(&self, project_id: &str) -> Vec<ProjectAction> {
        let Some(root_project_id) = self.root_project_id_for_project(project_id) else {
            return self.ui.global_actions.clone();
        };

        let global_action_ids: HashSet<&str> = self
            .ui
            .global_actions
            .iter()
            .map(|action| action.id.as_str())
            .collect();
        let mut actions = self
            .project(&root_project_id)
            .map(|project| project.actions.clone())
            .unwrap_or_default();
        actions.retain(|action| !global_action_ids.contains(action.id.as_str()));
        actions.extend(self.ui.global_actions.clone());
        actions
    }

    pub fn upsert_project_action(
        &mut self,
        project_id: &str,
        mut action: ProjectAction,
        save_global_copy: bool,
    ) -> Result<(), String> {
        let root_project_id = self
            .root_project_id_for_project(project_id)
            .ok_or_else(|| "Could not find the project group for this repository.".to_string())?;
        if save_global_copy {
            if let Some(project) = self.project_mut(&root_project_id) {
                project.actions.retain(|existing| existing.id != action.id);
            } else {
                return Err("Could not find the root project for this repository.".to_string());
            }

            action.scope = ProjectActionScope::Global;
            upsert_action(&mut self.ui.global_actions, action);
        } else {
            self.ui
                .global_actions
                .retain(|existing| existing.id != action.id);

            action.scope = ProjectActionScope::Project;
            let Some(project) = self.project_mut(&root_project_id) else {
                return Err("Could not find the root project for this repository.".to_string());
            };
            upsert_action(&mut project.actions, action);
        }
        self.refresh_runtime_views();
        self.save();
        Ok(())
    }

    pub fn upsert_global_action(&mut self, mut action: ProjectAction) {
        action.scope = ProjectActionScope::Global;
        upsert_action(&mut self.ui.global_actions, action);
        self.save();
    }

    pub fn delete_project_action(&mut self, project_id: &str, action_id: &str) -> bool {
        let Some(root_project_id) = self.root_project_id_for_project(project_id) else {
            return false;
        };
        let mut removed = false;
        if let Some(project) = self.project_mut(&root_project_id) {
            let before = project.actions.len();
            project.actions.retain(|action| action.id != action_id);
            removed = project.actions.len() != before;
        }
        let before_global = self.ui.global_actions.len();
        self.ui
            .global_actions
            .retain(|action| action.id != action_id);
        removed |= self.ui.global_actions.len() != before_global;
        if removed {
            self.refresh_runtime_views();
            self.save();
        }
        removed
    }

    pub fn automatic_project_actions(&self, project_id: &str) -> Vec<ProjectAction> {
        self.project_actions(project_id)
            .into_iter()
            .filter(|action| action.run_on_worktree_create)
            .collect()
    }

    pub fn set_repo_default_commit_action(
        &mut self,
        repo_id: impl Into<String>,
        action: RepoDefaultCommitAction,
    ) {
        let repo_id = repo_id.into();
        if self.ui.repo_default_commit_actions.get(&repo_id) == Some(&action) {
            return;
        }

        self.ui.repo_default_commit_actions.insert(repo_id, action);
        self.save();
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
            self.ui.repo_default_commit_actions.remove(&repo_id);
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

    pub fn update_task_branch(
        &mut self,
        task_id: &str,
        target_project_id: &str,
        branch_name: &str,
    ) -> Option<(String, String)> {
        let task = self.tasks_by_id.get_mut(task_id)?;
        let old_section_id = task.section_id.clone();
        task.target_project_id = target_project_id.to_string();
        task.branch_name = branch_name.to_string();
        task.section_id =
            crate::section::SectionId::for_task(target_project_id, branch_name, task_id)
                .store_key();
        let new_section_id = task.section_id.clone();
        if let Some(section) = self.terminal_sections.remove(&old_section_id) {
            self.terminal_sections
                .insert(new_section_id.clone(), section);
        }
        if self.ui.last_active_section_id.as_deref() == Some(&old_section_id) {
            self.ui.last_active_section_id = Some(new_section_id.clone());
        }
        self.rebuild_runtime_views();
        self.save();
        Some((old_section_id, new_section_id))
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

    pub fn set_shortcut_binding(&mut self, action: ShortcutAction, binding: impl Into<String>) {
        self.ui.shortcuts.set_binding(action, binding);
        self.save();
    }

    pub fn agent_launch_args(&self, agent_id: &str) -> &[String] {
        self.ui
            .agent_launch_args
            .get(agent_id)
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    pub fn set_agent_launch_args(
        &mut self,
        agent_id: impl Into<String>,
        args: Vec<String>,
    ) -> bool {
        let agent_id = agent_id.into();
        if args.is_empty() {
            return self.remove_agent_launch_args(&agent_id);
        }

        if self.ui.agent_launch_args.get(&agent_id) == Some(&args) {
            return false;
        }

        self.ui.agent_launch_args.insert(agent_id, args);
        self.save();
        true
    }

    pub fn remove_agent_launch_args(&mut self, agent_id: &str) -> bool {
        let removed = self.ui.agent_launch_args.remove(agent_id).is_some();
        if removed {
            self.save();
        }
        removed
    }

    pub fn git_commit_generation_script(&self) -> &str {
        match self.ui.git_commit_generation_script.as_deref() {
            Some(script) if !script.trim().is_empty() => script,
            _ => default_commit_generation_script(),
        }
    }

    pub fn set_git_commit_generation_script(&mut self, script: impl Into<String>) -> bool {
        let script = script.into();
        let normalized = if script.trim().is_empty()
            || script.trim() == default_commit_generation_script().trim()
        {
            None
        } else {
            Some(script)
        };

        if self.ui.git_commit_generation_script == normalized {
            return false;
        }

        self.ui.git_commit_generation_script = normalized;
        self.save();
        true
    }

    pub fn reset_git_commit_generation_script(&mut self) -> bool {
        if self.ui.git_commit_generation_script.take().is_none() {
            return false;
        }

        self.save();
        true
    }

    pub fn git_pr_generation_script(&self) -> &str {
        match self.ui.git_pr_generation_script.as_deref() {
            Some(script) if !script.trim().is_empty() => script,
            _ => default_pr_generation_script(),
        }
    }

    pub fn set_git_pr_generation_script(&mut self, script: impl Into<String>) -> bool {
        let script = script.into();
        let normalized =
            if script.trim().is_empty() || script.trim() == default_pr_generation_script().trim() {
                None
            } else {
                Some(script)
            };

        if self.ui.git_pr_generation_script == normalized {
            return false;
        }

        self.ui.git_pr_generation_script = normalized;
        self.save();
        true
    }

    pub fn reset_git_pr_generation_script(&mut self) -> bool {
        if self.ui.git_pr_generation_script.take().is_none() {
            return false;
        }

        self.save();
        true
    }

    pub fn enabled_agent_ids(&self) -> Vec<&'static str> {
        effective_enabled_agents(self.ui.enabled_agents.as_ref())
            .into_iter()
            .map(|agent| agent.id)
            .collect()
    }

    pub fn default_agent_id(&self) -> Option<&'static str> {
        let enabled = self.enabled_agent_ids();

        self.ui
            .default_agent_id
            .as_deref()
            .and_then(|agent_id| {
                enabled
                    .iter()
                    .copied()
                    .find(|enabled_id| *enabled_id == agent_id)
            })
            .or_else(|| {
                enabled
                    .iter()
                    .copied()
                    .find(|agent_id| *agent_id == DEFAULT_AGENT_ID)
            })
            .or_else(|| enabled.first().copied())
    }

    pub fn agent_enabled(&self, agent_id: &str) -> bool {
        self.ui
            .enabled_agents
            .as_ref()
            .map_or(true, |enabled| enabled.contains(agent_id))
    }

    pub fn agent_is_default(&self, agent_id: &str) -> bool {
        self.default_agent_id() == Some(agent_id)
    }

    pub fn set_default_agent(&mut self, agent_id: &str) -> bool {
        if !self.agent_enabled(agent_id) {
            return false;
        }

        if self.ui.default_agent_id.as_deref() == Some(agent_id) {
            return false;
        }

        self.ui.default_agent_id = Some(agent_id.to_string());
        self.save();
        true
    }

    pub fn set_agent_enabled(&mut self, agent_id: &str, enabled: bool) -> bool {
        let mut configured = self.ui.enabled_agents.clone().unwrap_or_else(|| {
            self.enabled_agent_ids()
                .into_iter()
                .map(str::to_string)
                .collect()
        });

        let changed = if enabled {
            configured.insert(agent_id.to_string())
        } else {
            configured.remove(agent_id)
        };

        if !changed {
            return false;
        }

        self.ui.enabled_agents = Some(configured);
        self.ui.default_agent_id = self.default_agent_id().map(str::to_string);
        self.save();
        true
    }

    pub fn enabled_open_in_apps(&self, available: &[OpenInAppKind]) -> Vec<OpenInAppKind> {
        effective_enabled_open_in_apps(available, self.ui.enabled_open_in_apps.as_ref())
    }

    pub fn preferred_open_in_app(&self, available: &[OpenInAppKind]) -> Option<OpenInAppKind> {
        let enabled = self.enabled_open_in_apps(available);
        self.ui
            .preferred_open_in_app
            .filter(|app| enabled.contains(app))
            .or_else(|| enabled.first().copied())
    }

    pub fn open_in_app_enabled(&self, app: OpenInAppKind, available: &[OpenInAppKind]) -> bool {
        self.enabled_open_in_apps(available).contains(&app)
    }

    pub fn set_open_in_app_enabled(
        &mut self,
        app: OpenInAppKind,
        enabled: bool,
        available: &[OpenInAppKind],
    ) {
        let mut configured = self
            .ui
            .enabled_open_in_apps
            .clone()
            .unwrap_or_else(|| available.iter().copied().collect());

        if enabled {
            configured.insert(app);
        } else {
            configured.remove(&app);
        }

        self.ui.enabled_open_in_apps = Some(configured);
        if self.preferred_open_in_app(available).is_none() {
            self.ui.preferred_open_in_app = None;
        } else if !enabled && self.ui.preferred_open_in_app == Some(app) {
            self.ui.preferred_open_in_app = self.enabled_open_in_apps(available).first().copied();
        }
        self.save();
    }

    pub fn set_preferred_open_in_app(&mut self, app: OpenInAppKind, available: &[OpenInAppKind]) {
        if !self.open_in_app_enabled(app, available) {
            return;
        }

        self.ui.preferred_open_in_app = Some(app);
        self.save();
    }

    pub fn clear_shortcut_binding(&mut self, action: ShortcutAction) {
        self.ui.shortcuts.clear_binding(action);
        self.save();
    }

    pub fn reset_shortcut_binding(&mut self, action: ShortcutAction) {
        self.ui.shortcuts.reset_binding(action);
        self.save();
    }

    pub fn reset_shortcuts(&mut self) {
        self.ui.shortcuts.reset_all();
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
        self.ui
            .repo_default_commit_actions
            .retain(|repo_id, _| self.repos.contains_key(repo_id));

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
            branch_settings: ProjectBranchSettings::default(),
            actions: Vec::new(),
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
        behind_count: current_branch
            .as_deref()
            .map(|branch| git_behind_count(path, branch))
            .unwrap_or(0),
        current_branch,
        metadata: include_metadata.then(|| read_project_git_metadata(path)),
    }
}

pub fn read_project_branch_compare_state(
    path: &Path,
    target_branch: &str,
) -> Result<ProjectBranchCompareState, String> {
    let spec = format!("{target_branch}...HEAD");
    let name_status_output = git_command(path)
        .args(["diff", "--name-status", "-M", "-z", &spec])
        .output()
        .map_err(|error| {
            format!("Could not compare the current branch against {target_branch}: {error}")
        })?;
    if !name_status_output.status.success() {
        return Err(format_git_command_failure(
            &format!("Could not compare the current branch against {target_branch}"),
            &name_status_output,
        ));
    }

    let numstat_output = git_command(path)
        .args(["diff", "--numstat", "-M", "-z", &spec])
        .output()
        .map_err(|error| {
            format!("Could not inspect compare stats against {target_branch}: {error}")
        })?;
    if !numstat_output.status.success() {
        return Err(format_git_command_failure(
            &format!("Could not inspect compare stats against {target_branch}"),
            &numstat_output,
        ));
    }

    let mut stats_by_key = parse_branch_compare_numstat_entries(&numstat_output.stdout)
        .into_iter()
        .map(|entry| ((entry.path.clone(), entry.original_path.clone()), entry))
        .collect::<HashMap<_, _>>();

    let mut files = parse_branch_compare_name_status_entries(&name_status_output.stdout)
        .into_iter()
        .map(|entry| {
            let stats = stats_by_key.remove(&(entry.path.clone(), entry.original_path.clone()));
            BranchCompareFile {
                path: entry.path,
                original_path: entry.original_path,
                status: entry.status,
                additions: stats.as_ref().map_or(0, |entry| entry.additions),
                deletions: stats.as_ref().map_or(0, |entry| entry.deletions),
            }
        })
        .collect::<Vec<_>>();

    files.sort_by(|left, right| left.path.cmp(&right.path));

    Ok(ProjectBranchCompareState {
        current_branch: git_current_branch(path),
        target_branch: target_branch.to_string(),
        files,
    })
}

pub fn read_project_branch_commit_state(
    path: &Path,
    requested_limit: usize,
) -> Result<ProjectBranchCommitState, String> {
    let overfetch_limit = requested_limit.saturating_add(1);
    let output = git_command(path)
        .args(["log", "--format=%H%x00%h%x00%s%x00%an%x00%cr%x1e", "-n"])
        .arg(overfetch_limit.to_string())
        .arg("HEAD")
        .output()
        .map_err(|error| format!("Could not list recent commits: {error}"))?;
    if !output.status.success() {
        return Err(format_git_command_failure(
            "Could not list recent commits",
            &output,
        ));
    }

    let (commits, has_more) = parse_recent_branch_commit_page(&output.stdout, requested_limit);

    Ok(ProjectBranchCommitState {
        current_branch: git_current_branch(path),
        requested_limit,
        has_more,
        commits,
    })
}

pub fn read_project_commit_file_changes(
    path: &Path,
    commit_id: &str,
) -> Result<ProjectCommitFileChanges, String> {
    let name_status_output = git_command(path)
        .args([
            "show",
            "--format=",
            "--name-status",
            "-M",
            "-m",
            "-z",
            commit_id,
        ])
        .output()
        .map_err(|error| format!("Could not list file changes for commit {commit_id}: {error}"))?;
    if !name_status_output.status.success() {
        return Err(format_git_command_failure(
            &format!("Could not list file changes for commit {commit_id}"),
            &name_status_output,
        ));
    }

    let numstat_output = git_command(path)
        .args(["show", "--format=", "--numstat", "-M", "-z", commit_id])
        .output()
        .map_err(|error| format!("Could not inspect diff stats for commit {commit_id}: {error}"))?;
    if !numstat_output.status.success() {
        return Err(format_git_command_failure(
            &format!("Could not inspect diff stats for commit {commit_id}"),
            &numstat_output,
        ));
    }

    let files = combine_commit_file_changes(
        parse_branch_compare_name_status_entries(&name_status_output.stdout),
        parse_branch_compare_numstat_entries(&numstat_output.stdout),
    );

    Ok(ProjectCommitFileChanges {
        commit_id: commit_id.to_string(),
        files,
    })
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
                    behind_count: branch.behind_count,
                },
            )
        })
        .collect();
    (branch_order, branches_by_name)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchCompareNameStatusEntry {
    path: String,
    original_path: Option<String>,
    status: char,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct BranchCompareNumStatEntry {
    path: String,
    original_path: Option<String>,
    additions: i32,
    deletions: i32,
}

fn parse_branch_compare_name_status_entries(bytes: &[u8]) -> Vec<BranchCompareNameStatusEntry> {
    let fields = bytes
        .split(|byte| *byte == 0)
        .filter(|field| !field.is_empty())
        .collect::<Vec<_>>();
    let mut entries = Vec::new();
    let mut index = 0;

    while index < fields.len() {
        let status_field = String::from_utf8_lossy(fields[index]).to_string();
        index += 1;

        let Some(status) = status_field.chars().next() else {
            continue;
        };

        if matches!(status, 'R' | 'C') {
            let Some(original_path) = fields
                .get(index)
                .map(|field| String::from_utf8_lossy(field).to_string())
            else {
                break;
            };
            let Some(path) = fields
                .get(index + 1)
                .map(|field| String::from_utf8_lossy(field).to_string())
            else {
                break;
            };
            index += 2;
            entries.push(BranchCompareNameStatusEntry {
                path,
                original_path: Some(original_path),
                status,
            });
            continue;
        }

        let Some(path) = fields
            .get(index)
            .map(|field| String::from_utf8_lossy(field).to_string())
        else {
            break;
        };
        index += 1;
        entries.push(BranchCompareNameStatusEntry {
            path,
            original_path: None,
            status,
        });
    }

    entries
}

fn parse_branch_compare_numstat_entries(bytes: &[u8]) -> Vec<BranchCompareNumStatEntry> {
    let mut entries = Vec::new();
    let mut fields = bytes.split(|byte| *byte == 0).peekable();

    while let Some(header) = fields.next() {
        if header.is_empty() {
            continue;
        }

        let header = String::from_utf8_lossy(header);
        let mut parts = header.split('\t');
        let additions = parse_branch_compare_numstat_value(parts.next().unwrap_or("").as_bytes());
        let deletions = parse_branch_compare_numstat_value(parts.next().unwrap_or("").as_bytes());
        let path_field = parts.next().unwrap_or_default();

        let (path, original_path) = if path_field.is_empty() {
            let original_path = fields
                .find(|field| !field.is_empty())
                .map(|field| String::from_utf8_lossy(field).to_string());
            let path = fields
                .find(|field| !field.is_empty())
                .map(|field| String::from_utf8_lossy(field).to_string());
            let Some(path) = path else {
                break;
            };
            (path, original_path)
        } else {
            (path_field.to_string(), None)
        };

        entries.push(BranchCompareNumStatEntry {
            path,
            original_path,
            additions,
            deletions,
        });
    }

    entries
}

fn parse_branch_compare_numstat_value(field: &[u8]) -> i32 {
    let value = String::from_utf8_lossy(field);
    if value == "-" {
        0
    } else {
        value.parse::<i32>().unwrap_or(0)
    }
}

fn combine_commit_file_changes(
    name_status_entries: Vec<BranchCompareNameStatusEntry>,
    numstat_entries: Vec<BranchCompareNumStatEntry>,
) -> Vec<BranchCompareFile> {
    let mut stats_by_key = numstat_entries
        .into_iter()
        .map(|entry| ((entry.path.clone(), entry.original_path.clone()), entry))
        .collect::<HashMap<_, _>>();
    let mut seen = HashSet::new();
    let mut files = Vec::new();

    for entry in name_status_entries {
        if !seen.insert((
            entry.path.clone(),
            entry.original_path.clone(),
            entry.status,
        )) {
            continue;
        }

        let stats = stats_by_key.remove(&(entry.path.clone(), entry.original_path.clone()));
        files.push(BranchCompareFile {
            path: entry.path,
            original_path: entry.original_path,
            status: entry.status,
            additions: stats.as_ref().map_or(0, |entry| entry.additions),
            deletions: stats.as_ref().map_or(0, |entry| entry.deletions),
        });
    }

    files.sort_by(|left, right| left.path.cmp(&right.path));
    files
}

fn parse_branch_commit_entries(bytes: &[u8]) -> Vec<BranchCommit> {
    bytes
        .split(|byte| *byte == 0x1e)
        .filter_map(|record| {
            let record = String::from_utf8_lossy(record);
            let record = record.trim();
            if record.is_empty() {
                return None;
            }

            let mut fields = record.split('\0');
            let id = fields.next()?.to_string();
            let short_id = fields.next()?.to_string();
            let subject = fields.next()?.to_string();
            let author_name = fields.next()?.to_string();
            let authored_relative = fields.next()?.to_string();

            Some(BranchCommit {
                id,
                short_id,
                subject,
                author_name,
                authored_relative,
            })
        })
        .collect()
}

fn parse_recent_branch_commit_page(
    bytes: &[u8],
    requested_limit: usize,
) -> (Vec<BranchCommit>, bool) {
    let mut commits = parse_branch_commit_entries(bytes);
    let has_more = commits.len() > requested_limit;
    if has_more {
        commits.truncate(requested_limit);
    }
    (commits, has_more)
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
    _project_name: &str,
    requested_task_name: &str,
    fallback_task_name: &str,
    branch_mode: TaskWorktreeBranchMode,
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

    let slug = slugify_task_name(&task_name);
    let worktree_slug = format!("{slug}-wt");
    let worktree_path = unique_worktree_path(repo_path, &worktree_slug);
    let Some(worktree_parent) = worktree_path.parent() else {
        return Err("Failed to determine the worktree parent directory.".to_string());
    };

    std::fs::create_dir_all(worktree_parent)
        .map_err(|error| format!("Failed to prepare the worktree directory: {error}"))?;

    let (branch_name, output) = match branch_mode {
        TaskWorktreeBranchMode::NewBranchFrom { source_branch } => {
            let base_branch = if source_branch.trim().is_empty() {
                current_branch(repo_path)
                    .or_else(|| git_default_branch(repo_path))
                    .ok_or_else(|| {
                        "Could not determine a base branch for the new worktree.".to_string()
                    })?
            } else {
                source_branch.trim().to_string()
            };
            let branch_name = unique_branch_name(repo_path, &slug)?;
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
            (branch_name, output)
        }
        TaskWorktreeBranchMode::ExistingBranch { branch } => {
            let branch_name = if branch.trim().is_empty() {
                current_branch(repo_path)
                    .or_else(|| git_default_branch(repo_path))
                    .ok_or_else(|| {
                        "Could not determine a branch for the new worktree.".to_string()
                    })?
            } else {
                branch.trim().to_string()
            };
            let output = git_command(repo_path)
                .args([
                    "worktree",
                    "add",
                    worktree_path.to_string_lossy().as_ref(),
                    &branch_name,
                ])
                .output()
                .map_err(|error| format!("Failed to create worktree: {error}"))?;
            (branch_name, output)
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let stdout = String::from_utf8_lossy(&output.stdout);
        return Err(
            if let Some(path) = git_worktree_checked_out_path(&stderr, &stdout) {
                format!("Branch {branch_name} is already checked out in another worktree: {path}")
            } else {
                format_git_command_failure("Git failed to create the worktree", &output)
            },
        );
    }

    Ok(CreatedTaskWorktree {
        path: worktree_path,
        branch_name,
        task_name,
    })
}

fn git_worktree_checked_out_path(stderr: &str, stdout: &str) -> Option<String> {
    let combined = format!("{stderr}\n{stdout}");
    if !combined.contains("is already checked out") {
        return None;
    }

    let (_, rest) = combined.split_once("is already checked out at ")?;
    let path = rest
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .trim_matches('\'')
        .trim_matches('"');

    (!path.is_empty()).then(|| path.to_string())
}

pub fn slugify_branch_name(name: &str) -> String {
    slugify_task_name(name)
}

pub fn unique_branch_name_for_repo(repo_path: &Path, base_slug: &str) -> Result<String, String> {
    unique_branch_name(repo_path, base_slug)
}

pub fn create_branch_from_head(
    repo_path: &Path,
    requested_branch_name: &str,
    mode: CreateBranchMode,
) -> Result<CreatedBranch, String> {
    let slug = slugify_branch_name(requested_branch_name);
    let branch_name = unique_branch_name(repo_path, &slug)?;
    match mode {
        CreateBranchMode::CurrentTask => {
            let output = git_command(repo_path)
                .args(["switch", "-c", &branch_name])
                .output()
                .map_err(|error| format!("Failed to create branch: {error}"))?;
            if !output.status.success() {
                return Err(format_git_command_failure(
                    "Git failed to create the branch",
                    &output,
                ));
            }

            Ok(CreatedBranch {
                path: repo_path.to_path_buf(),
                branch_name: branch_name.clone(),
                task_name: branch_name,
                migration_stash: None,
            })
        }
        CreateBranchMode::Worktree { migrate_changes } => {
            let worktree_slug = format!("{branch_name}-wt");
            let worktree_path = unique_worktree_path(repo_path, &worktree_slug);
            let Some(worktree_parent) = worktree_path.parent() else {
                return Err("Failed to determine the worktree parent directory.".to_string());
            };
            std::fs::create_dir_all(worktree_parent)
                .map_err(|error| format!("Failed to prepare the worktree directory: {error}"))?;

            let stash_ref = if migrate_changes {
                create_branch_migration_stash(repo_path, &branch_name)?
            } else {
                None
            };

            let output = git_command(repo_path)
                .args([
                    "worktree",
                    "add",
                    "-b",
                    &branch_name,
                    worktree_path.to_string_lossy().as_ref(),
                    "HEAD",
                ])
                .output()
                .map_err(|error| format!("Failed to create worktree: {error}"))?;

            if !output.status.success() {
                return Err(format_git_command_failure(
                    "Git failed to create the worktree",
                    &output,
                ));
            }

            if let Some(stash_ref) = stash_ref.as_deref() {
                let apply = git_command(&worktree_path)
                    .args(["stash", "apply", "--index", stash_ref])
                    .output()
                    .map_err(|error| {
                        format!(
                            "Created the worktree, but failed to apply migrated changes from {stash_ref}: {error}"
                        )
                    })?;
                if !apply.status.success() {
                    return Err(format!(
                        "{}. The stash is still available as {stash_ref}.",
                        format_git_command_failure(
                            "Created the worktree, but Git failed to apply migrated changes",
                            &apply,
                        )
                    ));
                }

                let drop = git_command(repo_path)
                    .args(["stash", "drop", stash_ref])
                    .output()
                    .map_err(|error| {
                        format!("Applied migrated changes, but failed to drop {stash_ref}: {error}")
                    })?;
                if !drop.status.success() {
                    return Err(format_git_command_failure(
                        "Applied migrated changes, but Git failed to drop the migration stash",
                        &drop,
                    ));
                }
            }

            Ok(CreatedBranch {
                path: worktree_path,
                branch_name: branch_name.clone(),
                task_name: branch_name,
                migration_stash: stash_ref,
            })
        }
    }
}

fn create_branch_migration_stash(
    repo_path: &Path,
    branch_name: &str,
) -> Result<Option<String>, String> {
    let status = git_command(repo_path)
        .args(["status", "--porcelain=v1", "--untracked-files=all"])
        .output()
        .map_err(|error| format!("Failed to inspect changes for migration: {error}"))?;
    if !status.status.success() {
        return Err(format_git_command_failure(
            "Git failed to inspect changes for migration",
            &status,
        ));
    }
    let status_text = String::from_utf8_lossy(&status.stdout).trim().to_string();
    if status_text.is_empty() {
        return Ok(None);
    }

    let before = git_stdout(repo_path, &["rev-parse", "-q", "--verify", "refs/stash"]);
    let message = format!("another-one-create-branch-{branch_name}");
    let output = git_command(repo_path)
        .args(["stash", "push", "--include-untracked", "-m", &message])
        .output()
        .map_err(|error| format!("Failed to stash changes for migration: {error}"))?;
    if !output.status.success() {
        let mut message =
            format_git_command_failure("Git failed to stash changes for migration", &output);
        if message.contains("No additional details were reported.") {
            message = format!(
                "{}. Git exited with {}. Current changes: {}",
                message,
                output.status,
                status_text.replace('\n', "; ")
            );
        }
        return Err(message);
    }

    let after =
        git_stdout(repo_path, &["rev-parse", "-q", "--verify", "refs/stash"]).unwrap_or_default();
    if before.as_deref() == Some(after.as_str()) {
        return Ok(None);
    }

    Ok(Some("stash@{0}".to_string()))
}

pub fn create_review_task_worktree(
    repo_path: &Path,
    task_name: &str,
    pull_request_number: u64,
    head_branch: &str,
) -> Result<CreatedTaskWorktree, String> {
    let head_branch = head_branch.trim();
    if head_branch.is_empty() {
        return Err("Could not determine the pull request head branch.".to_string());
    }

    let base_ref = fetch_pull_request_head(repo_path, pull_request_number)?;
    let worktree_path = unique_worktree_path(repo_path, &review_worktree_slug(pull_request_number));
    let Some(worktree_parent) = worktree_path.parent() else {
        return Err("Failed to determine the worktree parent directory.".to_string());
    };

    std::fs::create_dir_all(worktree_parent)
        .map_err(|error| format!("Failed to prepare the worktree directory: {error}"))?;

    let output = git_command(repo_path)
        .args([
            "worktree",
            "add",
            "--detach",
            worktree_path.to_string_lossy().as_ref(),
            &base_ref,
        ])
        .output()
        .map_err(|error| format!("Failed to create review worktree: {error}"))?;

    if !output.status.success() {
        return Err(format_git_command_failure(
            "Git failed to create the review worktree",
            &output,
        ));
    }

    Ok(CreatedTaskWorktree {
        path: worktree_path,
        branch_name: head_branch.to_string(),
        task_name: task_name.trim().to_string(),
    })
}

fn review_worktree_slug(pull_request_number: u64) -> String {
    format!("review-{pull_request_number}-wt")
}

fn fetch_pull_request_head(repo_path: &Path, pull_request_number: u64) -> Result<String, String> {
    let fetched_ref = format!("refs/remotes/origin/pr-{pull_request_number}");
    let refspec = format!("+pull/{pull_request_number}/head:{fetched_ref}");
    let output = git_command(repo_path)
        .args(["fetch", "origin", &refspec])
        .output()
        .map_err(|error| format!("Failed to fetch pull request head: {error}"))?;

    if output.status.success() {
        Ok(fetched_ref)
    } else {
        Err(format_git_command_failure(
            "Git failed to fetch the pull request head",
            &output,
        ))
    }
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
            "--sort=-committerdate",
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
        let behind_count = git_behind_count(path, name);

        branches.push(Branch {
            name: name.to_string(),
            lines_added,
            lines_removed,
            ahead_count,
            behind_count,
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
            "--sort=-committerdate",
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
                    behind_count: 0,
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
        behind_count: git_behind_count(path, &branch_name),
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

fn app_worktrees_root(home_dir: &Path) -> PathBuf {
    home_dir.join(".another-one").join("worktrees")
}

fn worktree_repo_directory_name(repo_path: &Path) -> String {
    let repo_root = detect_repo_common_dir(repo_path)
        .and_then(|common_dir| common_dir.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| repo_path.to_path_buf());

    repo_root
        .file_name()
        .and_then(|name| name.to_str())
        .map(slugify_task_name)
        .filter(|name| !name.is_empty())
        .unwrap_or_else(|| "repo".to_string())
}

fn worktree_parent_dir_with_root(repo_path: &Path, worktrees_root: Option<PathBuf>) -> PathBuf {
    worktrees_root
        .map(|root| root.join(worktree_repo_directory_name(repo_path)))
        .unwrap_or_else(|| repo_path.parent().unwrap_or(repo_path).to_path_buf())
}

fn unique_worktree_path(repo_path: &Path, slug: &str) -> PathBuf {
    let parent = worktree_parent_dir_with_root(
        repo_path,
        dirs::home_dir().map(|home| app_worktrees_root(&home)),
    );

    for suffix in 0..1000 {
        let directory_name = if suffix == 0 {
            slug.to_string()
        } else {
            format!("{slug}-{}", suffix + 1)
        };
        let candidate = parent.join(directory_name);
        if !candidate.exists() {
            return candidate;
        }
    }

    parent.join(format!("{slug}-overflow"))
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

fn git_behind_count(path: &Path, branch: &str) -> usize {
    let upstream = format!("{branch}@{{upstream}}");
    if let Ok(out) = git_command(path)
        .args(["rev-list", "--count", &format!("{branch}..{upstream}")])
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
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    use crate::agents::{
        AgentProviderKind, TerminalLaunchConfig, TerminalRestoreStatus, TerminalSessionKind,
        TerminalSessionRef, DEFAULT_AGENT_ID,
    };
    use crate::open_in::OpenInAppKind;
    use crate::shortcuts::ShortcutSettings;

    use super::{
        app_worktrees_root, combine_commit_file_changes, create_branch_from_head,
        create_task_worktree, current_branch, format_git_command_error, git_stdout,
        parse_branch_commit_entries, parse_branch_compare_name_status_entries,
        parse_branch_compare_numstat_entries, parse_recent_branch_commit_page,
        project_action_agent_launch_args, review_worktree_slug, slugify_branch_name,
        unique_branch_name_for_repo, worktree_parent_dir_with_root, BranchCompareNameStatusEntry,
        BranchCompareNumStatEntry, CreateBranchMode, PersistedSectionState, PersistedTerminalTab,
        Project, ProjectAction, ProjectActionAccess, ProjectActionIcon, ProjectActionKind,
        ProjectActionScope, ProjectBranchSettingField, ProjectBranchSettings, ProjectCheckoutState,
        ProjectKind, RepoDefaultCommitAction, RepoRecord, StoreFile, Task, TaskKind,
        TaskWorktreeBranchMode, UiState,
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
            branch_settings: ProjectBranchSettings::default(),
            actions: Vec::new(),
            worktree_name: worktree_name.map(str::to_string),
            repo_common_dir: None,
        }
    }

    #[test]
    fn persisted_terminal_tab_round_trips_pinned() {
        let tab = PersistedTerminalTab {
            id: "tab-1".to_string(),
            title: "Codex".to_string(),
            pinned: true,
            fixed_title: None,
            provider: None,
            launch_config: None,
            restore_status: Default::default(),
        };

        let json = serde_json::to_string(&tab).expect("serialize persisted tab");
        let restored: PersistedTerminalTab =
            serde_json::from_str(&json).expect("deserialize persisted tab");

        assert!(restored.pinned);
    }

    #[test]
    fn persisted_terminal_tab_round_trips_fixed_title() {
        let tab = PersistedTerminalTab {
            id: "tab-1".to_string(),
            title: "Run tests".to_string(),
            pinned: false,
            fixed_title: Some("Run tests".to_string()),
            provider: None,
            launch_config: None,
            restore_status: Default::default(),
        };

        let json = serde_json::to_string(&tab).expect("serialize persisted tab");
        let restored: PersistedTerminalTab =
            serde_json::from_str(&json).expect("deserialize persisted tab");

        assert_eq!(restored.fixed_title.as_deref(), Some("Run tests"));
    }

    #[test]
    fn persisted_terminal_tab_defaults_missing_pinned_to_false() {
        let json = r#"{"id":"tab-1","title":"Codex"}"#;

        let restored: PersistedTerminalTab =
            serde_json::from_str(json).expect("deserialize older persisted tab");

        assert!(!restored.pinned);
        assert_eq!(restored.fixed_title, None);
    }

    fn sample_project_store(root_project: Project) -> super::ProjectStore {
        let file_path = std::env::temp_dir().join(format!(
            "another-one-project-store-test-{}.json",
            uuid::Uuid::new_v4()
        ));
        let project_id = root_project.id.clone();

        let mut store = super::ProjectStore {
            repos: HashMap::new(),
            projects_by_id: HashMap::from([(project_id.clone(), root_project)]),
            projects: Vec::new(),
            project_order: vec![project_id],
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: super::UiState::default(),
            file_path,
        };
        store.refresh_runtime_views();
        store
    }

    fn sample_shell_action(id: &str, scope: ProjectActionScope) -> ProjectAction {
        ProjectAction {
            id: id.to_string(),
            name: "Ok".to_string(),
            icon: ProjectActionIcon::Play,
            run_on_worktree_create: false,
            scope,
            kind: ProjectActionKind::Shell {
                command: "echo ok".to_string(),
            },
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
    fn worktree_parent_dir_with_root_uses_hidden_app_directory() {
        let temp_root =
            std::env::temp_dir().join(format!("another-one-test-{}", uuid::Uuid::new_v4()));
        let repo_path = temp_root.join("repos").join("sample-app");
        let home_dir = temp_root.join("home");
        let expected = app_worktrees_root(&home_dir).join("sample-app");

        fs::create_dir_all(&repo_path).expect("repo path should exist");

        let parent = worktree_parent_dir_with_root(&repo_path, Some(app_worktrees_root(&home_dir)));

        assert_eq!(parent, expected);
    }

    #[test]
    fn review_worktree_slug_uses_pull_request_number() {
        assert_eq!(review_worktree_slug(1808), "review-1808-wt");
    }

    fn run_git(path: &std::path::Path, args: &[&str]) {
        let output = Command::new("git")
            .args(args)
            .current_dir(path)
            .output()
            .expect("git should run");
        assert!(
            output.status.success(),
            "git {:?} failed: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn init_repo() -> tempfile::TempDir {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        run_git(temp_dir.path(), &["init", "-b", "main"]);
        run_git(
            temp_dir.path(),
            &["config", "user.email", "test@example.com"],
        );
        run_git(temp_dir.path(), &["config", "user.name", "Test User"]);
        fs::write(temp_dir.path().join("file.txt"), "base\n").expect("file should write");
        run_git(temp_dir.path(), &["add", "."]);
        run_git(temp_dir.path(), &["commit", "-m", "initial"]);
        temp_dir
    }

    #[test]
    fn slugifies_and_suffixes_duplicate_branch_names() {
        let repo = init_repo();
        run_git(repo.path(), &["branch", "feature-test"]);

        assert_eq!(slugify_branch_name(" Feature Test!! "), "feature-test");
        let unique = unique_branch_name_for_repo(repo.path(), "feature-test")
            .expect("unique branch should resolve");
        assert_eq!(unique, "feature-test-2");
    }

    #[test]
    fn create_task_worktree_creates_unique_branch_from_selected_source_branch() {
        let repo = init_repo();
        run_git(repo.path(), &["switch", "-c", "feature/base"]);
        fs::write(repo.path().join("file.txt"), "base\nfeature\n").expect("file should write");
        run_git(repo.path(), &["add", "."]);
        run_git(repo.path(), &["commit", "-m", "feature base"]);
        let source_head =
            git_stdout(repo.path(), &["rev-parse", "HEAD"]).expect("source head should resolve");
        run_git(repo.path(), &["switch", "main"]);

        let created = create_task_worktree(
            repo.path(),
            "Project",
            "Feature Task",
            "Fallback",
            TaskWorktreeBranchMode::NewBranchFrom {
                source_branch: "feature/base".to_string(),
            },
        )
        .expect("worktree should be created");

        assert_eq!(created.branch_name, "feature-task");
        assert_eq!(
            current_branch(&created.path).as_deref(),
            Some("feature-task")
        );
        assert_eq!(
            git_stdout(&created.path, &["rev-parse", "HEAD"]).as_deref(),
            Some(source_head.as_str())
        );
    }

    #[test]
    fn create_task_worktree_uses_existing_branch_without_creating_new_branch() {
        let repo = init_repo();
        run_git(repo.path(), &["branch", "feature/existing"]);

        let created = create_task_worktree(
            repo.path(),
            "Project",
            "Use Existing",
            "Fallback",
            TaskWorktreeBranchMode::ExistingBranch {
                branch: "feature/existing".to_string(),
            },
        )
        .expect("worktree should be created");

        assert_eq!(created.branch_name, "feature/existing");
        assert_eq!(
            current_branch(&created.path).as_deref(),
            Some("feature/existing")
        );
        assert!(
            !git_stdout(
                repo.path(),
                &["show-ref", "--verify", "refs/heads/use-existing"]
            )
            .is_some_and(|output| !output.is_empty()),
            "existing branch mode should not create a generated task branch"
        );
    }

    #[test]
    fn create_task_worktree_errors_when_existing_branch_is_checked_out_elsewhere() {
        let repo = init_repo();

        let error = create_task_worktree(
            repo.path(),
            "Project",
            "Already Checked Out",
            "Fallback",
            TaskWorktreeBranchMode::ExistingBranch {
                branch: "main".to_string(),
            },
        )
        .expect_err("checked-out branch should be rejected");

        assert!(
            error.starts_with("Branch main is already checked out in another worktree: "),
            "error was {error:?}"
        );
        assert!(
            error.contains(&repo.path().display().to_string()),
            "error should include checkout path, was {error:?}"
        );
    }

    #[test]
    fn creates_current_task_branch_with_dirty_changes_left_in_place() {
        let repo = init_repo();
        fs::write(repo.path().join("file.txt"), "base\nchanged\n").expect("file should write");
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("file should write");
        run_git(repo.path(), &["add", "staged.txt"]);

        let created =
            create_branch_from_head(repo.path(), "Feature Here", CreateBranchMode::CurrentTask)
                .expect("branch should be created");
        assert_eq!(created.branch_name, "feature-here");
        assert_eq!(current_branch(repo.path()).as_deref(), Some("feature-here"));
        let status = git_stdout(repo.path(), &["status", "--porcelain"]).unwrap_or_default();
        assert!(status.contains("file.txt"), "status was {status:?}");
        assert!(status.contains("A  staged.txt"), "status was {status:?}");
    }

    #[test]
    fn creates_clean_worktree_branch_without_migration() {
        let repo = init_repo();
        fs::write(repo.path().join("file.txt"), "base\nchanged\n").expect("file should write");

        let created = create_branch_from_head(
            repo.path(),
            "Clean Worktree",
            CreateBranchMode::Worktree {
                migrate_changes: false,
            },
        )
        .expect("worktree branch should be created");
        assert_eq!(created.branch_name, "clean-worktree");
        assert_eq!(
            current_branch(&created.path).as_deref(),
            Some("clean-worktree")
        );
        assert_eq!(
            git_stdout(&created.path, &["status", "--porcelain"]).unwrap_or_default(),
            ""
        );
        let source_status = git_stdout(repo.path(), &["status", "--porcelain"]).unwrap_or_default();
        assert!(
            source_status.contains("file.txt"),
            "source status was {source_status:?}"
        );
    }

    #[test]
    fn migrates_staged_unstaged_and_untracked_changes_to_worktree() {
        let repo = init_repo();
        fs::write(repo.path().join("file.txt"), "base\nchanged\n").expect("file should write");
        fs::write(repo.path().join("staged.txt"), "staged\n").expect("file should write");
        run_git(repo.path(), &["add", "staged.txt"]);
        fs::write(repo.path().join("untracked.txt"), "untracked\n").expect("file should write");

        let created = create_branch_from_head(
            repo.path(),
            "Migrate Worktree",
            CreateBranchMode::Worktree {
                migrate_changes: true,
            },
        )
        .expect("worktree branch should be created");

        assert_eq!(
            git_stdout(repo.path(), &["status", "--porcelain"]).unwrap_or_default(),
            ""
        );
        let status = git_stdout(&created.path, &["status", "--porcelain"]).unwrap_or_default();
        assert!(status.contains("file.txt"), "status was {status:?}");
        assert!(status.contains("A  staged.txt"), "status was {status:?}");
        assert!(status.contains("?? untracked.txt"), "status was {status:?}");
    }

    #[test]
    fn store_file_deserializes_projects_without_branch_settings() {
        let json = serde_json::json!({
            "version": super::STORE_VERSION,
            "repos": {
                "repo": {
                    "id": "repo",
                    "common_dir": "/tmp/root/.git",
                    "branch_order": ["main"],
                    "branches_by_name": {
                        "main": {
                            "name": "main",
                            "last_commit_relative": "1 day ago",
                            "is_default": true,
                            "ahead_count": 0,
                            "behind_count": 0
                        }
                    }
                }
            },
            "projects": {
                "root": {
                    "id": "root",
                    "repo_id": "repo",
                    "name": "Root",
                    "path": "/tmp/root",
                    "kind": "Root",
                    "checkout": {
                        "current_branch": "main",
                        "lines_added": 0,
                        "lines_removed": 0
                    }
                }
            },
            "project_order": ["root"],
            "tasks": {},
            "task_ids_by_root_project": {},
            "sections": {},
            "ui": {
                "left_sidebar_open": true,
                "expanded_repo_ids": [],
                "repo_default_commit_actions": {},
                "pinned_task_ids": [],
                "last_active_section_id": null,
                "enabled_open_in_apps": null,
                "preferred_open_in_app": null,
                "agent_launch_args": {},
                "shortcuts": ShortcutSettings::default()
            }
        });

        let store: StoreFile =
            serde_json::from_value(json).expect("legacy store JSON should deserialize");
        let project = store.projects.get("root").expect("project should exist");

        assert_eq!(project.branch_settings, ProjectBranchSettings::default());
        assert!(project.actions.is_empty());
        assert!(store.ui.global_actions.is_empty());
    }

    #[test]
    fn project_branch_settings_round_trip() {
        let mut store = StoreFile::default();
        let mut project = sample_project("root", None);
        project.branch_settings = ProjectBranchSettings {
            default_branch: Some("main".to_string()),
            default_target_branch: Some("release".to_string()),
        };
        store.projects.insert(project.id.clone(), project);

        let json = serde_json::to_string(&store).expect("store should serialize");
        let round_trip: StoreFile = serde_json::from_str(&json).expect("store should deserialize");
        let project = round_trip
            .projects
            .get("root")
            .expect("project should exist after round-trip");

        assert_eq!(
            project.branch_settings,
            ProjectBranchSettings {
                default_branch: Some("main".to_string()),
                default_target_branch: Some("release".to_string()),
            }
        );
    }

    #[test]
    fn project_and_global_actions_round_trip() {
        let mut store = StoreFile::default();
        let mut project = sample_project("root", None);
        project.actions.push(ProjectAction {
            id: "project-action".to_string(),
            name: "Test".to_string(),
            icon: ProjectActionIcon::Test,
            run_on_worktree_create: true,
            scope: ProjectActionScope::Project,
            kind: ProjectActionKind::Shell {
                command: "cargo test".to_string(),
            },
        });
        store.projects.insert(project.id.clone(), project);
        store.ui.global_actions.push(ProjectAction {
            id: "global-action".to_string(),
            name: "Review".to_string(),
            icon: ProjectActionIcon::Agent,
            run_on_worktree_create: false,
            scope: ProjectActionScope::Global,
            kind: ProjectActionKind::Agent {
                prompt: "Review this branch".to_string(),
                provider: AgentProviderKind::Codex,
                model: Some("gpt-5.4".to_string()),
                traits: None,
                mode: None,
                access: ProjectActionAccess::WorkspaceWrite,
            },
        });

        let json = serde_json::to_string(&store).expect("store should serialize");
        let round_trip: StoreFile = serde_json::from_str(&json).expect("store should deserialize");

        assert_eq!(round_trip.projects["root"].actions.len(), 1);
        assert_eq!(round_trip.ui.global_actions.len(), 1);
        assert_eq!(
            round_trip.projects["root"].actions[0].kind,
            ProjectActionKind::Shell {
                command: "cargo test".to_string()
            }
        );
    }

    #[test]
    fn upserting_global_action_removes_project_scoped_copy() {
        let mut project = sample_project("root", None);
        project.actions.push(sample_shell_action(
            "action-ok",
            ProjectActionScope::Project,
        ));
        let mut store = sample_project_store(project);

        store
            .upsert_project_action(
                "root",
                sample_shell_action("action-ok", ProjectActionScope::Project),
                true,
            )
            .expect("global action save should succeed");

        assert!(store.projects_by_id["root"].actions.is_empty());
        assert_eq!(store.ui.global_actions.len(), 1);
        assert_eq!(store.ui.global_actions[0].scope, ProjectActionScope::Global);

        let visible_actions = store.project_actions("root");
        assert_eq!(visible_actions.len(), 1);
        assert_eq!(visible_actions[0].id, "action-ok");
        assert_eq!(visible_actions[0].scope, ProjectActionScope::Global);
    }

    #[test]
    fn project_actions_prefer_global_copy_when_ids_overlap() {
        let mut project = sample_project("root", None);
        project.actions.push(sample_shell_action(
            "action-ok",
            ProjectActionScope::Project,
        ));
        let mut store = sample_project_store(project);
        store
            .ui
            .global_actions
            .push(sample_shell_action("action-ok", ProjectActionScope::Global));

        let visible_actions = store.project_actions("root");

        assert_eq!(visible_actions.len(), 1);
        assert_eq!(visible_actions[0].id, "action-ok");
        assert_eq!(visible_actions[0].scope, ProjectActionScope::Global);
    }

    #[test]
    fn upserting_project_action_removes_global_scoped_copy() {
        let project = sample_project("root", None);
        let mut store = sample_project_store(project);
        store
            .ui
            .global_actions
            .push(sample_shell_action("action-ok", ProjectActionScope::Global));

        store
            .upsert_project_action(
                "root",
                sample_shell_action("action-ok", ProjectActionScope::Global),
                false,
            )
            .expect("project action save should succeed");

        assert!(store.ui.global_actions.is_empty());
        assert_eq!(store.projects_by_id["root"].actions.len(), 1);
        assert_eq!(
            store.projects_by_id["root"].actions[0].scope,
            ProjectActionScope::Project
        );

        let visible_actions = store.project_actions("root");
        assert_eq!(visible_actions.len(), 1);
        assert_eq!(visible_actions[0].id, "action-ok");
        assert_eq!(visible_actions[0].scope, ProjectActionScope::Project);
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
                                pinned: false,
                                fixed_title: None,
                                provider: None,
                                launch_config: Some(TerminalLaunchConfig::default()),
                                restore_status: TerminalRestoreStatus::NotStarted,
                            },
                            PersistedTerminalTab {
                                id: "1".to_string(),
                                title: "Claude Code".to_string(),
                                pinned: false,
                                fixed_title: None,
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
                            pinned: false,
                            fixed_title: None,
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
                                pinned: false,
                                fixed_title: None,
                                provider: None,
                                launch_config: Some(TerminalLaunchConfig::default()),
                                restore_status: TerminalRestoreStatus::NotStarted,
                            },
                            PersistedTerminalTab {
                                id: "1".to_string(),
                                title: "Claude Code".to_string(),
                                pinned: false,
                                fixed_title: None,
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
                            pinned: false,
                            fixed_title: None,
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
                repo_default_commit_actions: HashMap::from([(
                    "repo".to_string(),
                    RepoDefaultCommitAction::CommitAndPush,
                )]),
                pinned_task_ids: HashSet::from(["task-1".to_string(), "task-2".to_string()]),
                last_active_section_id: None,
                enabled_open_in_apps: None,
                preferred_open_in_app: None,
                enabled_agents: None,
                default_agent_id: None,
                agent_launch_args: HashMap::new(),
                git_commit_generation_script: None,
                git_pr_generation_script: None,
                shortcuts: ShortcutSettings::default(),
                global_actions: Vec::new(),
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
            round_trip.ui.repo_default_commit_actions,
            HashMap::from([("repo".to_string(), RepoDefaultCommitAction::CommitAndPush)])
        );
        assert_eq!(
            round_trip.ui.pinned_task_ids,
            HashSet::from(["task-1".to_string(), "task-2".to_string()])
        );
    }

    #[test]
    fn clear_missing_branch_settings_removes_stale_values_and_reports_fallbacks() {
        let mut root_project = sample_project("root", None);
        root_project.branch_settings = ProjectBranchSettings {
            default_branch: Some("release".to_string()),
            default_target_branch: Some("staging".to_string()),
        };

        let mut store = super::ProjectStore {
            repos: HashMap::from([(
                "repo".to_string(),
                RepoRecord {
                    id: "repo".to_string(),
                    common_dir: Some(PathBuf::from("/tmp/repo/.git")),
                    branch_order: vec!["main".to_string(), "feature".to_string()],
                    branches_by_name: HashMap::from([(
                        "main".to_string(),
                        super::RepoBranchRecord {
                            name: "main".to_string(),
                            last_commit_relative: "1 day ago".to_string(),
                            is_default: true,
                            ahead_count: 0,
                            behind_count: 0,
                        },
                    )]),
                },
            )]),
            projects_by_id: HashMap::from([("root".to_string(), root_project)]),
            projects: Vec::new(),
            project_order: vec!["root".to_string()],
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: super::UiState::default(),
            file_path: PathBuf::from("/tmp/projects.json"),
        };
        store.refresh_runtime_views();

        let invalid = store.clear_missing_branch_settings("root");
        let project = store.project("root").expect("root project should exist");

        assert_eq!(project.branch_settings, ProjectBranchSettings::default());
        assert_eq!(invalid.len(), 2);
        assert!(invalid.iter().any(|entry| {
            entry.field == ProjectBranchSettingField::DefaultBranch
                && entry.branch_name == "release"
                && entry.fallback_branch.as_deref() == Some("main")
        }));
        assert!(invalid.iter().any(|entry| {
            entry.field == ProjectBranchSettingField::DefaultTargetBranch
                && entry.branch_name == "staging"
                && entry.fallback_branch.is_none()
        }));
    }

    #[test]
    fn root_project_resolution_uses_root_for_worktree_projects() {
        let mut root_project = sample_project("root", None);
        root_project.repo_id = "repo".to_string();
        let mut worktree_project = sample_project("wt", Some("wt"));
        worktree_project.repo_id = "repo".to_string();

        let mut store = super::ProjectStore {
            repos: HashMap::from([(
                "repo".to_string(),
                RepoRecord {
                    id: "repo".to_string(),
                    common_dir: Some(PathBuf::from("/tmp/repo/.git")),
                    branch_order: vec!["main".to_string()],
                    branches_by_name: HashMap::new(),
                },
            )]),
            projects_by_id: HashMap::from([
                ("root".to_string(), root_project),
                ("wt".to_string(), worktree_project),
            ]),
            projects: Vec::new(),
            project_order: vec!["root".to_string(), "wt".to_string()],
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: super::UiState::default(),
            file_path: PathBuf::from("/tmp/projects.json"),
        };
        store.refresh_runtime_views();

        assert_eq!(
            store.root_project_id_for_project("wt").as_deref(),
            Some("root")
        );
    }

    #[test]
    fn primary_branch_for_project_prefers_saved_default_branch() {
        let mut root_project = sample_project("root", None);
        root_project.checkout.current_branch = Some("feature".to_string());
        root_project.branch_settings.default_branch = Some("main".to_string());

        let mut store = super::ProjectStore {
            repos: HashMap::from([(
                "repo".to_string(),
                RepoRecord {
                    id: "repo".to_string(),
                    common_dir: Some(PathBuf::from("/tmp/repo/.git")),
                    branch_order: vec!["main".to_string(), "feature".to_string()],
                    branches_by_name: HashMap::from([
                        (
                            "main".to_string(),
                            super::RepoBranchRecord {
                                name: "main".to_string(),
                                last_commit_relative: "1 day ago".to_string(),
                                is_default: true,
                                ahead_count: 0,
                                behind_count: 0,
                            },
                        ),
                        (
                            "feature".to_string(),
                            super::RepoBranchRecord {
                                name: "feature".to_string(),
                                last_commit_relative: "1 hour ago".to_string(),
                                is_default: false,
                                ahead_count: 0,
                                behind_count: 0,
                            },
                        ),
                    ]),
                },
            )]),
            projects_by_id: HashMap::from([("root".to_string(), root_project)]),
            projects: Vec::new(),
            project_order: vec!["root".to_string()],
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: super::UiState::default(),
            file_path: PathBuf::from("/tmp/projects.json"),
        };
        store.refresh_runtime_views();

        let branch = store
            .primary_branch_for_project("root", true)
            .expect("preferred branch should resolve");

        assert_eq!(branch.name, "main");
    }

    #[test]
    fn parse_branch_compare_entries_support_renames() {
        let name_status = b"R100\0old/path.rs\0new/path.rs\0M\0src/app.rs\0";
        let numstat = [
            b"12\t0\t\0".as_slice(),
            b"old/path.rs\0".as_slice(),
            b"new/path.rs\0".as_slice(),
            b"5\t1\tsrc/app.rs\0".as_slice(),
        ]
        .concat();

        let statuses = parse_branch_compare_name_status_entries(name_status);
        let stats = parse_branch_compare_numstat_entries(&numstat);

        assert_eq!(statuses.len(), 2);
        assert_eq!(statuses[0].status, 'R');
        assert_eq!(statuses[0].original_path.as_deref(), Some("old/path.rs"));
        assert_eq!(statuses[0].path, "new/path.rs");
        assert_eq!(stats.len(), 2);
        assert_eq!(stats[0].original_path.as_deref(), Some("old/path.rs"));
        assert_eq!(stats[0].path, "new/path.rs");
        assert_eq!(stats[0].additions, 12);
        assert_eq!(stats[1].deletions, 1);
    }

    #[test]
    fn parse_branch_commit_entries_supports_trailing_newlines() {
        let commits = parse_branch_commit_entries(
            b"abc123\0abc123\0Add panel\0Jeff\01 hour ago\x1e\n\
              def456\0def456\0Fix empty state\0Sam\02 hours ago\x1e\n",
        );

        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].id, "abc123");
        assert_eq!(commits[0].subject, "Add panel");
        assert_eq!(commits[1].author_name, "Sam");
        assert_eq!(commits[1].authored_relative, "2 hours ago");
    }

    #[test]
    fn combine_commit_file_changes_deduplicates_merge_name_status_entries() {
        let files = combine_commit_file_changes(
            vec![
                BranchCompareNameStatusEntry {
                    path: "AGENTS.md".to_string(),
                    original_path: None,
                    status: 'M',
                },
                BranchCompareNameStatusEntry {
                    path: "AGENTS.md".to_string(),
                    original_path: None,
                    status: 'M',
                },
                BranchCompareNameStatusEntry {
                    path: "src/app.rs".to_string(),
                    original_path: None,
                    status: 'A',
                },
            ],
            vec![
                BranchCompareNumStatEntry {
                    path: "AGENTS.md".to_string(),
                    original_path: None,
                    additions: 30,
                    deletions: 0,
                },
                BranchCompareNumStatEntry {
                    path: "src/app.rs".to_string(),
                    original_path: None,
                    additions: 12,
                    deletions: 3,
                },
            ],
        );

        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "AGENTS.md");
        assert_eq!(files[0].additions, 30);
        assert_eq!(files[1].path, "src/app.rs");
        assert_eq!(files[1].status, 'A');
        assert_eq!(files[1].deletions, 3);
    }

    #[test]
    fn recent_commit_page_exact_limit_has_no_more_results() {
        let bytes = (0..20)
            .map(|index| {
                format!(
                    "commit-{index}\0{:07x}\0Commit {index}\0Jeff\0{} hour ago\x1e\n",
                    index + 1,
                    index + 1
                )
            })
            .collect::<String>();

        let (commits, has_more) = parse_recent_branch_commit_page(bytes.as_bytes(), 20);

        assert_eq!(commits.len(), 20);
        assert!(!has_more);
        assert_eq!(commits[0].id, "commit-0");
        assert_eq!(commits[19].subject, "Commit 19");
    }

    #[test]
    fn recent_commit_page_overfetch_sets_has_more() {
        let bytes = (0..21)
            .map(|index| {
                format!(
                    "commit-{index}\0{:07x}\0Commit {index}\0Jeff\0{} hour ago\x1e\n",
                    index + 1,
                    index + 1
                )
            })
            .collect::<String>();

        let (commits, has_more) = parse_recent_branch_commit_page(bytes.as_bytes(), 20);

        assert_eq!(commits.len(), 20);
        assert!(has_more);
        assert_eq!(commits[19].id, "commit-19");
    }

    #[test]
    fn sanitize_removes_stale_repo_default_commit_actions() {
        let mut store = super::ProjectStore {
            repos: HashMap::from([(
                "repo".to_string(),
                RepoRecord {
                    id: "repo".to_string(),
                    common_dir: None,
                    branch_order: Vec::new(),
                    branches_by_name: HashMap::new(),
                },
            )]),
            projects_by_id: HashMap::from([("root".to_string(), sample_project("root", None))]),
            projects: Vec::new(),
            project_order: vec!["root".to_string()],
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: super::UiState {
                repo_default_commit_actions: HashMap::from([
                    ("repo".to_string(), RepoDefaultCommitAction::Commit),
                    ("stale".to_string(), RepoDefaultCommitAction::CommitAndPush),
                ]),
                ..super::UiState::default()
            },
            file_path: PathBuf::from("/tmp/test-projects.json"),
        };

        store.sanitize();

        assert_eq!(
            store.ui.repo_default_commit_actions,
            HashMap::from([("repo".to_string(), RepoDefaultCommitAction::Commit)])
        );
    }

    #[test]
    fn preferred_open_in_app_falls_back_and_updates() {
        let mut store = super::ProjectStore {
            repos: HashMap::new(),
            projects_by_id: HashMap::new(),
            projects: Vec::new(),
            project_order: Vec::new(),
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: super::UiState {
                preferred_open_in_app: Some(OpenInAppKind::Zed),
                enabled_open_in_apps: Some(HashSet::from([
                    OpenInAppKind::Cursor,
                    OpenInAppKind::VsCode,
                ])),
                ..super::UiState::default()
            },
            file_path: PathBuf::from("/tmp/test-projects.json"),
        };

        let available = vec![OpenInAppKind::Cursor, OpenInAppKind::VsCode];
        assert_eq!(
            store.preferred_open_in_app(&available),
            Some(OpenInAppKind::Cursor)
        );

        store.set_preferred_open_in_app(OpenInAppKind::VsCode, &available);
        assert_eq!(
            store.preferred_open_in_app(&available),
            Some(OpenInAppKind::VsCode)
        );
    }

    #[test]
    fn store_file_round_trip_preserves_agent_launch_args() {
        let mut store = StoreFile::default();
        store.ui = UiState {
            enabled_agents: Some(HashSet::from([
                "codex".to_string(),
                "claude-code".to_string(),
            ])),
            default_agent_id: Some("codex".to_string()),
            agent_launch_args: HashMap::from([
                ("codex".to_string(), vec!["--yolo".to_string()]),
                (
                    "claude-code".to_string(),
                    vec!["--dangerously-skip-permissions".to_string()],
                ),
            ]),
            ..UiState::default()
        };

        let json = serde_json::to_string(&store).expect("store should serialize");
        let round_trip: StoreFile = serde_json::from_str(&json).expect("store should deserialize");

        assert_eq!(
            round_trip.ui.agent_launch_args.get("codex"),
            Some(&vec!["--yolo".to_string()])
        );
        assert_eq!(
            round_trip.ui.agent_launch_args.get("claude-code"),
            Some(&vec!["--dangerously-skip-permissions".to_string()])
        );
        assert_eq!(
            round_trip.ui.enabled_agents,
            Some(HashSet::from([
                "codex".to_string(),
                "claude-code".to_string()
            ]))
        );
        assert_eq!(round_trip.ui.default_agent_id.as_deref(), Some("codex"));
    }

    #[test]
    fn project_action_codex_launch_args_include_prompt_model_and_access() {
        let action = ProjectAction {
            id: "a".to_string(),
            name: "Ask Codex".to_string(),
            icon: ProjectActionIcon::Agent,
            run_on_worktree_create: false,
            scope: ProjectActionScope::Project,
            kind: ProjectActionKind::Agent {
                prompt: "Fix the bug".to_string(),
                provider: AgentProviderKind::Codex,
                model: Some("gpt-5.4".to_string()),
                traits: None,
                mode: None,
                access: ProjectActionAccess::WorkspaceWrite,
            },
        };

        assert_eq!(
            project_action_agent_launch_args(&action).expect("args should build"),
            vec![
                "--model",
                "gpt-5.4",
                "--sandbox",
                "workspace-write",
                "--ask-for-approval",
                "on-request",
                "Fix the bug"
            ]
        );
    }

    #[test]
    fn project_action_codex_full_access_uses_bypass_flag_only() {
        let action = ProjectAction {
            id: "a".to_string(),
            name: "Ask Codex".to_string(),
            icon: ProjectActionIcon::Agent,
            run_on_worktree_create: false,
            scope: ProjectActionScope::Project,
            kind: ProjectActionKind::Agent {
                prompt: "Fix the bug".to_string(),
                provider: AgentProviderKind::Codex,
                model: None,
                traits: None,
                mode: None,
                access: ProjectActionAccess::FullAccess,
            },
        };

        assert_eq!(
            project_action_agent_launch_args(&action).expect("args should build"),
            vec!["--dangerously-bypass-approvals-and-sandbox", "Fix the bug"]
        );
    }

    #[test]
    fn project_action_claude_launch_args_include_prompt_model_traits_and_access() {
        let action = ProjectAction {
            id: "a".to_string(),
            name: "Ask Claude".to_string(),
            icon: ProjectActionIcon::Agent,
            run_on_worktree_create: false,
            scope: ProjectActionScope::Project,
            kind: ProjectActionKind::Agent {
                prompt: "Summarize".to_string(),
                provider: AgentProviderKind::ClaudeCode,
                model: Some("sonnet".to_string()),
                traits: Some("high".to_string()),
                mode: None,
                access: ProjectActionAccess::FullAccess,
            },
        };

        assert_eq!(
            project_action_agent_launch_args(&action).expect("args should build"),
            vec![
                "--model",
                "sonnet",
                "--effort",
                "high",
                "--permission-mode",
                "bypassPermissions",
                "Summarize"
            ]
        );
    }

    #[test]
    fn project_action_plan_mode_overrides_access() {
        let codex_action = ProjectAction {
            id: "a".to_string(),
            name: "Plan with Codex".to_string(),
            icon: ProjectActionIcon::Agent,
            run_on_worktree_create: false,
            scope: ProjectActionScope::Project,
            kind: ProjectActionKind::Agent {
                prompt: "Refactor this".to_string(),
                provider: AgentProviderKind::Codex,
                model: None,
                traits: None,
                mode: Some("plan".to_string()),
                access: ProjectActionAccess::FullAccess,
            },
        };
        let codex_args =
            project_action_agent_launch_args(&codex_action).expect("args should build");
        assert_eq!(
            codex_args[0..4],
            ["--sandbox", "read-only", "--ask-for-approval", "on-request"]
        );
        assert!(codex_args
            .last()
            .expect("prompt arg should exist")
            .starts_with("You are in plan mode."));

        let claude_action = ProjectAction {
            id: "b".to_string(),
            name: "Plan with Claude".to_string(),
            icon: ProjectActionIcon::Agent,
            run_on_worktree_create: false,
            scope: ProjectActionScope::Project,
            kind: ProjectActionKind::Agent {
                prompt: "Refactor this".to_string(),
                provider: AgentProviderKind::ClaudeCode,
                model: None,
                traits: None,
                mode: Some("plan".to_string()),
                access: ProjectActionAccess::FullAccess,
            },
        };
        assert_eq!(
            project_action_agent_launch_args(&claude_action).expect("args should build")[0..2],
            ["--permission-mode", "plan"]
        );
    }

    #[test]
    fn shell_action_launch_args_are_empty() {
        let action = ProjectAction {
            id: "a".to_string(),
            name: "Test".to_string(),
            icon: ProjectActionIcon::Test,
            run_on_worktree_create: false,
            scope: ProjectActionScope::Project,
            kind: ProjectActionKind::Shell {
                command: "cargo test".to_string(),
            },
        };

        assert_eq!(
            project_action_agent_launch_args(&action).expect("shell args should build"),
            Vec::<String>::new()
        );
    }

    #[test]
    fn git_commit_generation_script_helpers_persist_and_reset() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let file_path = temp_dir.path().join("projects.json");
        let mut store = super::ProjectStore {
            repos: HashMap::new(),
            projects_by_id: HashMap::new(),
            projects: Vec::new(),
            project_order: Vec::new(),
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: UiState::default(),
            file_path: file_path.clone(),
        };

        let custom_script = "Use conventional commits.";
        assert!(store.set_git_commit_generation_script(custom_script));
        assert_eq!(store.git_commit_generation_script(), custom_script);

        let saved: StoreFile = serde_json::from_str(
            &fs::read_to_string(&file_path).expect("saved config should exist"),
        )
        .expect("saved config should deserialize");
        assert_eq!(
            saved.ui.git_commit_generation_script.as_deref(),
            Some(custom_script)
        );

        assert!(store.reset_git_commit_generation_script());
        assert_eq!(
            store.git_commit_generation_script(),
            crate::git_actions::default_commit_generation_script()
        );

        let saved: StoreFile = serde_json::from_str(
            &fs::read_to_string(&file_path).expect("saved config should exist"),
        )
        .expect("saved config should deserialize");
        assert!(saved.ui.git_commit_generation_script.is_none());
    }

    #[test]
    fn git_pr_generation_script_helpers_persist_and_reset() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let file_path = temp_dir.path().join("projects.json");
        let mut store = super::ProjectStore {
            repos: HashMap::new(),
            projects_by_id: HashMap::new(),
            projects: Vec::new(),
            project_order: Vec::new(),
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: UiState::default(),
            file_path: file_path.clone(),
        };

        let custom_script = "Return a concise PR title followed by a reviewer-focused body.";
        assert!(store.set_git_pr_generation_script(custom_script));
        assert_eq!(store.git_pr_generation_script(), custom_script);

        let saved: StoreFile = serde_json::from_str(
            &fs::read_to_string(&file_path).expect("saved config should exist"),
        )
        .expect("saved config should deserialize");
        assert_eq!(
            saved.ui.git_pr_generation_script.as_deref(),
            Some(custom_script)
        );

        assert!(store.reset_git_pr_generation_script());
        assert_eq!(
            store.git_pr_generation_script(),
            crate::git_actions::default_pr_generation_script()
        );

        let saved: StoreFile = serde_json::from_str(
            &fs::read_to_string(&file_path).expect("saved config should exist"),
        )
        .expect("saved config should deserialize");
        assert!(saved.ui.git_pr_generation_script.is_none());
    }

    #[test]
    fn store_file_loads_without_agent_launch_args() {
        let json = serde_json::json!({
            "version": super::STORE_VERSION,
            "repos": {},
            "projects": {},
            "project_order": [],
            "tasks": {},
            "task_ids_by_root_project": {},
            "sections": {},
            "ui": {
                "left_sidebar_open": true,
                "expanded_repo_ids": [],
                "repo_default_commit_actions": {},
                "pinned_task_ids": [],
                "last_active_section_id": null,
                "enabled_open_in_apps": null,
                "preferred_open_in_app": null,
                "enabled_agents": null,
                "default_agent_id": null,
                "shortcuts": ShortcutSettings::default()
            }
        });

        let store: StoreFile =
            serde_json::from_value(json).expect("legacy store JSON should deserialize");

        assert!(store.ui.agent_launch_args.is_empty());
        assert!(store.ui.enabled_agents.is_none());
        assert!(store.ui.default_agent_id.is_none());
    }

    #[test]
    fn store_file_preserves_unknown_agent_launch_arg_keys() {
        let json = serde_json::json!({
            "version": super::STORE_VERSION,
            "repos": {},
            "projects": {},
            "project_order": [],
            "tasks": {},
            "task_ids_by_root_project": {},
            "sections": {},
            "ui": {
                "agent_launch_args": {
                    "future-agent": ["--future-flag"]
                }
            }
        });

        let store: StoreFile = serde_json::from_value(json).expect("store JSON should deserialize");
        let json = serde_json::to_value(&store).expect("store JSON should serialize");

        assert_eq!(
            json.get("ui")
                .and_then(|ui| ui.get("agent_launch_args"))
                .and_then(|args| args.get("future-agent"))
                .and_then(|value| value.as_array())
                .map(|values| {
                    values
                        .iter()
                        .filter_map(|value| value.as_str().map(str::to_string))
                        .collect::<Vec<_>>()
                }),
            Some(vec!["--future-flag".to_string()])
        );
    }

    #[test]
    fn store_agent_launch_arg_helpers_persist_add_and_remove() {
        let temp_dir = tempfile::tempdir().expect("temp dir should exist");
        let file_path = temp_dir.path().join("projects.json");
        let mut store = super::ProjectStore {
            repos: HashMap::new(),
            projects_by_id: HashMap::new(),
            projects: Vec::new(),
            project_order: Vec::new(),
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: UiState::default(),
            file_path: file_path.clone(),
        };

        assert!(store
            .set_agent_launch_args("codex", vec!["--yolo".to_string(), "--profile".to_string()]));
        assert_eq!(
            store.agent_launch_args("codex"),
            ["--yolo".to_string(), "--profile".to_string()]
        );

        let saved: StoreFile = serde_json::from_str(
            &fs::read_to_string(&file_path).expect("saved config should exist"),
        )
        .expect("saved config should deserialize");
        assert_eq!(
            saved.ui.agent_launch_args.get("codex"),
            Some(&vec!["--yolo".to_string(), "--profile".to_string()])
        );

        assert!(store.remove_agent_launch_args("codex"));
        assert!(store.agent_launch_args("codex").is_empty());

        let saved: StoreFile = serde_json::from_str(
            &fs::read_to_string(&file_path).expect("saved config should exist"),
        )
        .expect("saved config should deserialize");
        assert!(!saved.ui.agent_launch_args.contains_key("codex"));
    }

    #[test]
    fn legacy_store_treats_all_agents_as_enabled() {
        let store = super::ProjectStore {
            repos: HashMap::new(),
            projects_by_id: HashMap::new(),
            projects: Vec::new(),
            project_order: Vec::new(),
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: UiState::default(),
            file_path: PathBuf::from("/tmp/test-projects.json"),
        };

        assert_eq!(
            store.enabled_agent_ids(),
            crate::agents::AGENTS
                .iter()
                .map(|agent| agent.id)
                .collect::<Vec<_>>()
        );
        assert!(crate::agents::AGENTS
            .iter()
            .all(|agent| store.agent_enabled(agent.id)));
        assert_eq!(store.default_agent_id(), Some(DEFAULT_AGENT_ID));
    }

    #[test]
    fn store_agent_enabled_helpers_preserve_display_order() {
        let mut store = super::ProjectStore {
            repos: HashMap::new(),
            projects_by_id: HashMap::new(),
            projects: Vec::new(),
            project_order: Vec::new(),
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: UiState {
                enabled_agents: Some(HashSet::from([
                    "forge".to_string(),
                    "codex".to_string(),
                    "claude-code".to_string(),
                ])),
                default_agent_id: Some("codex".to_string()),
                ..UiState::default()
            },
            file_path: PathBuf::from("/tmp/test-projects.json"),
        };

        assert_eq!(
            store.enabled_agent_ids(),
            vec!["claude-code", "codex", "forge"]
        );
        assert!(store.agent_enabled("codex"));
        assert!(!store.agent_enabled("pi"));
        assert!(store.agent_is_default("codex"));

        assert!(store.set_agent_enabled("codex", false));
        assert!(!store.agent_enabled("codex"));
        assert_eq!(store.default_agent_id(), Some("claude-code"));
        assert!(store.set_agent_enabled("pi", true));
        assert!(store.agent_enabled("pi"));
        assert_eq!(
            store.enabled_agent_ids(),
            vec!["claude-code", "pi", "forge"]
        );
        assert_eq!(store.default_agent_id(), Some("claude-code"));
    }

    #[test]
    fn store_default_agent_prefers_saved_enabled_value_and_falls_back() {
        let mut store = super::ProjectStore {
            repos: HashMap::new(),
            projects_by_id: HashMap::new(),
            projects: Vec::new(),
            project_order: Vec::new(),
            tasks_by_id: HashMap::new(),
            tasks: HashMap::new(),
            task_ids_by_root_project: HashMap::new(),
            terminal_sections: HashMap::new(),
            ui: UiState {
                enabled_agents: Some(HashSet::from([
                    "claude-code".to_string(),
                    "codex".to_string(),
                ])),
                default_agent_id: Some("codex".to_string()),
                ..UiState::default()
            },
            file_path: PathBuf::from("/tmp/test-projects.json"),
        };

        assert_eq!(store.default_agent_id(), Some("codex"));
        assert!(store.set_default_agent("claude-code"));
        assert_eq!(store.default_agent_id(), Some("claude-code"));
        assert!(!store.set_default_agent("pi"));

        assert!(store.set_agent_enabled("claude-code", false));
        assert_eq!(store.default_agent_id(), Some("codex"));
    }
}
