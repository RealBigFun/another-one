//! Per-tab terminal task.
//!
//! Owns one `alacritty_terminal::Term` + `Processor` + the queue of
//! `EventListener` events the parser surfaces (PtyWrite, Title, Bell,
//! …). The task is the single mutator of the Term; viewers and
//! producers communicate through the inbox channel
//! ([`TerminalCommand`]) and never touch the Term directly. No
//! `Mutex<Term>` — single-owner-no-lock is the whole point.
//!
//! ## Phase 2 scope
//!
//! Phase 2a (this commit) wires the skeleton:
//!
//! - [`TerminalCommand`] command enum (extended commit-by-commit by
//!   the rest of Phase 2).
//! - The task loop: drain the inbox, feed `Bytes` through the parser,
//!   drain the Term event queue, mark the tab dirty.
//! - [`spawn_terminal_task`] entry point and [`TerminalTaskHandle`]
//!   the registry will hold once Phase 3 wires subscribe/unsubscribe.
//!
//! Phase 2b adds frame serialization + `RequestFullFrame`. Phase 2c
//! adds resize / search / scrollback. Phase 2d adds the bell / title
//! side-channel.
//!
//! ## Why a tokio task and not just a function
//!
//! Eventually each Term task gets PTY input, viewer subscriptions,
//! and a 16 ms-tick coalesce timer in one `select!`. Standing it up
//! as a task now (with only the bytes arm wired) means future phases
//! add `select!` arms and command variants without restructuring.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

use alacritty_terminal::event::{Event as AlacrittyEvent, EventListener};
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi;
use another_one_core::terminal_types::TerminalGridSize;
use daemon_proto::TerminalFrame;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::JoinHandle;

use super::frame::serialize_full_frame;

/// Default inbox capacity. Sized to absorb a burst of small `Bytes`
/// chunks from a chatty harness without backpressuring the PTY reader
/// before the Term task has a chance to drain. The PTY reader is a
/// blocking `std::thread`; once Phase 4 wires it up, `blocking_send`
/// into this channel is the natural backpressure path when the Term
/// task is genuinely stuck (which would only happen during
/// `Processor::advance` of a degenerate input).
const INBOX_CAPACITY: usize = 256;

/// A unit of work for the per-tab Term task. The enum grows over
/// Phase 2 as handlers come online; pattern matches inside the task
/// loop must stay exhaustive on every commit so a missed variant is
/// a compile error.
#[derive(Debug)]
pub enum TerminalCommand {
    /// Bytes from the PTY reader. Fed through the VT parser into the
    /// Term. The actual reader-to-task plumbing lands in Phase 4
    /// (`Control::LaunchTab` moves to the daemon); for Phase 2 the
    /// only producer is the test suite.
    Bytes(Vec<u8>),
    /// Hand the latest serialized `Full` frame back through the
    /// reply oneshot. Used by Phase 3's pacer to recover after a
    /// `seq` gap, by tests that want a deterministic snapshot, and
    /// by future `Control::TerminalSubscribe` first-frame delivery.
    /// Replies with `None` when the task has not yet observed any
    /// bytes (frame `seq` would be 0 and the snapshot is empty
    /// anyway, but we surface the absence so callers don't conflate
    /// it with "freshly cleared screen").
    RequestFullFrame {
        reply: oneshot::Sender<Option<Arc<TerminalFrame>>>,
    },
    /// Tear down the task. The loop returns after this; the Term and
    /// any held resources drop on its way out.
    Shutdown,
}

/// Handle the daemon registry holds for one running Term task. Drop
/// the handle to send a `Shutdown` and detach the join in the
/// background; the registry's removal path (Phase 3+) calls the
/// explicit `shutdown` to wait for clean teardown.
#[derive(Debug)]
pub struct TerminalTaskHandle {
    inbox: mpsc::Sender<TerminalCommand>,
    /// Latest emitted frame. `None` until the task observes its
    /// first byte and emits a `Full`. Cloned cheaply by Phase 3's
    /// pacer when subscribing.
    frame_watch: watch::Receiver<Option<Arc<TerminalFrame>>>,
    join: Option<JoinHandle<()>>,
}

impl TerminalTaskHandle {
    /// Subscribe to the task's frame stream. Phase 3's per-viewer
    /// pacer holds a clone of this receiver; Phase 2 tests use it
    /// directly to assert frame emission.
    pub fn subscribe(&self) -> watch::Receiver<Option<Arc<TerminalFrame>>> {
        self.frame_watch.clone()
    }

