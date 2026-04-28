//! Iroh QUIC transport + connection state machine.
//!
//! Endpoint identity + pairing material (secret key, TOFU allowlist,
//! pairing URL / QR PNG) are loaded from paths supplied by the caller
//! so the same code backs two embedders:
//!
//!   - `daemon-sandbox` binary — persists under
//!     `$XDG_DATA_HOME/another-one-sandbox/`.
//!   - Desktop `AnotherOneApp` — persists alongside the desktop's
//!     own config under `$XDG_CONFIG_HOME/another-one/daemon/`.
//!
//! Wire format: one bidi QUIC stream per connection, length-prefixed
//! framing (see [`crate::frame`]). Per-connection state machine:
//! zero or one attached tab at a time; on `AttachTab` the daemon
//! subscribes to that tab's live PTY broadcast and forwards bytes
//! as `TY_DATA` frames. New clients send terminal input through
//! `Control::TabInput`; inbound `TY_DATA` is still accepted for
//! legacy clients.

use std::path::{Path, PathBuf};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc, Mutex,
};

use anyhow::Context;
use iroh::endpoint::{presets, Connection, Incoming};
use iroh::{Endpoint, EndpointAddr, SecretKey};
use tokio::sync::{broadcast, mpsc};
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};

use crate::frame::{self, Control, ControlEnvelope, ErrKind, WorkerReply, WorkerReplyEnvelope};
use crate::registry::{DaemonRegistry, EndpointHandle, PairState};

/// ALPN advertised by the daemon. Version-suffixed so future protocol
/// breaks can be versioned cleanly (`/1`, `/2`, …).
///
/// `/1` introduced:
///   - `Control::Hello { protocol_version }` — explicit in-band
///     version field so a peer that bypasses ALPN (e.g. via a proxy
///     that strips it) is still rejected with a deterministic close
///     reason rather than blowing up on the first unknown variant.
///   - `request_id` correlation on every Control / WorkerReply
///     envelope.
///   - `WorkerReply::Err { request_id, kind, message }` for
///     uniform per-request failure reporting.
pub const ALPN: &[u8] = b"anotherone/pty/1";

/// In-band protocol version carried in `Control::Hello`. Bumped in
/// lockstep with the ALPN suffix; mismatches close the connection
/// with [`CLOSE_REASON_INCOMPATIBLE_VERSION`].
pub const PROTOCOL_VERSION: u32 = 1;

/// QUIC close reason emitted to unauthorised peers. Short on purpose:
/// the CONNECTION_CLOSE frame is observable on the wire, so long
/// user-facing copy here would leak product UX text to an on-path
/// observer. Clients match on this byte string and expand it into
/// localisable copy ("Pairing expired — please re-scan the QR")
/// in the UI.
pub const CLOSE_REASON_UNPAIRED: &[u8] = b"anotherone/unpaired";

/// QUIC close reason for a peer whose `Control::Hello.protocol_version`
/// disagrees with this daemon's [`PROTOCOL_VERSION`]. Sent before any
/// other frame is decoded, so a v0 client (or a future v2 client
/// hitting a v1 daemon) gets a clean shutdown instead of a serde
/// panic mid-stream. Mirrors the substring match clients perform on
/// the close reason.
pub const CLOSE_REASON_INCOMPATIBLE_VERSION: &[u8] = b"anotherone/incompatible-version";

/// Bring up an iroh endpoint backed by `registry`. Returns once the
/// endpoint is online + the pairing QR has been rendered; the accept
/// loop runs on a detached task owned by the returned handle (drop
/// or `abort()` the handle to shut it down).
pub async fn run_embedded(
    registry: Arc<dyn DaemonRegistry>,
    secret_key_path: PathBuf,
    paired_peers_path: PathBuf,
) -> anyhow::Result<EndpointHandle> {
    let secret_key = load_or_create_secret_key(&secret_key_path)?;
    // Use the minimal preset for the embedded desktop daemon.
    //
    // `presets::N0` enables pkarr publishing and default relay wiring.
    // On macOS release app launches we've seen that background publish
    // path abort inside iroh/libmalloc during startup. The desktop only
    // needs a stable local endpoint plus direct addresses in the pairing
    // URL, so keep the embedded daemon on direct-only transport here.
    let endpoint = Endpoint::builder(presets::Minimal)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("bind iroh endpoint")?;

    let endpoint_id = endpoint.id().to_string();
    info!("iroh EndpointId: {endpoint_id}");
    info!("iroh ALPN: {}", String::from_utf8_lossy(ALPN));

    // Don't call `endpoint.online()` here. iroh's `online()` loops on
    // `home_relay_status()` waiting for a relay to report connected,
    // but we configured `presets::Minimal` precisely *because* we
    // don't use a relay — so the watcher would fire forever and the
    // daemon thread would park in `block_on(run_endpoint)` for the
    // process lifetime. (iroh's own docs note `online()` is for
    // endpoints that need to be "dialable… over the internet" via a
    // relay; ours just need direct LAN addresses for pairing.)
    //
    // For Minimal, the direct addresses are populated synchronously
    // by network-interface enumeration after `bind()`, so
    // `endpoint.addr()` is ready immediately.
    let addr = endpoint.addr();
    info!("iroh endpoint ready: {addr:?}");

    let nonce = generate_pair_nonce();
    let pairing_url = build_pairing_url_with_token(&addr, &nonce);
    let qr_png_bytes = render_qr_png_bytes(&pairing_url).context("render pairing QR PNG")?;
    let pair_state = Arc::new(Mutex::new(PairState {
        nonce: Some(nonce),
        addr: addr.clone(),
        pairing_url,
        qr_png_bytes,
    }));

    // Spawn the accept loop. The root task owns the endpoint; each
    // incoming connection spawns its own task so slow clients can't
    // starve the accept loop.
    let registry_cloned = registry.clone();
    let pair_state_cloned = pair_state.clone();
    let root_handle = tokio::spawn(async move {
        while let Some(incoming) = endpoint.accept().await {
            let registry = registry_cloned.clone();
            let paired_path = paired_peers_path.clone();
            let pair_state = pair_state_cloned.clone();
            tokio::spawn(async move {
                if let Err(e) = handle_incoming(incoming, registry, &paired_path, pair_state).await
                {
                    warn!(error = %e, "iroh connection error");
                }
            });
        }
    });

    Ok(EndpointHandle {
        endpoint_id,
        pair_state,
        _root_task: root_handle.abort_handle(),
    })
}

// ---- connection state machine ----------------------------------

/// State of the one-at-a-time PTY attachment on this connection.
struct Attached {
    section_id: String,
    tab_id: String,
    /// Abort handle for the forwarder task draining the per-tab
    /// broadcast into this connection's outbound mpsc. Dropped /
    /// aborted when the client detaches or attaches elsewhere.
    forwarder: Option<AbortHandle>,
}

impl Attached {
    fn abort_forwarder(self) {
        if let Some(forwarder) = self.forwarder {
            forwarder.abort();
        }
    }
}

#[derive(Debug)]
struct OutboundFrame {
    ty: u8,
    payload: Vec<u8>,
    data_generation: Option<u64>,
}

type OutboundTx = mpsc::Sender<OutboundFrame>;

