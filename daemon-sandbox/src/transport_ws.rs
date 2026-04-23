//! WebSocket transport for the sandbox daemon.
//!
//! Wire format:
//! - Binary frames carry raw PTY bytes (both directions).
//! - Text frames carry JSON control messages; currently just `resize`.
//!
//! This is the transport the Flutter sandbox client uses until Iroh has a
//! Flutter-reachable wrapper.

use std::net::SocketAddr;

use anyhow::Context;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use serde::Deserialize;
use tracing::{debug, error, info, warn};

use crate::pty::PtySession;

/// Default listen address when `DAEMON_ADDR` isn't set.
pub const DEFAULT_ADDR: &str = "127.0.0.1:5617";

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Control {
    Resize { cols: u16, rows: u16 },
}

pub async fn serve<F>(addr: SocketAddr, shutdown: F) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    let app = Router::new().route("/pty", get(handler));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .context("bind WebSocket address")?;
    info!("WebSocket listening on ws://{}/pty", addr);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown)
        .await
        .context("serve WebSocket")
}

async fn handler(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(handle_session)
}

async fn handle_session(mut ws: WebSocket) {
    info!("ws client connected");

    let mut session = match PtySession::spawn(80, 24) {
        Ok(s) => s,
        Err(e) => {
            error!(error = %e, "spawn pty session failed");
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
                None => break,
            },
            msg = ws.recv() => match msg {
                Some(Ok(Message::Binary(bytes))) => {
                    if let Err(e) = session.write_input(&bytes) {
                        warn!(error = %e, "pty write failed");
                        break;
                    }
                }
                Some(Ok(Message::Text(text))) => {
                    match serde_json::from_str::<Control>(&text) {
                        Ok(Control::Resize { cols, rows }) => {
                            if let Err(e) = session.resize(cols, rows) {
                                warn!(error = %e, "pty resize failed");
                            } else {
                                debug!(cols, rows, "ws resized");
                            }
                        }
                        Err(e) => warn!(error = %e, text = %text, "bad ws control"),
                    }
                }
                Some(Ok(Message::Close(_))) | None => break,
                Some(Ok(_)) => {} // ping/pong handled by axum
                Some(Err(e)) => {
                    warn!(error = %e, "ws error");
                    break;
                }
            },
        }
    }

    session.close();
    info!("ws session ended");
}
