//! Embedded iroh daemon host.
//!
//! Desktop is GPUI-only — no ambient tokio runtime — so booting the
//! `daemon-sandbox` library requires us to bring our own runtime.
//! This module owns:
//!
//! * A dedicated OS thread that runs a `tokio::runtime::Runtime` and
//!   blocks on `daemon::run_endpoint`.
//! * [`RegistryState`] — shared state the registry trait object reads
//!   (projects, live broadcast senders, live writers, pending resize
//!   requests). Wrapped in an `Arc<Mutex<…>>` so the daemon's tokio
//!   tasks can query it without cx access; the GPUI side mutates the
//!   same mutex on every `TerminalLaunchReply::Launched` /
//!   `…::Terminated` / tab-close.
//! * [`DesktopTerminalRegistry`] — the `daemon::DaemonRegistry`
//!   impl handed to `run_endpoint`. Holds a `Weak` back to
//!   `RegistryState` so dropping the app still lets the daemon task
//!   unwind cleanly.
//!
//! Resize is intentionally *not* executed on the tokio thread: the
//! live `MasterPty` lives inside `LiveTerminalRuntime` on the GPUI
//! thread. Instead, `tab_resize` enqueues a
//! [`TabResizeRequest`] on an `mpsc` the GPUI render tick drains.

use std::collections::{HashMap, HashSet};
use std::io::Write;
use std::path::PathBuf;
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::thread;

use tokio::sync::broadcast;

use daemon::{DaemonRegistry, EndpointHandle};
use daemon_proto::{
    ActiveGitStateWire, AgentProvider, AgentSettingsRowWire, AgentSettingsViewWire,
    AgentSummaryWire, ChangedFileWire, EnabledAgentsViewWire, GitActionScriptsView,
    McpCatalogEntryDto, McpServerDto, McpSettingsView, McpSourceDto, McpTransportKindDto,
    OpenInAppSettingsRowWire, OpenInAppWire, OpenInSettingsViewWire, OpenInStateWire, ProjectKind,
    ProjectSummary, ShortcutSettingsRow, ShortcutSettingsView, TabSummary, TaskSummary,
    ToolbarActionOutcome as WireToolbarActionOutcome,
};

use another_one_core::agents::{AgentProviderKind, AGENTS};
use another_one_core::git_actions::{
    execute_toolbar_git_action, GitActionSettings, ToolbarActionError, ToolbarGitAction,
};
use another_one_core::git_service::{ChangedFilesGitMutation, ChangedFilesGitMutationReply};
use another_one_core::mcp::catalog;
use another_one_core::mcp::registry::McpRegistry;
use another_one_core::mcp::{McpServer, McpSource, McpTransport};
use another_one_core::project_store::{
    read_project_git_state, revert_changed_file, stage_all_changes, stage_changed_file,
    unstage_all_changes, unstage_changed_file, ChangedFile, ProjectGitState,
    ProjectKind as CoreProjectKind, ProjectStore,
};
use another_one_core::section::SectionId;
use another_one_core::shortcuts::{ShortcutAction, ALL_SHORTCUT_ACTIONS};

use crate::open_in::{detect_available_open_in_apps, open_path_in_app, OpenInAppKind};
use crate::terminal_runtime::TerminalRuntimeKey;

/// viewer_id used for the in-process desktop view. Stable across the
/// app's lifetime; the app exits before it would ever need to disconnect.
pub(crate) const DESKTOP_LOCAL_VIEWER_ID: &str = "desktop-local";

/// Shared state the GPUI thread writes and the daemon's tokio tasks
/// read. Everything behind one `Mutex` because contention is
/// negligible at PTY-launch rates (tens per session), whereas keeping
/// projects/broadcasts/writers in sync would require multiple locks to
/// be held in order and is fragile to refactor later.
pub struct RegistryState {
    /// Snapshot of the desktop's projects/tasks/tabs, refreshed from
    /// `AnotherOneApp::project_store` on every mutation. The daemon's
    /// `ListProjects` handler reads directly from this snapshot so it
    /// doesn't need to post work back to the GPUI thread.
    pub(crate) project_store: ProjectStore,
    /// Per-tab PTY output broadcast senders, cloned from the
    /// launcher's `PreparedTerminalRuntime::output_broadcast`. Mobile
    /// `AttachTab` subscribes to the matching sender.
    pub(crate) broadcasts: HashMap<TerminalRuntimeKey, broadcast::Sender<Vec<u8>>>,
    /// Per-tab master-PTY writer handles shared with
    /// `LiveTerminalRuntime`. Mobile keystrokes flow through these
    /// exactly like desktop keystrokes do.
    pub(crate) writers: HashMap<TerminalRuntimeKey, Arc<Mutex<Box<dyn Write + Send>>>>,
    /// Resize requests queued by the daemon thread; drained on the
    /// GPUI render tick where `LiveTerminalRuntime::resize` is safe to
    /// call.
    pub(crate) pending_resizes: Vec<TabResizeRequest>,
    /// Per-tab set of currently-attached viewers and the viewport
    /// size each wants. The PTY for a tab is resized to the **min**
    /// across the viewer entries here so a wide desktop window can't
    /// make the PTY too wide for a phone to render. A viewer
    /// appears in at most one tab's map at a time (switching
    /// focused tabs clears the prior entry); leaving the session
    /// clears every entry for that viewer.
    pub(crate) active_viewers: HashMap<TerminalRuntimeKey, HashMap<String, (u16, u16)>>,
    /// Tracks which tab each viewer currently has in focus — used to
    /// clear their prior entry when they switch or detach.
    pub(crate) viewer_focus: HashMap<String, TerminalRuntimeKey>,
    /// Last effective size applied to each tab's PTY; avoids
    /// re-enqueueing identical resize requests on every keystroke.
    pub(crate) effective_sizes: HashMap<TerminalRuntimeKey, (u16, u16)>,
    /// Tab-launch requests from any client (mobile). Drained on the
    /// GPUI render tick, where the task's persisted `launch_config`
    /// is resolved from the project store and the PTY is spawned via
    /// `spawn_terminal_launch`. Desktop sidebar clicks go through a
    /// different path today for legacy reasons; both produce the same
    /// end state (a live entry in `broadcasts` + `writers`).
    pub(crate) pending_tab_launches: Vec<TabLaunchRequest>,
    /// Spawn-terminal asks routed in from the daemon (MCP). Each
    /// carries a sync channel responder that the GPUI-thread drain
    /// uses to deliver the new tab id (or an error string) back to
    /// the blocking MCP caller. Cleared every render tick.
    pub(crate) pending_spawn_terminals: Vec<PendingSpawnTerminal>,
    /// Close-tab asks routed in from the daemon (MCP). Same shape
    /// as `pending_spawn_terminals`.
    pub(crate) pending_close_tabs: Vec<PendingCloseTab>,
    /// Select-focus asks routed in from the daemon (MCP).
    pub(crate) pending_select_focus: Vec<PendingSelectFocus>,
    /// UiAction dispatches routed in from the daemon (MCP). Drained
    /// on the GPUI render tick onto `AnotherOneApp::dispatch_ui_action`.
    /// Same shape as `pending_select_focus` — sync responder so the
    /// blocking MCP caller observes the result inline.
    pub(crate) pending_ui_actions: Vec<PendingUiAction>,
    /// Keys currently mid-spawn. Populated when either path
    /// (daemon-queued mobile LaunchTab **or** desktop sidebar click)
    /// kicks off a `spawn_terminal_launch`; cleared on
    /// `TerminalLaunchReply::Launched` / `Failed` / tab close. The
    /// daemon checks this to dedupe — earlier builds only checked
    /// `pending_tab_launches` + `broadcasts`, which left a window
    /// between "spawn kicked off" and "Launched reply observed"
    /// where a second LaunchTab would spawn a duplicate PTY.
    pub(crate) in_flight_launches: HashSet<TerminalRuntimeKey>,
    /// Stable broadcast sender shared with `DesktopTerminalRegistry`
    /// so the desktop GUI can fire a state-change tick after every
    /// `project_store` mutation. Connected mobile sessions push a
    /// fresh `WorkerReply::ProjectList` to their peer on each tick.
    pub(crate) state_change_tx: tokio::sync::broadcast::Sender<()>,
}

