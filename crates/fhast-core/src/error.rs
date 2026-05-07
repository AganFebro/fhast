use std::path::PathBuf;

use reqwest::StatusCode;
use thiserror::Error;

use crate::model::ParseDownloadStatusError;
use crate::storage::StorageError;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("download not found: {0}")]
    DownloadNotFound(String),
    #[error("invalid URL: {0}")]
    InvalidUrl(String),
    #[error("unsafe output path: {0}")]
    UnsafeOutputPath(String),
    #[error("output file already exists: {}", .0.display())]
    OutputExists(PathBuf),
    #[error("HTTP request failed: {0}")]
    Http(#[from] reqwest::Error),
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("server returned HTTP {0}")]
    HttpStatus(StatusCode),
    #[error("remote file changed; capture or add the URL again")]
    NeedsRefresh,
    #[error("server did not honor resume range request")]
    ResumeRejected,
    #[error("server did not honor segment range request")]
    SegmentRangeRejected,
    #[error("checksum mismatch for {algorithm}: expected {expected}, got {actual}")]
    ChecksumMismatch {
        algorithm: &'static str,
        expected: String,
        actual: String,
    },
    #[error("download was paused")]
    Paused,
    #[error("invalid download status: {0}")]
    InvalidStatus(#[from] ParseDownloadStatusError),
    #[error("storage error: {0}")]
    Storage(#[from] StorageError),
}
