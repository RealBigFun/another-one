//! Iroh QUIC transport for the sandbox daemon.
//!
//! Wire format: one bidirectional QUIC stream per connection carrying raw PTY
//! bytes both directions. No framing, no resize — sandbox-grade. Resize and
//! control framing get added when the real mobile client lands.

use anyhow::Context;
use iroh::endpoint::{presets, Connection, Incoming};
use iroh::Endpoint;
use tracing::{debug, info, warn};

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

    let mut session = PtySession::spawn(80, 24).context("pty session")?;
    let mut output_rx = session.output_rx;
    let mut writer = session.master_writer;

    // PTY output → Iroh send stream, on its own task so we can reader-loop.
    let send_task = tokio::spawn(async move {
        while let Some(bytes) = output_rx.recv().await {
            if send.write_all(&bytes).await.is_err() {
                debug!("iroh send failed");
                break;
            }
        }
        let _ = send.finish();
    });

    // Iroh recv stream → PTY master writer (current task, blocking-style).
    let mut buf = vec![0u8; 4096];
    loop {
        match recv.read(&mut buf).await {
            Ok(Some(0)) | Ok(None) => break,
            Ok(Some(n)) => {
                if let Err(e) = std::io::Write::write_all(&mut writer, &buf[..n]) {
                    warn!(error = %e, "iroh→pty write failed");
                    break;
                }
                let _ = std::io::Write::flush(&mut writer);
            }
            Err(e) => {
                debug!(error = %e, "iroh recv error");
                break;
            }
        }
    }

    // Teardown. Stop pumping bytes out first, then kill the shell.
    send_task.abort();
    let _ = session.child.kill();
    let _ = session.child.wait();
    drop(session.master);
    info!("iroh session ended");
    Ok(())
}