    /// Snapshot the latest frame without spawning a watcher. `None`
    /// when no bytes have been parsed yet.
    pub fn latest_frame(&self) -> Option<Arc<TerminalFrame>> {
        self.frame_watch.borrow().clone()
    }
    /// Send a command into the task's inbox. Returns `Err` when the
    /// task has already exited — callers treat that as "tab is gone"
    /// and should drop the handle.
    pub async fn send(
        &self,
        command: TerminalCommand,
    ) -> Result<(), mpsc::error::SendError<TerminalCommand>> {
        self.inbox.send(command).await
    }

    /// Best-effort synchronous send for non-tokio callers (e.g. the
    /// PTY reader thread Phase 4 will spawn). Returns `Err` if the
    /// inbox is closed or full — a full inbox is the intended
    /// backpressure signal upstream of the task.
    pub fn try_send(
        &self,
        command: TerminalCommand,
    ) -> Result<(), mpsc::error::TrySendError<TerminalCommand>> {
        self.inbox.try_send(command)
    }

    /// Cleanly stop the task and wait for the loop to exit.
    pub async fn shutdown(mut self) -> std::io::Result<()> {
        // Best-effort send; if the task already exited the channel
        // is closed and we just await the join.
        let _ = self.inbox.send(TerminalCommand::Shutdown).await;
        if let Some(join) = self.join.take() {
            join.await
                .map_err(|e| std::io::Error::other(format!("terminal task join: {e}")))?;
        }
        Ok(())
    }
}

impl Drop for TerminalTaskHandle {
    fn drop(&mut self) {
        // Background shutdown: best-effort signal so the task winds
        // down even when the handle is dropped without an explicit
        // `shutdown().await`. The JoinHandle, if still present, is
        // detached — the runtime drops it when it completes.
        // Production code paths should prefer `shutdown().await` so
        // the caller can observe a panic in the task; this is a
        // safety net.
        let inbox = self.inbox.clone();
        tokio::spawn(async move {
            let _ = inbox.send(TerminalCommand::Shutdown).await;
        });
    }
}

/// Spawn a per-tab Term task on the current tokio runtime. Returns
/// the handle the registry holds. The task starts idle: nothing to
/// do until `Bytes` arrive on the inbox.
pub fn spawn_terminal_task(size: TerminalGridSize) -> TerminalTaskHandle {
    let (tx, rx) = mpsc::channel(INBOX_CAPACITY);
    let (frame_tx, frame_rx) = watch::channel(None::<Arc<TerminalFrame>>);
    let join = tokio::spawn(run_terminal_task(rx, size, frame_tx));
    TerminalTaskHandle {
        inbox: tx,
        frame_watch: frame_rx,
        join: Some(join),
    }
}

/// Per-task state. Private to this module; the public surface is the
/// command enum and the handle.
struct TerminalTask {
    term: Term<TermEventProxy>,
    parser: ansi::Processor,
    /// Events the alacritty `Term` surfaces during parsing
    /// (PtyWrite, Title, Bell, ColorRequest, …). The proxy pushes
    /// here; the task drains after each `Bytes` and routes to the
    /// side-channel + writer (Phases 2d, 4).
    event_queue: Arc<Mutex<VecDeque<AlacrittyEvent>>>,
    /// Set after every Term mutation; cleared when the task emits a
    /// frame. With Phase 2b's frame emission wired the flag is
    /// load-bearing only for batching across multiple `Bytes`
    /// chunks in one drain; Phase 3's pacer pulls from the watch
    /// channel directly.
    dirty: bool,
    /// Monotonic frame counter. Increments on every emitted `Full`;
    /// resets only when the task is recreated (PTY relaunch).
    seq: u64,
    /// Channel viewers (and Phase 3 pacers) read frames from. The
    /// receiver count tells us whether anyone is listening; with
    /// zero receivers we still update the watch (cheap), so a late
    /// subscriber that calls `latest_frame()` gets the current state.
    frame_watch: watch::Sender<Option<Arc<TerminalFrame>>>,
    /// Latest frame, also kept here so `RequestFullFrame` can reply
    /// without re-borrowing the watch channel.
    latest: Option<Arc<TerminalFrame>>,
}

impl TerminalTask {
    fn new(size: TerminalGridSize, frame_watch: watch::Sender<Option<Arc<TerminalFrame>>>) -> Self {
        let event_queue = Arc::new(Mutex::new(VecDeque::new()));
        let proxy = TermEventProxy {
            queue: Arc::clone(&event_queue),
        };
        let term = Term::new(Config::default(), &size, proxy);
        Self {
            term,
            parser: ansi::Processor::default(),
            event_queue,
            dirty: false,
            seq: 0,
            frame_watch,
            latest: None,
        }
    }

