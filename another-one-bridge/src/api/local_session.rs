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
