//! UDS (Unix Domain Socket) `Transport` impl. Same framing shape as
//! the iroh stack — `[1B type][4B BE length][N B payload]` per frame,
//! with `Control` and `WorkerReply` JSON-encoded as the payload of
//! `TY_CONTROL` / `TY_WORKER_REPLY` frames respectively.
//!
//! ## Why UDS
//!
//! Desktop ↔ MCP shim today is bespoke wiring under
//! `daemon::transport_mcp`; this crate reframes it as "a UDS
//! transport that the daemon happens to dispatch MCP-flavored verbs
//! on." Same Session / ServerSession contract, no MCP-specific
//! plumbing in the daemon.
//!
//! ## What's not here
//!
//! - **Pairing handshake.** UDS is local; trust comes from filesystem
//!   permissions on the socket path. New transports (anything with a
//!   network distance) layer their own auth on top.
//! - **Multiplexing.** One `UdsSession` per stream connection. The
//!   listener side accepts each incoming connection as a separate
//!   `Box<dyn ServerSession>`; the abstract layer above handles
//!   dispatch.

use std::collections::HashMap;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex as StdMutex};
use std::task::{Context as TaskContext, Poll};

use daemon_proto::{
    Control, ControlEnvelope, WorkerReply, WorkerReplyEnvelope, MAX_FRAME_BYTES, PUSH_REQUEST_ID,
    TY_CONTROL, TY_DATA, TY_WORKER_REPLY,
};
use futures_core::Stream;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};
use tracing::{debug, warn};

use crate::{
    DialTarget, EventStream, RequestId, ServerSession, Session, SessionEvent, SessionFuture,
    Transport, TransportError, TransportFactory,
};

// ──────────────────────────────────────────────────────────────────
// Framing helpers (UDS-local; mirrors daemon::frame::{read,write}_frame)
// ──────────────────────────────────────────────────────────────────

async fn write_frame(
    stream: &mut UnixStream,
    ty: u8,
    payload: &[u8],
) -> Result<(), TransportError> {
    let mut header = [0u8; 5];
    header[0] = ty;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    stream
        .write_all(&header)
        .await
        .map_err(|e| TransportError::Other(format!("write header: {e}")))?;
    stream
        .write_all(payload)
        .await
        .map_err(|e| TransportError::Other(format!("write payload: {e}")))?;
    Ok(())
}

async fn read_frame(stream: &mut UnixStream) -> Result<Option<(u8, Vec<u8>)>, TransportError> {
    let mut header = [0u8; 5];
    match stream.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(TransportError::Other(format!("read header: {e}"))),
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(TransportError::Encoding(format!(
            "frame too large: {len} > {MAX_FRAME_BYTES}"
        )));
    }
    let mut payload = vec![0u8; len];
    stream
        .read_exact(&mut payload)
        .await
        .map_err(|e| TransportError::Other(format!("read payload: {e}")))?;
    Ok(Some((ty, payload)))
}

// ──────────────────────────────────────────────────────────────────
// Outbound frame queue (shared shape between client + server halves)
// ──────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct OutboundFrame {
    ty: u8,
    payload: Vec<u8>,
}
type OutboundTx = mpsc::UnboundedSender<OutboundFrame>;
type OutboundRx = mpsc::UnboundedReceiver<OutboundFrame>;

/// Spawn the writer task that drains `outbound_rx` onto the wire.
/// Lives until the channel closes (last sender dropped) or the
/// stream errors.
fn spawn_writer(mut stream_writer: tokio::net::unix::OwnedWriteHalf, mut outbound_rx: OutboundRx) {
    tokio::spawn(async move {
        while let Some(frame) = outbound_rx.recv().await {
            let mut header = [0u8; 5];
            header[0] = frame.ty;
            header[1..5].copy_from_slice(&(frame.payload.len() as u32).to_be_bytes());
            if let Err(e) = stream_writer.write_all(&header).await {
                debug!(error = %e, "uds writer: header failed");
                break;
            }
            if let Err(e) = stream_writer.write_all(&frame.payload).await {
                debug!(error = %e, "uds writer: payload failed");
                break;
            }
        }
        let _ = stream_writer.shutdown().await;
    });
}

