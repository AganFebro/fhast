use thiserror::Error;

#[derive(Debug, Error)]
pub enum IpcError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("connection closed before a complete IPC message was received")]
    ConnectionClosed,
    #[error("unsupported IPC version {actual}; expected {expected}")]
    UnsupportedVersion { expected: u16, actual: u16 },
    #[error("connection failed: {0}")]
    ConnectionFailed(String),
}
