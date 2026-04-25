//! In-process FFI session for the future Flutter desktop client.
//!
//! Mirror of [`super::iroh_client::IrohSession`] but with no network
//! transport — the desktop binary hosts its own daemon
//! (`core::daemon_embed::RegistryState`), so for local-host
//! operations there's no need to round-trip through QUIC. The Dart
//! `LocalConnection` (a future implementor of `DaemonConnection`)
//! will hold a `LocalSession` and call its methods directly.
//!
//! This commit ships the API surface as stubs only — every method
//! returns an `unimplemented` error so the FRB-generated Dart
//! bindings have something to bind against. Subsequent commits wire
//! each method to `RegistryState` (project-list reads, terminal
//! launch/attach, PTY stdin send, worker-reply broadcasts).
//!
//! Why surface-only first: the alternative — landing one FFI verb
//! at a time fully wired — requires plumbing the daemon's
//! `RegistryState` into the bridge crate before anything can work,
//! which means a bigger first PR. Stub-first lets the Dart side
//! migrate to a `DaemonConnection`-shaped consumer without waiting
//! for the Rust plumbing, and the stubs fail loudly enough at
//! runtime that callers know they're not ready yet.

use std::sync::Mutex;

use flutter_rust_bridge::frb;

use super::iroh_client::WorkerReply;
use crate::frb_generated::StreamSink;

/// Opaque handle to an in-process daemon session. Dart holds it and
/// calls methods; Rust will eventually proxy those calls to a
/// shared `RegistryState`. Today every method returns an error.
#[frb(opaque)]
pub struct LocalSession {
    /// One-shot guard so [`Self::close`] is idempotent. Subsequent
    /// commits will hold the actual daemon-handle here.
    _closed: Mutex<bool>,
}

/// Construct a session bound to the desktop's in-process daemon.
///
/// Today this just allocates the handle — the eventual
/// implementation will look up the active `RegistryState`
/// (initialized when the desktop binary boots and calls
/// `daemon_embed::run` on its dedicated thread) and clone an
/// `Arc` of it into the session.
pub async fn local_connect() -> anyhow::Result<LocalSession> {
    Ok(LocalSession {
        _closed: Mutex::new(false),
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

    /// Ask the daemon to send its full project tree as a
    /// `WorkerReply::ProjectList`. The reply arrives via
    /// [`Self::subscribe_worker_replies`].
    pub async fn list_projects(&self) -> anyhow::Result<()> {
        Err(unimplemented_err("list_projects"))
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
    pub async fn subscribe_worker_replies(
        &self,
        _sink: StreamSink<WorkerReply>,
    ) -> anyhow::Result<()> {
        Err(unimplemented_err("subscribe_worker_replies"))
    }

    /// Close the session. Idempotent.
    pub async fn close(&self) {
        if let Ok(mut closed) = self._closed.lock() {
            *closed = true;
        }
    }
}

fn unimplemented_err(method: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "LocalSession::{method} is not yet implemented; tracking issue: \
         wire to core::daemon_embed::RegistryState in a follow-up PR"
    )
}
