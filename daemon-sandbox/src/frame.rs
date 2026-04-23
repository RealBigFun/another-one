//! Simple length-prefixed framing for the Iroh bidi stream.
//!
//! Wire format: `[1 byte type][4 bytes BE length][N bytes payload]`.
//!
//! Types:
//! - `0x00` — PTY data (raw bytes, either direction)
//! - `0x01` — JSON control message (UTF-8; see [`Control`])
//!
//! Used by both the server ([`super::transport_iroh`]) and the client
//! smoke test (`bin/iroh-client.rs`). See [[docs/architecture/transport-abstraction]].

use anyhow::Context;
use serde::{Deserialize, Serialize};

pub const TY_DATA: u8 = 0x00;
pub const TY_CONTROL: u8 = 0x01;

/// Reject any frame larger than this so a bogus length header can't
/// make us allocate gigabytes.
pub const MAX_FRAME_BYTES: usize = 16 * 1024 * 1024; // 16 MiB

/// Control messages (type=1 frames). Payload is JSON.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Control {
    Resize { cols: u16, rows: u16 },
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
    send.write_all_async(&header).await.context("write header")?;
    send.write_all_async(payload).await.context("write payload")?;
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
