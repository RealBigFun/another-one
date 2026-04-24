//! Unix domain socket listener for local MCP clients.
//!
//! The daemon exposes itself as an MCP server over a UDS so
//! harnesses (Claude Code, Cursor, Codex, etc.) can list tasks,
//! read terminal output, and spawn new tasks without going
//! through iroh/mobile pairing. The `another-one-mcp-shim`
//! stdio binary forwards between a harness's stdin/stdout and
//! this socket.
//!
//! Each connection gets its own tokio task that wraps the UDS
//! stream in an `McpOrchestrator` session via
//! `another_one_core::mcp::server::serve`. The server side is
//! sync (line-oriented I/O on std streams) — we enter it via
//! `spawn_blocking` so the reactor isn't held during long tool
//! calls like `run_command`.
//!
//! ### Windows
//!
//! Named-pipe transport is stubbed out and returns an error on
//! Windows for now; mobile is the priority platform for the
//! daemon and desktop targets today are mac+linux per the
//! project's AGENTS.md. Named-pipe support is a follow-up.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tokio::task::AbortHandle;

use another_one_core::mcp::orchestrator::McpOrchestrator;

/// Handle the caller holds to keep the MCP listener alive. Drop
/// to abort the accept loop and remove the socket file.
pub struct McpListener {
    pub socket_path: PathBuf,
    abort: Option<AbortHandle>,
}

impl Drop for McpListener {
    fn drop(&mut self) {
        if let Some(h) = self.abort.take() {
            h.abort();
        }
        // Best-effort cleanup of the socket file. If another
        // process now owns it, unlinking is still OK — UDS
        // unlink doesn't affect established connections.
        let _ = std::fs::remove_file(&self.socket_path);
    }
}

/// Bind a UDS at `socket_path`, accept connections, and run each
/// through the MCP session loop backed by `orchestrator`.
///
/// The returned handle should be kept alive for the duration of
/// the daemon's run — dropping it shuts the listener down and
/// unlinks the socket file. Missing parent directories are
/// created; a pre-existing socket at the path is unlinked first
/// (stale sockets from a crashed prior run).
#[cfg(unix)]
pub fn spawn(
    socket_path: PathBuf,
    orchestrator: Arc<dyn McpOrchestrator>,
) -> anyhow::Result<McpListener> {
    use tokio::net::UnixListener;

    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    // Unlink any stale socket left over from a prior crash.
    let _ = std::fs::remove_file(&socket_path);

    let listener = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind MCP socket at {}", socket_path.display()))?;
    restrict_socket_mode(&socket_path)?;

    tracing::info!(path = %socket_path.display(), "mcp: listening on UDS");
    let accept_task = tokio::spawn(accept_loop(listener, orchestrator));

    Ok(McpListener {
        socket_path,
        abort: Some(accept_task.abort_handle()),
    })
}

#[cfg(not(unix))]
pub fn spawn(
    _socket_path: PathBuf,
    _orchestrator: Arc<dyn McpOrchestrator>,
) -> anyhow::Result<McpListener> {
    Err(anyhow::anyhow!(
        "local MCP transport is not yet implemented on non-unix platforms"
    ))
}

#[cfg(unix)]
async fn accept_loop(
    listener: tokio::net::UnixListener,
    orchestrator: Arc<dyn McpOrchestrator>,
) {
    loop {
        match listener.accept().await {
            Ok((stream, _addr)) => {
                let orch = orchestrator.clone();
                tokio::spawn(handle_connection(stream, orch));
            }
            Err(err) => {
                tracing::warn!(?err, "mcp: accept failed; continuing");
                // Accept errors on a UDS listener are unusual;
                // back off briefly before retrying so we don't
                // spin in a loop if the socket got into a bad
                // state.
                tokio::time::sleep(std::time::Duration::from_millis(200)).await;
            }
        }
    }
}

#[cfg(unix)]
async fn handle_connection(stream: tokio::net::UnixStream, orchestrator: Arc<dyn McpOrchestrator>) {
    // Split into owned read + write halves, then hand to a
    // blocking task that runs the sync `serve` loop.
    let std_stream = match stream.into_std() {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!(?err, "mcp: failed to convert stream to std");
            return;
        }
    };
    if let Err(err) = std_stream.set_nonblocking(false) {
        tracing::warn!(?err, "mcp: set_nonblocking(false) failed");
        return;
    }
    let reader_stream = match std_stream.try_clone() {
        Ok(s) => s,
        Err(err) => {
            tracing::warn!(?err, "mcp: failed to clone UDS stream");
            return;
        }
    };

    let join = tokio::task::spawn_blocking(move || {
        another_one_core::mcp::server::serve(reader_stream, std_stream, orchestrator)
    });
    match join.await {
        Ok(Ok(())) => tracing::debug!("mcp: session ended cleanly"),
        Ok(Err(err)) => tracing::warn!(?err, "mcp: session ended with I/O error"),
        Err(err) => tracing::warn!(?err, "mcp: serve task join failed"),
    }
}

/// Lock the socket down to owner-only access. On unix the mode
/// check happens at connect time for UDS (connecting peer must
/// have permission to the socket file), so 0600 is the right
/// default for a per-user local endpoint.
#[cfg(unix)]
fn restrict_socket_mode(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let perms = std::fs::Permissions::from_mode(0o600);
    std::fs::set_permissions(path, perms)
        .with_context(|| format!("chmod 0600 on {} failed", path.display()))
}

/// Return the default per-user socket path. Lives under
/// `$XDG_RUNTIME_DIR/another-one/mcp.sock` when that's set
/// (Linux); falls back to `${TMPDIR:-/tmp}/another-one-mcp-$UID.sock`
/// otherwise. macOS ships no `$XDG_RUNTIME_DIR` so the fallback is
/// the common case there.
pub fn default_socket_path() -> PathBuf {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("another-one").join("mcp.sock");
    }
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    // Namespace per-user in `/tmp` so two logins on the same box
    // don't collide on the same socket. `$USER` is set in every
    // POSIX login shell; fall back to "anon" if an embedder is
    // running with an unusual environment.
    let user = std::env::var("USER").unwrap_or_else(|_| "anon".into());
    tmp.join(format!("another-one-mcp-{user}.sock"))
}
