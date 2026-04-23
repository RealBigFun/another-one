//! Iroh QUIC transport for the sandbox daemon.
//!
//! Wire format: one bidirectional QUIC stream per connection, each message
//! framed as `[1 byte type][4 bytes BE length][N bytes payload]` (see
//! [`crate::frame`]). `0x00` frames carry PTY bytes in either direction,
//! `0x01` frames carry JSON control messages (currently `resize`).

use std::path::PathBuf;

use anyhow::Context;
use iroh::endpoint::{presets, Connection, Incoming};
use iroh::{Endpoint, SecretKey};
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::frame::{self, Control, PullRequestInfo, PullRequestState, WorkerReply};
use crate::pty::PtySession;

/// ALPN advertised by the sandbox. Version-suffixed so future protocol breaks
/// can be versioned cleanly (`/1`, `/2`, …).
pub const ALPN: &[u8] = b"anotherone/pty/0";

/// Returns the XDG-ish data directory for the sandbox daemon, creating it
/// if missing. Resolution order matches the XDG Base Directory spec enough
/// for our purposes: `$XDG_DATA_HOME/another-one-sandbox` if set, otherwise
/// `$HOME/.local/share/another-one-sandbox`. No external `dirs` dep — keeps
/// the daemon binary lean.
fn data_dir() -> anyhow::Result<PathBuf> {
    let base = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg)
    } else {
        let home = std::env::var("HOME").context("HOME is unset — can't locate data dir")?;
        PathBuf::from(home).join(".local").join("share")
    };
    let dir = base.join("another-one-sandbox");
    std::fs::create_dir_all(&dir).with_context(|| format!("create data dir {}", dir.display()))?;
    Ok(dir)
}

/// Loads the daemon's persistent Ed25519 secret key from
/// `<data_dir>/secret_key` (32 hex-encoded bytes). Generates a fresh one
/// and writes it to disk the first time. Giving the daemon a stable
/// identity across restarts means paired clients don't have to re-discover
/// its `EndpointId` every time the process starts.
fn load_or_create_secret_key() -> anyhow::Result<SecretKey> {
    let path = data_dir()?.join("secret_key");
    if let Ok(content) = std::fs::read_to_string(&path) {
        let trimmed = content.trim();
        let bytes = hex_decode_32(trimmed)
            .with_context(|| format!("parse secret key at {}", path.display()))?;
        Ok(SecretKey::from_bytes(&bytes))
    } else {
        let sk = SecretKey::generate();
        let hex = hex_encode_32(&sk.to_bytes());
        std::fs::write(&path, format!("{hex}\n"))
            .with_context(|| format!("write secret key to {}", path.display()))?;
        // Tighten perms on unix — 0600 so other users on the box can't read.
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600));
        }
        info!("generated new persistent secret key at {}", path.display());
        Ok(sk)
    }
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

