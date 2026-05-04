//! In-memory `Transport` impl — paired client / server sessions
//! talking through a tokio channel pair. No iroh, no UDS, no
//! framing. Used for tests that exercise the abstract verb layer
//! without spinning up a real network stack, and as the second
//! concrete impl that proves the trait surface isn't iroh-shaped by
//! accident.
//!
//! ## Topology
//!
//! ```text
//!     ┌───────────────┐                    ┌───────────────────┐
//!     │ InMemorySession│ <- replies <-- + push -- │ InMemoryServerSession│
//!     │   (client)     │ -- calls + push_data --> │   (server)            │
//!     └───────────────┘                    └───────────────────┘
//!         ▲                                          ▲
//!     dial()                                    accept()
//! ```
//!
//! The two halves share two unbounded mpsc channels:
//!   - `client → server` carries `(RequestId, Control)` and raw push
//!     bytes.
//!   - `server → client` carries `(RequestId, WorkerReply)` and raw
//!     PTY bytes the server pushes back.
//!
//! Pairing happens at construction time via [`pair`]: callers get
//! a matched `(InMemoryServerSession, InMemorySession)` tuple. For
//! a multi-session test harness, [`InMemoryTransport`] +
//! [`InMemoryTransportFactory`] coordinate via a shared `accept`
//! queue keyed by name.

use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll};

use daemon_proto::{Control, WorkerReply};
use futures_core::Stream;
use tokio::sync::{mpsc, oneshot, Mutex as AsyncMutex};

use crate::{
    DialTarget, EventStream, RequestId, ServerSession, Session, SessionEvent, SessionFuture,
    Transport, TransportError, TransportFactory,
};

// ──────────────────────────────────────────────────────────────────
// Wire shapes (in-process; no encoding)
// ──────────────────────────────────────────────────────────────────

/// What a client sends to the server. No envelope encoding — we hand
/// over typed values. The transport invariant (correlate replies via
/// RequestId) is preserved by carrying the id in-band.
enum ClientFrame {
    Call {
        request_id: RequestId,
        control: Control,
    },
    /// Raw client → server bytes for an attached tab. Today's
    /// abstract surface routes these through `Session::push_data`;
    /// concrete consumers don't read them yet (no daemon verb in
    /// the current set requires it). Kept as a variant so the
    /// in-memory transport can demultiplex when a consumer arrives.
    #[allow(dead_code)]
    PushData {
        section_id: String,
        tab_id: String,
        bytes: Vec<u8>,
    },
}

/// What a server sends to the client.
enum ServerFrame {
    Reply {
        request_id: RequestId,
        reply: WorkerReply,
    },
    /// Daemon-pushed reply (broadcast / future verbs). The trait
    /// surface translates this into [`SessionEvent::Push`].
    PushReply(WorkerReply),
    /// PTY bytes for an attached tab. Tagged with `(section_id,
    /// tab_id)` so [`Session::events`] can demultiplex.
    PtyBytes {
        section_id: String,
        tab_id: String,
        bytes: Vec<u8>,
    },
    /// The server is closing the session, optionally with a reason.
    Closed { reason: Option<String> },
}

// ──────────────────────────────────────────────────────────────────
// Direct pair (the simple, no-discovery API)
// ──────────────────────────────────────────────────────────────────

