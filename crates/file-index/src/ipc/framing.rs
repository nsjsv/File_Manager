use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use super::client::IndexClientError;

const MAX_FRAME_BYTES: usize = 32 * 1024 * 1024;

pub(crate) async fn read_frame<T>(
    stream: &mut (impl AsyncRead + Unpin),
) -> Result<T, IndexClientError>
where
    T: DeserializeOwned,
{
    let mut length_bytes = [0; 4];
    stream.read_exact(&mut length_bytes).await?;
    let length = u32::from_le_bytes(length_bytes) as usize;
    if length > MAX_FRAME_BYTES {
        return Err(IndexClientError::Protocol(format!(
            "index IPC frame exceeds {MAX_FRAME_BYTES} bytes"
        )));
    }
    let mut payload = vec![0; length];
    stream.read_exact(&mut payload).await?;
    bincode::deserialize(&payload).map_err(IndexClientError::from)
}

pub(crate) async fn write_frame<T>(
    stream: &mut (impl AsyncWrite + Unpin),
    value: &T,
) -> Result<(), IndexClientError>
where
    T: Serialize,
{
    let payload = bincode::serialize(value)?;
    if payload.len() > MAX_FRAME_BYTES {
        return Err(IndexClientError::Protocol(format!(
            "index IPC frame exceeds {MAX_FRAME_BYTES} bytes"
        )));
    }
    let length = u32::try_from(payload.len())
        .map_err(|_| IndexClientError::Protocol("index IPC frame is too large".to_owned()))?;
    stream.write_all(&length.to_le_bytes()).await?;
    stream.write_all(&payload).await?;
    stream.flush().await?;
    Ok(())
}
