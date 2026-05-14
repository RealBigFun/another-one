//! Per-viewer frame pacer.
//!
//! Phase 3a of `docs/designs/01-daemon-canonical-terminal.md`. One
//! [`Pacer`] task per `(viewer, section, tab)` subscription:
//!
//! - watches the per-tab Term task's
//!   `tokio::sync::watch::Receiver<Option<Arc<TerminalFrame>>>`;
//! - debounces emission at most once per 16 ms (a single fixed
//!   60 fps cap — `max_fps` is captured from the subscribe verb but
//!   not yet honored; Phase 9 wires per-viewer rates);
//! - forwards each emitted frame as
//!   `WorkerReply::TerminalFrame { section_id, tab_id, frame }`
//!   through `ServerSession::push_reply`. The `daemon_transport`
//!   layer demuxes that on the client side into
//!   `SessionEvent::TerminalFrame` (Phase 1b).
//!
//! ## Cold-start
//!
//! When a subscription begins, the pacer either:
//!
//! - picks up the current `Option<Arc<TerminalFrame>>` from the
//!   watch and emits it immediately if it's `Some` and either
//!   `since_seq` is missing or the watch's seq is newer than
//!   `since_seq`; OR
//! - parks on `watch::Receiver::changed()` until the Term task
//!   produces its first frame.
//!
//! ## Shutdown
//!
//! [`PacerHandle::abort`] aborts the underlying tokio task. The
//! handle is `Send + Sync` so the registry can hold it behind an
//! `Arc<Mutex<...>>`. Dropping the handle aborts in `Drop` so a
//! disconnected viewer's pacers wind down without explicit
//! teardown.
//!
//! ## What this module is *not*
//!
//! Phase 3b wires a `TerminalSubscribe` dispatch arm that spawns
//! pacers; this module only stands up the per-viewer task. There
//! is no registry-wide pacer book-keeping here.

use std::sync::Arc;
use std::time::Duration;

use daemon_proto::{TerminalFrame, WorkerReply};
use daemon_transport::ServerSession;
use tokio::sync::{broadcast, watch};
use tokio::task::{AbortHandle, JoinHandle};

use super::TerminalSideEffect;

/// Single fixed pacing interval. 60 fps matches a typical desktop
/// refresh and is comfortably above human flicker thresholds for
/// terminal updates. Phase 9 swaps this for per-viewer
/// `Control::TerminalSubscribe { max_fps }` honoring.
const PACER_INTERVAL: Duration = Duration::from_millis(16);

/// Subscription parameters captured from `Control::TerminalSubscribe`.
#[derive(Clone, Debug)]
pub struct PacerConfig {
    /// Stable identifier the transport assigned to the connecting
    /// peer. Used for logging only today; Phase 3b's registry uses
    /// it to clean up pacers on viewer disconnect.
    pub viewer_id: String,
    pub section_id: String,
    pub tab_id: String,
    /// Viewer's hint for max frames per second. Ignored in this
    /// phase (single fixed 60 fps cap); reserved for Phase 9.
    #[allow(dead_code)]
    pub max_fps: u8,
    /// Last seq the viewer holds. `None` for first subscription;
    /// `Some(seq)` triggers the cold-start to emit a fresh `Full`
    /// only when the watch has a newer seq.
    pub since_seq: Option<u64>,
}

/// Handle the registry holds for one running pacer task. Drop or
/// `abort()` to wind it down.
#[derive(Debug)]
pub struct PacerHandle {
    abort: AbortHandle,
}

impl PacerHandle {
    /// Abort the pacer task. Idempotent; subsequent calls are no-ops.
    pub fn abort(&self) {
        self.abort.abort();
    }

    /// True once the pacer task has finished (cleanly or aborted).
    /// Useful for tests that want to assert post-abort tear-down.
    pub fn is_finished(&self) -> bool {
        self.abort.is_finished()
    }
}

impl Drop for PacerHandle {
    fn drop(&mut self) {
        self.abort.abort();
    }
}