/// Runs the Iroh endpoint loop until its `accept()` stream ends.
pub async fn serve() -> anyhow::Result<()> {
    let secret_key = load_or_create_secret_key()?;
    let endpoint = Endpoint::builder(presets::N0)
        .secret_key(secret_key)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("bind iroh endpoint")?;

    let endpoint_id = endpoint.id();
    info!("iroh EndpointId: {}", endpoint_id);
    info!("iroh ALPN: {}", String::from_utf8_lossy(ALPN));

    // Sandbox convenience: stash the EndpointId in /tmp so `iroh-client`
    // can pick it up without copy/paste. Production daemons should persist
    // their secret key and publish identity through a pairing flow instead.
    let id_path = std::env::temp_dir().join("daemon-sandbox.nodeid");
    if let Err(e) = std::fs::write(&id_path, endpoint_id.to_string()) {
        warn!(error = %e, "failed to write NodeId hint file");
    } else {
        info!("EndpointId written to {}", id_path.display());
    }

    endpoint.online().await;
    info!("iroh endpoint online: {:?}", endpoint.addr());

    // After going online, also stash the full EndpointAddr (id + direct
    // socket addrs + relay URLs) as a newline-delimited text file so
    // iroh-client and the mobile sandbox can dial without depending on DNS
    // address lookup. Relay entries let off-LAN clients (CGNAT'd mobile on
    // cellular, different networks) reach the daemon through the dev relay
    // mesh when direct hole-punching fails.
    // Format:
    //   id=<hex>
    //   addr=<ip:port>
    //   …
    //   relay=<url>
    //   …
    let addr = endpoint.addr();
    let mut ticket = format!("id={}\n", addr.id);
    for ip in addr.ip_addrs() {
        ticket.push_str(&format!("addr={ip}\n"));
    }
    for relay in addr.relay_urls() {
        ticket.push_str(&format!("relay={relay}\n"));
    }
    let ticket_path = std::env::temp_dir().join("daemon-sandbox.ticket");
    if let Err(e) = std::fs::write(&ticket_path, ticket) {
        warn!(error = %e, "failed to write ticket file");
    } else {
        info!("Ticket written to {}", ticket_path.display());
    }

    // Print a single-line pairing URL and ASCII QR on stdout — the phone
    // can scan the QR with its default camera app, copy the URL, and paste
    // it into the endpoint field. Also write a PNG next to the ticket so
    // you can open it in any image viewer if your terminal's font/contrast
    // isn't scannable. Direct addrs are included so on-LAN devices can use
    // the fast path; relay is included so cellular/off-LAN falls back.
    let pairing_url = build_pairing_url(&addr);
    println!();
    println!("Pairing URL:\n  {pairing_url}");
    match render_qr_ascii(&pairing_url) {
        Ok(qr) => {
            println!();
            print!("{qr}");
        }
        Err(e) => warn!(error = %e, "failed to render ASCII pairing QR"),
    }
    let png_path = std::env::temp_dir().join("daemon-sandbox.pairing.png");
    match write_qr_png(&pairing_url, &png_path) {
        Ok(()) => {
            println!();
            println!("Pairing QR also written to {}", png_path.display());
        }
        Err(e) => warn!(error = %e, "failed to write pairing PNG"),
    }

    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(async move {
            if let Err(e) = handle_incoming(incoming).await {
                warn!(error = %e, "iroh connection error");
            }
        });
    }
    Ok(())
}

async fn handle_incoming(incoming: Incoming) -> anyhow::Result<()> {
    let conn = incoming
        .accept()
        .context("accept")?
        .await
        .context("handshake")?;
    let remote = conn.remote_id();

    // Authorize via a trust-on-first-use allowlist. The first client to
    // connect gets saved as the owning device; subsequent unknown
    // EndpointIds are rejected immediately. Delete the allowlist file to
    // re-pair. This is the sandbox's stand-in for a real pairing token
    // flow — quick, zero-UX, good enough to keep random iroh peers who
    // learn the NodeId out.
    match authorize_remote(&remote.to_string()) {
        Ok(Authorization::Paired) => {
            info!(%remote, "iroh client connected (paired)");
        }
        Ok(Authorization::FirstPair) => {
            info!(%remote, "iroh client connected (first-pair, added to allowlist)");
        }
        Err(e) => {
            warn!(%remote, error = %e, "rejecting unknown peer");
            conn.close(1u8.into(), b"not paired");
            return Ok(());
        }
    }
    handle_connection(conn).await
}

enum Authorization {
    /// The remote's EndpointId was already in the allowlist.
    Paired,
    /// Allowlist was empty; we just added this remote.
    FirstPair,
}

