//! Length-prefixed framing + transport adapters for the Iroh bidi
//! stream.
//!
//! Wire format: `[1 byte type][4 bytes BE length][N bytes payload]`.
//!
//! The wire **types** (`Control`, `WorkerReply`, `ControlEnvelope`,
//! `WorkerReplyEnvelope`, `ProjectSummary`, etc.) live in the
//! `daemon-proto` crate so the desktop daemon and every client
//! deserialize the same code path. This module keeps only the IO
//! helpers — read/write frame, the small `ReadExactish` /
//! `WriteAllAsync` trait surface, and the iroh impls — because those
//! pull in `tokio` + `iroh` + `anyhow` and have no business shipping
//! to the lean client crate.
//!
//! Used by the server ([`super::transport_iroh`]) and the
//! standalone smoke-test bin. See
//! [[docs/architecture/transport-abstraction]].

use anyhow::Context;

// TODO(another-one-eha): drop this re-export. Daemon-internal call
// sites should import wire types directly from `daemon_proto`; the
// glob is here only so the extraction PR could land in one piece.
pub use daemon_proto::*;

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
