//! Boot the embedded daemon in-process (Flutter desktop only).
//!
//! Replaces the GPUI binary's `desktop::daemon_host` for the future
//! Flutter desktop. The bridge:
//!
//!   1. Loads the on-disk `ProjectStore` and constructs a
//!      [`RegistryState`].
//!   2. Wraps it in a `BridgeDaemonRegistry` that mirrors
//!      `desktop::DesktopTerminalRegistry`'s semantics but reads
//!      from the bridge's own state (no `AnotherOneApp` to project
//!      from).
//!   3. Spawns a dedicated OS thread with a tokio runtime, blocks on
//!      [`daemon_sandbox::run_endpoint`], and on success registers
//!      the resulting [`EndpointHandle`] with both
//!      [`crate::local_pair`] and [`crate::local_registry`] so
//!      `LocalSession` + the pair-mobile FRB API can read from
//!      it.
//!
//! Idempotent at the registration layer: both seams use
//! `OnceLock`, so a second `boot_embedded_daemon` call from Dart is
//! a no-op (we early-out before spawning the thread).
//!
//! MCP transport is not started here — the GPUI desktop owns that
//! today. Phase 6 of the migration will move MCP wiring into core
//! and fold it back in.

use std::io::Write;
use std::sync::{Arc, Mutex, OnceLock, Weak};
use std::thread;

use tokio::sync::broadcast;

use another_one_core::agents::AgentProviderKind;
use another_one_core::daemon_embed::{
    daemon_paths, key_from_wire, RegistryState, TabLaunchRequest,
};
use another_one_core::project_store::ProjectKind as CoreProjectKind;
use another_one_core::project_store::ProjectStore;
use another_one_core::section::SectionId;
use another_one_core::terminal_types::TerminalRuntimeKey;

use daemon_sandbox::frame::{
    ActiveGitStateWire, AgentProvider, ChangedFileWire, CommitWire, ProjectKind, ProjectSummary,
    RecentCommitsWire, TabSummary, TaskSummary,
};
use daemon_sandbox::registry::RegistryFuture;
use daemon_sandbox::{DaemonRegistry, EndpointHandle};

use crate::local_pair::{set_local_pair_info, LocalPairInfo};
use crate::local_registry::set_local_registry;

/// Tracks whether the embedded daemon has been booted in this
/// process. `OnceLock` so two concurrent `boot_embedded_daemon`
/// calls from Dart resolve to the same boot.
static BOOTED: OnceLock<()> = OnceLock::new();

/// Build, register, and boot the embedded daemon. Idempotent: a
/// second call no-ops. Returns as soon as the registry is wired and
/// the daemon thread is spawned; the endpoint handshake completes
/// asynchronously on its own runtime, so [`crate::api::pair::pairing_info`]
/// may return `None` for a few hundred milliseconds after this
/// returns. The pair-mobile UI's empty state covers that window.
pub(crate) fn boot() -> Result<(), String> {
    if BOOTED.get().is_some() {
        return Ok(());
    }

    let store = ProjectStore::load();
    let registry_state = Arc::new(Mutex::new(RegistryState::new(store)));
    set_local_registry(registry_state.clone());

    crate::pty_drain::spawn_drain(registry_state.clone());

    thread::Builder::new()
        .name("another-one-embedded-daemon".into())
        .spawn(move || run(registry_state))
        .map_err(|e| format!("spawn embedded daemon thread: {e}"))?;

    // Mark booted now — even if the endpoint handshake later fails,
    // we don't want a second `boot_embedded_daemon` call to spawn a
    // duplicate registry. The Dart UI surfaces "daemon not ready"
    // for as long as `local_pair_info()` is unset.
    let _ = BOOTED.set(());
    Ok(())
}

