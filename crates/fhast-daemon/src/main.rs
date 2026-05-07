use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context, Result};
use fhast_core::model::DownloadStatus;
use fhast_core::{
    paths, resolve_output_path, DownloadOutcome, DownloadRecord, HttpDownloader, Storage,
};
use fhast_ipc::{
    bind_daemon_socket, connect_to_daemon, read_message, split_stream, write_message, DownloadDto,
    EventDto, IpcRequest, IpcResponse, IpcStream, SegmentDto, IPC_VERSION,
};
use interprocess::local_socket::traits::tokio::Listener;
use tokio::io::BufReader;
use tokio::sync::{mpsc, Mutex};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use uuid::Uuid;

const DEFAULT_DOWNLOAD_CONNECTIONS: u16 = 8;

struct SpeedTracker {
    last_bytes: u64,
    last_time: Instant,
    speed: f64,
}

impl SpeedTracker {
    fn new() -> Self {
        Self {
            last_bytes: 0,
            last_time: Instant::now(),
            speed: 0.0,
        }
    }

    fn update(&mut self, bytes: u64) -> f64 {
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_time).as_secs_f64();
        if elapsed >= 0.5 && bytes > self.last_bytes {
            self.speed = (bytes - self.last_bytes) as f64 / elapsed;
        }
        self.last_bytes = bytes;
        self.last_time = now;
        self.speed
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_target(false).init();

    let socket_path = paths::socket_path().context("resolve daemon socket path")?;
    if connect_to_daemon(&socket_path).await.is_ok() {
        anyhow::bail!(
            "fhast-daemon is already running at {}",
            socket_path.display()
        );
    }
    remove_stale_socket(&socket_path).await?;

    let storage = Storage::open(paths::database_path().context("resolve database path")?)
        .context("open fhast database")?;
    storage
        .requeue_interrupted_downloads_async()
        .await
        .context("requeue interrupted downloads")?;

    let state = Arc::new(DaemonState {
        storage,
        downloader: HttpDownloader::default(),
        active_downloads: Mutex::new(HashMap::new()),
        speeds: Mutex::new(HashMap::new()),
    });

    let listener = bind_daemon_socket(&socket_path).context("bind daemon socket")?;
    info!(socket = %socket_path.display(), "fhast-daemon started");
    schedule_next_queued_download(state.clone());

    let (shutdown_tx, mut shutdown_rx) = mpsc::unbounded_channel::<()>();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let stream = accepted.context("accept daemon IPC connection")?;
                let state = state.clone();
                let shutdown_tx = shutdown_tx.clone();
                tokio::spawn(async move {
                    if let Err(error) = handle_client(state, stream, shutdown_tx).await {
                        warn!(%error, "IPC client handling failed");
                    }
                });
            }
            _ = shutdown_rx.recv() => {
                pause_active_downloads(state.clone()).await;
                break;
            }
        }
    }

    remove_stale_socket(&socket_path).await?;
    info!("fhast-daemon stopped");
    Ok(())
}

struct DaemonState {
    storage: Storage,
    downloader: HttpDownloader,
    active_downloads: Mutex<HashMap<String, CancellationToken>>,
    speeds: Mutex<HashMap<String, SpeedTracker>>,
}

async fn handle_client(
    state: Arc<DaemonState>,
    stream: IpcStream,
    shutdown_tx: mpsc::UnboundedSender<()>,
) -> Result<()> {
    let (reader, mut writer) = split_stream(stream);
    let mut reader = BufReader::new(reader);
    let response = match read_message::<IpcRequest, _>(&mut reader).await {
        Ok(request) => match handle_request(state, request, shutdown_tx).await {
            Ok(response) => response,
            Err(error) => IpcResponse::Error {
                version: IPC_VERSION,
                message: error.to_string(),
            },
        },
        Err(error) => IpcResponse::Error {
            version: IPC_VERSION,
            message: error.to_string(),
        },
    };

    write_message(&mut writer, &response).await?;
    Ok(())
}

