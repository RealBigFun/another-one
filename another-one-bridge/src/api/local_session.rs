//! In-process FFI session for the future Flutter desktop client.
//!
//! Mirror of [`super::iroh_client::IrohSession`] but with no network
//! transport — the desktop binary hosts its own daemon
//! (`core::daemon_embed::RegistryState`), so for local-host
//! operations there's no need to round-trip through QUIC. The Dart
//! `LocalConnection` (a future implementor of `DaemonConnection`)
//! will hold a `LocalSession` and call its methods directly.
//!
//! This commit wires the worker-replies stream end-to-end and a
//! synthetic `list_projects` so Dart consumers can validate the
//! round-trip. The other methods are stubs returning
//! "unimplemented" errors — they get wired one at a time as Phase 2
//! work progresses, with each commit hooking one verb into the
//! shared `RegistryState` (kept in `core::daemon_embed`).

use std::sync::Mutex;

use flutter_rust_bridge::frb;
use tokio::sync::mpsc;

use super::iroh_client::{tokio_rt, ProjectSummary, WorkerReply};
use crate::frb_generated::StreamSink;

/// Opaque handle to an in-process daemon session. Dart holds it and
/// calls methods; Rust will eventually proxy those calls to a
/// shared `RegistryState`. Today the worker-replies channel is real
/// and `list_projects` pushes a synthetic empty list through it; the
/// rest are stubs.
#[frb(opaque)]
pub struct LocalSession {
    /// Producer side of the worker-replies stream. Cloned into
    /// every method that wants to push a reply (today: just
    /// `list_projects`). Dropped on `close`.
    worker_replies_tx: Mutex<Option<mpsc::UnboundedSender<WorkerReply>>>,
    /// Receiver kept until [`Self::subscribe_worker_replies`] takes
    /// it; one-shot subscription, same shape as `IrohSession`.
    worker_replies_rx: Mutex<Option<mpsc::UnboundedReceiver<WorkerReply>>>,
}

/// Construct a session bound to the desktop's in-process daemon.
///
/// Today this allocates the worker-replies channel and returns a
/// session whose data-streaming methods are stubs. The eventual
/// implementation will look up the active `RegistryState`
/// (initialized when the desktop binary boots and calls
/// `daemon_embed::run` on its dedicated thread) and clone an `Arc`
/// of it into the session for the read methods to consult.
pub async fn local_connect() -> anyhow::Result<LocalSession> {
    let (tx, rx) = mpsc::unbounded_channel();
    Ok(LocalSession {
        worker_replies_tx: Mutex::new(Some(tx)),
        worker_replies_rx: Mutex::new(Some(rx)),
    })
}

impl LocalSession {
    /// Send raw PTY stdin bytes to the currently-attached tab.
    pub async fn send(&self, _bytes: Vec<u8>) -> anyhow::Result<()> {
        Err(unimplemented_err("send"))
    }

    /// Resize the currently-attached tab's PTY.
    pub async fn tab_resize(&self, _cols: u16, _rows: u16) -> anyhow::Result<()> {
        Err(unimplemented_err("tab_resize"))
    }

    /// Push a project list through [`Self::subscribe_worker_replies`].
    ///
    /// Today the list is a synthetic empty `Vec<ProjectSummary>` —
    /// just enough for Dart-side consumers to validate the
    /// round-trip end-to-end. The real implementation reads from
    /// `core::daemon_embed::RegistryState::project_store` and
    /// flattens it into `ProjectSummary` / `TaskSummary` /
    /// `TabSummary` exactly the way the iroh side does.
    pub async fn list_projects(&self) -> anyhow::Result<()> {
        let tx = {
            let guard = self.worker_replies_tx.lock().expect("worker_replies_tx mutex poisoned");
            guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("session closed"))?
                .clone()
        };
        tx.send(WorkerReply::ProjectList {
            projects: synthetic_project_list(),
        })
        .map_err(|_| anyhow::anyhow!("worker-replies receiver dropped"))?;
        Ok(())
    }

    /// Subscribe to live PTY bytes for a specific tab. At most one
    /// attachment per session.
    pub async fn attach_tab(
        &self,
        _section_id: String,
        _tab_id: String,
    ) -> anyhow::Result<()> {
        Err(unimplemented_err("attach_tab"))
    }

    /// Stop forwarding PTY bytes for the currently-attached tab.
    pub async fn detach_tab(&self) -> anyhow::Result<()> {
        Err(unimplemented_err("detach_tab"))
    }

    /// Ask the daemon to spawn the given tab's PTY if it isn't
    /// already running.
    pub async fn launch_tab(
        &self,
        _section_id: String,
        _tab_id: String,
    ) -> anyhow::Result<()> {
        Err(unimplemented_err("launch_tab"))
    }

    /// Stream PTY bytes for the attached tab into a Dart sink.
    pub async fn subscribe(&self, _sink: StreamSink<Vec<u8>>) -> anyhow::Result<()> {
        Err(unimplemented_err("subscribe"))
    }

    /// Stream worker replies (project list, future: git refresh,
    /// MCP tool results) into a Dart sink.
    ///
    /// One-shot: the second call returns an "already subscribed"
    /// error. Replies arrive in the order they were pushed by
    /// methods like [`Self::list_projects`].
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

    /// Close the session. Drops the worker-replies sender (so any
    /// active subscription's forwarder loop exits) and is
    /// idempotent on subsequent calls.
    pub async fn close(&self) {
        if let Ok(mut guard) = self.worker_replies_tx.lock() {
            guard.take();
        }
    }
}

fn unimplemented_err(method: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "LocalSession::{method} is not yet implemented; tracking issue: \
         wire to core::daemon_embed::RegistryState in a follow-up commit"
    )
}

/// Placeholder project list used by [`LocalSession::list_projects`]
/// until `RegistryState` plumbing lands. Returns an empty `Vec` —
/// Dart consumers can already test their wiring (subscribe →
/// receive `WorkerReply::ProjectList { projects: [] }` → render
/// "no projects yet" empty state).
fn synthetic_project_list() -> Vec<ProjectSummary> {
    Vec::new()
}
