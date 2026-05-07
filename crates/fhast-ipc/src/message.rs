use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::error::IpcError;

pub const IPC_VERSION: u16 = 1;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcRequest {
    AddDownload {
        version: u16,
        url: String,
        output_path: Option<PathBuf>,
        working_dir: PathBuf,
        connections: Option<u16>,
        checksum_sha256: Option<String>,
        checksum_md5: Option<String>,
        headers: Option<Vec<(String, String)>>,
        sensitive_headers: Option<Vec<(String, String)>>,
    },
    ListDownloads {
        version: u16,
    },
    DownloadInfo {
        version: u16,
        id: String,
    },
    DownloadSegments {
        version: u16,
        id: String,
    },
    DownloadHeaders {
        version: u16,
        id: String,
    },
    PauseDownload {
        version: u16,
        id: String,
    },
    ResumeDownload {
        version: u16,
        id: String,
    },
    RemoveDownload {
        version: u16,
        id: String,
    },
    RetryDownload {
        version: u16,
        id: String,
    },
    RecentEvents {
        version: u16,
        limit: u32,
    },
    StopDaemon {
        version: u16,
    },
}

impl IpcRequest {
    pub fn version(&self) -> u16 {
        match self {
            Self::AddDownload { version, .. }
            | Self::ListDownloads { version }
            | Self::DownloadInfo { version, .. }
            | Self::DownloadSegments { version, .. }
            | Self::DownloadHeaders { version, .. }
            | Self::PauseDownload { version, .. }
            | Self::ResumeDownload { version, .. }
            | Self::RemoveDownload { version, .. }
            | Self::RetryDownload { version, .. }
            | Self::RecentEvents { version, .. }
            | Self::StopDaemon { version } => *version,
        }
    }

    pub fn validate_version(&self) -> Result<(), IpcError> {
        let actual = self.version();
        if actual == IPC_VERSION {
            Ok(())
        } else {
            Err(IpcError::UnsupportedVersion {
                expected: IPC_VERSION,
                actual,
            })
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadDto {
    pub id: String,
    pub url: String,
    pub final_path: String,
    pub status: String,
    pub total_size: Option<u64>,
    pub downloaded_bytes: u64,
    pub downloaded_bytes_per_second: Option<f64>,
    pub created_at: String,
    pub updated_at: String,
    pub last_error: Option<String>,
    pub requested_connections: u16,
    pub checksum_sha256: Option<String>,
    pub checksum_md5: Option<String>,
    pub header_count: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventDto {
    pub id: String,
    pub download_id: Option<String>,
    pub level: String,
    pub message: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SegmentDto {
    pub id: String,
    pub download_id: String,
    pub index: u32,
    pub start_byte: u64,
    pub end_byte: u64,
    pub downloaded_bytes: u64,
    pub status: String,
    pub last_error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum IpcResponse {
    DownloadAdded {
        version: u16,
        download: DownloadDto,
    },
    DownloadList {
        version: u16,
        downloads: Vec<DownloadDto>,
    },
    DownloadInfo {
        version: u16,
        download: DownloadDto,
    },
    SegmentList {
        version: u16,
        segments: Vec<SegmentDto>,
    },
    EventList {
        version: u16,
        events: Vec<EventDto>,
    },
    HeaderList {
        version: u16,
        headers: Vec<(String, String, bool)>,
    },
    Ok {
        version: u16,
        message: String,
    },
    Error {
        version: u16,
        message: String,
    },
}
