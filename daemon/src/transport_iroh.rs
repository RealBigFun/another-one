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
//! as `TY_DATA` frames. Inbound `TY_DATA` is routed to the attached
//! tab's PTY input.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use anyhow::Context;
use iroh::endpoint::{presets, Connection, Incoming, RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, SecretKey};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tracing::{debug, info, warn};

use crate::dispatch::{serve_session_with_attach, AttachState};
use crate::frame::{read_frame, write_frame};
use daemon_proto::{
    Control, ControlEnvelope, WorkerReply, WorkerReplyEnvelope, PUSH_REQUEST_ID, TY_CONTROL,
    TY_DATA, TY_WORKER_REPLY,
};
use crate::registry::{DaemonRegistry, EndpointHandle, PairState};
use daemon_transport::{
    RequestId, ServerSession, SessionFuture, TransportError,
};

use daemon_proto::{ALPN, PROTOCOL_VERSION};

/// QUIC close reason emitted to unauthorised peers. Short on purpose:
/// the CONNECTION_CLOSE frame is observable on the wire, so long
/// user-facing copy here would leak product UX text to an on-path
/// observer. Clients match on this byte string and expand it into
/// localisable copy ("Pairing expired — please re-scan the QR")
/// in the UI. Keep in lockstep with the substring match in
/// `mobile/lib/src/transport_iroh.dart::_statusForError`.
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
        viewer_id.clone(),
        authz,
        paired_peers_path,
        pair_state,
    )
    .await;
    // viewer_disconnected is also called by serve_session on its way
    // out; calling it again here is a defensive no-op (idempotent for
    // unknown viewer ids) that catches the pre-Hello / handshake
    // failure paths where serve_session never ran.
    registry.viewer_disconnected(&viewer_id);
    result
}

#[derive(Clone, Copy)]
enum PostAuth {
    AlreadyPaired,
    AwaitHello,
}

/// Drive one accepted iroh connection through pairing and then hand
/// it off to the transport-agnostic dispatcher. Splits the lifecycle
/// into two phases:
///
///   1. **Pre-handshake** (this function, in-line). Read frames until
///      the peer is authorised (paired-list match or successful
///      `Control::Hello` TOFU). Bytes that arrive before the Hello
///      are bounded — control frames must be `Hello`, raw `TY_DATA`
///      from an unpaired peer is a hard reject.
///   2. **Post-handshake**. Construct an [`IrohServerSession`] over the
///      same bidi streams and run [`serve_session_with_attach`] against
///      it. The dispatcher owns verb routing, attach lifecycle, and
///      forwarder spawning; this function only sees its return value.
async fn handle_connection(
    conn: Connection,
    registry: Arc<dyn DaemonRegistry>,
    viewer_id: String,
    mut authz: PostAuth,
    paired_peers_path: &Path,
    pair_state: Arc<Mutex<PairState>>,
) -> anyhow::Result<()> {
    let (send, mut recv) = conn.accept_bi().await.context("accept_bi")?;

    // Pre-handshake: while authz == AwaitHello, consume control frames
    // looking for a valid Hello. Anything else (TY_DATA, mid-stream
    // control verbs) is a hard reject. Once authz flips to
    // AlreadyPaired we drop out of the loop and run the dispatcher
    // over an IrohServerSession that owns the streams.
    while matches!(authz, PostAuth::AwaitHello) {
        match read_frame(&mut recv).await {
            Ok(Some((TY_DATA, _))) => {
                warn!(viewer_id, "pre-Hello data from unpaired peer; rejecting");
                conn.close(1u8.into(), CLOSE_REASON_UNPAIRED);
                return Ok(());
            }
            Ok(Some((TY_CONTROL, payload))) => {
                let envelope = match serde_json::from_slice::<ControlEnvelope>(&payload) {
                    Ok(e) => e,
                    Err(e) => {
                        warn!(error = %e, "bad iroh control frame during pairing");
                        continue;
                    }
                };
                let ControlEnvelope {
                    request_id: _,
                    control: ctrl,
                } = envelope;
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
                        return Ok(());
                    }
                }
                match consume_hello(ctrl, &viewer_id, &pair_state, paired_peers_path) {
                    Ok(()) => {
                        authz = PostAuth::AlreadyPaired;
                        info!(viewer_id, "TOFU pair complete");
                    }
                    Err(e) => {
                        warn!(viewer_id, error = %e, "rejecting unpaired peer");
                        conn.close(1u8.into(), CLOSE_REASON_UNPAIRED);
                        return Ok(());
                    }
                }
            }
            Ok(Some((ty, _))) => warn!(frame_type = ty, "unknown iroh frame type during pairing"),
            Ok(None) => {
                debug!("iroh peer closed send before Hello");
                return Ok(());
            }
            Err(e) => {
                warn!(error = %e, "iroh frame read failed during pairing");
                return Ok(());
            }
        }
    }

    // Already-paired (or just-paired): hand the streams + registry to
    // the abstract dispatcher. The session owns its own attach state
    // so its frame loop can route inbound TY_DATA into
    // `registry.tab_input` based on the live attach key.
    let attach = Arc::new(AttachState::new());
    let session = Arc::new(IrohServerSession::new(
        send,
        recv,
        viewer_id.clone(),
        Arc::clone(&registry),
        Arc::clone(&attach),
    )) as Arc<dyn ServerSession>;

    if let Err(e) = serve_session_with_attach(session, registry, attach).await {
        debug!(viewer_id, error = %e, "iroh session ended with error");
    }
    info!("iroh session ended");
    Ok(())
}

