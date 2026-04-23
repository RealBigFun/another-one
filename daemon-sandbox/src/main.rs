//! Throwaway daemon for mobile-companion sandbox.
//!
//! Exposes one WebSocket endpoint (`/pty`) that spawns a bash PTY and bridges
//! raw bytes bidirectionally. No auth, no Iroh, no session management — the
//! point is to prove the transport works before layering in real pieces.
//!
//! Protocol:
//!   - Binary frames (client ↔ server): raw PTY bytes.
//!   - Text frames (client → server): JSON control messages. Currently only
//!     `{"type":"resize","cols":N,"rows":N}` is understood.

use std::io::{Read, Write};
use std::net::SocketAddr;

use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "daemon_sandbox=debug,info".into()),
        )
        .init();

    let addr: SocketAddr = std::env::var("DAEMON_ADDR")
        .unwrap_or_else(|_| "127.0.0.1:5617".to_string())
        .parse()
        .expect("invalid DAEMON_ADDR");

    let app = Router::new().route("/pty", get(ws_handler));

    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .expect("bind failed");
    info!("daemon listening on ws://{}/pty", addr);
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("serve failed");
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown requested");
}

async fn ws_handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_pty_session)
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    Resize { cols: u16, rows: u16 },
}

async fn handle_pty_session(mut ws: WebSocket) {
    info!("client connected");

    let pty_system = native_pty_system();
    let pair = match pty_system.openpty(PtySize {
        rows: 24,
        cols: 80,
        pixel_width: 0,
        pixel_height: 0,
    }) {
        Ok(p) => p,
        Err(e) => {
            error!(error = %e, "openpty failed");
            return;
        }
    };

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "bash".to_string());
    let mut cmd = CommandBuilder::new(&shell);
    cmd.env("TERM", "xterm-256color");
    if let Ok(home) = std::env::var("HOME") {
        cmd.cwd(home);
    }

    let mut child = match pair.slave.spawn_command(cmd) {
        Ok(c) => c,
        Err(e) => {
            error!(error = %e, "spawn shell failed");
            return;
        }
    };
    drop(pair.slave);

    let mut master_reader = match pair.master.try_clone_reader() {
        Ok(r) => r,
        Err(e) => {
            error!(error = %e, "try_clone_reader failed");
            let _ = child.kill();
            return;
        }
    };
    let mut master_writer = match pair.master.take_writer() {
        Ok(w) => w,
        Err(e) => {
            error!(error = %e, "take_writer failed");
            let _ = child.kill();
            return;
        }
    };

    // Blocking read thread → async channel → WebSocket.
    let (tx, mut rx) = mpsc::channel::<Vec<u8>>(64);
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
                        debug!("pty→ws channel closed");
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

    // The master handle holds the PTY alive; move it into a scope that
    // drops only when the session ends.
    let master = pair.master;

    loop {
        tokio::select! {
            bytes = rx.recv() => match bytes {
                Some(bytes) => {
                    if ws.send(Message::Binary(bytes.into())).await.is_err() {
                        debug!("ws send failed");
                        break;
                    }
                }
                None => {
                    debug!("pty channel closed");
                    break;
                }
            },
            msg = ws.recv() => match msg {
                Some(Ok(Message::Binary(bytes))) => {
                    if let Err(e) = master_writer.write_all(&bytes) {
                        warn!(error = %e, "pty write failed");
                        break;
                    }
                    let _ = master_writer.flush();
                }
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<Control>(&text) {
                        Ok(Control::Resize { cols, rows }) => {
                            if let Err(e) = master.resize(PtySize {
                                cols,
                                rows,
                                pixel_width: 0,
                                pixel_height: 0,
                            }) {
                                warn!(error = %e, "pty resize failed");
                            } else {
                                debug!(cols, rows, "resized");
                            }
                        }
                        Err(e) => warn!(error = %e, text = %text, "bad control message"),
                    }
                }
                Some(Ok(Message::Close(_))) | None => {
                    debug!("client closed");
                    break;
                }
                Some(Ok(_)) => {} // ping/pong handled by axum
                Some(Err(e)) => {
                    warn!(error = %e, "ws error");
                    break;
                }
            },
        }
    }

    let _ = child.kill();
    let _ = child.wait();
    drop(master);
    info!("session ended");
}