impl RegistryState {
    pub fn new(project_store: ProjectStore) -> Self {
        // Capacity 16 — server-side push pumps drop duplicates,
        // a small buffer prevents `Lagged` on bursts of mutations.
        let (state_change_tx, _) = tokio::sync::broadcast::channel(16);
        Self {
            project_store,
            broadcasts: HashMap::new(),
            writers: HashMap::new(),
            pending_resizes: Vec::new(),
            pending_tab_launches: Vec::new(),
            pending_spawn_terminals: Vec::new(),
            pending_close_tabs: Vec::new(),
            pending_select_focus: Vec::new(),
            pending_ui_actions: Vec::new(),
            in_flight_launches: HashSet::new(),
            active_viewers: HashMap::new(),
            viewer_focus: HashMap::new(),
            effective_sizes: HashMap::new(),
            state_change_tx,
        }
    }

    /// Recompute the min-across-viewers size for `key` and, if it
    /// changed since the last effective size, enqueue a resize for
    /// the GPUI render tick to apply. Returns the effective size so
    /// callers can log / debug — not otherwise used.
    pub(crate) fn recompute_effective_size(
        &mut self,
        key: &TerminalRuntimeKey,
    ) -> Option<(u16, u16)> {
        let viewers = self.active_viewers.get(key)?;
        if viewers.is_empty() {
            return None;
        }
        let (cols, rows) = viewers
            .values()
            .fold((u16::MAX, u16::MAX), |(c, r), (vc, vr)| {
                (c.min(*vc), r.min(*vr))
            });
        let effective = (cols.max(1), rows.max(1));
        if self.effective_sizes.get(key).copied() == Some(effective) {
            return Some(effective);
        }
        self.effective_sizes.insert(key.clone(), effective);
        self.pending_resizes.push(TabResizeRequest {
            key: key.clone(),
            cols: effective.0,
            rows: effective.1,
        });
        Some(effective)
    }
}

/// A "please launch this tab" ask from a remote client. Same shape
/// as the sidebar-click path on the desktop would produce, minus the
/// GUI-level affordances (active-page toggling, etc.).
#[derive(Clone, Debug)]
pub(crate) struct TabLaunchRequest {
    pub key: TerminalRuntimeKey,
}

/// A pending tab resize request from a mobile client. The daemon's
/// `tab_resize` impl pushes one of these onto
/// `RegistryState.pending_resizes`; `AnotherOneApp` drains them on the
/// render tick and forwards to `LiveTerminalRuntime::resize`.
#[derive(Clone, Debug)]
pub(crate) struct TabResizeRequest {
    pub key: TerminalRuntimeKey,
    pub cols: u16,
    pub rows: u16,
}

/// MCP `dispatch_ui_action` ask — desktop-only ephemera the GUI
/// can also dispatch (overlay open/close, zoom, focus, etc.). The
/// drain calls `AnotherOneApp::dispatch_ui_action` on the GPUI
/// thread.
pub(crate) struct PendingUiAction {
    pub action: another_one_core::mcp::orchestrator::UiAction,
    pub responder: std::sync::mpsc::SyncSender<Result<(), String>>,
}

/// MCP `select_focus` ask — moves a client's focus, optionally on
/// behalf of a peer (privileged surface). The drain emits the
/// underlying `client_select_for` call on the GPUI thread.
pub(crate) struct PendingSelectFocus {
    pub focus: another_one_core::clients::Focus,
    pub for_client: Option<another_one_core::clients::ClientId>,
    pub client_handle: Option<String>,
    pub responder: std::sync::mpsc::SyncSender<Result<(), String>>,
}

/// MCP `close_tab` ask. Same queue/drain pattern as the spawn case.
pub(crate) struct PendingCloseTab {
    pub tab_id: String,
    pub client_handle: Option<String>,
    pub responder: std::sync::mpsc::SyncSender<Result<(), String>>,
}

/// MCP `spawn_terminal` ask. Carries the request + a sync responder
/// the GPUI-thread drain sends the resulting tab id back through.
/// `responder` is `Option<…>` so the drain can take it; once taken
/// the entry is consumed.
pub(crate) struct PendingSpawnTerminal {
    pub project_id: Option<String>,
    pub task_id: Option<String>,
    pub cwd: Option<String>,
    /// Optional caller-identifying string. Lifted into a `ClientId`
    /// of the form `mcp:<handle>` so the event bus can attribute
    /// the resulting `TaskOpened` / `TabOpened` event to the
    /// originating MCP client. None → "anonymous".
    pub client_handle: Option<String>,
    pub responder: std::sync::mpsc::SyncSender<
        Result<another_one_core::mcp::orchestrator::SpawnTerminalResponse, String>,
    >,
}

/// `DaemonRegistry` implementation that projects `AnotherOneApp`
/// state onto the wire. Holds a `Weak` so a late-arriving daemon
/// callback after app shutdown drops out cleanly instead of keeping
/// the app alive.
pub struct DesktopTerminalRegistry {
    inner: Weak<Mutex<RegistryState>>,
    /// Stable broadcast sender for state-change notifications.
    /// Cloned out of `RegistryState::state_change_tx` at construction
    /// so the trait impl can serve `subscribe_state_changes` /
    /// `notify_state_changed` without re-taking the inner state lock.
    state_tx: tokio::sync::broadcast::Sender<()>,
}

impl DesktopTerminalRegistry {
    pub fn new(inner: Weak<Mutex<RegistryState>>) -> Self {
        // Pull the canonical sender out of the shared `RegistryState`
        // so notifications fired by the GUI's
        // `sync_registry_project_store` reach our subscribers too.
        let state_tx = inner
            .upgrade()
            .and_then(|arc| arc.lock().ok().map(|guard| guard.state_change_tx.clone()))
            .unwrap_or_else(|| tokio::sync::broadcast::channel(16).0);
        Self { inner, state_tx }
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut RegistryState) -> R) -> Option<R> {
        let arc = self.inner.upgrade()?;
        let mut guard = arc.lock().ok()?;
        Some(f(&mut guard))
    }

    /// Mutator helper for daemon-side store writes: locks state,
    /// runs `f` with `&mut ProjectStore`, then persists to disk and
    /// fires the state-change broadcast so connected sessions push
    /// a fresh `WorkerReply::ProjectList` to peers. Use from every
    /// `Control::*` handler that mutates the store.
    ///
    /// Save errors are logged but not surfaced — keeping them out of
    /// the trait return type means call-site authors can write
    /// "match the existing GUI mutator" without worrying about
    /// per-handler error mapping. A failed save is exotic enough
    /// (disk full / permissions) that the daemon's `tracing` log is
    /// the right surface anyway.
    #[allow(dead_code)] // call-site migrations land in commits 6–9
    fn with_store_mut<R>(&self, f: impl FnOnce(&mut ProjectStore) -> R) -> Option<R> {
        self.with_state(|state| {
            let result = f(&mut state.project_store);
            // `ProjectStore::save` swallows errors internally
            // (logs + returns ()); no Result to map here.
            state.project_store.save();
            // Fire the broadcast tick so every connected session's
            // push pump sends a fresh ProjectList. Same channel the
            // GUI's sync_registry_project_store fires on.
            let _ = state.state_change_tx.send(());
            result
        })
    }
}

impl DaemonRegistry for DesktopTerminalRegistry {
    fn health(&self) -> Result<(), String> {
        self.with_state(|_| ())
            .ok_or_else(|| "desktop registry state is unavailable".to_string())
    }

    fn list_projects(&self) -> Vec<ProjectSummary> {
        // Read straight from the in-memory store. Every desktop
        // direct-mutation reaches `RegistryState.project_store` via
        // `commit_local_mutation` → `sync_registry_project_store`,
        // and every daemon-side mutation flows through
        // `with_store_mut` (also writes here). The legacy
        // `ProjectStore::load()` reload-from-disk on every
        // ListProjects was a workaround for the GUI mutating without
        // syncing; obsolete now that all paths funnel through one of
        // those two helpers.
        self.with_state(|state| project_summaries(state))
            .unwrap_or_default()
    }

    fn subscribe_state_changes(&self) -> tokio::sync::broadcast::Receiver<()> {
        self.state_tx.subscribe()
    }