async fn handle_request(
    state: Arc<DaemonState>,
    request: IpcRequest,
    shutdown_tx: mpsc::UnboundedSender<()>,
) -> Result<IpcResponse> {
    request.validate_version()?;

    match request {
        IpcRequest::AddDownload {
            url,
            output_path,
            working_dir,
            connections,
            checksum_sha256,
            checksum_md5,
            headers,
            sensitive_headers,
            ..
        } => {
            let output = resolve_output_path(&url, output_path.as_deref(), &working_dir)?;
            let id = Uuid::new_v4().to_string();
            let mut record =
                DownloadRecord::new(id.clone(), url, output.final_path, output.temp_path);
            record.requested_connections =
                connections.unwrap_or(DEFAULT_DOWNLOAD_CONNECTIONS).max(1);
            record.checksum_sha256 = checksum_sha256;
            record.checksum_md5 = checksum_md5;
            state.storage.insert_download_async(record.clone()).await?;

            if let Some(ref h) = headers {
                let sh_ref = sensitive_headers.as_deref().unwrap_or(&[]);
                state
                    .storage
                    .add_headers_batch(&id, h.clone(), sh_ref.to_vec())
                    .await?;
                info!(
                    download_id = %id,
                    normal = h.len(),
                    sensitive = sh_ref.len(),
                    "stored download headers"
                );
            } else if let Some(ref sh) = sensitive_headers {
                state
                    .storage
                    .add_headers_batch(&id, vec![], sh.clone())
                    .await?;
                info!(
                    download_id = %id,
                    sensitive = sh.len(),
                    "stored sensitive download headers"
                );
            }

            add_event(&state.storage, Some(&id), "info", "download queued").await;
            schedule_next_queued_download(state.clone());

            Ok(IpcResponse::DownloadAdded {
                version: IPC_VERSION,
                download: download_to_dto_with_speed(record, &state.speeds),
            })
        }
        IpcRequest::ListDownloads { .. } => {
            let downloads = state
                .storage
                .list_downloads_async()
                .await?
                .into_iter()
                .map(|d| download_to_dto_with_speed(d, &state.speeds))
                .collect();
            Ok(IpcResponse::DownloadList {
                version: IPC_VERSION,
                downloads,
            })
        }
        IpcRequest::DownloadInfo { id, .. } => {
            let download = state
                .storage
                .get_download_async(&id)
                .await?
                .ok_or_else(|| anyhow::anyhow!("download not found: {id}"))?;
            Ok(IpcResponse::DownloadInfo {
                version: IPC_VERSION,
                download: download_to_dto_with_speed(download, &state.speeds),
            })
        }
        IpcRequest::PauseDownload { id, .. } => {
            pause_download(state, &id).await?;
            Ok(IpcResponse::Ok {
                version: IPC_VERSION,
                message: format!("paused {id}"),
            })
        }
        IpcRequest::ResumeDownload { id, .. } => {
            resume_download(state.clone(), &id).await?;
            schedule_next_queued_download(state);
            Ok(IpcResponse::Ok {
                version: IPC_VERSION,
                message: format!("queued {id} for resume"),
            })
        }
        IpcRequest::RemoveDownload { id, .. } => {
            remove_download(state.clone(), &id).await?;
            Ok(IpcResponse::Ok {
                version: IPC_VERSION,
                message: format!("removed {id}"),
            })
        }
        IpcRequest::RetryDownload { id, .. } => {
            retry_download(state.clone(), &id).await?;
            schedule_next_queued_download(state);
            Ok(IpcResponse::Ok {
                version: IPC_VERSION,
                message: format!("queued {id} for retry"),
            })
        }
        IpcRequest::RecentEvents { limit, .. } => {
            let events = state
                .storage
                .recent_events_async(limit)
                .await?
                .into_iter()
                .map(|event| EventDto {
                    id: event.id,
                    download_id: event.download_id,
                    level: event.level,
                    message: event.message,
                    created_at: event.created_at,
                })
                .collect();
            Ok(IpcResponse::EventList {
                version: IPC_VERSION,
                events,
            })
        }
        IpcRequest::DownloadSegments { id, .. } => {
            let segments = state
                .storage
                .list_segments_async(&id)
                .await?
                .into_iter()
                .map(|segment| SegmentDto {
                    id: segment.id,
                    download_id: segment.download_id,
                    index: segment.index,
                    start_byte: segment.start_byte,
                    end_byte: segment.end_byte,
                    downloaded_bytes: segment.downloaded_bytes,
                    status: segment.status.as_str().to_owned(),
                    last_error: segment.last_error,
                })
                .collect();
            Ok(IpcResponse::SegmentList {
                version: IPC_VERSION,
                segments,
            })
        }
        IpcRequest::DownloadHeaders { id, .. } => {
            let headers = state.storage.get_headers_detail_async(&id).await?;
            Ok(IpcResponse::HeaderList {
                version: IPC_VERSION,
                headers,
            })
        }
        IpcRequest::StopDaemon { .. } => {
            shutdown_tx
                .send(())
                .context("send daemon shutdown signal")?;
            Ok(IpcResponse::Ok {
                version: IPC_VERSION,
                message: "daemon stopping".to_owned(),
            })
        }
    }
}