/// Build a matched pair of in-memory sessions. Returns `(server,
/// client)` — pass `client` to whatever needs a [`Session`] and
/// `server` to whatever drives a [`ServerSession`].
///
/// The pair is independent of any transport / factory machinery. Use
/// it for unit tests that just need both halves wired up without
/// the discovery dance.
pub fn pair(peer_id: impl Into<String>) -> (Box<dyn ServerSession>, Box<dyn Session>) {
    let peer_id = peer_id.into();
    let (c2s_tx, c2s_rx) = mpsc::unbounded_channel::<ClientFrame>();
    let (s2c_tx, s2c_rx) = mpsc::unbounded_channel::<ServerFrame>();

    let pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<WorkerReply>>>> =
        Arc::new(Mutex::new(HashMap::new()));
    let event_buf: Arc<Mutex<VecDeque<SessionEvent>>> = Arc::new(Mutex::new(VecDeque::new()));
    let event_waker: Arc<Mutex<Option<std::task::Waker>>> = Arc::new(Mutex::new(None));
    let session_closed = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Spawn the recv-side router. Lives until the s2c channel closes
    // (i.e. the server dropped its sender) — at that point we yield
    // SessionEvent::Closed and shut down.
    let pending_clone = Arc::clone(&pending);
    let event_buf_clone = Arc::clone(&event_buf);
    let event_waker_clone = Arc::clone(&event_waker);
    let session_closed_clone = Arc::clone(&session_closed);
    tokio::spawn(async move {
        let mut s2c_rx = s2c_rx;
        while let Some(frame) = s2c_rx.recv().await {
            match frame {
                ServerFrame::Reply { request_id, reply } => {
                    let routed = pending_clone
                        .lock()
                        .expect("pending poisoned")
                        .remove(&request_id);
                    if let Some(tx) = routed {
                        let _ = tx.send(reply);
                    } else {
                        // Reply with no awaiter — surface as a Push
                        // so the events stream sees it. Matches the
                        // spirit of the iroh impl's
                        // request_id-fallthrough behaviour.
                        push_event(
                            &event_buf_clone,
                            &event_waker_clone,
                            SessionEvent::Push(reply),
                        );
                    }
                }
                ServerFrame::PushReply(reply) => {
                    push_event(
                        &event_buf_clone,
                        &event_waker_clone,
                        SessionEvent::Push(reply),
                    );
                }
                ServerFrame::PtyBytes {
                    section_id,
                    tab_id,
                    bytes,
                } => {
                    push_event(
                        &event_buf_clone,
                        &event_waker_clone,
                        SessionEvent::PtyBytes {
                            section_id,
                            tab_id,
                            bytes,
                        },
                    );
                }
                ServerFrame::Closed { reason } => {
                    session_closed_clone.store(true, std::sync::atomic::Ordering::Release);
                    push_event(
                        &event_buf_clone,
                        &event_waker_clone,
                        SessionEvent::Closed { reason },
                    );
                    break;
                }
            }
        }
        // Channel closed without an explicit Closed frame —
        // synthesise one so consumers terminate.
        if !session_closed_clone.load(std::sync::atomic::Ordering::Acquire) {
            session_closed_clone.store(true, std::sync::atomic::Ordering::Release);
            push_event(
                &event_buf_clone,
                &event_waker_clone,
                SessionEvent::Closed {
                    reason: Some("server dropped".into()),
                },
            );
        }
    });

    let next_request_id = Arc::new(std::sync::atomic::AtomicU64::new(1));

    let client = InMemorySession {
        c2s_tx,
        pending,
        event_buf,
        event_waker,
        session_closed,
        next_request_id,
    };
    let server = InMemoryServerSession {
        peer_id,
        c2s_rx: AsyncMutex::new(c2s_rx),
        s2c_tx: AsyncMutex::new(Some(s2c_tx)),
    };
    (
        Box::new(server) as Box<dyn ServerSession>,
        Box::new(client) as Box<dyn Session>,
    )
}

fn push_event(
    buf: &Mutex<VecDeque<SessionEvent>>,
    waker_slot: &Mutex<Option<std::task::Waker>>,
    event: SessionEvent,
) {
    buf.lock().expect("event_buf poisoned").push_back(event);
    if let Some(w) = waker_slot.lock().expect("waker poisoned").take() {
        w.wake();
    }
}

// ──────────────────────────────────────────────────────────────────
// Client side
// ──────────────────────────────────────────────────────────────────

struct InMemorySession {
    c2s_tx: mpsc::UnboundedSender<ClientFrame>,
    pending: Arc<Mutex<HashMap<RequestId, oneshot::Sender<WorkerReply>>>>,
    event_buf: Arc<Mutex<VecDeque<SessionEvent>>>,
    event_waker: Arc<Mutex<Option<std::task::Waker>>>,
    session_closed: Arc<std::sync::atomic::AtomicBool>,
    next_request_id: Arc<std::sync::atomic::AtomicU64>,
}

