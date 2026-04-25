//! Desktop-side glue for the embedded iroh daemon host.
//!
//! The headless half of this module — `RegistryState`, the pending-
//! request records, the on-disk path resolver, and the wire-key
//! parser — lives in `another_one_core::daemon_embed` so the future
//! Flutter UI shell can reuse it. This file keeps only the pieces
//! that depend on `daemon-sandbox` types (the wire-summary structs
//! and the `TerminalRegistry` trait) and on the desktop's
//! `mcp_orchestrator` factory:
//!
//! * [`DesktopTerminalRegistry`] — `daemon_sandbox::TerminalRegistry`
//!   impl handed to `run_endpoint`. Holds a `Weak` back to
//!   `RegistryState` so dropping the app still lets the daemon task
//!   unwind cleanly.
//! * [`spawn`] / `run` — dedicated OS thread that runs a
//!   `tokio::runtime::Runtime` and blocks on
//!   `daemon_sandbox::run_endpoint`.
//! * `project_summaries` + the `…ProjectKind` / `…AgentProvider`
//!   mappers — convert headless `core` types into the `daemon_sandbox`
//!   wire types.
//!
//! `daemon-sandbox` already depends on `another-one-core`; both the
//! `TerminalRegistry` impl and `run_endpoint` boot sequence import
//! from `daemon-sandbox`, so they can't move into core without
//! creating a dependency cycle. They stay here until the GPUI app is
//! deleted in Phase 6 of the Flutter migration.

use std::io::Write;
use std::sync::{mpsc, Arc, Mutex, Weak};
use std::thread;

use tokio::sync::broadcast;

use daemon_sandbox::frame::{AgentProvider, ProjectKind, ProjectSummary, TabSummary, TaskSummary};
use daemon_sandbox::{EndpointHandle, TerminalRegistry};

use another_one_core::agents::AgentProviderKind;
use another_one_core::project_store::ProjectKind as CoreProjectKind;
use another_one_core::section::SectionId;

use crate::terminal_runtime::TerminalRuntimeKey;

// Re-export the headless symbols from `another_one_core::daemon_embed`
// so the rest of the desktop crate keeps reaching them through
// `crate::daemon_host::…` paths without a global find-and-replace.
// Phase 6 deletes both this re-export and the entire desktop crate.
pub(crate) use another_one_core::daemon_embed::{
    daemon_paths, key_from_wire, paired_peers_path, RegistryState, TabLaunchRequest,
    TabResizeRequest, DESKTOP_LOCAL_VIEWER_ID,
};

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
        self.with_state(|state| project_summaries(state))
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
    let registry: Arc<dyn TerminalRegistry> = Arc::new(DesktopTerminalRegistry::new(weak.clone()));
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
