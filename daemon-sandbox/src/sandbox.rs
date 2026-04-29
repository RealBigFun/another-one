//! `DaemonRegistry` impl used by the standalone `daemon-sandbox`
//! binary. It reads the same project store as the desktop app for
//! project list/mutation, and keeps a single fake shell project as a
//! fallback when no real projects exist. The first `attach_tab` call
//! for that fallback spawns a bash PTY, subsequent attaches return a
//! fresh receiver on the same broadcast.
//!
//! The desktop crate supplies its *own* `DaemonRegistry` impl that
//! wraps the running `AnotherOneApp`. This module is not linked into
//! that path.

use std::sync::{Arc, Mutex};

use portable_pty::MasterPty;
use tokio::sync::broadcast;
use tracing::{debug, warn};

use another_one_core::agents::AgentProviderKind;
use another_one_core::process::TrackedProcess;
use another_one_core::project_store::{
    prepare_project, ProjectKind as CoreProjectKind, ProjectStore,
};
use another_one_core::resource_usage::ResourceUsageSampler;

use crate::frame::{
    AgentProvider, ChangedFileWire, ProjectKind, ProjectSummary, ResourceUsageSnapshotWire,
    TabSummary, TaskSummary,
};
use crate::pty::PtySession;
use crate::registry::{DaemonRegistry, RegistryFuture};

const SANDBOX_PROJECT_ID: &str = "sandbox";
const SANDBOX_TASK_ID: &str = "sandbox-task";
const SANDBOX_SECTION_ID: &str = "sandbox-section";
const SANDBOX_TAB_ID: &str = "sandbox-tab";

struct Shell {
    tx: broadcast::Sender<Vec<u8>>,
    writer: Arc<Mutex<Box<dyn std::io::Write + Send>>>,
    master: Arc<Mutex<Box<dyn MasterPty + Send>>>,
    process_id: Option<u32>,
}

pub struct SandboxRegistry {
    shell: Mutex<Option<Shell>>,
    resource_sampler: Mutex<ResourceUsageSampler>,
}

impl SandboxRegistry {
    pub fn new() -> Self {
        Self {
            shell: Mutex::new(None),
            resource_sampler: Mutex::new(ResourceUsageSampler::default()),
        }
    }

    /// Lazily spawn the singleton PTY on first attach.
    fn ensure_shell(&self) -> anyhow::Result<broadcast::Sender<Vec<u8>>> {
        let mut guard = self.shell.lock().unwrap();
        if let Some(shell) = guard.as_ref() {
            return Ok(shell.tx.clone());
        }

        let session = PtySession::spawn(80, 24)?;
        let (tx, _rx0) = broadcast::channel::<Vec<u8>>(64);
        let tx_for_pump = tx.clone();
        let mut output_rx = session.output_rx;
        // Pump the PTY's mpsc output into the broadcast so all
        // mobile subscribers see the same bytes.
        tokio::spawn(async move {
            while let Some(bytes) = output_rx.recv().await {
                let _ = tx_for_pump.send(bytes);
            }
        });

        *guard = Some(Shell {
            tx: tx.clone(),
            writer: Arc::new(Mutex::new(session.master_writer)),
            master: Arc::new(Mutex::new(session.master)),
            process_id: session.process_id,
        });
        Ok(tx)
    }

    fn tracked_processes(&self) -> Vec<TrackedProcess> {
        let guard = self.shell.lock().unwrap();
        let Some(shell) = guard.as_ref() else {
            return Vec::new();
        };
        let Some(pid) = shell.process_id else {
            return Vec::new();
        };
        vec![TrackedProcess {
            pid,
            key: format!("resource-session:sandbox:{pid}"),
            label: "bash".to_string(),
            project_key: format!("resource-project:{SANDBOX_PROJECT_ID}"),
            project_label: "Sandbox".to_string(),
            task_key: format!("resource-task:{SANDBOX_TASK_ID}"),
            task_label: "shell".to_string(),
            icon_path: "assets/icons/icons__terminal.svg",
        }]
    }
}

impl Default for SandboxRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl DaemonRegistry for SandboxRegistry {
    fn list_projects(&self) -> Vec<ProjectSummary> {
        let projects = project_summaries_from_store(&ProjectStore::load());
        if projects.is_empty() {
            vec![sandbox_project_summary()]
        } else {
            projects
        }
    }

    fn attach_tab(&self, section_id: &str, tab_id: &str) -> Option<broadcast::Receiver<Vec<u8>>> {
        if section_id != SANDBOX_SECTION_ID || tab_id != SANDBOX_TAB_ID {
            debug!(section_id, tab_id, "sandbox: unknown section/tab");
            return None;
        }
        match self.ensure_shell() {
            Ok(tx) => Some(tx.subscribe()),
            Err(e) => {
                warn!(error = %e, "sandbox: shell spawn failed");
                None
            }
        }
    }

