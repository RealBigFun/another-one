//! Standalone `daemon-sandbox` binary.
//!
//! Runs two parallel transports against a synthetic single-task
//! [`SandboxRegistry`]:
//!
//!   - Optional WebSocket on `ws://127.0.0.1:5617/pty` — legacy,
//!     unauthenticated, and disabled by default.
//!   - Iroh QUIC on ALPN `anotherone/pty/1` — the main path; the
//!     mobile app and `iroh-client` smoke test both dial it.
//!
//! The library crate (`daemon_sandbox::run_endpoint`) powers the
//! iroh side. The desktop app links the same library and supplies
//! its *own* `DaemonRegistry` impl — when you're running the real
//! AnotherOne app, this binary isn't involved.

use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Context;
use tracing::{info, warn};

use daemon_sandbox::sandbox::SandboxRegistry;
use daemon_sandbox::{transport_ws, EndpointHandle};

const ENABLE_INSECURE_WS_ENV: &str = "ANOTHER_ONE_ENABLE_INSECURE_WS";

#[derive(Default)]
struct PublishedArtifacts {
    pairing_png_path: Option<PathBuf>,
}

impl Drop for PublishedArtifacts {
    fn drop(&mut self) {
        let Some(path) = self.pairing_png_path.as_ref() else {
            return;
        };
        if let Err(e) = std::fs::remove_file(path) {
            if e.kind() != std::io::ErrorKind::NotFound {
                warn!(path = %path.display(), error = %e, "failed to clean up pairing PNG");
            }
        }
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "daemon_sandbox=debug,info".into()),
        )
        .init();

    let data_dir = sandbox_data_dir()?;

    let registry = Arc::new(SandboxRegistry::new());
    let handle = daemon_sandbox::run_endpoint(
        registry,
        data_dir.join("secret_key"),
        data_dir.join("paired_peers"),
    )
    .await
    .context("start embedded iroh endpoint")?;

    let _published_artifacts = publish_sandbox_artifacts(&handle);

    // WebSocket transport remains self-contained: it spawns its own
    // PTY per connection, unrelated to the iroh path. This path only
    // exists to smoke-test the legacy Flutter WebSocket transport, so
    // keep it disabled by default and require an explicit opt-in.
    let ws_task = if insecure_ws_enabled() {
        let ws_addr: SocketAddr = std::env::var("DAEMON_ADDR")
            .unwrap_or_else(|_| transport_ws::DEFAULT_ADDR.to_string())
            .parse()
            .context("invalid DAEMON_ADDR")?;
        warn!(
            env = ENABLE_INSECURE_WS_ENV,
            addr = %ws_addr,
            "starting insecure legacy WebSocket PTY transport"
        );
        Some(tokio::spawn(transport_ws::serve(
            ws_addr,
            shutdown_signal(),
        )))
    } else {
        info!(
            env = ENABLE_INSECURE_WS_ENV,
            "legacy WebSocket PTY transport disabled"
        );
        None
    };

    if let Some(ws_task) = ws_task {
        let _ = ws_task.await;
    } else {
        shutdown_signal().await;
    }

    // Keep the iroh endpoint alive until the WS transport exits.
    drop(handle);
    Ok(())
}

fn sandbox_data_dir() -> anyhow::Result<PathBuf> {
    let base = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg)
    } else {
        let home = std::env::var("HOME").context("HOME is unset — can't locate data dir")?;
        PathBuf::from(home).join(".local").join("share")
    };
    let dir = base.join("another-one-sandbox");
    std::fs::create_dir_all(&dir).with_context(|| format!("create data dir {}", dir.display()))?;
    Ok(dir)
}

fn insecure_ws_enabled() -> bool {
    matches!(
        std::env::var(ENABLE_INSECURE_WS_ENV).as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    )
}

/// Drop the pairing URL into `/tmp/daemon-sandbox.ticket` for
/// `iroh-client` / external tooling, and publish the token-bearing QR
/// PNG into a private per-user sandbox-data directory with restrictive
/// permissions. Also echoes the URL on stdout for humans.
///
/// This is the one thing the library doesn't do for you — the
/// library is pure embedding, no filesystem side effects. The
/// sandbox binary re-publishes its EndpointHandle here explicitly.
fn publish_sandbox_artifacts(handle: &EndpointHandle) -> PublishedArtifacts {
    let mut artifacts = PublishedArtifacts::default();
    info!("iroh EndpointId: {}", handle.endpoint_id);
    let pairing_url = handle.pairing_url();
    println!("\nPairing URL:\n  {}", pairing_url);

    let ticket_path = std::env::temp_dir().join("daemon-sandbox.ticket");
    let ticket_body = ticket_body_from_url(&pairing_url);
    if let Err(e) = std::fs::write(&ticket_path, ticket_body) {
        warn!(error = %e, "failed to write ticket file");
    } else {
        info!("Ticket written to {}", ticket_path.display());
    }

    match publish_private_pairing_png(&handle.qr_png_bytes()) {
        Ok(png_path) => {
            println!("Pairing QR also written to {}", png_path.display());
            artifacts.pairing_png_path = Some(png_path);
        }
        Err(e) => {
            warn!(error = %e, "failed to write pairing PNG");
        }
    }

    // Legacy hint file — iroh-client still checks this path when no
    // .ticket is present. Writing the raw EndpointId keeps that
    // fallback working.
    let nodeid_path = std::env::temp_dir().join("daemon-sandbox.nodeid");
    let _ = std::fs::write(&nodeid_path, &handle.endpoint_id);
    artifacts
}

fn publish_private_pairing_png(png_bytes: &[u8]) -> anyhow::Result<PathBuf> {
    let dir = sandbox_data_dir()?.join("pairing");
    ensure_private_dir(&dir)?;
    let path = dir.join("daemon-sandbox.pairing.png");
    write_private_file(&path, png_bytes)?;
    Ok(path)
}

fn ensure_private_dir(path: &Path) -> anyhow::Result<()> {
    std::fs::create_dir_all(path).with_context(|| format!("create dir {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o700))
            .with_context(|| format!("set dir permissions {}", path.display()))?;
    }
    Ok(())
}

fn write_private_file(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options
        .open(path)
        .with_context(|| format!("open file {}", path.display()))?;
    use std::io::Write;
    file.write_all(bytes)
        .with_context(|| format!("write file {}", path.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
            .with_context(|| format!("set file permissions {}", path.display()))?;
    }
    Ok(())
}

/// Convert an `iroh://<id>?direct=…&relay=…` URL back into the flat
/// `id=…\naddr=…\nrelay=…` ticket format the smoke client parses.
fn ticket_body_from_url(url: &str) -> String {
    let url = url.strip_prefix("iroh://").unwrap_or(url);
    let (id, rest) = url.split_once('?').unwrap_or((url, ""));
    let mut body = format!("id={id}\n");
    for part in rest.split('&') {
        if let Some(directs) = part.strip_prefix("direct=") {
            for a in directs.split(',') {
                if !a.is_empty() {
                    body.push_str(&format!("addr={a}\n"));
                }
            }
        } else if let Some(relay) = part.strip_prefix("relay=") {
            let decoded = urlencoding::decode(relay)
                .map(|c| c.into_owned())
                .unwrap_or_else(|_| relay.to_string());
            body.push_str(&format!("relay={decoded}\n"));
        }
    }
    body
}

async fn shutdown_signal() {
    let _ = tokio::signal::ctrl_c().await;
    info!("shutdown requested");
}