async fn handle_incoming(
    incoming: Incoming,
    registry: Arc<dyn DaemonRegistry>,
    paired_peers_path: &Path,
    pair_state: Arc<Mutex<PairState>>,
) -> anyhow::Result<()> {
    let conn = incoming
        .accept()
        .context("accept")?
        .await
        .context("handshake")?;
    let remote = conn.remote_id();
    let viewer_id = remote.to_string();

    let authz = match peer_status(&viewer_id, paired_peers_path) {
        Ok(PeerStatus::Paired) => {
            info!(%remote, "iroh client connected (paired)");
            PostAuth::AlreadyPaired
        }
        Ok(PeerStatus::Unknown) => {
            // Paired-peer list is empty OR this peer isn't in it. We
            // accept the connection but defer authorisation until the
            // peer sends `Control::Hello` with a matching nonce over
            // the bidi stream — that's handled in `handle_connection`.
            info!(%remote, "iroh client connected (unknown — awaiting Hello)");
            PostAuth::AwaitHello
        }
        Err(e) => {
            warn!(%remote, error = %e, "rejecting peer");
            conn.close(1u8.into(), CLOSE_REASON_UNPAIRED);
            return Ok(());
        }
    };

    let result = handle_connection(
        conn,
        registry.clone(),
        &viewer_id,
        authz,
        paired_peers_path,
        pair_state,
    )
    .await;
    // Clear this viewer's size entries so a stale small viewport
    // doesn't keep the PTY cramped after the session ends.
    registry.viewer_disconnected(&viewer_id);
    result
}

#[derive(Clone, Copy)]
enum PostAuth {
    AlreadyPaired,
    AwaitHello,
}

async fn handle_connection(
    conn: Connection,
    registry: Arc<dyn DaemonRegistry>,
    viewer_id: &str,
    mut authz: PostAuth,
    paired_peers_path: &Path,
    pair_state: Arc<Mutex<PairState>>,
) -> anyhow::Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await.context("accept_bi")?;

    // Outbound mpsc: all producers (worker-reply replies + the PTY
    // forwarder task) push frames; the writer task owns `send` and
    // serialises writes.
    let data_generation = Arc::new(AtomicU64::new(0));
    let data_generation_for_writer = data_generation.clone();
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<OutboundFrame>(64);
    let writer_task = tokio::spawn(async move {
        while let Some(frame) = outbound_rx.recv().await {
            if frame.ty == frame::TY_DATA
                && frame.data_generation != Some(data_generation_for_writer.load(Ordering::Relaxed))
            {
                continue;
            }
            if let Err(e) = frame::write_frame(&mut send, frame.ty, &frame.payload).await {
                debug!(error = %e, "iroh frame write failed");
                break;
            }
        }
        let _ = send.finish();
    });

    let mut attached: Option<Attached> = None;

    loop {
        match frame::read_frame(&mut recv).await {
            Ok(Some((frame::TY_DATA, payload))) => {
                if matches!(authz, PostAuth::AwaitHello) {
                    warn!(viewer_id, "pre-Hello data from unpaired peer; rejecting");
                    conn.close(1u8.into(), CLOSE_REASON_UNPAIRED);
                    break;
                }
                route_pty_input(registry.as_ref(), attached.as_ref(), &payload);
                // No attachment → silently drop. Not an error:
                // clients may type during the race between AttachTab
                // going out and the first reply coming back.
            }
            Ok(Some((frame::TY_CONTROL, payload))) => {
                match serde_json::from_slice::<ControlEnvelope>(&payload) {
                    Ok(envelope) => {
                        let ControlEnvelope {
                            request_id,
                            control: ctrl,
                        } = envelope;
                        // Version-check Hello regardless of pairing state.
                        // A v0 client that somehow squeaks past the ALPN
                        // gate — or a v2 client speculatively dialling
                        // a v1 daemon — must be told why the connection
                        // is closing, not allowed to drift further into
                        // the protocol where serde would eventually
                        // panic on an unknown variant.
                        if let Control::Hello {
                            protocol_version, ..
                        } = &ctrl
                        {
                            if *protocol_version != PROTOCOL_VERSION {
                                warn!(
                                    viewer_id,
                                    peer_version = *protocol_version,
                                    daemon_version = PROTOCOL_VERSION,
                                    "rejecting peer with incompatible protocol version"
                                );
                                conn.close(1u8.into(), CLOSE_REASON_INCOMPATIBLE_VERSION);
                                break;
                            }
                        }
                        if matches!(authz, PostAuth::AwaitHello) {
                            match consume_hello(ctrl, viewer_id, &pair_state, paired_peers_path) {
                                Ok(()) => {
                                    authz = PostAuth::AlreadyPaired;
                                    info!(viewer_id, "TOFU pair complete");
                                    continue;
                                }
                                Err(e) => {
                                    warn!(viewer_id, error = %e, "rejecting unpaired peer");
                                    conn.close(1u8.into(), CLOSE_REASON_UNPAIRED);
                                    break;
                                }
                            }
                        }
                        handle_control(
                            request_id,
                            ctrl,
                            &registry,
                            &outbound_tx,
                            &data_generation,
                            &mut attached,
                            viewer_id,
                        )
                        .await
                        .unwrap_or_else(|e| {
                            warn!(error = %e, "control dispatch failed");
                        });
                    }
                    Err(e) => warn!(error = %e, "bad iroh control frame"),
                }
            }
            Ok(Some((ty, _))) => warn!(frame_type = ty, "unknown iroh frame type"),
            Ok(None) => {
                debug!("iroh peer closed send");
                break;
            }
            Err(e) => {
                warn!(error = %e, "iroh frame read failed");
                break;
            }
        }
    }

    if let Some(att) = attached.take() {
        if att.forwarder.is_some() {
            registry.note_tab_output_observed(viewer_id, &att.section_id, &att.tab_id);
        }
        att.abort_forwarder();
    }
    drop(outbound_tx);
    writer_task.abort();
    info!("iroh session ended");
    Ok(())
}

fn route_pty_input(registry: &dyn DaemonRegistry, attached: Option<&Attached>, payload: &[u8]) {
    let Some(att) = attached else {
        return;
    };
    registry.tab_input(&att.section_id, &att.tab_id, payload);
}

fn route_terminal_input_event(
    registry: &dyn DaemonRegistry,
    attached: Option<&Attached>,
    event: &frame::TerminalInputEvent,
) {
    let bytes = event.pty_bytes();
    route_pty_input(registry, attached, &bytes);
}

/// Validate a `Control::Hello` from an unpaired peer. On match, consume
/// the nonce (so a second reader of the same QR can't re-pair) and
/// append the peer's `NodeId` to the allowlist. Any other control
/// frame, missing token, mismatched token, or no outstanding nonce is
/// rejected.
fn consume_hello(
    ctrl: Control,
    viewer_id: &str,
    pair_state: &Arc<Mutex<PairState>>,
    paired_peers_path: &Path,
) -> anyhow::Result<()> {
    let Control::Hello { pair_token, .. } = ctrl else {
        anyhow::bail!("first frame from unpaired peer must be Control::Hello");
    };
    let presented =
        pair_token.ok_or_else(|| anyhow::anyhow!("Hello from unpaired peer missing pair_token"))?;

    // Hold the nonce lock until the allowlist write succeeds so
    // validation, persistence, and nonce-consumption form one atomic
    // pairing step. That closes the race where two concurrent Hellos
    // carrying the same QR token could both pass validation before one
    // of them cleared the nonce.
    let mut state = pair_state.lock().unwrap_or_else(|p| p.into_inner());
    let expected = state
        .nonce
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("no outstanding pair nonce (consumed or not rolled)"))?;
    if !constant_time_eq(expected.as_bytes(), presented.as_bytes()) {
        anyhow::bail!("pair_token mismatch");
    }

    persist_pairing(viewer_id, paired_peers_path)?;
    state.nonce = None;
    Ok(())
}