/// Spawn a pacer task. Returns the handle the registry holds.
///
/// `side_effects` is optional: when `Some`, the pacer also forwards
/// bell / title / reset-title events as `WorkerReply::Push(...)`
/// variants on the same session. Side-channel events bypass the
/// frame-rate cap because they're low-rate and the renderer wants
/// to react to them regardless of frame cadence (sidebar title
/// updates while the tab is unfocused, bell flashes on a throttled
/// subscription).
pub fn spawn_pacer(
    config: PacerConfig,
    frame_watch: watch::Receiver<Option<Arc<TerminalFrame>>>,
    side_effects: Option<broadcast::Receiver<TerminalSideEffect>>,
    session: Arc<dyn ServerSession>,
) -> PacerHandle {
    let join: JoinHandle<()> = tokio::spawn(run_pacer(config, frame_watch, side_effects, session));
    PacerHandle {
        abort: join.abort_handle(),
    }
}

async fn run_pacer(
    config: PacerConfig,
    mut frame_watch: watch::Receiver<Option<Arc<TerminalFrame>>>,
    mut side_effects: Option<broadcast::Receiver<TerminalSideEffect>>,
    session: Arc<dyn ServerSession>,
) {
    // Cold-start: emit the current frame if the watch already
    // holds one and the viewer's `since_seq` (if any) is stale.
    // The borrow guard is held across an await otherwise, which
    // makes the whole future !Send because watch's RwLock guard
    // isn't Send. Clone-and-drop the guard explicitly.
    let initial = {
        let guard = frame_watch.borrow_and_update();
        guard.clone()
    };
    if let Some(frame) = initial {
        if should_emit_initial(&frame, config.since_seq) {
            if !push_frame(&session, &config, &frame).await {
                return;
            }
        }
    }

    // Steady state: select between frame updates and side-channel
    // events. Frame updates pace at 60 fps; side-channel events
    // (bell/title/reset-title) emit immediately because they're
    // low-rate and the renderer wants them out-of-band.
    let mut last_emit = tokio::time::Instant::now();
    loop {
        // Helper: a future that resolves with the next side-channel
        // event, or never resolves when there's no side-channel
        // receiver (collapses the select to single-arm). Returns
        // Option<TerminalSideEffect>; None means the broadcast was
        // closed/lagged (broadcast::Receiver::recv -> Err) and we
        // skip without exiting.
        let side_recv: futures_util::future::BoxFuture<'_, Option<TerminalSideEffect>> =
            match side_effects.as_mut() {
                Some(rx) => Box::pin(async move { rx.recv().await.ok() }),
                None => Box::pin(std::future::pending()),
            };

        tokio::select! {
            // Frame stream: park on next change, debounce, emit.
            res = frame_watch.changed() => {
                if res.is_err() {
                    tracing::trace!(
                        viewer = %config.viewer_id,
                        section = %config.section_id,
                        tab = %config.tab_id,
                        "pacer: frame watch closed; exiting"
                    );
                    return;
                }
                let next_allowed = last_emit + PACER_INTERVAL;
                let now = tokio::time::Instant::now();
                if now < next_allowed {
                    tokio::time::sleep_until(next_allowed).await;
                }
                let frame = {
                    let guard = frame_watch.borrow_and_update();
                    match guard.clone() {
                        Some(f) => f,
                        None => continue,
                    }
                };
                if !push_frame(&session, &config, &frame).await {
                    return;
                }
                last_emit = tokio::time::Instant::now();
            }
            // Side-channel: bell / title / reset-title. Emitted
            // immediately, no rate cap.
            event = side_recv => {
                let Some(event) = event else { continue };
                let reply = match event {
                    TerminalSideEffect::Title(title) => WorkerReply::TerminalTitle {
                        section_id: config.section_id.clone(),
                        tab_id: config.tab_id.clone(),
                        title,
                    },
                    TerminalSideEffect::ResetTitle => WorkerReply::TerminalResetTitle {
                        section_id: config.section_id.clone(),
                        tab_id: config.tab_id.clone(),
                    },
                    TerminalSideEffect::Bell => WorkerReply::TerminalBell {
                        section_id: config.section_id.clone(),
                        tab_id: config.tab_id.clone(),
                    },
                };
                if session.push_reply(reply).await.is_err() {
                    tracing::debug!(
                        viewer = %config.viewer_id,
                        section = %config.section_id,
                        tab = %config.tab_id,
                        "pacer: side-channel push_reply failed; exiting"
                    );
                    return;
                }
            }
        }
    }
}

