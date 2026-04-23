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
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};

/// Must match the daemon's ALPN byte string.
const ALPN: &[u8] = b"anotherone/pty/0";

/// Dedicated tokio runtime for all iroh work. FRB's default async executor
/// is not a tokio runtime, so iroh's network actors never get polled if we
/// run them on the calling task. Everything below shuffles work onto here.
fn tokio_rt() -> &'static Runtime {
    static RT: OnceLock<Runtime> = OnceLock::new();
    RT.get_or_init(|| {
        tracing::info!("mobile_core: building dedicated tokio runtime");
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
    // Force the runtime to initialize eagerly so first-call latency doesn't
    // include runtime construction.
    let _ = tokio_rt();
}

/// Opaque handle to a live Iroh QUIC session. Dart holds this object and
/// calls methods on it; the actual Iroh state lives in Rust.
#[frb(opaque)]
pub struct IrohSession {
    /// The local endpoint we bound for this session. Closed on Drop.
    _endpoint: Endpoint,
    /// Sends bytes from Rust to the send task, which writes them into the
    /// QUIC send stream. `None` means closed.
    send_tx: Mutex<Option<mpsc::Sender<Vec<u8>>>>,
    /// Holds the bytes-from-daemon stream until `subscribe()` wires it to a
    /// Dart `StreamSink`. Taken once and moved into the forwarding task.
    incoming_rx: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    /// Closes the underlying connection when invoked.
    closer: Mutex<Option<tokio::sync::oneshot::Sender<()>>>,
}

/// Dial a daemon's Iroh endpoint by its public `EndpointId` (the hex string
/// formerly known as NodeId).
pub async fn iroh_connect(endpoint_id: String) -> anyhow::Result<IrohSession> {
    // Delegate to the tokio runtime so iroh's actors actually get polled.
    // We `await` the JoinHandle, which is runtime-agnostic as a Future.
    tokio_rt()
        .spawn(async move { iroh_connect_inner(endpoint_id).await })
        .await
        .map_err(|e| anyhow::anyhow!("connect task panicked: {e}"))?
}

async fn iroh_connect_inner(endpoint_id: String) -> anyhow::Result<IrohSession> {
    tracing::info!("iroh_connect: id={}", endpoint_id);

    let id: EndpointId = endpoint_id
        .trim()
        .parse()
        .context("invalid EndpointId")?;

    tracing::info!("iroh_connect: binding local endpoint");
    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind client endpoint")?;
    tracing::info!("iroh_connect: endpoint bound, waiting for online");

    endpoint.online().await;
    tracing::info!("iroh_connect: online, dialing");

    let conn = endpoint
        .connect(EndpointAddr::new(id), ALPN)
        .await
        .context("connect to daemon")?;
    tracing::info!("iroh_connect: connected");

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;
    tracing::info!("iroh_connect: opened bidi stream");

    // Outbound pipe: Dart → channel → QUIC send stream.
    let (send_tx, mut send_rx) = mpsc::channel::<Vec<u8>>(64);
    tokio_rt().spawn(async move {
        while let Some(bytes) = send_rx.recv().await {
            if send.write_all(&bytes).await.is_err() {
                break;
            }
        }
        let _ = send.finish();
    });

    // Inbound pipe: QUIC recv stream → channel → Dart (once subscribed).
    let (incoming_tx, incoming_rx) = mpsc::channel::<Vec<u8>>(128);
    let (close_tx, mut close_rx) = tokio::sync::oneshot::channel::<()>();
    let conn_for_close = conn.clone();
    tokio_rt().spawn(async move {
        let mut buf = vec![0u8; 4096];
        loop {
            tokio::select! {
                _ = &mut close_rx => break,
                read = recv.read(&mut buf) => match read {
                    Ok(Some(0)) | Ok(None) => break,
                    Ok(Some(n)) => {
                        if incoming_tx.send(buf[..n].to_vec()).await.is_err() {
                            break;
                        }
                    }
                    Err(_) => break,
                },
            }
        }
        conn_for_close.close(0u8.into(), b"close");
    });

    Ok(IrohSession {
        _endpoint: endpoint,
        send_tx: Mutex::new(Some(send_tx)),
        incoming_rx: Mutex::new(Some(incoming_rx)),
        closer: Mutex::new(Some(close_tx)),
    })
}

impl IrohSession {
    /// Send raw bytes to the daemon (will be written into the PTY's stdin).
    pub async fn send(&self, bytes: Vec<u8>) -> anyhow::Result<()> {
        let tx = self.send_tx.lock().await;
        match tx.as_ref() {
            Some(tx) => tx
                .send(bytes)
                .await
                .map_err(|_| anyhow::anyhow!("send channel closed")),
            None => Err(anyhow::anyhow!("session closed")),
        }
    }

    /// Start pushing inbound bytes into the given Dart StreamSink. Call once
    /// per session; subsequent calls return an error.
    pub async fn subscribe(
        &self,
        sink: StreamSink<Vec<u8>>,
    ) -> anyhow::Result<()> {
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

    /// Closes the session. Safe to call multiple times.
    pub async fn close(&self) {
        self.send_tx.lock().await.take();
        if let Some(close_tx) = self.closer.lock().await.take() {
            let _ = close_tx.send(());
        }
    }
}
