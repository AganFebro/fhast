use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::{anyhow, Result};
use fhast_ipc::{
    connect_to_daemon, read_message, split_stream, write_message, DownloadDto, EventDto,
    IpcRequest, IpcResponse, SegmentDto, IPC_VERSION,
};
use tokio::io::BufReader;

const AUTO_GRAB_RECENT_SECONDS: u64 = 15 * 60;

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub downloads: Vec<DownloadDto>,
    pub events: Vec<EventDto>,
    pub auto_grab: AutoGrabStatus,
}

#[derive(Clone, Debug, Default)]
pub struct AutoGrabStatus {
    pub recent_native_capture: bool,
    pub last_capture_at: Option<String>,
}

#[derive(Clone, Debug)]
pub struct DetailData {
    pub download: DownloadDto,
    pub segments: Vec<SegmentDto>,
    pub headers: Vec<(String, String, bool)>,
}

#[derive(Clone, Debug)]
pub struct AddDownloadCommand {
    pub url: String,
    pub output_path: Option<PathBuf>,
    pub working_dir: PathBuf,
    pub connections: Option<u16>,
    pub checksum_sha256: Option<String>,
    pub checksum_md5: Option<String>,
}

pub async fn send_request(socket_path: &Path, request: IpcRequest) -> Result<IpcResponse> {
    let stream = connect_to_daemon(socket_path).await?;
    let (reader, mut writer) = split_stream(stream);
    let mut reader = BufReader::new(reader);

    write_message(&mut writer, &request).await?;
    let response = read_message(&mut reader).await?;
    Ok(response)
}

pub async fn load_snapshot(socket_path: PathBuf) -> Result<Snapshot> {
    let downloads = match send_request(
        &socket_path,
        IpcRequest::ListDownloads {
            version: IPC_VERSION,
        },
    )
    .await?
    {
        IpcResponse::DownloadList { downloads, .. } => downloads,
        IpcResponse::Error { message, .. } => return Err(anyhow!(message)),
        other => return Err(anyhow!("unexpected daemon response: {other:?}")),
    };

    let events = match send_request(
        &socket_path,
        IpcRequest::RecentEvents {
            version: IPC_VERSION,
            limit: 50,
        },
    )
    .await?
    {
        IpcResponse::EventList { events, .. } => events,
        IpcResponse::Error { message, .. } => return Err(anyhow!(message)),
        other => return Err(anyhow!("unexpected daemon response: {other:?}")),
    };

    let auto_grab = detect_auto_grab_status(&socket_path, &events).await;
    Ok(Snapshot {
        downloads,
        events,
        auto_grab,
    })
}

pub async fn load_detail(socket_path: PathBuf, id: String) -> Result<DetailData> {
    let download = match send_request(
        &socket_path,
        IpcRequest::DownloadInfo {
            version: IPC_VERSION,
            id: id.clone(),
        },
    )
    .await?
    {
        IpcResponse::DownloadInfo { download, .. } => download,
        IpcResponse::Error { message, .. } => return Err(anyhow!(message)),
        other => return Err(anyhow!("unexpected daemon response: {other:?}")),
    };

    let segments = match send_request(
        &socket_path,
        IpcRequest::DownloadSegments {
            version: IPC_VERSION,
            id: id.clone(),
        },
    )
    .await?
    {
        IpcResponse::SegmentList { segments, .. } => segments,
        IpcResponse::Error { .. } => Vec::new(),
        _ => Vec::new(),
    };

    let headers = match send_request(
        &socket_path,
        IpcRequest::DownloadHeaders {
            version: IPC_VERSION,
            id,
        },
    )
    .await?
    {
        IpcResponse::HeaderList { headers, .. } => headers,
        IpcResponse::Error { .. } => Vec::new(),
        _ => Vec::new(),
    };

    Ok(DetailData {
        download,
        segments,
        headers,
    })
}

pub async fn add_download(socket_path: PathBuf, command: AddDownloadCommand) -> Result<String> {
    let response = send_request(
        &socket_path,
        IpcRequest::AddDownload {
            version: IPC_VERSION,
            url: command.url,
            output_path: command.output_path,
            working_dir: command.working_dir,
            connections: command.connections,
            checksum_sha256: command.checksum_sha256,
            checksum_md5: command.checksum_md5,
            headers: None,
            sensitive_headers: None,
        },
    )
    .await?;

    match response {
        IpcResponse::DownloadAdded { download, .. } => {
            Ok(format!("queued {} -> {}", download.id, download.final_path))
        }
        IpcResponse::Error { message, .. } => Err(anyhow!(message)),
        other => Err(anyhow!("unexpected daemon response: {other:?}")),
    }
}

pub async fn control_download(socket_path: PathBuf, request: IpcRequest) -> Result<String> {
    let response = send_request(&socket_path, request).await?;
    match response {
        IpcResponse::Ok { message, .. } => Ok(message),
        IpcResponse::Error { message, .. } => Err(anyhow!(message)),
        other => Err(anyhow!("unexpected daemon response: {other:?}")),
    }
}

async fn detect_auto_grab_status(socket_path: &Path, events: &[EventDto]) -> AutoGrabStatus {
    let now = unix_now();
    let mut seen = HashSet::new();

    for event in events {
        if event.message != "download queued" {
            continue;
        }

        let Some(download_id) = event.download_id.as_ref() else {
            continue;
        };

        if !seen.insert(download_id.clone()) {
            continue;
        }

        let Some(created_at) = event.created_at.parse::<u64>().ok() else {
            continue;
        };

        if now.saturating_sub(created_at) > AUTO_GRAB_RECENT_SECONDS {
            continue;
        }

        let headers = send_request(
            socket_path,
            IpcRequest::DownloadHeaders {
                version: IPC_VERSION,
                id: download_id.clone(),
            },
        )
        .await;

        let Ok(IpcResponse::HeaderList { headers, .. }) = headers else {
            continue;
        };

        if headers.iter().any(|(name, value, _)| {
            name.eq_ignore_ascii_case("x-fhast-origin") && value == "native-host"
        }) {
            return AutoGrabStatus {
                recent_native_capture: true,
                last_capture_at: Some(event.created_at.clone()),
            };
        }
    }

    AutoGrabStatus::default()
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
