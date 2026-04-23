//! Iroh client exposed to Dart via flutter_rust_bridge.
//!
//! One `IrohSession` represents a live QUIC connection to a daemon that
//! speaks the `anotherone/pty/0` ALPN. Dart uses:
//!
//!   1. `iroh_connect(endpoint_id)` to dial.
//!   2. `session.send(bytes)` to deliver PTY input.
//!   3. `session.subscribe(sink)` to start receiving PTY output as a stream
//!      of `Vec<u8>` chunks.
//!   4. `session.close()` when finished.
//!
//! All iroh network work runs on a dedicated multi-thread tokio runtime
//! because FRB's default async executor is not a tokio runtime — iroh's
//! UDP sockets and internal actor tasks require tokio specifically, and
//! without this indirection `Endpoint::bind()` hangs forever on Android.

use std::sync::OnceLock;

use anyhow::Context;
use flutter_rust_bridge::frb;
use tokio::runtime::Runtime;
use tokio::sync::{mpsc, Mutex};

use crate::frb_generated::StreamSink;
use iroh::dns::DnsResolver;
use iroh::endpoint::presets;
use iroh::endpoint::{RecvStream, SendStream};
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode, RelayUrl};

/// Must match the daemon's ALPN byte string.
const ALPN: &[u8] = b"anotherone/pty/0";

// Frame wire format, matching daemon-sandbox/src/frame.rs:
//   [1 byte type][4 bytes BE length][N bytes payload]
const TY_DATA: u8 = 0x00;
const TY_CONTROL: u8 = 0x01;
const TY_WORKER_REPLY: u8 = 0x02;
/// See `daemon-sandbox/src/frame.rs::MAX_FRAME_BYTES` for the rationale;
/// keep this value in lockstep with the daemon's cap.
const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Messages that can be sent via a type=1 control frame. Extend in lock-step
/// with `daemon-sandbox/src/frame.rs::Control`.
///
/// Serialize-only: the Dart side doesn't need to decode control
/// frames (they're strictly client → daemon today).
#[derive(Debug, Clone, serde::Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Ask the daemon to watch `project_path` and start pushing
    /// [`WorkerReply::GitRefresh`] frames for it. Mirror of
    /// `daemon-sandbox/src/frame.rs::Control::WatchProject`.
    WatchProject {
        project_path: String,
    },
}

/// Daemon → client worker replies (type=2 frame payload, JSON). Mirror
/// of `daemon-sandbox/src/frame.rs::WorkerReply`; keep variants in
/// lockstep with the daemon's schema.
///
/// Each variant is a curated projection of one core worker's reply,
/// not a mechanical derive on the `core::*_service` reply structs.
/// That lets the daemon evolve its internal types freely and makes
/// the public wire schema a deliberate artifact.
///
/// FRB-exposed: passed to Dart as a tagged union via the
/// `subscribe_worker_replies` stream on [`IrohSession`].
#[derive(Debug, Clone, serde::Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerReply {
    /// Projection of `core::git_service::GitRefreshReply`.
    GitRefresh {
        project_id: String,
        current_branch: Option<String>,
        changed_file_count: usize,
        ahead: usize,
        behind: usize,
    },
    /// Projection of `core::git_service::ProjectPullRequestReply`.
    /// `pr = None` → checked, no PR found (distinct from "not yet
    /// checked"). Mirror of `daemon-sandbox/src/frame.rs`.
    PullRequestStatus {
        project_id: String,
        branch_name: String,
        pr: Option<PullRequestInfo>,
    },
}

/// Mirror of `daemon-sandbox/src/frame.rs::PullRequestInfo`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PullRequestInfo {
    pub number: u64,
    pub url: String,
    pub state: PullRequestState,
}

/// Mirror of `daemon-sandbox/src/frame.rs::PullRequestState`.
/// Wire form is lowercase: `"open"`, `"closed"`, `"merged"`.
#[derive(Debug, Clone, Copy, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// Writes one frame to the Iroh send stream.
async fn write_frame(send: &mut SendStream, ty: u8, payload: &[u8]) -> anyhow::Result<()> {
    let mut header = [0u8; 5];
    header[0] = ty;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    send.write_all(&header).await?;
    send.write_all(payload).await?;
    Ok(())
}

