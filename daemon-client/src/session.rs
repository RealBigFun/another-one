//! Live iroh client session: bind a local endpoint, connect to a
//! daemon by pairing URL, send the `Hello` handshake, and pump frames
//! until close. Streams PTY bytes and `WorkerReply`s back to the
//! caller via `tokio::sync::mpsc` channels (the legacy
//! `mobile-core::IrohSession` plumbed these into FRB `StreamSink`s;
//! we leave them as plain channels and let the UI layer adapt).
//!
//! Ported from `mobile-core/src/api/iroh_client.rs` lines ~310-720.
//! All `#[frb(...)]` attributes and `StreamSink` plumbing have been
//! removed; UI code drains incoming bytes / worker replies via the
//! polling [`Session::next_incoming_bytes`] /
//! [`Session::next_worker_reply`] methods. Persistent secret keys
//! (legacy `load_or_create_device_secret_key`) are deliberately
//! omitted — every dial uses an ephemeral [`SecretKey::generate`].
//! Persistence is a follow-up.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};

use anyhow::Context;
use std::sync::Mutex as StdMutex;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot, Mutex};

use iroh::dns::DnsResolver;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl, SecretKey};

use crate::frame::{read_frame, write_frame};
use crate::pairing_url::parse_pairing_url;
use crate::status::{push_status, DialStatus};
use daemon_proto::{
    Control, ControlEnvelope, WorkerReply, WorkerReplyEnvelope, ALPN, PROTOCOL_VERSION, TY_CONTROL,
    TY_DATA, TY_WORKER_REPLY,
};

/// Dedicated tokio runtime for all iroh work. Callers may live on any
/// executor (or none at all — the GPUI desktop app drives this from
/// its background executor); shuffling onto a dedicated multi-thread
/// tokio runtime keeps iroh's UDP sockets and internal actors driven
/// regardless of what the host is doing. Same shape as the legacy
/// `mobile-core::tokio_rt`.
fn tokio_rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("daemon-client")
            .build()
            .expect("build tokio runtime")
    })
}

/// Load the iroh-client secret key from `path`, or generate and
/// persist a fresh one if the file doesn't exist. Mirrors the
/// server-side `load_or_create_secret_key` in
/// `daemon::transport_iroh` (same hex-encoded 32-byte shape,
/// trimmed on read) so the two sides can swap implementations if
/// the types ever unify. Writes go through a straight `fs::write`
/// today — crash-consistency fsync is the daemon-side concern in
/// #57, mirror here when the corresponding client-side failure
/// surfaces.
fn load_or_create_client_secret_key(path: &std::path::Path) -> anyhow::Result<SecretKey> {
    use anyhow::Context;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create iroh key dir {}", parent.display()))?;
    }
    if let Ok(content) = std::fs::read_to_string(path) {
        let trimmed = content.trim();
        let bytes = hex_decode_32(trimmed)
            .with_context(|| format!("parse iroh key at {}", path.display()))?;
        return Ok(SecretKey::from_bytes(&bytes));
    }
    let sk = SecretKey::generate();
    let hex = hex_encode_32(&sk.to_bytes());
    std::fs::write(path, format!("{hex}\n"))
        .with_context(|| format!("write iroh key to {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    tracing::info!(path = %path.display(), "daemon-client: generated new persistent secret key");
    Ok(sk)
}

fn hex_encode_32(bytes: &[u8; 32]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(64);
    for b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 0xf) as usize] as char);
    }
    out
}

fn hex_decode_32(s: &str) -> anyhow::Result<[u8; 32]> {
    if s.len() != 64 {
        anyhow::bail!("secret key must be 64 hex chars, got {}", s.len());
    }
    let mut out = [0u8; 32];
    for (i, byte_out) in out.iter_mut().enumerate() {
        let hi = hex_nibble(
            s.as_bytes()
                .get(i * 2)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("hex index out of bounds"))?,
        )?;
        let lo = hex_nibble(
            s.as_bytes()
                .get(i * 2 + 1)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("hex index out of bounds"))?,
        )?;
        *byte_out = (hi << 4) | lo;
    }
    Ok(out)
}

