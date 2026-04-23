//! Sandbox client for the daemon's Iroh endpoint. Dials a NodeId, opens a
//! bidirectional stream, sends a line of input, prints the PTY output for a
//! few seconds, then exits. Smoke test only — the real mobile client will
//! eventually use a flutter_rust_bridge wrapper around this pattern.
//!
//! Usage:
//!   cargo run -p daemon-sandbox --bin iroh-client
//!     (reads NodeId from /tmp/daemon-sandbox.nodeid)
//!   cargo run -p daemon-sandbox --bin iroh-client -- <node-id>

use std::net::SocketAddr;
use std::time::Duration;

use anyhow::Context;
use iroh::endpoint::presets;
use iroh::{Endpoint, EndpointAddr, EndpointId};

#[path = "../frame.rs"]
mod frame;

const ALPN: &[u8] = b"anotherone/pty/0";

/// Reads `/tmp/daemon-sandbox.ticket` (written by the daemon on startup)
/// and returns `(EndpointId, Vec<SocketAddr>)`. Returns `None` if the
/// file doesn't exist or lacks an id line.
fn load_ticket() -> anyhow::Result<Option<(EndpointId, Vec<SocketAddr>)>> {
    let path = std::env::temp_dir().join("daemon-sandbox.ticket");
    let Ok(content) = std::fs::read_to_string(&path) else {
        return Ok(None);
    };
    let mut id = None;
    let mut addrs = Vec::new();
    for line in content.lines() {
        if let Some(rest) = line.strip_prefix("id=") {
            id = Some(rest.trim().parse().context("parse EndpointId in ticket")?);
        } else if let Some(rest) = line.strip_prefix("addr=") {
            addrs.push(rest.trim().parse().context("parse addr in ticket")?);
        }
    }
    Ok(id.map(|id| (id, addrs)))
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt().init();

    // Prefer the ticket file (has direct addrs → no DNS dependency). Fall
    // back to a CLI argument or the older nodeid-only hint file.
    let (endpoint_id, direct_addrs) = if let Some(args) = std::env::args().nth(1) {
        let id: EndpointId = args.parse().context("invalid EndpointId argument")?;
        (id, Vec::new())
    } else if let Some(ticket) = load_ticket()? {
        ticket
    } else {
        let path = std::env::temp_dir().join("daemon-sandbox.nodeid");
        let content = std::fs::read_to_string(&path).with_context(|| {
            format!(
                "no EndpointId argument and no ticket at {} — is the daemon running?",
                path.display()
            )
        })?;
        (
            content.trim().parse().context("parse EndpointId")?,
            Vec::new(),
        )
    };
    eprintln!(
        "[client] dialing {} ({} direct addrs)",
        endpoint_id,
        direct_addrs.len()
    );

    let endpoint = Endpoint::bind(presets::N0)
        .await
        .context("bind client endpoint")?;
    let mut addr = EndpointAddr::new(endpoint_id);
    for sa in &direct_addrs {
        addr = addr.with_ip_addr(*sa);
    }
    let conn = endpoint.connect(addr, ALPN).await.context("connect")?;
    eprintln!("[client] connected");

    let (mut send, mut recv) = conn.open_bi().await.context("open_bi")?;

    // Send a resize control frame first (type 1, JSON payload) so the
    // daemon's PTY is appropriately sized before anything else.
    let resize = serde_json::to_vec(&frame::Control::Resize {
        cols: 100,
        rows: 30,
    })?;
    frame::write_frame(&mut send, frame::TY_CONTROL, &resize)
        .await
        .context("write resize control")?;
    eprintln!("[client] sent resize 100x30");

    frame::write_frame(
        &mut send,
        frame::TY_DATA,
        b"echo HELLO_FROM_IROH_$((7*6))\n",
    )
    .await
    .context("write input")?;
    eprintln!("[client] sent input");

    // Read for ~2s.
    let deadline = tokio::time::Instant::now() + Duration::from_secs(2);
    let mut total = 0usize;
    loop {
        tokio::select! {
            read = frame::read_frame(&mut recv) => match read {
                Ok(Some((frame::TY_DATA, payload))) => {
                    total += payload.len();
                    let text = String::from_utf8_lossy(&payload);
                    eprintln!("[server→client {}B] {:?}", payload.len(), text);
                }
                Ok(Some((ty, payload))) => {
                    eprintln!("[server→client type={} {}B]", ty, payload.len());
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

    // Graceful shutdown.
    let _ = send.finish();
    conn.close(0u8.into(), b"done");
    endpoint.close().await;
    Ok(())
}
