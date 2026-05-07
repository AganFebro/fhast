use std::path::{Path, PathBuf};

use futures_util::stream::FuturesUnordered;
use futures_util::StreamExt;
use reqwest::header::{CONTENT_LENGTH, CONTENT_RANGE, ETAG, IF_RANGE, LAST_MODIFIED, RANGE};
use reqwest::{Client, StatusCode};
use tokio::fs::{self, File, OpenOptions};
use tokio::io::{AsyncWrite, AsyncWriteExt};
use tokio_util::sync::CancellationToken;

use crate::checksum::{Md5Checksum, Sha256Checksum};
use crate::config::{FhastConfig, SensitiveHeaderRetention};
use crate::error::CoreError;
use crate::model::{DownloadRecord, DownloadStatus, SegmentRecord, SegmentStatus};
use crate::segment::{effective_segment_connections, plan_fixed_segments};
use crate::storage::Storage;

const PROGRESS_FLUSH_BYTES: u64 = 1024 * 1024;
const SEGMENT_RETRY_ATTEMPTS: u8 = 3;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DownloadOutcome {
    Completed,
    Paused,
}

#[derive(Debug, Clone)]
pub struct HttpDownloader {
    client: Client,
    config: FhastConfig,
}

impl Default for HttpDownloader {
    fn default() -> Self {
        Self {
            client: Client::new(),
            config: FhastConfig::default(),
        }
    }
}

impl HttpDownloader {
    pub fn new(config: FhastConfig) -> Self {
        Self {
            client: Client::new(),
            config,
        }
    }

    pub fn with_headers(headers: &[(String, String)]) -> Self {
        let mut header_map = reqwest::header::HeaderMap::new();
        for (name, value) in headers {
            let header_name = match reqwest::header::HeaderName::from_bytes(name.as_bytes()) {
                Ok(n) => n,
                Err(_) => {
                    tracing::warn!(%name, "skipping invalid header name");
                    continue;
                }
            };
            let header_value = match reqwest::header::HeaderValue::from_bytes(value.as_bytes()) {
                Ok(v) => v,
                Err(_) => {
                    tracing::warn!(%name, "skipping invalid header value (len={})", value.len());
                    continue;
                }
            };
            header_map.insert(header_name, header_value);
        }
        Self {
            client: Client::builder()
                .default_headers(header_map)
                .build()
                .unwrap_or_default(),
            config: FhastConfig::default(),
        }
    }
}

impl HttpDownloader {
    pub async fn download(
        &self,
        storage: &Storage,
        id: &str,
        cancellation: CancellationToken,
    ) -> Result<DownloadOutcome, CoreError> {
        let record = storage
            .get_download_async(id)
            .await?
            .ok_or_else(|| CoreError::DownloadNotFound(id.to_owned()))?;

        storage
            .set_status_async(id, DownloadStatus::Probing, None)
            .await?;
        let probe = self.probe(&record.url).await?;
        ensure_resume_valid(&record.etag, &record.last_modified, &probe)?;
        storage
            .set_probe_info_async(
                id,
                probe.total_size,
                probe.etag.clone(),
                probe.last_modified.clone(),
            )
            .await?;

        let segmented = record.requested_connections > 1
            && probe.total_size.is_some()
            && self.range_probe(&record.url).await?.is_some();

        if segmented {
            match self
                .download_segmented(storage, record.clone(), probe.clone(), cancellation.clone())
                .await
            {
                Ok(outcome) => return Ok(outcome),
                Err(CoreError::SegmentRangeRejected) => {
                    storage
                        .replace_segments_async(&record.id, Vec::new())
                        .await?;
                    remove_path_if_exists(segment_dir(&record)).await?;
                }
                Err(error) => return Err(error),
            }
        }

        self.download_single(storage, record, probe, cancellation)
            .await
    }

