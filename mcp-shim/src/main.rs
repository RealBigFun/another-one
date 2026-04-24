//! `another-one-mcp-shim` — stdio ↔ UDS forwarder.
//!
//! MCP clients (Claude Code, Cursor, Codex, Gemini, etc.) launch
//! their configured stdio servers as subprocesses and talk to
//! them over stdin/stdout. The AnotherOne daemon runs in-process
//! inside the desktop app and listens on a Unix domain socket;
//! this shim is what the harness actually spawns. It:
//!
//!   1. Figures out the socket path (argv[1] or `ANOTHERONE_MCP_SOCKET`
//!      env var, falling back to the standard per-user default).
//!   2. Connects to the UDS.
//!   3. Copies stdin → socket and socket → stdout on two threads.
//!   4. Exits cleanly when either side closes.
//!
//! Deliberately small: no tokio, no MCP parsing. Just bytes. That
//! keeps cold-start time low (the shim runs once per MCP session
//! the harness opens) and makes the shim itself audit-trivial.

use std::io::{self, Read, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::thread;

const EXIT_USAGE: u8 = 64; // EX_USAGE
const EXIT_UNAVAILABLE: u8 = 69; // EX_UNAVAILABLE
const EXIT_IO: u8 = 74; // EX_IOERR

fn main() -> ExitCode {
    let socket_path = match resolve_socket_path() {
        Ok(p) => p,
        Err(msg) => {
            eprintln!("{msg}");
            return ExitCode::from(EXIT_USAGE);
        }
    };

    match run(&socket_path) {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("another-one-mcp-shim: {err}");
            match err.kind() {
                io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused => {
                    ExitCode::from(EXIT_UNAVAILABLE)
                }
                _ => ExitCode::from(EXIT_IO),
            }
        }
    }
}

fn run(socket_path: &Path) -> io::Result<()> {
    use std::net::Shutdown;

    let stream = UnixStream::connect(socket_path)?;
    let stream_read = stream.try_clone()?;
    let stream_write_handle = stream.try_clone()?;
    let stream_write = stream;

    // stdin → socket on a worker thread.
    let _stdin_thread = thread::spawn(move || copy_until_eof(io::stdin().lock(), stream_write));

    // socket → stdout on the main thread so main exits as soon
    // as the server closes.
    let stdout = io::stdout();
    copy_until_eof(stream_read, stdout.lock())?;

    // Server closed. Shut down the write half so the stdin→socket
    // thread's next write returns `BrokenPipe` and the thread
    // exits. If we `join()`ed here instead, and the harness
    // hadn't closed stdin yet, the thread would be blocked on
    // `stdin().read` forever and the shim process would hang —
    // leaving the harness waiting on our exit before it can reap
    // the MCP session.
    let _ = stream_write_handle.shutdown(Shutdown::Write);
    // Don't join: std::io::stdin has no portable "close" API we
    // can pull to wake the read; dropping the thread handle here
    // detaches it, and std::process::exit below reaps the whole
    // process.
    std::process::exit(0);
}

fn copy_until_eof<R: Read, W: Write>(mut src: R, mut dst: W) -> io::Result<()> {
    let mut buf = [0u8; 8192];
    loop {
        let n = match src.read(&mut buf) {
            Ok(0) => return Ok(()),
            Ok(n) => n,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e),
        };
        dst.write_all(&buf[..n])?;
        dst.flush()?;
    }
}

/// Resolution order:
///   1. argv[1], if present.
///   2. `ANOTHERONE_MCP_SOCKET` env var.
///   3. Per-user default matching `daemon_sandbox::transport_mcp::default_socket_path`.
fn resolve_socket_path() -> Result<PathBuf, String> {
    if let Some(arg) = std::env::args().nth(1) {
        return Ok(PathBuf::from(arg));
    }
    if let Some(env) = std::env::var_os("ANOTHERONE_MCP_SOCKET") {
        return Ok(PathBuf::from(env));
    }
    Ok(default_socket_path())
}

// Mirror of `daemon_sandbox::transport_mcp::default_socket_path`,
// duplicated here so the shim has no dep on daemon-sandbox (keeps
// the shim binary tiny + cold-start-fast). Any change to the
// default path needs to be made in both places — the daemon-side
// version is authoritative; this is a copy for a process that
// intentionally can't link the daemon crate.
fn default_socket_path() -> PathBuf {
    if let Some(runtime) = std::env::var_os("XDG_RUNTIME_DIR") {
        return PathBuf::from(runtime).join("another-one").join("mcp.sock");
    }
    let tmp = std::env::var_os("TMPDIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    // SAFETY: getuid is always safe on unix. On platforms
    // without libc we fall back to 0 — the non-unix shim path
    // doesn't hit UDS anyway.
    let uid = {
        #[cfg(unix)]
        unsafe {
            extern "C" {
                fn getuid() -> u32;
            }
            getuid()
        }
        #[cfg(not(unix))]
        0
    };
    tmp.join(format!("another-one-mcp-{uid}")).join("mcp.sock")
}