/// Reads one frame from the Iroh recv stream; returns `None` on clean EOF.
async fn read_frame(recv: &mut RecvStream) -> anyhow::Result<Option<(u8, Vec<u8>)>> {
    let mut header = [0u8; 5];
    let mut read = 0;
    while read < 5 {
        match recv.read(&mut header[read..]).await? {
            Some(0) | None => {
                return if read == 0 {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("stream ended mid-header"))
                };
            }
            Some(n) => read += n,
        }
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if len > MAX_FRAME_BYTES {
        anyhow::bail!("frame too large: {len} bytes");
    }
    let mut payload = vec![0u8; len];
    read = 0;
    while read < len {
        match recv.read(&mut payload[read..]).await? {
            Some(0) | None => anyhow::bail!("stream ended mid-payload"),
            Some(n) => read += n,
        }
    }
    Ok(Some((ty, payload)))
}

/// Dedicated tokio runtime for all iroh work. FRB's default async executor
/// is not a tokio runtime, so iroh's network actors never get polled if we
/// run them on the calling task. Everything below shuffles work onto here.
fn tokio_rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .worker_threads(2)
            .thread_name("mobile_core-tokio")
            .build()
            .expect("build tokio runtime")
    })
}

#[frb(init)]
pub fn init_app() {
    flutter_rust_bridge::setup_default_user_utils();
    setup_tracing();
    // Force the runtime to initialize eagerly so first-call latency doesn't
    // include runtime construction.
    let _ = tokio_rt();
}

/// Install a tracing subscriber that routes events to Android's logcat on
/// Android, and to stderr elsewhere. Default filter is modest; override with
/// `RUST_LOG` when debugging (e.g. `RUST_LOG=iroh=debug`).
fn setup_tracing() {
    use tracing_subscriber::{prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("warn,mobile_core=info,iroh=warn"));

    #[cfg(target_os = "android")]
    let layer = tracing_android::layer("mobile_core").expect("tracing-android layer");

    #[cfg(not(target_os = "android"))]
    let layer = tracing_subscriber::fmt::layer();

    let _ = tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .try_init();
}