// ──────────────────────────────────────────────────────────────────
// Client side
// ──────────────────────────────────────────────────────────────────

/// `TransportFactory` impl that dials a daemon over UDS.
#[derive(Clone, Default)]
pub struct UdsTransportFactory;

impl UdsTransportFactory {
    pub fn new() -> Self {
        Self
    }
}

impl TransportFactory for UdsTransportFactory {
    fn dial<'a>(
        &'a self,
        target: DialTarget,
    ) -> SessionFuture<'a, Result<Box<dyn Session>, TransportError>> {
        Box::pin(async move {
            let path = match target {
                DialTarget::SocketPath(p) => p,
                other => {
                    return Err(TransportError::Connect(format!(
                        "uds transport only handles DialTarget::SocketPath, got {other:?}"
                    )));
                }
            };
            let stream = UnixStream::connect(&path)
                .await
                .map_err(|e| TransportError::Connect(format!("connect {path:?}: {e}")))?;
            let (read_half, write_half) = stream.into_split();
            let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<OutboundFrame>();
            spawn_writer(write_half, outbound_rx);

            let pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<WorkerReply>>>> =
                Arc::new(StdMutex::new(HashMap::new()));
            let (events_tx, events_rx) = mpsc::unbounded_channel::<SessionEvent>();

            let pending_for_reader = Arc::clone(&pending);
            let events_for_reader = events_tx.clone();
            tokio::spawn(client_reader_loop(
                read_half,
                pending_for_reader,
                events_for_reader,
            ));

            let session = UdsSession {
                outbound_tx,
                pending,
                next_request_id: AtomicU64::new(1),
                events_rx: StdMutex::new(Some(events_rx)),
            };
            Ok(Box::new(session) as Box<dyn Session>)
        })
    }
}

async fn client_reader_loop(
    mut read_half: tokio::net::unix::OwnedReadHalf,
    pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<WorkerReply>>>>,
    events_tx: mpsc::UnboundedSender<SessionEvent>,
) {
    loop {
        let frame = match read_one_frame(&mut read_half).await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                debug!(error = ?e, "uds client reader: frame error");
                break;
            }
        };
        match frame.0 {
            TY_DATA => {
                // Server→client: decode the #138 (section_id,
                // tab_id) tag from the frame payload so consumers
                // get correctly-demuxed PtyBytes events. Legacy
                // untagged frames fall through to empty tags, same
                // shape the pre-tagging code emitted — consumers
                // that don't care (single-attach) keep working.
                if let Some((section_id, tab_id, body)) = daemon_proto::decode_pty_data(&frame.1) {
                    let _ = events_tx.send(SessionEvent::PtyBytes {
                        section_id,
                        tab_id,
                        bytes: body,
                    });
                } else {
                    let _ = events_tx.send(SessionEvent::PtyBytes {
                        section_id: String::new(),
                        tab_id: String::new(),
                        bytes: frame.1,
                    });
                }
            }
            TY_WORKER_REPLY => {
                let envelope: WorkerReplyEnvelope = match serde_json::from_slice(&frame.1) {
                    Ok(e) => e,
                    Err(e) => {
                        warn!(error = %e, "uds: bad worker reply envelope");
                        continue;
                    }
                };
                let routed = pending
                    .lock()
                    .expect("pending poisoned")
                    .remove(&envelope.request_id);
                if let Some(tx) = routed {
                    let _ = tx.send(envelope.reply);
                } else if envelope.request_id == PUSH_REQUEST_ID {
                    let _ = events_tx.send(SessionEvent::Push(envelope.reply));
                } else {
                    debug!(
                        request_id = envelope.request_id,
                        "uds: orphan worker reply (no pending awaiter)"
                    );
                }
            }
            other => debug!(ty = other, "uds: unhandled frame type"),
        }
    }
    let _ = events_tx.send(SessionEvent::Closed { reason: None });
}

async fn read_one_frame(
    read_half: &mut tokio::net::unix::OwnedReadHalf,
) -> Result<Option<(u8, Vec<u8>)>, TransportError> {
    let mut header = [0u8; 5];
    match read_half.read_exact(&mut header).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(TransportError::Other(format!("read header: {e}"))),
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if len > MAX_FRAME_BYTES {
        return Err(TransportError::Encoding(format!(
            "frame too large: {len} > {MAX_FRAME_BYTES}"
        )));
    }
    let mut payload = vec![0u8; len];
    read_half
        .read_exact(&mut payload)
        .await
        .map_err(|e| TransportError::Other(format!("read payload: {e}")))?;
    Ok(Some((ty, payload)))
}