/// Check the allowlist at `<data_dir>/paired_peers` against `remote_id`.
/// Adds the remote on the first-ever call (TOFU). Returns `Err` on any
/// other mismatch — the caller closes the connection without starting a
/// PTY.
///
/// Read failures other than `NotFound` bail out rather than TOFU-pair
/// the connecting peer: a transient `EACCES`/`EIO` or a manually-edited
/// permissions botch used to look exactly like "file empty" and would
/// silently accept the next incoming EndpointId, overwriting a real
/// pairing.
///
/// The first-pair write uses `create_new(true)` so two clients racing to
/// be the first-ever connection can't both win: exactly one CREATE
/// syscall succeeds; the other collides with `AlreadyExists` and falls
/// through to a normal allowlist check. Non-atomic read-then-write
/// would otherwise happily let both through.
fn authorize_remote(remote_id: &str) -> anyhow::Result<Authorization> {
    use std::io::{ErrorKind, Write};

    let path = data_dir()?.join("paired_peers");
    let existing = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) if e.kind() == ErrorKind::NotFound => String::new(),
        Err(e) => {
            return Err(anyhow::Error::from(e))
                .with_context(|| format!("read allowlist {}", path.display()));
        }
    };
    let peers: Vec<&str> = existing
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !line.starts_with('#'))
        .collect();
    if peers.contains(&remote_id) {
        return Ok(Authorization::Paired);
    }
    if !peers.is_empty() {
        anyhow::bail!(
            "remote {remote_id} is not in {} (delete the file to re-pair)",
            path.display()
        )
    }

    // First-pair path. Atomic create so a concurrent first connect can't
    // both pair; losers get `AlreadyExists` and must re-enter through
    // the normal allowlist check above on their next dial.
    let line = format!("{remote_id}\n");
    let mut opts = std::fs::OpenOptions::new();
    opts.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        opts.mode(0o600);
    }
    match opts.open(&path) {
        Ok(mut f) => {
            f.write_all(line.as_bytes())
                .with_context(|| format!("write allowlist {}", path.display()))?;
            Ok(Authorization::FirstPair)
        }
        Err(e) if e.kind() == ErrorKind::AlreadyExists => {
            anyhow::bail!(
                "lost first-pair race for {remote_id}; \
                 re-dial after inspecting {}",
                path.display()
            )
        }
        Err(e) => Err(anyhow::Error::from(e))
            .with_context(|| format!("create allowlist {}", path.display())),
    }
}