/// Server-side `ServerSession` impl backed by an iroh QUIC bidi
/// stream. Wraps:
///
///   - the bidi `SendStream` + `RecvStream`,
///   - an outbound mpsc the writer task drains (so the dispatcher's
///     `reply` / `push_data` calls can come from any task),
///   - the registry handle (to route inbound `TY_DATA` to
///     `registry.tab_input` per the live attach key),
///   - the per-session [`AttachState`] (read-only here — the
///     dispatcher mutates it on `AttachTab` / `DetachTab`).
struct IrohServerSession {
    peer_id: String,
    /// Recv half plus the registry / attach state we need to route
    /// inbound `TY_DATA` frames into the registry's tab input. Held
    /// behind an `AsyncMutex` because `next_call` is called from the
    /// dispatcher loop which is `&self` only.
    incoming: AsyncMutex<IncomingHalf>,
    outbound_tx: mpsc::Sender<OutboundFrame>,
    /// Writer task handle. Dropped on `close()` so the writer task
    /// finishes draining its queue.
    writer_task: Mutex<Option<tokio::task::JoinHandle<()>>>,
}

struct IncomingHalf {
    recv: RecvStream,
    registry: Arc<dyn DaemonRegistry>,
    attach: Arc<AttachState>,
}

#[derive(Debug)]
struct OutboundFrame {
    ty: u8,
    payload: Vec<u8>,
}

impl IrohServerSession {
    fn new(
        send: SendStream,
        recv: RecvStream,
        peer_id: String,
        registry: Arc<dyn DaemonRegistry>,
        attach: Arc<AttachState>,
    ) -> Self {
        let (outbound_tx, mut outbound_rx) = mpsc::channel::<OutboundFrame>(64);
        let mut send = send;
        let writer_task = tokio::spawn(async move {
            while let Some(frame) = outbound_rx.recv().await {
                if let Err(e) = write_frame(&mut send, frame.ty, &frame.payload).await {
                    debug!(error = %e, "iroh frame write failed");
                    break;
                }
            }
            let _ = send.finish();
        });
        // We don't retain the iroh `Connection` here. `close()` aborts
        // the writer task and lets the stream drop emit FIN; concrete
        // QUIC close-with-reason support belongs in
        // `handle_connection` (which holds the Connection) and lands
        // when a caller needs it.
        Self {
            peer_id,
            incoming: AsyncMutex::new(IncomingHalf {
                recv,
                registry,
                attach,
            }),
            outbound_tx,
            writer_task: Mutex::new(Some(writer_task)),
        }
    }
}

impl ServerSession for IrohServerSession {
    fn peer_id(&self) -> &str {
        &self.peer_id
    }