fn run(registry_state: Arc<Mutex<RegistryState>>) {
    // Mirrors `desktop::daemon_host::run`. Four workers: a single
    // stuck PTY write under `block_in_place` shouldn't be able to
    // starve the accept loop + writers + forwarders all at once.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .thread_name("another-one-embedded-rt")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            tracing::error!("build embedded daemon runtime: {e:#}");
            return;
        }
    };

    let weak = Arc::downgrade(&registry_state);
    drop(registry_state);
    let registry: Arc<dyn DaemonRegistry> = Arc::new(BridgeDaemonRegistry::new(weak));

    let paths = match daemon_paths() {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("resolve daemon paths: {e:#}");
            return;
        }
    };

    let endpoint_result = runtime.block_on(async {
        daemon_sandbox::run_endpoint(registry, paths.secret_key, paths.paired_peers).await
    });

    match endpoint_result {
        Ok(handle) => {
            let adapter: Arc<dyn LocalPairInfo> =
                Arc::new(EndpointHandlePairAdapter::new(Arc::new(handle)));
            set_local_pair_info(adapter);
            // Park forever — the handle stays in `set_local_pair_info`'s
            // `OnceLock` for the rest of the process. Dropping the
            // runtime would tear down the endpoint.
            loop {
                thread::park();
            }
        }
        Err(e) => {
            tracing::error!("embedded daemon boot failed: {e:#}");
        }
    }
}

/// Adapts an `EndpointHandle` to `LocalPairInfo`. Splitting the
/// trait off the concrete handle is the seam that lets the bridge
/// expose pair info without leaking `daemon_sandbox` types into
/// `crate::local_pair` (which `api/pair.rs` depends on).
struct EndpointHandlePairAdapter {
    handle: Arc<EndpointHandle>,
}

impl EndpointHandlePairAdapter {
    fn new(handle: Arc<EndpointHandle>) -> Self {
        Self { handle }
    }
}

impl LocalPairInfo for EndpointHandlePairAdapter {
    fn pairing_url(&self) -> String {
        self.handle.pairing_url()
    }

    fn qr_png_bytes(&self) -> Vec<u8> {
        self.handle.qr_png_bytes()
    }

    fn regenerate_pairing(&self) -> Result<(), String> {
        self.handle
            .regenerate_pairing()
            .map_err(|e| format!("{e:#}"))
    }
}

/// `DaemonRegistry` impl that operates directly on the bridge's
/// `RegistryState`. Mirrors `desktop::DesktopTerminalRegistry` but
/// without the desktop's project-summary projection logic — that
/// will return when the Flutter desktop port owns the project tree
/// directly. For now `list_projects` flattens the in-memory store
/// the same way `LocalSession::list_projects` does (see
/// `api/local_session.rs::flatten_project_store`).
struct BridgeDaemonRegistry {
    inner: Weak<Mutex<RegistryState>>,
}

impl BridgeDaemonRegistry {
    fn new(inner: Weak<Mutex<RegistryState>>) -> Self {
        Self { inner }
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut RegistryState) -> R) -> Option<R> {
        let arc = self.inner.upgrade()?;
        let mut guard = arc.lock().ok()?;
        Some(f(&mut guard))
    }
}

impl DaemonRegistry for BridgeDaemonRegistry {
    fn list_projects(&self) -> Vec<ProjectSummary> {
        // Project flattening mirrors `LocalSession::list_projects`'s
        // `flatten_project_store`. Worktree-kind projects collapse
        // into their root via `Task::target_project_id`; mobile sees
        // the same tree the desktop sidebar does.
        self.with_state(|state| flatten_state_to_frame(state))
            .unwrap_or_default()
    }

    fn attach_tab(
        &self,
        section_id: &str,
        tab_id: &str,
    ) -> Option<broadcast::Receiver<Vec<u8>>> {
        let key = key_from_wire(section_id, tab_id)?;
        self.with_state(|state| state.broadcasts.get(&key).map(|tx| tx.subscribe()))
            .flatten()
    }

