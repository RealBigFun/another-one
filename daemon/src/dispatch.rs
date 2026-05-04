//! Transport-agnostic verb dispatch.
//!
//! The legacy `transport_iroh::handle_control` function does verb
//! dispatch *inside* the iroh frame loop — `outbound_tx`, attach-
//! state forwarding, and request-id correlation are all interleaved.
//! That works for one transport but blocks the
//! abstract-`ServerSession` cutover the daemon-transport epic
//! (`another-one-iem`) is driving toward.
//!
//! This module is the seam: a [`serve_session`] entry point that
//! loops on a [`ServerSession::next_call`], dispatches by `Control`
//! variant, and emits replies via [`ServerSession::reply`]. No iroh
//! types, no `OutboundTx`, no attach forwarder handles — just the
//! abstract trait surface plus the registry.
//!
//! ## Scope today
//!
//! Implements the **read-only / state-mutation verbs** that don't
//! depend on a live PTY-broadcast forwarder (per-tab subscriptions,
//! TY_DATA fan-out generation tracking, etc.). That covers most of
//! today's `Control` surface — `ListProjects`, project mutations,
//! task creation, agent settings, open-in settings, etc. Verbs that
//! touch attach state (`AttachTab` / `DetachTab` / `Resize` /
//! `TabResize`) are handled with `WorkerReply::Err` for now and
//! tracked under [`another-one-pqs`] as the remainder of the iroh
//! server-side cutover.
//!
//! Once the attach machinery is split into a transport-agnostic
//! shape, this dispatcher absorbs those verbs too and the iroh
//! transport's `handle_connection` shrinks to just framing +
//! pairing.

use std::sync::Arc;

use daemon_proto::{Control, ErrKind, WorkerReply};
use daemon_transport::{ServerSession, TransportError};

use crate::registry::DaemonRegistry;

/// Drive `session` against `registry` until the peer closes. Pulls
/// verbs via [`ServerSession::next_call`], dispatches by variant,
/// emits the matching reply via [`ServerSession::reply`].
///
/// Returns when `next_call` yields `Ok(None)` (clean close) or any
/// step errors. The caller is responsible for tearing the session
/// down and logging the outcome.
pub async fn serve_session(
    session: Box<dyn ServerSession>,
    registry: Arc<dyn DaemonRegistry>,
) -> Result<(), TransportError> {
    loop {
        let Some((request_id, ctrl)) = session.next_call().await? else {
            return Ok(());
        };
        let reply = dispatch_call(ctrl, registry.as_ref(), session.peer_id()).await;
        session.reply(request_id, reply).await?;
    }
}