struct UdsSession {
    outbound_tx: OutboundTx,
    pending: Arc<StdMutex<HashMap<u64, oneshot::Sender<WorkerReply>>>>,
    next_request_id: AtomicU64,
    events_rx: StdMutex<Option<mpsc::UnboundedReceiver<SessionEvent>>>,
}

impl Session for UdsSession {
    fn call<'a>(&'a self, verb: Control) -> SessionFuture<'a, Result<WorkerReply, TransportError>> {
        Box::pin(async move {
            let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
            let (tx, rx) = oneshot::channel();
            self.pending
                .lock()
                .expect("pending poisoned")
                .insert(request_id, tx);
            let envelope = ControlEnvelope {
                request_id,
                control: verb,
            };
            let payload = serde_json::to_vec(&envelope)
                .map_err(|e| TransportError::Encoding(format!("encode control: {e}")))?;
            self.outbound_tx
                .send(OutboundFrame {
                    ty: TY_CONTROL,
                    payload,
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
        // Client→server data (user input into the attached tab).
        // Tag with (section_id, tab_id) so the server routes by the
        // frame's own label instead of its stale attach snapshot;
        // prevents the mid-stream-attach-switch race described at
        // #138.
        let payload = daemon_proto::encode_pty_data(section_id, tab_id, bytes);
        Box::pin(async move {
            self.outbound_tx
                .send(OutboundFrame {
                    ty: TY_DATA,
                    payload,
                })
                .map_err(|_| TransportError::Closed(Some("server dropped".into())))
        })
    }

    fn events(&self) -> EventStream {
        let rx = self.events_rx.lock().expect("events_rx poisoned").take();
        Box::pin(UdsEventStream {
            rx,
            terminated: false,
        })
    }

    fn close<'a>(
        &'a self,
        _reason: Option<&'a str>,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        // Dropping the outbound_tx clone the writer task holds is
        // what closes the wire. Explicit close is a no-op; the
        // contract permits it.
        Box::pin(async move { Ok(()) })
    }
}

struct UdsEventStream {
    rx: Option<mpsc::UnboundedReceiver<SessionEvent>>,
    terminated: bool,
}

impl Stream for UdsEventStream {
    type Item = SessionEvent;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<Option<Self::Item>> {
        if self.terminated {
            return Poll::Ready(None);
        }
        let Some(rx) = self.rx.as_mut() else {
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

// ──────────────────────────────────────────────────────────────────
// Server side
// ──────────────────────────────────────────────────────────────────

/// Server-side `Transport`. Binds a `UnixListener` at the supplied
/// path; each `accept()` yields one `Box<dyn ServerSession>` per
/// inbound connection.
pub struct UdsTransport {
    listener: UnixListener,
    path: PathBuf,
    next_call_id: Arc<AtomicU64>,
}

impl UdsTransport {
    /// Bind a UDS listener at `path`. Removes any pre-existing
    /// socket at the path (matches today's daemon::transport_mcp
    /// behaviour for crashed-prior-instance recovery; new transports
    /// can layer their own retry-on-bind policy).
    pub fn bind(path: impl Into<PathBuf>) -> Result<Self, TransportError> {
        let path = path.into();
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)
            .map_err(|e| TransportError::Other(format!("bind {path:?}: {e}")))?;
        Ok(Self {
            listener,
            path,
            next_call_id: Arc::new(AtomicU64::new(1)),
        })
    }

    pub fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for UdsTransport {
    fn drop(&mut self) {
        // Best-effort cleanup so a crashed daemon doesn't leave a
        // stale socket. Production daemon should also wire the
        // panic-hook + signal shutdown patterns from
        // daemon::transport_mcp::install_cleanup_hooks.
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Transport for UdsTransport {
    fn accept(
        &mut self,
    ) -> SessionFuture<'_, Result<Option<Box<dyn ServerSession>>, TransportError>> {
        Box::pin(async move {
            let (stream, _addr) = self
                .listener
                .accept()
                .await
                .map_err(|e| TransportError::Other(format!("accept: {e}")))?;
            let (read_half, write_half) = stream.into_split();
            let (outbound_tx, outbound_rx) = mpsc::unbounded_channel::<OutboundFrame>();
            spawn_writer(write_half, outbound_rx);

            let (call_tx, call_rx) = mpsc::unbounded_channel::<(RequestId, Control)>();
            // peer_id: pid+uid would be ideal here but tokio's UnixStream
            // doesn't expose SO_PEERCRED out of the box. Fall back to
            // a per-connection counter for now — concrete consumers
            // that need real identity wire it up via libc::getsockopt
            // in a follow-up.
            let connection_id = self.next_call_id.fetch_add(1, Ordering::Relaxed);
            let peer_id = format!("uds:conn-{connection_id}");

            tokio::spawn(server_reader_loop(read_half, call_tx));

            let session = UdsServerSession {
                peer_id,
                outbound_tx,
                call_rx: tokio::sync::Mutex::new(call_rx),
            };
            Ok(Some(Box::new(session) as Box<dyn ServerSession>))
        })
    }
}

async fn server_reader_loop(
    mut read_half: tokio::net::unix::OwnedReadHalf,
    call_tx: mpsc::UnboundedSender<(RequestId, Control)>,
) {
    loop {
        let frame = match read_one_frame(&mut read_half).await {
            Ok(Some(f)) => f,
            Ok(None) => break,
            Err(e) => {
                debug!(error = ?e, "uds server reader: frame error");
                break;
            }
        };
        match frame.0 {
            TY_CONTROL => {
                let envelope: ControlEnvelope = match serde_json::from_slice(&frame.1) {
                    Ok(e) => e,
                    Err(e) => {
                        warn!(error = %e, "uds: bad control envelope");
                        continue;
                    }
                };
                if call_tx
                    .send((RequestId(envelope.request_id), envelope.control))
                    .is_err()
                {
                    break;
                }
            }
            TY_DATA => {
                // Client→server input arriving here. Today no
                // UDS-backed server handler consumes it (the dispatch
                // layer has its own path), so we drop on the floor;
                // decoding + logging the tag for visibility when
                // it's eventually wired up.
                if let Some((section_id, tab_id, body)) = daemon_proto::decode_pty_data(&frame.1) {
                    debug!(
                        section_id,
                        tab_id,
                        bytes = body.len(),
                        "uds: dropped inbound TY_DATA"
                    );
                } else {
                    debug!(
                        bytes = frame.1.len(),
                        "uds: dropped inbound TY_DATA (untagged)"
                    );
                }
            }
            other => debug!(ty = other, "uds: unhandled inbound frame type"),
        }
    }
}

struct UdsServerSession {
    peer_id: String,
    outbound_tx: OutboundTx,
    call_rx: tokio::sync::Mutex<mpsc::UnboundedReceiver<(RequestId, Control)>>,
}

impl ServerSession for UdsServerSession {
    fn peer_id(&self) -> &str {
        &self.peer_id
    }

    fn next_call<'a>(
        &'a self,
    ) -> SessionFuture<'a, Result<Option<(RequestId, Control)>, TransportError>> {
        Box::pin(async move {
            let mut rx = self.call_rx.lock().await;
            Ok(rx.recv().await)
        })
    }

    fn reply<'a>(
        &'a self,
        request_id: RequestId,
        reply: WorkerReply,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            let envelope = WorkerReplyEnvelope {
                request_id: request_id.0,
                reply,
            };
            let payload = serde_json::to_vec(&envelope)
                .map_err(|e| TransportError::Encoding(format!("encode reply: {e}")))?;
            self.outbound_tx
                .send(OutboundFrame {
                    ty: TY_WORKER_REPLY,
                    payload,
                })
                .map_err(|_| TransportError::Closed(Some("client dropped".into())))
        })
    }

    fn push_data<'a>(
        &'a self,
        section_id: &'a str,
        tab_id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        // Server→client push for the UDS transport. Same #138 tag
        // shape as the iroh server push so the client demux is
        // transport-independent.
        let payload = daemon_proto::encode_pty_data(section_id, tab_id, bytes);
        Box::pin(async move {
            self.outbound_tx
                .send(OutboundFrame {
                    ty: TY_DATA,
                    payload,
                })
                .map_err(|_| TransportError::Closed(Some("client dropped".into())))
        })
    }

    fn push_reply<'a>(
        &'a self,
        reply: WorkerReply,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        Box::pin(async move {
            let envelope = WorkerReplyEnvelope {
                request_id: PUSH_REQUEST_ID,
                reply,
            };
            let payload = serde_json::to_vec(&envelope)
                .map_err(|e| TransportError::Encoding(format!("encode push reply: {e}")))?;
            self.outbound_tx
                .send(OutboundFrame {
                    ty: TY_WORKER_REPLY,
                    payload,
                })
                .map_err(|_| TransportError::Closed(Some("client dropped".into())))
        })
    }

    fn close<'a>(
        &'a self,
        _reason: Option<&'a [u8]>,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        Box::pin(async move { Ok(()) })
    }
}

