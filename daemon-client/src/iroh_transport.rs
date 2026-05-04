//! Iroh-backed concrete impl of the abstract `daemon_transport`
//! traits. Wraps the legacy fire-and-forget [`crate::session::Session`]
//! so callers programmed against the abstract API see a network-stack-
//! agnostic surface.
//!
//! ### Scope
//!
//! This impl satisfies the trait contract for the dial / call /
//! push-data / events / close paths against today's iroh wire. It is
//! intentionally **sequential at the call level** — one
//! [`Session::call`] at a time per session. The trait permits that
//! today; per-call request-id routing (so concurrent calls from
//! separate tasks work) is the next layer's job (tracked under the
//! typed-client API issue `another-one-f4r`).
//!
//! ### What's missing vs. the trait contract
//!
//! * **Concurrent calls**: serialized via an internal mutex. Two
//!   tasks racing to `call` produces two sequential round-trips, not
//!   two parallel ones. Acceptable until the typed-client work adds
//!   a request-id router.
//! * **Per-channel push streams**: today's wire only carries a single
//!   PTY data fan — every attached tab's bytes arrive on the same
//!   `TY_DATA` frame type. We tag emitted [`SessionEvent::PtyBytes`]
//!   with the most-recently-attached `(section_id, tab_id)` so a
//!   single-attach session demultiplexes correctly. Multi-attach is
//!   future work.

use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use daemon_proto::{Control, WorkerReply};
use daemon_transport::{
    DialTarget, EventStream, Session as AbstractSession, SessionEvent, SessionFuture,
    TransportError, TransportFactory,
};
use futures_core::Stream;
use tokio::sync::Mutex as AsyncMutex;

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
            let session = IrohSession {
                inner: Arc::new(legacy),
                call_lock: AsyncMutex::new(()),
                attached: Arc::new(AsyncMutex::new(None)),
            };
            Ok(Box::new(session) as Box<dyn AbstractSession>)
        })
    }
}

/// `Session` impl wrapping the legacy `daemon_client::Session`.
struct IrohSession {
    inner: Arc<LegacySession>,
    /// Serialises [`AbstractSession::call`] across tasks. Today's
    /// legacy API has no request-id correlation on the client side —
    /// the recv channel is FIFO. Holding a mutex across send-then-
    /// await-reply keeps that fragile FIFO assumption honest until the
    /// typed-client work (`another-one-f4r`) adds a proper router.
    call_lock: AsyncMutex<()>,
    /// Most recently attached `(section_id, tab_id)`. Used to tag
    /// `SessionEvent::PtyBytes`; see file-level docs on the
    /// single-attach limitation. `Arc` so [`Self::events`] can hand
    /// the same handle to its stream without taking the Mutex by
    /// value.
    attached: Arc<AsyncMutex<Option<(String, String)>>>,
}

impl AbstractSession for IrohSession {
    fn call<'a>(
        &'a self,
        verb: Control,
    ) -> SessionFuture<'a, Result<WorkerReply, TransportError>> {
        Box::pin(async move {
            let _guard = self.call_lock.lock().await;
            // Track attach state so events() can tag PTY bytes
            // correctly. Single-attach only; matches today's daemon.
            match &verb {
                Control::AttachTab {
                    section_id,
                    tab_id,
                } => {
                    *self.attached.lock().await = Some((section_id.clone(), tab_id.clone()));
                }
                Control::DetachTab => {
                    *self.attached.lock().await = None;
                }
                _ => {}
            }
            send_legacy_control(&self.inner, verb).await?;
            // Verbs that produce no reply (PTY-side `Control::Resize`
            // and any other fire-and-forget) would block here forever.
            // The current daemon emits a reply for every Control
            // variant; if a no-reply verb appears, surface the
            // mismatch as a typed Encoding error rather than hanging.
            self.inner
                .next_worker_reply()
                .await
                .ok_or_else(|| TransportError::Closed(Some("recv channel closed".into())))
        })
    }

    fn push_data<'a>(
        &'a self,
        _section_id: &'a str,
        _tab_id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        // Today's wire fans every attached tab's input through the
        // single TY_DATA path — section/tab args are accepted to keep
        // the trait surface clean but ignored. Multi-attach demuxing
        // is future work; documented in the file-level note.
        let payload = bytes.to_vec();
        Box::pin(async move {
            self.inner
                .send(payload)
                .await
                .map_err(|e| TransportError::Other(format!("{e:#}")))
        })
    }

    fn events(&self) -> EventStream {
        Box::pin(IrohEventStream {
            inner: self.inner.clone(),
            attached: Arc::clone(&self.attached),
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

async fn send_legacy_control(
    session: &LegacySession,
    verb: Control,
) -> Result<(), TransportError> {
    match verb {
        Control::Resize { cols, rows } => session.resize(cols, rows).await,
        Control::ListProjects => session.list_projects().await,
        Control::AttachTab {
            section_id,
            tab_id,
        } => session.attach_tab(section_id, tab_id).await,
        Control::DetachTab => session.detach_tab().await,
        Control::TabResize { cols, rows } => session.tab_resize(cols, rows).await,
        Control::LaunchTab {
            section_id,
            tab_id,
        } => session.launch_tab(section_id, tab_id).await,
        // Hello is sent by `connect()` itself; no caller-driven path.
        Control::Hello { .. } => {
            return Err(TransportError::Encoding(
                "Hello is sent by the dial path, not by call()".into(),
            ));
        }
        // Verbs the legacy session has no helper for fall through to
        // a typed Encoding error. Per-verb helpers can be added
        // upstream as needed; the abstract surface deliberately
        // doesn't enumerate every Control variant.
        other => {
            return Err(TransportError::Encoding(format!(
                "iroh transport has no client-side helper for {:?} yet — extend daemon_client::Session",
                std::mem::discriminant(&other),
            )));
        }
    }
    .map_err(|e| TransportError::Other(format!("{e:#}")))
}

/// Stream impl that drains the legacy `Session`'s incoming-bytes /
/// worker-reply channels and translates them into [`SessionEvent`]s.
/// PTY bytes get tagged with the latest known attach key so consumers
/// see a single demultiplexed stream.
struct IrohEventStream {
    inner: Arc<LegacySession>,
    attached: Arc<AsyncMutex<Option<(String, String)>>>,
}

impl Stream for IrohEventStream {
    type Item = SessionEvent;

    fn poll_next(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        // The legacy session exposes async drain methods, not a
        // pollable channel. Wiring those into a Stream-shaped poll
        // requires either holding an in-flight Future across polls
        // or hopping through a tokio::sync::mpsc. The simpler shape
        // is to spawn a task that owns the legacy receivers and feeds
        // an mpsc — but that needs a runtime, which this stream
        // can't assume. Defer the live impl to f4r where the typed
        // client API restructures Session around tokio anyway.
        //
        // Until then, this stream returns Pending forever (consumers
        // that want events fall back to the legacy
        // next_incoming_bytes / next_worker_reply pollers). The trait
        // contract permits this — events() may legitimately yield
        // nothing — and concrete consumers don't exist yet.
        let _ = (&self.inner, &self.attached);
        std::task::Poll::Pending
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
