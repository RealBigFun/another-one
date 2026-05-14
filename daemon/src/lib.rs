//! Daemon library. The desktop app links this to host an iroh
//! endpoint in-process; the `daemon-sandbox` binary in the same
//! crate links it too, and wraps it with a standalone
//! PTY-shell-per-connection behavior for smoke testing.
//!
//! Public entry point for embedders: [`run_endpoint`]. The caller
//! passes a [`DaemonRegistry`] trait object describing how to
//! enumerate projects/tasks/tabs and attach to live PTYs. The
//! library constructs an iroh `Endpoint`, performs TOFU pairing,
//! and dispatches incoming control frames to the registry.
//!
//! The library deliberately avoids spawning its own PTYs — that's
//! the sandbox binary's concern. When embedded in the desktop app,
//! the existing terminal runtime is the source of truth; the daemon
//! is a pure transport + routing layer.

use std::path::PathBuf;
use std::sync::Arc;

pub(crate) mod commands;
pub mod dispatch;
pub mod frame;
pub mod registry;
pub mod terminal;
pub mod transport_iroh;
pub mod transport_mcp;

pub use registry::{DaemonRegistry, EndpointHandle};
pub use transport_iroh::persist_pairing;

// These two are the sandbox binary's own helpers; re-exported so the
// binary can call them via `daemon::sandbox::*` without
// having to use `crate::`.
pub mod pty;
pub mod sandbox;
pub mod transport_ws;

/// Start an iroh endpoint backed by `registry`. Blocks until
/// shutdown is signalled via the returned handle (dropped or
/// `.abort()` called). Spawns its own tokio tasks on the ambient
/// runtime; the caller must ensure a tokio runtime is current.
///
/// Paths are where the daemon persists its iroh secret key
/// (stable EndpointId across restarts) and TOFU allowlist. The
/// desktop passes paths under the app's data dir; the sandbox
/// binary passes `~/.local/share/another-one-sandbox/…`.
pub async fn run_endpoint(
    registry: Arc<dyn DaemonRegistry>,
    secret_key_path: PathBuf,
    paired_peers_path: PathBuf,
) -> anyhow::Result<EndpointHandle> {
    transport_iroh::run_embedded(registry, secret_key_path, paired_peers_path).await
}
