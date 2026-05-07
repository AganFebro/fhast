use std::fmt;
use std::path::PathBuf;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use thiserror::Error;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DownloadStatus {
    Queued,
    Probing,
    Downloading,
    Paused,
    Merging,
    Verifying,
    Completed,
    Failed,
    NeedsRefresh,
    Cancelled,
}

impl DownloadStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Probing => "probing",
            Self::Downloading => "downloading",
            Self::Paused => "paused",
            Self::Merging => "merging",
            Self::Verifying => "verifying",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::NeedsRefresh => "needs_refresh",
            Self::Cancelled => "cancelled",
        }
    }
}

impl fmt::Display for DownloadStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for DownloadStatus {
    type Err = ParseDownloadStatusError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "probing" => Ok(Self::Probing),
            "downloading" => Ok(Self::Downloading),
            "paused" => Ok(Self::Paused),
            "merging" => Ok(Self::Merging),
            "verifying" => Ok(Self::Verifying),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            "needs_refresh" => Ok(Self::NeedsRefresh),
            "cancelled" => Ok(Self::Cancelled),
            _ => Err(ParseDownloadStatusError(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Error)]
#[error("unknown download status `{0}`")]
pub struct ParseDownloadStatusError(String);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SegmentStatus {
    Queued,
    Downloading,
    Completed,
    Failed,
}

impl SegmentStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Downloading => "downloading",
            Self::Completed => "completed",
            Self::Failed => "failed",
        }
    }
}

impl fmt::Display for SegmentStatus {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.as_str())
    }
}

impl FromStr for SegmentStatus {
    type Err = ParseSegmentStatusError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "queued" => Ok(Self::Queued),
            "downloading" => Ok(Self::Downloading),
            "completed" => Ok(Self::Completed),
            "failed" => Ok(Self::Failed),
            _ => Err(ParseSegmentStatusError(value.to_owned())),
        }
    }
}

#[derive(Debug, Clone, Error)]
#[error("unknown segment status `{0}`")]
pub struct ParseSegmentStatusError(String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentRecord {
    pub id: String,
    pub download_id: String,
    pub index: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
    pub status: SegmentStatus,
    pub last_error: Option<String>,
}

impl SegmentRecord {
    pub fn expected_len(&self) -> u64 {
        self.end_byte - self.start_byte + 1
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadRecord {
    pub id: String,
    pub url: String,
    pub final_path: PathBuf,
    pub temp_path: PathBuf,
    pub status: DownloadStatus,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub etag: Option<String>,
    pub last_modified: Option<String>,
    pub checksum_sha256: Option<String>,
    pub checksum_md5: Option<String>,
    pub requested_connections: u16,
    pub created_at: String,
    pub updated_at: String,
    pub last_error: Option<String>,
}

impl DownloadRecord {
    pub fn new(id: String, url: String, final_path: PathBuf, temp_path: PathBuf) -> Self {
        let now = unix_timestamp_string();

        Self {
            id,
            url,
            final_path,
            temp_path,
            status: DownloadStatus::Queued,
            total_size: None,
            downloaded_bytes: 0,
            etag: None,
            last_modified: None,
            checksum_sha256: None,
            checksum_md5: None,
            requested_connections: 1,
            created_at: now.clone(),
            updated_at: now,
            last_error: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventRecord {
    pub id: String,
    pub download_id: Option<String>,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

pub fn unix_timestamp_string() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
        .to_string()
}