impl Session for InMemorySession {
    fn call<'a>(&'a self, verb: Control) -> SessionFuture<'a, Result<WorkerReply, TransportError>> {
        Box::pin(async move {
            let request_id = RequestId(
                self.next_request_id
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            );
            let (tx, rx) = oneshot::channel();
            self.pending
                .lock()
                .expect("pending poisoned")
                .insert(request_id, tx);
            self.c2s_tx
                .send(ClientFrame::Call {
                    request_id,
                    control: verb,
                })
                .map_err(|_| TransportError::Closed(Some("server dropped".into())))?;
            rx.await
                .map_err(|_| TransportError::Closed(Some("reply channel dropped".into())))
        })
    }

    fn push_data<'a>(
        &'a self,
        section_id: &'a str,
        tab_id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        let frame = ClientFrame::PushData {
            section_id: section_id.to_string(),
            tab_id: tab_id.to_string(),
            bytes: bytes.to_vec(),
        };
        Box::pin(async move {
            self.c2s_tx
                .send(frame)
                .map_err(|_| TransportError::Closed(Some("server dropped".into())))
        })
    }

    fn events(&self) -> EventStream {
        Box::pin(InMemoryEventStream {
            buf: Arc::clone(&self.event_buf),
            waker: Arc::clone(&self.event_waker),
            closed: Arc::clone(&self.session_closed),
            terminated: false,
        })
    }

    fn close<'a>(
        &'a self,
        _reason: Option<&'a str>,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        // Drop the c2s sender so the server's recv loop exits. We
        // can't actually drop a field from `&self`, but the server
        // sees Closed when its s2c_tx returns SendError, which
        // happens when this session is dropped. The explicit
        // close() method is therefore a no-op for the in-memory
        // transport — the trait contract permits it (idempotent,
        // dropping the session also closes it).
        Box::pin(async move { Ok(()) })
    }
}

struct InMemoryEventStream {
    buf: Arc<Mutex<VecDeque<SessionEvent>>>,
    waker: Arc<Mutex<Option<std::task::Waker>>>,
    closed: Arc<std::sync::atomic::AtomicBool>,
    /// Set after we yield `SessionEvent::Closed` so subsequent polls
    /// return Ready(None) per the trait contract.
    terminated: bool,
}

impl Stream for InMemoryEventStream {
    type Item = SessionEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }
        let next = self.buf.lock().expect("event_buf poisoned").pop_front();
        if let Some(event) = next {
            if matches!(event, SessionEvent::Closed { .. }) {
                self.terminated = true;
            }
            return Poll::Ready(Some(event));
        }
        // No buffered event. If the session is closed and the buffer
        // was empty, terminate the stream.
        if self.closed.load(std::sync::atomic::Ordering::Acquire) {
            self.terminated = true;
            return Poll::Ready(None);
        }
        *self.waker.lock().expect("waker poisoned") = Some(cx.waker().clone());
        Poll::Pending
    }
}

// ──────────────────────────────────────────────────────────────────
// Server side
// ──────────────────────────────────────────────────────────────────

struct InMemoryServerSession {
    peer_id: String,
    c2s_rx: AsyncMutex<mpsc::UnboundedReceiver<ClientFrame>>,
    /// `None` after `close()` so subsequent reply / push attempts
    /// fail rather than silently hanging.
    s2c_tx: AsyncMutex<Option<mpsc::UnboundedSender<ServerFrame>>>,
}

impl ServerSession for InMemoryServerSession {
    fn peer_id(&self) -> &str {
        &self.peer_id
    }

    fn next_call<'a>(
        &'a self,
    ) -> SessionFuture<'a, Result<Option<(RequestId, Control)>, TransportError>> {
        Box::pin(async move {
            loop {
                let mut rx = self.c2s_rx.lock().await;
                let Some(frame) = rx.recv().await else {
                    return Ok(None);
                };
                match frame {
                    ClientFrame::Call {
                        request_id,
                        control,
                    } => return Ok(Some((request_id, control))),
                    ClientFrame::PushData { .. } => {
                        // Push data isn't a "call" — it's input the
                        // peer streams without expecting a reply.
                        // Concrete daemons currently treat raw PTY
                        // input as a side-channel; the abstract
                        // surface returns from next_call only on
                        // Control verbs. Push-data is observable on
                        // the server via [`Self::take_push_data`]
                        // (added when a real consumer needs it).
                        // For now we drop on the floor — no daemon
                        // verb in today's set requires it.
                        continue;
                    }
                }
            }
        })
    }

    fn reply<'a>(
        &'a self,
        request_id: RequestId,
        reply: WorkerReply,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            let tx = self.s2c_tx.lock().await;
            let Some(tx) = tx.as_ref() else {
                return Err(TransportError::Closed(Some("session closed".into())));
            };
            tx.send(ServerFrame::Reply { request_id, reply })
                .map_err(|_| TransportError::Closed(Some("client dropped".into())))
        })
    }

    fn push_data<'a>(
        &'a self,
        section_id: &'a str,
        tab_id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        let frame = ServerFrame::PtyBytes {
            section_id: section_id.to_string(),
            tab_id: tab_id.to_string(),
            bytes: bytes.to_vec(),
        };
        Box::pin(async move {
            let tx = self.s2c_tx.lock().await;
            let Some(tx) = tx.as_ref() else {
                return Err(TransportError::Closed(Some("session closed".into())));
            };
            tx.send(frame)
                .map_err(|_| TransportError::Closed(Some("client dropped".into())))
        })
    }

    fn push_reply<'a>(
        &'a self,
        reply: WorkerReply,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            let tx = self.s2c_tx.lock().await;
            let Some(tx) = tx.as_ref() else {
                return Err(TransportError::Closed(Some("session closed".into())));
            };
            tx.send(ServerFrame::PushReply(reply))
                .map_err(|_| TransportError::Closed(Some("client dropped".into())))
        })
    }

    fn close<'a>(
        &'a self,
        reason: Option<&'a [u8]>,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        let reason_str = reason.and_then(|r| std::str::from_utf8(r).ok().map(str::to_string));
        Box::pin(async move {
            let mut tx_slot = self.s2c_tx.lock().await;
            if let Some(tx) = tx_slot.take() {
                let _ = tx.send(ServerFrame::Closed { reason: reason_str });
            }
            Ok(())
        })
    }
}

