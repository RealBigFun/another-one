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
use iroh::endpoint::{presets, Connection, Incoming};
use iroh::{Endpoint, EndpointAddr, SecretKey};
use tokio::sync::{broadcast, mpsc};
use tokio::task::AbortHandle;
use tracing::{debug, info, warn};

use crate::frame::{self, Control, WorkerReply};
use crate::registry::{EndpointHandle, PairState, TerminalRegistry};

/// ALPN advertised by the daemon. Version-suffixed so future protocol
/// breaks can be versioned cleanly (`/1`, `/2`, …).
pub const ALPN: &[u8] = b"anotherone/pty/0";

/// Bring up an iroh endpoint backed by `registry`. Returns once the
/// endpoint is online + the pairing QR has been rendered; the accept
/// loop runs on a detached task owned by the returned handle (drop
/// or `abort()` the handle to shut it down).
pub async fn run_embedded(
    registry: Arc<dyn TerminalRegistry>,
    secret_key_path: PathBuf,
    paired_peers_path: PathBuf,
) -> anyhow::Result<EndpointHandle> {
    let secret_key = load_or_create_secret_key(&secret_key_path)?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("bind iroh endpoint")?;

    let endpoint_id = endpoint.id().to_string();
    info!("iroh EndpointId: {endpoint_id}");
    info!("iroh ALPN: {}", String::from_utf8_lossy(ALPN));

    endpoint.online().await;
    let addr = endpoint.addr();
    info!("iroh endpoint online: {addr:?}");

    let nonce = generate_pair_nonce();
    let pairing_url = build_pairing_url_with_token(&addr, &nonce);
    let qr_png_bytes =
        render_qr_png_bytes(&pairing_url).context("render pairing QR PNG")?;
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
                if let Err(e) =
                    handle_incoming(incoming, registry, &paired_path, pair_state).await
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
    forwarder: AbortHandle,
}