// `write_frame` / `read_frame` are reserved for future direct use
// (e.g. a synchronous handshake step before the writer task spins
// up). They're kept here even though the connection-side code uses
// the OutboundTx queue exclusively today.
#[allow(dead_code)]
fn _keep_alive() {
    let _ = (write_frame, read_frame);
}

#[cfg(test)]
mod tests {
    use super::*;
    use daemon_proto::{ErrKind, ProjectSummary};

    #[tokio::test]
    async fn uds_round_trips_a_call() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sock");

        let mut transport = UdsTransport::bind(&path).expect("bind");

        let factory = UdsTransportFactory::new();
        let dial_path = path.clone();
        let client_task = tokio::spawn(async move {
            factory
                .dial(DialTarget::SocketPath(dial_path))
                .await
                .expect("dial")
        });

        let server_session = transport
            .accept()
            .await
            .expect("accept ok")
            .expect("session");
        let client = client_task.await.expect("client task");

        // Tiny daemon-shaped task on the server side.
        let server_task = tokio::spawn(async move {
            let (id, verb) = server_session
                .next_call()
                .await
                .expect("recv ok")
                .expect("call before close");
            assert!(matches!(verb, Control::ListProjects));
            server_session
                .reply(
                    id,
                    WorkerReply::ProjectList {
                        projects: vec![ProjectSummary {
                            id: "p1".into(),
                            name: "p1".into(),
                            path: "/tmp/p1".into(),
                            current_branch: Some("main".into()),
                            ..Default::default()
                        }],
                        repos: vec![],
                        ui: Default::default(),
                    },
                )
                .await
                .expect("reply");
        });

