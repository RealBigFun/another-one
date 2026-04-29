//! Embedded iroh daemon host.
//!
//! Desktop is GPUI-only — no ambient tokio runtime — so booting the
//! `daemon-sandbox` library requires us to bring our own runtime.
//! This module owns:
//!
//! * A dedicated OS thread that runs a `tokio::runtime::Runtime` and
//!   blocks on `daemon_sandbox::run_endpoint`.
//! * [`RegistryState`] — shared state the registry trait object reads
//!   (projects, live broadcast senders, live writers, pending resize
//!   requests). Wrapped in an `Arc<Mutex<…>>` so the daemon's tokio
//!   tasks can query it without cx access; the GPUI side mutates the
//!   same mutex on every `TerminalLaunchReply::Launched` /
//!   `…::Terminated` / tab-close.
//! * [`DesktopTerminalRegistry`] — the `daemon_sandbox::DaemonRegistry`
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
use std::path::{Path, PathBuf};
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::thread;

use tokio::sync::broadcast;

use daemon_sandbox::frame::{
    AgentProvider, AgentSettingsRowWire, AgentSettingsViewWire, AgentSummaryWire,
    EnabledAgentsViewWire, GitActionScriptsView, McpCatalogEntryDto, McpServerDto, McpSettingsView,
    McpSourceDto, McpTransportKindDto, OpenInAppSettingsRowWire, OpenInAppWire,
    OpenInSettingsViewWire, OpenInStateWire, ProjectKind, ProjectSummary, ShortcutSettingsRow,
    ShortcutSettingsView, TabSummary, TaskSummary,
};
use daemon_sandbox::{DaemonRegistry, EndpointHandle};

