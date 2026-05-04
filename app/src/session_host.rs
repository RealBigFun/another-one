//! GUI-side adapter around `daemon_transport::Session`.
//!
//! Every daemon interaction the desktop and mobile GUIs make routes
//! through `AnotherOneApp::session: Arc<dyn Session>`. The transport
//! impl behind it is decided at startup — desktop holds the client
//! half of an `in_memory::pair()` whose server half is driven by
//! `daemon::dispatch::serve_session` on the embedded daemon's tokio
//! runtime; mobile starts with [`NoSession`] (every call returns
//! `TransportError::Closed`) and swaps to `IrohSession` once the QR
//! pair flow lands a `daemon_client::iroh_factory().dial(...)` result.
//!
//! The seam exists so `app::ensure_active_terminal_runtime`, the
//! Add/Remove project handlers, the git staging UI, and friends can
//! emit `Control::…` verbs uniformly — narrow desktop, wide desktop
//! and mobile take the same code path; only the platform-specific
//! `app::new` branches on which `Session` impl to construct.
//!
//! ## Runtime
//!
//! `Session::call` returns a `SessionFuture` that needs an executor.
//! The GPUI render thread isn't tokio-aware, so this module holds a
//! shared multi-thread tokio runtime via [`runtime_handle`]. Callers
//! either:
//!
//! * use [`dispatch_fire_and_forget`] for "kick a verb, pump replies
//!   into a queue/callback" patterns (the dominant call site shape —
//!   `ensure_active_terminal_runtime`'s `LaunchTab` + `AttachTab`
//!   issuance, project Add/Remove, git staging, etc.);
//! * await the returned future themselves on a tokio runtime they
//!   already have (the embedded daemon thread, MCP orchestrator,
//!   tests).
//!
//! Replies and `SessionEvent`s land in process-wide queues drained
//! on the GPUI render tick — same pattern as the existing
//! `iroh_client::drain_worker_replies`.

use std::sync::{Arc, OnceLock};

use daemon_proto::{Control, WorkerReply};
use daemon_transport::{
    EventStream, Session, SessionEvent, SessionFuture, TransportError,
};
use futures_core::Stream;
use tokio::runtime::{Handle, Runtime};

/// Shared runtime used to drive `Session::call` futures from threads
/// that aren't themselves tokio-aware (GPUI render thread, callbacks
/// invoked from sync GPUI handlers). Two workers — same shape as
/// `daemon_client::tokio_rt` — is plenty for the GUI's call rate.
fn runtime() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("session-host")
            .build()
            .expect("build session-host runtime")
    })
}

/// Public handle to the shared runtime. Daemon-host hands this out so
/// `in_memory::pair()` can be constructed in a tokio context (the
/// pair's recv router is `tokio::spawn`-ed on entry).
pub(crate) fn runtime_handle() -> Handle {
    runtime().handle().clone()
}

/// Fire-and-forget verb dispatch. Spawns `session.call(verb)` on the
/// shared runtime; on completion, hands the reply (or `TransportError`)
/// to `on_reply`. Drop semantics: dropping the returned `JoinHandle`
/// is fine — the spawned task completes on its own.
///
/// Use for GUI handlers that don't need to await inline (the GPUI
/// render thread can't anyway). Replies land back on the GUI by the
/// callback queueing onto a channel the render tick drains.
pub(crate) fn dispatch_fire_and_forget<F>(session: Arc<dyn Session>, verb: Control, on_reply: F)
where
    F: FnOnce(Result<WorkerReply, TransportError>) + Send + 'static,
{
    runtime().spawn(async move {
        let result = session.call(verb).await;
        on_reply(result);
    });
}

/// Subscribe to a session's event stream and pipe each event into the
/// supplied callback. Spawns a task on the shared runtime that drains
/// the stream until it terminates. The callback runs on a runtime
/// worker — pump work back to the GUI via channels.
#[allow(dead_code)] // wired up by the events-bridge commit
pub(crate) fn spawn_event_pump<F>(stream: EventStream, mut on_event: F)
where
    F: FnMut(SessionEvent) + Send + 'static,
{
    use futures_util::StreamExt;
    runtime().spawn(async move {
        let mut stream = stream;
        while let Some(event) = stream.next().await {
            on_event(event);
        }
    });
}

/// Placeholder `Session` for a GUI that hasn't paired yet (Android
/// pre-QR-scan). Every call returns `TransportError::Closed`. Replaced
/// with the real `IrohSession` once `iroh_client::dial` succeeds and
/// the QR pair flow stores it on `AnotherOneApp::session`.
///
/// `dead_code` is annotated because the constructor is reachable only
/// from the `cfg(target_os = "android")` branch of `AnotherOneApp::new`;
/// host-target `cargo check` doesn't see that arm.
#[allow(dead_code)]
pub(crate) struct NoSession {
    reason: String,
}

impl NoSession {
    #[allow(dead_code)]
    pub(crate) fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }
}

impl Session for NoSession {
    fn call<'a>(&'a self, _verb: Control) -> SessionFuture<'a, Result<WorkerReply, TransportError>> {
        let reason = self.reason.clone();
        Box::pin(async move { Err(TransportError::Closed(Some(reason))) })
    }

    fn push_data<'a>(
        &'a self,
        _section_id: &'a str,
        _tab_id: &'a str,
        _bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        let reason = self.reason.clone();
        Box::pin(async move { Err(TransportError::Closed(Some(reason))) })
    }

    fn events(&self) -> EventStream {
        // An immediately-terminated stream so consumers don't poll
        // forever waiting for events that can't arrive.
        Box::pin(EmptyStream)
    }

    fn close<'a>(
        &'a self,
        _reason: Option<&'a str>,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        Box::pin(async move { Ok(()) })
    }
}

struct EmptyStream;

impl Stream for EmptyStream {
    type Item = SessionEvent;

    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(None)
    }
}