        let reply = client.call(Control::ListProjects).await.expect("call");
        match reply {
            WorkerReply::ProjectList { projects, .. } => {
                assert_eq!(projects.len(), 1);
                assert_eq!(projects[0].id, "p1");
            }
            other => panic!("expected ProjectList, got {other:?}"),
        }

        server_task.await.unwrap();
    }

    #[tokio::test]
    async fn uds_concurrent_calls_correlate() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("test.sock");

        let mut transport = UdsTransport::bind(&path).expect("bind");
        let factory = UdsTransportFactory::new();
        let dial_path = path.clone();
        let client_task = tokio::spawn(async move {
            factory
                .dial(DialTarget::SocketPath(dial_path))
                .await
                .expect("dial")
        });
        let server_session = transport.accept().await.unwrap().unwrap();
        let client = Arc::new(client_task.await.unwrap());

        let server_task = tokio::spawn(async move {
            let (id1, _) = server_session.next_call().await.unwrap().unwrap();
            let (id2, _) = server_session.next_call().await.unwrap().unwrap();
            // Reply id2 first.
            server_session
                .reply(
                    id2,
                    WorkerReply::ProjectList {
                        projects: vec![],
                        repos: vec![],
                        ui: Default::default(),
                    },
                )
                .await
                .unwrap();
            server_session
                .reply(
                    id1,
                    WorkerReply::Err {
                        kind: ErrKind::Internal,
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
        assert!(matches!(r1, WorkerReply::Err { .. }));
        assert!(matches!(r2, WorkerReply::ProjectList { .. }));
        server_task.await.unwrap();
    }
}