    fn notify_state_changed(&self) {
        // `send` returns Err only if there are no receivers. That's
        // fine — no one's listening yet, no work to do.
        let _ = self.state_tx.send(());
    }

    fn remove_project(&self, project_id: &str) -> anyhow::Result<()> {
        let project_id = project_id.to_string();
        self.with_store_mut(move |store| {
            store.remove_project(&project_id);
        })
        .ok_or_else(|| anyhow::anyhow!(registry_unavailable()))?;
        Ok(())
    }

    fn rename_task(
        &self,
        task_id: &str,
        new_name: &str,
    ) -> (bool, Option<TaskSummary>) {
        let task_id = task_id.to_string();
        let new_name = new_name.to_string();
        let result = self.with_state(|state| {
            let changed = state.project_store.rename_task(&task_id, &new_name);
            if changed {
                state.project_store.save();
                let _ = state.state_change_tx.send(());
            }
            (changed, task_summary_for(state, &task_id))
        });
        result.unwrap_or((false, None))
    }

    fn set_task_pinned(
        &self,
        task_id: &str,
        pinned: bool,
    ) -> (bool, Option<TaskSummary>) {
        let task_id = task_id.to_string();
        let result = self.with_state(|state| {
            let changed = state.project_store.set_task_pinned(&task_id, pinned);
            if changed {
                state.project_store.save();
                let _ = state.state_change_tx.send(());
            }
            (changed, task_summary_for(state, &task_id))
        });
        result.unwrap_or((false, None))
    }

    fn remove_task(&self, project_id: &str, task_id: &str) -> bool {
        let project_id = project_id.to_string();
        let task_id = task_id.to_string();
        self.with_store_mut(move |store| store.remove_task(&project_id, &task_id).is_some())
            .unwrap_or(false)
    }

    fn set_branch_setting(
        &self,
        project_id: &str,
        field: &str,
        branch_name: Option<&str>,
    ) -> Result<bool, String> {
        let project_id = project_id.to_string();
        let branch_name = branch_name.map(str::to_string);
        let changed = match field {
            "default-branch" => self
                .with_store_mut(move |store| {
                    store
                        .update_default_branch(&project_id, branch_name.clone())
                        .map_err(|e| e.to_string())
                })
                .ok_or_else(registry_unavailable)??,
            "default-target-branch" => self
                .with_store_mut(move |store| {
                    store
                        .update_default_target_branch(&project_id, branch_name.clone())
                        .map_err(|e| e.to_string())
                })
                .ok_or_else(registry_unavailable)??,
            other => return Err(format!("unknown branch_setting field: {other}")),
        };
        Ok(changed)
    }

    fn persist_section_state(&self, section_id: &str, persisted: serde_json::Value) {
        let Ok(persisted) = serde_json::from_value::<
            another_one_core::project_store::PersistedSectionState,
        >(persisted) else {
            tracing::warn!(section_id, "PersistSectionState payload failed to decode");
            return;
        };
        let Some(parsed) = SectionId::from_store_key(section_id) else {
            tracing::warn!(section_id, "PersistSectionState section_id malformed");
            return;
        };
        self.with_store_mut(|store| {
            if let Some(task_id) = parsed.task_id.as_deref() {
                store.update_task_tabs(task_id, &persisted);
            } else {
                store.set_terminal_section(section_id.to_string(), persisted);
            }
        });
    }

    fn set_last_active_section(&self, section_id: Option<String>) {
        self.with_store_mut(|store| {
            store.set_last_active_section_key(section_id);
        });
    }

    fn set_sidebar_git_metadata_visible(&self, visible: bool) {
        self.with_store_mut(|store| {
            store.set_sidebar_git_metadata_visible(visible);
        });
    }

    fn set_repo_default_commit_action(&self, repo_id: &str, action: &str) {
        let parsed = match action {
            "commit" => another_one_core::project_store::RepoDefaultCommitAction::Commit,
            "commit-and-push" => {
                another_one_core::project_store::RepoDefaultCommitAction::CommitAndPush
            }
            other => {
                tracing::warn!(other, "SetRepoDefaultCommitAction: unknown action id");
                return;
            }
        };
        let repo_id_owned = repo_id.to_string();
        self.with_store_mut(move |store| {
            store.set_repo_default_commit_action(repo_id_owned, parsed);
        });
    }

    fn update_task_branch(
        &self,
        task_id: &str,
        target_project_id: &str,
        branch_name: &str,
    ) {
        let task_id = task_id.to_string();
        let target_project_id = target_project_id.to_string();
        let branch_name = branch_name.to_string();
        self.with_store_mut(move |store| {
            let _ = store.update_task_branch(&task_id, &target_project_id, &branch_name);
        });
    }

    fn set_expanded_repos(&self, expanded_repo_ids: Vec<String>) {
        let set: std::collections::HashSet<String> = expanded_repo_ids.into_iter().collect();
        self.with_store_mut(move |store| {
            store.set_expanded_repos(&set);
        });
    }

    fn set_git_commit_llm(&self, settings: serde_json::Value) {
        let Ok(settings) = serde_json::from_value::<
            another_one_core::git_actions::GitActionLlmSettings,
        >(settings) else {
            tracing::warn!("SetGitCommitLlm payload failed to decode");
            return;
        };
        self.with_store_mut(move |store| {
            let _ = store.set_git_commit_generation_llm(settings);
        });
    }

    fn set_git_pr_llm(&self, settings: serde_json::Value) {
        let Ok(settings) = serde_json::from_value::<
            another_one_core::git_actions::GitActionLlmSettings,
        >(settings) else {
            tracing::warn!("SetGitPrLlm payload failed to decode");
            return;
        };
        self.with_store_mut(move |store| {
            let _ = store.set_git_pr_generation_llm(settings);
        });
    }

    fn ui_snapshot(&self) -> daemon_proto::UiSnapshot {
        self.with_state(|state| {
            let ui = &state.project_store.ui;
            daemon_proto::UiSnapshot {
                expanded_repo_ids: ui.expanded_repo_ids.iter().cloned().collect(),
                pinned_task_ids: ui
                    .pinned_task_ids
                    .iter()
                    .map(|id| (String::new(), id.clone()))
                    .collect(),
                last_active_section_id: ui.last_active_section_id.clone(),
                left_sidebar_open: ui.left_sidebar_open,
                show_sidebar_git_metadata: ui.show_sidebar_git_metadata,
                shortcuts: serde_json::to_value(&ui.shortcuts).ok(),
                agent_launch_args_overrides: serde_json::to_value(&ui.agent_launch_args).ok(),
                default_agent_id: ui.default_agent_id.clone(),
                enabled_agents: ui
                    .enabled_agents
                    .as_ref()
                    .map(|set| set.iter().cloned().collect()),
                open_in_apps: ui
                    .enabled_open_in_apps
                    .as_ref()
                    .and_then(|s| serde_json::to_value(s).ok()),
                preferred_open_in_app: ui
                    .preferred_open_in_app
                    .as_ref()
                    .map(|kind| kind.id().to_string()),
                git_commit_generation_script: ui.git_commit_generation_script.clone(),
                git_pr_generation_script: ui.git_pr_generation_script.clone(),
                git_commit_generation_llm: serde_json::to_value(&ui.git_commit_generation_llm).ok(),
                git_pr_generation_llm: serde_json::to_value(&ui.git_pr_generation_llm).ok(),
            }
        })
        .unwrap_or_default()
    }

    fn attach_tab(&self, section_id: &str, tab_id: &str) -> Option<broadcast::Receiver<Vec<u8>>> {
        let key = key_from_wire(section_id, tab_id)?;
        self.with_state(|state| state.broadcasts.get(&key).map(|tx| tx.subscribe()))
            .flatten()
    }