    async fn download_single(
        &self,
        storage: &Storage,
        record: DownloadRecord,
        probe: ProbeResult,
        cancellation: CancellationToken,
    ) -> Result<DownloadOutcome, CoreError> {
        let id = record.id.as_str();
        let max_retries = self.config.max_retries;
        let mut attempt = 0u8;

        loop {
            attempt += 1;
            match self
                .download_single_attempt(
                    storage,
                    record.clone(),
                    probe.clone(),
                    cancellation.clone(),
                )
                .await
            {
                Ok(outcome) => return Ok(outcome),
                Err(error) if error.is_transient() && attempt < max_retries => {
                    let delay = crate::error::retry_delay(attempt, None);
                    tracing::warn!(
                        download_id = id,
                        attempt,
                        max_retries,
                        delay_ms = delay.as_millis(),
                        %error,
                        "transient error, retrying",
                    );
                    storage
                        .set_status_async(
                            id,
                            DownloadStatus::Queued,
                            Some(format!("retrying after transient error (attempt {attempt}/{max_retries}): {error}")),
                        )
                        .await?;
                    tokio::time::sleep(delay).await;
                }
                Err(error) => return Err(error),
            }
        }
    }

    async fn download_single_attempt(
        &self,
        storage: &Storage,
        record: DownloadRecord,
        probe: ProbeResult,
        cancellation: CancellationToken,
    ) -> Result<DownloadOutcome, CoreError> {
        let id = record.id.as_str();
        let mut resume_offset = temp_file_len(&record.temp_path).await?;
        let mut request = self.client.get(&record.url);
        if resume_offset > 0 {
            request = request.header(RANGE, format!("bytes={resume_offset}-"));

            if let Some(validator) = record.etag.as_ref().or(record.last_modified.as_ref()) {
                request = request.header(IF_RANGE, validator);
            }
        }

        let response = request.send().await?;
        let status = response.status();
        if matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN) {
            return Err(CoreError::NeedsRefresh);
        }
        if matches!(status, StatusCode::TOO_MANY_REQUESTS) {
            if let Some(delay) = crate::error::parse_retry_after(response.headers()) {
                tokio::time::sleep(delay).await;
            }
            return Err(CoreError::HttpStatus(status));
        }
        if status.is_server_error() {
            return Err(CoreError::HttpStatus(status));
        }

        let append = if resume_offset > 0 {
            match status {
                StatusCode::PARTIAL_CONTENT => true,
                StatusCode::OK => {
                    remove_file_if_exists(&record.temp_path).await?;
                    resume_offset = 0;
                    false
                }
                _ => return Err(CoreError::HttpStatus(status)),
            }
        } else if status.is_success() {
            false
        } else {
            return Err(CoreError::HttpStatus(status));
        };

        let headers = response.headers().clone();
        let total_size = parse_content_range_total(&headers)
            .or(probe.total_size)
            .or_else(|| {
                response
                    .content_length()
                    .map(|length| length + resume_offset)
            });
        storage
            .set_probe_info_async(
                id,
                total_size,
                header_to_string(&headers, ETAG),
                header_to_string(&headers, LAST_MODIFIED),
            )
            .await?;

        ensure_parent_dir(&record.final_path).await?;
        let mut file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(&record.temp_path)
            .await?;
        let mut downloaded_bytes = resume_offset;
        let mut last_persisted_bytes = downloaded_bytes;
        storage
            .set_status_async(id, DownloadStatus::Downloading, None)
            .await?;
        storage.set_progress_async(id, downloaded_bytes).await?;

        let mut stream = response.bytes_stream();
        loop {
            tokio::select! {
                () = cancellation.cancelled() => {
                    file.flush().await?;
                    storage.set_progress_async(id, downloaded_bytes).await?;
                    storage.set_status_async(id, DownloadStatus::Paused, None).await?;
                    return Ok(DownloadOutcome::Paused);
                }
                maybe_chunk = stream.next() => {
                    match maybe_chunk {
                        Some(chunk) => {
                            let chunk = chunk?;
                            file.write_all(&chunk).await?;
                            downloaded_bytes += chunk.len() as u64;
                            if downloaded_bytes.saturating_sub(last_persisted_bytes) >= PROGRESS_FLUSH_BYTES {
                                storage.set_progress_async(id, downloaded_bytes).await?;
                                last_persisted_bytes = downloaded_bytes;
                            }
                        }
                        None => break,
                    }
                }
            }
        }

        file.flush().await?;
        drop(file);

