use serde::de::DeserializeOwned;
use serde::Serialize;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt};

use crate::error::IpcError;

pub async fn read_message<T, R>(reader: &mut R) -> Result<T, IpcError>
where
    T: DeserializeOwned,
    R: AsyncBufRead + Unpin,
{
    let mut line = String::new();
    let bytes_read = reader.read_line(&mut line).await?;
    if bytes_read == 0 {
        return Err(IpcError::ConnectionClosed);
    }

    Ok(serde_json::from_str(line.trim_end())?)
}

pub async fn write_message<T, W>(writer: &mut W, message: &T) -> Result<(), IpcError>
where
    T: Serialize,
    W: AsyncWrite + Unpin,
{
    let mut payload = serde_json::to_vec(message)?;
    payload.push(b'\n');
    writer.write_all(&payload).await?;
    writer.flush().await?;

    Ok(())
}
