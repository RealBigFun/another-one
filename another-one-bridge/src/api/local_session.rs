//! In-process FFI session for the future Flutter desktop client.
//!
//! Mirror of [`super::iroh_client::IrohSession`] but with no network
//! transport — the desktop binary hosts its own daemon
//! (`core::daemon_embed::RegistryState`), so for local-host
//! operations there's no need to round-trip through QUIC. The Dart
//! `LocalConnection` (a future implementor of `DaemonConnection`)
//! holds a `LocalSession` and calls its methods directly.
//!
//! Lifecycle: `local_connect` allocates the session and its two
//! channels (worker replies + per-tab incoming bytes). The host
//! binary registers its `Arc<Mutex<RegistryState>>` once at boot via
//! `crate::local_registry::set_local_registry`. Method calls then
//! translate Dart-side intents (list_projects, attach_tab, send,
//! etc.) into reads/writes against `RegistryState`.

use std::collections::HashMap;
use std::io::Write;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use another_one_core::agents::AgentProviderKind;
use another_one_core::daemon_embed::{key_from_wire, RegistryState, TabLaunchRequest};
use another_one_core::open_in::OpenInAppKind;
use another_one_core::platform::{CurrentPlatform, HeadlessPlatform};
use another_one_core::project_store::{
    PersistedSectionState, PersistedTerminalTab, ProjectAction, ProjectActionAccess,
    ProjectActionIcon, ProjectActionKind, ProjectActionScope,
    ProjectKind as CoreProjectKind,
};
use another_one_core::section::SectionId;
use another_one_core::terminal_types::TerminalRuntimeKey;
use flutter_rust_bridge::frb;
use tokio::sync::{broadcast, mpsc};
use tokio::task::AbortHandle;

use super::iroh_client::{
    tokio_rt, AgentProvider, ProjectKind, ProjectSummary, TabSummary, TaskSummary, WorkerReply,
};
use crate::frb_generated::StreamSink;
use crate::local_registry::local_registry;

/// Tracks the currently-attached tab on a `LocalSession`. Reset on
/// `attach_tab` (replaces existing) and on `detach_tab` / `close`.
struct AttachedTab {
    key: TerminalRuntimeKey,
    /// Aborts the tokio task that drains the tab's broadcast into
    /// `incoming_tx`. Dropped on detach so the task stops pushing
    /// bytes; the channel-receiver side cleans itself up when the
    /// session drops.
    forwarder: AbortHandle,
}

/// Opaque handle to an in-process daemon session.
#[frb(opaque)]
pub struct LocalSession {
    /// Stable identifier for this session inside `RegistryState`'s
    /// per-viewer maps (`active_viewers`, `viewer_focus`). Format
    /// `"local-<n>"` where `<n>` is a process-monotonic counter.
    /// The value is opaque; consumers MUST NOT depend on the format.
    viewer_id: String,
    /// Currently-attached tab + its forwarder abort handle.
    attached: Mutex<Option<AttachedTab>>,
    /// Producer side of the worker-replies stream. Cloned into every
    /// method that pushes a reply (today: `list_projects`). Dropped
    /// on `close` so any active forwarder exits cleanly.
    worker_replies_tx: Mutex<Option<mpsc::UnboundedSender<WorkerReply>>>,
    /// Held until `subscribe_worker_replies` takes it; one-shot
    /// subscription, same shape as `IrohSession`.
    worker_replies_rx: Mutex<Option<mpsc::UnboundedReceiver<WorkerReply>>>,
    /// Producer side of the per-tab PTY-byte stream. Cloned into the
    /// per-attach forwarder task. Dropped on `close`.
    incoming_tx: Mutex<Option<mpsc::UnboundedSender<Vec<u8>>>>,
    /// Held until `subscribe` takes it; one-shot.
    incoming_rx: Mutex<Option<mpsc::UnboundedReceiver<Vec<u8>>>>,
}

/// Process-wide counter for `LocalSession::viewer_id`. Two parallel
/// `local_connect` calls (test harnesses, hot-restart) get
/// distinct ids so their `RegistryState::active_viewers` entries
/// don't collide.
static VIEWER_COUNTER: AtomicU64 = AtomicU64::new(0);

/// Construct a session bound to the desktop's in-process daemon.
pub async fn local_connect() -> anyhow::Result<LocalSession> {
    let (worker_tx, worker_rx) = mpsc::unbounded_channel();
    let (incoming_tx, incoming_rx) = mpsc::unbounded_channel();
    let viewer_id = format!(
        "local-{}",
        VIEWER_COUNTER.fetch_add(1, Ordering::Relaxed)
    );
    Ok(LocalSession {
        viewer_id,
        attached: Mutex::new(None),
        worker_replies_tx: Mutex::new(Some(worker_tx)),
        worker_replies_rx: Mutex::new(Some(worker_rx)),
        incoming_tx: Mutex::new(Some(incoming_tx)),
        incoming_rx: Mutex::new(Some(incoming_rx)),
    })
}

