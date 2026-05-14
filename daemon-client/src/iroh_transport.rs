//! Iroh-backed concrete impl of the abstract `daemon_transport`
//! traits. Wraps the legacy [`crate::session::Session`] so callers
//! programmed against the abstract API see a network-stack-agnostic
//! surface.
//!
//! ### Scope
//!
//! Concurrent calls are correlated by `request_id` (via the legacy
//! session's call-routing); per-PTY-attach bytes flow through the
//! [`Session::events`] stream tagged with the most-recently-attached
//! `(section_id, tab_id)`.
//!
//! ### What's missing vs. the trait contract
//!
//! * **Per-channel push streams**: today's wire only carries a single
//!   PTY data fan — every attached tab's bytes arrive on the same
//!   `TY_DATA` frame type. The events stream tags bytes with the
//!   most-recently-attached `(section_id, tab_id)`, which is correct
//!   for single-attach sessions (the only shape the daemon supports
//!   today). Multi-attach demuxing is future work.
//! * **Push WorkerReply broadcasts**: `WorkerReply` frames at
//!   `request_id == 0` (future broadcast verbs like
//!   `ProjectListChanged`) aren't surfaced through `events()` because
//!   the legacy session's `next_worker_reply` mpsc is consumed
//!   elsewhere (`app/src/iroh_client.rs`). Wiring those into events
//!   waits on the typed-client refactor that retires the polling
//!   path entirely.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context as TaskContext, Poll};

use daemon_proto::{Control, WorkerReply};
use daemon_transport::{
    DialTarget, EventStream, Session as AbstractSession, SessionEvent, SessionFuture,
    TransportError, TransportFactory,
};
use futures_core::Stream;
use tokio::sync::{mpsc, Mutex as AsyncMutex};

use crate::session::{connect, Session as LegacySession};

/// `TransportFactory` impl that dials a daemon over today's iroh
/// pairing flow. Stateless — the host can construct one once at
/// startup and clone it (it's `Arc`-friendly).
#[derive(Clone, Default)]
pub struct IrohTransportFactory;

impl IrohTransportFactory {
    pub fn new() -> Self {
        Self
    }
}

impl TransportFactory for IrohTransportFactory {
    fn dial<'a>(
        &'a self,
        target: DialTarget,
    ) -> SessionFuture<'a, Result<Box<dyn AbstractSession>, TransportError>> {
        Box::pin(async move {
            let pairing_url = match target {
                DialTarget::PairingUrl(url) => url,
                DialTarget::SocketPath(path) => {
                    return Err(TransportError::Connect(format!(
                        "iroh transport doesn't speak unix sockets ({:?})",
                        path
                    )));
                }
                DialTarget::InProcess(name) => {
                    return Err(TransportError::Connect(format!(
                        "iroh transport doesn't speak in-process channels ({name:?})"
                    )));
                }
            };
            let legacy = connect(&pairing_url)
                .await
                .map_err(|e| TransportError::Connect(format!("{e:#}")))?;
            Ok(wrap_legacy_session(Arc::new(legacy)))
        })
    }
}

/// `Session` impl wrapping the legacy `daemon_client::Session`. Now
/// Wrap an existing legacy [`LegacySession`] into an
/// [`AbstractSession`]. Spawns the same `next_incoming_bytes →
/// SessionEvent::PtyBytes` bridge task that [`IrohTransportFactory::dial`]
/// uses internally, so any holder of an `Arc<LegacySession>` (e.g.
/// app-side code that already owns one from `connect()`) can hand a
/// matched abstract session to consumers that program against
/// [`AbstractSession`] without re-dialing.
///
/// The bridge consumes `next_incoming_bytes` on the legacy session,
/// so callers must not also drain that channel themselves. Worker
/// replies (`next_worker_reply`) are a separate channel and stay
/// available for legacy polling consumers.
///
/// **Daemon-canonical terminal frames (design 01).** This bridge
/// does not yet demux `WorkerReply::TerminalFrame` pushes into
/// `SessionEvent::TerminalFrame`; today they ride the existing
/// `next_worker_reply` channel that
/// `app/src/iroh_client.rs::drain_worker_replies` polls. Wiring the
/// demux into the abstract events stream lands in Phase 5b of
/// `docs/designs/01-daemon-canonical-terminal.md` (the desktop
/// viewer cutover), at which point the global drain becomes
/// redundant and is removed.
pub fn wrap_legacy_session(inner: Arc<LegacySession>) -> Box<dyn AbstractSession> {
    let attached = Arc::new(AsyncMutex::new(None::<(String, String)>));
    let (events_tx, events_rx) = mpsc::unbounded_channel::<SessionEvent>();
    let bridge_session = Arc::clone(&inner);
    let bridge_attached = Arc::clone(&attached);
    tokio::spawn(async move {
        // Each inbound chunk is now tagged by the daemon per #138,
        // so we forward the tuple directly instead of re-tagging
        // from the local `attached` mutex — which was the race
        // that let mid-stream attach switches mislabel bytes into
        // the newly-attached tab. Legacy untagged frames arrive
        // with empty ids; fall back to the local attach snapshot
        // so pre-#138 daemons still work.
        while let Some((section_id, tab_id, bytes)) = bridge_session.next_incoming_bytes().await {
            let (section_id, tab_id) = if !section_id.is_empty() && !tab_id.is_empty() {
                (section_id, tab_id)
            } else {
                match &*bridge_attached.lock().await {
                    Some(pair) => pair.clone(),
                    None => continue,
                }
            };
            if events_tx
                .send(SessionEvent::PtyBytes {
                    section_id,
                    tab_id,
                    bytes,
                })
                .is_err()
            {
                break;
            }
        }
        let _ = events_tx.send(SessionEvent::Closed { reason: None });
    });
    Box::new(IrohSession {
        inner,
        attached,
        events_rx: StdMutex::new(Some(events_rx)),
    })
}