/// Opaque handle to a live Iroh QUIC session. Dart holds this object and
/// calls methods on it; the actual Iroh state lives in Rust.
#[frb(opaque)]
pub struct IrohSession {
    /// The local endpoint we bound for this session. Closed on Drop.
    _endpoint: Endpoint,
    /// Sends framed messages (ty, payload) from Rust to the send task,
    /// which writes them into the QUIC send stream. `None` means closed.
    send_tx: Mutex<Option<mpsc::Sender<(u8, Vec<u8>)>>>,
    /// Holds the bytes-from-daemon stream until `subscribe()` wires it to a
    /// Dart `StreamSink`. Taken once and moved into the forwarding task.
    incoming_rx: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    /// Holds decoded worker replies (from `TY_WORKER_REPLY` frames) until
    /// `subscribe_worker_replies()` wires it to a Dart sink. Same
    /// one-shot-take semantics as `incoming_rx`.
    worker_replies_rx: Mutex<Option<mpsc::Receiver<WorkerReply>>>,
    /// Closes the underlying connection when invoked.
    closer: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// Dial a daemon's Iroh endpoint by its public `EndpointId`.
///
/// At least one of `direct_addrs` or `relay_urls` must be non-empty — the
/// sandbox has no address-lookup service, so we can't discover how to reach
/// the daemon on our own. The daemon's ticket file prints both; pass them
/// through. When both are given iroh prefers the direct path and falls
/// back to the relay if hole-punching fails (the typical mobile-cellular
/// path).
pub async fn iroh_connect(
    endpoint_id: String,
    direct_addrs: Vec<String>,
    relay_urls: Vec<String>,
) -> anyhow::Result<IrohSession> {
    tokio_rt()
        .spawn(async move { iroh_connect_inner(endpoint_id, direct_addrs, relay_urls).await })
        .await
        .map_err(|e| anyhow::anyhow!("connect task panicked: {e}"))?
}

async fn iroh_connect_inner(
    endpoint_id: String,
    direct_addrs: Vec<String>,
    relay_urls: Vec<String>,
) -> anyhow::Result<IrohSession> {
    tracing::info!(
        "iroh_connect: id={} direct={:?} relays={:?}",
        endpoint_id,
        direct_addrs,
        relay_urls,
    );

    let id: EndpointId = endpoint_id.trim().parse().context("invalid EndpointId")?;

    // Parse direct addresses eagerly so bad input surfaces before bind.
    let parsed_addrs: Vec<std::net::SocketAddr> = direct_addrs
        .iter()
        .map(|s| {
            s.parse::<std::net::SocketAddr>()
                .map_err(|e| anyhow::anyhow!("bad direct addr {s:?}: {e}"))
        })
        .collect::<anyhow::Result<_>>()?;
    let parsed_relays: Vec<RelayUrl> = relay_urls
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

    // Relay mode: if the caller gave us a relay URL, honour it (N0's dev
    // mesh lives behind `RelayMode::Default`). Otherwise stay disabled for
    // the LAN-only direct path.
    let relay_mode = if parsed_relays.is_empty() {
        RelayMode::Disabled
    } else {
        RelayMode::Default
    };
    tracing::info!(
        "iroh_connect: binding (Minimal preset, relay_mode={:?}, explicit DNS)",
        relay_mode,
    );
    // Android gotcha: `DnsResolver::default()` calls `with_system_defaults()`
    // which tries to read `/etc/resolv.conf`. iroh's own doc notes this "does
    // not work at least on some Androids" and says it falls back to Google
    // DNS — but in practice on the emulator the read hangs long enough to
    // stall bind(). We explicitly hand iroh a resolver so it skips system
    // detection entirely.
    //
    // Default is Cloudflare (`1.1.1.1:53`) rather than Google (`8.8.8.8:53`)
    // so every user's daemon lookups don't default to a Google-operated
    // resolver. Override with the `ANOTHERONE_DNS` env var if the user
    // wants a different provider — any `<ip>:<port>` string parseable as a
    // `SocketAddr` works. Fall back to the default silently on parse error
    // so a fat-fingered env var doesn't brick the mobile app.
    let dns_addr: std::net::SocketAddr = std::env::var("ANOTHERONE_DNS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or_else(|| "1.1.1.1:53".parse().expect("static ipv4 socket addr"));
    tracing::info!(%dns_addr, "iroh_connect: using configured DNS resolver");
    let dns = DnsResolver::with_nameserver(dns_addr);
    let endpoint = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Endpoint::builder(presets::Minimal)
            .relay_mode(relay_mode)
            .alpns(vec![])
            .dns_resolver(dns)
            .bind(),
    )
    .await
    .map_err(|_| anyhow::anyhow!("bind timed out after 15s (Minimal+DNS)"))?
    .context("bind client endpoint")?;
    tracing::info!("iroh_connect: endpoint bound, dialing {}", id);

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
    tracing::info!("iroh_connect: connected");

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;
    tracing::info!("iroh_connect: opened bidi stream");

    // Outbound pipe: Dart → channel → framed writes to Iroh send stream.
    // Channel items are already-framed (ty, payload) pairs so the writer
    // task doesn't need to know the protocol.
    let (send_tx, mut send_rx) = mpsc::channel::<(u8, Vec<u8>)>(64);
    tokio_rt().spawn(async move {
        while let Some((ty, payload)) = send_rx.recv().await {
            if let Err(e) = write_frame(&mut send, ty, &payload).await {
                tracing::debug!(error = %e, "iroh frame write failed");
                break;
            }
        }
        let _ = send.finish();
    });

    // Inbound pipe: framed reads from Iroh → per-frame-type channel → Dart
    // (once subscribed). Type=0 frames carry PTY output; type=2 frames carry
    // JSON-encoded `WorkerReply`s. Type=1 (server→client control) is
    // reserved for future use. Unknown types are logged and dropped so older
    // clients stay forwards-compatible as the daemon adds variants.
    let (incoming_tx, incoming_rx) = mpsc::channel::<Vec<u8>>(128);
    let (worker_replies_tx, worker_replies_rx) = mpsc::channel::<WorkerReply>(64);
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();
    let conn_for_close = conn.clone();
    tokio_rt().spawn(async move {
        loop {
            tokio::select! {
                _ = &mut close_rx => break,
                frame = read_frame(&mut recv) => match frame {
                    Ok(Some((TY_DATA, payload))) => {
                        if incoming_tx.send(payload).await.is_err() {
                            break;
                        }
                    }
                    Ok(Some((TY_WORKER_REPLY, payload))) => {
                        match serde_json::from_slice::<WorkerReply>(&payload) {
                            Ok(reply) => {
                                // `try_send` instead of `send().await` on
                                // purpose: this recv task also feeds the
                                // PTY stream (`incoming_tx`, above), which
                                // *does* want backpressure. If Dart never
                                // calls `subscribe_worker_replies`, the
                                // receiver sits idle inside the session
                                // `Option<…>` — the channel is open but
                                // never drained, so `send().await` would
                                // block forever on the 65th reply and
                                // stall PTY output with it. Drop the reply
                                // on a full or closed channel instead.
                                use tokio::sync::mpsc::error::TrySendError;
                                match worker_replies_tx.try_send(reply) {
                                    Ok(()) => {}
                                    Err(TrySendError::Full(_)) => {
                                        tracing::debug!("worker_replies channel full; dropping frame (no subscriber or slow consumer)");
                                    }
                                    Err(TrySendError::Closed(_)) => {
                                        tracing::debug!("worker_replies channel closed; dropping frame");
                                    }
                                }
                            }
                            Err(e) => {
                                tracing::warn!(
                                    error = %e,
                                    payload_bytes = payload.len(),
                                    "failed to decode worker_reply frame"
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

    Ok(IrohSession {
        _endpoint: endpoint,
        send_tx: Mutex::new(Some(send_tx)),
        incoming_rx: Mutex::new(Some(incoming_rx)),
        worker_replies_rx: Mutex::new(Some(worker_replies_rx)),
        closer: Mutex::new(Some(close_tx)),
    })
}

impl IrohSession {
    /// Send raw bytes to the daemon (will be written into the PTY's stdin).
    pub async fn send(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        self.send_frame(TY_DATA, bytes).await
    }

    /// Request a PTY resize on the daemon's end. Goes through the same
    /// stream as data, multiplexed by frame type.
    pub async fn resize(&self, cols: u16, rows: u16) -> anyhow::Result<()> {
        let payload =
            serde_json::to_vec(&Control::Resize { cols, rows }).context("encode resize")?;
        self.send_frame(TY_CONTROL, payload).await
    }

    /// Ask the daemon to watch `project_path` and start forwarding
    /// [`WorkerReply::GitRefresh`] frames for it. See
    /// `daemon-sandbox/src/frame.rs::Control::WatchProject` for the
    /// daemon-side semantics. Reissuing replaces the previous
    /// subscription.
    pub async fn watch_project(&self, project_path: String) -> anyhow::Result<()> {
        let payload = serde_json::to_vec(&Control::WatchProject { project_path })
            .context("encode watch_project")?;
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

    /// Start pushing inbound bytes into the given Dart StreamSink. Call once
    /// per session; subsequent calls return an error.
    pub async fn subscribe(&self, sink: StreamSink<Vec<u8>>) -> anyhow::Result<()> {
        let mut guard = self.incoming_rx.lock().await;
        let mut rx = guard
            .take()
            .ok_or_else(|| anyhow::anyhow!("already subscribed"))?;
        drop(guard);

        tokio_rt().spawn(async move {
            while let Some(bytes) = rx.recv().await {
                if sink.add(bytes).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    /// Start pushing decoded worker replies into the given Dart StreamSink.
    /// Same one-shot subscription shape as [`subscribe`]; the second call
    /// returns an error. Replies arrive in the order the daemon sent them.
    pub async fn subscribe_worker_replies(
        &self,
        sink: StreamSink<WorkerReply>,
    ) -> anyhow::Result<()> {
        let mut guard = self.worker_replies_rx.lock().await;
        let mut rx = guard
            .take()
            .ok_or_else(|| anyhow::anyhow!("already subscribed to worker replies"))?;
        drop(guard);

        tokio_rt().spawn(async move {
            while let Some(reply) = rx.recv().await {
                if sink.add(reply).is_err() {
                    break;
                }
            }
        });
        Ok(())
    }

    /// Closes the session. Safe to call multiple times.
    pub async fn close(&self) {
        self.send_tx.lock().await.take();
        if let Some(close_tx) = self.closer.lock().await.take() {
            let _ = close_tx.send(());
        }
    }
}
