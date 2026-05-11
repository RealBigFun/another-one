//! UI-side shim around `daemon_client`. Owns the fire-and-forget
//! "kick off a dial" entry point used by the QR scan flow, the
//! process-wide queues that surface dial status + worker replies to
//! the GPUI render tick, and a holder for the live `Session` so it
//! survives past `dial()` (the `Session`'s recv-loop tasks die when
//! the handle drops, so we have to keep one alive somewhere as long
//! as we want the connection up).
//!
//! All real protocol/transport logic lives in `daemon_client`
//! (`../../daemon-client`). This file is intentionally small — if it
//! grows beyond a few dozen lines of UI plumbing, the new logic
//! almost certainly belongs over there.

use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub use daemon_client::{drain_status as drain_dial_status, DialStatus};
use daemon_transport::Session as AbstractSession;

/// Worker replies received from the active session, queued for the
/// next render tick to drain. Right now we only forward
/// `WorkerReply::ProjectList` (the only variant the phone consumes
/// today); future variants land here as we wire more of the daemon
/// surface into the UI.
static WORKER_REPLY_QUEUE: OnceLock<Mutex<Vec<daemon_proto::WorkerReply>>> = OnceLock::new();

fn worker_reply_queue() -> &'static Mutex<Vec<daemon_proto::WorkerReply>> {
    WORKER_REPLY_QUEUE.get_or_init(|| Mutex::new(Vec::new()))
}

/// Take all queued worker replies. Called by `AnotherOneApp` on the
/// render tick; the GPUI side never blocks waiting for replies.
pub fn drain_worker_replies() -> Vec<daemon_proto::WorkerReply> {
    worker_reply_queue()
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

fn push_worker_reply(reply: daemon_proto::WorkerReply) {
    if let Ok(mut q) = worker_reply_queue().lock() {
        q.push(reply);
    }
}

/// Holds the live session for the lifetime of the dial task. We keep
/// this in a `OnceLock`-style slot rather than threading it through
/// `AnotherOneApp` because the GPUI render thread isn't async — every
/// session method is `async`, so the session itself must live on the
/// tokio runtime that owns the recv loop. UI code drives the session
/// indirectly via `request_*` helpers below that hop onto the tokio
/// runtime via `daemon_client`'s internals.
static ACTIVE_SESSION: OnceLock<Mutex<Option<std::sync::Arc<daemon_client::Session>>>> =
    OnceLock::new();

fn active_session_slot() -> &'static Mutex<Option<std::sync::Arc<daemon_client::Session>>> {
    ACTIVE_SESSION.get_or_init(|| Mutex::new(None))
}

fn store_session(s: daemon_client::Session) -> std::sync::Arc<daemon_client::Session> {
    let arc = std::sync::Arc::new(s);
    if let Ok(mut slot) = active_session_slot().lock() {
        *slot = Some(arc.clone());
    }
    arc
}

/// Pending session handoff slot. After the legacy QR-pair dial
/// succeeds, [`dial`] also wraps the legacy session into an abstract
/// [`AbstractSession`] (via [`daemon_client::wrap_legacy_session`])
/// and drops it here. The GPUI render tick takes it and calls
/// `AnotherOneApp::replace_session`, which re-spawns the
/// session-events pump on the new session — that's how mobile starts
/// receiving `SessionEvent::PtyBytes` from the daemon's
/// `AttachTab` forwarder.
///
/// Additive on top of the legacy `WORKER_REPLY_QUEUE` flow: the
/// legacy session keeps pumping `next_worker_reply` for the project
/// list snapshot; the wrapped abstract session covers PTY bytes via
/// `next_incoming_bytes` (different mpsc).
static PENDING_SESSION_HANDOFF: OnceLock<Mutex<Option<Arc<dyn AbstractSession>>>> = OnceLock::new();

fn handoff_slot() -> &'static Mutex<Option<Arc<dyn AbstractSession>>> {
    PENDING_SESSION_HANDOFF.get_or_init(|| Mutex::new(None))
}

/// Take any pending session handoff, called by the GPUI render tick.
pub fn take_pending_session() -> Option<Arc<dyn AbstractSession>> {
    handoff_slot().lock().ok().and_then(|mut s| s.take())
}

fn store_pending_session(session: Arc<dyn AbstractSession>) {
    if let Ok(mut slot) = handoff_slot().lock() {
        *slot = Some(session);
    }
}

/// Kick off a dial against the given pairing URL. Returns immediately;
/// progress flows through `daemon_client::drain_status` (re-exported
/// as [`drain_dial_status`]) and worker replies through
/// [`drain_worker_replies`]. After the dial succeeds we automatically
/// fire `ListProjects` so the sidebar populates without the UI having
/// to know about the daemon protocol.
pub fn dial(pairing_url: String) {
    std::thread::Builder::new()
        .name("iroh-dial-spawner".into())
        .spawn(move || {
            // `daemon_client::connect` is async; spin a short-lived
            // current-thread runtime here just to await the future.
            // The session itself runs on `daemon-client`'s own
            // multi-thread runtime, which long-outlives this thread.
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(err) => {
                    daemon_client::status::push_status(DialStatus::Error(format!(
                        "spawn runtime: {err}"
                    )));
                    return;
                }
            };
            rt.block_on(async {
                // Persist the client-side iroh identity at the
                // mobile app's internal-data path when one is set
                // (set by `android_main` from
                // `AndroidApp::internal_data_path`). Desktop's
                // `iroh_secret_key_path()` stub returns `None`,
                // so this defaults to the legacy ephemeral
                // behaviour off-Android — desktop doesn't run the
                // iroh *client* against its own daemon.
                let key_path = crate::mobile::iroh_secret_key_path();
                let session = match daemon_client::connect_with_secret_key(
                    &pairing_url,
                    key_path,
                )
                .await
                {
                    Ok(s) => s,
                    Err(_) => {
                        // `connect_inner` already pushed a `DialStatus::Error`.
                        return;
                    }
                };
                let session = store_session(session);

                // Additionally wrap the legacy session into the
                // abstract `daemon_transport::Session` shape and hand
                // it to the GPUI render tick. The renderer's
                // `replace_session` re-spawns the events pump on the
                // new session so PTY bytes (delivered via the daemon's
                // `AttachTab` forwarder → `next_incoming_bytes`) flow
                // into `drain_session_events`. The legacy
                // worker-reply pump below still drives the project
                // list — both can coexist on the same legacy session.
                let abstract_session: Arc<dyn AbstractSession> = Arc::from(
                    daemon_client::wrap_legacy_session(std::sync::Arc::clone(&session)),
                );
                store_pending_session(abstract_session);

                // Auto-fetch the project tree right after Hello.
                if let Err(err) = session.list_projects().await {
                    daemon_client::status::push_status(DialStatus::Error(format!(
                        "list_projects: {err}"
                    )));
                    return;
                }

                // Forward every incoming worker reply into the
                // process-wide queue. Loop runs until the daemon
                // closes the stream (recv returns `None`).
                let session_for_loop = session.clone();
                tokio::spawn(async move {
                    while let Some(reply) = session_for_loop.next_worker_reply().await {
                        push_worker_reply(reply);
                    }
                });

                // Park this task forever — the `Arc<Session>` we just
                // stored holds the connection open. Without parking,
                // the current_thread runtime returns and the spawn
                // thread exits; that's fine because the recv loop
                // lives on `daemon-client`'s own runtime, but parking
                // here keeps the spawn thread name visible in tracing
                // for as long as the session exists.
                loop {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            });
        })
        .ok();
}