// ──────────────────────────────────────────────────────────────────
// Transport / TransportFactory (named-pair discovery)
// ──────────────────────────────────────────────────────────────────

/// Shared registry coordinating named in-memory pairs. Hosts a queue
/// of pending server-side sessions per name; client `dial`s match
/// against the queue. Process-wide singleton in test code by
/// convention — keep one per harness so the names don't collide
/// across tests.
#[derive(Clone, Default)]
pub struct InMemoryDirectory {
    // Coordination state lives on the factory's `transports` map for
    // now — the directory is currently a marker / future
    // extensibility point (per-name auth, peer ids). Kept on the
    // public API so adding state here later doesn't churn callers.
    #[allow(dead_code)]
    inner: Arc<Mutex<DirectoryInner>>,
}

#[derive(Default)]
#[allow(dead_code)]
struct DirectoryInner {
    /// Each name has a queue of waiting client `dial` halves —
    /// reserved for the timed-pairing path that hasn't materialised
    /// yet.
    waiting: HashMap<String, VecDeque<oneshot::Sender<Box<dyn Session>>>>,
}

impl InMemoryDirectory {
    pub fn new() -> Self {
        Self::default()
    }
}

/// Server-side `Transport`. Pulls server halves of named pairs as
/// clients dial them.
pub struct InMemoryTransport {
    name: String,
    /// Reserved for future cross-transport coordination. Kept on
    /// the constructor signature so registering shared policy
    /// (auth, rate limits) later doesn't churn callers.
    #[allow(dead_code)]
    directory: InMemoryDirectory,
    /// Pending server-side sessions waiting for accept(). Pushed
    /// when a client dials; popped on accept().
    pending_servers: Arc<Mutex<VecDeque<Box<dyn ServerSession>>>>,
    /// Notified when a new pending session lands so accept() can
    /// wake from its wait.
    notify: Arc<tokio::sync::Notify>,
}