async fn handle_connection(conn: Connection) -> anyhow::Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await.context("accept_bi")?;

    let session = PtySession::spawn(80, 24).context("pty session")?;
    let mut output_rx = session.output_rx;
    let mut writer = session.master_writer;

    // The recv loop needs to send resize commands to the master, which is
    // held by `session`. Use a channel so the loop doesn't need a direct
    // handle across the split.
    let (resize_tx, mut resize_rx) = mpsc::channel::<(u16, u16)>(8);
    let master = session.master;
    let resize_task = tokio::spawn(async move {
        while let Some((cols, rows)) = resize_rx.recv().await {
            if let Err(e) = master.resize(iroh_pty_size(cols, rows)) {
                warn!(error = %e, "pty resize failed");
            } else {
                debug!(cols, rows, "iroh resized");
            }
        }
        // Drop the master when the channel closes so the PTY frees.
        drop(master);
    });

    // Multiplexed outbound frames. Anything the daemon wants to send
    // the client (PTY bytes, worker replies, future push
    // notifications) goes through this channel so a single task owns
    // `send` and serialises writes — concurrent writers on a QUIC
    // send stream would either race or require an async mutex.
    let (outbound_tx, mut outbound_rx) = mpsc::channel::<(u8, Vec<u8>)>(16);

    // PTY output → outbound queue as TY_DATA frames.
    let pty_outbound_tx = outbound_tx.clone();
    let pty_relay_task = tokio::spawn(async move {
        while let Some(bytes) = output_rx.recv().await {
            if pty_outbound_tx
                .send((frame::TY_DATA, bytes))
                .await
                .is_err()
            {
                // writer task exited; stop pumping.
                break;
            }
        }
    });

    // Single writer: drains the outbound queue into the QUIC stream.
    // Keeps running as long as any producer (pty_relay_task or a
    // worker forwarder spawned from the Control::WatchProject dispatch
    // below) still holds a clone of `outbound_tx`. We drop our own
    // clone after the recv loop exits so the writer terminates
    // cleanly during teardown.
    let send_task = tokio::spawn(async move {
        while let Some((ty, payload)) = outbound_rx.recv().await {
            if let Err(e) = frame::write_frame(&mut send, ty, &payload).await {
                debug!(error = %e, "iroh frame write failed");
                break;
            }
        }
        let _ = send.finish();
    });

    // Iroh recv stream → dispatch by frame type.
    loop {
        match frame::read_frame(&mut recv).await {
            Ok(Some((frame::TY_DATA, payload))) => {
                if let Err(e) = std::io::Write::write_all(&mut writer, &payload) {
                    warn!(error = %e, "iroh→pty write failed");
                    break;
                }
                let _ = std::io::Write::flush(&mut writer);
            }
            Ok(Some((frame::TY_CONTROL, payload))) => {
                match serde_json::from_slice::<Control>(&payload) {
                    Ok(Control::Resize { cols, rows }) => {
                        if resize_tx.send((cols, rows)).await.is_err() {
                            debug!("resize channel closed");
                            break;
                        }
                    }
                    Ok(Control::WatchProject { project_path }) => {
                        let path = PathBuf::from(&project_path);
                        if !path.is_dir() {
                            warn!(
                                project_path = %project_path,
                                "WatchProject ignored: path does not exist or is not a directory"
                            );
                        } else {
                            let worker_tx = outbound_tx.clone();
                            tokio::spawn(async move {
                                if let Err(e) = forward_project_state(worker_tx, path).await {
                                    warn!(error = %e, "project state forward failed");
                                }
                            });
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "bad iroh control frame");
                    }
                }
            }
            Ok(Some((ty, _))) => {
                warn!(frame_type = ty, "unknown iroh frame type");
            }
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

    // Teardown. Dropping resize_tx closes the resize channel →
    // resize_task drops the master → PTY frees. Dropping outbound_tx
    // lets send_task finish once all worker forwarders die off —
    // spawn_refresh workers are one-shot and self-terminate, so this
    // naturally converges.
    drop(resize_tx);
    drop(outbound_tx);
    pty_relay_task.abort();
    send_task.abort();
    resize_task.abort();
    let mut child = session.child;
    let _ = child.kill();
    let _ = child.wait();
    info!("iroh session ended");
    Ok(())
}

/// Run the full watched-project pipeline once: subscribe to
/// `spawn_refresh`, forward the `GitRefreshReply` as a wire frame,
/// and — if the refresh returned a current branch — chain a
/// `spawn_pull_request_lookup` for that branch and forward the
/// resulting status as a second frame.
///
/// Errors here are never fatal to the session — they drop this task
/// silently; PTY bridging keeps working. Each stage fails
/// independently: a failed PR lookup doesn't retroactively invalidate
/// the refresh that already shipped.
async fn forward_project_state(
    outbound_tx: mpsc::Sender<(u8, Vec<u8>)>,
    project_path: PathBuf,
) -> anyhow::Result<()> {
    let project_id = project_path.display().to_string();

    // Stage 1: git refresh.
    let mut refresh_rx = another_one_core::git_service::spawn_refresh(
        project_id.clone(),
        project_path.clone(),
        /* include_metadata */ true,
        /* commit_limit      */ None,
        /* compare_target    */ None,
    );
    let refresh = refresh_rx
        .recv()
        .await
        .context("git_refresh sender dropped before sending a reply")?;

    let current_branch = refresh.state.current_branch.clone();
    let refresh_wire = WorkerReply::GitRefresh {
        project_id: refresh.project_id.clone(),
        current_branch: refresh.state.current_branch,
        changed_file_count: refresh.state.changed_files.len(),
        ahead: refresh.state.ahead_count,
        behind: refresh.state.behind_count,
    };
    send_worker_reply(&outbound_tx, &refresh_wire)
        .await
        .context("forward git_refresh reply")?;

    // Stage 2: PR lookup (only if we know the branch name).
    let Some(branch_name) = current_branch else {
        return Ok(());
    };

    // `spawn_pull_request_lookup` is queue-shaped — it takes a
    // broadcast::Sender (so desktop can share one receiver across
    // many lookups). For the daemon's one-shot use we allocate a
    // throwaway channel per call, capacity 1.
    let (pr_tx, mut pr_rx) = tokio::sync::broadcast::channel(1);
    another_one_core::git_service::spawn_pull_request_lookup(
        pr_tx,
        /* lookup_key */ format!("{project_id}:{branch_name}"),
        project_path,
        branch_name.clone(),
    );
    let pr_reply = pr_rx
        .recv()
        .await
        .context("pull_request_lookup sender dropped before sending a reply")?;

    let pr_wire = WorkerReply::PullRequestStatus {
        project_id,
        branch_name,
        pr: pr_reply.pull_request.map(|status| PullRequestInfo {
            number: status.number,
            url: status.url,
            state: map_pr_state(status.state),
        }),
    };
    send_worker_reply(&outbound_tx, &pr_wire)
        .await
        .context("forward pull_request reply")?;

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

fn map_pr_state(state: another_one_core::git_actions::PullRequestState) -> PullRequestState {
    use another_one_core::git_actions::PullRequestState as Core;
    match state {
        Core::Open => PullRequestState::Open,
        Core::Closed => PullRequestState::Closed,
        Core::Merged => PullRequestState::Merged,
    }
}

fn iroh_pty_size(cols: u16, rows: u16) -> portable_pty::PtySize {
    portable_pty::PtySize {
        cols,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    }
}

/// Builds the `iroh://…?direct=…&relay=…` pairing URL the mobile app
/// understands. Direct addrs are comma-separated; relay URLs are
/// percent-encoded so the `://` inside each relay URL doesn't confuse
/// Dart's `Uri.parse`.
fn build_pairing_url(addr: &iroh::EndpointAddr) -> String {
    let directs = addr
        .ip_addrs()
        .map(|ip| ip.to_string())
        .collect::<Vec<_>>()
        .join(",");
    let relays = addr
        .relay_urls()
        .map(|u| urlencoding::encode(u.as_str()).into_owned())
        .collect::<Vec<_>>()
        .join(",");
    let mut url = format!("iroh://{}", addr.id);
    let mut params: Vec<String> = Vec::new();
    if !directs.is_empty() {
        params.push(format!("direct={directs}"));
    }
    if !relays.is_empty() {
        params.push(format!("relay={relays}"));
    }
    if !params.is_empty() {
        url.push('?');
        url.push_str(&params.join("&"));
    }
    url
}

/// Renders `input` as an ASCII QR code (two chars per pixel, quiet zone
/// included) suitable for pasting into a terminal window. Returns a
/// string that ends with a newline.
fn render_qr_ascii(input: &str) -> anyhow::Result<String> {
    use qrcode::{render::unicode::Dense1x2, QrCode};
    let code = QrCode::new(input.as_bytes()).context("encode QR")?;
    Ok(code
        .render::<Dense1x2>()
        .dark_color(Dense1x2::Light)
        .light_color(Dense1x2::Dark)
        .build())
}

/// Renders `input` as a PNG and writes it to `path`. We scale modules up
/// to 12 px with an 8-module quiet zone so phone cameras can focus on it
/// at typical laptop-viewing distance without hunting.
fn write_qr_png(input: &str, path: &std::path::Path) -> anyhow::Result<()> {
    use qrcode::QrCode;
    let code = QrCode::new(input.as_bytes()).context("encode QR")?;
    let buf = code
        .render::<image::Luma<u8>>()
        .min_dimensions(480, 480)
        .quiet_zone(true)
        .build();
    buf.save_with_format(path, image::ImageFormat::Png)
        .with_context(|| format!("write {}", path.display()))?;
    Ok(())
}
