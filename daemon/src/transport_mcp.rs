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
use std::sync::{Arc, Mutex, OnceLock};

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
/// unlinks the socket file.
///
/// ## Security-relevant behaviour
///
/// - The parent directory is ensured `chmod 0700` so even a brief
///   window where the socket file has permissive mode is
///   unreachable to other local users.
/// - We set a tight `umask(0o177)` before `bind(2)` so the socket
///   file is created `0600` from the start, closing the TOCTOU
///   window between bind and a post-hoc chmod.
/// - Before unlinking any pre-existing file at `socket_path` we
///   `lstat(2)` it and confirm it's a socket owned by our uid.
///   A symlink, regular file, or socket owned by somebody else
///   is left alone and causes bind to fail — rather than letting
///   a race or a hostile pre-squat at a predictable path give us
///   a wrong-owner accept loop.
/// - On concurrent AnotherOne startup, a live sibling socket is
///   detected by a probe `connect(2)`; we refuse to clobber it.
#[cfg(unix)]
pub fn spawn(
    socket_path: PathBuf,
    orchestrator: Arc<dyn McpOrchestrator>,
) -> anyhow::Result<McpListener> {
    use tokio::net::UnixListener;

    if let Some(parent) = socket_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
        // `chmod 0700` on the directory. Safe to apply on every
        // startup: if the parent already existed with tighter
        // perms, 0700 is the same or looser; if it was looser,
        // we tighten.
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(parent, std::fs::Permissions::from_mode(0o700));
    }

    unlink_if_ours_and_dead(&socket_path)?;

    // Tight umask so the bound socket file is 0600 from the
    // moment it exists on the filesystem — closes the TOCTOU
    // window between bind(2) and a post-hoc chmod.
    let prev_umask = set_umask(0o177);
    let listener_result = UnixListener::bind(&socket_path)
        .with_context(|| format!("failed to bind MCP socket at {}", socket_path.display()));
    set_umask(prev_umask);
    let listener = listener_result?;

    tracing::info!(path = %socket_path.display(), "mcp: listening on UDS");
    let accept_task = tokio::spawn(accept_loop(listener, orchestrator));

    // Cleanup wiring so the user never sees a stale socket file:
    // - Drop on `McpListener` (graceful shutdown) — already covered.
    // - Panic hook (any thread panics) — installed once.
    // - SIGINT / SIGTERM / SIGHUP — caught and handled here so a
    //   plain `Ctrl-C` or window-manager close also unlinks before
    //   the process exits.
    install_cleanup_hooks(socket_path.clone());

    Ok(McpListener {
        socket_path,
        abort: Some(accept_task.abort_handle()),
    })
}

/// Tracks the active MCP socket path so the panic hook + signal
/// handler know what to unlink. `OnceLock<Mutex<Option<…>>>` gives
/// us idempotent install + late-arriving updates if the socket path
/// ever rotates within a single process.
static CLEANUP_PATH: OnceLock<Mutex<Option<PathBuf>>> = OnceLock::new();
static CLEANUP_HOOKS_INSTALLED: OnceLock<()> = OnceLock::new();

#[cfg(unix)]
fn install_cleanup_hooks(path: PathBuf) {
    let slot = CLEANUP_PATH.get_or_init(|| Mutex::new(None));
    if let Ok(mut guard) = slot.lock() {
        *guard = Some(path.clone());
    }

    // Panic hook + signal handler are installed exactly once per
    // process. Repeat calls (e.g. socket rotation) just refresh the
    // path stored above.
    CLEANUP_HOOKS_INSTALLED.get_or_init(|| {
        // Chain on top of whatever hook is currently installed
        // (likely the default backtrace one) so panics still print
        // their usual diagnostics — we just unlink first.
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            unlink_cleanup_path();
            prev(info);
        }));

        // Signal handler runs on the daemon's tokio runtime — that
        // runtime is alive for the entire process lifetime.
        tokio::spawn(async {
            use tokio::signal::unix::{signal, SignalKind};
            let mut sigterm = match signal(SignalKind::terminate()) {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(?err, "mcp: SIGTERM handler unavailable");
                    return;
                }
            };
            let mut sigint = match signal(SignalKind::interrupt()) {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(?err, "mcp: SIGINT handler unavailable");
                    return;
                }
            };
            let mut sighup = match signal(SignalKind::hangup()) {
                Ok(s) => s,
                Err(err) => {
                    tracing::warn!(?err, "mcp: SIGHUP handler unavailable");
                    return;
                }
            };
            let signame = tokio::select! {
                _ = sigterm.recv() => "SIGTERM",
                _ = sigint.recv() => "SIGINT",
                _ = sighup.recv() => "SIGHUP",
            };
            tracing::info!(signal = signame, "mcp: caught termination signal — unlinking socket");
            unlink_cleanup_path();
            // Exit code 130 is the conventional "script terminated
            // by Ctrl+C" code. Plain 0 would mask whether the
            // process exited cleanly vs got signaled.
            std::process::exit(if signame == "SIGINT" { 130 } else { 0 });
        });
    });
}