impl LocalSession {
    /// Send raw PTY stdin bytes to the currently-attached tab.
    ///
    /// Looks up the tab's writer in `RegistryState::writers` and
    /// writes synchronously. Errors if no tab is attached or the
    /// writer has been dropped (tab exited / runtime gone).
    pub async fn send(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let key = self.attached_key()?;
        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("send: set_local_registry not called"))?;
        let writer = {
            let state = registry
                .lock()
                .map_err(|_| anyhow::anyhow!("send: RegistryState mutex poisoned"))?;
            state
                .writers
                .get(&key)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("send: no writer for attached tab"))?
        };
        let mut writer = writer
            .lock()
            .map_err(|_| anyhow::anyhow!("send: writer mutex poisoned"))?;
        writer
            .write_all(&bytes)
            .map_err(|err| anyhow::anyhow!("send: PTY write failed: {err}"))?;
        Ok(())
    }

    /// Resize the currently-attached tab's PTY.
    ///
    /// Updates this viewer's entry in `RegistryState::active_viewers`
    /// and asks the registry to recompute the effective
    /// (min-across-viewers) size. The desktop UI render tick drains
    /// the resulting `pending_resizes` queue.
    pub async fn tab_resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        let key = self.attached_key()?;
        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("tab_resize: set_local_registry not called"))?;
        let mut state = registry
            .lock()
            .map_err(|_| anyhow::anyhow!("tab_resize: RegistryState mutex poisoned"))?;
        state
            .active_viewers
            .entry(key.clone())
            .or_insert_with(HashMap::new)
            .insert(self.viewer_id.clone(), (cols, rows));
        state.recompute_effective_size(&key);
        Ok(())
    }

    /// Push a project list through [`Self::subscribe_worker_replies`].
    ///
    /// Reads from the host-registered [`RegistryState::project_store`]
    /// and flattens it into the bridge's `ProjectSummary` / `TaskSummary` /
    /// `TabSummary` shape. Boot-order forgiving — if the registry
    /// hasn't been registered yet, sends an empty list.
    pub async fn list_projects(&self) -> anyhow::Result<()> {
        let tx = {
            let guard = self
                .worker_replies_tx
                .lock()
                .expect("worker_replies_tx mutex poisoned");
            guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("session closed"))?
                .clone()
        };

        let projects = match local_registry() {
            Some(registry) => match registry.lock() {
                Ok(state) => flatten_project_store(&state),
                Err(_) => Vec::new(),
            },
            None => Vec::new(),
        };

        tx.send(WorkerReply::ProjectList { projects })
            .map_err(|_| anyhow::anyhow!("worker-replies receiver dropped"))?;
        Ok(())
    }

    /// Create a worktree task on `project_id`. Spawns a fresh git
    /// worktree from `source_branch` (the new branch is named after
    /// the slugified `task_name`), prepares the project, and inserts
    /// both the worktree project and the task into the daemon's
    /// store. Returns the new task's `section_id` so the caller can
    /// navigate to it.
    ///
    /// `agent_provider` is optional; `None` means launch a plain
    /// shell (matches `TerminalLaunchConfig::default()`). When set,
    /// future `launch_tab` calls on the new task's section spawn
    /// the agent CLI with its standard arguments.
    ///
    /// Heavy filesystem work (`create_task_worktree` →
    /// `prepare_project`) runs on a dedicated thread inside
    /// `spawn_task_creation`. We await its broadcast channel reply,
    /// then mutate the registry under one lock.
    pub async fn create_worktree_task(
        &self,
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_provider: Option<AgentProvider>,
    ) -> anyhow::Result<String> {
        // Resolve the project up front so we can complain clearly if
        // it's gone, before spawning the worktree thread.
        let (project_path, project_name, target_project_id) = {
            let registry = local_registry().ok_or_else(|| {
                anyhow::anyhow!("create_worktree_task: set_local_registry not called")
            })?;
            let state = registry
                .lock()
                .map_err(|_| anyhow::anyhow!("create_worktree_task: RegistryState mutex poisoned"))?;
            let project = state
                .project_store
                .project(&project_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("create_worktree_task: unknown project_id `{project_id}`")
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
            Some(provider) => another_one_core::agents::TerminalLaunchConfig::for_provider(provider),
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
        // lock so the listProjects push that follows sees both.
        let section_id = {
            let registry = local_registry().ok_or_else(|| {
                anyhow::anyhow!("create_worktree_task: set_local_registry vanished")
            })?;
            let mut state = registry
                .lock()
                .map_err(|_| anyhow::anyhow!("create_worktree_task: registry mutex poisoned"))?;
            let inserted_worktree =
                state.project_store.insert_prepared_project(success.project.clone());
            // If a worktree project at that path was already known
            // (re-running the modal pointing at an existing
            // worktree), we still proceed to the task insert.
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
                    id: task_id,
                    name: success.task_name,
                    kind: another_one_core::project_store::TaskKind::Worktree,
                    root_project_id: target_project_id,
                    target_project_id: worktree_project_id.clone(),
                    branch_name: success.branch_name,
                    section_id: section_key.clone(),
                    worktree_project_id: Some(worktree_project_id),
                    tabs: Vec::new(),
                    active_tab_id: String::new(),
                    next_tab_id: 0,
                    cwd: None,
                });
            state.project_store.save();
            section_key
        };
        self.list_projects().await?;
        Ok(section_id)
    }

    /// Rename a task. Empty / whitespace-only names are rejected so
    /// the daemon never persists a blank label. Returns whether
    /// anything was actually written (an unknown id or a no-op
    /// rename returns `Ok(false)`). Pushes a fresh `ProjectList`
    /// reply on success so the sidebar redraws.
    pub async fn rename_task(
        &self,
        task_id: String,
        new_name: String,
    ) -> anyhow::Result<bool> {
        let trimmed = new_name.trim().to_string();
        if trimmed.is_empty() {
            return Ok(false);
        }
        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("rename_task: set_local_registry not called"))?;
        let changed = {
            let mut state = registry
                .lock()
                .map_err(|_| anyhow::anyhow!("rename_task: RegistryState mutex poisoned"))?;
            let Some(task) = state.project_store.task_mut(&task_id) else {
                return Ok(false);
            };
            if task.name == trimmed {
                false
            } else {
                task.name = trimmed;
                true
            }
        };
        if changed {
            // Re-acquire the lock for save (rebuild_runtime_views +
            // disk write); `task_mut` only mutates in-memory state
            // and the GPUI desktop persists explicitly after.
            if let Ok(state) = registry.lock() {
                state.project_store.save();
            }
            self.list_projects().await?;
        }
        Ok(changed)
    }

    /// Pin or unpin a task. Pinned tasks float to the top of their
    /// project's task list (mirrors `child_entries.sort_by_key(!is_pinned)`
    /// in the GPUI sidebar). Returns whether the pin state actually
    /// changed; an idempotent re-set is `Ok(false)`. Pushes a fresh
    /// `ProjectList` reply on every call so the sort updates
    /// immediately.
    pub async fn set_task_pinned(
        &self,
        task_id: String,
        pinned: bool,
    ) -> anyhow::Result<bool> {
        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("set_task_pinned: set_local_registry not called"))?;
        let changed = {
            let mut state = registry.lock().map_err(|_| {
                anyhow::anyhow!("set_task_pinned: RegistryState mutex poisoned")
            })?;
            let changed = state.project_store.set_task_pinned(&task_id, pinned);
            if changed {
                state.project_store.save();
            }
            changed
        };
        if changed {
            self.list_projects().await?;
        }
        Ok(changed)
    }

    /// Remove a task (and its terminal sections) from the embedded
    /// daemon's store. The on-disk worktree branch is left
    /// untouched — the GPUI side has the same semantics. Returns
    /// `Ok(true)` if a task was removed, `Ok(false)` for an unknown
    /// id (idempotent).
    pub async fn remove_task(
        &self,
        project_id: String,
        task_id: String,
    ) -> anyhow::Result<bool> {
        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("remove_task: set_local_registry not called"))?;
        let removed = {
            let mut state = registry
                .lock()
                .map_err(|_| anyhow::anyhow!("remove_task: RegistryState mutex poisoned"))?;
            state
                .project_store
                .remove_task(&project_id, &task_id)
                .is_some()
        };
        if removed {
            self.list_projects().await?;
        }
        Ok(removed)
    }

    /// Remove a project from the embedded daemon's store. Cascades
    /// to the project's tasks + terminal sections (see
    /// [`another_one_core::project_store::ProjectStore::remove_project`]).
    /// Idempotent — passing an unknown id is silently a no-op.
    /// Pushes a fresh `ProjectList` reply on completion so
    /// subscribers refresh.
    pub async fn remove_project(&self, project_id: String) -> anyhow::Result<()> {
        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("remove_project: set_local_registry not called"))?;
        {
            let mut state = registry.lock().map_err(|_| {
                anyhow::anyhow!("remove_project: RegistryState mutex poisoned")
            })?;
            state.project_store.remove_project(&project_id);
        }
        self.list_projects().await?;
        Ok(())
    }

    /// Add an existing on-disk project to the embedded daemon's
    /// project store. Returns `Ok(true)` if the project was inserted,
    /// `Ok(false)` if a project at the same path already existed
    /// (idempotent — re-adding is a no-op, not an error).
    ///
    /// Heavy `prepare_project` work runs on a dedicated thread (see
    /// [`another_one_core::project_service::spawn_project_add`]) so
    /// the FRB caller doesn't block. On success, pushes a fresh
    /// `ProjectList` reply so listeners refresh without a follow-up
    /// `list_projects()` round-trip.
    pub async fn add_project(&self, path: String) -> anyhow::Result<bool> {
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

        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("add_project: set_local_registry not called"))?;
        let inserted = {
            let mut state = registry
                .lock()
                .map_err(|_| anyhow::anyhow!("add_project: RegistryState mutex poisoned"))?;
            state.project_store.insert_prepared_project(prepared)
        };

        if inserted {
            self.list_projects().await?;
        }
        Ok(inserted)
    }

    /// Read the list of files with working-tree changes for
    /// `project_id`, mirroring GPUI's right-sidebar Changes pane
    /// data source. Calls into
    /// [`another_one_core::project_store::read_project_git_state`]
    /// with `include_metadata=false` (the right sidebar doesn't
    /// need branch ahead/behind for this view) inside a
    /// `spawn_blocking` so the FRB caller's tokio runtime stays
    /// free.
    ///
    /// Returns `Ok(None)` when the project id is unknown — UI
    /// renders an empty list rather than surfacing the lookup
    /// failure as an error toast (matches GPUI's "no panel" gate).
    pub async fn read_changed_files(
        &self,
        project_id: String,
    ) -> anyhow::Result<Option<Vec<ChangedFileDto>>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("read_changed_files: set_local_registry not called")
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!("read_changed_files: RegistryState mutex poisoned")
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        };
        let Some(project_path) = project_path else {
            return Ok(None);
        };
        let git_state = tokio::task::spawn_blocking(move || {
            another_one_core::project_store::read_project_git_state(
                &project_path,
                false,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("read_changed_files join: {e}"))?;
        let files = git_state
            .changed_files
            .into_iter()
            .map(changed_file_to_dto)
            .collect();
        Ok(Some(files))
    }

    /// Recent commits on `project_id`'s current branch, capped at
    /// `limit` entries. Powers the right sidebar's Commits pane —
    /// reads through
    /// [`another_one_core::project_store::read_project_branch_commit_state`]
    /// inside `spawn_blocking` so the FRB caller's tokio runtime
    /// stays free.
    ///
    /// `limit` mirrors GPUI's `commit_page_size_for_project` — the
    /// caller picks the page size; here we don't enforce a default
    /// so the UI can choose. Returns `Ok(None)` for unknown
    /// projects (UI shows the empty state instead of an error).
    pub async fn read_recent_commits(
        &self,
        project_id: String,
        limit: u32,
    ) -> anyhow::Result<Option<RecentCommitsView>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("read_recent_commits: set_local_registry not called")
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!(
                    "read_recent_commits: RegistryState mutex poisoned"
                )
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        };
        let Some(project_path) = project_path else {
            return Ok(None);
        };
        let limit = limit as usize;
        let result = tokio::task::spawn_blocking(move || {
            another_one_core::project_store::read_project_branch_commit_state(
                &project_path,
                limit,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("read_recent_commits join: {e}"))?
        .map_err(|e| anyhow::anyhow!("read_recent_commits: {e}"))?;
        Ok(Some(RecentCommitsView {
            current_branch: result.current_branch,
            has_more: result.has_more,
            commits: result.commits.into_iter().map(commit_to_dto).collect(),
        }))
    }

    /// Stage one changed file via `git add -A -- <path>`. `original_path`
    /// is set only for renames/copies — the helper passes both
    /// arguments so git can resolve the rename pair correctly.
    /// Errors bubble up as anyhow with the git stderr appended,
    /// matching what GPUI shows in toasts.
    pub async fn stage_changed_file(
        &self,
        project_id: String,
        path: String,
        original_path: Option<String>,
    ) -> anyhow::Result<()> {
        run_changed_file_action(
            &project_id,
            move |project_path| {
                let mut changed = another_one_core::project_store::ChangedFile::default();
                changed.path = path;
                changed.original_path = original_path;
                another_one_core::project_store::stage_changed_file(
                    &project_path,
                    &changed,
                )
            },
        )
        .await
    }

    /// Unstage one changed file via `git restore --staged -- <path>`,
    /// falling back to `git reset HEAD -- <path>` if the repo is
    /// pre-2.23 (matches `core::unstage_changed_file`).
    pub async fn unstage_changed_file(
        &self,
        project_id: String,
        path: String,
        original_path: Option<String>,
    ) -> anyhow::Result<()> {
        run_changed_file_action(
            &project_id,
            move |project_path| {
                let mut changed = another_one_core::project_store::ChangedFile::default();
                changed.path = path;
                changed.original_path = original_path;
                another_one_core::project_store::unstage_changed_file(
                    &project_path,
                    &changed,
                )
            },
        )
        .await
    }

    /// `git add -A` on the project root — stage every change.
    pub async fn stage_all_changes(&self, project_id: String) -> anyhow::Result<()> {
        run_changed_file_action(&project_id, |project_path| {
            another_one_core::project_store::stage_all_changes(&project_path)
        })
        .await
    }

    /// Discard one file's changes. Untracked files are deleted; tracked
    /// files are restored from HEAD via `git restore` (with checkout
    /// fallback for older git). Mirrors GPUI's `revert_changed_file`
    /// behaviour: returns success even on no-op, errors only when
    /// every git invocation fails.
    pub async fn discard_changed_file(
        &self,
        project_id: String,
        path: String,
        original_path: Option<String>,
        untracked: bool,
    ) -> anyhow::Result<()> {
        run_changed_file_action(
            &project_id,
            move |project_path| {
                let mut changed = another_one_core::project_store::ChangedFile::default();
                changed.path = path.clone();
                changed.original_path = original_path;
                changed.untracked = untracked;
                if another_one_core::project_store::revert_changed_file(
                    &project_path,
                    &changed,
                ) {
                    Ok(())
                } else {
                    Err(format!("Could not discard {path}"))
                }
            },
        )
        .await
    }

    /// Unstage every currently-staged change.
    pub async fn unstage_all_changes(&self, project_id: String) -> anyhow::Result<()> {
        run_changed_file_action(&project_id, |project_path| {
            another_one_core::project_store::unstage_all_changes(&project_path)
        })
        .await
    }

    /// Fetch open pull requests for `project_id` filtered by
    /// `filter_index` (0=all, 1=needs my review, 2=author:@me,
    /// 3=draft) plus an optional free-text `query`. Powers the
    /// project page's Open PRs section.
    ///
    /// Routes [`another_one_core::git_actions::find_project_pull_requests`]
    /// inside `spawn_blocking` (it shells out to `gh pr list`).
    /// Returns `Ok(None)` for unknown projects; errors propagate
    /// for gh CLI failures (CLI missing, auth, network).
    pub async fn find_project_pull_requests(
        &self,
        project_id: String,
        filter_index: u32,
        query: String,
    ) -> anyhow::Result<Option<Vec<ProjectPagePullRequestDto>>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "find_project_pull_requests: set_local_registry not called"
            )
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!(
                    "find_project_pull_requests: RegistryState mutex poisoned"
                )
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        };
        let Some(project_path) = project_path else {
            return Ok(None);
        };
        let trimmed = query.trim().to_string();
        let q = if trimmed.is_empty() { None } else { Some(trimmed) };
        let result = tokio::task::spawn_blocking(move || {
            another_one_core::git_actions::find_project_pull_requests(
                &project_path,
                filter_index as usize,
                q.as_deref(),
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("find_project_pull_requests join: {e}"))?
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Some(result.into_iter().map(pr_to_dto).collect()))
    }

    /// Spawn a review task for a pull request — clones into a
    /// worktree at the PR's head branch, prepares the project,
    /// inserts both into the daemon's store, and returns the new
    /// section_id.
    ///
    /// Routes
    /// [`another_one_core::project_service::spawn_review_task_creation`]
    /// and mirrors the `create_worktree_task` pattern. Auto-runs
    /// project actions and the configured agent CLI when
    /// `agent_provider` is set.
    pub async fn create_review_task(
        &self,
        project_id: String,
        pull_request_number: u64,
        head_branch: String,
        agent_provider: Option<AgentProvider>,
    ) -> anyhow::Result<String> {
        let (project_path, project_name, target_project_id) = {
            let registry = local_registry().ok_or_else(|| {
                anyhow::anyhow!(
                    "create_review_task: set_local_registry not called"
                )
            })?;
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!("create_review_task: RegistryState mutex poisoned")
            })?;
            let project = state
                .project_store
                .project(&project_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "create_review_task: unknown project_id `{project_id}`"
                    )
                })?;
            (
                project.path.clone(),
                project.name.clone(),
                project.id.clone(),
            )
        };

        let task_name = format!("review-pr-{pull_request_number}");
        let launch_config = match agent_provider.map(map_agent_provider_back) {
            Some(provider) => {
                another_one_core::agents::TerminalLaunchConfig::for_provider(provider)
            }
            None => another_one_core::agents::TerminalLaunchConfig::default(),
        };
        let mut rx = another_one_core::project_service::spawn_review_task_creation(
            target_project_id.clone(),
            project_path,
            task_name,
            pull_request_number,
            head_branch,
            launch_config,
            true,
            true,
        );
        let reply = rx.recv().await.map_err(|_| {
            anyhow::anyhow!("review task worker dropped before reply")
        })?;
        let success = reply
            .result
            .map_err(|f| anyhow::anyhow!("create review task: {}", f.message))?;

        let section_id = {
            let registry = local_registry().ok_or_else(|| {
                anyhow::anyhow!("create_review_task: set_local_registry vanished")
            })?;
            let mut state = registry.lock().map_err(|_| {
                anyhow::anyhow!("create_review_task: registry mutex poisoned")
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
            // Use a name like "Review #42 (project-name)" for the
            // task list entry so users can scan multiple review
            // tasks easily.
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
                    id: task_id,
                    name: format!("Review #{pull_request_number} ({project_name})"),
                    kind: another_one_core::project_store::TaskKind::Worktree,
                    root_project_id: target_project_id,
                    target_project_id: worktree_project_id.clone(),
                    branch_name: success.branch_name,
                    section_id: section_key.clone(),
                    worktree_project_id: Some(worktree_project_id),
                    tabs: Vec::new(),
                    active_tab_id: String::new(),
                    next_tab_id: 0,
                    cwd: None,
                });
            state.project_store.save();
            section_key
        };
        self.list_projects().await?;
        Ok(section_id)
    }

    /// Create a new branch from HEAD on `project_id`. When
    /// `use_current_task` is true, switches the current checkout
    /// (no new worktree). Otherwise a new worktree is created
    /// next to the existing project, optionally migrating any
    /// uncommitted changes.
    ///
    /// Returns the new task's `section_id` for the worktree case;
    /// empty string for the current-task case (the caller's UI
    /// just dismisses the modal). Routes
    /// [`another_one_core::project_service::spawn_branch_creation`].
    pub async fn create_branch(
        &self,
        project_id: String,
        branch_name: String,
        use_current_task: bool,
        migrate_changes: bool,
    ) -> anyhow::Result<String> {
        let (project_path, project_name, target_project_id) = {
            let registry = local_registry().ok_or_else(|| {
                anyhow::anyhow!(
                    "create_branch: set_local_registry not called"
                )
            })?;
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!("create_branch: RegistryState mutex poisoned")
            })?;
            let project = state
                .project_store
                .project(&project_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "create_branch: unknown project_id `{project_id}`"
                    )
                })?;
            (
                project.path.clone(),
                project.name.clone(),
                project.id.clone(),
            )
        };
        let mut rx = another_one_core::project_service::spawn_branch_creation(
            target_project_id.clone(),
            project_path,
            branch_name,
            use_current_task,
            migrate_changes,
        );
        let reply = rx.recv().await.map_err(|_| {
            anyhow::anyhow!("branch creation worker dropped before reply")
        })?;
        let success = reply
            .result
            .map_err(|f| anyhow::anyhow!("create branch: {}", f.message))?;

        if let Some(prepared) = success.project {
            // Worktree mode — insert the new project + task and
            // return the section_id so the caller can navigate.
            let section_id = {
                let registry = local_registry().ok_or_else(|| {
                    anyhow::anyhow!("create_branch: set_local_registry vanished")
                })?;
                let mut state = registry.lock().map_err(|_| {
                    anyhow::anyhow!("create_branch: registry mutex poisoned")
                })?;
                let inserted_worktree = state
                    .project_store
                    .insert_prepared_project(prepared.clone());
                let worktree_project_id = if inserted_worktree {
                    prepared.project.id.clone()
                } else {
                    state
                        .project_store
                        .projects
                        .iter()
                        .find(|p| p.path == prepared.project.path)
                        .map(|p| p.id.clone())
                        .unwrap_or_else(|| prepared.project.id.clone())
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
                        id: task_id,
                        name: success.task_name,
                        kind: another_one_core::project_store::TaskKind::Worktree,
                        root_project_id: target_project_id,
                        target_project_id: worktree_project_id.clone(),
                        branch_name: success.branch_name,
                        section_id: section_key.clone(),
                        worktree_project_id: Some(worktree_project_id),
                        tabs: Vec::new(),
                        active_tab_id: String::new(),
                        next_tab_id: 0,
                        cwd: None,
                    });
                state.project_store.save();
                section_key
            };
            self.list_projects().await?;
            Ok(section_id)
        } else {
            // Current-task mode — branch swap on the existing
            // checkout, no new project. Refresh the project list
            // so the UI sees the updated current branch and
            // dismiss the modal.
            let _ = project_name; // unused in this branch
            self.list_projects().await?;
            Ok(String::new())
        }
    }

    /// Compute the canonical branch slug for a free-text input.
    /// Mirrors GPUI's live preview that updates beneath the
    /// branch-name input. Routes
    /// [`another_one_core::project_store::slugify_branch_name`].
    /// Takes `&self` only so FRB binds it as an instance method
    /// on `LocalSession` — it doesn't actually read session state.
    pub async fn slugify_branch_name(&self, name: String) -> String {
        let _ = self;
        another_one_core::project_store::slugify_branch_name(&name)
    }

    /// User's preferred default commit action for the active
    /// project's root repo (`"commit"` or `"commit-and-push"`).
    /// `None` when no preference has been recorded — UI defaults to
    /// `"commit"` in that case, matching GPUI's
    /// `resolve_idle_primary_git_action` fallback.
    pub async fn repo_default_commit_action(
        &self,
        project_id: String,
    ) -> anyhow::Result<Option<String>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "repo_default_commit_action: set_local_registry not called"
            )
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "repo_default_commit_action: RegistryState mutex poisoned"
            )
        })?;
        let Some(project) = state.project_store.project(&project_id) else {
            return Ok(None);
        };
        Ok(state
            .project_store
            .repo_default_commit_action(&project.repo_id)
            .map(|a| match a {
                another_one_core::project_store::RepoDefaultCommitAction::Commit => {
                    "commit".to_string()
                }
                another_one_core::project_store::RepoDefaultCommitAction::CommitAndPush => {
                    "commit-and-push".to_string()
                }
            }))
    }

    /// List of available branches on `project_id`'s git repo.
    /// Wraps `ProjectStore::branch_names`. Powers the new-task
    /// modal's source-branch dropdown.
    ///
    /// Returns an empty list for unknown projects — same
    /// behaviour the core helper has.
    pub async fn read_project_branches(
        &self,
        project_id: String,
    ) -> anyhow::Result<Vec<String>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "read_project_branches: set_local_registry not called"
            )
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "read_project_branches: RegistryState mutex poisoned"
            )
        })?;
        Ok(state.project_store.branch_names(&project_id))
    }

    /// Submit the new-task modal: spawns a worktree task or a
    /// direct-mode task depending on `worktree_mode`. Mirrors
    /// `desktop/src/app.rs::submit_new_task_modal` +
    /// `launch_task_request`.
    ///
    /// `agent_ids` is the new-task modal's multi-select picks; the
    /// resulting `TerminalLaunchConfig` is built via
    /// `terminal_launch_config_for_selected_agents` (so an empty
    /// set + a `Shell` sentinel pass through to a default
    /// `TerminalLaunchConfig` exactly the same way GPUI does).
    ///
    /// `branch_mode_existing=true` reuses an existing branch in
    /// the worktree path; `false` cuts a new branch from
    /// `source_branch`. Ignored when `worktree_mode=false`
    /// (Direct mode always uses the project's current branch).
    ///
    /// Returns the new task's section_id so the UI can navigate.
    pub async fn submit_new_task(
        &self,
        project_id: String,
        task_name: String,
        source_branch: String,
        agent_ids: Vec<String>,
        branch_mode_existing: bool,
        worktree_mode: bool,
    ) -> anyhow::Result<String> {
        let trimmed = task_name.trim().to_string();
        if trimmed.is_empty() {
            anyhow::bail!("submit_new_task: task_name must not be blank");
        }
        let agent_set: std::collections::HashSet<String> =
            agent_ids.into_iter().collect();
        let launch_config =
            another_one_core::agents::terminal_launch_config_for_selected_agents(
                &agent_set,
            );

        if worktree_mode {
            return self
                .submit_worktree_task(project_id, trimmed, source_branch, branch_mode_existing, launch_config)
                .await;
        }
        self.submit_direct_task(project_id, trimmed, source_branch, launch_config)
    }

    async fn submit_worktree_task(
        &self,
        project_id: String,
        task_name: String,
        source_branch: String,
        branch_mode_existing: bool,
        launch_config: another_one_core::agents::TerminalLaunchConfig,
    ) -> anyhow::Result<String> {
        let (project_path, project_name, target_project_id) = {
            let registry = local_registry().ok_or_else(|| {
                anyhow::anyhow!("submit_worktree_task: set_local_registry not called")
            })?;
            let state = registry
                .lock()
                .map_err(|_| anyhow::anyhow!("submit_worktree_task: RegistryState mutex poisoned"))?;
            let project = state
                .project_store
                .project(&project_id)
                .ok_or_else(|| {
                    anyhow::anyhow!("submit_worktree_task: unknown project_id `{project_id}`")
                })?;
            (project.path.clone(), project.name.clone(), project.id.clone())
        };
        let branch_mode = if branch_mode_existing {
            another_one_core::project_store::TaskWorktreeBranchMode::ExistingBranch {
                branch: source_branch,
            }
        } else {
            another_one_core::project_store::TaskWorktreeBranchMode::NewBranchFrom {
                source_branch,
            }
        };

        let mut rx = another_one_core::project_service::spawn_task_creation(
            target_project_id.clone(),
            project_path,
            project_name,
            task_name.clone(),
            task_name,
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

        let section_id = {
            let registry = local_registry().ok_or_else(|| {
                anyhow::anyhow!("submit_worktree_task: set_local_registry vanished")
            })?;
            let mut state = registry
                .lock()
                .map_err(|_| anyhow::anyhow!("submit_worktree_task: registry mutex poisoned"))?;
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
                    id: task_id,
                    name: success.task_name,
                    kind: another_one_core::project_store::TaskKind::Worktree,
                    root_project_id: target_project_id,
                    target_project_id: worktree_project_id.clone(),
                    branch_name: success.branch_name,
                    section_id: section_key.clone(),
                    worktree_project_id: Some(worktree_project_id),
                    tabs: Vec::new(),
                    active_tab_id: String::new(),
                    next_tab_id: 0,
                    cwd: None,
                });
            state.project_store.save();
            section_key
        };
        self.list_projects().await?;
        Ok(section_id)
    }

    fn submit_direct_task(
        &self,
        project_id: String,
        task_name: String,
        source_branch: String,
        _launch_config: another_one_core::agents::TerminalLaunchConfig,
    ) -> anyhow::Result<String> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("submit_direct_task: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("submit_direct_task: RegistryState mutex poisoned")
        })?;
        let project = state
            .project_store
            .project(&project_id)
            .ok_or_else(|| {
                anyhow::anyhow!("submit_direct_task: unknown project_id `{project_id}`")
            })?;
        let project_id_owned = project.id.clone();
        let branch_name = state
            .project_store
            .current_branch_name(&project_id_owned)
            .filter(|b| !b.is_empty())
            .unwrap_or_else(|| {
                if source_branch.is_empty() {
                    "main".to_string()
                } else {
                    source_branch
                }
            });
        let task_id = uuid::Uuid::new_v4().to_string();
        let section = another_one_core::section::SectionId::for_task(
            &project_id_owned,
            &branch_name,
            &task_id,
        );
        let section_key = section.store_key();
        state
            .project_store
            .insert_task(another_one_core::project_store::Task {
                id: task_id,
                name: task_name,
                kind: another_one_core::project_store::TaskKind::Direct,
                root_project_id: project_id_owned.clone(),
                target_project_id: project_id_owned,
                branch_name,
                section_id: section_key.clone(),
                worktree_project_id: None,
                tabs: Vec::new(),
                active_tab_id: String::new(),
                next_tab_id: 0,
                cwd: None,
            });
        state.project_store.save();
        Ok(section_key)
    }

    /// Default branch GPUI seeds the new-task modal with for
    /// `project_id`. Wraps `ProjectStore::primary_branch_for_project`
    /// with `prefer_default=true`.
    pub async fn primary_branch_for_project(
        &self,
        project_id: String,
    ) -> anyhow::Result<Option<String>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "primary_branch_for_project: set_local_registry not called"
            )
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "primary_branch_for_project: RegistryState mutex poisoned"
            )
        })?;
        Ok(state
            .project_store
            .primary_branch_for_project(&project_id, true)
            .map(|branch| branch.name))
    }

    /// Activate `tab_id` inside `section_id` — only updates the
    /// section's persisted `active_tab_id`. Does not relaunch
    /// (the Dart-side `selectedTabProvider` triggers attach via
    /// `attach_tab`).
    pub async fn activate_section_tab(
        &self,
        section_id: String,
        tab_id: String,
    ) -> anyhow::Result<()> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("activate_section_tab: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("activate_section_tab: RegistryState mutex poisoned")
        })?;
        let Some(section) = state
            .project_store
            .terminal_sections
            .get(&section_id)
            .cloned()
        else {
            return Ok(());
        };
        if !section.tabs.iter().any(|t| t.id == tab_id) {
            return Ok(());
        }
        let mut next = section;
        next.active_tab_id = tab_id;
        state.project_store.set_section_state(section_id, next);
        Ok(())
    }

    /// Remove a tab from `section_id`. If the closed tab was
    /// active, the new active tab is the previous neighbour
    /// (or the new last tab when closing the head). Returns the
    /// new active tab id (empty when the section is now empty).
    pub async fn close_section_tab(
        &self,
        section_id: String,
        tab_id: String,
    ) -> anyhow::Result<String> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("close_section_tab: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("close_section_tab: RegistryState mutex poisoned")
        })?;
        let Some(section) = state
            .project_store
            .terminal_sections
            .get(&section_id)
            .cloned()
        else {
            return Ok(String::new());
        };
        let Some(idx) = section.tabs.iter().position(|t| t.id == tab_id) else {
            return Ok(section.active_tab_id);
        };
        let mut next = section;
        next.tabs.remove(idx);
        if next.active_tab_id == tab_id {
            next.active_tab_id = if next.tabs.is_empty() {
                String::new()
            } else {
                let new_idx = idx.min(next.tabs.len().saturating_sub(1));
                next.tabs[new_idx].id.clone()
            };
        }
        // Free the live broadcast/writer entries (best-effort —
        // the daemon-side drain owns the actual PTY teardown).
        let key = TerminalRuntimeKey {
            section_id: another_one_core::section::SectionId::from_store_key(&section_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "close_section_tab: malformed section_id `{section_id}`"
                    )
                })?,
            tab_id: tab_id.clone(),
        };
        state.broadcasts.remove(&key);
        state.writers.remove(&key);
        state.in_flight_launches.remove(&key);
        state.pending_post_launch_input.remove(&key);
        state
            .pending_tab_launches
            .retain(|req| req.key != key);
        let active = next.active_tab_id.clone();
        state.project_store.set_section_state(section_id, next);
        Ok(active)
    }

    /// Flip the `pinned` flag on one tab. Mirrors GPUI's
    /// `toggle_tab_pinned` — pinned tabs sort to the head of the
    /// strip but the bridge stores them in insertion order; UI
    /// re-orders pinned-first at render time.
    pub async fn toggle_section_tab_pinned(
        &self,
        section_id: String,
        tab_id: String,
    ) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("toggle_section_tab_pinned: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "toggle_section_tab_pinned: RegistryState mutex poisoned"
            )
        })?;
        let Some(section) = state
            .project_store
            .terminal_sections
            .get(&section_id)
            .cloned()
        else {
            return Ok(false);
        };
        let mut next = section;
        let mut new_pinned = false;
        let mut found = false;
        for tab in next.tabs.iter_mut() {
            if tab.id == tab_id {
                tab.pinned = !tab.pinned;
                new_pinned = tab.pinned;
                found = true;
                break;
            }
        }
        if !found {
            return Ok(false);
        }
        state.project_store.set_section_state(section_id, next);
        Ok(new_pinned)
    }

    /// Append an agent tab (or plain shell, when `agent_id` is
    /// empty / the Terminal sentinel) to `section_id`'s task and
    /// queue its PTY launch. Mirrors
    /// `desktop/src/add_agent_modal.rs::submit_add_agent_modal`.
    ///
    /// Returns the new tab id so the UI can switch to it.
    /// Empty `agent_id` is treated the same way as GPUI's
    /// `terminal_launch_config_for_selected_agent(None)` —
    /// i.e. opens a plain shell in the section's worktree.
    pub async fn add_agent_to_section(
        &self,
        section_id: String,
        agent_id: String,
    ) -> anyhow::Result<String> {
        let key_section = another_one_core::section::SectionId::from_store_key(&section_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "add_agent_to_section: malformed section_id `{section_id}` — expected SectionId::store_key()"
                )
            })?;
        let trimmed = agent_id.trim().to_string();
        let launch_config = if trimmed.is_empty() {
            another_one_core::agents::TerminalLaunchConfig::default()
        } else {
            let mut set = std::collections::HashSet::new();
            set.insert(trimmed);
            another_one_core::agents::terminal_launch_config_for_selected_agents(&set)
        };
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("add_agent_to_section: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("add_agent_to_section: RegistryState mutex poisoned")
        })?;

        let tab_id = uuid::Uuid::new_v4().to_string();
        let tab = PersistedTerminalTab {
            id: tab_id.clone(),
            title: launch_config.default_title(),
            pinned: false,
            fixed_title: None,
            provider: launch_config.provider,
            launch_config: Some(launch_config.clone()),
            restore_status: another_one_core::agents::TerminalRestoreStatus::Launching,
        };
        let key = TerminalRuntimeKey {
            section_id: key_section,
            tab_id: tab_id.clone(),
        };
        let mut existing_section = state
            .project_store
            .terminal_sections
            .get(&section_id)
            .cloned()
            .unwrap_or_else(|| PersistedSectionState {
                active_tab_id: String::new(),
                next_tab_id: 1,
                cwd: None,
                tabs: Vec::new(),
            });
        existing_section.tabs.push(tab);
        existing_section.active_tab_id = tab_id.clone();
        existing_section.next_tab_id = existing_section.next_tab_id.saturating_add(1);
        state
            .project_store
            .set_section_state(section_id.clone(), existing_section);
        state
            .pending_tab_launches
            .push(TabLaunchRequest { key });
        Ok(tab_id)
    }

    /// Full agent registry — every agent in
    /// [`another_one_core::agents::AGENTS`] paired with its
    /// per-host enabled flag, default flag, and launch-args list.
    /// Drives the Settings → Agents page; the new-task /
    /// add-agent modals use the narrower `read_enabled_agents`.
    pub async fn read_agent_settings(
        &self,
    ) -> anyhow::Result<AgentSettingsView> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("read_agent_settings: set_local_registry not called")
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!("read_agent_settings: RegistryState mutex poisoned")
        })?;
        let default_agent_id =
            state.project_store.default_agent_id().map(str::to_string);
        let agents = another_one_core::agents::AGENTS
            .iter()
            .map(|agent| AgentSettingsRow {
                id: agent.id.to_string(),
                label: agent.label.to_string(),
                icon_path: agent.icon.to_string(),
                provider: agent.provider.map(map_agent_provider),
                enabled: state.project_store.agent_enabled(agent.id),
                is_default:
                    default_agent_id.as_deref() == Some(agent.id),
                launch_args: state
                    .project_store
                    .agent_launch_args(agent.id)
                    .to_vec(),
            })
            .collect();
        Ok(AgentSettingsView {
            agents,
            default_agent_id,
        })
    }

    /// Toggle an agent's enabled flag. Returns whether the value
    /// actually changed (a redundant set is a no-op + `false`).
    pub async fn set_agent_enabled(
        &self,
        agent_id: String,
        enabled: bool,
    ) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("set_agent_enabled: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("set_agent_enabled: RegistryState mutex poisoned")
        })?;
        Ok(state
            .project_store
            .set_agent_enabled(&agent_id, enabled))
    }

    /// Mark `agent_id` as the default agent. Mirrors GPUI's
    /// `set_default_agent`. Returns whether anything changed.
    pub async fn set_default_agent(
        &self,
        agent_id: String,
    ) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("set_default_agent: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("set_default_agent: RegistryState mutex poisoned")
        })?;
        Ok(state.project_store.set_default_agent(&agent_id))
    }

    /// Snapshot of the Settings → MCP page. Pairs the static
    /// catalog entries with the on-disk registry so the UI can
    /// flip catalog rows between "Add" prompts and live registry
    /// rows in one render pass.
    ///
    /// `sync_errors` is empty in this first cut — sync state today
    /// lives only in the desktop binary's GPUI app object. Once
    /// `sync_all` runs through the bridge, the errors set will be
    /// repopulated here for the per-provider danger tint.
    pub async fn read_mcp_settings(
        &self,
    ) -> anyhow::Result<McpSettingsView> {
        let _ = self;
        let registry = tokio::task::spawn_blocking(
            another_one_core::mcp::registry::McpRegistry::load,
        )
        .await
        .map_err(|e| anyhow::anyhow!("read_mcp_settings join: {e}"))?;
        let catalog = another_one_core::mcp::catalog::entries()
            .iter()
            .map(|entry| McpCatalogEntryDto {
                id: entry.id.to_string(),
                label: entry.label.to_string(),
                description: entry.description.to_string(),
                docs_url: entry.docs_url.to_string(),
            })
            .collect();
        let entries = registry
            .entries
            .iter()
            .map(mcp_server_to_dto)
            .collect();
        Ok(McpSettingsView {
            catalog_entries: catalog,
            registry_entries: entries,
            sync_error_provider_ids: Vec::new(),
        })
    }

    /// Add one catalog entry to the registry. No-op when the id
    /// isn't a known catalog id or the entry's already in the
    /// registry (mirrors `mcp_add_from_catalog`).
    pub async fn mcp_add_from_catalog(
        &self,
        catalog_id: String,
    ) -> anyhow::Result<()> {
        let _ = self;
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let entry = match another_one_core::mcp::catalog::find(&catalog_id) {
                Some(e) => e,
                None => return Ok(()),
            };
            let mut registry =
                another_one_core::mcp::registry::McpRegistry::load();
            registry.upsert(another_one_core::mcp::catalog::instantiate(entry));
            registry
                .save()
                .map_err(|e| anyhow::anyhow!("save mcp registry: {e}"))?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("mcp_add_from_catalog join: {e}"))?
    }

    /// Toggle one entry's enabled flag for a provider. Runs
    /// `sync_all` on success so the harness's native config picks
    /// up the change. Provider id is the kebab-case name —
    /// `claude-code`, `cursor-agent`, etc.
    pub async fn mcp_toggle(
        &self,
        entry_id: String,
        provider_id: String,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let provider = parse_provider_id(&provider_id).ok_or_else(|| {
            anyhow::anyhow!(
                "mcp_toggle: unknown provider id `{provider_id}`"
            )
        })?;
        let _ = self;
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let mut registry =
                another_one_core::mcp::registry::McpRegistry::load();
            if !registry.toggle(&entry_id, provider, enabled) {
                return Ok(());
            }
            let _report = registry.sync_all();
            registry
                .save()
                .map_err(|e| anyhow::anyhow!("save mcp registry: {e}"))?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("mcp_toggle join: {e}"))?
    }

    /// Remove one entry from the registry. Runs `sync_all` on
    /// success so the harness's native config drops the row.
    pub async fn mcp_remove(
        &self,
        entry_id: String,
    ) -> anyhow::Result<()> {
        let _ = self;
        tokio::task::spawn_blocking(move || -> anyhow::Result<()> {
            let mut registry =
                another_one_core::mcp::registry::McpRegistry::load();
            if !registry.remove(&entry_id) {
                return Ok(());
            }
            let _report = registry.sync_all();
            registry
                .save()
                .map_err(|e| anyhow::anyhow!("save mcp registry: {e}"))?;
            Ok(())
        })
        .await
        .map_err(|e| anyhow::anyhow!("mcp_remove join: {e}"))?
    }

    /// Snapshot of the Settings → Keybindings page. Every shortcut
    /// action paired with its current + default binding strings
    /// (kebab-case modifier names, e.g. `cmd-shift-]`).
    pub async fn read_shortcut_settings(
        &self,
    ) -> anyhow::Result<ShortcutSettingsView> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "read_shortcut_settings: set_local_registry not called"
            )
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "read_shortcut_settings: RegistryState mutex poisoned"
            )
        })?;
        let shortcuts = &state.project_store.ui.shortcuts;
        let rows = another_one_core::shortcuts::ALL_SHORTCUT_ACTIONS
            .iter()
            .map(|action| ShortcutSettingsRow {
                id: shortcut_action_id(*action).to_string(),
                label: action.label().to_string(),
                current_binding: shortcuts.binding_for(*action).to_string(),
                default_binding: action.default_binding().to_string(),
            })
            .collect();
        Ok(ShortcutSettingsView { actions: rows })
    }

    /// Set / clear / reset one shortcut binding. `binding` is the
    /// kebab-case modifier string (e.g. `"cmd-shift-]"`); pass the
    /// empty string to clear (the action becomes inert).
    pub async fn set_shortcut_binding(
        &self,
        action_id: String,
        binding: String,
    ) -> anyhow::Result<()> {
        let action = parse_shortcut_action_id(&action_id).ok_or_else(|| {
            anyhow::anyhow!(
                "set_shortcut_binding: unknown action id `{action_id}`"
            )
        })?;
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "set_shortcut_binding: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "set_shortcut_binding: RegistryState mutex poisoned"
            )
        })?;
        state
            .project_store
            .set_shortcut_binding(action, binding);
        Ok(())
    }

    /// Reset one shortcut back to its default binding.
    pub async fn reset_shortcut_binding(
        &self,
        action_id: String,
    ) -> anyhow::Result<()> {
        let action = parse_shortcut_action_id(&action_id).ok_or_else(|| {
            anyhow::anyhow!(
                "reset_shortcut_binding: unknown action id `{action_id}`"
            )
        })?;
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "reset_shortcut_binding: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "reset_shortcut_binding: RegistryState mutex poisoned"
            )
        })?;
        state.project_store.reset_shortcut_binding(action);
        Ok(())
    }

    /// Snapshot of the Settings → Git Actions page state. Returns
    /// the user-customised commit + PR scripts if set, plus the
    /// resolved-current text (built-in default when no override).
    /// `using_default` reflects whether each script is using the
    /// built-in template — drives the "Currently using the default
    /// built-in template." vs "...custom template..." subtitle.
    pub async fn read_git_action_scripts(
        &self,
    ) -> anyhow::Result<GitActionScriptsView> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "read_git_action_scripts: set_local_registry not called"
            )
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "read_git_action_scripts: RegistryState mutex poisoned"
            )
        })?;
        let store = &state.project_store;
        Ok(GitActionScriptsView {
            commit_script: store.git_commit_generation_script().to_string(),
            commit_using_default:
                store.ui.git_commit_generation_script.is_none(),
            pr_script: store.git_pr_generation_script().to_string(),
            pr_using_default:
                store.ui.git_pr_generation_script.is_none(),
        })
    }

    /// Set the commit-message generation script. Empty / matching
    /// the default reverts to the built-in template (matches the
    /// short-circuit in core).
    pub async fn set_git_commit_script(
        &self,
        script: String,
    ) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "set_git_commit_script: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "set_git_commit_script: RegistryState mutex poisoned"
            )
        })?;
        Ok(state
            .project_store
            .set_git_commit_generation_script(script))
    }

    /// Drop the commit-message override and revert to the built-in
    /// default. Returns whether anything was removed.
    pub async fn reset_git_commit_script(&self) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "reset_git_commit_script: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "reset_git_commit_script: RegistryState mutex poisoned"
            )
        })?;
        Ok(state.project_store.reset_git_commit_generation_script())
    }

    /// Set the PR title/body generation script. Same short-circuit
    /// rules as `set_git_commit_script`.
    pub async fn set_git_pr_script(
        &self,
        script: String,
    ) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "set_git_pr_script: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "set_git_pr_script: RegistryState mutex poisoned"
            )
        })?;
        Ok(state.project_store.set_git_pr_generation_script(script))
    }

    /// Reset the PR script back to the built-in template.
    pub async fn reset_git_pr_script(&self) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "reset_git_pr_script: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "reset_git_pr_script: RegistryState mutex poisoned"
            )
        })?;
        // Mirrors core's reset which only knows the commit one;
        // do the equivalent inline for PR.
        let removed = state
            .project_store
            .ui
            .git_pr_generation_script
            .take()
            .is_some();
        if removed {
            state.project_store.save();
        }
        Ok(removed)
    }

    /// Snapshot of every detected Open-In app on this host paired
    /// with its current enabled flag. Drives the Settings → Open
    /// In page (the titlebar dropdown still uses the narrower
    /// `open_in_state` for its primary-action lookup).
    pub async fn read_open_in_settings(
        &self,
    ) -> anyhow::Result<OpenInSettingsView> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "read_open_in_settings: set_local_registry not called"
            )
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "read_open_in_settings: RegistryState mutex poisoned"
            )
        })?;
        let available = available_open_in_apps();
        let enabled = state
            .project_store
            .enabled_open_in_apps(&available);
        Ok(OpenInSettingsView {
            available_apps: available
                .iter()
                .map(|app| OpenInAppSettingsRow {
                    id: app.id().to_string(),
                    label: app.label().to_string(),
                    description: app.description().to_string(),
                    icon_path: app.icon_path().to_string(),
                    enabled: enabled.contains(app),
                })
                .collect(),
        })
    }

    /// Toggle one Open-In app's enabled flag in the user's Settings
    /// → Open In page. Mirrors GPUI's `set_open_in_app_enabled`.
    pub async fn set_open_in_app_enabled(
        &self,
        app_id: String,
        enabled: bool,
    ) -> anyhow::Result<()> {
        let app = parse_open_in_app_id(&app_id).ok_or_else(|| {
            anyhow::anyhow!(
                "set_open_in_app_enabled: unknown app id `{app_id}`"
            )
        })?;
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "set_open_in_app_enabled: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "set_open_in_app_enabled: RegistryState mutex poisoned"
            )
        })?;
        let available = available_open_in_apps();
        state
            .project_store
            .set_open_in_app_enabled(app, enabled, &available);
        Ok(())
    }

    /// Replace the launch-args list for `agent_id`. Empty `args`
    /// removes the entry entirely (matches core's `set_agent_launch_args`
    /// → `remove_agent_launch_args` short-circuit).
    pub async fn set_agent_launch_args(
        &self,
        agent_id: String,
        args: Vec<String>,
    ) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "set_agent_launch_args: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "set_agent_launch_args: RegistryState mutex poisoned"
            )
        })?;
        Ok(state
            .project_store
            .set_agent_launch_args(agent_id, args))
    }

    /// Snapshot of agents the user has enabled on this host plus
    /// the id of the one they've picked as default. Drives the
    /// new-task modal's agent multi-select. The returned list is
    /// in the canonical AGENTS order — UI renders it as is.
    pub async fn read_enabled_agents(
        &self,
    ) -> anyhow::Result<EnabledAgentsView> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("read_enabled_agents: set_local_registry not called")
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!("read_enabled_agents: RegistryState mutex poisoned")
        })?;
        let enabled =
            another_one_core::agents::effective_enabled_agents(
                state.project_store.ui.enabled_agents.as_ref(),
            );
        let agents = enabled.iter().map(agent_def_to_dto).collect();
        let default_agent_id =
            state.project_store.default_agent_id().map(str::to_string);
        Ok(EnabledAgentsView {
            agents,
            default_agent_id,
        })
    }

    /// Project actions configured for `project_id` — the merged
    /// list of per-project + global custom actions, in the same
    /// order GPUI's titlebar split-button dropdown renders. Per-
    /// project entries override global ones with the same id (so
    /// "save global copy" can later be undone by deleting the
    /// project entry).
    ///
    /// Returns an empty list when `project_id` is unknown — matches
    /// `ProjectStore::project_actions` behaviour for that case.
    pub async fn list_project_actions(
        &self,
        project_id: String,
    ) -> anyhow::Result<Vec<ProjectActionDto>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("list_project_actions: set_local_registry not called")
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!("list_project_actions: RegistryState mutex poisoned")
        })?;
        Ok(state
            .project_store
            .project_actions(&project_id)
            .into_iter()
            .map(project_action_to_dto)
            .collect())
    }

    /// Insert or update one custom action.
    ///
    /// `save_global_copy=true` saves to `UiState::global_actions`
    /// (visible across every project on this host) and removes any
    /// per-project copy with the same id. `false` saves to the
    /// project's own `actions` list and removes any global copy with
    /// the same id. Mirrors `ProjectStore::upsert_project_action`.
    ///
    /// `dto.id` may be empty when editing a brand-new action — the
    /// bridge generates a uuid in that case so callers don't have to
    /// reach for `uuid` from Dart.
    pub async fn save_project_action(
        &self,
        project_id: String,
        action: ProjectActionDto,
        save_global_copy: bool,
    ) -> anyhow::Result<()> {
        let core_action = project_action_from_dto(action)?;
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("save_project_action: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("save_project_action: RegistryState mutex poisoned")
        })?;
        state
            .project_store
            .upsert_project_action(&project_id, core_action, save_global_copy)
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Remove one custom action by id from both the project's own
    /// list and `UiState::global_actions` (whichever currently holds
    /// it). Returns `true` if anything was removed.
    pub async fn delete_project_action(
        &self,
        project_id: String,
        action_id: String,
    ) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("delete_project_action: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("delete_project_action: RegistryState mutex poisoned")
        })?;
        Ok(state
            .project_store
            .delete_project_action(&project_id, &action_id))
    }

    /// Run one custom action inside `section_id`'s task.
    ///
    /// Mirrors `desktop/src/app.rs::run_project_action_in_section`:
    /// builds the launch config (shell or agent kind), appends a
    /// fresh `PersistedTerminalTab` to the section, sets it as the
    /// active tab, queues a `TabLaunchRequest` for the daemon's
    /// drain to spawn, and (for shell actions) records `command\n`
    /// in `pending_post_launch_input` so the drain writes it once
    /// the PTY is up.
    ///
    /// Note (2026-04-25): the bridge's `embedded_daemon` does not
    /// drain `pending_tab_launches` yet (tracked as `another-one-v0k`).
    /// Every other call site already queues there; once the drain
    /// lands, custom-action shell runs come up end-to-end. Until
    /// then this verb commits the tab to the persistent store and
    /// queues — visually correct, but no PTY spawns.
    ///
    /// Returns the new tab id so the caller can switch to it.
    pub async fn run_project_action(
        &self,
        project_id: String,
        section_id: String,
        action_id: String,
    ) -> anyhow::Result<String> {
        let key_section = another_one_core::section::SectionId::from_store_key(&section_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "run_project_action: malformed section_id `{section_id}` — expected SectionId::store_key()"
                )
            })?;
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("run_project_action: set_local_registry not called")
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("run_project_action: RegistryState mutex poisoned")
        })?;

        let action = state
            .project_store
            .project_actions(&project_id)
            .into_iter()
            .find(|a| a.id == action_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "run_project_action: unknown action_id `{action_id}` for project `{project_id}`"
                )
            })?;

        let (launch_config, post_launch_input, fixed_title) = match &action.kind {
            ProjectActionKind::Shell { command } => {
                let trimmed = command.trim();
                if trimmed.is_empty() {
                    anyhow::bail!(
                        "Shell actions need a command before they can run."
                    );
                }
                let title = {
                    let name = action.name.trim();
                    (!name.is_empty()).then(|| name.to_string())
                };
                (
                    another_one_core::agents::TerminalLaunchConfig::default(),
                    Some(format!("{trimmed}\n").into_bytes()),
                    title,
                )
            }
            ProjectActionKind::Agent { provider, .. } => {
                let args =
                    another_one_core::project_store::project_action_agent_launch_args(&action)
                        .map_err(|e| anyhow::anyhow!(e))?;
                (
                    another_one_core::agents::TerminalLaunchConfig::for_provider(*provider)
                        .with_extra_args(args)
                        .with_agent_launch_args(false),
                    None,
                    None,
                )
            }
        };

        let tab_id = uuid::Uuid::new_v4().to_string();
        let title = fixed_title
            .clone()
            .unwrap_or_else(|| launch_config.default_title());
        let tab = PersistedTerminalTab {
            id: tab_id.clone(),
            title,
            pinned: false,
            fixed_title,
            provider: launch_config.provider,
            launch_config: Some(launch_config.clone()),
            restore_status: another_one_core::agents::TerminalRestoreStatus::Launching,
        };

        let key = TerminalRuntimeKey {
            section_id: key_section,
            tab_id: tab_id.clone(),
        };

        let mut existing_section = state
            .project_store
            .terminal_sections
            .get(&section_id)
            .cloned()
            .unwrap_or_else(|| PersistedSectionState {
                active_tab_id: String::new(),
                next_tab_id: 1,
                cwd: None,
                tabs: Vec::new(),
            });
        existing_section.tabs.push(tab);
        existing_section.active_tab_id = tab_id.clone();
        existing_section.next_tab_id = existing_section.next_tab_id.saturating_add(1);
        state
            .project_store
            .set_section_state(section_id.clone(), existing_section);

        if let Some(input) = post_launch_input {
            state
                .pending_post_launch_input
                .insert(key.clone(), input);
        }
        state
            .pending_tab_launches
            .push(TabLaunchRequest { key });

        Ok(tab_id)
    }

    /// Snapshot the active project's branch metadata: current
    /// branch, ahead/behind counts. Powers the titlebar git-actions
    /// split-button's primary-action selection (Commit when there
    /// are changes, Push when ahead, Pull when behind, Fetch
    /// otherwise — the changes-vs-clean side comes from
    /// `read_changed_files`).
    ///
    /// Reads through `read_project_git_state` with
    /// `include_metadata=true` (so ahead/behind populate) inside
    /// `spawn_blocking`. Returns `Ok(None)` for unknown projects.
    pub async fn read_active_git_state(
        &self,
        project_id: String,
    ) -> anyhow::Result<Option<ActiveGitStateDto>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "read_active_git_state: set_local_registry not called"
            )
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!(
                    "read_active_git_state: RegistryState mutex poisoned"
                )
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        };
        let Some(project_path) = project_path else {
            return Ok(None);
        };
        let state = tokio::task::spawn_blocking(move || {
            another_one_core::project_store::read_project_git_state(
                &project_path,
                true,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("read_active_git_state join: {e}"))?;
        Ok(Some(ActiveGitStateDto {
            current_branch: state.current_branch,
            ahead_count: state.ahead_count as u32,
            behind_count: state.behind_count as u32,
        }))
    }

    /// Resolve the latest PullRequest status for `project_id`'s
    /// current branch (open PR's number + url + state). Powers the
    /// titlebar git-actions dropdown's Create PR / Draft PR
    /// enabledness gate.
    ///
    /// Routes [`another_one_core::git_actions::find_latest_pull_request_status`]
    /// inside `spawn_blocking`. Returns `Ok(None)` when the project
    /// is unknown or no PR exists for the branch.
    pub async fn find_pull_request_status(
        &self,
        project_id: String,
    ) -> anyhow::Result<Option<PullRequestStatusDto>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "find_pull_request_status: set_local_registry not called"
            )
        })?;
        let project_path_and_branch = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!(
                    "find_pull_request_status: RegistryState mutex poisoned"
                )
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .and_then(|project| {
                    project
                        .checkout
                        .current_branch
                        .clone()
                        .map(|branch| (project.path.clone(), branch))
                })
        };
        let Some((project_path, head_branch)) = project_path_and_branch else {
            return Ok(None);
        };
        let result = tokio::task::spawn_blocking(move || {
            another_one_core::git_actions::find_latest_pull_request_status(
                &project_path,
                &head_branch,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("find_pull_request_status join: {e}"))?;
        Ok(result.map(|pr| PullRequestStatusDto {
            number: pr.number,
            url: pr.url,
            state: match pr.state {
                another_one_core::git_actions::PullRequestState::Open => {
                    PullRequestStateDto::Open
                }
                another_one_core::git_actions::PullRequestState::Closed => {
                    PullRequestStateDto::Closed
                }
                another_one_core::git_actions::PullRequestState::Merged => {
                    PullRequestStateDto::Merged
                }
            },
        }))
    }

    /// Run a toolbar git action (Commit, Push, Pull, etc.) against
    /// `project_id`. Routes
    /// [`another_one_core::git_actions::execute_toolbar_git_action`]
    /// via spawn_blocking.
    ///
    /// `action_id` is one of: `"commit"`, `"commit-and-push"`,
    /// `"undo-last-commit"`, `"fetch"`, `"pull"`, `"push"`,
    /// `"force-push"`, `"create-pr"`, `"create-draft-pr"`. Other
    /// values error.
    ///
    /// Returns the toast message + warning/refresh flags so the UI
    /// can surface a snackbar and decide whether to invalidate
    /// changedFilesProvider / activeGitStateProvider.
    pub async fn run_toolbar_git_action(
        &self,
        project_id: String,
        action_id: String,
    ) -> anyhow::Result<ToolbarActionOutcomeDto> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "run_toolbar_git_action: set_local_registry not called"
            )
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!(
                    "run_toolbar_git_action: RegistryState mutex poisoned"
                )
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "run_toolbar_git_action: unknown project_id `{project_id}`"
                    )
                })?
        };
        let action = parse_toolbar_action_id(&action_id)?;
        let outcome = tokio::task::spawn_blocking(move || {
            let mut on_progress = |_msg: String| {};
            another_one_core::git_actions::execute_toolbar_git_action(
                &project_path,
                action,
                another_one_core::git_actions::GitActionSettings::default(),
                &mut on_progress,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("run_toolbar_git_action join: {e}"))?;
        outcome
            .map(|o| ToolbarActionOutcomeDto {
                toast_message: o.toast_message,
                warning: o.warning,
                refresh_git_state: o.refresh_git_state,
            })
            .map_err(|err| anyhow::anyhow!(err.message))
    }

    /// Diff the project's current branch against `target_branch`
    /// (= `target..HEAD`). Powers the right sidebar's Compare pane.
    /// Routes
    /// [`another_one_core::project_store::read_project_branch_compare_state`]
    /// inside `spawn_blocking`.
    ///
    /// Returns `Ok(None)` for unknown projects. Errors propagate
    /// from git when the target branch doesn't exist or the diff
    /// invocation fails.
    pub async fn read_branch_compare_state(
        &self,
        project_id: String,
        target_branch: String,
    ) -> anyhow::Result<Option<BranchCompareView>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "read_branch_compare_state: set_local_registry not called"
            )
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!(
                    "read_branch_compare_state: RegistryState mutex poisoned"
                )
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        };
        let Some(project_path) = project_path else {
            return Ok(None);
        };
        let result = tokio::task::spawn_blocking(move || {
            another_one_core::project_store::read_project_branch_compare_state(
                &project_path,
                &target_branch,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("read_branch_compare_state join: {e}"))?
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Some(BranchCompareView {
            current_branch: result.current_branch,
            target_branch: result.target_branch,
            files: result.files.into_iter().map(branch_compare_file_to_dto).collect(),
        }))
    }

    /// Snapshot the resolved branch settings for `project_id` —
    /// configured + effective values for default and target branch
    /// plus the available branch list for the dropdown.
    /// Returns `Ok(None)` when the project is unknown or has no
    /// repo metadata yet.
    pub async fn resolved_branch_settings(
        &self,
        project_id: String,
    ) -> anyhow::Result<Option<ResolvedProjectBranchSettingsDto>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "resolved_branch_settings: set_local_registry not called"
            )
        })?;
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "resolved_branch_settings: RegistryState mutex poisoned"
            )
        })?;
        Ok(state
            .project_store
            .resolved_branch_settings(&project_id)
            .map(|s| ResolvedProjectBranchSettingsDto {
                root_project_id: s.root_project_id,
                available_branches: s.available_branches,
                configured_default_branch: s.configured_default_branch,
                effective_default_branch: s.effective_default_branch,
                configured_default_target_branch: s.configured_default_target_branch,
                effective_default_target_branch: s.effective_default_target_branch,
            }))
    }

    /// Update the configured default branch or default-target branch
    /// for `project_id`'s root project. `field` must be one of
    /// `"default-branch"` or `"default-target-branch"`. `branch_name`
    /// of `None` clears the configured override (resolved-effective
    /// goes back to automatic).
    ///
    /// Returns `Ok(true)` when the persisted store changed,
    /// `Ok(false)` for a no-op re-set. Errors when the branch isn't
    /// in the available list (matches GPUI's validation), or when
    /// the project lookup fails.
    pub async fn set_project_branch_setting(
        &self,
        project_id: String,
        field: String,
        branch_name: Option<String>,
    ) -> anyhow::Result<bool> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "set_project_branch_setting: set_local_registry not called"
            )
        })?;
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!(
                "set_project_branch_setting: RegistryState mutex poisoned"
            )
        })?;
        let result = match field.as_str() {
            "default-branch" => state
                .project_store
                .update_default_branch(&project_id, branch_name),
            "default-target-branch" => state
                .project_store
                .update_default_target_branch(&project_id, branch_name),
            other => Err(format!(
                "set_project_branch_setting: unknown field `{other}`"
            )),
        };
        result.map_err(|e| anyhow::anyhow!(e))
    }

    /// Per-commit file change list for `project_id` / `commit_id`.
    /// Powers the right-sidebar Commits pane's expandable per-row
    /// file list. Routes
    /// [`another_one_core::project_store::read_project_commit_file_changes`]
    /// inside `spawn_blocking` so the FRB caller's tokio runtime
    /// stays free.
    ///
    /// Returns `Ok(None)` when the project id is unknown — UI shows
    /// the "Couldn't load file changes" empty state in that case.
    /// Errors propagate from git (commit not in tree, etc.).
    pub async fn read_commit_file_changes(
        &self,
        project_id: String,
        commit_id: String,
    ) -> anyhow::Result<Option<Vec<BranchCompareFileDto>>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "read_commit_file_changes: set_local_registry not called"
            )
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!(
                    "read_commit_file_changes: RegistryState mutex poisoned"
                )
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        };
        let Some(project_path) = project_path else {
            return Ok(None);
        };
        let result = tokio::task::spawn_blocking(move || {
            another_one_core::project_store::read_project_commit_file_changes(
                &project_path,
                &commit_id,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("read_commit_file_changes join: {e}"))?
        .map_err(|e| anyhow::anyhow!(e))?;
        Ok(Some(
            result.files.into_iter().map(branch_compare_file_to_dto).collect(),
        ))
    }

    /// Pull-request CI checks for `project_id`'s current branch.
    /// Powers the right sidebar's Checks pane. Calls into
    /// [`another_one_core::git_actions::find_pull_request_checks`]
    /// (which shells out to `gh pr checks`) inside `spawn_blocking`.
    ///
    /// Three-state return:
    ///   * `Ok(Some(list))` — the PR exists and these are its checks
    ///     (may be empty when no checks are configured).
    ///   * `Ok(None)` — no PR for the current branch, or the project
    ///     id is unknown. UI shows the empty state.
    ///   * `Err(_)` — gh CLI missing, network failure, or any other
    ///     hard error. UI surfaces the message.
    pub async fn read_pull_request_checks(
        &self,
        project_id: String,
    ) -> anyhow::Result<Option<Vec<CheckDto>>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!(
                "read_pull_request_checks: set_local_registry not called"
            )
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!(
                    "read_pull_request_checks: RegistryState mutex poisoned"
                )
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        };
        let Some(project_path) = project_path else {
            return Ok(None);
        };
        let result = tokio::task::spawn_blocking(move || {
            another_one_core::git_actions::find_pull_request_checks(
                &project_path,
                None,
            )
        })
        .await
        .map_err(|e| anyhow::anyhow!("read_pull_request_checks join: {e}"))?;
        match result {
            Ok(Some(checks)) => {
                Ok(Some(checks.into_iter().map(check_to_dto).collect()))
            }
            Ok(None) => Ok(None),
            Err(message) => Err(anyhow::anyhow!(message)),
        }
    }

    /// Resolve the GitHub remote URL for a project by shelling out to
    /// `git remote get-url origin` and normalising the result through
    /// [`another_one_core::git_actions::find_github_repo_url`].
    /// Returns `None` if the project isn't tracked, has no `origin`
    /// remote, or the remote isn't a github.com URL.
    ///
    /// The blocking git invocation runs in `spawn_blocking` so the
    /// FRB caller's tokio runtime stays free. Dart caches the
    /// result per-project — there's no expectation of liveness if
    /// the user changes the remote at runtime, matching the GPUI
    /// build's "look up once at boot" behaviour
    /// (`spawn_github_link_lookup` in `core::git_service`).
    pub async fn read_project_github_url(
        &self,
        project_id: String,
    ) -> anyhow::Result<Option<String>> {
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("read_project_github_url: set_local_registry not called")
        })?;
        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!("read_project_github_url: RegistryState mutex poisoned")
            })?;
            state
                .project_store
                .projects
                .iter()
                .find(|project| project.id == project_id)
                .map(|project| project.path.clone())
        };
        let Some(project_path) = project_path else {
            return Ok(None);
        };
        let url = tokio::task::spawn_blocking(move || {
            another_one_core::git_actions::find_github_repo_url(&project_path)
        })
        .await
        .map_err(|e| anyhow::anyhow!("github url lookup join: {e}"))?;
        Ok(url)
    }

    /// Snapshot of the host's "Open In" configuration: which apps
    /// are enabled (intersection of installed-on-host with the user's
    /// configured set) and which one was last picked as the
    /// preferred default.
    ///
    /// The titlebar split-button uses `preferred_app_id` for its
    /// primary-action icon and `enabled_apps` for the chevron
    /// dropdown. Both are global across projects — `project_id` only
    /// matters when actually launching, not when rendering the
    /// chrome.
    ///
    /// Cheap to call repeatedly: the install detection runs through
    /// `<CurrentPlatform as HeadlessPlatform>::is_open_in_app_available`
    /// (a `$PATH` walk on Linux/Windows, bundle existence on macOS),
    /// and the project store read is a single mutex acquisition.
    pub async fn open_in_state(&self) -> anyhow::Result<OpenInState> {
        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("open_in_state: set_local_registry not called"))?;
        let state = registry
            .lock()
            .map_err(|_| anyhow::anyhow!("open_in_state: RegistryState mutex poisoned"))?;

        let available = available_open_in_apps();
        let enabled_apps = state
            .project_store
            .enabled_open_in_apps(&available)
            .into_iter()
            .map(open_in_app_to_dto)
            .collect();
        let preferred_app_id = state
            .project_store
            .preferred_open_in_app(&available)
            .map(|app| app.id().to_string());

        Ok(OpenInState {
            enabled_apps,
            preferred_app_id,
        })
    }

    /// Open a project's directory in the named app and record it as
    /// the user's preferred default. Mirrors GPUI's
    /// `App::open_project_directory_in_app`: spawn the platform
    /// command, then on success persist `preferred_open_in_app` so
    /// the next titlebar click goes there directly.
    ///
    /// The "spawn first, save preferred only on success" ordering
    /// matches GPUI — a failed spawn doesn't leave the preferred
    /// pointing at a broken target.
    pub async fn open_project_in_app(
        &self,
        project_id: String,
        app_id: String,
    ) -> anyhow::Result<()> {
        let app = parse_open_in_app_id(&app_id).ok_or_else(|| {
            anyhow::anyhow!("open_project_in_app: unknown app id `{app_id}`")
        })?;
        let registry = local_registry().ok_or_else(|| {
            anyhow::anyhow!("open_project_in_app: set_local_registry not called")
        })?;

        let project_path = {
            let state = registry.lock().map_err(|_| {
                anyhow::anyhow!("open_project_in_app: RegistryState mutex poisoned")
            })?;
            state
                .project_store
                .project(&project_id)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "open_project_in_app: unknown project_id `{project_id}`"
                    )
                })?
                .path
                .clone()
        };

        let mut command = <CurrentPlatform as HeadlessPlatform>::command_for_open_in(
            app,
            &project_path,
        );
        command
            .spawn()
            .map_err(|err| anyhow::anyhow!("open in {}: {err}", app.label()))?;

        let available = available_open_in_apps();
        let mut state = registry.lock().map_err(|_| {
            anyhow::anyhow!("open_project_in_app: RegistryState mutex poisoned")
        })?;
        state
            .project_store
            .set_preferred_open_in_app(app, &available);
        Ok(())
    }

    /// Subscribe to live PTY bytes for `(section_id, tab_id)`.
    ///
    /// Replaces any previous attachment: aborts the previous
    /// forwarder, clears this viewer's entry from the previous
    /// tab's `active_viewers`, then subscribes to the new tab's
    /// broadcast and spawns a forwarder that drains it into
    /// `incoming_tx`. The new viewport size has to be set via
    /// [`Self::tab_resize`] after attach — bytes start flowing
    /// immediately, but no resize is implied.
    ///
    /// Errors if the tab isn't running (no `broadcasts` entry); the
    /// caller should `launch_tab` first.
    pub async fn attach_tab(
        &self,
        section_id: String,
        tab_id: String,
    ) -> anyhow::Result<()> {
        let key = key_from_wire(&section_id, &tab_id).ok_or_else(|| {
            anyhow::anyhow!(
                "attach_tab: malformed section_id `{section_id}` — expected SectionId::store_key()"
            )
        })?;
        let registry = local_registry()
            .ok_or_else(|| anyhow::anyhow!("attach_tab: set_local_registry not called"))?;

        // Tear down any previous attachment first so its forwarder
        // stops pushing bytes from the old tab into the shared
        // incoming channel.
        self.detach_internal();

        // Subscribe to the new tab's broadcast under a brief lock so
        // we can safely both read `broadcasts` and update viewer
        // tracking in one critical section.
        //
        // Race tolerance: a callsite that fires `launch_tab` and
        // immediately `attach_tab` can arrive here before the
        // pending-launch queue has been drained and the broadcast
        // sender published. Poll briefly (~500ms total, 25ms
        // intervals) before giving up so a normal launch+attach
        // pair on the in-process FFI path doesn't race-fail. iroh
        // peers absorb this delay in QUIC RTT; local callers don't.
        let mut broadcast_rx = {
            let mut sender_opt = None;
            for _ in 0..20 {
                // Tight scope: the MutexGuard must drop *before* the
                // `.await` below, otherwise the enclosing future
                // becomes !Send (std::sync::MutexGuard is !Send) and
                // FRB's tokio executor refuses to schedule it.
                {
                    let mut state = registry.lock().map_err(|_| {
                        anyhow::anyhow!("attach_tab: RegistryState mutex poisoned")
                    })?;
                    if let Some(s) = state.broadcasts.get(&key).cloned() {
                        state
                            .viewer_focus
                            .insert(self.viewer_id.clone(), key.clone());
                        sender_opt = Some(s);
                        break;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(25)).await;
            }
            let sender = sender_opt.ok_or_else(|| {
                anyhow::anyhow!(
                    "attach_tab: tab is not running yet — call launch_tab first"
                )
            })?;
            sender.subscribe()
        };

        let incoming_tx = {
            let guard = self
                .incoming_tx
                .lock()
                .expect("incoming_tx mutex poisoned");
            guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("attach_tab: session closed"))?
                .clone()
        };

        // Drain the broadcast into the session's incoming channel.
        // `Lagged` is treated as a skip (matches iroh side); `Closed`
        // ends the loop, which happens when the tab's PTY exits and
        // the broadcast sender is dropped.
        let join = tokio_rt().spawn(async move {
            loop {
                match broadcast_rx.recv().await {
                    Ok(bytes) => {
                        if incoming_tx.send(bytes).is_err() {
                            break;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(broadcast::error::RecvError::Closed) => break,
                }
            }
        });

        let mut attached = self.attached.lock().expect("attached mutex poisoned");
        *attached = Some(AttachedTab {
            key,
            forwarder: join.abort_handle(),
        });
        Ok(())
    }

    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent.
    pub async fn detach_tab(&self) -> anyhow::Result<()> {
        self.detach_internal();
        Ok(())
    }

    /// Ask the daemon to spawn the given tab's PTY if it isn't
    /// already running. See [`Self`] doc for the launch flow.
    pub async fn launch_tab(
        &self,
        section_id: String,
        tab_id: String,
    ) -> anyhow::Result<()> {
        let key = key_from_wire(&section_id, &tab_id).ok_or_else(|| {
            anyhow::anyhow!(
                "launch_tab: malformed section_id `{section_id}` — expected SectionId::store_key()"
            )
        })?;
        let registry = match local_registry() {
            Some(r) => r,
            None => return Ok(()),
        };
        let mut state = registry
            .lock()
            .map_err(|_| anyhow::anyhow!("launch_tab: RegistryState mutex poisoned"))?;
        state.pending_tab_launches.push(TabLaunchRequest { key });
        Ok(())
    }

    /// Stream PTY bytes for the attached tab into a Dart sink.
    /// One-shot subscription; the second call returns
    /// "already subscribed".
    pub async fn subscribe(&self, sink: StreamSink<Vec<u8>>) -> anyhow::Result<()> {
        let mut rx = {
            let mut guard = self
                .incoming_rx
                .lock()
                .expect("incoming_rx mutex poisoned");
            guard
                .take()
                .ok_or_else(|| anyhow::anyhow!("already subscribed"))?
        };

        tokio_rt().spawn(async move {
            while let Some(bytes) = rx.recv().await {
                if sink.add(bytes).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    /// Stream worker replies into a Dart sink. One-shot.
    pub async fn subscribe_worker_replies(
        &self,
        sink: StreamSink<WorkerReply>,
    ) -> anyhow::Result<()> {
        let mut rx = {
            let mut guard = self
                .worker_replies_rx
                .lock()
                .expect("worker_replies_rx mutex poisoned");
            guard
                .take()
                .ok_or_else(|| anyhow::anyhow!("already subscribed to worker replies"))?
        };

        tokio_rt().spawn(async move {
            while let Some(reply) = rx.recv().await {
                if sink.add(reply).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    /// Close the session: detaches any attached tab, drops both
    /// channel senders so active subscriptions exit, and clears
    /// per-viewer state on the registry. Idempotent.
    pub async fn close(&self) {
        self.detach_internal();
        if let Ok(mut guard) = self.worker_replies_tx.lock() {
            guard.take();
        }
        if let Ok(mut guard) = self.incoming_tx.lock() {
            guard.take();
        }
    }

    /// Synchronous half of `detach_tab` — also called by `attach_tab`
    /// (to replace a previous attachment) and `close`. Best-effort:
    /// poisoned mutexes silently no-op rather than propagate.
    fn detach_internal(&self) {
        let prev = match self.attached.lock() {
            Ok(mut guard) => guard.take(),
            Err(_) => return,
        };
        let Some(prev) = prev else {
            return;
        };
        prev.forwarder.abort();
        let Some(registry) = local_registry() else {
            return;
        };
        if let Ok(mut state) = registry.lock() {
            state.viewer_focus.remove(&self.viewer_id);
            if let Some(viewers) = state.active_viewers.get_mut(&prev.key) {
                viewers.remove(&self.viewer_id);
            }
            state.recompute_effective_size(&prev.key);
        }
    }

    fn attached_key(&self) -> anyhow::Result<TerminalRuntimeKey> {
        let attached = self
            .attached
            .lock()
            .map_err(|_| anyhow::anyhow!("attached mutex poisoned"))?;
        attached
            .as_ref()
            .map(|a| a.key.clone())
            .ok_or_else(|| anyhow::anyhow!("no tab attached"))
    }
}

/// Flatten the desktop's `RegistryState` into the bridge's
/// `ProjectSummary` / `TaskSummary` / `TabSummary` shape. Mirrors
/// `desktop/src/daemon_host.rs::project_summaries` so the LocalSession
/// path matches what iroh clients see.
///
/// Worktree-kind projects are filtered out — they're nested under
/// their root via `Task.worktree_project_id` and shouldn't appear at
/// the top level of the mobile drawer / desktop sidebar.
fn flatten_project_store(state: &RegistryState) -> Vec<ProjectSummary> {
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
                .map(|task| {
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
                    // Diff stats + last_commit_relative come from
                    // `branch_view`, which only fills the lines_added/
                    // lines_removed pair when the queried branch is
                    // the *project's* current branch. For a worktree
                    // task that's the worktree project, not the root —
                    // so query against `target_project_id` (which
                    // points at the worktree project for worktree
                    // tasks and at the root for non-worktree tasks).
                    let branch_view =
                        store.branch_view(&task.target_project_id, &task.branch_name);
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
                })
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

/// Common scaffolding for the four stage/unstage verbs: resolve
/// the project path off the registry, hop into `spawn_blocking`
/// for the git shell-out, surface the core helper's `Result<_,
/// String>` as `anyhow::Result`. Pulled out so each verb stays a
/// thin wrapper.
async fn run_changed_file_action<F>(project_id: &str, action: F) -> anyhow::Result<()>
where
    F: FnOnce(std::path::PathBuf) -> Result<(), String> + Send + 'static,
{
    let registry = local_registry().ok_or_else(|| {
        anyhow::anyhow!("changed-file action: set_local_registry not called")
    })?;
    let project_path = {
        let state = registry.lock().map_err(|_| {
            anyhow::anyhow!("changed-file action: RegistryState mutex poisoned")
        })?;
        state
            .project_store
            .projects
            .iter()
            .find(|project| project.id == project_id)
            .map(|project| project.path.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "changed-file action: unknown project_id `{project_id}`"
                )
            })?
    };
    tokio::task::spawn_blocking(move || action(project_path))
        .await
        .map_err(|e| anyhow::anyhow!("changed-file action join: {e}"))?
        .map_err(|e| anyhow::anyhow!(e))
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::ToolbarActionOutcome`]. The
/// titlebar surfaces `toast_message` as a snackbar (warning palette
/// when `warning` is true) and uses `refresh_git_state` to decide
/// whether to invalidate the active changed-files / git-state
/// providers after the call returns.
#[derive(Debug, Clone)]
pub struct ToolbarActionOutcomeDto {
    pub toast_message: String,
    pub warning: bool,
    pub refresh_git_state: bool,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::PullRequestStatus`]. Drives
/// the titlebar dropdown's Create PR / Draft PR enabledness — when
/// a PR already exists for the branch, those rows are disabled.
#[derive(Debug, Clone)]
pub struct PullRequestStatusDto {
    pub number: u64,
    pub url: String,
    pub state: PullRequestStateDto,
}

/// Branch metadata snapshot for the active project — current branch
/// name plus ahead/behind counts. Drives the titlebar's
/// idle-primary-action selection.
#[derive(Debug, Clone)]
pub struct ActiveGitStateDto {
    pub current_branch: Option<String>,
    pub ahead_count: u32,
    pub behind_count: u32,
}

/// Action-id → core enum for [`LocalSession::run_toolbar_git_action`].
/// Kept local since only the bridge consumes the string form.
fn parse_toolbar_action_id(
    id: &str,
) -> anyhow::Result<another_one_core::git_actions::ToolbarGitAction> {
    use another_one_core::git_actions::ToolbarGitAction;
    Ok(match id {
        "commit" => ToolbarGitAction::Commit,
        "commit-and-push" => ToolbarGitAction::CommitAndPush,
        "undo-last-commit" => ToolbarGitAction::UndoLastCommit,
        "fetch" => ToolbarGitAction::Fetch,
        "pull" => ToolbarGitAction::Pull,
        "push" => ToolbarGitAction::Push { force: false },
        "force-push" => ToolbarGitAction::Push { force: true },
        "create-pr" => ToolbarGitAction::CreatePr {
            draft: false,
            base_branch: None,
        },
        "create-draft-pr" => ToolbarGitAction::CreatePr {
            draft: true,
            base_branch: None,
        },
        other => {
            return Err(anyhow::anyhow!(
                "run_toolbar_git_action: unknown action_id `{other}`"
            ))
        }
    })
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectBranchCompareState`].
/// Drives the right sidebar's Compare pane: the current branch +
/// configured target + the file list of the diff.
#[derive(Debug, Clone)]
pub struct BranchCompareView {
    pub current_branch: Option<String>,
    pub target_branch: String,
    pub files: Vec<BranchCompareFileDto>,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::PullRequestState`]. Drives the
/// chip + chrome on each PR row: open vs merged vs closed shapes
/// the badge palette and the row-level affordances.
#[derive(Debug, Clone, Copy)]
pub enum PullRequestStateDto {
    Open,
    Closed,
    Merged,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::ProjectPagePullRequest`]. One
/// entry per row in the project page's Open PRs section.
#[derive(Debug, Clone)]
pub struct ProjectPagePullRequestDto {
    pub number: u64,
    pub url: String,
    pub title: String,
    /// Head ref (the PR's source branch). Rendered in mono on the
    /// row's bottom line.
    pub branch: String,
    pub author: String,
    pub lines_added: i32,
    pub lines_removed: i32,
    pub draft: bool,
    /// `true` when GitHub's review_decision is REVIEW_REQUIRED —
    /// drives the red CI badge and 'Review required' chip.
    pub review_required: bool,
    pub review_requested_to_me: bool,
    pub created_by_me: bool,
    pub state: PullRequestStateDto,
}

fn pr_to_dto(
    pr: another_one_core::git_actions::ProjectPagePullRequest,
) -> ProjectPagePullRequestDto {
    ProjectPagePullRequestDto {
        number: pr.number,
        url: pr.url,
        title: pr.title,
        branch: pr.branch,
        author: pr.author,
        lines_added: pr.lines_added,
        lines_removed: pr.lines_removed,
        draft: pr.draft,
        review_required: pr.review_required,
        review_requested_to_me: pr.review_requested_to_me,
        created_by_me: pr.created_by_me,
        state: match pr.state {
            another_one_core::git_actions::PullRequestState::Open => {
                PullRequestStateDto::Open
            }
            another_one_core::git_actions::PullRequestState::Closed => {
                PullRequestStateDto::Closed
            }
            another_one_core::git_actions::PullRequestState::Merged => {
                PullRequestStateDto::Merged
            }
        },
    }
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ResolvedProjectBranchSettings`].
/// Drives the project page Configuration panel: the current
/// configured + effective values for both fields, plus the
/// available branch list the dropdowns enumerate.
#[derive(Debug, Clone)]
pub struct ResolvedProjectBranchSettingsDto {
    pub root_project_id: String,
    pub available_branches: Vec<String>,
    /// `Some(name)` when the user explicitly picked a default branch;
    /// `None` means automatic (UI shows the trigger label as
    /// "Automatic").
    pub configured_default_branch: Option<String>,
    /// What the project actually uses today — falls back to
    /// `automatic_primary_branch_name` when configured is None or
    /// unavailable.
    pub effective_default_branch: Option<String>,
    pub configured_default_target_branch: Option<String>,
    pub effective_default_target_branch: Option<String>,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::BranchCompareFile`]. Each
/// entry is one file changed inside a commit (or branch compare).
/// `status` is the single git status char ('A', 'M', 'D', 'R', 'C',
/// 'T') passed through verbatim — UI maps it via the same
/// `changed_file_status_color` table the Changes pane uses.
#[derive(Debug, Clone)]
pub struct BranchCompareFileDto {
    pub path: String,
    /// Set on rename/copy entries — the from-path. UI renders
    /// "Renamed from {original_path}" beneath the row when present.
    pub original_path: Option<String>,
    /// Single status char as a 1-char string (FRB doesn't expose
    /// `char` directly).
    pub status: String,
    pub additions: i32,
    pub deletions: i32,
}

fn branch_compare_file_to_dto(
    f: another_one_core::project_store::BranchCompareFile,
) -> BranchCompareFileDto {
    BranchCompareFileDto {
        path: f.path,
        original_path: f.original_path,
        status: f.status.to_string(),
        additions: f.additions,
        deletions: f.deletions,
    }
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::PullRequestCheckBucket`].
/// Drives the glyph + colour for each check row on the right
/// sidebar's Checks pane.
#[derive(Debug, Clone, Copy)]
pub enum CheckBucket {
    Pass,
    Fail,
    Pending,
    Skipping,
    Cancel,
}

/// FRB-friendly mirror of
/// [`another_one_core::git_actions::PullRequestCheck`]. Mostly raw
/// — UI maps `bucket` to glyph/colour and `state` is the verbatim
/// string `gh pr checks` returned ("pass", "in_progress", etc.).
#[derive(Debug, Clone)]
pub struct CheckDto {
    /// Check name (e.g. "build / linux", "lint").
    pub name: String,
    /// Raw state string from gh CLI; shown as the row subtitle.
    pub state: String,
    pub bucket: CheckBucket,
    /// Optional human description gh CLI sometimes provides.
    pub description: Option<String>,
    /// Link to the check run page on GitHub. UI renders the row
    /// clickable when set.
    pub link: Option<String>,
    /// Pre-formatted "1m 23s"-style duration. None for checks that
    /// haven't started or completed.
    pub duration_text: Option<String>,
}

fn check_to_dto(c: another_one_core::git_actions::PullRequestCheck) -> CheckDto {
    CheckDto {
        name: c.name,
        state: c.state,
        bucket: match c.bucket {
            another_one_core::git_actions::PullRequestCheckBucket::Pass => {
                CheckBucket::Pass
            }
            another_one_core::git_actions::PullRequestCheckBucket::Fail => {
                CheckBucket::Fail
            }
            another_one_core::git_actions::PullRequestCheckBucket::Pending => {
                CheckBucket::Pending
            }
            another_one_core::git_actions::PullRequestCheckBucket::Skipping => {
                CheckBucket::Skipping
            }
            another_one_core::git_actions::PullRequestCheckBucket::Cancel => {
                CheckBucket::Cancel
            }
        },
        description: c.description,
        link: c.link,
        duration_text: c.duration_text,
    }
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::BranchCommit`]. Carries the
/// pre-computed relative authored timestamp ("3 hours ago") so the
/// UI doesn't have to round-trip through chrono on every redraw.
#[derive(Debug, Clone)]
pub struct CommitDto {
    /// Full SHA — used as the row id and for the diff lookup.
    pub id: String,
    /// 7-char abbreviated SHA shown next to the message.
    pub short_id: String,
    /// First line of the commit message.
    pub subject: String,
    pub author_name: String,
    /// Pre-formatted "X minutes ago"-style label. Computed Rust-side
    /// because chrono is already a dep there; doing it Dart-side
    /// would mean shipping a humanize-duration package for one
    /// caller. This is borderline display logic but the read is
    /// one-shot per pane open so the FFI cost is a wash.
    pub authored_relative: String,
}

fn commit_to_dto(c: another_one_core::project_store::BranchCommit) -> CommitDto {
    CommitDto {
        id: c.id,
        short_id: c.short_id,
        subject: c.subject,
        author_name: c.author_name,
        authored_relative: c.authored_relative,
    }
}

/// FRB-friendly snapshot of the right sidebar's Commits pane data.
#[derive(Debug, Clone)]
pub struct RecentCommitsView {
    /// Current branch — shown as the pane subtitle in GPUI.
    pub current_branch: Option<String>,
    /// True when more commits exist past the requested `limit`. UI
    /// uses this to render a "Load more" affordance.
    pub has_more: bool,
    pub commits: Vec<CommitDto>,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ChangedFile`]. Carries the
/// raw status chars + diff counts; UI maps them to glyphs/colours
/// per `desktop/src/right_sidebar.rs::changed_file_status_char`
/// and `changed_file_status_color`. We don't pre-format on the
/// Rust side so the bridge stays display-agnostic and we don't
/// pay the cross-FFI cost of re-encoding every redraw.
#[derive(Debug, Clone)]
pub struct ChangedFileDto {
    /// Path relative to the project root, the way `git status` reports it.
    pub path: String,
    /// Set on rename (`R`) / copy (`C`) entries — the from-path. UI
    /// renders this as `original → path` when present.
    pub original_path: Option<String>,
    pub staged_additions: i32,
    pub staged_deletions: i32,
    pub unstaged_additions: i32,
    pub unstaged_deletions: i32,
    /// Index status char from `git status --porcelain` — `M`/`A`/`D`/
    /// `R`/`C`/`?`/' '. UI maps via the GPUI char-to-glyph table.
    pub index_status: String,
    /// Worktree status char — same alphabet as `index_status`.
    pub worktree_status: String,
    /// True when the file is `??` (untracked) in `git status`.
    pub untracked: bool,
}

fn changed_file_to_dto(
    f: another_one_core::project_store::ChangedFile,
) -> ChangedFileDto {
    ChangedFileDto {
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

/// FRB-friendly mirror of [`OpenInAppKind`] with the
/// pre-computed display strings. Lives here (not in core) because
/// FRB's binding generator only walks bridge crate types — we'd
/// need a re-export shim either way and the mapping is one-to-one.
///
/// `Deserialize` exists so the iroh transport can decode the wire
/// payload (`OpenInAppWire` from `daemon-sandbox`) straight into this
/// type — the field names match by design. FRB ignores extra derives.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OpenInAppDto {
    /// Stable id matching `OpenInAppKind::id()` — `"cursor"`,
    /// `"zed"`, `"vscode"`, `"file-manager"`. Round-trips through
    /// [`LocalSession::open_project_in_app`].
    pub id: String,
    /// Human-readable label rendered in the dropdown. Localised at
    /// the platform level (Finder vs File Manager vs File Explorer).
    pub label: String,
    /// Tooltip text — same copy GPUI's titlebar dropdown uses.
    pub description: String,
    /// Asset path for the app's glyph, relative to the app bundle's
    /// asset root. Both the GPUI and Flutter UIs ship the same
    /// `assets/icons/open_in__*.svg` files, so the path is valid
    /// on either side without translation.
    pub icon_path: String,
}

/// Snapshot returned by [`LocalSession::open_in_state`].
///
/// `Deserialize` exists for the iroh transport; field names match
/// `daemon-sandbox::frame::OpenInStateWire` so the wire JSON decodes
/// directly into this DTO without a per-field map step.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct OpenInState {
    /// Apps offered in the dropdown, ordered as `OpenInAppKind::all()`
    /// declares them — Cursor, Zed, VS Code, File Manager.
    pub enabled_apps: Vec<OpenInAppDto>,
    /// Id of the app the titlebar's primary action launches. `None`
    /// when no app is enabled at all (a fresh install on a host with
    /// none of the editors detected).
    pub preferred_app_id: Option<String>,
}

/// Filter [`OpenInAppKind::all`] down to what the host says is
/// installed, preserving the canonical order.
fn available_open_in_apps() -> Vec<OpenInAppKind> {
    OpenInAppKind::all()
        .into_iter()
        .filter(|app| {
            <CurrentPlatform as HeadlessPlatform>::is_open_in_app_available(*app)
        })
        .collect()
}

fn open_in_app_to_dto(app: OpenInAppKind) -> OpenInAppDto {
    OpenInAppDto {
        id: app.id().to_string(),
        label: app.label().to_string(),
        description: app.description().to_string(),
        icon_path: app.icon_path().to_string(),
    }
}

/// Inverse of [`OpenInAppKind::id`]. Kept local — only the bridge
/// round-trips ids over FRB; no other consumer needs it.
fn parse_open_in_app_id(id: &str) -> Option<OpenInAppKind> {
    match id {
        "cursor" => Some(OpenInAppKind::Cursor),
        "zed" => Some(OpenInAppKind::Zed),
        "vscode" => Some(OpenInAppKind::VsCode),
        "file-manager" => Some(OpenInAppKind::FileManager),
        _ => None,
    }
}

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
        // The wire enum has a `Shell` variant for "no agent, just a
        // shell" — that maps to a default `TerminalLaunchConfig`, so
        // we never have to re-translate it back to a core
        // `AgentProviderKind`. The caller treats `Some(Shell)` like
        // `None` upstream of this fn; gate it here so the match is
        // exhaustive.
        AgentProvider::Shell => AgentProviderKind::ClaudeCode,
    }
}

/// FRB-friendly mirror of one entry in
/// [`another_one_core::agents::AGENTS`]. Carries everything the
/// new-task modal's agent multi-select needs to render a chip
/// (label + icon path) without the UI side hard-coding a copy.
///
/// `Deserialize` lets the iroh transport decode the daemon's
/// `AgentSummaryWire` directly into this DTO.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct AgentSummaryDto {
    /// Stable id used by the bridge's `submit_new_task` verb.
    pub id: String,
    pub label: String,
    pub icon_path: String,
    pub provider: Option<AgentProvider>,
}

/// Snapshot returned by [`LocalSession::read_enabled_agents`].
/// Pairs the enabled-agents list with the user's preferred default
/// (the chip the modal pre-checks on open).
///
/// `Deserialize` shape matches `daemon-sandbox::frame::EnabledAgentsViewWire`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct EnabledAgentsView {
    pub agents: Vec<AgentSummaryDto>,
    pub default_agent_id: Option<String>,
}

/// One row of the Settings → Agents page. Carries everything the
/// page renders (label + icon + enabled / default flags +
/// per-agent launch args list) so the UI can update its state
/// without re-issuing reads after every toggle.
#[derive(Debug, Clone)]
pub struct AgentSettingsRow {
    pub id: String,
    pub label: String,
    pub icon_path: String,
    pub provider: Option<AgentProvider>,
    pub enabled: bool,
    pub is_default: bool,
    pub launch_args: Vec<String>,
}

/// Snapshot returned by [`LocalSession::read_agent_settings`].
#[derive(Debug, Clone)]
pub struct AgentSettingsView {
    /// Every agent in `AGENTS` (canonical order), enabled-or-not.
    pub agents: Vec<AgentSettingsRow>,
    pub default_agent_id: Option<String>,
}

/// One row of the Settings → Open In page. Carries everything the
/// page renders (label + icon + description + per-host enabled
/// flag). UI maps these to clickable rows that toggle through
/// [`LocalSession::set_open_in_app_enabled`].
#[derive(Debug, Clone)]
pub struct OpenInAppSettingsRow {
    /// Stable id matching `OpenInAppKind::id()` — `"cursor"`,
    /// `"zed"`, `"vscode"`, `"file-manager"`.
    pub id: String,
    pub label: String,
    pub description: String,
    pub icon_path: String,
    pub enabled: bool,
}

/// Snapshot returned by [`LocalSession::read_open_in_settings`].
#[derive(Debug, Clone)]
pub struct OpenInSettingsView {
    /// Every Open-In app the host detected as installed, in
    /// canonical order. Empty when no supported app is on the
    /// host's PATH / installed.
    pub available_apps: Vec<OpenInAppSettingsRow>,
}

/// Snapshot returned by [`LocalSession::read_git_action_scripts`].
/// Carries the resolved-current text for both the commit and PR
/// scripts (built-in default when there's no override) plus a
/// `using_default` flag per script so the UI can flip the
/// subtitle copy without re-checking.
#[derive(Debug, Clone)]
pub struct GitActionScriptsView {
    pub commit_script: String,
    pub commit_using_default: bool,
    pub pr_script: String,
    pub pr_using_default: bool,
}

/// One row of the Settings → Keybindings page. Carries the
/// human-readable label + the current binding string + the
/// built-in default binding for "reset" affordances.
#[derive(Debug, Clone)]
pub struct ShortcutSettingsRow {
    /// Stable kebab-case id for the action (`cycle-projects`,
    /// `new-task`, etc.). Round-trips through
    /// [`LocalSession::set_shortcut_binding`].
    pub id: String,
    pub label: String,
    /// Current binding string, e.g. `"cmd-shift-]"`. Empty when
    /// the action has been intentionally cleared.
    pub current_binding: String,
    pub default_binding: String,
}

/// Snapshot returned by [`LocalSession::read_shortcut_settings`].
#[derive(Debug, Clone)]
pub struct ShortcutSettingsView {
    pub actions: Vec<ShortcutSettingsRow>,
}

/// FRB-friendly mirror of [`another_one_core::mcp::McpSource`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpSourceDto {
    Catalog,
    Custom,
    BuiltInDaemon,
}

/// FRB-friendly mirror of [`another_one_core::mcp::McpTransport`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransportKindDto {
    Stdio,
    Http,
}

/// One row of the Settings → MCP page's registry section.
#[derive(Debug, Clone)]
pub struct McpServerDto {
    pub id: String,
    pub label: String,
    pub source: McpSourceDto,
    pub transport_kind: McpTransportKindDto,
    /// Provider ids (kebab-case: `claude-code`, `cursor-agent`,
    /// `codex`, `gemini`, `opencode`, `amp`) the entry is enabled
    /// for. UI maps these to short labels.
    pub enabled_for: Vec<String>,
}

/// One row of the Settings → MCP page's catalog section. Carries
/// the static metadata; the UI flips this into an `McpServerDto`
/// row after `mcp_add_from_catalog`.
#[derive(Debug, Clone)]
pub struct McpCatalogEntryDto {
    pub id: String,
    pub label: String,
    pub description: String,
    pub docs_url: String,
}

/// Snapshot returned by [`LocalSession::read_mcp_settings`].
#[derive(Debug, Clone)]
pub struct McpSettingsView {
    pub catalog_entries: Vec<McpCatalogEntryDto>,
    pub registry_entries: Vec<McpServerDto>,
    /// Providers whose last sync failed — UI tints their toggle
    /// red. Empty in this first cut (sync errors live only in
    /// GPUI's `mcp_last_sync_errors` today).
    pub sync_error_provider_ids: Vec<String>,
}

fn mcp_server_to_dto(
    server: &another_one_core::mcp::McpServer,
) -> McpServerDto {
    use another_one_core::agents::AgentProviderKind;
    let enabled_for = [
        AgentProviderKind::ClaudeCode,
        AgentProviderKind::CursorAgent,
        AgentProviderKind::Codex,
        AgentProviderKind::Gemini,
        AgentProviderKind::OpenCode,
        AgentProviderKind::Amp,
    ]
    .into_iter()
    .filter(|p| server.enabled_for.contains(p))
    .map(provider_id)
    .map(str::to_string)
    .collect();
    McpServerDto {
        id: server.id.clone(),
        label: server.label.clone(),
        source: match server.source {
            another_one_core::mcp::McpSource::Catalog => McpSourceDto::Catalog,
            another_one_core::mcp::McpSource::Custom => McpSourceDto::Custom,
            another_one_core::mcp::McpSource::BuiltInDaemon => {
                McpSourceDto::BuiltInDaemon
            }
        },
        transport_kind: match server.transport {
            another_one_core::mcp::McpTransport::Stdio { .. } => {
                McpTransportKindDto::Stdio
            }
            another_one_core::mcp::McpTransport::Http { .. } => {
                McpTransportKindDto::Http
            }
        },
        enabled_for,
    }
}

fn provider_id(
    p: another_one_core::agents::AgentProviderKind,
) -> &'static str {
    use another_one_core::agents::AgentProviderKind;
    match p {
        AgentProviderKind::ClaudeCode => "claude-code",
        AgentProviderKind::CursorAgent => "cursor-agent",
        AgentProviderKind::Codex => "codex",
        AgentProviderKind::Pi => "pi",
        AgentProviderKind::Gemini => "gemini",
        AgentProviderKind::OpenCode => "opencode",
        AgentProviderKind::Amp => "amp",
        AgentProviderKind::RovoDev => "rovo-dev",
        AgentProviderKind::Forge => "forge",
    }
}

fn parse_provider_id(
    id: &str,
) -> Option<another_one_core::agents::AgentProviderKind> {
    use another_one_core::agents::AgentProviderKind;
    match id {
        "claude-code" => Some(AgentProviderKind::ClaudeCode),
        "cursor-agent" => Some(AgentProviderKind::CursorAgent),
        "codex" => Some(AgentProviderKind::Codex),
        "pi" => Some(AgentProviderKind::Pi),
        "gemini" => Some(AgentProviderKind::Gemini),
        "opencode" => Some(AgentProviderKind::OpenCode),
        "amp" => Some(AgentProviderKind::Amp),
        "rovo-dev" => Some(AgentProviderKind::RovoDev),
        "forge" => Some(AgentProviderKind::Forge),
        _ => None,
    }
}

fn shortcut_action_id(
    action: another_one_core::shortcuts::ShortcutAction,
) -> &'static str {
    use another_one_core::shortcuts::ShortcutAction;
    match action {
        ShortcutAction::CycleProjects => "cycle-projects",
        ShortcutAction::NewTabInCurrentTask => "new-tab-in-current-task",
        ShortcutAction::NewTask => "new-task",
        ShortcutAction::CloseCurrentTab => "close-current-tab",
        ShortcutAction::NextTab => "next-tab",
        ShortcutAction::PreviousTab => "previous-tab",
        ShortcutAction::NextTask => "next-task",
        ShortcutAction::PreviousTask => "previous-task",
    }
}