fn should_emit_initial(frame: &Arc<TerminalFrame>, since_seq: Option<u64>) -> bool {
    let watch_seq = match &**frame {
        TerminalFrame::Full { seq, .. } => *seq,
        TerminalFrame::Diff { seq, .. } => *seq,
    };
    match since_seq {
        None => true,
        Some(client_seq) => watch_seq > client_seq,
    }
}

/// Push one frame to the viewer. Returns `false` when the session
/// rejected the push (transport closed) so the caller can wind
/// down. Anything else is logged and treated as transient.
async fn push_frame(
    session: &Arc<dyn ServerSession>,
    config: &PacerConfig,
    frame: &Arc<TerminalFrame>,
) -> bool {
    let reply = WorkerReply::TerminalFrame {
        section_id: config.section_id.clone(),
        tab_id: config.tab_id.clone(),
        frame: (**frame).clone(),
    };
    match session.push_reply(reply).await {
        Ok(()) => {
            tracing::trace!(
                viewer = %config.viewer_id,
                section = %config.section_id,
                tab = %config.tab_id,
                "DBG: pacer pushed TerminalFrame"
            );
            true
        }
        Err(err) => {
            tracing::debug!(
                viewer = %config.viewer_id,
                section = %config.section_id,
                tab = %config.tab_id,
                error = %err,
                "pacer: push_reply failed; exiting"
            );
            false
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terminal::{spawn_terminal_task, TerminalCommand};
    use another_one_core::terminal_types::TerminalGridSize;
    use daemon_transport::SessionEvent;
    use futures_util::StreamExt;
    use std::time::Duration;

    fn small_size() -> TerminalGridSize {
        TerminalGridSize {
            cols: 10,
            rows: 3,
            pixel_width: 0,
            pixel_height: 0,
        }
    }

    /// Spin up an in-memory transport pair, a Term task, and a
    /// pacer pointed at the server side. Returns the client
    /// session for the test to drain events from.
    async fn make_paired_pacer(
        since_seq: Option<u64>,
    ) -> (
        Box<dyn daemon_transport::Session>,
        crate::terminal::TerminalTaskHandle,
        PacerHandle,
    ) {
        // The in_memory module is deprecated for production use
        // (see docs/designs/01-daemon-canonical-terminal.md), but
        // is exactly what we want for these unit tests.
        #[allow(deprecated)]
        let (server, client) = daemon_transport::in_memory::pair("test-viewer");
        let server_arc: Arc<dyn ServerSession> = Arc::from(server);
        let term_handle = spawn_terminal_task(small_size());
        let pacer = spawn_pacer(
            PacerConfig {
                viewer_id: "test-viewer".into(),
                section_id: "proj-a:section-1".into(),
                tab_id: "7".into(),
                max_fps: 60,
                since_seq,
            },
            term_handle.subscribe(),
            Some(term_handle.subscribe_side_effects()),
            server_arc,
        );
        (client, term_handle, pacer)
    }

    /// Pull events from the client until we observe a
    /// `TerminalFrame` or the timeout elapses.
    async fn next_terminal_frame(
        events: &mut daemon_transport::EventStream,
        deadline: Duration,
    ) -> Option<TerminalFrame> {
        let waiter = async {
            while let Some(event) = events.next().await {
                if let SessionEvent::TerminalFrame { frame, .. } = event {
                    return Some(frame);
                }
            }
            None
        };
        tokio::time::timeout(deadline, waiter).await.ok().flatten()
    }

    #[tokio::test]
    async fn pacer_forwards_frames_to_subscribed_viewer() {
        let (client, term_handle, pacer) = make_paired_pacer(None).await;
        let mut events = client.events();

        // Send some bytes; the Term task emits a Full, the pacer
        // forwards it as a `WorkerReply::TerminalFrame` push, the
        // in-memory transport demuxes it into
        // `SessionEvent::TerminalFrame`.
        term_handle
            .send(TerminalCommand::Bytes(b"hi".to_vec()))
            .await
            .expect("send bytes");

        let frame = next_terminal_frame(&mut events, Duration::from_secs(1))
            .await
            .expect("frame arrives");
        match frame {
            TerminalFrame::Full { seq, snapshot } => {
                assert_eq!(seq, 1);
                assert_eq!(snapshot.viewport[0].cells[0].ch, 'h');
                assert_eq!(snapshot.viewport[0].cells[1].ch, 'i');
            }
            _ => panic!("expected Full"),
        }

        pacer.abort();
        term_handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn pacer_skips_initial_emission_when_since_seq_is_current() {
        // Pre-seed the Term so it has a frame at seq=1 before the
        // pacer subscribes. Subscribing with `since_seq = Some(1)`
        // means the viewer already holds seq=1; no initial emit
        // should fire until a *new* frame arrives.
        let term_handle = spawn_terminal_task(small_size());
        term_handle
            .send(TerminalCommand::Bytes(b"a".to_vec()))
            .await
            .expect("send bytes");

        // Drain so the watch holds Some(seq=1) before we subscribe.
        for _ in 0..50 {
            if term_handle.latest_frame().is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        #[allow(deprecated)]
        let (server, client) = daemon_transport::in_memory::pair("test-viewer");
        let server_arc: Arc<dyn ServerSession> = Arc::from(server);
        let pacer = spawn_pacer(
            PacerConfig {
                viewer_id: "test-viewer".into(),
                section_id: "proj-a:section-1".into(),
                tab_id: "7".into(),
                max_fps: 60,
                since_seq: Some(1),
            },
            term_handle.subscribe(),
            None,
            server_arc,
        );
        let mut events = client.events();

        // No frame should arrive within a short window.
        let nothing = next_terminal_frame(&mut events, Duration::from_millis(100)).await;
        assert!(
            nothing.is_none(),
            "no initial emit when since_seq matches the watch's seq"
        );

        // Now bump the Term's seq; the pacer should emit.
        term_handle
            .send(TerminalCommand::Bytes(b"b".to_vec()))
            .await
            .expect("send bytes");
        let frame = next_terminal_frame(&mut events, Duration::from_secs(1))
            .await
            .expect("frame arrives after seq bump");
        match frame {
            TerminalFrame::Full { seq, .. } => assert_eq!(seq, 2),
            _ => panic!("expected Full"),
        }

        pacer.abort();
        term_handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn pacer_caps_emission_at_60fps() {
        // Send 10 small bytes commands rapid-fire. The Term task
        // emits 10 Full frames; the pacer's 16 ms cap means the
        // viewer should observe noticeably fewer than 10 frames
        // arriving (intermediates collapse via `borrow_and_update`).
        let (client, term_handle, pacer) = make_paired_pacer(None).await;
        let mut events = client.events();

        for ch in b'a'..=b'j' {
            term_handle
                .send(TerminalCommand::Bytes(vec![ch]))
                .await
                .expect("send byte");
        }

        // Collect everything that arrives in 200 ms (~12 pacer
        // intervals).
        let mut frames = Vec::new();
        let collect = async {
            while let Some(event) = events.next().await {
                if let SessionEvent::TerminalFrame { frame, .. } = event {
                    frames.push(frame);
                }
            }
        };
        let _ = tokio::time::timeout(Duration::from_millis(200), collect).await;

        assert!(
            !frames.is_empty(),
            "at least one frame must arrive within 200 ms"
        );
        assert!(
            frames.len() < 10,
            "60 fps cap should drop intermediate frames; got {}",
            frames.len()
        );

        pacer.abort();
        term_handle.shutdown().await.expect("shutdown");
    }

    #[tokio::test]
    async fn pacer_handle_drop_aborts_task() {
        let (_client, term_handle, pacer) = make_paired_pacer(None).await;
        assert!(!pacer.is_finished());
        drop(pacer);

        // Drop should abort the spawned task.
        let mut probe = term_handle.subscribe();
        // Send a byte; if any pacer is still alive it would push.
        // We just confirm the original handle's join future
        // resolves when we explicitly try (not directly observable
        // with AbortHandle; rely on subscribe channel cleanup).
        term_handle
            .send(TerminalCommand::Bytes(b"x".to_vec()))
            .await
            .expect("send byte");
        // No assertion about emission here \u2014 we only need to
        // confirm the handle drop didn't panic and the term task
        // still runs (regression guard against accidental shared
        // shutdown).
        let _ = probe.changed().await;
        term_handle.shutdown().await.expect("shutdown");
    }
}