    fn tab_input(&self, section_id: &str, tab_id: &str, bytes: &[u8]) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        // Clone writer Arc out, drop state lock, then write — same
        // rationale as desktop::DesktopTerminalRegistry::tab_input.
        let writer = self
            .with_state(|state| state.writers.get(&key).cloned())
            .flatten();
        let Some(writer) = writer else { return };
        tokio::task::block_in_place(|| {
            let mut guard = match writer.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let _ = guard.write_all(bytes);
            let _ = guard.flush();
        });
    }

    fn tab_resize(
        &self,
        viewer_id: &str,
        section_id: &str,
        tab_id: &str,
        cols: u16,
        rows: u16,
    ) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        self.with_state(|state| {
            if let Some(old_key) = state.viewer_focus.get(viewer_id).cloned() {
                if old_key != key {
                    if let Some(map) = state.active_viewers.get_mut(&old_key) {
                        map.remove(viewer_id);
                        if map.is_empty() {
                            state.active_viewers.remove(&old_key);
                            state.effective_sizes.remove(&old_key);
                        }
                    }
                    state.recompute_effective_size(&old_key);
                }
            }
            state
                .active_viewers
                .entry(key.clone())
                .or_default()
                .insert(viewer_id.to_string(), (cols, rows));
            state
                .viewer_focus
                .insert(viewer_id.to_string(), key.clone());
            state.recompute_effective_size(&key);
        });
    }

    fn viewer_disconnected(&self, viewer_id: &str) {
        self.with_state(|state| {
            state.viewer_focus.remove(viewer_id);
            let touched: Vec<TerminalRuntimeKey> = state
                .active_viewers
                .iter_mut()
                .filter_map(|(key, map)| {
                    if map.remove(viewer_id).is_some() {
                        Some(key.clone())
                    } else {
                        None
                    }
                })
                .collect();
            for key in touched {
                let empty = state
                    .active_viewers
                    .get(&key)
                    .map(|m| m.is_empty())
                    .unwrap_or(true);
                if empty {
                    state.active_viewers.remove(&key);
                    state.effective_sizes.remove(&key);
                } else {
                    state.recompute_effective_size(&key);
                }
            }
        });
    }

    fn launch_tab(&self, section_id: &str, tab_id: &str) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        self.with_state(|state| {
            if state.broadcasts.contains_key(&key) {
                return;
            }
            if state.in_flight_launches.contains(&key) {
                return;
            }
            if state.pending_tab_launches.iter().any(|r| r.key == key) {
                return;
            }
            state.pending_tab_launches.push(TabLaunchRequest { key });
        });
    }

    // ── Task mutation (another-one-ojm.3) ─────────────────────────
    //
    // Mirror of `LocalSession::{create_worktree_task, rename_task,
    // set_task_pinned, remove_task}` in `api/local_session.rs`. The
    // delegating shape is intentional: same registry-locking pattern,
    // same core-service spawn, same persistence rules — both transports
    // converge on identical store mutations.

    fn create_worktree_task(
        &self,
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_provider: Option<AgentProvider>,
    ) -> RegistryFuture<'_, anyhow::Result<TaskSummary>> {
        let weak = self.inner.clone();
        Box::pin(async move {
            // Resolve project metadata up front so we can fail clearly
            // before spawning the worker thread.
            let (project_path, project_name, target_project_id) = {
                let arc = weak.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_worktree_task: registry state dropped")
                })?;
                let state = arc.lock().map_err(|_| {
                    anyhow::anyhow!("create_worktree_task: RegistryState mutex poisoned")
                })?;
                let project = state
                    .project_store
                    .project(&project_id)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "create_worktree_task: unknown project_id `{project_id}`"
                        )
                    })?;
                (
                    project.path.clone(),
                    project.name.clone(),
                    project.id.clone(),
                )
            };

            let trimmed = task_name.trim().to_string();
            if trimmed.is_empty() {
                anyhow::bail!("create_worktree_task: task_name must not be blank");
            }
            let generated = trimmed.clone();

            let launch_config = match agent_provider.map(map_agent_provider_back) {
                Some(provider) => {
                    another_one_core::agents::TerminalLaunchConfig::for_provider(provider)
                }
                None => another_one_core::agents::TerminalLaunchConfig::default(),
            };
            let branch_mode =
                another_one_core::project_store::TaskWorktreeBranchMode::NewBranchFrom {
                    source_branch,
                };

            let mut rx = another_one_core::project_service::spawn_task_creation(
                target_project_id.clone(),
                project_path,
                project_name,
                trimmed,
                generated,
                branch_mode,
                launch_config,
            );
            let reply = rx
                .recv()
                .await
                .map_err(|_| anyhow::anyhow!("task creation worker dropped"))?;
            let success = reply
                .result
                .map_err(|f| anyhow::anyhow!("create task: {}", f.message))?;

            // Insert the prepared worktree project + the task under one
            // lock so the inline-snapshot reply observes both.
            let summary = {
                let arc = weak.upgrade().ok_or_else(|| {
                    anyhow::anyhow!("create_worktree_task: registry state dropped after worker")
                })?;
                let mut state = arc.lock().map_err(|_| {
                    anyhow::anyhow!("create_worktree_task: registry mutex poisoned")
                })?;
                let inserted_worktree = state
                    .project_store
                    .insert_prepared_project(success.project.clone());
                let worktree_project_id = if inserted_worktree {
                    success.project.project.id.clone()
                } else {
                    state
                        .project_store
                        .projects
                        .iter()
                        .find(|p| p.path == success.project.project.path)
                        .map(|p| p.id.clone())
                        .unwrap_or_else(|| success.project.project.id.clone())
                };
                let task_id = uuid::Uuid::new_v4().to_string();
                let section = another_one_core::section::SectionId::for_task(
                    &worktree_project_id,
                    &success.branch_name,
                    &task_id,
                );
                let section_key = section.store_key();
                state
                    .project_store
                    .insert_task(another_one_core::project_store::Task {
                        id: task_id.clone(),
                        name: success.task_name,
                        kind: another_one_core::project_store::TaskKind::Worktree,
                        root_project_id: target_project_id,
                        target_project_id: worktree_project_id.clone(),
                        branch_name: success.branch_name,
                        section_id: section_key,
                        worktree_project_id: Some(worktree_project_id),
                        tabs: Vec::new(),
                        active_tab_id: String::new(),
                        next_tab_id: 0,
                        cwd: None,
                    });
                state.project_store.save();
                lookup_task_summary(&state, &task_id).ok_or_else(|| {
                    anyhow::anyhow!("create_worktree_task: task vanished after insert")
                })?
            };
            Ok(summary)
        })
    }

    fn rename_task(&self, task_id: &str, new_name: &str) -> (bool, Option<TaskSummary>) {
        let trimmed = new_name.trim().to_string();
        if trimmed.is_empty() {
            // Reject blank renames daemon-side, same as LocalSession.
            // Return the existing snapshot so the issuer can render
            // the old name in its UI without a follow-up read.
            return self
                .with_state(|state| (false, lookup_task_summary(state, task_id)))
                .unwrap_or((false, None));
        }
        self.with_state(|state| {
            let Some(task) = state.project_store.task_mut(task_id) else {
                return (false, None);
            };
            let changed = if task.name == trimmed {
                false
            } else {
                task.name = trimmed;
                true
            };
            if changed {
                state.project_store.save();
            }
            (changed, lookup_task_summary(state, task_id))
        })
        .unwrap_or((false, None))
    }

    fn set_task_pinned(&self, task_id: &str, pinned: bool) -> (bool, Option<TaskSummary>) {
        self.with_state(|state| {
            let changed = state.project_store.set_task_pinned(task_id, pinned);
            if changed {
                state.project_store.save();
            }
            (changed, lookup_task_summary(state, task_id))
        })
        .unwrap_or((false, None))
    }

    fn remove_task(&self, project_id: &str, task_id: &str) -> bool {
        self.with_state(|state| {
            state
                .project_store
                .remove_task(project_id, task_id)
                .is_some()
        })
        .unwrap_or(false)
    }

    // ── Project mutation (another-one-ojm.2) ──────────────────────

    /// Mirror of `LocalSession::add_project`: run `prepare_project`
    /// on a background thread (via `spawn_project_add`), then take
    /// the registry lock, insert, and project the post-mutation
    /// snapshot under the same lock so a concurrent removal can't
    /// race in between insertion and projection.
    fn add_project<'a>(
        &'a self,
        path: String,
    ) -> RegistryFuture<'a, anyhow::Result<ProjectSummary>> {
        Box::pin(async move {
            let mut rx = another_one_core::project_service::spawn_project_add(
                std::path::PathBuf::from(path),
            );
            let reply = rx
                .recv()
                .await
                .map_err(|_| anyhow::anyhow!("project add worker dropped"))?;
            let prepared = reply
                .result
                .map_err(|e| anyhow::anyhow!("prepare project: {e}"))?;
            let new_project_id = prepared.project.id.clone();

            let arc = self
                .inner
                .upgrade()
                .ok_or_else(|| anyhow::anyhow!("add_project: registry state dropped"))?;
            let mut guard = arc
                .lock()
                .map_err(|_| anyhow::anyhow!("add_project: RegistryState mutex poisoned"))?;
            let inserted = guard.project_store.insert_prepared_project(prepared);
            if !inserted {
                anyhow::bail!("project at this path already exists");
            }
            let project = guard
                .project_store
                .project(&new_project_id)
                .cloned()
                .ok_or_else(|| {
                    anyhow::anyhow!("add_project: inserted project missing from store")
                })?;
            Ok(project_to_frame(&guard, &project))
        })
    }

    /// Mirror of `LocalSession::remove_project`. Takes the registry
    /// lock and delegates to `project_store.remove_project`, which
    /// cascades to tasks + terminal sections. Idempotent on unknown
    /// ids — same semantics LocalSession exposes today.
    fn remove_project(&self, project_id: &str) -> anyhow::Result<()> {
        let arc = self
            .inner
            .upgrade()
            .ok_or_else(|| anyhow::anyhow!("remove_project: registry state dropped"))?;
        let mut guard = arc
            .lock()
            .map_err(|_| anyhow::anyhow!("remove_project: RegistryState mutex poisoned"))?;
        guard.project_store.remove_project(project_id);
        Ok(())
    }

    // ── Git state read verbs (`another-one-ojm.4`) ─────────────────
    //
    // Mirror of `LocalSession::read_*`/`*_branch_*` methods in
    // `api/local_session.rs`. The shapes are intentionally
    // field-for-field identical — keep them in sync when the FRB DTO
    // is extended.

    fn read_project_branches(&self, project_id: &str) -> Vec<String> {
        self.with_state(|state| state.project_store.branch_names(project_id))
            .unwrap_or_default()
    }

    fn primary_branch_for_project(&self, project_id: &str) -> Option<String> {
        self.with_state(|state| {
            state
                .project_store
                .primary_branch_for_project(project_id, true)
                .map(|branch| branch.name)
        })
        .flatten()
    }

    fn repo_default_commit_action(&self, project_id: &str) -> Option<String> {
        self.with_state(|state| {
            let project = state.project_store.project(project_id)?;
            state
                .project_store
                .repo_default_commit_action(&project.repo_id)
                .map(|a| match a {
                    another_one_core::project_store::RepoDefaultCommitAction::Commit => {
                        "commit".to_string()
                    }
                    another_one_core::project_store::RepoDefaultCommitAction::CommitAndPush => {
                        "commit-and-push".to_string()
                    }
                })
        })
        .flatten()
    }

    fn read_active_git_state(&self, project_id: &str) -> Option<ActiveGitStateWire> {
        let project_path = self.project_path(project_id)?;
        // The git invocation can shell out — wrap in `block_in_place`
        // so the daemon's tokio worker isn't held across the syscall.
        let state = tokio::task::block_in_place(|| {
            another_one_core::project_store::read_project_git_state(&project_path, true)
        });
        Some(ActiveGitStateWire {
            current_branch: state.current_branch,
            ahead_count: state.ahead_count as u32,
            behind_count: state.behind_count as u32,
        })
    }

    fn read_changed_files(&self, project_id: &str) -> Option<Vec<ChangedFileWire>> {
        let project_path = self.project_path(project_id)?;
        let git_state = tokio::task::block_in_place(|| {
            another_one_core::project_store::read_project_git_state(&project_path, false)
        });
        Some(
            git_state
                .changed_files
                .into_iter()
                .map(changed_file_to_wire)
                .collect(),
        )
    }

    fn read_project_github_url(&self, project_id: &str) -> Option<String> {
        let project_path = self.project_path(project_id)?;
        tokio::task::block_in_place(|| {
            another_one_core::git_actions::find_github_repo_url(&project_path)
        })
    }

    fn read_recent_commits(
        &self,
        project_id: &str,
        limit: usize,
    ) -> Result<Option<RecentCommitsWire>, String> {
        let Some(project_path) = self.project_path(project_id) else {
            return Ok(None);
        };
        let result = tokio::task::block_in_place(|| {
            another_one_core::project_store::read_project_branch_commit_state(
                &project_path,
                limit,
            )
        })?;
        Ok(Some(RecentCommitsWire {
            current_branch: result.current_branch,
            has_more: result.has_more,
            commits: result.commits.into_iter().map(commit_to_wire).collect(),
        }))
    }
}