    fn tab_input(&self, section_id: &str, tab_id: &str, bytes: &[u8]) {
        if section_id != SANDBOX_SECTION_ID || tab_id != SANDBOX_TAB_ID {
            return;
        }
        let guard = self.shell.lock().unwrap();
        let Some(shell) = guard.as_ref() else { return };
        let mut writer = shell.writer.lock().unwrap();
        let _ = writer.write_all(bytes);
        let _ = writer.flush();
    }

    fn tab_resize(&self, _viewer_id: &str, section_id: &str, tab_id: &str, cols: u16, rows: u16) {
        // Sandbox has a single shell and a single viewer in practice;
        // min-across-viewers collapses to "whatever arrived last".
        if section_id != SANDBOX_SECTION_ID || tab_id != SANDBOX_TAB_ID {
            return;
        }
        let guard = self.shell.lock().unwrap();
        let Some(shell) = guard.as_ref() else { return };
        let master = shell.master.lock().unwrap();
        let _ = master.resize(portable_pty::PtySize {
            cols,
            rows,
            pixel_width: 0,
            pixel_height: 0,
        });
    }

    fn read_changed_files(&self, project_id: &str) -> Option<Vec<ChangedFileWire>> {
        if project_id != SANDBOX_PROJECT_ID {
            return None;
        }
        let cwd = std::env::current_dir().ok()?;
        Some(
            another_one_core::project_store::list_changed_files(&cwd)
                .into_iter()
                .map(|file| ChangedFileWire {
                    path: file.path,
                    original_path: file.original_path,
                    staged_additions: file.staged_additions,
                    staged_deletions: file.staged_deletions,
                    unstaged_additions: file.unstaged_additions,
                    unstaged_deletions: file.unstaged_deletions,
                    index_status: file.index_status.to_string(),
                    worktree_status: file.worktree_status.to_string(),
                    untracked: file.untracked,
                })
                .collect(),
        )
    }

    fn add_project<'a>(
        &'a self,
        path: String,
    ) -> RegistryFuture<'a, anyhow::Result<ProjectSummary>> {
        Box::pin(async move {
            tokio::task::spawn_blocking(move || add_project_blocking(path))
                .await
                .map_err(|err| anyhow::anyhow!("add project worker failed: {err}"))?
        })
    }

    fn remove_project(&self, project_id: &str) -> anyhow::Result<()> {
        if project_id == SANDBOX_PROJECT_ID {
            return Err(anyhow::anyhow!("cannot remove sandbox fallback project"));
        }
        let mut store = ProjectStore::load();
        store.remove_project(project_id);
        Ok(())
    }

    fn read_resource_usage(&self, app_pid: u32) -> ResourceUsageSnapshotWire {
        let tracked_processes = self.tracked_processes();
        let mut sampler = self.resource_sampler.lock().unwrap();
        sampler.sample(app_pid, &tracked_processes).into()
    }
}

fn add_project_blocking(path: String) -> anyhow::Result<ProjectSummary> {
    let prepared = prepare_project(std::path::Path::new(&path)).map_err(anyhow::Error::msg)?;
    let project_id = prepared.project.id.clone();
    let project_name = prepared.project.name.clone();

    let mut store = ProjectStore::load();
    if !store.insert_prepared_project(prepared) {
        anyhow::bail!("{project_name} is already in the sidebar.");
    }
    let store = ProjectStore::load();

    project_summaries_from_store(&store)
        .into_iter()
        .find(|project| project.id == project_id)
        .ok_or_else(|| anyhow::anyhow!("project was added but is not visible in the sidebar"))
}

fn project_summaries_from_store(store: &ProjectStore) -> Vec<ProjectSummary> {
    store
        .projects
        .iter()
        .filter(|project| matches!(project.kind, CoreProjectKind::Root))
        .map(|project| {
            let tasks = store
                .tasks
                .get(&project.id)
                .cloned()
                .unwrap_or_default()
                .into_iter()
                .map(|task| task_to_summary(store, task))
                .collect();
            ProjectSummary {
                id: project.id.clone(),
                name: project.name.clone(),
                path: project.path.to_string_lossy().into_owned(),
                kind: map_project_kind(project.kind),
                current_branch: project.checkout.current_branch.clone(),
                tasks,
            }
        })
        .collect()
}

