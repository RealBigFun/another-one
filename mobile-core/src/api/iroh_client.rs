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
use iroh::{Endpoint, EndpointAddr, EndpointId, RelayMode};

/// Must match the daemon's ALPN byte string.
const ALPN: &[u8] = b"anotherone/pty/0";

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
    let layer = tracing_android::layer("mobile_core")
        .expect("tracing-android layer");

    #[cfg(not(target_os = "android"))]
    let layer = tracing_subscriber::fmt::layer();

    let _ = tracing_subscriber::registry().with(filter).with(layer).try_init();
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


/// Dial a daemon's Iroh endpoint by its public `EndpointId`, with one or more
/// explicit direct `host:port` socket addresses.
///
/// The sandbox does not ship an address-lookup service or relay, so the
/// client needs the daemon's IP:port to dial. The daemon prints its
/// EndpointAddr on startup; pass those addresses through here.
pub async fn iroh_connect(
    endpoint_id: String,
    direct_addrs: Vec<String>,
) -> anyhow::Result<IrohSession> {
    tokio_rt()
        .spawn(async move { iroh_connect_inner(endpoint_id, direct_addrs).await })
        .await
        .map_err(|e| anyhow::anyhow!("connect task panicked: {e}"))?
}

async fn iroh_connect_inner(
    endpoint_id: String,
    direct_addrs: Vec<String>,
) -> anyhow::Result<IrohSession> {
    tracing::info!(
        "iroh_connect: id={} direct={:?}",
        endpoint_id,
        direct_addrs
    );

    let id: EndpointId = endpoint_id
        .trim()
        .parse()
        .context("invalid EndpointId")?;

    // Parse direct addresses eagerly so bad input surfaces before bind.
    let parsed_addrs: Vec<std::net::SocketAddr> = direct_addrs
        .iter()
        .map(|s| {
            s.parse::<std::net::SocketAddr>()
                .map_err(|e| anyhow::anyhow!("bad direct addr {s:?}: {e}"))
        })
        .collect::<anyhow::Result<_>>()?;
    if parsed_addrs.is_empty() {
        return Err(anyhow::anyhow!(
            "at least one direct address is required (sandbox has no address lookup)"
        ));
    }

    tracing::info!("iroh_connect: binding (Minimal preset + explicit DNS)");
    // Android gotcha: `DnsResolver::default()` calls `with_system_defaults()`
    // which tries to read `/etc/resolv.conf`. iroh's own doc notes this "does
    // not work at least on some Androids" and says it falls back to Google
    // DNS — but in practice on the emulator the read hangs long enough to
    // stall bind(). We explicitly hand iroh a resolver pinned to 8.8.8.8 to
    // skip system detection entirely.
    let dns = DnsResolver::with_nameserver(
        "8.8.8.8:53".parse().expect("static ipv4 socket addr"),
    );
    let endpoint = tokio::time::timeout(
        std::time::Duration::from_secs(15),
        Endpoint::builder(presets::Minimal)
            .relay_mode(RelayMode::Disabled)
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
                    Err(e) => {
                        tracing::warn!(error = %e, "iroh recv error");
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