impl BridgeDaemonRegistry {
    /// Resolve a project id to its on-disk path by snapshot of the
    /// in-memory store. Used by every git-state read verb that shells
    /// out — the caller drops the registry lock before the (blocking)
    /// git work, so a hung `git status` doesn't block every other
    /// registry method for the duration.
    fn project_path(&self, project_id: &str) -> Option<std::path::PathBuf> {
        self.with_state(|state| {
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        })
        .flatten()
    }
}

/// Wire `frame::AgentProvider` → core `AgentProviderKind`. Mirror of
/// the same-named helper in `api/local_session.rs`; the wire enum's
/// `Shell` variant has no core counterpart (it represents "no agent,
/// just a shell" — the caller treats `Some(Shell)` like `None`
/// upstream of this fn, but the match is exhaustive).
fn map_agent_provider_back(kind: AgentProvider) -> AgentProviderKind {
    match kind {
        AgentProvider::ClaudeCode => AgentProviderKind::ClaudeCode,
        AgentProvider::CursorAgent => AgentProviderKind::CursorAgent,
        AgentProvider::Codex => AgentProviderKind::Codex,
        AgentProvider::Pi => AgentProviderKind::Pi,
        AgentProvider::Gemini => AgentProviderKind::Gemini,
        AgentProvider::OpenCode => AgentProviderKind::OpenCode,
        AgentProvider::Amp => AgentProviderKind::Amp,
        AgentProvider::RovoDev => AgentProviderKind::RovoDev,
        AgentProvider::Forge => AgentProviderKind::Forge,
        AgentProvider::Shell => AgentProviderKind::ClaudeCode,
    }
}