#[cfg(not(unix))]
fn install_cleanup_hooks(_path: PathBuf) {}

fn unlink_cleanup_path() {
    let Some(slot) = CLEANUP_PATH.get() else {
        return;
    };
    let Ok(guard) = slot.lock() else {
        return;
    };
    let Some(path) = guard.as_ref() else {
        return;
    };
    let _ = std::fs::remove_file(path);
}

/// Only remove `path` if it exists, is a socket, and is owned by
/// our euid. If it exists *and* looks live (connect probe
/// succeeds), bail so a concurrent AnotherOne instance can keep
/// serving.
#[cfg(unix)]
fn unlink_if_ours_and_dead(path: &Path) -> anyhow::Result<()> {
    use std::os::unix::fs::{FileTypeExt, MetadataExt};

    let meta = match std::fs::symlink_metadata(path) {
        Ok(m) => m,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(err) => {
            return Err(anyhow::Error::from(err).context(format!(
                "failed to stat existing {} before bind",
                path.display()
            )))
        }
    };
    let ft = meta.file_type();
    if !ft.is_socket() {
        anyhow::bail!(
            "refusing to unlink non-socket at {} — inspect before retrying",
            path.display()
        );
    }
    // SAFETY: geteuid is always safe on unix.
    let our_uid = unsafe { libc::geteuid() };
    if meta.uid() != our_uid {
        anyhow::bail!(
            "refusing to unlink socket at {} owned by uid {} (ours is {})",
            path.display(),
            meta.uid(),
            our_uid,
        );
    }
    // Probe: if something is listening, another AnotherOne
    // instance is alive. Don't rug-pull it.
    if let Ok(stream) = std::os::unix::net::UnixStream::connect(path) {
        drop(stream);
        anyhow::bail!("another MCP listener is already serving {}", path.display());
    }
    // Dead socket: unlink.
    std::fs::remove_file(path)
        .with_context(|| format!("failed to unlink stale socket at {}", path.display()))?;
    Ok(())
}

#[cfg(unix)]
fn set_umask(mask: u32) -> u32 {
    // SAFETY: umask(2) takes and returns the prior mask; always safe.
    unsafe { libc::umask(mask as libc::mode_t) as u32 }
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
async fn accept_loop(listener: tokio::net::UnixListener, orchestrator: Arc<dyn McpOrchestrator>) {
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

/// Return the default per-user socket path. Lives under
/// `$XDG_RUNTIME_DIR/another-one/mcp.sock` when that's set
/// (Linux); falls back to `${TMPDIR:-/tmp}/another-one-mcp-<uid>/mcp.sock`
/// otherwise. Keyed on effective UID (not `$USER`) so a hostile
/// `USER` environment can't collide with another user's socket.
/// The parent directory is chmod 0700 by `spawn()`.
pub fn default_socket_path() -> PathBuf {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("another-one").join("mcp.sock");
    }
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    #[cfg(unix)]
    // SAFETY: geteuid is always safe on unix.
    let uid = unsafe { libc::geteuid() };
    #[cfg(not(unix))]
    let uid: u32 = 0;
    tmp.join(format!("another-one-mcp-{uid}")).join("mcp.sock")
}