async fn handle_incoming(
    incoming: Incoming,
    registry: Arc<dyn TerminalRegistry>,
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
            conn.close(1u8.into(), b"anotherone/unpaired: scan the pairing QR again");
            return Ok(());
        }
    };

    let result =
        handle_connection(conn, registry.clone(), &viewer_id, authz, paired_peers_path, pair_state)
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
    registry: Arc<dyn TerminalRegistry>,
    viewer_id: &str,
    mut authz: PostAuth,
    paired_peers_path: &Path,
    pair_state: Arc<Mutex<PairState>>,
) -> anyhow::Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await.context("accept_bi")?;

    // Outbound mpsc: all producers (worker-reply replies + the PTY
    // forwarder task) push (type, payload) tuples; the writer task
    // owns `send` and serialises writes.
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<(u8, Vec<u8>)>(64);
    let writer_task = tokio::spawn(async move {
        while let Some((ty, payload)) = outbound_rx.recv().await {
            if let Err(e) = frame::write_frame(&mut send, ty, &payload).await {
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
                    conn.close(
                        1u8.into(),
                        b"anotherone/unpaired: scan the pairing QR again",
                    );
                    break;
                }
                if let Some(att) = &attached {
                    registry.tab_input(&att.section_id, &att.tab_id, &payload);
                }
                // No attachment → silently drop. Not an error:
                // clients may type during the race between AttachTab
                // going out and the first reply coming back.
            }
            Ok(Some((frame::TY_CONTROL, payload))) => {
                match serde_json::from_slice::<Control>(&payload) {
                    Ok(ctrl) => {
                        if matches!(authz, PostAuth::AwaitHello) {
                            match consume_hello(ctrl, viewer_id, &pair_state, paired_peers_path)
                            {
                                Ok(()) => {
                                    authz = PostAuth::AlreadyPaired;
                                    info!(viewer_id, "TOFU pair complete");
                                    continue;
                                }
                                Err(e) => {
                                    warn!(viewer_id, error = %e, "rejecting unpaired peer");
                                    conn.close(
                                        1u8.into(),
                                        b"anotherone/unpaired: scan the pairing QR again",
                                    );
                                    break;
                                }
                            }
                        }
                        handle_control(
                            ctrl,
                            &registry,
                            &outbound_tx,
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
        att.forwarder.abort();
    }
    drop(outbound_tx);
    writer_task.abort();
    info!("iroh session ended");
    Ok(())
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
    let Control::Hello { pair_token } = ctrl else {
        anyhow::bail!("first frame from unpaired peer must be Control::Hello");
    };
    let presented = pair_token.ok_or_else(|| {
        anyhow::anyhow!("Hello from unpaired peer missing pair_token")
    })?;

    {
        let mut state = pair_state.lock().unwrap();
        let expected = state
            .nonce
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("no outstanding pair nonce (consumed or not rolled)"))?;
        // Constant-time compare not strictly required (an attacker
        // can't iterate nonces across a single connect), but cheap
        // enough to do anyway.
        if !constant_time_eq(expected.as_bytes(), presented.as_bytes()) {
            anyhow::bail!("pair_token mismatch");
        }
        state.nonce = None; // consume on success
    }

    persist_pairing(viewer_id, paired_peers_path)
}

async fn handle_control(
    ctrl: Control,
    registry: &Arc<dyn TerminalRegistry>,
    outbound_tx: &mpsc::Sender<(u8, Vec<u8>)>,
    attached: &mut Option<Attached>,
    viewer_id: &str,
) -> anyhow::Result<()> {
    match ctrl {
        Control::Resize { cols, rows } | Control::TabResize { cols, rows } => {
            if let Some(att) = attached.as_ref() {
                registry.tab_resize(
                    viewer_id,
                    &att.section_id,
                    &att.tab_id,
                    cols,
                    rows,
                );
            }
        }
        Control::ListProjects => {
            let projects = registry.list_projects();
            let wire = WorkerReply::ProjectList { projects };
            send_worker_reply(outbound_tx, &wire).await?;
        }
        Control::AttachTab {
            section_id,
            tab_id,
        } => {
            // Drop any prior attachment on this connection.
            if let Some(prev) = attached.take() {
                prev.forwarder.abort();
            }

            let Some(mut rx) = registry.attach_tab(&section_id, &tab_id) else {
                warn!(section_id, tab_id, "attach_tab: no such live runtime");
                return Ok(());
            };

            let out = outbound_tx.clone();
            let forwarder = tokio::spawn(async move {
                loop {
                    match rx.recv().await {
                        Ok(bytes) => {
                            if out.send((frame::TY_DATA, bytes)).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            // Slow mobile consumer. Skip over the
                            // missing chunks — the PTY is lossless
                            // from the desktop's point of view, but
                            // the mobile terminal will just see a
                            // resume from the new tail. Rare in
                            // practice at our frame sizes.
                            warn!(lagged = n, "attach forwarder lagged");
                        }
                    }
                }
            });

            *attached = Some(Attached {
                section_id,
                tab_id,
                forwarder: forwarder.abort_handle(),
            });
        }
        Control::DetachTab => {
            if let Some(prev) = attached.take() {
                prev.forwarder.abort();
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
        Control::LaunchTab {
            section_id,
            tab_id,
        } => {
            registry.launch_tab(&section_id, &tab_id);
        }
        Control::Hello { .. } => {
            // Hello is only meaningful as the *first* control frame
            // from an unpaired peer — see `consume_hello`. A paired
            // peer that sends it mid-session is harmless but pointless;
            // drop it rather than error.
            debug!("stray Control::Hello from already-paired peer; ignored");
        }
    }
    Ok(())
}

async fn send_worker_reply(
    outbound_tx: &mpsc::Sender<(u8, Vec<u8>)>,
    reply: &WorkerReply,
) -> anyhow::Result<()> {
    let payload = serde_json::to_vec(reply).context("serialize worker reply")?;
    outbound_tx
        .send((frame::TY_WORKER_REPLY, payload))
        .await
        .map_err(|_| anyhow::anyhow!("outbound queue closed before worker reply was sent"))
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
/// perms if needed. Called after a successful TOFU Hello.
fn persist_pairing(remote_id: &str, path: &Path) -> anyhow::Result<()> {
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
pub(crate) fn build_pairing_url_with_token(
    addr: &EndpointAddr,
    pair_token: &str,
) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;

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
        assert_eq!(&png[..8], &[0x89, b'P', b'N', b'G', b'\r', b'\n', 0x1a, b'\n']);
    }
}
