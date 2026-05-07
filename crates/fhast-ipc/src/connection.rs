use std::path::PathBuf;

use anyhow::Context;
use interprocess::local_socket::traits::tokio::Stream;
use interprocess::local_socket::{self as ls, ListenerOptions, Name, ToFsName};
use tokio::io::{AsyncBufRead, AsyncWrite};

use crate::error::IpcError;

pub type IpcStream = interprocess::local_socket::tokio::Stream;
pub type IpcListener = interprocess::local_socket::tokio::Listener;

fn create_name(path: &PathBuf) -> Result<Name<'static>, IpcError> {
    let path = std::path::absolute(path)
        .map_err(|e| IpcError::ConnectionFailed(format!("resolve absolute socket path: {e}")))?;

    #[cfg(not(windows))]
    {
        path.to_fs_name::<ls::GenericFilePath>()
            .map_err(|e| IpcError::ConnectionFailed(format!("invalid socket path: {e}")))
    }
    #[cfg(windows)]
    {
        let name_str = path.to_string_lossy().replace('\\', "/");
        use interprocess::local_socket::ToNsName;
        name_str
            .to_ns_name::<ls::GenericNamespaced>()
            .map_err(|e| IpcError::ConnectionFailed(format!("invalid named pipe name: {e}")))
    }
}

pub async fn connect_to_daemon(
    socket_path: impl AsRef<std::path::Path>,
) -> anyhow::Result<IpcStream> {
    let path = socket_path.as_ref().to_path_buf();
    let name = create_name(&path).context("create socket name")?;
    let stream = IpcStream::connect(name)
        .await
        .with_context(|| format!("connect to daemon socket {}", path.display()))?;
    Ok(stream)
}

pub fn bind_daemon_socket(socket_path: impl AsRef<std::path::Path>) -> anyhow::Result<IpcListener> {
    let path = socket_path.as_ref().to_path_buf();

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("create socket parent directory {}", parent.display()))?;
    }

    let name = create_name(&path).context("create socket name")?;

    let opts = ListenerOptions::new().name(name);
    let listener = opts
        .create_tokio()
        .map_err(|e| anyhow::anyhow!("bind daemon socket {}: {e}", path.display()))?;
    Ok(listener)
}

/// Split a stream into split read/write halves compatible with read_message/write_message.
pub fn split_stream(stream: IpcStream) -> (impl AsyncBufRead, impl AsyncWrite) {
    let (read, write) = tokio::io::split(stream);
    (tokio::io::BufReader::new(read), write)
}
