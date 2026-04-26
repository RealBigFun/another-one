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
use another_one_core::project_store::ProjectKind as CoreProjectKind;
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
#[derive(Debug, Clone)]
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
#[derive(Debug, Clone)]
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
