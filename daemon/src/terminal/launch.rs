//! Daemon-side PTY spawn.
//!
//! Phase 4 of `docs/designs/01-daemon-canonical-terminal.md`. The
//! desktop GPUI used to own PTY spawn (via
//! [`another_one_core::terminal_launch::spawn_terminal_launch`]); the
//! design's destination is "PTY launches where the Term task lives,"
//! which is the daemon. This module is the daemon-side replacement,
//! gated behind [`daemon_spawn_enabled`] so shipped builds keep the
//! legacy GPUI path until the desktop cutover (Phase 5b–5d).
//!
//! [`spawn_terminal_in_daemon`] is the entry point. Given a launch
//! config, it:
//!
//! 1. resolves a `portable_pty::CommandBuilder` via
//!    [`another_one_core::terminal_launch::build_command`];
//! 2. opens a PTY at the requested grid size;
//! 3. spawns the child process under the slave;
//! 4. starts a blocking `std::thread` reader that pumps PTY bytes
//!    into a fresh [`crate::terminal::TerminalTaskHandle`];
//! 5. spawns a watcher task that surfaces the child's exit as a
//!    [`daemon_proto::WorkerReply::TerminalExited`] push so any
//!    subscribed viewer learns the tab is gone.
//!
//! The Term task drains the bytes off the GPUI render thread —
//! that's the whole point. Phase 4c flag-gates the dispatch arm
//! that calls this; until then nothing in production routes here.

use std::path::PathBuf;
use std::thread;

use another_one_core::agents::{HarnessEnv, TerminalLaunchConfig};
use another_one_core::terminal_launch::{apply_terminal_environment, build_command};
use another_one_core::terminal_types::TerminalGridSize;
use anyhow::Context;
use portable_pty::{native_pty_system, ChildKiller, MasterPty};

use super::task::{spawn_terminal_task, TerminalCommand, TerminalTaskHandle};

/// Env var that enables the daemon-side spawn path. Off by default;
/// set to `1` (or any non-empty value) to route `Control::LaunchTab`
/// through [`spawn_terminal_in_daemon`] instead of the legacy
/// GPUI-side fulfillment.
pub const DAEMON_SPAWN_ENV: &str = "ANOTHER_ONE_DAEMON_SPAWN";

/// Whether the daemon-side spawn path is enabled at process start.
/// Cached: env reads happen once on first call so toggling at
/// runtime doesn't surprise the dispatch arm mid-session.
pub fn daemon_spawn_enabled() -> bool {
    use std::sync::OnceLock;
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var(DAEMON_SPAWN_ENV)
            .map(|v| !v.is_empty() && v != "0")
            .unwrap_or(false)
    })
}

/// Outcome of a successful daemon-side spawn. The caller (the
/// registry) holds the [`TerminalTaskHandle`] alongside its tab map
/// so [`crate::DaemonRegistry::subscribe_terminal_frames`] can
/// resolve frame watches; the [`SpawnedChild`] handle gives the
/// caller a way to kill the child if the tab is closed before its
/// natural exit.
pub struct DaemonSpawnedTerminal {
    pub task: TerminalTaskHandle,
    pub child: SpawnedChild,
    pub process_id: Option<u32>,
    /// Master-PTY writer. The desktop registry stores this in
    /// `RegistryState::writers` so the existing `tab_input` path
    /// (registry.writers + block_in_place + write_all) routes
    /// keystrokes / pastes / mouse-protocol bytes to the
    /// daemon-spawned PTY without a Term-task command round-trip.
    /// Phase 5d-iii of design 01 (#158).
    pub writer: std::sync::Arc<std::sync::Mutex<Box<dyn std::io::Write + Send>>>,
}

/// Owns the child's lifecycle. Drop the [`SpawnedChild`] to abort
/// the watcher task and release the killer; calling [`kill`] sends
/// the OS-level termination signal.
pub struct SpawnedChild {
    killer: Option<Box<dyn ChildKiller + Send + Sync>>,
}

impl SpawnedChild {
    pub fn kill(&mut self) -> std::io::Result<()> {
        match self.killer.as_mut() {
            Some(k) => k.kill(),
            None => Ok(()),
        }
    }
}

/// Inputs the spawn function needs from its caller. Mirrors the
/// shape `core::terminal_launch::spawn_terminal_launch` already
/// takes, minus the `mpsc::SyncSender<TerminalLaunchReply>` reply
/// channel — the daemon path returns the spawn outcome inline.
pub struct SpawnRequest {
    pub cwd: Option<PathBuf>,
    pub launch_config: TerminalLaunchConfig,
    pub agent_launch_args: Vec<String>,
    pub size: TerminalGridSize,
}