    /// Apply one chunk of bytes from the PTY reader. Drains any
    /// events the parser surfaces during this advance and discards
    /// them for now; Phase 2d replaces the discard with a real
    /// side-channel emitter. Bumps `seq` and emits a fresh `Full`
    /// frame so subscribed viewers (or `RequestFullFrame` callers)
    /// observe the post-advance state.
    fn apply_bytes(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.dirty = true;
        if let Ok(mut queue) = self.event_queue.lock() {
            queue.clear();
        }
        self.emit_full_frame();
    }

    /// Build a `TerminalFrame::Full` from the current Term state
    /// and publish it on the watch channel. Phase 3's pacer
    /// observes via `Receiver::changed`; Phase 2 tests observe via
    /// `Receiver::borrow`.
    fn emit_full_frame(&mut self) {
        self.seq = self.seq.wrapping_add(1);
        let frame = serialize_full_frame(&self.term, self.seq);
        // `send` returns `Err` only when there are zero receivers;
        // we still want to hold the latest internally so a future
        // subscriber sees current state. Watch values are
        // automatically retained by the channel even with zero
        // receivers, so the explicit error is just informational.
        let _ = self.frame_watch.send(Some(Arc::clone(&frame)));
        self.latest = Some(frame);
        self.dirty = false;
    }

    /// Borrow the underlying Term for tests. Production code paths
    /// route through the command inbox — this is the seam the
    /// in-task tests use to assert grid state.
    #[cfg(test)]
    fn term(&self) -> &Term<TermEventProxy> {
        &self.term
    }

    #[cfg(test)]
    fn dirty(&self) -> bool {
        self.dirty
    }

    #[cfg(test)]
    fn latest(&self) -> Option<Arc<TerminalFrame>> {
        self.latest.clone()
    }
}

/// `EventListener` impl backing the Term's event queue. Same shape
/// as `app::terminal_runtime::RuntimeEventProxy`; runs on whatever
/// thread invokes `Processor::advance` — in the daemon Term task,
/// that's the task's tokio worker.
#[derive(Clone)]
struct TermEventProxy {
    queue: Arc<Mutex<VecDeque<AlacrittyEvent>>>,
}

impl EventListener for TermEventProxy {
    fn send_event(&self, event: AlacrittyEvent) {
        match self.queue.lock() {
            Ok(mut queue) => queue.push_back(event),
            Err(poisoned) => {
                // A poisoned lock means a previous holder panicked.
                // The Term task's own `apply_bytes` is the only
                // writer, so recovering and pushing is strictly
                // better than panicking the daemon worker.
                let mut queue = poisoned.into_inner();
                queue.push_back(event);
            }
        }
    }
}