fn hex_nibble(c: u8) -> anyhow::Result<u8> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(10 + c - b'a'),
        b'A'..=b'F' => Ok(10 + c - b'A'),
        _ => anyhow::bail!("invalid hex digit {c:#x}"),
    }
}

/// Events the UI may want beyond raw incoming bytes / worker replies.
/// Placeholder for now — the connect/recv plumbing currently emits no
/// `SessionEvent`s; growth happens as UI code asks for more signal.
#[derive(Debug, Clone)]
pub enum SessionEvent {
    /// The frame reader loop exited (clean EOF or error). Surfaced so
    /// the UI can stop showing "connected" without polling the recv
    /// channels for `None`.
    Disconnected,
}

/// Opaque handle to a live iroh QUIC session. UI code holds this and
/// drives the daemon via the `send` / `resize` / `attach_tab` / etc.
/// methods, draining inbound traffic with `next_incoming_bytes` and
/// `next_worker_reply`.
pub struct Session {
    /// The local endpoint we bound for this session. Closed on Drop.
    _endpoint: Endpoint,
    /// Sends framed messages (ty, payload) from callers to the writer
    /// task, which writes them into the QUIC send stream. `None`
    /// means closed.
    send_tx: Mutex<Option<mpsc::Sender<(u8, Vec<u8>)>>>,
    /// Holds the bytes-from-daemon receiver. UI code drains via
    /// [`Session::next_incoming_bytes`].
    incoming_rx: Mutex<Option<mpsc::Receiver<(String, String, Vec<u8>)>>>,
    /// Holds decoded worker replies (from `TY_WORKER_REPLY` frames).
    /// UI code drains via [`Session::next_worker_reply`].
    worker_replies_rx: Mutex<Option<mpsc::Receiver<WorkerReply>>>,
    /// Closes the underlying connection when invoked.
    closer: Mutex<Option<oneshot::Sender<()>>>,
    /// Per-call request id, bumped for every `Control` envelope. The
    /// daemon echoes it in the matching `WorkerReplyEnvelope.request_id`
    /// so callers can correlate responses to requests when multiple
    /// calls are in flight. Starts at 2 because Hello (sent during
    /// `connect`) used 1 — keeping the counter monotonic-from-1
    /// across the session avoids a "did Hello succeed?" ambiguity.
    next_request_id: AtomicU64,
    /// Pending [`Session::call`] awaiters keyed by their assigned
    /// `request_id`. The recv loop matches incoming
    /// `WorkerReplyEnvelope.request_id` against this map and routes
    /// the reply to the awaiting oneshot. Replies whose `request_id`
    /// has no entry (push frames with `request_id == 0`, or replies
    /// whose caller already gave up) fall through to the legacy
    /// FIFO `worker_replies_rx` channel so today's polling consumers
    /// keep working unchanged.
    pending_calls: Arc<StdMutex<HashMap<u64, oneshot::Sender<WorkerReply>>>>,
}

/// Dial a daemon's iroh endpoint by pairing URL. The URL carries the
/// daemon's `EndpointId`, direct addrs, optional relay URLs, and the
/// TOFU `pair` token. On success, the returned [`Session`] is live —
/// the `Hello` frame has been queued (and the channel-send confirmed)
/// and both the send and recv frame tasks are running on the shared
/// tokio runtime.
///
/// Status events are pushed to the process-wide queue in
/// [`crate::status`] as the dial progresses (`Started`, `Bound`,
/// `Connected`, `HelloSent`, or `Error`).
///
/// Wrapped in `tokio_rt().spawn(...).await` so callers can be on any
/// executor — same pattern as the legacy `iroh_connect`.
pub async fn connect(pairing_url: &str) -> anyhow::Result<Session> {
    connect_with_secret_key(pairing_url, None).await
}