impl InMemoryTransport {
    /// Bind a transport to `name`. Clients dialling
    /// `DialTarget::InProcess(name)` against a matching factory will
    /// land in this transport's accept queue.
    pub fn bind(name: impl Into<String>, directory: InMemoryDirectory) -> Self {
        Self {
            name: name.into(),
            directory,
            pending_servers: Arc::new(Mutex::new(VecDeque::new())),
            notify: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Push a paired server half onto the queue, returning the
    /// matching client half. Used by the factory's `dial` path.
    fn enqueue_pair(&self) -> Box<dyn Session> {
        let (server, client) = pair(format!("inproc:{}", self.name));
        self.pending_servers
            .lock()
            .expect("pending poisoned")
            .push_back(server);
        self.notify.notify_one();
        client
    }
}

impl Transport for InMemoryTransport {
    fn accept(
        &mut self,
    ) -> SessionFuture<'_, Result<Option<Box<dyn ServerSession>>, TransportError>> {
        Box::pin(async move {
            loop {
                {
                    let mut q = self.pending_servers.lock().expect("pending poisoned");
                    if let Some(s) = q.pop_front() {
                        return Ok(Some(s));
                    }
                }
                self.notify.notified().await;
            }
        })
    }
}

/// Client-side factory. Hosts a directory shared with one or more
/// `InMemoryTransport`s (matched by name).
#[derive(Clone)]
pub struct InMemoryTransportFactory {
    /// Coordination state for future timed-pairing semantics. The
    /// `transports` map below is what dials currently route through;
    /// keep `directory` on the surface so it doesn't churn when
    /// per-name policy lands.
    #[allow(dead_code)]
    directory: InMemoryDirectory,
    /// Named transports we know about. The factory needs a direct
    /// handle so it can call `enqueue_pair` synchronously — there's
    /// no real network to dial through.
    transports: Arc<Mutex<HashMap<String, Arc<Mutex<InMemoryTransport>>>>>,
}

impl InMemoryTransportFactory {
    pub fn new(directory: InMemoryDirectory) -> Self {
        Self {
            directory,
            transports: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register a transport so dials by name can find it. Returns
    /// the same transport handle so the host can drive `accept`.
    pub fn register(&self, transport: InMemoryTransport) -> Arc<Mutex<InMemoryTransport>> {
        let name = transport.name.clone();
        let handle = Arc::new(Mutex::new(transport));
        self.transports
            .lock()
            .expect("transports poisoned")
            .insert(name, Arc::clone(&handle));
        handle
    }
}

impl TransportFactory for InMemoryTransportFactory {
    fn dial<'a>(
        &'a self,
        target: DialTarget,
    ) -> SessionFuture<'a, Result<Box<dyn Session>, TransportError>> {
        Box::pin(async move {
            let name = match target {
                DialTarget::InProcess(name) => name,
                other => {
                    return Err(TransportError::Connect(format!(
                        "in-memory transport only handles DialTarget::InProcess, got {other:?}"
                    )))
                }
            };
            let transport = {
                let map = self.transports.lock().expect("transports poisoned");
                map.get(&name).cloned()
            };
            let Some(transport) = transport else {
                return Err(TransportError::Connect(format!(
                    "no in-memory transport registered for {name:?}"
                )));
            };
            // `directory` is currently unused as a coordination
            // primitive — `transports` map covers what tests need.
            // Kept on the factory because an upcoming step (timed
            // pairing, peer-id allocation) will key off it.
            let _ = &self.directory;
            let client = transport.lock().expect("transport poisoned").enqueue_pair();
            Ok(client)
        })
    }
}

// ──────────────────────────────────────────────────────────────────
// Tests
// ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use daemon_proto::{Control, WorkerReply};

    #[tokio::test]
    async fn pair_round_trips_a_call() {
        let (server, client) = pair("test-peer");

        // Spawn a tiny daemon-shaped task that handles one call.
        let server_task = tokio::spawn(async move {
            let (id, verb) = server
                .next_call()
                .await
                .expect("recv call")
                .expect("call before close");
            assert!(matches!(verb, Control::ListProjects));
            server
                .reply(id, WorkerReply::ProjectList { projects: vec![] })
                .await
                .expect("reply");
        });

        let reply = client.call(Control::ListProjects).await.expect("call");
        assert!(matches!(reply, WorkerReply::ProjectList { .. }));
        server_task.await.expect("server task");
    }

    #[tokio::test]
    async fn pair_concurrent_calls_correlate() {
        let (server, client) = pair("test-peer");
        let client = Arc::new(client);

        // Server replies in REVERSE order to ensure the client
        // routes by request_id, not by recv-order.
        let server_task = tokio::spawn(async move {
            let (id1, _) = server.next_call().await.unwrap().unwrap();
            let (id2, _) = server.next_call().await.unwrap().unwrap();
            // Reply to id2 FIRST.
            server
                .reply(id2, WorkerReply::ProjectList { projects: vec![] })
                .await
                .unwrap();
            // Then id1.
            server
                .reply(
                    id1,
                    WorkerReply::Err {
                        kind: daemon_proto::ErrKind::Internal,
                        message: "first".into(),
                    },
                )
                .await
                .unwrap();
        });

        let c1 = Arc::clone(&client);
        let h1 = tokio::spawn(async move { c1.call(Control::ListProjects).await });
        let c2 = Arc::clone(&client);
        let h2 = tokio::spawn(async move { c2.call(Control::ListProjects).await });

        let r1 = h1.await.unwrap().expect("c1");
        let r2 = h2.await.unwrap().expect("c2");
        // c1 sent first → got the Err. c2 sent second → got the
        // ProjectList. If correlation were broken we'd see them
        // swapped.
        assert!(
            matches!(r1, WorkerReply::Err { .. }),
            "c1 should receive the err keyed to its id"
        );
        assert!(
            matches!(r2, WorkerReply::ProjectList { .. }),
            "c2 should receive the project list keyed to its id"
        );
        server_task.await.unwrap();
    }
}