    fn tab_input(&self, section_id: &str, tab_id: &str, bytes: &[u8]) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        // Clone the writer Arc out of RegistryState *first*, drop
        // the outer state lock, THEN do the blocking PTY write.
        // Holding the state lock across `write_all` + `flush`
        // serialises every daemon task on the tokio worker pool
        // while one mobile keystroke is in flight — if the PTY
        // pipe blocks, the whole daemon stalls.
        let writer = self
            .with_state(|state| state.writers.get(&key).cloned())
            .flatten();
        let Some(writer) = writer else { return };
        // `write_all` on a portable-pty master is a plain blocking
        // syscall. If the child has stopped reading (paused agent,
        // pipe buffer full, fork bomb), the write can park for
        // seconds. Without `block_in_place` that parks a tokio
        // worker thread entirely — reducing our 4-worker pool to
        // 3, 2, 1, eventually zero. `block_in_place` hands the
        // worker back to the runtime for the duration of the
        // syscall, letting the accept loop / forwarder / other
        // tab's writer keep draining.
        //
        // Ordering note: `tab_input` is called from inside the
        // single async task that reads frames off this viewer's
        // QUIC stream, sequentially, one frame at a time. So the
        // `block_in_place` calls from this viewer are naturally
        // serialised by that task's single execution. Cross-viewer
        // ordering is mediated by the inner `Mutex` (a second
        // viewer typing into the same PTY waits for the first
        // viewer's write to finish). Swapping to `spawn_blocking`
        // would break this sequentiality — concurrent spawns can
        // race for the Mutex and interleave multi-byte sequences
        // like `\e[A`.
        //
        // Poison recovery: if a prior write panicked under the
        // guard, we still want to try — a poisoned lock here just
        // means the last write crashed, not that the fd is dead.
        // Clobbering the data is no worse than the panic already
        // did.
        tokio::task::block_in_place(|| {
            let mut guard = match writer.lock() {
                Ok(g) => g,
                Err(p) => p.into_inner(),
            };
            let _ = guard.write_all(bytes);
            let _ = guard.flush();
        });
    }

    fn tab_resize(&self, viewer_id: &str, section_id: &str, tab_id: &str, cols: u16, rows: u16) {
        let Some(key) = key_from_wire(section_id, tab_id) else {
            return;
        };
        self.with_state(|state| {
            // If this viewer was focused on a different tab, drop
            // its size entry there first — a viewer can only claim
            // one tab at a time.
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
            // Scan every active_viewers map, not just the one this
            // viewer was "focused" on. The trait contract says
            // "forget *every* size announcement this viewer made",
            // and a race between `tab_resize` and a concurrent
            // focus change could leave a stale entry in a prior
            // tab's map without updating viewer_focus — the old
            // "drop only the focused key" logic would then silently
            // orphan that claim, clamping a tab nobody's watching.
            //
            // Collect keys first to avoid borrow-across-iter issues
            // when we recompute / prune below.
            let touched_keys: Vec<TerminalRuntimeKey> = state
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
            for key in touched_keys {
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
            // Skip if already live — no point re-queuing a spawn
            // for a tab that's broadcasting.
            if state.broadcasts.contains_key(&key) {
                return;
            }
            // Skip if a spawn is already in flight — either queued
            // for the GPUI tick (`pending_tab_launches`) or already
            // kicked off but awaiting `Launched`
            // (`in_flight_launches`). Both desktop-click and
            // daemon-dispatched spawns populate the latter, so the
            // race window where only one of them saw each other's
            // progress is closed.
            if state.in_flight_launches.contains(&key) {
                return;
            }
            if state.pending_tab_launches.iter().any(|r| r.key == key) {
                return;
            }
            state.pending_tab_launches.push(TabLaunchRequest { key });
        });
    }

    fn read_active_git_state(&self, project_id: &str) -> Option<ActiveGitStateWire> {
        let project_path = self
            .with_state(|state| project_path(state, project_id))
            .flatten()?;
        let state = read_project_git_state(&project_path, true);
        Some(active_git_state_wire(&state))
    }

    fn read_changed_files(&self, project_id: &str) -> Option<Vec<ChangedFileWire>> {
        let project_path = self
            .with_state(|state| project_path(state, project_id))
            .flatten()?;
        Some(changed_files_wire(
            read_project_git_state(&project_path, false).changed_files,
        ))
    }

    fn stage_changed_file<'a>(
        &'a self,
        project_id: &'a str,
        path: &'a str,
        original_path: Option<&'a str>,
    ) -> daemon::registry::RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let mutation = ChangedFilesGitMutation::StageFile {
            changed: changed_file_for_mutation(path, original_path, false),
        };
        git_mutation_future(inner, project_id, mutation)
    }

    fn unstage_changed_file<'a>(
        &'a self,
        project_id: &'a str,
        path: &'a str,
        original_path: Option<&'a str>,
    ) -> daemon::registry::RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let mutation = ChangedFilesGitMutation::UnstageFile {
            changed: changed_file_for_mutation(path, original_path, false),
        };
        git_mutation_future(inner, project_id, mutation)
    }

    fn stage_all_changes<'a>(
        &'a self,
        project_id: &'a str,
    ) -> daemon::registry::RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        git_mutation_future(
            self.inner.clone(),
            project_id.to_string(),
            ChangedFilesGitMutation::StageAll,
        )
    }

    fn unstage_all_changes<'a>(
        &'a self,
        project_id: &'a str,
    ) -> daemon::registry::RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        git_mutation_future(
            self.inner.clone(),
            project_id.to_string(),
            ChangedFilesGitMutation::UnstageAll,
        )
    }

    fn discard_changed_file<'a>(
        &'a self,
        project_id: &'a str,
        path: &'a str,
        untracked: bool,
        original_path: Option<&'a str>,
    ) -> daemon::registry::RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let mutation = ChangedFilesGitMutation::RevertFiles {
            changed_files: vec![changed_file_for_mutation(path, original_path, untracked)],
        };
        git_mutation_future(inner, project_id, mutation)
    }

    fn discard_all_changes<'a>(
        &'a self,
        project_id: &'a str,
        files: Vec<ChangedFileWire>,
    ) -> daemon::registry::RegistryFuture<'a, anyhow::Result<(Vec<ChangedFileWire>, Vec<String>)>>
    {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        Box::pin(async move {
            let outcome = tokio::task::spawn_blocking(move || {
                run_changed_files_git_mutation_for_weak(
                    inner,
                    &project_id,
                    ChangedFilesGitMutation::RevertFiles {
                        changed_files: files.into_iter().map(changed_file_from_wire).collect(),
                    },
                )
            })
            .await
            .map_err(|error| anyhow::anyhow!("git mutation task failed: {error}"))??;
            Ok((changed_files_wire(outcome.changed_files), Vec::new()))
        })
    }

    fn run_toolbar_git_action<'a>(
        &'a self,
        project_id: &'a str,
        action_id: &'a str,
    ) -> daemon::registry::RegistryFuture<'a, anyhow::Result<WireToolbarActionOutcome>> {
        let inner = self.inner.clone();
        let project_id = project_id.to_string();
        let action_id = action_id.to_string();
        Box::pin(async move {
            let outcome = tokio::task::spawn_blocking(move || {
                let action = toolbar_action_from_id_for_weak(&inner, &project_id, &action_id)?;
                let mut progress = |_message: String| {};
                run_toolbar_git_action_for_weak(inner, &project_id, action, &mut progress)
                    .map_err(toolbar_action_error)
            })
            .await
            .map_err(|error| anyhow::anyhow!("toolbar git action task failed: {error}"))??;
            Ok(WireToolbarActionOutcome {
                toast_message: outcome.toast_message,
                warning: outcome.warning,
                refresh_git_state: outcome.refresh_git_state,
            })
        })
    }

    fn open_in_state(&self) -> Option<OpenInStateWire> {
        let available = detect_available_open_in_apps();
        self.with_state(|state| {
            let enabled = state.project_store.enabled_open_in_apps(&available);
            let preferred = state.project_store.preferred_open_in_app(&available);
            OpenInStateWire {
                enabled_apps: enabled.into_iter().map(open_in_app_wire).collect(),
                preferred_app_id: preferred.map(|app| app.id().to_string()),
            }
        })
    }

    fn read_enabled_agents(&self) -> EnabledAgentsViewWire {
        self.with_state(|state| {
            let enabled_ids = state
                .project_store
                .enabled_agent_ids()
                .into_iter()
                .collect::<HashSet<_>>();
            EnabledAgentsViewWire {
                agents: AGENTS
                    .iter()
                    .filter(|agent| enabled_ids.contains(agent.id))
                    .map(agent_summary_wire)
                    .collect(),
                default_agent_id: state.project_store.default_agent_id().map(str::to_string),
            }
        })
        .unwrap_or(EnabledAgentsViewWire {
            agents: Vec::new(),
            default_agent_id: None,
        })
    }

    fn read_agent_settings(&self) -> AgentSettingsViewWire {
        self.with_state(|state| agent_settings_view(&state.project_store))
            .unwrap_or(AgentSettingsViewWire {
                agents: Vec::new(),
                default_agent_id: None,
            })
    }

    fn set_agent_enabled(&self, agent_id: &str, enabled: bool) -> Result<bool, String> {
        ensure_agent_id(agent_id)?;
        self.with_store_mut(|store| store.set_agent_enabled(agent_id, enabled))
            .ok_or_else(registry_unavailable)
    }

    fn set_default_agent(&self, agent_id: &str) -> Result<bool, String> {
        ensure_agent_id(agent_id)?;
        self.with_store_mut(|store| store.set_default_agent(agent_id))
            .ok_or_else(registry_unavailable)
    }

    fn set_agent_launch_args(&self, agent_id: &str, args: Vec<String>) -> Result<bool, String> {
        ensure_agent_id(agent_id)?;
        self.with_store_mut(|store| store.set_agent_launch_args(agent_id, args))
            .ok_or_else(registry_unavailable)
    }

    fn read_open_in_settings(&self) -> Option<OpenInSettingsViewWire> {
        let available = detect_available_open_in_apps();
        self.with_state(|state| OpenInSettingsViewWire {
            available_apps: available
                .iter()
                .copied()
                .map(|app| OpenInAppSettingsRowWire {
                    id: app.id().to_string(),
                    label: app.label().to_string(),
                    description: app.description().to_string(),
                    icon_path: app.icon_path().to_string(),
                    enabled: state.project_store.open_in_app_enabled(app, &available),
                })
                .collect(),
        })
    }

    fn set_open_in_app_enabled(&self, app_id: &str, enabled: bool) -> Result<(), String> {
        let app =
            open_in_app_from_id(app_id).ok_or_else(|| format!("unknown Open-In app: {app_id}"))?;
        let available = detect_available_open_in_apps();
        self.with_store_mut(|store| {
            store.set_open_in_app_enabled(app, enabled, &available);
        })
        .ok_or_else(registry_unavailable)
    }

    fn open_project_in_app(&self, project_id: &str, app_id: &str) -> Result<(), String> {
        let app =
            open_in_app_from_id(app_id).ok_or_else(|| format!("unknown Open-In app: {app_id}"))?;
        let (path, available) = self
            .with_state(|state| {
                let path = state
                    .project_store
                    .project(project_id)
                    .map(|project| project.path.clone());
                (path, detect_available_open_in_apps())
            })
            .ok_or_else(registry_unavailable)?;
        let path = path.ok_or_else(|| format!("unknown project: {project_id}"))?;
        open_path_in_app(&path, app)?;
        self.with_store_mut(|store| {
            store.set_preferred_open_in_app(app, &available);
        })
        .ok_or_else(registry_unavailable)
    }

    fn read_git_action_scripts(&self) -> GitActionScriptsView {
        self.with_state(|state| GitActionScriptsView {
            commit_script: state
                .project_store
                .git_commit_generation_script()
                .to_string(),
            commit_using_default: state
                .project_store
                .ui
                .git_commit_generation_script
                .as_deref()
                .is_none_or(|script| script.trim().is_empty()),
            pr_script: state.project_store.git_pr_generation_script().to_string(),
            pr_using_default: state
                .project_store
                .ui
                .git_pr_generation_script
                .as_deref()
                .is_none_or(|script| script.trim().is_empty()),
        })
        .unwrap_or(GitActionScriptsView {
            commit_script: String::new(),
            commit_using_default: true,
            pr_script: String::new(),
            pr_using_default: true,
        })
    }

    fn set_git_commit_script(&self, script: &str) -> Result<bool, String> {
        self.with_store_mut(|store| store.set_git_commit_generation_script(script))
            .ok_or_else(registry_unavailable)
    }

    fn reset_git_commit_script(&self) -> Result<bool, String> {
        self.with_store_mut(|store| store.reset_git_commit_generation_script())
            .ok_or_else(registry_unavailable)
    }

    fn set_git_pr_script(&self, script: &str) -> Result<bool, String> {
        self.with_store_mut(|store| store.set_git_pr_generation_script(script))
            .ok_or_else(registry_unavailable)
    }

    fn reset_git_pr_script(&self) -> Result<bool, String> {
        self.with_store_mut(|store| store.reset_git_pr_generation_script())
            .ok_or_else(registry_unavailable)
    }

    fn read_shortcut_settings(&self) -> ShortcutSettingsView {
        self.with_state(|state| ShortcutSettingsView {
            actions: ALL_SHORTCUT_ACTIONS
                .into_iter()
                .map(|action| ShortcutSettingsRow {
                    id: shortcut_action_id(action).to_string(),
                    label: action.label().to_string(),
                    current_binding: state
                        .project_store
                        .ui
                        .shortcuts
                        .binding_for(action)
                        .to_string(),
                    default_binding: action.default_binding().to_string(),
                })
                .collect(),
        })
        .unwrap_or(ShortcutSettingsView {
            actions: Vec::new(),
        })
    }

    fn set_shortcut_binding(&self, action_id: &str, binding: &str) -> Result<(), String> {
        let action = shortcut_action_from_id(action_id)
            .ok_or_else(|| format!("unknown shortcut action: {action_id}"))?;
        self.with_store_mut(|store| {
            if binding.is_empty() {
                store.clear_shortcut_binding(action);
            } else {
                store.set_shortcut_binding(action, binding);
            }
        })
        .ok_or_else(registry_unavailable)
    }

    fn reset_shortcut_binding(&self, action_id: &str) -> Result<(), String> {
        let action = shortcut_action_from_id(action_id)
            .ok_or_else(|| format!("unknown shortcut action: {action_id}"))?;
        self.with_store_mut(|store| store.reset_shortcut_binding(action))
            .ok_or_else(registry_unavailable)
    }

    fn read_mcp_settings(&self) -> McpSettingsView {
        let registry = McpRegistry::load();
        McpSettingsView {
            catalog_entries: catalog::entries()
                .iter()
                .map(mcp_catalog_entry_dto)
                .collect(),
            registry_entries: registry.entries.iter().map(mcp_server_dto).collect(),
            sync_error_provider_ids: Vec::new(),
        }
    }

    fn mcp_add_from_catalog(&self, catalog_id: &str) -> Result<(), String> {
        let Some(entry) = catalog::find(catalog_id) else {
            return Ok(());
        };
        let mut registry = McpRegistry::load();
        registry.upsert(catalog::instantiate(entry));
        registry.save().map_err(|err| err.to_string())
    }

    fn mcp_toggle(&self, entry_id: &str, provider_id: &str, enabled: bool) -> Result<(), String> {
        let provider = provider_from_id(provider_id)
            .ok_or_else(|| format!("unknown provider: {provider_id}"))?;
        let mut registry = McpRegistry::load();
        if !registry.toggle(entry_id, provider, enabled) {
            return Err(format!("unknown MCP entry: {entry_id}"));
        }
        let sync_errors = mcp_sync_errors(registry.sync_all());
        registry.save().map_err(|err| err.to_string())?;
        if sync_errors.is_empty() {
            Ok(())
        } else {
            Err(format!("MCP sync failed: {}", sync_errors.join("; ")))
        }
    }

    fn mcp_remove(&self, entry_id: &str) -> Result<(), String> {
        let mut registry = McpRegistry::load();
        if !registry.remove(entry_id) {
            return Ok(());
        }
        let sync_errors = mcp_sync_errors(registry.sync_all());
        registry.save().map_err(|err| err.to_string())?;
        if sync_errors.is_empty() {
            Ok(())
        } else {
            Err(format!("MCP sync failed: {}", sync_errors.join("; ")))
        }
    }
}