use another_one_core::agents::{
    AgentProviderKind, TerminalLaunchConfig, TerminalRestoreStatus, AGENTS,
};
use another_one_core::git_actions::find_github_repo_url;
use another_one_core::mcp::catalog;
use another_one_core::mcp::registry::McpRegistry;
use another_one_core::mcp::{McpServer, McpSource, McpTransport};
use another_one_core::project_store::{
    PersistedSectionState, PersistedTerminalTab, ProjectKind as CoreProjectKind, ProjectStore,
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
pub(crate) struct RegistryState {
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
    /// Tab-close requests from daemon clients. The daemon thread can
    /// update persisted state immediately, but live PTY teardown must
    /// run on the GPUI thread that owns `LiveTerminalRuntime`.
    pub(crate) pending_tab_closures: Vec<TabCloseRequest>,
    /// Keys currently mid-spawn. Populated when either path
    /// (daemon-queued mobile LaunchTab **or** desktop sidebar click)
    /// kicks off a `spawn_terminal_launch`; cleared on
    /// `TerminalLaunchReply::Launched` / `Failed` / tab close. The
    /// daemon checks this to dedupe — earlier builds only checked
    /// `pending_tab_launches` + `broadcasts`, which left a window
    /// between "spawn kicked off" and "Launched reply observed"
    /// where a second LaunchTab would spawn a duplicate PTY.
    pub(crate) in_flight_launches: HashSet<TerminalRuntimeKey>,
}

impl RegistryState {
    pub(crate) fn new(project_store: ProjectStore) -> Self {
        Self {
            project_store,
            broadcasts: HashMap::new(),
            writers: HashMap::new(),
            pending_resizes: Vec::new(),
            pending_tab_launches: Vec::new(),
            pending_tab_closures: Vec::new(),
            in_flight_launches: HashSet::new(),
            active_viewers: HashMap::new(),
            viewer_focus: HashMap::new(),
            effective_sizes: HashMap::new(),
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

#[derive(Clone, Debug)]
pub(crate) struct TabCloseRequest {
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

/// `DaemonRegistry` implementation that projects `AnotherOneApp`
/// state onto the wire. Holds a `Weak` so a late-arriving daemon
/// callback after app shutdown drops out cleanly instead of keeping
/// the app alive.
pub(crate) struct DesktopTerminalRegistry {
    inner: Weak<Mutex<RegistryState>>,
}

impl DesktopTerminalRegistry {
    pub(crate) fn new(inner: Weak<Mutex<RegistryState>>) -> Self {
        Self { inner }
    }

    fn with_state<R>(&self, f: impl FnOnce(&mut RegistryState) -> R) -> Option<R> {
        let arc = self.inner.upgrade()?;
        let mut guard = arc.lock().ok()?;
        Some(f(&mut guard))
    }

    fn with_fresh_project_store<R>(&self, f: impl FnOnce(&mut ProjectStore) -> R) -> Option<R> {
        self.with_state(|state| {
            state.project_store = ProjectStore::load();
            f(&mut state.project_store)
        })
    }

    fn mutate_persisted_section<R>(
        &self,
        section_id: &str,
        f: impl FnOnce(&mut PersistedSectionState) -> Result<R, String>,
    ) -> Result<R, String> {
        let section = SectionId::from_store_key(section_id)
            .ok_or_else(|| format!("malformed section id: {section_id}"))?;
        let section_key = section.store_key();
        let result = self
            .with_fresh_project_store(|store| {
                let mut persisted = store
                    .terminal_sections
                    .get(&section_key)
                    .cloned()
                    .ok_or_else(|| format!("unknown section: {section_key}"))?;
                let result = f(&mut persisted)?;
                store.set_terminal_section(section_key.clone(), persisted);
                store.set_last_active_section_key(Some(section_key.clone()));
                Ok::<R, String>(result)
            })
            .ok_or_else(registry_unavailable)??;
        self.with_state(|state| {
            state.project_store = ProjectStore::load();
        });
        Ok(result)
    }
}

impl DaemonRegistry for DesktopTerminalRegistry {
    fn health(&self) -> Result<(), String> {
        self.with_state(|_| ())
            .ok_or_else(|| "desktop registry state is unavailable".to_string())
    }

    fn list_projects(&self) -> Vec<ProjectSummary> {
        self.with_state(|state| {
            // Project/task data lives in the same store as main
            // (`.../another-one/projects.json`). Refresh here so
            // daemon clients never read a stale GPUI snapshot after
            // project/task mutations; live PTY running state is still
            // layered from this registry's broadcast maps below.
            state.project_store = ProjectStore::load();
            project_summaries(state)
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

    fn add_agent_to_section(&self, section_id: &str, agent_id: &str) -> Result<String, String> {
        let launch_config = launch_config_for_agent_id(agent_id)?;
        let tab_id = self.mutate_persisted_section(section_id, |section| {
            let tab_id = uuid::Uuid::new_v4().to_string();
            section.next_tab_id = section.next_tab_id.saturating_add(1);
            section
                .tabs
                .push(persisted_tab_for_launch_config(&tab_id, launch_config));
            section.active_tab_id = tab_id.clone();
            Ok(tab_id)
        })?;
        self.launch_tab(section_id, &tab_id);
        Ok(tab_id)
    }

    fn activate_section_tab(&self, section_id: &str, tab_id: &str) -> Result<(), String> {
        self.mutate_persisted_section(section_id, |section| {
            if !section.tabs.iter().any(|tab| tab.id == tab_id) {
                return Err(format!("unknown tab: {tab_id}"));
            }
            section.active_tab_id = tab_id.to_string();
            Ok(())
        })
    }

    fn close_section_tab(&self, section_id: &str, tab_id: &str) -> Result<String, String> {
        let active_tab_id = self.mutate_persisted_section(section_id, |section| {
            let removed_index = section
                .tabs
                .iter()
                .position(|tab| tab.id == tab_id)
                .ok_or_else(|| format!("unknown tab: {tab_id}"))?;
            let removed_was_active = section.active_tab_id == tab_id;
            section.tabs.remove(removed_index);
            if section.tabs.is_empty() {
                section.active_tab_id.clear();
            } else if removed_was_active
                || !section
                    .tabs
                    .iter()
                    .any(|tab| tab.id == section.active_tab_id)
            {
                let next_index = removed_index.min(section.tabs.len().saturating_sub(1));
                section.active_tab_id = section.tabs[next_index].id.clone();
            }
            Ok(section.active_tab_id.clone())
        })?;
        if let Some(key) = key_from_wire(section_id, tab_id) {
            self.with_state(|state| {
                remove_registry_tab_state(state, &key);
                state.pending_tab_closures.push(TabCloseRequest { key });
            });
        }
        Ok(active_tab_id)
    }

    fn toggle_section_tab_pinned(&self, section_id: &str, tab_id: &str) -> Result<bool, String> {
        self.mutate_persisted_section(section_id, |section| {
            let tab = section
                .tabs
                .iter_mut()
                .find(|tab| tab.id == tab_id)
                .ok_or_else(|| format!("unknown tab: {tab_id}"))?;
            tab.pinned = !tab.pinned;
            let pinned = tab.pinned;
            let active_tab_id = section.active_tab_id.clone();
            section.tabs.sort_by_key(|tab| !tab.pinned);
            section.active_tab_id = active_tab_id;
            Ok(pinned)
        })
    }

    fn open_in_state(&self) -> Option<OpenInStateWire> {
        let available = detect_available_open_in_apps();
        self.with_fresh_project_store(|store| {
            let enabled = store.enabled_open_in_apps(&available);
            let preferred = store.preferred_open_in_app(&available);
            OpenInStateWire {
                enabled_apps: enabled.into_iter().map(open_in_app_wire).collect(),
                preferred_app_id: preferred.map(|app| app.id().to_string()),
            }
        })
    }

    fn read_enabled_agents(&self) -> EnabledAgentsViewWire {
        self.with_fresh_project_store(|store| {
            let enabled_ids = store
                .enabled_agent_ids()
                .into_iter()
                .collect::<HashSet<_>>();
            EnabledAgentsViewWire {
                agents: AGENTS
                    .iter()
                    .filter(|agent| enabled_ids.contains(agent.id))
                    .map(agent_summary_wire)
                    .collect(),
                default_agent_id: store.default_agent_id().map(str::to_string),
            }
        })
        .unwrap_or(EnabledAgentsViewWire {
            agents: Vec::new(),
            default_agent_id: None,
        })
    }

    fn read_agent_settings(&self) -> AgentSettingsViewWire {
        self.with_fresh_project_store(|store| agent_settings_view(store))
            .unwrap_or(AgentSettingsViewWire {
                agents: Vec::new(),
                default_agent_id: None,
            })
    }

    fn set_agent_enabled(&self, agent_id: &str, enabled: bool) -> Result<bool, String> {
        ensure_agent_id(agent_id)?;
        self.with_fresh_project_store(|store| store.set_agent_enabled(agent_id, enabled))
            .ok_or_else(registry_unavailable)
    }

    fn set_default_agent(&self, agent_id: &str) -> Result<bool, String> {
        ensure_agent_id(agent_id)?;
        self.with_fresh_project_store(|store| store.set_default_agent(agent_id))
            .ok_or_else(registry_unavailable)
    }

    fn set_agent_launch_args(&self, agent_id: &str, args: Vec<String>) -> Result<bool, String> {
        ensure_agent_id(agent_id)?;
        self.with_fresh_project_store(|store| store.set_agent_launch_args(agent_id, args))
            .ok_or_else(registry_unavailable)
    }

    fn read_open_in_settings(&self) -> Option<OpenInSettingsViewWire> {
        let available = detect_available_open_in_apps();
        self.with_fresh_project_store(|store| OpenInSettingsViewWire {
            available_apps: available
                .iter()
                .copied()
                .map(|app| OpenInAppSettingsRowWire {
                    id: app.id().to_string(),
                    label: app.label().to_string(),
                    description: app.description().to_string(),
                    icon_path: app.icon_path().to_string(),
                    enabled: store.open_in_app_enabled(app, &available),
                })
                .collect(),
        })
    }

    fn set_open_in_app_enabled(&self, app_id: &str, enabled: bool) -> Result<(), String> {
        let app =
            open_in_app_from_id(app_id).ok_or_else(|| format!("unknown Open-In app: {app_id}"))?;
        let available = detect_available_open_in_apps();
        self.with_fresh_project_store(|store| {
            store.set_open_in_app_enabled(app, enabled, &available);
        })
        .ok_or_else(registry_unavailable)
    }

    fn open_project_in_app(&self, project_id: &str, app_id: &str) -> Result<(), String> {
        let app =
            open_in_app_from_id(app_id).ok_or_else(|| format!("unknown Open-In app: {app_id}"))?;
        let (path, available) = self
            .with_fresh_project_store(|store| {
                let path = store
                    .project(project_id)
                    .map(|project| project.path.clone());
                (path, detect_available_open_in_apps())
            })
            .ok_or_else(registry_unavailable)?;
        let path = path.ok_or_else(|| format!("unknown project: {project_id}"))?;
        open_path_in_app(&path, app)?;
        self.with_fresh_project_store(|store| {
            store.set_preferred_open_in_app(app, &available);
        })
        .ok_or_else(registry_unavailable)
    }

    fn read_git_action_scripts(&self) -> GitActionScriptsView {
        self.with_fresh_project_store(|store| GitActionScriptsView {
            commit_script: store.git_commit_generation_script().to_string(),
            commit_using_default: store
                .ui
                .git_commit_generation_script
                .as_deref()
                .is_none_or(|script| script.trim().is_empty()),
            pr_script: store.git_pr_generation_script().to_string(),
            pr_using_default: store
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
        self.with_fresh_project_store(|store| store.set_git_commit_generation_script(script))
            .ok_or_else(registry_unavailable)
    }

    fn reset_git_commit_script(&self) -> Result<bool, String> {
        self.with_fresh_project_store(|store| store.reset_git_commit_generation_script())
            .ok_or_else(registry_unavailable)
    }

    fn set_git_pr_script(&self, script: &str) -> Result<bool, String> {
        self.with_fresh_project_store(|store| store.set_git_pr_generation_script(script))
            .ok_or_else(registry_unavailable)
    }

    fn reset_git_pr_script(&self) -> Result<bool, String> {
        self.with_fresh_project_store(|store| store.reset_git_pr_generation_script())
            .ok_or_else(registry_unavailable)
    }

    fn read_shortcut_settings(&self) -> ShortcutSettingsView {
        self.with_fresh_project_store(|store| ShortcutSettingsView {
            actions: ALL_SHORTCUT_ACTIONS
                .into_iter()
                .map(|action| ShortcutSettingsRow {
                    id: shortcut_action_id(action).to_string(),
                    label: action.label().to_string(),
                    current_binding: store.ui.shortcuts.binding_for(action).to_string(),
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
        self.with_fresh_project_store(|store| {
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
        self.with_fresh_project_store(|store| store.reset_shortcut_binding(action))
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

    fn read_project_github_url(&self, project_id: &str) -> Option<String> {
        let project_path = self
            .with_fresh_project_store(|store| {
                store
                    .projects
                    .iter()
                    .find(|project| project.id == project_id)
                    .map(|project| project.path.clone())
            })
            .flatten()?;

        tokio::task::block_in_place(|| find_github_repo_url(&project_path))
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

fn launch_config_for_agent_id(agent_id: &str) -> Result<TerminalLaunchConfig, String> {
    if agent_id.is_empty() {
        return Ok(TerminalLaunchConfig::default());
    }
    let agent = AGENTS
        .iter()
        .find(|agent| agent.id == agent_id)
        .ok_or_else(|| format!("unknown agent: {agent_id}"))?;
    let provider = agent
        .provider
        .ok_or_else(|| format!("agent has no terminal provider: {agent_id}"))?;
    Ok(TerminalLaunchConfig::for_provider(provider))
}

fn persisted_tab_for_launch_config(
    tab_id: &str,
    launch_config: TerminalLaunchConfig,
) -> PersistedTerminalTab {
    PersistedTerminalTab {
        id: tab_id.to_string(),
        title: launch_config.default_title(),
        pinned: false,
        fixed_title: None,
        provider: launch_config.provider,
        launch_config: Some(launch_config),
        restore_status: TerminalRestoreStatus::NotStarted,
        failure_message: None,
        failure_details: None,
    }
}

fn remove_registry_tab_state(state: &mut RegistryState, key: &TerminalRuntimeKey) {
    state.broadcasts.remove(key);
    state.writers.remove(key);
    state.active_viewers.remove(key);
    state.effective_sizes.remove(key);
    state.in_flight_launches.remove(key);
    state
        .pending_tab_launches
        .retain(|request| request.key != *key);
    state.viewer_focus.retain(|_, focus_key| focus_key != key);
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
        // Mobile drawer mirrors the desktop sidebar's *root* project
        // list — worktree-kind projects are nested under their root
        // (via `task.worktree_project_id`) and should never appear at
        // the top level. Filter them out here.
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
                restore_status: tab.restore_status,
                failure_message: tab.failure_message,
                failure_details: tab.failure_details,
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

fn shortcut_action_id(action: ShortcutAction) -> &'static str {
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

/// Spawn the embedded daemon on a dedicated OS thread with its own
/// tokio runtime. Returns a receiver the GPUI render tick polls; the
/// first `try_recv` that yields the handle caches it on
/// `AnotherOneApp`.
///
/// The thread keeps running until the process exits; dropping the
/// `EndpointHandle` (which happens when `AnotherOneApp` drops) aborts
/// the endpoint's root task, the runtime unwinds, and the thread
/// returns. No signalling needed on the app side.
pub(crate) fn spawn(
    registry_state: Arc<Mutex<RegistryState>>,
) -> mpsc::Receiver<anyhow::Result<EndpointHandle>> {
    let (tx, rx) = mpsc::channel();
    thread::Builder::new()
        .name("another-one-daemon".into())
        .spawn(move || run(registry_state, tx))
        .expect("spawn daemon-host thread");
    rx
}

fn run(
    registry_state: Arc<Mutex<RegistryState>>,
    tx: mpsc::Sender<anyhow::Result<EndpointHandle>>,
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
    let mcp_orchestrator = crate::mcp_orchestrator::arc(weak);

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
    let mcp_socket_path = daemon_sandbox::transport_mcp::default_socket_path();
    match runtime.block_on(async {
        daemon_sandbox::transport_mcp::spawn(mcp_socket_path.clone(), mcp_orchestrator)
    }) {
        Ok(listener) => {
            log::info!(
                "mcp: daemon MCP listener started at {}",
                mcp_socket_path.display()
            );
            std::mem::forget(listener);
        }
        Err(err) => {
            log::warn!("mcp: failed to start local listener; continuing: {err}");
        }
    }

    let endpoint_result = runtime.block_on(async {
        daemon_sandbox::run_endpoint(registry, paths.secret_key, paths.paired_peers).await
    });

    match endpoint_result {
        Ok(handle) => {
            publish_embedded_daemon_ticket(&handle, &paths.ticket);
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
    ticket: PathBuf,
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
        ticket: dir.join("endpoint.ticket"),
    })
}

fn publish_embedded_daemon_ticket(handle: &EndpointHandle, ticket_path: &Path) {
    let body = daemon_ticket_body(handle);
    if let Err(e) = write_private_file(ticket_path, body.as_bytes()) {
        log::warn!(
            "daemon: failed to publish embedded endpoint ticket at {}: {e:#}",
            ticket_path.display()
        );
    } else {
        log::info!(
            "daemon: embedded endpoint ticket written to {}",
            ticket_path.display()
        );
    }

    // Keep the legacy smoke-client path fresh too. Slint prefers the
    // embedded ticket above, but this prevents stale standalone
    // sandbox tickets from confusing local iroh diagnostics.
    let legacy_path = std::env::temp_dir().join("daemon-sandbox.ticket");
    if let Err(e) = write_private_file(&legacy_path, body.as_bytes()) {
        log::warn!(
            "daemon: failed to refresh legacy endpoint ticket at {}: {e:#}",
            legacy_path.display()
        );
    }
}

fn daemon_ticket_body(handle: &EndpointHandle) -> String {
    let mut body = format!("id={}\n", handle.endpoint_id);
    for addr in handle.direct_addrs() {
        body.push_str("addr=");
        body.push_str(&addr);
        body.push('\n');
    }
    for relay in handle.relay_urls() {
        body.push_str("relay=");
        body.push_str(&relay);
        body.push('\n');
    }
    body
}

fn write_private_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| anyhow::anyhow!("create dir {}: {e}", parent.display()))?;
    }
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .map_err(|e| anyhow::anyhow!("open {}: {e}", path.display()))?;
    file.write_all(bytes)
        .map_err(|e| anyhow::anyhow!("write {}: {e}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .map_err(|e| anyhow::anyhow!("set permissions {}: {e}", path.display()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    static PROJECT_STORE_ENV_LOCK: Mutex<()> = Mutex::new(());

    struct EnvRestore {
        key: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl EnvRestore {
        fn set(key: &'static str, value: &std::path::Path) -> Self {
            let previous = std::env::var_os(key);
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvRestore {
        fn drop(&mut self) {
            if let Some(previous) = self.previous.as_ref() {
                std::env::set_var(self.key, previous);
            } else {
                std::env::remove_var(self.key);
            }
        }
    }

    #[test]
    fn settings_reads_refresh_project_store_before_projecting_shortcuts() {
        let _lock = PROJECT_STORE_ENV_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        let temp_dir = tempfile::tempdir().expect("temp config dir");
        let _env = EnvRestore::set("XDG_CONFIG_HOME", temp_dir.path());

        let stale_store = ProjectStore::load();
        let mut disk_store = ProjectStore::load();
        disk_store.clear_shortcut_binding(ShortcutAction::NewTask);

        let state = Arc::new(Mutex::new(RegistryState::new(stale_store)));
        let registry = DesktopTerminalRegistry::new(Arc::downgrade(&state));

        let shortcuts = registry.read_shortcut_settings();

        let new_task = shortcuts
            .actions
            .iter()
            .find(|row| row.id == "new-task")
            .expect("new-task shortcut row");
        assert_eq!(new_task.current_binding, "");
        assert_eq!(
            new_task.default_binding,
            ShortcutAction::NewTask.default_binding()
        );
    }

    #[test]
    fn tab_launch_config_maps_empty_agent_to_shell() {
        let config = launch_config_for_agent_id("").expect("empty agent id maps to shell");

        assert_eq!(config.provider, None);
        assert_eq!(config.default_title(), "Terminal");
    }

    #[test]
    fn tab_launch_config_rejects_unknown_agent() {
        let error = launch_config_for_agent_id("not-a-real-agent").unwrap_err();

        assert!(error.contains("unknown agent"));
    }

    #[test]
    fn persisted_tab_uses_launch_config_title_and_provider() {
        let config = TerminalLaunchConfig::for_provider(AgentProviderKind::ClaudeCode);
        let tab = persisted_tab_for_launch_config("tab-1", config);

        assert_eq!(tab.id, "tab-1");
        assert_eq!(tab.title, "Claude Code");
        assert_eq!(tab.provider, Some(AgentProviderKind::ClaudeCode));
        assert_eq!(tab.restore_status, TerminalRestoreStatus::NotStarted);
    }
}