        finalize_download(storage, &record, downloaded_bytes, total_size, self.config).await
    }

    async fn download_segmented(
        &self,
        storage: &Storage,
        record: DownloadRecord,
        probe: ProbeResult,
        cancellation: CancellationToken,
    ) -> Result<DownloadOutcome, CoreError> {
        let id = record.id.as_str();
        let total_size = probe.total_size.ok_or(CoreError::SegmentRangeRejected)?;
        let connections = effective_segment_connections(
            record.requested_connections,
            &record.url,
            self.config.max_connections_global,
            self.config.max_connections_per_host,
        );
        let segments = plan_fixed_segments(id, total_size, connections)
            .map_err(|_| CoreError::SegmentRangeRejected)?;
        if segments.len() <= 1 {
            return Err(CoreError::SegmentRangeRejected);
        }

        let part_dir = segment_dir(&record);
        remove_path_if_exists(&part_dir).await?;
        fs::create_dir_all(&part_dir).await?;
        storage.replace_segments_async(id, segments.clone()).await?;
        storage.set_progress_async(id, 0).await?;
        storage
            .set_status_async(id, DownloadStatus::Downloading, None)
            .await?;

        let worker_cancellation = cancellation.child_token();

        let mut workers = FuturesUnordered::new();
        for segment in segments.clone() {
            workers.push(download_segment_worker(
                self.client.clone(),
                storage.clone(),
                record.url.clone(),
                part_path(&part_dir, segment.index),
                segment,
                worker_cancellation.clone(),
                SEGMENT_RETRY_ATTEMPTS,
            ));
        }

        let mut total_downloaded = 0;
        while let Some(result) = workers.next().await {
            match result {
                Ok(downloaded) => {
                    total_downloaded += downloaded;
                    storage.set_progress_async(id, total_downloaded).await?;
                }
                Err(CoreError::Paused) => {
                    worker_cancellation.cancel();
                    storage
                        .set_status_async(id, DownloadStatus::Paused, None)
                        .await?;
                    return Ok(DownloadOutcome::Paused);
                }
                Err(error) => {
                    worker_cancellation.cancel();
                    return Err(error);
                }
            }
        }

        storage
            .set_status_async(id, DownloadStatus::Merging, None)
            .await?;
        merge_parts(&part_dir, &record.temp_path, &segments).await?;
        remove_path_if_exists(&part_dir).await?;
        finalize_download(storage, &record, total_size, Some(total_size), self.config).await
    }

    async fn probe(&self, url: &str) -> Result<ProbeResult, CoreError> {
        let response = match self.client.head(url).send().await {
            Ok(response) => response,
            Err(_) => return Ok(ProbeResult::default()),
        };

        if matches!(
            response.status(),
            StatusCode::METHOD_NOT_ALLOWED | StatusCode::NOT_IMPLEMENTED
        ) {
            return Ok(ProbeResult::default());
        }
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err(CoreError::NeedsRefresh);
        }
        if !response.status().is_success() {
            return Ok(ProbeResult::default());
        }

        let headers = response.headers();
        Ok(ProbeResult {
            total_size: header_to_string(headers, CONTENT_LENGTH)
                .and_then(|value| value.parse().ok()),
            etag: header_to_string(headers, ETAG),
            last_modified: header_to_string(headers, LAST_MODIFIED),
        })
    }

    async fn range_probe(&self, url: &str) -> Result<Option<u64>, CoreError> {
        let response = self
            .client
            .get(url)
            .header(RANGE, "bytes=0-0")
            .send()
            .await?;
        if matches!(
            response.status(),
            StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN
        ) {
            return Err(CoreError::NeedsRefresh);
        }
        if response.status() != StatusCode::PARTIAL_CONTENT {
            return Ok(None);
        }

        Ok(parse_content_range_total(response.headers()))
    }
}

#[derive(Debug, Clone, Default)]
struct ProbeResult {
    total_size: Option<u64>,
    etag: Option<String>,
    last_modified: Option<String>,
}