/// `connect`, but with an optional on-disk path to persist the
/// iroh-client secret key at. When `None`, the dial uses an
/// ephemeral `SecretKey::generate()` — the historical behaviour,
/// fine for one-shot tools and the `daemon-sandbox` smoke test.
///
/// When `Some(path)`, a 32-byte hex key file at that path is
/// loaded; if absent, a fresh key is generated, written to the
/// path, and returned. Persisting the identity across dials keeps
/// the daemon's allowlist entry stable — without it, every
/// reconnect after the first pair is rejected as an unpaired peer
/// because the TOFU nonce only rolls on explicit reset.
///
/// Mobile callers pass the app's internal-data path
/// (`AndroidApp::internal_data_path`), which survives
/// `adb install -r` but wipes on uninstall — matching the
/// lifecycle of the pairing relationship itself.
pub async fn connect_with_secret_key(
    pairing_url: &str,
    secret_key_path: Option<std::path::PathBuf>,
) -> anyhow::Result<Session> {
    let url = pairing_url.to_string();
    tokio_rt()
        .spawn(async move { connect_inner(url, secret_key_path).await })
        .await
        .map_err(|e| anyhow::anyhow!("connect task panicked: {e}"))?
}

async fn connect_inner(
    pairing_url: String,
    secret_key_path: Option<std::path::PathBuf>,
) -> anyhow::Result<Session> {
    // Wrap the body so any early-return error gets surfaced as a
    // `DialStatus::Error` before being propagated to the caller.
    match connect_inner_impl(pairing_url, secret_key_path).await {
        Ok(session) => Ok(session),
        Err(e) => {
            push_status(DialStatus::Error(e.to_string()));
            Err(e)
        }
    }
}

