//! Sandbox client for the daemon's Iroh endpoint. Dials a NodeId, opens a
//! bidirectional stream, sends a line of input, prints the PTY output for a
//! few seconds, then exits. Smoke test only — the real mobile client will
//! eventually use a flutter_rust_bridge wrapper around this pattern.
//!
//! Usage:
//!   cargo run -p daemon-sandbox --bin iroh-client
//!     (reads NodeId from /tmp/daemon-sandbox.nodeid)
//!   cargo run -p daemon-sandbox --bin iroh-client -- <node-id>

use std::time::Duration;

use anyhow::Context;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};

const ALPN: &[u8] = b"anotherone/pty/0";

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    let endpoint_id: EndpointId = match std::env::args().nth(1) {
        Some(arg) => arg.parse().context("invalid EndpointId argument")?,
        None => {
            let path = std::env::temp_dir().join("daemon-sandbox.nodeid");
            let content = std::fs::read_to_string(&path).with_context(|| {
                format!(
                    "no EndpointId argument and {} not readable — is the daemon running?",
                    path.display()
                )
            })?;
            content.trim().parse().context("parse EndpointId from temp file")?
        }
    };
    eprintln!("[client] dialing {}", endpoint_id);

    let endpoint = Endpoint::bind(presets::N0).await.context("bind client endpoint")?;
    let addr = EndpointAddr::new(endpoint_id);
    let conn = endpoint.connect(addr, ALPN).await.context("connect")?;
    eprintln!("[client] connected");

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;

    send.write_all(b"echo HELLO_FROM_IROH_$((7*6))\n").await.context("write")?;
    eprintln!("[client] sent input");

    // Read for ~2s.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut buf = vec![0u8; 4096];
    let mut total = 0usize;
    loop {
        tokio::select! {
            read = recv.read(&mut buf) => match read {
                Ok(Some(0)) => break,
                Ok(Some(n)) => {
                    total += n;
                    let text = String::from_utf8_lossy(&buf[..n]);
                    eprintln!("[server→client {}B] {:?}", n, text);
                }
                Ok(None) => break,
                Err(e) => {
                    eprintln!("[client] recv error: {e}");
                    break;
                }
            },
            _ = tokio::time::sleep_until(deadline) => {
                eprintln!("[client] 2s quiet window — done (total {}B)", total);
                break;
            }
        }
    }

    // Graceful shutdown: finish the stream, close the connection, close the
    // endpoint. Without endpoint.close(), iroh logs a loud "Endpoint dropped
    // without calling close" error.
    let _ = send.finish();
    conn.close(0u8.into(), b"done");
    endpoint.close().await;
    Ok(())
}
