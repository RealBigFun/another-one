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
//! * [`DesktopTerminalRegistry`] — the `daemon_sandbox::TerminalRegistry`
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

use daemon_sandbox::frame::{
    AgentProvider, ProjectKind, ProjectSummary, TabSummary, TaskSummary,
};
use daemon_sandbox::{EndpointHandle, TerminalRegistry};

use another_one_core::agents::AgentProviderKind;
use another_one_core::project_store::{ProjectKind as CoreProjectKind, ProjectStore};
use another_one_core::section::SectionId;

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
    pub(crate) broadcasts:
        HashMap<TerminalRuntimeKey, broadcast::Sender<Vec<u8>>>,
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
    pub(crate) active_viewers:
        HashMap<TerminalRuntimeKey, HashMap<String, (u16, u16)>>,
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
        let (cols, rows) = viewers.values().fold((u16::MAX, u16::MAX), |(c, r), (vc, vr)| {
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

/// `TerminalRegistry` implementation that projects `AnotherOneApp`
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
}

impl TerminalRegistry for DesktopTerminalRegistry {
    fn list_projects(&self) -> Vec<ProjectSummary> {
        self.with_state(|state| project_summaries(state)).unwrap_or_default()
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
        // Clone the writer Arc out of RegistryState *first*, drop
        // the outer state lock, THEN do the blocking PTY write.
        // Holding the state lock across `write_all` + `flush`
        // serialises every daemon task on the tokio worker pool
        // while one mobile keystroke is in flight — if the PTY
        // pipe blocks, the whole daemon stalls.
        let writer = self.with_state(|state| state.writers.get(&key).cloned()).flatten();
        let Some(writer) = writer else { return };
        if let Ok(mut guard) = writer.lock() {
            let _ = guard.write_all(bytes);
            let _ = guard.flush();
        };
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
            state.viewer_focus.insert(viewer_id.to_string(), key.clone());
            state.recompute_effective_size(&key);
        });
    }

    fn viewer_disconnected(&self, viewer_id: &str) {
        self.with_state(|state| {
            let Some(key) = state.viewer_focus.remove(viewer_id) else {
                return;
            };
            let empty = state
                .active_viewers
                .get_mut(&key)
                .map(|map| {
                    map.remove(viewer_id);
                    map.is_empty()
                })
                .unwrap_or(true);
            if empty {
                state.active_viewers.remove(&key);
                state.effective_sizes.remove(&key);
            } else {
                state.recompute_effective_size(&key);
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
            if state
                .pending_tab_launches
                .iter()
                .any(|r| r.key == key)
            {
                return;
            }
            state
                .pending_tab_launches
                .push(TabLaunchRequest { key });
        });
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
                .map(|task| {
                    let section_key = task.section_id.clone();
                    let parsed_section = SectionId::from_store_key(&section_key);
                    let task_pinned =
                        store.ui.pinned_task_ids.contains(&task.id);
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
                    TaskSummary {
                        id: task.id,
                        name: task.name,
                        section_id: section_key,
                        branch_name: task.branch_name,
                        active_tab_id: task.active_tab_id,
                        tabs,
                        pinned: task_pinned,
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
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .thread_name("another-one-daemon-rt")
        .build()
    {
        Ok(rt) => rt,
        Err(e) => {
            let _ = tx.send(Err(anyhow::Error::new(e).context("build daemon tokio runtime")));
            return;
        }
    };

    let weak = Arc::downgrade(&registry_state);
    drop(registry_state); // drop the strong ref we took for spawn; the app still holds one.
    let registry: Arc<dyn TerminalRegistry> = Arc::new(DesktopTerminalRegistry::new(weak));

    let paths = match daemon_paths() {
        Ok(p) => p,
        Err(e) => {
            let _ = tx.send(Err(e));
            return;
        }
    };

    let endpoint_result = runtime.block_on(async {
        daemon_sandbox::run_endpoint(registry, paths.secret_key, paths.paired_peers).await
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