async fn connect_inner_impl(
    pairing_url: String,
    secret_key_path: Option<std::path::PathBuf>,
) -> anyhow::Result<Session> {
    let parsed = parse_pairing_url(&pairing_url).context("parse pairing url")?;
    tracing::info!(
        endpoint_id = %parsed.endpoint_id,
        direct = ?parsed.direct_addrs,
        relays = ?parsed.relay_urls,
        "daemon-client connect: parsed pairing url",
    );
    push_status(DialStatus::Started {
        endpoint_id: parsed.endpoint_id.clone(),
    });

    let id: EndpointId = parsed
        .endpoint_id
        .trim()
        .parse()
        .context("invalid EndpointId")?;

    // Parse direct addresses eagerly so bad input surfaces before bind.
    let parsed_addrs: Vec<std::net::SocketAddr> = parsed
        .direct_addrs
        .iter()
        .map(|s| {
            s.parse::<std::net::SocketAddr>()
                .map_err(|e| anyhow::anyhow!("bad direct addr {s:?}: {e}"))
        })
        .collect::<anyhow::Result<_>>()?;
    let parsed_relays: Vec<RelayUrl> = parsed
        .relay_urls
        .iter()
        .map(|s| {
            s.parse::<RelayUrl>()
                .map_err(|e| anyhow::anyhow!("bad relay url {s:?}: {e}"))
        })
        .collect::<anyhow::Result<_>>()?;
    if parsed_addrs.is_empty() && parsed_relays.is_empty() {
        return Err(anyhow::anyhow!(
            "at least one direct address or relay URL is required \
             (sandbox has no address lookup)"
        ));
    }

    // Relay mode: if the caller gave us a relay URL, honour it (N0's
    // dev mesh lives behind `RelayMode::Default`). Otherwise stay
    // disabled for the LAN-only direct path.
    let relay_mode = if parsed_relays.is_empty() {
        RelayMode::Disabled
    } else {
        RelayMode::Default
    };
    tracing::info!(
        ?relay_mode,
        "daemon-client connect: binding (Minimal preset, explicit DNS)",
    );
    // Explicit Cloudflare DNS by default. Override via `ANOTHERONE_DNS`
    // env var (any `<ip>:<port>` parseable as a `SocketAddr` works).
    // Falling back silently on parse error keeps a fat-fingered env
    // var from bricking the dial.
    let dns_addr: std::net::SocketAddr = std::env::var("ANOTHERONE_DNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "1.1.1.1:53".parse().expect("static ipv4 socket addr"));
    tracing::info!(%dns_addr, "daemon-client connect: using configured DNS resolver");
    let dns = DnsResolver::with_nameserver(dns_addr);
    // Client-side iroh identity. When `secret_key_path` is `Some`,
    // persist-or-create so the daemon's allowlist entry stays
    // valid across reconnects (see `connect_with_secret_key`
    // docs). When `None`, fall through to the legacy
    // ephemeral-per-dial behaviour.
    let secret_key = match secret_key_path.as_deref() {
        Some(path) => load_or_create_client_secret_key(path)?,
        None => SecretKey::generate(),
    };
    let endpoint = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Endpoint::builder(presets::Minimal)
            .secret_key(secret_key)
            .relay_mode(relay_mode)
            .alpns(vec![])
            .dns_resolver(dns)
            .bind(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("bind timed out after 15s (Minimal+DNS)"))?
    .context("bind client endpoint")?;
    tracing::info!("daemon-client connect: endpoint bound, dialing {}", id);
    push_status(DialStatus::Bound);

    let mut addr = EndpointAddr::new(id);
    for sa in &parsed_addrs {
        addr = addr.with_ip_addr(*sa);
    }
    for url in parsed_relays {
        addr = addr.with_relay_url(url);
    }

    let conn = tokio::time::timeout(
        std::time::Duration::from_secs(10),
        endpoint.connect(addr, ALPN),
    )
    .await
    .map_err(|_| anyhow::anyhow!("connect timed out after 10s"))?
    .context("connect to daemon")?;
    tracing::info!("daemon-client connect: connected");
    push_status(DialStatus::Connected);

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;
    tracing::info!("daemon-client connect: opened bidi stream");

    // Outbound pipe: callers → channel → framed writes to iroh send
    // stream. Channel items are already-typed `(ty, payload)` pairs so
    // the writer task doesn't need to know the protocol.
    let (send_tx, mut send_rx) = mpsc::channel::<(u8, Vec<u8>)>(64);
    // First frame MUST be `Control::Hello` so the daemon can complete
    // TOFU pairing before any other control / data frames arrive. The
    // daemon ignores Hello from already-paired peers, so sending it
    // unconditionally is safe. Send via the mpsc so ordering is
    // preserved with whatever the caller queues next.
    //
    // Wrapped in `ControlEnvelope` per the daemon's wire format —
    // `request_id = 1` (we reserve `0` for unsolicited push frames).
    // `protocol_version` MUST equal `PROTOCOL_VERSION`; mismatch
    // closes the connection with `anotherone/incompatible-version`
    // before any other frames flow.
    let hello_payload = serde_json::to_vec(&ControlEnvelope {
        request_id: 1,
        control: Control::Hello {
            pair_token: parsed.pair_token,
            protocol_version: PROTOCOL_VERSION,
        },
    })
    .context("encode hello")?;
    send_tx
        .send((TY_CONTROL, hello_payload))
        .await
        .map_err(|_| anyhow::anyhow!("send channel closed before hello"))?;
    push_status(DialStatus::HelloSent);

    tokio_rt().spawn(async move {
        while let Some((ty, payload)) = send_rx.recv().await {
            if let Err(e) = write_frame(&mut send, ty, &payload).await {
                tracing::debug!(error = %e, "iroh frame write failed");
                break;
            }
        }
        let _ = send.finish();
    });

    // Inbound pipe: framed reads from iroh → per-frame-type channel →
    // caller (via the polling next_incoming_bytes / next_worker_reply
    // methods). Type=0 frames carry PTY output; type=2 frames carry
    // JSON-encoded `WorkerReply`s. Type=1 (server→client control) is
    // reserved for future use. Unknown types are logged and dropped
    // so older clients stay forwards-compatible as the daemon adds
    // variants.
    let (incoming_tx, incoming_rx) = mpsc::channel::<(String, String, Vec<u8>)>(128);
    let (worker_replies_tx, worker_replies_rx) = mpsc::channel::<WorkerReply>(64);
    let (close_tx, mut close_rx) = oneshot::channel::<()>();
    let conn_for_close = conn.clone();
    let pending_calls: Arc<StdMutex<HashMap<u64, oneshot::Sender<WorkerReply>>>> =
        Arc::new(StdMutex::new(HashMap::new()));
    let pending_calls_recv = Arc::clone(&pending_calls);
    tokio_rt().spawn(async move {
        loop {
            tokio::select! {
                _ = &mut close_rx => break,
                frame = read_frame(&mut recv) => match frame {
                    Ok(Some((TY_DATA, payload))) => {
                        // Decode the #138 (section_id, tab_id) tag
                        // the daemon attaches to every PTY chunk.
                        // Untagged legacy frames (decoder returns
                        // None) fall through with empty ids so
                        // older daemons keep working until both
                        // sides ship the tag.
                        let (section_id, tab_id, body) =
                            match daemon_proto::decode_pty_data(&payload) {
                                Some(decoded) => decoded,
                                None => (String::new(), String::new(), payload),
                            };
                        if incoming_tx
                            .send((section_id, tab_id, body))
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Ok(Some((TY_WORKER_REPLY, payload))) => {
                        // Two-stage decode for forwards-compat: parse
                        // as a generic JSON value first so we can peek
                        // at the `kind` discriminator. If the kind is
                        // one the current build knows, do the strict
                        // decode; otherwise log + drop so a future
                        // daemon variant doesn't blow up an older
                        // client. Strict decode goes through
                        // `WorkerReplyEnvelope` because the wire
                        // payload carries `{"request_id":N,"kind":...}`
                        // flattened together.
                        match serde_json::from_slice::<serde_json::Value>(&payload) {
                            Ok(value) => {
                                // Clone the discriminator before the
                                // strict decode moves `value` —
                                // otherwise we'd have no way to log
                                // the unknown variant name.
                                let kind = value
                                    .get("kind")
                                    .and_then(|k| k.as_str())
                                    .unwrap_or("<missing>")
                                    .to_string();
                                match serde_json::from_value::<WorkerReplyEnvelope>(value) {
                                    Ok(envelope) => {
                                        // First try to route by
                                        // request_id to a pending
                                        // `Session::call` oneshot. If
                                        // there's no awaiter (push
                                        // frame with id 0, or a
                                        // legacy `send_control` caller
                                        // that polls
                                        // `next_worker_reply`),
                                        // fall through to the FIFO.
                                        let routed = {
                                            let mut pending =
                                                pending_calls_recv.lock().expect("pending_calls poisoned");
                                            pending.remove(&envelope.request_id)
                                        };
                                        if let Some(sender) = routed {
                                            // Discard send-error: the
                                            // caller dropped the
                                            // receiver before the
                                            // reply arrived. That's
                                            // legal (caller awaited
                                            // with timeout, etc.).
                                            let _ = sender.send(envelope.reply);
                                        } else {
                                            // try_send, not send().await — this
                                            // recv task also drives the PTY
                                            // stream which *does* want
                                            // backpressure; we can't let a
                                            // stuck worker_replies consumer
                                            // stall PTY bytes.
                                            use tokio::sync::mpsc::error::TrySendError;
                                            match worker_replies_tx.try_send(envelope.reply) {
                                                Ok(()) => {}
                                                Err(TrySendError::Full(_)) => {
                                                    tracing::debug!("worker_replies channel full; dropping frame");
                                                }
                                                Err(TrySendError::Closed(_)) => {
                                                    tracing::debug!("worker_replies channel closed; dropping frame");
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        tracing::debug!(
                                            kind,
                                            error = %e,
                                            "unknown/unsupported worker_reply variant; dropping (daemon is newer than client?)"
                                        );
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    payload_bytes = payload.len(),
                                    "failed to parse worker_reply frame as JSON"
                                );
                            }
                        }
                    }
                    Ok(Some((ty, _))) => {
                        tracing::debug!(frame_type = ty, "unhandled iroh frame type");
                    }
                    Ok(None) => break,
                    Err(e) => {
                        tracing::warn!(error = %e, "iroh frame read failed");
                        break;
                    }
                },
            }
        }
        conn_for_close.close(0u8.into(), b"close");
    });

    Ok(Session {
        _endpoint: endpoint,
        send_tx: Mutex::new(Some(send_tx)),
        incoming_rx: Mutex::new(Some(incoming_rx)),
        worker_replies_rx: Mutex::new(Some(worker_replies_rx)),
        closer: Mutex::new(Some(close_tx)),
        next_request_id: AtomicU64::new(2),
        pending_calls,
    })
}

impl Session {
    /// Public entry point: dial by pairing URL. Convenience wrapper
    /// around the module-level [`connect`]. Same semantics.
    pub async fn connect(pairing_url: &str) -> anyhow::Result<Session> {
        connect(pairing_url).await
    }

    /// Send raw bytes to the daemon (will be written into the
    /// attached PTY's stdin).
    pub async fn send(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.send_frame(TY_DATA, bytes).await
    }

    /// Send tagged PTY input for `(section_id, tab_id)`. The tag
    /// lets the daemon route the bytes to the right tab without
    /// relying on its stale attach snapshot — fixing the
    /// mid-stream-attach race described in #138. Consumers should
    /// prefer this over [`Session::send`], which leaves the
    /// routing implicit.
    pub async fn send_tab_data(
        &self,
        section_id: &str,
        tab_id: &str,
        bytes: &[u8],
    ) -> anyhow::Result<()> {
        let payload = daemon_proto::encode_pty_data(section_id, tab_id, bytes);
        self.send_frame(TY_DATA, payload).await
    }

    /// Request a PTY resize on the daemon's end (legacy standalone
    /// sandbox path). Goes through the same stream as data,
    /// multiplexed by frame type. For tab-routed resizes use
    /// [`Session::tab_resize`] after [`Session::attach_tab`].
    pub async fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.send_control(Control::Resize { cols, rows }).await
    }

    /// Ask the daemon to send back its current project list as a
    /// [`WorkerReply::ProjectList`] frame.
    pub async fn list_projects(&self) -> anyhow::Result<()> {
        self.send_control(Control::ListProjects).await
    }

    /// Subscribe this session to the live PTY byte stream for
    /// `(section_id, tab_id)`. The daemon will forward the attached
    /// tab's output as [`TY_DATA`] frames. At most one attachment
    /// per session — re-issuing replaces the previous one.
    pub async fn attach_tab(&self, section_id: String, tab_id: String) -> anyhow::Result<()> {
        self.send_control(Control::AttachTab { section_id, tab_id })
            .await
    }

    /// Stop forwarding PTY bytes for the currently-attached tab.
    /// Idempotent if nothing is attached.
    pub async fn detach_tab(&self) -> anyhow::Result<()> {
        self.send_control(Control::DetachTab).await
    }

    /// Resize the currently-attached tab's PTY. Silently no-ops on
    /// the daemon when nothing is attached.
    pub async fn tab_resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        self.send_control(Control::TabResize { cols, rows }).await
    }

    /// Ask the daemon to launch the tab's PTY if it isn't already
    /// live. No-op on the daemon side if the tab is already running.
    /// After this, a subsequent [`Session::attach_tab`] will receive
    /// bytes.
    pub async fn launch_tab(&self, section_id: String, tab_id: String) -> anyhow::Result<()> {
        self.send_control(Control::LaunchTab { section_id, tab_id })
            .await
    }

    /// Drain the next inbound PTY-data frame. Returns
    /// `Some((section_id, tab_id, bytes))` tagged per-frame by the
    /// daemon (see #138); empty `section_id`/`tab_id` indicates a
    /// legacy untagged frame the caller should route via its own
    /// attach state. `None` once the frame reader task has exited
    /// (clean EOF, error, or `close`). Polling swap-in for the
    /// legacy `subscribe(StreamSink)` — UI code spawns a task to
    /// drain into whatever surface it likes.
    pub async fn next_incoming_bytes(&self) -> Option<(String, String, Vec<u8>)> {
        let mut guard = self.incoming_rx.lock().await;
        match guard.as_mut() {
            Some(rx) => rx.recv().await,
            None => None,
        }
    }

    /// Drain the next decoded worker reply. Same shape as
    /// [`Session::next_incoming_bytes`].
    pub async fn next_worker_reply(&self) -> Option<WorkerReply> {
        let mut guard = self.worker_replies_rx.lock().await;
        match guard.as_mut() {
            Some(rx) => rx.recv().await,
            None => None,
        }
    }

    /// Close the session. Fires the closer oneshot (terminating the
    /// recv loop, which in turn calls `conn.close`) and drops the
    /// outbound channel sender (which lets the writer task drain and
    /// finish). Safe to call multiple times — the second and later
    /// calls are no-ops.
    pub async fn close(&self) {
        // Drop the outbound sender first so the writer task drains
        // its queue and calls `send.finish()` cleanly before the
        // connection goes away.
        self.send_tx.lock().await.take();
        if let Some(close_tx) = self.closer.lock().await.take() {
            let _ = close_tx.send(());
        }
    }

    /// Issue a verb and await its matching reply. The daemon's reply
    /// is correlated against the assigned `request_id`, so concurrent
    /// `call`s from separate tasks no longer race the FIFO recv
    /// channel — each gets exactly the reply for its own verb.
    ///
    /// Replies the daemon emits as `WorkerReply::Err { kind, message }`
    /// are returned as-is (this is the typed daemon-side error path);
    /// transport-level failures (closed connection, encode error) come
    /// back as `Err`.
    pub async fn call(&self, control: Control) -> anyhow::Result<WorkerReply> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel::<WorkerReply>();
        {
            let mut pending = self.pending_calls.lock().expect("pending_calls poisoned");
            pending.insert(request_id, tx);
        }
        let envelope = daemon_proto::ControlEnvelope {
            request_id,
            control,
        };
        let payload = serde_json::to_vec(&envelope).context("encode control envelope")?;
        if let Err(e) = self.send_frame(TY_CONTROL, payload).await {
            // Best-effort cleanup: drop the registration so the recv
            // loop's lookup doesn't waste time on an abandoned id.
            let mut pending = self.pending_calls.lock().expect("pending_calls poisoned");
            pending.remove(&request_id);
            return Err(e);
        }
        rx.await
            .map_err(|_| anyhow::anyhow!("session closed before reply arrived"))
    }

    /// Wrap a `Control` in the daemon's required `ControlEnvelope`
    /// (carrying a freshly-allocated `request_id`) and queue it on the
    /// outbound writer task. Every per-method helper above goes
    /// through here so they all stay envelope-compliant.
    ///
    /// Legacy fire-and-forget path kept for callers that drain replies
    /// via `next_worker_reply` rather than awaiting a typed `call`.
    /// New call sites should use [`Session::call`] instead.
    async fn send_control(&self, control: Control) -> anyhow::Result<()> {
        let request_id = self.next_request_id.fetch_add(1, Ordering::Relaxed);
        let envelope = daemon_proto::ControlEnvelope {
            request_id,
            control,
        };
        let payload = serde_json::to_vec(&envelope).context("encode control envelope")?;
        self.send_frame(TY_CONTROL, payload).await
    }

    async fn send_frame(&self, ty: u8, payload: Vec<u8>) -> anyhow::Result<()> {
        let tx = self.send_tx.lock().await;
        match tx.as_ref() {
            Some(tx) => tx
                .send((ty, payload))
                .await
                .map_err(|_| anyhow::anyhow!("send channel closed")),
            None => Err(anyhow::anyhow!("session closed")),
        }
    }
}