fn task_to_summary(
    store: &ProjectStore,
    task: another_one_core::project_store::Task,
) -> TaskSummary {
    let task_pinned = store.ui.pinned_task_ids.contains(&task.id);
    let tabs = task
        .tabs
        .into_iter()
        .map(|tab| TabSummary {
            id: tab.id,
            title: tab.title,
            provider: tab.provider.map(map_agent_provider),
            running: false,
            pinned: tab.pinned,
            fixed_title: tab.fixed_title,
            restore_status: tab.restore_status,
            failure_message: tab.failure_message,
            failure_details: tab.failure_details,
        })
        .collect();
    let branch_view = store.branch_view(&task.target_project_id, &task.branch_name);
    let last_commit_relative = branch_view
        .as_ref()
        .map(|branch| branch.last_commit_relative.clone())
        .unwrap_or_default();
    let (lines_added, lines_removed) = branch_view
        .map(|branch| (branch.lines_added, branch.lines_removed))
        .unwrap_or((0, 0));
    TaskSummary {
        id: task.id,
        name: task.name,
        section_id: task.section_id,
        branch_name: task.branch_name,
        active_tab_id: task.active_tab_id,
        tabs,
        pinned: task_pinned,
        last_commit_relative,
        lines_added,
        lines_removed,
        target_project_id: task.target_project_id,
    }
}

fn sandbox_project_summary() -> ProjectSummary {
    ProjectSummary {
        id: SANDBOX_PROJECT_ID.to_string(),
        name: "Sandbox".to_string(),
        path: "/".to_string(),
        kind: ProjectKind::Root,
        current_branch: None,
        tasks: vec![TaskSummary {
            id: SANDBOX_TASK_ID.to_string(),
            name: "shell".to_string(),
            section_id: SANDBOX_SECTION_ID.to_string(),
            branch_name: String::new(),
            active_tab_id: SANDBOX_TAB_ID.to_string(),
            tabs: vec![TabSummary {
                id: SANDBOX_TAB_ID.to_string(),
                title: "bash".to_string(),
                provider: Some(AgentProvider::Shell),
                running: true,
                pinned: false,
                fixed_title: None,
                restore_status: another_one_core::agents::TerminalRestoreStatus::Ready,
                failure_message: None,
                failure_details: None,
            }],
            pinned: false,
            last_commit_relative: String::new(),
            lines_added: 0,
            lines_removed: 0,
            target_project_id: SANDBOX_PROJECT_ID.to_string(),
        }],
    }
}

fn map_project_kind(kind: CoreProjectKind) -> ProjectKind {
    match kind {
        CoreProjectKind::Root => ProjectKind::Root,
        CoreProjectKind::Worktree => ProjectKind::Worktree,
    }
}

fn map_agent_provider(kind: AgentProviderKind) -> AgentProvider {
    match kind {
        AgentProviderKind::ClaudeCode => AgentProvider::ClaudeCode,
        AgentProviderKind::CursorAgent => AgentProvider::CursorAgent,
        AgentProviderKind::Codex => AgentProvider::Codex,
        AgentProviderKind::Pi => AgentProvider::Pi,
        AgentProviderKind::Gemini => AgentProvider::Gemini,
        AgentProviderKind::OpenCode => AgentProvider::OpenCode,
        AgentProviderKind::Amp => AgentProvider::Amp,
        AgentProviderKind::RovoDev => AgentProvider::RovoDev,
        AgentProviderKind::Forge => AgentProvider::Forge,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_project_persists_to_project_store() {
        let previous_config_home = std::env::var_os("XDG_CONFIG_HOME");
        let config_home = tempfile::tempdir().expect("temp config home");
        let project_dir = tempfile::tempdir().expect("temp project dir");
        std::env::set_var("XDG_CONFIG_HOME", config_home.path());

        let project_path = project_dir.path().to_string_lossy().into_owned();
        let summary = add_project_blocking(project_path.clone()).expect("add project");
        let canonical_path = project_dir
            .path()
            .canonicalize()
            .expect("canonical project path")
            .to_string_lossy()
            .into_owned();

        assert_eq!(summary.path, canonical_path);
        assert!(project_summaries_from_store(&ProjectStore::load())
            .iter()
            .any(|project| project.id == summary.id && project.path == canonical_path));

        let duplicate_error = add_project_blocking(project_path)
            .expect_err("duplicate project should be rejected")
            .to_string();
        assert!(duplicate_error.contains("already in the sidebar"));

        if let Some(previous_config_home) = previous_config_home {
            std::env::set_var("XDG_CONFIG_HOME", previous_config_home);
        } else {
            std::env::remove_var("XDG_CONFIG_HOME");
        }
    }
}