pub(crate) fn spawn_changed_files_mutation(
    sender: broadcast::Sender<ChangedFilesGitMutationReply>,
    registry_state: Arc<Mutex<RegistryState>>,
    project_id: String,
    mutation: ChangedFilesGitMutation,
) {
    thread::spawn(move || {
        let result =
            run_changed_files_git_mutation_for_state(registry_state, &project_id, mutation)
                .map_err(|error| format!("{error:#}"));
        let _ = sender.send(ChangedFilesGitMutationReply { project_id, result });
    });
}

pub(crate) fn run_toolbar_git_action(
    registry_state: Arc<Mutex<RegistryState>>,
    project_id: &str,
    action: ToolbarGitAction,
    on_progress: &mut dyn FnMut(String),
) -> Result<another_one_core::git_actions::ToolbarActionOutcome, ToolbarActionError> {
    let weak = Arc::downgrade(&registry_state);
    run_toolbar_git_action_for_weak(weak, project_id, action, on_progress)
}

fn git_mutation_future<'a>(
    inner: Weak<Mutex<RegistryState>>,
    project_id: String,
    mutation: ChangedFilesGitMutation,
) -> daemon::registry::RegistryFuture<'a, anyhow::Result<Vec<ChangedFileWire>>> {
    Box::pin(async move {
        let outcome = tokio::task::spawn_blocking(move || {
            run_changed_files_git_mutation_for_weak(inner, &project_id, mutation)
        })
        .await
        .map_err(|error| anyhow::anyhow!("git mutation task failed: {error}"))??;
        Ok(changed_files_wire(outcome.changed_files))
    })
}