/// Map a single `Control` verb to a `WorkerReply`. Pure function over
/// the registry — no I/O on the transport side, no per-session state.
/// Verbs that require attach-state bookkeeping (PTY forwarder,
/// tab-resize routing) return `WorkerReply::Err` with a clear
/// message; those land in a follow-up that lifts the attach
/// machinery into a transport-agnostic shape.
async fn dispatch_call(
    ctrl: Control,
    registry: &dyn DaemonRegistry,
    _viewer_id: &str,
) -> WorkerReply {
    // The iroh transport gates every non-Hello verb on a
    // `registry.health()` check (one source of truth for "registry
    // dropped" — typically when the desktop app is quitting). Match
    // that here so the abstract dispatch path doesn't accidentally
    // route verbs into a half-shutdown registry.
    if !matches!(ctrl, Control::Hello { .. }) {
        if let Err(message) = registry.health() {
            return WorkerReply::Err {
                kind: ErrKind::Internal,
                message,
            };
        }
    }

    // Per-domain helpers that pre-handle a contiguous set of
    // variants. Each returns `Ok(reply)` if it consumed the verb and
    // `Err(ctrl)` to pass it on. Mirrors the legacy
    // `transport_iroh::handle_control` flow so adding a verb to
    // either path lights up automatically here too.
    let ctrl = match crate::commands::agent_settings::handle(ctrl, registry) {
        Ok(reply) => return reply,
        Err(ctrl) => ctrl,
    };
    let ctrl = match crate::commands::open_in::handle(ctrl, registry) {
        Ok(reply) => return reply,
        Err(ctrl) => ctrl,
    };

    match ctrl {
        Control::ListProjects => WorkerReply::ProjectList {
            projects: registry.list_projects(),
        },

        Control::ListProjectActions { project_id } => WorkerReply::ProjectActionsAck {
            actions: registry.list_project_actions(&project_id),
        },

        Control::Hello { .. } => {
            // Hello is the dial-time pairing handshake — concrete
            // transports consume it before constructing the
            // ServerSession the dispatcher sees. If one squeaks
            // through, surface a typed error rather than letting it
            // hit a generic fall-through.
            WorkerReply::Err {
                kind: ErrKind::Internal,
                message: "Hello reached dispatch — transport should have consumed it".into(),
            }
        }

        // Attach-bookkeeping verbs return Err today. The remainder
        // of `another-one-pqs` lifts the per-connection attach state
        // (broadcast subscription, TY_DATA forwarder generation)
        // into a transport-agnostic shape; once that lands, these
        // arms light up with real handlers.
        Control::AttachTab { .. }
        | Control::DetachTab
        | Control::Resize { .. }
        | Control::TabResize { .. }
        | Control::LaunchTab { .. } => WorkerReply::Err {
            kind: ErrKind::Internal,
            message: "tab attach/resize verbs not yet wired through serve_session — \
                     drive these via daemon::transport_iroh until pqs ships the cutover"
                .into(),
        },

        // Everything else falls through to a typed Err. Adding a
        // dispatch arm for a new verb is a one-place change here
        // (mirrored in transport_iroh::handle_control until the full
        // cutover collapses both into this dispatcher).
        other => WorkerReply::Err {
            kind: ErrKind::Internal,
            message: format!(
                "serve_session has no dispatch arm for {:?} yet",
                std::mem::discriminant(&other)
            ),
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use daemon_proto::ProjectSummary;
    use daemon_transport::in_memory::pair;
    // `Session::call` is dispatched through `Box<dyn Session>` so
    // the trait isn't strictly required in scope, but importing it
    // keeps the intent explicit at the call sites below.
    #[allow(unused_imports)]
    use daemon_transport::Session as _;
    use std::sync::Mutex;

    /// Minimal registry stub — health is OK; list_projects returns a
    /// fixed slice. Everything else panics so the test fails loudly
    /// if dispatch routes to an unimplemented verb.
    struct StubRegistry {
        projects: Mutex<Vec<ProjectSummary>>,
    }

    impl StubRegistry {
        fn new() -> Self {
            Self {
                projects: Mutex::new(vec![ProjectSummary {
                    id: "p1".into(),
                    name: "p1".into(),
                    path: "/tmp/p1".into(),
                    kind: daemon_proto::ProjectKind::Root,
                    current_branch: Some("main".into()),
                    tasks: vec![],
                }]),
            }
        }
    }

    impl DaemonRegistry for StubRegistry {
        fn health(&self) -> Result<(), String> {
            Ok(())
        }
        fn list_projects(&self) -> Vec<ProjectSummary> {
            self.projects.lock().unwrap().clone()
        }
        fn attach_tab(
            &self,
            _: &str,
            _: &str,
        ) -> Option<tokio::sync::broadcast::Receiver<Vec<u8>>> {
            None
        }
        fn tab_input(&self, _: &str, _: &str, _: &[u8]) {}
        fn tab_resize(&self, _: &str, _: &str, _: &str, _: u16, _: u16) {}
    }

    #[tokio::test]
    async fn serve_session_round_trips_list_projects() {
        let (server, client) = pair("test-peer");

        let registry: Arc<dyn DaemonRegistry> = Arc::new(StubRegistry::new());
        let server_task = tokio::spawn(serve_session(server, Arc::clone(&registry)));

        let reply = client
            .call(Control::ListProjects)
            .await
            .expect("call should succeed");

        match reply {
            WorkerReply::ProjectList { projects } => {
                assert_eq!(projects.len(), 1, "stub returns one project");
                assert_eq!(projects[0].id, "p1");
            }
            other => panic!("expected ProjectList, got {other:?}"),
        }

        // Drop the client to close the session; serve_session's
        // next_call returns Ok(None) and the task exits cleanly.
        drop(client);
        server_task
            .await
            .expect("serve task")
            .expect("serve_session result");
    }

    #[tokio::test]
    async fn serve_session_returns_typed_err_for_attach_verbs() {
        let (server, client) = pair("test-peer");
        let registry: Arc<dyn DaemonRegistry> = Arc::new(StubRegistry::new());
        let _server_task = tokio::spawn(serve_session(server, registry));

        let reply = client
            .call(Control::AttachTab {
                section_id: "s".into(),
                tab_id: "t".into(),
            })
            .await
            .expect("call");
        match reply {
            WorkerReply::Err { kind, message } => {
                assert!(matches!(kind, ErrKind::Internal));
                assert!(
                    message.contains("attach"),
                    "message should explain why: {message}"
                );
            }
            other => panic!("expected Err for AttachTab, got {other:?}"),
        }
    }
}
