//! Iroh QUIC transport for the sandbox daemon.
//!
//! Wire format: one bidirectional QUIC stream per connection, each message
//! framed as `[1 byte type][4 bytes BE length][N bytes payload]` (see
//! [`crate::frame`]). `0x00` frames carry PTY bytes in either direction,
//! `0x01` frames carry JSON control messages (currently `resize`).

use anyhow::Context;
use iroh::endpoint::{presets, Connection, Incoming};
use iroh::Endpoint;
use tokio::sync::mpsc;
use tracing::{debug, info, warn};

use crate::frame::{self, Control};
use crate::pty::PtySession;

/// ALPN advertised by the sandbox. Version-suffixed so future protocol breaks
/// can be versioned cleanly (`/1`, `/2`, …).
pub const ALPN: &[u8] = b"anotherone/pty/0";

/// Runs the Iroh endpoint loop until its `accept()` stream ends.
pub async fn serve() -> anyhow::Result<()> {
    let endpoint = Endpoint::builder(presets::N0)
        .alpns(vec![ALPN.to_vec()])
        .bind()
        .await
        .context("bind iroh endpoint")?;

    let endpoint_id = endpoint.id();
    info!("iroh EndpointId: {}", endpoint_id);
    info!("iroh ALPN: {}", String::from_utf8_lossy(ALPN));

    // Sandbox convenience: stash the EndpointId in /tmp so `iroh-client`
    // can pick it up without copy/paste. Production daemons should persist
    // their secret key and publish identity through a pairing flow instead.
    let id_path = std::env::temp_dir().join("daemon-sandbox.nodeid");
    if let Err(e) = std::fs::write(&id_path, endpoint_id.to_string()) {
        warn!(error = %e, "failed to write NodeId hint file");
    } else {
        info!("EndpointId written to {}", id_path.display());
    }

    endpoint.online().await;
    info!("iroh endpoint online: {:?}", endpoint.addr());

    // After going online, also stash the full EndpointAddr (id + direct
    // socket addrs + relay URLs) as a newline-delimited text file so
    // iroh-client and the mobile sandbox can dial without depending on DNS
    // address lookup. Relay entries let off-LAN clients (CGNAT'd mobile on
    // cellular, different networks) reach the daemon through the dev relay
    // mesh when direct hole-punching fails.
    // Format:
    //   id=<hex>
    //   addr=<ip:port>
    //   …
    //   relay=<url>
    //   …
    let addr = endpoint.addr();
    let mut ticket = format!("id={}\n", addr.id);
    for ip in addr.ip_addrs() {
        ticket.push_str(&format!("addr={ip}\n"));
    }
    for relay in addr.relay_urls() {
        ticket.push_str(&format!("relay={relay}\n"));
    }
    let ticket_path = std::env::temp_dir().join("daemon-sandbox.ticket");
    if let Err(e) = std::fs::write(&ticket_path, ticket) {
        warn!(error = %e, "failed to write ticket file");
    } else {
        info!("Ticket written to {}", ticket_path.display());
    }

    while let Some(incoming) = endpoint.accept().await {
        tokio::spawn(async move {
            if let Err(e) = handle_incoming(incoming).await {
                warn!(error = %e, "iroh connection error");
            }
        });
    }
    Ok(())
}

async fn handle_incoming(incoming: Incoming) -> anyhow::Result<()> {
    let conn = incoming.accept().context("accept")?.await.context("handshake")?;
    let remote = conn.remote_id();
    info!(%remote, "iroh client connected");
    handle_connection(conn).await
}

async fn handle_connection(conn: Connection) -> anyhow::Result<()> {
    let (mut send, mut recv) = conn.accept_bi().await.context("accept_bi")?;

    let session = PtySession::spawn(80, 24).context("pty session")?;
    let mut output_rx = session.output_rx;
    let mut writer = session.master_writer;

    // The recv loop needs to send resize commands to the master, which is
    // held by `session`. Use a channel so the loop doesn't need a direct
    // handle across the split.
    let (resize_tx, mut resize_rx) = mpsc::channel::<(u16, u16)>(8);
    let master = session.master;
    let resize_task = tokio::spawn(async move {
        while let Some((cols, rows)) = resize_rx.recv().await {
            if let Err(e) = master.resize(iroh_pty_size(cols, rows)) {
                warn!(error = %e, "pty resize failed");
            } else {
                debug!(cols, rows, "iroh resized");
            }
        }
        // Drop the master when the channel closes so the PTY frees.
        drop(master);
    });

    // PTY output → Iroh send stream, framed as type-0.
    let send_task = tokio::spawn(async move {
        while let Some(bytes) = output_rx.recv().await {
            if let Err(e) = frame::write_frame(&mut send, frame::TY_DATA, &bytes).await {
                debug!(error = %e, "iroh frame write failed");
                break;
            }
        }
        let _ = send.finish();
    });

    // Iroh recv stream → dispatch by frame type.
    loop {
        match frame::read_frame(&mut recv).await {
            Ok(Some((frame::TY_DATA, payload))) => {
                if let Err(e) = std::io::Write::write_all(&mut writer, &payload) {
                    warn!(error = %e, "iroh→pty write failed");
                    break;
                }
                let _ = std::io::Write::flush(&mut writer);
            }
            Ok(Some((frame::TY_CONTROL, payload))) => {
                match serde_json::from_slice::<Control>(&payload) {
                    Ok(Control::Resize { cols, rows }) => {
                        if resize_tx.send((cols, rows)).await.is_err() {
                            debug!("resize channel closed");
                            break;
                        }
                    }
                    Err(e) => {
                        warn!(error = %e, "bad iroh control frame");
                    }
                }
            }
            Ok(Some((ty, _))) => {
                warn!(frame_type = ty, "unknown iroh frame type");
            }
            Ok(None) => {
                debug!("iroh peer closed send");
                break;
            }
            Err(e) => {
                warn!(error = %e, "iroh frame read failed");
                break;
            }
        }
    }

    // Teardown. Dropping resize_tx closes the resize channel → resize_task
    // drops the master → PTY frees. Then stop pumping output + kill shell.
    drop(resize_tx);
    send_task.abort();
    resize_task.abort();
    let mut child = session.child;
    let _ = child.kill();
    let _ = child.wait();
    info!("iroh session ended");
    Ok(())
}

fn iroh_pty_size(cols: u16, rows: u16) -> portable_pty::PtySize {
    portable_pty::PtySize {
        cols,
        rows,
        pixel_width: 0,
        pixel_height: 0,
    }
}