fn run_changed_files_git_mutation_for_state(
    registry_state: Arc<Mutex<RegistryState>>,
    project_id: &str,
    mutation: ChangedFilesGitMutation,
) -> anyhow::Result<ProjectGitState> {
    run_changed_files_git_mutation_for_weak(Arc::downgrade(&registry_state), project_id, mutation)
}

fn run_changed_files_git_mutation_for_weak(
    inner: Weak<Mutex<RegistryState>>,
    project_id: &str,
    mutation: ChangedFilesGitMutation,
) -> anyhow::Result<ProjectGitState> {
    let project_path = with_registry_state(&inner, |state| project_path(state, project_id))
        .flatten()
        .ok_or_else(|| anyhow::anyhow!("unknown project: {project_id}"))?;

    another_one_core::git_operation::run_serialized_git_operation_for_path(&project_path, || {
        match mutation {
            ChangedFilesGitMutation::StageFile { changed } => {
                stage_changed_file(&project_path, &changed)
                    .map(|_| read_project_git_state(&project_path, false))
            }
            ChangedFilesGitMutation::UnstageFile { changed } => {
                unstage_changed_file(&project_path, &changed)
                    .map(|_| read_project_git_state(&project_path, false))
            }
            ChangedFilesGitMutation::StageAll => stage_all_changes(&project_path)
                .map(|_| read_project_git_state(&project_path, false)),
            ChangedFilesGitMutation::UnstageAll => unstage_all_changes(&project_path)
                .map(|_| read_project_git_state(&project_path, false)),
            ChangedFilesGitMutation::RevertFiles { changed_files } => {
                let reverted_any = changed_files.iter().fold(false, |reverted_any, changed| {
                    revert_changed_file(&project_path, changed) || reverted_any
                });

                if reverted_any {
                    Ok(read_project_git_state(&project_path, false))
                } else {
                    Err("Could not discard the selected file changes.".to_string())
                }
            }
        }
    })
    .map_err(|error| anyhow::anyhow!(error))
}

fn run_toolbar_git_action_for_weak(
    inner: Weak<Mutex<RegistryState>>,
    project_id: &str,
    action: ToolbarGitAction,
    on_progress: &mut dyn FnMut(String),
) -> Result<another_one_core::git_actions::ToolbarActionOutcome, ToolbarActionError> {
    let (project_path, settings) = with_registry_state(&inner, |state| {
        let project_path = project_path(state, project_id)?;
        Some((project_path, git_action_settings(&state.project_store)))
    })
    .flatten()
    .ok_or_else(|| ToolbarActionError {
        message: format!("unknown project: {project_id}"),
        refresh_git_state: false,
    })?;

    execute_toolbar_git_action(&project_path, action, settings, on_progress)
}

fn toolbar_action_from_id_for_weak(
    inner: &Weak<Mutex<RegistryState>>,
    project_id: &str,
    action_id: &str,
) -> anyhow::Result<ToolbarGitAction> {
    with_registry_state(inner, |state| {
        toolbar_action_from_id(&state.project_store, project_id, action_id)
    })
    .ok_or_else(|| anyhow::anyhow!(registry_unavailable()))?
}

fn toolbar_action_from_id(
    store: &ProjectStore,
    project_id: &str,
    action_id: &str,
) -> anyhow::Result<ToolbarGitAction> {
    let action = match action_id {
        "commit" => ToolbarGitAction::Commit,
        "commit-and-push" => ToolbarGitAction::CommitAndPush,
        "undo-last-commit" => ToolbarGitAction::UndoLastCommit,
        "fetch" => ToolbarGitAction::Fetch,
        "pull" => ToolbarGitAction::Pull,
        "push" => ToolbarGitAction::Push { force: false },
        "force-push" => ToolbarGitAction::Push { force: true },
        "create-pr" => ToolbarGitAction::CreatePr {
            draft: false,
            base_branch: store
                .resolved_branch_settings(project_id)
                .and_then(|settings| settings.effective_default_target_branch),
        },
        "create-draft-pr" => ToolbarGitAction::CreatePr {
            draft: true,
            base_branch: store
                .resolved_branch_settings(project_id)
                .and_then(|settings| settings.effective_default_target_branch),
        },
        _ => anyhow::bail!("unknown toolbar git action: {action_id}"),
    };
    Ok(action)
}

fn toolbar_action_error(error: ToolbarActionError) -> anyhow::Error {
    anyhow::anyhow!(error.message)
}

fn git_action_settings(store: &ProjectStore) -> GitActionSettings {
    GitActionSettings {
        commit_generation_script: store.git_commit_generation_script().to_string(),
        pr_generation_script: store.git_pr_generation_script().to_string(),
        commit_llm: store.git_commit_generation_llm(),
        pr_llm: store.git_pr_generation_llm(),
    }
}

fn project_path(state: &RegistryState, project_id: &str) -> Option<PathBuf> {
    state
        .project_store
        .project(project_id)
        .map(|project| project.path.clone())
}

fn with_registry_state<R>(
    inner: &Weak<Mutex<RegistryState>>,
    f: impl FnOnce(&mut RegistryState) -> R,
) -> Option<R> {
    let arc = inner.upgrade()?;
    let mut guard = arc.lock().ok()?;
    Some(f(&mut guard))
}

fn changed_file_for_mutation(
    path: &str,
    original_path: Option<&str>,
    untracked: bool,
) -> ChangedFile {
    ChangedFile {
        path: path.to_string(),
        original_path: original_path.map(str::to_string),
        staged_additions: 0,
        staged_deletions: 0,
        unstaged_additions: 0,
        unstaged_deletions: 0,
        index_status: ' ',
        worktree_status: if untracked { '?' } else { ' ' },
        untracked,
    }
}

fn changed_file_from_wire(file: ChangedFileWire) -> ChangedFile {
    ChangedFile {
        path: file.path,
        original_path: file.original_path,
        staged_additions: file.staged_additions,
        staged_deletions: file.staged_deletions,
        unstaged_additions: file.unstaged_additions,
        unstaged_deletions: file.unstaged_deletions,
        index_status: single_status_char(&file.index_status),
        worktree_status: single_status_char(&file.worktree_status),
        untracked: file.untracked,
    }
}

fn changed_file_wire(file: ChangedFile) -> ChangedFileWire {
    ChangedFileWire {
        path: file.path,
        original_path: file.original_path,
        staged_additions: file.staged_additions,
        staged_deletions: file.staged_deletions,
        unstaged_additions: file.unstaged_additions,
        unstaged_deletions: file.unstaged_deletions,
        index_status: file.index_status.to_string(),
        worktree_status: file.worktree_status.to_string(),
        untracked: file.untracked,
    }
}

fn changed_files_wire(files: Vec<ChangedFile>) -> Vec<ChangedFileWire> {
    files.into_iter().map(changed_file_wire).collect()
}

fn active_git_state_wire(state: &ProjectGitState) -> ActiveGitStateWire {
    ActiveGitStateWire {
        current_branch: state.current_branch.clone(),
        ahead_count: state.ahead_count as u32,
        behind_count: state.behind_count as u32,
    }
}

fn single_status_char(value: &str) -> char {
    value.chars().next().unwrap_or(' ')
}