/// uses the per-call request-id router on the legacy session, so
/// concurrent `call`s from separate tasks correlate cleanly instead
/// of racing the FIFO recv channel.
struct IrohSession {
    inner: Arc<LegacySession>,
    /// Most recently attached `(section_id, tab_id)`. Used to tag
    /// `SessionEvent::PtyBytes`; see file-level docs on the
    /// single-attach limitation. `Arc` so the bridge task spawned in
    /// `dial` can read it without owning the session.
    attached: Arc<AsyncMutex<Option<(String, String)>>>,
    /// Single-consumer events stream. Consumed (`take()`) by the
    /// first call to [`Self::events`]; subsequent calls get a
    /// terminated stream. Std `Mutex` rather than tokio's because
    /// the take is brief, sync, and `events()` is called from
    /// `Stream::poll_next` contexts where async locking would be
    /// awkward.
    events_rx: StdMutex<Option<mpsc::UnboundedReceiver<SessionEvent>>>,
}

impl AbstractSession for IrohSession {
    fn call<'a>(&'a self, verb: Control) -> SessionFuture<'a, Result<WorkerReply, TransportError>> {
        Box::pin(async move {
            // Track attach state so events() can tag PTY bytes
            // correctly. Single-attach only; matches today's daemon.
            match &verb {
                Control::AttachTab { section_id, tab_id } => {
                    *self.attached.lock().await = Some((section_id.clone(), tab_id.clone()));
                }
                Control::DetachTab => {
                    *self.attached.lock().await = None;
                }
                _ => {}
            }
            // Hello is reserved for the dial path — surface the
            // mismatch instead of routing it through `call`.
            if let Control::Hello { .. } = verb {
                return Err(TransportError::Encoding(
                    "Hello is sent by the dial path, not by call()".into(),
                ));
            }
            self.inner
                .call(verb)
                .await
                .map_err(|e| TransportError::Other(format!("{e:#}")))
        })
    }

    fn push_data<'a>(
        &'a self,
        section_id: &'a str,
        tab_id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        // Tag per-frame with (section_id, tab_id) so the daemon
        // demuxes from the frame, not from its stale attach
        // snapshot (see #138). `send_tab_data` is the tagged
        // counterpart to the legacy `send(bytes)` path.
        Box::pin(async move {
            self.inner
                .send_tab_data(section_id, tab_id, bytes)
                .await
                .map_err(|e| TransportError::Other(format!("{e:#}")))
        })
    }

    fn events(&self) -> EventStream {
        let rx = self.events_rx.lock().expect("events_rx poisoned").take();
        Box::pin(IrohEventStream {
            rx,
            terminated: false,
        })
    }

    fn close<'a>(
        &'a self,
        _reason: Option<&'a str>,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            self.inner.close().await;
            Ok(())
        })
    }
}

// `send_legacy_control` was the per-verb helper-routing layer used
// while the legacy session lacked a typed `call`. With request-id
// correlation in place, `Session::call` handles every variant
// uniformly — keeping a separate routing table here would only
// duplicate the verb match.

/// Stream impl that drains the events mpsc fed by the bridge task
/// `dial` spawned. Yields `SessionEvent::PtyBytes` tagged with the
/// current attach key, then `SessionEvent::Closed` when the bridge
/// shuts down. After yielding `Closed` the stream terminates per the
/// trait contract.
struct IrohEventStream {
    /// `None` means a previous `events()` call already took the
    /// receiver — second consumers get an immediately-terminated
    /// stream. Documented on the trait method.
    rx: Option<mpsc::UnboundedReceiver<SessionEvent>>,
    terminated: bool,
}

impl Stream for IrohEventStream {
    type Item = SessionEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }
        let Some(rx) = self.rx.as_mut() else {
            // Second consumer; nothing to yield ever.
            self.terminated = true;
            return Poll::Ready(None);
        };
        match rx.poll_recv(cx) {
            Poll::Ready(Some(event)) => {
                if matches!(event, SessionEvent::Closed { .. }) {
                    self.terminated = true;
                }
                Poll::Ready(Some(event))
            }
            Poll::Ready(None) => {
                self.terminated = true;
                Poll::Ready(None)
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

/// Convenience: build an `Arc<dyn TransportFactory>` for the iroh
/// transport. Hosts wire this in at startup; everything below this
/// layer takes the trait object and stays network-stack-agnostic.
pub fn iroh_factory() -> Arc<dyn TransportFactory> {
    Arc::new(IrohTransportFactory)
}

/// Re-export of [`DialTarget`] for ergonomics — callers can build
/// pairing-URL targets without reaching across crates.
pub fn pairing_target(url: impl Into<String>) -> DialTarget {
    DialTarget::PairingUrl(url.into())
}

/// Re-export of [`DialTarget::SocketPath`] for ergonomics. Kept here
/// even though the iroh impl rejects it — the trait surface lives in
/// `daemon-transport`, this helper lets callers construct a target
/// without importing the abstract crate too.
pub fn socket_target(path: PathBuf) -> DialTarget {
    DialTarget::SocketPath(path)
}