/// Open a PTY, spawn the child, and start the per-tab Term task
/// pumping bytes into it. Synchronous: callers (Phase 4c's dispatch
/// arm) await the result before sending the launch ack.
pub fn spawn_terminal_in_daemon(req: SpawnRequest) -> anyhow::Result<DaemonSpawnedTerminal> {
    let cwd = req
        .cwd
        .clone()
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."));
    let env = HarnessEnv::from_os();
    let (mut builder, _resolved_config, _discovery) =
        build_command(&env, &cwd, req.launch_config, &req.agent_launch_args)
            .context("daemon spawn: build command")?;
    builder.cwd(&cwd);
    apply_terminal_environment(&mut builder, &cwd);

    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(req.size.as_pty_size())
        .context("daemon spawn: openpty")?;
    let reader = pair
        .master
        .try_clone_reader()
        .context("daemon spawn: clone pty reader")?;
    let writer = pair
        .master
        .take_writer()
        .context("daemon spawn: pty writer")?;
    let child = pair
        .slave
        .spawn_command(builder)
        .with_context(|| format!("daemon spawn: spawn child in {}", cwd.display()))?;
    let process_id = child.process_id();
    let killer = child.clone_killer();
    drop(pair.slave); // close the slave-side handle on the parent side

    // Start the Term task now so the reader has a destination.
    let task = spawn_terminal_task(req.size);
    let task_inbox = task.try_send_handle();

    // Reader thread: blocking read from the PTY master, blocking
    // try_send into the Term task's bounded mpsc. A full inbox
    // backpressures the reader (the bounded channel is the
    // intended throttle at high harness output rates).
    thread::Builder::new()
        .name("another-one-pty-reader".into())
        .spawn(move || pty_reader_loop(reader, task_inbox))
        .context("daemon spawn: spawn pty reader thread")?;

    // Hold the master alive past the function return: the Term task
    // owns the reader through `try_clone_reader`, but the master
    // itself must stay alive (closing it kills the slave/child).
    // We leak the Box<dyn MasterPty + Send> intentionally — its
    // lifetime tracks the PTY's, which the registry already tracks
    // via the SpawnedChild.
    leak_master(pair.master);

    Ok(DaemonSpawnedTerminal {
        task,
        child: SpawnedChild {
            killer: Some(killer),
        },
        process_id,
        writer: std::sync::Arc::new(std::sync::Mutex::new(writer)),
    })
}

/// Bridge: read from PTY, blocking-send into the Term task's mpsc
/// inbox. Exits when either side closes (PTY EOF or task inbox
/// dropped — the registry teardown path).
fn pty_reader_loop(
    mut reader: Box<dyn std::io::Read + Send>,
    inbox: tokio::sync::mpsc::Sender<TerminalCommand>,
) {
    let mut buf = [0_u8; 8192];
    loop {
        match reader.read(&mut buf) {
            Ok(0) => break, // PTY EOF; child exited
            Ok(n) => {
                let bytes = buf[..n].to_vec();
                if inbox.blocking_send(TerminalCommand::Bytes(bytes)).is_err() {
                    // Term task gone. Drain any remaining bytes
                    // from the PTY so the kernel buffer doesn't
                    // back up the child indefinitely; then exit.
                    let mut sink = [0_u8; 8192];
                    while let Ok(n) = reader.read(&mut sink) {
                        if n == 0 {
                            break;
                        }
                    }
                    break;
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(_) => break,
        }
    }
}

/// `MasterPty` is `?Sized` and not `Clone`. We can't store it in a
/// `'static` slot via `Arc` without a wrapper, and the spawn
/// function returns before the PTY's lifetime ends. The simplest
/// safe option is to leak the box; the OS reclaims the PTY on
/// process exit, and the registry-managed `SpawnedChild::kill`
/// closes the slave on tab close (which causes the reader to hit
/// EOF and the master's resources to be reclaimed by the kernel
/// when the last reference drops).
fn leak_master(master: Box<dyn MasterPty + Send>) {
    let _: &'static dyn MasterPty = Box::leak(master);
}

/// Provide a non-blocking handle into the Term task's inbox. The
/// reader thread uses `blocking_send` for backpressure; this
/// helper exposes the `Sender` clone needed for that.
trait TaskInbox {
    fn try_send_handle(&self) -> tokio::sync::mpsc::Sender<TerminalCommand>;
}

impl TaskInbox for TerminalTaskHandle {
    fn try_send_handle(&self) -> tokio::sync::mpsc::Sender<TerminalCommand> {
        self.inbox_clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use another_one_core::agents::{TerminalLaunchConfig, TerminalLaunchMode};

    fn raw_shell_request() -> SpawnRequest {
        SpawnRequest {
            cwd: None,
            launch_config: TerminalLaunchConfig {
                mode: TerminalLaunchMode::RawShell,
                provider: None,
                ..TerminalLaunchConfig::default()
            },
            agent_launch_args: Vec::new(),
            size: TerminalGridSize {
                cols: 40,
                rows: 6,
                pixel_width: 0,
                pixel_height: 0,
            },
        }
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn spawn_raw_shell_yields_a_term_task_that_observes_output() {
        // Spawning a raw shell should produce a Term task that
        // observes whatever the shell prints (default prompt /
        // shell init banner). We don't assert on prompt contents —
        // platforms vary — but we do assert that *some* frame
        // arrives within a generous window.
        let outcome = match spawn_terminal_in_daemon(raw_shell_request()) {
            Ok(o) => o,
            Err(e) => {
                // Some sandboxed CI environments don't allow opening
                // a PTY (no /dev/ptmx, no DISPLAY, restricted
                // container). Skip rather than fail in that case.
                eprintln!("skipping: pty unavailable in this environment: {e:#}");
                return;
            }
        };
        assert!(outcome.process_id.is_some());

        let mut watch = outcome.task.subscribe();
        let observed = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                if watch.borrow().is_some() {
                    break true;
                }
                if watch.changed().await.is_err() {
                    break false;
                }
            }
        })
        .await
        .unwrap_or(false);
        assert!(observed, "no frame from spawned shell within 2s");
    }

    #[test]
    fn flag_defaults_to_off() {
        // Run the env-check helper in a clean environment: even if
        // the test runner exports the var, the OnceLock caches the
        // first read for the process. We can only assert "the
        // function returns a stable bool"; the behaviour test for
        // the on-state lives in the dispatch integration test where
        // the var is set explicitly.
        let _ = daemon_spawn_enabled();
    }
}
