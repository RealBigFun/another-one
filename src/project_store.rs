//! Persistent project store.
//!
//! Projects are saved as JSON in `~/.config/another-one/projects.json`.
//! Each project is a folder the user has added. The store is designed to be
//! extended later with per-project settings and sub-worktrees.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::agents::{AgentProviderKind, ResumeTarget, TerminalLaunchKind};

/// A single project entry. Will later carry per-project settings, worktrees, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Project {
    /// Stable identifier (UUID v4).
    pub id: String,
    /// Display name (derived from folder name at add-time, but stored so it can be renamed).
    pub name: String,
    /// Absolute path to the project root folder.
    pub path: PathBuf,
    /// Per-project settings (reserved for future use).
    #[serde(default)]
    pub settings: ProjectSettings,
    /// Sub-worktrees (reserved for future use).
    #[serde(default)]
    pub worktrees: Vec<Worktree>,
    /// Branches detected in this project.
    #[serde(default)]
    pub branches: Vec<Branch>,
    /// Name of the git worktree if this is not the main working tree.
    #[serde(default)]
    pub worktree_name: Option<String>,
    /// Canonical git common-dir used to group worktrees that belong to the same repo.
    #[serde(default)]
    pub repo_common_dir: Option<PathBuf>,
}

/// A git branch with optional diff stats.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Branch {
    pub name: String,
    /// Lines added vs upstream (if available).
    pub lines_added: i32,
    /// Lines removed vs upstream (if available).
    pub lines_removed: i32,
    /// Number of commits ahead of the configured upstream branch.
    #[serde(default)]
    pub ahead_count: usize,
    /// Human-readable time since last commit (e.g. "3mo ago").
    pub last_commit_relative: String,
    /// Whether this is the default/primary branch.
    pub is_default: bool,
    /// Whether this branch is currently checked out.
    #[serde(default)]
    pub is_current: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGitMetadata {
    pub branches: Vec<Branch>,
    pub worktree_name: Option<String>,
    pub repo_common_dir: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProjectGitState {
    pub changed_files: Vec<ChangedFile>,
    pub ahead_count: usize,
    pub metadata: Option<ProjectGitMetadata>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DirectTask {
    pub id: String,
    pub name: String,
    pub branch_name: String,
}

#[derive(Debug, Clone)]
pub struct CreatedTaskWorktree {
    pub path: PathBuf,
    pub branch_name: String,
    pub task_name: String,
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

/// Placeholder for future per-project settings.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProjectSettings {
    // Will hold things like custom color, pinned status, etc.
}

/// Placeholder for future sub-worktree support.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Worktree {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedTerminalTab {
    pub id: usize,
    pub title: String,
    pub kind: TerminalLaunchKind,
    #[serde(default)]
    pub provider: Option<AgentProviderKind>,
    #[serde(default)]
    pub launch_argv: Vec<String>,
    #[serde(default)]
    pub resume_target: Option<ResumeTarget>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PersistedSectionState {
    pub active_tab_id: usize,
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
    pub expanded_project_ids: Option<HashSet<String>>,
    #[serde(default)]
    pub pinned_direct_task_ids: HashSet<String>,
    #[serde(default)]
    pub pinned_worktree_project_ids: HashSet<String>,
    #[serde(default)]
    pub last_active_section_key: Option<String>,
}

impl Default for UiState {
    fn default() -> Self {
        Self {
            left_sidebar_open: default_left_sidebar_open(),
            expanded_project_ids: None,
            pinned_direct_task_ids: HashSet::new(),
            pinned_worktree_project_ids: HashSet::new(),
            last_active_section_key: None,
        }
    }
}

fn default_left_sidebar_open() -> bool {
    true
}

/// The on-disk format.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct StoreFile {
    #[serde(default)]
    projects: Vec<Project>,
    #[serde(default)]
    direct_tasks: HashMap<String, Vec<DirectTask>>,
    #[serde(default)]
    worktree_task_names: HashMap<String, String>,
    #[serde(default)]
    terminal_sections: HashMap<String, PersistedSectionState>,
    #[serde(default)]
    ui: UiState,
}

/// In-memory project store with load/save to disk.
#[derive(Debug, Clone)]
pub struct ProjectStore {
    pub projects: Vec<Project>,
    pub direct_tasks: HashMap<String, Vec<DirectTask>>,
    pub worktree_task_names: HashMap<String, String>,
    pub terminal_sections: HashMap<String, PersistedSectionState>,
    pub ui: UiState,
    file_path: PathBuf,
}

impl ProjectStore {
    /// Load (or create) the store from the default config location.
    #[hotpath::measure]
    pub fn load() -> Self {
        let file_path = Self::config_path();
        let StoreFile {
            mut projects,
            mut direct_tasks,
            mut worktree_task_names,
            mut terminal_sections,
            mut ui,
        } = Self::read_from_disk(&file_path);

        // Detect worktree status for all projects on load.
        for project in &mut projects {
            project.worktree_name = detect_worktree_name(&project.path);
            project.repo_common_dir = detect_repo_common_dir(&project.path);
        }

        let project_ids: HashSet<_> = projects.iter().map(|project| project.id.clone()).collect();
        let direct_task_ids: HashSet<_> = direct_tasks
            .values()
            .flatten()
            .map(|task| task.id.clone())
            .collect();
        direct_tasks.retain(|project_id, _| project_ids.contains(project_id));
        worktree_task_names.retain(|project_id, _| project_ids.contains(project_id));
        terminal_sections.retain(|section_key, _| {
            let mut parts = section_key.splitn(3, "::");
            let Some(project_id) = parts.next() else {
                return false;
            };
            let Some(_branch_name) = parts.next() else {
                return false;
            };
            let Some(task_id) = parts.next() else {
                return false;
            };

            project_ids.contains(project_id)
                && (task_id.is_empty() || direct_task_ids.contains(task_id))
        });
        if let Some(expanded_project_ids) = ui.expanded_project_ids.as_mut() {
            expanded_project_ids.retain(|project_id| project_ids.contains(project_id));
        }
        ui.pinned_direct_task_ids
            .retain(|task_id| direct_task_ids.contains(task_id));
        ui.pinned_worktree_project_ids
            .retain(|project_id| project_ids.contains(project_id));
        if ui
            .last_active_section_key
            .as_ref()
            .is_some_and(|section_key| !terminal_sections.contains_key(section_key))
        {
            ui.last_active_section_key = None;
        }

        Self {
            projects,
            direct_tasks,
            worktree_task_names,
            terminal_sections,
            ui,
            file_path,
        }
    }

    /// Remove a project by id.
    #[allow(dead_code)]
    pub fn remove_project(&mut self, id: &str) {
        self.projects.retain(|p| p.id != id);
        self.direct_tasks.remove(id);
        self.worktree_task_names.remove(id);
        self.terminal_sections
            .retain(|section_key, _| !section_key.starts_with(&format!("{id}::")));
        if self
            .ui
            .last_active_section_key
            .as_ref()
            .is_some_and(|section_key| section_key.starts_with(&format!("{id}::")))
        {
            self.ui.last_active_section_key = None;
        }
        self.ui.pinned_worktree_project_ids.remove(id);
        self.save();
    }

    pub fn remove_direct_task(&mut self, project_id: &str, task_id: &str) -> Option<DirectTask> {
        let tasks = self.direct_tasks.get_mut(project_id)?;
        let task_index = tasks.iter().position(|task| task.id == task_id)?;
        let removed = tasks.remove(task_index);
        if tasks.is_empty() {
            self.direct_tasks.remove(project_id);
        }
        self.ui.pinned_direct_task_ids.remove(task_id);
        Some(removed)
    }

    pub fn set_direct_task_pinned(&mut self, task_id: &str, pinned: bool) -> bool {
        if pinned {
            self.ui.pinned_direct_task_ids.insert(task_id.to_string())
        } else {
            self.ui.pinned_direct_task_ids.remove(task_id)
        }
    }

    pub fn set_worktree_pinned(&mut self, project_id: &str, pinned: bool) -> bool {
        if pinned {
            self.ui
                .pinned_worktree_project_ids
                .insert(project_id.to_string())
        } else {
            self.ui.pinned_worktree_project_ids.remove(project_id)
        }
    }

    pub fn insert_project(&mut self, project: Project) -> bool {
        if self
            .projects
            .iter()
            .any(|existing| existing.path == project.path)
        {
            return false;
        }

        self.projects.push(project);
        self.save();
        true
    }

    /// Persist current state to disk.
    #[hotpath::measure]
    pub fn save(&self) {
        let store = StoreFile {
            projects: self.projects.clone(),
            direct_tasks: self.direct_tasks.clone(),
            worktree_task_names: self.worktree_task_names.clone(),
            terminal_sections: self.terminal_sections.clone(),
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

    pub fn set_expanded_projects(&mut self, expanded_project_ids: &HashSet<String>) {
        self.ui.expanded_project_ids = Some(expanded_project_ids.clone());
        self.save();
    }

    pub fn set_last_active_section_key(&mut self, section_key: Option<String>) {
        if self.ui.last_active_section_key == section_key {
            return;
        }

        self.ui.last_active_section_key = section_key;
        self.save();
    }

    pub fn set_terminal_section(
        &mut self,
        section_key: impl Into<String>,
        state: PersistedSectionState,
    ) {
        self.terminal_sections.insert(section_key.into(), state);
        self.save();
    }

    pub fn remove_terminal_section(&mut self, section_key: &str) -> bool {
        let removed = self.terminal_sections.remove(section_key).is_some();
        if removed {
            if self.ui.last_active_section_key.as_deref() == Some(section_key) {
                self.ui.last_active_section_key = None;
            }
            self.save();
        }
        removed
    }

    pub fn remove_terminal_sections(&mut self, section_keys: &HashSet<String>) -> bool {
        let before = self.terminal_sections.len();
        self.terminal_sections
            .retain(|section_key, _| !section_keys.contains(section_key));
        let changed = before != self.terminal_sections.len();
        if changed {
            if self
                .ui
                .last_active_section_key
                .as_ref()
                .is_some_and(|section_key| section_keys.contains(section_key))
            {
                self.ui.last_active_section_key = None;
            }
            self.save();
        }
        changed
    }

    // --- private helpers ---

    fn config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("another-one").join("projects.json")
    }

    fn legacy_config_path() -> PathBuf {
        let base = dirs::config_dir().unwrap_or_else(|| PathBuf::from("."));
        base.join("three-column").join("projects.json")
    }

    fn read_from_disk(path: &Path) -> StoreFile {
        let fallback_path = Self::legacy_config_path();
        let read_path = if path.exists() {
            path
        } else if fallback_path.exists() {
            fallback_path.as_path()
        } else {
            path
        };

        match std::fs::read_to_string(read_path) {
            Ok(contents) => serde_json::from_str::<StoreFile>(&contents).unwrap_or_default(),
            Err(_) => StoreFile::default(),
        }
    }
}

pub fn prepare_project(folder: &Path) -> Result<Project, String> {
    let canonical = folder
        .canonicalize()
        .unwrap_or_else(|_| folder.to_path_buf());

    let name = canonical
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| canonical.display().to_string());

    Ok(Project {
        id: uuid::Uuid::new_v4().to_string(),
        name,
        path: canonical.clone(),
        settings: ProjectSettings::default(),
        worktrees: Vec::new(),
        branches: detect_branches(&canonical),
        worktree_name: detect_worktree_name(&canonical),
        repo_common_dir: detect_repo_common_dir(&canonical),
    })
}

#[hotpath::measure]
pub fn read_project_git_state(path: &Path, include_metadata: bool) -> ProjectGitState {
    ProjectGitState {
        changed_files: list_changed_files(path),
        ahead_count: git_current_branch(path)
            .map(|branch| git_ahead_count(path, &branch))
            .unwrap_or(0),
        metadata: include_metadata.then(|| read_project_git_metadata(path)),
    }
}

fn read_project_git_metadata(path: &Path) -> ProjectGitMetadata {
    ProjectGitMetadata {
        branches: detect_branches(path),
        worktree_name: detect_worktree_name(path),
        repo_common_dir: detect_repo_common_dir(path),
    }
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

    use crate::agents::{AgentProviderKind, ResumeTarget, TerminalLaunchKind};

    use super::{
        format_git_command_error, DirectTask, PersistedSectionState, PersistedTerminalTab, Project,
        ProjectSettings, StoreFile,
    };

    fn sample_project(id: &str, worktree_name: Option<&str>) -> Project {
        Project {
            id: id.to_string(),
            name: format!("Project {id}"),
            path: PathBuf::from(format!("/tmp/{id}")),
            settings: ProjectSettings::default(),
            worktrees: Vec::new(),
            branches: Vec::new(),
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
    fn store_file_defaults_new_sidebar_fields_for_old_json() {
        let store: StoreFile =
            serde_json::from_str(r#"{"projects":[{"id":"p1","name":"Project","path":"/tmp/p1"}]}"#)
                .expect("old store JSON should still deserialize");

        assert_eq!(store.projects.len(), 1);
        assert!(store.direct_tasks.is_empty());
        assert!(store.worktree_task_names.is_empty());
        assert!(store.terminal_sections.is_empty());
        assert!(store.ui.left_sidebar_open);
        assert_eq!(store.ui.expanded_project_ids, None);
        assert!(store.ui.pinned_direct_task_ids.is_empty());
        assert!(store.ui.pinned_worktree_project_ids.is_empty());
        assert_eq!(store.ui.last_active_section_key, None);
    }

    #[test]
    fn store_file_round_trip_preserves_sidebar_task_state() {
        let store = StoreFile {
            projects: vec![
                sample_project("root", None),
                sample_project("wt1", Some("wt1")),
            ],
            direct_tasks: HashMap::from([(
                "root".to_string(),
                vec![DirectTask {
                    id: "task-1".to_string(),
                    name: "Investigate bug".to_string(),
                    branch_name: "feature/persist-tasks".to_string(),
                }],
            )]),
            worktree_task_names: HashMap::from([(
                "wt1".to_string(),
                "Friendly worktree name".to_string(),
            )]),
            terminal_sections: HashMap::from([
                (
                    "root::main::".to_string(),
                    PersistedSectionState {
                        active_tab_id: 1,
                        next_tab_id: 3,
                        cwd: Some(PathBuf::from("/tmp/root")),
                        tabs: vec![
                            PersistedTerminalTab {
                                id: 0,
                                title: "Terminal".to_string(),
                                kind: TerminalLaunchKind::Shell,
                                provider: None,
                                launch_argv: Vec::new(),
                                resume_target: None,
                            },
                            PersistedTerminalTab {
                                id: 1,
                                title: "Claude Code".to_string(),
                                kind: TerminalLaunchKind::Agent,
                                provider: Some(AgentProviderKind::ClaudeCode),
                                launch_argv: vec![
                                    "claude".to_string(),
                                    "--resume".to_string(),
                                    "session-123".to_string(),
                                ],
                                resume_target: Some(ResumeTarget::id("session-123")),
                            },
                        ],
                    },
                ),
                (
                    "wt1::feature/worktree::".to_string(),
                    PersistedSectionState {
                        active_tab_id: 0,
                        next_tab_id: 1,
                        cwd: Some(PathBuf::from("/tmp/wt1")),
                        tabs: vec![PersistedTerminalTab {
                            id: 0,
                            title: "Pi".to_string(),
                            kind: TerminalLaunchKind::Agent,
                            provider: Some(AgentProviderKind::Pi),
                            launch_argv: vec![
                                "pi".to_string(),
                                "--session".to_string(),
                                "/tmp/pi-session.jsonl".to_string(),
                            ],
                            resume_target: Some(ResumeTarget::path("/tmp/pi-session.jsonl")),
                        }],
                    },
                ),
            ]),
            ui: super::UiState {
                left_sidebar_open: false,
                expanded_project_ids: Some(HashSet::from(["root".to_string(), "wt1".to_string()])),
                pinned_direct_task_ids: HashSet::from(["task-1".to_string()]),
                pinned_worktree_project_ids: HashSet::from(["wt1".to_string()]),
                last_active_section_key: Some("root::main::".to_string()),
            },
        };

        let json = serde_json::to_string(&store).expect("store JSON should serialize");
        let round_trip: StoreFile =
            serde_json::from_str(&json).expect("store JSON should deserialize");

        assert_eq!(round_trip.projects.len(), 2);
        assert_eq!(
            round_trip
                .direct_tasks
                .get("root")
                .expect("direct tasks should round-trip")[0]
                .name,
            "Investigate bug"
        );
        assert_eq!(
            round_trip
                .worktree_task_names
                .get("wt1")
                .expect("worktree task name should round-trip"),
            "Friendly worktree name"
        );
        assert_eq!(
            round_trip
                .terminal_sections
                .get("root::main::")
                .expect("root terminal state should round-trip")
                .active_tab_id,
            1
        );
        assert_eq!(
            round_trip
                .terminal_sections
                .get("wt1::feature/worktree::")
                .expect("worktree terminal state should round-trip")
                .tabs[0]
                .resume_target,
            Some(ResumeTarget::path("/tmp/pi-session.jsonl"))
        );
        assert!(!round_trip.ui.left_sidebar_open);
        assert_eq!(
            round_trip.ui.expanded_project_ids,
            Some(HashSet::from(["root".to_string(), "wt1".to_string()]))
        );
        assert_eq!(
            round_trip.ui.pinned_direct_task_ids,
            HashSet::from(["task-1".to_string()])
        );
        assert_eq!(
            round_trip.ui.pinned_worktree_project_ids,
            HashSet::from(["wt1".to_string()])
        );
        assert_eq!(
            round_trip.ui.last_active_section_key.as_deref(),
            Some("root::main::")
        );
    }
}