/// Parse a wire `section_id` (a `SectionId::store_key()`) + `tab_id`
/// into a `TerminalRuntimeKey`. Returns `None` if the section key is
/// malformed — the daemon will treat the tab as unknown.
fn key_from_wire(section_id: &str, tab_id: &str) -> Option<TerminalRuntimeKey> {
    let section = SectionId::from_store_key(section_id)?;
    Some(TerminalRuntimeKey {
        section_id: section,
        tab_id: tab_id.to_string(),
    })
}

/// Build the `ProjectList` snapshot from the current `RegistryState`.
/// Mirrors the desktop sidebar ordering: projects follow
/// `ProjectStore::project_order`, tasks follow
/// `task_ids_by_root_project`.
fn project_summaries(state: &RegistryState) -> Vec<ProjectSummary> {
    let store = &state.project_store;
    store
        .projects
        .iter()
        // Include both Root and Worktree projects: tasks reference
        // worktree projects via `target_project_id`, and the
        // client-side `absorb_projection` → `sanitize` drops any task
        // whose target project isn't in the projection. The sidebar
        // UI filters to Root entries on its own; the wire format
        // carries the full graph.
        .map(|project| {
            // Tasks are keyed by root_project_id, so worktree summaries
            // naturally carry an empty task list — the task lives on
            // its root.
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
                repo_id: project.repo_id.clone(),
                worktree_name: project.worktree_name.clone(),
                checkout: serde_json::to_value(&project.checkout).ok(),
                branch_settings: serde_json::to_value(&project.branch_settings).ok(),
                actions: serde_json::to_value(&project.actions).unwrap_or_default(),
            }
        })
        .collect()
}

/// Look up the wire `TaskSummary` for `task_id` in the current
/// `RegistryState`. Used by mutator trait impls (`rename_task` /
/// `set_task_pinned`) to return the post-mutation projection inline
/// per the trait contract.
fn task_summary_for(state: &RegistryState, task_id: &str) -> Option<TaskSummary> {
    let task = state
        .project_store
        .tasks
        .values()
        .flatten()
        .find(|t| t.id == task_id)
        .cloned()?;
    Some(task_to_summary(state, task))
}

fn task_to_summary(
    state: &RegistryState,
    task: another_one_core::project_store::Task,
) -> TaskSummary {
    let store = &state.project_store;
    let section_key = task.section_id.clone();
    let parsed_section = SectionId::from_store_key(&section_key);
    let task_pinned = store.ui.pinned_task_ids.contains(&task.id);
    let cwd = task
        .cwd
        .as_ref()
        .map(|p| p.to_string_lossy().into_owned());
    let next_tab_id = task.next_tab_id;
    let kind_value = serde_json::to_value(&task.kind).ok();
    let root_project_id = task.root_project_id.clone();
    let worktree_project_id = task.worktree_project_id.clone();
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
                restore_status: tab.restore_status,
                failure_message: tab.failure_message,
                failure_details: tab.failure_details,
                launch_config: tab.launch_config.as_ref().and_then(|cfg| {
                    serde_json::to_value(cfg).ok()
                }),
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
        cwd,
        next_tab_id,
        root_project_id,
        kind: kind_value,
        worktree_project_id,
    }
}

// Trivial wrappers retained as call-site readability sugar around
// the bidirectional `From` impls in `core::project_store` /
// `core::agents`. Inlining the `.into()` at each site is fine; this
// is just a one-liner and the indirection is free at codegen time.
fn map_project_kind(kind: CoreProjectKind) -> ProjectKind {
    kind.into()
}

fn map_agent_provider(kind: AgentProviderKind) -> AgentProvider {
    kind.into()
}

fn agent_summary_wire(agent: &another_one_core::agents::AgentDef) -> AgentSummaryWire {
    AgentSummaryWire {
        id: agent.id.to_string(),
        label: agent.label.to_string(),
        icon_path: agent.icon.to_string(),
        provider: agent.provider.map(map_agent_provider),
    }
}

fn agent_settings_view(store: &ProjectStore) -> AgentSettingsViewWire {
    let default_agent_id = store.default_agent_id().map(str::to_string);
    AgentSettingsViewWire {
        agents: AGENTS
            .iter()
            .map(|agent| AgentSettingsRowWire {
                id: agent.id.to_string(),
                label: agent.label.to_string(),
                icon_path: agent.icon.to_string(),
                provider: agent.provider.map(map_agent_provider),
                enabled: store.agent_enabled(agent.id),
                is_default: store.agent_is_default(agent.id),
                launch_args: store.agent_launch_args(agent.id).to_vec(),
            })
            .collect(),
        default_agent_id,
    }
}

fn ensure_agent_id(agent_id: &str) -> Result<(), String> {
    if AGENTS.iter().any(|agent| agent.id == agent_id) {
        Ok(())
    } else {
        Err(format!("unknown agent: {agent_id}"))
    }
}

fn registry_unavailable() -> String {
    "desktop registry state is unavailable".to_string()
}

fn open_in_app_wire(app: OpenInAppKind) -> OpenInAppWire {
    OpenInAppWire {
        id: app.id().to_string(),
        label: app.label().to_string(),
        description: app.description().to_string(),
        icon_path: app.icon_path().to_string(),
    }
}

fn open_in_app_from_id(id: &str) -> Option<OpenInAppKind> {
    OpenInAppKind::all().into_iter().find(|app| app.id() == id)
}

pub(crate) fn shortcut_action_id(action: ShortcutAction) -> &'static str {
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