async fn handle_control(
    request_id: u64,
    ctrl: Control,
    registry: &Arc<dyn DaemonRegistry>,
    outbound_tx: &OutboundTx,
    data_generation: &Arc<AtomicU64>,
    attached: &mut Option<Attached>,
    viewer_id: &str,
) -> anyhow::Result<()> {
    if !matches!(ctrl, Control::Hello { .. }) {
        if let Err(message) = registry.health() {
            send_err(outbound_tx, request_id, ErrKind::Internal, message).await?;
            return Ok(());
        }
    }

    match ctrl {
        Control::Resize { cols, rows } | Control::TabResize { cols, rows } => {
            if let Some(att) = attached.as_ref() {
                registry.tab_resize(viewer_id, &att.section_id, &att.tab_id, cols, rows);
            }
        }
        Control::TabInput { event } => {
            route_terminal_input_event(registry.as_ref(), attached.as_ref(), &event);
        }
        Control::ListProjects => {
            let projects = registry.list_projects();
            let wire = WorkerReply::ProjectList { projects };
            send_worker_reply(outbound_tx, request_id, &wire).await?;
        }
        Control::OpenInState => {
            let reply = match registry.open_in_state() {
                Some(state) => WorkerReply::OpenInStateAck { state },
                None => WorkerReply::Err {
                    message: "open_in_state: registry does not surface \
                              Open-In on this host"
                        .to_string(),
                    kind: ErrKind::Unsupported,
                },
            };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::ListProjectActions { project_id } => {
            let actions = registry.list_project_actions(&project_id);
            let reply = WorkerReply::ProjectActionsAck { actions };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::ReadEnabledAgents => {
            let view = registry.read_enabled_agents();
            let reply = WorkerReply::EnabledAgentsAck { view };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::SubmitNewTask {
            project_id,
            task_name,
            source_branch,
            agent_ids,
            branch_mode_existing,
            worktree_mode,
        } => {
            let outcome = registry
                .submit_new_task(
                    project_id,
                    task_name,
                    source_branch,
                    agent_ids,
                    branch_mode_existing,
                    worktree_mode,
                )
                .await;
            match outcome {
                Ok(section_id) => {
                    let reply = WorkerReply::SubmitNewTaskAck { section_id };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::AddAgentToSection {
            section_id,
            agent_id,
        } => match registry.add_agent_to_section(&section_id, &agent_id) {
            Ok(tab_id) => {
                let reply = WorkerReply::AddAgentToSectionAck { tab_id };
                send_worker_reply(outbound_tx, request_id, &reply).await?;
            }
            Err(message) => {
                let kind = if message.contains("unknown") || message.contains("malformed") {
                    ErrKind::UnknownId
                } else {
                    ErrKind::Internal
                };
                send_worker_reply(outbound_tx, request_id, &WorkerReply::Err { message, kind })
                    .await?;
            }
        },
        Control::ActivateSectionTab { section_id, tab_id } => {
            match registry.activate_section_tab(&section_id, &tab_id) {
                Ok(()) => {
                    send_worker_reply(outbound_tx, request_id, &WorkerReply::ActivateSectionTabAck)
                        .await?;
                }
                Err(message) => {
                    let kind = if message.contains("unknown") || message.contains("malformed") {
                        ErrKind::UnknownId
                    } else {
                        ErrKind::Internal
                    };
                    send_worker_reply(outbound_tx, request_id, &WorkerReply::Err { message, kind })
                        .await?;
                }
            }
        }
        Control::CloseSectionTab { section_id, tab_id } => {
            match registry.close_section_tab(&section_id, &tab_id) {
                Ok(active_tab_id) => {
                    let reply = WorkerReply::CloseSectionTabAck { active_tab_id };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(message) => {
                    let kind = if message.contains("unknown") || message.contains("malformed") {
                        ErrKind::UnknownId
                    } else {
                        ErrKind::Internal
                    };
                    send_worker_reply(outbound_tx, request_id, &WorkerReply::Err { message, kind })
                        .await?;
                }
            }
        }
        Control::ToggleSectionTabPinned { section_id, tab_id } => {
            match registry.toggle_section_tab_pinned(&section_id, &tab_id) {
                Ok(pinned) => {
                    let reply = WorkerReply::ToggleSectionTabPinnedAck { pinned };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(message) => {
                    let kind = if message.contains("unknown") || message.contains("malformed") {
                        ErrKind::UnknownId
                    } else {
                        ErrKind::Internal
                    };
                    send_worker_reply(outbound_tx, request_id, &WorkerReply::Err { message, kind })
                        .await?;
                }
            }
        }
        Control::ReadAgentSettings => {
            let view = registry.read_agent_settings();
            let reply = WorkerReply::AgentSettingsAck { view };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::SetAgentEnabled { agent_id, enabled } => {
            match registry.set_agent_enabled(&agent_id, enabled) {
                Ok(changed) => {
                    let reply = WorkerReply::SetAgentEnabledAck { changed };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(message) => {
                    let kind = if message.contains("unknown agent") {
                        ErrKind::UnknownId
                    } else {
                        ErrKind::Internal
                    };
                    send_err(outbound_tx, request_id, kind, message).await?;
                }
            }
        }
        Control::SetDefaultAgent { agent_id } => match registry.set_default_agent(&agent_id) {
            Ok(changed) => {
                let reply = WorkerReply::SetDefaultAgentAck { changed };
                send_worker_reply(outbound_tx, request_id, &reply).await?;
            }
            Err(message) => {
                let kind = if message.contains("unknown agent") {
                    ErrKind::UnknownId
                } else {
                    ErrKind::Internal
                };
                send_err(outbound_tx, request_id, kind, message).await?;
            }
        },
        Control::SetAgentLaunchArgs { agent_id, args } => {
            match registry.set_agent_launch_args(&agent_id, args) {
                Ok(changed) => {
                    let reply = WorkerReply::SetAgentLaunchArgsAck { changed };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(message) => {
                    let kind = if message.contains("unknown agent") {
                        ErrKind::UnknownId
                    } else {
                        ErrKind::Internal
                    };
                    send_err(outbound_tx, request_id, kind, message).await?;
                }
            }
        }
        Control::ReadOpenInSettings => {
            let reply = match registry.read_open_in_settings() {
                Some(view) => WorkerReply::OpenInSettingsAck { view },
                None => WorkerReply::Err {
                    message: "read_open_in_settings: registry does not surface \
                              Open-In settings on this host"
                        .to_string(),
                    kind: ErrKind::Unsupported,
                },
            };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::SetOpenInAppEnabled { app_id, enabled } => {
            match registry.set_open_in_app_enabled(&app_id, enabled) {
                Ok(()) => {
                    let reply = WorkerReply::SetOpenInAppEnabledAck;
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(message) => {
                    send_err(outbound_tx, request_id, ErrKind::Internal, message).await?;
                }
            }
        }
        Control::OpenProjectInApp { project_id, app_id } => {
            match registry.open_project_in_app(&project_id, &app_id) {
                Ok(()) => {
                    let reply = WorkerReply::OpenProjectInAppAck;
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(message) => {
                    send_err(outbound_tx, request_id, ErrKind::Internal, message).await?;
                }
            }
        }
        Control::RunProjectAction {
            project_id,
            section_id,
            action_id,
        } => {
            let reply = match registry.run_project_action(&project_id, &section_id, &action_id) {
                Ok(tab_id) => WorkerReply::RunProjectActionAck { tab_id },
                Err(message) => WorkerReply::Err {
                    message,
                    // `Internal` covers the diverse failure surface
                    // (unknown ids, malformed section, command
                    // empty, project store mutex poisoned). We keep
                    // the wire kind coarse here rather than threading
                    // a parsed failure type all the way through. UI should
                    // surface the message verbatim in a toast.
                    kind: ErrKind::Internal,
                },
            };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::SaveProjectAction {
            project_id,
            action,
            save_global_copy,
        } => match registry.save_project_action(&project_id, action, save_global_copy) {
            Ok(()) => {
                let reply = WorkerReply::SaveProjectActionAck;
                send_worker_reply(outbound_tx, request_id, &reply).await?;
            }
            Err(message) => {
                send_err(outbound_tx, request_id, ErrKind::Internal, message).await?;
            }
        },
        Control::DeleteProjectAction {
            project_id,
            action_id,
        } => {
            let deleted = registry.delete_project_action(&project_id, &action_id);
            let reply = WorkerReply::DeleteProjectActionAck { deleted };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::AttachTab { section_id, tab_id } => {
            let same_target = attached
                .as_ref()
                .is_some_and(|prev| prev.section_id == section_id && prev.tab_id == tab_id);
            let generation = if same_target {
                data_generation.load(Ordering::Relaxed)
            } else {
                data_generation
                    .fetch_add(1, Ordering::Relaxed)
                    .wrapping_add(1)
            };
            // Drop any prior attachment on this connection.
            if let Some(prev) = attached.take() {
                if prev.forwarder.is_some() {
                    registry.note_tab_output_observed(viewer_id, &prev.section_id, &prev.tab_id);
                }
                prev.abort_forwarder();
            }
            // Clear this viewer's viewport claim from the prior tab
            // before installing a new one. Without this, switching
            // attach targets leaves the old tab's `active_viewers`
            // entry stale until the first TabResize arrives — which
            // often doesn't fire on cold attach, leaving the old
            // tab's PTY clamped to this phone's viewport despite
            // the phone having moved on.
            if !same_target {
                registry.viewer_disconnected(viewer_id);
            }

            let Some(attachment) = registry.attach_tab_with_replay(viewer_id, &section_id, &tab_id)
            else {
                debug!(section_id, tab_id, "attach_tab: waiting for live runtime");
                *attached = Some(Attached {
                    section_id,
                    tab_id,
                    forwarder: None,
                });
                return Ok(());
            };

            let mut rx = attachment.receiver;
            let replay = attachment.replay;
            let out = outbound_tx.clone();
            let forwarder = tokio::spawn(async move {
                for bytes in replay {
                    if out
                        .send(OutboundFrame {
                            ty: frame::TY_DATA,
                            payload: bytes,
                            data_generation: Some(generation),
                        })
                        .await
                        .is_err()
                    {
                        return;
                    }
                }
                loop {
                    match rx.recv().await {
                        Ok(bytes) => {
                            if out
                                .send(OutboundFrame {
                                    ty: frame::TY_DATA,
                                    payload: bytes,
                                    data_generation: Some(generation),
                                })
                                .await
                                .is_err()
                            {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            // Slow mobile consumer lost `n` chunks.
                            // Silently resuming from the new tail
                            // would leave the client's terminal
                            // state machine stranded — mid-CSI or
                            // mid-alt-screen, cursor at wrong row —
                            // because the skipped bytes carried the
                            // closing escape sequences. There's no
                            // in-band resync we can perform; the
                            // only correct recovery is to tear down
                            // the attachment and let the client
                            // reconnect, where it'll get a fresh
                            // scrollback replay + a clean VT state.
                            warn!(
                                lagged = n,
                                "attach forwarder lagged; dropping attachment to force reattach"
                            );
                            break;
                        }
                    }
                }
            });

            *attached = Some(Attached {
                section_id,
                tab_id,
                forwarder: Some(forwarder.abort_handle()),
            });
        }
        Control::DetachTab => {
            data_generation.fetch_add(1, Ordering::Relaxed);
            if let Some(prev) = attached.take() {
                if prev.forwarder.is_some() {
                    registry.note_tab_output_observed(viewer_id, &prev.section_id, &prev.tab_id);
                }
                prev.abort_forwarder();
            }
            // A detached viewer has no focused tab, so their
            // viewport claim is stale — clear it so the PTY
            // re-aggregates to the remaining viewers' min (or lifts
            // the clamp entirely if this was the last viewer).
            // Same semantics as viewer_disconnected on session end,
            // just without closing the control stream.
            registry.viewer_disconnected(viewer_id);
        }
        Control::WatchProject { project_path: _ } => {
            // Legacy no-op. Kept in the enum for serde-compat with
            // any lingering clients; new clients use
            // ListProjects + AttachTab.
            debug!("legacy Control::WatchProject ignored");
        }
        Control::LaunchTab { section_id, tab_id } => {
            registry.launch_tab(&section_id, &tab_id);
        }
        Control::AddProject { path } => {
            // `add_project` is async — `prepare_project` runs on a
            // background thread inside the registry impl. We await
            // the result here on the per-connection task; the
            // outbound writer task is a separate task and keeps
            // pumping any other queued frames in the meantime.
            match registry.add_project(path).await {
                Ok(project) => {
                    let wire = WorkerReply::ProjectAdded { project };
                    send_worker_reply(outbound_tx, request_id, &wire).await?;
                }
                Err(e) => {
                    let wire = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &wire).await?;
                }
            }
        }
        Control::RemoveProject { project_id } => match registry.remove_project(&project_id) {
            Ok(()) => {
                let wire = WorkerReply::ProjectRemoved { project_id };
                send_worker_reply(outbound_tx, request_id, &wire).await?;
            }
            Err(e) => {
                let wire = WorkerReply::Err {
                    message: format!("{e:#}"),
                    kind: ErrKind::Internal,
                };
                send_worker_reply(outbound_tx, request_id, &wire).await?;
            }
        },
        Control::Hello { .. } => {
            // Hello is only meaningful as the *first* control frame
            // from an unpaired peer — see `consume_hello`. A paired
            // peer that sends it mid-session is harmless but pointless;
            // drop it rather than error.
            debug!("stray Control::Hello from already-paired peer; ignored");
        }
        Control::CreateWorktreeTask {
            project_id,
            task_name,
            source_branch,
            agent_provider,
        } => {
            // The registry future may take tens of seconds (worker
            // thread spawns + git worktree creation + prepare_project).
            // We `await` it inline rather than detaching so a
            // `WorkerReply::Err` lands on the same connection if the
            // worker fails — preserves request_id correlation.
            let project_id_for_reply = project_id.clone();
            let result = registry
                .create_worktree_task(project_id, task_name, source_branch, agent_provider)
                .await;
            match result {
                Ok(task) => {
                    let reply = WorkerReply::TaskCreated {
                        project_id: project_id_for_reply,
                        task,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::StageChangedFile {
            project_id,
            path,
            original_path,
        } => {
            // Inline-snapshot ack: the registry's stage helper runs
            // the git mutation *and* re-reads the post-mutation
            // changed-files list, so the caller's ack carries the
            // refreshed Changes pane state in the same round-trip.
            let outcome = registry
                .stage_changed_file(&project_id, &path, original_path.as_deref())
                .await;
            match outcome {
                Ok(changed_files) => {
                    let reply = WorkerReply::StageChangedFileAck { changed_files };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::UnstageChangedFile {
            project_id,
            path,
            original_path,
        } => {
            let outcome = registry
                .unstage_changed_file(&project_id, &path, original_path.as_deref())
                .await;
            match outcome {
                Ok(changed_files) => {
                    let reply = WorkerReply::UnstageChangedFileAck { changed_files };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::StageAllChanges { project_id } => {
            let outcome = registry.stage_all_changes(&project_id).await;
            match outcome {
                Ok(changed_files) => {
                    let reply = WorkerReply::StageAllChangesAck { changed_files };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::UnstageAllChanges { project_id } => {
            let outcome = registry.unstage_all_changes(&project_id).await;
            match outcome {
                Ok(changed_files) => {
                    let reply = WorkerReply::UnstageAllChangesAck { changed_files };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::DiscardChangedFile {
            project_id,
            path,
            untracked,
            original_path,
        } => {
            let outcome = registry
                .discard_changed_file(&project_id, &path, untracked, original_path.as_deref())
                .await;
            match outcome {
                Ok(changed_files) => {
                    let reply = WorkerReply::DiscardChangedFileAck { changed_files };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::DiscardAllChanges { project_id, files } => {
            let outcome = registry.discard_all_changes(&project_id, files).await;
            match outcome {
                Ok((changed_files, failures)) => {
                    let reply = WorkerReply::DiscardAllChangesAck {
                        changed_files,
                        failures,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::RunToolbarGitAction {
            project_id,
            action_id,
        } => {
            let outcome = registry
                .run_toolbar_git_action(&project_id, &action_id)
                .await;
            match outcome {
                Ok(outcome) => {
                    let reply = WorkerReply::ToolbarActionOutcomeAck { outcome };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::CreateBranch {
            project_id,
            branch_name,
            use_current_task,
            migrate_changes,
        } => {
            let outcome = registry
                .create_branch(&project_id, &branch_name, use_current_task, migrate_changes)
                .await;
            match outcome {
                Ok((section_id, projects)) => {
                    let reply = WorkerReply::CreateBranchAck {
                        section_id,
                        projects,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::RenameTask { task_id, new_name } => {
            let (changed, task) = registry.rename_task(&task_id, &new_name);
            let reply = WorkerReply::TaskRenamed { changed, task };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::SetTaskPinned { task_id, pinned } => {
            let (changed, task) = registry.set_task_pinned(&task_id, pinned);
            let reply = WorkerReply::TaskPinned { changed, task };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::RemoveTask {
            project_id,
            task_id,
        } => {
            let removed = registry.remove_task(&project_id, &task_id);
            let reply = WorkerReply::TaskRemoved {
                project_id,
                task_id,
                removed,
            };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::SlugifyBranchName { name } => {
            let slug = registry.slugify_branch_name(&name);
            let reply = WorkerReply::SlugifyBranchNameAck { slug };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::ReadProjectBranches { project_id } => {
            let branches = registry.read_project_branches(&project_id);
            let reply = WorkerReply::ProjectBranchesAck { branches };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::PrimaryBranchForProject { project_id } => {
            let branch = registry.primary_branch_for_project(&project_id);
            let reply = WorkerReply::PrimaryBranchAck { branch };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::RepoDefaultCommitAction { project_id } => {
            let action = registry.repo_default_commit_action(&project_id);
            let reply = WorkerReply::RepoDefaultCommitActionAck { action };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::ReadActiveGitState { project_id } => {
            let state = registry.read_active_git_state(&project_id);
            let reply = WorkerReply::ActiveGitStateAck { state };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::ReadChangedFiles { project_id } => {
            let files = registry.read_changed_files(&project_id);
            let reply = WorkerReply::ChangedFilesAck { files };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::ReadProjectGithubUrl { project_id } => {
            let url = registry.read_project_github_url(&project_id);
            let reply = WorkerReply::ProjectGithubUrlAck { url };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::ReadRecentCommits { project_id, limit } => {
            match registry.read_recent_commits(&project_id, limit as usize) {
                Ok(view) => {
                    let reply = WorkerReply::RecentCommitsAck { view };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(message) => {
                    send_err(outbound_tx, request_id, ErrKind::Internal, message).await?;
                }
            }
        }
        Control::ReadCommitFileChanges {
            project_id,
            commit_id,
        } => match registry.read_commit_file_changes(&project_id, &commit_id) {
            Ok(files) => {
                let reply = WorkerReply::CommitFileChangesAck { files };
                send_worker_reply(outbound_tx, request_id, &reply).await?;
            }
            Err(message) => {
                send_err(outbound_tx, request_id, ErrKind::Internal, message).await?;
            }
        },
        Control::ReadBranchCompareState {
            project_id,
            target_branch,
        } => match registry.read_branch_compare_state(&project_id, &target_branch) {
            Ok(view) => {
                let reply = WorkerReply::BranchCompareAck { view };
                send_worker_reply(outbound_tx, request_id, &reply).await?;
            }
            Err(message) => {
                send_err(outbound_tx, request_id, ErrKind::Internal, message).await?;
            }
        },
        Control::ReadBranchSettings { project_id } => {
            let settings = registry.read_branch_settings(&project_id);
            let reply = WorkerReply::BranchSettingsAck { settings };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::SetBranchSetting {
            project_id,
            field,
            branch_name,
        } => match registry.set_branch_setting(&project_id, &field, branch_name.as_deref()) {
            Ok(changed) => {
                let reply = WorkerReply::SetBranchSettingAck { changed };
                send_worker_reply(outbound_tx, request_id, &reply).await?;
            }
            Err(message) => {
                send_err(outbound_tx, request_id, ErrKind::Internal, message).await?;
            }
        },
        Control::CreateReviewTask {
            project_id,
            pull_request_number,
            head_branch,
            agent_provider,
        } => {
            let outcome = registry
                .create_review_task(
                    &project_id,
                    pull_request_number,
                    &head_branch,
                    agent_provider,
                )
                .await;
            match outcome {
                Ok((section_id, projects)) => {
                    let reply = WorkerReply::CreateReviewTaskAck {
                        section_id,
                        projects,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
                Err(e) => {
                    let reply = WorkerReply::Err {
                        message: format!("{e:#}"),
                        kind: ErrKind::Internal,
                    };
                    send_worker_reply(outbound_tx, request_id, &reply).await?;
                }
            }
        }
        Control::FindPullRequestStatus { project_id } => {
            // Pure read: route into the registry, marshal Ok(None) /
            // Ok(Some(_)) into PullRequestStatusAck, and convert any
            // hard failure into WorkerReply::Err so the channel
            // stays open for other in-flight requests on this
            // session.
            let reply = match registry.find_pull_request_status(&project_id) {
                Ok(status) => WorkerReply::PullRequestStatusAck { status },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::ReadPullRequestChecks { project_id } => {
            // Same shape as FindPullRequestStatus — Ok(Some/None) →
            // PullRequestChecksAck, Err → WorkerReply::Err. The
            // three-state contract is documented on
            // `DaemonRegistry::read_pull_request_checks`.
            let reply = match registry.read_pull_request_checks(&project_id) {
                Ok(checks) => WorkerReply::PullRequestChecksAck { checks },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }
        Control::FindProjectPullRequests {
            project_id,
            filter_index,
            query,
        } => {
            // Same Ok/Err split as the sibling PR readers above.
            // `Ok(None)` covers unknown-project; gh CLI / auth /
            // network errors land as WorkerReply::Err.
            let reply = match registry.find_project_pull_requests(&project_id, filter_index, &query)
            {
                Ok(prs) => WorkerReply::ProjectPullRequestsAck { prs },
                Err(message) => WorkerReply::Err {
                    message,
                    kind: ErrKind::Internal,
                },
            };
            send_worker_reply(outbound_tx, request_id, &reply).await?;
        }

        // ── Settings → Git Actions (`another-one-ojm.8`) ───────────
        Control::ReadGitActionScripts => {
            let view = registry.read_git_action_scripts();
            send_worker_reply(
                outbound_tx,
                request_id,
                &WorkerReply::GitActionScriptsAck { view },
            )
            .await?;
        }
        Control::SetGitCommitScript { script } => match registry.set_git_commit_script(&script) {
            Ok(changed) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::SetGitCommitScriptAck { changed },
                )
                .await?;
            }
            Err(message) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::Err {
                        message,
                        kind: crate::frame::ErrKind::Internal,
                    },
                )
                .await?;
            }
        },
        Control::ResetGitCommitScript => match registry.reset_git_commit_script() {
            Ok(changed) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::ResetGitCommitScriptAck { changed },
                )
                .await?;
            }
            Err(message) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::Err {
                        message,
                        kind: crate::frame::ErrKind::Internal,
                    },
                )
                .await?;
            }
        },
        Control::SetGitPrScript { script } => match registry.set_git_pr_script(&script) {
            Ok(changed) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::SetGitPrScriptAck { changed },
                )
                .await?;
            }
            Err(message) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::Err {
                        message,
                        kind: crate::frame::ErrKind::Internal,
                    },
                )
                .await?;
            }
        },
        Control::ResetGitPrScript => match registry.reset_git_pr_script() {
            Ok(changed) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::ResetGitPrScriptAck { changed },
                )
                .await?;
            }
            Err(message) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::Err {
                        message,
                        kind: crate::frame::ErrKind::Internal,
                    },
                )
                .await?;
            }
        },

        // ── Settings → Keybindings (`another-one-ojm.8`) ───────────
        Control::ReadShortcutSettings => {
            let view = registry.read_shortcut_settings();
            send_worker_reply(
                outbound_tx,
                request_id,
                &WorkerReply::ShortcutSettingsAck { view },
            )
            .await?;
        }
        Control::SetShortcutBinding { action_id, binding } => {
            match registry.set_shortcut_binding(&action_id, &binding) {
                Ok(()) => {
                    send_worker_reply(outbound_tx, request_id, &WorkerReply::SetShortcutBindingAck)
                        .await?;
                }
                Err(message) => {
                    let kind = if message.contains("unknown action id") {
                        crate::frame::ErrKind::UnknownId
                    } else {
                        crate::frame::ErrKind::Internal
                    };
                    send_worker_reply(outbound_tx, request_id, &WorkerReply::Err { message, kind })
                        .await?;
                }
            }
        }
        Control::ResetShortcutBinding { action_id } => {
            match registry.reset_shortcut_binding(&action_id) {
                Ok(()) => {
                    send_worker_reply(
                        outbound_tx,
                        request_id,
                        &WorkerReply::ResetShortcutBindingAck,
                    )
                    .await?;
                }
                Err(message) => {
                    let kind = if message.contains("unknown action id") {
                        crate::frame::ErrKind::UnknownId
                    } else {
                        crate::frame::ErrKind::Internal
                    };
                    send_worker_reply(outbound_tx, request_id, &WorkerReply::Err { message, kind })
                        .await?;
                }
            }
        }

        // ── Settings → MCP (`another-one-ojm.8`) ──────────────────
        Control::ReadMcpSettings => {
            let view = registry.read_mcp_settings();
            send_worker_reply(
                outbound_tx,
                request_id,
                &WorkerReply::McpSettingsAck { view },
            )
            .await?;
        }
        Control::McpAddFromCatalog { catalog_id } => {
            match registry.mcp_add_from_catalog(&catalog_id) {
                Ok(()) => {
                    send_worker_reply(outbound_tx, request_id, &WorkerReply::McpAddFromCatalogAck)
                        .await?;
                }
                Err(message) => {
                    send_worker_reply(
                        outbound_tx,
                        request_id,
                        &WorkerReply::Err {
                            message,
                            kind: crate::frame::ErrKind::Internal,
                        },
                    )
                    .await?;
                }
            }
        }
        Control::McpToggle {
            entry_id,
            provider_id,
            enabled,
        } => match registry.mcp_toggle(&entry_id, &provider_id, enabled) {
            Ok(()) => {
                send_worker_reply(outbound_tx, request_id, &WorkerReply::McpToggleAck).await?;
            }
            Err(message) => {
                let kind = if message.contains("unknown provider id") {
                    crate::frame::ErrKind::UnknownId
                } else {
                    crate::frame::ErrKind::Internal
                };
                send_worker_reply(outbound_tx, request_id, &WorkerReply::Err { message, kind })
                    .await?;
            }
        },
        Control::McpRemove { entry_id } => match registry.mcp_remove(&entry_id) {
            Ok(()) => {
                send_worker_reply(outbound_tx, request_id, &WorkerReply::McpRemoveAck).await?;
            }
            Err(message) => {
                send_worker_reply(
                    outbound_tx,
                    request_id,
                    &WorkerReply::Err {
                        message,
                        kind: crate::frame::ErrKind::Internal,
                    },
                )
                .await?;
            }
        },
    }
    Ok(())
}

/// Serialise a [`WorkerReply`] inside a [`WorkerReplyEnvelope`] tagged
/// with `request_id` and push it to the outbound writer task. Use
/// [`frame::PUSH_REQUEST_ID`] (= `0`) for daemon-originated frames
/// that aren't replying to a specific call (e.g. PTY data — though
/// data frames bypass this entirely via `TY_DATA`, the same id-0
/// rule applies if/when we add push variants of `WorkerReply`).
async fn send_worker_reply(
    outbound_tx: &OutboundTx,
    request_id: u64,
    reply: &WorkerReply,
) -> anyhow::Result<()> {
    let envelope = WorkerReplyEnvelope {
        request_id,
        reply: reply.clone(),
    };
    let payload = serde_json::to_vec(&envelope).context("serialize worker reply")?;
    outbound_tx
        .send(OutboundFrame {
            ty: frame::TY_WORKER_REPLY,
            payload,
            data_generation: None,
        })
        .await
        .map_err(|_| anyhow::anyhow!("outbound queue closed before worker reply was sent"))
}

/// Convenience wrapper around [`send_worker_reply`] for the
/// `Err`-frame failure mode used by the git-state read verbs in
/// `another-one-ojm.4`. Verbs that return `Result<_, String>` from
/// the registry route the `Err` arm through here so the connection
/// stays open for other in-flight requests on the same session.
async fn send_err(
    outbound_tx: &OutboundTx,
    request_id: u64,
    kind: ErrKind,
    message: String,
) -> anyhow::Result<()> {
    let reply = WorkerReply::Err { message, kind };
    send_worker_reply(outbound_tx, request_id, &reply).await
}

// ---- pairing / identity plumbing -------------------------------

enum PeerStatus {
    Paired,
    Unknown,
}

/// Generate a 128-bit random nonce as a 32-char hex string. Fits
/// cleanly in a URL query param and is long enough that brute-forcing
/// it over the network is infeasible on the timescale of a pairing
/// session.
pub(crate) fn generate_pair_nonce() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(32);
    for &b in &bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

/// Constant-time byte comparison. Returns false on length mismatch.
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut acc = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        acc |= x ^ y;
    }
    acc == 0
}

fn load_or_create_secret_key(path: &Path) -> anyhow::Result<SecretKey> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create secret key dir {}", parent.display()))?;
    }
    if let Ok(content) = std::fs::read_to_string(path) {
        let trimmed = content.trim();
        let bytes = hex_decode_32(trimmed)
            .with_context(|| format!("parse secret key at {}", path.display()))?;
        Ok(SecretKey::from_bytes(&bytes))
    } else {
        let sk = SecretKey::generate();
        let hex = hex_encode_32(&sk.to_bytes());
        std::fs::write(path, format!("{hex}\n"))
            .with_context(|| format!("write secret key to {}", path.display()))?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
        }
        info!("generated new persistent secret key at {}", path.display());
        Ok(sk)
    }
}

/// Classify a remote `NodeId` against the allowlist. `Paired` means
/// the peer is on the list and can proceed without a Hello frame;
/// `Unknown` means the peer must prove fresh pairing via
/// [`consume_hello`] before the daemon honours any control or data
/// frames. This function never mutates the allowlist — call
/// [`persist_pairing`] on successful Hello.
fn peer_status(remote_id: &str, path: &Path) -> anyhow::Result<PeerStatus> {
    use std::io::ErrorKind;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create allowlist dir {}", parent.display()))?;
    }
    let existing = match std::fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(anyhow::Error::from(e))
                .with_context(|| format!("read allowlist {}", path.display()));
        }
    };
    let paired = existing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .any(|peer| peer == remote_id);
    if paired {
        Ok(PeerStatus::Paired)
    } else {
        Ok(PeerStatus::Unknown)
    }
}

/// Append `remote_id` to the allowlist, creating the file with 0600
/// perms if needed. Called after a successful TOFU Hello — and from
/// the desktop bootstrap (`another-one-ojm.9`) to pre-allowlist its
/// own loopback-client NodeId so dialing the embedded daemon over
/// iroh skips the Hello dance, leaving the pair nonce intact for
/// real device pairing flows.
///
/// Idempotent — duplicate appends are harmless because `peer_status`
/// short-circuits on the first match.
pub fn persist_pairing(remote_id: &str, path: &Path) -> anyhow::Result<()> {
    use std::io::Write;

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create allowlist dir {}", parent.display()))?;
    }
    let line = format!("{remote_id}\n");
    let mut opts = std::fs::OpenOptions::new();
    opts.append(true).create(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    let mut f = opts
        .open(path)
        .with_context(|| format!("open allowlist {}", path.display()))?;
    f.write_all(line.as_bytes())
        .with_context(|| format!("write allowlist {}", path.display()))?;
    Ok(())
}

/// Build the `iroh://…?direct=…&relay=…&pair=…` URL remote clients
/// dial. The trailing `pair=<hex>` encodes the current TOFU nonce;
/// the client echoes it back as the `pair_token` field of
/// [`Control::Hello`] on its first control frame.
pub(crate) fn build_pairing_url_with_token(addr: &EndpointAddr, pair_token: &str) -> String {
    let direct = addr
        .ip_addrs()
        .map(|a| a.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let relay = addr
        .relay_urls()
        .next()
        .map(|r| r.to_string())
        .map(|r| urlencoding::encode(&r).into_owned());
    let mut url = format!("iroh://{}", addr.id);
    let mut have_query = false;
    if !direct.is_empty() {
        url.push_str(&format!("?direct={direct}"));
        have_query = true;
    }
    if let Some(relay) = relay {
        let sep = if have_query { '&' } else { '?' };
        url.push_str(&format!("{sep}relay={relay}"));
        have_query = true;
    }
    let sep = if have_query { '&' } else { '?' };
    url.push_str(&format!("{sep}pair={pair_token}"));
    url
}

/// Render a PNG of the pairing QR into a byte vec. No filesystem —
/// embedders hand the bytes straight to their UI (GPUI image, Slint
/// image, terminal PNG dumper, etc.).
pub(crate) fn render_qr_png_bytes(text: &str) -> anyhow::Result<Vec<u8>> {
    use image::{ImageFormat, Luma};
    use qrcode::QrCode;

    let code = QrCode::new(text.as_bytes()).context("QR encode")?;
    let image = code.render::<Luma<u8>>().min_dimensions(256, 256).build();
    let mut bytes: Vec<u8> = Vec::new();
    image
        .write_to(&mut std::io::Cursor::new(&mut bytes), ImageFormat::Png)
        .context("encode PNG")?;
    Ok(bytes)
}

fn hex_encode_32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode_32(s: &str) -> anyhow::Result<[u8; 32]> {
    if s.len() != 64 {
        anyhow::bail!("expected 64 hex chars, got {}", s.len());
    }
    let mut out = [0u8; 32];
    for (i, byte) in out.iter_mut().enumerate() {
        let hi = u8::from_str_radix(&s[i * 2..i * 2 + 1], 16).context("bad hex")?;
        let lo = u8::from_str_radix(&s[i * 2 + 1..i * 2 + 2], 16).context("bad hex")?;
        *byte = (hi << 4) | lo;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[derive(Default)]
    struct RecordingRegistry {
        inputs: Mutex<Vec<(String, String, Vec<u8>)>>,
    }

    impl DaemonRegistry for RecordingRegistry {
        fn list_projects(&self) -> Vec<crate::frame::ProjectSummary> {
            Vec::new()
        }

        fn attach_tab(
            &self,
            _section_id: &str,
            _tab_id: &str,
        ) -> Option<broadcast::Receiver<Vec<u8>>> {
            None
        }

        fn tab_input(&self, section_id: &str, tab_id: &str, bytes: &[u8]) {
            self.inputs.lock().unwrap().push((
                section_id.to_string(),
                tab_id.to_string(),
                bytes.to_vec(),
            ));
        }

        fn tab_resize(
            &self,
            _viewer_id: &str,
            _section_id: &str,
            _tab_id: &str,
            _cols: u16,
            _rows: u16,
        ) {
        }
    }

    struct UnhealthyRegistry;

    impl DaemonRegistry for UnhealthyRegistry {
        fn health(&self) -> Result<(), String> {
            Err("registry unavailable".to_string())
        }

        fn list_projects(&self) -> Vec<crate::frame::ProjectSummary> {
            panic!("list_projects should not be called when health fails");
        }

        fn attach_tab(
            &self,
            _section_id: &str,
            _tab_id: &str,
        ) -> Option<broadcast::Receiver<Vec<u8>>> {
            None
        }

        fn tab_input(&self, _section_id: &str, _tab_id: &str, _bytes: &[u8]) {}

        fn tab_resize(
            &self,
            _viewer_id: &str,
            _section_id: &str,
            _tab_id: &str,
            _cols: u16,
            _rows: u16,
        ) {
        }
    }

    fn test_pair_state(nonce: &str) -> Arc<Mutex<PairState>> {
        Arc::new(Mutex::new(PairState {
            nonce: Some(nonce.to_string()),
            addr: EndpointAddr::new(SecretKey::generate().public().into()),
            pairing_url: String::new(),
            qr_png_bytes: Vec::new(),
        }))
    }

    #[test]
    fn route_pty_input_forwards_terminal_mouse_bytes_unchanged() {
        let registry = RecordingRegistry::default();
        let attached = Attached {
            section_id: "section-1".to_string(),
            tab_id: "tab-1".to_string(),
            forwarder: None,
        };
        let mouse_bytes = b"\x1b[<0;12;34M\x1b[<0;12;34m".to_vec();

        route_pty_input(&registry, Some(&attached), &mouse_bytes);

        let inputs = registry.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].0, "section-1");
        assert_eq!(inputs[0].1, "tab-1");
        assert_eq!(inputs[0].2, mouse_bytes);
    }

    #[test]
    fn route_pty_input_drops_bytes_without_attachment() {
        let registry = RecordingRegistry::default();

        route_pty_input(&registry, None, b"\x1b[<64;1;1M");

        assert!(registry.inputs.lock().unwrap().is_empty());
    }

    #[test]
    fn route_terminal_input_event_encodes_typed_paste() {
        let registry = RecordingRegistry::default();
        let attached = Attached {
            section_id: "section-1".to_string(),
            tab_id: "tab-1".to_string(),
            forwarder: None,
        };
        let event = frame::TerminalInputEvent::Paste {
            text: "hello".to_string(),
            bracketed: true,
        };

        route_terminal_input_event(&registry, Some(&attached), &event);

        let inputs = registry.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].2, b"\x1b[200~hello\x1b[201~");
    }

    #[tokio::test]
    async fn handle_control_routes_tab_input_to_attached_tab() {
        let registry = Arc::new(RecordingRegistry::default());
        let registry_dyn: Arc<dyn DaemonRegistry> = registry.clone();
        let (outbound_tx, _outbound_rx) = mpsc::channel(1);
        let data_generation = Arc::new(AtomicU64::new(0));
        let mut attached = Some(Attached {
            section_id: "section-1".to_string(),
            tab_id: "tab-1".to_string(),
            forwarder: None,
        });

        handle_control(
            11,
            Control::TabInput {
                event: frame::TerminalInputEvent::Focus { focused: true },
            },
            &registry_dyn,
            &outbound_tx,
            &data_generation,
            &mut attached,
            "viewer-1",
        )
        .await
        .expect("tab input control routes");

        let inputs = registry.inputs.lock().unwrap();
        assert_eq!(inputs.len(), 1);
        assert_eq!(inputs[0].0, "section-1");
        assert_eq!(inputs[0].1, "tab-1");
        assert_eq!(inputs[0].2, b"\x1b[I");
    }

    #[tokio::test]
    async fn handle_control_returns_err_when_registry_health_fails() {
        let registry: Arc<dyn DaemonRegistry> = Arc::new(UnhealthyRegistry);
        let (outbound_tx, mut outbound_rx) = mpsc::channel(1);
        let data_generation = Arc::new(AtomicU64::new(0));
        let mut attached = None;

        handle_control(
            42,
            Control::ListProjects,
            &registry,
            &outbound_tx,
            &data_generation,
            &mut attached,
            "viewer-1",
        )
        .await
        .expect("health failure should be encoded as a worker reply");

        let frame = outbound_rx.recv().await.expect("worker reply");
        assert_eq!(frame.ty, crate::frame::TY_WORKER_REPLY);
        let envelope: WorkerReplyEnvelope =
            serde_json::from_slice(&frame.payload).expect("decode worker reply");
        assert_eq!(envelope.request_id, 42);
        match envelope.reply {
            WorkerReply::Err { message, kind } => {
                assert_eq!(message, "registry unavailable");
                assert!(matches!(kind, ErrKind::Internal));
            }
            other => panic!("expected WorkerReply::Err, got {other:?}"),
        }
    }

    #[test]
    fn hex_roundtrips() {
        let bytes = [
            0xde, 0xad, 0xbe, 0xef, 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17,
            18, 19, 20, 21, 22, 23, 24, 25, 26, 27,
        ];
        let s = hex_encode_32(&bytes);
        assert_eq!(s.len(), 64);
        let back = hex_decode_32(&s).unwrap();
        assert_eq!(back, bytes);
    }

    #[test]
    fn render_qr_png_produces_png_magic_bytes() {
        let png = render_qr_png_bytes("iroh://test").unwrap();
        assert!(png.len() > 100);
        assert_eq!(
            &png[..8],
            &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']
        );
    }

    #[test]
    fn consume_hello_persists_peer_and_consumes_nonce() {
        let dir = tempdir().unwrap();
        let allowlist = dir.path().join("paired_peers");
        let pair_state = test_pair_state("abc123");

        consume_hello(
            Control::Hello {
                pair_token: Some("abc123".to_string()),
                protocol_version: PROTOCOL_VERSION,
            },
            "peer-1",
            &pair_state,
            &allowlist,
        )
        .unwrap();

        let stored = std::fs::read_to_string(&allowlist).unwrap();
        assert_eq!(stored, "peer-1\n");
        assert_eq!(
            pair_state.lock().unwrap_or_else(|p| p.into_inner()).nonce,
            None
        );
    }

    #[test]
    fn consume_hello_keeps_nonce_when_allowlist_write_fails() {
        let dir = tempdir().unwrap();
        let pair_state = test_pair_state("abc123");

        let err = consume_hello(
            Control::Hello {
                pair_token: Some("abc123".to_string()),
                protocol_version: PROTOCOL_VERSION,
            },
            "peer-1",
            &pair_state,
            dir.path(),
        )
        .unwrap_err();

        assert!(err.to_string().contains("open allowlist"));
        assert_eq!(
            pair_state.lock().unwrap_or_else(|p| p.into_inner()).nonce,
            Some("abc123".to_string())
        );
    }
}
