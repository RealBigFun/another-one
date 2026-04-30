//! Wire frame I/O against `iroh::endpoint::{SendStream, RecvStream}`.
//!
//! Header is `[1B type][4B BE length][N B payload]`. Frames over
//! `MAX_FRAME_BYTES` are rejected. Mirror of
//! `daemon/src/frame.rs`.

use iroh::endpoint::{RecvStream, SendStream};

use crate::protocol::MAX_FRAME_BYTES;

/// Writes one frame to the Iroh send stream.
pub async fn write_frame(send: &mut SendStream, ty: u8, payload: &[u8]) -> anyhow::Result<()> {
    let mut header = [0u8; 5];
    header[0] = ty;
    header[1..5].copy_from_slice(&(payload.len() as u32).to_be_bytes());
    send.write_all(&header).await?;
    send.write_all(payload).await?;
    Ok(())
}

/// Reads one frame from the Iroh recv stream; returns `None` on clean EOF.
pub async fn read_frame(recv: &mut RecvStream) -> anyhow::Result<Option<(u8, Vec<u8>)>> {
    let mut header = [0u8; 5];
    let mut read = 0;
    while read < 5 {
        match recv.read(&mut header[read..]).await? {
            Some(0) | None => {
                return if read == 0 {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("stream ended mid-header"))
                };
            }
            Some(n) => read += n,
        }
    }
    let ty = header[0];
    let len = u32::from_be_bytes([header[1], header[2], header[3], header[4]]) as usize;
    if len > MAX_FRAME_BYTES {
        anyhow::bail!("frame too large: {len} bytes");
    }
    let mut payload = vec![0u8; len];
    read = 0;
    while read < len {
        match recv.read(&mut payload[read..]).await? {
            Some(0) | None => anyhow::bail!("stream ended mid-payload"),
            Some(n) => read += n,
        }
    }
    Ok(Some((ty, payload)))
}