fn parse_shortcut_action_id(
    id: &str,
) -> Option<another_one_core::shortcuts::ShortcutAction> {
    use another_one_core::shortcuts::ShortcutAction;
    match id {
        "cycle-projects" => Some(ShortcutAction::CycleProjects),
        "new-tab-in-current-task" => Some(ShortcutAction::NewTabInCurrentTask),
        "new-task" => Some(ShortcutAction::NewTask),
        "close-current-tab" => Some(ShortcutAction::CloseCurrentTab),
        "next-tab" => Some(ShortcutAction::NextTab),
        "previous-tab" => Some(ShortcutAction::PreviousTab),
        "next-task" => Some(ShortcutAction::NextTask),
        "previous-task" => Some(ShortcutAction::PreviousTask),
        _ => None,
    }
}

fn agent_def_to_dto(
    agent: &&'static another_one_core::agents::AgentDef,
) -> AgentSummaryDto {
    AgentSummaryDto {
        id: agent.id.to_string(),
        label: agent.label.to_string(),
        icon_path: agent.icon.to_string(),
        provider: agent.provider.map(map_agent_provider),
    }
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectActionIcon`]. Stable
/// kebab-case ids round-trip the GPUI on-disk format
/// (`projects.json`) so a user can switch desktop binaries without
/// the icon picker resetting.
///
/// `Deserialize` exists so the iroh transport can decode the wire
/// payload (`ProjectActionIconWire` from `daemon-sandbox`) directly
/// into this DTO. Wire form is kebab-case to match
/// `core::project_store::ProjectActionIcon`'s on-disk shape.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionIconDto {
    Play,
    Test,
    Lint,
    Configure,
    Build,
    Debug,
    Agent,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectActionScope`]. Ordered
