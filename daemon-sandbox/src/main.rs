//! Throwaway daemon for mobile-companion sandbox.
//!
//! Exposes two parallel transports that both bridge bytes to a spawned PTY:
//!
//!   1. WebSocket on `ws://127.0.0.1:5617/pty`
//!      - Binary frames ↔ raw PTY bytes.
//!      - Text frames ↔ JSON control messages (currently only `resize`).
//!      - Used by the Flutter sandbox client.
//!
//!   2. Iroh QUIC endpoint on ALPN `anotherone/pty/0`
//!      - Single bidirectional stream per connection.
//!      - Raw PTY bytes both directions, no framing. Resize is hardcoded to
//!        80×24 for now; control channel deferred until we need it.
//!      - Used by the iroh-client smoke test and (eventually) a Rust-bridged
//!        mobile client.
//!
//! No auth, no session management — the point is to prove both transports
//! shape before the real daemon picks a winner.

use std::io::{Read, Write};
use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Context;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use iroh::endpoint::{presets, Connection, Incoming};
use iroh::Endpoint;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::{mpsc, Mutex};
use tracing::{debug, error, info, warn};

pub const IROH_ALPN: &[u8] = b"anotherone/pty/0";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "daemon_sandbox=debug,info".into()),
        )
        .init();

    let ws_addr: SocketAddr = std::env::var("DAEMON_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:5617".to_string())
        .parse()
        .context("invalid DAEMON_ADDR")?;

    // Iroh endpoint runs alongside the WebSocket server.
    let iroh_handle = tokio::spawn(iroh_server_loop());

    let app = Router::new().route("/pty", get(ws_handler));
    let listener = tokio::net::TcpListener::bind(ws_addr)
        .await
        .context("bind WebSocket address")?;
    info!("WebSocket listening on ws://{}/pty", ws_addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("serve WebSocket")?;

    iroh_handle.abort();
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown requested");
}

// ==========================================================================
// PTY session — shared by both transports.
// ==========================================================================

struct PtySession {
    /// Receives bytes streamed from the PTY master.
    output_rx: mpsc::Receiver<Vec<u8>>,
    /// Write to this to send bytes into the PTY (stdin for the shell).
    master_writer: Box<dyn Write + Send>,
    /// Retained so we can resize and so the PTY stays open.
    master: Box<dyn portable_pty::MasterPty + Send>,
    /// The shell child; killed on session end.
    child: Box<dyn portable_pty::Child + Send + Sync>,
}

fn spawn_pty_session(cols: u16, rows: u16) -> anyhow::Result<PtySession> {
    let pty_system = native_pty_system();
    let pair = pty_system
        .openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })
        .context("openpty")?;

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.env("TERM", "xterm-256color");
    if let Ok(home) = std::env::var("HOME") {
        cmd.cwd(home);
    }

    let child = pair.slave.spawn_command(cmd).context("spawn shell")?;
    drop(pair.slave);

    let mut master_reader = pair.master.try_clone_reader().context("clone reader")?;
    let master_writer = pair.master.take_writer().context("take writer")?;

    let (tx, rx) = mpsc::channel::<Vec<u8>>(64);
    std::thread::spawn(move || {
        let mut buf = [0u8; 4096];
        loop {
            match master_reader.read(&mut buf) {
                Ok(0) => {
                    debug!("pty eof");
                    break;
                }
                Ok(n) => {
                    if tx.blocking_send(buf[..n].to_vec()).is_err() {
                        debug!("pty→transport channel closed");
                        break;
                    }
                }
                Err(e) => {
                    debug!(error = %e, "pty read error");
                    break;
                }
            }
        }
    });

    Ok(PtySession {
        output_rx: rx,
        master_writer,
        master: pair.master,
        child,
    })
}

// ==========================================================================
// WebSocket transport.
// ==========================================================================

async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_ws_session)
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WsControl {
    Resize { cols: u16, rows: u16 },
}

