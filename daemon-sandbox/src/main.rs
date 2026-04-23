//! Throwaway daemon for mobile-companion sandbox.
//!
//! Runs two parallel transports, both bridging bytes to a spawned PTY:
//!
//!   - WebSocket on `ws://127.0.0.1:5617/pty` — used by the Flutter client.
//!   - Iroh QUIC on ALPN `anotherone/pty/0` — used by the iroh-client smoke
//!     test and (eventually) a Flutter client via flutter_rust_bridge.
//!
//! No auth, no session management — this exists to prove the transport
//! shape. See `plan` / `AGENTS.md` for where this is headed.

mod pty;
mod transport_iroh;
mod transport_ws;

use std::net::SocketAddr;

use anyhow::Context;
use tracing::info;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "daemon_sandbox=debug,info".into()),
        )
        .init();

    let ws_addr: SocketAddr = std::env::var("DAEMON_ADDR")
        .unwrap_or_else(|_| transport_ws::DEFAULT_ADDR.to_string())
        .parse()
        .context("invalid DAEMON_ADDR")?;

    let iroh_handle = tokio::spawn(transport_iroh::serve());
    transport_ws::serve(ws_addr, shutdown_signal()).await?;

    iroh_handle.abort();
    Ok(())
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown requested");
}
