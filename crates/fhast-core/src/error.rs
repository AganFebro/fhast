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
    #[error("disk full")]
    DiskFull,
    #[error("permission denied")]
    PermissionDenied,
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

impl CoreError {
    pub fn is_transient(&self) -> bool {
        match self {
            Self::Http(reqwest_error) => is_transient_reqwest(reqwest_error),
            Self::Io(io_error) => is_transient_io(io_error),
            Self::HttpStatus(status) => is_transient_status(*status),
            Self::DiskFull | Self::PermissionDenied => false,
            Self::NeedsRefresh | Self::SegmentRangeRejected | Self::ResumeRejected => false,
            Self::Paused => false,
            _ => false,
        }
    }
}

fn is_transient_reqwest(err: &reqwest::Error) -> bool {
    if err.is_timeout() || err.is_connect() || err.is_request() {
        return true;
    }
    if let Some(source) = std::error::Error::source(err) {
        if let Some(io) = source.downcast_ref::<std::io::Error>() {
            return is_transient_io(io);
        }
    }
    false
}

fn is_transient_io(err: &std::io::Error) -> bool {
    use std::io::ErrorKind;
    matches!(
        err.kind(),
        ErrorKind::TimedOut
            | ErrorKind::ConnectionReset
            | ErrorKind::ConnectionRefused
            | ErrorKind::ConnectionAborted
            | ErrorKind::UnexpectedEof
    )
}

fn is_transient_status(status: StatusCode) -> bool {
    status.is_server_error() || status == StatusCode::TOO_MANY_REQUESTS
}

const INITIAL_BACKOFF_MS: u64 = 500;
const MAX_BACKOFF_MS: u64 = 30_000;

pub fn retry_delay(attempt: u8, base: Option<u64>) -> std::time::Duration {
    let base = base.unwrap_or(INITIAL_BACKOFF_MS);
    let delay = base * 2u64.saturating_pow(u32::from(attempt.saturating_sub(1)));
    std::time::Duration::from_millis(delay.min(MAX_BACKOFF_MS))
}

pub fn parse_retry_after(headers: &reqwest::header::HeaderMap) -> Option<std::time::Duration> {
    use reqwest::header::RETRY_AFTER;
    let value = headers.get(RETRY_AFTER)?.to_str().ok()?;
    if let Ok(secs) = value.parse::<u64>() {
        return Some(std::time::Duration::from_secs(secs));
    }
    None
}

pub fn classify_io_error(err: &std::io::Error) -> CoreError {
    use std::io::ErrorKind;
    match err.kind() {
        ErrorKind::PermissionDenied => CoreError::PermissionDenied,
        _ => {
            let raw = err.raw_os_error();
            if is_disk_full_error(raw) {
                CoreError::DiskFull
            } else {
                CoreError::Io(err.kind().into())
            }
        }
    }
}

fn is_disk_full_error(raw: Option<i32>) -> bool {
    #[cfg(target_os = "linux")]
    {
        raw == Some(28) || raw == Some(122)
    }
    #[cfg(target_os = "macos")]
    {
        raw == Some(28) || raw == Some(69)
    }
    #[cfg(windows)]
    {
        raw == Some(112) || raw == Some(39)
    }
    #[cfg(not(any(target_os = "linux", target_os = "macos", windows)))]
    {
        false
    }
}