/// Flatten the bridge's `RegistryState` into the iroh wire's
/// `frame::ProjectSummary` shape. Mirrors `flatten_project_store`
/// in `api/local_session.rs` (which produces the FRB-side
/// `ProjectSummary`); the two namespaces are field-for-field
/// compatible. Worktree-kind projects collapse into their root via
/// `Task::target_project_id`, so mobile peers see the same tree
/// the desktop sidebar paints.
fn flatten_state_to_frame(state: &RegistryState) -> Vec<ProjectSummary> {
    let store = &state.project_store;
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
                .map(|task| task_to_summary(state, task))
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

/// Project a single owned `core::project_store::Task` into the iroh
/// wire's [`TaskSummary`]. Same contract as the inline conversion
/// in [`flatten_state_to_frame`]; lifted into its own helper so the
/// task-mutation reply paths (`TaskCreated`, `TaskRenamed`, etc.)
/// can build inline snapshots without re-flattening the whole
/// project tree.
fn task_to_summary(
    state: &RegistryState,
    task: another_one_core::project_store::Task,
) -> TaskSummary {
    let store = &state.project_store;
    let section_key = task.section_id.clone();
    let parsed_section = SectionId::from_store_key(&section_key);
    let task_pinned = store.ui.pinned_task_ids.contains(&task.id);
    let tabs = task
        .tabs
        .into_iter()
        .map(|tab| {
            let running = parsed_section
                .as_ref()
                .map(|section| TerminalRuntimeKey {
                    section_id: section.clone(),
                    tab_id: tab.id.clone(),
                })
                .map(|key| state.broadcasts.contains_key(&key))
                .unwrap_or(false);
            TabSummary {
                id: tab.id,
                title: tab.title,
                provider: tab.provider.map(map_agent_provider),
                running,
                pinned: tab.pinned,
                fixed_title: tab.fixed_title,
            }
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
        section_id: section_key,
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

/// Look up a task by id in the registry's project store and project
/// it as a [`TaskSummary`]. Returns `None` for an unknown id.
fn lookup_task_summary(state: &RegistryState, task_id: &str) -> Option<TaskSummary> {
    let task = state.project_store.task(task_id)?.clone();
    Some(task_to_summary(state, task))
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
        // `frame::AgentProvider::Shell` is the wire-side catch-all
        // for `tab.provider == None`; the bridge maps via
        // `.map(map_agent_provider)`, so a `None` core provider
        // stays `None` on the wire (mobile renders it as Shell on
        // its end). No core `Shell` variant exists, intentionally.
    }
}

fn changed_file_to_wire(f: another_one_core::project_store::ChangedFile) -> ChangedFileWire {
    ChangedFileWire {
        path: f.path,
        original_path: f.original_path,
        staged_additions: f.staged_additions,
        staged_deletions: f.staged_deletions,
        unstaged_additions: f.unstaged_additions,
        unstaged_deletions: f.unstaged_deletions,
        index_status: f.index_status.to_string(),
        worktree_status: f.worktree_status.to_string(),
        untracked: f.untracked,
    }
}

fn commit_to_wire(c: another_one_core::project_store::BranchCommit) -> CommitWire {
    CommitWire {
        id: c.id,
        short_id: c.short_id,
        subject: c.subject,
        author_name: c.author_name,
        authored_relative: c.authored_relative,
    }
}
