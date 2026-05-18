//! WebSocket transport for the sandbox daemon.
//!
//! Wire format:
//! - Binary frames carry raw PTY bytes (both directions).
//! - Text frames are not used; any text frame logs a warning.
//!
//! **Authentication: none.** This transport is an emulator-and-loopback
//! convenience: the Flutter emulator reaches the host via `10.0.2.2`,
//! which is really the host's `127.0.0.1`. Any off-LAN device (real
//! phone, tablet, another machine) **must** use the Iroh transport,
//! which has pairing + TOFU-allowlist auth.
//!
//! To enforce this, [`serve`] refuses to bind a non-loopback address.
//! Do not add a "let me turn this off" flag — that's the footgun. If
//! you need a remote unauthenticated shell you can roll your own with
//! `socat`; don't grow this daemon to accept one.

use std::net::{IpAddr, SocketAddr};

use anyhow::Context;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use tracing::{debug, error, info, warn};

use crate::pty::PtySession;

/// Default listen address when `DAEMON_ADDR` isn't set.
pub const DEFAULT_ADDR: &str = "127.0.0.1:5617";

fn is_loopback(addr: &SocketAddr) -> bool {
    match addr.ip() {
        IpAddr::V4(v4) => v4.is_loopback(),
        IpAddr::V6(v6) => v6.is_loopback(),
    }
}

pub async fn serve<F>(addr: SocketAddr, shutdown: F) -> anyhow::Result<()>
where
    F: std::future::Future<Output = ()> + Send + 'static,
{
    if !is_loopback(&addr) {
        anyhow::bail!(
            "refusing to bind WebSocket to non-loopback address {addr}: this \
             transport is unauthenticated and only safe on loopback. Off-host \
             clients must use the Iroh transport (see the pairing URL/QR \
             printed on startup)."
        );
    }
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
                    warn!(text = %text, "ws transport: unexpected text frame; no control verbs are supported");
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