async fn handle_ws_session(mut ws: WebSocket) {
    info!("ws client connected");

    let mut session = match spawn_pty_session(80, 24) {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "spawn_pty_session failed");
            return;
        }
    };

    loop {
        tokio::select! {
            bytes = session.output_rx.recv() => match bytes {
                Some(bytes) => {
                    if ws.send(Message::Binary(bytes.into())).await.is_err() {
                        debug!("ws send failed");
                        break;
                    }
                }
                None => { break; }
            },
            msg = ws.recv() => match msg {
                Some(Ok(Message::Binary(bytes))) => {
                    if let Err(e) = session.master_writer.write_all(&bytes) {
                        warn!(error = %e, "pty write failed");
                        break;
                    }
                    let _ = session.master_writer.flush();
                }
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<WsControl>(&text) {
                        Ok(WsControl::Resize { cols, rows }) => {
                            if let Err(e) = session.master.resize(PtySize {
                                cols, rows, pixel_width: 0, pixel_height: 0,
                            }) {
                                warn!(error = %e, "pty resize failed");
                            } else {
                                debug!(cols, rows, "ws resized");
                            }
                        }
                        Err(e) => warn!(error = %e, text = %text, "bad ws control"),
                    }
                }
                Some(Ok(Message::Close(_))) | None => { break; }
                Some(Ok(_)) => {}
                Some(Err(e)) => { warn!(error = %e, "ws error"); break; }
            },
        }
    }

    let _ = session.child.kill();
    let _ = session.child.wait();
    drop(session.master);
    info!("ws session ended");
}

// ==========================================================================
// Iroh transport.
// ==========================================================================

async fn iroh_server_loop() -> anyhow::Result<()> {
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![IROH_ALPN.to_vec()])
        .bind()
        .await
        .context("bind iroh endpoint")?;

    let endpoint_id = endpoint.id();
    info!("iroh EndpointId: {}", endpoint_id);
    info!("iroh ALPN: {}", String::from_utf8_lossy(IROH_ALPN));

    // For convenience during dev, also write the EndpointId to a temp file so
    // the iroh-client binary can pick it up without copy/paste.
    let id_path = std::env::temp_dir().join("daemon-sandbox.nodeid");
    let _ = std::fs::write(&id_path, endpoint_id.to_string());
    info!("EndpointId also written to {}", id_path.display());

    // Wait for the endpoint to have a home relay / be reachable off-LAN.
    // Not required for localhost testing but nice when it's there.
    endpoint.online().await;
    info!("iroh endpoint online: {:?}", endpoint.addr());

    let endpoint = Arc::new(endpoint);
    loop {
        let accept = endpoint.accept();
        let Some(incoming) = accept.await else { break };
        let ep = endpoint.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_iroh_incoming(incoming).await {
                warn!(error = %e, "iroh connection error");
            }
            let _ = ep;
        });
    }
    Ok(())
}

async fn handle_iroh_incoming(incoming: Incoming) -> anyhow::Result<()> {
    let conn = incoming.accept().context("accept")?.await.context("handshake")?;
    let remote = conn.remote_id();
    info!(%remote, "iroh client connected");
    handle_iroh_connection(conn).await
}

async fn handle_iroh_connection(conn: Connection) -> anyhow::Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await.context("accept_bi")?;

    let mut session = spawn_pty_session(80, 24).context("pty session")?;

    // Move the writer into an Arc<Mutex> so both the recv-loop and the
    // end-of-session cleanup can touch it. (Cleanup just drops; the recv
    // loop is the only writer during the session.)
    let writer = Arc::new(Mutex::new(session.master_writer));

    // Task: PTY output → iroh send stream.
    let mut output_rx = session.output_rx;
    let send_task = tokio::spawn(async move {
        while let Some(bytes) = output_rx.recv().await {
            if send.write_all(&bytes).await.is_err() {
                debug!("iroh send failed");
                break;
            }
        }
        let _ = send.finish();
    });

    // Current task: iroh recv stream → PTY master writer.
    let mut buf = vec![0u8; 4096];
    loop {
        match recv.read(&mut buf).await {
            Ok(Some(0)) => break,
            Ok(Some(n)) => {
                let mut w = writer.lock().await;
                if let Err(e) = w.write_all(&buf[..n]) {
                    warn!(error = %e, "iroh→pty write failed");
                    break;
                }
                let _ = w.flush();
            }
            Ok(None) => {
                debug!("iroh recv stream finished");
                break;
            }
            Err(e) => {
                debug!(error = %e, "iroh recv error");
                break;
            }
        }
    }

    let _ = session.child.kill();
    let _ = session.child.wait();
    drop(session.master);
    send_task.abort();
    info!("iroh session ended");
    Ok(())
}