    fn next_call<'a>(
        &'a self,
    ) -> SessionFuture<'a, Result<Option<(RequestId, Control)>, TransportError>> {
        Box::pin(async move {
            let mut incoming = self.incoming.lock().await;
            loop {
                match read_frame(&mut incoming.recv).await {
                    Ok(Some((TY_DATA, payload))) => {
                        // Route directly into the registry's tab
                        // input based on the live attach target. No
                        // attachment → silently drop (clients may
                        // type during the AttachTab → first-reply
                        // race).
                        if let Some((section_id, tab_id)) = incoming.attach.snapshot_target() {
                            incoming
                                .registry
                                .tab_input(&section_id, &tab_id, &payload);
                        }
                    }
                    Ok(Some((TY_CONTROL, payload))) => {
                        let envelope: ControlEnvelope = match serde_json::from_slice(&payload) {
                            Ok(e) => e,
                            Err(e) => {
                                warn!(error = %e, "bad iroh control frame");
                                continue;
                            }
                        };
                        return Ok(Some((
                            RequestId(envelope.request_id),
                            envelope.control,
                        )));
                    }
                    Ok(Some((ty, _))) => {
                        warn!(frame_type = ty, "unknown iroh frame type");
                    }
                    Ok(None) => {
                        debug!("iroh peer closed send");
                        return Ok(None);
                    }
                    Err(e) => {
                        return Err(TransportError::Other(format!("iroh frame read: {e:#}")));
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
            let envelope = WorkerReplyEnvelope {
                request_id: request_id.0,
                reply,
            };
            let payload = serde_json::to_vec(&envelope)
                .map_err(|e| TransportError::Encoding(format!("worker reply: {e}")))?;
            self.outbound_tx
                .send(OutboundFrame {
                    ty: TY_WORKER_REPLY,
                    payload,
                })
                .await
                .map_err(|_| TransportError::Closed(Some("outbound queue closed".into())))
        })
    }

    fn push_data<'a>(
        &'a self,
        _section_id: &'a str,
        _tab_id: &'a str,
        bytes: &'a [u8],
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        // Today's wire is a single TY_DATA fan — bytes for the
        // (single) currently-attached tab go untagged. The
        // (section_id, tab_id) plumbing exists for the future
        // multi-attach world; for now we ignore them on the wire and
        // rely on the client's attach state to demultiplex.
        let payload = bytes.to_vec();
        Box::pin(async move {
            self.outbound_tx
                .send(OutboundFrame {
                    ty: TY_DATA,
                    payload,
                })
                .await
                .map_err(|_| TransportError::Closed(Some("outbound queue closed".into())))
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
                .map_err(|e| TransportError::Encoding(format!("push reply: {e}")))?;
            self.outbound_tx
                .send(OutboundFrame {
                    ty: TY_WORKER_REPLY,
                    payload,
                })
                .await
                .map_err(|_| TransportError::Closed(Some("outbound queue closed".into())))
        })
    }

    fn close<'a>(
        &'a self,
        _reason: Option<&'a [u8]>,
    ) -> SessionFuture<'a, Result<(), TransportError>> {
        // Drop the writer task's join handle; the writer task ends
        // when the outbound channel closes (which happens when the
        // last sender is dropped — i.e. when this session is dropped
        // entirely). For an explicit close, abort the writer task so
        // any queued frames are discarded.
        let handle = self
            .writer_task
            .lock()
            .expect("writer task lock poisoned")
            .take();
        Box::pin(async move {
            if let Some(handle) = handle {
                handle.abort();
            }
            Ok(())
        })
    }
}

impl Drop for IrohServerSession {
    fn drop(&mut self) {
        if let Ok(mut slot) = self.writer_task.lock() {
            if let Some(handle) = slot.take() {
                handle.abort();
            }
        }
    }
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
/// real mobile pairing flows.
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

/// Build the `iroh://…?direct=…&relay=…&pair=…` URL the mobile
/// client dials. The trailing `pair=<hex>` encodes the current TOFU
/// nonce; the mobile client echoes it back as the `pair_token` field
/// of [`Control::Hello`] on its first control frame.
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
/// embedders hand the bytes straight to their UI (GPUI image,
/// Flutter image, terminal PNG dumper, etc.).
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

// ── iroh stream → frame trait adapters ────────────────────────────
//
// Live here so `daemon::frame` stays transport-agnostic. New
// transports add their own `ReadExactish` / `WriteAllAsync` impls
// next to their stream types; no changes here are needed for the
// daemon's framing logic.

impl crate::frame::ReadExactish for iroh::endpoint::RecvStream {
    async fn read_exactish(
        &mut self,
        buf: &mut [u8],
    ) -> anyhow::Result<crate::frame::ReadOutcome> {
        let mut read = 0;
        while read < buf.len() {
            match self.read(&mut buf[read..]).await {
                Ok(Some(0)) | Ok(None) => {
                    return if read == 0 {
                        Ok(crate::frame::ReadOutcome::Closed)
                    } else {
                        Err(anyhow::anyhow!(
                            "stream closed mid-read after {read} of {} bytes",
                            buf.len()
                        ))
                    };
                }
                Ok(Some(n)) => {
                    read += n;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(crate::frame::ReadOutcome::Got)
    }
}

impl crate::frame::WriteAllAsync for iroh::endpoint::SendStream {
    async fn write_all_async(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.write_all(data).await.map_err(Into::into)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn test_pair_state(nonce: &str) -> Arc<Mutex<PairState>> {
        Arc::new(Mutex::new(PairState {
            nonce: Some(nonce.to_string()),
            addr: EndpointAddr::new(SecretKey::generate().public().into()),
            pairing_url: String::new(),
            qr_png_bytes: Vec::new(),
        }))
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