async fn start_next_queued_download(state: Arc<DaemonState>) {
    let mut active_downloads = state.active_downloads.lock().await;
    if !active_downloads.is_empty() {
        return;
    }

    let next_download = match state.storage.first_queued_download_async().await {
        Ok(download) => download,
        Err(error) => {
            error!(%error, "failed to query next queued download");
            return;
        }
    };

    let Some(download) = next_download else {
        return;
    };

    let id = download.id.clone();
    let cancellation = CancellationToken::new();
    active_downloads.insert(id.clone(), cancellation.clone());
    drop(active_downloads);

    tokio::spawn(async move {
        run_download(state, id, cancellation).await;
    });
}

fn schedule_next_queued_download(state: Arc<DaemonState>) {
    tokio::spawn(async move {
        start_next_queued_download(state).await;
    });
}

async fn run_download(state: Arc<DaemonState>, id: String, cancellation: CancellationToken) {
    add_event(&state.storage, Some(&id), "info", "download started").await;
    let speed_id = id.clone();
    let speed_state = state.clone();
    let speed_cancel = cancellation.clone();
    let speed_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_millis(500));
        loop {
            tokio::select! {
                _ = speed_cancel.cancelled() => break,
                _ = interval.tick() => {
                    if let Ok(Some(download)) = speed_state.storage.get_download_async(&speed_id).await {
                        let mut speeds = speed_state.speeds.lock().await;
                        speeds.entry(speed_id.clone())
                            .or_insert_with(SpeedTracker::new)
                            .update(download.downloaded_bytes);
                    }
                }
            }
        }
    });

    let custom_headers = state
        .storage
        .get_headers_async(&id)
        .await
        .ok()
        .unwrap_or_default();

    let downloader = if custom_headers.is_empty() {
        state.downloader.clone()
    } else {
        HttpDownloader::with_headers(&custom_headers)
    };

    let result = downloader.download(&state.storage, &id, cancellation).await;

    speed_task.abort();

    match result {
        Ok(DownloadOutcome::Completed) => {
            add_event(&state.storage, Some(&id), "info", "download completed").await;
        }
        Ok(DownloadOutcome::Paused) => {
            add_event(&state.storage, Some(&id), "info", "download paused").await;
        }
        Err(fhast_core::CoreError::NeedsRefresh) => {
            let message = "link expired or forbidden; capture or add the URL again";
            if let Err(error) = state
                .storage
                .set_status_async(&id, DownloadStatus::NeedsRefresh, Some(message.to_owned()))
                .await
            {
                error!(%error, download_id = %id, "failed to mark download needs_refresh");
            }
            add_event(&state.storage, Some(&id), "warn", message).await;
        }
        Err(error) => {
            let message = error.to_string();
            if let Err(storage_error) = state
                .storage
                .set_status_async(&id, DownloadStatus::Failed, Some(message.clone()))
                .await
            {
                error!(%storage_error, download_id = %id, "failed to mark download failed");
            }
            add_event(&state.storage, Some(&id), "error", &message).await;
        }
    }

    state.active_downloads.lock().await.remove(&id);
    state.speeds.lock().await.remove(&id);
    schedule_next_queued_download(state);
}