async fn download_segment_worker(
    client: Client,
    storage: Storage,
    url: String,
    path: PathBuf,
    segment: SegmentRecord,
    cancellation: CancellationToken,
    retry_attempts: u8,
) -> Result<u64, CoreError> {
    for attempt in 1..=retry_attempts {
        match download_segment_attempt(
            client.clone(),
            storage.clone(),
            url.clone(),
            path.clone(),
            segment.clone(),
            cancellation.clone(),
        )
        .await
        {
            Ok(downloaded) => return Ok(downloaded),
            Err(
                err @ (CoreError::Paused
                | CoreError::SegmentRangeRejected
                | CoreError::NeedsRefresh),
            ) => {
                return Err(err);
            }
            Err(error) if !error.is_transient() => return Err(error),
            Err(error) if attempt == retry_attempts => return Err(error),
            Err(error) => {
                storage
                    .set_segment_status_async(
                        &segment.id,
                        SegmentStatus::Queued,
                        temp_file_len(&path).await.unwrap_or(0),
                        Some(format!("retrying after segment error: {error}")),
                    )
                    .await?;
                tokio::time::sleep(segment_retry_delay(attempt)).await;
            }
        }
    }

    Err(CoreError::SegmentRangeRejected)
}

async fn download_segment_attempt(
    client: Client,
    storage: Storage,
    url: String,
    path: PathBuf,
    segment: SegmentRecord,
    cancellation: CancellationToken,
) -> Result<u64, CoreError> {
    let mut start_byte = segment.start_byte;
    let existing_len = temp_file_len(&path).await?;
    if existing_len > segment.expected_len() {
        remove_file_if_exists(&path).await?;
    } else {
        start_byte += existing_len;
    }

    if start_byte > segment.end_byte {
        storage
            .set_segment_status_async(
                &segment.id,
                SegmentStatus::Completed,
                segment.expected_len(),
                None,
            )
            .await?;
        return Ok(segment.expected_len());
    }

    storage
        .set_segment_status_async(&segment.id, SegmentStatus::Downloading, existing_len, None)
        .await?;

    let response = client
        .get(url)
        .header(RANGE, format!("bytes={start_byte}-{}", segment.end_byte))
        .send()
        .await?;
    match response.status() {
        StatusCode::PARTIAL_CONTENT => {}
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => {
            return Err(CoreError::NeedsRefresh);
        }
        StatusCode::OK | StatusCode::RANGE_NOT_SATISFIABLE => {
            storage
                .set_segment_status_async(
                    &segment.id,
                    SegmentStatus::Failed,
                    existing_len,
                    Some("server rejected segment range".to_owned()),
                )
                .await?;
            return Err(CoreError::SegmentRangeRejected);
        }
        StatusCode::TOO_MANY_REQUESTS => {
            if let Some(delay) = crate::error::parse_retry_after(response.headers()) {
                tokio::time::sleep(delay).await;
            }
            return Err(CoreError::HttpStatus(StatusCode::TOO_MANY_REQUESTS));
        }
        status => return Err(CoreError::HttpStatus(status)),
    }

    let append = existing_len > 0;
    let mut file = OpenOptions::new()
        .create(true)
        .write(true)
        .append(append)
        .truncate(!append)
        .open(&path)
        .await?;
    let mut downloaded = existing_len;
    let mut last_persisted = existing_len;
    let mut stream = response.bytes_stream();

    loop {
        tokio::select! {
            () = cancellation.cancelled() => {
                file.flush().await?;
                storage
                    .set_segment_status_async(&segment.id, SegmentStatus::Queued, downloaded, None)
                    .await?;
                return Err(CoreError::Paused);
            }
            maybe_chunk = stream.next() => {
                match maybe_chunk {
                    Some(chunk) => {
                        let chunk = chunk?;
                        file.write_all(&chunk).await?;
                        downloaded += chunk.len() as u64;
                        if downloaded.saturating_sub(last_persisted) >= PROGRESS_FLUSH_BYTES {
                            storage
                                .set_segment_status_async(&segment.id, SegmentStatus::Downloading, downloaded, None)
                                .await?;
                            last_persisted = downloaded;
                        }
                    }
                    None => break,
                }
            }
        }
    }

    file.flush().await?;
    if downloaded != segment.expected_len() {
        storage
            .set_segment_status_async(
                &segment.id,
                SegmentStatus::Failed,
                downloaded,
                Some("segment size mismatch".to_owned()),
            )
            .await?;
        return Err(CoreError::SegmentRangeRejected);
    }

    storage
        .set_segment_status_async(&segment.id, SegmentStatus::Completed, downloaded, None)
        .await?;
    Ok(downloaded)
}

fn segment_retry_delay(attempt: u8) -> std::time::Duration {
    std::time::Duration::from_millis(250 * u64::from(attempt))
}

