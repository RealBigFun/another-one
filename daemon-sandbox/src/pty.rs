//! PTY session plumbing shared by every transport.
//!
//! A [`PtySession`] owns a spawned shell, a writer into its stdin, a handle
//! for resizing, and a Tokio receiver that yields bytes read off the master
//! by a dedicated blocking thread. Transport modules drop a session into a
//! select loop and translate its I/O into their wire format; the PTY code
//! doesn't know or care which transport is on the other side.

use std::io::{Read, Write};

use anyhow::Context;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tokio::sync::mpsc;
use tracing::debug;

/// One live shell attached to a transport.
///
/// Drop order matters on teardown: `child` first (so the shell exits and the
/// master read thread sees EOF), then `master` (closes the PTY), then the
/// writer. The recommended pattern is for callers to `.close()` explicitly.
pub struct PtySession {
    /// Yields bytes streamed from the PTY master. Produced by a blocking
    /// thread; closed when the shell exits or the PTY is dropped.
    pub output_rx: mpsc::Receiver<Vec<u8>>,
    /// Write to this to deliver input into the shell (stdin).
    pub master_writer: Box<dyn Write + Send>,
    /// Held so the PTY stays open and so transports can resize.
    pub master: Box<dyn portable_pty::MasterPty + Send>,
    /// Shell child process. Killed on [`PtySession::close`].
    pub child: Box<dyn portable_pty::Child + Send + Sync>,
    /// OS process id for resource tracking, when the PTY backend
    /// exposes one.
    pub process_id: Option<u32>,
}

impl PtySession {
    /// Opens a PTY and spawns a command with `TERM=xterm-256color` and
    /// `cwd=$HOME`. Command resolution order:
    ///   1. `AGENT_CMD` env var (e.g. `AGENT_CMD=claude`) — lets you point the
    ///      sandbox at an agent CLI without overloading `$SHELL`.
    ///   2. `$SHELL` — your login shell.
    ///   3. `bash` — hard fallback.
    pub fn spawn(cols: u16, rows: u16) -> anyhow::Result<Self> {
        let pty_system = native_pty_system();
        let pair = pty_system
            .openpty(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;

        let shell = std::env::var("AGENT_CMD")
            .or_else(|_| std::env::var("SHELL"))
            .unwrap_or_else(|_| "bash".to_string());
        let mut cmd = CommandBuilder::new(&shell);
        cmd.env("TERM", "xterm-256color");
        if let Ok(home) = std::env::var("HOME") {
            cmd.cwd(home);
        }

        let child = pair.slave.spawn_command(cmd).context("spawn shell")?;
        let process_id = child.process_id();
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().context("clone reader")?;
        let writer = pair.master.take_writer().context("take writer")?;

        let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) => {
                        debug!("pty eof");
                        break;
                    }
                    Ok(n) => {
                        if tx.blocking_send(buf[..n].to_vec()).is_err() {
                            debug!("pty→transport channel closed");
                            break;
                        }
                    }
                    Err(e) => {
                        debug!(error = %e, "pty read error");
                        break;
                    }
                }
            }
        });

        Ok(Self {
            output_rx: rx,
            master_writer: writer,
            master: pair.master,
            child,
            process_id,
        })
    }

    /// Applies a new grid size to the PTY. Returns an error if the resize
    /// syscall fails; transports should log and continue, not abort.
    pub fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.master
            .resize(PtySize {
                cols,
                rows,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize")
    }

    /// Writes input bytes to the shell's stdin.
    pub fn write_input(&mut self, bytes: &[u8]) -> std::io::Result<()> {
        self.master_writer.write_all(bytes)?;
        self.master_writer.flush()
    }

    /// Terminates the shell and releases the PTY. Safe to call multiple times
    /// (the child's `kill`/`wait` are idempotent enough for our purposes).
    pub fn close(mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        drop(self.master);
    }
}