async fn run_terminal_task(
    mut inbox: mpsc::Receiver<TerminalCommand>,
    size: TerminalGridSize,
    frame_watch: watch::Sender<Option<Arc<TerminalFrame>>>,
) {
    let mut state = TerminalTask::new(size, frame_watch);
    while let Some(command) = inbox.recv().await {
        match command {
            TerminalCommand::Bytes(bytes) => {
                state.apply_bytes(&bytes);
            }
            TerminalCommand::RequestFullFrame { reply } => {
                // Send a fresh frame even if no bytes have arrived;
                // None tells the caller "task is up but the screen
                // is still default-blank". Subscribers join via
                // `subscribe()` instead of polling, so this verb
                // is mostly for tests + Phase 3 cold-start.
                let _ = reply.send(state.latest.clone());
            }
            TerminalCommand::Shutdown => {
                tracing::trace!("terminal task shutdown");
                break;
            }
        }
    }
    // Inbox closed without an explicit Shutdown is also a clean
    // termination — the registry dropped the handle. Just exit;
    // alacritty's `Term` cleans up on drop.
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::grid::Dimensions;
    use alacritty_terminal::index::{Column, Line, Point};
    use daemon_proto::TerminalFrame;

    fn small_size() -> TerminalGridSize {
        TerminalGridSize {
            cols: 10,
            rows: 3,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    fn fresh_state() -> TerminalTask {
        let (frame_tx, _frame_rx) = watch::channel(None::<Arc<TerminalFrame>>);
        TerminalTask::new(small_size(), frame_tx)
    }

    #[test]
    fn apply_bytes_writes_into_grid_and_marks_dirty() {
        let mut state = fresh_state();
        // Freshly constructed task isn't dirty and has no frame.
        assert!(state.latest().is_none(), "no frame before any bytes");

        state.apply_bytes(b"hello");

        // Apply emits a frame, which clears `dirty`.
        assert!(!state.dirty(), "dirty cleared after frame emit");

        // First five cells of the first row should now contain
        // 'h','e','l','l','o' — sanity-check via the alacritty grid.
        let grid = state.term().grid();
        let row = Line(0);
        let chars: String = (0..5)
            .map(|c| grid[Point::new(row, Column(c))].c)
            .collect();
        assert_eq!(chars, "hello");
    }

    #[test]
    fn apply_bytes_handles_empty_chunks_idempotently() {
        let mut state = fresh_state();
        state.apply_bytes(b"");
        // Empty advance still emits a frame (callers can't tell
        // "no change" from "changed back to identical" without seq;
        // the seq is the source of truth for change tracking).
        let grid = state.term().grid();
        assert_eq!(grid.columns(), 10);
        assert_eq!(grid.screen_lines(), 3);
        assert!(state.latest().is_some());
    }

    #[test]
    fn emit_full_frame_increments_seq_and_publishes_snapshot() {
        let mut state = fresh_state();
        state.apply_bytes(b"hi");
        let first = state.latest().expect("first frame");
        state.apply_bytes(b"there");
        let second = state.latest().expect("second frame");

        match (&*first, &*second) {
            (
                TerminalFrame::Full {
                    seq: s1,
                    snapshot: snap1,
                },
                TerminalFrame::Full {
                    seq: s2,
                    snapshot: snap2,
                },
            ) => {
                assert_eq!(*s1, 1);
                assert_eq!(*s2, 2);
                assert_eq!(snap1.cols, 10);
                assert_eq!(snap1.rows, 3);
                let row0 = &snap2.viewport[0].cells;
                let chars: String = row0.iter().map(|c| c.ch).collect();
                assert!(
                    chars.starts_with("hithere"),
                    "row 0 starts with 'hithere', got {chars:?}"
                );
            }
            _ => panic!("expected two Full frames"),
        }
    }

    #[tokio::test]
    async fn task_runs_and_shuts_down_cleanly() {
        let handle = spawn_terminal_task(small_size());
        handle
            .send(TerminalCommand::Bytes(b"abc".to_vec()))
            .await
            .expect("send bytes");
        handle.shutdown().await.expect("shutdown clean");
    }

    #[tokio::test]
    async fn task_exits_when_handle_dropped() {
        let handle = spawn_terminal_task(small_size());
        let inbox = handle.inbox.clone();
        drop(handle);
        for _ in 0..50 {
            if inbox.is_closed() {
                return;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        panic!("task did not close inbox after handle drop");
    }

    #[tokio::test]
    async fn request_full_frame_returns_none_before_any_bytes() {
        let handle = spawn_terminal_task(small_size());
        let (tx, rx) = oneshot::channel();
        handle
            .send(TerminalCommand::RequestFullFrame { reply: tx })
            .await
            .expect("send request");
        let frame = rx.await.expect("reply");
        assert!(frame.is_none(), "no frame before any bytes parsed");
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn request_full_frame_returns_latest_after_bytes() {
        let handle = spawn_terminal_task(small_size());
        handle
            .send(TerminalCommand::Bytes(b"hi".to_vec()))
            .await
            .expect("send bytes");
        let (tx, rx) = oneshot::channel();
        handle
            .send(TerminalCommand::RequestFullFrame { reply: tx })
            .await
            .expect("send request");
        let frame = rx.await.expect("reply").expect("frame present");
        match &*frame {
            TerminalFrame::Full { seq, snapshot } => {
                assert_eq!(*seq, 1);
                assert_eq!(snapshot.viewport[0].cells[0].ch, 'h');
                assert_eq!(snapshot.viewport[0].cells[1].ch, 'i');
            }
            _ => panic!("expected Full frame"),
        }
        handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn watch_receiver_observes_frame_emission() {
        let handle = spawn_terminal_task(small_size());
        let mut rx = handle.subscribe();

        // Initial value is `None` (no frames yet).
        assert!(rx.borrow().is_none());

        handle
            .send(TerminalCommand::Bytes(b"x".to_vec()))
            .await
            .expect("send bytes");

        // Wait for the watch to flip; bounded so a regression
        // doesn't hang the test harness.
        for _ in 0..50 {
            if rx.borrow().is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
        let snap = rx.borrow().clone().expect("frame published");
        match &*snap {
            TerminalFrame::Full { seq, .. } => assert_eq!(*seq, 1),
            _ => panic!("expected Full"),
        }
        handle.shutdown().await.expect("shutdown");
    }
}