async fn pause_download(state: Arc<DaemonState>, id: &str) -> Result<()> {
    let download = state
        .storage
        .get_download_async(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("download not found: {id}"))?;

    if let Some(cancellation) = state.active_downloads.lock().await.get(id) {
        cancellation.cancel();
    } else if matches!(
        download.status,
        DownloadStatus::Queued | DownloadStatus::Failed
    ) {
        state
            .storage
            .set_status_async(id, DownloadStatus::Paused, None)
            .await?;
    }

    Ok(())
}

async fn resume_download(state: Arc<DaemonState>, id: &str) -> Result<()> {
    let download = state
        .storage
        .get_download_async(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("download not found: {id}"))?;

    if matches!(
        download.status,
        DownloadStatus::Paused | DownloadStatus::Failed | DownloadStatus::NeedsRefresh
    ) {
        state
            .storage
            .set_status_async(id, DownloadStatus::Queued, None)
            .await?;
    }

    Ok(())
}

async fn remove_download(state: Arc<DaemonState>, id: &str) -> Result<()> {
    if let Some(cancellation) = state.active_downloads.lock().await.remove(id) {
        cancellation.cancel();
    }

    let download = state
        .storage
        .get_download_async(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("download not found: {id}"))?;

    let _ = tokio::fs::remove_file(&download.final_path).await;
    let _ = tokio::fs::remove_file(&download.temp_path).await;
    if let Some(parent) = download.temp_path.parent() {
        let segment_dir = parent.join(format!(
            "{}.fhast-segments",
            download
                .temp_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        let _ = tokio::fs::remove_dir_all(&segment_dir).await;
    }

    state.storage.delete_download_async(id).await?;
    state.speeds.lock().await.remove(id);

    Ok(())
}

async fn retry_download(state: Arc<DaemonState>, id: &str) -> Result<()> {
    let download = state
        .storage
        .get_download_async(id)
        .await?
        .ok_or_else(|| anyhow::anyhow!("download not found: {id}"))?;

    state
        .storage
        .set_status_async(id, DownloadStatus::Queued, None)
        .await?;

    if let Some(temp_dir) = download.temp_path.parent() {
        let segment_dir = temp_dir.join(format!(
            "{}.fhast-segments",
            download
                .temp_path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
        ));
        let _ = tokio::fs::remove_dir_all(&segment_dir).await;
    }
    let _ = tokio::fs::remove_file(&download.temp_path).await;
    state.storage.delete_segments_async(id).await?;
    state.speeds.lock().await.remove(id);

    Ok(())
}

async fn pause_active_downloads(state: Arc<DaemonState>) {
    {
        let active_downloads = state.active_downloads.lock().await;
        for cancellation in active_downloads.values() {
            cancellation.cancel();
        }
    }

    for _ in 0..50 {
        if state.active_downloads.lock().await.is_empty() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    warn!("timed out waiting for active downloads to pause during shutdown");
}

async fn remove_stale_socket(path: &std::path::Path) -> Result<()> {
    match tokio::fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error).context("remove stale daemon socket"),
    }
}

fn download_to_dto_with_speed(
    download: DownloadRecord,
    speeds: &Mutex<HashMap<String, SpeedTracker>>,
) -> DownloadDto {
    DownloadDto {
        id: download.id.clone(),
        url: download.url,
        final_path: download.final_path.to_string_lossy().into_owned(),
        status: download.status.as_str().to_owned(),
        total_size: download.total_size,
        downloaded_bytes: download.downloaded_bytes,
        downloaded_bytes_per_second: speeds
            .try_lock()
            .ok()
            .and_then(|map| map.get(&download.id).map(|t| t.speed)),
        created_at: download.created_at,
        updated_at: download.updated_at,
        last_error: download.last_error,
        requested_connections: download.requested_connections,
        checksum_sha256: download.checksum_sha256,
        checksum_md5: download.checksum_md5,
        header_count: None,
    }
}

async fn add_event(storage: &Storage, download_id: Option<&str>, level: &str, message: &str) {
    if let Err(error) = storage
        .add_event_async(
            Uuid::new_v4().to_string(),
            download_id.map(ToOwned::to_owned),
            level.to_owned(),
            message.to_owned(),
        )
        .await
    {
        warn!(%error, "failed to write event");
    }
}
