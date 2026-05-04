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
use daemon_transport::{DialTarget, Session as AbstractSession};

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

/// Pending session handoff slot. The QR-pair dial task drops the
/// freshly-dialed `daemon_transport::Session` here; the GPUI render
/// tick takes it and calls `AnotherOneApp::replace_session`, which
/// re-spawns the session-events pump on the new session so PTY bytes
/// start flowing into `session_events_rx`.
static PENDING_SESSION_HANDOFF: OnceLock<Mutex<Option<Arc<dyn AbstractSession>>>> = OnceLock::new();

fn handoff_slot() -> &'static Mutex<Option<Arc<dyn AbstractSession>>> {
    PENDING_SESSION_HANDOFF.get_or_init(|| Mutex::new(None))
}

/// Take any pending session handoff. Called by `AnotherOneApp` on the
/// render tick; if `Some`, the app immediately calls
/// `replace_session`.
pub fn take_pending_session() -> Option<Arc<dyn AbstractSession>> {
    handoff_slot().lock().ok().and_then(|mut s| s.take())
}

fn store_pending_session(session: Arc<dyn AbstractSession>) {
    if let Ok(mut slot) = handoff_slot().lock() {
        *slot = Some(session);
    }
}

/// Live abstract session held for the lifetime of the dial — keeps
/// the iroh recv loop / events bridge alive past `dial()`'s spawn
/// thread exiting.
static ACTIVE_ABSTRACT_SESSION: OnceLock<Mutex<Option<Arc<dyn AbstractSession>>>> = OnceLock::new();

fn abstract_session_slot() -> &'static Mutex<Option<Arc<dyn AbstractSession>>> {
    ACTIVE_ABSTRACT_SESSION.get_or_init(|| Mutex::new(None))
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
                // Route the dial through the abstract transport
                // factory so mobile and desktop share a single
                // `Session` shape. The factory's iroh impl
                // internally reuses `daemon_client::connect()` and
                // bridges incoming PTY bytes into
                // `SessionEvent::PtyBytes` — which the renderer's
                // `session_events_rx` already consumes.
                let factory = daemon_client::iroh_factory();
                let session = match factory.dial(DialTarget::PairingUrl(pairing_url)).await {
                    Ok(s) => Arc::from(s),
                    Err(err) => {
                        daemon_client::status::push_status(DialStatus::Error(format!(
                            "dial: {err}"
                        )));
                        return;
                    }
                };

                // Hold a strong ref so the iroh recv loop / events
                // bridge stays alive past this spawn thread exiting.
                if let Ok(mut slot) = abstract_session_slot().lock() {
                    *slot = Some(Arc::clone(&session));
                }

                // Hand the session to the GUI render tick — it
                // calls `AnotherOneApp::replace_session(...)` which
                // re-spawns the session-events pump.
                store_pending_session(Arc::clone(&session));

                // Auto-fetch the project tree right after Hello so
                // the sidebar populates without the UI having to
                // know the daemon protocol. Reply lands in the
                // legacy worker_reply_queue that
                // `drain_remote_worker_replies` already drains.
                match session.call(daemon_proto::Control::ListProjects).await {
                    Ok(reply) => push_worker_reply(reply),
                    Err(err) => {
                        daemon_client::status::push_status(DialStatus::Error(format!(
                            "list_projects: {err}"
                        )));
                        return;
                    }
                }

                // Park this task — the `Arc<dyn Session>` we stored
                // holds the connection open. Parking keeps the
                // spawn thread name visible in tracing for the
                // lifetime of the session.
                loop {
                    tokio::time::sleep(Duration::from_secs(60)).await;
                }
            });
        })
        .ok();
}