async fn finalize_download(
    storage: &Storage,
    record: &DownloadRecord,
    downloaded_bytes: u64,
    total_size: Option<u64>,
    config: FhastConfig,
) -> Result<DownloadOutcome, CoreError> {
    if let Some(expected_size) = total_size {
        if downloaded_bytes != expected_size {
            return Err(CoreError::ResumeRejected);
        }
    }

    storage
        .set_status_async(&record.id, DownloadStatus::Verifying, None)
        .await?;
    verify_checksums(record).await?;

    if record.final_path.exists() {
        return Err(CoreError::OutputExists(record.final_path.clone()));
    }

    fs::rename(&record.temp_path, &record.final_path).await?;
    storage
        .complete_download_async(&record.id, downloaded_bytes)
        .await?;
    if config.privacy.sensitive_header_retention == SensitiveHeaderRetention::UntilComplete {
        storage.delete_sensitive_headers_async(&record.id).await?;
    }

    Ok(DownloadOutcome::Completed)
}

async fn verify_checksums(record: &DownloadRecord) -> Result<(), CoreError> {
    if let Some(expected) = &record.checksum_sha256 {
        Sha256Checksum::new(expected)
            .verify_file(&record.temp_path)
            .await?;
    }
    if let Some(expected) = &record.checksum_md5 {
        Md5Checksum::new(expected)
            .verify_file(&record.temp_path)
            .await?;
    }

    Ok(())
}

async fn merge_parts(
    part_dir: &Path,
    temp_path: &Path,
    segments: &[SegmentRecord],
) -> Result<(), CoreError> {
    remove_file_if_exists(temp_path).await?;
    ensure_parent_dir(temp_path).await?;
    let mut output = File::create(temp_path).await?;

    for segment in segments {
        let mut part = File::open(part_path(part_dir, segment.index)).await?;
        tokio::io::copy(&mut part, &mut output).await?;
    }

    output.flush().await?;
    Ok(())
}

fn ensure_resume_valid(
    old_etag: &Option<String>,
    old_last_modified: &Option<String>,
    probe: &ProbeResult,
) -> Result<(), CoreError> {
    if old_etag.is_some() && probe.etag.is_some() && old_etag != &probe.etag {
        return Err(CoreError::NeedsRefresh);
    }
    if old_last_modified.is_some()
        && probe.last_modified.is_some()
        && old_last_modified != &probe.last_modified
    {
        return Err(CoreError::NeedsRefresh);
    }

    Ok(())
}

fn header_to_string(
    headers: &reqwest::header::HeaderMap,
    name: reqwest::header::HeaderName,
) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(ToOwned::to_owned)
}

fn parse_content_range_total(headers: &reqwest::header::HeaderMap) -> Option<u64> {
    let value = header_to_string(headers, CONTENT_RANGE)?;
    let (_, total) = value.rsplit_once('/')?;

    if total == "*" {
        None
    } else {
        total.parse().ok()
    }
}

fn segment_dir(record: &DownloadRecord) -> PathBuf {
    record.temp_path.with_extension("fhast-segments")
}

fn part_path(part_dir: &Path, index: u32) -> PathBuf {
    part_dir.join(format!("segment-{index}.part"))
}

async fn temp_file_len(path: &Path) -> Result<u64, CoreError> {
    match fs::metadata(path).await {
        Ok(metadata) => Ok(metadata.len()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(0),
        Err(error) => Err(CoreError::Io(error)),
    }
}

async fn ensure_parent_dir(path: &Path) -> Result<(), CoreError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).await?;
    }

    Ok(())
}

async fn remove_file_if_exists(path: &Path) -> Result<(), CoreError> {
    match fs::remove_file(path).await {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CoreError::Io(error)),
    }
}

async fn remove_path_if_exists(path: impl AsRef<Path>) -> Result<(), CoreError> {
    let path = path.as_ref();
    match fs::metadata(path).await {
        Ok(metadata) if metadata.is_dir() => {
            fs::remove_dir_all(path).await?;
            Ok(())
        }
        Ok(_) => remove_file_if_exists(path).await,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(CoreError::Io(error)),
    }
}

#[allow(dead_code)]
async fn _assert_async_write<W: AsyncWrite + Unpin>(_: &mut W) {}
