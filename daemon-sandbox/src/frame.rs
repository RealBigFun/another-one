//! Simple length-prefixed framing for the Iroh bidi stream.
//!
//! Wire format: `[1 byte type][4 bytes BE length][N bytes payload]`.
//!
//! Types:
//! - `0x00` — PTY data (raw bytes, either direction)
//! - `0x01` — JSON control message (UTF-8; see [`Control`])
//! - `0x02` — JSON worker reply (UTF-8; see [`WorkerReply`]).
//!   Daemon → client only. One variant per core-extracted worker that
//!   the daemon forwards to the client. Unknown variants MUST be
//!   ignored by clients so older clients keep working as we add
//!   workers.
//!
//! Used by both the server ([`super::transport_iroh`]) and the client
//! smoke test (`bin/iroh-client.rs`). See [[docs/architecture/transport-abstraction]].

use anyhow::Context;
use serde::{Deserialize, Serialize};

pub const TY_DATA: u8 = 0x00;
pub const TY_CONTROL: u8 = 0x01;
pub const TY_WORKER_REPLY: u8 = 0x02;

/// Reject any frame larger than this. 64 KiB is comfortably more than
/// any real PTY chunk (readers use 4 KiB buffers) or resize JSON payload
/// (~40 bytes), so there is no legitimate reason for a peer to announce
/// a larger frame. Keeping the cap tight limits how much a compromised
/// paired peer can make the daemon allocate per frame.
pub const MAX_FRAME_BYTES: usize = 64 * 1024;

/// Client → daemon session-control messages (type=1 frames). Payload
/// is JSON. Server → client control is not currently used (the daemon
/// pushes data via `0x00` and worker replies via `0x02`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    Resize {
        cols: u16,
        rows: u16,
    },
    /// Ask the daemon to spawn the `git_refresh` worker for
    /// `project_path` and forward its reply as a
    /// [`TY_WORKER_REPLY`] frame. Per-session; reissuing replaces
    /// the previous subscription. `project_path` is an absolute
    /// path on the daemon host — the paired client is trusted to
    /// ask for paths it's allowed to see (the TOFU allowlist is
    /// the trust boundary for the sandbox).
    WatchProject {
        project_path: String,
    },
}

/// Worker replies (type=2 frames). Payload is JSON. Daemon → client
/// only.
///
/// Each variant is a lossy projection of one `core::*_service`
/// worker's reply type. We deliberately do *not* derive Serialize on
/// the core reply types themselves — those structs are shaped for the
/// desktop's GPUI state, with nested `Result<_, String>` and internal
/// metadata the mobile UI doesn't need. This wire type is the curated
/// subset we commit to as a public protocol.
///
/// Wire-compat rules:
/// - `#[serde(tag = "kind")]` — every message carries its discriminator,
///   so new variants can be added without renumbering.
/// - New variants: clients built before the variant existed hit
///   serde's "unknown variant" error. To stay forwards-compatible,
///   clients SHOULD decode into a shape that tolerates unknown
///   variants (e.g., decode to `serde_json::Value` first, then try
///   `WorkerReply`). The current Flutter client just logs-and-ignores
///   unknown frame *types* (via the `0x02` discriminator itself), so
///   until it upgrades to variant-awareness, the daemon should only
///   emit variants the contemporaneous client supports. Track client
///   capability out of band (ALPN version bump or a hello frame) when
///   we move beyond this slice.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum WorkerReply {
    /// Projection of `core::git_service::GitRefreshReply`. Contains
    /// only the fields the mobile UI currently needs; expand as UI
    /// grows.
    GitRefresh {
        project_id: String,
        current_branch: Option<String>,
        changed_file_count: usize,
        ahead: usize,
        behind: usize,
    },
    /// Projection of `core::git_service::ProjectPullRequestReply`.
    /// `pr = None` means "checked and no open/recent PR for
    /// `branch_name`", distinct from "haven't looked yet". The
    /// daemon only emits one of these per WatchProject session
    /// after the GitRefresh reply, and only if the refresh found
    /// a current branch.
    PullRequestStatus {
        project_id: String,
        branch_name: String,
        pr: Option<PullRequestInfo>,
    },
}

/// Lossy wire projection of `core::git_actions::PullRequestStatus`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PullRequestInfo {
    pub number: u64,
    pub url: String,
    pub state: PullRequestState,
}

/// Mirror of `core::git_actions::PullRequestState`. Serialised as
/// lowercase strings (`"open"`, `"closed"`, `"merged"`) for a
/// readable wire shape.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PullRequestState {
    Open,
    Closed,
    Merged,
}

/// Reads one frame from an Iroh `RecvStream`. Returns `None` when the
/// peer has cleanly closed the send side.
pub async fn read_frame<R>(recv: &mut R) -> anyhow::Result<Option<(u8, Vec<u8>)>>
where
    R: ReadExactish + Unpin,
{
    let mut header = [0u8; 5];
    match recv.read_exactish(&mut header).await? {
        ReadOutcome::Closed => return Ok(None),
        ReadOutcome::Got => {}
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    anyhow::ensure!(
        len <= MAX_FRAME_BYTES,
        "frame too large: {len} bytes (max {MAX_FRAME_BYTES})"
    );
    let mut payload = vec![0u8; len];
    match recv.read_exactish(&mut payload).await? {
        ReadOutcome::Closed => anyhow::bail!("stream ended mid-frame"),
        ReadOutcome::Got => {}
    }
    Ok(Some((ty, payload)))
}

/// Writes one frame to an Iroh `SendStream`.
pub async fn write_frame<W>(send: &mut W, ty: u8, payload: &[u8]) -> anyhow::Result<()>
where
    W: WriteAllAsync + Unpin,
{
    let mut header = [0u8; 5];
    header[0] = ty;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    send.write_all_async(&header)
        .await
        .context("write header")?;
    send.write_all_async(payload)
        .await
        .context("write payload")?;
    Ok(())
}

// Tiny trait-shaped adapters so we can use these helpers with both iroh's
// send/recv streams and any future transport wrapper. Keeps the frame
// module transport-agnostic.

pub enum ReadOutcome {
    Got,
    Closed,
}

pub trait ReadExactish {
    fn read_exactish(
        &mut self,
        buf: &mut [u8],
    ) -> impl std::future::Future<Output = anyhow::Result<ReadOutcome>> + Send;
}

pub trait WriteAllAsync {
    fn write_all_async(
        &mut self,
        data: &[u8],
    ) -> impl std::future::Future<Output = anyhow::Result<()>> + Send;
}

impl ReadExactish for iroh::endpoint::RecvStream {
    async fn read_exactish(&mut self, buf: &mut [u8]) -> anyhow::Result<ReadOutcome> {
        let mut read = 0;
        while read < buf.len() {
            match self.read(&mut buf[read..]).await {
                Ok(Some(0)) | Ok(None) => {
                    return if read == 0 {
                        Ok(ReadOutcome::Closed)
                    } else {
                        Err(anyhow::anyhow!(
                            "stream closed mid-read after {read} of {} bytes",
                            buf.len()
                        ))
                    };
                }
                Ok(Some(n)) => {
                    read += n;
                }
                Err(e) => return Err(e.into()),
            }
        }
        Ok(ReadOutcome::Got)
    }
}

impl WriteAllAsync for iroh::endpoint::SendStream {
    async fn write_all_async(&mut self, data: &[u8]) -> anyhow::Result<()> {
        self.write_all(data).await.map_err(Into::into)
    }
}