/// "project first, global last" because that's how the dropdown row
/// order treats them — global rows render with a globe glyph beside
/// the action label.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionScopeDto {
    Project,
    Global,
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectActionAccess`]. Drives
/// the agent-mode CLI's permission flag — `default` passes nothing
/// extra, the other three map to `--read-only`, `--workspace-write`,
/// `--full-access` (Claude Code today; other providers ignore).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectActionAccessDto {
    Default,
    ReadOnly,
    WorkspaceWrite,
    FullAccess,
}

/// Tagged-union mirror of
/// [`another_one_core::project_store::ProjectActionKind`]. The Dart
/// side discriminates on the variant; FRB emits a sealed-class
/// hierarchy with `Shell` and `Agent` subclasses.
///
/// `Deserialize` lets the iroh transport decode the daemon's
/// `ProjectActionKindWire` (externally-tagged via serde default)
/// directly into this enum.
#[derive(Debug, Clone, serde::Deserialize)]
pub enum ProjectActionKindDto {
    /// A shell command typed verbatim into a freshly-spawned PTY.
    /// `command` is run as `<command>\n` so multi-line input works
    /// the same way it would in an interactive shell.
    Shell { command: String },
    /// An agent CLI launch. `prompt` is the seed message;
    /// `model`/`traits`/`mode`/`access` are agent-specific knobs
    /// fed to `project_action_agent_launch_args`.
    Agent {
        prompt: String,
        provider: AgentProvider,
        model: Option<String>,
        traits: Option<String>,
        mode: Option<String>,
        access: ProjectActionAccessDto,
    },
}

/// FRB-friendly mirror of
/// [`another_one_core::project_store::ProjectAction`]. Carries the
/// lot — id (empty for a never-saved action), display name, icon,
/// run-on-worktree-create flag, scope, and the kind-specific
/// payload. UI maps `icon` to its asset path via
/// `ProjectActionIconDto.icon_path` (Dart-side helper).
///
/// `Deserialize` lets the iroh transport decode the daemon's
/// `ProjectActionWire` shape directly into this DTO; field names
/// align by design.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct ProjectActionDto {
    pub id: String,
    pub name: String,
    pub icon: ProjectActionIconDto,
    pub run_on_worktree_create: bool,
    pub scope: ProjectActionScopeDto,
    pub kind: ProjectActionKindDto,
}

fn map_action_icon(icon: ProjectActionIcon) -> ProjectActionIconDto {
    match icon {
        ProjectActionIcon::Play => ProjectActionIconDto::Play,
        ProjectActionIcon::Test => ProjectActionIconDto::Test,
        ProjectActionIcon::Lint => ProjectActionIconDto::Lint,
        ProjectActionIcon::Configure => ProjectActionIconDto::Configure,
        ProjectActionIcon::Build => ProjectActionIconDto::Build,
        ProjectActionIcon::Debug => ProjectActionIconDto::Debug,
        ProjectActionIcon::Agent => ProjectActionIconDto::Agent,
    }
}

fn map_action_icon_back(icon: ProjectActionIconDto) -> ProjectActionIcon {
    match icon {
        ProjectActionIconDto::Play => ProjectActionIcon::Play,
        ProjectActionIconDto::Test => ProjectActionIcon::Test,
        ProjectActionIconDto::Lint => ProjectActionIcon::Lint,
        ProjectActionIconDto::Configure => ProjectActionIcon::Configure,
        ProjectActionIconDto::Build => ProjectActionIcon::Build,
        ProjectActionIconDto::Debug => ProjectActionIcon::Debug,
        ProjectActionIconDto::Agent => ProjectActionIcon::Agent,
    }
}

fn map_action_scope(scope: ProjectActionScope) -> ProjectActionScopeDto {
    match scope {
        ProjectActionScope::Project => ProjectActionScopeDto::Project,
        ProjectActionScope::Global => ProjectActionScopeDto::Global,
    }
}

fn map_action_scope_back(scope: ProjectActionScopeDto) -> ProjectActionScope {
    match scope {
        ProjectActionScopeDto::Project => ProjectActionScope::Project,
        ProjectActionScopeDto::Global => ProjectActionScope::Global,
    }
}

fn map_action_access(access: ProjectActionAccess) -> ProjectActionAccessDto {
    match access {
        ProjectActionAccess::Default => ProjectActionAccessDto::Default,
        ProjectActionAccess::ReadOnly => ProjectActionAccessDto::ReadOnly,
        ProjectActionAccess::WorkspaceWrite => ProjectActionAccessDto::WorkspaceWrite,
        ProjectActionAccess::FullAccess => ProjectActionAccessDto::FullAccess,
    }
}

fn map_action_access_back(access: ProjectActionAccessDto) -> ProjectActionAccess {
    match access {
        ProjectActionAccessDto::Default => ProjectActionAccess::Default,
        ProjectActionAccessDto::ReadOnly => ProjectActionAccess::ReadOnly,
        ProjectActionAccessDto::WorkspaceWrite => ProjectActionAccess::WorkspaceWrite,
        ProjectActionAccessDto::FullAccess => ProjectActionAccess::FullAccess,
    }
}

fn project_action_to_dto(action: ProjectAction) -> ProjectActionDto {
    let kind = match action.kind {
        ProjectActionKind::Shell { command } => {
            ProjectActionKindDto::Shell { command }
        }
        ProjectActionKind::Agent {
            prompt,
            provider,
            model,
            traits,
            mode,
            access,
        } => ProjectActionKindDto::Agent {
            prompt,
            provider: map_agent_provider(provider),
            model,
            traits,
            mode,
            access: map_action_access(access),
        },
    };
    ProjectActionDto {
        id: action.id,
        name: action.name,
        icon: map_action_icon(action.icon),
        run_on_worktree_create: action.run_on_worktree_create,
        scope: map_action_scope(action.scope),
        kind,
    }
}

/// Round-trip `ProjectActionDto` → `ProjectAction`. Empty `dto.id`
/// means "brand new action, never saved" — we mint a uuid so
/// `upsert_project_action` always has a stable key. `dto.scope` is
/// captured but `upsert_project_action` overwrites it based on the
/// `save_global_copy` flag the caller passes; the round-trip is
/// kept to keep the DTO symmetric.
fn project_action_from_dto(dto: ProjectActionDto) -> anyhow::Result<ProjectAction> {
    let kind = match dto.kind {
        ProjectActionKindDto::Shell { command } => {
            ProjectActionKind::Shell { command }
        }
        ProjectActionKindDto::Agent {
            prompt,
            provider,
            model,
            traits,
            mode,
            access,
        } => ProjectActionKind::Agent {
            prompt,
            provider: map_agent_provider_back(provider),
            model,
            traits,
            mode,
            access: map_action_access_back(access),
        },
    };
    let id = if dto.id.trim().is_empty() {
        uuid::Uuid::new_v4().to_string()
    } else {
        dto.id
    };
    Ok(ProjectAction {
        id,
        name: dto.name,
        icon: map_action_icon_back(dto.icon),
        run_on_worktree_create: dto.run_on_worktree_create,
        scope: map_action_scope_back(dto.scope),
        kind,
    })
}