fn shortcut_action_from_id(id: &str) -> Option<ShortcutAction> {
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

fn provider_id(provider: AgentProviderKind) -> &'static str {
    match provider {
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

fn provider_from_id(id: &str) -> Option<AgentProviderKind> {
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

fn mcp_source_dto(source: McpSource) -> McpSourceDto {
    match source {
        McpSource::Catalog => McpSourceDto::Catalog,
        McpSource::Custom => McpSourceDto::Custom,
        McpSource::BuiltInDaemon => McpSourceDto::BuiltInDaemon,
    }
}

fn mcp_transport_kind_dto(transport: &McpTransport) -> McpTransportKindDto {
    match transport {
        McpTransport::Stdio { .. } => McpTransportKindDto::Stdio,
        McpTransport::Http { .. } => McpTransportKindDto::Http,
    }
}

fn mcp_server_dto(server: &McpServer) -> McpServerDto {
    let mut enabled_for = server
        .enabled_for
        .iter()
        .map(|provider| provider_id(*provider).to_string())
        .collect::<Vec<_>>();
    enabled_for.sort();
    McpServerDto {
        id: server.id.clone(),
        label: server.label.clone(),
        source: mcp_source_dto(server.source),
        transport_kind: mcp_transport_kind_dto(&server.transport),
        enabled_for,
    }
}

fn mcp_catalog_entry_dto(entry: &catalog::CatalogEntry) -> McpCatalogEntryDto {
    McpCatalogEntryDto {
        id: entry.id.to_string(),
        label: entry.label.to_string(),
        description: entry.description.to_string(),
        docs_url: entry.docs_url.to_string(),
    }
}

fn mcp_sync_errors(report: HashMap<AgentProviderKind, anyhow::Result<()>>) -> Vec<String> {
    report
        .into_iter()
        .filter_map(|(provider, result)| {
            result
                .err()
                .map(|err| format!("{}: {err:#}", provider_id(provider)))
        })
        .collect()
}

/// Bundle of handles the daemon-host thread hands back to the GUI.
/// `endpoint_rx` carries the iroh `EndpointHandle` once the network
/// endpoint binds (mobile clients dial this via QR pairing). `session`
/// is the in-process client half of an `in_memory::pair()` whose
/// server half the daemon-host drives via `serve_session` — every
/// daemon interaction the GUI makes on desktop flows through this
/// session so the network-vs-in-process distinction is opaque to
/// callers (mobile holds an `IrohSession` on the same trait).
pub(crate) struct DaemonHostHandles {
    pub(crate) endpoint_rx: mpsc::Receiver<anyhow::Result<EndpointHandle>>,
    pub(crate) session: Arc<dyn daemon_transport::Session>,
}

/// Spawn the embedded daemon on a dedicated OS thread with its own
/// tokio runtime. Returns the in-process `Session` the GUI uses to
/// reach the embedded daemon plus a receiver the GPUI render tick
/// polls for the `EndpointHandle`; the first `try_recv` that yields
/// the handle caches it on `AnotherOneApp`.
///
/// The thread keeps running until the process exits; dropping the
/// `EndpointHandle` (which happens when `AnotherOneApp` drops) aborts
/// the endpoint's root task, the runtime unwinds, and the thread
/// returns. No signalling needed on the app side.
pub(crate) fn spawn(
    registry_state: Arc<Mutex<RegistryState>>,
    event_bus: tokio::sync::broadcast::Sender<another_one_core::clients::ClientEvent>,
) -> DaemonHostHandles {
    let (endpoint_tx, endpoint_rx) = mpsc::channel();
    // Build the in-memory pair *before* spawning the daemon thread so
    // we can hand the client half back synchronously. `pair()` itself
    // needs a tokio context (it `tokio::spawn`s the recv router) — use
    // the shared session-host runtime which is also what drives every
    // GUI-issued `session.call(...)`.
    let (server_session, client_session) = crate::session_host::runtime_handle()
        .block_on(async { daemon_transport::in_memory::pair("gui:desktop") });
    let session: Arc<dyn daemon_transport::Session> = Arc::from(client_session);
    let server_session: Arc<dyn daemon_transport::ServerSession> = Arc::from(server_session);
    thread::Builder::new()
        .name("another-one-daemon".into())
        .spawn(move || run(registry_state, event_bus, endpoint_tx, server_session))
        .expect("spawn daemon-host thread");
    DaemonHostHandles {
        endpoint_rx,
        session,
    }
}

fn run(
    registry_state: Arc<Mutex<RegistryState>>,
    event_bus: tokio::sync::broadcast::Sender<another_one_core::clients::ClientEvent>,
    tx: mpsc::Sender<anyhow::Result<EndpointHandle>>,
    in_process_server: Arc<dyn daemon_transport::ServerSession>,
) {
    // Four workers so a single stuck PTY write (child paused /
    // pipe buffer full) + its `block_in_place` scope don't starve
    // accept loop, writer task, and forwarder concurrently. Two is
    // the minimum viable count; four gives comfortable headroom
    // against the ~3 concurrent tab_inputs you can get when desktop
    // + phone type at the same tab during a resize burst.
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(4)
        .enable_all()
        .thread_name("another-one-daemon-rt")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = tx.send(Err(
                anyhow::Error::new(e).context("build daemon tokio runtime")
            ));
            return;
        }
    };

    let weak = Arc::downgrade(&registry_state);
    drop(registry_state); // drop the strong ref we took for spawn; the app still holds one.
    let registry: Arc<dyn DaemonRegistry> = Arc::new(DesktopTerminalRegistry::new(weak.clone()));
    let mcp_orchestrator = crate::mcp_orchestrator::arc(weak, event_bus);

    let paths = match daemon_paths() {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(Err(e));
            return;
        }
    };

    // Start the local MCP UDS listener alongside the iroh
    // endpoint. The handle is leaked on success because the
    // daemon thread parks for the rest of the process lifetime;
    // `McpListener::drop` would unlink the socket, which is
    // exactly what we want at process exit but not mid-run. A
    // bind failure only warns — the desktop still runs without
    // a local MCP socket (mobile iroh path is independent).
    let mcp_socket_path = daemon::transport_mcp::default_socket_path();
    // Retry the bind in the background when it loses a startup race
    // with a still-running prior instance. The probe in
    // `unlink_if_ours_and_dead` only sees "alive" if a listener
    // actually answers; once that listener dies we take over on the
    // next retry tick. Backs off from 5s → 30s after the first few
    // misses to keep logs quiet during long overlaps.
    let mcp_path_for_task = mcp_socket_path.clone();
    let mcp_orch_for_task = mcp_orchestrator.clone();
    runtime.spawn(async move {
        let mut attempt: u32 = 0;
        let listener = loop {
            match daemon::transport_mcp::spawn(mcp_path_for_task.clone(), mcp_orch_for_task.clone())
            {
                Ok(listener) => {
                    if attempt > 0 {
                        log::info!(
                            "mcp: bound listener at {} after {} retries",
                            mcp_path_for_task.display(),
                            attempt
                        );
                    } else {
                        log::info!(
                            "mcp: daemon MCP listener started at {}",
                            mcp_path_for_task.display()
                        );
                    }
                    break listener;
                }
                Err(err) => {
                    if attempt == 0 {
                        log::warn!(
                            "mcp: initial bind at {} failed ({err}); retrying",
                            mcp_path_for_task.display()
                        );
                    } else if attempt % 12 == 0 {
                        log::warn!(
                            "mcp: still unable to bind at {} after {} attempts ({err})",
                            mcp_path_for_task.display(),
                            attempt + 1
                        );
                    }
                    let delay = if attempt < 6 {
                        std::time::Duration::from_secs(5)
                    } else {
                        std::time::Duration::from_secs(30)
                    };
                    tokio::time::sleep(delay).await;
                    attempt += 1;
                }
            }
        };
        // Park the task forever, holding the listener in scope. When
        // the daemon's runtime shuts down (process exit) the task is
        // aborted and the listener's `Drop` runs — which unlinks the
        // socket file. Combined with the panic hook + SIGTERM/SIGINT
        // handler in `transport_mcp::spawn`, every termination path
        // cleans up the socket transparently to the user.
        std::future::pending::<()>().await;
        drop(listener);
    });

    // Drive the in-process Session. The GUI on the same process holds
    // the matched client half (`session: Arc<dyn Session>` on
    // `AnotherOneApp`) and issues every daemon-equivalent verb through
    // it; we accept those verbs here on the daemon's own runtime via
    // the same `serve_session` dispatcher the iroh accept loop drives
    // for mobile clients. Nothing about handler logic is in-process
    // vs over the wire — it's the same dispatch path either way, which
    // is the whole point of the daemon-transport seam.
    let in_process_registry = registry.clone();
    runtime.spawn(async move {
        if let Err(e) = daemon::dispatch::serve_session(in_process_server, in_process_registry).await {
            log::warn!("in-process serve_session ended with error: {e}");
        }
    });

    let endpoint_result = runtime.block_on(async {
        daemon::run_endpoint(registry, paths.secret_key, paths.paired_peers).await
    });

    match endpoint_result {
        Ok(handle) => {
            if tx.send(Ok(handle)).is_err() {
                // App dropped before we returned; abort immediately by
                // dropping the runtime and returning.
                return;
            }
            // Park the thread for the rest of the process lifetime —
            // dropping the runtime would cancel the endpoint, but the
            // app holds the handle; instead, park until the handle is
            // dropped and the endpoint's root task aborts, at which
            // point block_on would have returned on a new awaiter. We
            // simply hold the runtime alive.
            loop {
                thread::park();
            }
        }
        Err(e) => {
            let _ = tx.send(Err(e));
        }
    }
}

struct DaemonPaths {
    secret_key: PathBuf,
    paired_peers: PathBuf,
}

/// Public accessor for the allowlist path so the "Pair mobile" modal's
/// reset button can unlink it. Thin wrapper; same resolution as the
/// daemon uses at boot.
pub(crate) fn paired_peers_path() -> anyhow::Result<PathBuf> {
    Ok(daemon_paths()?.paired_peers)
}

/// Resolve the on-disk paths for the daemon's identity + TOFU
/// allowlist. Mirrors the sandbox binary's resolution logic, but
/// roots the directory under `…/another-one/daemon/` so an embedded
/// daemon (running alongside the regular AnotherOne config) doesn't
/// collide with a standalone `daemon-sandbox` running on the same
/// machine.
fn daemon_paths() -> anyhow::Result<DaemonPaths> {
    let base = if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        PathBuf::from(xdg)
    } else {
        let home = std::env::var("HOME")
            .map_err(|_| anyhow::anyhow!("HOME is unset — can't locate daemon config dir"))?;
        PathBuf::from(home).join(".config")
    };
    let dir = base.join("another-one").join("daemon");
    std::fs::create_dir_all(&dir)
        .map_err(|e| anyhow::anyhow!("create daemon dir {}: {e}", dir.display()))?;
    Ok(DaemonPaths {
        secret_key: dir.join("secret_key"),
        paired_peers: dir.join("paired_peers"),
    })
}
